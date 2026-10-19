//! The menu of places on the left (Design `design/shell/` `nav()`, #49): Inbox, Merge requests (every
//! open one, #57), Review requests and My requests, then the repository's Commits, Branches and
//! Working tree, Settings at the foot. It folds to icons (the title bar's first button), and is
//! drawn folded while a merge request is reviewed unless opened there. The Inbox is a place of its
//! own (`super::inbox`).

use super::requests::{Scope, Side};
use super::{Selection, Workspace, icons, px, theme};
use gpui::{ClickEvent, Context, FontWeight, div, prelude::*};

/// The menu's width, open and folded to icons.
const OPEN: f32 = 216.;
const ICONS: f32 = 52.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Inbox,
    Requests,
    ReviewRequests,
    MyRequests,
    Commits,
    Branches,
    WorkingTree,
    Settings,
}

impl Workspace {
    /// The place the window shows now.
    pub(super) fn place(&self) -> Place {
        if self.settings_open {
            Place::Settings
        } else if self.inbox {
            Place::Inbox
        } else if self.picker_open && self.requests.side == Side::Requests {
            match self.requests.scope {
                Scope::All => Place::Requests,
                Scope::ReviewMe => Place::ReviewRequests,
                Scope::Mine => Place::MyRequests,
            }
        } else if self.picker_open {
            Place::Branches
        } else if self.selection == Selection::WorkingTree {
            Place::WorkingTree
        } else {
            Place::Commits
        }
    }

    /// Folded to icons: by choice, or while a merge request is reviewed (unless opened there).
    pub(super) fn places_folded(&self) -> bool {
        if self.showing_request() {
            !self.places_peek
        } else {
            self.layout.fold_places
        }
    }

    pub(super) fn toggle_places(&mut self, cx: &mut Context<Self>) {
        if self.showing_request() {
            self.places_peek = !self.places_peek;
        } else {
            self.layout.fold_places = !self.layout.fold_places;
            self.layout.save();
        }
        cx.notify();
    }

    pub(super) fn go_place(&mut self, place: Place, cx: &mut Context<Self>) {
        if place != Place::Inbox && self.inbox {
            self.inbox = false;
        }
        if place != Place::Settings && self.settings_open {
            self.settings_open = false;
        }
        if !matches!(
            place,
            Place::Requests | Place::ReviewRequests | Place::MyRequests | Place::Branches
        ) {
            self.close_picker(cx);
        }
        match place {
            Place::Inbox => {
                self.inbox = true;
                self.notices.at = self.inbox_rows().first().copied().unwrap_or(0);
                self.refresh_notices(cx);
            }
            Place::Requests | Place::ReviewRequests | Place::MyRequests => {
                self.requests.scope = match place {
                    Place::MyRequests => Scope::Mine,
                    Place::ReviewRequests => Scope::ReviewMe,
                    _ => Scope::All,
                };
                self.open_picker(true, cx);
            }
            Place::Branches => self.open_picker(false, cx),
            Place::Commits => {
                if matches!(self.selection, Selection::WorkingTree | Selection::None)
                    && !self.commits.is_empty()
                {
                    self.select_commit(0, cx);
                }
            }
            Place::WorkingTree => self.select_working_tree(cx),
            Place::Settings => self.open_settings(cx),
        }
        cx.notify();
    }

    /// The menu; None without a repository and in zen.
    pub(super) fn render_places(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if self.root.is_none() || self.zen.is_some() {
            return None;
        }
        let folded = self.places_folded();
        let here = self.place();
        let me = self.requests.login.is_some();
        let count = |scope: Scope| {
            self.requests
                .list
                .iter()
                .filter(|m| self.in_scope(m, scope))
                .count()
        };
        let item = |place: Place, icon: &'static str, label: &'static str, n: Option<usize>| {
            let on = here == place;
            let id = match place {
                Place::Inbox => "place-inbox",
                Place::Requests => "place-requests",
                Place::ReviewRequests => "place-review",
                Place::MyRequests => "place-mine",
                Place::Commits => "place-commits",
                Place::Branches => "place-branches",
                Place::WorkingTree => "place-work",
                Place::Settings => "place-settings",
            };
            // Folded, a badge says something waits for you; an open request does not.
            let n = n.filter(|&n| n > 0 && !(folded && place == Place::Requests));
            div()
                .id(id)
                .relative()
                .h(px(32.))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(10.))
                .when(folded, |s| s.justify_center())
                .when(!folded, |s| s.px(px(8.)))
                .rounded(px(super::ROW_RADIUS))
                .cursor_pointer()
                .text_color(if on { theme::text() } else { theme::muted() })
                .when(on, |s| s.bg(theme::selected()))
                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                .group(id)
                .child(
                    icons::icon(icon)
                        .size(px(17.))
                        .text_color(if on { theme::text() } else { theme::muted() })
                        .group_hover(id, |s| s.text_color(theme::text())),
                )
                .when(!folded, |s| {
                    s.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(super::words::say(label).into_owned()),
                    )
                    .children(n.map(|n| {
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(n.to_string())
                    }))
                })
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
                    .tooltip_show_delay(crate::ui::hover::LONG)
                    .tooltip(move |_, cx| {
                        cx.new(|_| super::TipOwned(super::words::say(label).into_owned().into()))
                            .into()
                    })
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.go_place(place, cx)))
        };
        let heading = |text: &'static str| {
            if folded {
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
                    .pt(px(14.))
                    .pb(px(4.))
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(text)
            }
        };
        let requests = self.requests.available;
        Some(
            div()
                .id("places")
                .w(px(if folded { ICONS } else { OPEN }))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(2.))
                .px(px(if folded { 6. } else { 8. }))
                .pb(px(4.))
                .overflow_hidden()
                .children(
                    requests
                        .then(|| item(Place::Inbox, "inbox", "Inbox", Some(self.notices.unread()))),
                )
                .children(requests.then(|| {
                    item(
                        Place::Requests,
                        "pull-request",
                        "Merge requests",
                        Some(count(Scope::All)),
                    )
                }))
                .children((requests && me).then(|| {
                    item(
                        Place::ReviewRequests,
                        "user-check",
                        "Review requests",
                        Some(count(Scope::ReviewMe)),
                    )
                }))
                .children((requests && me).then(|| {
                    item(
                        Place::MyRequests,
                        "eye",
                        "My requests",
                        Some(count(Scope::Mine)),
                    )
                }))
                .children((!self.menu_projects().is_empty()).then(|| heading("Projects")))
                .children(self.menu_projects().into_iter().map(|(ix, tab)| {
                    let mark = super::rail::initials(&tab.title);
                    let tint = super::rail::hue(&tab.title);
                    div()
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
                            .children(
                                (!tab.active && tab.unread > 0).then(|| {
                                    div()
                                        .flex_none()
                                        .text_size(px(11.))
                                        .text_color(theme::accent())
                                        .child(tab.unread.to_string())
                                }),
                            )
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.with_shell(window, cx, move |s, window, cx| {
                                s.select(ix, window, cx)
                            })
                        }))
                }))
                .child(heading("Repository"))
                .child(item(Place::Commits, "commit", "Commits", None))
                .child(item(Place::Branches, "branch", "Branches", None))
                .child(item(
                    Place::WorkingTree,
                    "files",
                    "Working tree",
                    Some(self.wt.count),
                ))
                .child(div().flex_1())
                .child(item(Place::Settings, "settings", "Settings", None)),
        )
    }
}
