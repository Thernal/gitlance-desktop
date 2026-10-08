//! The GitLance window: branches, versions and commits on the left, the diff on the right.

mod format;
mod rows;
mod theme;

use crate::git::{BranchRef, ChangeKind, CommitInfo, FileDiff, LineKind, RefKind, Repo, Version};
use crate::storage::{self, Layout};
use gpui::{
    App, ClickEvent, Context, CursorStyle, FocusHandle, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent, PathPromptOptions,
    Point, Rgba, ScrollStrategy, SharedString, StyledText, Task, TitlebarOptions,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions, actions, div, prelude::*, px,
    size, uniform_list,
};
use rows::{Cell, FileView, Row};
use std::path::PathBuf;
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
        Quit
    ]
);

/// How much history a branch shows.
const COMMIT_LIMIT: usize = 5000;
const DIFF_ROW: f32 = 20.;
const TITLE_BAR: f32 = 38.;
const TAB_WIDTH: usize = 4;
/// Space between islands; the resize handles live in it.
const GAP: f32 = 8.;
const ISLAND_RADIUS: f32 = 10.;
const ROW_RADIUS: f32 = 6.;

pub fn run(path: Option<PathBuf>) {
    gpui_platform::application().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("cmd-o", Open, None),
            KeyBinding::new("cmd-r", Refresh, None),
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("up", PreviousCommit, Some("Workspace")),
            KeyBinding::new("down", NextCommit, Some("Workspace")),
            KeyBinding::new("k", PreviousCommit, Some("Workspace")),
            KeyBinding::new("j", NextCommit, Some("Workspace")),
            KeyBinding::new("left", PreviousFile, Some("Workspace")),
            KeyBinding::new("right", NextFile, Some("Workspace")),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus([
            Menu::new("GitLance").items([MenuItem::action("Quit GitLance", Quit)]),
            Menu::new("File").items([
                MenuItem::action("Open Repository…", Open),
                MenuItem::action("Refresh", Refresh),
            ]),
        ]);

        let bounds = gpui::Bounds::centered(None, size(px(1480.), px(920.)), cx);
        let options = WindowOptions {
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
        cx.activate(true);
        #[cfg(feature = "snapshot")]
        snapshot(window, cx);
        #[cfg(not(feature = "snapshot"))]
        let _ = window;
    });
}

/// Waits for loading (`GITLANCE_SNAPSHOT_DELAY_MS`, 2500 by default), optionally compares the first
/// and last versions (`GITLANCE_SNAPSHOT_VERSIONS=1`), then writes the frame to `GITLANCE_SNAPSHOT`
/// and quits.
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
        window
            .update(cx, |_, window, _| {
                window.refresh();
            })
            .ok();
        wait(300).await;
        let saved = window.update(cx, |_, window, _| {
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
    view: Option<FileView>,
    commit_scroll: UniformListScrollHandle,
    file_scroll: UniformListScrollHandle,
    diff_scroll: UniformListScrollHandle,
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
            view: None,
            commit_scroll: UniformListScrollHandle::new(),
            file_scroll: UniformListScrollHandle::new(),
            diff_scroll: UniformListScrollHandle::new(),
            repo_task: None,
            branch_task: None,
            diff_task: None,
            view_task: None,
        };
        if let Some(path) = start {
            this.open_repo(path, None, cx);
        }
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
                    anyhow::Ok((repo.root(), repo.branches()?))
                })
                .await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok((root, branches)) => {
                        this.recent = storage::remember(&root);
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
        self.branch = None;
        self.versions.clear();
        self.commits.clear();
        self.clear_diff();
    }

    fn clear_diff(&mut self) {
        self.selection = Selection::None;
        self.diff = None;
        self.diff_task = None;
        self.view = None;
        self.view_task = None;
        self.file = 0;
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
        let (Some(root), Some(commit)) = (self.root.clone(), self.commits.get(ix).cloned()) else {
            return;
        };
        self.clear_diff();
        self.selection = Selection::Commit(ix);
        self.commit_scroll
            .scroll_to_item(ix, ScrollStrategy::Nearest);
        let id = commit.id;
        self.load_diff(cx, move || {
            let files = Repo::open(&root)?.commit_diff(id)?;
            Ok(Diff {
                header: Header::Commit(commit),
                files: Arc::new(files),
            })
        });
    }

    fn select_versions(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let (Some(root), Some(a), Some(b)) = (
            self.root.clone(),
            self.versions.get(from).cloned(),
            self.versions.get(to).cloned(),
        ) else {
            return;
        };
        self.clear_diff();
        self.selection = Selection::Versions { from, to };
        self.load_diff(cx, move || {
            let diff = Repo::open(&root)?.version_diff(&a, &b)?;
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

    fn load_diff(
        &mut self,
        cx: &mut Context<Self>,
        load: impl FnOnce() -> anyhow::Result<Diff> + Send + 'static,
    ) {
        self.diff_task = Some(cx.spawn(async move |this, cx| {
            let loaded = cx.background_executor().spawn(async move { load() }).await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok(diff) => {
                        let empty = diff.files.is_empty();
                        this.diff = Some(diff);
                        this.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
                        if !empty {
                            this.select_file(0, cx);
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
        self.view = None;
        self.file_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        self.diff_scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.view_task = Some(cx.spawn(async move |this, cx| {
            let view = cx
                .background_executor()
                .spawn(async move { FileView::build(&file) })
                .await;
            this.update(cx, |this, cx| {
                this.view = Some(view);
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
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
        self.open_repo(root, keep, cx);
    }

    fn previous_commit(&mut self, _: &PreviousCommit, _: &mut Window, cx: &mut Context<Self>) {
        if let Selection::Commit(ix) = self.selection
            && ix > 0
        {
            self.select_commit(ix - 1, cx);
        }
    }

    fn next_commit(&mut self, _: &NextCommit, _: &mut Window, cx: &mut Context<Self>) {
        match self.selection {
            Selection::Commit(ix) if ix + 1 < self.commits.len() => self.select_commit(ix + 1, cx),
            Selection::None if !self.commits.is_empty() => self.select_commit(0, cx),
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
        let repo = self
            .root
            .as_ref()
            .and_then(|r| r.file_name())
            .map(|n| n.to_string_lossy().into_owned());
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
            .pl(px(84.))
            .pr(px(GAP))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(repo.unwrap_or_else(|| "GitLance".to_owned())),
            )
            .children(branch.map(|b| div().text_color(theme::muted()).child(b)))
            .child(div().flex_1())
            .when(self.root.is_some(), |s| {
                s.child(button("refresh", "Refresh", "⌘R").on_click(
                    cx.listener(|this, _, window, cx| this.refresh(&Refresh, window, cx)),
                ))
            })
            .child(
                button("open", "Open…", "⌘O")
                    .on_click(cx.listener(|this, _, window, cx| this.open(&Open, window, cx))),
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
                    .child(
                        uniform_list(
                            "commits",
                            self.commits.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|ix| this.render_commit_row(ix, cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.commit_scroll)
                        .flex_1()
                        .pb(px(6.)),
                    ),
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
            .child(div().w_full().truncate().child(commit.summary.clone()))
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
            .child(self.render_header(diff))
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

    fn render_header(&self, diff: &Diff) -> impl IntoElement {
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
        let header = island().flex_none().gap_1().px_4().py_3();
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
        let body = match (file, &self.view) {
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
            (Some(_), Some(view)) => uniform_list(
                "diff",
                view.rows.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                    let Some(view) = &this.view else {
                        return Vec::new();
                    };
                    range
                        .map(|ix| render_row(view, view.rows[ix]))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.diff_scroll)
            .flex_1()
            .font_family(theme::CODE_FONT)
            .text_size(px(12.))
            .into_any_element(),
        };
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

        div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::previous_commit))
            .on_action(cx.listener(Self::next_commit))
            .on_action(cx.listener(Self::previous_file))
            .on_action(cx.listener(Self::next_file))
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

fn render_row(view: &FileView, row: Row) -> impl IntoElement + use<> {
    let line = div().h(px(DIFF_ROW)).w_full().flex();
    match row {
        Row::Hunk {
            old_start,
            new_start,
        } => line
            .items_center()
            .px_3()
            .rounded(px(ROW_RADIUS))
            .bg(theme::hover())
            .text_color(theme::faint())
            .child(format!("@@ −{old_start} +{new_start} @@"))
            .into_any_element(),
        Row::Lines { left, right } => line
            .child(render_cell(view, left, true))
            .child(div().w(px(1.)).h_full().bg(theme::border()))
            .child(render_cell(view, right, false))
            .into_any_element(),
    }
}

fn render_cell(view: &FileView, cell: Option<Cell>, old: bool) -> impl IntoElement + use<> {
    let half = div().flex_1().min_w_0().h_full().flex().overflow_hidden();
    let gutter_width = px(view.gutter_digits as f32 * 7.5 + 16.);
    let Some(cell) = cell else {
        return half.bg(theme::panel());
    };
    let (text, spans) = if old {
        view.old.line(cell.line)
    } else {
        view.new.line(cell.line)
    };
    let (text, spans) = rows::expand_tabs(text, spans, TAB_WIDTH);
    let highlights: Vec<_> = spans
        .into_iter()
        .map(|s| {
            (
                s.range,
                HighlightStyle {
                    color: Some(theme::code(s.color)),
                    font_style: s.italic.then_some(FontStyle::Italic),
                    ..Default::default()
                },
            )
        })
        .collect();
    let (line_bg, gutter_bg): (Option<Rgba>, Option<Rgba>) = match cell.kind {
        LineKind::Context => (None, None),
        LineKind::Added => (Some(theme::added_line()), Some(theme::added_gutter())),
        LineKind::Removed => (Some(theme::removed_line()), Some(theme::removed_gutter())),
    };
    half.when_some(line_bg, |s, bg| s.bg(bg))
        .child(
            div()
                .w(gutter_width)
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .pr_2()
                .when_some(gutter_bg, |s, bg| s.bg(bg))
                .text_color(theme::faint())
                .child(cell.line.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .pl_2()
                .whitespace_nowrap()
                .overflow_hidden()
                .child(StyledText::new(text).with_highlights(highlights)),
        )
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
