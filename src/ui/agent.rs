//! Sending the review to an agent's chat (`crate::agents`): the reviewer's comments stay drafts
//! until *Send to agent* (⇧⌘A); a card then lists this repository's chats — those listening first,
//! then Claude Code's recent ones, which get the comments when they listen — and a new chat. The
//! agent's answers come back every couple of seconds, and the reviewer resolves or reopens each.

use super::keys::{self, On};
use super::px;
use super::requests::{comment_item, group_heading};
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

/// The "send to which chat?" card.
pub struct AgentPick {
    chats: Vec<PickChat>,
    selected: usize,
    /// The comments it sends.
    ids: Vec<u64>,
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
        let mut chats: Vec<PickChat> = Self::bus()
            .map(|bus| bus.chats(&root))
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
        });
        cx.notify();
    }

    /// Sends `ids` to the chat they usually go to (#50 F: there is usually one): the last one
    /// comments went to while it listens, else any that listens; with none, the card asks.
    pub(super) fn send_agent(&mut self, ids: Vec<u64>, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let chats = Self::bus().map(|b| b.chats(&root)).unwrap_or_default();
        let last = self
            .comments
            .iter()
            .rev()
            .filter_map(|c| c.sent.as_ref())
            .find(|s| !s.session.is_empty())
            .map(|s| s.session.clone());
        let chat = chats
            .iter()
            .find(|c| Some(&c.session) == last.as_ref() && c.listening())
            .or_else(|| chats.iter().find(|c| c.listening()));
        let Some(chat) = chat.cloned() else {
            return self.open_agent_pick_for(ids, cx);
        };
        agents::send(&mut self.comments, &ids, &chat.session, &chat.title);
        self.save_comments();
        self.refresh_rows();
        cx.notify();
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
                Some(
                    div()
                        .id(("form-option", id as usize * 64 + i))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .px(px(8.))
                        .py(px(6.))
                        .rounded(px(ROW_RADIUS))
                        .cursor_pointer()
                        .when(on, |s| s.bg(theme::agent_bg()))
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
                        })
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
        if self.tool() == Some(super::tools::Tool::Agents) && self.refresh_chats() {
            cx.notify();
        }
        let (Some(root), Some(bus)) = (self.root.clone(), Self::bus()) else {
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
        let chats = match (self.root.as_ref(), Self::bus()) {
            (Some(root), Some(bus)) => bus.chats(root),
            _ => Vec::new(),
        };
        let changed = chats != self.agent_chats;
        self.agent_chats = chats;
        changed
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
        let chats =
            self.agent_chats.iter().enumerate().map(|(ix, c)| {
                let busy = if c.listening() {
                    0
                } else {
                    working(&c.session)
                };
                div()
                    .mx(px(6.))
                    .px(px(8.))
                    .py(px(6.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().flex_none().size(px(8.)).rounded_full().bg(
                                if c.listening() {
                                    theme::added()
                                } else if busy > 0 {
                                    theme::agent()
                                } else {
                                    theme::faint()
                                },
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(c.agent.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .when(busy > 0, |s| s.text_color(theme::agent()))
                                    .child(if c.listening() {
                                        "listening".to_owned()
                                    } else if busy > 0 {
                                        format!("working · {}", super::plural(busy, "comment"))
                                    } else {
                                        "quiet".to_owned()
                                    }),
                            ),
                    )
                    .children((!c.title.is_empty()).then(|| {
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
                            .id(("chat-title", ix))
                            .pl(px(16.))
                            .truncate()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(c.title.clone())
                            .tooltip_show_delay(super::hover::LONG)
                            .tooltip(move |_, cx| cx.new(|_| super::TipOwned(said.clone())).into())
                    }))
            });
        // Where each comment is in its life with the agent (#50: Drafts · Asks you · Sent ·
        // Addressed · Sent back · Resolved).
        #[derive(PartialEq)]
        enum Group {
            Drafts,
            Asks,
            Sent,
            Addressed,
            Back,
            Resolved,
        }
        let group_of = |c: &crate::review::Comment| match c.state {
            State::Draft if c.sent.is_some() && c.answer.is_none() => Group::Back,
            State::Draft => Group::Drafts,
            State::Sent if !c.replies.is_empty() && c.answer.is_none() => Group::Asks,
            State::Sent => Group::Sent,
            State::Addressed => Group::Addressed,
            State::Resolved => Group::Resolved,
        };
        let group = |which: Group, label: &'static str| {
            let items: Vec<_> = self
                .comments
                .iter()
                .filter(|c| group_of(c) == which)
                .map(|c| {
                    let id = c.id;
                    comment_item(
                        ("agent-comment", id as usize),
                        theme::agent(),
                        format!(
                            "{}:{}",
                            c.path.rsplit('/').next().unwrap_or(&c.path),
                            c.line
                        ),
                        self.one_line(&c.body),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.jump_to(id, cx)))
                })
                .collect();
            (!items.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .child(group_heading(format!("{label} · {}", items.len()), None))
                    .children(items)
            })
        };
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
                .child(keys::caps("⇧⌘A", On::Filled))
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
                .children(self.agent_chats.is_empty().then(|| {
                    div()
                        .px(px(14.))
                        .py(px(4.))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child("No chat listens in this repository. An agent starts listening with GitLance agent-watch (Settings → Agents).")
                }))
                .children(group(Group::Drafts, "Drafts"))
                .children(group(Group::Asks, "Asks you"))
                .children(group(Group::Sent, "Sent"))
                .children(group(Group::Addressed, "Addressed"))
                .children(group(Group::Back, "Sent back"))
                .children(group(Group::Resolved, "Resolved"))
                .children(self.comments.is_empty().then(|| {
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child("No comments for your agent. Write one on a line with c and pick Your agent.")
                }))
                .into_any_element(),
            foot: send,
        }
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
                                .child(format!(
                                    "Send {count} comment{} to",
                                    if count == 1 { "" } else { "s" }
                                )),
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
                "Adds GitLance's MCP server to every Claude Code profile. A chat then starts GitLance's watcher in the background on its own, shows up in Send to agent (⇧⌘A), and answers each comment; you resolve them here. A chat that was already open needs a restart, or ask it to “listen to GitLance”.",
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
