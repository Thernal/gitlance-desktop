//! Repository tabs: several workspaces in one window, each with its own repository, selection,
//! filter and scroll position. `⌘T` opens one, `⌘W` closes, `⌘1…9` switch, `⌘⇧T` reopens the last
//! closed. The strip sits under the title bar and shows only with two or more tabs.

use super::px;
use super::{ROW_RADIUS, Workspace, theme};
use gpui::{ClickEvent, Context, Entity, MouseButton, WeakEntity, Window, div, prelude::*};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// What a workspace's tab strip needs to know about each tab.
#[derive(Clone)]
pub struct TabInfo {
    pub title: String,
    pub news: bool,
    pub active: bool,
}

pub type TabModel = Rc<RefCell<Vec<TabInfo>>>;

pub struct Shell {
    tabs: Vec<Entity<Workspace>>,
    active: usize,
    closed: Vec<PathBuf>,
    model: TabModel,
    /// The session as last written, so it is written again only when it changes.
    saved: String,
}

impl Shell {
    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let model: TabModel = Rc::default();
        let mut shell = Self {
            tabs: Vec::new(),
            active: 0,
            closed: Vec::new(),
            model,
            saved: String::new(),
        };
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
        shell
    }

    /// Wraps a new workspace into the shell and shows it.
    fn attach(&mut self, ws: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        let model = self.model.clone();
        ws.update(cx, |w, _| {
            w.shell = Some(weak);
            w.tabs = model;
        });
        cx.observe(&ws, |_, _, cx| cx.notify()).detach();
        self.tabs.push(ws);
        self.active = self.tabs.len() - 1;
        self.focus_active(window, cx);
        cx.notify();
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

    pub(super) fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ws = cx.new(|cx| Workspace::new(None, false, window, cx));
        self.attach(ws, window, cx);
    }

    /// Opens `path` in a tab of its own (the snapshot checks use it).
    #[cfg(feature = "snapshot")]
    pub(super) fn open_tab(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let ws = cx.new(|cx| Workspace::new(Some(path), false, window, cx));
        self.attach(ws, window, cx);
    }

    pub(super) fn select(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.tabs.len() && ix != self.active {
            self.active = ix;
            self.focus_active(window, cx);
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
        self.tabs.remove(ix);
        if self.active > ix || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Writes the open repositories and the active tab when they differ from what was written.
    fn save_session(&mut self, cx: &mut Context<Self>) {
        if cfg!(feature = "snapshot") && std::env::var_os("GITLANCE_SNAPSHOT").is_some() {
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
        self.root.as_ref().and_then(|r| r.file_name()).map_or_else(
            || "New tab".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    pub(super) fn has_news(&self) -> bool {
        self.watch.pending.is_some() || self.watch.new_from.is_some()
    }

    /// Runs `f` on the shell this workspace is a tab of.
    pub(super) fn with_shell(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Shell, &mut Context<Shell>),
    ) {
        if let Some(shell) = self.shell.as_ref().and_then(WeakEntity::upgrade) {
            shell.update(cx, f);
        }
    }

    /// The strip of tabs, under the title bar; absent with a single tab.
    pub(super) fn render_tab_strip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let tabs = self.tabs.borrow().clone();
        (tabs.len() > 1).then(|| {
            div()
                .flex_none()
                .h(px(30.))
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(super::GAP))
                .pb(px(super::GAP / 2.))
                .children(tabs.into_iter().enumerate().map(|(ix, tab)| {
                    div()
                        .id(("tab", ix))
                        .flex()
                        .items_center()
                        .gap_2()
                        .h(px(26.))
                        .max_w(px(200.))
                        .pl(px(10.))
                        .pr(px(4.))
                        .rounded(px(ROW_RADIUS))
                        .cursor_pointer()
                        .when(tab.active, |s| s.bg(theme::selected()))
                        .when(!tab.active, |s| {
                            s.text_color(theme::muted())
                                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                        })
                        .child(div().min_w_0().truncate().child(tab.title))
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
                                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                                .child("×")
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.with_shell(cx, |s, cx| s.close(ix, window, cx))
                                })),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.with_shell(cx, |s, cx| s.select(ix, window, cx))
                        }))
                }))
                .child(
                    div()
                        .id("tab-new")
                        .w(px(26.))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(ROW_RADIUS))
                        .text_color(theme::muted())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                        .child("+")
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.with_shell(cx, |s, cx| s.new_tab(window, cx))
                        })),
                )
        })
    }
}
