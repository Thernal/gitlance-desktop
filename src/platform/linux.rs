//! Linux and the BSDs: the XDG data folder, launchers on the PATH and the system's own title bar.

use std::path::PathBuf;

pub const MAC: bool = false;

pub const TRANSPARENT_TITLEBAR: bool = false;
pub const TITLEBAR_INSET: f32 = 12.;

/// The desktop file names the icon (the window's app id is `gitlance`).
pub fn set_icon() {}

/// The system's "reduce motion" is not read here yet; Settings → Appearance → Animations is (#56).
pub fn reduce_motion() -> bool {
    false
}

/// The window's picture for a report (#118): not taken here yet.
pub fn window_picture() -> Option<Vec<u8>> {
    None
}

pub fn data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| Some(std::env::home_dir()?.join(".local/share")))?;
    Some(base.join("gitlance"))
}

pub fn launcher(launcher: &super::Launcher) -> Option<PathBuf> {
    super::on_path(launcher.on_path, &[])
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

/// Starts GitLance, `exe`, on `project`, detached from the caller (#153).
pub fn launch(exe: &std::path::Path, project: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    std::process::Command::new(exe)
        .arg(project)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
}

/// The desktop's terminal (`x-terminal-emulator`, else GNOME's or KDE's).
pub fn open_terminal(dir: &std::path::Path, program: &str, arg: &str) -> std::io::Result<()> {
    let command = format!(
        "cd {} && {program} {}; exec $SHELL",
        super::shell_quote(&dir.display().to_string()),
        super::shell_quote(arg)
    );
    for (term, flag) in [
        ("x-terminal-emulator", "-e"),
        ("gnome-terminal", "--"),
        ("konsole", "-e"),
    ] {
        if std::process::Command::new(term)
            .args([flag, "sh", "-c", &command])
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }
    Err(std::io::Error::other("no terminal found"))
}

pub const WATCHES_TREES: bool = false;

pub const NAMES_APPS: bool = false;

/// The desktop does not say which apps open a file here: the default only (#78).
pub fn apps_for(_file: &std::path::Path) -> Vec<(String, PathBuf)> {
    Vec::new()
}

/// `xdg-open`, or the app's program with the file.
pub fn open_with(file: &std::path::Path, app: Option<&std::path::Path>) -> std::io::Result<()> {
    let program = app.map_or(std::ffi::OsStr::new("xdg-open"), |a| a.as_os_str());
    std::process::Command::new(program)
        .arg(file)
        .spawn()
        .map(|_| ())
}

pub fn preview() -> Option<PathBuf> {
    None
}
