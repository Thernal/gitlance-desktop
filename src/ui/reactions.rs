//! Reactions on a request's notes (Design #9 B, `docs/specs/reactions.md`): pills under a note
//! once it has some (yours outlined, hover names who, a click toggles yours); hovering a note
//! floats a bar on its corner — your five most used, ☺+ for the rest, 🤖 a note for your agent
//! (#50 G), Reply with the author already mentioned; ⇧E opens it on the selected discussion's newest note. ☺+ is a search over
//! every emoji (your recent first, then the rest), arrows and ↵, `:name:` at its foot. On GitHub
//! only its eight, no search. The five are counted on this Mac (`reactions-used.txt`).

use super::{ROW_RADIUS, Workspace, px, theme};
use crate::{emoji, mr, storage};
use gpui::{AnyElement, Context, Rgba, WeakEntity, div, prelude::*};
use std::collections::HashMap;

const USED: &str = "reactions-used.txt";
/// ☺+'s cells to a row.
const COLS: usize = 12;
/// Until there is a history: the five most people reach for.
const DEFAULT_FIVE: [&str; 5] = ["thumbsup", "heart", "tada", "eyes", "rocket"];

impl Workspace {
    /// Reads the reactions on every note of the open request, in the background.
    pub(super) fn load_awards(&mut self, cx: &mut Context<Self>) {
        let (Some(remote), Some(iid)) = (self.web.clone(), self.requests.current) else {
            return;
        };
        let notes: Vec<(u64, bool)> = self
            .requests
            .threads
            .iter()
            .flat_map(|t| {
                let issue = t.id.starts_with("issue|");
                t.notes.iter().map(move |n| (n.id, issue))
            })
            .collect();
        self.requests.award_task = Some(cx.spawn(async move |this, cx| {
            let awards = cx
                .background_executor()
                .spawn(async move { mr::awards(&remote, iid, &notes) })
                .await;
            this.update(cx, |this, cx| {
                if this.requests.current == Some(iid)
                    && let Ok(awards) = awards
                {
                    this.requests.awards = awards;
                    this.refresh_rows();
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Your five most used reactions (GitHub: from its eight).
    fn five(&self) -> Vec<&'static str> {
        let github = self.web.as_ref().is_some_and(|w| w.github);
        let mut counts: Vec<(String, usize)> = storage::read(USED)
            .lines()
            .filter_map(|l| {
                let (name, n) = l.split_once(' ')?;
                Some((name.to_owned(), n.parse().ok()?))
            })
            .collect();
        counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        let mut out: Vec<&'static str> = counts
            .iter()
            .filter_map(|(name, _)| emoji::ALL.iter().find(|(n, _)| n == name).map(|(n, _)| *n))
            .filter(|n| !github || emoji::github_name(n).is_some())
            .collect();
        for d in DEFAULT_FIVE {
            if !out.contains(&d) {
                out.push(d);
            }
        }
        out.truncate(5);
        out
    }

    fn count_use(name: &str) {
        let text = storage::read(USED);
        let mut lines: Vec<(String, usize)> = text
            .lines()
            .filter_map(|l| {
                let (n, c) = l.split_once(' ')?;
                Some((n.to_owned(), c.parse().ok()?))
            })
            .collect();
        match lines.iter_mut().find(|(n, _)| n == name) {
            Some((_, c)) => *c += 1,
            None => lines.push((name.to_owned(), 1)),
        }
        storage::save(
            USED,
            lines.iter().map(|(n, c)| format!("{n} {c}\n")).collect(),
        );
    }

    /// Adds your reaction `name` to note `note`, or takes it back.
    pub(super) fn toggle_reaction(&mut self, note: u64, name: &str, cx: &mut Context<Self>) {
        let (Some(remote), Some(iid)) = (self.web.clone(), self.requests.current) else {
            return;
        };
        let me = self.requests.login.clone().unwrap_or_default();
        let issue = self
            .requests
            .threads
            .iter()
            .find(|t| t.notes.iter().any(|n| n.id == note))
            .is_some_and(|t| t.id.starts_with("issue|"));
        let list = self.requests.awards.entry(note).or_default();
        let name = name.to_owned();
        let mine = list.iter().position(|a| a.name == name && a.user == me);
        // Shown at once; a failure puts it back and says why.
        let undo = list.clone();
        match mine {
            Some(at) => {
                let award = list.remove(at);
                self.spawn_reaction(note, undo, cx, move || {
                    mr::unaward(&remote, iid, note, issue, &award.name, award.id).map(|_| None)
                });
            }
            None => {
                list.push(mr::Award {
                    id: 0,
                    name: name.clone(),
                    user: me.clone(),
                });
                Self::count_use(&name);
                self.spawn_reaction(note, undo, cx, move || {
                    mr::award(&remote, iid, note, issue, &name).map(|id| Some((id, name)))
                });
            }
        }
        self.reaction_grid = None;
        self.refresh_rows();
        cx.notify();
    }

    fn spawn_reaction(
        &mut self,
        note: u64,
        undo: Vec<mr::Award>,
        cx: &mut Context<Self>,
        call: impl FnOnce() -> anyhow::Result<Option<(u64, String)>> + Send + 'static,
    ) {
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { call() }).await;
            this.update(cx, |this, cx| {
                match result {
                    // The new reaction learns its id, to be taken back later.
                    Ok(Some((id, name))) => {
                        if let Some(a) = this
                            .requests
                            .awards
                            .get_mut(&note)
                            .and_then(|l| l.iter_mut().find(|a| a.name == name && a.id == 0))
                        {
                            a.id = id;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        this.requests.awards.insert(note, undo);
                        this.error = Some(format!("{e:#}").into());
                        this.refresh_rows();
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Reply with the note's author already mentioned (#9 B, #51).
    pub(super) fn reply_mentioning(&mut self, thread: usize, login: &str, cx: &mut Context<Self>) {
        self.start_reply(thread, cx);
        if !login.is_empty() {
            self.insert_into_compose(&format!("@{login} "));
        }
        cx.notify();
    }

    /// Under a note: its reactions as pills, one per emoji, yours outlined.
    pub(super) fn reaction_pills(&self, note: u64, this: WeakEntity<Self>) -> Option<AnyElement> {
        let list = self.requests.awards.get(&note).filter(|l| !l.is_empty())?;
        let me = self.requests.login.clone().unwrap_or_default();
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for a in list {
            match groups.iter_mut().find(|(n, _)| *n == a.name) {
                Some((_, who)) => who.push(a.user.clone()),
                None => groups.push((a.name.clone(), vec![a.user.clone()])),
            }
        }
        Some(
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .mt(px(6.))
                .children(groups.into_iter().enumerate().map(|(i, (name, who))| {
                    let mine = who.contains(&me);
                    let tip: gpui::SharedString = format!(":{name}: {}", who.join(", ")).into();
                    let this = this.clone();
                    let toggle = name.clone();
                    div()
                        .id(("reaction", note as usize * 64 + i))
                        .h(px(24.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .rounded_full()
                        .border_1()
                        .border_color(if mine {
                            Rgba {
                                a: 0.55,
                                ..theme::accent()
                            }
                        } else {
                            theme::island_border()
                        })
                        .bg(if mine {
                            theme::info_bg()
                        } else {
                            theme::hover()
                        })
                        .cursor_pointer()
                        .child(emoji::get(&name).unwrap_or("·"))
                        .child(div().text_size(px(12.)).child(who.len().to_string()))
                        .tooltip_show_delay(crate::ui::hover::LONG)
                        .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                        .on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| this.toggle_reaction(note, &toggle, cx))
                                .ok();
                        })
                }))
                .into_any_element(),
        )
    }

    /// The bar on a hovered note: the five, ☺+ and Reply.
    pub(super) fn hover_bar(
        &self,
        thread: usize,
        note: u64,
        author: &str,
        this: WeakEntity<Self>,
    ) -> Option<AnyElement> {
        let open = self.reaction_grid == Some(note);
        if self.hover_note != Some(note) && !open {
            return None;
        }
        let cell = |id: (&'static str, usize), child: AnyElement| {
            div()
                .id(id)
                .size(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(ROW_RADIUS))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(child)
        };
        let author = author.to_owned();
        let five = self.five();
        let (grid_this, reply_this, agent_this) = (this.clone(), this.clone(), this.clone());
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .p(px(2.))
                .rounded(px(ROW_RADIUS))
                .border_1()
                .border_color(theme::island_border())
                .bg(theme::panel())
                .children(five.into_iter().enumerate().map(|(i, name)| {
                    let this = this.clone();
                    cell(
                        ("bar-react", note as usize * 8 + i),
                        div()
                            .child(emoji::get(name).unwrap_or("·"))
                            .into_any_element(),
                    )
                    .on_click(move |_, _, cx| {
                        this.update(cx, |this, cx| this.toggle_reaction(note, name, cx))
                            .ok();
                    })
                }))
                .child(
                    cell(
                        ("bar-more", note as usize),
                        div()
                            .text_color(if open { theme::text() } else { theme::muted() })
                            .child("☺+")
                            .into_any_element(),
                    )
                    .on_click(move |_, _, cx| {
                        grid_this
                            .update(cx, |this, cx| {
                                if open {
                                    this.close_reaction_grid(cx)
                                } else {
                                    this.open_reaction_grid(note, cx)
                                }
                            })
                            .ok();
                    }),
                )
                .child(
                    div()
                        .w(px(1.))
                        .h(px(18.))
                        .mx(px(2.))
                        .bg(theme::island_border()),
                )
                // A note on this discussion for your agent, kept on this Mac (#50 G).
                .child(
                    cell(
                        ("bar-agent", note as usize),
                        div().child("🤖").into_any_element(),
                    )
                    .tooltip_show_delay(crate::ui::hover::LONG)
                    .tooltip(|_, cx| {
                        cx.new(|_| super::Tip("A note for your agent — only on this Mac"))
                            .into()
                    })
                    .on_click(move |_, _, cx| {
                        agent_this
                            .update(cx, |this, cx| this.start_thread_note(thread, cx))
                            .ok();
                    }),
                )
                .child(
                    div()
                        .id(("bar-reply", note as usize))
                        .h(px(26.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .rounded(px(ROW_RADIUS))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                        .child("Reply")
                        .on_click(move |_, _, cx| {
                            reply_this
                                .update(cx, |this, cx| this.reply_mentioning(thread, &author, cx))
                                .ok();
                        }),
                )
                .into_any_element(),
        )
    }

    /// ⇧E: ☺+ on the newest note of the selected discussion, its search taking the keyboard.
    pub(super) fn react_focused(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self
            .acting_thread()
            .filter(|_| self.request_target().is_some())
        else {
            return;
        };
        let Some(note) = self.requests.threads.get(ix).and_then(|t| t.notes.last()) else {
            return;
        };
        self.thread_at = Some(ix);
        self.open_reaction_grid(note.id, cx);
    }

    fn open_reaction_grid(&mut self, note: u64, cx: &mut Context<Self>) {
        self.reaction_grid = Some(note);
        self.reaction_query.clear();
        self.reaction_at = 0;
        self.reset_field_edit();
        self.refresh_rows();
        cx.notify();
    }

    fn close_reaction_grid(&mut self, cx: &mut Context<Self>) {
        self.reaction_grid = None;
        self.reaction_query.clear();
        self.refresh_rows();
        cx.notify();
    }

    /// What ☺+ offers for its search: your recent first (empty search), then every other match.
    fn grid_names(&self) -> Vec<&'static str> {
        if self.web.as_ref().is_some_and(|w| w.github) {
            return emoji::GITHUB.iter().map(|(_, n)| *n).collect();
        }
        let q = self.reaction_query.trim().to_lowercase();
        if !q.is_empty() {
            return emoji::find(&q, usize::MAX)
                .into_iter()
                .map(|(n, _)| n)
                .collect();
        }
        let recent = self.five();
        let rest = emoji::ALL
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| !recent.contains(n));
        recent.iter().copied().chain(rest).collect()
    }

    /// Keys while ☺+ is open: typing searches, arrows move, ↵ reacts, esc closes.
    pub(super) fn reaction_key(&mut self, event: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        let Some(note) = self.reaction_grid else {
            return;
        };
        let key = &event.keystroke;
        let count = self.grid_names().len();
        let at = self.reaction_at;
        let moved = match key.key.as_str() {
            "left" => Some(at.saturating_sub(1)),
            "right" => Some((at + 1).min(count.saturating_sub(1))),
            "up" => Some(at.saturating_sub(COLS)),
            "down" => Some((at + COLS).min(count.saturating_sub(1))),
            _ => None,
        };
        if let Some(to) = moved.filter(|_| key.modifiers.number_of_modifiers() == 0) {
            self.reaction_at = to;
            return cx.notify();
        }
        match key.key.as_str() {
            "escape" => return self.close_reaction_grid(cx),
            "enter" => {
                if let Some(name) = self.grid_names().get(at).copied() {
                    self.toggle_reaction(note, name, cx);
                }
                return self.close_reaction_grid(cx);
            }
            _ => {}
        }
        if self.web.as_ref().is_some_and(|w| w.github) {
            return;
        }
        let mut query = std::mem::take(&mut self.reaction_query);
        let edit = super::input::edit(&mut query, &mut self.field_edit, key, cx);
        self.reaction_query = query;
        if matches!(edit, super::input::Edit::Changed) {
            self.reaction_at = 0;
        }
        cx.notify();
    }

    /// ☺+: a search over every reaction there is (GitHub: its eight), under the note.
    pub(super) fn reaction_grid(&self, note: u64, this: WeakEntity<Self>) -> Option<AnyElement> {
        if self.reaction_grid != Some(note) {
            return None;
        }
        let github = self.web.as_ref().is_some_and(|w| w.github);
        let names = self.grid_names();
        let at = self.reaction_at.min(names.len().saturating_sub(1));
        let search = (!github).then(|| {
            div()
                .mb(px(4.))
                .px(px(8.))
                .py(px(4.))
                .rounded(px(ROW_RADIUS))
                .border_1()
                .border_color(theme::focus())
                .bg(theme::base())
                .child(self.field_look(
                    &self.reaction_query,
                    true,
                    "Search emoji",
                    Default::default(),
                    this.clone(),
                ))
        });
        let recent = (!github && self.reaction_query.trim().is_empty()).then_some(5);
        let foot = names.get(at).map(|name| {
            div()
                .mt(px(4.))
                .flex()
                .gap_3()
                .text_size(px(11.))
                .text_color(theme::faint())
                .child(
                    div()
                        .flex_1()
                        .text_color(theme::muted())
                        .child(format!(":{name}:")),
                )
                .child("←↑↓→ move · ↵ react · esc")
        });
        let cells = names.into_iter().enumerate().map(|(i, name)| {
            let this = this.clone();
            let tip: gpui::SharedString = format!(":{name}:").into();
            div()
                .id(("grid-react", i))
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(ROW_RADIUS))
                .when(i == at, |s| s.bg(theme::selected()))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(emoji::get(name).unwrap_or("·"))
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                .on_click(move |_, _, cx| {
                    this.update(cx, |this, cx| this.toggle_reaction(note, name, cx))
                        .ok();
                })
        });
        let mut cells: Vec<AnyElement> = cells.map(IntoElement::into_any_element).collect();
        // Recent first, a line under them.
        if let Some(n) = recent.filter(|&n| n < cells.len()) {
            cells.insert(
                n,
                div()
                    .w_full()
                    .h(px(1.))
                    .my(px(3.))
                    .bg(theme::island_border())
                    .into_any_element(),
            );
        }
        Some(
            div()
                .mt(px(6.))
                .p(px(6.))
                .w(px(COLS as f32 * 30. + 14.))
                .rounded(px(8.))
                .border_1()
                .border_color(theme::island_border())
                .bg(theme::panel())
                .children(search)
                .child(
                    div()
                        .id(("grid-scroll", note as usize))
                        .max_h(px(220.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_wrap()
                        .gap(px(2.))
                        .children(cells),
                )
                .children(foot)
                .into_any_element(),
        )
    }
}

/// Keeps the map type named once.
pub type Awards = HashMap<u64, Vec<mr::Award>>;
