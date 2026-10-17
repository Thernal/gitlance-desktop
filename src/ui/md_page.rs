//! A changed markdown file read as a page (Design #11 A + B, `docs/specs/markdown-viewer.md`): its
//! card has Rendered | Source (M), Rendered by default, and the page follows the toolbar's Split |
//! Unified. Unified draws the new version with the changes in it — a bar in the margin for every
//! changed block, green added, yellow changed with its words marked, red removed (faded and struck;
//! a removed section folds to one line). Split draws the old version beside the new, block beside
//! block, a hatched gap where one side has none. Images come from the repository at each side's
//! version (`FileDiff::pictures`). A comment on a block goes to the source line it starts on.

use super::diff_view::{LineNote, NoteTone, chip, group};
use super::markdown::{Pictures, doc_block};
use super::{Selection, Workspace, px, theme};
use crate::git::FileDiff;
use crate::markdown::{self, Block};
use crate::mddiff::{self, Change, Row};
use gpui::{AnyElement, ClickEvent, Context, FontWeight, Rgba, WeakEntity, div, prelude::*};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// A document's rows, by the texts compared (their addresses).
type Rows = RefCell<HashMap<(usize, usize), Rc<Vec<Row>>>>;

/// How changed documents are read, and their rows kept between frames.
#[derive(Default)]
pub struct Page {
    /// Source: the ordinary diff of the text.
    pub source: bool,
    /// Removed sections opened, by path and the first old line.
    open: HashSet<(String, u32)>,
    /// After a click on a block: the line to put a comment on once the source is laid out.
    pub comment: Option<(bool, u32)>,
    /// A document's rows, by the texts compared.
    cache: Rows,
}

impl Page {
    fn rows(&self, file: &FileDiff) -> Rc<Vec<Row>> {
        let key = |t: &Option<std::sync::Arc<str>>| {
            t.as_ref()
                .map_or(0, |t| std::sync::Arc::as_ptr(t) as *const u8 as usize)
        };
        let key = (key(&file.old_text), key(&file.new_text));
        let mut cache = self.cache.borrow_mut();
        if cache.len() > 16 {
            cache.clear();
        }
        cache
            .entry(key)
            .or_insert_with(|| {
                Rc::new(mddiff::diff(
                    file.old_text.as_deref().unwrap_or_default(),
                    file.new_text.as_deref().unwrap_or_default(),
                ))
            })
            .clone()
    }
}

/// The margin bar's colour for a change.
fn bar(change: Change) -> Option<Rgba> {
    match change {
        Change::Same => None,
        Change::Added => Some(theme::added()),
        Change::Changed => Some(theme::warning()),
        Change::Removed => Some(theme::removed()),
    }
}

/// The section a run of removed blocks starts: its heading, when it starts with one.
fn heading(block: &Block) -> Option<String> {
    match block {
        Block::Heading(_, i) => Some(i.text.clone()),
        _ => None,
    }
}

impl Workspace {
    /// Whether file `file` is drawn as a page (not as its text diff).
    pub(super) fn shows_page(&self, file: &FileDiff) -> bool {
        !self.page.source
            && markdown::is_document(file.path())
            && file.note.is_none()
            && !file.hunks.is_empty()
            && (file.old_text.is_some() || file.new_text.is_some())
    }

    /// Rendered | Source, M, and with `legend` what the bars mean while rendered, for a document's
    /// header.
    pub(super) fn page_switch(
        &self,
        file: &FileDiff,
        legend: bool,
        this: WeakEntity<Self>,
    ) -> Option<AnyElement> {
        if !markdown::is_document(file.path()) || file.note.is_some() || file.hunks.is_empty() {
            return None;
        }
        let source = self.page.source;
        let set = |to: bool| {
            let this = this.clone();
            move |_: &ClickEvent, _: &mut gpui::Window, cx: &mut gpui::App| {
                cx.stop_propagation();
                this.update(cx, |this, cx| this.set_page_source(to, cx))
                    .ok();
            }
        };
        let key = |color: Rgba, word: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(div().w(px(3.)).h(px(12.)).rounded(px(2.)).bg(color))
                .child(word)
        };
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    group()
                        .child(chip("page-rendered", "Rendered", !source).on_click(set(false)))
                        .child(chip("page-source", "Source", source).on_click(set(true))),
                )
                .child(super::keys::caps("M", super::keys::On::Plain))
                .children((legend && !source).then(|| {
                    div()
                        .flex()
                        .gap(px(10.))
                        .ml(px(4.))
                        .text_size(px(11.))
                        .text_color(theme::muted())
                        .child(key(theme::added(), "added"))
                        .child(key(theme::warning(), "changed"))
                        .child(key(theme::removed(), "removed"))
                }))
                .into_any_element(),
        )
    }

    /// Rendered or Source for every document (M).
    pub(super) fn set_page_source(&mut self, source: bool, cx: &mut Context<Self>) {
        if self.page.source == source {
            return;
        }
        self.page.source = source;
        self.sync_stack();
        self.build_stack(cx);
        if self.stack.is_none() {
            self.relayout();
        }
        cx.notify();
    }

    pub(super) fn toggle_page_source(&mut self, cx: &mut Context<Self>) {
        self.set_page_source(!self.page.source, cx);
    }

    /// Goes to `line` of file `ix` in Source; with `comment`, starts a comment on it.
    fn page_to_source(
        &mut self,
        ix: usize,
        old: bool,
        line: u32,
        comment: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(ix))
            .map(|f| f.path().to_owned())
        else {
            return;
        };
        self.page.comment = comment.then_some((old, line));
        if ix != self.file {
            self.page.source = true;
            self.sync_stack();
            self.build_stack(cx);
            self.jump_to_line(&path, line, old, cx);
            return;
        }
        self.set_page_source(true, cx);
        self.jump_line = Some((line, old));
        if self.stack.is_none() {
            self.apply_jump(cx);
        }
    }

    /// What a click on the comment button of file `ix`'s third block does (a snapshot's way in).
    #[cfg(feature = "snapshot")]
    pub(super) fn page_block_comment(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(file) = self.diff.as_ref().and_then(|d| d.files.get(ix)) else {
            return;
        };
        let rows = self.page.rows(file);
        if let Some((_, (line, _))) = rows.get(2).and_then(|r| r.new.clone()) {
            self.page_to_source(ix, false, line, true, cx);
        }
    }

    /// A comment asked for on a block, once the file's source is laid out.
    pub(super) fn page_comment(&mut self, cx: &mut Context<Self>) {
        if self.data.is_some()
            && !self.rows.is_empty()
            && let Some((old, line)) = self.page.comment.take()
        {
            self.start_comment(old, line, cx);
        }
    }

    /// The column heads of Split: the versions compared.
    fn page_heads(&self) -> (String, String) {
        match self.selection {
            Selection::Versions { from, to } => {
                match (self.versions.get(from), self.versions.get(to)) {
                    (Some(a), Some(b)) => (
                        format!("v{} · before", a.number),
                        format!("v{} · now", b.number),
                    ),
                    _ => ("Before".into(), "Now".into()),
                }
            }
            Selection::WorkingTree => ("HEAD · before".into(), "Working tree · now".into()),
            Selection::Commit(ix) => match self.commits.get(ix) {
                Some(c) => {
                    let sha = super::format::short(c.id);
                    (format!("{sha}^ · before"), format!("{sha} · now"))
                }
                None => ("Before".into(), "After".into()),
            },
            Selection::None => ("Before".into(), "After".into()),
        }
    }

    /// File `ix` as a page: Unified or Split as the toolbar says.
    pub(super) fn render_page(&self, ix: usize, this: WeakEntity<Self>) -> AnyElement {
        let Some(file) = self.diff.as_ref().and_then(|d| d.files.get(ix)) else {
            return div().into_any_element();
        };
        let rows = self.page.rows(file);
        let path = file.path().to_owned();
        let side = |old: bool| {
            let pictures: Pictures = file
                .pictures
                .iter()
                .filter(|p| p.old == old)
                .map(|p| (p.src.clone(), (self.images.image(&p.image), p.image.size)))
                .collect();
            Rc::new(pictures)
        };
        let (old_pics, new_pics) = (side(true), side(false));
        let data = (ix == self.file).then_some(self.data.as_deref()).flatten();
        let notes: Vec<LineNote> = self
            .line_notes_in(&path, data)
            .into_iter()
            .filter(|n| n.tone != NoteTone::Bookmark)
            .collect();
        let ctx = BlockCtx {
            ix,
            this,
            notes: Rc::new(notes),
        };
        let page = if self.options.unified {
            self.unified(&rows, &path, &ctx, &old_pics, &new_pics)
        } else {
            self.split(&rows, &ctx, &old_pics, &new_pics)
        };
        div()
            .w_full()
            .py(px(16.))
            .pl(px(44.))
            .pr(px(24.))
            .font_family(theme::UI_FONT)
            .text_size(px(14.))
            .line_height(px(22.))
            .text_color(theme::text())
            .child(page)
            .into_any_element()
    }

    fn unified(
        &self,
        rows: &[Row],
        path: &str,
        ctx: &BlockCtx,
        old_pics: &Rc<Pictures>,
        new_pics: &Rc<Pictures>,
    ) -> AnyElement {
        let mut out: Vec<AnyElement> = Vec::new();
        let mut k = 0;
        while k < rows.len() {
            let row = &rows[k];
            // A run of removed blocks that starts a section folds to one line.
            if row.change == Change::Removed
                && let Some((first, (line, _))) = &row.old
                && heading(first).is_some()
            {
                let end = k + rows[k..]
                    .iter()
                    .take_while(|r| r.change == Change::Removed)
                    .count();
                let names: Vec<String> = rows[k..end]
                    .iter()
                    .filter_map(|r| r.old.as_ref().and_then(|(b, _)| heading(b)))
                    .collect();
                let key = (path.to_owned(), *line);
                let open = self.page.open.contains(&key);
                out.push(fold_line(&names, open, key, ctx.this.clone(), k));
                if open {
                    for (n, r) in rows[k..end].iter().enumerate() {
                        out.push(ctx.block(r, true, old_pics, k + n, true));
                    }
                }
                k = end;
                continue;
            }
            let pics = if row.change == Change::Removed {
                old_pics
            } else {
                new_pics
            };
            out.push(ctx.block(row, row.change == Change::Removed, pics, k, true));
            k += 1;
        }
        div()
            .max_w(px(880.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .children(out)
            .into_any_element()
    }

    fn split(
        &self,
        rows: &[Row],
        ctx: &BlockCtx,
        old_pics: &Rc<Pictures>,
        new_pics: &Rc<Pictures>,
    ) -> AnyElement {
        let (before, now) = self.page_heads();
        let head = |text: String| {
            let (version, rest) = text.split_once(" · ").unwrap_or((&text, ""));
            div()
                .flex_1()
                .min_w_0()
                .pb(px(6.))
                .border_b_1()
                .border_color(theme::island_border())
                .text_size(px(12.))
                .text_color(theme::faint())
                .flex()
                .gap(px(4.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(version.to_owned()),
                )
                .children((!rest.is_empty()).then(|| format!("· {rest}")))
        };
        let hatch = || {
            div()
                .flex_1()
                .min_w_0()
                .min_h(px(24.))
                .mr(px(64.))
                .rounded(px(4.))
                .bg(gpui::pattern_slash(theme::selected(), 1., 6.))
                .into_any_element()
        };
        let cell = |el: AnyElement| div().flex_1().min_w_0().child(el).into_any_element();
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .gap(px(56.))
                    .child(head(before))
                    .child(head(now)),
            )
            .children(rows.iter().enumerate().map(|(k, r)| {
                let (left, right) = match r.change {
                    Change::Added => (hatch(), cell(ctx.side_block(r, false, new_pics, k))),
                    Change::Removed => (cell(ctx.side_block(r, true, old_pics, k)), hatch()),
                    _ => (
                        cell(ctx.side_block(r, true, old_pics, k)),
                        cell(ctx.side_block(r, false, new_pics, k)),
                    ),
                };
                div().flex().gap(px(56.)).child(left).child(right)
            }))
            .into_any_element()
    }
}

/// What every block of one page needs.
struct BlockCtx {
    ix: usize,
    this: WeakEntity<Workspace>,
    notes: Rc<Vec<LineNote>>,
}

impl BlockCtx {
    /// A block of Unified: the old one when `old` (removed: faded and struck), else the new one,
    /// or for a changed block the two read together.
    fn block(
        &self,
        r: &Row,
        old: bool,
        pics: &Rc<Pictures>,
        k: usize,
        unified: bool,
    ) -> AnyElement {
        let (block, lines) = match (old, r) {
            (
                true,
                Row {
                    old: Some((b, l)), ..
                },
            ) => (b, *l),
            (
                false,
                Row {
                    merged: Some(m),
                    new: Some((_, l)),
                    ..
                },
            ) if unified => (m, *l),
            (
                _,
                Row {
                    new: Some((b, l)), ..
                },
            ) => (b, *l),
            (
                _,
                Row {
                    old: Some((b, l)), ..
                },
            ) => (b, *l),
            _ => return div().into_any_element(),
        };
        let struck = unified && r.change == Change::Removed;
        self.frame(
            doc_block(block, format!("page-{}-{k}-{old}", self.ix), pics.clone()),
            bar(r.change),
            struck,
            old,
            lines,
            k,
        )
    }

    /// A block of Split: one side's own (a changed one with its side's words marked).
    fn side_block(&self, r: &Row, old: bool, pics: &Rc<Pictures>, k: usize) -> AnyElement {
        self.block(r, old, pics, k, false)
    }

    /// A block in its margin: the change's bar left, the comments on its lines and a comment
    /// button right (on hover).
    fn frame(
        &self,
        body: AnyElement,
        color: Option<Rgba>,
        struck: bool,
        old: bool,
        (first, last): (u32, u32),
        k: usize,
    ) -> AnyElement {
        let count: usize = self
            .notes
            .iter()
            .filter(|n| n.old == old && (first..=last).contains(&n.line))
            .map(|n| n.count)
            .sum();
        let at = self
            .notes
            .iter()
            .find(|n| n.old == old && (first..=last).contains(&n.line))
            .map_or(first, |n| n.line);
        let (ix, show, write) = (self.ix, self.this.clone(), self.this.clone());
        let group_name = "page-block";
        div()
            .id(("page-block", k * 2 + old as usize))
            .group(group_name)
            .relative()
            .children(color.map(|c| {
                div()
                    .absolute()
                    .left(px(-20.))
                    .top(px(2.))
                    .bottom(px(2.))
                    .w(px(3.))
                    .rounded(px(2.))
                    .bg(c)
            }))
            .child(
                div()
                    .pr(px(64.))
                    .when(struck, |s| s.opacity(0.6).line_through())
                    .child(body),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .flex()
                    .gap(px(4.))
                    .children((count > 0).then(|| {
                        div()
                            .id(("page-notes", k * 2 + old as usize))
                            .px(px(6.))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .rounded_full()
                            .bg(theme::info_bg())
                            .text_size(px(11.))
                            .text_color(theme::accent())
                            .cursor_pointer()
                            .child(super::icons::icon("message").size(px(12.)))
                            .child(count.to_string())
                            .on_click(move |_: &ClickEvent, _, cx| {
                                show.update(cx, |this, cx| {
                                    this.page_to_source(ix, old, at, false, cx)
                                })
                                .ok();
                            })
                    }))
                    .child(
                        div()
                            .id(("page-comment", k * 2 + old as usize))
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .invisible()
                            .group_hover(group_name, |s| s.visible())
                            .bg(theme::hover())
                            .text_color(theme::muted())
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme::text()))
                            .child("+")
                            .on_click(move |_: &ClickEvent, _, cx| {
                                write
                                    .update(cx, |this, cx| {
                                        this.page_to_source(ix, old, first, true, cx)
                                    })
                                    .ok();
                            }),
                    ),
            )
            .into_any_element()
    }
}

/// "1 section removed: Known issues", and a click shows it where it was.
fn fold_line(
    names: &[String],
    open: bool,
    key: (String, u32),
    this: WeakEntity<Workspace>,
    k: usize,
) -> AnyElement {
    let what = match names.len() {
        1 => "1 section removed".to_owned(),
        n => format!("{n} sections removed"),
    };
    div()
        .id(("page-fold", k))
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .h(px(28.))
        .rounded(px(4.))
        .bg(theme::removed_line())
        .text_size(px(12.))
        .text_color(theme::removed())
        .cursor_pointer()
        .child(
            super::icons::icon("x")
                .size(px(13.))
                .text_color(theme::removed()),
        )
        .child(div().flex_none().child(format!("{what}:")))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .child(names.join(", ")),
        )
        .child(
            div()
                .text_color(theme::faint())
                .child(if open { "hide" } else { "show" }),
        )
        .on_click(move |_: &ClickEvent, _, cx| {
            this.update(cx, |this, cx| {
                if !this.page.open.remove(&key) {
                    this.page.open.insert(key.clone());
                }
                this.remeasure_pages();
                cx.notify();
            })
            .ok();
        })
        .into_any_element()
}
