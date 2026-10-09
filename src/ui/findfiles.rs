//! Search in all files of the diff (⌘⌥F): what the IDE calls Find in Files, but over the lines of
//! this diff. Results are grouped by file; ↵ opens one and leaves the match marked in the file.
//! Designed in `../Design/mockups/ide-ideas/a-ideas.html` (3).

use super::input::{self, Edit};
use super::{ISLAND_RADIUS, ROW_RADIUS, Workspace, plural, theme};
use crate::git::LineKind;
use gpui::{
    ClickEvent, Context, FontWeight, HighlightStyle, KeyDownEvent, MouseButton, ScrollHandle,
    StyledText, div, prelude::*, px,
};
use std::ops::Range;

/// No more results than a person can read.
const MAX_HITS: usize = 400;

struct Hit {
    file: usize,
    line: u32,
    old: bool,
    text: String,
    range: Range<usize>,
}

pub struct FindFiles {
    query: String,
    all: bool,
    at: usize,
    case: bool,
    word: bool,
    hits: Vec<Hit>,
    files_with_hits: usize,
    capped: bool,
    scroll: ScrollHandle,
}

/// Where `needle` first occurs in `hay` (a byte range), under the toggles.
fn find_in(hay: &str, needle: &str, case: bool, word: bool) -> Option<Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    let lowered;
    let (hay_cmp, needle_cmp): (&str, String) = if case || !hay.is_ascii() {
        (hay, needle.to_owned())
    } else {
        lowered = hay.to_ascii_lowercase();
        (lowered.as_str(), needle.to_ascii_lowercase())
    };
    let mut from = 0;
    while let Some(at) = hay_cmp[from..].find(&needle_cmp) {
        let start = from + at;
        let end = start + needle_cmp.len();
        let boundary = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if !word
            || (boundary(hay[..start].chars().next_back()) && boundary(hay[end..].chars().next()))
        {
            return Some(start..end);
        }
        from = end;
    }
    None
}

impl Workspace {
    pub(super) fn open_find_files(&mut self, cx: &mut Context<Self>) {
        if self.diff.is_none() || self.settings_open {
            return;
        }
        self.palette = None;
        self.field = None;
        self.field_all = false;
        let query = self.dfind.clone().unwrap_or_default();
        self.ffind = Some(FindFiles {
            query,
            all: false,
            at: 0,
            case: false,
            word: false,
            hits: Vec::new(),
            files_with_hits: 0,
            capped: false,
            scroll: ScrollHandle::new(),
        });
        self.refresh_ffind();
        cx.notify();
    }

    pub(super) fn close_find_files(&mut self, cx: &mut Context<Self>) {
        if self.ffind.take().is_some() {
            cx.notify();
        }
    }

    fn refresh_ffind(&mut self) {
        let (Some(f), Some(diff)) = (&self.ffind, &self.diff) else {
            return;
        };
        let (query, case, word) = (f.query.clone(), f.case, f.word);
        let mut hits = Vec::new();
        let mut files_with_hits = 0;
        let mut capped = false;
        if !query.is_empty() {
            'files: for (file_ix, file) in diff.files.iter().enumerate() {
                let new_lines: Vec<&str> = file
                    .new_text
                    .as_deref()
                    .unwrap_or_default()
                    .lines()
                    .collect();
                let old_lines: Vec<&str> = file
                    .old_text
                    .as_deref()
                    .unwrap_or_default()
                    .lines()
                    .collect();
                let before = hits.len();
                for hunk in &file.hunks {
                    for line in &hunk.lines {
                        let (old, n) = match (line.kind, line.new_line, line.old_line) {
                            (LineKind::Removed, _, Some(o)) => (true, o),
                            (_, Some(n), _) => (false, n),
                            _ => continue,
                        };
                        let Some(text) = (if old { &old_lines } else { &new_lines })
                            .get(n.saturating_sub(1) as usize)
                        else {
                            continue;
                        };
                        if let Some(range) = find_in(text, &query, case, word) {
                            if hits.len() >= MAX_HITS {
                                capped = true;
                                break 'files;
                            }
                            hits.push(Hit {
                                file: file_ix,
                                line: n,
                                old,
                                text: (*text).to_owned(),
                                range,
                            });
                        }
                    }
                }
                if hits.len() > before {
                    files_with_hits += 1;
                }
            }
        }
        if let Some(f) = &mut self.ffind {
            f.at = f.at.min(hits.len().saturating_sub(1));
            f.hits = hits;
            f.files_with_hits = files_with_hits;
            f.capped = capped;
        }
    }

    pub(super) fn ffind_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let Some(f) = self.ffind.as_mut() else {
            return;
        };
        if key.modifiers.alt && matches!(key.key.as_str(), "c" | "w") {
            if key.key == "c" {
                f.case = !f.case;
            } else {
                f.word = !f.word;
            }
            self.refresh_ffind();
            return cx.notify();
        }
        match input::edit(&mut f.query, &mut f.all, key, cx) {
            Edit::Changed => {
                f.at = 0;
                self.refresh_ffind();
            }
            Edit::Enter { .. } => {
                let at = f.at;
                return self.open_hit(at, cx);
            }
            Edit::Escape => {
                self.ffind = None;
            }
            Edit::Selected | Edit::Ignored => {}
        }
        cx.notify();
    }

    pub(super) fn ffind_step(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(f) = self.ffind.as_mut() else {
            return;
        };
        if f.hits.is_empty() {
            return;
        }
        let n = f.hits.len();
        f.at = if down {
            (f.at + 1) % n
        } else {
            (f.at + n - 1) % n
        };
        // The list holds a heading per file as well as the hits.
        let row = f.at
            + f.hits[..=f.at]
                .iter()
                .map(|h| h.file)
                .collect::<std::collections::BTreeSet<_>>()
                .len();
        f.scroll.scroll_to_item(row.saturating_sub(1));
        cx.notify();
    }

    fn open_hit(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(f) = self.ffind.take() else {
            return;
        };
        let Some(hit) = f.hits.get(at) else {
            self.ffind = Some(f);
            return;
        };
        let path = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(hit.file))
            .map(|file| file.path().to_owned());
        if let Some(path) = path {
            self.set_dfind(f.query.clone(), cx);
            self.jump_to_line(&path, hit.line, hit.old, cx);
        }
        cx.notify();
    }

    pub(super) fn render_find_files(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let f = self.ffind.as_ref()?;
        let diff = self.diff.as_ref()?;
        let toggle = |id: &'static str, label: &'static str, on: bool, tip: &'static str| {
            div()
                .id(id)
                .px(px(6.))
                .h(px(22.))
                .flex()
                .items_center()
                .rounded(px(ROW_RADIUS))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if on { theme::text() } else { theme::muted() })
                .when(on, |s| s.bg(theme::selected()))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .tooltip(move |_, cx| cx.new(|_| super::Tip(tip)).into())
                .child(label)
        };
        let mut rows = Vec::new();
        let mut last_file = usize::MAX;
        for (ix, hit) in f.hits.iter().enumerate() {
            if hit.file != last_file {
                last_file = hit.file;
                let count = f.hits.iter().filter(|h| h.file == hit.file).count();
                rows.push(
                    div()
                        .px(px(14.))
                        .pt(px(8.))
                        .pb(px(2.))
                        .flex()
                        .gap_2()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::faint())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(diff.files[hit.file].path().to_owned()),
                        )
                        .child(count.to_string())
                        .into_any_element(),
                );
            }
            let trimmed = hit.text.trim_start();
            let skip = hit.text.len() - trimmed.len();
            let range = hit.range.start.saturating_sub(skip)..hit.range.end.saturating_sub(skip);
            let styled = StyledText::new(trimmed.to_owned()).with_highlights([(
                range,
                HighlightStyle {
                    color: Some(theme::warning().into()),
                    background_color: Some(theme::warning_bg().into()),
                    ..Default::default()
                },
            )]);
            rows.push(
                super::row(("ffind-hit", ix), ix == f.at)
                    .h(px(26.))
                    .gap_3()
                    .child(
                        div()
                            .w(px(46.))
                            .flex_none()
                            .text_align(gpui::TextAlign::Right)
                            .font_family(theme::CODE_FONT)
                            .text_size(px(11.))
                            .text_color(if hit.old {
                                theme::removed()
                            } else {
                                theme::muted()
                            })
                            .child(format!("{}{}", if hit.old { "−" } else { "" }, hit.line)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(theme::CODE_FONT)
                            .text_size(px(12.))
                            .child(styled),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_hit(ix, cx)))
                    .into_any_element(),
            );
        }
        let summary = if f.query.is_empty() {
            "Type to search the lines of this diff.".to_owned()
        } else if f.hits.is_empty() {
            "Nothing matches.".to_owned()
        } else {
            format!(
                "{}{} in {}",
                if f.capped { "first " } else { "" },
                plural(f.hits.len(), "result"),
                plural(f.files_with_hits, "file")
            )
        };
        Some(
            div()
                .id("ffind-backdrop")
                .absolute()
                .size_full()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(72.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_find_files(cx)),
                )
                .child(
                    div()
                        .occlude()
                        .w(px(720.))
                        .max_h(px(620.))
                        .flex()
                        .flex_col()
                        .rounded(px(ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .overflow_hidden()
                        .child(super::island_label("Search in all files of this diff"))
                        .child(
                            div()
                                .h(px(40.))
                                .mx(px(10.))
                                .px(px(10.))
                                .mb(px(6.))
                                .flex()
                                .items_center()
                                .gap_2()
                                .rounded(px(ROW_RADIUS))
                                .border_2()
                                .border_color(theme::focus())
                                .bg(theme::base())
                                .child(div().text_color(theme::faint()).child("⌕"))
                                .child(input::field_text(&f.query, true, f.all, "Search the diff"))
                                .child(
                                    toggle("ffind-case", "Aa", f.case, "Match case  ⌥C").on_click(
                                        cx.listener(|this, _: &ClickEvent, _, cx| {
                                            if let Some(f) = this.ffind.as_mut() {
                                                f.case = !f.case;
                                            }
                                            this.refresh_ffind();
                                            cx.notify();
                                        }),
                                    ),
                                )
                                .child(
                                    toggle("ffind-word", "W", f.word, "Whole word  ⌥W").on_click(
                                        cx.listener(|this, _: &ClickEvent, _, cx| {
                                            if let Some(f) = this.ffind.as_mut() {
                                                f.word = !f.word;
                                            }
                                            this.refresh_ffind();
                                            cx.notify();
                                        }),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .px(px(14.))
                                .pb(px(4.))
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child(summary),
                        )
                        .child(
                            div()
                                .id("ffind-list")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .track_scroll(&f.scroll)
                                .px(px(6.))
                                .pb(px(8.))
                                .children(rows),
                        ),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::find_in;

    #[test]
    fn finds_ignoring_case_by_default() {
        assert_eq!(
            find_in("let Backoff = 1;", "backoff", false, false),
            Some(4..11)
        );
        assert_eq!(find_in("let Backoff = 1;", "backoff", true, false), None);
    }

    #[test]
    fn whole_word_skips_longer_words() {
        assert_eq!(
            find_in("backoff_delay backoff", "backoff", false, true),
            Some(14..21)
        );
        assert_eq!(find_in("backoffs", "backoff", false, true), None);
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert_eq!(find_in("anything", "", false, false), None);
    }
}
