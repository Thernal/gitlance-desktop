//! Linux and the BSDs: the XDG data folder, launchers on the PATH and the system's own title bar.

use std::path::PathBuf;

pub const MAC: bool = false;

pub const TRANSPARENT_TITLEBAR: bool = false;
pub const TITLEBAR_INSET: f32 = 12.;

/// The desktop file names the icon (the window's app id is `gitlance`).
pub fn set_icon() {}

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
