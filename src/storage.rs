//! What GitLance remembers between runs, as small text files in its application support directory:
//! recently opened repositories, pane sizes and how diffs are shown.

use std::path::{Path, PathBuf};

const RECENT_LIMIT: usize = 10;

fn file(name: &str) -> Option<PathBuf> {
    let home = std::env::home_dir()?;
    Some(home.join("Library/Application Support/GitLance").join(name))
}

/// Writes `body` to `name`; failures only cost what would have been remembered.
fn save(name: &str, body: String) {
    if let Some(file) = file(name)
        && let Some(dir) = file.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = std::fs::write(file, body);
    }
}

fn read(name: &str) -> String {
    file(name)
        .and_then(|f| std::fs::read_to_string(f).ok())
        .unwrap_or_default()
}

/// Recently opened repositories that still exist, newest first.
pub fn recent() -> Vec<PathBuf> {
    read("recent.txt")
        .lines()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect()
}

/// Moves `path` to the front of the recent list and saves it.
pub fn remember(path: &Path) -> Vec<PathBuf> {
    let mut list = recent();
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(RECENT_LIMIT);
    let body: Vec<String> = list.iter().map(|p| p.display().to_string()).collect();
    save("recent.txt", body.join("\n") + "\n");
    list
}

/// Pane sizes in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Width of the branches/versions/commits column.
    pub sidebar: f32,
    /// Width of the changed-files column.
    pub files: f32,
    /// Height of the branch list.
    pub branches: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sidebar: 340.,
            files: 280.,
            branches: 180.,
        }
    }
}

impl Layout {
    pub fn load() -> Self {
        Self::parse(&read("layout.txt"))
    }

    pub fn save(&self) {
        save("layout.txt", self.to_string());
    }

    fn parse(text: &str) -> Self {
        let mut layout = Self::default();
        for (key, value) in pairs(text) {
            let Ok(value) = value.parse::<f32>() else {
                continue;
            };
            match key {
                "sidebar" => layout.sidebar = value,
                "files" => layout.files = value,
                "branches" => layout.branches = value,
                _ => {}
            }
        }
        layout
    }
}

impl std::fmt::Display for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "sidebar={}", self.sidebar)?;
        writeln!(f, "files={}", self.files)?;
        writeln!(f, "branches={}", self.branches)
    }
}

/// How a diff is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewOptions {
    /// One column with removals above additions, instead of side by side.
    pub unified: bool,
    /// difftastic's syntax-aware alignment and token marks, instead of git's line diff.
    pub structural: bool,
    /// Long lines wrap instead of scrolling horizontally.
    pub wrap: bool,
    /// Every unchanged line, instead of three around each change.
    pub full_context: bool,
    /// Whitespace-only changes count as unchanged.
    pub ignore_whitespace: bool,
}

impl ViewOptions {
    pub fn load() -> Self {
        Self::parse(&read("view.txt"))
    }

    pub fn save(&self) {
        save("view.txt", self.to_string());
    }

    fn parse(text: &str) -> Self {
        let mut options = Self::default();
        for (key, value) in pairs(text) {
            let value = value == "true";
            match key {
                "unified" => options.unified = value,
                "structural" => options.structural = value,
                "wrap" => options.wrap = value,
                "full_context" => options.full_context = value,
                "ignore_whitespace" => options.ignore_whitespace = value,
                _ => {}
            }
        }
        options
    }
}

impl std::fmt::Display for ViewOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "unified={}", self.unified)?;
        writeln!(f, "structural={}", self.structural)?;
        writeln!(f, "wrap={}", self.wrap)?;
        writeln!(f, "full_context={}", self.full_context)?;
        writeln!(f, "ignore_whitespace={}", self.ignore_whitespace)
    }
}

/// `key=value` lines; anything else is skipped.
fn pairs(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_round_trips_and_ignores_junk() {
        let layout = Layout {
            sidebar: 400.,
            files: 250.5,
            branches: 120.,
        };
        assert_eq!(Layout::parse(&layout.to_string()), layout);
        assert_eq!(
            Layout::parse("files=abc\nnope\nwidth=3\n"),
            Layout::default()
        );
    }
}
