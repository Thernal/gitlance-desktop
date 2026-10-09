//! What GitLance remembers between runs, as small text files in its application support directory:
//! recently opened repositories, pane sizes and how diffs are shown.

use std::path::{Path, PathBuf};

const RECENT_LIMIT: usize = 10;

fn file(name: &str) -> Option<PathBuf> {
    let home = std::env::home_dir()?;
    Some(home.join("Library/Application Support/GitLance").join(name))
}

/// Writes `body` to `name`; failures only cost what would have been remembered.
pub(crate) fn save(name: &str, body: String) {
    if let Some(file) = file(name)
        && let Some(dir) = file.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = std::fs::write(file, body);
    }
}

pub(crate) fn read(name: &str) -> String {
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

/// The tabs open when the window last changed, and which one was in front.
pub fn session() -> (Vec<PathBuf>, usize) {
    let text = read("session.txt");
    let mut lines = text.lines();
    let active = lines.next().and_then(|l| l.parse().ok()).unwrap_or(0);
    let tabs: Vec<PathBuf> = lines.map(PathBuf::from).filter(|p| p.is_dir()).collect();
    (tabs, active)
}

/// Keeps the open tabs (repository roots, in order) and the one in front.
pub fn save_session(tabs: &[PathBuf], active: usize) {
    let mut body = format!("{active}\n");
    for t in tabs {
        body.push_str(&format!("{}\n", t.display()));
    }
    save("session.txt", body);
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

/// Whether background fetch is on for this repository (opt-in, per repository).
pub fn fetch_enabled(root: &Path) -> bool {
    let want = root.display().to_string();
    read("fetch.txt").lines().any(|l| l == want)
}

pub fn set_fetch_enabled(root: &Path, on: bool) {
    let want = root.display().to_string();
    let mut lines: Vec<String> = read("fetch.txt")
        .lines()
        .filter(|l| !l.is_empty() && *l != want)
        .map(str::to_owned)
        .collect();
    if on {
        lines.push(want);
    }
    save("fetch.txt", lines.join("\n") + "\n");
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
    /// The branches/versions/commits column and the files column are shown (⌘1, ⌘2).
    pub show_sidebar: bool,
    pub show_files: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sidebar: 340.,
            files: 280.,
            branches: 180.,
            show_sidebar: true,
            show_files: true,
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
            match key {
                "show_sidebar" => layout.show_sidebar = value != "false",
                "show_files" => layout.show_files = value != "false",
                _ => {}
            }
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
        writeln!(f, "branches={}", self.branches)?;
        writeln!(f, "show_sidebar={}", self.show_sidebar)?;
        writeln!(f, "show_files={}", self.show_files)
    }
}

/// What a diff marks inside its changed lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffMode {
    /// git's line diff: whole lines.
    Lines,
    /// The line diff, plus the words that changed inside a changed line.
    #[default]
    Words,
    /// difftastic's syntax-aware alignment and token marks.
    Structural,
}

impl DiffMode {
    pub const ALL: [DiffMode; 3] = [DiffMode::Lines, DiffMode::Words, DiffMode::Structural];

    pub fn label(self) -> &'static str {
        match self {
            DiffMode::Lines => "Lines",
            DiffMode::Words => "Words",
            DiffMode::Structural => "Structural",
        }
    }

    /// The next mode, wrapping around.
    pub fn next(self) -> Self {
        match self {
            DiffMode::Lines => DiffMode::Words,
            DiffMode::Words => DiffMode::Structural,
            DiffMode::Structural => DiffMode::Lines,
        }
    }

    fn key(self) -> &'static str {
        match self {
            DiffMode::Lines => "lines",
            DiffMode::Words => "words",
            DiffMode::Structural => "structural",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// How a changed word is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkStyle {
    #[default]
    Tinted,
    Underlined,
}

impl MarkStyle {
    pub const ALL: [MarkStyle; 2] = [MarkStyle::Tinted, MarkStyle::Underlined];

    pub fn label(self) -> &'static str {
        match self {
            MarkStyle::Tinted => "Tinted",
            MarkStyle::Underlined => "Underlined",
        }
    }

    fn key(self) -> &'static str {
        match self {
            MarkStyle::Tinted => "tinted",
            MarkStyle::Underlined => "underlined",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// How the window follows the system appearance. Only the dark theme exists so far, so both
/// choices look the same until a light palette is designed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    System,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 2] = [Appearance::System, Appearance::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Appearance::System => "System",
            Appearance::Dark => "Dark",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Dark => "dark",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == key)
    }
}

/// Preferences from the Settings page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The editor files open in; `None` takes the first one installed.
    pub editor: Option<crate::editor::Editor>,
    pub appearance: Appearance,
    /// How changed words are drawn, in Words and Structural modes.
    pub mark_style: MarkStyle,
    /// The mode a diff opens in.
    pub default_mode: DiffMode,
    /// Re-read the repository when something outside the app changes it.
    pub auto_refresh: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            editor: None,
            appearance: Appearance::default(),
            mark_style: MarkStyle::default(),
            default_mode: DiffMode::default(),
            auto_refresh: true,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        Self::parse(&read("settings.txt"))
    }

    pub fn save(&self) {
        save("settings.txt", self.to_string());
    }

    fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for (key, value) in pairs(text) {
            match key {
                "mark_style" => {
                    settings.mark_style = MarkStyle::from_key(value).unwrap_or_default()
                }
                "default_mode" => {
                    settings.default_mode = DiffMode::from_key(value).unwrap_or_default()
                }
                "auto_refresh" => settings.auto_refresh = value != "false",
                "editor" => settings.editor = crate::editor::Editor::from_key(value),
                "appearance" => {
                    settings.appearance = Appearance::from_key(value).unwrap_or_default()
                }
                _ => {}
            }
        }
        settings
    }
}

impl std::fmt::Display for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "mark_style={}", self.mark_style.key())?;
        writeln!(f, "default_mode={}", self.default_mode.key())?;
        writeln!(f, "auto_refresh={}", self.auto_refresh)?;
        writeln!(f, "appearance={}", self.appearance.key())?;
        if let Some(editor) = self.editor {
            writeln!(f, "editor={}", editor.key())?;
        }
        Ok(())
    }
}

/// How a diff is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewOptions {
    /// One column with removals above additions, instead of side by side.
    pub unified: bool,
    /// What is marked inside changed lines. Starts as the default mode; not remembered.
    pub mode: DiffMode,
    /// Long lines wrap instead of scrolling horizontally.
    pub wrap: bool,
    /// Every unchanged line, instead of three around each change.
    pub full_context: bool,
    /// Whitespace-only changes count as unchanged.
    pub ignore_whitespace: bool,
    /// Changed files as a flat list instead of a folder tree.
    pub list_files: bool,
}

impl ViewOptions {
    pub fn load(settings: &Settings) -> Self {
        let mut options = Self::parse(&read("view.txt"));
        options.mode = settings.default_mode;
        options
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
                "wrap" => options.wrap = value,
                "full_context" => options.full_context = value,
                "ignore_whitespace" => options.ignore_whitespace = value,
                "list_files" => options.list_files = value,
                _ => {}
            }
        }
        options
    }
}

impl std::fmt::Display for ViewOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "unified={}", self.unified)?;
        writeln!(f, "wrap={}", self.wrap)?;
        writeln!(f, "full_context={}", self.full_context)?;
        writeln!(f, "ignore_whitespace={}", self.ignore_whitespace)?;
        writeln!(f, "list_files={}", self.list_files)
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
    fn settings_round_trip_and_default_to_the_designed_choices() {
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(Settings::default().default_mode, DiffMode::Words);
        assert_eq!(Settings::default().mark_style, MarkStyle::Tinted);
        let settings = Settings {
            editor: Some(crate::editor::Editor::Zed),
            appearance: Appearance::Dark,
            mark_style: MarkStyle::Underlined,
            default_mode: DiffMode::Structural,
            auto_refresh: false,
        };
        assert_eq!(Settings::parse(&settings.to_string()), settings);
    }

    #[test]
    fn layout_round_trips_and_ignores_junk() {
        let layout = Layout {
            sidebar: 400.,
            files: 250.5,
            branches: 120.,
            show_sidebar: false,
            show_files: true,
        };
        assert_eq!(Layout::parse(&layout.to_string()), layout);
        assert_eq!(
            Layout::parse("files=abc\nnope\nwidth=3\n"),
            Layout::default()
        );
    }
}
