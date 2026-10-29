//! The Inbox, a place of the menu (#49), as Design L4 draws it (STYLE.md: "notifications listed,
//! the selected one previewed and answerable"): the notifications on the left under Today and
//! Earlier; on the right the selected one's request, the code its discussion is on (the lines
//! around it as the request changed them), the whole discussion, and a reply written there —
//! Reply ⌘↵, Resolve. A notice with no discussion (a mention in the request's description, say)
//! is answered with a comment on the request; a review request is only opened. j k move, ↵ opens
//! the request, e is done with it, r replies. Per project: the open repository's notices, and
//! among them what your agents answered on your notes (#116): a question, or "done".

use super::comments::Target;
use super::discussion::{self, Kind as Button, Source};
use super::hover::CutInTip;
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
    /// What your agents answered (#116).
    Agents,
    Reviews,
}

impl InboxFilter {
    fn lets(self, kind: Kind) -> bool {
        match self {
            InboxFilter::All => true,
            InboxFilter::Mentions => kind == Kind::Mention,
            InboxFilter::Agents => false,
            InboxFilter::Reviews => kind == Kind::Review,
        }
    }

    fn lets_agents(self) -> bool {
        matches!(self, InboxFilter::All | InboxFilter::Agents)
    }
}

/// A row of the Inbox: a notice from the host, or a note of yours an agent answered (#116), by
/// its comment's id.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InboxRow {
    Notice(usize),
    Agent(u64),
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
    /// The discussion whose code is being read: its excerpt shimmers.
    reading: Option<String>,
    task: Option<Task<()>>,
    /// What the composer writes to: request, and its discussion (none: a comment on the request).
    pub reply: Option<(u64, Option<String>)>,
    /// The agent's answer selected, by its comment's id; none: the notice at `notices.at`.
    pub agent: Option<u64>,
}

impl Workspace {
    /// What the Inbox lists under its filter, newest first: the notices, and the agents' answers
    /// waiting for you (#116).
    pub(super) fn inbox_rows(&self) -> Vec<InboxRow> {
        let filter = self.inbox_view.filter;
        let mut rows: Vec<(i64, InboxRow)> = self
            .notices
            .list
            .iter()
            .enumerate()
            .filter(|(_, n)| filter.lets(n.kind))
            .map(|(ix, n)| (n.created, InboxRow::Notice(ix)))
            .collect();
        if filter.lets_agents() {
            rows.extend(
                self.agent_notices()
                    .into_iter()
                    .map(|(id, at, _)| (at, InboxRow::Agent(id))),
            );
        }
        rows.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        rows.into_iter().map(|(_, row)| row).collect()
    }

    /// Your notes an agent answered that wait for you: it asked something (your answer, once
    /// given, waits to be sent from here), or it says it is done. With when it last wrote, and
    /// the key that is read once looked at (a new answer is new again).
    pub(super) fn agent_notices(&self) -> Vec<(u64, i64, String)> {
        use super::agent::Kind as AgentKind;
        let alive = |session: &str| self.chat_listens(session);
        self.comments
            .iter()
            .filter(|c| !c.archived)
            .filter(|c| match super::agent::kind_of(c, &alive) {
                AgentKind::Asks | AgentKind::Addressed => true,
                AgentKind::Draft => c.answer.is_some() && !c.replies.is_empty(),
                _ => false,
            })
            .filter_map(|c| {
                let at = c.replies.last()?.at;
                Some((c.id, at, format!("agent:{}:{}", c.id, c.replies.len())))
            })
            .collect()
    }

    /// The Inbox's count: notices not read, and agents' answers not looked at.
    pub(super) fn inbox_unread(&self) -> usize {
        self.notices.unread()
            + self
                .agent_notices()
                .iter()
                .filter(|(_, _, key)| !self.notices.is_read(key))
                .count()
    }

    /// The Inbox's selected row: an agent's answer while it waits for you, else the notice.
    pub(super) fn inbox_current(&self) -> Option<InboxRow> {
        match self.inbox_view.agent {
            Some(id) if self.agent_notices().iter().any(|(i, ..)| *i == id) => {
                Some(InboxRow::Agent(id))
            }
            _ => (!self.notices.list.is_empty()).then_some(InboxRow::Notice(self.notices.at)),
        }
    }

    /// Your answer to agent's question `id` was written in the Inbox: it goes to its chat now,
    /// and the next row is selected (Design D: "Answer, next").
    pub(super) fn inbox_answered(&mut self, id: u64, cx: &mut Context<Self>) {
        let rows = self.inbox_rows();
        let at = rows.iter().position(|r| *r == InboxRow::Agent(id));
        self.send_agent(vec![id], cx);
        let rows = self.inbox_rows();
        if let Some(&next) = at.and_then(|a| rows.get(a).or(rows.last())) {
            self.inbox_select(next, cx);
        }
    }

    /// j k in the Inbox: the next or previous listed notice.
    pub(super) fn inbox_step(&mut self, down: bool, cx: &mut Context<Self>) {
        let rows = self.inbox_rows();
        if rows.is_empty() {
            return;
        }
        let at = self
            .inbox_current()
            .and_then(|r| rows.iter().position(|&o| o == r));
        let next = match (at, down) {
            (Some(i), true) => (i + 1).min(rows.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
            (None, _) => 0,
        };
        self.inbox_select(rows[next], cx);
    }

    fn inbox_select(&mut self, row: InboxRow, cx: &mut Context<Self>) {
        if Some(row) != self.inbox_current() {
            self.drop_inbox_reply();
        }
        match row {
            InboxRow::Notice(ix) => {
                self.inbox_view.agent = None;
                self.notices.at = ix;
                self.inbox_selected(cx);
            }
            InboxRow::Agent(id) => {
                self.inbox_view.agent = Some(id);
                // Looked at: read, until the agent writes again.
                if let Some((_, _, key)) = self.agent_notices().into_iter().find(|(i, ..)| *i == id)
                {
                    self.notices.mark_read(key);
                }
            }
        }
        cx.notify();
    }

    /// An empty reply goes when another notice is chosen; one with text stays until sent or
    /// cancelled.
    fn drop_inbox_reply(&mut self) {
        if self.compose.as_ref().is_some_and(|c| {
            matches!(c.target, Target::InboxReply | Target::Answer(_))
                && c.edit.text().trim().is_empty()
        }) {
            self.compose = None;
        }
    }

    /// ↵ in the Inbox: the notice's request, or the line of the note the agent answered.
    pub(super) fn inbox_open(&mut self, cx: &mut Context<Self>) {
        match self.inbox_current() {
            Some(InboxRow::Agent(id)) => self.open_agent_note(id, cx),
            Some(InboxRow::Notice(ix)) => self.open_notice(ix, cx),
            None => {}
        }
    }

    /// e in the Inbox: done with the notice; an agent's question for later (read, the next one
    /// on), its "done" resolved.
    pub(super) fn inbox_done(&mut self, cx: &mut Context<Self>) {
        match self.inbox_current() {
            Some(InboxRow::Agent(id)) => {
                let addressed = self
                    .comments
                    .iter()
                    .any(|c| c.id == id && c.state == crate::review::State::Addressed);
                if addressed {
                    self.resolve_agent_comment(id, cx);
                }
                self.inbox_step(true, cx);
            }
            Some(InboxRow::Notice(ix)) => self.dismiss_notice(ix, cx),
            None => {}
        }
    }

    /// The note an agent answered, where it is written: a file's line in Code, a folder's page;
    /// a line of the old side where a diff shows it.
    fn open_agent_note(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some((path, line, old)) = self
            .comments
            .iter()
            .find(|c| c.id == id)
            .map(|c| (c.path.clone(), c.line, c.old))
        else {
            return;
        };
        if path.ends_with('/') || old || self.root.is_none() {
            self.inbox = false;
            return self.jump_to(id, cx);
        }
        self.file_jump = Some((path.clone(), line));
        self.nav.space = super::spaces::Space::Code;
        self.open_code_file(path, false, cx);
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
        let Some(m) = self.requests.find(n.iid) else {
            return;
        };
        let heads: Vec<String> = t.commit.iter().cloned().chain([m.head.clone()]).collect();
        let (id, target, new, old) = (t.id.clone(), m.target.clone(), t.new_line, t.old_line);
        self.inbox_view.reading = Some(id.clone());
        self.inbox_view.task = Some(cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { excerpt(&root, &target, &heads, &path, new, old) })
                .await;
            this.update(cx, |this, cx| {
                this.inbox_view.reading.take_if(|r| *r == id);
                this.inbox_view.excerpts.insert(id, found);
                cx.notify();
            })
            .ok();
        }));
    }

    /// r in the Inbox: the composer under the selected notice. A review request has nothing to
    /// answer.
    pub(super) fn reply_in_inbox(&mut self, cx: &mut Context<Self>) {
        // An agent's question: your answer, in its thread.
        if let Some(InboxRow::Agent(id)) = self.inbox_current() {
            return self.start_answer(id, cx);
        }
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

    /// Resolves (or opens again) discussion `id` of request `iid` from the Inbox: at once, faded
    /// until the host answers (#139).
    fn inbox_resolve(&mut self, iid: u64, id: String, resolved: bool, cx: &mut Context<Self>) {
        let op = super::waiting::Op::Resolve {
            thread: id.clone(),
            resolved,
        };
        self.send_change(
            iid,
            vec![op],
            move |remote| crate::mr::resolve(&remote, iid, &id, resolved),
            Self::say_refused,
            cx,
        );
    }

    /// The Inbox's filters, in its page bar (#71).
    pub(super) fn inbox_filters(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let filter = |f: InboxFilter, label: &'static str| {
            let id = match f {
                InboxFilter::All => "inbox-all",
                InboxFilter::Mentions => "inbox-mentions",
                InboxFilter::Agents => "inbox-agents",
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
            .child(filter(InboxFilter::Agents, "Agents"))
            .child(filter(InboxFilter::Reviews, "Reviews"))
    }

    /// The Inbox: its panes, or rows loading before the first read, or one message when nothing
    /// is listed; a banner over it while the host cannot be read (#72).
    pub(super) fn render_inbox(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let rows = self.inbox_rows();
        let body = if !self.notices.loaded && rows.is_empty() {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap(px(super::gap()))
                .child(
                    island()
                        .w(px(self.layout.list))
                        .flex_none()
                        .p(px(16.))
                        .child(super::roots::skeleton("inbox-loading", 6)),
                )
                .child(
                    island()
                        .flex_1()
                        .bg(theme::editor())
                        .px(px(28.))
                        .py(px(26.))
                        .child(super::roots::skeleton("inbox-preview-loading", 3)),
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
                InboxFilter::Agents => (
                    "Nothing from your agents",
                    "When an agent asks about one of your notes, or says it is done, it lands here.",
                ),
                InboxFilter::Reviews => (
                    "No review requests",
                    "When someone asks for your review, it lands here.",
                ),
            };
            let all = (self.inbox_view.filter != InboxFilter::All
                && (!self.notices.list.is_empty() || !self.agent_notices().is_empty()))
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
        rows: &[InboxRow],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let today = format::date(now());
        let mut list: Vec<AnyElement> = Vec::new();
        let mut group = None;
        let agents = self.agent_notices();
        for &row in rows {
            let at = match row {
                InboxRow::Notice(ix) => self.notices.list[ix].created,
                InboxRow::Agent(id) => agents
                    .iter()
                    .find(|(i, ..)| *i == id)
                    .map_or(0, |(_, at, _)| *at),
            };
            let heading = if format::date(at) == today {
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
            list.push(match row {
                InboxRow::Notice(ix) => self.render_inbox_row(ix, cx).into_any_element(),
                InboxRow::Agent(id) => self.render_agent_row(id, cx).into_any_element(),
            });
        }
        let preview = self
            .inbox_current()
            .filter(|r| rows.contains(r))
            .and_then(|r| match r {
                InboxRow::Notice(ix) => self
                    .notices
                    .list
                    .get(ix)
                    .map(|n| self.render_inbox_preview(n, cx).into_any_element()),
                InboxRow::Agent(id) => self.render_agent_preview(id, cx),
            })
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
            .child(
                if self.compact() {
                    island().flex_1().min_w_0()
                } else {
                    island()
                        .w(px(super::roots::list_width(self.layout.list)))
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
            // Its handle widens or narrows the list, kept for every list page (the developer,
            // 2026-10-23); a double click sets it back.
            .children((!self.compact()).then(|| self.handle(super::Split::List, cx)))
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
                    .child(inbox_keys(match self.inbox_current() {
                        Some(InboxRow::Agent(id)) => Some(
                            self.comments
                                .iter()
                                .any(|c| c.id == id && c.state == crate::review::State::Addressed),
                        ),
                        _ => None,
                    }))
            }))
    }

    /// One notice: unread dot, who did what, the request always; when, and the start of what was
    /// written, while the list has 260 px (#79); all of it in its tooltip.
    fn render_inbox_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        use super::hover::Hover;
        let n = &self.notices.list[ix];
        let fresh = self.notice_unread(ix);
        let on = self.inbox_current() == Some(InboxRow::Notice(ix));
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
        let iid = n.iid;
        div()
            .id(("inbox-row", ix))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                    this.request_context(iid, e.position, cx)
                }),
            )
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
                                    .cut_in_tip()
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
                            .cut_in_tip()
                            .text_size(px(12.))
                            .text_color(theme::accent())
                            .child(format!("{} {}", words::number(n.iid), n.title)),
                    )
                    .children((room.second() && !gist.is_empty()).then(|| {
                        div()
                            .cut_in_tip()
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
                this.inbox_select(InboxRow::Notice(ix), cx)
            }))
    }

    /// Who an agent's answer is from: the chat's agent ("Claude"), and the chat's title.
    fn agent_of(&self, c: &crate::review::Comment) -> (String, String) {
        let title = c
            .sent
            .as_ref()
            .map(|s| crate::labels::chat(&s.session).unwrap_or_else(|| s.title.clone()))
            .unwrap_or_default();
        let agent = c
            .sent
            .as_ref()
            .and_then(|s| self.agent_chats.iter().find(|ch| ch.session == s.session))
            .map(|ch| ch.agent.clone())
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "Agent".to_owned());
        (agent, title)
    }

    /// What the agent did, as the row says it: asked, your answer waiting to be sent, done.
    fn agent_verb(c: &crate::review::Comment) -> &'static str {
        use crate::review::State;
        match c.state {
            State::Addressed => "addressed your note",
            State::Draft => "asks you · answered, not sent",
            _ => "asks you",
        }
    }

    /// One agent's answer (#116): purple where a notice is blue — who and what, the note's
    /// place, the start of what it wrote.
    fn render_agent_row(&self, id: u64, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        use super::hover::Hover;
        let Some(c) = self.comments.iter().find(|c| c.id == id) else {
            return div().id(("inbox-agent", id as usize));
        };
        let fresh = self
            .agent_notices()
            .iter()
            .any(|(i, _, key)| *i == id && !self.notices.is_read(key));
        let on = self.inbox_current() == Some(InboxRow::Agent(id));
        let room = self.widths.room("inbox");
        let (agent, chat) = self.agent_of(c);
        let at = c.replies.last().map_or(0, |r| r.at);
        let said = c
            .replies
            .last()
            .map(|r| crate::markdown::plain(&r.body))
            .unwrap_or_default();
        let gist = said
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_owned();
        let place = note_place(c);
        let verb = Self::agent_verb(c);
        let mut tip = vec![format!("{agent} {verb}"), place.clone()];
        if !chat.is_empty() {
            tip.push(format!("chat “{chat}”"));
        }
        tip.push(super::hover::full_date(at));
        if !said.trim().is_empty() {
            tip.push(said.trim().to_owned());
        }
        div()
            .id(("inbox-agent", id as usize))
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
                    .when(fresh, |s| s.bg(theme::agent())),
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
                                    .flex()
                                    .gap(px(5.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .when(fresh, |s| s.font_weight(FontWeight::SEMIBOLD))
                                    .child(agent)
                                    .child(div().text_color(theme::agent()).child(verb)),
                            )
                            .children(room.second().then(|| {
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .child(format::ago(at))
                            })),
                    )
                    .child(
                        div()
                            .cut_in_tip()
                            .text_size(px(12.))
                            .font_family(theme::CODE_FONT)
                            .text_color(theme::agent())
                            .child(place),
                    )
                    .children((room.second() && !gist.is_empty()).then(|| {
                        div()
                            .cut_in_tip()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(gist)
                    })),
            )
            .tip(tip.join("\n"))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if this.compact() {
                    return this.open_agent_note(id, cx);
                }
                this.inbox_select(InboxRow::Agent(id), cx)
            }))
    }

    /// The selected agent's answer: whose and where, the lines the note is on as they were, the
    /// purple thread itself — your note, what the agent said, its form, your answer and Send, or
    /// Resolve —, and the way to its line.
    fn render_agent_preview(&self, id: u64, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::review::State;
        let c = self.comments.iter().find(|c| c.id == id)?;
        let (agent, chat) = self.agent_of(c);
        let at = c.replies.last().map_or(0, |r| r.at);
        let props = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .text_size(px(12.))
            .text_color(theme::muted())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_color(theme::agent())
                    .child(icons::icon("bot").size(px(14.)).text_color(theme::agent()))
                    .child(if chat.is_empty() {
                        agent.clone()
                    } else {
                        format!("{agent} · {chat}")
                    }),
            )
            .children(c.branch.clone().filter(|b| !b.is_empty()).map(|b| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        icons::icon("branch")
                            .size(px(14.))
                            .text_color(theme::muted()),
                    )
                    .child(div().font_family(theme::CODE_FONT).child(b))
            }))
            .child(div().flex_1())
            .child(format!(
                "{agent} {} · {}",
                Self::agent_verb(c),
                format::ago(at)
            ));
        let title = div()
            .text_size(px(19.))
            .line_height(px(26.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(note_place(c));
        // The lines the note is on, as they were when it was written.
        let code = (!c.path.ends_with('/') && !c.code.is_empty()).then(|| {
            let first = c.line.saturating_sub(c.above.len() as u32);
            let lines = c
                .above
                .iter()
                .chain([&c.code])
                .enumerate()
                .map(|(k, text)| {
                    let n = Some(first + k as u32);
                    ExcerptLine {
                        kind: LineKind::Context,
                        old: if c.old { n } else { None },
                        new: if c.old { None } else { n },
                        text: text.clone(),
                        on: true,
                    }
                })
                .collect();
            excerpt_view(&c.path, c.line, Some(Some(&Excerpt { lines })))
        });
        let thread = self.render_thread(id, 0., cx.entity().downgrade());
        let open = super::bar::bar_button("inbox-agent-open", "Open at the line", "↵", true)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_agent_note(id, cx)));
        let later = super::bar::bar_button(
            "inbox-agent-done",
            if c.state == State::Addressed {
                "Resolve"
            } else {
                "Later"
            },
            "E",
            false,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.inbox_done(cx)));
        Some(
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
                .child(div().mx(px(-12.)).child(thread))
                .child(div().flex().gap_2().child(open).child(later))
                .into_any_element(),
        )
    }

    /// The selected notice: its request, the code, the discussion, the reply.
    fn render_inbox_preview(&self, n: &Notice, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ix = self.notices.at;
        let mr = self.requests.find(n.iid);
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
            let path = t.path.clone()?;
            let line = t.new_line.or(t.old_line)?;
            match self.inbox_view.excerpts.get(&t.id) {
                Some(ex) => Some(excerpt_view(&path, line, Some(ex.as_ref()))),
                None if self.inbox_view.reading.as_ref() == Some(&t.id) => {
                    Some(excerpt_view(&path, line, None))
                }
                None => None,
            }
        });
        // The discussion, or what the notice says when there is none (a mention in a description).
        let talk = match thread {
            Some(t) => {
                let last = t.notes.len().saturating_sub(1);
                let notes = t.notes.iter().enumerate().map(|(i, note)| {
                    // Sent from here or the diff: faded until the host answers; the read after it is
                    // the Inbox's next look (#139).
                    let waits = self.requests.waiting.note(n.iid, u64::MAX, note.id);
                    discussion::note(
                        &note.author,
                        note.login == me,
                        None,
                        waits.map_or_else(|| format::ago(note.created), str::to_owned),
                        markdown::body(
                            &note.body,
                            self.settings.rich_comments,
                            format!("md-inbox-{}", note.id),
                            None,
                            Some(self.selecting(cx.entity().downgrade())),
                        ),
                        i == last,
                        None,
                        None,
                    )
                    .when(waits.is_some(), |s| s.opacity(super::waiting::FADED))
                });
                let (iid, id, resolved) = (n.iid, t.id.clone(), t.resolved);
                let resolving = self
                    .requests
                    .waiting
                    .thread(iid, u64::MAX, &id)
                    .filter(|_| t.resolvable);
                discussion::frame(Source::GitLab, n.note.is_some())
                    .children(notes)
                    .children(t.resolvable.then(|| {
                        div().flex().justify_end().pt(px(6.)).child(
                            discussion::button(
                                "inbox-resolve",
                                match resolving {
                                    Some(word) => word,
                                    None if resolved => "Reopen",
                                    None => "Resolve",
                                },
                                None,
                                Button::Plain,
                            )
                            .when(resolving.is_some(), |s| s.opacity(super::waiting::FADED))
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
                        Some(self.selecting(cx.entity().downgrade())),
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

/// Where a note is written: `refunds/`, `Money.kt:23`.
fn note_place(c: &crate::review::Comment) -> String {
    if c.path.ends_with('/') {
        return c.path.clone();
    }
    let name = c.path.rsplit('/').next().unwrap_or(&c.path);
    format!("{name}:{}", c.line)
}

/// The keys of the Inbox, along the preview's foot; `agent`: an agent's answer is selected, and
/// whether it says it is done.
fn inbox_keys(agent: Option<bool>) -> Div {
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
        .child(key(
            &["↵"],
            if agent.is_some() {
                "open at the line"
            } else {
                "open request"
            },
        ))
        .child(key(
            &["E"],
            match agent {
                Some(true) => "resolve",
                Some(false) => "later",
                None => "done",
            },
        ))
        .child(key(
            &["R"],
            if agent.is_some() { "answer" } else { "reply" },
        ))
}

/// The card of the code a discussion is on: the path and line, then the lines (or a word that
/// they are not here).
/// `ex`: None while it is being read, `Some(None)` when the commit is not here.
fn excerpt_view(path: &str, line: u32, ex: Option<Option<&Excerpt>>) -> Div {
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
        .child(super::hover::cut("excerpt-path", path.to_owned()).flex_1())
        .child(format!("line {line}"));
    let body = match ex {
        None => div()
            .px(px(12.))
            .pt(px(4.))
            .pb(px(12.))
            .child(super::motion::shimmer("inbox-excerpt-loading", 4)),
        Some(Some(ex)) => div()
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
        Some(None) => div()
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
