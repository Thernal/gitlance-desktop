//! Commit search: the field above the commit list, its keyboard handling, and the filtered view of
//! `Workspace::commits`. The matching itself is `crate::search`.

use super::diff_view::chip;
use super::input::{self, Edit};
use super::px;
use super::typing::{self, Input};
use super::{COMMIT_PAGE, Older, Workspace, rows, theme};
use crate::git::Repo;
use crate::search::{self, Query};
use crate::ui::rows::Row;
use git2::Oid;
use gpui::{
    ClickEvent, Context, HighlightStyle, KeyDownEvent, SharedString, StyledText, Task, div,
    prelude::*,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Qualifiers offered as tags under the field.
const QUALIFIERS: [&str; 3] = ["author:", "path:", "since:"];
/// Commits whose paths are read per background step.
const PATH_CHUNK: usize = 250;

#[derive(Default)]
pub struct Find {
    /// The text in the field; `None` while the field is not active.
    pub text: Option<String>,
    pub query: Query,
    /// Indices into `Workspace::commits` of the commits that match; `None` shows every commit.
    pub shown: Option<Vec<usize>>,
    /// Paths each commit touched, read in the background after a branch loads.
    paths: HashMap<Oid, Vec<String>>,
    /// Paths are still being read.
    pub indexing: bool,
    task: Option<Task<()>>,
}

impl Find {
    /// A new repository: nothing known about its commits.
    pub fn reset(&mut self) {
        self.paths.clear();
        self.indexing = false;
        self.task = None;
        self.shown = None;
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Workspace {
    /// Rows in the commit list.
    pub(super) fn commit_rows(&self) -> usize {
        self.find
            .shown
            .as_ref()
            .map_or(self.commits.len(), Vec::len)
    }

    /// The index in `commits` shown at list row `row`.
    pub(super) fn commit_at(&self, row: usize) -> usize {
        self.find.shown.as_ref().map_or(row, |s| s[row])
    }

    /// The list row of commit `ix`, if it is shown.
    pub(super) fn row_of(&self, ix: usize) -> Option<usize> {
        match &self.find.shown {
            None => Some(ix),
            Some(shown) => shown.iter().position(|&i| i == ix),
        }
    }

    /// The branch's older history, a page more (#25): as the list is scrolled near its end, and
    /// on and on while a search has not found what it looks for. A page is as long as what is
    /// listed, so a search through a long history reads it in a few.
    pub(super) fn load_older_commits(&mut self, cx: &mut Context<Self>) {
        let (Some(root), Some(older)) = (self.root.clone(), self.older.as_mut()) else {
            return;
        };
        if older.task.is_some() {
            return;
        }
        let (tip, skip) = (older.tip, self.commits.len());
        let count = skip.max(COMMIT_PAGE);
        older.task = Some(cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { Repo::open(&root)?.log(tip, skip, count, None) })
                .await;
            this.update(cx, |this, cx| {
                // Another branch, or the history read again meanwhile: not these.
                if this.older.as_ref().is_none_or(|o| o.tip != tip) || this.commits.len() != skip {
                    return;
                }
                match read {
                    Ok(page) => {
                        let more = page.len() >= count;
                        this.commits.extend(page);
                        this.older = more.then_some(Older { tip, task: None });
                        this.refilter();
                        this.index_paths(cx);
                    }
                    Err(e) => {
                        this.older = None;
                        this.error = Some(format!("{e:#}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Reads a page more of history once this frame is drawn, unless one is on its way.
    pub(super) fn want_older_commits(&self, cx: &mut Context<Self>) {
        if self.older.as_ref().is_none_or(|o| o.task.is_some()) {
            return;
        }
        let this = cx.entity().downgrade();
        cx.defer(move |cx| {
            this.update(cx, |this, cx| this.load_older_commits(cx)).ok();
        });
    }

    /// Filters `commits` by the field's text; the selection stays where it is.
    pub(super) fn refilter(&mut self) {
        let text = self.find.text.clone().unwrap_or_default();
        self.find.query = Query::parse(&text, now());
        self.find.shown = (!self.find.query.is_empty()).then(|| {
            self.commits
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    let paths = self.find.paths.get(&c.id).map(Vec::as_slice);
                    // A path condition cannot be judged before the paths are known.
                    self.find.query.matches(c, paths)
                })
                .map(|(ix, _)| ix)
                .collect()
        });
    }

    pub(super) fn start_find(&mut self, cx: &mut Context<Self>) {
        if self.find.text.is_none() {
            self.reset_field_edit();
        }
        self.settings_open = false;
        if self.find.text.is_none() {
            self.find.text = Some(String::new());
        }
        self.take_keyboard(Input::CommitSearch);
        cx.notify();
    }

    pub(super) fn close_find(&mut self, cx: &mut Context<Self>) {
        self.find.text = None;
        self.refilter();
        if let Selection::Commit(ix) = self.selection
            && let Some(row) = self.row_of(ix)
        {
            self.commit_scroll
                .scroll_to_item(row, gpui::ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn set_find_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.find.text = Some(text);
        self.refilter();
        if let Selection::Commit(ix) = self.selection
            && let Some(row) = self.row_of(ix)
        {
            self.commit_scroll
                .scroll_to_item(row, gpui::ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    /// The rows of the open diff that hold the find text, and where each change begins.
    pub(super) fn recompute_matches(&mut self) {
        self.matches = match &self.data {
            Some(data) => rows::matching_rows(data, &self.rows, &self.dterms),
            None => Vec::new(),
        };
        self.match_at = self.match_at.min(self.matches.len().saturating_sub(1));
        let mut changes = Vec::new();
        let mut inside = false;
        for (ix, row) in self.rows.iter().enumerate() {
            match row {
                Row::Thread(_) | Row::Request(_) | Row::Draft(_) | Row::Note(_) | Row::Composer => {
                    continue;
                }
                row if rows::is_change(row) => {
                    if !inside {
                        changes.push(ix);
                    }
                    inside = true;
                }
                _ => inside = false,
            }
        }
        self.changes = changes;
        self.change_at = self.change_at.filter(|&at| at < self.changes.len());
        self.recompute_stack_matches();
    }

    /// Scrolls the diff so `row` sits a few lines below the top.
    pub(super) fn scroll_to_row(&mut self, row: usize) {
        self.open_active();
        let row = row + self.list_base();
        self.diff_list.scroll_to(gpui::ListOffset {
            item_ix: row.saturating_sub(3),
            offset_in_item: px(0.),
        });
    }

    /// Brings `rows` into view when they are not, the first a third of the view from the top; rows
    /// already in view stay where they are. The view is counted in code rows: right after a
    /// relayout the list has not measured what lies below, so `scroll_to_reveal_item` cannot reach
    /// it.
    pub(super) fn reveal_rows(&mut self, rows: std::ops::Range<usize>) {
        self.open_active();
        let base = self.list_base();
        let rows = rows.start + base..rows.end + base;
        let height = f32::from(self.diff_list.viewport_bounds().size.height);
        let fit = (height / f32::from(px(self.row_height()))).floor() as usize;
        let top = self.diff_list.logical_scroll_top().item_ix;
        // The top row may be cut, and threads are taller than a code row: keep a margin.
        if fit > 6 && rows.start > top && rows.end + 3 <= top + fit {
            return;
        }
        self.diff_list.scroll_to(gpui::ListOffset {
            item_ix: rows.start.saturating_sub(fit / 3),
            offset_in_item: px(0.),
        });
    }

    /// Steps to the next (or previous) matching line.
    pub(super) fn step_match(&mut self, forward: bool, cx: &mut Context<Self>) {
        if let Some(stack) = &self.stack {
            let n = stack.matches.len();
            if n > 0 {
                let at = if forward {
                    (stack.match_at + 1) % n
                } else {
                    (stack.match_at + n - 1) % n
                };
                self.go_to_stack_match(at, cx);
            }
            return;
        }
        if self.matches.is_empty() {
            return;
        }
        let n = self.matches.len();
        self.match_at = if forward {
            (self.match_at + 1) % n
        } else {
            (self.match_at + n - 1) % n
        };
        self.scroll_to_row(self.matches[self.match_at]);
        cx.notify();
    }

    /// F7 / ⇧F7: the next (or previous) change in the open file.
    pub(super) fn step_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        // A document read as a page: its changed blocks (#11).
        if self.step_page(forward, cx) {
            return;
        }
        let n = self.changes.len();
        if n == 0 {
            return;
        }
        let at = match (self.change_at, forward) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(at), true) => (at + 1) % n,
            (Some(at), false) => (at + n - 1) % n,
        };
        self.change_at = Some(at);
        self.scroll_to_row(self.changes[at]);
        cx.notify();
    }

    /// A key: to the input with the keyboard (#168), else the keys of what is on screen.
    pub(super) fn find_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_keyboard();
        match self.typing() {
            Some(Input::FindFiles) => return self.ffind_key(event, cx),
            Some(Input::Palette) => return self.palette_key(event, window, cx),
            Some(Input::OpenAsk) => return self.open_ask_key(event, cx),
            Some(Input::AgentPick) => return self.agent_pick_key(event, cx),
            Some(Input::UpdateCard) => return self.update_card_key(event, cx),
            Some(Input::Beside) => return self.beside_key(event, cx),
            Some(Input::Reactions) => return self.reaction_key(event, cx),
            Some(Input::Submit) => return self.submit_key(event, cx),
            Some(Input::NewRequest) => return self.newreq_key(event, cx),
            Some(Input::GoLine) => return self.goline_key(event, cx),
            Some(Input::Report) => return self.report_key(event, cx),
            Some(Input::Compose) => return self.compose_key(event, cx),
            Some(Input::Field) => {
                self.filter_key(event, cx);
                return;
            }
            Some(Input::DiffFind) => return self.dfind_key(event, cx),
            Some(Input::CommitSearch) => return self.search_key(event, cx),
            None => {}
        }
        let key = &event.keystroke;
        // ⌫ on the draft picked in the Agents window deletes it; Undo is in its toast (#76).
        if key.key == "backspace"
            && self.tool() == Some(super::tools::Tool::Agents)
            && let Some(id) = self.agent_row.filter(|id| {
                self.comments
                    .iter()
                    .any(|c| c.id == *id && c.state == crate::review::State::Draft)
            })
        {
            return self.delete_draft(id, cx);
        }
        // Esc closes the innermost thing — a card, menu or popover, the find bar, a filter, the
        // tool window with the keyboard — then goes one page up (#71).
        if key.key == "escape" {
            if self.done_card {
                self.done_card = false;
                cx.notify();
            } else if self.label_edit.is_some() {
                self.close_label(cx);
            } else if self.update_card.is_some() {
                self.close_update_card(cx);
            } else if self.shortcuts {
                self.shortcuts = false;
                cx.notify();
            } else if self.ctx_menu.is_some() {
                self.close_menu(cx);
            } else if self.goline.is_some() {
                self.close_goline();
                cx.notify();
            } else if self.dfind.is_some() {
                self.close_dfind(cx);
            } else if self.find.text.is_some() {
                self.close_find(cx);
            } else if self.side_drawer() {
                self.close_drawer(cx);
            } else if !self.nav.picked.is_empty() {
                self.nav.picked.clear();
                cx.notify();
            } else if self.settings_open && self.theme_edit_escape(cx) {
            } else if self.settings_open {
                self.settings_open = false;
                cx.notify();
            } else if self.note_sel.take().is_some() || self.sel.take().is_some() {
                cx.notify();
            } else if self.tool().is_some() && self.zone == super::zones::Zone::Tool {
                self.set_tool(None, cx);
            } else if self.clear_filter() {
                cx.notify();
            } else if self.unpick_compare(cx) {
            } else {
                self.go_up(cx);
            }
            return;
        }
        // 0…9 answer the agent's form in view (#10 C).
        if key.modifiers.number_of_modifiers() == 0
            && let Some(digit) = key.key.parse::<usize>().ok().filter(|d| *d < 10)
        {
            self.answer_by_key(digit, cx);
        }
    }

    /// Esc after esc: the filter that was left with text, of the list on screen, is cleared.
    fn clear_filter(&mut self) -> bool {
        if !self.pfilter.is_empty()
            && self.diff.is_some()
            && self
                .space()
                .is_some_and(|s| s != super::spaces::Space::Code)
        {
            self.pfilter.clear();
            return true;
        }
        if !self.bfilter.is_empty() && self.at_root_list() {
            self.bfilter.clear();
            return true;
        }
        false
    }

    /// Typing into the find bar over the diff; esc lets the keyboard go, what was typed kept.
    fn dfind_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let Some(mut text) = self.dfind.clone() else {
            return;
        };
        // ⌥R: the text as a regular expression, or as typed again (#113).
        if key.modifiers.alt && key.key == "r" {
            return self.toggle_dfind_regex(cx);
        }
        match input::edit(&mut text, &mut self.field_edit, key, cx) {
            Edit::Changed => self.set_dfind(text, cx),
            Edit::Selected => cx.notify(),
            Edit::Enter { shift } => self.step_match(!shift, cx),
            Edit::Escape => self.leave_input(cx),
            Edit::Ignored => {}
        }
    }

    /// Typing into the commits' search; ↵ opens the first commit found.
    fn search_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(mut text) = self.find.text.clone() else {
            return;
        };
        match input::edit(&mut text, &mut self.field_edit, &event.keystroke, cx) {
            Edit::Changed => self.set_find_text(text, cx),
            Edit::Selected => cx.notify(),
            Edit::Enter { .. } => {
                if self.commit_rows() > 0 {
                    self.select_commit(self.commit_at(0), cx);
                }
            }
            Edit::Escape => self.leave_input(cx),
            Edit::Ignored => {}
        }
    }

    // ---- find in the diff ----------------------------------------------------------------

    /// ⌘F: a find bar over the open diff, like the page find of a browser or an IDE.
    pub(super) fn start_dfind(&mut self, cx: &mut Context<Self>) {
        if self.dfind.is_none() {
            self.reset_field_edit();
        }
        self.settings_open = false;
        if self.dfind.is_none() {
            self.dfind = Some(String::new());
        }
        self.take_keyboard(Input::DiffFind);
        cx.notify();
    }

    /// `.*` in ⌘F's bar: its text read as a regular expression, or as typed.
    pub(super) fn toggle_dfind_regex(&mut self, cx: &mut Context<Self>) {
        self.dfind_regex = !self.dfind_regex;
        let text = self.dfind.clone().unwrap_or_default();
        self.set_dfind(text, cx);
    }

    pub(super) fn close_dfind(&mut self, cx: &mut Context<Self>) {
        self.dfind = None;
        self.dterms = Arc::default();
        self.recompute_matches();
        cx.notify();
    }

    pub(super) fn set_dfind(&mut self, text: String, cx: &mut Context<Self>) {
        let terms = if self.dfind_regex {
            search::Terms::regex(&text)
        } else {
            Ok(search::Terms::text(&text))
        };
        self.dfind_error = terms.as_ref().err().cloned();
        self.dterms = Arc::new(terms.unwrap_or_default());
        self.dfind = Some(text);
        self.match_at = 0;
        self.recompute_matches();
        if self.stack.is_some() {
            // The folded files are built to be searched too.
            self.build_stack(cx);
            if let Some(at) = self.first_stack_match() {
                self.go_to_stack_match(at, cx);
            }
        } else if let Some(&row) = self.matches.first() {
            self.scroll_to_row(row);
        }
        cx.notify();
    }

    /// The find bar above the diff: the text, "2 of 4 lines", previous, next, close.
    pub(super) fn render_dfind(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let text = self.dfind.clone()?;
        let active = self.has_keyboard(Input::DiffFind);
        let count = if let Some(error) = &self.dfind_error {
            format!("Not a regular expression: {error}")
        } else if self.dterms.is_empty() {
            String::new()
        } else if let Some(count) = self.stack_match_count() {
            count
        } else if self.matches.is_empty() {
            "No matches".to_owned()
        } else {
            format!("{} of {} lines", self.match_at + 1, self.matches.len())
        };
        let step = |id: &'static str, label: &'static str, forward: bool| {
            chip(id, label, false).on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| this.step_match(forward, cx)),
            )
        };
        let bar = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px(px(10.))
            .py(px(6.))
            .border_b_1()
            .border_color(theme::island_border())
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(26.))
                    .px_2()
                    .rounded(px(super::ROW_RADIUS))
                    .border_2()
                    .border_color(if active {
                        theme::focus()
                    } else {
                        theme::island_border()
                    })
                    .bg(theme::base())
                    .cursor_text()
                    .child(
                        super::icons::icon("search")
                            .size(px(14.))
                            .text_color(theme::faint()),
                    )
                    .child(self.field_text(&text, active, "Find in this diff", cx))
                    .child({
                        use super::hover::Hover;
                        chip("dfind-regex", ".*", self.dfind_regex)
                            .tip("Regular expression  ⌥R")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.toggle_dfind_regex(cx)
                            }))
                    }),
            )
            .child(
                super::hover::cut("find-count", count.clone())
                    .text_size(px(12.))
                    .text_color(if count == "No matches" || self.dfind_error.is_some() {
                        theme::removed()
                    } else {
                        theme::muted()
                    }),
            )
            .child(step("dfind-prev", "↑", false))
            .child(step("dfind-next", "↓", true))
            .child(
                chip("dfind-close", "✕", false)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_dfind(cx))),
            );
        Some(typing::input_box(bar, Input::DiffFind, cx))
    }

    /// Reads the paths of every loaded commit, a chunk at a time; the filter updates as they arrive.
    pub(super) fn index_paths(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let ids: Vec<Oid> = self
            .commits
            .iter()
            .map(|c| c.id)
            .filter(|id| !self.find.paths.contains_key(id))
            .collect();
        if ids.is_empty() {
            self.find.indexing = false;
            return;
        }
        self.find.indexing = true;
        self.find.task = Some(cx.spawn(async move |this, cx| {
            for chunk in ids.chunks(PATH_CHUNK) {
                let chunk = chunk.to_vec();
                let root = root.clone();
                let read = cx
                    .background_executor()
                    .spawn(async move {
                        Repo::open(&root)
                            .map(|repo| repo.changed_paths(&chunk))
                            .unwrap_or_default()
                    })
                    .await;
                let alive = this.update(cx, |this, cx| {
                    this.find.paths.extend(read);
                    if this.find.query.needs_paths() || this.find.text.is_some() {
                        this.refilter();
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            this.update(cx, |this, cx| {
                this.find.indexing = false;
                cx.notify();
            })
            .ok();
        }));
    }

    /// The search field, its qualifier tags and the count line.
    pub(super) fn render_find(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let active = self.has_keyboard(Input::CommitSearch);
        let text = self.find.text.clone().unwrap_or_default();
        let field = div()
            .id("find")
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .mx(px(6.))
            .mb(px(6.))
            .px_2()
            .h(px(28.))
            .rounded(px(super::ROW_RADIUS))
            .border_2()
            .border_color(if active {
                theme::focus()
            } else {
                theme::island_border()
            })
            .bg(theme::base())
            .cursor_text()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.start_find(cx)))
            .child(
                super::icons::icon("search")
                    .size(px(14.))
                    .text_color(theme::faint()),
            )
            .child(self.field_text(&text, active, "Search commits", cx))
            .child(div().text_size(px(11.)).text_color(theme::faint()).child(
                super::keys::text(if text.is_empty() { "⌘⇧F" } else { "esc" }).into_owned(),
            ));
        let tags = active.then(|| {
            div()
                .flex_none()
                .flex()
                .gap_1()
                .px(px(8.))
                .pb(px(6.))
                .children(QUALIFIERS.into_iter().map(|q| {
                    div()
                        .id(q)
                        .px(px(6.))
                        .rounded_full()
                        .border_1()
                        .border_color(theme::faint())
                        .text_size(px(10.))
                        .text_color(theme::muted())
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme::text()))
                        .child(q)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            let mut text = this.find.text.clone().unwrap_or_default();
                            if !text.is_empty() && !text.ends_with(' ') {
                                text.push(' ');
                            }
                            text.push_str(q);
                            this.set_find_text(text, cx);
                        }))
                }))
        });
        let count = self.find.shown.as_ref().map(|shown| {
            let mut line = format!("{} of {} commits", shown.len(), self.commits.len());
            // Older history still being read for it (#25).
            if self.older.is_some() {
                line.push_str(" · reading older ones…");
            } else if self.find.indexing {
                line.push_str(" · reading paths…");
            }
            div()
                .flex_none()
                .px(px(14.))
                .pb(px(4.))
                .text_size(px(11.))
                .text_color(theme::faint())
                .child(line)
        });
        let el = div()
            .flex()
            .flex_col()
            .child(field)
            .children(tags)
            .children(count);
        typing::input_box(el, Input::CommitSearch, cx)
    }

    /// The summary of commit `ix`, with the search words highlighted.
    pub(super) fn summary_text(&self, summary: &SharedString) -> StyledText {
        let ranges = search::highlights(summary, self.find.query.words());
        StyledText::new(summary.clone()).with_highlights(ranges.into_iter().map(|range| {
            (
                range,
                HighlightStyle {
                    color: Some(theme::warning().into()),
                    background_color: Some(theme::warning_bg().into()),
                    ..Default::default()
                },
            )
        }))
    }

    /// Shown instead of the list when nothing matches.
    pub(super) fn render_no_match(&self) -> impl IntoElement + use<> {
        let text = self.find.text.clone().unwrap_or_default();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .px_4()
            .text_color(theme::muted())
            .child(format!("No commit matches “{text}”."))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(match self.older {
                        Some(_) => format!(
                            "Searched the {} newest commits; reading older ones…",
                            self.commits.len()
                        ),
                        None => format!("Searched all {} commits.", self.commits.len()),
                    }),
            )
    }
}

use super::Selection;
