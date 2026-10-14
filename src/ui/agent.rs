//! Sending the review to an agent's chat (`crate::agents`): the reviewer's comments stay drafts
//! until *Send to agent* (⇧⌘A); a card then lists this repository's chats — those listening first,
//! then Claude Code's recent ones, which get the comments when they listen — and a new chat. The
//! agent's answers come back every couple of seconds, and the reviewer resolves or reopens each.

use super::keys::{self, On};
use super::px;
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
                        .child(div().truncate().child(chat.title.clone()))
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
            .bg(theme::focus())
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
