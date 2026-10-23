//! Sending the review to an agent's chat (`crate::agents`): the reviewer's comments stay drafts
//! until *Send to agent* (⌥⌘A); a card then lists this repository's chats — those listening first,
//! then Claude Code's recent ones, which get the comments when they listen — and a new chat. The
//! agent's answers come back every couple of seconds, and the reviewer resolves or reopens each.

use super::icons;
use super::keys::{self, On};
use super::px;
use super::requests::group_heading;
use super::tools::ToolView;
use super::{ISLAND_RADIUS, ROW_RADIUS, Workspace, theme};
use crate::agents::{self, Bus};
use crate::review::State;
use gpui::{
    ClickEvent, ClipboardItem, Context, FontWeight, KeyDownEvent, MouseButton, div, prelude::*,
};

/// A chat the card offers.
#[derive(Clone)]
pub struct PickChat {
    /// Empty for a new chat.
    session: String,
    title: String,
    agent: String,
    listening: bool,
}

/// The "send to which chat?" card; with `route`, which chat a branch's notes go to (#77).
pub struct AgentPick {
    chats: Vec<PickChat>,
    selected: usize,
    /// The comments it sends.
    ids: Vec<u64>,
    /// The branch whose route it picks ("" for any); nothing is sent then.
    route: Option<String>,
}

impl Workspace {
    fn bus() -> Option<Bus> {
        Bus::new()
    }

    /// The drafts the next sending takes.
    pub(super) fn agent_drafts(&self) -> Vec<u64> {
        self.comments
            .iter()
            .filter(|c| c.state == State::Draft)
            .map(|c| c.id)
            .collect()
    }

    /// Opens the card; without drafts, says so.
    pub(super) fn open_agent_pick(&mut self, cx: &mut Context<Self>) {
        let ids = self.agent_drafts();
        self.open_agent_pick_for(ids, cx);
    }

    /// The chat card for `ids`: where to send them.
    pub(super) fn open_agent_pick_for(&mut self, ids: Vec<u64>, cx: &mut Context<Self>) {
        if ids.is_empty() {
            self.error =
                Some("No comments to send — every comment is already with an agent.".into());
            return cx.notify();
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let project = self.project.clone().unwrap_or_else(|| root.clone());
        let mut chats: Vec<PickChat> = Self::bus()
            .map(|bus| bus.chats(&project))
            .unwrap_or_default()
            .into_iter()
            .map(|c| PickChat {
                listening: c.listening(),
                session: c.session,
                title: c.title,
                agent: c.agent,
            })
            .collect();
        chats.sort_by_key(|c| !c.listening);
        for (session, title) in agents::claude_chats(&root) {
            if !chats.iter().any(|c| c.session == session) {
                chats.push(PickChat {
                    session,
                    title,
                    agent: "Claude Code".into(),
                    listening: false,
                });
            }
        }
        chats.push(PickChat {
            session: String::new(),
            title: "New chat".into(),
            agent: "Claude Code".into(),
            listening: true,
        });
        self.agent_pick = Some(AgentPick {
            chats,
            selected: 0,
            ids,
            route: None,
        });
        cx.notify();
    }

    /// Change…: the card offers the chats that listen; the one picked takes `branch`'s notes.
    fn open_route_pick(&mut self, branch: String, cx: &mut Context<Self>) {
        let now = self.route_of(Some(&branch)).map(|c| c.session.clone());
        let chats: Vec<PickChat> = self
            .agent_chats
            .iter()
            .filter(|c| c.listening())
            .map(|c| PickChat {
                session: c.session.clone(),
                title: c.title.clone(),
                agent: c.agent.clone(),
                listening: true,
            })
            .collect();
        let selected = chats
            .iter()
            .position(|c| Some(&c.session) == now.as_ref())
            .unwrap_or(0);
        self.agent_pick = Some(AgentPick {
            chats,
            selected,
            ids: Vec::new(),
            route: Some(branch),
        });
        cx.notify();
    }

    /// Send to … instead: a note its gone chat never took goes to its route now.
    fn resend(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(c) = self.comments.iter_mut().find(|c| c.id == id) {
            c.state = State::Draft;
            c.sent = None;
        }
        self.send_agent(vec![id], cx);
    }

    /// Chat `session` is here and listening: a note sent to it and not taken is not waiting.
    fn chat_listens(&self, session: &str) -> bool {
        self.agent_chats
            .iter()
            .any(|c| c.session == session && c.listening())
    }

    /// Another chat than the page's is changing file `path`: a note of its on the file was taken
    /// and not answered yet (#77). Its name, letter and hue.
    pub(super) fn changing(&self, path: &str) -> Option<(String, char, gpui::Rgba)> {
        let alive = |session: &str| self.chat_listens(session);
        let page = self
            .route_of(self.page_branch().as_deref())
            .map(|c| c.session.clone());
        let session = self
            .comments
            .iter()
            .filter(|c| c.path == path && !c.archived && kind_of(c, &alive) == Kind::Working)
            .filter_map(|c| c.sent.as_ref())
            .map(|s| s.session.clone())
            .find(|s| Some(s) != page.as_ref())?;
        let chat = self.agent_chats.iter().find(|c| c.session == session)?;
        let (letter, hue) = self.chat_mark(&session);
        let name = if chat.title.is_empty() {
            chat.agent.clone()
        } else {
            chat.title.clone()
        };
        Some((name, letter, hue))
    }

    /// A chat's mark (#77): its agent's first letter on a hue of its own — the code palette's,
    /// never the agent's purple or the public orange — by its place among the chats here.
    pub(super) fn chat_mark(&self, session: &str) -> (char, gpui::Rgba) {
        let hues = [
            theme::accent(),
            theme::added(),
            theme::renamed(),
            theme::removed(),
        ];
        let mut sessions: Vec<&str> = self
            .agent_chats
            .iter()
            .map(|c| c.session.as_str())
            .collect();
        sessions.sort_unstable();
        let at = sessions.iter().position(|s| *s == session).unwrap_or(0);
        let letter = self
            .agent_chats
            .iter()
            .find(|c| c.session == session)
            .and_then(|c| c.agent.chars().next())
            .unwrap_or('?');
        (letter, hues[at % hues.len()])
    }

    /// Sends each of `ids` to its route (#77): the chat working on the branch it was written on,
    /// else the project's default; with no chat listening, the card asks.
    pub(super) fn send_agent(&mut self, ids: Vec<u64>, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        self.refresh_chats();
        let mut by_chat: Vec<(agents::Chat, Vec<u64>)> = Vec::new();
        let mut nowhere = Vec::new();
        for id in ids {
            let branch = self
                .comments
                .iter()
                .find(|c| c.id == id)
                .and_then(|c| c.branch.clone());
            match self.route_of(branch.as_deref()).cloned() {
                Some(chat) => match by_chat.iter_mut().find(|(c, _)| c.session == chat.session) {
                    Some((_, ids)) => ids.push(id),
                    None => by_chat.push((chat, vec![id])),
                },
                None => nowhere.push(id),
            }
        }
        for (chat, ids) in &by_chat {
            agents::send(&mut self.comments, ids, &chat.session, &chat.title);
        }
        if !by_chat.is_empty() {
            self.save_comments();
            self.refresh_rows();
            cx.notify();
        }
        if !nowhere.is_empty() {
            self.open_agent_pick_for(nowhere, cx);
        }
    }

    /// The chat a note on `branch` goes to (`agents::route`), with the one picked for it.
    pub(super) fn route_of(&self, branch: Option<&str>) -> Option<&agents::Chat> {
        let main = self.project.as_ref().or(self.root.as_ref())?;
        let chosen = branch
            .and_then(|b| self.chat_routes.get(b))
            .or(self.chat_routes.get(""))
            .map(String::as_str);
        agents::route(branch, chosen, &self.agent_chats, main)
    }

    /// The branch of the page on screen, as a note written there keeps it: a request's source
    /// branch, a branch's own, else the one checked out (#77).
    pub(super) fn page_branch(&self) -> Option<String> {
        if let Some(iid) = self.requests.current
            && let Some(m) = self.requests.list.iter().find(|m| m.iid == iid)
            && self.nav.space == super::spaces::Space::Requests
        {
            return Some(m.source.clone());
        }
        let shown = match (
            self.nav.space,
            self.branch.and_then(|ix| self.branches.get(ix)),
        ) {
            (super::spaces::Space::Branches, Some(b)) => Some(b),
            _ => self.branches.iter().find(|b| b.is_head),
        }?;
        // A remote branch is the branch it tracks there.
        Some(match shown.kind {
            crate::git::RefKind::Remote => shown
                .name
                .split_once('/')
                .map_or(shown.name.clone(), |(_, n)| n.to_owned()),
            _ => shown.name.clone(),
        })
    }

    /// The reviewer's pick on the agent's form for comment `id`: saved to the drafts at once
    /// (#10 C) — it goes with the next send.
    pub(super) fn pick_answer(&mut self, id: u64, pick: Option<usize>, cx: &mut Context<Self>) {
        let Some(c) = self.comments.iter_mut().find(|c| c.id == id) else {
            return;
        };
        let form = c.replies.last().and_then(|r| r.form.clone());
        let a = c.answer.get_or_insert_with(|| crate::review::Answered {
            picks: match &form {
                Some(f) if f.kind == crate::review::FormKind::Order => {
                    (0..f.choices().len()).collect()
                }
                Some(f) if f.kind.many() => f
                    .choices()
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| o.on)
                    .map(|(i, _)| i)
                    .collect(),
                _ => Vec::new(),
            },
            ..Default::default()
        });
        match (pick, &form) {
            (None, _) => {
                a.your_call = !a.your_call;
            }
            (Some(i), Some(f)) if f.kind.many() => {
                a.your_call = false;
                match a.picks.iter().position(|p| *p == i) {
                    Some(at) => {
                        a.picks.remove(at);
                    }
                    None => a.picks.push(i),
                }
                a.picks.sort_unstable();
            }
            (Some(i), _) => {
                a.your_call = false;
                a.picks = vec![i];
            }
        }
        c.state = State::Draft;
        self.save_comments();
        self.refresh_rows();
        cx.notify();
    }

    /// Order: moves item `at` of the answer one place up (or down).
    pub(super) fn move_answer(&mut self, id: u64, at: usize, up: bool, cx: &mut Context<Self>) {
        let Some(c) = self.comments.iter_mut().find(|c| c.id == id) else {
            return;
        };
        let n = c
            .replies
            .last()
            .and_then(|r| r.form.as_ref())
            .map_or(0, |f| f.choices().len());
        let a = c.answer.get_or_insert_with(|| crate::review::Answered {
            picks: (0..n).collect(),
            ..Default::default()
        });
        let to = if up { at.checked_sub(1) } else { Some(at + 1) };
        if let Some(to) = to.filter(|&t| t < a.picks.len()) {
            a.picks.swap(at, to);
            c.state = State::Draft;
            self.save_comments();
            self.refresh_rows();
            cx.notify();
        }
    }

    /// Under the agent's question: its form, answered with clicks; the foot sends (#10 C).
    pub(super) fn render_agent_form(
        &self,
        id: u64,
        this: gpui::WeakEntity<Self>,
    ) -> Option<gpui::AnyElement> {
        use crate::review::FormKind;
        let c = self.comments.iter().find(|c| c.id == id)?;
        if !matches!(c.state, State::Sent | State::Draft) {
            return None;
        }
        let reply = c.replies.last()?;
        let form = reply.form.as_ref();
        let answer = c.answer.clone().unwrap_or_default();
        let answered = c.answer.is_some();
        let others = self.agent_drafts().into_iter().filter(|d| *d != id).count();
        let order: Vec<usize> = match form {
            Some(f) if f.kind == FormKind::Order => {
                if answer.picks.len() == f.choices().len() {
                    answer.picks.clone()
                } else {
                    (0..f.choices().len()).collect()
                }
            }
            Some(f) => (0..f.choices().len()).collect(),
            None => Vec::new(),
        };
        let mark = |on: bool, many: bool| {
            div()
                .size(px(14.))
                .flex_none()
                .rounded(px(if many { 3. } else { 7. }))
                .border_2()
                .border_color(if on { theme::agent() } else { theme::faint() })
                .when(on, |s| s.bg(theme::agent()))
        };
        let rows: Vec<gpui::AnyElement> = match form {
            Some(f) if f.kind == FormKind::Line => {
                self.render_line_choices(c, f, &answer, this.clone())
            }
            _ => self.render_option_rows(id, form, &order, &answer, answered, &mark, this.clone()),
        };
        // Text: one's own value beside the suggestions (#10 C).
        let own = form.filter(|f| f.kind == FormKind::Text).map(|_| {
            let edit = this.clone();
            div()
                .id(("form-own", id as usize))
                .mx(px(8.))
                .mt(px(2.))
                .h(px(30.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(ROW_RADIUS))
                .border_1()
                .border_color(theme::agent())
                .bg(theme::base())
                .cursor_text()
                .font_family(theme::CODE_FONT)
                .text_size(px(12.))
                .child(if answer.value.is_empty() {
                    div().text_color(theme::faint()).child("Your own value…")
                } else {
                    div().child(answer.value.clone())
                })
                .on_click(move |_, _, cx| {
                    edit.update(cx, |this, cx| this.start_answer_value(id, cx))
                        .ok();
                })
                .into_any_element()
        });
        let (call, note, only, all) = (this.clone(), this.clone(), this.clone(), this);
        let button = |idn: (&'static str, usize), label: String, primary: bool| {
            div()
                .id(idn)
                .h(px(26.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(ROW_RADIUS))
                .cursor_pointer()
                .text_size(px(12.))
                .when(primary, |s| {
                    s.bg(theme::agent())
                        .text_color(theme::base())
                        .font_weight(FontWeight::SEMIBOLD)
                })
                .when(!primary, |s| {
                    s.border_1()
                        .border_color(theme::island_border())
                        .hover(|s| s.bg(theme::hover()))
                })
                .child(label)
        };
        Some(
            div()
                .mx(px(12.))
                .mb(px(10.))
                .ml(px(52.))
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(gpui::Rgba {
                    a: 0.35,
                    ..theme::agent()
                })
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(rows)
                .children(own)
                .children((!answer.note.is_empty()).then(|| {
                    div()
                        .px(px(8.))
                        .pt(px(4.))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(format!("Your note: {}", answer.note))
                }))
                .child(
                    div()
                        .pt(px(6.))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(
                            button(("form-call", id as usize), "0  Your call".to_owned(), false)
                                .when(answer.your_call, |s| s.bg(theme::agent_bg()))
                                .on_click(move |_, _, cx| {
                                    call.update(cx, |this, cx| this.pick_answer(id, None, cx))
                                        .ok();
                                }),
                        )
                        .child(
                            button(
                                ("form-note", id as usize),
                                super::keys::text("⇧N  Add a note").into_owned(),
                                false,
                            )
                            .on_click(move |_, _, cx| {
                                note.update(cx, |this, cx| this.start_answer(id, cx)).ok();
                            }),
                        )
                        .children(answered.then(|| {
                            div()
                                .text_size(px(12.))
                                .text_color(theme::agent())
                                .child("✓ In drafts")
                        }))
                        .child(div().flex_1())
                        .children((answered && others > 0).then(|| {
                            button(
                                ("form-only", id as usize),
                                super::keys::text("Send this only  ⌥⌘↵").into_owned(),
                                false,
                            )
                            .on_click(move |_, _, cx| {
                                only.update(cx, |this, cx| this.send_agent(vec![id], cx))
                                    .ok();
                            })
                        }))
                        .children(answered.then(|| {
                            let label = if others > 0 {
                                format!("Send all {}  ⇧⌘↵", others + 1)
                            } else {
                                "Send  ⇧⌘↵".to_owned()
                            };
                            button(
                                ("form-all", id as usize),
                                super::keys::text(&label).into_owned(),
                                true,
                            )
                            .on_click(move |_, _, cx| {
                                all.update(cx, |this, cx| {
                                    let ids = this.agent_drafts();
                                    this.send_agent(ids, cx)
                                })
                                .ok();
                            })
                        })),
                )
                .into_any_element(),
        )
    }

    /// Line: the lines the agent suggests drawn as code, with the lines between them from the
    /// file (when it is the one open); a suggested line is picked with a click (Design
    /// `kinds.html`, line).
    fn render_line_choices(
        &self,
        c: &crate::review::Comment,
        f: &crate::review::Form,
        answer: &crate::review::Answered,
        this: gpui::WeakEntity<Self>,
    ) -> Vec<gpui::AnyElement> {
        let id = c.id;
        let choices = f.choices();
        // Each suggestion's line number: the first number in its label.
        let number = |label: &str| -> Option<u32> {
            let digits: String = label
                .chars()
                .skip_while(|ch| !ch.is_ascii_digit())
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse().ok()
        };
        let suggested: Vec<(usize, u32, String)> = choices
            .iter()
            .enumerate()
            .filter_map(|(i, o)| {
                let code = o.code.first().cloned().unwrap_or_else(|| o.detail.clone());
                Some((i, number(&o.label)?, code))
            })
            .collect();
        if suggested.is_empty() {
            return Vec::new();
        }
        let (low, high) = suggested.iter().fold((u32::MAX, 0), |(lo, hi), (_, n, _)| {
            (lo.min(*n), hi.max(*n))
        });
        // The file's own lines fill the gaps when it is the open one.
        let open = self
            .data
            .as_ref()
            .filter(|_| self.current_path().as_deref() == Some(c.path.as_str()));
        let text_of = |n: u32| -> Option<String> {
            suggested
                .iter()
                .find(|(_, m, code)| *m == n && !code.is_empty())
                .map(|(_, _, code)| code.clone())
                .or_else(|| {
                    let side = open?.side(false);
                    (n >= 1 && n <= side.count()).then(|| side.line(n).0.to_owned())
                })
        };
        let span = if open.is_some() && high - low <= 12 {
            (low.saturating_sub(1).max(1)..=high + 1).collect::<Vec<u32>>()
        } else {
            suggested.iter().map(|(_, n, _)| *n).collect()
        };
        let rows = span.into_iter().filter_map(|n| {
            let choice = suggested
                .iter()
                .find(|(_, m, _)| *m == n)
                .map(|(i, _, _)| *i);
            // A suggestion stays pickable even when its text is not known here.
            let text = text_of(n).or_else(|| choice.map(|_| String::new()))?;
            let picked = choice.is_some_and(|i| answer.picks.contains(&i));
            let pick = this.clone();
            Some(
                div()
                    .id(("form-line", n as usize))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(8.))
                    .when(picked, |s| {
                        s.bg(theme::agent_bg())
                            .border_l(px(3.))
                            .border_color(theme::agent())
                    })
                    .child(
                        div()
                            .w(px(28.))
                            .flex_none()
                            .text_right()
                            .text_color(if choice.is_some() {
                                theme::agent()
                            } else {
                                theme::faint()
                            })
                            .child(n.to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(text),
                    )
                    .children(picked.then(|| {
                        div()
                            .flex_none()
                            .font_family(theme::UI_FONT)
                            .text_size(px(11.))
                            .text_color(theme::agent())
                            .child("← here")
                    }))
                    .when_some(choice, |s, i| {
                        s.cursor_pointer().hover(|s| s.bg(theme::hover())).on_click(
                            move |_, _, cx| {
                                pick.update(cx, |this, cx| this.pick_answer(id, Some(i), cx))
                                    .ok();
                            },
                        )
                    })
                    .into_any_element(),
            )
        });
        vec![
            div()
                .mx(px(8.))
                .py(px(4.))
                .rounded(px(6.))
                .border_1()
                .border_color(theme::island_border())
                .bg(theme::editor())
                .font_family(theme::CODE_FONT)
                .text_size(px(12.))
                .children(rows)
                .into_any_element(),
            div()
                .px(px(8.))
                .text_size(px(11.))
                .text_color(theme::faint())
                .child(format!(
                    "click a line, or 1–{} for the ones it suggests",
                    suggested.len()
                ))
                .into_any_element(),
        ]
    }

    /// The asked comment the form keys act on: the first one with a form from the top of the
    /// view (#10 C: 1…9 pick, 0 your call, ⇧N a note).
    pub(super) fn acting_agent_form(&self) -> Option<u64> {
        let top = self.local_top();
        self.rows.iter().skip(top).find_map(|row| match row {
            super::rows::Row::Thread(id) => self
                .comments
                .iter()
                .find(|c| c.id == *id)
                .filter(|c| matches!(c.state, State::Sent | State::Draft))
                .filter(|c| c.replies.last().is_some_and(|r| r.form.is_some()))
                .map(|c| c.id),
            _ => None,
        })
    }

    /// A digit on the acting form: 0 is your call, 1…9 the option drawn with that number (Line:
    /// the suggestion in order). Whether it took the key.
    pub(super) fn answer_by_key(&mut self, digit: usize, cx: &mut Context<Self>) -> bool {
        use crate::review::FormKind;
        let Some(id) = self.acting_agent_form() else {
            return false;
        };
        if digit == 0 {
            self.pick_answer(id, None, cx);
            return true;
        }
        let Some(f) = self
            .comments
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.replies.last())
            .and_then(|r| r.form.clone())
        else {
            return false;
        };
        if f.kind == FormKind::Order || digit > f.choices().len() {
            return false;
        }
        self.pick_answer(id, Some(digit - 1), cx);
        true
    }

    /// The options of a form, one row each: number, mark, label, detail, code (Order: ↑ ↓).
    #[allow(clippy::too_many_arguments)]
    fn render_option_rows(
        &self,
        id: u64,
        form: Option<&crate::review::Form>,
        order: &[usize],
        answer: &crate::review::Answered,
        answered: bool,
        mark: &dyn Fn(bool, bool) -> gpui::Div,
        this: gpui::WeakEntity<Self>,
    ) -> Vec<gpui::AnyElement> {
        use crate::review::FormKind;
        let Some(f) = form else {
            return Vec::new();
        };

        let choices = f.choices();
        let many = f.kind.many();
        order
            .iter()
            .enumerate()
            .filter_map(|(place, &i)| {
                let o = choices.get(i)?.clone();
                let on = if f.kind == FormKind::Order {
                    false
                } else if answered {
                    answer.picks.contains(&i)
                } else {
                    many && o.on
                };
                let (pick, up, down) = (this.clone(), this.clone(), this.clone());
                let order_kind = f.kind == FormKind::Order;
                let row = div()
                    .id(("form-option", id as usize * 64 + i))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(ROW_RADIUS))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .w(px(16.))
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .child((place + 1).to_string()),
                            )
                            .children((!order_kind).then(|| mark(on, many)))
                            .child(div().flex_1().child(o.label.clone()))
                            .children((!o.detail.is_empty()).then(|| {
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::muted())
                                    .child(o.detail.clone())
                            }))
                            .children(order_kind.then(|| {
                                div()
                                    .flex()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .id(("form-up", id as usize * 64 + i))
                                            .px(px(6.))
                                            .rounded(px(4.))
                                            .hover(|s| s.bg(theme::selected()))
                                            .child("↑")
                                            .on_click(move |_, _, cx| {
                                                cx.stop_propagation();
                                                up.update(cx, |this, cx| {
                                                    this.move_answer(id, place, true, cx)
                                                })
                                                .ok();
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id(("form-down", id as usize * 64 + i))
                                            .px(px(6.))
                                            .rounded(px(4.))
                                            .hover(|s| s.bg(theme::selected()))
                                            .child("↓")
                                            .on_click(move |_, _, cx| {
                                                cx.stop_propagation();
                                                down.update(cx, |this, cx| {
                                                    this.move_answer(id, place, false, cx)
                                                })
                                                .ok();
                                            }),
                                    )
                            })),
                    )
                    .children((!o.code.is_empty()).then(|| {
                        div()
                            .ml(px(38.))
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .bg(theme::editor())
                            .font_family(theme::CODE_FONT)
                            .text_size(px(12.))
                            .children(o.code.iter().map(|l| div().child(l.clone())))
                    }))
                    .when(!order_kind, |s| {
                        s.on_click(move |_, _, cx| {
                            pick.update(cx, |this, cx| this.pick_answer(id, Some(i), cx))
                                .ok();
                        })
                    });
                // The option picked lights up (#56, 17).
                let tint = theme::agent_bg();
                Some(
                    super::motion::toward(
                        row,
                        ("form-option-on", id as usize * 64 + i),
                        on,
                        false,
                        super::motion::FAST,
                        move |row, v| {
                            row.bg(gpui::Rgba {
                                a: tint.a * v,
                                ..tint
                            })
                        },
                    )
                    .into_any_element(),
                )
            })
            .collect()
    }

    fn send_to_chat(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(pick) = self.agent_pick.take() else {
            return;
        };
        let Some(chat) = pick.chats.get(ix).cloned() else {
            return;
        };
        if let Some(branch) = pick.route {
            self.chat_routes.insert(branch, chat.session);
            return cx.notify();
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let title = if chat.session.is_empty() {
            "a new chat".to_owned()
        } else {
            chat.title.clone()
        };
        agents::send(&mut self.comments, &pick.ids, &chat.session, &title);
        self.save_comments();
        if chat.session.is_empty() {
            let prompt = "GitLance sent review comments to this chat. Call the gitlance list_comments tool, change the code for each comment and answer each with address_comment.";
            if let Err(e) = crate::platform::open_terminal(&root, "claude", prompt) {
                self.error =
                    Some(format!("Could not open a terminal for the new chat: {e}").into());
            }
        }
        self.refresh_rows();
        cx.notify();
    }

    pub(super) fn agent_pick_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(pick) = self.agent_pick.as_mut() else {
            return;
        };
        let last = pick.chats.len().saturating_sub(1);
        match event.keystroke.key.as_str() {
            "escape" => self.agent_pick = None,
            "enter" => {
                let ix = pick.selected;
                return self.send_to_chat(ix, cx);
            }
            "down" | "j" | "tab" => pick.selected = (pick.selected + 1).min(last),
            "up" | "k" => pick.selected = pick.selected.saturating_sub(1),
            _ => return,
        }
        cx.notify();
    }

    /// Takes in what the agents answered since the last look (from the 2-second poll).
    pub(super) fn poll_agents(&mut self, cx: &mut Context<Self>) {
        // The chats are read on every beat: the stripe shows who listens, a toast who comes and
        // goes (#77).
        let before = self.agent_chats.clone();
        let known = self.chats_known;
        if self.refresh_chats() {
            if known {
                self.say_chats_changed(&before, cx);
            }
            cx.notify();
        }
        self.chats_known = true;
        let (Some(root), Some(bus)) = (self.project.clone().or(self.root.clone()), Self::bus())
        else {
            return;
        };
        let stamp = bus.answers_stamp(&root);
        if stamp.is_none() || stamp == self.answers_stamp {
            return;
        }
        self.answers_stamp = stamp;
        if agents::apply(&mut self.comments, &bus.answers(&root)) {
            self.save_comments();
            self.refresh_rows();
            cx.notify();
        }
    }

    /// Reads this repository's chats again for the Agents window; whether they changed.
    pub(super) fn refresh_chats(&mut self) -> bool {
        let mut chats = match (self.project.as_ref().or(self.root.as_ref()), Self::bus()) {
            (Some(root), Some(bus)) => bus.chats(root),
            _ => Vec::new(),
        };
        // A chat long gone with nothing of the review on it is not shown (#77).
        chats.retain(|c| {
            c.listening()
                || agents::now() - c.beat < 24 * 3600
                || self
                    .comments
                    .iter()
                    .any(|n| n.sent.as_ref().is_some_and(|s| s.session == c.session))
        });
        let changed = chats != self.agent_chats;
        self.agent_chats = chats;
        changed
    }

    /// A toast for each chat that began listening or went since `before`; one taking notes to work
    /// on says nothing (#77).
    fn say_chats_changed(&mut self, before: &[agents::Chat], cx: &mut Context<Self>) {
        let name = |c: &agents::Chat| {
            if c.title.is_empty() {
                c.agent.clone()
            } else {
                format!("{} · {}", c.agent, c.title)
            }
        };
        let working = |session: &str| {
            self.comments.iter().any(|c| {
                c.state == State::Sent
                    && c.sent
                        .as_ref()
                        .is_some_and(|s| s.session == session && s.delivered)
            })
        };
        let mut said: Vec<String> = Vec::new();
        for c in &self.agent_chats {
            let was = before.iter().find(|b| b.session == c.session);
            match (was.map(agents::Chat::listening), c.listening()) {
                (None, true) => said.push(format!("{} is listening", name(c))),
                (Some(true), false) if !working(&c.session) => {
                    said.push(format!("{} stopped listening", name(c)))
                }
                _ => {}
            }
        }
        for b in before {
            if b.listening() && !self.agent_chats.iter().any(|c| c.session == b.session) {
                said.push(format!("{} stopped listening", name(b)));
            }
        }
        for text in said {
            self.toast_info(text.into(), cx);
        }
    }

    /// The Agents tool window: the chats that listen here, then the purple comments by state.
    pub(super) fn agents_tool(&self, cx: &mut Context<Self>) -> ToolView {
        let drafts = self.agent_drafts().len();
        // A chat works while it has comments it took and has not answered, and is not listening:
        // the watcher wakes it with them and runs again when it is done.
        let working = |session: &str| {
            self.comments
                .iter()
                .filter(|c| c.state == State::Sent)
                .filter(|c| {
                    c.sent
                        .as_ref()
                        .is_some_and(|s| s.delivered && s.session == session)
                })
                .filter(|c| c.replies.is_empty() || c.answer.is_some())
                .count()
        };
        // Each chat here, in any worktree: its mark, what it works on, its state and branch (#77).
        let chats = self.agent_chats.iter().enumerate().map(|(ix, c)| {
            let busy = if c.listening() {
                0
            } else {
                working(&c.session)
            };
            let (letter, hue) = self.chat_mark(&c.session);
            let state = if c.listening() {
                "listening".to_owned()
            } else if busy > 0 {
                format!("working · {}", super::plural(busy, "comment"))
            } else {
                "gone".to_owned()
            };
            // A long hover: the whole title, where it works, when it was last heard (X59).
            let said: gpui::SharedString = format!(
                "{}\n{} in {} · last heard {}",
                c.title,
                c.agent,
                super::format::home_relative(&c.repo),
                super::hover::full_date(c.beat)
            )
            .into();
            div()
                .id(("chat-row", ix))
                .mx(px(6.))
                .px(px(8.))
                .py(px(6.))
                .flex()
                .items_start()
                .gap(px(8.))
                .tooltip_show_delay(super::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| super::TipOwned(said.clone())).into())
                .child(chat_badge(letter, hue, !c.listening() && busy == 0))
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
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(if c.title.is_empty() {
                                            c.agent.clone()
                                        } else {
                                            c.title.clone()
                                        }),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(px(11.))
                                        .text_color(if c.listening() {
                                            theme::added()
                                        } else if busy > 0 {
                                            theme::agent()
                                        } else {
                                            theme::faint()
                                        })
                                        .child(state),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(c.agent.clone())
                                .children((!c.branch.is_empty()).then(|| {
                                    div().font_family(theme::CODE_FONT).child(c.branch.clone())
                                })),
                        ),
                )
        });
        // Where a note written here goes, and the way to change it (#77).
        let branch = self.page_branch();
        let route = self.route_of(branch.as_deref()).cloned().map(|chat| {
            let (letter, hue) = self.chat_mark(&chat.session);
            let on = branch.clone().unwrap_or_default();
            div()
                .mx(px(14.))
                .mt(px(6.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(theme::muted())
                .child(match &branch {
                    Some(b) => format!("Notes on {b} go to"),
                    None => "Notes go to".to_owned(),
                })
                .child(chat_badge(letter, hue, false))
                .child(
                    div()
                        .text_color(theme::text())
                        .child(if chat.title.is_empty() {
                            chat.agent.clone()
                        } else {
                            chat.title.clone()
                        }),
                )
                .child(
                    div()
                        .id("route-change")
                        .text_color(theme::accent())
                        .cursor_pointer()
                        .child("Change…")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.open_route_pick(on.clone(), cx)
                        })),
                )
        });
        // One list, newest first, each row its kind as a chip (#76, B1); the filter narrows it.
        let alive = |session: &str| self.chat_listens(session);
        let archived = self.comments.iter().filter(|c| c.archived).count();
        let listed: Vec<&crate::review::Comment> = self
            .listed_agent_ids()
            .into_iter()
            .filter_map(|id| self.comments.iter().find(|c| c.id == id))
            .collect();
        let counts = |f: Filter| {
            self.comments
                .iter()
                .filter(|c| !c.archived && f.takes(kind_of(c, &alive)))
                .count()
        };
        let filters = [Filter::All, Filter::NeedsYou, Filter::Waiting, Filter::Done]
            .into_iter()
            .fold(super::diff_view::group(), |group, f| {
                group.child(
                    super::diff_view::chip(
                        f.id(),
                        format!("{} {}", f.label(), counts(f)),
                        self.agents_filter == f && !self.agents_archived,
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.agents_filter = f;
                            this.agents_archived = false;
                            cx.notify();
                        },
                    )),
                )
            });
        let rows: Vec<_> = listed
            .into_iter()
            .map(|c| self.agent_row(c, kind_of(c, &alive), cx))
            .collect();
        let empty = rows.is_empty();
        let send = (drafts > 0).then(|| {
            let send = div()
                .id("send-to-agent")
                .px_2()
                .h(px(24.))
                .flex()
                .flex_none()
                .items_center()
                .gap_2()
                .rounded(px(ROW_RADIUS))
                .bg(theme::agent())
                .text_color(theme::base())
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .child(if drafts > 1 {
                    format!("Send all {drafts}")
                } else {
                    "Send".to_owned()
                })
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    let ids = this.agent_drafts();
                    this.send_agent(ids, cx)
                }));
            // The chat it goes to changes rarely: here, beside it.
            let chat = div()
                .id("send-choose-chat")
                .px_2()
                .h(px(24.))
                .flex()
                .items_center()
                .rounded(px(ROW_RADIUS))
                .text_size(px(12.))
                .text_color(theme::muted())
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                .child("Choose the chat…")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_agent_pick(cx)));
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(send)
                .child(chat)
                .into_any_element()
        });
        ToolView {
            title: "Agents".into(),
            count: (drafts > 0).then(|| super::plural(drafts, "draft")),
            actions: Vec::new(),
            body: div()
                .id("agents-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .py(px(6.))
                .child(group_heading("Chats here".to_owned(), None))
                .children(chats)
                .children(route)
                .children(self.agent_chats.is_empty().then(|| {
                    div()
                        .px(px(14.))
                        .py(px(4.))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child("No chat listens in this repository. An agent starts listening with GitLance agent-watch (Settings → Agents).")
                }))
                .child(div().px(px(10.)).pt(px(8.)).pb(px(6.)).child(filters))
                .children(rows)
                .children(empty.then(|| {
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(if self.comments.is_empty() {
                            "No comments for your agent. Write one on a line with c and pick Your agent."
                        } else {
                            "Nothing here."
                        })
                }))
                .into_any_element(),
            foot: Some(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .children(send)
                    .child(div().flex_1())
                    .children(self.comments.iter().any(|c| {
                        !c.archived && c.state == State::Resolved
                    }).then(|| {
                        foot_button("archive-resolved", "Archive resolved".to_owned())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.archive_resolved(cx)
                            }))
                    }))
                    .children((archived > 0 || self.agents_archived).then(|| {
                        foot_button(
                            "show-archived",
                            if self.agents_archived {
                                "Back to the list".to_owned()
                            } else {
                                format!("Archived · {archived}")
                            },
                        )
                        .text_color(theme::accent())
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.agents_archived = !this.agents_archived;
                            this.agent_row = None;
                            cx.notify();
                        }))
                    }))
                    .into_any_element(),
            ),
        }
    }

    /// A row of the Agents window: its kind as a chip, where it is, its text; picked, what can be
    /// done with it (#76).
    fn agent_row(
        &self,
        c: &crate::review::Comment,
        kind: Kind,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = c.id;
        let picked = self.agent_row == Some(id);
        let place = format!(
            "{}:{}",
            c.path.rsplit('/').next().unwrap_or(&c.path),
            c.line
        );
        let when = c.replies.last().map(|r| r.at).filter(|&at| at > 0);
        let mut actions: Vec<gpui::AnyElement> = Vec::new();
        // Where a note its gone chat never took would go now (#77).
        let instead = self.route_of(c.branch.as_deref()).map(|chat| {
            if chat.title.is_empty() {
                chat.agent.clone()
            } else {
                chat.title.clone()
            }
        });
        let mut action =
            |label: String, primary: bool, run: fn(&mut Self, u64, &mut Context<Self>)| {
                actions.push(
                    row_button(
                        ("agent-act", id as usize * 8 + actions.len()),
                        label,
                        primary,
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        run(this, id, cx)
                    }))
                    .into_any_element(),
                );
            };
        if c.archived {
            action("Unarchive".to_owned(), false, |this, id, cx| {
                this.set_archived(id, false, cx)
            });
        } else {
            match kind {
                Kind::Draft => {
                    action("Send".to_owned(), true, |this, id, cx| {
                        this.send_agent(vec![id], cx)
                    });
                    action("Edit".to_owned(), false, |this, id, cx| {
                        this.start_edit(super::comments::Change::Agent(id), cx)
                    });
                    action("Delete".to_owned(), false, |this, id, cx| {
                        this.delete_draft(id, cx)
                    });
                }
                Kind::Asks => action("Answer…".to_owned(), true, |this, id, cx| {
                    this.jump_to(id, cx)
                }),
                Kind::Sent => action("Withdraw".to_owned(), false, |this, id, cx| {
                    this.withdraw(id, cx)
                }),
                // A note a gone chat never took (#77): to the chat its route takes now, or back.
                Kind::Waiting => {
                    if let Some(to) = instead {
                        action(format!("Send to {to} instead"), true, |this, id, cx| {
                            this.resend(id, cx)
                        });
                    }
                    action("Back to drafts".to_owned(), false, |this, id, cx| {
                        this.withdraw(id, cx)
                    });
                }
                Kind::Addressed => {
                    action("Resolve".to_owned(), true, |this, id, cx| {
                        this.resolve_agent_comment(id, cx)
                    });
                    action("Archive".to_owned(), false, |this, id, cx| {
                        this.set_archived(id, true, cx)
                    });
                }
                Kind::Working | Kind::Resolved => {
                    action("Archive".to_owned(), false, |this, id, cx| {
                        this.set_archived(id, true, cx)
                    })
                }
            }
        }
        div()
            .id(("agent-row", id as usize))
            .px(px(12.))
            .py(px(8.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .border_b_1()
            .border_color(theme::island_border())
            .cursor_pointer()
            .when(picked, |s| s.bg(theme::hover()))
            .when(!picked, |s| s.hover(|s| s.bg(theme::hover())))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .child(kind_chip(kind))
                    // The chat it went to (#77).
                    .children(c.sent.as_ref().filter(|s| !s.session.is_empty()).map(|s| {
                        let (letter, hue) = self.chat_mark(&s.session);
                        chat_badge(letter, hue, !self.chat_listens(&s.session))
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(theme::CODE_FONT)
                            .text_color(theme::muted())
                            .child(place),
                    )
                    .children(when.map(|at| {
                        div()
                            .flex_none()
                            .text_color(theme::faint())
                            .child(super::format::ago(at))
                    }))
                    // A draft goes with × too, no question asked.
                    .children((kind == Kind::Draft && !c.archived).then(|| {
                        div()
                            .id(("agent-delete", id as usize))
                            .flex_none()
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .hover(|s| s.bg(theme::selected()))
                            .child(icons::icon("x").size(px(11.)).text_color(theme::faint()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.delete_draft(id, cx)
                            }))
                    })),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_clamp(2)
                    .child(self.one_line(&c.body)),
            )
            .children((picked && !actions.is_empty()).then(|| {
                div()
                    .pt(px(4.))
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .children(actions)
            }))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.agent_row = Some(id);
                this.jump_to(id, cx)
            }))
            .into_any_element()
    }

    /// The comments the Agents window lists, newest first: the filter's, or the archived ones.
    fn listed_agent_ids(&self) -> Vec<u64> {
        let alive = |session: &str| self.chat_listens(session);
        let mut ids: Vec<u64> = self
            .comments
            .iter()
            .filter(|c| c.archived == self.agents_archived)
            .filter(|c| self.agents_archived || self.agents_filter.takes(kind_of(c, &alive)))
            .map(|c| c.id)
            .collect();
        ids.sort_by_key(|&id| std::cmp::Reverse(id));
        ids
    }

    /// ↑ ↓ in the Agents window: another row picked, shown on its line.
    pub(super) fn step_agent_row(&mut self, down: bool, cx: &mut Context<Self>) {
        let ids = self.listed_agent_ids();
        let at = self
            .agent_row
            .and_then(|id| ids.iter().position(|&i| i == id));
        let to = match (at, down) {
            (Some(i), true) => (i + 1).min(ids.len().saturating_sub(1)),
            (Some(i), false) => i.saturating_sub(1),
            (None, _) => 0,
        };
        if let Some(&id) = ids.get(to) {
            self.agent_row = Some(id);
            self.jump_to(id, cx);
        }
        cx.notify();
    }

    /// ↵ in the Agents window: the picked row's first action — Send, Answer, Withdraw, Resolve,
    /// Archive.
    pub(super) fn agent_row_action(&mut self, cx: &mut Context<Self>) {
        let alive = |session: &str| self.chat_listens(session);
        let Some((id, kind, archived)) = self
            .agent_row
            .and_then(|id| self.comments.iter().find(|c| c.id == id))
            .map(|c| (c.id, kind_of(c, &alive), c.archived))
        else {
            return;
        };
        match (archived, kind) {
            (true, _) => self.set_archived(id, false, cx),
            (false, Kind::Draft) => self.send_agent(vec![id], cx),
            (false, Kind::Asks) => self.jump_to(id, cx),
            (false, Kind::Sent | Kind::Waiting) => self.withdraw(id, cx),
            (false, Kind::Addressed) => self.resolve_agent_comment(id, cx),
            (false, Kind::Working | Kind::Resolved) => self.set_archived(id, true, cx),
        }
    }

    /// A draft goes, no question asked; the toast's Undo brings it back (#76).
    pub(super) fn delete_draft(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(comment) = self.comments.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        self.delete_comment(id, cx);
        if self.agent_row == Some(id) {
            self.agent_row = None;
        }
        self.toast_deleted(comment, cx);
    }

    /// A sent one its chat has not taken yet comes back to the drafts.
    fn withdraw(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(c) = self.comments.iter_mut().find(|c| c.id == id) {
            c.state = State::Draft;
            c.sent = None;
            self.save_comments();
            self.refresh_rows();
            cx.notify();
        }
    }

    fn set_archived(&mut self, id: u64, on: bool, cx: &mut Context<Self>) {
        if let Some(c) = self.comments.iter_mut().find(|c| c.id == id) {
            c.archived = on;
            self.save_comments();
            self.refresh_rows();
            cx.notify();
        }
    }

    /// Archive resolved: every resolved one goes off the lists and its line.
    fn archive_resolved(&mut self, cx: &mut Context<Self>) {
        for c in self
            .comments
            .iter_mut()
            .filter(|c| c.state == State::Resolved)
        {
            c.archived = true;
        }
        self.save_comments();
        self.refresh_rows();
        cx.notify();
    }

    /// The reviewer agrees the agent did it.
    pub(super) fn resolve_agent_comment(&mut self, id: u64, cx: &mut Context<Self>) {
        self.set_agent_state(id, State::Resolved, cx);
    }

    /// Not done: back to a draft, to be sent again (with what was said so far).
    pub(super) fn reopen_agent_comment(&mut self, id: u64, cx: &mut Context<Self>) {
        self.set_agent_state(id, State::Draft, cx);
    }

    fn set_agent_state(&mut self, id: u64, state: State, cx: &mut Context<Self>) {
        if let Some(c) = self.comments.iter_mut().find(|c| c.id == id) {
            c.state = state;
            self.save_comments();
            self.refresh_rows();
            cx.notify();
        }
    }

    pub(super) fn render_agent_pick(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let pick = self.agent_pick.as_ref()?;
        let count = pick.ids.len();
        let rows = pick.chats.iter().enumerate().map(|(ix, chat)| {
            let on = ix == pick.selected;
            let (dot, state) = if chat.session.is_empty() {
                ("+", "opens a terminal".to_owned())
            } else if chat.listening {
                ("●", "listening".to_owned())
            } else {
                ("○", "gets them when it listens".to_owned())
            };
            div()
                .id(("agent-chat", ix))
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded(px(ROW_RADIUS))
                .when(on, |s| s.bg(theme::selected()))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(
                    div()
                        .w(px(14.))
                        .text_color(if chat.listening {
                            theme::accent()
                        } else {
                            theme::faint()
                        })
                        .child(dot),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(super::hover::cut(
                            ("pick-chat-title", ix),
                            chat.title.clone(),
                        ))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child(format!("{} · {state}", chat.agent)),
                        ),
                )
                .children(on.then(|| keys::caps("↵", On::Plain)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.send_to_chat(ix, cx)))
        });
        Some(
            div()
                .id("agent-pick-backdrop")
                .absolute()
                .size_full()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(120.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.agent_pick = None;
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .occlude()
                        .w(px(520.))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_4()
                        .rounded(px(ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .text_size(px(13.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(match &pick.route {
                                    Some(b) if !b.is_empty() => format!("Notes on {b} go to"),
                                    Some(_) => "Notes go to".to_owned(),
                                    None => format!(
                                        "Send {count} comment{} to",
                                        if count == 1 { "" } else { "s" }
                                    ),
                                }),
                        )
                        .children(rows)
                        .child(
                            div()
                                .pt_1()
                                .text_size(px(12.))
                                .text_color(theme::faint())
                                .child(keys::text(
                                    "A chat listens once its agent runs GitLance's watcher — Settings → Agents connects Claude Code. ↑↓ choose, esc cancels.",
                                ).into_owned()),
                        ),
                ),
        )
    }

    /// Settings → Agents: connect Claude Code (every profile), the commands for others.
    pub(super) fn render_agents_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let server = agents::server_command();
        let claude_cmd = format!("claude mcp add -s user gitlance -- {server}");
        let codex_cmd = format!("codex mcp add gitlance -- {server}");
        let profiles = agents::claude_profiles()
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect::<Vec<_>>()
            .join(", ");
        let connect = div()
            .id("agents-connect-claude")
            .px_3()
            .h(px(28.))
            .flex()
            .items_center()
            .rounded(px(ROW_RADIUS))
            .bg(theme::agent())
            .text_color(theme::base())
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .child(if self.agent_setup_busy {
                "Connecting…"
            } else {
                "Connect"
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.connect_claude(cx)));
        let command = |id: &'static str, label: &'static str, text: String| {
            let copy = text.clone();
            div()
                .flex()
                .flex_col()
                .gap_1()
                .py(px(8.))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(label),
                )
                .child(
                    div()
                        .id(id)
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .rounded(px(ROW_RADIUS))
                        .bg(theme::base())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_family(theme::CODE_FONT)
                                .text_size(px(12.))
                                .child(text),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child("Copy"),
                        )
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                        }),
                )
        };
        div()
            .flex()
            .flex_col()
            .child(super::settings::item(
                "Claude Code",
                "Adds GitLance's MCP server to every Claude Code profile. A chat then starts GitLance's watcher in the background on its own, shows up in Send to agent (⌥⌘A), and answers each comment; you resolve them here. A chat that was already open needs a restart, or ask it to “listen to GitLance”.",
                connect,
            ))
            .children(self.agent_setup.clone().map(|text| {
                div()
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(text)
            }))
            .child(
                div()
                    .py(px(6.))
                    .text_size(px(12.))
                    .text_color(theme::faint())
                    .child(if profiles.is_empty() {
                        "No Claude Code profile found.".to_owned()
                    } else {
                        format!("Profiles: {profiles}")
                    }),
            )
            .child(command("agents-cmd-claude", "Or by hand, in a terminal:", claude_cmd))
            .child(command(
                "agents-cmd-codex",
                "Codex (its tools; chats are not listed in Send to agent yet):",
                codex_cmd,
            ))
    }

    fn connect_claude(&mut self, cx: &mut Context<Self>) {
        if self.agent_setup_busy {
            return;
        }
        self.agent_setup_busy = true;
        self.agent_setup = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { agents::connect_claude() })
                .await;
            this.update(cx, |this, cx| {
                this.agent_setup_busy = false;
                this.agent_setup = Some(match result {
                    Ok(text) => text,
                    Err(text) => format!("Not connected — {text}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// Where a comment is in its life with the agent, as the Agents window says it (#76).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The agent asked something and waits for the answer.
    Asks,
    /// Not sent yet, or sent back.
    Draft,
    /// Sent, not taken by its chat yet.
    Sent,
    /// Sent to a chat that is gone, never taken.
    Waiting,
    /// Taken by its chat.
    Working,
    Addressed,
    Resolved,
}

/// `c`'s kind; `alive` says whether a chat (by session) still listens.
pub fn kind_of(c: &crate::review::Comment, alive: &dyn Fn(&str) -> bool) -> Kind {
    match c.state {
        State::Draft => Kind::Draft,
        State::Sent if !c.replies.is_empty() && c.answer.is_none() => Kind::Asks,
        State::Sent => match &c.sent {
            Some(s) if s.delivered => Kind::Working,
            Some(s) if !s.session.is_empty() && !alive(&s.session) => Kind::Waiting,
            _ => Kind::Sent,
        },
        State::Addressed => Kind::Addressed,
        State::Resolved => Kind::Resolved,
    }
}

/// The Agents window's filters: what needs you (an answer, a send), what waits on the agent, what
/// is done.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    NeedsYou,
    Waiting,
    Done,
}

impl Filter {
    fn takes(self, kind: Kind) -> bool {
        match self {
            Filter::All => true,
            Filter::NeedsYou => matches!(kind, Kind::Asks | Kind::Draft),
            Filter::Waiting => matches!(kind, Kind::Sent | Kind::Waiting | Kind::Working),
            Filter::Done => matches!(kind, Kind::Addressed | Kind::Resolved),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::NeedsYou => "Needs you",
            Filter::Waiting => "Waiting",
            Filter::Done => "Done",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Filter::All => "agents-all",
            Filter::NeedsYou => "agents-needs-you",
            Filter::Waiting => "agents-waiting",
            Filter::Done => "agents-done",
        }
    }
}

/// A kind's chip: the agent's purple for what is the agent's, green for what is done.
fn kind_chip(kind: Kind) -> gpui::Div {
    let (word, color, filled) = match kind {
        Kind::Asks => ("Asks you", theme::agent(), true),
        Kind::Draft => ("Draft", theme::agent(), false),
        Kind::Sent => ("Sent", theme::muted(), false),
        Kind::Waiting => ("Waiting", theme::warning(), false),
        Kind::Working => ("Working", theme::agent(), false),
        Kind::Addressed => ("Addressed", theme::added(), false),
        Kind::Resolved => ("Resolved", theme::added(), true),
    };
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(4.))
        .border_1()
        .border_color(color)
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .when(filled, |s| s.bg(color).text_color(theme::base()))
        .when(!filled, |s| s.text_color(color))
        .child(word)
}

/// A button of a picked row: Send filled, the rest plain.
fn row_button(
    id: impl Into<gpui::ElementId>,
    label: String,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .h(px(24.))
        .flex()
        .items_center()
        .rounded(px(ROW_RADIUS))
        .text_size(px(12.))
        .cursor_pointer()
        .when(primary, |s| {
            s.bg(theme::agent())
                .text_color(theme::base())
                .font_weight(FontWeight::SEMIBOLD)
        })
        .when(!primary, |s| {
            s.border_1()
                .border_color(theme::island_border())
                .hover(|s| s.bg(theme::selected()))
        })
        .child(label)
}

fn foot_button(id: &'static str, label: String) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .h(px(24.))
        .flex()
        .items_center()
        .rounded(px(ROW_RADIUS))
        .text_size(px(12.))
        .text_color(theme::muted())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
        .child(label)
}

/// A chat's mark: its letter on its hue, hollow when it is gone.
pub(super) fn chat_badge(letter: char, hue: gpui::Rgba, gone: bool) -> gpui::Div {
    div()
        .flex_none()
        .size(px(18.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.))
        .font_weight(FontWeight::BOLD)
        .border_1()
        .border_color(hue)
        .when(!gone, |s| s.bg(hue).text_color(theme::base()))
        .when(gone, |s| s.text_color(hue))
        .child(letter.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{Comment, Reply, Sent};

    #[test]
    fn a_comment_is_of_the_kind_its_life_says() {
        let alive = |s: &str| s == "chat-1";
        let sent = |session: &str, delivered: bool| Comment {
            state: State::Sent,
            sent: Some(Sent {
                session: session.to_owned(),
                delivered,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(kind_of(&Comment::default(), &alive), Kind::Draft);
        assert_eq!(kind_of(&sent("chat-1", false), &alive), Kind::Sent);
        assert_eq!(kind_of(&sent("gone", false), &alive), Kind::Waiting);
        assert_eq!(kind_of(&sent("chat-1", true), &alive), Kind::Working);
        let asks = Comment {
            replies: vec![Reply::default()],
            ..sent("chat-1", true)
        };
        assert_eq!(kind_of(&asks, &alive), Kind::Asks);
        assert!(Filter::NeedsYou.takes(Kind::Asks) && Filter::NeedsYou.takes(Kind::Draft));
        assert!(Filter::Waiting.takes(Kind::Working) && !Filter::Waiting.takes(Kind::Resolved));
        assert!(Filter::Done.takes(Kind::Addressed) && Filter::All.takes(Kind::Sent));
    }
}
