//! Windows: `%APPDATA%`, launchers on the PATH and the system's own title bar.

use std::path::PathBuf;

pub const MAC: bool = false;

pub const TRANSPARENT_TITLEBAR: bool = false;
pub const TITLEBAR_INSET: f32 = 12.;

/// The executable carries no icon resource yet; the taskbar shows the default one.
pub fn set_icon() {}

pub fn data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| Some(std::env::home_dir()?.join("AppData/Roaming")))?;
    Some(base.join("GitLance"))
}

/// `code` is `code.cmd`, so the PATHEXT extensions are tried too.
pub fn launcher(launcher: &super::Launcher) -> Option<PathBuf> {
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
        .collect();
    super::on_path(launcher.on_path, &exts)
}

/// A file under the user's profile is already theirs alone; nothing to narrow.
pub fn owner_only(_file: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}
