//! The GitLance window: branches, versions and commits on the left, the diff on the right.

mod agent;
mod annotate;
mod bookmarks;
mod comments;
mod create;
mod details;
mod diff_view;
mod discussion;
mod fetch;
mod find;
mod findfiles;
mod format;
mod frame;
mod goto;
mod icons;
mod image_view;
mod input;
mod keymap;
mod keys;
mod lists;
mod markdown;
mod menu;
mod notifications;
mod opening;
mod palette;
mod places;
mod rail;
mod recents;
mod requests;
mod reviewed;
mod rows;
mod select;
mod settings;
mod shell;
mod shortcuts;
mod side;
mod stack;
mod submit;
mod theme;
mod toast;
mod tools;
mod watch;
mod words;
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
    div, font, list, prelude::*, size,
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
        ToggleOneFile,
        ToggleExpandAll,
        ToggleRichComments,
        LeaveRequest,
        ToggleAllReviewed,
        NextRequest,
        PreviousRequest,
        CommentLine,
        SubmitReview,
        ApproveRequest,
        ToggleNotifications,
        SwitchTo,
        NextDraft,
        PreviousDraft,
        CreateRequest,
        SendToAgent,
        ShowShortcuts,
        ToggleThread,
        ToggleComments,
        ToggleDetails,
        MarkReviewed,
        OpenRecent,
        ZoomIn,
        ZoomOut,
        ZoomReset,
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
        EditComment,
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
/// Space between islands; the resize handles live in it. Flat panes have none (`gap`).
const GAP: f32 = 8.;

/// Flat panes (Settings → Appearance): hairlines, no gaps, square corners. With `PROJECTS`, the
/// window-wide part of the settings: every tab and window reads it, set when settings load or change.
static FLAT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// How several projects show: `storage::Projects::ALL[n]`.
static PROJECTS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn flat() -> bool {
    FLAT.load(std::sync::atomic::Ordering::Relaxed)
}

fn projects() -> storage::Projects {
    let n = PROJECTS.load(std::sync::atomic::Ordering::Relaxed) as usize;
    storage::Projects::ALL[n.min(storage::Projects::ALL.len() - 1)]
}

/// Makes `settings`' window-wide choices the ones every tab draws.
fn share_settings(settings: &Settings) {
    use std::sync::atomic::Ordering::Relaxed;
    FLAT.store(settings.flat, Relaxed);
    let n = storage::Projects::ALL
        .iter()
        .position(|p| *p == settings.projects)
        .unwrap_or(0);
    PROJECTS.store(n as u8, Relaxed);
}

/// The space between panes: `GAP` for islands, none for flat panes.
fn gap() -> f32 {
    if flat() { 0. } else { GAP }
}
const ISLAND_RADIUS: f32 = 10.;
const ROW_RADIUS: f32 = 6.;

/// The keys of the app as shipped; `keymap.txt` changes them (`keymap.rs`).
fn default_bindings() -> Vec<KeyBinding> {
    bindings(crate::platform::keys)
}

/// The shipped keys, written with macOS keys and put in this OS's terms by `k`.
fn bindings(k: fn(&str) -> String) -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(&k("cmd-o"), Open, None),
        KeyBinding::new(&k("cmd-r"), Refresh, None),
        KeyBinding::new(&k("cmd-q"), Quit, None),
        KeyBinding::new(&k("cmd-,"), OpenSettings, None),
        KeyBinding::new(&k("cmd-f"), Find, Some("Workspace")),
        KeyBinding::new(&k("cmd-shift-f"), FindCommits, Some("Workspace")),
        KeyBinding::new(&k("cmd-p"), FilterFiles, Some("Workspace")),
        KeyBinding::new(&k("cmd-["), GoBack, Some("Workspace")),
        KeyBinding::new(&k("cmd-]"), GoForward, Some("Workspace")),
        KeyBinding::new(&k("cmd-alt-1"), ToggleSidebar, Some("Workspace")),
        KeyBinding::new(&k("cmd-alt-2"), ToggleFiles, Some("Workspace")),
        KeyBinding::new(&k("cmd-alt-3"), ToggleRequests, Some("Workspace")),
        KeyBinding::new(&k("c"), CommentLine, Some("Workspace && !Typing")),
        KeyBinding::new(
            &k("cmd-shift-enter"),
            SubmitReview,
            Some("Workspace && !Typing"),
        ),
        KeyBinding::new(&k("cmd-alt-m"), CreateRequest, Some("Workspace && !Typing")),
        KeyBinding::new(&k("cmd-shift-a"), SendToAgent, Some("Workspace && !Typing")),
        KeyBinding::new(&k("a"), ApproveRequest, Some("Workspace && !Typing")),
        KeyBinding::new(&k("cmd-b"), SwitchTo, Some("Workspace && !Typing")),
        KeyBinding::new(&k("cmd-shift-n"), ToggleNotifications, Some("Workspace")),
        KeyBinding::new(&k("alt-n"), NextDraft, Some("Workspace && !Typing")),
        KeyBinding::new(&k("alt-p"), PreviousDraft, Some("Workspace && !Typing")),
        KeyBinding::new(&k("?"), ShowShortcuts, Some("Workspace && !Typing")),
        KeyBinding::new(&k("cmd-/"), ShowShortcuts, Some("Workspace")),
        KeyBinding::new(&k("shift-n"), NextThread, Some("Workspace && !Typing")),
        KeyBinding::new(&k("shift-p"), PreviousThread, Some("Workspace && !Typing")),
        KeyBinding::new(&k("r"), ReplyThread, Some("Workspace && !Typing")),
        KeyBinding::new(&k("x"), ResolveThread, Some("Workspace && !Typing")),
        KeyBinding::new(&k("e"), EditComment, Some("Workspace && !Typing")),
        KeyBinding::new(&k("o"), ToggleThread, Some("Workspace && !Typing")),
        KeyBinding::new(
            &k("cmd-shift-r"),
            ToggleComments,
            Some("Workspace && !Typing"),
        ),
        KeyBinding::new(&k("cmd-i"), ToggleDetails, Some("Workspace && !Typing")),
        KeyBinding::new(&k("v"), MarkReviewed, Some("Workspace && !Typing")),
        KeyBinding::new(&k("cmd-e"), OpenRecent, Some("Workspace")),
        KeyBinding::new(&k("cmd-="), ZoomIn, Some("Workspace")),
        KeyBinding::new(&k("cmd-+"), ZoomIn, Some("Workspace")),
        KeyBinding::new(&k("cmd-shift-="), ZoomIn, Some("Workspace")),
        KeyBinding::new(&k("cmd--"), ZoomOut, Some("Workspace")),
        KeyBinding::new(&k("cmd-_"), ZoomOut, Some("Workspace")),
        KeyBinding::new(&k("cmd-0"), ZoomReset, Some("Workspace")),
        KeyBinding::new(&k("f3"), ToggleBookmark, Some("Workspace && !Typing")),
        KeyBinding::new(&k("cmd-f3"), ShowBookmarks, Some("Workspace")),
        KeyBinding::new(&k("cmd-shift-o"), ShowStructure, Some("Workspace")),
        KeyBinding::new(
            &k("cmd-alt-b"),
            ToggleAnnotate,
            Some("Workspace && !Typing"),
        ),
        KeyBinding::new(&k("cmd-alt-f"), FindInFiles, Some("Workspace")),
        KeyBinding::new(&k("shift-v"), ToggleReviewed, Some("Workspace && !Typing")),
        KeyBinding::new(&k("tab"), NextZone, Some("Workspace && !Typing")),
        KeyBinding::new(&k("shift-tab"), PreviousZone, Some("Workspace && !Typing")),
        KeyBinding::new(&k("enter"), Activate, Some("Workspace && !Typing")),
        KeyBinding::new(&k("pagedown"), PageDown, Some("Workspace && !Typing")),
        KeyBinding::new(&k("space"), PageDown, Some("Workspace && !Typing")),
        KeyBinding::new(&k("pageup"), PageUp, Some("Workspace && !Typing")),
        KeyBinding::new(&k("shift-space"), PageUp, Some("Workspace && !Typing")),
        KeyBinding::new(&k("home"), DiffStart, Some("Workspace && !Typing")),
        KeyBinding::new(&k("end"), DiffEnd, Some("Workspace && !Typing")),
        KeyBinding::new(&k("h"), PreviousFile, Some("Workspace && !Typing")),
        KeyBinding::new(&k("l"), NextFile, Some("Workspace && !Typing")),
        KeyBinding::new(&k("["), PreviousFile, Some("Workspace && !Typing")),
        KeyBinding::new(&k("]"), NextFile, Some("Workspace && !Typing")),
        KeyBinding::new(&k("ctrl-1"), FocusBranches, Some("Workspace")),
        KeyBinding::new(&k("ctrl-2"), FocusCommits, Some("Workspace")),
        KeyBinding::new(&k("ctrl-3"), FocusFiles, Some("Workspace")),
        KeyBinding::new(&k("ctrl-4"), FocusDiffZone, Some("Workspace")),
        KeyBinding::new(&k("cmd-t"), NewTab, Some("Workspace")),
        KeyBinding::new(&k("cmd-w"), CloseTab, Some("Workspace")),
        KeyBinding::new(&k("cmd-shift-t"), ReopenTab, Some("Workspace")),
        KeyBinding::new(&k("cmd-shift-]"), NextTab, Some("Workspace")),
        KeyBinding::new(&k("cmd-shift-["), PreviousTab, Some("Workspace")),
        KeyBinding::new(&k("cmd-1"), Tab1, Some("Workspace")),
        KeyBinding::new(&k("cmd-2"), Tab2, Some("Workspace")),
        KeyBinding::new(&k("cmd-3"), Tab3, Some("Workspace")),
        KeyBinding::new(&k("cmd-4"), Tab4, Some("Workspace")),
        KeyBinding::new(&k("cmd-5"), Tab5, Some("Workspace")),
        KeyBinding::new(&k("cmd-6"), Tab6, Some("Workspace")),
        KeyBinding::new(&k("cmd-7"), Tab7, Some("Workspace")),
        KeyBinding::new(&k("cmd-8"), Tab8, Some("Workspace")),
        KeyBinding::new(&k("cmd-9"), Tab9, Some("Workspace")),
        KeyBinding::new(&k("cmd-."), FocusDiff, Some("Workspace")),
        KeyBinding::new(&k("cmd-c"), CopySelection, Some("Workspace")),
        KeyBinding::new(&k("cmd-a"), SelectAll, Some("Workspace && !Typing")),
        KeyBinding::new(&k("f7"), NextChange, Some("Workspace")),
        KeyBinding::new(&k("shift-f7"), PreviousChange, Some("Workspace")),
        KeyBinding::new(&k("n"), NextChange, Some("Workspace && !Typing")),
        KeyBinding::new(&k("p"), PreviousChange, Some("Workspace && !Typing")),
        KeyBinding::new(&k("cmd-g"), NextMatch, Some("Workspace")),
        KeyBinding::new(&k("cmd-shift-g"), PreviousMatch, Some("Workspace")),
        KeyBinding::new(
            &k("up"),
            PreviousCommit,
            Some("Workspace && !Palette && !Caret"),
        ),
        KeyBinding::new(
            &k("down"),
            NextCommit,
            Some("Workspace && !Palette && !Caret"),
        ),
        KeyBinding::new(&k("up"), PaletteUp, Some("Palette")),
        KeyBinding::new(&k("down"), PaletteDown, Some("Palette")),
        KeyBinding::new(&k("cmd-k"), OpenPalette, Some("Workspace")),
        KeyBinding::new(&k("cmd-shift-c"), Compare, Some("Workspace")),
        // Bare letters and arrows are typing while the search field is active.
        KeyBinding::new(&k("k"), PreviousCommit, Some("Workspace && !Typing")),
        KeyBinding::new(&k("j"), NextCommit, Some("Workspace && !Typing")),
        KeyBinding::new(&k("left"), PreviousFile, Some("Workspace && !Typing")),
        KeyBinding::new(&k("right"), NextFile, Some("Workspace && !Typing")),
        KeyBinding::new(&k("w"), CycleMode, Some("Workspace && !Typing")),
        KeyBinding::new(&k("alt-u"), ToggleUnified, Some("Workspace")),
        KeyBinding::new(&k("alt-d"), CycleMode, Some("Workspace")),
        KeyBinding::new(&k("alt-z"), ToggleWrap, Some("Workspace")),
        KeyBinding::new(&k("alt-e"), ToggleFullContext, Some("Workspace")),
        KeyBinding::new(&k("alt-w"), ToggleWhitespace, Some("Workspace")),
        KeyBinding::new(&k("alt-a"), ToggleOneFile, Some("Workspace")),
        KeyBinding::new(&k("alt-shift-a"), ToggleExpandAll, Some("Workspace")),
        KeyBinding::new(
            &k("alt-m"),
            ToggleRichComments,
            Some("Workspace && !Typing"),
        ),
        KeyBinding::new(&k("cmd-alt-l"), LeaveRequest, Some("Workspace")),
        KeyBinding::new(
            &k("alt-shift-v"),
            ToggleAllReviewed,
            Some("Workspace && !Typing"),
        ),
        KeyBinding::new(&k("alt-down"), NextRequest, Some("Workspace && !Typing")),
        KeyBinding::new(&k("alt-up"), PreviousRequest, Some("Workspace && !Typing")),
    ]
}

pub fn run(path: Option<PathBuf>) {
    gpui_platform::application()
        .with_assets(icons::Icons)
        .run(move |cx: &mut App| {
            keymap::install(default_bindings(), cx);
            if !cfg!(feature = "snapshot") || std::env::var_os("GITLANCE_SNAPSHOT").is_none() {
                crate::platform::set_icon();
            }
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.set_menus(menus(&ViewOptions::load(&Settings::load())));

            // A snapshot renders offscreen: no window shown, no focus taken.
            let offscreen =
                cfg!(feature = "snapshot") && std::env::var_os("GITLANCE_SNAPSHOT").is_some();
            let options = window_options(offscreen, cx);
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

/// A window of GitLance: its size, title bar and smallest size.
fn window_options(offscreen: bool, cx: &mut App) -> WindowOptions {
    let (width, height) = window_size();
    let bounds = gpui::Bounds::centered(None, size(px(width), px(height)), cx);
    WindowOptions {
        show: !offscreen,
        focus: !offscreen,
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("GitLance".into()),
            appears_transparent: crate::platform::TRANSPARENT_TITLEBAR,
            traffic_light_position: Some(Point::new(px(12.), px(12.))),
        }),
        window_min_size: Some(size(px(900.), px(500.))),
        app_id: Some("gitlance".into()),
        ..Default::default()
    }
}

/// Opens `path` in a window of its own (the rail's "Open in a new window").
fn open_new_window(path: PathBuf, cx: &mut App) {
    let options = window_options(false, cx);
    let opened = cx.open_window(options, |window, cx| {
        cx.new(|cx| shell::Shell::secondary(path, window, cx))
    });
    if opened.is_ok() {
        cx.activate(true);
    }
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
            MenuItem::action("Zoom In", ZoomIn),
            MenuItem::action("Zoom Out", ZoomOut),
            MenuItem::action("Actual Size", ZoomReset),
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
            MenuItem::action("One File at a Time", ToggleOneFile).checked(options.one_file),
        ]),
        Menu::new("Help").items([MenuItem::action("Keyboard Shortcuts", ShowShortcuts)]),
    ]
}

/// A view option, for the toolbar and the View menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opt {
    Unified,
    Wrap,
    FullContext,
    Whitespace,
    OneFile,
    FoldReviewed,
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
                                    ..Default::default()
                                });
                                this.start_comment(false, line + 1, cx);
                                if let Some(c) = this.compose.as_mut() {
                                    c.edit = crate::textedit::TextEdit::new(
                                        "Name this after what it holds.",
                                    );
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
                            this.open_titled(Point::new(px(1090.), px(78.)), groups, cx);
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
                        "swipe" | "onion" => {
                            this.images.mode = if name.trim() == "swipe" {
                                image_view::Mode::Swipe
                            } else {
                                image_view::Mode::Onion
                            };
                            continue;
                        }
                        "agents" => {
                            this.open_settings(cx);
                            this.settings_section = settings::Section::Agents;
                            continue;
                        }
                        "send" => {
                            this.open_agent_pick(cx);
                            continue;
                        }
                        // A side panel section folded (a snapshot run never saves it).
                        "fold-versions" => {
                            this.layout.fold_versions = true;
                            continue;
                        }
                        "fold-commits" => {
                            this.layout.fold_commits = true;
                            continue;
                        }
                        "fold-places" => {
                            this.layout.fold_places = true;
                            continue;
                        }
                        "rail-tabs" => {
                            this.settings.projects = crate::storage::Projects::Tabs;
                            share_settings(&this.settings);
                            continue;
                        }
                        "rail-menu" => {
                            this.settings.projects = crate::storage::Projects::Menu;
                            share_settings(&this.settings);
                            continue;
                        }
                        "flat" => {
                            this.settings.flat = true;
                            share_settings(&this.settings);
                            continue;
                        }
                        "appearance" => {
                            this.open_settings(cx);
                            this.settings_section = settings::Section::Appearance;
                            continue;
                        }
                        "inbox" => {
                            this.go_place(places::Place::Inbox, cx);
                            continue;
                        }
                        "review-requests" => {
                            this.go_place(places::Place::ReviewRequests, cx);
                            continue;
                        }
                        "fold-changes" => {
                            this.layout.show_files = false;
                            continue;
                        }
                        // A tool window on the right, without saving it as the open one.
                        tool if tool.starts_with("tool-") => {
                            if let Some(t) = tools::Tool::parse(&tool[5..]) {
                                this.set_tool(Some(t), cx);
                            }
                            continue;
                        }
                        "keyboard" => {
                            this.open_settings(cx);
                            this.settings_section = settings::Section::Keyboard;
                            this.start_field(Field::Keys, cx);
                            continue;
                        }
                        "wrap" => Opt::Wrap,
                        "all-lines" => Opt::FullContext,
                        "whitespace" => Opt::Whitespace,
                        "one-file" => Opt::OneFile,
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
        // Opening another repository while this one is open: the card asks, or Settings decides.
        if let Ok(path) = std::env::var("GITLANCE_SNAPSHOT_OPEN") {
            ws.update(cx, |this, cx| this.request_open(path.into(), None, cx));
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
            // `wait` gives a diff a second to load before the next key.
            for key in keys.split_whitespace() {
                if key == "wait" {
                    wait(1000).await;
                    continue;
                }
                if let Ok(keystroke) = gpui::Keystroke::parse(key) {
                    cx.update_window(window.into(), |_, window, cx| {
                        window.dispatch_keystroke(keystroke, cx);
                    })
                    .ok();
                }
                wait(400).await;
            }
        }
        // A hidden window gets no display-link frames: draw one by hand. Images start decoding
        // in the background at a draw, so a first draw, a pause, then the one that is saved.
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .ok();
        wait(800).await;
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
        /// The base is a local branch behind its upstream.
        stale: Option<crate::git::Stale>,
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
    /// The tool window's left edge: dragging left widens it.
    Tool,
}

impl Split {
    fn vertical(self) -> bool {
        false
    }

    fn bounds(self) -> (f32, f32) {
        match self {
            Split::Sidebar => (220., 720.),
            Split::Tool => (260., 640.),
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
    /// Errors shown as toasts (`toast.rs`).
    toasts: toast::Toasts,
    /// A link's target waiting for its repository to load (`goto.rs`).
    pending_link: Option<crate::links::Target>,
    /// The comment or line a link pointed at, waiting for the request's diff and discussions.
    spot: Option<goto::Spot>,
    /// How changed images are shown (`image_view.rs`).
    images: image_view::Images,
    /// What Settings → Keyboard is filtered by.
    keys_filter: String,
    /// The part of Settings shown.
    settings_section: settings::Section,
    /// A selection in the text of a comment (`markdown::TextSel`).
    note_sel: Option<markdown::TextSel>,
    /// The action whose new key Settings → Keyboard is waiting for.
    recording: Option<&'static str>,
    /// Zen (⌘.): what the branches and files islands were before it hid them.
    zen: Option<(bool, bool)>,
    /// The Submit review card (`submit.rs`).
    submit: Option<submit::Submit>,
    /// The pending comment ⌥N / ⌥P is on.
    draft_at: Option<usize>,
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
    /// The window this workspace is in: a repository can open in a new tab of it from anywhere.
    window: gpui::AnyWindowHandle,
    /// The bell and its list (`notifications.rs`).
    notices: notifications::Notices,
    /// The "this tab or a new tab?" card (`opening.rs`).
    open_ask: Option<opening::OpenAsk>,
    /// The "send to which chat?" card (`agent.rs`).
    agent_pick: Option<agent::AgentPick>,
    /// When the agents' answers were last read (`agent.rs`).
    answers_stamp: Option<std::time::SystemTime>,
    /// The chats of this repository, for the Agents window (read while it is open).
    agent_chats: Vec<crate::agents::Chat>,
    /// The Inbox place is shown, and which of its notifications it lists.
    inbox: bool,
    inbox_filter: places::InboxFilter,
    /// The menu of places opened while a merge request is reviewed (it is folded there).
    places_peek: bool,
    /// Settings → Agents: what connecting said, and whether it runs.
    agent_setup: Option<String>,
    agent_setup_busy: bool,
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
    zoom_applied: bool,
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
    /// The card that comes up when the last file of a diff is marked reviewed.
    done_card: bool,
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
    /// The merge request the last comment went to: the next composer on its diff starts there.
    last_request_target: Option<u64>,
    /// Who is reviewing (git's `user.name`): the composer's avatar.
    me: String,
    /// A comment to scroll to once its file is laid out.
    jump: Option<u64>,
    /// "Copied ✓" on the Copy button until something changes.
    copied: bool,
    /// The caret, selection and undo of the field that has the keyboard (`input`).
    field_edit: crate::textedit::TextEdit,
    /// How far the one-line field with the keyboard is scrolled sideways.
    field_scroll: Rc<Shared<f32>>,
    /// The last layout of the wrapped field (the composer, a description) and the x ↑ ↓ keep.
    field_lines: input::Lines,
    field_goal: Option<gpui::Pixels>,
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
    /// Every file of the diff in one scroll (`stack.rs`); `None` while files are shown one at a
    /// time.
    stack: Option<stack::Stack>,
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
        let mut settings = Settings::load();
        share_settings(&settings);
        // The keyboard card greets the very first launch, once (never a check's snapshot).
        let greet = !settings.keys_seen
            && std::env::var_os("GITLANCE_SNAPSHOT").is_none()
            && !shortcuts::SHOWN.swap(true, std::sync::atomic::Ordering::Relaxed);
        if greet {
            settings.keys_seen = true;
            settings.save();
        }
        let start = path.or_else(|| open_recent.then(|| recent.first().cloned()).flatten());
        let mut this = Self {
            focus,
            layout: Layout::load(),
            drag: None,
            root: None,
            recent,
            error: None,
            toasts: Default::default(),
            submit: None,
            draft_at: None,
            zen: None,
            recording: None,
            note_sel: None,
            settings_section: Default::default(),
            keys_filter: String::new(),
            images: Default::default(),
            pending_link: None,
            spot: None,
            window: window.window_handle(),
            open_ask: None,
            agent_pick: None,
            answers_stamp: None,
            agent_chats: Vec::new(),
            inbox: false,
            inbox_filter: Default::default(),
            places_peek: false,
            agent_setup: None,
            agent_setup_busy: false,
            notices: Default::default(),
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
            shortcuts: greet,
            zoom_applied: false,
            bookmarks: bookmarks::Store::load(),
            recents: Vec::new(),
            annotate: false,
            annotations: Vec::new(),
            annot_task: None,
            ffind: None,
            reviewed: reviewed::Store::load(),
            done_card: false,
            review_scope: None,
            fps: Vec::new(),
            unfolded: HashSet::new(),
            picker_open: false,
            win_width: 1480.,
            narrow: false,
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
            last_request_target: None,
            me: "You".to_owned(),
            jump: None,
            copied: false,
            field_edit: Default::default(),
            field_scroll: Rc::default(),
            field_lines: Rc::default(),
            field_goal: None,
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
            stack: None,
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
                    let me = repo.user_name();
                    anyhow::Ok((
                        repo.root(),
                        repo.branches()?,
                        git_dir,
                        fingerprint,
                        web,
                        (decor, me),
                    ))
                })
                .await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok((root, branches, git_dir, fingerprint, web, (decor, me))) => {
                        this.web = web;
                        this.decor = decor;
                        this.me = me.unwrap_or_else(|| "You".to_owned());
                        this.recent = storage::remember(&root);
                        if this.root.as_ref() != Some(&root) {
                            this.find.reset();
                        }
                        if this.root.as_ref() != Some(&root) {
                            this.comments = crate::review::load(&root);
                            this.answers_stamp = None;
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
                        this.follow_pending_link(cx);
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
        self.stack = None;
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
                        this.follow_pending_link(cx);
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
                        // The files of the diff it replaces are gone.
                        this.stack = None;
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
                        this.resolve_spot(false, cx);
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
        self.remember_place(
            recents::Key::File(file.path().to_owned()),
            file.path().to_owned(),
            "file".to_owned(),
        );
        if self.wants_stack() {
            return self.stack_select(ix, cx);
        }
        self.file = ix;
        self.fcursor = None;
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

    /// Lays the selected file out again for the current options; the diff scrolls to the top
    /// (with all files in one scroll, only its rows that changed are replaced).
    fn relayout(&mut self) {
        let block = self
            .stack
            .as_ref()
            .and_then(|s| s.block_of(self.file))
            .map(|b| (b, self.shown_rows(b)));
        let rows = self
            .data
            .as_ref()
            .map(|data| rows::layout(data, &self.view_options(), &self.expanded))
            .unwrap_or_default();
        self.rows = self.with_comments(rows);
        self.notes = self.line_notes();
        match block {
            Some((b, before)) => self.stack_changed(b, before),
            None => self.diff_list.reset(self.rows.len()),
        }
        self.offset = self.offset.min(self.max_offset());
        self.recompute_matches();
    }

    /// Every file laid out again: the open one, or all of them in one scroll.
    fn relayout_all(&mut self) {
        if self.stack.is_some() {
            self.relayout_stack();
        } else {
            self.relayout();
        }
    }

    /// Shows the hidden lines of an unchanged run in place, keeping the scroll position.
    fn expand_gap(&mut self, segment: usize, cx: &mut Context<Self>) {
        let Some(data) = self.data.clone() else {
            return;
        };
        if self.stack.is_some() {
            self.expanded.insert(segment);
            self.relayout();
            return cx.notify();
        }
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
            Opt::OneFile => self.options.one_file,
            Opt::FoldReviewed => self.options.fold_reviewed,
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
            Opt::OneFile => self.options.one_file = value,
            Opt::FoldReviewed => self.options.fold_reviewed = value,
        }
        cx.set_menus(menus(&self.options));
        match opt {
            Opt::Unified | Opt::FullContext => self.relayout_all(),
            Opt::FoldReviewed => self.refold(cx),
            Opt::OneFile => {
                self.restore_scroll = self.scroll_place();
                self.clear_file();
                self.select_file(self.file, cx);
            }
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
        if self.stack.is_some() {
            self.reopen_stack(cx);
        } else {
            self.select_file(self.file, cx);
        }
        cx.notify();
    }

    /// The View menu: the diff mode, then the options that are on or off.
    /// The View ▾ menu (#48 A): one flat list of what the toolbar does not show, in groups.
    fn view_menu(&self) -> Vec<(Option<&'static str>, Vec<Entry>)> {
        let o = self.options;
        let tool = self.tool();
        vec![
            (
                Some("Changes"),
                DiffMode::ALL
                    .iter()
                    .map(|&mode| {
                        Entry::new(mode.label(), Act::SetMode(mode)).checked(o.mode == mode)
                    })
                    .collect(),
            ),
            (
                Some("Show"),
                vec![
                    Entry::new("Every line of the file", Act::Toggle(Opt::FullContext))
                        .key("⌥E")
                        .checked(o.full_context),
                    Entry::new("Hide whitespace changes", Act::Toggle(Opt::Whitespace))
                        .key("⌥W")
                        .checked(o.ignore_whitespace),
                    Entry::new("Wrap long lines", Act::Toggle(Opt::Wrap))
                        .key("⌥Z")
                        .checked(o.wrap),
                    Entry::new("Annotate (blame)", Act::Annotate)
                        .key("⌥⌘B")
                        .checked(self.annotate),
                ],
            ),
            (
                Some("Files"),
                vec![
                    Entry::new("One file at a time", Act::Toggle(Opt::OneFile))
                        .key("⌥A")
                        .checked(o.one_file),
                    Entry::new("Fold reviewed files", Act::Toggle(Opt::FoldReviewed))
                        .checked(o.fold_reviewed),
                    Entry::new("Mark reviewed as I scroll past", Act::AutoReviewed)
                        .checked(self.settings.auto_reviewed),
                ],
            ),
            (
                Some("Panels"),
                vec![
                    Entry::new("Fold the menu to icons", Act::FoldPlaces)
                        .checked(self.places_folded()),
                    Entry::new("Discussions", Act::Tool(tools::Tool::Discussions))
                        .key("⌘⇧R")
                        .checked(tool == Some(tools::Tool::Discussions)),
                    Entry::new("Agents", Act::Tool(tools::Tool::Agents))
                        .checked(tool == Some(tools::Tool::Agents)),
                ],
            ),
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

    /// The code's font size and line height with the zoom applied.
    fn code_size(&self) -> f32 {
        (CODE_SIZE * f32::from(self.settings.zoom) / 100.).round()
    }

    fn row_height(&self) -> f32 {
        (DIFF_ROW * f32::from(self.settings.zoom) / 100.).round()
    }

    /// ⌘+ and ⌘−: the whole interface one step bigger or smaller.
    pub(super) fn ui_zoom_by(&mut self, steps: i32, window: &mut Window, cx: &mut Context<Self>) {
        let next = i32::from(self.settings.ui_zoom) + steps * i32::from(Settings::ZOOM_STEP);
        self.set_ui_zoom(next.max(0) as u16, window, cx);
    }

    pub(super) fn set_ui_zoom(
        &mut self,
        percent: u16,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let zoom = percent.clamp(Settings::ZOOM_MIN, Settings::ZOOM_MAX);
        if zoom == self.settings.ui_zoom {
            return;
        }
        self.settings.ui_zoom = zoom;
        self.settings.save();
        apply_ui_zoom(zoom, window);
        self.char_width = 0.;
        self.relayout_all();
        cx.notify();
    }

    /// Code size only (Settings): on top of the interface size.
    pub(super) fn zoom_by(&mut self, steps: i32, cx: &mut Context<Self>) {
        let step = i32::from(Settings::ZOOM_STEP);
        let next = i32::from(self.settings.zoom) + steps * step;
        self.set_zoom(next.max(0) as u16, cx);
    }

    pub(super) fn set_zoom(&mut self, percent: u16, cx: &mut Context<Self>) {
        let zoom = percent.clamp(Settings::ZOOM_MIN, Settings::ZOOM_MAX);
        if zoom == self.settings.zoom {
            return;
        }
        self.settings.zoom = zoom;
        self.settings.save();
        // The width of a character is measured again at the new size; the rows change height.
        self.char_width = 0.;
        self.relayout_all();
        cx.notify();
    }

    /// The options the rows are laid out with: a diff under 700 px wide is unified.
    fn view_options(&self) -> ViewOptions {
        let mut options = self.options;
        options.unified |= self.narrow;
        options
    }

    /// The side panel's Changes is open and has the files as a zone.
    pub(super) fn files_zone(&self) -> bool {
        self.layout.show_sidebar
            && self.layout.show_files
            && self.diff.as_ref().is_some_and(|d| d.files.len() > 1)
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
        self.sync_stack();
        self.build_stack(cx);
        cx.notify();
    }

    fn row_style(&self) -> RowStyle<'_> {
        self.row_style_for(self.data.as_deref(), &self.notes, true)
    }

    /// How a row of a file built as `data` is drawn; the selection and blame only in the `active`
    /// file.
    fn row_style_for<'a>(
        &'a self,
        data: Option<&FileData>,
        notes: &'a [diff_view::LineNote],
        active: bool,
    ) -> RowStyle<'a> {
        let digits = data.map_or(3, |d| d.gutter_digits);
        RowStyle {
            wrap: self.options.wrap,
            offset: self.offset,
            marks: self.settings.mark_style,
            terms: &self.dterms,
            strong: false,
            sel: self.sel.filter(|_| active),
            notes,
            annot: self
                .annotate
                .then_some(if active { &self.annotations[..] } else { &[] }),
            row: self.row_height(),
            // Room for the comment mark (16 px) left of the number.
            gutter: digits.max(3) as f32 * self.char_width + 30.,
        }
    }

    /// How far the longest line can scroll before its end reaches the right edge.
    fn max_offset(&self) -> f32 {
        let Some(widest) = self.widest() else {
            return 0.;
        };
        let style = self.row_style();
        let sides = if self.view_options().unified { 1. } else { 2. };
        // A file's card is inset from the sides, and framed.
        let card = if self.stack.is_some() {
            2. * stack::CARD_MARGIN + 2.
        } else {
            0.
        };
        let visible =
            (self.body_width.get() - card - style.chrome(self.view_options().unified)) / sides;
        (widest as f32 * self.char_width + 24. - visible).max(0.)
    }

    fn scroll_horizontally(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.options.wrap {
            return;
        }
        let delta = f32::from(event.delta.pixel_delta(px(self.row_height())).x);
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
                this.update(cx, |this, cx| this.request_open(path, None, cx))
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

    /// ⌥⌘2: folds or opens the side panel's Changes.
    fn toggle_files(&mut self, _: &ToggleFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.fold_section(2, cx);
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
        // ⌥⌘3 lists every request; the menu's places narrow it.
        self.requests.scope = requests::Scope::All;
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

    /// ⌘.: zen — the diff alone, without the rail, with a line of the keys that move a review on;
    /// pressed again, the islands come back as they were.
    fn focus_diff(&mut self, _: &FocusDiff, _: &mut Window, cx: &mut Context<Self>) {
        match self.zen.take() {
            Some((sidebar, files)) => {
                self.layout.show_sidebar = sidebar;
                self.layout.show_files = files;
            }
            None => {
                self.zen = Some((self.layout.show_sidebar, self.layout.show_files));
                self.layout.show_sidebar = false;
                self.layout.show_files = false;
            }
        }
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

    fn toggle_expand_all_action(
        &mut self,
        _: &ToggleExpandAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_expand_all(cx);
    }

    fn toggle_one_file(&mut self, _: &ToggleOneFile, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle(Opt::OneFile, cx);
    }

    fn toggle_whitespace(&mut self, _: &ToggleWhitespace, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle(Opt::Whitespace, cx);
    }

    fn toggle_rich_comments_action(
        &mut self,
        _: &ToggleRichComments,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_rich_comments(cx);
    }

    /// A click compares that version with the latest; ⌘-click makes it the newer side instead.
    fn click_version(&mut self, ix: usize, event: &ClickEvent, cx: &mut Context<Self>) {
        let latest = self.versions.len() - 1;
        let (from, to) = match (self.selection, event.modifiers().secondary()) {
            (Selection::Versions { from, .. }, true) if from != ix => (from.min(ix), from.max(ix)),
            _ if ix == latest => (latest - 1, latest),
            _ => (ix, latest),
        };
        self.select_versions(from, to, cx);
    }

    // ---- resizing ------------------------------------------------------------------------

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
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .when(!commits_tab, |s| s.child(self.render_file(diff, cx)))
                    .when(commits_tab, |s| s.child(self.render_pairs(diff, cx))),
            )
            .into_any_element()
    }

    /// What the screen shows, in a few words: zen's one line on top.
    fn header_title(&self, diff: &Diff) -> String {
        if let Some(m) = self
            .requests
            .current
            .filter(|_| self.showing_request())
            .and_then(|iid| self.requests.list.iter().find(|m| m.iid == iid))
        {
            return format!("{} {}", words::number(m.iid), m.title);
        }
        match &diff.header {
            Header::Commit(c) => c.summary.clone(),
            Header::WorkingTree { .. } => "Working tree".to_owned(),
            Header::Compare { base, head, .. } => format!("{} … {}", base.0, head.0),
            Header::Interdiff { new, .. } => format!("Interdiff · {}", new.summary),
            Header::Versions { from, to, .. } => format!("v{} → v{}", from.number, to.number),
        }
    }

    /// Zen's header: one line — what is shown, the file, how far the review is.
    fn render_zen_head(&self, diff: &Diff) -> impl IntoElement + use<> {
        let file = diff.files.get(self.file).map(|f| f.path().to_owned());
        island()
            .flex_none()
            .flex_row()
            .items_center()
            .gap_3()
            .h(px(36.))
            .px_4()
            .child(
                div()
                    .max_w(px(520.))
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.header_title(diff)),
            )
            .children(file.map(|f| {
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(theme::CODE_FONT)
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(f)
            }))
            .children(self.review_progress().map(|(done, total)| {
                div()
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(format!("{done} / {total} reviewed"))
            }))
    }

    /// The header row over the panes: the screen's identity.
    fn render_header(&self, diff: &Diff, cx: &mut Context<Self>) -> impl IntoElement {
        island()
            .flex_none()
            .flex_row()
            .items_start()
            .gap_4()
            .px_4()
            .py(px(8.))
            .child(self.render_summary(diff, cx))
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
            .children(self.stack.is_some().then(|| {
                let folded = self.any_folded();
                diff_view::group().child(
                    chip(
                        "expand-all",
                        if folded { "Expand all" } else { "Collapse all" },
                        false,
                    )
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.expand_all(folded, cx)),
                    ),
                )
            }))
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
                                this.open_titled(at, groups, cx);
                            }),
                        ),
                ),
            )
    }

    /// The left of the diff's toolbar: which range is shown, as the screen offers it.
    fn render_what(&self, diff: &Diff, cx: &mut Context<Self>) -> gpui::AnyElement {
        match &diff.header {
            Header::Commit(c) => div()
                .text_size(px(12.))
                .text_color(theme::muted())
                .child(format!("{} · against its parent", format::short(c.id)))
                .into_any_element(),
            Header::WorkingTree { only_unstaged } => {
                let only = *only_unstaged;
                diff_view::group()
                    .child(chip("wt-all", "All changes", !only).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.set_only_unstaged(false, cx)
                        }),
                    ))
                    .child(chip("wt-unstaged", "Not staged only", only).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.set_only_unstaged(true, cx)),
                    ))
                    .into_any_element()
            }
            Header::Compare {
                base,
                head,
                since_merge_base,
                request,
                ..
            } => {
                let (b, h, since, req) = (base.clone(), head.clone(), *since_merge_base, *request);
                let (b2, h2) = (b.clone(), h.clone());
                let (b3, h3) = (b.clone(), h.clone());
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        diff_view::group()
                            .child(chip("cmp-since", "Since merge base", since).on_click(
                                cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.run_compare(b.clone(), h.clone(), true, req, cx)
                                }),
                            ))
                            .child(chip("cmp-direct", "Direct", !since).on_click(cx.listener(
                                move |this, _: &ClickEvent, _, cx| {
                                    this.run_compare(b2.clone(), h2.clone(), false, req, cx)
                                },
                            ))),
                    )
                    .child(chip("cmp-swap", "⇄ Swap", false).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.run_compare(h3.clone(), b3.clone(), since, None, cx)
                        },
                    )))
                    .into_any_element()
            }
            Header::Interdiff { from, to, .. } => {
                let (from, to) = (*from, *to);
                chip("back-to-commits", "← Commits", false)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.select_versions(from, to, cx)
                    }))
                    .into_any_element()
            }
            Header::Versions { .. } => diff_view::group()
                .child(
                    chip("tab-files", "Files", self.version_tab == VersionTab::Files).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.version_tab = VersionTab::Files;
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    chip(
                        "tab-commits",
                        format!("Commits · {}", diff.pairs.len()),
                        self.version_tab == VersionTab::Commits,
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.version_tab = VersionTab::Commits;
                        cx.notify();
                    })),
                )
                .into_any_element(),
        }
    }

    /// The diff's toolbar, read left to right: what is shown, then how it is drawn.
    fn render_diff_toolbar(&self, diff: &Diff, cx: &mut Context<Self>) -> impl IntoElement {
        // A narrow diff wraps it: how it is drawn goes under what is shown, never out of view.
        div()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .min_h(px(40.))
            .px(px(12.))
            .py(px(6.))
            .border_b_1()
            .border_color(theme::island_border())
            .child(self.render_what(diff, cx))
            .child(div().flex_1())
            .child(self.render_toolbar(cx))
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
                                    .child(if only {
                                        "Working tree against the index"
                                    } else {
                                        "Working tree against HEAD"
                                    }),
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
                stale,
            } => {
                let (since, req) = (*since_merge_base, *request);
                let (h4, b5, h5) = (head.clone(), base.clone(), head.clone());
                // Why the result looks wrong, with the one click that fixes it.
                let warning = |text: String, button: gpui::Stateful<gpui::Div>| {
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_3()
                        .py(px(6.))
                        .rounded(px(ROW_RADIUS))
                        .bg(theme::warning_bg())
                        .text_size(px(12.))
                        .text_color(theme::warning())
                        .child(div().flex_1().min_w_0().child(text))
                        .child(
                            button
                                .flex_none()
                                .bg(theme::hover())
                                .text_color(theme::text()),
                        )
                };
                let stale_note = stale.clone().map(|st| {
                    let upstream = (st.upstream.clone(), st.tip);
                    let h = h4.clone();
                    warning(
                        format!(
                            "{} is {} behind {}: their changes show up here too",
                            base.0,
                            plural(st.behind, "commit"),
                            st.upstream
                        ),
                        chip("cmp-upstream", format!("Use {}", st.upstream), false).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.run_compare(upstream.clone(), h.clone(), since, req, cx)
                            }),
                        ),
                    )
                });
                let empty_note = (since && *commits == 0).then(|| {
                    warning(
                        format!(
                            "{} has nothing {} lacks: are the sides swapped?",
                            head.0, base.0
                        ),
                        chip("cmp-swap-hint", "⇄ Swap", false).on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                this.run_compare(h5.clone(), b5.clone(), since, None, cx)
                            },
                        )),
                    )
                });
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
                            ),
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
                    .children(stale_note)
                    .children(empty_note)
            }
            Header::Interdiff { old, new, .. } => header
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
                ),
            Header::Versions {
                from,
                to,
                rebased,
                conflicts,
            } => header
                .child(
                    div().flex().items_center().gap_3().child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("Changes from v{} to v{}", from.number, to.number)),
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
            _ if self.stack.is_some() => self.render_rows(cx).into_any_element(),
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
            (Some(file), _) if self.shows_image(file) => self.render_image(file, cx),
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
            .child(self.render_diff_toolbar(diff, cx))
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
                    .child(change_badge(f.change))
                    .child(div().flex_1().min_w_0().truncate().child(path))
                    // An SVG read as its source can go back to the picture.
                    .children(
                        (self.images.source && image_view::is_svg(f))
                            .then(|| image_view::source_switch(true, cx)),
                    )
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
            // Painted last, over the rows that come after it.
            .children(
                self.render_goline(cx.entity().downgrade())
                    .map(|prompt| gpui::deferred(prompt).with_priority(1)),
            )
            .children(
                self.render_done_card(cx)
                    .map(|card| gpui::deferred(card).with_priority(1)),
            )
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
                    .when(self.stack.is_none(), |s| s.px(px(4.)).pt(px(4.)).pb(px(6.)))
                    .when(self.stack.is_some(), |s| {
                        s.bg(theme::base()).rounded_b(px(ISLAND_RADIUS))
                    })
                    .child(body),
            )
    }
}

impl Workspace {
    /// The selected file's rows, scrolled vertically by the list and horizontally by `offset`.
    fn render_rows(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let outer = this.clone();
        let row = move |ix: usize, cx: &mut App| -> gpui::AnyElement {
            let Some(workspace) = this.upgrade() else {
                return div().into_any_element();
            };
            let workspace = workspace.read(cx);
            // With all files in one scroll: the file of the row, and its place among the file's
            // rows (the active file's are the workspace's own).
            let (file, ix) = match &workspace.stack {
                Some(stack) => {
                    let (b, inside) = stack.locate(ix);
                    let file = stack.blocks[b].file;
                    if inside == 0 {
                        return workspace.render_block_header(file, this.clone());
                    }
                    if workspace.body(b) != stack::Body::Rows {
                        return workspace.render_block_note(b, file, this.clone());
                    }
                    (Some((b, file)), inside - 1)
                }
                None => (None, ix),
            };
            let active = file.is_none_or(|(_, f)| f == workspace.file);
            let (data, row, mut style) = match file {
                Some((b, _)) if !active => {
                    let Some(stack) = &workspace.stack else {
                        return div().into_any_element();
                    };
                    let block = &stack.blocks[b];
                    let (Some(data), Some(&row)) = (block.data.clone(), block.rows.get(ix)) else {
                        return div().into_any_element();
                    };
                    let style = workspace.row_style_for(Some(&data), &block.notes, false);
                    (data, row, style)
                }
                _ => {
                    let (Some(data), Some(&row)) = (workspace.data.clone(), workspace.rows.get(ix))
                    else {
                        return div().into_any_element();
                    };
                    (data, row, workspace.row_style())
                }
            };
            style.strong = match (&workspace.stack, file) {
                (Some(stack), Some((_, f))) => stack.matches.get(stack.match_at) == Some(&(f, ix)),
                _ => workspace.matches.get(workspace.match_at) == Some(&ix),
            };
            let indent = style.gutter + 8.;
            match row {
                Row::Thread(id) => return workspace.render_thread(id, indent, this.clone()),
                Row::Request(ix) => {
                    return workspace.render_request_thread(ix, indent, this.clone());
                }
                Row::Draft(ix) => return workspace.render_draft(ix, indent, this.clone()),
                Row::Composer if workspace.compose.is_none() && active => {
                    return workspace.render_goline_row(indent, this.clone());
                }
                Row::Composer if workspace.compose.is_none() => return div().into_any_element(),
                Row::Composer => return workspace.render_composer(indent, this.clone()),
                _ => {}
            }
            let marked = active && workspace.goline.is_some() && workspace.goline_row == Some(ix);
            // A press in another file makes it the active one first.
            let file = file.map(|(_, f)| f).filter(|_| !active);
            let touch = move |this: &mut Self, cx: &mut Context<Self>| {
                if let Some(file) = file {
                    this.activate_file(file, cx);
                }
            };
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
                        .update(cx, |this, cx| {
                            touch(this, cx);
                            this.open_annotated_commit(line, cx)
                        })
                        .ok();
                }),
                toggle: Box::new(move |old, line, cx| {
                    toggle
                        .update(cx, |this, cx| {
                            touch(this, cx);
                            this.toggle_notes_at(old, line, cx)
                        })
                        .ok();
                }),
                comment: Box::new(move |old, line, cx| {
                    comment
                        .update(cx, |this, cx| {
                            touch(this, cx);
                            this.start_comment(old, line, cx)
                        })
                        .ok();
                }),
                press: Box::new(move |old, line, byte, clicks, shift, cx| {
                    press
                        .update(cx, |this, cx| {
                            touch(this, cx);
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
                        .update(cx, |this, cx| {
                            touch(this, cx);
                            this.line_context(old, line, at, cx)
                        })
                        .ok();
                }),
            };
            let element = diff_view::row(
                &data,
                row,
                style,
                move |segment, cx| {
                    expand
                        .update(cx, |this, cx| {
                            touch(this, cx);
                            this.expand_gap(segment, cx)
                        })
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
        };
        // With all files in one scroll, a file's rows are framed as its card.
        let rows = list(self.diff_list.clone(), move |ix, _, cx| {
            let element = row(ix, cx);
            let Some(workspace) = outer.upgrade() else {
                return element;
            };
            let workspace = workspace.read(cx);
            match &workspace.stack {
                Some(stack) if stack.locate(ix).1 > 0 => {
                    let file = stack.blocks[stack.locate(ix).0].file;
                    workspace.card_row(element, file, stack.last_of_block(ix))
                }
                _ => element,
            }
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
            // The ground between the files' cards.
            .when(self.stack.is_some(), |s| s.bg(theme::base()))
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .on_scroll_wheel(cx.listener(|this, event, _, cx| this.scroll_horizontally(event, cx)))
            .font_family(theme::CODE_FONT)
            .text_size(px(self.code_size()))
            .line_height(px(self.row_height()))
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
/// A length in logical pixels at the interface size the reader chose (⌘+ ⌘−): every size in the
/// window goes through here, so the whole of it grows and shrinks together.
pub(crate) fn px(value: f32) -> gpui::Pixels {
    gpui::px(value * f32::from(UI_ZOOM.load(std::sync::atomic::Ordering::Relaxed)) / 100.)
}

/// Makes `percent` the interface size: `px` and the window's rem (what `px_2`, `gap_3` … use).
pub(crate) fn apply_ui_zoom(percent: u16, window: &mut Window) {
    UI_ZOOM.store(percent, std::sync::atomic::Ordering::Relaxed);
    window.set_rem_size(gpui::px(16. * f32::from(percent) / 100.));
}

/// The interface size, in percent; read by `px`.
static UI_ZOOM: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(100);

fn island() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(if flat() { 0. } else { ISLAND_RADIUS }))
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
    let text: SharedString = text.into();
    section_label(SharedString::from(words::say(&text).into_owned()))
        .px(px(14.))
        .pt(px(10.))
        .pb(px(6.))
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
            .child(SharedString::from(keys::text(&self.0).into_owned()))
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
            .child(keys::text(self.0).into_owned())
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
                .child(keys::text(shortcut).into_owned()),
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
