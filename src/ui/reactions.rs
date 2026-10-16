//! Reactions on a request's notes (Design #9 B, `docs/specs/reactions.md`): pills under a note
//! once it has some (yours outlined, hover names who, a click toggles yours); hovering a note
//! floats a bar on its corner — your five most used, ☺+ for the rest, Reply with the author
//! already mentioned. On GitHub only its eight. The five are counted on this Mac
//! (`reactions-used.txt`).

use super::{ROW_RADIUS, Workspace, px, theme};
use crate::{emoji, mr, storage};
use gpui::{AnyElement, Context, Rgba, WeakEntity, div, prelude::*};
use std::collections::HashMap;

const USED: &str = "reactions-used.txt";
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
        let (grid_this, reply_this) = (this.clone(), this.clone());
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
                                this.reaction_grid = if open { None } else { Some(note) };
                                cx.notify();
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

    /// ☺+: every reaction there is (GitHub: its eight), under the note.
    pub(super) fn reaction_grid(&self, note: u64, this: WeakEntity<Self>) -> Option<AnyElement> {
        if self.reaction_grid != Some(note) {
            return None;
        }
        let github = self.web.as_ref().is_some_and(|w| w.github);
        let names: Vec<&'static str> = if github {
            emoji::GITHUB.iter().map(|(_, n)| *n).collect()
        } else {
            emoji::ALL.iter().map(|(n, _)| *n).collect()
        };
        Some(
            div()
                .mt(px(6.))
                .p(px(6.))
                .max_w(px(420.))
                .rounded(px(8.))
                .border_1()
                .border_color(theme::island_border())
                .bg(theme::panel())
                .flex()
                .flex_wrap()
                .gap(px(2.))
                .children(names.into_iter().enumerate().map(|(i, name)| {
                    let this = this.clone();
                    let tip: gpui::SharedString = format!(":{name}:").into();
                    div()
                        .id(("grid-react", note as usize * 128 + i))
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(ROW_RADIUS))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .child(emoji::get(name).unwrap_or("·"))
                        .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                        .on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| this.toggle_reaction(note, name, cx))
                                .ok();
                        })
                }))
                .into_any_element(),
        )
    }
}

/// Keeps the map type named once.
pub type Awards = HashMap<u64, Vec<mr::Award>>;
