//! The menu of places on the left (Design `design/shell/` `nav()`, #49): Inbox, Merge requests (every
//! open one, #57), Review requests and My requests, then the repository's Commits, Branches and Working tree, Settings at the foot. It
//! folds to icons (the title bar's first button), and is drawn folded while a merge request is
//! reviewed unless opened there. The Inbox is a place of its own: the notifications listed, the
//! selected one previewed (Design flow 05 step 1, flow 06 step 3).

use super::notifications::Kind;
use super::requests::{Scope, Side};
use super::{Selection, Workspace, chip, format, icons, island, px, theme};
use gpui::{ClickEvent, Context, FontWeight, SharedString, div, prelude::*};

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

/// Which notifications the Inbox lists.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum InboxFilter {
    #[default]
    All,
    Mentions,
    Reviews,
}

impl InboxFilter {
    fn lets(self, kind: Kind) -> bool {
        match self {
            InboxFilter::All => true,
            InboxFilter::Mentions => kind == Kind::Mention,
            InboxFilter::Reviews => kind == Kind::Review,
        }
    }
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

    /// The notices the Inbox lists under its filter, as indexes of `notices.list`.
    pub(super) fn inbox_rows(&self) -> Vec<usize> {
        self.notices
            .list
            .iter()
            .enumerate()
            .filter(|(_, n)| self.inbox_filter.lets(n.kind))
            .map(|(ix, _)| ix)
            .collect()
    }

    /// j k in the Inbox: the next or previous listed notice.
    pub(super) fn inbox_step(&mut self, down: bool, cx: &mut Context<Self>) {
        let rows = self.inbox_rows();
        if rows.is_empty() {
            return;
        }
        let at = rows.iter().position(|&ix| ix == self.notices.at);
        let next = match (at, down) {
            (Some(i), true) => (i + 1).min(rows.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
            (None, _) => 0,
        };
        self.notices.at = rows[next];
        cx.notify();
    }

    /// The Inbox's header row: its name, how many are new, the filters, Mark all read.
    pub(super) fn render_inbox_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let unread = self.notices.unread();
        let filter = |f: InboxFilter, label: &'static str| {
            let id = match f {
                InboxFilter::All => "inbox-all",
                InboxFilter::Mentions => "inbox-mentions",
                InboxFilter::Reviews => "inbox-reviews",
            };
            chip(id, label, self.inbox_filter == f).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.inbox_filter = f;
                    if let Some(&first) = this.inbox_rows().first() {
                        this.notices.at = first;
                    }
                    cx.notify();
                },
            ))
        };
        island()
            .flex_none()
            .flex_row()
            .items_center()
            .gap_3()
            .px_4()
            .py(px(10.))
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Inbox"),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(if unread > 0 {
                        format!("{unread} new")
                    } else {
                        "Nothing new".to_owned()
                    }),
            )
            .child(
                super::diff_view::group()
                    .child(filter(InboxFilter::All, "All"))
                    .child(filter(InboxFilter::Mentions, "Mentions"))
                    .child(filter(InboxFilter::Reviews, "Reviews")),
            )
            .children((unread > 0).then(|| {
                chip("inbox-read-all", "Mark all read", false)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.read_all_notices(cx)))
            }))
    }

    /// The Inbox's panes: the list, and the selected notice previewed.
    pub(super) fn render_inbox(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let rows = self.inbox_rows();
        let selected = self
            .notices
            .list
            .get(self.notices.at)
            .filter(|_| rows.contains(&self.notices.at));
        let list = rows.iter().map(|&ix| {
            let n = &self.notices.list[ix];
            let fresh = self.notice_unread(ix);
            let on = ix == self.notices.at;
            div()
                .id(("inbox-row", ix))
                .mx(px(6.))
                .px(px(10.))
                .py(px(8.))
                .flex()
                .items_start()
                .gap(px(10.))
                .rounded(px(super::ROW_RADIUS))
                .cursor_pointer()
                .when(on, |s| s.bg(theme::selected()))
                .hover(|s| s.bg(theme::hover()))
                .child(
                    div()
                        .flex_none()
                        .mt(px(6.))
                        .size(px(7.))
                        .rounded_full()
                        .when(fresh, |s| s.bg(theme::accent())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .when(fresh, |s| s.font_weight(FontWeight::SEMIBOLD))
                                        .child(format!("{} {}", n.who, n.kind_verb())),
                                )
                                .child(
                                    super::hover::ago(
                                        gpui::SharedString::from(format!(
                                            "inbox-ago-{}",
                                            n.created
                                        )),
                                        n.created,
                                    )
                                    .text_size(px(11.))
                                    .text_color(theme::faint()),
                                ),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(theme::accent())
                                .child(format!("{} {}", super::words::number(n.iid), n.title)),
                        ),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.notices.at = ix;
                    cx.notify();
                }))
        });
        let preview = match selected {
            Some(n) => {
                let ix = self.notices.at;
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p(px(20.))
                    .child(
                        div()
                            .text_color(theme::muted())
                            .child(format!("{} {} · {}", n.who, n.kind_verb(), format::ago(n.created))),
                    )
                    .child(
                        div()
                            .text_size(px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{} {}", super::words::number(n.iid), n.title)),
                    )
                    .children((!n.text.trim().is_empty()).then(|| {
                        div()
                            .px(px(14.))
                            .py(px(10.))
                            .rounded(px(10.))
                            .bg(theme::panel())
                            .text_color(theme::text())
                            .child(SharedString::from(n.text.clone()))
                    }))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                chip("inbox-open", "Open  ↵", false).on_click(cx.listener(
                                    move |this, _: &ClickEvent, _, cx| {
                                        this.inbox = false;
                                        this.open_notice(ix, cx)
                                    },
                                )),
                            )
                            .child(chip("inbox-done", "Done  E", false).on_click(cx.listener(
                                move |this, _: &ClickEvent, _, cx| this.dismiss_notice(ix, cx),
                            ))),
                    )
                    .into_any_element()
            }
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::muted())
                .child(if self.notices.list.is_empty() {
                    "Nothing here. Mentions, review requests and replies in your discussions come in here."
                } else {
                    "Choose a notification."
                })
                .into_any_element(),
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(super::gap()))
            .child(
                island()
                    .w(px(self.layout.sidebar.max(320.)))
                    .flex_none()
                    .child(
                        div()
                            .id("inbox-list")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .py(px(6.))
                            .children(list),
                    ),
            )
            .child(
                island()
                    .flex_1()
                    .min_w_0()
                    .bg(theme::editor())
                    .child(preview),
            )
    }
}
