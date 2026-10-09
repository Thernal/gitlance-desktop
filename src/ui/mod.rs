//! The GitLance window: branches, versions and commits on the left, the diff on the right.

mod annotate;
mod bookmarks;
mod comments;
mod create;
mod diff_view;
mod fetch;
mod find;
mod findfiles;
mod format;
mod icons;
mod input;
mod lists;
mod menu;
mod palette;
mod recents;
mod requests;
mod reviewed;
mod rows;
mod select;
mod settings;
mod shell;
mod shortcuts;
mod theme;
mod watch;
mod working;
mod zones;

use crate::git::{
    BranchRef, ChangeKind, CommitInfo, DiffSettings, FileDiff, PairCommit, PairKind, RangePair,
    Repo, Version,
};
use crate::storage::{self, DiffMode, Layout, Settings, ViewOptions};
use diff_view::{RowStyle, chip};
use gpui::{
    App, ClickEvent, Context, CursorStyle, FocusHandle, FontWeight, KeyBinding, KeyDownEvent,
    ListAlignment, ListState, Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent,
    PathPromptOptions, Point, Rgba, ScrollStrategy, ScrollWheelEvent, SharedString, Task,
    TitlebarOptions, UniformListScrollHandle, Window, WindowBounds, WindowOptions, actions, canvas,
    div, font, list, prelude::*, px, size, uniform_list,
};
use lists::{Field, History};
use menu::{Act, CtxMenu, Entry};
use rows::{FileData, Row};
use select::Sel;
use std::cell::Cell as Shared;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

actions!(
    gitlance,
    [
        Open,
        Refresh,
        PreviousCommit,
        NextCommit,
        PreviousFile,
        NextFile,
        ToggleUnified,
        CycleMode,
        SetLines,
        SetWords,
        SetStructural,
        NextMatch,
        PreviousMatch,
        FindCommits,
        FilterFiles,
        GoBack,
        GoForward,
        ToggleSidebar,
        ToggleFiles,
        ToggleRequests,
        FocusDiff,
        CopySelection,
        SelectAll,
        NextChange,
        PreviousChange,
        OpenSettings,
        Find,
        ToggleWrap,
        ToggleFullContext,
        ToggleWhitespace,
        CommentLine,
        SubmitReview,
        CreateRequest,
        ShowShortcuts,
        ToggleThread,
        ToggleComments,
        MarkReviewed,
        OpenRecent,
        ToggleBookmark,
        ShowBookmarks,
        ShowStructure,
        ToggleAnnotate,
        FindInFiles,
        ToggleReviewed,
        NextThread,
        PreviousThread,
        ReplyThread,
        ResolveThread,
        NextZone,
        PreviousZone,
        Activate,
        PageUp,
        PageDown,
        DiffStart,
        DiffEnd,
        FocusBranches,
        FocusCommits,
        FocusFiles,
        FocusDiffZone,
        NewTab,
        CloseTab,
        ReopenTab,
        NextTab,
        PreviousTab,
        Tab1,
        Tab2,
        Tab3,
        Tab4,
        Tab5,
        Tab6,
        Tab7,
        Tab8,
        Tab9,
        OpenPalette,
        Compare,
        PaletteUp,
        PaletteDown,
        Quit
    ]
);

/// How much history a branch shows.
const COMMIT_LIMIT: usize = 5000;
const DIFF_ROW: f32 = 20.;
const CODE_SIZE: f32 = 12.;
const TITLE_BAR: f32 = 38.;
/// Space between islands; the resize handles live in it.
const GAP: f32 = 8.;
const ISLAND_RADIUS: f32 = 10.;
const ROW_RADIUS: f32 = 6.;

pub fn run(path: Option<PathBuf>) {
    gpui_platform::application()
        .with_assets(icons::Icons)
        .run(move |cx: &mut App| {
            cx.bind_keys([
                KeyBinding::new("cmd-o", Open, None),
                KeyBinding::new("cmd-r", Refresh, None),
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("cmd-,", OpenSettings, None),
                KeyBinding::new("cmd-f", Find, Some("Workspace")),
                KeyBinding::new("cmd-shift-f", FindCommits, Some("Workspace")),
                KeyBinding::new("cmd-p", FilterFiles, Some("Workspace")),
                KeyBinding::new("cmd-[", GoBack, Some("Workspace")),
                KeyBinding::new("cmd-]", GoForward, Some("Workspace")),
                KeyBinding::new("cmd-alt-1", ToggleSidebar, Some("Workspace")),
                KeyBinding::new("cmd-alt-2", ToggleFiles, Some("Workspace")),
                KeyBinding::new("cmd-alt-3", ToggleRequests, Some("Workspace")),
                KeyBinding::new("c", CommentLine, Some("Workspace && !Typing")),
                KeyBinding::new(
                    "cmd-shift-enter",
                    SubmitReview,
                    Some("Workspace && !Typing"),
                ),
                KeyBinding::new("cmd-alt-m", CreateRequest, Some("Workspace && !Typing")),
                KeyBinding::new("?", ShowShortcuts, Some("Workspace && !Typing")),
                KeyBinding::new("shift-n", NextThread, Some("Workspace && !Typing")),
                KeyBinding::new("shift-p", PreviousThread, Some("Workspace && !Typing")),
                KeyBinding::new("r", ReplyThread, Some("Workspace && !Typing")),
                KeyBinding::new("x", ResolveThread, Some("Workspace && !Typing")),
                KeyBinding::new("o", ToggleThread, Some("Workspace && !Typing")),
                KeyBinding::new("cmd-shift-r", ToggleComments, Some("Workspace && !Typing")),
                KeyBinding::new("v", MarkReviewed, Some("Workspace && !Typing")),
                KeyBinding::new("cmd-e", OpenRecent, Some("Workspace")),
                KeyBinding::new("f3", ToggleBookmark, Some("Workspace && !Typing")),
                KeyBinding::new("cmd-f3", ShowBookmarks, Some("Workspace")),
                KeyBinding::new("cmd-shift-o", ShowStructure, Some("Workspace")),
                KeyBinding::new("cmd-alt-b", ToggleAnnotate, Some("Workspace && !Typing")),
                KeyBinding::new("cmd-alt-f", FindInFiles, Some("Workspace")),
                KeyBinding::new("shift-v", ToggleReviewed, Some("Workspace && !Typing")),
                KeyBinding::new("tab", NextZone, Some("Workspace && !Typing")),
                KeyBinding::new("shift-tab", PreviousZone, Some("Workspace && !Typing")),
                KeyBinding::new("enter", Activate, Some("Workspace && !Typing")),
                KeyBinding::new("pagedown", PageDown, Some("Workspace && !Typing")),
                KeyBinding::new("space", PageDown, Some("Workspace && !Typing")),
                KeyBinding::new("pageup", PageUp, Some("Workspace && !Typing")),
                KeyBinding::new("shift-space", PageUp, Some("Workspace && !Typing")),
                KeyBinding::new("home", DiffStart, Some("Workspace && !Typing")),
                KeyBinding::new("end", DiffEnd, Some("Workspace && !Typing")),
                KeyBinding::new("h", PreviousFile, Some("Workspace && !Typing")),
                KeyBinding::new("l", NextFile, Some("Workspace && !Typing")),
                KeyBinding::new("ctrl-1", FocusBranches, Some("Workspace")),
                KeyBinding::new("ctrl-2", FocusCommits, Some("Workspace")),
                KeyBinding::new("ctrl-3", FocusFiles, Some("Workspace")),
                KeyBinding::new("ctrl-4", FocusDiffZone, Some("Workspace")),
                KeyBinding::new("cmd-t", NewTab, Some("Workspace")),
                KeyBinding::new("cmd-w", CloseTab, Some("Workspace")),
                KeyBinding::new("cmd-shift-t", ReopenTab, Some("Workspace")),
                KeyBinding::new("cmd-shift-]", NextTab, Some("Workspace")),
                KeyBinding::new("cmd-shift-[", PreviousTab, Some("Workspace")),
                KeyBinding::new("cmd-1", Tab1, Some("Workspace")),
                KeyBinding::new("cmd-2", Tab2, Some("Workspace")),
                KeyBinding::new("cmd-3", Tab3, Some("Workspace")),
                KeyBinding::new("cmd-4", Tab4, Some("Workspace")),
                KeyBinding::new("cmd-5", Tab5, Some("Workspace")),
                KeyBinding::new("cmd-6", Tab6, Some("Workspace")),
                KeyBinding::new("cmd-7", Tab7, Some("Workspace")),
                KeyBinding::new("cmd-8", Tab8, Some("Workspace")),
                KeyBinding::new("cmd-9", Tab9, Some("Workspace")),
                KeyBinding::new("cmd-.", FocusDiff, Some("Workspace")),
                KeyBinding::new("cmd-c", CopySelection, Some("Workspace && !Typing")),
                KeyBinding::new("cmd-a", SelectAll, Some("Workspace && !Typing")),
                KeyBinding::new("f7", NextChange, Some("Workspace")),
                KeyBinding::new("shift-f7", PreviousChange, Some("Workspace")),
                KeyBinding::new("n", NextChange, Some("Workspace && !Typing")),
                KeyBinding::new("p", PreviousChange, Some("Workspace && !Typing")),
                KeyBinding::new("cmd-g", NextMatch, Some("Workspace")),
                KeyBinding::new("cmd-shift-g", PreviousMatch, Some("Workspace")),
                KeyBinding::new("up", PreviousCommit, Some("Workspace && !Palette")),
                KeyBinding::new("down", NextCommit, Some("Workspace && !Palette")),
                KeyBinding::new("up", PaletteUp, Some("Palette")),
                KeyBinding::new("down", PaletteDown, Some("Palette")),
                KeyBinding::new("cmd-k", OpenPalette, Some("Workspace")),
                KeyBinding::new("cmd-shift-c", Compare, Some("Workspace")),
                // Bare letters and arrows are typing while the search field is active.
                KeyBinding::new("k", PreviousCommit, Some("Workspace && !Typing")),
                KeyBinding::new("j", NextCommit, Some("Workspace && !Typing")),
                KeyBinding::new("left", PreviousFile, Some("Workspace && !Typing")),
                KeyBinding::new("right", NextFile, Some("Workspace && !Typing")),
                KeyBinding::new("w", CycleMode, Some("Workspace && !Typing")),
                KeyBinding::new("alt-u", ToggleUnified, Some("Workspace")),
                KeyBinding::new("alt-d", CycleMode, Some("Workspace")),
                KeyBinding::new("alt-z", ToggleWrap, Some("Workspace")),
                KeyBinding::new("alt-e", ToggleFullContext, Some("Workspace")),
                KeyBinding::new("alt-w", ToggleWhitespace, Some("Workspace")),
            ]);
            if !cfg!(feature = "snapshot") || std::env::var_os("GITLANCE_SNAPSHOT").is_none() {
                crate::dock::set_icon();
            }
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.set_menus(menus(&ViewOptions::load(&Settings::load())));

            let (width, height) = window_size();
            let bounds = gpui::Bounds::centered(None, size(px(width), px(height)), cx);
            // A snapshot renders offscreen: no window shown, no focus taken.
            let offscreen =
                cfg!(feature = "snapshot") && std::env::var_os("GITLANCE_SNAPSHOT").is_some();
            let options = WindowOptions {
                show: !offscreen,
                focus: !offscreen,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("GitLance".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(Point::new(px(12.), px(12.))),
                }),
                window_min_size: Some(size(px(900.), px(500.))),
                ..Default::default()
            };
            let window = cx
                .open_window(options, |window, cx| {
                    cx.new(|cx| shell::Shell::new(path, window, cx))
                })
                .expect("open the window");
            if !offscreen {
                cx.activate(true);
            }
            #[cfg(feature = "snapshot")]
            snapshot(window, cx);
            #[cfg(not(feature = "snapshot"))]
            let _ = window;
        });
}

/// The window's size: 1480 × 920, or `GITLANCE_SNAPSHOT_SIZE=WxH` in a snapshot (to check the
/// layout of a small window).
fn window_size() -> (f32, f32) {
    #[cfg(feature = "snapshot")]
    if let Some((w, h)) = std::env::var("GITLANCE_SNAPSHOT_SIZE")
        .ok()
        .and_then(|v| v.split_once('x').map(|(w, h)| (w.to_owned(), h.to_owned())))
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
    {
        return (w, h);
    }
    (1480., 920.)
}

/// The menu bar; View items carry a check mark for the options in effect.
fn menus(options: &ViewOptions) -> Vec<Menu> {
    vec![
        Menu::new("GitLance").items([
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit GitLance", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Tab", NewTab),
            MenuItem::action("Close Tab", CloseTab),
            MenuItem::action("Reopen Closed Tab", ReopenTab),
            MenuItem::separator(),
            MenuItem::action("Open Repository…", Open),
            MenuItem::action("Create Merge Request…", CreateRequest),
            MenuItem::action("Refresh", Refresh),
            MenuItem::separator(),
            MenuItem::action("Find in Diff…", Find),
            MenuItem::action("Find Next", NextMatch),
            MenuItem::action("Find Previous", PreviousMatch),
            MenuItem::separator(),
            MenuItem::action("Find Commits…", FindCommits),
            MenuItem::action("Filter Files…", FilterFiles),
            MenuItem::separator(),
            MenuItem::action("Next Change", NextChange),
            MenuItem::action("Previous Change", PreviousChange),
        ]),
        Menu::new("Go").items([
            MenuItem::action("Back", GoBack),
            MenuItem::action("Forward", GoForward),
            MenuItem::separator(),
            MenuItem::action("Next Tab", NextTab),
            MenuItem::action("Previous Tab", PreviousTab),
        ]),
        Menu::new("View").items([
            MenuItem::action("Show Branches and Commits", ToggleSidebar),
            MenuItem::action("Show Files", ToggleFiles),
            MenuItem::action("Focus on the Diff", FocusDiff),
            MenuItem::separator(),
            MenuItem::action("Unified Diff", ToggleUnified).checked(options.unified),
            MenuItem::action("Line Diff", SetLines).checked(options.mode == DiffMode::Lines),
            MenuItem::action("Word Diff", SetWords).checked(options.mode == DiffMode::Words),
            MenuItem::action("Structural Diff (difftastic)", SetStructural)
                .checked(options.mode == DiffMode::Structural),
            MenuItem::separator(),
            MenuItem::separator(),
            MenuItem::action("Wrap Long Lines", ToggleWrap).checked(options.wrap),
            MenuItem::action("Show All Lines", ToggleFullContext).checked(options.full_context),
            MenuItem::action("Hide Whitespace Changes", ToggleWhitespace)
                .checked(options.ignore_whitespace),
        ]),
    ]
}

/// A view option, for the toolbar and the View menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opt {
    Unified,
    Wrap,
    FullContext,
    Whitespace,
}

/// Waits for loading (`GITLANCE_SNAPSHOT_DELAY_MS`, 2500 by default), selects a commit by SHA
/// prefix (`GITLANCE_SNAPSHOT_COMMIT`) and a file by path (`GITLANCE_SNAPSHOT_FILE`), turns on view options
/// (`GITLANCE_SNAPSHOT_VIEW=unified,structural,wrap,all-lines,whitespace`, not saved), optionally
/// compares the first and last versions (`GITLANCE_SNAPSHOT_VERSIONS=1`), then writes the frame to
/// `GITLANCE_SNAPSHOT` and quits.
#[cfg(feature = "snapshot")]
fn snapshot(window: gpui::WindowHandle<shell::Shell>, cx: &mut App) {
    use std::time::Duration;
    let Some(out) = std::env::var_os("GITLANCE_SNAPSHOT").map(PathBuf::from) else {
        return;
    };
    let versions = std::env::var_os("GITLANCE_SNAPSHOT_VERSIONS").is_some();
    let Ok(shell) = window.entity(cx) else {
        return;
    };
    let ws = shell.read(cx).first();
    cx.spawn(async move |cx| {
        let executor = cx.background_executor().clone();
        let wait = |ms| executor.timer(Duration::from_millis(ms));
        let delay = std::env::var("GITLANCE_SNAPSHOT_DELAY_MS")
            .ok()
            .and_then(|d| d.parse().ok())
            .unwrap_or(2500);
        wait(delay).await;
        if let Ok(path) = std::env::var("GITLANCE_SNAPSHOT_TABS") {
            // A second repository in a tab of its own.
            window
                .update(cx, |shell, window, cx| {
                    shell.open_tab(path.into(), window, cx)
                })
                .ok();
            wait(2500).await;
        }
        if let Ok(prefix) = std::env::var("GITLANCE_SNAPSHOT_COMMIT") {
            ws.update(cx, |this, cx| {
                let found = this
                    .commits
                    .iter()
                    .position(|c| c.id.to_string().starts_with(&prefix));
                if let Some(ix) = found {
                    this.select_commit(ix, cx);
                }
            });
            wait(1500).await;
        }
        if let Ok(path) = std::env::var("GITLANCE_SNAPSHOT_FILE") {
            ws.update(cx, |this, cx| {
                let found = this
                    .diff
                    .as_ref()
                    .and_then(|d| d.files.iter().position(|f| f.path() == path));
                if let Some(ix) = found {
                    this.select_file(ix, cx);
                }
            });
            wait(1500).await;
        }
        if let Ok(view) = std::env::var("GITLANCE_SNAPSHOT_VIEW") {
            ws.update(cx, |this, cx| {
                for name in view.split(',') {
                    let opt = match name.trim() {
                        "requests" => {
                            this.requests.side = requests::Side::Requests;
                            continue;
                        }
                        "unified" => Opt::Unified,
                        "split" => {
                            this.set_option_unsaved(Opt::Unified, false, cx);
                            continue;
                        }
                        "lines" => {
                            this.set_mode(DiffMode::Lines, cx);
                            continue;
                        }
                        "words" => {
                            this.set_mode(DiffMode::Words, cx);
                            continue;
                        }
                        "structural" => {
                            this.set_mode(DiffMode::Structural, cx);
                            continue;
                        }
                        "underlined" => {
                            // Not saved either: only this frame.
                            this.settings.mark_style = crate::storage::MarkStyle::Underlined;
                            continue;
                        }
                        "comments" => {
                            // Unsaved sample comments on the first changed line, one being written.
                            let (Some(data), Some(path)) = (&this.data, this.current_path()) else {
                                continue;
                            };
                            let target = this.rows.iter().find_map(|row| match row {
                                Row::Split { right: Some(c), .. }
                                | Row::Unified {
                                    cell: c,
                                    old: false,
                                    ..
                                } if c.kind == crate::git::LineKind::Added => Some(c.line),
                                _ => None,
                            });
                            if let Some(line) = target {
                                let code = data.side(false).line(line).0.to_owned();
                                this.comments.push(crate::review::Comment {
                                    id: 1,
                                    path: path.clone(),
                                    old: false,
                                    line,
                                    code: code.clone(),
                                    body: "Clamp this value — the API rejects anything else."
                                        .into(),
                                    at: "a10f3cf".into(),
                                });
                                this.start_comment(false, line + 1, cx);
                                if let Some(c) = this.compose.as_mut() {
                                    c.body = "Name this after what it holds.".into();
                                }
                                this.refresh_rows();
                            }
                            continue;
                        }
                        "notice" => {
                            this.notice = Some(Notice {
                                text: "v3 arrived — this comparison now ends at v3.".into(),
                                undo: Selection::None,
                            });
                            continue;
                        }
                        "viewmenu" => {
                            let groups = this.view_menu();
                            this.open_menu(Point::new(px(1090.), px(78.)), groups, cx);
                            continue;
                        }
                        "linemenu" => {
                            this.sel = Some(Sel {
                                old: false,
                                anchor: select::Pos { line: 66, byte: 4 },
                                head: select::Pos { line: 68, byte: 12 },
                            });
                            this.line_context(false, 67, Point::new(px(900.), px(520.)), cx);
                            continue;
                        }
                        "menu" => {
                            this.repo_menu = true;
                            continue;
                        }
                        "settings" => {
                            this.open_settings(cx);
                            continue;
                        }
                        "wrap" => Opt::Wrap,
                        "all-lines" => Opt::FullContext,
                        "whitespace" => Opt::Whitespace,
                        _ => continue,
                    };
                    // Not saved: a snapshot must not change the user's settings.
                    this.set_option_unsaved(opt, true, cx);
                }
            });
            wait(1500).await;
        }
        if let Ok(text) = std::env::var("GITLANCE_SNAPSHOT_FIND") {
            ws.update(cx, |this, cx| {
                this.start_dfind(cx);
                this.set_dfind(text, cx);
                this.step_change(true, cx);
            });
            wait(800).await;
        }
        if let Ok(text) = std::env::var("GITLANCE_SNAPSHOT_SEARCH") {
            ws.update(cx, |this, cx| {
                this.find.text = Some(text);
                this.refilter();
                this.recompute_matches();
                cx.notify();
            });
            wait(800).await;
        }
        if versions {
            ws.update(cx, |this, cx| {
                if this.versions.len() > 1 {
                    this.select_versions(0, this.versions.len() - 1, cx);
                }
            });
            wait(1500).await;
        }
        if std::env::var("GITLANCE_SNAPSHOT_VIEW").is_ok_and(|v| v.contains("interdiff")) {
            ws.update(cx, |this, cx| this.select_interdiff(0, cx));
            wait(1500).await;
        }
        if std::env::var("GITLANCE_SNAPSHOT_VIEW").is_ok_and(|v| v.contains("worktree")) {
            ws.update(cx, |this, cx| this.select_working_tree(cx));
            wait(2500).await;
        }
        if let Ok(spec) = std::env::var("GITLANCE_SNAPSHOT_COMPARE") {
            ws.update(cx, |this, cx| {
                let repo = this.root.as_ref().and_then(|r| Repo::open(r).ok());
                let mut sides = spec
                    .split(',')
                    .filter_map(|s| Some((s.to_owned(), repo.as_ref()?.resolve(s).ok()?)));
                if let (Some(base), Some(head)) = (sides.next(), sides.next()) {
                    this.run_compare(base, head, true, None, cx);
                }
            });
            wait(1500).await;
        }
        if let Ok(query) = std::env::var("GITLANCE_SNAPSHOT_PALETTE") {
            ws.update(cx, |this, cx| {
                let kind = if query.starts_with("compare:") {
                    palette::Kind::Base
                } else {
                    palette::Kind::Jump
                };
                this.open_palette(kind, cx);
                this.type_in_palette(query.trim_start_matches("compare:"));
            });
            wait(500).await;
        }
        if let Ok(iid) = std::env::var("GITLANCE_SNAPSHOT_MR") {
            ws.update(cx, |this, cx| {
                if let Ok(iid) = iid.parse() {
                    this.select_request(iid, cx);
                }
            });
            wait(2000).await;
        }
        if let Ok(keys) = std::env::var("GITLANCE_SNAPSHOT_KEYS") {
            // Keystrokes sent to the window itself (not the keyboard), e.g. `right right cmd-t`.
            for key in keys.split_whitespace() {
                if let Ok(keystroke) = gpui::Keystroke::parse(key) {
                    cx.update_window(window.into(), |_, window, cx| {
                        window.dispatch_keystroke(keystroke, cx);
                    })
                    .ok();
                }
                wait(400).await;
            }
        }
        // A hidden window gets no display-link frames: draw one by hand.
        // `update_window` leaves the root view free for the draw to render.
        let saved = cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window
                .render_to_image()
                .and_then(|image| Ok(image.save(&out)?))
        });
        if let Err(err) = saved.and_then(|r| r) {
            eprintln!("snapshot failed: {err:#}");
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    None,
    Commit(usize),
    Versions { from: usize, to: usize },
    WorkingTree,
}

enum Header {
    Commit(CommitInfo),
    Versions {
        from: Version,
        to: Version,
        rebased: bool,
        conflicts: usize,
    },
    /// Uncommitted work against `HEAD`.
    WorkingTree {
        only_unstaged: bool,
    },
    /// Any two refs: what `head` changed against `base`.
    Compare {
        base: (String, git2::Oid),
        head: (String, git2::Oid),
        since_merge_base: bool,
        start: git2::Oid,
        commits: usize,
        /// The merge request this is the diff of, so comments can go to it.
        request: Option<u64>,
    },
    /// One commit of an older version against its counterpart in a newer one.
    Interdiff {
        from: usize,
        to: usize,
        old: PairCommit,
        new: PairCommit,
    },
}

/// A ref's name and the commit it points at.
type NamedTip = (String, git2::Oid);
/// Base, head and merge request of a diff to show.
type PendingCompare = (NamedTip, NamedTip, u64);

/// The working tree's count and fingerprint, polled while a repository is open.
#[derive(Default)]
struct WorkingState {
    count: usize,
    hash: u64,
    only_unstaged: bool,
    task: Option<gpui::Task<()>>,
}

/// What the version comparison shows: the files that changed, or the commits paired up.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VersionTab {
    Files,
    Commits,
}

/// A change the user did not ask for, with the way back.
struct Notice {
    text: String,
    undo: Selection,
}

struct Diff {
    header: Header,
    files: Arc<Vec<FileDiff>>,
    /// The commits of a version comparison, paired (empty for anything else).
    pairs: Vec<RangePair>,
    /// Per path, a word for the file list (the working tree: staged, untracked…).
    tags: std::collections::HashMap<String, &'static str>,
}

/// A pane boundary that can be dragged.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Split {
    Sidebar,
    Files,
}

impl Split {
    fn vertical(self) -> bool {
        false
    }

    fn bounds(self) -> (f32, f32) {
        match self {
            Split::Sidebar => (220., 720.),
            Split::Files => (160., 640.),
        }
    }
}

#[derive(Clone, Copy)]
struct Drag {
    split: Split,
    /// Pointer coordinate along the split's axis when the drag began.
    origin: f32,
    /// Pane size when the drag began.
    size: f32,
}

pub struct Workspace {
    focus: FocusHandle,
    layout: Layout,
    drag: Option<Drag>,
    root: Option<PathBuf>,
    recent: Vec<PathBuf>,
    error: Option<SharedString>,
    title: String,
    branches: Vec<BranchRef>,
    branch: Option<usize>,
    versions: Vec<Version>,
    commits: Vec<CommitInfo>,
    selection: Selection,
    version_tab: VersionTab,
    palette: Option<palette::Palette>,
    zone: zones::Zone,
    fcursor: Option<String>,
    wt: WorkingState,
    fetch: fetch::Fetch,
    shell: Option<gpui::WeakEntity<shell::Shell>>,
    requests: requests::Requests,
    /// A merge request's diff to show once its branch has loaded: base, head, request.
    pending_compare: Option<PendingCompare>,
    /// The merge request whose diff is open (or was last opened from the list).
    open_request: Option<PendingCompare>,
    /// What the last "Test" of the GitLab connection in Settings found.
    gitlab_check: Option<Result<String, String>>,
    /// The card for a new merge request, while it is open.
    newreq: Option<create::NewRequest>,
    /// The keyboard card is open.
    shortcuts: bool,
    bookmarks: bookmarks::Store,
    /// Places opened lately, newest first.
    recents: Vec<recents::Entry>,
    /// The annotation column (who wrote each line) is on, and what blame said of the open file.
    annotate: bool,
    annotations: Vec<diff_view::AnnCell>,
    annot_task: Option<Task<()>>,
    /// The search over all files of the diff, while it is open.
    ffind: Option<findfiles::FindFiles>,
    /// The marks of files read, and the scope and file fingerprints of the open diff.
    reviewed: reviewed::Store,
    review_scope: Option<String>,
    fps: Vec<u64>,
    /// Generated files the reader asked to see in full.
    unfolded: HashSet<String>,
    /// The branch / merge request picker hangs from the title bar.
    picker_open: bool,
    /// The window's width in logical pixels, as of the last frame.
    win_width: f32,
    /// The diff is narrower than 700 px: it is drawn unified whatever the option says.
    narrow: bool,
    /// The files list is open over the diff (a window too narrow for its column).
    files_popover: bool,
    /// The comment marks of the open file's gutter.
    notes: Vec<diff_view::LineNote>,
    /// Discussions whose open or folded state differs from the default (a resolved one is folded).
    toggled_mr: HashSet<usize>,
    /// Agent comments the reader folded.
    toggled_agent: HashSet<u64>,
    /// The discussion `r`, `x` and ⇧n / ⇧p act on (an index into the request's threads).
    thread_at: Option<usize>,
    /// The token being entered in the connect panel; shown as dots.
    token_input: String,
    /// A token is being checked against GitLab.
    testing: bool,
    /// The line number being typed for a quick comment.
    goline: Option<String>,
    /// The diff row the `c` marker stands on.
    goline_row: Option<usize>,
    jump_line: Option<(u32, bool)>,
    tabs: shell::TabModel,
    diff: Option<Diff>,
    file: usize,
    options: ViewOptions,
    settings: Settings,
    settings_open: bool,
    /// The repository menu in the title bar is open.
    repo_menu: bool,
    find: find::Find,
    watch: watch::Watch,
    /// Review comments of this repository, the one being written, and the Review island.
    comments: Vec<crate::review::Comment>,
    compose: Option<comments::Compose>,
    /// A comment to scroll to once its file is laid out.
    jump: Option<u64>,
    /// "Copied ✓" on the Copy button until something changes.
    copied: bool,
    /// Everything in the field that has the keyboard is selected (⌘A).
    field_all: bool,
    /// Back / forward through what was looked at.
    history: History,
    /// Where the View button was painted, so its menu hangs from it.
    view_bounds: Rc<Shared<Option<gpui::Bounds<gpui::Pixels>>>>,
    /// The branch and path filters, and which of them (if any) takes the keyboard.
    bfilter: String,
    pfilter: String,
    field: Option<Field>,
    /// Folders closed in the file tree.
    collapsed: HashSet<String>,
    /// Labels on commits: where branches, remote branches and tags point.
    decor: std::collections::HashMap<git2::Oid, Vec<crate::git::Deco>>,
    /// A context menu open at the pointer, and the repository's address on the web.
    ctx_menu: Option<CtxMenu>,
    web: Option<crate::git::WebRemote>,
    /// The selected text of the open diff, and whether the button is down (a drag extends it).
    sel: Option<Sel>,
    selecting: bool,
    /// The find bar's text over the open diff, and what it looks for.
    dfind: Option<String>,
    dterms: Arc<[String]>,
    /// Rows where a change begins, and the one stepped to (F7).
    changes: Vec<usize>,
    change_at: Option<usize>,
    /// The commit message's full body is shown.
    body_expanded: bool,
    /// Rows of the open diff that hold the find text, and the one stepped to.
    matches: Vec<usize>,
    match_at: usize,
    /// Said above the diff when it changed under the user.
    notice: Option<Notice>,
    /// Scroll position (file path, offset) to put back once the reloaded file is laid out.
    restore_scroll: Option<(String, gpui::ListOffset)>,
    /// The selected file, built in the background.
    data: Option<Arc<FileData>>,
    /// `data` laid out for `options`.
    rows: Vec<Row>,
    /// Unchanged runs of the selected file the user opened.
    expanded: HashSet<usize>,
    diff_list: ListState,
    /// Horizontal scroll of the diff, in pixels.
    offset: f32,
    /// Width of the diff body at the last paint.
    body_width: Rc<Shared<f32>>,
    /// Advance of one code character, measured on the first render.
    char_width: f32,
    commit_scroll: UniformListScrollHandle,
    file_scroll: UniformListScrollHandle,
    // Replacing a task drops, and so cancels, the one it replaces.
    repo_task: Option<Task<()>>,
    branch_task: Option<Task<()>>,
    diff_task: Option<Task<()>>,
    view_task: Option<Task<()>>,
}

impl Workspace {
    /// `open_recent`: with no `path`, open the most recent repository.
    fn new(
        path: Option<PathBuf>,
        open_recent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let recent = storage::recent();
        let settings = Settings::load();
        let start = path.or_else(|| open_recent.then(|| recent.first().cloned()).flatten());
        let mut this = Self {
            focus,
            layout: Layout::load(),
            drag: None,
            root: None,
            recent,
            error: None,
            title: String::new(),
            branches: Vec::new(),
            branch: None,
            versions: Vec::new(),
            commits: Vec::new(),
            selection: Selection::None,
            version_tab: VersionTab::Files,
            palette: None,
            zone: zones::Zone::default(),
            fcursor: None,
            wt: WorkingState::default(),
            fetch: fetch::Fetch::default(),
            shell: None,
            requests: Default::default(),
            pending_compare: None,
            open_request: None,
            gitlab_check: None,
            newreq: None,
            shortcuts: false,
            bookmarks: bookmarks::Store::load(),
            recents: Vec::new(),
            annotate: false,
            annotations: Vec::new(),
            annot_task: None,
            ffind: None,
            reviewed: reviewed::Store::load(),
            review_scope: None,
            fps: Vec::new(),
            unfolded: HashSet::new(),
            picker_open: false,
            win_width: 1480.,
            narrow: false,
            files_popover: false,
            notes: Vec::new(),
            toggled_mr: HashSet::new(),
            toggled_agent: HashSet::new(),
            thread_at: None,
            token_input: String::new(),
            testing: false,
            goline: None,
            goline_row: None,
            jump_line: None,
            tabs: Default::default(),
            diff: None,
            file: 0,
            options: ViewOptions::load(&settings),
            settings,
            settings_open: false,
            repo_menu: false,
            find: find::Find::default(),
            watch: watch::Watch::default(),
            comments: Vec::new(),
            compose: None,
            jump: None,
            copied: false,
            field_all: false,
            history: History::default(),
            view_bounds: Rc::new(Shared::new(None)),
            bfilter: String::new(),
            pfilter: String::new(),
            field: None,
            collapsed: HashSet::new(),
            decor: Default::default(),
            ctx_menu: None,
            web: None,
            sel: None,
            selecting: false,
            dfind: None,
            dterms: Arc::default(),
            changes: Vec::new(),
            change_at: None,
            body_expanded: false,
            matches: Vec::new(),
            match_at: 0,
            notice: None,
            restore_scroll: None,
            data: None,
            rows: Vec::new(),
            expanded: HashSet::new(),
            diff_list: ListState::new(0, ListAlignment::Top, px(400.)),
            offset: 0.,
            body_width: Rc::new(Shared::new(0.)),
            char_width: 0.,
            commit_scroll: UniformListScrollHandle::new(),
            file_scroll: UniformListScrollHandle::new(),
            repo_task: None,
            branch_task: None,
            diff_task: None,
            view_task: None,
        };
        if let Some(path) = start {
            this.open_repo(path, None, cx);
        }
        Self::start_watching(cx);
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.window_activated(cx);
            }
        })
        .detach();
        this
    }

    // ---- loading -------------------------------------------------------------------------

    /// Opens `path`; `keep` re-selects a branch (by refname) and a commit after a refresh.
    fn open_repo(
        &mut self,
        path: PathBuf,
        keep: Option<(String, Option<git2::Oid>)>,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        self.repo_task = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let repo = Repo::open(&path)?;
                    let git_dir = repo.git_dir();
                    let fingerprint = crate::git::fingerprint(&git_dir);
                    let web = repo.web_remote();
                    let decor = repo.decorations();
                    anyhow::Ok((
                        repo.root(),
                        repo.branches()?,
                        git_dir,
                        fingerprint,
                        web,
                        decor,
                    ))
                })
                .await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok((root, branches, git_dir, fingerprint, web, decor)) => {
                        this.web = web;
                        this.decor = decor;
                        this.recent = storage::remember(&root);
                        if this.root.as_ref() != Some(&root) {
                            this.find.reset();
                        }
                        if this.root.as_ref() != Some(&root) {
                            this.comments = crate::review::load(&root);
                            this.compose = None;
                        }
                        this.watch.watch(git_dir, fingerprint);
                        this.start_working_poll(cx);
                        this.start_fetching(cx);
                        this.load_requests(cx);
                        this.root = Some(root);
                        this.branches = branches;
                        let (refname, commit) = keep.unzip();
                        let ix = refname
                            .and_then(|r| this.branches.iter().position(|b| b.refname == r))
                            .or_else(|| this.branches.iter().position(|b| b.is_head))
                            .or((!this.branches.is_empty()).then_some(0));
                        this.clear_branch();
                        if let Some(ix) = ix {
                            this.select_branch(ix, commit.flatten(), cx);
                        }
                    }
                    Err(err) => this.error = Some(format!("{err:#}").into()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn clear_branch(&mut self) {
        self.watch.forget();
        self.branch = None;
        self.versions.clear();
        self.commits.clear();
        self.clear_diff();
    }

    fn clear_diff(&mut self) {
        self.unfolded.clear();
        self.notice = None;
        self.body_expanded = false;
        self.selection = Selection::None;
        self.diff = None;
        self.diff_task = None;
        self.clear_file();
        self.file = 0;
    }

    fn clear_file(&mut self) {
        self.annotations.clear();
        self.data = None;
        self.view_task = None;
        self.rows.clear();
        self.expanded.clear();
        self.sel = None;
        self.matches.clear();
        self.match_at = 0;
        self.changes.clear();
        self.change_at = None;
        self.offset = 0.;
        self.diff_list.reset(0);
    }

    fn select_branch(&mut self, ix: usize, keep: Option<git2::Oid>, cx: &mut Context<Self>) {
        self.sync_request(ix, cx);
        let (Some(root), Some(branch)) = (self.root.clone(), self.branches.get(ix)) else {
            return;
        };
        let (refname, tip) = (branch.refname.clone(), branch.tip);
        self.clear_branch();
        self.open_request = None;
        self.branch = Some(ix);
        self.branch_task = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let repo = Repo::open(&root)?;
                    // A ref without a reflog simply has no earlier versions.
                    let versions = repo.versions(&refname).unwrap_or_default();
                    anyhow::Ok((versions, repo.log(tip, COMMIT_LIMIT)?))
                })
                .await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok((versions, commits)) => {
                        this.versions = versions;
                        this.commits = commits;
                        this.refilter();
                        this.index_paths(cx);
                        let ix = keep
                            .and_then(|id| this.commits.iter().position(|c| c.id == id))
                            .unwrap_or(0);
                        if !this.commits.is_empty() {
                            this.select_commit(ix, cx);
                        }
                        if let Some((base, head, iid)) = this.pending_compare.take() {
                            this.open_request = Some((base.clone(), head.clone(), iid));
                            this.show_request_commits(&base, &head);
                            this.apply_mr_versions();
                            this.run_compare(base, head, true, Some(iid), cx);
                        }
                    }
                    Err(err) => this.error = Some(format!("{err:#}").into()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn select_commit(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.load_commit(ix, None, cx);
    }

    /// Shows commit `ix`; `keep` re-selects a file by path.
    fn load_commit(&mut self, ix: usize, keep: Option<String>, cx: &mut Context<Self>) {
        let (Some(root), Some(commit)) = (self.root.clone(), self.commits.get(ix).cloned()) else {
            return;
        };
        self.clear_diff();
        self.selection = Selection::Commit(ix);
        self.record();
        self.remember_place(
            recents::Key::Commit(commit.id),
            commit.summary.clone(),
            format!("commit {}", format::short(commit.id)),
        );
        if let Some(row) = self.row_of(ix) {
            self.commit_scroll
                .scroll_to_item(row, ScrollStrategy::Nearest);
        }
        let id = commit.id;
        let settings = self.settings();
        self.load_diff(keep, cx, move || {
            let files = Repo::open(&root)?.commit_diff(id, settings)?;
            Ok(Diff {
                header: Header::Commit(commit),
                files: Arc::new(files),
                pairs: Vec::new(),
                tags: Default::default(),
            })
        });
    }

    fn select_versions(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        self.load_versions(from, to, None, cx);
    }

    fn load_versions(
        &mut self,
        from: usize,
        to: usize,
        keep: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let (Some(root), Some(a), Some(b)) = (
            self.root.clone(),
            self.versions.get(from).cloned(),
            self.versions.get(to).cloned(),
        ) else {
            return;
        };
        if keep.is_none() {
            self.version_tab = if b.commits > 1 {
                VersionTab::Commits
            } else {
                VersionTab::Files
            };
        }
        self.clear_diff();
        self.selection = Selection::Versions { from, to };
        self.record();
        let settings = self.settings();
        self.load_diff(keep, cx, move || {
            let repo = Repo::open(&root)?;
            let diff = repo.version_diff(&a, &b, settings)?;
            let pairs = repo.range_pairs(&a, &b).unwrap_or_default();
            Ok(Diff {
                header: Header::Versions {
                    from: a,
                    to: b,
                    rebased: diff.rebased,
                    conflicts: diff.conflicts.len(),
                },
                files: Arc::new(diff.files),
                pairs,
                tags: Default::default(),
            })
        });
    }

    /// The interdiff of one modified commit of the comparison `from`..`to`.
    fn select_interdiff(&mut self, pair: usize, cx: &mut Context<Self>) {
        let (Some(root), Selection::Versions { from, to }) = (self.root.clone(), self.selection)
        else {
            return;
        };
        let Some((old, new)) = self
            .diff
            .as_ref()
            .and_then(|d| d.pairs.get(pair))
            .and_then(|p| Some((p.old.clone()?, p.new.clone()?)))
        else {
            return;
        };
        self.clear_diff();
        self.selection = Selection::Versions { from, to };
        let settings = self.settings();
        self.load_diff(None, cx, move || {
            let diff = Repo::open(&root)?.interdiff(old.id, new.id, settings)?;
            Ok(Diff {
                header: Header::Interdiff { from, to, old, new },
                files: Arc::new(diff.files),
                pairs: Vec::new(),
                tags: Default::default(),
            })
        });
    }

    fn settings(&self) -> DiffSettings {
        DiffSettings {
            ignore_whitespace: self.options.ignore_whitespace,
        }
    }

    fn load_diff(
        &mut self,
        keep: Option<String>,
        cx: &mut Context<Self>,
        load: impl FnOnce() -> anyhow::Result<Diff> + Send + 'static,
    ) {
        self.diff_task = Some(cx.spawn(async move |this, cx| {
            let loaded = cx.background_executor().spawn(async move { load() }).await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok(diff) => {
                        let ix =
                            keep.and_then(|path| diff.files.iter().position(|f| f.path() == path));
                        let empty = diff.files.is_empty();
                        this.diff = Some(diff);
                        this.index_review();
                        // The first file as the island lists it (a tree sorts folders first), so
                        // → walks on from the top.
                        let ix = ix
                            .or_else(|| this.file_order().first().copied())
                            .unwrap_or(0);
                        this.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
                        if !empty {
                            this.select_file(ix, cx);
                        }
                    }
                    Err(err) => this.error = Some(format!("{err:#}").into()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn select_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(file) = self.diff.as_ref().and_then(|d| d.files.get(ix)).cloned() else {
            return;
        };
        self.file = ix;
        self.fcursor = None;
        self.remember_place(
            recents::Key::File(file.path().to_owned()),
            file.path().to_owned(),
            "file".to_owned(),
        );
        if self.files_popover {
            self.files_popover = false;
            self.zone = zones::Zone::Diff;
        }
        self.clear_file();
        self.scroll_file_into_view(ix);
        let mode = self.options.mode;
        self.view_task = Some(cx.spawn(async move |this, cx| {
            let data = cx
                .background_executor()
                .spawn(async move { FileData::build(&file, mode) })
                .await;
            this.update(cx, |this, cx| {
                this.data = Some(Arc::new(data));
                this.refresh_annotations(cx);
                this.relayout();
                this.apply_jump(cx);
                if let Some((path, offset)) = this.restore_scroll.take()
                    && this
                        .diff
                        .as_ref()
                        .and_then(|d| d.files.get(this.file))
                        .map(|f| f.path())
                        == Some(path.as_str())
                {
                    this.diff_list.scroll_to(offset);
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Lays the selected file out again for the current options; the diff scrolls to the top.
    fn relayout(&mut self) {
        let rows = self
            .data
            .as_ref()
            .map(|data| rows::layout(data, &self.view_options(), &self.expanded))
            .unwrap_or_default();
        self.rows = self.with_comments(rows);
        self.notes = self.line_notes();
        self.diff_list.reset(self.rows.len());
        self.offset = self.offset.min(self.max_offset());
        self.recompute_matches();
    }

    /// Shows the hidden lines of an unchanged run in place, keeping the scroll position.
    fn expand_gap(&mut self, segment: usize, cx: &mut Context<Self>) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let Some(at) = self
            .rows
            .iter()
            .position(|r| matches!(r, Row::Gap { segment: s, .. } if *s == segment))
        else {
            return;
        };
        self.expanded.insert(segment);
        let rows = self.with_comments(rows::layout(&data, &self.view_options(), &self.expanded));
        let inserted = rows.len() + 1 - self.rows.len();
        self.rows = rows;
        self.diff_list.splice(at..at + 1, inserted);
        self.recompute_matches();
        cx.notify();
    }

    fn option(&self, opt: Opt) -> bool {
        match opt {
            Opt::Unified => self.options.unified,
            Opt::Wrap => self.options.wrap,
            Opt::FullContext => self.options.full_context,
            Opt::Whitespace => self.options.ignore_whitespace,
        }
    }

    fn set_option(&mut self, opt: Opt, value: bool, cx: &mut Context<Self>) {
        if self.option(opt) != value {
            self.set_option_unsaved(opt, value, cx);
            self.options.save();
        }
    }

    fn set_option_unsaved(&mut self, opt: Opt, value: bool, cx: &mut Context<Self>) {
        if self.option(opt) == value {
            return;
        }
        match opt {
            Opt::Unified => self.options.unified = value,
            Opt::Wrap => self.options.wrap = value,
            Opt::FullContext => self.options.full_context = value,
            Opt::Whitespace => self.options.ignore_whitespace = value,
        }
        cx.set_menus(menus(&self.options));
        match opt {
            Opt::Unified | Opt::FullContext => self.relayout(),
            // Same rows, new heights.
            Opt::Wrap => self.diff_list.remeasure(),
            Opt::Whitespace => self.reload_diff(cx),
        }
        cx.notify();
    }

    /// Changes what a diff marks inside changed lines. Not remembered: Settings holds the default.
    fn set_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        if self.options.mode == mode {
            return;
        }
        self.options.mode = mode;
        cx.set_menus(menus(&self.options));
        self.select_file(self.file, cx);
        cx.notify();
    }

    /// The View menu: the diff mode, then the options that are on or off.
    fn view_menu(&self) -> Vec<Vec<Entry>> {
        let o = self.options;
        vec![
            DiffMode::ALL
                .iter()
                .map(|&mode| {
                    Entry::new(format!("{} diff", mode.label()), Act::SetMode(mode))
                        .checked(o.mode == mode)
                })
                .collect(),
            vec![
                Entry::new("Annotate — who wrote each line", Act::Annotate)
                    .key("⌥⌘B")
                    .checked(self.annotate),
                Entry::new("Wrap long lines", Act::Toggle(Opt::Wrap))
                    .key("⌥Z")
                    .checked(o.wrap),
                Entry::new("Show all lines", Act::Toggle(Opt::FullContext))
                    .key("⌥E")
                    .checked(o.full_context),
                Entry::new("Hide whitespace changes", Act::Toggle(Opt::Whitespace))
                    .key("⌥W")
                    .checked(o.ignore_whitespace),
            ],
        ]
    }

    // ---- selecting and context menus ---------------------------------------------------

    /// The new-side line shown at `row`, to open the editor where the reader is.
    fn new_line_of_row(&self, row: usize) -> Option<u32> {
        match *self.rows.get(row)? {
            rows::Row::Split { right, .. } => right.map(|c| c.line),
            rows::Row::Unified { new_line, .. } => new_line,
            _ => None,
        }
    }

    /// A right click on a diff line: copy it (or the selection), comment on it, open it.
    fn line_context(
        &mut self,
        old: bool,
        line: u32,
        at: Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(data) = &self.data else {
            return;
        };
        let in_selection = self.sel.is_some_and(|s| {
            let (start, end) = s.ends();
            !s.is_empty() && s.old == old && (start.line..=end.line).contains(&line)
        });
        let copy = match self.selected_text() {
            Some(text) if in_selection => Entry::new("Copy selection", Act::Copy(text)).key("⌘C"),
            _ => Entry::new(
                "Copy line",
                Act::Copy(data.side(old).line(line).0.to_owned()),
            ),
        };
        let mut second = vec![Entry::new(
            "Comment on this line",
            Act::Comment { old, line },
        )];
        if let (false, Some(path), Some(editor)) = (
            old,
            self.current_path(),
            crate::editor::pick(self.settings.editor),
        ) {
            second.push(Entry::new(
                format!("Open in {} at line {line}", editor.label()),
                Act::OpenEditor { path, line },
            ));
        }
        // What a reviewer pastes into a chat: a link to the line, its place, the command.
        let mut share = Vec::new();
        if let (Some(path), Some(rev)) = (self.current_path(), self.viewed_rev()) {
            if let (false, Some(web)) = (old, &self.web) {
                share.push(
                    Entry::new(
                        format!("Copy link to line · {}", web.name()),
                        Act::Copy(web.blob(&rev.to_string(), &path, line)),
                    )
                    .key("↗"),
                );
            }
            share.push(Entry::new(
                "Copy path:line",
                Act::Copy(format!("{path}:{line}")),
            ));
            if matches!(self.selection, Selection::Commit(_)) {
                share.push(Entry::new(
                    format!("Copy git show {}", format::short(rev)),
                    Act::Copy(format!("git show {}", format::short(rev))),
                ));
            }
        }
        self.open_menu(at, vec![vec![copy], second, share], cx);
    }

    /// The revision whose files the open diff shows on its new side; none for the working tree.
    pub(super) fn viewed_rev(&self) -> Option<git2::Oid> {
        match &self.diff.as_ref()?.header {
            Header::Commit(c) => Some(c.id),
            Header::Compare { head, .. } => Some(head.1),
            Header::Versions { to, .. } => Some(to.tip),
            Header::Interdiff { new, .. } => Some(new.id),
            Header::WorkingTree { .. } => None,
        }
    }

    fn commit_context(&mut self, ix: usize, at: Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let Some(commit) = self.commits.get(ix) else {
            return;
        };
        let mut groups = vec![vec![
            Entry::new("Copy SHA", Act::Copy(commit.id.to_string())),
            Entry::new("Copy message", Act::Copy(commit.message.clone())),
        ]];
        if let Some(web) = &self.web {
            groups.push(vec![Entry::new(
                format!("Open in {} ↗", web.name()),
                Act::OpenUrl(web.commit(&commit.id.to_string())),
            )]);
        }
        self.open_menu(at, groups, cx);
    }

    fn file_context(&mut self, ix: usize, at: Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let Some(path) = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(ix))
            .map(|f| f.path().to_owned())
        else {
            return;
        };
        let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        let mut groups = vec![vec![
            Entry::new("Copy path", Act::Copy(path.clone())),
            Entry::new("Copy file name", Act::Copy(name)),
        ]];
        if let Some(editor) = crate::editor::pick(self.settings.editor) {
            groups.push(vec![Entry::new(
                format!("Open in {}", editor.label()),
                Act::OpenEditor { path, line: 1 },
            )]);
        }
        self.open_menu(at, groups, cx);
    }

    fn branch_context(&mut self, ix: usize, at: Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let Some(branch) = self.branches.get(ix) else {
            return;
        };
        let mut groups = vec![vec![Entry::new(
            "Copy branch name",
            Act::Copy(branch.name.clone()),
        )]];
        if self.requests.available && branch.name != "main" && branch.name != "master" {
            groups.push(vec![
                Entry::new("Create merge request…", Act::CreateRequest(ix)).key("⌥⌘M"),
            ]);
        }
        if let Some(web) = &self.web {
            groups.push(vec![Entry::new(
                format!("Open in {} ↗", web.name()),
                Act::OpenUrl(web.branch(&branch.name)),
            )]);
        }
        self.open_menu(at, groups, cx);
    }

    fn toggle(&mut self, opt: Opt, cx: &mut Context<Self>) {
        self.set_option(opt, !self.option(opt), cx);
    }

    /// Recomputes the selected commit's or versions' diff, staying on the same file.
    fn reload_diff(&mut self, cx: &mut Context<Self>) {
        let keep = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(self.file))
            .map(|f| f.path().to_owned());
        match self.selection {
            Selection::Commit(ix) => self.load_commit(ix, keep, cx),
            Selection::Versions { from, to } => self.load_versions(from, to, keep, cx),
            Selection::WorkingTree => self.load_working_tree(keep, cx),
            Selection::None => {}
        }
    }

    /// The options the rows are laid out with: a diff under 700 px wide is unified.
    fn view_options(&self) -> ViewOptions {
        let mut options = self.options;
        options.unified |= self.narrow;
        options
    }

    /// A window under 1 100 px has no room for the files column: it opens over the diff instead.
    fn compact(&self) -> bool {
        self.win_width < 1100.
    }

    /// The files column is drawn: there is more than one file to list and room for the column.
    fn files_shown(&self) -> bool {
        self.layout.show_files
            && !self.compact()
            && self.diff.as_ref().is_some_and(|d| d.files.len() > 1)
    }

    /// The files exist as a zone (a column, or the popover of a narrow window).
    pub(super) fn files_zone(&self) -> bool {
        self.layout.show_files && self.diff.as_ref().is_some_and(|d| d.files.len() > 1)
    }

    /// As wide as the longest path needs, within 200 px and the width the column was dragged to.
    pub(super) fn files_width(&self) -> f32 {
        let longest = self
            .diff
            .as_ref()
            .and_then(|d| d.files.iter().map(|f| f.path().chars().count()).max())
            .unwrap_or(0);
        (longest as f32 * 7.4 + 96.).clamp(200., self.layout.files.max(200.))
    }

    /// A generated file with a long change starts folded; the reader can open it.
    pub(super) fn generated_folded(&self, file: &FileDiff) -> bool {
        self.settings.fold_generated
            && crate::generated::is_generated(file.path())
            && file.added + file.removed > crate::generated::FOLD_ABOVE
            && !self.unfolded.contains(file.path())
    }

    fn unfold(&mut self, path: &str, cx: &mut Context<Self>) {
        self.unfolded.insert(path.to_owned());
        cx.notify();
    }

    fn row_style(&self) -> RowStyle<'_> {
        let digits = self.data.as_ref().map_or(3, |d| d.gutter_digits);
        RowStyle {
            wrap: self.options.wrap,
            offset: self.offset,
            marks: self.settings.mark_style,
            terms: &self.dterms,
            strong: false,
            sel: self.sel,
            notes: &self.notes,
            annot: self.annotate.then_some(&self.annotations[..]),
            // Room for the comment mark (16 px) left of the number.
            gutter: digits.max(3) as f32 * self.char_width + 30.,
            whole: self
                .diff
                .as_ref()
                .and_then(|d| d.files.get(self.file))
                .is_some_and(|f| {
                    matches!(
                        f.change,
                        crate::git::ChangeKind::Added | crate::git::ChangeKind::Deleted
                    )
                }),
        }
    }

    /// How far the longest line can scroll before its end reaches the right edge.
    fn max_offset(&self) -> f32 {
        let Some(data) = &self.data else {
            return 0.;
        };
        let style = self.row_style();
        let sides = if self.view_options().unified { 1. } else { 2. };
        let visible = (self.body_width.get() - style.chrome(self.view_options().unified)) / sides;
        (data.widest as f32 * self.char_width + 24. - visible).max(0.)
    }

    fn scroll_horizontally(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.options.wrap {
            return;
        }
        let delta = f32::from(event.delta.pixel_delta(px(DIFF_ROW)).x);
        if delta == 0. {
            return;
        }
        let offset = (self.offset - delta).clamp(0., self.max_offset());
        if offset != self.offset {
            self.offset = offset;
            cx.notify();
        }
    }

    // ---- actions -------------------------------------------------------------------------

    fn open(&mut self, _: &Open, _: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                this.update(cx, |this, cx| this.open_repo(path, None, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let branch = self.branch.and_then(|ix| self.branches.get(ix));
        let commit = match self.selection {
            Selection::Commit(ix) => self.commits.get(ix).map(|c| c.id),
            _ => None,
        };
        let keep = branch.map(|b| (b.refname.clone(), commit));
        self.watch.forget();
        self.open_repo(root, keep, cx);
    }

    fn previous_commit(&mut self, _: &PreviousCommit, _: &mut Window, cx: &mut Context<Self>) {
        self.zone_vertical(false, cx);
    }

    fn next_commit(&mut self, _: &NextCommit, _: &mut Window, cx: &mut Context<Self>) {
        self.zone_vertical(true, cx);
    }

    fn previous_file(&mut self, _: &PreviousFile, _: &mut Window, cx: &mut Context<Self>) {
        self.zone_horizontal(false, cx);
    }

    fn next_file(&mut self, _: &NextFile, _: &mut Window, cx: &mut Context<Self>) {
        self.zone_horizontal(true, cx);
    }

    fn filter_files_action(&mut self, _: &FilterFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.zone = zones::Zone::Files;
        self.start_field(Field::Files, cx);
    }

    fn go_back_action(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        self.go_back(cx);
    }

    fn go_forward_action(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        self.go_forward(cx);
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.layout.show_sidebar = !self.layout.show_sidebar;
        self.layout.save();
        cx.notify();
    }

    fn toggle_files(&mut self, _: &ToggleFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.layout.show_files = !self.layout.show_files;
        self.layout.save();
        cx.notify();
    }

    /// ⌥⌘3 or the rail's third button: the sidebar's top island shows the merge requests; again,
    /// the branches.
    fn toggle_requests(&mut self, _: &ToggleRequests, _: &mut Window, cx: &mut Context<Self>) {
        if !self.requests.available && !self.requests.connectable {
            return;
        }
        if self.picker_open && self.requests.side == requests::Side::Requests {
            return self.close_picker(cx);
        }
        self.open_picker(true, cx);
    }

    /// The picker under the title bar: branches, or (`requests`) merge requests. The filter, or
    /// the token field of the connect panel, takes the keyboard at once.
    pub(super) fn open_picker(&mut self, requests: bool, cx: &mut Context<Self>) {
        let side = if requests && (self.requests.available || self.requests.connectable) {
            requests::Side::Requests
        } else {
            requests::Side::Branches
        };
        self.requests.side = side;
        self.picker_open = true;
        self.zone = zones::Zone::Branches;
        let field = if side == requests::Side::Requests && self.requests.connectable {
            lists::Field::Token
        } else {
            lists::Field::Branches
        };
        self.start_field(field, cx);
    }

    pub(super) fn close_picker(&mut self, cx: &mut Context<Self>) {
        if !self.picker_open {
            return;
        }
        self.picker_open = false;
        self.field = None;
        self.bfilter.clear();
        self.zone = zones::Zone::Commits;
        cx.notify();
    }

    fn render_picker(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.picker_open {
            return None;
        }
        Some(
            div()
                .id("picker-backdrop")
                .absolute()
                .size_full()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_picker(cx)),
                )
                .child(
                    island()
                        .occlude()
                        .absolute()
                        .left(px(56.))
                        .top(px(44.))
                        .w(px(400.))
                        .h(px(self.picker_height()))
                        .border_2()
                        .border_color(theme::focus())
                        .shadow_lg()
                        .child(self.render_branches_header())
                        .child(
                            if self.requests.side == requests::Side::Requests
                                && self.requests.connectable
                            {
                                self.render_connect(cx).into_any_element()
                            } else if self.requests.side == requests::Side::Requests
                                && self.requests.available
                            {
                                self.render_requests(cx).into_any_element()
                            } else {
                                self.render_branches(cx).into_any_element()
                            },
                        ),
                ),
        )
    }

    /// ⌘.: the diff alone; pressed again, the islands come back.
    fn focus_diff(&mut self, _: &FocusDiff, _: &mut Window, cx: &mut Context<Self>) {
        let hide = self.layout.show_sidebar || self.layout.show_files;
        self.layout.show_sidebar = !hide;
        self.layout.show_files = !hide;
        self.layout.save();
        cx.notify();
    }

    fn toggle_unified(&mut self, _: &ToggleUnified, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle(Opt::Unified, cx);
    }

    fn cycle_mode(&mut self, _: &CycleMode, _: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(self.options.mode.next(), cx);
    }

    fn set_lines(&mut self, _: &SetLines, _: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(DiffMode::Lines, cx);
    }

    fn set_words(&mut self, _: &SetWords, _: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(DiffMode::Words, cx);
    }

    fn set_structural(&mut self, _: &SetStructural, _: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(DiffMode::Structural, cx);
    }

    fn open_settings_action(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.open_settings(cx);
    }

    fn open_palette_action(&mut self, _: &OpenPalette, _: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(cx);
        } else {
            self.open_palette(palette::Kind::Jump, cx);
        }
    }

    fn compare_action(&mut self, _: &Compare, _: &mut Window, cx: &mut Context<Self>) {
        if self.root.is_some() {
            self.open_palette(palette::Kind::Base, cx);
        }
    }

    fn palette_up(&mut self, _: &PaletteUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.ffind.is_some() {
            return self.ffind_step(false, cx);
        }
        self.palette_step(false, cx);
    }

    fn palette_down(&mut self, _: &PaletteDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.ffind.is_some() {
            return self.ffind_step(true, cx);
        }
        self.palette_step(true, cx);
    }

    fn next_match(&mut self, _: &NextMatch, _: &mut Window, cx: &mut Context<Self>) {
        self.step_match(true, cx);
    }

    fn previous_match(&mut self, _: &PreviousMatch, _: &mut Window, cx: &mut Context<Self>) {
        self.step_match(false, cx);
    }

    /// ⌘F finds in the open diff; with none open it searches the commits.
    fn find_action(&mut self, _: &Find, _: &mut Window, cx: &mut Context<Self>) {
        if self.diff.is_some() && !self.settings_open {
            self.start_dfind(cx);
        } else {
            self.start_find(cx);
        }
    }

    fn find_commits_action(&mut self, _: &FindCommits, _: &mut Window, cx: &mut Context<Self>) {
        self.start_find(cx);
    }

    fn next_change(&mut self, _: &NextChange, _: &mut Window, cx: &mut Context<Self>) {
        self.step_change(true, cx);
    }

    fn previous_change(&mut self, _: &PreviousChange, _: &mut Window, cx: &mut Context<Self>) {
        self.step_change(false, cx);
    }

    fn toggle_wrap(&mut self, _: &ToggleWrap, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle(Opt::Wrap, cx);
    }

    fn toggle_full_context(
        &mut self,
        _: &ToggleFullContext,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle(Opt::FullContext, cx);
    }

    fn toggle_whitespace(&mut self, _: &ToggleWhitespace, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle(Opt::Whitespace, cx);
    }

    /// A click compares that version with the latest; ⌘-click makes it the newer side instead.
    fn click_version(&mut self, ix: usize, event: &ClickEvent, cx: &mut Context<Self>) {
        let latest = self.versions.len() - 1;
        let (from, to) = match (self.selection, event.modifiers().platform) {
            (Selection::Versions { from, .. }, true) if from != ix => (from.min(ix), from.max(ix)),
            _ if ix == latest => (latest - 1, latest),
            _ => (ix, latest),
        };
        self.select_versions(from, to, cx);
    }

    // ---- resizing ------------------------------------------------------------------------

    fn size(&mut self, split: Split) -> &mut f32 {
        match split {
            Split::Sidebar => &mut self.layout.sidebar,
            Split::Files => &mut self.layout.files,
        }
    }

    fn start_drag(&mut self, split: Split, event: &MouseDownEvent, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if event.click_count == 2 {
            *self.size(split) = match split {
                Split::Sidebar => Layout::default().sidebar,
                Split::Files => Layout::default().files,
            };
            self.layout.save();
            cx.notify();
            return;
        }
        let origin = axis(split, event.position);
        let size = *self.size(split);
        self.drag = Some(Drag {
            split,
            origin,
            size,
        });
    }

    fn drag_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            // The button was released outside the window.
            self.end_drag(cx);
            return;
        }
        let (min, max) = drag.split.bounds();
        let size = (drag.size + axis(drag.split, event.position) - drag.origin).clamp(min, max);
        *self.size(drag.split) = size;
        cx.notify();
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            self.layout.save();
            cx.notify();
        }
    }

    /// The gap after a pane (right of it, or below when vertical), which resizes the pane.
    fn handle(&self, split: Split, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let active = self.drag.is_some_and(|d| d.split == split);
        let group = match split {
            Split::Sidebar => "split-sidebar",
            Split::Files => "split-files",
        };
        let line = div()
            .rounded_full()
            .when(active, |s| s.bg(theme::splitter()))
            .group_hover(group, |s| s.bg(theme::splitter()));
        let gap = div()
            .group(group)
            .flex_none()
            .flex()
            .items_center()
            .justify_center();
        let gap = if split.vertical() {
            gap.h(px(GAP))
                .w_full()
                .cursor(CursorStyle::ResizeUpDown)
                .child(line.h(px(2.)).w(px(48.)))
        } else {
            gap.w(px(GAP))
                .h_full()
                .cursor(CursorStyle::ResizeLeftRight)
                .child(line.w(px(2.)).h(px(48.)))
        };
        gap.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event, _, cx| this.start_drag(split, event, cx)),
        )
    }

    // ---- rendering -----------------------------------------------------------------------

    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let repo = self.root.as_ref().and_then(|r| r.file_name()).map_or_else(
            || "GitLance".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let branch = self
            .branch
            .and_then(|ix| self.branches.get(ix))
            .map(|b| b.name.clone());
        div()
            .h(px(TITLE_BAR))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .pl(px(78.))
            .pr(px(GAP))
            .child(
                div()
                    .flex()
                    .gap(px(2.))
                    .child(
                        nav_button("go-back", "back", "Back  ⌘[", self.history.can_back())
                            .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
                    )
                    .child(
                        nav_button(
                            "go-forward",
                            "forward",
                            "Forward  ⌘]",
                            self.history.can_forward(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx))),
                    ),
            )
            .child(
                // The repository is a menu: open, refresh and the recent repositories.
                div()
                    .id("repo-menu")
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(28.))
                    .px_2()
                    .rounded(px(ROW_RADIUS))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .when(self.repo_menu, |s| s.bg(theme::hover()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(repo)
                    .child(icons::icon("chevron").text_color(theme::muted()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.repo_menu = !this.repo_menu;
                        cx.notify();
                    })),
            )
            .child(
                // The empty part of the bar: a double click zooms the window, as in other apps.
                div()
                    .id("title-bar")
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap_3()
                    .on_click(|event, window, _| {
                        if event.click_count() == 2 {
                            window.titlebar_double_click();
                        }
                    })
                    .children(branch.map(|b| {
                        div()
                            .id("branch-picker")
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .h(px(26.))
                            .rounded(px(ROW_RADIUS))
                            .text_color(theme::muted())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                            .tooltip(|_, cx| cx.new(|_| Tip("Branches  ⌃1")).into())
                            .child(icons::icon("branch").text_color(theme::muted()))
                            .child(b)
                            .child(icons::icon("chevron").text_color(theme::muted()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.picker_open
                                    && this.requests.side == requests::Side::Branches
                                {
                                    this.close_picker(cx)
                                } else {
                                    this.open_picker(false, cx)
                                }
                            }))
                    }))
                    .children(self.render_request_title(cx)),
            )
            .children(self.render_fetch(cx))
            .children(self.render_status(cx))
            .child(
                icon_button("settings-open", "settings", "Settings  ⌘,")
                    .on_click(cx.listener(|this, _, _, cx| this.open_settings(cx))),
            )
    }

    /// The menu under the repository name, over a backdrop that closes it.
    fn render_repo_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let item =
            |id: &'static str, icon: &'static str, label: &'static str, key: &'static str| {
                row(id, false)
                    .h(px(30.))
                    .gap_2()
                    .child(icons::icon(icon).text_color(theme::muted()))
                    .child(div().flex_1().child(label))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(key),
                    )
            };
        div()
            .absolute()
            .size_full()
            .child(
                div()
                    .id("repo-menu-backdrop")
                    .absolute()
                    .size_full()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.repo_menu = false;
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .occlude()
                    .top(px(TITLE_BAR - 4.))
                    .left(px(78.))
                    .w(px(340.))
                    .p(px(6.))
                    .flex()
                    .flex_col()
                    .rounded(px(ISLAND_RADIUS))
                    .border_1()
                    .border_color(theme::island_border())
                    .bg(theme::panel())
                    .shadow_lg()
                    .child(
                        item("menu-open", "open", "Open repository…", "⌘O").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.repo_menu = false;
                                this.open(&Open, window, cx);
                            },
                        )),
                    )
                    .when(self.root.is_some(), |s| {
                        s.child(item("menu-refresh", "refresh", "Refresh", "⌘R").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.repo_menu = false;
                                this.refresh(&Refresh, window, cx);
                            }),
                        ))
                    })
                    .children((!self.recent.is_empty()).then(|| {
                        div()
                            .flex()
                            .flex_col()
                            .child(div().h(px(1.)).my(px(4.)).bg(theme::island_border()))
                            .child(island_label("Recent"))
                            .children(self.recent.iter().enumerate().map(|(ix, path)| {
                                let target = path.clone();
                                let name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                let parent =
                                    path.parent().map(format::home_relative).unwrap_or_default();
                                row(("menu-recent", ix), self.root.as_ref() == Some(path))
                                    .h(px(30.))
                                    .gap_2()
                                    .child(div().child(name))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(11.))
                                            .text_color(theme::faint())
                                            .child(parent),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.repo_menu = false;
                                        this.open_repo(target.clone(), None, cx);
                                    }))
                            }))
                    })),
            )
    }

    /// The rail on the window's left edge: one button per hideable island (⌥⌘1, ⌥⌘2).
    fn render_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let button = |id: &'static str, icon: &'static str, on: bool, tip: &'static str| {
            div()
                .id(id)
                .size(px(34.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(ROW_RADIUS + 2.))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .when(on, |s| s.bg(theme::hover()))
                .tooltip(move |_, cx| cx.new(|_| Tip(tip)).into())
                .child(icons::icon(icon).size(px(22.)).text_color(if on {
                    theme::accent()
                } else {
                    theme::muted()
                }))
        };
        div()
            .w(px(42.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .pt(px(4.))
            .child(
                button(
                    "rail-sidebar",
                    "sidebar",
                    self.layout.show_sidebar,
                    "Branches and commits  ⌥⌘1",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_sidebar(&ToggleSidebar, window, cx)
                })),
            )
            .child(
                button("rail-files", "files", self.layout.show_files, "Files  ⌥⌘2").on_click(
                    cx.listener(|this, _, window, cx| this.toggle_files(&ToggleFiles, window, cx)),
                ),
            )
            .children(
                (self.requests.available || self.requests.connectable).then(|| {
                    let on =
                        self.layout.show_sidebar && self.requests.side == requests::Side::Requests;
                    let n = self.requests.list.len();
                    div()
                        .relative()
                        .child(
                            button("rail-requests", "pull-request", on, "Merge requests  ⌥⌘3")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_requests(&ToggleRequests, window, cx)
                                })),
                        )
                        .children((n > 0 && self.requests.available).then(|| {
                            div()
                                .absolute()
                                .top(px(-2.))
                                .right(px(-2.))
                                .min_w(px(16.))
                                .h(px(16.))
                                .px(px(4.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(theme::accent())
                                .text_color(theme::editor())
                                .text_size(px(10.))
                                .font_weight(FontWeight::BOLD)
                                .child(n.to_string())
                        }))
                }),
            )
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .m(px(GAP))
            .mt_0()
            .rounded(px(ISLAND_RADIUS))
            .bg(theme::editor())
            .child(
                div()
                    .text_size(px(22.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("GitLance"),
            )
            .child(
                div()
                    .text_color(theme::muted())
                    .child("Open a repository to browse its commits and branch versions — or drop a folder anywhere in this window."),
            )
            .child(
                button("welcome-open", "Open Repository…", "⌘O")
                    .on_click(cx.listener(|this, _, window, cx| this.open(&Open, window, cx))),
            )
            .children((!self.recent.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .mt_4()
                    .child(section_label("Recent"))
                    .children(self.recent.iter().enumerate().map(|(ix, path)| {
                        let path = path.clone();
                        div()
                            .id(("recent", ix))
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .child(path.display().to_string())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_repo(path.clone(), None, cx)
                            }))
                    }))
            }))
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.layout.sidebar))
            .flex_none()
            .flex()
            .flex_col()
            .children((self.versions.len() > 1).then(|| {
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(self.render_versions(cx))
                    .child(div().h(px(GAP)))
            }))
            .child(
                island()
                    .flex_1()
                    .min_h(px(300.))
                    .border_2()
                    .border_color(self.zone_border(zones::Zone::Commits))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.set_zone(zones::Zone::Commits, cx)),
                    )
                    .child({
                        // A narrow sidebar has no room for the label and the chips on one line.
                        let stacked = self.layout.sidebar < 300.
                            && (self.open_request.is_some()
                                || self.wt.count > 0
                                || self.selection == Selection::WorkingTree);
                        let chips = div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .when(stacked, |s| s.px(px(8.)).pb(px(4.)))
                            .when(!stacked, |s| s.pr(px(8.)))
                            .children(self.render_request_chip(cx))
                            .children(self.render_working_chip(cx))
                            .child(
                                commit_chip("commit-search", self.find.text.is_some())
                                    .child("⌕")
                                    .tooltip(|_, cx| cx.new(|_| Tip("Search commits  ⌘⇧F")).into())
                                    .on_click(cx.listener(|this, _, _, cx| this.start_find(cx))),
                            );
                        let label = island_label(format!(
                            "Commits · {}{}",
                            self.commits.len(),
                            self.zone_tag(zones::Zone::Commits)
                        ))
                        .flex_none();
                        if stacked {
                            div().flex().flex_col().child(label).child(chips)
                        } else {
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(label)
                                .child(div().flex_1())
                                .child(chips)
                        }
                    })
                    .children(self.find.text.is_some().then(|| self.render_find(cx)))
                    .children(self.render_pill(cx))
                    .child(if self.find.shown.as_ref().is_some_and(Vec::is_empty) {
                        self.render_no_match().into_any_element()
                    } else {
                        uniform_list(
                            "commits",
                            self.commit_rows(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|row| this.render_commit_row(this.commit_at(row), cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.commit_scroll)
                        .flex_1()
                        .pb(px(6.))
                        .into_any_element()
                    }),
            )
    }

    /// The height of the picker: what its rows need, up to 560 px.
    fn picker_height(&self) -> f32 {
        let content = if self.requests.connectable && self.requests.side == requests::Side::Requests
        {
            300.
        } else if self.requests.available && self.requests.side == requests::Side::Requests {
            92. + 56. * self.requests.list.len() as f32 + 34.
        } else {
            // Label and filter, a header and a row each: roughly what `branch_items` draws.
            92. + 34. * self.branches.len() as f32 + 28. * 2.
        };
        content.clamp(150., 560.)
    }

    fn render_versions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let latest = self.versions.len() - 1;
        let (from, to) = match self.selection {
            Selection::Versions { from, to } => (Some(from), Some(to)),
            _ => (None, None),
        };
        // Who made a version is only worth a line when more than one person did.
        let several = self
            .versions
            .iter()
            .any(|v| v.author != self.versions[0].author);
        let folded = self.layout.fold_versions;
        island()
            .pb(px(6.))
            .child(
                island_label(format!(
                    "{} Versions · {}",
                    if folded { "▸" } else { "▾" },
                    self.versions.len()
                ))
                .id("fold-versions")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.layout.fold_versions = !this.layout.fold_versions;
                    this.layout.save();
                    cx.notify();
                })),
            )
            .children(
                self.versions
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|_| !folded)
                    .map(|(ix, v)| {
                        let role = if Some(ix) == from {
                            Some("from")
                        } else if Some(ix) == to {
                            Some("to")
                        } else {
                            None
                        };
                        let chosen = role.is_some();
                        let kind = match format::reason(&v.reason) {
                            "fetch" => "fetched",
                            "unknown" => "version",
                            k => k,
                        };
                        let mut detail: Vec<String> = Vec::new();
                        if several {
                            detail.push(v.author.clone());
                        }
                        detail.push(format::ago(v.time));
                        match ix.checked_sub(1).and_then(|p| self.versions.get(p)) {
                            None => detail.push(plural(v.commits, "commit")),
                            Some(prev) => {
                                if prev.base != v.base {
                                    detail.push(format!(
                                        "onto {}",
                                        v.base.map(format::short).unwrap_or_default()
                                    ));
                                }
                                detail.push(match v.commits.cmp(&prev.commits) {
                                    std::cmp::Ordering::Greater => {
                                        format!("+{}", plural(v.commits - prev.commits, "commit"))
                                    }
                                    std::cmp::Ordering::Less => {
                                        format!("−{}", plural(prev.commits - v.commits, "commit"))
                                    }
                                    std::cmp::Ordering::Equal => {
                                        format!("same {}", plural(v.commits, "commit"))
                                    }
                                });
                            }
                        }
                        // The rail: a dot per version and a line to the older one.
                        let rail = div()
                            .flex_none()
                            .relative()
                            .w(px(14.))
                            .h(px(44.))
                            .when(ix > 0, |s| {
                                s.child(
                                    div()
                                        .absolute()
                                        .left(px(6.5))
                                        .top(px(18.))
                                        .w(px(1.))
                                        .h(px(30.))
                                        .bg(theme::selected()),
                                )
                            })
                            .child(
                                div()
                                    .absolute()
                                    .left(px(3.))
                                    .top(px(13.))
                                    .size(px(8.))
                                    .rounded_full()
                                    .bg(if chosen {
                                        theme::accent()
                                    } else {
                                        theme::faint()
                                    }),
                            );
                        let item = row(("version", ix), chosen)
                            .h(px(44.))
                            .items_start()
                            .gap_2()
                            .child(rail)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .justify_center()
                                    .h_full()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(theme::accent())
                                                    .child(format!("v{}", v.number)),
                                            )
                                            .child(kind.to_owned())
                                            .child(
                                                div()
                                                    .font_family(theme::CODE_FONT)
                                                    .text_size(px(11.))
                                                    .text_color(theme::muted())
                                                    .child(format::short(v.tip)),
                                            )
                                            .child(div().flex_1())
                                            .children(role.map(|r| tag(r, theme::accent())))
                                            .when(ix == latest && role.is_none(), |s| {
                                                s.child(tag("latest", theme::faint()))
                                            })
                                            .when(
                                                self.watch.new_from.is_some_and(|from| ix >= from),
                                                |s| s.child(tag("new", theme::accent())),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .truncate()
                                            .text_size(px(11.))
                                            .text_color(theme::muted())
                                            .child(detail.join(" · ")),
                                    ),
                            )
                            .on_click(cx.listener(move |this, event, _, cx| {
                                this.click_version(ix, event, cx)
                            }));
                        div().px(px(6.)).py(px(1.)).child(item)
                    }),
            )
    }

    /// The commits of two versions, paired; a modified pair opens its interdiff.
    fn render_pairs(&self, diff: &Diff, cx: &mut Context<Self>) -> impl IntoElement {
        island()
            .flex_1()
            .min_w_0()
            .bg(theme::editor())
            .child(island_label(
                "Commits · = unchanged  ! modified  + new  − dropped",
            ))
            .child(
                div()
                    .id("pairs")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(px(6.))
                    .pb(px(6.))
                    .children(diff.pairs.iter().enumerate().map(|(ix, p)| {
                        let (mark, color, label) = match p.kind {
                            PairKind::Unchanged => ("=", theme::muted(), "unchanged"),
                            PairKind::Modified => ("!", theme::warning(), "modified"),
                            PairKind::New => ("+", theme::added(), "new"),
                            PairKind::Dropped => ("−", theme::removed(), "dropped"),
                        };
                        let ids = match (&p.old, &p.new) {
                            (Some(o), Some(n)) => {
                                format!("{} → {}", format::short(o.id), format::short(n.id))
                            }
                            (Some(o), None) => format::short(o.id),
                            (None, Some(n)) => {
                                format!("{}   → {}", " ".repeat(7), format::short(n.id))
                            }
                            (None, None) => String::new(),
                        };
                        let summary = p
                            .new
                            .as_ref()
                            .or(p.old.as_ref())
                            .map(|c| c.summary.clone())
                            .unwrap_or_default();
                        let moved = match p.moved {
                            0 => String::new(),
                            n if n > 0 => format!("moved ↑{n}"),
                            n => format!("moved ↓{}", -n),
                        };
                        let open = p.kind == PairKind::Modified;
                        let item =
                            row(("pair", ix), false)
                                .h(px(32.))
                                .gap_3()
                                .when(!open, |s| s.cursor_default())
                                .child(
                                    div()
                                        .w(px(14.))
                                        .font_family(theme::CODE_FONT)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(color)
                                        .child(mark),
                                )
                                .child(
                                    div()
                                        .w(px(150.))
                                        .flex_none()
                                        .font_family(theme::CODE_FONT)
                                        .text_size(px(11.))
                                        .text_color(theme::muted())
                                        .child(ids),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .when(p.kind == PairKind::Dropped, |s| {
                                            s.line_through().text_color(theme::muted())
                                        })
                                        .child(summary),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(px(12.))
                                        .text_color(theme::muted())
                                        .child(if moved.is_empty() {
                                            label.to_owned()
                                        } else {
                                            format!("{label} · {moved}")
                                        }),
                                )
                                .children(open.then(|| {
                                    div()
                                        .flex_none()
                                        .text_color(theme::accent())
                                        .child("interdiff →")
                                }))
                                .when(open, |s| {
                                    s.on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_interdiff(ix, cx)
                                    }))
                                });
                        div().w_full().py(px(1.)).child(item)
                    })),
            )
    }

    fn render_commit_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let commit = &self.commits[ix];
        let selected = self.selection == Selection::Commit(ix);
        // The author is told only when more than one person made the commits in view.
        let several = self
            .commits
            .iter()
            .any(|c| c.author != self.commits[0].author);
        let item = row(("commit", ix), selected)
            .h(px(62.))
            .flex_col()
            .items_start()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .line_height(px(17.))
                    .overflow_hidden()
                    .line_clamp(2)
                    .child(self.summary_text(&commit.summary.clone().into())),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap_2()
                    .text_size(px(11.))
                    .text_color(theme::muted())
                    .child(
                        div()
                            .font_family(theme::CODE_FONT)
                            .child(format::short(commit.id)),
                    )
                    .child(
                        div().flex().min_w_0().flex_shrink(1.).gap_2().children(
                            self.decor
                                .get(&commit.id)
                                .map(|d| {
                                    lists::deco_tags(d).into_iter().take(1).collect::<Vec<_>>()
                                })
                                .unwrap_or_default(),
                        ),
                    )
                    .children((several && !self.decor.contains_key(&commit.id)).then(|| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(commit.author.clone())
                    }))
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .child(format::ago(commit.time)),
                    ),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.commit_context(ix, event.position, cx)
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.select_commit(ix, cx)));
        div().w_full().h(px(64.)).px(px(6.)).py(px(1.)).child(item)
    }

    fn render_diff(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(diff) = &self.diff else {
            let message = if self.diff_task.is_some() {
                "Loading…"
            } else {
                "Select a commit or a version."
            };
            return island()
                .flex_1()
                .items_center()
                .justify_center()
                .bg(theme::editor())
                .text_color(theme::muted())
                .child(message)
                .into_any_element();
        };
        let commits_tab = matches!(diff.header, Header::Versions { .. })
            && self.version_tab == VersionTab::Commits;
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(self.render_header(diff, cx))
            .child(div().h(px(GAP)).flex_none())
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .when(!commits_tab, |s| {
                        s.children(self.files_shown().then(|| {
                            div()
                                .flex()
                                .flex_none()
                                .child(self.render_files(diff, cx))
                                .child(self.handle(Split::Files, cx))
                        }))
                        .child(self.render_file(diff, cx))
                        .children(
                            (self.compact() && self.files_popover && self.files_zone()).then(
                                || {
                                    div()
                                        .absolute()
                                        .left(px(0.))
                                        .top(px(0.))
                                        .bottom(px(0.))
                                        .flex()
                                        .shadow_lg()
                                        .child(self.render_files(diff, cx))
                                },
                            ),
                        )
                    })
                    .when(commits_tab, |s| s.child(self.render_pairs(diff, cx)))
                    .children(self.render_comments_drawer(cx)),
            )
            .into_any_element()
    }

    fn render_header(&self, diff: &Diff, cx: &mut Context<Self>) -> impl IntoElement {
        island()
            .flex_none()
            .flex_row()
            .items_start()
            .gap_4()
            .px_4()
            .py(px(8.))
            .child(self.render_summary(diff, cx))
            .child(self.render_toolbar(cx))
    }

    /// The diff options, as chips; the same toggles live in the View menu.
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let set = |opt: Opt, value: bool| {
            cx.listener(move |this: &mut Self, _: &ClickEvent, _: &mut Window, cx| {
                this.set_option(opt, value, cx)
            })
        };
        let o = self.options;
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .child(
                diff_view::group()
                    .child(chip("split", "Split", !o.unified).on_click(set(Opt::Unified, false)))
                    .child(chip("unified", "Unified", o.unified).on_click(set(Opt::Unified, true))),
            )
            .child(
                diff_view::group().child(
                    chip(
                        "comments-chip",
                        match self.comments_total() {
                            0 => "Comments".to_owned(),
                            n => format!("Comments · {n}"),
                        },
                        self.layout.comments_open,
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.set_comments_open(!this.layout.comments_open, cx)
                    })),
                ),
            )
            .child(
                diff_view::group().child(
                    chip("view", "View ▾", self.ctx_menu.is_some())
                        .relative()
                        .child({
                            let bounds = self.view_bounds.clone();
                            canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
                                .absolute()
                                .size_full()
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                // The menu hangs from the button, its right edge on the button's.
                                let Some(b) = this.view_bounds.get() else {
                                    return;
                                };
                                let at =
                                    Point::new(b.right() - px(menu::WIDTH), b.bottom() + px(6.));
                                let groups = this.view_menu();
                                this.open_menu(at, groups, cx);
                            }),
                        ),
                ),
            )
    }

    fn render_summary(&self, diff: &Diff, cx: &mut Context<Self>) -> impl IntoElement {
        let (added, removed) = diff
            .files
            .iter()
            .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
        let stats = div()
            .flex()
            .gap_2()
            .text_size(px(12.))
            .child(
                div()
                    .text_color(theme::muted())
                    .child(plural(diff.files.len(), "file")),
            )
            .child(div().text_color(theme::added()).child(format!("+{added}")))
            .child(
                div()
                    .text_color(theme::removed())
                    .child(format!("−{removed}")),
            );
        let header = div().flex_1().min_w_0().flex().flex_col().gap_1();
        match &diff.header {
            Header::Commit(commit) => {
                // The explanation under the title, without the trailers (Co-Authored-By …).
                let body = commit
                    .message
                    .split_once('\n')
                    .map(|(_, body)| {
                        body.lines()
                            .filter(|l| !is_trailer(l))
                            .collect::<Vec<_>>()
                            .join("\n")
                            .trim()
                            .to_owned()
                    })
                    .filter(|b| !b.is_empty());
                let (full, short) = (commit.id.to_string(), format::short(commit.id));
                header
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(commit.summary.clone()),
                    )
                    .children(body.clone().filter(|_| self.body_expanded).map(|b| {
                        div()
                            .id("commit-body")
                            .max_h(px(220.))
                            .overflow_y_scroll()
                            .text_color(theme::muted())
                            .child(b)
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_x(px(12.))
                            .gap_y(px(2.))
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(div().font_family(theme::CODE_FONT).child(short))
                                    .child(
                                        div()
                                            .id("copy-hash")
                                            .text_color(theme::accent())
                                            .cursor_pointer()
                                            .hover(|s| s.text_color(theme::text()))
                                            .child("copy")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.run_act(Act::Copy(full.clone()), cx)
                                            })),
                                    ),
                            )
                            .child(commit.author.clone())
                            .children(body.is_some().then(|| {
                                div()
                                    .id("body-toggle")
                                    .text_color(theme::accent())
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(theme::text()))
                                    .child(if self.body_expanded {
                                        "Message ▴"
                                    } else {
                                        "Message ▾"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.body_expanded = !this.body_expanded;
                                        cx.notify();
                                    }))
                            }))
                            .child(format::date(commit.time))
                            .child(stats)
                            .children(self.web.as_ref().map(|web| {
                                let url = web.commit(&commit.id.to_string());
                                div()
                                    .id("open-web")
                                    .text_color(theme::accent())
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(theme::text()))
                                    .child(format!("Open in {} ↗", web.name()))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.run_act(Act::OpenUrl(url.clone()), cx)
                                    }))
                            })),
                    )
            }
            Header::WorkingTree { only_unstaged } => {
                let only = *only_unstaged;
                header
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap_x(px(12.))
                            .gap_y(px(4.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Working tree against HEAD"),
                            )
                            .child(
                                diff_view::group()
                                    .child(chip("wt-all", "All changes", !only).on_click(
                                        cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.set_only_unstaged(false, cx)
                                        }),
                                    ))
                                    .child(chip("wt-unstaged", "Not staged only", only).on_click(
                                        cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.set_only_unstaged(true, cx)
                                        }),
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child("Read-only: staging and committing happen elsewhere")
                            .child(stats),
                    )
            }
            Header::Compare {
                base,
                head,
                since_merge_base,
                start,
                commits,
                request,
            } => {
                let (b, h, since, req) = (base.clone(), head.clone(), *since_merge_base, *request);
                let (b2, h2) = (b.clone(), h.clone());
                let (b3, h3) = (b.clone(), h.clone());
                header
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap_x(px(12.))
                            .gap_y(px(4.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("{} … {}", base.0, head.0)),
                            )
                            .child(
                                diff_view::group()
                                    .child(chip("cmp-since", "Since merge base", since).on_click(
                                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                                            this.run_compare(b.clone(), h.clone(), true, req, cx)
                                        }),
                                    ))
                                    .child(chip("cmp-direct", "Direct", !since).on_click(
                                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                                            this.run_compare(b2.clone(), h2.clone(), false, req, cx)
                                        }),
                                    )),
                            )
                            .child(chip("cmp-swap", "⇄ Swap", false).on_click(cx.listener(
                                move |this, _: &ClickEvent, _, cx| {
                                    this.run_compare(h3.clone(), b3.clone(), since, None, cx)
                                },
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(format!(
                                "{} · from {}",
                                plural(*commits, "commit"),
                                format::short(*start)
                            ))
                            .child(stats),
                    )
            }
            Header::Interdiff { from, to, old, new } => {
                let (from, to) = (*from, *to);
                header
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap_x(px(12.))
                            .gap_y(px(4.))
                            .child(chip("back-to-commits", "← Commits", false).on_click(
                                cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.select_versions(from, to, cx)
                                }),
                            ))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("Interdiff · {}", new.summary)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(format!(
                                "{} → {} · the old commit's patch against the new one's",
                                format::short(old.id),
                                format::short(new.id)
                            ))
                            .child(stats),
                    )
            }
            Header::Versions {
                from,
                to,
                rebased,
                conflicts,
            } => header
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("Changes from v{} to v{}", from.number, to.number)),
                        )
                        .child(
                            diff_view::group()
                                .child(
                                    chip(
                                        "tab-files",
                                        "Files",
                                        self.version_tab == VersionTab::Files,
                                    )
                                    .on_click(cx.listener(
                                        |this, _: &ClickEvent, _, cx| {
                                            this.version_tab = VersionTab::Files;
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(
                                    chip(
                                        "tab-commits",
                                        format!("Commits · {}", diff.pairs.len()),
                                        self.version_tab == VersionTab::Commits,
                                    )
                                    .on_click(cx.listener(
                                        |this, _: &ClickEvent, _, cx| {
                                            this.version_tab = VersionTab::Commits;
                                            cx.notify();
                                        },
                                    )),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(format!(
                            "v{} {} → v{} {}",
                            from.number,
                            format::short(from.tip),
                            to.number,
                            format::short(to.tip)
                        ))
                        .when(*rebased, |s| {
                            s.child(div().text_color(theme::warning()).child(format!(
                                "rebased onto {} — upstream changes hidden",
                                to.base.map(format::short).unwrap_or_default()
                            )))
                        })
                        .when(*conflicts > 0, |s| {
                            s.child(div().text_color(theme::removed()).child(format!(
                                "{} with rebase conflicts",
                                plural(*conflicts, "file")
                            )))
                        })
                        .child(stats),
                ),
        }
    }

    fn render_file(&self, diff: &Diff, cx: &mut Context<Self>) -> impl IntoElement {
        let file = diff.files.get(self.file);
        let body = match (file, &self.data) {
            (Some(f), _) if self.generated_folded(f) => {
                let path = f.path().to_owned();
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .text_color(theme::muted())
                    .child(format!(
                        "{} is generated: {} changed lines are folded.",
                        path.rsplit('/').next().unwrap_or(&path),
                        f.added + f.removed
                    ))
                    .child(
                        div()
                            .id("unfold-generated")
                            .px_3()
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .rounded(px(ROW_RADIUS))
                            .bg(theme::hover())
                            .text_color(theme::text())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::selected()))
                            .child("Show it anyway  ↵")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.unfold(&path, cx)
                            })),
                    )
                    .into_any_element()
            }
            (None, _) => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::muted())
                .child("No changes")
                .into_any_element(),
            (Some(file), _) if file.hunks.is_empty() => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::muted())
                .child(
                    file.note
                        .clone()
                        .unwrap_or_else(|| "No content changes".to_owned()),
                )
                .into_any_element(),
            (Some(_), None) => div().flex_1().into_any_element(),
            (Some(_), Some(_)) => self.render_rows(cx).into_any_element(),
        };
        let mode_note = self.data.as_ref().and_then(|d| d.mode_note.clone());
        island()
            .relative()
            .flex_1()
            .min_w_0()
            .bg(theme::editor())
            .border_2()
            .border_color(self.zone_border(zones::Zone::Diff))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.set_zone(zones::Zone::Diff, cx)),
            )
            .children(file.map(|f| {
                let path = match (&f.old_path, &f.new_path) {
                    (Some(old), Some(new)) if old != new => format!("{old} → {new}"),
                    _ => f.path().to_owned(),
                };
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(36.))
                    .px_4()
                    .border_b_1()
                    .border_color(theme::island_border())
                    .children((self.compact() && self.files_zone()).then(|| {
                        chip(
                            "files-popover",
                            format!("▾ {}", plural(diff.files.len(), "file")),
                            self.files_popover,
                        )
                        .on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                let open = !this.files_popover;
                                this.files_popover = open;
                                if open {
                                    this.zone = zones::Zone::Files;
                                }
                                cx.notify();
                            },
                        ))
                    }))
                    .child(change_badge(f.change))
                    .child(div().flex_1().min_w_0().truncate().child(path))
                    .children(self.render_review_chip(cx))
                    .children(
                        (!f.hunks.is_empty())
                            .then_some(f.note.clone())
                            .flatten()
                            .map(|n| div().text_color(theme::warning()).child(n)),
                    )
                    .children((!self.changes.is_empty()).then(|| {
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_size(px(12.)).text_color(theme::muted()).child(
                                match self.change_at {
                                    Some(at) => {
                                        format!("change {} of {}", at + 1, self.changes.len())
                                    }
                                    None => plural(self.changes.len(), "change"),
                                },
                            ))
                            .child(
                                diff_view::group()
                                    .child(chip("prev-change", "↑", false).on_click(cx.listener(
                                        |this, _: &ClickEvent, _, cx| this.step_change(false, cx),
                                    )))
                                    .child(chip("next-change", "↓", false).on_click(cx.listener(
                                        |this, _: &ClickEvent, _, cx| this.step_change(true, cx),
                                    ))),
                            )
                    }))
                    .children(crate::editor::pick(self.settings.editor).map(|editor| {
                        let path = f.path().to_owned();
                        let line = self
                            .changes
                            .get(self.change_at.unwrap_or(0))
                            .and_then(|&row| self.new_line_of_row(row))
                            .unwrap_or(1);
                        chip(
                            "open-editor",
                            format!("Open in {} ↗", editor.label()),
                            false,
                        )
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                this.run_act(
                                    Act::OpenEditor {
                                        path: path.clone(),
                                        line,
                                    },
                                    cx,
                                )
                            },
                        ))
                    }))
                    .children(mode_note.clone().map(|n| {
                        let unavailable = n.starts_with("Structural diff");
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(if unavailable {
                                theme::warning()
                            } else {
                                theme::faint()
                            })
                            .child(n)
                    }))
            }))
            .children(self.render_dfind(cx))
            .children(self.render_goline())
            .children(self.notice.as_ref().map(|notice| {
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .mx(px(8.))
                    .mt(px(6.))
                    .px_3()
                    .py(px(6.))
                    .rounded(px(ROW_RADIUS))
                    .bg(theme::warning_bg())
                    .text_size(px(12.))
                    .text_color(theme::warning())
                    .child(div().flex_1().child(notice.text.clone()))
                    .child(
                        div()
                            .id("notice-undo")
                            .px_2()
                            .rounded(px(ROW_RADIUS))
                            .font_weight(FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .child("Undo")
                            .on_click(cx.listener(|this, _, _, cx| this.undo_notice(cx))),
                    )
                    .child(
                        div()
                            .id("notice-dismiss")
                            .px_2()
                            .rounded(px(ROW_RADIUS))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .child("✕")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notice = None;
                                cx.notify();
                            })),
                    )
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .px(px(4.))
                    .pt(px(4.))
                    .pb(px(6.))
                    .child(body),
            )
    }
}

impl Workspace {
    /// The selected file's rows, scrolled vertically by the list and horizontally by `offset`.
    fn render_rows(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let rows = list(self.diff_list.clone(), move |ix, _, cx| {
            let Some(workspace) = this.upgrade() else {
                return div().into_any_element();
            };
            let workspace = workspace.read(cx);
            let (Some(data), Some(&row)) = (workspace.data.clone(), workspace.rows.get(ix)) else {
                return div().into_any_element();
            };
            let mut style = workspace.row_style();
            style.strong = workspace.matches.get(workspace.match_at) == Some(&ix);
            let indent = style.gutter + 8.;
            match row {
                Row::Thread(id) => return workspace.render_thread(id, indent, this.clone()),
                Row::Request(ix) => {
                    return workspace.render_request_thread(ix, indent, this.clone());
                }
                Row::Draft(ix) => return workspace.render_draft(ix, indent, this.clone()),
                Row::Composer if workspace.compose.is_none() => {
                    return workspace.render_goline_row(indent);
                }
                Row::Composer => return workspace.render_composer(indent, this.clone()),
                _ => {}
            }
            let marked = workspace.goline.is_some() && workspace.goline_row == Some(ix);
            let (expand, comment, press, drag, context) = (
                this.clone(),
                this.clone(),
                this.clone(),
                this.clone(),
                this.clone(),
            );
            let toggle = this.clone();
            let annotate = this.clone();
            let events = diff_view::Events {
                annotate: Box::new(move |_, line, cx| {
                    annotate
                        .update(cx, |this, cx| this.open_annotated_commit(line, cx))
                        .ok();
                }),
                toggle: Box::new(move |old, line, cx| {
                    toggle
                        .update(cx, |this, cx| this.toggle_notes_at(old, line, cx))
                        .ok();
                }),
                comment: Box::new(move |old, line, cx| {
                    comment
                        .update(cx, |this, cx| this.start_comment(old, line, cx))
                        .ok();
                }),
                press: Box::new(move |old, line, byte, clicks, shift, cx| {
                    press
                        .update(cx, |this, cx| {
                            this.select_press(old, line, byte, clicks, shift, cx)
                        })
                        .ok();
                }),
                drag: Box::new(move |old, line, byte, cx| {
                    drag.update(cx, |this, cx| this.select_drag(old, line, byte, cx))
                        .ok();
                }),
                context: Box::new(move |old, line, at, cx| {
                    context
                        .update(cx, |this, cx| this.line_context(old, line, at, cx))
                        .ok();
                }),
            };
            let element = diff_view::row(
                &data,
                row,
                style,
                move |segment, cx| {
                    expand
                        .update(cx, |this, cx| this.expand_gap(segment, cx))
                        .ok();
                },
                std::rc::Rc::new(events),
            );
            if marked {
                return div()
                    .w_full()
                    .border_l_2()
                    .border_color(theme::focus())
                    .bg(theme::hover())
                    .child(element)
                    .into_any_element();
            }
            element
        })
        .flex_1();

        let width = self.body_width.clone();
        let max = self.max_offset();
        let thumb = (!self.options.wrap && max > 0.).then(|| {
            let track = self.body_width.get();
            let share = track / (track + max);
            let thumb = (track * share).max(32.);
            div()
                .absolute()
                .bottom(px(2.))
                .h(px(4.))
                .w(px(thumb))
                .left(px((track - thumb) * self.offset / max))
                .rounded_full()
                .bg(theme::scrollbar())
        });
        div()
            .id("diff-body")
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .on_scroll_wheel(cx.listener(|this, event, _, cx| this.scroll_horizontally(event, cx)))
            .font_family(theme::CODE_FONT)
            .text_size(px(CODE_SIZE))
            .line_height(px(DIFF_ROW))
            .child(rows)
            .child(
                canvas(
                    move |bounds, _, _| width.set(f32::from(bounds.size.width)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .children(thumb)
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match &self.root {
            Some(root) => format!("GitLance — {}", root.display()),
            None => "GitLance".to_owned(),
        };
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        // The widths the layout rules read: the window's, and last frame's width of the diff.
        let win: f32 = window.viewport_size().width.into();
        if (win - self.win_width).abs() > 0.5 {
            self.win_width = win;
            if !self.compact() {
                self.files_popover = false;
            }
        }
        let body = self.body_width.get();
        let narrow = body > 0. && body < 700.;
        if narrow != self.narrow {
            self.narrow = narrow;
            self.relayout();
        }
        if self.char_width == 0. {
            let text = window.text_system();
            let id = text.resolve_font(&font(theme::CODE_FONT));
            self.char_width = text
                .advance(id, px(CODE_SIZE), 'm')
                .map_or(7.2, |size| f32::from(size.width));
        }

        div()
            .id("workspace")
            .key_context(
                if self.find.text.is_some()
                    || self.dfind.is_some()
                    || self.compose.is_some()
                    || self.field.is_some()
                    || self.palette.is_some()
                    || self.ffind.is_some()
                    || self.goline.is_some()
                    || self.newreq.is_some()
                {
                    if self.palette.is_some() || self.ffind.is_some() {
                        "Workspace Typing Palette"
                    } else {
                        "Workspace Typing"
                    }
                } else {
                    "Workspace"
                },
            )
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::previous_commit))
            .on_action(cx.listener(Self::next_commit))
            .on_action(cx.listener(Self::previous_file))
            .on_action(cx.listener(Self::next_file))
            .on_action(cx.listener(Self::toggle_unified))
            .on_action(cx.listener(Self::cycle_mode))
            .on_action(cx.listener(Self::set_lines))
            .on_action(cx.listener(Self::set_words))
            .on_action(cx.listener(Self::set_structural))
            .on_action(cx.listener(Self::open_settings_action))
            .on_action(cx.listener(Self::find_action))
            .on_action(cx.listener(Self::find_commits_action))
            .on_action(cx.listener(Self::filter_files_action))
            .on_action(cx.listener(Self::go_back_action))
            .on_action(cx.listener(Self::go_forward_action))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_files))
            .on_action(cx.listener(Self::toggle_requests))
            .on_action(cx.listener(Self::focus_diff))
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::next_change))
            .on_action(cx.listener(Self::previous_change))
            .on_action(cx.listener(Self::next_match))
            .on_action(cx.listener(Self::previous_match))
            .on_action(cx.listener(|this, _: &CommentLine, _, cx| this.open_goline(cx)))
            .on_action(cx.listener(|this, _: &SubmitReview, _, cx| this.submit_review(cx)))
            .on_action(cx.listener(|this, _: &CreateRequest, _, cx| this.open_create(None, cx)))
            .on_action(cx.listener(|this, _: &ShowShortcuts, _, cx| this.toggle_shortcuts(cx)))
            .on_action(cx.listener(|this, _: &NextThread, _, cx| this.step_thread(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousThread, _, cx| this.step_thread(false, cx)))
            .on_action(cx.listener(|this, _: &ReplyThread, _, cx| this.reply_focused(cx)))
            .on_action(cx.listener(|this, _: &ResolveThread, _, cx| this.resolve_focused(cx)))
            .on_action(cx.listener(|this, _: &ToggleThread, _, cx| this.toggle_focused(cx)))
            .on_action(cx.listener(|this, _: &MarkReviewed, _, cx| this.mark_and_advance(cx)))
            .on_action(cx.listener(|this, _: &ToggleBookmark, _, cx| this.toggle_bookmark(cx)))
            .on_action(cx.listener(|this, _: &ShowBookmarks, _, cx| {
                if this.palette.is_some() {
                    this.close_palette(cx);
                } else if this.diff.is_some() {
                    this.open_palette(palette::Kind::Bookmarks, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowStructure, _, cx| {
                if this.palette.is_some() {
                    this.close_palette(cx);
                } else if this.diff.is_some() {
                    this.open_palette(palette::Kind::Structure, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenRecent, _, cx| {
                if this.palette.is_some() {
                    this.close_palette(cx);
                } else {
                    this.open_palette(palette::Kind::Recent, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleAnnotate, _, cx| this.toggle_annotate(cx)))
            .on_action(cx.listener(|this, _: &FindInFiles, _, cx| this.open_find_files(cx)))
            .on_action(cx.listener(|this, _: &ToggleReviewed, _, cx| this.toggle_reviewed(cx)))
            .on_action(cx.listener(|this, _: &ToggleComments, _, cx| {
                this.set_comments_open(!this.layout.comments_open, cx)
            }))
            .on_action(cx.listener(|this, _: &NextZone, _, cx| this.step_zone(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousZone, _, cx| this.step_zone(false, cx)))
            .on_action(cx.listener(|this, _: &Activate, _, cx| this.activate(cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page(true, cx)))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page(false, cx)))
            .on_action(cx.listener(|this, _: &DiffStart, _, cx| this.diff_edge(false, cx)))
            .on_action(cx.listener(|this, _: &DiffEnd, _, cx| this.diff_edge(true, cx)))
            .on_action(cx.listener(|this, _: &FocusBranches, _, cx| this.open_picker(false, cx)))
            .on_action(
                cx.listener(|this, _: &FocusCommits, _, cx| {
                    this.set_zone(zones::Zone::Commits, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &FocusFiles, _, cx| this.set_zone(zones::Zone::Files, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FocusDiffZone, _, cx| this.set_zone(zones::Zone::Diff, cx)),
            )
            .on_action(cx.listener(|this, _: &NewTab, window, cx| {
                this.with_shell(cx, |s, cx| s.new_tab(window, cx))
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                this.with_shell(cx, |s, cx| {
                    let active = s.active_ix();
                    s.close(active, window, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &ReopenTab, window, cx| {
                this.with_shell(cx, |s, cx| s.reopen(window, cx))
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| {
                this.with_shell(cx, |s, cx| s.step(true, window, cx))
            }))
            .on_action(cx.listener(|this, _: &PreviousTab, window, cx| {
                this.with_shell(cx, |s, cx| s.step(false, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab1, window, cx| {
                this.with_shell(cx, |s, cx| s.select(0, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab2, window, cx| {
                this.with_shell(cx, |s, cx| s.select(1, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab3, window, cx| {
                this.with_shell(cx, |s, cx| s.select(2, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab4, window, cx| {
                this.with_shell(cx, |s, cx| s.select(3, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab5, window, cx| {
                this.with_shell(cx, |s, cx| s.select(4, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab6, window, cx| {
                this.with_shell(cx, |s, cx| s.select(5, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab7, window, cx| {
                this.with_shell(cx, |s, cx| s.select(6, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab8, window, cx| {
                this.with_shell(cx, |s, cx| s.select(7, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab9, window, cx| {
                this.with_shell(cx, |s, cx| s.select(8, window, cx))
            }))
            .on_action(cx.listener(Self::open_palette_action))
            .on_action(cx.listener(Self::compare_action))
            .on_action(cx.listener(Self::palette_up))
            .on_action(cx.listener(Self::palette_down))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.find_key(event, window, cx)
            }))
            .on_action(cx.listener(Self::toggle_wrap))
            .on_action(cx.listener(Self::toggle_full_context))
            .on_action(cx.listener(Self::toggle_whitespace))
            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().first() {
                    this.open_repo(path.clone(), None, cx);
                }
            }))
            .on_mouse_move(cx.listener(|this, event, _, cx| this.drag_move(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.selecting = false;
                    this.end_drag(cx)
                }),
            )
            .when_some(self.drag, |s, drag| {
                s.cursor(if drag.split.vertical() {
                    CursorStyle::ResizeUpDown
                } else {
                    CursorStyle::ResizeLeftRight
                })
            })
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme::base())
            .text_color(theme::text())
            .font_family(theme::UI_FONT)
            .text_size(px(13.))
            .child(self.render_title_bar(cx))
            .children(self.render_tab_strip(cx))
            .children(self.error.clone().map(|e| {
                div()
                    .flex_none()
                    .mx(px(GAP))
                    .mb(px(GAP))
                    .px_4()
                    .py_2()
                    .rounded(px(ISLAND_RADIUS))
                    .bg(theme::removed_line())
                    .text_color(theme::removed())
                    .child(e)
            }))
            .child(match self.root {
                _ if self.settings_open => self.render_settings(cx).into_any_element(),
                None => self.render_welcome(cx).into_any_element(),
                Some(_) => div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .px(px(GAP))
                    .pb(px(GAP))
                    .child(self.render_rail(cx))
                    .children(self.layout.show_sidebar.then(|| {
                        div()
                            .flex()
                            .flex_none()
                            .child(self.render_sidebar(cx))
                            .child(self.handle(Split::Sidebar, cx))
                    }))
                    .child(self.render_diff(cx))
                    .into_any_element(),
            })
            // Last, so it paints over everything.
            .children(self.repo_menu.then(|| self.render_repo_menu(cx)))
            .children(self.render_ctx_menu(window.viewport_size(), cx))
            .children(self.render_picker(cx))
            .children(self.render_find_files(cx))
            .children(self.render_create(cx))
            .children(self.render_shortcuts(cx))
            .children(self.render_palette(cx))
    }
}

/// The pointer coordinate along the axis `split` resizes.
fn axis(split: Split, position: Point<gpui::Pixels>) -> f32 {
    f32::from(if split.vertical() {
        position.y
    } else {
        position.x
    })
}

pub(super) fn render_file_row(
    file: &FileDiff,
    tag: Option<&&'static str>,
    ix: usize,
    selected: bool,
    review: Option<gpui::AnyElement>,
    done: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let path = file.path();
    let (dir, name) = match path.rsplit_once('/') {
        Some((dir, name)) => (Some(dir.to_owned()), name.to_owned()),
        None => (None, path.to_owned()),
    };
    let item = row(("file", ix), selected)
        .h(px(40.))
        .gap_2()
        .when(done, |s| s.text_color(theme::muted()))
        .children(review)
        .child(change_badge(file.change))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().child(name))
                .children(dir.map(|d| {
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(d)
                })),
        )
        .children(generated_tag(file))
        .children(working::file_tag(tag))
        .child(stats(file))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.file_context(ix, event.position, cx)
            }),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.select_file(ix, cx)));
    div().w_full().h(px(42.)).px(px(6.)).py(px(1.)).child(item)
}

/// "generated" beside a lock or generated file.
pub(super) fn generated_tag(file: &FileDiff) -> Option<gpui::Div> {
    crate::generated::is_generated(file.path()).then(|| {
        div()
            .flex_none()
            .text_size(px(10.))
            .text_color(theme::faint())
            .child("generated")
    })
}

/// A file's added and removed line counts.
fn stats(file: &FileDiff) -> impl IntoElement + use<> {
    let (added, removed) = (file.added, file.removed);
    div()
        .flex()
        .gap_1()
        .text_size(px(11.))
        .when(added > 0, |s| {
            s.child(div().text_color(theme::added()).child(format!("+{added}")))
        })
        .when(removed > 0, |s| {
            s.child(
                div()
                    .text_color(theme::removed())
                    .child(format!("−{removed}")),
            )
        })
}

fn change_badge(change: ChangeKind) -> impl IntoElement {
    let (letter, color) = match change {
        ChangeKind::Added => ("A", theme::added()),
        ChangeKind::Deleted => ("D", theme::removed()),
        ChangeKind::Modified => ("M", theme::accent()),
        ChangeKind::Renamed => ("R", theme::renamed()),
        ChangeKind::Copied => ("C", theme::renamed()),
        ChangeKind::TypeChanged => ("T", theme::warning()),
    };
    div()
        .w(px(16.))
        .flex_none()
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .text_color(color)
        .child(letter)
}

fn section_label(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex_none()
        .pb_1()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::faint())
        .child(text.into().to_uppercase())
}

/// A rounded pane floating on the window background.
fn island() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(ISLAND_RADIUS))
        .border_1()
        .border_color(theme::island_border())
        .bg(theme::panel())
}

/// A `Key: value` line of the kind git calls a trailer.
fn is_trailer(line: &str) -> bool {
    const KEYS: [&str; 7] = [
        "co-authored-by",
        "signed-off-by",
        "reviewed-by",
        "acked-by",
        "tested-by",
        "reported-by",
        "cc",
    ];
    line.split_once(':')
        .is_some_and(|(key, _)| KEYS.contains(&key.trim().to_lowercase().as_str()))
}

/// A small pill on the Commits label.
fn commit_chip(id: &'static str, on: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(22.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(5.))
        .rounded(px(ROW_RADIUS))
        .text_size(px(12.))
        .text_color(if on { theme::text() } else { theme::muted() })
        .cursor_pointer()
        .when(on, |s| s.bg(theme::selected()))
        .hover(|s| s.bg(theme::hover()))
}

fn island_label(text: impl Into<SharedString>) -> gpui::Div {
    section_label(text).px(px(14.)).pt(px(10.)).pb(px(6.))
}

/// A clickable list row: a rounded pill, filled when selected.
fn row(id: impl Into<gpui::ElementId>, selected: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w_full()
        .flex()
        .items_center()
        .px(px(8.))
        .rounded(px(ROW_RADIUS))
        .cursor_pointer()
        .when(selected, |s| s.bg(theme::selected()))
        .when(!selected, |s| s.hover(|s| s.bg(theme::hover())))
}

fn tag(text: &'static str, color: Rgba) -> impl IntoElement {
    div()
        .px(px(6.))
        .rounded_full()
        .border_1()
        .border_color(color)
        .text_size(px(10.))
        .text_color(color)
        .child(text)
}

/// A square icon button for the title bar; the tooltip names it and its shortcut.
fn icon_button(
    id: &'static str,
    icon: &'static str,
    tip: &'static str,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(ROW_RADIUS))
        .group(id)
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        // An svg takes its colour from itself, not from its parent.
        .child(
            icons::icon(icon)
                .text_color(theme::muted())
                .group_hover(id, |s| s.text_color(theme::text())),
        )
        .tooltip(move |_, cx| cx.new(|_| Tip(tip)).into())
}

/// Back / forward: dim when there is nowhere to go (the handlers check again).
fn nav_button(
    id: &'static str,
    icon: &'static str,
    tip: &'static str,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    let button = div()
        .id(id)
        .size(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(ROW_RADIUS))
        .group(id)
        .child(
            icons::icon(icon)
                .text_color(if enabled {
                    theme::muted()
                } else {
                    theme::faint()
                })
                .when(enabled, |s| {
                    s.group_hover(id, |s| s.text_color(theme::text()))
                }),
        );
    if enabled {
        button
            .cursor_pointer()
            .hover(|s| s.bg(theme::hover()))
            .tooltip(move |_, cx| cx.new(|_| Tip(tip)).into())
    } else {
        button.opacity(0.45)
    }
}

/// A tooltip: a small panel with a line of text.
struct Tip(&'static str);

/// A tooltip whose text is made when it is shown (blame's: the commit's title, hash and age).
struct TipOwned(SharedString);

impl Render for TipOwned {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .mt(px(6.))
            .px_2()
            .py_1()
            .rounded(px(ROW_RADIUS))
            .border_1()
            .border_color(theme::island_border())
            .bg(theme::hover())
            .text_size(px(11.))
            .text_color(theme::text())
            .child(self.0.clone())
    }
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .mt(px(6.))
            .px_2()
            .py_1()
            .rounded(px(ROW_RADIUS))
            .border_1()
            .border_color(theme::island_border())
            .bg(theme::hover())
            .text_size(px(11.))
            .text_color(theme::text())
            .child(self.0)
    }
}

fn button(
    id: &'static str,
    label: &'static str,
    shortcut: &'static str,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .rounded(px(8.))
        .border_1()
        .border_color(theme::island_border())
        .bg(theme::panel())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(label)
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme::faint())
                .child(shortcut),
        )
}

fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        return format!("1 {noun}");
    }
    // "reply" → "replies", not "replys".
    match noun.strip_suffix('y') {
        Some(stem) if stem.ends_with(|c: char| !"aeiou".contains(c)) => format!("{n} {stem}ies"),
        _ => format!("{n} {noun}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailers_are_told_from_prose() {
        assert!(is_trailer("Co-Authored-By: Someone <a@b.c>"));
        assert!(is_trailer("signed-off-by: Someone"));
        assert!(!is_trailer("Fixes: the retry loop"));
        assert!(!is_trailer("A sentence: with a colon."));
        assert!(!is_trailer("no colon at all"));
    }

    #[test]
    fn plurals_read_naturally() {
        assert_eq!(plural(1, "file"), "1 file");
        assert_eq!(plural(3, "commit"), "3 commits");
        assert_eq!(plural(2, "reply"), "2 replies");
        assert_eq!(plural(2, "day"), "2 days");
    }

    #[test]
    fn the_window_has_a_default_size() {
        assert_eq!(window_size(), (1480., 920.));
    }
}
