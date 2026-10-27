//! Open in (#78): what the page shows — Code's file or folder, else the diff's file — in the app it
//! belongs to, named on the button: a text file in your editor at its line, an image or a PDF in
//! Preview, anything else in the app the system opens it with, a folder in your editor. ▾ lists
//! the other apps, Reveal in Finder, Copy path, and "Always open .kt files in …", a choice kept per
//! extension in `open-with.txt`. ⌥⌘O opens, ⌥⌘R reveals.

use super::menu::{Act, Entry};
use super::nav::Page;
use super::spaces::Space;
use super::{Workspace, icons, px, theme};
use crate::editor::{self, Editor};
use crate::platform;
use gpui::{AnyElement, ClickEvent, Context, MouseButton, MouseDownEvent, div, prelude::*};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const FILE: &str = "open-with.txt";

/// Finder with it selected, as the rail's project menu words it off macOS.
pub(super) const REVEAL: &str = if platform::MAC {
    "Reveal in Finder"
} else {
    "Show in its folder"
};

/// An app a file opens in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum App {
    /// One of the editors GitLance opens at a line.
    Editor(Editor),
    /// Any other, by name and where it is.
    Other { name: String, path: PathBuf },
    /// What the system opens it with, where the system does not name it (Windows, Linux).
    Default,
}

impl App {
    pub fn name(&self) -> String {
        match self {
            App::Editor(e) => e.label().to_owned(),
            App::Other { name, .. } => name.clone(),
            App::Default => "the default app".to_owned(),
        }
    }

    /// As `open-with.txt` writes it.
    fn key(&self) -> String {
        match self {
            App::Editor(e) => format!("editor:{}", e.key()),
            App::Other { name, path } => format!("app:{name}\t{}", path.display()),
            App::Default => "default".to_owned(),
        }
    }

    fn parse(text: &str) -> Option<App> {
        if text == "default" {
            return Some(App::Default);
        }
        if let Some(key) = text.strip_prefix("editor:") {
            return Editor::from_key(key).map(App::Editor);
        }
        let (name, path) = text.strip_prefix("app:")?.split_once('\t')?;
        Some(App::Other {
            name: name.to_owned(),
            path: PathBuf::from(path),
        })
    }

    /// Still here to open things.
    fn installed(&self) -> bool {
        match self {
            App::Editor(e) => e.installed(),
            App::Other { path, .. } => path.exists(),
            App::Default => true,
        }
    }

    /// Opens `path` at `line` where the app can.
    fn open(&self, path: &Path, line: u32) -> Result<(), String> {
        match self {
            App::Editor(e) => e.open(path, line),
            App::Other { name, path: app } => platform::open_with(path, Some(app))
                .map_err(|e| format!("Could not open it in {name}: {e}")),
            App::Default => {
                platform::open_with(path, None).map_err(|e| format!("Could not open it: {e}"))
            }
        }
    }
}

/// What kind of thing a target is, as the rule reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Folder,
    Text,
    /// An image or a PDF.
    Picture,
    Other,
}

/// The app `kind` opens in: the one kept for its extension while it is there, else a folder or a
/// text file in the editor, a picture in Preview, anything else in the system's default; failing
/// those, the system's default. None: nothing opens it, the button asks.
pub fn rule(
    kind: Kind,
    kept: Option<App>,
    editor: Option<Editor>,
    preview: Option<PathBuf>,
    default: Option<App>,
) -> Option<App> {
    let preview = preview.map(|path| App::Other {
        name: "Preview".to_owned(),
        path,
    });
    kept.or(match kind {
        Kind::Folder | Kind::Text => editor.map(App::Editor).or(default),
        Kind::Picture => preview.or(default),
        Kind::Other => default,
    })
}

/// The kind of `path`: a folder, a picture by its extension, else text when its start has no NUL.
fn kind_of(path: &Path) -> Kind {
    if path.is_dir() {
        return Kind::Folder;
    }
    let name = path.to_string_lossy();
    if crate::git::ImageKind::of(&name).is_some() || name.to_lowercase().ends_with(".pdf") {
        return Kind::Picture;
    }
    let mut head = [0u8; 8000];
    let read = std::fs::File::open(path)
        .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
        .unwrap_or(0);
    if binary(&head[..read]) {
        Kind::Other
    } else {
        Kind::Text
    }
}

/// A NUL in the first 8000 bytes: binary, as git judges it.
pub fn binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8000)].contains(&0)
}

/// `path`'s extension as the kept choices are: lower case, with no dot.
fn extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .filter(|e| !e.is_empty())
}

/// The kept choices, by extension.
pub fn load() -> HashMap<String, App> {
    crate::storage::read(FILE)
        .lines()
        .filter_map(|l| {
            let (ext, app) = l.split_once('\t')?;
            Some((ext.to_owned(), App::parse(app)?))
        })
        .collect()
}

fn save(choices: &HashMap<String, App>) {
    let mut lines: Vec<String> = choices
        .iter()
        .map(|(ext, app)| format!("{ext}\t{}", app.key()))
        .collect();
    lines.sort();
    crate::storage::save(FILE, lines.join("\n") + "\n");
}

/// The app the button names for a path, as last worked out: the rule reads the disk and asks the
/// system, so not on every frame. And the app last picked in the ▾, which the path keeps for the
/// session — what "Always open" then keeps for its extension.
#[derive(Default)]
pub struct Cache {
    named: std::cell::RefCell<Option<Named>>,
    picked: Option<(PathBuf, App)>,
}

/// A path, the editor chosen in Settings then, and the app the rule named.
type Named = (PathBuf, Option<Editor>, Option<App>);

impl Workspace {
    /// What the page shows, to open, and at which line: Code's file at the top line shown, or its
    /// folder; a diff's file at its change.
    pub(super) fn open_target(&self) -> Option<(PathBuf, u32)> {
        let root = self.root.clone()?;
        match (self.nav.space, self.nav_page()) {
            (Space::Code, Some(Page::File(path))) => {
                Some((root.join(path), self.file_top_line(path)))
            }
            (Space::Code, Some(Page::Folder(path))) => Some((root.join(path), 1)),
            (Space::Code, _) => Some((root, 1)),
            // A page showing a diff: not a request's Overview or Commits.
            (_, Some(_))
                if matches!(self.page_tab(), None | Some(super::nav::Section::Changes)) =>
            {
                let line = self
                    .changes
                    .get(self.change_at.unwrap_or(0))
                    .and_then(|&row| self.new_line_of_row(row))
                    .unwrap_or(1);
                Some((root.join(self.current_path()?), line))
            }
            _ => None,
        }
    }

    /// The app `path` opens in: the one picked for it in the ▾, else by the rule.
    pub(super) fn open_app(&self, path: &Path) -> Option<App> {
        if let Some((at, app)) = &self.open_cache.picked
            && at == path
        {
            return Some(app.clone());
        }
        let chosen = self.settings.editor;
        if let Some((at, editor, app)) = self.open_cache.named.borrow().as_ref()
            && at == path
            && *editor == chosen
        {
            return app.clone();
        }
        let kind = kind_of(path);
        let kept = extension(path)
            .filter(|_| kind != Kind::Folder)
            .and_then(|ext| self.open_with.get(&ext).cloned())
            .filter(App::installed);
        let default = if !platform::NAMES_APPS {
            Some(App::Default)
        } else if kind == Kind::Folder {
            None
        } else {
            platform::apps_for(path)
                .into_iter()
                .next()
                .map(|(name, path)| App::Other { name, path })
        };
        let app = rule(
            kind,
            kept,
            editor::pick(chosen),
            platform::preview(),
            default,
        );
        *self.open_cache.named.borrow_mut() = Some((path.to_owned(), chosen, app.clone()));
        app
    }

    /// ⌥⌘O: the page's file or folder in its app; with none, it says to choose one.
    pub(super) fn open_in(&mut self, cx: &mut Context<Self>) {
        let Some((path, line)) = self.open_target() else {
            return;
        };
        match self.open_app(&path) {
            Some(app) => self.error = app.open(&path, line).err().map(Into::into),
            None => {
                self.error =
                    Some("Nothing here opens this file. Choose an app in Open in’s ▾.".into())
            }
        }
        cx.notify();
    }

    /// ⌥⌘R: the page's file or folder in Finder.
    pub(super) fn reveal_target(&mut self, cx: &mut Context<Self>) {
        if let Some((path, _)) = self.open_target() {
            cx.reveal_path(&path);
        }
    }

    /// Open in's ▾: the apps that open it, the current one ticked; Reveal in Finder, Copy path;
    /// the choice to keep for its extension; Editor settings.
    pub(super) fn open_in_menu(&mut self, at: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let Some((path, _)) = self.open_target() else {
            return;
        };
        let now = self.open_app(&path);
        let kind = kind_of(&path);
        let yours = editor::pick(self.settings.editor);
        let mut apps: Vec<(App, &'static str)> = Vec::new();
        if matches!(kind, Kind::Text | Kind::Folder) {
            apps.extend(Editor::ALL.into_iter().filter(|e| e.installed()).map(|e| {
                (
                    App::Editor(e),
                    if Some(e) == yours { "your editor" } else { "" },
                )
            }));
        }
        if kind != Kind::Folder {
            if !platform::NAMES_APPS {
                apps.push((App::Default, ""));
            }
            for (n, (name, at)) in platform::apps_for(&path).into_iter().enumerate() {
                // An editor the system lists is the one above, which opens at the line.
                let editor = Editor::ALL.into_iter().any(|e| e.bundle() == name);
                if !editor && !apps.iter().any(|(a, _)| a.name() == name) {
                    let note = if n == 0 { "the default" } else { "" };
                    apps.push((App::Other { name, path: at }, note));
                }
            }
        }
        if let Some(now) = &now
            && !apps.iter().any(|(a, _)| a == now)
        {
            apps.insert(0, (now.clone(), ""));
        }
        let open = apps
            .into_iter()
            .map(|(app, note)| {
                let on = now.as_ref() == Some(&app);
                Entry::new(app.name(), Act::OpenWith(app))
                    .key(note)
                    .checked(on)
            })
            .collect();
        let places = vec![
            Entry::new(REVEAL, Act::Reveal(path.clone())).key("⌥⌘R"),
            Entry::new("Copy path", Act::Copy(path.display().to_string())),
        ];
        let mut keep = Vec::new();
        if let (Some(ext), Some(app)) = (extension(&path).filter(|_| kind != Kind::Folder), &now) {
            let kept = self.open_with.get(&ext) == Some(app);
            keep.push(
                Entry::new(
                    format!("Always open .{ext} in {}", app.name()),
                    Act::AlwaysOpen(ext),
                )
                .checked(kept),
            );
        }
        keep.push(Entry::new("Editor settings…", Act::EditorSettings));
        self.open_titled(
            at,
            vec![(Some("Open with"), open), (None, places), (None, keep)],
            cx,
        );
    }

    /// A pick in the ▾: the target opens in `app`, and the button names it for the target until
    /// another is picked or the session ends.
    pub(super) fn open_with_app(&mut self, app: App) {
        if let Some((path, line)) = self.open_target() {
            self.error = app.open(&path, line).err().map(Into::into);
            self.open_cache.picked = Some((path, app));
        }
    }

    /// A file or folder of Code's tree opened in `app`.
    pub(super) fn open_path_with(&mut self, path: &Path, app: App) {
        self.error = app.open(path, 1).err().map(Into::into);
    }

    /// "Always open .ext files in …": kept, or let go when it was.
    pub(super) fn always_open(&mut self, ext: String) {
        let Some((path, _)) = self.open_target() else {
            return;
        };
        match self.open_app(&path) {
            Some(app) if self.open_with.get(&ext) != Some(&app) => {
                self.open_with.insert(ext, app);
            }
            _ => {
                self.open_with.remove(&ext);
            }
        }
        save(&self.open_with);
        *self.open_cache.named.borrow_mut() = None;
    }

    /// The bar's Open in: the button naming the app with ⌥⌘O, and its ▾; "Choose an app…" when
    /// nothing opens it, which opens the ▾.
    pub(super) fn open_in_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (path, _) = self.open_target()?;
        let main = match self.open_app(&path) {
            Some(app) => self
                .bar_action(
                    "bar-open-in",
                    "external",
                    &format!("Open in {}", app.name()),
                    "⌥⌘O",
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_in(cx))),
            None => self
                .bar_action("bar-open-in", "external", "Choose an app…", "")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, e: &MouseDownEvent, _, cx| {
                        this.open_in_menu(e.position, cx)
                    }),
                ),
        };
        let more = div()
            .id("bar-open-in-more")
            .flex_none()
            .h(px(28.))
            .w(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_r(px(super::ROW_RADIUS))
            .border_1()
            .border_l_0()
            .border_color(theme::island_border())
            .cursor_pointer()
            .hover(|s| s.bg(theme::hover()))
            .child(
                icons::icon("chevron")
                    .size(px(13.))
                    .text_color(theme::muted()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| this.open_in_menu(e.position, cx)),
            );
        Some(
            div()
                .flex()
                .flex_none()
                .child(main.rounded_r(px(0.)))
                .child(more)
                .into_any_element(),
        )
    }

    /// A file Code cannot show as text: so it says, with Open in and Reveal in Finder.
    pub(super) fn render_binary(&self, size: u64, cx: &mut Context<Self>) -> AnyElement {
        let app = self
            .open_target()
            .and_then(|(path, _)| self.open_app(&path));
        let label = app.as_ref().map_or_else(
            || "Choose an app…".to_owned(),
            |a| format!("Open in {}", a.name()),
        );
        let known = app.is_some();
        let actions = div()
            .flex()
            .gap_2()
            .child(
                super::bar::bar_button("binary-open", &label, "", false).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                        if known {
                            this.open_in(cx)
                        } else {
                            this.open_in_menu(e.position, cx)
                        }
                    }),
                ),
            )
            .child(
                super::bar::bar_button("binary-reveal", REVEAL, "", false)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reveal_target(cx))),
            );
        super::roots::empty_state(
            "file",
            "This file can’t be shown as text".to_owned(),
            format!(
                "{} of binary data.",
                super::image_view::bytes(size as usize)
            ),
            Some(actions.into_any_element()),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> Option<App> {
        Some(App::Other {
            name: name.to_owned(),
            path: PathBuf::from(format!("/Applications/{name}.app")),
        })
    }

    #[test]
    fn each_kind_names_its_app() {
        let zed = Some(Editor::Zed);
        let preview = Some(PathBuf::from("/System/Applications/Preview.app"));
        let name = |kind, kept, editor, default| {
            rule(kind, kept, editor, preview.clone(), default).map(|a| a.name())
        };
        // A text file and a folder: the editor; with none, the system's default.
        assert_eq!(
            name(Kind::Text, None, zed, app("TextEdit")).as_deref(),
            Some("Zed")
        );
        assert_eq!(name(Kind::Folder, None, zed, None).as_deref(), Some("Zed"));
        assert_eq!(
            name(Kind::Text, None, None, app("TextEdit")).as_deref(),
            Some("TextEdit")
        );
        // A picture: Preview, whatever the default.
        assert_eq!(
            name(Kind::Picture, None, zed, app("Pixelmator")).as_deref(),
            Some("Preview")
        );
        // Anything else: the system's default; with none, nothing to name — the button asks.
        assert_eq!(
            name(Kind::Other, None, zed, app("Adobe Photoshop")).as_deref(),
            Some("Adobe Photoshop")
        );
        assert_eq!(name(Kind::Other, None, zed, None), None);
        // A choice kept for the extension comes first.
        let kept = Some(App::Editor(Editor::VsCode));
        assert_eq!(
            name(Kind::Text, kept, zed, None).as_deref(),
            Some("VS Code")
        );
    }

    #[test]
    fn a_kept_choice_reads_back() {
        for app in [
            App::Editor(Editor::Zed),
            App::Default,
            App::Other {
                name: "Adobe Photoshop".into(),
                path: "/Applications/Adobe Photoshop 2026/Adobe Photoshop 2026.app".into(),
            },
        ] {
            assert_eq!(App::parse(&app.key()), Some(app));
        }
    }
}
