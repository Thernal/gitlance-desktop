//! A changed markdown file read as a page (Design #11 A + B, `docs/specs/markdown-viewer.md`): its
//! card has Rendered | Source (M, the active file's), Rendered by default; a document turned to
//! Source stays so between launches (`page-source.txt`). The page follows the toolbar's Split |
//! Unified. Unified draws the new version with the changes in it — a bar in the margin for every
//! changed block, green added, yellow changed with its words marked, red removed (faded and struck;
//! a removed section folds to one line). Split draws the old version beside the new, block beside
//! block, a hatched gap where one side has none. Images come from the repository at each side's
//! version (`FileDiff::pictures`). A comment on a block goes to the source line it starts on,
//! written under the block on the page; its count opens the block's comments there.
//! Viewed one file at a time the page scrolls on its own: n / p step through its changed blocks,
//! space and ⇧space scroll it.

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

const SOURCE_FILE: &str = "page-source.txt";

/// How changed documents are read, and their rows kept between frames.
#[derive(Default)]
pub struct Page {
    /// The documents read as Source (the ordinary diff of the text), by path; of the repository
    /// `source_of`.
    source: HashSet<String>,
    source_of: Option<String>,
    /// Removed sections opened, by path and the first old line.
    open: HashSet<(String, u32)>,
    /// Blocks whose comments are shown under them: path, side, first line.
    notes_open: HashSet<(String, bool, u32)>,
    /// A document's rows, by the texts compared.
    cache: Rows,
    /// One file at a time: the page's own scroll, its children the blocks.
    scroll: gpui::ScrollHandle,
    /// The changed blocks among those children, as last drawn; the one n / p last went to.
    changes: RefCell<Vec<usize>>,
    at: Option<usize>,
}

/// The paths of repository `repo` in the lines of `page-source.txt` (`repo<TAB>path`).
fn source_paths(text: &str, repo: &str) -> HashSet<String> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(r, _)| *r == repo)
        .map(|(_, p)| p.to_owned())
        .collect()
}

/// `text` with repository `repo`'s lines replaced by `paths`; other repositories' kept.
fn with_source_paths(text: &str, repo: &str, paths: &HashSet<String>) -> String {
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| l.split_once('\t').is_some_and(|(r, _)| r != repo))
        .map(str::to_owned)
        .chain(paths.iter().map(|p| format!("{repo}\t{p}")))
        .collect();
    lines.sort_unstable();
    lines.join("\n") + "\n"
}

impl Page {
    /// Another file: n / p start again from the top of its view.
    pub fn leave(&mut self) {
        self.at = None;
    }

    /// Document `path` is read as Source.
    pub fn is_source(&self, path: &str) -> bool {
        self.source.contains(path)
    }

    /// The documents of repository `repo` read as Source, once per repository.
    pub fn load(&mut self, repo: &str) {
        if self.source_of.as_deref() != Some(repo) {
            self.source = source_paths(&crate::storage::read(SOURCE_FILE), repo);
            self.source_of = Some(repo.to_owned());
        }
    }

    /// Document `path` read as Source or not; remembered.
    fn set_source(&mut self, path: &str, source: bool) -> bool {
        let changed = if source {
            self.source.insert(path.to_owned())
        } else {
            self.source.remove(path)
        };
        if changed && let Some(repo) = &self.source_of {
            crate::storage::save(
                SOURCE_FILE,
                with_source_paths(&crate::storage::read(SOURCE_FILE), repo, &self.source),
            );
        }
        changed
    }

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
        !self.page.is_source(file.path())
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
        let source = self.page.is_source(file.path());
        let set = |to: bool| {
            let this = this.clone();
            let path = file.path().to_owned();
            move |_: &ClickEvent, _: &mut gpui::Window, cx: &mut gpui::App| {
                cx.stop_propagation();
                this.update(cx, |this, cx| this.set_page_source(&path, to, cx))
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
                        .child(
                            chip(
                                gpui::ElementId::Name(
                                    format!("page-rendered-{}", file.path()).into(),
                                ),
                                "Rendered",
                                !source,
                            )
                            .on_click(set(false)),
                        )
                        .child(
                            chip(
                                gpui::ElementId::Name(
                                    format!("page-source-{}", file.path()).into(),
                                ),
                                "Source",
                                source,
                            )
                            .on_click(set(true)),
                        ),
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

    /// Document `path` read Rendered or as Source.
    pub(super) fn set_page_source(&mut self, path: &str, source: bool, cx: &mut Context<Self>) {
        if let Some(root) = &self.root {
            self.page.load(&crate::review::key(root));
        }
        if !self.page.set_source(path, source) {
            return;
        }
        // The other form fades in: Source's rows as a file just opened, the page as a page
        // (#56, 15).
        if let Some(ix) = self
            .diff
            .as_ref()
            .and_then(|d| d.files.iter().position(|f| f.path() == path))
        {
            self.mark_motion(&format!("file-{ix}"), cx);
            self.mark_motion(&format!("page-{ix}"), cx);
        }
        self.sync_stack();
        self.build_stack(cx);
        if self.stack.is_none() {
            self.relayout();
        }
        cx.notify();
    }

    /// M: the active file, when it is a document, turns Rendered or Source.
    pub(super) fn toggle_page_source(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(self.file))
            .map(|f| f.path().to_owned())
            .filter(|p| markdown::is_document(p))
        else {
            return;
        };
        let source = self.page.is_source(&path);
        self.set_page_source(&path, !source, cx);
    }

    /// The comment button of file `ix`'s third block (a snapshot's way in).
    #[cfg(feature = "snapshot")]
    pub(super) fn page_block_comment(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(file) = self.diff.as_ref().and_then(|d| d.files.get(ix)) else {
            return;
        };
        let rows = self.page.rows(file);
        if let Some((_, (line, _))) = rows.get(2).and_then(|r| r.new.clone()) {
            self.start_page_comment(ix, false, line, cx);
        }
    }

    /// The count of file `ix`'s third block opened (a snapshot's way in).
    #[cfg(feature = "snapshot")]
    pub(super) fn page_block_notes(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(file) = self.diff.as_ref().and_then(|d| d.files.get(ix)) else {
            return;
        };
        let rows = self.page.rows(file);
        if let Some((_, (line, _))) = rows.get(2).and_then(|r| r.new.clone()) {
            self.toggle_page_notes(ix, false, line, cx);
        }
    }

    /// A block's count: its comments shown under it on the page, or hidden again.
    fn toggle_page_notes(&mut self, ix: usize, old: bool, first: u32, cx: &mut Context<Self>) {
        let Some(path) = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(ix))
            .map(|f| f.path().to_owned())
        else {
            return;
        };
        let key = (path, old, first);
        if !self.page.notes_open.remove(&key) {
            self.page.notes_open.insert(key);
        }
        self.remeasure_pages();
        cx.notify();
    }

    /// What goes under each block of file `ix` (by side and first line): the composer written
    /// on it, and the comments of a block whose count was opened.
    fn page_under(
        &self,
        ix: usize,
        path: &str,
        rows: &[Row],
        this: &WeakEntity<Self>,
    ) -> HashMap<(bool, u32), Vec<AnyElement>> {
        let span = |old: bool, line: u32| {
            rows.iter()
                .filter_map(|r| if old { r.old.as_ref() } else { r.new.as_ref() })
                .map(|(_, l)| *l)
                .find(|(a, b)| (*a..=*b).contains(&line))
        };
        let mut under: HashMap<(bool, u32), Vec<AnyElement>> = HashMap::new();
        if let Some((_, old, line)) = self.page_compose().filter(|(f, _, _)| *f == ix)
            && let Some((first, _)) = span(old, line)
            && let Some(composer) = self.compose_box(this.clone())
        {
            under
                .entry((old, first))
                .or_default()
                .push(div().mt(px(8.)).child(composer).into_any_element());
        }
        for (p, old, first) in &self.page.notes_open {
            let Some((first, last)) = span(*old, *first).filter(|_| p == path) else {
                continue;
            };
            let here = |o: bool, n: u32| o == *old && (first..=last).contains(&n);
            let mut notes: Vec<AnyElement> = Vec::new();
            if self.showing_request() {
                notes.extend(
                    self.request_threads_in(path)
                        .into_iter()
                        .filter(|&(_, o, n)| here(o, n))
                        .map(|(t, _, _)| self.render_request_thread(t, 0., this.clone())),
                );
            }
            notes.extend(
                self.comments
                    .iter()
                    .filter(|c| c.path == path && here(c.old, c.line))
                    .map(|c| self.render_thread(c.id, 0., this.clone())),
            );
            under.entry((*old, first)).or_default().extend(
                notes
                    .into_iter()
                    .map(|n| div().mt(px(8.)).child(n).into_any_element()),
            );
        }
        under
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

    /// File `ix` as a page in the list of every file: Unified or Split as the toolbar says.
    pub(super) fn render_page(&self, ix: usize, this: WeakEntity<Self>) -> AnyElement {
        let blocks = self.page_blocks(ix, this);
        let page = page_text(div().w_full().py(px(16.)).pl(px(44.)).pr(px(24.))).child(
            div()
                .when(self.options.unified, |s| s.max_w(px(880.)))
                .flex()
                .flex_col()
                .gap(px(12.))
                .children(blocks.into_iter().map(|(b, _)| b)),
        );
        if self.motion.recent(&format!("page-{ix}")) {
            super::motion::appear(page, ("page-coming", ix), super::motion::FAST, |page, v| {
                page.opacity(v)
            })
            .into_any_element()
        } else {
            page.into_any_element()
        }
    }

    /// File `ix` alone as a page that scrolls on its own, its blocks its children, so n / p can
    /// step through the changed ones.
    pub(super) fn render_page_alone(&self, ix: usize, this: WeakEntity<Self>) -> AnyElement {
        let blocks = self.page_blocks(ix, this);
        *self.page.changes.borrow_mut() = blocks
            .iter()
            .enumerate()
            .filter(|(_, (_, changed))| *changed)
            .map(|(k, _)| k)
            .collect();
        let unified = self.options.unified;
        // Laid over its place rather than in it: its blocks never make the pane taller than the
        // window, so it scrolls.
        let page = page_text(
            div()
                .id("page")
                .track_scroll(&self.page.scroll)
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(12.))
                .py(px(16.))
                .pl(px(44.))
                .pr(px(24.)),
        )
        .children(blocks.into_iter().map(|(b, _)| {
            div()
                .flex_none()
                .when(unified, |s| s.max_w(px(880.)))
                .child(b)
        }));
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(page)
            .into_any_element()
    }

    /// n / p on a page viewed alone: the next (or previous) changed block, from the last one gone
    /// to, else from the top of the view. Whether the page took the keys.
    pub(super) fn step_page(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        if !self.page_alone() {
            return false;
        }
        let changes = self.page.changes.borrow().clone();
        let top = self.page.scroll.top_item();
        let from = self.page.at.filter(|&at| at + 1 >= top);
        let to = if forward {
            changes
                .iter()
                .copied()
                .find(|&c| from.map_or(c >= top, |at| c > at))
        } else {
            changes
                .iter()
                .rev()
                .copied()
                .find(|&c| c < from.unwrap_or(top))
        };
        if let Some(to) = to {
            self.page.at = Some(to);
            self.page.scroll.scroll_to_top_of_item(to);
            cx.notify();
        }
        true
    }

    /// Space and ⇧space on a page viewed alone: a screen down or up. Whether the page took it.
    pub(super) fn scroll_page(&mut self, down: bool, cx: &mut Context<Self>) -> bool {
        if !self.page_alone() {
            return false;
        }
        let s = &self.page.scroll;
        let step = s.bounds().size.height * 0.85;
        let y = if down {
            (s.offset().y - step).max(-s.max_offset().y)
        } else {
            (s.offset().y + step).min(px(0.))
        };
        s.set_offset(gpui::point(s.offset().x, y));
        self.page.at = None;
        cx.notify();
        true
    }

    /// One file at a time, and it is a page.
    fn page_alone(&self) -> bool {
        self.stack.is_none()
            && self
                .diff
                .as_ref()
                .and_then(|d| d.files.get(self.file))
                .is_some_and(|f| self.shows_page(f))
    }

    /// File `ix`'s blocks: Unified or Split as the toolbar says, each with whether it is a change.
    fn page_blocks(&self, ix: usize, this: WeakEntity<Self>) -> Vec<(AnyElement, bool)> {
        let Some(file) = self.diff.as_ref().and_then(|d| d.files.get(ix)) else {
            return Vec::new();
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
        let under = self.page_under(ix, &path, &rows, &this);
        let ctx = BlockCtx {
            ix,
            this,
            notes: Rc::new(notes),
            under: RefCell::new(under),
        };
        if self.options.unified {
            self.unified(&rows, &path, &ctx, &old_pics, &new_pics)
        } else {
            self.split(&rows, &ctx, &old_pics, &new_pics)
        }
    }

    /// Unified's blocks, each with whether it is a change.
    fn unified(
        &self,
        rows: &[Row],
        path: &str,
        ctx: &BlockCtx,
        old_pics: &Rc<Pictures>,
        new_pics: &Rc<Pictures>,
    ) -> Vec<(AnyElement, bool)> {
        let mut out: Vec<(AnyElement, bool)> = Vec::new();
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
                out.push((fold_line(&names, open, key, ctx.this.clone(), k), true));
                if open {
                    // Opened, its blocks fade in (#56, 15).
                    for (n, r) in rows[k..end].iter().enumerate() {
                        let block = super::motion::appear(
                            div().child(ctx.block(r, true, old_pics, k + n, true)),
                            ("removed-opening", k + n),
                            super::motion::BASE,
                            |block, v| block.opacity(v),
                        );
                        out.push((block.into_any_element(), false));
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
            out.push((
                ctx.block(row, row.change == Change::Removed, pics, k, true),
                row.change != Change::Same,
            ));
            k += 1;
        }
        out
    }

    /// Split's column heads, then its rows, each with whether it is a change.
    fn split(
        &self,
        rows: &[Row],
        ctx: &BlockCtx,
        old_pics: &Rc<Pictures>,
        new_pics: &Rc<Pictures>,
    ) -> Vec<(AnyElement, bool)> {
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
        let heads = div()
            .flex()
            .gap(px(56.))
            .child(head(before))
            .child(head(now))
            .into_any_element();
        std::iter::once((heads, false))
            .chain(rows.iter().enumerate().map(|(k, r)| {
                let (left, right) = match r.change {
                    Change::Added => (hatch(), cell(ctx.side_block(r, false, new_pics, k))),
                    Change::Removed => (cell(ctx.side_block(r, true, old_pics, k)), hatch()),
                    _ => (
                        cell(ctx.side_block(r, true, old_pics, k)),
                        cell(ctx.side_block(r, false, new_pics, k)),
                    ),
                };
                (
                    div()
                        .flex()
                        .gap(px(56.))
                        .child(left)
                        .child(right)
                        .into_any_element(),
                    r.change != Change::Same,
                )
            }))
            .collect()
    }
}

/// What every block of one page needs.
struct BlockCtx {
    ix: usize,
    this: WeakEntity<Workspace>,
    notes: Rc<Vec<LineNote>>,
    /// What goes under a block (the composer, its opened comments), by side and first line.
    under: RefCell<HashMap<(bool, u32), Vec<AnyElement>>>,
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
        let (ix, show, write) = (self.ix, self.this.clone(), self.this.clone());
        let under = self
            .under
            .borrow_mut()
            .remove(&(old, first))
            .unwrap_or_default();
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
            .children(under)
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
                            .child(
                                super::icons::icon("message")
                                    .size(px(12.))
                                    .text_color(theme::accent()),
                            )
                            .child(count.to_string())
                            .on_click(move |_: &ClickEvent, _, cx| {
                                show.update(cx, |this, cx| {
                                    this.toggle_page_notes(ix, old, first, cx)
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
                                        this.start_page_comment(ix, old, first, cx)
                                    })
                                    .ok();
                            }),
                    ),
            )
            .into_any_element()
    }
}

/// A page's type: the document reads in the interface's font, a size up.
fn page_text<E: Styled>(el: E) -> E {
    el.font_family(theme::UI_FONT)
        .text_size(px(14.))
        .line_height(px(22.))
        .text_color(theme::text())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_read_as_source_are_kept_per_repository() {
        let text = "a\tREADME.md\nb\tdocs/x.md\n";
        assert_eq!(
            source_paths(text, "a"),
            HashSet::from(["README.md".to_owned()])
        );
        let paths = HashSet::from(["CHANGELOG.md".to_owned()]);
        let saved = with_source_paths(text, "a", &paths);
        assert_eq!(saved, "a\tCHANGELOG.md\nb\tdocs/x.md\n");
        assert_eq!(
            source_paths(&saved, "b"),
            HashSet::from(["docs/x.md".to_owned()])
        );
    }
}
