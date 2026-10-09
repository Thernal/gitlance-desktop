//! Commit search: the field above the commit list, its keyboard handling, and the filtered view of
//! `Workspace::commits`. The matching itself is `crate::search`.

use super::diff_view::chip;
use super::input::{self, Edit};
use super::{COMMIT_LIMIT, Workspace, rows, theme};
use crate::git::Repo;
use crate::search::{self, Query};
use crate::ui::rows::Row;
use git2::Oid;
use gpui::{
    ClickEvent, Context, HighlightStyle, KeyDownEvent, SharedString, StyledText, Task, div,
    prelude::*, px,
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
        self.field_all = false;
        self.field = None;
        self.settings_open = false;
        self.dfind = None;
        self.dterms = Arc::default();
        self.recompute_matches();
        if self.find.text.is_none() {
            self.find.text = Some(String::new());
        }
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
                Row::Thread(_) | Row::Request(_) | Row::Draft(_) | Row::Composer => continue,
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
    }

    /// Scrolls the diff so `row` sits a few lines below the top.
    pub(super) fn scroll_to_row(&mut self, row: usize) {
        self.diff_list.scroll_to(gpui::ListOffset {
            item_ix: row.saturating_sub(3),
            offset_in_item: px(0.),
        });
    }

    /// Steps to the next (or previous) matching line.
    pub(super) fn step_match(&mut self, forward: bool, cx: &mut Context<Self>) {
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

    /// Typing into the open field: the diff find, else the commit search.
    pub(super) fn find_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.is_some() {
            return self.palette_key(event, window, cx);
        }
        if self.goline.is_some() {
            return self.goline_key(event, cx);
        }
        if self.compose.is_some() {
            return self.compose_key(event, cx);
        }
        let key = &event.keystroke;
        if key.key == "escape" {
            if self.ctx_menu.is_some() {
                self.close_menu(cx);
            } else if self.repo_menu {
                self.repo_menu = false;
                cx.notify();
            } else if self.field.is_some() {
                self.filter_key(event, cx);
            } else if self.dfind.is_some() {
                self.close_dfind(cx);
            } else if self.find.text.is_some() {
                self.close_find(cx);
            } else if self.settings_open {
                self.settings_open = false;
                cx.notify();
            } else if self.sel.take().is_some() {
                cx.notify();
            }
            return;
        }
        if self.field.is_some() {
            self.filter_key(event, cx);
            return;
        }
        if let Some(mut text) = self.dfind.clone() {
            match input::edit(&mut text, &mut self.field_all, key, cx) {
                Edit::Changed => self.set_dfind(text, cx),
                Edit::Selected => cx.notify(),
                Edit::Enter { shift } => self.step_match(!shift, cx),
                Edit::Escape | Edit::Ignored => {}
            }
            return;
        }
        if let Some(mut text) = self.find.text.clone() {
            match input::edit(&mut text, &mut self.field_all, key, cx) {
                Edit::Changed => self.set_find_text(text, cx),
                Edit::Selected => cx.notify(),
                Edit::Enter { .. } => {
                    if self.commit_rows() > 0 {
                        self.select_commit(self.commit_at(0), cx);
                    }
                }
                Edit::Escape | Edit::Ignored => {}
            }
        }
    }

    // ---- find in the diff ----------------------------------------------------------------

    /// ⌘F: a find bar over the open diff, like the page find of a browser or an IDE.
    pub(super) fn start_dfind(&mut self, cx: &mut Context<Self>) {
        self.field_all = false;
        self.field = None;
        self.settings_open = false;
        self.find.text = None;
        self.refilter();
        if self.dfind.is_none() {
            self.dfind = Some(String::new());
        }
        cx.notify();
    }

    pub(super) fn close_dfind(&mut self, cx: &mut Context<Self>) {
        self.dfind = None;
        self.dterms = Arc::default();
        self.recompute_matches();
        cx.notify();
    }

    pub(super) fn set_dfind(&mut self, text: String, cx: &mut Context<Self>) {
        self.dterms = if text.is_empty() {
            Arc::default()
        } else {
            Arc::from(vec![text.to_lowercase()])
        };
        self.dfind = Some(text);
        self.match_at = 0;
        self.recompute_matches();
        if let Some(&row) = self.matches.first() {
            self.scroll_to_row(row);
        }
        cx.notify();
    }

    /// The find bar above the diff: the text, "2 of 4 lines", previous, next, close.
    pub(super) fn render_dfind(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let text = self.dfind.clone()?;
        let count = if self.dterms.is_empty() {
            String::new()
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
        Some(
            div()
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
                        .border_color(theme::focus())
                        .bg(theme::base())
                        .child(div().text_color(theme::faint()).child("⌕"))
                        .child(input::field_text(
                            &text,
                            true,
                            self.field_all,
                            "Find in this diff",
                        )),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(if count == "No matches" {
                            theme::removed()
                        } else {
                            theme::muted()
                        })
                        .child(count),
                )
                .child(step("dfind-prev", "↑", false))
                .child(step("dfind-next", "↓", true))
                .child(
                    chip("dfind-close", "✕", false)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_dfind(cx))),
                ),
        )
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
        let active = self.find.text.is_some();
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
            .child(div().text_color(theme::faint()).child("⌕"))
            .child(input::field_text(
                &text,
                active,
                self.field_all,
                "Search commits",
            ))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(if text.is_empty() { "⌘⇧F" } else { "esc" }),
            );
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
            if self.find.indexing {
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
        div()
            .flex()
            .flex_col()
            .child(field)
            .children(tags)
            .children(count)
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
                    .child(format!(
                        "Searched the {} commits loaded (up to {COMMIT_LIMIT}).",
                        self.commits.len()
                    )),
            )
    }
}

use super::Selection;
