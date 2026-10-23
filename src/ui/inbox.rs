//! The Inbox, a place of the menu (#49), as Design L4 draws it (STYLE.md: "notifications listed,
//! the selected one previewed and answerable"): the notifications on the left under Today and
//! Earlier; on the right the selected one's request, the code its discussion is on (the lines
//! around it as the request changed them), the whole discussion, and a reply written there —
//! Reply ⌘↵, Resolve. A notice with no discussion (a mention in the request's description, say)
//! is answered with a comment on the request; a review request is only opened. j k move, ↵ opens
//! the request, e is done with it, r replies. Per project: the open repository's notices.

use super::comments::Target;
use super::discussion::{self, Kind as Button, Source};
use super::keys::{self, On};
use super::notifications::{Kind, Notice};
use super::{Workspace, chip, format, icons, island, markdown, px, theme, words};
use crate::git::{DiffLine, DiffSettings, LineKind, Repo};
use crate::mr::Thread;
use gpui::{AnyElement, ClickEvent, Context, Div, FontWeight, Task, div, prelude::*};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Lines shown above and below the commented one.
const ABOVE: usize = 3;
const BELOW: usize = 2;

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

/// The code a discussion is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Excerpt {
    pub lines: Vec<ExcerptLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExcerptLine {
    pub kind: LineKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
    /// The line the discussion is on.
    pub on: bool,
}

#[derive(Default)]
pub struct InboxView {
    pub filter: InboxFilter,
    /// By discussion id; `None` once read and not found (the commit is not here).
    excerpts: HashMap<String, Option<Excerpt>>,
    task: Option<Task<()>>,
    /// What the composer writes to: request, and its discussion (none: a comment on the request).
    pub reply: Option<(u64, Option<String>)>,
}

impl Workspace {
    /// The notices the Inbox lists under its filter, as indexes of `notices.list`.
    pub(super) fn inbox_rows(&self) -> Vec<usize> {
        self.notices
            .list
            .iter()
            .enumerate()
            .filter(|(_, n)| self.inbox_view.filter.lets(n.kind))
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
        self.inbox_select(rows[next], cx);
    }

    fn inbox_select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix != self.notices.at {
            self.drop_inbox_reply();
        }
        self.notices.at = ix;
        self.inbox_selected(cx);
        cx.notify();
    }

    /// An empty reply goes when another notice is chosen; one with text stays until sent or
    /// cancelled.
    fn drop_inbox_reply(&mut self) {
        if self
            .compose
            .as_ref()
            .is_some_and(|c| c.target == Target::InboxReply && c.edit.text().trim().is_empty())
        {
            self.compose = None;
        }
    }

    /// The selected notice's discussion, when it has one and it was read.
    fn inbox_thread(&self, n: &Notice) -> Option<&Thread> {
        self.notices.thread_of(n.iid, n.note?)
    }

    /// The selected notice changed (or its discussions came in): reads the code its discussion
    /// is on, once, in the background.
    pub(super) fn inbox_selected(&mut self, cx: &mut Context<Self>) {
        let Some(n) = self.notices.list.get(self.notices.at) else {
            return;
        };
        let Some(t) = self.inbox_thread(n) else {
            return;
        };
        if self.inbox_view.excerpts.contains_key(&t.id) {
            return;
        }
        let (Some(root), Some(path)) = (self.root.clone(), t.path.clone()) else {
            return;
        };
        let Some(m) = self.requests.list.iter().find(|m| m.iid == n.iid) else {
            return;
        };
        let heads: Vec<String> = t.commit.iter().cloned().chain([m.head.clone()]).collect();
        let (id, target, new, old) = (t.id.clone(), m.target.clone(), t.new_line, t.old_line);
        self.inbox_view.task = Some(cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { excerpt(&root, &target, &heads, &path, new, old) })
                .await;
            this.update(cx, |this, cx| {
                this.inbox_view.excerpts.insert(id, found);
                cx.notify();
            })
            .ok();
        }));
    }

    /// r in the Inbox: the composer under the selected notice. A review request has nothing to
    /// answer.
    pub(super) fn reply_in_inbox(&mut self, cx: &mut Context<Self>) {
        let Some(n) = self.notices.list.get(self.notices.at) else {
            return;
        };
        if matches!(n.kind, Kind::Review | Kind::Todo) {
            return;
        }
        let iid = n.iid;
        let (discussion, spot) = match self.inbox_thread(n) {
            Some(t) => (
                Some(t.id.clone()),
                t.path.clone().zip(t.new_line.or(t.old_line)),
            ),
            None => (None, None),
        };
        self.start_inbox_reply(iid, discussion, spot, cx);
    }

    /// Resolves (or opens again) discussion `id` of request `iid` from the Inbox.
    fn inbox_resolve(&mut self, iid: u64, id: String, resolved: bool, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { crate::mr::resolve(&remote, iid, &id, resolved) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => this.inbox_posted(iid, cx),
                Err(e) => {
                    this.error = Some(format!("{e:#}").into());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Something was posted to request `iid` from the Inbox: its discussions are read again.
    pub(super) fn inbox_posted(&mut self, iid: u64, cx: &mut Context<Self>) {
        self.notices.forget(iid);
        self.load_notices(cx);
    }

    /// The Inbox's filters, in its page bar (#71).
    pub(super) fn inbox_filters(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let filter = |f: InboxFilter, label: &'static str| {
            let id = match f {
                InboxFilter::All => "inbox-all",
                InboxFilter::Mentions => "inbox-mentions",
                InboxFilter::Reviews => "inbox-reviews",
            };
            chip(id, label, self.inbox_view.filter == f).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.inbox_view.filter = f;
                    if let Some(&first) = this.inbox_rows().first() {
                        this.inbox_select(first, cx);
                    }
                    cx.notify();
                },
            ))
        };
        super::diff_view::group()
            .child(filter(InboxFilter::All, "All"))
            .child(filter(InboxFilter::Mentions, "Mentions"))
            .child(filter(InboxFilter::Reviews, "Reviews"))
    }

    /// The Inbox: its panes, or rows loading before the first read, or one message when nothing
    /// is listed; a banner over it while the host cannot be read (#72).
    pub(super) fn render_inbox(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let rows = self.inbox_rows();
        let body = if !self.notices.loaded && self.notices.list.is_empty() {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap(px(super::gap()))
                .child(
                    island()
                        .w(px(self.layout.sidebar.max(360.)))
                        .flex_none()
                        .p(px(16.))
                        .child(super::roots::skeleton(6)),
                )
                .child(
                    island()
                        .flex_1()
                        .bg(theme::editor())
                        .px(px(28.))
                        .py(px(26.))
                        .child(super::roots::skeleton(3)),
                )
                .into_any_element()
        } else if rows.is_empty() {
            let (title, text) = match self.inbox_view.filter {
                InboxFilter::All => (
                    "Nothing needs you",
                    "Review requests, mentions and replies in your discussions land here.",
                ),
                InboxFilter::Mentions => (
                    "No mentions",
                    "When someone mentions you in this project, it lands here.",
                ),
                InboxFilter::Reviews => (
                    "No review requests",
                    "When someone asks for your review, it lands here.",
                ),
            };
            let all = (self.inbox_view.filter != InboxFilter::All && !self.notices.list.is_empty())
                .then(|| {
                    div()
                        .id("inbox-show-all")
                        .child(super::roots::bar_button("Show all"))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.inbox_view.filter = InboxFilter::All;
                            if let Some(&first) = this.inbox_rows().first() {
                                this.inbox_select(first, cx);
                            }
                            cx.notify();
                        }))
                        .into_any_element()
                });
            island()
                .flex_1()
                .bg(theme::editor())
                // Its button's room kept when there is none, so switching the filter moves
                // nothing.
                .child(super::roots::empty_state(
                    "inbox",
                    title.to_owned(),
                    text.to_owned(),
                    Some(all.unwrap_or_else(|| div().h(px(28.)).into_any_element())),
                ))
                .into_any_element()
        } else {
            self.render_inbox_panes(&rows, cx).into_any_element()
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(super::gap()))
            .children(self.list_banner(cx))
            .child(body)
    }

    /// The Inbox's panes: the list, and the selected notice previewed and answerable.
    fn render_inbox_panes(
        &self,
        rows: &[usize],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let today = format::date(now());
        let mut list: Vec<AnyElement> = Vec::new();
        let mut group = None;
        for &ix in rows {
            let n = &self.notices.list[ix];
            let heading = if format::date(n.created) == today {
                "Today"
            } else {
                "Earlier"
            };
            if group != Some(heading) {
                group = Some(heading);
                list.push(
                    div()
                        .px(px(16.))
                        .pt(px(10.))
                        .pb(px(4.))
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(heading)
                        .into_any_element(),
                );
            }
            list.push(self.render_inbox_row(ix, cx).into_any_element());
        }
        let preview = self
            .notices
            .list
            .get(self.notices.at)
            .filter(|_| rows.contains(&self.notices.at))
            .map(|n| self.render_inbox_preview(n, cx).into_any_element())
            .unwrap_or_else(|| {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme::muted())
                    .child("Choose a notification.")
                    .into_any_element()
            });
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(super::gap()))
            .child(
                if self.compact() {
                    island().flex_1().min_w_0()
                } else {
                    island()
                        .w(px(super::roots::list_width(self.layout.sidebar.max(360.))))
                        .flex_none()
                }
                .relative()
                .child(self.widths.measure("inbox"))
                .child(
                    div()
                        .id("inbox-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .pb(px(6.))
                        .children(list),
                ),
            )
            // Narrow, the list is the page and a notice opens where it points (#80).
            .children((!self.compact()).then(|| {
                island()
                    .flex_1()
                    .min_w_0()
                    .bg(theme::editor())
                    .child(
                        div()
                            .id("inbox-preview")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(preview),
                    )
                    .child(inbox_keys())
            }))
    }

    /// One notice: unread dot, who did what, the request always; when, and the start of what was
    /// written, while the list has 260 px (#79); all of it in its tooltip.
    fn render_inbox_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        use super::hover::Hover;
        let n = &self.notices.list[ix];
        let fresh = self.notice_unread(ix);
        let on = ix == self.notices.at;
        let room = self.widths.room("inbox");
        let text = crate::markdown::plain(&n.text);
        let gist = text
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim();
        let mut tip = vec![
            format!("{} {}", n.who, n.kind_verb()),
            format!("{} {}", words::number(n.iid), n.title),
            super::hover::full_date(n.created),
        ];
        if !text.trim().is_empty() {
            tip.push(text.trim().to_owned());
        }
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
                            .children(room.second().then(|| {
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .child(format::ago(n.created))
                            })),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(theme::accent())
                            .child(format!("{} {}", words::number(n.iid), n.title)),
                    )
                    .children((room.second() && !gist.is_empty()).then(|| {
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(gist.to_owned())
                    })),
            )
            .tip(tip.join("\n"))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if this.compact() {
                    this.inbox = false;
                    return this.open_notice(ix, cx);
                }
                this.inbox_select(ix, cx)
            }))
    }

    /// The selected notice: its request, the code, the discussion, the reply.
    fn render_inbox_preview(&self, n: &Notice, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ix = self.notices.at;
        let mr = self.requests.list.iter().find(|m| m.iid == n.iid);
        let thread = self.inbox_thread(n);
        let me = self.requests.login.as_deref().unwrap_or_default();
        let props = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .text_size(px(12.))
            .text_color(theme::muted())
            .child(
                div()
                    .font_family(theme::CODE_FONT)
                    .text_color(theme::accent())
                    .child(words::number(n.iid)),
            )
            .children(mr.map(|m| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        icons::icon("pull-request")
                            .size(px(14.))
                            .text_color(theme::muted()),
                    )
                    .child(format!("{} · {} → {}", m.author, m.source, m.target))
            }))
            .child(div().flex_1())
            .child(format!(
                "{} {} · {}",
                n.who,
                n.kind_verb(),
                format::ago(n.created)
            ));
        let title = div()
            .text_size(px(19.))
            .line_height(px(26.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(n.title.clone());
        let code = thread.and_then(|t| {
            let ex = self.inbox_view.excerpts.get(&t.id)?;
            let path = t.path.clone()?;
            let line = t.new_line.or(t.old_line)?;
            Some(excerpt_view(&path, line, ex.as_ref()))
        });
        // The discussion, or what the notice says when there is none (a mention in a description).
        let talk = match thread {
            Some(t) => {
                let last = t.notes.len().saturating_sub(1);
                let notes = t.notes.iter().enumerate().map(|(i, note)| {
                    discussion::note(
                        &note.author,
                        note.login == me,
                        None,
                        format::ago(note.created),
                        markdown::body(
                            &note.body,
                            self.settings.rich_comments,
                            format!("md-inbox-{}", note.id),
                            None,
                            None,
                        ),
                        i == last,
                        None,
                        None,
                    )
                });
                let (iid, id, resolved) = (n.iid, t.id.clone(), t.resolved);
                discussion::frame(Source::GitLab, n.note.is_some())
                    .children(notes)
                    .children(t.resolvable.then(|| {
                        div().flex().justify_end().pt(px(6.)).child(
                            discussion::button(
                                "inbox-resolve",
                                if resolved { "Reopen" } else { "Resolve" },
                                None,
                                Button::Plain,
                            )
                            .on_click(cx.listener(
                                move |this, _: &ClickEvent, _, cx| {
                                    this.inbox_resolve(iid, id.clone(), !resolved, cx)
                                },
                            )),
                        )
                    }))
                    .into_any_element()
            }
            None if !n.text.trim().is_empty() => discussion::frame(Source::GitLab, false)
                .child(discussion::note(
                    &n.who,
                    false,
                    None,
                    format::ago(n.created),
                    markdown::body(
                        &n.text,
                        self.settings.rich_comments,
                        format!("md-inbox-{}", n.key),
                        None,
                        None,
                    ),
                    true,
                    None,
                    None,
                ))
                .into_any_element(),
            None => div().into_any_element(),
        };
        let answerable = !matches!(n.kind, Kind::Review | Kind::Todo);
        let writing = self
            .compose
            .as_ref()
            .is_some_and(|c| c.target == Target::InboxReply);
        let composer = answerable.then(|| {
            if writing {
                self.reply_editor(cx.entity().downgrade())
                    .map(|e| e.max_w(px(720.)).into_any_element())
                    .unwrap_or_else(|| div().into_any_element())
            } else {
                let placeholder = if thread.is_some() {
                    format!("Reply to {}…", n.who)
                } else {
                    format!("Comment on {}…", words::number(n.iid))
                };
                div()
                    .id("inbox-reply")
                    .max_w(px(720.))
                    .h(px(36.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme::island_border())
                    .bg(theme::panel())
                    .cursor_text()
                    .text_color(theme::faint())
                    .child(div().flex_1().child(placeholder))
                    .child(keys::caps("R", On::Plain))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reply_in_inbox(cx)))
                    .into_any_element()
            }
        });
        let actions = div()
            .flex()
            .gap_2()
            .child(
                // Buttons that look like buttons, the way on filled (the developer, 2026-10-23).
                super::bar::bar_button("inbox-open", "Open request", "↵", true).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.inbox = false;
                        this.open_notice(ix, cx)
                    }),
                ),
            )
            .child(
                super::bar::bar_button("inbox-done", "Done", "E", false).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.dismiss_notice(ix, cx)),
                ),
            );
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .px(px(28.))
            .py(px(22.))
            .max_w(px(820.))
            .child(props)
            .child(title)
            .children(code)
            .child(talk)
            .children(composer)
            .child(actions)
    }
}

/// The keys of the Inbox, along the preview's foot.
fn inbox_keys() -> Div {
    let key = |caps: &[&str], what: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .children(caps.iter().map(|c| keys::caps(c, On::Plain)))
            .child(div().pl(px(2.)).child(what))
    };
    div()
        .flex_none()
        .flex()
        .gap_4()
        .px(px(16.))
        .py(px(8.))
        .border_t_1()
        .border_color(theme::island_border())
        .text_size(px(12.))
        .text_color(theme::muted())
        .child(key(&["J", "K"], "move"))
        .child(key(&["↵"], "open request"))
        .child(key(&["E"], "done"))
        .child(key(&["R"], "reply"))
}

/// The card of the code a discussion is on: the path and line, then the lines (or a word that
/// they are not here).
fn excerpt_view(path: &str, line: u32, ex: Option<&Excerpt>) -> Div {
    let head = div()
        .flex()
        .items_center()
        .gap_2()
        .px(px(12.))
        .py(px(7.))
        .text_size(px(12.))
        .text_color(theme::muted())
        .child(
            icons::icon("files")
                .size(px(14.))
                .text_color(theme::muted()),
        )
        .child(div().flex_1().min_w_0().truncate().child(path.to_owned()))
        .child(format!("line {line}"));
    let body = match ex {
        Some(ex) => div()
            .pb(px(6.))
            .font_family(theme::CODE_FONT)
            .text_size(px(12.))
            .children(ex.lines.iter().map(|l| {
                let number = |n: Option<u32>| {
                    div()
                        .w(px(36.))
                        .flex_none()
                        .text_right()
                        .pr(px(8.))
                        .text_color(theme::faint())
                        .child(n.map(|n| n.to_string()).unwrap_or_default())
                };
                let sign = match l.kind {
                    LineKind::Added => "+",
                    LineKind::Removed => "−",
                    LineKind::Context => " ",
                };
                div()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .map(|s| match l.kind {
                        LineKind::Added => s.bg(theme::added_line()),
                        LineKind::Removed => s.bg(theme::removed_line()),
                        LineKind::Context => s,
                    })
                    .border_l(px(3.))
                    .border_color(if l.on {
                        theme::warning()
                    } else {
                        gpui::transparent_black().into()
                    })
                    .child(number(l.old))
                    .child(number(l.new))
                    .child(div().w(px(16.)).flex_none().child(sign))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_color(theme::code_text())
                            .child(l.text.clone()),
                    )
            })),
        None => div()
            .px(px(12.))
            .pb(px(10.))
            .text_size(px(12.))
            .text_color(theme::faint())
            .child("The commit is not in this repository yet — fetch to see the code."),
    };
    div()
        .max_w(px(720.))
        .flex()
        .flex_col()
        .rounded(px(10.))
        .overflow_hidden()
        .border_1()
        .border_color(theme::island_border())
        .bg(theme::panel())
        .child(head)
        .child(body)
}

pub(super) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The lines around line `new` (or `old`, a removed line) of `path`, as the request changes them:
/// from where its head (the first of `heads` that is here) left `origin/<target>`. A line the
/// request did not change shows as unchanged code at the head.
fn excerpt(
    root: &Path,
    target: &str,
    heads: &[String],
    path: &str,
    new: Option<u32>,
    old: Option<u32>,
) -> Option<Excerpt> {
    let repo = Repo::open(&PathBuf::from(root)).ok()?;
    let head = heads.iter().find_map(|h| repo.resolve(h).ok())?;
    let line = new.or(old)?;
    let base = repo
        .resolve(&format!("origin/{target}"))
        .or_else(|_| repo.resolve(target))
        .ok();
    let file = base
        .and_then(|base| repo.compare(base, head, true, DiffSettings::default()).ok())
        .and_then(|c| c.files.into_iter().find(|f| f.path() == path));
    if let Some(file) = file {
        for hunk in &file.hunks {
            if let Some(at) = position(&hunk.lines, new, old) {
                let (from, to) = around(hunk.lines.len(), at);
                let old_text = file.old_text.as_deref().unwrap_or_default();
                let new_text = file.new_text.as_deref().unwrap_or_default();
                let lines = hunk.lines[from..to]
                    .iter()
                    .enumerate()
                    .map(|(i, l)| ExcerptLine {
                        kind: l.kind,
                        old: l.old_line,
                        new: l.new_line,
                        text: match (l.kind, l.old_line, l.new_line) {
                            (LineKind::Removed, Some(o), _) => nth(old_text, o),
                            (_, _, Some(n)) => nth(new_text, n),
                            _ => String::new(),
                        },
                        on: from + i == at,
                    })
                    .collect();
                return Some(Excerpt { lines });
            }
        }
    }
    // Not among the changes: the code as it is at the head.
    let text = repo.file_at(head, path)?;
    let count = text.lines().count();
    let at = (line as usize).checked_sub(1).filter(|&at| at < count)?;
    let (from, to) = around(count, at);
    Some(Excerpt {
        lines: (from..to)
            .map(|i| ExcerptLine {
                kind: LineKind::Context,
                old: None,
                new: Some(i as u32 + 1),
                text: nth(&text, i as u32 + 1),
                on: i == at,
            })
            .collect(),
    })
}

/// Where in a hunk the discussion's line is: by its new number, or a removed line by its old.
fn position(lines: &[DiffLine], new: Option<u32>, old: Option<u32>) -> Option<usize> {
    match new {
        Some(n) => lines.iter().position(|l| l.new_line == Some(n)),
        None => lines
            .iter()
            .position(|l| l.kind == LineKind::Removed && l.old_line == old),
    }
}

/// The rows shown of `len` around row `at`: `ABOVE` before it and `BELOW` after.
fn around(len: usize, at: usize) -> (usize, usize) {
    (at.saturating_sub(ABOVE), (at + BELOW + 1).min(len))
}

/// Line `n` (1-based) of `text`, without its end.
fn nth(text: &str, n: u32) -> String {
    text.lines()
        .nth(n as usize - 1)
        .map(|l| l.trim_end().to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: LineKind, old: Option<u32>, new: Option<u32>) -> DiffLine {
        DiffLine {
            kind,
            old_line: old,
            new_line: new,
        }
    }

    #[test]
    fn the_rows_around_a_line_stop_at_the_edges() {
        assert_eq!(around(10, 5), (2, 8));
        assert_eq!(around(10, 1), (0, 4));
        assert_eq!(around(4, 3), (0, 4));
    }

    #[test]
    fn a_removed_line_is_found_by_its_old_number() {
        let lines = [
            line(LineKind::Context, Some(4), Some(4)),
            line(LineKind::Removed, Some(5), None),
            line(LineKind::Added, None, Some(5)),
        ];
        assert_eq!(position(&lines, None, Some(5)), Some(1));
        assert_eq!(position(&lines, Some(5), None), Some(2));
        assert_eq!(position(&lines, Some(4), Some(4)), Some(0));
        assert_eq!(position(&lines, Some(9), None), None);
    }

    #[test]
    fn a_line_past_the_text_is_empty() {
        assert_eq!(nth("a\nb  \n", 2), "b");
        assert_eq!(nth("a\n", 3), "");
    }
}
