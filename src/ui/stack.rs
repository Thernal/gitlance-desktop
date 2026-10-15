//! Every file of a diff in one scroll (Design `docs/specs/all-files.md`): the diff's list holds a
//! block per file, a header row and the file's rows under it.
//!
//! One file is *active* — the one in view, or the one last clicked. Its data, rows, opened runs and
//! comment marks are `Workspace::data`, `rows`, `expanded` and `notes`, numbered from its first
//! row, so everything that works on one file (find, F7, `c`, selections, annotate, bookmarks)
//! works on it unchanged; `list_base` turns its row numbers into the list's. The other blocks keep
//! theirs here and are drawn as they are until a click or the scroll makes them active.

use super::diff_view::LineNote;
use super::reviewed::State;
use super::rows::{self, FileData, Row};
use super::{ROW_RADIUS, Workspace, px, theme};
use crate::git::FileDiff;
use gpui::{
    ClickEvent, Context, FontWeight, ListOffset, MouseButton, Task, WeakEntity, div, prelude::*,
};
use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

pub(super) struct Block {
    pub file: usize,
    pub data: Option<Arc<FileData>>,
    pub rows: Vec<Row>,
    pub expanded: HashSet<usize>,
    pub notes: Vec<LineNote>,
    /// The rows the list holds under the header.
    len: usize,
}

#[derive(Default)]
pub(super) struct Stack {
    pub blocks: Vec<Block>,
    /// The block of each file, by file index.
    of_file: Vec<Option<usize>>,
    /// The list index of each block's header; the list's length last.
    starts: Vec<usize>,
    /// Files folded or opened against the default (a reviewed file starts folded).
    flipped: HashSet<usize>,
    building: bool,
    task: Option<Task<()>>,
    /// Where the view goes once the active file is laid out: rows below its header.
    anchor: Option<ListOffset>,
}

impl Stack {
    fn new(order: &[usize], files: usize) -> Self {
        let mut of_file = vec![None; files];
        let blocks = order
            .iter()
            .enumerate()
            .map(|(b, &file)| {
                of_file[file] = Some(b);
                Block {
                    file,
                    data: None,
                    rows: Vec::new(),
                    expanded: HashSet::new(),
                    notes: Vec::new(),
                    len: 0,
                }
            })
            .collect();
        let mut stack = Self {
            blocks,
            of_file,
            ..Default::default()
        };
        stack.reindex();
        stack
    }

    fn reindex(&mut self) {
        self.starts.clear();
        let mut at = 0;
        for block in &self.blocks {
            self.starts.push(at);
            at += 1 + block.len;
        }
        self.starts.push(at);
    }

    /// The list's length.
    pub fn total(&self) -> usize {
        self.starts.last().copied().unwrap_or(0)
    }

    /// The block of list row `ix`, and the row's place in it (0 is the header).
    pub fn locate(&self, ix: usize) -> (usize, usize) {
        let b = self
            .starts
            .partition_point(|&s| s <= ix)
            .saturating_sub(1)
            .min(self.blocks.len().saturating_sub(1));
        (b, ix.saturating_sub(self.starts[b]))
    }

    pub fn block_of(&self, file: usize) -> Option<usize> {
        self.of_file.get(file).copied().flatten()
    }

    /// The list index of file `file`'s header.
    pub fn header(&self, file: usize) -> Option<usize> {
        Some(self.starts[self.block_of(file)?])
    }

    /// The list rows of block `b`, header included.
    fn span(&self, b: usize) -> Range<usize> {
        self.starts[b]..self.starts[b + 1]
    }
}

/// What a file shows under its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Body {
    /// Nothing: the header alone.
    Folded,
    /// Its rows are being built.
    Waiting,
    /// One line about it: generated, an image, no content changes.
    Note,
    Rows,
}

/// The part of `old` that `new` replaces: where it starts, how many rows go and how many come.
/// Rows equal at both ends stay, so the list keeps their heights and the scroll stays put.
fn changed_span(old: &[Row], new: &[Row]) -> Option<(usize, usize, usize)> {
    let head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (gone, come) = (old.len() - head - tail, new.len() - head - tail);
    (gone > 0 || come > 0).then_some((head, gone, come))
}

impl Workspace {
    /// Every file in one scroll: the option is on and the diff has more than one file.
    pub(super) fn wants_stack(&self) -> bool {
        !self.options.one_file && self.diff.as_ref().is_some_and(|d| d.files.len() > 1)
    }

    /// The list index of the active file's first row.
    pub(super) fn list_base(&self) -> usize {
        self.stack
            .as_ref()
            .and_then(|s| s.header(self.file))
            .map_or(0, |h| h + 1)
    }

    /// The active file's row at the top of the view (its first while the view is above it).
    pub(super) fn local_top(&self) -> usize {
        self.diff_list
            .logical_scroll_top()
            .item_ix
            .saturating_sub(self.list_base())
    }

    fn needs_rows(&self, file: &FileDiff) -> bool {
        !file.hunks.is_empty() && !self.generated_folded(file) && !self.shows_image(file)
    }

    /// File `ix` shows its header alone: it is reviewed, unless the reader opened it (or the other
    /// way round).
    pub(super) fn folded(&self, ix: usize) -> bool {
        let reviewed = self.review_state(ix) == Some(State::Done);
        reviewed != self.stack.as_ref().is_some_and(|s| s.flipped.contains(&ix))
    }

    pub(super) fn body(&self, b: usize) -> Body {
        let (Some(stack), Some(diff)) = (&self.stack, &self.diff) else {
            return Body::Note;
        };
        let block = &stack.blocks[b];
        let Some(file) = diff.files.get(block.file) else {
            return Body::Note;
        };
        let (built, rows) = if block.file == self.file {
            (self.data.is_some(), self.rows.len())
        } else {
            (block.data.is_some(), block.rows.len())
        };
        if self.folded(block.file) {
            Body::Folded
        } else if !self.needs_rows(file) {
            Body::Note
        } else if !built {
            Body::Waiting
        } else if rows == 0 {
            Body::Note
        } else {
            Body::Rows
        }
    }

    fn body_len(&self, b: usize) -> usize {
        match self.body(b) {
            Body::Folded => 0,
            Body::Waiting | Body::Note => 1,
            Body::Rows
                if self
                    .stack
                    .as_ref()
                    .is_some_and(|s| s.blocks[b].file == self.file) =>
            {
                self.rows.len()
            }
            Body::Rows => self.stack.as_ref().map_or(0, |s| s.blocks[b].rows.len()),
        }
    }

    /// Block `b`'s rows as the list holds them, when it holds rows.
    pub(super) fn shown_rows(&self, b: usize) -> Option<Vec<Row>> {
        let stack = self.stack.as_ref()?;
        (self.body(b) == Body::Rows).then(|| {
            if stack.blocks[b].file == self.file {
                self.rows.clone()
            } else {
                stack.blocks[b].rows.clone()
            }
        })
    }

    /// Tells the list that block `b` changed from `before` (its rows, when it showed rows): only
    /// the rows that differ are replaced.
    pub(super) fn stack_changed(&mut self, b: usize, before: Option<Vec<Row>>) {
        let after = self.shown_rows(b);
        let len = self.body_len(b);
        let Some(stack) = self.stack.as_mut() else {
            return;
        };
        let at = stack.starts[b] + 1;
        match (before, after) {
            (Some(old), Some(new)) => {
                if let Some((head, gone, come)) = changed_span(&old, &new) {
                    self.diff_list.splice(at + head..at + head + gone, come);
                }
            }
            _ => {
                let old = stack.blocks[b].len;
                self.diff_list.splice(at..at + old, len);
            }
        }
        stack.blocks[b].len = len;
        stack.reindex();
    }

    /// Brings every block's length in the list in line with what it shows now (after files were
    /// marked reviewed, folded or opened).
    pub(super) fn sync_stack(&mut self) {
        let Some(n) = self.stack.as_ref().map(|s| s.blocks.len()) else {
            return;
        };
        let lens: Vec<usize> = (0..n).map(|b| self.body_len(b)).collect();
        let Some(stack) = self.stack.as_mut() else {
            return;
        };
        // From the last block up, so the list indexes of the blocks above stay right.
        for b in (0..n).rev() {
            let old = stack.blocks[b].len;
            if old != lens[b] {
                let at = stack.starts[b] + 1;
                self.diff_list.splice(at..at + old, lens[b]);
                stack.blocks[b].len = lens[b];
            }
        }
        stack.reindex();
    }

    /// Puts the files of the diff in one list and shows file `ix` at the top.
    fn open_stack(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(diff) = &self.diff else {
            return;
        };
        let order: Vec<usize> =
            super::lists::file_items(&diff.files, self.options.list_files, "", &HashSet::new())
                .into_iter()
                .filter_map(|item| match item {
                    super::lists::FileItem::File { ix, .. } => Some(ix),
                    super::lists::FileItem::Dir { .. } => None,
                })
                .collect();
        let files = diff.files.len();
        // The place a reload keeps: rows below the header of the file it was in.
        let anchor = self
            .restore_scroll
            .take()
            .filter(|(path, _)| diff.files.get(ix).is_some_and(|f| f.path() == path))
            .map(|(_, offset)| offset);
        self.clear_file();
        self.file = ix;
        let mut stack = Stack::new(&order, files);
        stack.anchor = anchor;
        self.stack = Some(stack);
        let n = order.len();
        let lens: Vec<usize> = (0..n).map(|b| self.body_len(b)).collect();
        if let Some(stack) = self.stack.as_mut() {
            for (block, len) in stack.blocks.iter_mut().zip(lens) {
                block.len = len;
            }
            stack.reindex();
            self.diff_list.reset(stack.total());
        }
        self.scroll_to_header(ix);
        self.build_stack(cx);
    }

    /// Builds the stack again (a new diff mode), keeping the place.
    pub(super) fn reopen_stack(&mut self, cx: &mut Context<Self>) {
        self.restore_scroll = self.scroll_place();
        let ix = self
            .restore_scroll
            .as_ref()
            .and_then(|(path, _)| {
                self.diff
                    .as_ref()?
                    .files
                    .iter()
                    .position(|f| f.path() == path)
            })
            .unwrap_or(self.file);
        self.open_stack(ix, cx);
    }

    /// The place the view is at: the file at the top and the rows below its header (the open
    /// file and the list's offset when files are shown one at a time).
    pub(super) fn scroll_place(&self) -> Option<(String, ListOffset)> {
        let diff = self.diff.as_ref()?;
        let top = self.diff_list.logical_scroll_top();
        let Some(stack) = &self.stack else {
            return Some((diff.files.get(self.file)?.path().to_owned(), top));
        };
        let (b, inside) = stack.locate(top.item_ix);
        let file = diff.files.get(stack.blocks.get(b)?.file)?;
        Some((
            file.path().to_owned(),
            // Counted from the file's first row, as when files are shown one at a time.
            ListOffset {
                item_ix: inside.saturating_sub(1),
                offset_in_item: top.offset_in_item,
            },
        ))
    }

    fn scroll_to_header(&mut self, ix: usize) {
        if let Some(at) = self.stack.as_ref().and_then(|s| s.header(ix)) {
            self.diff_list.scroll_to(ListOffset {
                item_ix: at,
                offset_in_item: px(0.),
            });
        }
    }

    /// Picking file `ix` (the files island, ← →, `v`, a link): it becomes active, opens when it
    /// was folded, and its header comes to the top.
    pub(super) fn stack_select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.stack.is_none() {
            return self.open_stack(ix, cx);
        }
        self.activate_file(ix, cx);
        if self.folded(ix) {
            self.flip_fold(ix, cx);
        }
        self.scroll_to_header(ix);
        if self.data.is_some() {
            self.apply_jump(cx);
        }
        self.build_stack(cx);
        cx.notify();
    }

    /// Makes file `ix` the one the keyboard and the header above the diff work on.
    pub(super) fn activate_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.file == ix || self.stack.is_none() {
            return;
        }
        // The prompt of `c` belongs to the file it was opened in.
        if self.goline.take().is_some() {
            self.goline_row = None;
            self.relayout();
        }
        let Some(stack) = self.stack.as_mut() else {
            return;
        };
        let (Some(from), Some(to)) = (stack.block_of(self.file), stack.block_of(ix)) else {
            return;
        };
        let block = &mut stack.blocks[from];
        block.data = self.data.take();
        block.rows = std::mem::take(&mut self.rows);
        block.expanded = std::mem::take(&mut self.expanded);
        block.notes = std::mem::take(&mut self.notes);
        let block = &mut stack.blocks[to];
        self.data = block.data.take();
        self.rows = std::mem::take(&mut block.rows);
        self.expanded = std::mem::take(&mut block.expanded);
        self.notes = std::mem::take(&mut block.notes);
        self.file = ix;
        self.fcursor = None;
        self.sel = None;
        self.change_at = None;
        self.match_at = 0;
        self.recompute_matches();
        self.refresh_annotations(cx);
        self.scroll_file_into_view(ix);
        cx.notify();
    }

    /// Folds file `ix` to its header, or opens it.
    pub(super) fn flip_fold(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(stack) = self.stack.as_mut() else {
            return;
        };
        if !stack.flipped.remove(&ix) {
            stack.flipped.insert(ix);
        }
        self.sync_stack();
        self.build_stack(cx);
        cx.notify();
    }

    /// Files marked or unmarked reviewed fold or open as their mark says again.
    pub(super) fn review_changed(&mut self, ixs: &[usize]) {
        let Some(stack) = self.stack.as_mut() else {
            return;
        };
        for ix in ixs {
            stack.flipped.remove(ix);
        }
        self.sync_stack();
    }

    /// The scroll moved the active file out of view: the file at the top takes over. With
    /// Settings → Diff → *Mark reviewed as I scroll past*, a file left above while scrolling down
    /// is marked reviewed.
    pub(super) fn follow_scroll(&mut self, cx: &mut Context<Self>) {
        if self.compose.is_some() || self.goline.is_some() || self.selecting {
            return;
        }
        let Some(stack) = &self.stack else {
            return;
        };
        let Some(b) = stack.block_of(self.file) else {
            return;
        };
        let span = stack.span(b);
        let above = self.diff_list.item_is_above_viewport(span.end - 1) == Some(true);
        let below = !above && self.diff_list.item_is_below_viewport(span.start) == Some(true);
        if !above && !below {
            return;
        }
        let (top, _) = stack.locate(self.diff_list.logical_scroll_top().item_ix);
        let to = stack.blocks[top].file;
        if to == self.file {
            return;
        }
        // Every file from the one left to the one now at the top was scrolled past, also those a
        // page skipped over.
        let passed: Vec<usize> = if above && self.settings.auto_reviewed {
            stack.blocks[b..top]
                .iter()
                .map(|block| block.file)
                .collect()
        } else {
            Vec::new()
        };
        self.activate_file(to, cx);
        let passed: Vec<usize> = passed
            .into_iter()
            .filter(|&ix| self.review_state(ix) == Some(State::Open))
            .collect();
        if !passed.is_empty() {
            self.set_reviewed_many(&passed, true, cx);
        }
    }

    /// The widest line of the files built, for the horizontal scroll.
    pub(super) fn widest(&self) -> Option<usize> {
        let mine = self.data.as_ref().map(|d| d.widest);
        let others = self
            .stack
            .iter()
            .flat_map(|s| &s.blocks)
            .filter_map(|b| b.data.as_ref().map(|d| d.widest));
        mine.into_iter().chain(others).max()
    }

    // ---- building -----------------------------------------------------------------------------

    /// Builds the files' rows in the background, one at a time: the active file first, then the
    /// files nearest to it.
    pub(super) fn build_stack(&mut self, cx: &mut Context<Self>) {
        let Some(stack) = self.stack.as_mut() else {
            return;
        };
        if stack.building {
            return;
        }
        stack.building = true;
        stack.task = Some(cx.spawn(async move |this, cx| {
            loop {
                let next = this
                    .update(cx, |this, _| {
                        let next = this.next_to_build();
                        if next.is_none()
                            && let Some(stack) = this.stack.as_mut()
                        {
                            stack.building = false;
                        }
                        next
                    })
                    .ok()
                    .flatten();
                let Some((ix, file, mode)) = next else {
                    break;
                };
                let data = cx
                    .background_executor()
                    .spawn(async move { FileData::build(&file, mode) })
                    .await;
                if this
                    .update(cx, |this, cx| this.stack_built(ix, data, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn next_to_build(&self) -> Option<(usize, FileDiff, crate::storage::DiffMode)> {
        let (stack, diff) = (self.stack.as_ref()?, self.diff.as_ref()?);
        let here = stack.block_of(self.file).unwrap_or(0);
        let n = stack.blocks.len();
        let near =
            (0..n).flat_map(|d| [here.checked_add(d), here.checked_sub(d).filter(|_| d > 0)]);
        near.flatten().filter(|&b| b < n).find_map(|b| {
            let ix = stack.blocks[b].file;
            let built = if ix == self.file {
                self.data.is_some()
            } else {
                stack.blocks[b].data.is_some()
            };
            let file = diff.files.get(ix)?;
            (!built && !self.folded(ix) && self.needs_rows(file))
                .then(|| (ix, file.clone(), self.options.mode))
        })
    }

    fn stack_built(&mut self, ix: usize, data: FileData, cx: &mut Context<Self>) {
        let data = Arc::new(data);
        if ix == self.file {
            self.data = Some(data);
            self.refresh_annotations(cx);
            self.relayout();
            if let Some(anchor) = self.stack.as_mut().and_then(|s| s.anchor.take())
                && let Some(header) = self.stack.as_ref().and_then(|s| s.header(ix))
            {
                self.diff_list.scroll_to(ListOffset {
                    item_ix: header + 1 + anchor.item_ix,
                    ..anchor
                });
            }
            self.apply_jump(cx);
        } else if let Some(b) = self.stack.as_ref().and_then(|s| s.block_of(ix)) {
            let Some(path) = self
                .diff
                .as_ref()
                .and_then(|d| d.files.get(ix))
                .map(|f| f.path().to_owned())
            else {
                return;
            };
            let before = self.shown_rows(b);
            self.lay_block(b, &path, data);
            self.stack_changed(b, before);
        }
        cx.notify();
    }

    /// Lays block `b` (not the active one) out with `data`.
    fn lay_block(&mut self, b: usize, path: &str, data: Arc<FileData>) {
        let Some(stack) = self.stack.as_ref() else {
            return;
        };
        let rows = rows::layout(&data, &self.view_options(), &stack.blocks[b].expanded);
        let rows = self.with_comments_in(path, &data, rows, false);
        let notes = self.line_notes_in(path, Some(&data));
        if let Some(block) = self.stack.as_mut().map(|s| &mut s.blocks[b]) {
            block.data = Some(data);
            block.rows = rows;
            block.notes = notes;
        }
    }

    /// Lays every file out again (an option changed, a comment came or went), changing in the list
    /// only the rows that differ.
    pub(super) fn relayout_stack(&mut self) {
        let Some(n) = self.stack.as_ref().map(|s| s.blocks.len()) else {
            return;
        };
        for b in 0..n {
            let Some((ix, data)) = self
                .stack
                .as_ref()
                .map(|s| (s.blocks[b].file, s.blocks[b].data.clone()))
            else {
                return;
            };
            if ix == self.file {
                self.relayout();
                continue;
            }
            let (Some(data), Some(path)) = (
                data,
                self.diff
                    .as_ref()
                    .and_then(|d| d.files.get(ix))
                    .map(|f| f.path().to_owned()),
            ) else {
                continue;
            };
            let before = self.shown_rows(b);
            self.lay_block(b, &path, data);
            self.stack_changed(b, before);
        }
        // Files that now fold, or no longer do (Settings → Fold generated files).
        self.sync_stack();
    }

    /// Opens the active file when it is folded: something in it is about to be shown.
    pub(super) fn open_active(&mut self) {
        if self.stack.is_some() && self.folded(self.file) {
            if let Some(stack) = self.stack.as_mut()
                && !stack.flipped.remove(&self.file)
            {
                stack.flipped.insert(self.file);
            }
            self.sync_stack();
        }
    }

    // ---- drawing ------------------------------------------------------------------------------

    /// A file's header in the list: a click folds or opens it, the box at the right marks it
    /// reviewed.
    pub(super) fn render_block_header(
        &self,
        ix: usize,
        this: WeakEntity<Self>,
    ) -> gpui::AnyElement {
        let Some(file) = self.diff.as_ref().and_then(|d| d.files.get(ix)) else {
            return div().into_any_element();
        };
        let path = match (&file.old_path, &file.new_path) {
            (Some(old), Some(new)) if old != new => format!("{old} → {new}"),
            _ => file.path().to_owned(),
        };
        let folded = self.folded(ix);
        let state = self.review_state(ix);
        let active = ix == self.file;
        let (fold, mark) = (this.clone(), this);
        div()
            .id(("file-header", ix))
            .w_full()
            .pt(px(6.))
            .child(
                div()
                    .mx(px(4.))
                    .h(px(32.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(ROW_RADIUS))
                    .border_1()
                    .border_color(if active {
                        theme::focus()
                    } else {
                        theme::island_border()
                    })
                    .bg(theme::panel())
                    .font_family(theme::UI_FONT)
                    .text_size(px(12.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(
                        div()
                            .w(px(10.))
                            .flex_none()
                            .text_color(theme::faint())
                            .child(if folded { "▸" } else { "▾" }),
                    )
                    .child(super::change_badge(file.change))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(if state == Some(State::Done) {
                                theme::muted()
                            } else {
                                theme::text()
                            })
                            .child(path),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_color(theme::added())
                                    .child(format!("+{}", file.added)),
                            )
                            .child(
                                div()
                                    .text_color(theme::removed())
                                    .child(format!("−{}", file.removed)),
                            ),
                    )
                    .children(state.map(|state| {
                        div()
                            .id(("file-header-mark", ix))
                            .flex_none()
                            .px_2()
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .rounded(px(ROW_RADIUS))
                            .text_color(match state {
                                State::Done => theme::added(),
                                State::Stale => theme::warning(),
                                State::Open => theme::muted(),
                            })
                            .font_weight(if state == State::Done {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .hover(|s| s.bg(theme::selected()).text_color(theme::text()))
                            .child(match state {
                                State::Done => "✓ Reviewed",
                                State::Stale => "↻ Changed — mark again",
                                State::Open => "Mark reviewed",
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(move |_: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                mark.update(cx, |this, cx| {
                                    let done = this.review_state(ix) == Some(State::Done);
                                    this.set_reviewed(ix, !done, cx);
                                })
                                .ok();
                            })
                    })),
            )
            .on_click(move |_: &ClickEvent, _, cx| {
                fold.update(cx, |this, cx| {
                    this.activate_file(ix, cx);
                    this.flip_fold(ix, cx);
                })
                .ok();
            })
            .into_any_element()
    }

    /// The one line under a header that has no rows to show (or none yet).
    pub(super) fn render_block_note(
        &self,
        b: usize,
        ix: usize,
        this: WeakEntity<Self>,
    ) -> gpui::AnyElement {
        let Some(file) = self.diff.as_ref().and_then(|d| d.files.get(ix)) else {
            return div().into_any_element();
        };
        let line = |text: String| {
            div()
                .w_full()
                .px(px(18.))
                .py(px(8.))
                .font_family(theme::UI_FONT)
                .text_size(px(12.))
                .text_color(theme::muted())
                .flex()
                .items_center()
                .gap_3()
                .child(text)
        };
        let action = |id: &'static str, label: &'static str| {
            div()
                .id((id, ix))
                .px_2()
                .h(px(22.))
                .flex()
                .items_center()
                .rounded(px(ROW_RADIUS))
                .bg(theme::hover())
                .text_color(theme::text())
                .cursor_pointer()
                .hover(|s| s.bg(theme::selected()))
                .child(label)
        };
        if self.body(b) == Body::Waiting {
            let rows = (file.added + file.removed + 6).min(14) as f32;
            return line("Loading…".to_owned())
                .h(px(self.row_height() * rows))
                .items_start()
                .text_color(theme::faint())
                .into_any_element();
        }
        if self.generated_folded(file) {
            let path = file.path().to_owned();
            return line(format!(
                "Generated: {} changed lines are folded.",
                file.added + file.removed
            ))
            .child(action("unfold-in-stack", "Show it anyway").on_click(
                move |_: &ClickEvent, _, cx| {
                    this.update(cx, |this, cx| this.unfold(&path, cx)).ok();
                },
            ))
            .into_any_element();
        }
        if self.shows_image(file) {
            return line("An image.".to_owned())
                .child(action("image-alone", "View it on its own").on_click(
                    move |_: &ClickEvent, _, cx| {
                        this.update(cx, |this, cx| this.view_alone(ix, cx)).ok();
                    },
                ))
                .into_any_element();
        }
        let text = if file.hunks.is_empty() {
            file.note
                .clone()
                .unwrap_or_else(|| "No content changes".to_owned())
        } else {
            "No changes".to_owned()
        };
        line(text).into_any_element()
    }

    /// Shows file `ix` alone (an image, which has no rows), until all files are asked for again.
    fn view_alone(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.set_option_unsaved(super::Opt::OneFile, true, cx);
        self.select_file(ix, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::LineKind;
    use crate::ui::rows::Cell;

    fn line(n: u32) -> Row {
        let cell = Cell {
            line: n,
            kind: LineKind::Context,
        };
        Row::Split {
            left: Some(cell),
            right: Some(cell),
        }
    }

    #[test]
    fn only_the_rows_that_differ_are_replaced() {
        let old = [
            line(1),
            Row::Gap {
                segment: 1,
                count: 9,
            },
            line(11),
        ];
        let new = [line(1), line(2), line(3), line(11)];
        assert_eq!(changed_span(&old, &new), Some((1, 1, 2)));
        assert_eq!(changed_span(&old, &old), None);
        // A thread put under a line.
        let new = [
            line(1),
            Row::Thread(4),
            Row::Gap {
                segment: 1,
                count: 9,
            },
            line(11),
        ];
        assert_eq!(changed_span(&old, &new), Some((1, 0, 1)));
    }

    #[test]
    fn a_row_is_found_in_its_block() {
        let mut stack = Stack::new(&[2, 0, 1], 3);
        stack.blocks[0].len = 4;
        stack.blocks[1].len = 0;
        stack.blocks[2].len = 2;
        stack.reindex();
        assert_eq!(stack.starts, vec![0, 5, 6, 9]);
        assert_eq!(stack.locate(0), (0, 0));
        assert_eq!(stack.locate(4), (0, 4));
        assert_eq!(stack.locate(5), (1, 0));
        assert_eq!(stack.locate(8), (2, 2));
        assert_eq!(stack.header(1), Some(6));
        assert_eq!(stack.block_of(2), Some(0));
    }
}
