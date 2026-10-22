//! A file read in Code (#71), opened from search everywhere's Files tab (D18, X53): read only, as
//! of the revision on screen when it was opened (`HEAD` when there is none, the file on disk for
//! the working tree), highlighted, with its line numbers. Open in editor; ⌘W closes it, esc goes
//! up to Code.

use super::{Header, Workspace, format, island, px, theme};
use crate::git::Repo;
use crate::highlight::{self, Span};
use gpui::{
    Context, FontStyle, HighlightStyle, SharedString, StyledText, Task, UniformListScrollHandle,
    div, prelude::*, uniform_list,
};

pub struct FileView {
    pub path: String,
    /// What it was read at: a short id, or the working tree.
    at: String,
    lines: Vec<String>,
    spans: Vec<Vec<Span>>,
    scroll: UniformListScrollHandle,
}

/// The file being read for the view.
pub type Reading = Option<Task<()>>;

impl FileView {
    /// What it was read at: a short id, or the working tree.
    pub fn at(&self) -> &str {
        &self.at
    }
}

impl Workspace {
    /// Shows `path` as it is in the version on screen, as Code's page (#71).
    pub(super) fn open_file_view(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.arrive(super::nav::Page::File(path.clone()));
        self.remember_place(
            super::recents::Key::File(path.clone()),
            path.clone(),
            "file".to_owned(),
        );
        let working = matches!(
            self.diff.as_ref().map(|d| &d.header),
            Some(Header::WorkingTree { .. })
        );
        let rev = self.viewed_rev();
        let file = path.clone();
        self.file_reading = Some(cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    let text: Option<(String, String)> = if working {
                        std::fs::read_to_string(root.join(&file))
                            .ok()
                            .map(|t| (t, "the working tree".to_owned()))
                    } else {
                        let repo = Repo::open(&root).ok()?;
                        let rev = match rev {
                            Some(rev) => rev,
                            None => repo.resolve("HEAD").ok()?,
                        };
                        repo.file_at(rev, &file).map(|t| (t, format::short(rev)))
                    };
                    let (text, at) = text?;
                    let spans = highlight::highlight(&file, &text);
                    let lines = text.lines().map(str::to_owned).collect();
                    Some((lines, spans, at))
                })
                .await;
            this.update(cx, |this, cx| {
                this.file_reading = None;
                match read {
                    Some((lines, spans, at)) => {
                        this.file_view = Some(FileView {
                            path,
                            at,
                            lines,
                            spans,
                            scroll: UniformListScrollHandle::new(),
                        });
                    }
                    None => {
                        this.error =
                            Some(format!("{path} is not text, or not in this version.").into());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub(super) fn render_file_view(&self) -> Option<gpui::AnyElement> {
        let view = self.file_view.as_ref()?;
        let size = self.code_size();
        let row = (size * 1.55).round();
        let digits = view.lines.len().max(1).to_string().len() as f32;
        let gutter = digits * size * 0.62 + 20.;
        let lines: Vec<(SharedString, Vec<Span>)> = view
            .lines
            .iter()
            .enumerate()
            .map(|(n, l)| widen(l, view.spans.get(n).map_or(&[][..], Vec::as_slice)))
            .collect();
        let body = uniform_list("file-view", lines.len(), move |range, _, _| {
            range
                .map(|n| {
                    let (text, spans) = &lines[n];
                    let highlights = spans
                        .iter()
                        .filter(|s| s.range.end <= text.len())
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
                    div()
                        .h(px(row))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .w(px(gutter))
                                .flex_none()
                                .flex()
                                .justify_end()
                                .pr_2()
                                .text_color(theme::muted())
                                .child((n + 1).to_string()),
                        )
                        .child(
                            div()
                                .pl_2()
                                .whitespace_nowrap()
                                .text_color(theme::code_text())
                                .child(StyledText::new(text.clone()).with_highlights(highlights)),
                        )
                })
                .collect()
        })
        .track_scroll(&view.scroll)
        .flex_1()
        .font_family(theme::CODE_FONT)
        .text_size(px(size));
        Some(
            island()
                .flex_1()
                .min_w_0()
                .bg(theme::editor())
                // Its path, revision and Open in are the page bar's (#71).
                .child(div().flex_1().min_h_0().flex().py(px(6.)).child(body))
                .into_any_element(),
        )
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
}
