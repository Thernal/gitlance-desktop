//! A file read in Code (#71, #75): read only, at Code's revision — the working tree on disk by
//! default, else a commit's —, highlighted, with its line numbers; a Markdown file rendered
//! (Source shows its lines), an image shown. A click on a line number writes a note for the agent
//! on that plain line: kept on the path, the line and its text, drawn under the line while the
//! text is there (else it is outdated, in the Agents window). Opened from another space, it is
//! read as that page shows the project. A binary file says so, with Open in (#78); ⌘W closes its
//! tab, esc goes up to Code.

use super::spaces::Space;
use super::{Header, Workspace, format, island, markdown, px, theme};
use crate::git::ImageKind;
use crate::git::Repo;
use crate::highlight::{self, Span};
use git2::Oid;
use gpui::{
    ClickEvent, Context, FontStyle, HighlightStyle, ListAlignment, ListState, SharedString,
    StyledText, Task, div, img, list, prelude::*,
};
use std::rc::Rc;
use std::sync::Arc;

pub struct FileView {
    pub path: String,
    /// What it was read at: a short id, or the working tree.
    at: String,
    /// The commit it was read at; None the working tree.
    rev: Option<Oid>,
    lines: Vec<String>,
    /// Its lines with their tabs widened, and their highlights.
    text: Rc<Vec<(SharedString, Vec<Span>)>>,
    list: ListState,
    /// A Markdown file: shown rendered, until Source.
    pub rendered: bool,
    /// An image file, decoded.
    image: Option<Arc<gpui::Image>>,
    /// A binary file that is no image: its size, and nothing to show (#78).
    binary: Option<u64>,
}

/// The file being read for the view.
pub type Reading = Option<Task<()>>;

/// A row of the file's list: a line, a note under it, or the note being written.
#[derive(Clone, Copy)]
enum Row {
    Line(usize),
    Note(u64),
    Composer,
}

impl FileView {
    /// What it was read at: a short id, or the working tree.
    pub fn at(&self) -> &str {
        &self.at
    }

    /// The commit it was read at; None the working tree.
    pub fn rev(&self) -> Option<Oid> {
        self.rev
    }

    /// Line `line` (1-based) as it is.
    pub fn line(&self, line: u32) -> Option<&str> {
        self.lines
            .get((line as usize).checked_sub(1)?)
            .map(String::as_str)
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    fn markdown(&self) -> bool {
        self.path.ends_with(".md") || self.path.ends_with(".markdown")
    }
}

/// Where a note on `code` written at line `line` sits now among `lines`: its line while the text
/// is there, else the nearest line with that text; None when it is gone (outdated).
fn place_note(lines: &[String], line: u32, code: &str) -> Option<usize> {
    let at = (line as usize).checked_sub(1)?;
    if lines.get(at).is_some_and(|l| l == code) {
        return Some(at);
    }
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| *l == code)
        .min_by_key(|(n, _)| n.abs_diff(at))
        .map(|(n, _)| n)
}

impl Workspace {
    /// A file's lines are on screen in Code: `c` writes a note on one of them.
    pub(super) fn code_file_shown(&self) -> bool {
        self.nav.space == Space::Code
            && self
                .file_view
                .as_ref()
                .is_some_and(|v| v.image.is_none() && !(v.markdown() && v.rendered))
    }

    /// Shows `path` in Code at its revision, as a tab; from another space, read as that page
    /// shows the project (#71, #75).
    pub(super) fn open_file_view(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.nav.space != Space::Code && !self.plain {
            match self.diff.as_ref().map(|d| &d.header) {
                Some(Header::WorkingTree { .. }) => self.code.rev = None,
                Some(_) => {
                    if let Some(rev) = self.viewed_rev() {
                        self.code.rev = Some((format::short(rev), rev));
                    }
                }
                None => {}
            }
        }
        if !self.code.tabs.contains(&path) && self.code.preview.as_ref() != Some(&path) {
            self.code.tabs.push(path.clone());
        }
        self.code.reveal(&path);
        self.arrive(super::nav::Page::File(path.clone()));
        self.read_code_files(false, cx);
        self.remember_place(
            super::recents::Key::File(path.clone()),
            path.clone(),
            "file".to_owned(),
        );
        let rev = self.code.at().filter(|_| !self.plain);
        let file = path.clone();
        self.file_reading = Some(cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    let at = match rev {
                        Some(rev) => format::short(rev),
                        None => "the working tree".to_owned(),
                    };
                    if let Some(kind) = ImageKind::of(&file) {
                        let bytes = match rev {
                            Some(rev) => Repo::open(&root).ok()?.bytes_at(rev, &file)?,
                            None => std::fs::read(root.join(&file)).ok()?,
                        };
                        let image = gpui::Image::from_bytes(super::image_view::format(kind), bytes);
                        return Some((Vec::new(), Vec::new(), at, Some(Arc::new(image)), None));
                    }
                    let bytes = match rev {
                        Some(rev) => Repo::open(&root).ok()?.bytes_at(rev, &file)?,
                        None => std::fs::read(root.join(&file)).ok()?,
                    };
                    if super::open_in::binary(&bytes) {
                        let size = Some(bytes.len() as u64);
                        return Some((Vec::new(), Vec::new(), at, None, size));
                    }
                    let text = String::from_utf8_lossy(&bytes);
                    let spans = highlight::highlight(&file, &text);
                    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
                    Some((lines, spans, at, None, None))
                })
                .await;
            this.update(cx, |this, cx| {
                this.file_reading = None;
                match read {
                    Some((lines, spans, at, image, binary)) => {
                        let text = lines
                            .iter()
                            .enumerate()
                            .map(|(n, l)| widen(l, spans.get(n).map_or(&[][..], Vec::as_slice)))
                            .collect();
                        this.file_view = Some(FileView {
                            path,
                            at,
                            rev,
                            lines,
                            text: Rc::new(text),
                            list: ListState::new(0, ListAlignment::Top, px(400.)),
                            rendered: true,
                            image,
                            binary,
                        });
                    }
                    None => {
                        this.error = Some(format!("{path} is not in this version.").into());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// The file's declarations, by keyword: (1-based line, depth, kind, name) — its Outline (#76).
    pub(super) fn file_outline(&self) -> Vec<(u32, usize, &'static str, String)> {
        let Some(view) = self.file_view.as_ref().filter(|v| v.image.is_none()) else {
            return Vec::new();
        };
        view.lines
            .iter()
            .enumerate()
            .filter_map(|(n, line)| {
                let (kind, name) = crate::symbols::declaration(line)?;
                let indent = line.len() - line.trim_start().len();
                Some((n as u32 + 1, indent / 4, kind, name))
            })
            .collect()
    }

    /// Line `line` of the file comes into view, its source shown.
    pub(super) fn reveal_file_line(&mut self, line: u32, cx: &mut Context<Self>) {
        let Some(view) = self.file_view.as_mut() else {
            return;
        };
        view.rendered = false;
        let Some(view) = self.file_view.as_ref() else {
            return;
        };
        let rows = self.file_rows(view);
        if view.list.item_count() != rows.len() {
            view.list.reset(rows.len());
        }
        if let Some(ix) = rows
            .iter()
            .position(|r| matches!(r, Row::Line(n) if *n + 1 == line as usize))
        {
            view.list.scroll_to_reveal_item(ix);
        }
        cx.notify();
    }

    /// The file's rows: its lines, each with the notes on it and the note being written.
    fn file_rows(&self, view: &FileView) -> Vec<Row> {
        let mut notes: Vec<(usize, u64)> = self
            .comments
            .iter()
            .filter(|c| c.path == view.path && !c.old && !c.archived)
            .filter_map(|c| Some((place_note(&view.lines, c.line, &c.code)?, c.id)))
            .collect();
        notes.sort_unstable();
        let compose = self
            .file_compose()
            .filter(|(p, _)| *p == view.path)
            .map(|(_, line)| line as usize - 1);
        let mut out = Vec::with_capacity(view.lines.len() + notes.len() + 1);
        let mut next = notes.iter().peekable();
        for n in 0..view.lines.len() {
            out.push(Row::Line(n));
            while let Some(&&(at, id)) = next.peek() {
                if at != n {
                    break;
                }
                out.push(Row::Note(id));
                next.next();
            }
            if compose == Some(n) {
                out.push(Row::Composer);
            }
        }
        out
    }

    /// The line at the top of `path`'s view as it is scrolled: where Open in opens it (#78).
    pub(super) fn file_top_line(&self, path: &str) -> u32 {
        let Some(view) = self.file_view.as_ref().filter(|v| v.path == path) else {
            return 1;
        };
        let top = view.list.logical_scroll_top().item_ix;
        self.file_rows(view)
            .iter()
            .take(top + 1)
            .rev()
            .find_map(|row| match row {
                Row::Line(n) => Some(*n as u32 + 1),
                _ => None,
            })
            .unwrap_or(1)
    }

    pub(super) fn render_file_view(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let view = self.file_view.as_ref()?;
        let head = self.render_file_head(view, cx);
        let body = if let Some(size) = view.binary {
            self.render_binary(size, cx)
        } else if let Some(image) = &view.image {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .child(img(image.clone()).max_w_full().max_h_full())
                .into_any_element()
        } else if view.markdown() && view.rendered {
            div()
                .id("file-rendered")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(px(24.))
                .py(px(18.))
                .text_size(px(13.))
                .child(markdown::body(
                    &view.lines.join("\n"),
                    true,
                    format!("file-{}", view.path),
                    None,
                    None,
                ))
                .into_any_element()
        } else {
            self.render_file_lines(view, cx)
        };
        Some(
            island()
                .flex_1()
                .min_w_0()
                .bg(theme::editor())
                .child(head)
                .child(body)
                .into_any_element(),
        )
    }

    /// The file's line over it: its path, language, lines and revision; Rendered | Source for a
    /// Markdown file; read only.
    fn render_file_head(&self, view: &FileView, cx: &mut Context<Self>) -> gpui::Div {
        let markdown = view.markdown() && view.image.is_none();
        let facts = if view.image.is_some() {
            format!("image · {}", view.at)
        } else if let Some(size) = view.binary {
            format!("{} · {}", super::image_view::bytes(size as usize), view.at)
        } else {
            format!("{} lines · {}", view.lines.len(), view.at)
        };
        div()
            .flex_none()
            .h(px(34.))
            .px(px(14.))
            .flex()
            .items_center()
            .gap(px(10.))
            .border_b_1()
            .border_color(theme::island_border())
            .text_size(px(12.))
            .child(
                div()
                    .font_family(theme::CODE_FONT)
                    .text_color(theme::muted())
                    .child(view.path.clone()),
            )
            .child(div().text_color(theme::faint()).child(facts))
            .child(div().flex_1())
            // `c` asks which line the note goes on.
            .children(
                self.goline
                    .as_ref()
                    .filter(|_| self.code_file_shown())
                    .map(|typed| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_color(theme::agent())
                            .child("Note on line")
                            .child(
                                div()
                                    .min_w(px(28.))
                                    .px(px(6.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(theme::agent())
                                    .child(if typed.is_empty() {
                                        " ".to_owned()
                                    } else {
                                        typed.clone()
                                    }),
                            )
                            .child(super::keys::caps("↵", super::keys::On::Plain))
                    }),
            )
            .children(markdown.then(|| {
                let rendered = view.rendered;
                super::diff_view::group()
                    .child(
                        super::diff_view::chip("file-rendered-on", "Rendered", rendered).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.set_rendered(true, cx)),
                        ),
                    )
                    .child(
                        super::diff_view::chip("file-source", "Source", !rendered).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.set_rendered(false, cx)),
                        ),
                    )
            }))
            .child(div().text_color(theme::faint()).child("read only"))
    }

    fn set_rendered(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(view) = &mut self.file_view {
            view.rendered = on;
        }
        cx.notify();
    }

    /// The lines with their numbers; a number's click writes a note on its line.
    fn render_file_lines(&self, view: &FileView, cx: &mut Context<Self>) -> gpui::AnyElement {
        let size = self.code_size();
        let row = (size * 1.55).round();
        let digits = view.lines.len().max(1).to_string().len() as f32;
        let gutter = digits * size * 0.62 + 20.;
        let rows = Rc::new(self.file_rows(view));
        if view.list.item_count() != rows.len() {
            let top = view.list.logical_scroll_top();
            view.list.reset(rows.len());
            view.list.scroll_to(top);
        }
        let text = view.text.clone();
        let this = cx.entity().downgrade();
        list(view.list.clone(), move |ix, _, cx| {
            let Some(&row_kind) = rows.get(ix) else {
                return div().into_any_element();
            };
            let n = match row_kind {
                Row::Line(n) => n,
                Row::Note(id) => {
                    let Some(ws) = this.upgrade() else {
                        return div().into_any_element();
                    };
                    return ws.read(cx).render_thread(id, gutter + 8., this.clone());
                }
                Row::Composer => {
                    let Some(ws) = this.upgrade() else {
                        return div().into_any_element();
                    };
                    return ws.read(cx).render_composer(gutter + 8., this.clone());
                }
            };
            let (line, spans) = &text[n];
            let highlights = spans
                .iter()
                .filter(|s| s.range.end <= line.len())
                .map(|s| {
                    (
                        s.range.clone(),
                        HighlightStyle {
                            color: Some(theme::code(s.color)),
                            font_style: s.italic.then_some(FontStyle::Italic),
                            font_weight: s.bold.then_some(gpui::FontWeight::SEMIBOLD),
                            ..Default::default()
                        },
                    )
                })
                .collect::<Vec<_>>();
            let comment = this.clone();
            div()
                .h(px(row))
                .flex()
                .items_center()
                .child(
                    div()
                        .id(("file-line", n))
                        .w(px(gutter))
                        .flex_none()
                        .flex()
                        .justify_end()
                        .pr_2()
                        .text_color(theme::muted())
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme::agent()))
                        .child((n + 1).to_string())
                        .on_click(move |_, _, cx| {
                            comment
                                .update(cx, |ws, cx| ws.start_file_comment(n as u32 + 1, cx))
                                .ok();
                        }),
                )
                .child(
                    div()
                        .pl_2()
                        .whitespace_nowrap()
                        .text_color(theme::code_text())
                        .child(StyledText::new(line.clone()).with_highlights(highlights)),
                )
                .into_any_element()
        })
        .flex_1()
        .py(px(6.))
        .font_family(theme::CODE_FONT)
        .text_size(px(size))
        .into_any_element()
    }
}

/// A line with its tabs as four spaces, and its spans moved to match; an empty line is a space.
fn widen(line: &str, spans: &[Span]) -> (SharedString, Vec<Span>) {
    let mut out = String::with_capacity(line.len());
    // Where each byte of `line` (and its end) lands in `out`.
    let mut at = Vec::with_capacity(line.len() + 1);
    for (i, c) in line.char_indices() {
        at.resize(i + 1, out.len());
        if c == '\t' {
            out.push_str("    ");
        } else {
            out.push(c);
        }
    }
    at.resize(line.len() + 1, out.len());
    let spans = spans
        .iter()
        .filter(|s| s.range.end <= line.len())
        .map(|s| Span {
            range: at[s.range.start]..at[s.range.end],
            ..s.clone()
        })
        .collect();
    if out.is_empty() {
        out.push(' ');
    }
    (out.into(), spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_widen_and_the_colours_follow() {
        let span = |r: std::ops::Range<usize>| Span {
            range: r,
            color: 1,
            italic: false,
            bold: false,
        };
        let (text, spans) = widen("\tfn x", &[span(1..3)]);
        assert_eq!(text.as_ref(), "    fn x");
        assert_eq!(spans[0].range, 4..6);
        assert_eq!(widen("", &[]).0.as_ref(), " ");
    }

    #[test]
    fn a_note_follows_its_text_and_is_gone_with_it() {
        let lines: Vec<String> = ["a", "fun round()", "b", "fun round()"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Still where it was written.
        assert_eq!(place_note(&lines, 2, "fun round()"), Some(1));
        // Lines came above it: the nearest line with its text.
        assert_eq!(place_note(&lines, 3, "fun round()"), Some(1));
        assert_eq!(place_note(&lines, 4, "fun round()"), Some(3));
        // Its text is gone: outdated.
        assert_eq!(place_note(&lines, 2, "fun round(mode)"), None);
    }
}
