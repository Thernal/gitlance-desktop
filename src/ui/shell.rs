//! Repository tabs: several workspaces in one window, each with its own repository, selection,
//! filter and scroll position. `⌘T` opens one, `⌘W` closes, `⌘1…9` switch, `⌘⇧T` reopens the last
//! closed. In Tabs mode they sit in the title bar (D12, D21).

use super::px;
use super::{ROW_RADIUS, Workspace, theme};
use gpui::{ClickEvent, Context, Entity, MouseButton, WeakEntity, Window, div, prelude::*};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// What a workspace's tabs need to know about each tab.
#[derive(Clone)]
pub struct TabInfo {
    pub title: String,
    pub news: bool,
    pub active: bool,
    /// The repository, for Reveal in Finder and a new window.
    pub root: Option<PathBuf>,
    /// Its unread notifications, counted on the rail.
    pub unread: usize,
}

pub type TabModel = Rc<RefCell<Vec<TabInfo>>>;

pub struct Shell {
    tabs: Vec<Entity<Workspace>>,
    active: usize,
    /// Each tab's "on screen", shared with its workspace: one behind another tab polls its
    /// repository seldom.
    shown: std::collections::HashMap<gpui::EntityId, std::rc::Rc<std::cell::Cell<bool>>>,
    closed: Vec<PathBuf>,
    model: TabModel,
    /// The session as last written, so it is written again only when it changes.
    saved: String,
    /// The first window keeps the session; one opened from the rail does not.
    primary: bool,
    /// The projects this window last said it has open, and when (#153).
    said: Vec<PathBuf>,
    said_at: i64,
    /// Projects opened for an agent whose tabs are still loading (a tab has its root once loaded),
    /// with when: not opened twice meanwhile.
    opening: Vec<(PathBuf, i64)>,
}

impl Shell {
    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with(path, true, window, cx)
    }

    /// A window of its own for `path`, opened from the rail: it does not write the session.
    pub fn secondary(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with(Some(path), false, window, cx)
    }

    fn with(
        path: Option<PathBuf>,
        primary: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let model: TabModel = Rc::default();
        let mut shell = Self {
            tabs: Vec::new(),
            active: 0,
            shown: Default::default(),
            closed: Vec::new(),
            model,
            saved: String::new(),
            primary,
            said: Vec::new(),
            said_at: 0,
            opening: Vec::new(),
        };
        // Agents read whether this window is there and what it has open; the first window opens
        // what they ask for (#153).
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(2))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| this.agents_beat(window, cx))
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
        let key = cx.entity_id().as_u64();
        cx.on_release(move |_, _| {
            if let Some(bus) = crate::agents::Bus::new() {
                bus.absent(std::process::id(), key);
            }
            super::system_notes::set_front(key, false);
        })
        .detach();
        // While a window is in front, news is not sent to the system's notifications (#26).
        super::system_notes::set_front(key, window.is_window_active());
        cx.observe_window_activation(window, move |_, window, _| {
            super::system_notes::set_front(key, window.is_window_active());
        })
        .detach();
        // An explicit path opens just that; otherwise the tabs of the last session come back.
        let (tabs, active) = if path.is_some() {
            (Vec::new(), 0)
        } else {
            crate::storage::session()
        };
        if tabs.is_empty() {
            let ws = cx.new(|cx| Workspace::new(path, true, window, cx));
            shell.attach(ws, window, cx);
        } else {
            for tab in tabs {
                let ws = cx.new(|cx| Workspace::new(Some(tab), false, window, cx));
                shell.attach(ws, window, cx);
            }
            shell.select(active.min(shell.tabs.len() - 1), window, cx);
        }
        // The first start: Set up over the welcome (#143); a release that adds a part, that part.
        let parts = if primary {
            super::setup::due()
        } else {
            Vec::new()
        };
        if !parts.is_empty() {
            shell.tabs[shell.active].update(cx, |w, cx| w.open_setup(parts, cx));
        }
        shell
    }

    /// Wraps a new workspace into the shell and shows it.
    fn attach(&mut self, ws: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) {
        self.wrap(ws, cx);
        self.active = self.tabs.len() - 1;
        self.focus_active(window, cx);
        self.mark_shown();
        cx.notify();
    }

    /// Wraps a new workspace into the shell as its last tab, the one on screen staying.
    fn wrap(&mut self, ws: Entity<Workspace>, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        let model = self.model.clone();
        let shown = ws.update(cx, |w, _| {
            w.shell = Some(weak);
            w.tabs = model;
            w.on_screen.clone()
        });
        self.shown.insert(ws.entity_id(), shown);
        cx.observe(&ws, |_, _, cx| cx.notify()).detach();
        self.tabs.push(ws);
        self.mark_shown();
        cx.notify();
    }

    /// Each tab's project: its main working tree, else its folder.
    fn projects(&self, cx: &gpui::App) -> Vec<PathBuf> {
        self.tabs
            .iter()
            .filter_map(|t| {
                let w = t.read(cx);
                w.project.clone().or_else(|| w.root.clone())
            })
            .collect()
    }

    /// Every 2 s: this window says it is there and what it has open (#153); the first window also
    /// opens a project an agent asked for, or acted on, that no window has, as a tab behind the one
    /// on screen — its workspace then takes the agent's event.
    fn agents_beat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bus) = crate::agents::Bus::new() else {
            return;
        };
        let mut projects = self.projects(cx);
        let now = crate::agents::now();
        // Said again when it changes, and every 10 s so it stays fresh.
        if projects != self.said || now - self.said_at >= 10 {
            let presence = crate::agents::Presence {
                pid: std::process::id(),
                projects: projects.clone(),
                beat: now,
            };
            if bus.present(cx.entity_id().as_u64(), &presence).is_ok() {
                self.said = projects.clone();
                self.said_at = now;
            }
        }
        if !self.primary {
            return;
        }
        let pid = std::process::id();
        self.opening
            .retain(|(path, at)| !projects.contains(path) && now - at < 30);
        let elsewhere: Vec<PathBuf> = bus
            .running()
            .into_iter()
            .filter(|p| p.pid == pid)
            .flat_map(|p| p.projects)
            .chain(self.opening.iter().map(|(path, _)| path.clone()))
            .collect();
        for event in bus.events() {
            let project = event.project.clone();
            if !projects.contains(&project) && !elsewhere.contains(&project) {
                if !project.is_dir() {
                    bus.take_event(&event.uid);
                    continue;
                }
                let ws = cx.new(|cx| Workspace::new(Some(project.clone()), false, window, cx));
                self.wrap(ws, cx);
                projects.push(project.clone());
                self.opening.push((project.clone(), now));
                let said = format!(
                    "{} opened {} in a tab",
                    event.agent,
                    crate::labels::project_name(&project)
                );
                self.tabs[self.active].update(cx, |w, cx| w.toast_info(said.into(), cx));
            }
            if event.what == crate::agents::What::Open {
                bus.take_event(&event.uid);
            }
        }
    }

    /// Tells each tab whether it is the one on screen (no workspace is touched: one may be
    /// mid-update).
    fn mark_shown(&self) {
        for (ix, tab) in self.tabs.iter().enumerate() {
            if let Some(shown) = self.shown.get(&tab.entity_id()) {
                shown.set(ix == self.active);
            }
        }
    }

    pub(super) fn active_ix(&self) -> usize {
        self.active
    }

    /// The first tab (the snapshot checks drive it).
    #[cfg(feature = "snapshot")]
    pub(super) fn first(&self) -> Entity<Workspace> {
        self.tabs[0].clone()
    }

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.tabs[self.active].read(cx).focus.clone();
        window.focus(&focus, cx);
    }

    /// The tab that has `path` open, as the tabs last said (a workspace may be mid-update).
    /// The tab of workspace `id`.
    pub(super) fn tab_ix(&self, id: gpui::EntityId) -> Option<usize> {
        self.tabs.iter().position(|t| t.entity_id() == id)
    }

    pub(super) fn tab_of(&self, path: &std::path::Path) -> Option<usize> {
        let same = |root: &std::path::Path| {
            root == path
                || matches!((root.canonicalize(), path.canonicalize()), (Ok(a), Ok(b)) if a == b)
        };
        self.model
            .borrow()
            .iter()
            .position(|t| t.root.as_deref().is_some_and(same))
    }

    /// Opens `path` in a tab of its own, and gives back its workspace; a project already open
    /// is gone to, not opened twice.
    pub(super) fn open_tab(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Workspace> {
        if let Some(ix) = self.tab_of(&path).filter(|&ix| ix < self.tabs.len()) {
            self.select(ix, window, cx);
            return self.tabs[ix].clone();
        }
        let ws = cx.new(|cx| Workspace::new(Some(path), false, window, cx));
        self.attach(ws.clone(), window, cx);
        ws
    }

    pub(super) fn select(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.tabs.len() && ix != self.active {
            self.active = ix;
            self.focus_active(window, cx);
            self.mark_shown();
            cx.notify();
        }
    }

    pub(super) fn step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.tabs.len();
        let ix = if forward {
            (self.active + 1) % n
        } else {
            (self.active + n - 1) % n
        };
        self.select(ix, window, cx);
    }

    pub(super) fn close(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        if self.tabs.len() == 1 {
            cx.quit();
            return;
        }
        // The tab being closed may be mid-update (a click inside it): only its path is read.
        let root = self.tabs[ix].read(cx).root.clone();
        self.closed.extend(root);
        let gone = self.tabs.remove(ix);
        self.shown.remove(&gone.entity_id());
        if self.active > ix || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.mark_shown();
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Writes the open repositories and the active tab when they differ from what was written.
    fn save_session(&mut self, cx: &mut Context<Self>) {
        if !self.primary
            || (cfg!(feature = "snapshot") && std::env::var_os("GITLANCE_SNAPSHOT").is_some())
        {
            return;
        }
        let mut roots = Vec::new();
        let mut active = 0;
        for (ix, tab) in self.tabs.iter().enumerate() {
            // A tab still loading has no root yet; it keeps the last session's place.
            if let Some(root) = tab.read(cx).root.clone() {
                if ix == self.active {
                    active = roots.len();
                }
                roots.push(root);
            }
        }
        if roots.is_empty() {
            return;
        }
        let key = format!("{active} {roots:?}");
        if key != self.saved {
            crate::storage::save_session(&roots, active);
            self.saved = key;
        }
    }

    /// Moves tab `from` to `to` (the rail's Move up / down and its drag); ⌘1…9 follow.
    pub(super) fn move_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let n = self.tabs.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let active = self.tabs[self.active].clone();
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = self.tabs.iter().position(|t| *t == active).unwrap_or(0);
        cx.notify();
    }

    pub(super) fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.closed.pop() {
            let ws = cx.new(|cx| Workspace::new(Some(path), false, window, cx));
            self.attach(ws, window, cx);
        }
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let infos = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let w = t.read(cx);
                TabInfo {
                    title: w.tab_title(),
                    news: i != self.active && w.has_news(),
                    active: i == self.active,
                    root: w.root.clone(),
                    unread: w.inbox_unread(),
                }
            })
            .collect();
        *self.model.borrow_mut() = infos;
        self.save_session(cx);
        div().size_full().child(self.tabs[self.active].clone())
    }
}

impl Workspace {
    pub(super) fn tab_title(&self) -> String {
        // Its own name when it has one (the developer, 2026-10-23), else its folder's.
        self.root
            .as_deref()
            .map_or_else(|| "New tab".to_owned(), crate::labels::project_name)
    }

    pub(super) fn has_news(&self) -> bool {
        self.watch.pending.is_some() || self.watch.new_from.is_some()
    }

    /// Runs `f` on the shell this workspace is a tab of.
    pub(super) fn with_shell(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
    ) {
        // After the event: the shell reads its tabs' workspaces (closing one reads its path), and
        // this workspace is still being updated now — reading it then panics.
        if let Some(shell) = self.shell.as_ref().and_then(WeakEntity::upgrade) {
            window.defer(cx, move |window, cx| {
                shell.update(cx, |s, cx| f(s, window, cx));
            });
        }
    }

    /// The tabs in the title bar (Design `shell.js`, D12, X10): each project's mark, name and unread
    /// count, a dot when it has news, ✕ on the active one (on the others under the pointer), + to
    /// open another. None unless Settings → Appearance chose Tabs.
    pub(super) fn render_title_tabs(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if super::projects() != crate::storage::Projects::Tabs {
            return None;
        }
        let tabs = self.tabs.borrow().clone();
        Some(
            div()
                .flex_none()
                .min_w_0()
                .max_w(px(760.))
                .overflow_hidden()
                .flex()
                .items_center()
                .gap(px(4.))
                .children(tabs.into_iter().enumerate().map(|(ix, tab)| {
                    let mark = super::rail::initials(&tab.title);
                    let hue = super::rail::project_hue(tab.root.as_deref(), &tab.title);
                    let group: gpui::SharedString = format!("tab-{ix}").into();
                    let (menu_tab, n) = (tab.clone(), self.tabs.borrow().len());
                    div()
                        .id(("tab", ix))
                        .group(group.clone())
                        // The rail's right click: name and colour, move, show, a window, close.
                        .on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                                this.project_menu(ix, n, &menu_tab, e.position, cx)
                            }),
                        )
                        .flex_shrink(1.)
                        .min_w(px(60.))
                        .max_w(px(220.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .pl(px(6.))
                        .pr(px(4.))
                        .rounded(px(ROW_RADIUS))
                        .cursor_pointer()
                        .when(tab.active, |s| {
                            s.bg(theme::panel())
                                .border_1()
                                .border_color(theme::island_border())
                        })
                        .when(!tab.active, |s| {
                            s.text_color(theme::muted())
                                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                        })
                        .child(super::rail::project_mark(&mark, hue, 18.))
                        .child(super::hover::cut(("tab-title", ix), tab.title.clone()).flex_1())
                        .children((!tab.active && tab.unread > 0).then(|| {
                            div()
                                .flex_none()
                                .min_w(px(16.))
                                .h(px(16.))
                                .px(px(4.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(theme::accent())
                                .text_size(px(10.))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(theme::on_accent())
                                .child(tab.unread.to_string())
                        }))
                        .children(tab.news.then(|| {
                            div()
                                .flex_none()
                                .size(px(6.))
                                .rounded_full()
                                .bg(theme::accent())
                        }))
                        .child(
                            div()
                                .id(("tab-close", ix))
                                .flex_none()
                                .w(px(18.))
                                .h(px(18.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .text_size(px(12.))
                                .text_color(theme::faint())
                                .when(!tab.active, |s| {
                                    s.invisible().group_hover(group, |s| s.visible())
                                })
                                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                                .child("×")
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.with_shell(window, cx, move |s, window, cx| {
                                        s.close(ix, window, cx)
                                    })
                                })),
                        )
                        // Dragged onto another tab, it takes that place, as on the rail.
                        .drag_over::<super::rail::DragProject>(|s, _, _, _| s.bg(theme::selected()))
                        .on_drag(super::rail::DragProject(ix), {
                            let title = tab.title.clone();
                            move |_, _, _, cx| cx.new(|_| super::spaces::DragLabel(title.clone()))
                        })
                        .on_drop(cx.listener(
                            move |this, dragged: &super::rail::DragProject, window, cx| {
                                let from = dragged.0;
                                this.with_shell(window, cx, move |s, _, cx| {
                                    s.move_tab(from, ix, cx)
                                });
                            },
                        ))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.with_shell(window, cx, move |s, window, cx| {
                                s.select(ix, window, cx)
                            })
                        }))
                }))
                .child(
                    div()
                        .id("tab-new")
                        .flex_none()
                        .w(px(26.))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(ROW_RADIUS))
                        .text_color(theme::muted())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                        .tooltip_show_delay(super::hover::LONG)
                        .tooltip(|_, cx| cx.new(|_| super::Tip("Open a repository  ⌘T")).into())
                        .child("+")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.open_palette(super::palette::Kind::Projects, cx)
                        })),
                )
                .into_any_element(),
        )
    }
}
