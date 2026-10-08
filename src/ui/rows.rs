//! A file diff laid out as side-by-side rows, with the highlighting of both sides.

use crate::git::{FileDiff, LineKind};
use crate::highlight::{self, Span};
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// 1-based.
    pub line: u32,
    pub kind: LineKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// The gap before a hunk, with where it starts on each side.
    Hunk { old_start: u32, new_start: u32 },
    Lines {
        left: Option<Cell>,
        right: Option<Cell>,
    },
}

/// One side's text, split into lines, with its highlighting.
pub struct Side {
    text: Arc<str>,
    lines: Vec<Range<usize>>,
    spans: Vec<Vec<Span>>,
}

impl Side {
    fn new(path: &str, text: Option<Arc<str>>) -> Self {
        let text = text.unwrap_or_else(|| Arc::from(""));
        let mut lines = Vec::new();
        let mut start = 0;
        for piece in text.split_inclusive('\n') {
            let content = piece.trim_end_matches(['\n', '\r']).len();
            lines.push(start..start + content);
            start += piece.len();
        }
        let spans = highlight::highlight(path, &text);
        Self { text, lines, spans }
    }

    /// The text of a 1-based line and its spans.
    pub fn line(&self, line: u32) -> (&str, &[Span]) {
        let ix = line.saturating_sub(1) as usize;
        let text = self
            .lines
            .get(ix)
            .map(|r| &self.text[r.clone()])
            .unwrap_or_default();
        let spans = self.spans.get(ix).map(Vec::as_slice).unwrap_or_default();
        (text, spans)
    }
}

pub struct FileView {
    pub rows: Vec<Row>,
    pub old: Side,
    pub new: Side,
    /// Digits needed for the widest line number.
    pub gutter_digits: usize,
}

impl FileView {
    pub fn build(file: &FileDiff) -> Self {
        let rows = rows(file);
        let widest = rows
            .iter()
            .filter_map(|row| match row {
                Row::Lines { left, right } => {
                    Some(left.map_or(0, |c| c.line).max(right.map_or(0, |c| c.line)))
                }
                Row::Hunk { .. } => None,
            })
            .max()
            .unwrap_or(0);
        let old_path = file.old_path.as_deref().unwrap_or(file.path());
        Self {
            rows,
            old: Side::new(old_path, file.old_text.clone()),
            new: Side::new(file.path(), file.new_text.clone()),
            gutter_digits: widest.to_string().len().max(3),
        }
    }
}

/// Context lines sit on both sides; a run of removals is paired with the additions after it.
pub fn rows(file: &FileDiff) -> Vec<Row> {
    let mut rows = Vec::new();
    for hunk in &file.hunks {
        rows.push(Row::Hunk {
            old_start: hunk.old_start,
            new_start: hunk.new_start,
        });
        let mut removed = Vec::new();
        let mut added = Vec::new();
        let flush = |rows: &mut Vec<Row>, removed: &mut Vec<Cell>, added: &mut Vec<Cell>| {
            for i in 0..removed.len().max(added.len()) {
                rows.push(Row::Lines {
                    left: removed.get(i).copied(),
                    right: added.get(i).copied(),
                });
            }
            removed.clear();
            added.clear();
        };
        for line in &hunk.lines {
            match (line.kind, line.old_line, line.new_line) {
                (LineKind::Removed, Some(old), _) => removed.push(Cell {
                    line: old,
                    kind: LineKind::Removed,
                }),
                (LineKind::Added, _, Some(new)) => added.push(Cell {
                    line: new,
                    kind: LineKind::Added,
                }),
                (LineKind::Context, Some(old), Some(new)) => {
                    flush(&mut rows, &mut removed, &mut added);
                    rows.push(Row::Lines {
                        left: Some(Cell {
                            line: old,
                            kind: LineKind::Context,
                        }),
                        right: Some(Cell {
                            line: new,
                            kind: LineKind::Context,
                        }),
                    });
                }
                _ => {}
            }
        }
        flush(&mut rows, &mut removed, &mut added);
    }
    rows
}

/// Expands tabs to `width` columns, moving the spans with the text.
pub fn expand_tabs(text: &str, spans: &[Span], width: usize) -> (String, Vec<Span>) {
    if !text.contains('\t') {
        return (text.to_owned(), spans.to_vec());
    }
    let mut out = String::with_capacity(text.len() + 16);
    // `map[i]` is where byte `i` of `text` lands in `out`.
    let mut map = Vec::with_capacity(text.len() + 1);
    let mut column = 0;
    for (i, ch) in text.char_indices() {
        while map.len() < i {
            map.push(out.len());
        }
        map.push(out.len());
        if ch == '\t' {
            let fill = width - column % width;
            out.extend(std::iter::repeat_n(' ', fill));
            column += fill;
        } else {
            out.push(ch);
            column += 1;
        }
    }
    while map.len() <= text.len() {
        map.push(out.len());
    }
    let spans = spans
        .iter()
        .map(|s| Span {
            range: map[s.range.start]..map[s.range.end],
            ..s.clone()
        })
        .collect();
    (out, spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{ChangeKind, DiffLine, Hunk};

    fn line(kind: LineKind, old: Option<u32>, new: Option<u32>) -> DiffLine {
        DiffLine {
            kind,
            old_line: old,
            new_line: new,
        }
    }

    #[test]
    fn removals_pair_with_additions() {
        let file = FileDiff {
            old_path: Some("a".into()),
            new_path: Some("a".into()),
            change: ChangeKind::Modified,
            added: 3,
            removed: 1,
            hunks: vec![Hunk {
                old_start: 1,
                new_start: 1,
                lines: vec![
                    line(LineKind::Context, Some(1), Some(1)),
                    line(LineKind::Removed, Some(2), None),
                    line(LineKind::Added, None, Some(2)),
                    line(LineKind::Added, None, Some(3)),
                    line(LineKind::Context, Some(3), Some(4)),
                    line(LineKind::Added, None, Some(5)),
                ],
            }],
            old_text: None,
            new_text: None,
            note: None,
        };
        let cell = |line, kind| Some(Cell { line, kind });
        assert_eq!(
            rows(&file),
            [
                Row::Hunk {
                    old_start: 1,
                    new_start: 1
                },
                Row::Lines {
                    left: cell(1, LineKind::Context),
                    right: cell(1, LineKind::Context)
                },
                Row::Lines {
                    left: cell(2, LineKind::Removed),
                    right: cell(2, LineKind::Added)
                },
                Row::Lines {
                    left: None,
                    right: cell(3, LineKind::Added)
                },
                Row::Lines {
                    left: cell(3, LineKind::Context),
                    right: cell(4, LineKind::Context)
                },
                Row::Lines {
                    left: None,
                    right: cell(5, LineKind::Added)
                },
            ]
        );
    }

    #[test]
    fn tabs_expand_and_spans_follow() {
        let span = |range| Span {
            range,
            color: 1,
            italic: false,
        };
        let (text, spans) = expand_tabs("\tab\tc", &[span(1..3), span(4..5)], 4);
        assert_eq!(text, "    ab  c");
        assert_eq!(spans, [span(4..6), span(8..9)]);
    }
}
