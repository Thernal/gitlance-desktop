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
