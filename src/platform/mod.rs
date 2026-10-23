//! What differs between macOS, Windows and Linux, behind one API: the app's data folder, its icon,
//! where an editor's launcher lives, how the window's title bar is drawn, and which apps open a
//! file (#78). Nothing outside this module asks which OS it runs on.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as os;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod linux;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use linux as os;

use std::path::PathBuf;

pub use os::{TRANSPARENT_TITLEBAR, alive, owner_only, reduce_motion, set_icon};

/// Whether this is macOS, where the primary modifier is ⌘; elsewhere it is Ctrl.
pub const MAC: bool = os::MAC;

/// A shipped binding, written with macOS keys (`cmd-o`), in this OS's keys.
pub fn keys(mac: &str) -> String {
    if MAC { mac.to_owned() } else { pc_keys(mac) }
}

/// macOS keys in Windows and Linux terms: `cmd-` is `ctrl-`, except ⌘1…⌘9 (the tabs), which are
/// `alt-1`…`alt-9`, as browsers there have them: `ctrl-1`…`ctrl-4` move between zones. ⌃⌘ (the
/// spaces) is Ctrl+Shift, since Ctrl is already ⌘'s.
pub fn pc_keys(mac: &str) -> String {
    mac.split(' ')
        .map(|stroke| {
            if let Some(key) = stroke.strip_prefix("ctrl-cmd-") {
                return format!("ctrl-shift-{key}");
            }
            let tab = stroke
                .strip_prefix("cmd-")
                .filter(|k| k.len() == 1 && k.as_bytes()[0].is_ascii_digit() && *k != "0");
            match tab {
                Some(digit) => format!("alt-{digit}"),
                None => stroke.replace("cmd-", "ctrl-"),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `m` moves the caret by words: ⌥ on macOS, Ctrl elsewhere.
pub fn word(m: &gpui::Modifiers) -> bool {
    if MAC { m.alt } else { m.control }
}

/// Whether `m` moves the caret to a line's ends with ← →: ⌘ on macOS; elsewhere Home and End do.
pub fn line(m: &gpui::Modifiers) -> bool {
    MAC && m.platform
}

/// The folder for what GitLance remembers between runs.
pub fn data_dir() -> Option<PathBuf> {
    os::data_dir()
}

/// Where an editor's command-line launcher can be found.
#[allow(dead_code)] // each OS reads only its own fields
pub struct Launcher {
    /// The app bundle's name starts with this (macOS): "Android Studio" also finds
    /// "Android Studio Preview.app".
    pub bundle: &'static str,
    /// The launcher inside that bundle (macOS).
    pub in_bundle: &'static str,
    /// The launcher's names on the PATH (Windows, Linux).
    pub on_path: &'static [&'static str],
}

/// The launcher's program, if the editor is installed.
pub fn launcher(launcher: &Launcher) -> Option<PathBuf> {
    os::launcher(launcher)
}

/// `text` as one word for a POSIX shell.
#[cfg(unix)]
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Opens a terminal window in `dir` running `program` with one argument, `arg` (a new agent chat
/// with its first message).
pub fn open_terminal(dir: &std::path::Path, program: &str, arg: &str) -> std::io::Result<()> {
    os::open_terminal(dir, program, arg)
}

/// Whether the system names the apps that open a file (macOS); elsewhere a file opens in "the
/// default app", what `open_with` without one starts (#78).
pub const NAMES_APPS: bool = os::NAMES_APPS;

/// Whether a whole working tree can be watched for changes cheaply: one stream on macOS
/// (FSEvents) and Windows; Linux's inotify needs a watch per folder, too many in a large tree,
/// so there git is asked on a slower beat instead (#87).
pub const WATCHES_TREES: bool = os::WATCHES_TREES;

/// The apps that can open `file`, by name and where they are, the system's default first (#78);
/// none where the system does not say.
pub fn apps_for(file: &std::path::Path) -> Vec<(String, PathBuf)> {
    os::apps_for(file)
}

/// Opens `file` (or a folder) in `app`, else in what the system opens it with.
pub fn open_with(file: &std::path::Path, app: Option<&std::path::Path>) -> std::io::Result<()> {
    os::open_with(file, app)
}

/// Where Preview is: images and PDFs open there (#78); None off macOS.
pub fn preview() -> Option<PathBuf> {
    os::preview()
}

/// The space the title bar leaves at its start for the window's own buttons.
pub const TITLEBAR_INSET: f32 = os::TITLEBAR_INSET;

/// The first of `names` found in a folder of the PATH, trying `extensions` after each name.
#[cfg(not(target_os = "macos"))]
fn on_path(names: &[&str], extensions: &[String]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    names.iter().find_map(|name| {
        dirs.iter().find_map(|dir| {
            std::iter::once(String::new())
                .chain(extensions.iter().cloned())
                .map(|ext| dir.join(format!("{name}{ext}")))
                .find(|p| p.is_file())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::pc_keys;

    #[test]
    fn mac_keys_become_ctrl_and_tabs_alt() {
        assert_eq!(pc_keys("cmd-shift-f"), "ctrl-shift-f");
        assert_eq!(pc_keys("cmd-alt-1"), "ctrl-alt-1");
        assert_eq!(pc_keys("cmd-3"), "alt-3");
        assert_eq!(pc_keys("cmd-0"), "ctrl-0");
        assert_eq!(pc_keys("cmd--"), "ctrl--");
        assert_eq!(pc_keys("ctrl-2"), "ctrl-2");
        assert_eq!(pc_keys("ctrl-cmd-4"), "ctrl-shift-4");
        assert_eq!(pc_keys("g g"), "g g");
    }
}
