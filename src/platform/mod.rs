//! What differs between macOS, Windows and Linux, behind one API: the app's data folder, its icon,
//! where an editor's launcher lives and how the window's title bar is drawn. Nothing outside this
//! module asks which OS it runs on.

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

pub use os::{TRANSPARENT_TITLEBAR, set_icon};

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
