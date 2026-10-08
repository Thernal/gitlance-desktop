//! What GitLance remembers between runs, as small text files in its application support directory:
//! recently opened repositories and the pane sizes.

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
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let Ok(value) = value.trim().parse::<f32>() else {
                continue;
            };
            match key.trim() {
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
