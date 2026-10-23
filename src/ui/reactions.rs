//! Reactions on a request's notes (Design #9 B, `docs/specs/reactions.md`): pills under a note
//! once it has some (yours outlined, hover names who, a click toggles yours); hovering a note
//! floats a bar on its corner — your five most used, ☺+ for the rest, 🤖 a note for your agent
//! (#50 G), Reply with the author already mentioned; ⇧E opens it on the selected discussion's newest note. ☺+ opens a
//! popup under the bar (#91): a search over every emoji GitLab has, your five first, then GitLab's
//! groups; arrows and ↵, `:name:` at its foot; `:name:` typed whole reacts with a name the list
//! lacks (a custom one). On GitHub only its eight, no search. The five are counted on this Mac
//! (`reactions-used.txt`).

use super::{ROW_RADIUS, Workspace, px, theme};
use crate::{emoji, mr, storage};
use gpui::{AnyElement, Context, Rgba, WeakEntity, div, prelude::*};
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

const USED: &str = "reactions-used.txt";
/// ☺+'s cells to a row, and a row's height.
const COLS: usize = 12;
const ROW: f32 = 30.;
/// How many rows show before it scrolls.
const ROWS_SHOWN: usize = 8;
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
            .filter_map(|(name, _)| emoji::all().iter().find(|e| e.name == name).map(|e| e.name))
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
        // A pill added pops in (#56, 11).
        self.mark_motion(&format!("award-{note}-{name}"), cx);
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
                    let pops = self.motion.recent(&format!("award-{note}-{name}"));
                    let pill = div()
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
                        // One the list lacks (a custom one) by its name.
                        .child(match emoji::get(&name) {
                            Some(glyph) => glyph.to_owned(),
                            None => format!(":{name}:"),
                        })
                        .child(div().text_size(px(12.)).child(who.len().to_string()))
                        .tooltip_show_delay(crate::ui::hover::LONG)
                        .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                        .on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| this.toggle_reaction(note, &toggle, cx))
                                .ok();
                        });
                    if pops {
                        super::motion::appear(
                            pill,
                            ("reaction-pop", note as usize * 64 + i),
                            super::motion::FAST,
                            |pill, v| pill.px(px(super::motion::mix(4., 8., v))).opacity(v),
                        )
                        .into_any_element()
                    } else {
                        pill.into_any_element()
                    }
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
        let popup = open.then(|| self.reaction_popup(note, this.clone()));
        let (grid_this, reply_this, agent_this) = (this.clone(), this.clone(), this.clone());
        Some(
            div()
                .relative()
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
                // ☺+'s popup hangs from the bar's lower right corner, over what is below
                // (above it near the window's foot), the discussion staying as it is.
                .children(popup.map(|popup| {
                    div().absolute().right_0().bottom_0().size_0().child(
                        gpui::deferred(
                            gpui::anchored()
                                .anchor(gpui::Anchor::TopRight)
                                .offset(gpui::point(px(0.), px(4.)))
                                .child(popup),
                        )
                        .with_priority(1),
                    )
                }))
                .into_any_element(),
        )
        // It fades in on the hovered note (#56, 11).
        .map(|bar| {
            super::motion::appear(
                div().child(bar),
                ("bar-coming", note as usize),
                super::motion::FAST,
                |bar, v| bar.opacity(v),
            )
            .into_any_element()
        })
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
        self.reaction_scroll
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
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

    /// What ☺+ offers, and its rows: your five then GitLab's groups, each under its name; searching,
    /// the matches, and `:name:` typed whole that the list lacks as a cell of its own. GitHub: its
    /// eight.
    fn grid(&self) -> (Vec<Cow<'static, str>>, Vec<GridRow>) {
        if self.web.as_ref().is_some_and(|w| w.github) {
            let names: Vec<_> = emoji::GITHUB
                .iter()
                .map(|(_, n)| Cow::Borrowed(*n))
                .collect();
            let rows = cell_rows(0..names.len());
            return (names, rows);
        }
        let typed = self.reaction_query.trim();
        if !typed.is_empty() {
            let mut names: Vec<Cow<'static, str>> = emoji::find(typed, usize::MAX)
                .into_iter()
                .map(|(n, _)| Cow::Borrowed(n))
                .collect();
            if let Some(name) = typed
                .strip_prefix(':')
                .and_then(|t| t.strip_suffix(':'))
                .filter(|n| emoji::could_be_name(n) && emoji::get(n).is_none())
            {
                names.push(Cow::Owned(name.to_owned()));
            }
            let rows = cell_rows(0..names.len());
            return (names, rows);
        }
        let mut names: Vec<Cow<'static, str>> = Vec::new();
        let mut rows = Vec::new();
        let mut group = |title: &'static str, of: &mut dyn Iterator<Item = &'static str>| {
            let from = names.len();
            names.extend(of.map(Cow::Borrowed));
            rows.push(GridRow::Group(title));
            rows.extend(cell_rows(from..names.len()));
        };
        group("Frequently used", &mut self.five().into_iter());
        let all = emoji::all();
        let mut at = 0;
        while let Some(first) = all[at..].iter().position(|e| !e.group.is_empty()) {
            let title = all[at + first].group;
            let end = all[at + first..]
                .iter()
                .position(|e| e.group != title && !e.group.is_empty())
                .map_or(all.len(), |n| at + first + n);
            group(
                title,
                &mut all[at + first..end]
                    .iter()
                    .filter(|e| !e.group.is_empty())
                    .map(|e| e.name),
            );
            at = end;
        }
        (names, rows)
    }

    /// Keys while ☺+ is open: typing searches, arrows move (↑ ↓ by row, across the groups), ↵
    /// reacts, esc closes.
    pub(super) fn reaction_key(&mut self, event: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        let Some(note) = self.reaction_grid else {
            return;
        };
        let key = &event.keystroke;
        let (names, rows) = self.grid();
        let at = self.reaction_at.min(names.len().saturating_sub(1));
        let moved = match key.key.as_str() {
            "left" => Some(at.saturating_sub(1)),
            "right" => Some((at + 1).min(names.len().saturating_sub(1))),
            "up" => Some(row_step(&rows, at, false)),
            "down" => Some(row_step(&rows, at, true)),
            _ => None,
        };
        if let Some(to) = moved.filter(|_| key.modifiers.number_of_modifiers() == 0) {
            self.reaction_at = to;
            if let Some(row) = row_of(&rows, to) {
                self.reaction_scroll
                    .scroll_to_item(row, gpui::ScrollStrategy::Nearest);
            }
            return cx.notify();
        }
        match key.key.as_str() {
            "escape" => return self.close_reaction_grid(cx),
            "enter" => {
                if let Some(name) = names.get(at) {
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
            self.reaction_scroll
                .scroll_to_item(0, gpui::ScrollStrategy::Top);
        }
        cx.notify();
    }

    /// ☺+'s popup: the search, the emoji in rows (only those on screen are built), the name of
    /// the one picked at its foot.
    fn reaction_popup(&self, note: u64, this: WeakEntity<Self>) -> AnyElement {
        let github = self.web.as_ref().is_some_and(|w| w.github);
        let (names, rows) = self.grid();
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
                    "Search emoji, or type :name:",
                    Default::default(),
                    this.clone(),
                ))
        });
        let foot =
            match names.get(at) {
                Some(name) => div().flex_1().truncate().text_color(theme::muted()).child(
                    match emoji::get(name) {
                        Some(_) => format!(":{name}:"),
                        None => format!(":{name}: — by name, if GitLab has it"),
                    },
                ),
                None => div().flex_1().child("No emoji by that name"),
            };
        let foot = div()
            .mt(px(4.))
            .flex()
            .gap_3()
            .text_size(px(11.))
            .text_color(theme::faint())
            .child(foot)
            .child("←↑↓→ · ↵ react · esc");
        let shown = rows.len().clamp(1, ROWS_SHOWN);
        let (names, rows) = (Rc::new(names), Rc::new(rows));
        let close = this.clone();
        let list = gpui::uniform_list(
            ("reaction-rows", note as usize),
            rows.len(),
            move |range: Range<usize>, _, _| {
                range
                    .map(|r| match &rows[r] {
                        GridRow::Group(title) => div()
                            .h(px(ROW))
                            .px(px(4.))
                            .flex()
                            .items_end()
                            .pb(px(4.))
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(*title)
                            .into_any_element(),
                        GridRow::Cells(cells) => div()
                            .h(px(ROW))
                            .flex()
                            .gap(px(2.))
                            .children(
                                cells
                                    .clone()
                                    .map(|i| grid_cell(note, i, &names[i], i == at, this.clone())),
                            )
                            .into_any_element(),
                    })
                    .collect()
            },
        )
        .track_scroll(&self.reaction_scroll)
        .h(px(shown as f32 * ROW));
        let popup = div()
            .id(("reaction-popup", note as usize))
            .occlude()
            .p(px(6.))
            .w(px(COLS as f32 * ROW + 14.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme::island_border())
            .bg(theme::panel())
            .shadow_lg()
            .children(search)
            .child(list)
            .child(foot)
            // A press anywhere else closes it.
            .on_mouse_down_out(move |_, _, cx| {
                close
                    .update(cx, |this, cx| {
                        if this.reaction_grid == Some(note) {
                            this.close_reaction_grid(cx)
                        }
                    })
                    .ok();
            });
        // It drops into place as it fades in, as a menu does.
        super::motion::appear(
            popup,
            ("reaction-popup-coming", note as usize),
            super::motion::FAST,
            |popup, v| popup.opacity(v).mt(px(super::motion::mix(-4., 0., v))),
        )
        .into_any_element()
    }
}

/// One of ☺+'s rows: a group's name, or up to `COLS` emoji by their place in the list.
#[derive(Clone)]
enum GridRow {
    Group(&'static str),
    Cells(Range<usize>),
}

fn cell_rows(of: Range<usize>) -> Vec<GridRow> {
    of.clone()
        .step_by(COLS)
        .map(|from| GridRow::Cells(from..(from + COLS).min(of.end)))
        .collect()
}

/// The row that holds emoji `at`.
fn row_of(rows: &[GridRow], at: usize) -> Option<usize> {
    rows.iter()
        .position(|r| matches!(r, GridRow::Cells(c) if c.contains(&at)))
}

/// ↑ or ↓ from emoji `at`: the same column of the row of emoji before or after (over a group's
/// name), the nearest when that row is shorter; at the first or last row it stays.
fn row_step(rows: &[GridRow], at: usize, down: bool) -> usize {
    let Some(row) = row_of(rows, at) else {
        return at;
    };
    let GridRow::Cells(here) = &rows[row] else {
        return at;
    };
    let col = at - here.start;
    let mut others: Box<dyn Iterator<Item = &GridRow>> = if down {
        Box::new(rows[row + 1..].iter())
    } else {
        Box::new(rows[..row].iter().rev())
    };
    match others.find_map(|r| match r {
        GridRow::Cells(c) => Some(c.clone()),
        GridRow::Group(_) => None,
    }) {
        Some(there) => (there.start + col).min(there.end - 1),
        None => at,
    }
}

/// An emoji of ☺+: a click reacts with it. One the list lacks (typed as `:name:`) shows its name.
fn grid_cell(
    note: u64,
    i: usize,
    name: &str,
    picked: bool,
    this: WeakEntity<Workspace>,
) -> AnyElement {
    let tip: gpui::SharedString = format!(":{name}:").into();
    let react = name.to_owned();
    let glyph = emoji::get(name);
    div()
        .id(("grid-react", i))
        .h(px(28.))
        .min_w(px(28.))
        .when(glyph.is_none(), |s| s.px(px(6.)).text_size(px(12.)))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(ROW_RADIUS))
        .when(picked, |s| s.bg(theme::selected()))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(match glyph {
            Some(glyph) => glyph.to_owned(),
            None => format!(":{name}:"),
        })
        .tooltip_show_delay(crate::ui::hover::LONG)
        .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
        .on_click(move |_, _, cx| {
            this.update(cx, |this, cx| this.toggle_reaction(note, &react, cx))
                .ok();
        })
        .into_any_element()
}

/// Keeps the map type named once.
pub type Awards = HashMap<u64, Vec<mr::Award>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_and_down_keep_the_column_across_groups() {
        // A group of 5, then one of 14: rows 0..5 · 5..17 · 17..19, names between.
        let mut rows = vec![GridRow::Group("a")];
        rows.extend(cell_rows(0..5));
        rows.push(GridRow::Group("b"));
        rows.extend(cell_rows(5..19));
        assert_eq!(row_of(&rows, 16), Some(3));
        // Down from the short row's fourth: the next group's fourth.
        assert_eq!(row_step(&rows, 3, true), 8);
        // Down into a shorter row: its last.
        assert_eq!(row_step(&rows, 15, true), 18);
        // Up over a group's name: the short row, as far as it goes.
        assert_eq!(row_step(&rows, 11, false), 4);
        // The first and the last row stay.
        assert_eq!(row_step(&rows, 2, false), 2);
        assert_eq!(row_step(&rows, 18, true), 18);
    }
}
