//! Opening a file in the reviewer's editor at a line: Android Studio, Zed or VS Code, through the
//! command each ships inside its app bundle, so nothing has to be installed on the PATH.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Editor {
    AndroidStudio,
    Zed,
    VsCode,
}

impl Editor {
    pub const ALL: [Editor; 3] = [Editor::AndroidStudio, Editor::Zed, Editor::VsCode];

    pub fn label(self) -> &'static str {
        match self {
            Editor::AndroidStudio => "Android Studio",
            Editor::Zed => "Zed",
            Editor::VsCode => "VS Code",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Editor::AndroidStudio => "android-studio",
            Editor::Zed => "zed",
            Editor::VsCode => "vscode",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.key() == key)
    }

    /// The app bundle, if installed under `/Applications` or `~/Applications`.
    fn bundle(self) -> Option<PathBuf> {
        let dirs = [
            PathBuf::from("/Applications"),
            std::env::home_dir()?.join("Applications"),
        ];
        dirs.iter().find_map(|dir| match self {
            // "Android Studio.app", "Android Studio Preview.app", "Android Studio Koala.app"…
            Editor::AndroidStudio => std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                (name.starts_with("Android Studio") && name.ends_with(".app")).then(|| e.path())
            }),
            Editor::Zed => Some(dir.join("Zed.app")).filter(|p| p.exists()),
            Editor::VsCode => Some(dir.join("Visual Studio Code.app")).filter(|p| p.exists()),
        })
    }

    pub fn installed(self) -> bool {
        self.bundle().is_some()
    }

    /// The program and arguments that open `file` at `line`, given the app bundle.
    fn command(self, bundle: &Path, file: &Path, line: u32) -> (PathBuf, Vec<String>) {
        let file_s = file.display().to_string();
        match self {
            Editor::AndroidStudio => (
                bundle.join("Contents/MacOS/studio"),
                vec!["--line".into(), line.to_string(), file_s],
            ),
            Editor::Zed => (
                bundle.join("Contents/MacOS/cli"),
                vec![format!("{file_s}:{line}")],
            ),
            Editor::VsCode => (
                bundle.join("Contents/Resources/app/bin/code"),
                vec!["-g".into(), format!("{file_s}:{line}")],
            ),
        }
    }

    /// Opens `file` at `line`; the error says what to do about it.
    pub fn open(self, file: &Path, line: u32) -> Result<(), String> {
        let Some(bundle) = self.bundle() else {
            return Err(format!("{} is not installed.", self.label()));
        };
        if !file.exists() {
            return Err(format!(
                "{} is not in the working tree, so there is nothing to open.",
                file.display()
            ));
        }
        let (program, args) = self.command(&bundle, file, line.max(1));
        Command::new(program)
            .args(args)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not start {}: {e}", self.label()))
    }
}

/// The editor to use: the chosen one if installed, else the first installed.
pub fn pick(chosen: Option<Editor>) -> Option<Editor> {
    chosen
        .filter(|e| e.installed())
        .or_else(|| Editor::ALL.into_iter().find(|e| e.installed()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_editor_gets_the_file_and_the_line() {
        let bundle = Path::new("/Applications/X.app");
        let file = Path::new("/work/repo/src/a.rs");
        let (program, args) = Editor::AndroidStudio.command(bundle, file, 12);
        assert!(program.ends_with("Contents/MacOS/studio"));
        assert_eq!(args, ["--line", "12", "/work/repo/src/a.rs"]);
        let (_, args) = Editor::Zed.command(bundle, file, 12);
        assert_eq!(args, ["/work/repo/src/a.rs:12"]);
        let (_, args) = Editor::VsCode.command(bundle, file, 12);
        assert_eq!(args, ["-g", "/work/repo/src/a.rs:12"]);
    }

    #[test]
    fn keys_round_trip() {
        for editor in Editor::ALL {
            assert_eq!(Editor::from_key(editor.key()), Some(editor));
        }
        assert_eq!(Editor::from_key("emacs"), None);
    }
}
