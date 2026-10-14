//! Opening a file in the reviewer's editor at a line: Android Studio, Zed or VS Code, through the
//! command each ships (inside its app bundle on macOS, on the PATH elsewhere — `platform`).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platform::{self, Launcher};

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

    /// Where the editor's command-line launcher lives.
    fn launcher(self) -> Launcher {
        match self {
            Editor::AndroidStudio => Launcher {
                bundle: "Android Studio",
                in_bundle: "Contents/MacOS/studio",
                on_path: &["studio", "studio.sh", "studio64"],
            },
            Editor::Zed => Launcher {
                bundle: "Zed",
                in_bundle: "Contents/MacOS/cli",
                on_path: &["zed", "zeditor"],
            },
            Editor::VsCode => Launcher {
                bundle: "Visual Studio Code",
                in_bundle: "Contents/Resources/app/bin/code",
                on_path: &["code"],
            },
        }
    }

    fn program(self) -> Option<PathBuf> {
        platform::launcher(&self.launcher())
    }

    pub fn installed(self) -> bool {
        self.program().is_some()
    }

    /// The arguments that open `file` at `line`.
    fn args(self, file: &Path, line: u32) -> Vec<String> {
        let file_s = file.display().to_string();
        match self {
            Editor::AndroidStudio => vec!["--line".into(), line.to_string(), file_s],
            Editor::Zed => vec![format!("{file_s}:{line}")],
            Editor::VsCode => vec!["-g".into(), format!("{file_s}:{line}")],
        }
    }

    /// Opens `file` at `line`; the error says what to do about it.
    pub fn open(self, file: &Path, line: u32) -> Result<(), String> {
        let Some(program) = self.program() else {
            return Err(format!("{} is not installed.", self.label()));
        };
        if !file.exists() {
            return Err(format!(
                "{} is not in the working tree, so there is nothing to open.",
                file.display()
            ));
        }
        Command::new(program)
            .args(self.args(file, line.max(1)))
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
        let file = Path::new("/work/repo/src/a.rs");
        let path = file.display().to_string();
        assert_eq!(
            Editor::AndroidStudio.args(file, 12),
            ["--line".to_string(), "12".into(), path.clone()]
        );
        assert_eq!(Editor::Zed.args(file, 12), [format!("{path}:12")]);
        assert_eq!(
            Editor::VsCode.args(file, 12),
            ["-g".to_string(), format!("{path}:12")]
        );
    }

    #[test]
    fn keys_round_trip() {
        for editor in Editor::ALL {
            assert_eq!(Editor::from_key(editor.key()), Some(editor));
        }
        assert_eq!(Editor::from_key("emacs"), None);
    }
}
