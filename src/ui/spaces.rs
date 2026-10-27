//! The menu on the left: the four spaces of the shell (#70) — Inbox, Code, Branches and
//! Merge requests — with what waits in each; a space the project cannot have is greyed, with the
//! line that says what it needs. What is on screen sits under its space: the request, else the
//! branch, and the file read in Code. In Menu mode the spaces nest under the active project's row
//! (X13). The menu folds to icons only when the user folds it — the title bar's first button or ⌘B
//! (D2); Settings is the title bar's gear. Until the spaces get pages of their own (#72, #75), Code
//! opens search everywhere on Files, Branches and Merge requests the picker beside the menu.

use super::nav::{Item, Page};
use super::requests::Scope;
use super::{Workspace, icons, px, theme};
use gpui::{
    ClickEvent, Context, FontWeight, MouseButton, SharedString, canvas, div, point, prelude::*,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The menu's width, open and folded to icons.
const OPEN: f32 = 216.;
const ICONS: f32 = 52.;
/// The menu's side padding, folded or open.
const EDGE: f32 = 6.;
/// A space's icon (17 px) from its row's start: centred in the folded menu, as far in open, and
/// further under a project.
const CENTRED: f32 = (ICONS - 2. * EDGE - 17.) / 2.;
const INSET: f32 = 10.;
const NESTED: f32 = 28.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Space {
    Inbox,
    Code,
    Branches,
    Requests,
}

impl Space {
    pub const ALL: [Space; 4] = [Space::Inbox, Space::Code, Space::Branches, Space::Requests];

    fn label(self) -> &'static str {
        match self {
            Space::Inbox => "Inbox",
            Space::Code => "Code",
            Space::Branches => "Branches",
            Space::Requests => "Merge requests",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Space::Inbox => "inbox",
            Space::Code => "folder",
            Space::Branches => "branch",
            Space::Requests => "pull-request",
        }
    }

    /// Its key as the app writes keys (Ctrl+Shift+1…4 on Windows and Linux).
    pub fn key(self) -> &'static str {
        match self {
            Space::Inbox => "⌃⌘1",
            Space::Code => "⌃⌘2",
            Space::Branches => "⌃⌘3",
            Space::Requests => "⌃⌘4",
        }
    }

    pub(super) fn id(self) -> &'static str {
        match self {
            Space::Inbox => "space-inbox",
            Space::Code => "space-code",
            Space::Branches => "space-branches",
            Space::Requests => "space-requests",
        }
    }

    /// As `layout.txt` keeps the menu's order.
    fn name(self) -> &'static str {
        self.id().trim_start_matches("space-")
    }
}

/// The menu's spaces in the order `kept` names them (dragged there), any it leaves out after
/// them in their first order.
pub fn ordered(kept: &str) -> Vec<Space> {
    let mut out: Vec<Space> = kept
        .split(',')
        .filter_map(|n| Space::ALL.into_iter().find(|s| s.name() == n.trim()))
        .collect();
    out.dedup();
    for s in Space::ALL {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// A space being dragged in the menu.
#[derive(Clone, Copy)]
pub struct DragSpace(pub Space);

/// What follows the pointer while a space or a tab is dragged: its name.
pub struct DragLabel(pub String);

impl Render for DragLabel {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(10.))
            .py(px(4.))
            .rounded(px(super::ROW_RADIUS))
            .bg(theme::selected())
            .border_1()
            .border_color(theme::island_border())
            .text_size(px(13.))
            .text_color(theme::text())
            .child(self.0.clone())
    }
}

/// What a project lacks for a space.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Missing {
    /// No GitLab or GitHub remote.
    Remote,
    /// A remote but no token: the space opens the connect panel.
    Token,
    /// A plain folder, not a git repository (#75).
    Git,
}

impl Missing {
    fn line(self) -> &'static str {
        match self {
            Missing::Remote => "Needs a GitLab or GitHub remote",
            Missing::Token => "Connect a GitLab token",
            Missing::Git => "Not a git repository · git init",
        }
    }
}

/// The menu's scroll (#146): its rows scroll once they are taller than the window — many
/// requests open under Merge requests —, and the row of the page on screen comes into view when
/// that page changes.
#[derive(Default)]
pub struct Places {
    scroll: gpui::ScrollHandle,
    /// The row last brought into view, by what it shows.
    revealed: RefCell<String>,
    /// The next frame brings the row on screen into view.
    reveal: Rc<Cell<bool>>,
}

/// What is open under a space, as its row in the menu.
struct OpenRow {
    /// The request or branch; None for Code's file.
    item: Option<Item>,
    icon: &'static str,
    label: String,
    mono: bool,
    tip: String,
    /// It is the page on screen.
    on: bool,
    /// Agent comments not sent yet on its files.
    drafts: bool,
    /// Notifications of it not read yet.
    news: bool,
    /// The agent that gathered it (#148): its mark, and how many of its pending comments wait.
    agent: Option<(char, gpui::Rgba, usize)>,
}

impl Space {
    /// Its place in `Space::ALL`.
    pub fn ix(self) -> usize {
        match self {
            Space::Inbox => 0,
            Space::Code => 1,
            Space::Branches => 2,
            Space::Requests => 3,
        }
    }
}

impl Workspace {
    /// The space on screen; none while Settings shows or no repository is open.
    pub(super) fn space(&self) -> Option<Space> {
        (!self.settings_open && self.root.is_some()).then_some(self.nav.space)
    }

    fn missing(&self, space: Space) -> Option<Missing> {
        match space {
            Space::Branches if self.plain => Some(Missing::Git),
            Space::Code | Space::Branches => None,
            Space::Inbox | Space::Requests if self.requests.available => None,
            Space::Inbox | Space::Requests if self.requests.connectable => Some(Missing::Token),
            Space::Inbox | Space::Requests => Some(Missing::Remote),
        }
    }

    /// What waits in a space: the Inbox's unread, the requests that ask for your review.
    fn waiting(&self, space: Space) -> usize {
        match space {
            Space::Inbox => self.inbox_unread(),
            Space::Requests => self
                .requests
                .list
                .iter()
                .filter(|m| self.in_scope(m, Scope::ReviewMe))
                .count(),
            Space::Code | Space::Branches => 0,
        }
    }

    /// The rows open under `space`: its requests or branches, Code's file.
    fn open_rows(&self, space: Space) -> Vec<OpenRow> {
        let here = self.space() == Some(space);
        if space == Space::Code {
            return match self.page_of(Space::Code) {
                Some(Page::File(path) | Page::Folder(path)) => vec![OpenRow {
                    item: None,
                    icon: if matches!(self.page_of(Space::Code), Some(Page::Folder(_))) {
                        "folder"
                    } else {
                        "file"
                    },
                    label: path.rsplit('/').next().unwrap_or(path).to_owned(),
                    mono: false,
                    tip: path.clone(),
                    on: here,
                    drafts: false,
                    news: false,
                    agent: None,
                }],
                _ => Vec::new(),
            };
        }
        let loaded = self.page_of(space).and_then(Page::item);
        self.open_items(space)
            .into_iter()
            .map(|open| {
                let on = here && self.is_shown(&open.item);
                // The one loaded says its files as they are now.
                let now: Vec<String>;
                let paths = match &self.diff {
                    Some(diff) if loaded.as_ref() == Some(&open.item) => {
                        now = diff.files.iter().map(|f| f.path().to_owned()).collect();
                        &now
                    }
                    _ => &open.paths,
                };
                let drafts = self
                    .comments
                    .iter()
                    .any(|c| c.state == crate::review::State::Draft && paths.contains(&c.path));
                match &open.item {
                    Item::Request(iid) => {
                        let m = self.requests.find(*iid);
                        let title = m.map(|m| m.title.clone()).unwrap_or_default();
                        // One merged or closed since stays here, saying so, until closed (#114).
                        let state = m
                            .map(|m| m.state)
                            .filter(|s| *s != crate::mr::MrState::Opened)
                            .map(|s| format!(" · {}", s.word().to_lowercase()))
                            .unwrap_or_default();
                        let label = format!("{} {title}{state}", super::words::number(*iid));
                        // Gathered by an agent: its line under the title (#148).
                        let gathered = self.gathered(*iid);
                        let tip = match gathered.and_then(|g| g.note.as_deref().map(|n| (g, n))) {
                            Some((g, note)) if !note.is_empty() => {
                                format!("{label}\n{}: {note}", g.agent)
                            }
                            Some((g, _)) => format!("{label}\nGathered by {}", g.agent),
                            None => label.clone(),
                        };
                        let agent = gathered.map(|g| {
                            let (letter, hue) = self.agent_mark(&g.agent);
                            (letter, hue, self.agent_waiting(*iid, &g.agent))
                        });
                        OpenRow {
                            item: Some(open.item.clone()),
                            icon: "pull-request",
                            tip,
                            label,
                            mono: false,
                            on,
                            drafts,
                            news: self.notices.unread_of(*iid),
                            agent,
                        }
                    }
                    Item::Branch(refname) => {
                        let name = self
                            .branches
                            .iter()
                            .find(|b| &b.refname == refname)
                            .map_or_else(|| refname.clone(), |b| b.name.clone());
                        OpenRow {
                            item: Some(open.item.clone()),
                            icon: "branch",
                            tip: name.clone(),
                            label: name,
                            mono: true,
                            on,
                            drafts,
                            news: false,
                            agent: None,
                        }
                    }
                }
            })
            .collect()
    }

    /// `from` dragged onto `to` in the menu: it takes `to`'s place, kept in `layout.txt`.
    fn move_space(&mut self, from: Space, to: Space, cx: &mut Context<Self>) {
        let mut order = ordered(&self.layout.spaces);
        let (Some(a), Some(b)) = (
            order.iter().position(|s| *s == from),
            order.iter().position(|s| *s == to),
        ) else {
            return;
        };
        let moved = order.remove(a);
        order.insert(b, moved);
        self.layout.spaces = order.iter().map(|s| s.name()).collect::<Vec<_>>().join(",");
        self.save_layout();
        cx.notify();
    }

    /// A space's row or ⌃⌘1…4: the page it was left on, its root when it is already on screen.
    /// One the project lacks a token for opens the connect panel; one it lacks a remote or git
    /// for does nothing.
    pub(super) fn go_space(&mut self, space: Space, cx: &mut Context<Self>) {
        match self.missing(space) {
            _ if self.root.is_none() => {}
            Some(Missing::Remote | Missing::Git) => {}
            Some(Missing::Token) => self.show(Space::Requests, None, cx),
            None => self.enter_space(space, cx),
        }
    }

    /// The menu; None without a repository and in zen.
    /// The row on screen, `key`, in the menu: when it is not the one last brought into view, the
    /// next frame scrolls the menu the least that shows it whole (#146). Drawn inside the row.
    fn reveal_row(&self, key: String) -> impl IntoElement + use<> {
        if *self.places.revealed.borrow() != key {
            self.places.revealed.replace(key);
            self.places.reveal.set(true);
        }
        let (scroll, reveal) = (self.places.scroll.clone(), self.places.reveal.clone());
        canvas(
            move |row, window, _| {
                if !reveal.replace(false) {
                    return;
                }
                let view = scroll.bounds();
                let by = if row.top() < view.top() {
                    view.top() - row.top()
                } else if row.bottom() > view.bottom() {
                    view.bottom() - row.bottom()
                } else {
                    return;
                };
                let (y, max) = (scroll.offset().y + by, scroll.max_offset().y);
                scroll.set_offset(point(scroll.offset().x, y.clamp(-max, gpui::px(0.))));
                window.request_animation_frame();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    pub(super) fn render_spaces(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        // Zen just began: it slides away (#56, 16).
        if self.root.is_none() || (self.zen.is_some() && !self.motion.recent("zen")) {
            return None;
        }
        let folded = self.menu_folded();
        let spaces = ordered(&self.layout.spaces);
        // Folding or unfolding: the names fade while the menu narrows or widens, and the icons
        // stay where they are (#56, 2).
        let moving = self.motion.recent("places-fold");
        let named = !folded || moving;
        let here = self.space();
        let projects = self.menu_projects();
        let nested = !projects.is_empty();
        let n_projects = projects.len();
        let row = |space: Space| {
            let id = space.id();
            let label = space.label();
            let missing = self.missing(space);
            let opens = self.open_rows(space);
            let shut = self.nav.folded[space.ix()];
            // The space's row is marked when none of what is open under it is the page — at its
            // root, or with them folded away.
            let within = here == Some(space);
            let on = within && (folded || shut || !opens.iter().any(|o| o.on));
            let n = Some(self.waiting(space)).filter(|&n| n > 0 && missing.is_none());
            let tip: gpui::SharedString = match missing {
                Some(m) => format!("{label} — {}", m.line()),
                None => format!("{label}  {}", space.key()),
            }
            .into();
            let disclose = (!folded && !opens.is_empty()).then(|| {
                div()
                    .id((id, 5usize))
                    .flex_none()
                    .size(px(18.))
                    .mr(px(-4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .hover(|s| s.bg(theme::hover()))
                    // Folded it points right, open down (#56).
                    .child(super::motion::toward(
                        icons::icon("chevron")
                            .size(px(13.))
                            .text_color(theme::faint()),
                        (id, 6usize),
                        !shut,
                        false,
                        super::motion::FAST,
                        |chevron, v| {
                            chevron.with_transformation(gpui::Transformation::rotate(
                                gpui::radians(-std::f32::consts::FRAC_PI_2 * (1. - v)),
                            ))
                        },
                    ))
                    .tooltip_show_delay(crate::ui::hover::LONG)
                    .tooltip(|_, cx| {
                        cx.new(|_| super::Tip("Show or hide what is open here"))
                            .into()
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                        if e.click_count() < 2 {
                            this.toggle_open_items(space, cx)
                        }
                    }))
            });
            let line = div()
                .id(id)
                .relative()
                .h(px(32.))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(10.))
                .pr(px(8.))
                .rounded(px(super::ROW_RADIUS))
                .text_color(if within {
                    theme::text()
                } else {
                    theme::muted()
                })
                .when(on, |s| {
                    s.bg(theme::selected())
                        .child(self.reveal_row(id.to_owned()))
                })
                .when(missing.is_some(), |s| s.opacity(0.5))
                .when(missing != Some(Missing::Remote), |s| {
                    s.cursor_pointer()
                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                })
                .group(id)
                .child(
                    icons::icon(space.icon())
                        .size(px(17.))
                        .text_color(if within {
                            theme::text()
                        } else {
                            theme::muted()
                        })
                        .when(missing != Some(Missing::Remote), |s| {
                            s.group_hover(id, |s| s.text_color(theme::text()))
                        }),
                )
                .when(named, |s| {
                    // The names fade in as the menu widens, and out as it narrows (#56, 2).
                    s.child(super::motion::toward(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(super::words::say(label).into_owned()),
                        (id, 1usize),
                        !folded,
                        moving,
                        super::motion::FAST,
                        |label, v| label.opacity(v),
                    ))
                    // The count is the badge's once the menu folds.
                    .children(n.filter(|_| !folded).map(|n| {
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(n.to_string())
                    }))
                    .children(disclose)
                })
                // Folded, a badge on the icon's corner says something waits for you; it fades in
                // as the menu narrows.
                .when(folded, |s| {
                    s.children(n.map(|n| {
                        let badge = div()
                            .absolute()
                            .top(px(3.))
                            .left(px(CENTRED + 11.5))
                            .min_w(px(14.))
                            .h(px(14.))
                            .px(px(3.))
                            .rounded_full()
                            .bg(theme::accent())
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(9.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::base())
                            .child(n.min(99).to_string());
                        super::motion::toward(
                            badge,
                            (id, 8usize),
                            true,
                            moving,
                            super::motion::FAST,
                            |badge, v| badge.opacity(v),
                        )
                    }))
                })
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                // Dragged onto another space, it takes that place in the menu (kept).
                .drag_over::<DragSpace>(|s, _, _, _| s.bg(theme::selected()))
                .on_drag(DragSpace(space), move |_, _, _, cx| {
                    cx.new(|_| DragLabel(super::words::say(label).into_owned()))
                })
                .on_drop(cx.listener(move |this, dragged: &DragSpace, _, cx| {
                    this.move_space(dragged.0, space, cx)
                }))
                // A double click folds or opens what is open under it, as its chevron does.
                .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                    if e.click_count() == 2 && !this.open_rows(space).is_empty() {
                        this.toggle_open_items(space, cx)
                    } else {
                        this.go_space(space, cx)
                    }
                }));
            // The icon's place: centred in the folded menu, after the indent open; it moves only
            // with the menu's own motion.
            let line = super::motion::toward(
                line,
                (id, 7usize),
                !folded,
                moving,
                super::motion::BASE,
                move |line, v| {
                    let open = if nested { NESTED } else { INSET };
                    line.pl(px(super::motion::mix(CENTRED, open, v)))
                },
            );
            let why = missing.filter(|_| !folded).map(|m| {
                div()
                    .flex_none()
                    .pt(px(2.))
                    .pl(px(if nested { 55. } else { 37. }))
                    .pr(px(8.))
                    .pb(px(6.))
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(super::words::say(m.line()).into_owned())
            });
            let shows = !folded && !shut && !opens.is_empty();
            let opens = opens
                .into_iter()
                .enumerate()
                .map(|(n, open)| {
                    let group: gpui::SharedString = format!("{id}-open-{n}").into();
                    let on = open.on;
                    let picked = open
                        .item
                        .as_ref()
                        .is_some_and(|i| self.nav.picked.contains(i));
                    let item = open.item.clone();
                    let close = item.clone();
                    let dot = |color: gpui::Rgba, tip: &'static str| {
                        div()
                            .id(SharedString::from(format!("{id}-dot-{n}-{tip}")))
                            .flex_none()
                            .size(px(7.))
                            .rounded_full()
                            .bg(color)
                            .tooltip_show_delay(crate::ui::hover::LONG)
                            .tooltip(move |_, cx| cx.new(|_| super::Tip(tip)).into())
                    };
                    div()
                        .id(SharedString::from(format!("{id}-open-{n}")))
                        .group(group.clone())
                        .flex_none()
                        .min_h(px(28.))
                        .ml(px(if nested { 35. } else { 17. }))
                        .pl(px(12.))
                        .pr(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.))
                        .cursor_pointer()
                        .when(on, |s| {
                            s.relative()
                                .border_l_2()
                                .border_color(theme::accent())
                                .rounded_r(px(super::ROW_RADIUS))
                                .bg(theme::selected())
                                .text_color(theme::text())
                                .child(self.reveal_row(format!("{id}:{}", open.tip)))
                        })
                        .when(!on, |s| {
                            s.border_l_1()
                                .border_color(theme::island_border())
                                .text_color(theme::muted())
                                .hover(|s| s.text_color(theme::text()))
                        })
                        // Picked with shift, for ⌘W.
                        .when(picked && !on, |s| {
                            s.rounded_r(px(super::ROW_RADIUS))
                                .bg(theme::selected())
                                .text_color(theme::text())
                        })
                        // Code's file by its own icon (#95).
                        .child(match open.item {
                            None if open.icon == "folder" => icons::folder_icon(&open.tip, 14.),
                            None => icons::file_icon(&open.tip, 14.),
                            Some(_) => icons::icon(open.icon)
                                .size(px(14.))
                                .text_color(theme::faint())
                                .into_any_element(),
                        })
                        .child(
                            super::hover::cut(
                                SharedString::from(format!("{id}-label-{n}")),
                                open.label,
                            )
                            .flex_1()
                            .when(open.mono, |s| s.font_family(theme::CODE_FONT)),
                        )
                        .children(open.agent.map(|(letter, hue, waiting)| {
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(
                                    super::agent::chat_badge(letter, hue, false)
                                        .size(px(14.))
                                        .text_size(px(8.)),
                                )
                                .when(waiting > 0, |s| {
                                    s.child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(theme::faint())
                                            .child(waiting.to_string()),
                                    )
                                })
                        }))
                        .children(open.drafts.then(|| dot(theme::agent(), "Unsent drafts")))
                        .children(open.news.then(|| dot(theme::accent(), "New activity")))
                        .child(
                            div()
                                .id(SharedString::from(format!("{id}-close-{n}")))
                                .flex_none()
                                .size(px(18.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .when(!on, |s| s.invisible().group_hover(group, |s| s.visible()))
                                .hover(|s| s.bg(theme::hover()))
                                .child(icons::icon("x").size(px(12.)).text_color(theme::faint()))
                                .tooltip_show_delay(crate::ui::hover::LONG)
                                .tooltip(|_, cx| cx.new(|_| super::Tip("Close  ⌘W")).into())
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    match &close {
                                        Some(item) => this.close_open(item, cx),
                                        None => this.close_shown(cx),
                                    }
                                })),
                        )
                        .tooltip_show_delay(crate::ui::hover::LONG)
                        .tooltip({
                            let tip: SharedString = open.tip.into();
                            move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into()
                        })
                        // Shift picks a run of them to close together with ⌘W.
                        .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| match &item {
                            Some(item) if e.modifiers().shift => this.pick_open(item.clone(), cx),
                            Some(item) => {
                                this.nav.picked.clear();
                                this.show_open(item, cx)
                            }
                            None => this.go_space(space, cx),
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            // What is open under it folds away and back in size and alpha (#110), the space
            // above it inside, so none is left at the end.
            let opens = self.folding(
                format!("{id}-opens"),
                &format!("opens-{id}"),
                shows,
                div()
                    .pt(px(2.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(opens),
            );
            div()
                .flex_none()
                .flex()
                .flex_col()
                .child(line)
                .children(why)
                .children(opens)
                .into_any_element()
        };
        let menu = div()
            .id("places")
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(EDGE))
            .pt(px(4.))
            .pb(px(4.))
            // Taller than the window, it scrolls (#146).
            .overflow_y_scroll()
            .track_scroll(&self.places.scroll)
            .when(nested, |s| {
                s.child(if folded {
                    div()
                        .flex_none()
                        .h(px(1.))
                        .mx(px(8.))
                        .my(px(10.))
                        .bg(theme::island_border())
                } else {
                    div()
                        .flex_none()
                        .px(px(8.))
                        .pt(px(10.))
                        .pb(px(4.))
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child("Projects")
                })
            })
            .children(projects.into_iter().flat_map(|(ix, tab)| {
                let mark = super::rail::initials(&tab.title);
                let tint = super::rail::project_hue(tab.root.as_deref(), &tab.title);
                let active = tab.active;
                let menu_tab = tab.clone();
                let project = div()
                    .id(("menu-project", ix))
                    // The rail's right click: name and colour, move, show, a window, close.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                            this.project_menu(ix, n_projects, &menu_tab, e.position, cx)
                        }),
                    )
                    .h(px(32.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(10.))
                    .pr(px(6.))
                    .rounded(px(super::ROW_RADIUS))
                    .cursor_pointer()
                    .when(tab.active, |s| s.bg(theme::hover()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(super::rail::project_mark(&mark, tint, 22.))
                    .when(named, |s| {
                        s.child(super::motion::toward(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(tab.title.clone()),
                            ("menu-project-name", ix),
                            !folded,
                            moving,
                            super::motion::FAST,
                            |name, v| name.opacity(v),
                        ))
                        .children((!tab.active && tab.unread > 0).then(|| {
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(theme::accent())
                                .child(tab.unread.to_string())
                        }))
                        // Open (its spaces under it) or another one to switch to.
                        .child(
                            icons::icon(if tab.active {
                                "chevron"
                            } else {
                                "chevron-right"
                            })
                            .size(px(14.))
                            .text_color(theme::faint()),
                        )
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.with_shell(window, cx, move |s, window, cx| s.select(ix, window, cx))
                    }));
                // Its mark (22 px) centred folded, near the edge open; it moves with the menu.
                let project = super::motion::toward(
                    project,
                    ("menu-project-place", ix),
                    !folded,
                    moving,
                    super::motion::BASE,
                    |project, v| {
                        let centred = (ICONS - 2. * EDGE - 22.) / 2.;
                        project.pl(px(super::motion::mix(centred, 6., v)))
                    },
                )
                .into_any_element();
                let mut rows = vec![project];
                if active {
                    rows.extend(spaces.iter().copied().map(&row));
                }
                rows
            }))
            .when(!nested, |s| s.children(spaces.iter().copied().map(&row)))
            // A narrow window's side panel, folded into this column (#80).
            .children(self.render_panel_icons(cx));
        // Folding to icons narrows it (#56, 2); zen slides it away (16).
        let menu = super::motion::toward(
            menu,
            "places-fold",
            !folded,
            false,
            super::motion::BASE,
            |menu, v| menu.w(px(super::motion::mix(ICONS, OPEN, v))),
        );
        Some(super::motion::toward(
            div().flex_none().flex().overflow_hidden().child(menu),
            "places-zen",
            self.zen.is_none(),
            self.motion.recent("zen"),
            super::motion::BASE,
            |menu, v| menu.max_w(px(OPEN * v)).opacity(v),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menus_order_is_kept_by_name_and_fills_in() {
        assert_eq!(ordered(""), Space::ALL.to_vec());
        assert_eq!(
            ordered("requests,inbox"),
            [Space::Requests, Space::Inbox, Space::Code, Space::Branches]
        );
        assert_eq!(ordered("nonsense,code,code").first(), Some(&Space::Code));
    }
}
