//! The page bar (#71): one 44 px line over every page — the path ending in the item's id, each
//! part above it a way up; "‹ Inbox" while a page reached from another space is on screen; the
//! page's view switch; its actions and one primary action. Nothing else: the item's facts are the
//! first block of its content. A request's bar carries the public orange along its top.

use super::nav::Page;
use super::spaces::Space;
use super::{Workspace, format, island, keys, px, theme, words};
use gpui::{AnyElement, ClickEvent, Context, FontWeight, SharedString, div, prelude::*};

/// A part of the path: its words, and the page it goes up to.
struct Crumb {
    label: String,
    mono: bool,
    to: Option<Option<Page>>,
}

impl Crumb {
    fn up(label: impl Into<String>, to: Option<Page>) -> Self {
        Self {
            label: label.into(),
            mono: false,
            to: Some(to),
        }
    }

    fn plain(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            mono: false,
            to: None,
        }
    }
}

impl Workspace {
    /// The bar of the page on screen; None in zen, which has its own line.
    pub(super) fn render_page_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.zen.is_some() {
            return None;
        }
        let space = self.nav.space;
        let page = self.nav_page().cloned();
        let (path, id, mono) = self.path_of(space, page.as_ref());
        // Narrow, the path is ‹ — one level up — and the id (#80).
        let up = self
            .compact()
            .then(|| path.iter().rev().find_map(|c| c.to.clone()))
            .flatten();
        let path = if self.compact() { Vec::new() } else { path };
        let count = self.count_of(space, page.as_ref());
        let public = space == Space::Requests && page.is_some();
        // Narrow, ‹ is one level up; ⌘[ and the title bar still go back (#80).
        let from = self.nav.from.filter(|_| page.is_some() && !self.compact());
        let crumbs = path.into_iter().enumerate().map(|(n, crumb)| {
            let link = crumb.to.clone();
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .id(SharedString::from(format!("crumb-{n}")))
                        .when(crumb.mono, |s| s.font_family(theme::CODE_FONT))
                        .child(words::say(&crumb.label).into_owned())
                        .when_some(link, |s, to| {
                            s.cursor_pointer()
                                .hover(|s| s.text_color(theme::accent()))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.nav.from = None;
                                    this.show(space, to.clone(), cx)
                                }))
                        }),
                )
                .child(div().text_color(theme::faint()).child("›"))
        });
        let bar = island()
            .relative()
            .overflow_hidden()
            .flex_none()
            .flex_row()
            .h(px(44.))
            .items_center()
            .gap(px(12.))
            .pl(px(14.))
            .pr(px(8.))
            .whitespace_nowrap()
            .children(from.map(|from| {
                div()
                    .id("bar-from")
                    .flex_none()
                    .px(px(8.))
                    .rounded_full()
                    .bg(theme::hover())
                    .text_size(px(11.))
                    .text_color(theme::muted())
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme::text()))
                    .child(format!("‹ {}", words::say(space_name(from))))
                    .tooltip_show_delay(super::hover::LONG)
                    .tooltip(|_, cx| {
                        cx.new(|_| super::Tip("Back where you came from  ⌘["))
                            .into()
                    })
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_back(cx)))
            }))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .overflow_hidden()
                    .text_color(theme::muted())
                    .children(up.map(|to| {
                        div()
                            .id("crumb-up")
                            .flex_none()
                            .px(px(4.))
                            .rounded(px(super::ROW_RADIUS))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                            .child("‹")
                            .tooltip_show_delay(super::hover::LONG)
                            .tooltip(|_, cx| cx.new(|_| super::Tip("Up a level  esc")).into())
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.nav.from = None;
                                this.show(space, to.clone(), cx)
                            }))
                    }))
                    .children(crumbs)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .when(mono, |s| s.font_family(theme::CODE_FONT))
                            .child(words::say(&id).into_owned()),
                    )
                    .children(count.map(|n| {
                        div()
                            .flex_none()
                            .ml(px(4.))
                            .text_size(px(12.))
                            .text_color(theme::faint())
                            .child(n)
                    })),
            )
            .children(self.view_of(space, page.as_ref(), cx))
            .child(div().flex_1().min_w_0())
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .children(self.actions_of(space, page.as_ref(), cx)),
            )
            // Public: what is done here is seen on GitLab (#71).
            // Over the island's own border, its top edge turning down with its round corners (a
            // strip would cross them square).
            .when(public, |s| {
                s.child(
                    div()
                        .absolute()
                        .top(px(-1.))
                        .left(px(-1.))
                        .right(px(-1.))
                        .bottom(px(-1.))
                        .rounded(px(if super::flat() {
                            0.
                        } else {
                            super::ISLAND_RADIUS
                        }))
                        .border_t_2()
                        .border_color(theme::warning()),
                )
            });
        Some(bar.into_any_element())
    }

    /// The path above the page, and its own id.
    fn path_of(&self, space: Space, page: Option<&Page>) -> (Vec<Crumb>, String, bool) {
        let project = self
            .root
            .as_ref()
            .and_then(|r| r.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let branch = |refname: &str| {
            self.branches
                .iter()
                .find(|b| b.refname == refname)
                .map_or_else(|| refname.to_owned(), |b| b.name.clone())
        };
        let versions = |a: usize, b: usize| match (self.versions.get(a), self.versions.get(b)) {
            (Some(a), Some(b)) => format!("v{} → v{}", a.number, b.number),
            _ => "versions".to_owned(),
        };
        let requests = || Crumb::up("Merge requests", None);
        match page {
            None => (Vec::new(), space_root(space, &project), false),
            Some(Page::File(path) | Page::Folder(path)) => {
                let mut parts: Vec<&str> = path.split('/').collect();
                let name = parts.pop().unwrap_or_default().to_owned();
                let mut crumbs = vec![Crumb::up("Code", None), Crumb::up(project, None)];
                crumbs.extend(parts.into_iter().map(Crumb::plain));
                (crumbs, name, false)
            }
            Some(Page::Branch(refname)) => {
                (vec![Crumb::up("Branches", None)], branch(refname), true)
            }
            // A commit is a level under the branch's Commits (#74).
            Some(Page::BranchCommit(refname, id)) => {
                let up = |label: String, mono: bool| Crumb {
                    label,
                    mono,
                    to: Some(Some(Page::Branch(refname.clone()))),
                };
                (
                    vec![
                        Crumb::up("Branches", None),
                        up(branch(refname), true),
                        up("Commits".to_owned(), false),
                    ],
                    format::short(*id),
                    true,
                )
            }
            Some(Page::Versions(refname, a, b)) => {
                let up = Crumb {
                    label: branch(refname),
                    mono: true,
                    to: Some(Some(Page::Branch(refname.clone()))),
                };
                (
                    vec![Crumb::up("Branches", None), up],
                    versions(*a, *b),
                    false,
                )
            }
            Some(Page::WorkingTree) => (
                vec![Crumb::up("Branches", None)],
                "Working tree".to_owned(),
                false,
            ),
            Some(Page::Compare(base, head)) => (
                vec![Crumb::up("Branches", None)],
                format!("{} … {}", base.0, head.0),
                true,
            ),
            Some(Page::Request(iid)) => (vec![requests()], words::number(*iid), false),
            // A commit is a level under the request's Commits (#73).
            Some(Page::RequestCommit(iid, id)) => (
                vec![
                    requests(),
                    Crumb::up(words::number(*iid), Some(Page::Request(*iid))),
                    Crumb::up("Commits", Some(Page::Request(*iid))),
                ],
                format::short(*id),
                true,
            ),
            Some(Page::RequestCommits(iid, newest, oldest)) => (
                vec![
                    requests(),
                    Crumb::up(words::number(*iid), Some(Page::Request(*iid))),
                    Crumb::up("Commits", Some(Page::Request(*iid))),
                ],
                format!("{}..{}", format::short(*oldest), format::short(*newest)),
                true,
            ),
            Some(Page::BranchCommits(refname, newest, oldest)) => {
                let up = |label: String, mono: bool| Crumb {
                    label,
                    mono,
                    to: Some(Some(Page::Branch(refname.clone()))),
                };
                (
                    vec![
                        Crumb::up("Branches", None),
                        up(branch(refname), true),
                        up("Commits".to_owned(), false),
                    ],
                    format!("{}..{}", format::short(*oldest), format::short(*newest)),
                    true,
                )
            }
            Some(Page::RequestVersions(iid, a, b)) => (
                vec![
                    requests(),
                    Crumb::up(words::number(*iid), Some(Page::Request(*iid))),
                ],
                versions(*a, *b),
                false,
            ),
        }
    }

    /// The faint count after a root's name.
    fn count_of(&self, space: Space, page: Option<&Page>) -> Option<String> {
        match (space, page) {
            (Space::Inbox, _) => Some(match self.notices.unread() {
                0 => "Nothing new".to_owned(),
                n => format!("{n} new"),
            }),
            // Once the filters say it with their counts, the name has none.
            (Space::Requests, None)
                if self.requests.available
                    && self.requests.login.is_none()
                    && !self.requests.list.is_empty() =>
            {
                Some(self.requests.list.len().to_string())
            }
            (Space::Branches, None) => Some(self.branches_count()),
            (Space::Code, Some(Page::File(_))) => {
                self.file_view.as_ref().map(|v| format!("as of {}", v.at()))
            }
            (Space::Code, Some(Page::Folder(path))) => {
                let inside = format!("{path}/");
                let n = self
                    .code
                    .files()
                    .iter()
                    .filter(|f| f.starts_with(&inside))
                    .count();
                Some(super::plural(n, "file"))
            }
            _ => None,
        }
    }

    /// The page's view switch: the Inbox's filters, the requests list's, a request's section
    /// tabs (#73).
    fn view_of(
        &self,
        space: Space,
        page: Option<&Page>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match (space, page) {
            (Space::Inbox, _) => Some(self.inbox_filters(cx).into_any_element()),
            (Space::Requests, None) if self.requests.available => {
                self.request_filters(cx).map(IntoElement::into_any_element)
            }
            (Space::Requests | Space::Branches, Some(_)) => {
                self.section_tabs(cx).map(IntoElement::into_any_element)
            }
            _ => None,
        }
    }

    /// The page's actions, its primary one last; a list's filter (#72).
    fn actions_of(
        &self,
        space: Space,
        page: Option<&Page>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = Vec::new();
        match (space, page) {
            (Space::Requests, None) if self.requests.available => {
                out.push(self.bar_filter("Filter", cx))
            }
            (Space::Branches, None) => out.push(self.bar_filter("Filter branches", cx)),
            (Space::Inbox, _) if self.notices.unread() > 0 => out.push(
                self.bar_action("bar-read-all", "check", "Mark all read", "")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.read_all_notices(cx)))
                    .into_any_element(),
            ),
            // A folder's page: a note for your agent on it (#97), then Open in.
            (Space::Code, Some(Page::Folder(path))) => {
                let folder = path.clone();
                out.push(
                    self.bar_action("bar-folder-note", "bot", "Note for your agent", "C")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.start_folder_comment(&folder, cx)
                        }))
                        .into_any_element(),
                );
                out.extend(self.open_in_button(cx));
            }
            // The file or the project's folder in the app it belongs to, named (#78).
            (Space::Code, _) => out.extend(self.open_in_button(cx)),
            (Space::Branches, Some(Page::Branch(refname))) => {
                let ix = self.branches.iter().position(|b| &b.refname == refname);
                match ix.and_then(|ix| self.request_of_branch(ix).map(|m| m.iid)) {
                    // Its request has a page of its own: the way there.
                    Some(iid) => out.push(
                        self.bar_action(
                            "bar-request",
                            "pull-request",
                            &format!("{} →", words::number(iid)),
                            "",
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.select_request(iid, cx)
                        }))
                        .into_any_element(),
                    ),
                    // Not while one is being written: its own Create is the primary then.
                    None if self.requests.available && self.newreq.is_none() => out.push(
                        bar_button("bar-create", "Create merge request", "⌥⌘M", true)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.open_create(ix, cx)
                            }))
                            .into_any_element(),
                    ),
                    None => {}
                }
            }
            (Space::Requests, Some(Page::RequestCommit(..)))
            | (Space::Branches, Some(Page::BranchCommit(..))) => {
                out.push(
                    self.bar_action("bar-previous-commit", "back", "Previous", "[")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.step_item_commit(false, cx);
                        }))
                        .into_any_element(),
                );
                out.push(
                    self.bar_action("bar-next-commit", "forward", "Next", "]")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.step_item_commit(true, cx);
                        }))
                        .into_any_element(),
                );
            }
            (Space::Requests, Some(Page::Request(_))) if self.showing_request() => {
                let pending = self.requests.drafts.len();
                let label = match pending {
                    0 => "Submit review".to_owned(),
                    n => format!("Submit review · {n}"),
                };
                out.push(
                    self.bar_action("bar-submit", "send", &label, "⇧⌘↵")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_submit(cx)))
                        .into_any_element(),
                );
                out.extend(
                    self.render_approval_chip(cx)
                        .map(IntoElement::into_any_element),
                );
            }
            _ => {}
        }
        out
    }
}

/// A root's name: the space's, Code's the project's.
fn space_root(space: Space, project: &str) -> String {
    match space {
        Space::Code => project.to_owned(),
        _ => space_name(space).to_owned(),
    }
}

fn space_name(space: Space) -> &'static str {
    match space {
        Space::Inbox => "Inbox",
        Space::Code => "Code",
        Space::Branches => "Branches",
        Space::Requests => "Merge requests",
    }
}

impl Workspace {
    /// A secondary action of the bar: its label and key, or in a narrow window its icon alone,
    /// the label and the key in its tooltip (#80).
    pub(super) fn bar_action(
        &self,
        id: &'static str,
        icon: &'static str,
        label: &str,
        key: &'static str,
    ) -> gpui::Stateful<gpui::Div> {
        if !self.compact() {
            return bar_button(id, label, key, false);
        }
        let tip = if key.is_empty() {
            words::say(label).into_owned()
        } else {
            format!("{}  {key}", words::say(label))
        };
        bar_icon(id, icon, tip)
    }
}

/// A bar's button as its icon alone, what it does in its tooltip.
pub(super) fn bar_icon(
    id: &'static str,
    icon: &'static str,
    tip: String,
) -> gpui::Stateful<gpui::Div> {
    use super::hover::Hover;
    div()
        .id(id)
        .flex_none()
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(super::ROW_RADIUS))
        .border_1()
        .border_color(theme::island_border())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(
            super::icons::icon(icon)
                .size(px(15.))
                .text_color(theme::muted()),
        )
        .tip(tip)
}

/// A button of the bar: its label and its key as caps; the primary one filled.
pub(super) fn bar_button(
    id: &'static str,
    label: &str,
    key: &'static str,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(super::ROW_RADIUS))
        .cursor_pointer()
        .text_size(px(13.))
        .when(primary, |s| {
            s.bg(theme::accent())
                .text_color(theme::on_accent())
                .font_weight(FontWeight::MEDIUM)
                .hover(|s| s.opacity(0.9))
        })
        .when(!primary, |s| {
            s.border_1()
                .border_color(theme::island_border())
                .hover(|s| s.bg(theme::hover()))
        })
        .child(words::say(label).into_owned())
        .when(!key.is_empty(), |s| {
            s.child(keys::caps(
                key,
                if primary {
                    keys::On::Filled
                } else {
                    keys::On::Plain
                },
            ))
        })
}
