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
use gpui::{ClickEvent, Context, FontWeight, MouseButton, SharedString, div, prelude::*};

/// The menu's width, open and folded to icons.
const OPEN: f32 = 216.;
const ICONS: f32 = 52.;

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

    fn id(self) -> &'static str {
        match self {
            Space::Inbox => "space-inbox",
            Space::Code => "space-code",
            Space::Branches => "space-branches",
            Space::Requests => "space-requests",
        }
    }
}

/// What a project lacks for a space.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Missing {
    /// No GitLab or GitHub remote.
    Remote,
    /// A remote but no token: the space opens the connect panel.
    Token,
}

impl Missing {
    fn line(self) -> &'static str {
        match self {
            Missing::Remote => "Needs a GitLab or GitHub remote",
            Missing::Token => "Connect a GitLab token",
        }
    }
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
            Space::Code | Space::Branches => None,
            Space::Inbox | Space::Requests if self.requests.available => None,
            Space::Inbox | Space::Requests if self.requests.connectable => Some(Missing::Token),
            Space::Inbox | Space::Requests => Some(Missing::Remote),
        }
    }

    /// What waits in a space: the Inbox's unread, the requests that ask for your review.
    fn waiting(&self, space: Space) -> usize {
        match space {
            Space::Inbox => self.notices.unread(),
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
                Some(Page::File(path)) => vec![OpenRow {
                    item: None,
                    icon: "file",
                    label: path.rsplit('/').next().unwrap_or(path).to_owned(),
                    mono: false,
                    tip: path.clone(),
                    on: here,
                    drafts: false,
                    news: false,
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
                        let title = self
                            .requests
                            .list
                            .iter()
                            .find(|m| m.iid == *iid)
                            .map(|m| m.title.clone())
                            .unwrap_or_default();
                        let label = format!("{} {title}", super::words::number(*iid));
                        OpenRow {
                            item: Some(open.item.clone()),
                            icon: "pull-request",
                            tip: label.clone(),
                            label,
                            mono: false,
                            on,
                            drafts,
                            news: self.notices.unread_of(*iid),
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
                        }
                    }
                }
            })
            .collect()
    }

    pub(super) fn menu_folded(&self) -> bool {
        self.layout.fold_places
    }

    /// Where the menu ends, from the window's left edge: the picker hangs beside it.
    pub(super) fn menu_right(&self) -> f32 {
        if self.zen.is_some() {
            return super::gap();
        }
        let rail = if super::projects() == crate::storage::Projects::Rail {
            48.
        } else {
            0.
        };
        super::gap() + rail + if self.menu_folded() { ICONS } else { OPEN }
    }

    /// The title bar's first button and ⌘B: the menu to icons and back, kept in `layout.txt`.
    pub(super) fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        self.layout.fold_places = !self.layout.fold_places;
        self.layout.save();
        cx.notify();
    }

    /// A space's row or ⌃⌘1…4: the page it was left on, its root when it is already on screen.
    /// One the project lacks a token for opens the connect panel; one it lacks a remote for
    /// does nothing.
    pub(super) fn go_space(&mut self, space: Space, cx: &mut Context<Self>) {
        match self.missing(space) {
            _ if self.root.is_none() => {}
            Some(Missing::Remote) => {}
            Some(Missing::Token) => self.show(Space::Requests, None, cx),
            None => self.enter_space(space, cx),
        }
    }

    /// The menu; None without a repository and in zen.
    pub(super) fn render_spaces(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        // Zen just began: it slides away (#56, 16).
        if self.root.is_none() || (self.zen.is_some() && !self.motion.recent("zen")) {
            return None;
        }
        let folded = self.menu_folded();
        let here = self.space();
        let projects = self.menu_projects();
        let nested = !projects.is_empty();
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
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.toggle_open_items(space, cx)
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
                .when(folded, |s| s.justify_center())
                .when(!folded, |s| s.px(px(8.)))
                .when(nested && !folded, |s| s.pl(px(26.)))
                .rounded(px(super::ROW_RADIUS))
                .text_color(if within {
                    theme::text()
                } else {
                    theme::muted()
                })
                .when(on, |s| s.bg(theme::selected()))
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
                .when(!folded, |s| {
                    // Unfolding, the names fade in as the menu widens (#56, 2).
                    s.child(super::motion::appear(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(super::words::say(label).into_owned()),
                        (id, 1usize),
                        super::motion::FAST,
                        |label, v| label.opacity(v),
                    ))
                    .children(n.map(|n| {
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(n.to_string())
                    }))
                    .children(disclose)
                })
                // Folded, a badge says something waits for you.
                .when(folded, |s| {
                    s.children(n.map(|n| {
                        div()
                            .absolute()
                            .top(px(3.))
                            .right(px(3.))
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
                            .child(n.min(99).to_string())
                    }))
                })
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.go_space(space, cx)));
            let why = missing.filter(|_| !folded).map(|m| {
                div()
                    .flex_none()
                    .pl(px(if nested { 55. } else { 37. }))
                    .pr(px(8.))
                    .pb(px(6.))
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(super::words::say(m.line()).into_owned())
            });
            let opens = opens
                .into_iter()
                .enumerate()
                .filter(|_| !folded && !shut)
                .map(|(n, open)| {
                    let group: gpui::SharedString = format!("{id}-open-{n}").into();
                    let on = open.on;
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
                            s.border_l_2()
                                .border_color(theme::accent())
                                .rounded_r(px(super::ROW_RADIUS))
                                .bg(theme::selected())
                                .text_color(theme::text())
                        })
                        .when(!on, |s| {
                            s.border_l_1()
                                .border_color(theme::island_border())
                                .text_color(theme::muted())
                                .hover(|s| s.text_color(theme::text()))
                        })
                        .child(
                            icons::icon(open.icon)
                                .size(px(14.))
                                .text_color(theme::faint()),
                        )
                        .child(
                            super::hover::cut(
                                SharedString::from(format!("{id}-label-{n}")),
                                open.label,
                            )
                            .flex_1()
                            .when(open.mono, |s| s.font_family(theme::CODE_FONT)),
                        )
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
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| match &item {
                            Some(item) => this.show_open(item, cx),
                            None => this.go_space(space, cx),
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(2.))
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
            .px(px(if folded { 6. } else { 8. }))
            .pt(px(4.))
            .pb(px(4.))
            .overflow_hidden()
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
                let tint = super::rail::hue(&tab.title);
                let active = tab.active;
                let project = div()
                    .id(("menu-project", ix))
                    .h(px(32.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(10.))
                    .when(folded, |s| s.justify_center())
                    .when(!folded, |s| s.px(px(6.)))
                    .rounded(px(super::ROW_RADIUS))
                    .cursor_pointer()
                    .when(tab.active, |s| s.bg(theme::hover()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(super::rail::project_mark(&mark, tint, 22.))
                    .when(!folded, |s| {
                        s.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(tab.title.clone()),
                        )
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
                    }))
                    .into_any_element();
                let mut rows = vec![project];
                if active {
                    rows.extend(Space::ALL.map(&row));
                }
                rows
            }))
            .when(!nested, |s| s.children(Space::ALL.map(&row)));
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
            |menu, v| menu.max_w(px(OPEN * v)),
        ))
    }
}
