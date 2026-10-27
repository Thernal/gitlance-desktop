//! macOS: the Dock icon, `~/Library/Application Support` and editors' app bundles.

use std::path::PathBuf;

/// The traffic lights sit on the app's own title bar.
pub const MAC: bool = true;

pub const TRANSPARENT_TITLEBAR: bool = true;
pub const TITLEBAR_INSET: f32 = 78.;

/// A bundle (`./bundle.sh`) carries the icon itself; this sets it for a bare binary (`cargo run`,
/// `./run.sh`), where macOS would show the generic executable.
pub fn set_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(include_bytes!("../../assets/icon.png"));
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        // SAFETY: called on the main thread with a valid image.
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}

/// Whether System Settings → Accessibility → Display → Reduce motion is on (#56).
pub fn reduce_motion() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// The key window as a PNG, at its nominal size (#118): the app's own window, so no screen
/// recording permission is asked for; `None` when it cannot be taken.
pub fn window_picture() -> Option<Vec<u8>> {
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};
    use core_graphics::window::{
        create_image, kCGWindowImageBoundsIgnoreFraming, kCGWindowImageNominalResolution,
        kCGWindowListOptionIncludingWindow,
    };
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    let window = app.keyWindow().or_else(|| app.mainWindow())?;
    let number = u32::try_from(window.windowNumber())
        .ok()
        .filter(|n| *n > 0)?;
    // CGRectNull: the window's own bounds.
    let null = CGRect::new(
        &CGPoint::new(f64::INFINITY, f64::INFINITY),
        &CGSize::new(0., 0.),
    );
    let shot = create_image(
        null,
        kCGWindowListOptionIncludingWindow,
        number,
        kCGWindowImageBoundsIgnoreFraming | kCGWindowImageNominalResolution,
    )?;
    let (width, height, row) = (shot.width(), shot.height(), shot.bytes_per_row());
    if shot.bits_per_pixel() != 32 || width == 0 || height == 0 {
        return None;
    }
    let data = shot.data();
    let bytes = data.bytes();
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let line = bytes.get(y * row..y * row + width * 4)?;
        // In memory B G R A (32-bit little-endian, alpha first).
        for [b, g, r, a] in line.as_chunks::<4>().0 {
            rgba.extend_from_slice(&[*r, *g, *b, *a]);
        }
    }
    let picture = image::RgbaImage::from_raw(width as u32, height as u32, rgba)?;
    let mut png = Vec::new();
    picture
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

pub fn data_dir() -> Option<PathBuf> {
    Some(std::env::home_dir()?.join("Library/Application Support/GitLance"))
}

/// The launcher inside the editor's bundle under `/Applications` or `~/Applications`, so nothing
/// has to be installed on the PATH.
pub fn launcher(launcher: &super::Launcher) -> Option<PathBuf> {
    let dirs = [
        PathBuf::from("/Applications"),
        std::env::home_dir()?.join("Applications"),
    ];
    dirs.iter().find_map(|dir| {
        std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name.starts_with(launcher.bundle) && name.ends_with(".app"))
                .then(|| e.path().join(launcher.in_bundle))
                .filter(|p| p.exists())
        })
    })
}

/// Makes `file` readable and writable by its owner only (a token).
pub fn owner_only(file: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))
}

/// Whether process `pid` still runs.
pub fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists and may be signalled.
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Starts GitLance, `exe`, on `project` (#153): inside its app bundle through Launch Services, in
/// the background so it takes no keys from the reviewer; a bare program detached from the caller.
pub fn launch(exe: &std::path::Path, project: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    if let Some(app) = exe
        .ancestors()
        .find(|a| a.extension().is_some_and(|x| x == "app"))
    {
        let status = Command::new("open")
            .args(["-g", "-a"])
            .arg(app)
            .arg("--args")
            .arg(project)
            .status()?;
        return if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other("macOS did not start it"))
        };
    }
    Command::new(exe)
        .arg(project)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
}

/// Terminal.app, through AppleScript: a new window that runs the command in a login shell.
pub fn open_terminal(dir: &std::path::Path, program: &str, arg: &str) -> std::io::Result<()> {
    let command = format!(
        "cd {} && {program} {}",
        super::shell_quote(&dir.display().to_string()),
        super::shell_quote(arg)
    );
    let script = format!(
        "tell application \"Terminal\"\n activate\n do script \"{}\"\nend tell",
        command.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let status = std::process::Command::new("osascript")
        .args(["-e", &script])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("Terminal did not start it"))
    }
}

pub const WATCHES_TREES: bool = true;

pub const NAMES_APPS: bool = true;

/// The apps Launch Services says open `file`, the default first (#78).
pub fn apps_for(file: &std::path::Path) -> Vec<(String, PathBuf)> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    let workspace = NSWorkspace::sharedWorkspace();
    let url = NSURL::fileURLWithPath(&NSString::from_str(&file.display().to_string()));
    let path = |u: &NSURL| u.path().map(|p| PathBuf::from(p.to_string()));
    let mut apps: Vec<PathBuf> = workspace
        .URLForApplicationToOpenURL(&url)
        .and_then(|u| path(&u))
        .into_iter()
        .collect();
    for u in workspace.URLsForApplicationsToOpenURL(&url).iter() {
        if let Some(p) = path(&u)
            && !apps.contains(&p)
        {
            apps.push(p);
        }
    }
    apps.into_iter()
        .filter_map(|p| Some((p.file_stem()?.to_string_lossy().into_owned(), p)))
        .collect()
}

/// `open -a <app> <file>`, or `open <file>` for the default.
pub fn open_with(file: &std::path::Path, app: Option<&std::path::Path>) -> std::io::Result<()> {
    let mut open = std::process::Command::new("open");
    if let Some(app) = app {
        open.arg("-a").arg(app);
    }
    open.arg(file).spawn().map(|_| ())
}

pub fn preview() -> Option<PathBuf> {
    [
        "/System/Applications/Preview.app",
        "/Applications/Preview.app",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}
