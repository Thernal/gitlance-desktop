//! Windows: `%APPDATA%`, launchers on the PATH and the system's own title bar.

use std::path::PathBuf;

pub const MAC: bool = false;

pub const TRANSPARENT_TITLEBAR: bool = false;
pub const TITLEBAR_INSET: f32 = 12.;

/// The executable carries no icon resource yet; the taskbar shows the default one.
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

/// Whether process `pid` still runs, as `tasklist` sees it.
pub fn alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(true)
}

/// Starts GitLance, `exe`, on `project`, detached from the caller's console (#153).
pub fn launch(exe: &std::path::Path, project: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    std::process::Command::new(exe)
        .arg(project)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(|_| ())
}

/// A new console window in `dir`.
pub fn open_terminal(dir: &std::path::Path, program: &str, arg: &str) -> std::io::Result<()> {
    std::process::Command::new("cmd")
        .args([
            "/c",
            "start",
            "",
            "cmd",
            "/k",
            program,
            &arg.replace('"', "'"),
        ])
        .current_dir(dir)
        .spawn()
        .map(|_| ())
}

pub const WATCHES_TREES: bool = true;

pub const NAMES_APPS: bool = false;

/// Windows is not asked which apps open a file: the default only (#78).
pub fn apps_for(_file: &std::path::Path) -> Vec<(String, PathBuf)> {
    Vec::new()
}

/// `explorer` opens it with its default, or the app's program with the file.
pub fn open_with(file: &std::path::Path, app: Option<&std::path::Path>) -> std::io::Result<()> {
    let program = app.map_or(std::ffi::OsStr::new("explorer"), |a| a.as_os_str());
    std::process::Command::new(program)
        .arg(file)
        .spawn()
        .map(|_| ())
}

pub fn preview() -> Option<PathBuf> {
    None
}

/// JetBrains IDEs keep tokens in the system's secret store here, which GitLance does not read yet.
pub fn ide_config_dirs() -> Vec<PathBuf> {
    Vec::new()
}

pub fn keychain_has(_service: &str) -> bool {
    false
}

pub fn keychain_read(_service: &str) -> Option<String> {
    None
}
