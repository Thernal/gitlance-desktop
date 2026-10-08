//! The GitLance window: branches, versions and commits on the left, the diff on the right.

mod diff_view;
mod find;
mod format;
mod icons;
mod rows;
mod settings;
mod theme;
mod watch;

use crate::git::{
    BranchRef, ChangeKind, CommitInfo, DiffSettings, FileDiff, RefKind, Repo, Version,
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
use rows::{FileData, Row};
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
        OpenSettings,
        Find,
        ToggleWrap,
        ToggleFullContext,
        ToggleWhitespace,
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
                KeyBinding::new("up", PreviousCommit, Some("Workspace")),
                KeyBinding::new("down", NextCommit, Some("Workspace")),
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
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.set_menus(menus(&ViewOptions::load(&Settings::load())));

            let bounds = gpui::Bounds::centered(None, size(px(1480.), px(920.)), cx);
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
                    cx.new(|cx| Workspace::new(path, window, cx))
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

/// The menu bar; View items carry a check mark for the options in effect.
fn menus(options: &ViewOptions) -> Vec<Menu> {
    vec![
        Menu::new("GitLance").items([
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit GitLance", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Open Repository…", Open),
            MenuItem::action("Refresh", Refresh),
            MenuItem::separator(),
            MenuItem::action("Find Commits…", Find),
        ]),
        Menu::new("View").items([
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
#[derive(Clone, Copy)]
enum Opt {
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
fn snapshot(window: gpui::WindowHandle<Workspace>, cx: &mut App) {
    use std::time::Duration;
    let Some(out) = std::env::var_os("GITLANCE_SNAPSHOT").map(PathBuf::from) else {
        return;
    };
    let versions = std::env::var_os("GITLANCE_SNAPSHOT_VERSIONS").is_some();
    cx.spawn(async move |cx| {
        let executor = cx.background_executor().clone();
        let wait = |ms| executor.timer(Duration::from_millis(ms));
        let delay = std::env::var("GITLANCE_SNAPSHOT_DELAY_MS")
            .ok()
            .and_then(|d| d.parse().ok())
            .unwrap_or(2500);
        wait(delay).await;
        if let Ok(prefix) = std::env::var("GITLANCE_SNAPSHOT_COMMIT") {
            window
                .update(cx, |this, _, cx| {
                    let found = this
                        .commits
                        .iter()
                        .position(|c| c.id.to_string().starts_with(&prefix));
                    if let Some(ix) = found {
                        this.select_commit(ix, cx);
                    }
                })
                .ok();
            wait(1500).await;
        }
        if let Ok(path) = std::env::var("GITLANCE_SNAPSHOT_FILE") {
            window
                .update(cx, |this, _, cx| {
                    let found = this
                        .diff
                        .as_ref()
                        .and_then(|d| d.files.iter().position(|f| f.path() == path));
                    if let Some(ix) = found {
                        this.select_file(ix, cx);
                    }
                })
                .ok();
            wait(1500).await;
        }
        if let Ok(view) = std::env::var("GITLANCE_SNAPSHOT_VIEW") {
            window
                .update(cx, |this, _, cx| {
                    for name in view.split(',') {
                        let opt = match name.trim() {
                            "unified" => Opt::Unified,
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
                })
                .ok();
            wait(1500).await;
        }
        if let Ok(text) = std::env::var("GITLANCE_SNAPSHOT_SEARCH") {
            window
                .update(cx, |this, _, cx| {
                    this.find.text = Some(text);
                    this.refilter();
                    cx.notify();
                })
                .ok();
            wait(800).await;
        }
        if versions {
            window
                .update(cx, |this, _, cx| {
                    if this.versions.len() > 1 {
                        this.select_versions(0, this.versions.len() - 1, cx);
                    }
                })
                .ok();
            wait(1500).await;
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
}

enum Header {
    Commit(CommitInfo),
    Versions {
        from: Version,
        to: Version,
        rebased: bool,
        conflicts: usize,
    },
}

struct Diff {
    header: Header,
    files: Arc<Vec<FileDiff>>,
}

/// A pane boundary that can be dragged.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Split {
    Sidebar,
    Files,
    Branches,
}

impl Split {
    fn vertical(self) -> bool {
        self == Split::Branches
    }

    fn bounds(self) -> (f32, f32) {
        match self {
            Split::Sidebar => (220., 720.),
            Split::Files => (160., 640.),
            Split::Branches => (48., 640.),
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
    diff: Option<Diff>,
    file: usize,
    options: ViewOptions,
    settings: Settings,
    settings_open: bool,
    /// The repository menu in the title bar is open.
    repo_menu: bool,
    find: find::Find,
    watch: watch::Watch,
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
    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let recent = storage::recent();
        let settings = Settings::load();
        let start = path.or_else(|| recent.first().cloned());
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
            diff: None,
            file: 0,
            options: ViewOptions::load(&settings),
            settings,
            settings_open: false,
            repo_menu: false,
            find: find::Find::default(),
            watch: watch::Watch::default(),
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
                    anyhow::Ok((repo.root(), repo.branches()?, git_dir, fingerprint))
                })
                .await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok((root, branches, git_dir, fingerprint)) => {
                        this.recent = storage::remember(&root);
                        if this.root.as_ref() != Some(&root) {
                            this.find.reset();
                        }
                        this.watch.watch(git_dir, fingerprint);
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
        self.selection = Selection::None;
        self.diff = None;
        self.diff_task = None;
        self.clear_file();
        self.file = 0;
    }

    fn clear_file(&mut self) {
        self.data = None;
        self.view_task = None;
        self.rows.clear();
        self.expanded.clear();
        self.offset = 0.;
        self.diff_list.reset(0);
    }

    fn select_branch(&mut self, ix: usize, keep: Option<git2::Oid>, cx: &mut Context<Self>) {
        let (Some(root), Some(branch)) = (self.root.clone(), self.branches.get(ix)) else {
            return;
        };
        let (refname, tip) = (branch.refname.clone(), branch.tip);
        self.clear_branch();
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
        self.clear_diff();
        self.selection = Selection::Versions { from, to };
        let settings = self.settings();
        self.load_diff(keep, cx, move || {
            let diff = Repo::open(&root)?.version_diff(&a, &b, settings)?;
            Ok(Diff {
                header: Header::Versions {
                    from: a,
                    to: b,
                    rebased: diff.rebased,
                    conflicts: diff.conflicts.len(),
                },
                files: Arc::new(diff.files),
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
                        let ix = keep
                            .and_then(|path| diff.files.iter().position(|f| f.path() == path))
                            .unwrap_or(0);
                        let empty = diff.files.is_empty();
                        this.diff = Some(diff);
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
        self.clear_file();
        self.file_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        let mode = self.options.mode;
        self.view_task = Some(cx.spawn(async move |this, cx| {
            let data = cx
                .background_executor()
                .spawn(async move { FileData::build(&file, mode) })
                .await;
            this.update(cx, |this, cx| {
                this.data = Some(Arc::new(data));
                this.relayout();
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Lays the selected file out again for the current options; the diff scrolls to the top.
    fn relayout(&mut self) {
        self.rows = self
            .data
            .as_ref()
            .map(|data| rows::layout(data, &self.options, &self.expanded))
            .unwrap_or_default();
        self.diff_list.reset(self.rows.len());
        self.offset = self.offset.min(self.max_offset());
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
        let rows = rows::layout(&data, &self.options, &self.expanded);
        let inserted = rows.len() + 1 - self.rows.len();
        self.rows = rows;
        self.diff_list.splice(at..at + 1, inserted);
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
            Selection::None => {}
        }
    }

    fn row_style(&self) -> RowStyle {
        let digits = self.data.as_ref().map_or(3, |d| d.gutter_digits);
        RowStyle {
            wrap: self.options.wrap,
            offset: self.offset,
            marks: self.settings.mark_style,
            gutter: digits as f32 * self.char_width + 16.,
        }
    }

    /// How far the longest line can scroll before its end reaches the right edge.
    fn max_offset(&self) -> f32 {
        let Some(data) = &self.data else {
            return 0.;
        };
        let style = self.row_style();
        let sides = if self.options.unified { 1. } else { 2. };
        let visible = (self.body_width.get() - style.chrome(self.options.unified)) / sides;
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
        if let Selection::Commit(ix) = self.selection
            && let Some(row) = self.row_of(ix)
            && row > 0
        {
            self.select_commit(self.commit_at(row - 1), cx);
        }
    }

    fn next_commit(&mut self, _: &NextCommit, _: &mut Window, cx: &mut Context<Self>) {
        match self.selection {
            Selection::Commit(ix) => {
                if let Some(row) = self.row_of(ix)
                    && row + 1 < self.commit_rows()
                {
                    self.select_commit(self.commit_at(row + 1), cx);
                }
            }
            Selection::None if self.commit_rows() > 0 => self.select_commit(self.commit_at(0), cx),
            _ => {}
        }
    }

    fn previous_file(&mut self, _: &PreviousFile, _: &mut Window, cx: &mut Context<Self>) {
        if self.file > 0 {
            self.select_file(self.file - 1, cx);
        }
    }

    fn next_file(&mut self, _: &NextFile, _: &mut Window, cx: &mut Context<Self>) {
        let count = self.diff.as_ref().map_or(0, |d| d.files.len());
        if self.file + 1 < count {
            self.select_file(self.file + 1, cx);
        }
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

    fn find_action(&mut self, _: &Find, _: &mut Window, cx: &mut Context<Self>) {
        self.start_find(cx);
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
            Split::Branches => &mut self.layout.branches,
        }
    }

    fn start_drag(&mut self, split: Split, event: &MouseDownEvent, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if event.click_count == 2 {
            *self.size(split) = match split {
                Split::Sidebar => Layout::default().sidebar,
                Split::Files => Layout::default().files,
                Split::Branches => Layout::default().branches,
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
            Split::Branches => "split-branches",
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
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_color(theme::muted())
                            .child(icons::icon("branch").text_color(theme::muted()))
                            .child(b)
                    })),
            )
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
                    .child("Open a repository to browse its commits and branch versions."),
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
        let branches = self.branches.iter().enumerate().map(|(ix, b)| {
            let selected = self.branch == Some(ix);
            row(("branch", ix), selected)
                .h(px(26.))
                .gap_2()
                .child(
                    div()
                        .w(px(8.))
                        .text_color(theme::accent())
                        .child(if b.is_head { "●" } else { "" }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(match b.kind {
                            RefKind::Local => theme::text(),
                            RefKind::Remote => theme::muted(),
                        })
                        .child(b.name.clone()),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.select_branch(ix, None, cx)))
        });

        div()
            .w(px(self.layout.sidebar))
            .flex_none()
            .flex()
            .flex_col()
            .child(
                island()
                    .h(px(self.layout.branches))
                    .child(island_label("Branches"))
                    .child(
                        div()
                            .id("branches")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(6.))
                            .pb(px(6.))
                            .children(branches.map(|b| div().py(px(1.)).child(b))),
                    ),
            )
            .child(self.handle(Split::Branches, cx))
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
                    .min_h_0()
                    .child(island_label(format!("Commits · {}", self.commits.len())))
                    .child(self.render_find(cx))
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

    fn render_versions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let latest = self.versions.len() - 1;
        let (from, to) = match self.selection {
            Selection::Versions { from, to } => (Some(from), Some(to)),
            _ => (None, None),
        };
        island()
            .pb(px(6.))
            .child(island_label(format!("Versions · {}", self.versions.len())))
            .children(self.versions.iter().enumerate().rev().map(|(ix, v)| {
                let role = if Some(ix) == from {
                    Some("from")
                } else if Some(ix) == to {
                    Some("to")
                } else {
                    None
                };
                let item = row(("version", ix), role.is_some())
                    .h(px(28.))
                    .gap_2()
                    .child(
                        div()
                            .w(px(28.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::accent())
                            .child(format!("v{}", v.number)),
                    )
                    .child(
                        div()
                            .font_family(theme::CODE_FONT)
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(format::short(v.tip)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(theme::muted())
                            .child(format!(
                                "{} · {} · {}",
                                format::reason(&v.reason),
                                plural(v.commits, "commit"),
                                format::ago(v.time)
                            )),
                    )
                    .children(role.map(|r| tag(r, theme::accent())))
                    .when(ix == latest && role.is_none(), |s| {
                        s.child(tag("latest", theme::faint()))
                    })
                    .when(self.watch.new_from.is_some_and(|from| ix >= from), |s| {
                        s.child(tag("new", theme::accent()))
                    })
                    .on_click(
                        cx.listener(move |this, event, _, cx| this.click_version(ix, event, cx)),
                    );
                div().px(px(6.)).py(px(1.)).child(item)
            }))
    }

    fn render_commit_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let commit = &self.commits[ix];
        let selected = self.selection == Selection::Commit(ix);
        let item = row(("commit", ix), selected)
            .h(px(46.))
            .flex_col()
            .items_start()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .truncate()
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
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(commit.author.clone()),
                    )
                    .child(format::ago(commit.time)),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.select_commit(ix, cx)));
        div().w_full().h(px(48.)).px(px(6.)).py(px(1.)).child(item)
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
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(self.render_header(diff, cx))
            .child(div().h(px(GAP)).flex_none())
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_files(diff, cx))
                    .child(self.handle(Split::Files, cx))
                    .child(self.render_file(diff, cx)),
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
            .py_3()
            .child(self.render_summary(diff))
            .child(self.render_toolbar(cx))
    }

    /// The diff options, as chips; the same toggles live in the View menu.
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let set = |opt: Opt, value: bool| {
            cx.listener(move |this: &mut Self, _: &ClickEvent, _: &mut Window, cx| {
                this.set_option(opt, value, cx)
            })
        };
        let toggle = |opt: Opt| {
            cx.listener(move |this: &mut Self, _: &ClickEvent, _: &mut Window, cx| {
                this.toggle(opt, cx)
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
            .child(diff_view::group().children(DiffMode::ALL.map(|mode| {
                chip(mode.label(), mode.label(), o.mode == mode).on_click(cx.listener(
                    move |this: &mut Self, _: &ClickEvent, _: &mut Window, cx| {
                        this.set_mode(mode, cx)
                    },
                ))
            })))
            .child(
                diff_view::group()
                    .child(chip("wrap", "Wrap", o.wrap).on_click(toggle(Opt::Wrap)))
                    .child(
                        chip("all-lines", "All lines", o.full_context)
                            .on_click(toggle(Opt::FullContext)),
                    )
                    .child(
                        chip("whitespace", "Hide whitespace", o.ignore_whitespace)
                            .on_click(toggle(Opt::Whitespace)),
                    ),
            )
    }

    fn render_summary(&self, diff: &Diff) -> impl IntoElement {
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
                let body = commit
                    .message
                    .split_once('\n')
                    .map(|(_, body)| body.trim().to_owned())
                    .filter(|b| !b.is_empty());
                header
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(commit.summary.clone()),
                    )
                    .children(body.map(|b| {
                        div()
                            .max_h(px(120.))
                            .overflow_hidden()
                            .text_color(theme::muted())
                            .child(b)
                    }))
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(
                                div()
                                    .font_family(theme::CODE_FONT)
                                    .child(commit.id.to_string()),
                            )
                            .child(format!("{} <{}>", commit.author, commit.email))
                            .child(format::date(commit.time))
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
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("Changes from v{} to v{}", from.number, to.number)),
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

    fn render_files(&self, diff: &Diff, cx: &mut Context<Self>) -> impl IntoElement {
        island()
            .w(px(self.layout.files))
            .flex_none()
            .child(island_label(plural(diff.files.len(), "file")))
            .child(
                uniform_list(
                    "files",
                    diff.files.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let Some(diff) = &this.diff else {
                            return Vec::new();
                        };
                        range
                            .map(|ix| render_file_row(&diff.files[ix], ix, this.file == ix, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.file_scroll)
                .flex_1()
                .pb(px(6.)),
            )
    }

    fn render_file(&self, diff: &Diff, cx: &mut Context<Self>) -> impl IntoElement {
        let file = diff.files.get(self.file);
        let body = match (file, &self.data) {
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
            .flex_1()
            .min_w_0()
            .bg(theme::editor())
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
                    .children(
                        (!f.hunks.is_empty())
                            .then_some(f.note.clone())
                            .flatten()
                            .map(|n| div().text_color(theme::warning()).child(n)),
                    )
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
            let style = workspace.row_style();
            let this = this.clone();
            diff_view::row(&data, row, style, move |segment, cx| {
                this.update(cx, |this, cx| this.expand_gap(segment, cx))
                    .ok();
            })
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
        if self.char_width == 0. {
            let text = window.text_system();
            let id = text.resolve_font(&font(theme::CODE_FONT));
            self.char_width = text
                .advance(id, px(CODE_SIZE), 'm')
                .map_or(7.2, |size| f32::from(size.width));
        }

        div()
            .id("workspace")
            .key_context(if self.find.text.is_some() {
                "Workspace Typing"
            } else {
                "Workspace"
            })
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
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.find_key(event, cx)))
            .on_action(cx.listener(Self::toggle_wrap))
            .on_action(cx.listener(Self::toggle_full_context))
            .on_action(cx.listener(Self::toggle_whitespace))
            .on_mouse_move(cx.listener(|this, event, _, cx| this.drag_move(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_drag(cx)),
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
                    .child(self.render_sidebar(cx))
                    .child(self.handle(Split::Sidebar, cx))
                    .child(self.render_diff(cx))
                    .into_any_element(),
            })
            // Last, so it paints over everything.
            .children(self.repo_menu.then(|| self.render_repo_menu(cx)))
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

fn render_file_row(
    file: &FileDiff,
    ix: usize,
    selected: bool,
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
        .child(
            div()
                .flex()
                .gap_1()
                .text_size(px(11.))
                .when(file.added > 0, |s| {
                    s.child(
                        div()
                            .text_color(theme::added())
                            .child(format!("+{}", file.added)),
                    )
                })
                .when(file.removed > 0, |s| {
                    s.child(
                        div()
                            .text_color(theme::removed())
                            .child(format!("−{}", file.removed)),
                    )
                }),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.select_file(ix, cx)));
    div().w_full().h(px(42.)).px(px(6.)).py(px(1.)).child(item)
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
        .size(px(28.))
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

/// A tooltip: a small panel with a line of text.
struct Tip(&'static str);

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
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}
