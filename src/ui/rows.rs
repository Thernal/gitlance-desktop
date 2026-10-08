//! A file diff as rows to draw. Building it (highlighting, difftastic) is slow and runs in the
//! background; laying it out — side by side or unified, with unchanged runs collapsed — is cheap
//! and reruns whenever a view option changes.

use crate::git::{FileDiff, LineKind};
use crate::highlight::{self, Span};
use crate::storage::{DiffMode, ViewOptions};
use crate::structural::{self, Alignment};
use crate::worddiff;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

/// Unchanged lines kept next to a change when the rest of a run is hidden.
const CONTEXT: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// 1-based.
    pub line: u32,
    pub kind: LineKind,
}

impl Cell {
    fn same(line: u32) -> Self {
        Self {
            line,
            kind: LineKind::Context,
        }
    }

    pub fn changed(self) -> bool {
        self.kind != LineKind::Context
    }
}

/// What a file's diff is made of, before it is laid out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    /// `len` unchanged lines, starting at `old` and `new`.
    Same { old: u32, new: u32, len: u32 },
    /// Aligned lines around a change; a side is `None` where it has no counterpart.
    Change(Vec<(Option<Cell>, Option<Cell>)>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// `count` hidden unchanged lines of segment `segment`.
    Gap { segment: usize, count: u32 },
    Split {
        left: Option<Cell>,
        right: Option<Cell>,
    },
    /// The thread of review comment `0` under the line above.
    Thread(u64),
    /// The box for writing a new comment under the line above.
    Composer,
    /// One line of a unified diff; its text is from the old side when `old`.
    Unified {
        cell: Cell,
        old: bool,
        old_line: Option<u32>,
        new_line: Option<u32>,
    },
}

/// One side's text, split into lines, with its highlighting and changed-token marks.
pub struct Side {
    text: Arc<str>,
    lines: Vec<Range<usize>>,
    spans: Vec<Vec<Span>>,
    marks: HashMap<u32, Vec<Range<usize>>>,
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
        Self {
            text,
            lines,
            spans,
            marks: HashMap::new(),
        }
    }

    pub fn count(&self) -> u32 {
        self.lines.len() as u32
    }

    /// The text of a 1-based line, its syntax spans and its changed-token marks.
    pub fn line(&self, line: u32) -> (&str, &[Span], &[Range<usize>]) {
        let ix = line.saturating_sub(1) as usize;
        let text = self
            .lines
            .get(ix)
            .map(|r| &self.text[r.clone()])
            .unwrap_or_default();
        let spans = self.spans.get(ix).map(Vec::as_slice).unwrap_or_default();
        let marks = self.marks.get(&line).map(Vec::as_slice).unwrap_or_default();
        (text, spans, marks)
    }

    fn widest(&self) -> usize {
        self.lines
            .iter()
            .map(|r| {
                let line = &self.text[r.clone()];
                line.chars().count() + line.matches('\t').count() * 3
            })
            .max()
            .unwrap_or(0)
    }
}

pub struct FileData {
    pub old: Side,
    pub new: Side,
    pub segments: Vec<Segment>,
    /// The structural diff's language, or why it is unavailable.
    pub mode_note: Option<String>,
    pub gutter_digits: usize,
    /// The longest line, in columns.
    pub widest: usize,
}

impl FileData {
    pub fn build(file: &FileDiff, mode: DiffMode) -> Self {
        let structural = mode == DiffMode::Structural;
        let old_path = file.old_path.as_deref().unwrap_or(file.path());
        let mut old = Side::new(old_path, file.old_text.clone());
        let mut new = Side::new(file.path(), file.new_text.clone());
        let mut mode_note = None;
        let mut segments = Vec::new();
        if !file.hunks.is_empty() {
            let aligned = structural
                .then(|| {
                    let old_text = file.old_text.as_deref().unwrap_or_default();
                    let new_text = file.new_text.as_deref().unwrap_or_default();
                    structural::diff(file.path(), old_text, new_text)
                })
                .transpose();
            segments = match aligned {
                // A created or deleted file: every line changed, which the line diff shows as well.
                Ok(Some(Some(alignment))) if alignment.pairs.is_empty() => {
                    mode_note = Some(format!("{} · structural", alignment.language));
                    line_segments(file, old.count())
                }
                Ok(Some(Some(alignment))) => {
                    mode_note = Some(format!("{} · structural", alignment.language));
                    let segments = structural_segments(&alignment);
                    old.marks = alignment.old_marks;
                    new.marks = alignment.new_marks;
                    segments
                }
                Ok(Some(None)) => {
                    mode_note =
                        Some("Structural diff needs difftastic: brew install difftastic".into());
                    line_segments(file, old.count())
                }
                Err(err) => {
                    mode_note = Some(format!("Structural diff failed: {err:#}"));
                    line_segments(file, old.count())
                }
                Ok(None) => line_segments(file, old.count()),
            };
        }
        if mode == DiffMode::Words {
            word_marks(&mut old, &mut new, &segments);
        }
        let gutter_digits = old.count().max(new.count()).to_string().len().max(3);
        let widest = old.widest().max(new.widest());
        Self {
            old,
            new,
            segments,
            mode_note,
            gutter_digits,
            widest,
        }
    }

    pub fn side(&self, old: bool) -> &Side {
        if old { &self.old } else { &self.new }
    }
}

/// Marks the words that changed in each removed line and the added line it is paired with.
fn word_marks(old: &mut Side, new: &mut Side, segments: &[Segment]) {
    for segment in segments {
        let Segment::Change(pairs) = segment else {
            continue;
        };
        for &(left, right) in pairs {
            let (Some(left), Some(right)) = (left, right) else {
                continue;
            };
            if !left.changed() || !right.changed() {
                continue;
            }
            let (a, b) = (old.line(left.line).0, new.line(right.line).0);
            if let Some((a, b)) = worddiff::marks(a, b) {
                old.marks.insert(left.line, a);
                new.marks.insert(right.line, b);
            }
        }
    }
}

/// git's hunks as segments: unchanged runs between them, removals paired with the additions after.
fn line_segments(file: &FileDiff, old_count: u32) -> Vec<Segment> {
    let mut segments = Vec::new();
    // The next line not yet placed, on each side.
    let (mut old, mut new) = (1u32, 1u32);
    let mut removed = Vec::new();
    let mut added = Vec::new();

    fn same(segments: &mut Vec<Segment>, old: u32, new: u32, len: u32) {
        if len == 0 {
            return;
        }
        if let Some(Segment::Same {
            old: o,
            new: n,
            len: l,
        }) = segments.last_mut()
            && *o + *l == old
            && *n + *l == new
        {
            *l += len;
            return;
        }
        segments.push(Segment::Same { old, new, len });
    }
    fn flush(segments: &mut Vec<Segment>, removed: &mut Vec<Cell>, added: &mut Vec<Cell>) {
        if removed.is_empty() && added.is_empty() {
            return;
        }
        let pairs = (0..removed.len().max(added.len()))
            .map(|i| (removed.get(i).copied(), added.get(i).copied()))
            .collect();
        segments.push(Segment::Change(pairs));
        removed.clear();
        added.clear();
    }

    for hunk in &file.hunks {
        for line in &hunk.lines {
            match (line.kind, line.old_line, line.new_line) {
                (LineKind::Context, Some(a), Some(b)) => {
                    flush(&mut segments, &mut removed, &mut added);
                    same(&mut segments, old, new, a - old);
                    same(&mut segments, a, b, 1);
                    (old, new) = (a + 1, b + 1);
                }
                (LineKind::Removed, Some(a), _) => {
                    if removed.is_empty() && added.is_empty() {
                        same(&mut segments, old, new, a - old);
                        new += a - old;
                    }
                    removed.push(Cell {
                        line: a,
                        kind: LineKind::Removed,
                    });
                    old = a + 1;
                }
                (LineKind::Added, _, Some(b)) => {
                    if removed.is_empty() && added.is_empty() {
                        same(&mut segments, old, new, b - new);
                        old += b - new;
                    }
                    added.push(Cell {
                        line: b,
                        kind: LineKind::Added,
                    });
                    new = b + 1;
                }
                _ => {}
            }
        }
        flush(&mut segments, &mut removed, &mut added);
    }
    same(&mut segments, old, new, (old_count + 1).saturating_sub(old));
    segments
}

/// difftastic's alignment as segments: a line is changed when it has token marks or no
/// counterpart; aligned lines without marks are unchanged, even if reformatted.
fn structural_segments(alignment: &Alignment) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    for &(a, b) in &alignment.pairs {
        let old_marked = a.is_some_and(|a| alignment.old_marks.contains_key(&a));
        let new_marked = b.is_some_and(|b| alignment.new_marks.contains_key(&b));
        if let (Some(a), Some(b), false, false) = (a, b, old_marked, new_marked) {
            match segments.last_mut() {
                Some(Segment::Same { old, new, len }) if *old + *len == a && *new + *len == b => {
                    *len += 1
                }
                _ => segments.push(Segment::Same {
                    old: a,
                    new: b,
                    len: 1,
                }),
            }
            continue;
        }
        let pair = (
            a.map(|line| Cell {
                line,
                kind: if old_marked {
                    LineKind::Removed
                } else {
                    LineKind::Context
                },
            }),
            b.map(|line| Cell {
                line,
                kind: if new_marked {
                    LineKind::Added
                } else {
                    LineKind::Context
                },
            }),
        );
        match segments.last_mut() {
            Some(Segment::Change(pairs)) => pairs.push(pair),
            _ => segments.push(Segment::Change(vec![pair])),
        }
    }
    segments
}

/// The rows for `options`; `expanded` holds segments whose hidden lines were asked for.
pub fn layout(data: &FileData, options: &ViewOptions, expanded: &HashSet<usize>) -> Vec<Row> {
    let mut rows = Vec::new();
    let last = data.segments.len().saturating_sub(1);
    for (ix, segment) in data.segments.iter().enumerate() {
        match *segment {
            Segment::Same { old, new, len } => {
                let lead = if ix == 0 { 0 } else { CONTEXT.min(len) };
                let trail = if ix == last { 0 } else { CONTEXT.min(len) };
                let show_all =
                    options.full_context || expanded.contains(&ix) || lead + trail >= len;
                let same = |i: u32| same_row(old + i, new + i, options.unified);
                if show_all {
                    rows.extend((0..len).map(same));
                } else {
                    rows.extend((0..lead).map(same));
                    rows.push(Row::Gap {
                        segment: ix,
                        count: len - lead - trail,
                    });
                    rows.extend((len - trail..len).map(same));
                }
            }
            Segment::Change(ref pairs) if !options.unified => {
                rows.extend(
                    pairs
                        .iter()
                        .map(|&(left, right)| Row::Split { left, right }),
                );
            }
            Segment::Change(ref pairs) => unified_change(pairs, &mut rows),
        }
    }
    rows
}

fn same_row(old: u32, new: u32, unified: bool) -> Row {
    if unified {
        Row::Unified {
            cell: Cell::same(new),
            old: false,
            old_line: Some(old),
            new_line: Some(new),
        }
    } else {
        Row::Split {
            left: Some(Cell::same(old)),
            right: Some(Cell::same(new)),
        }
    }
}

/// Removals above additions; unmarked lines inside a structural change stay in place.
fn unified_change(pairs: &[(Option<Cell>, Option<Cell>)], rows: &mut Vec<Row>) {
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let flush = |rows: &mut Vec<Row>, removed: &mut Vec<Cell>, added: &mut Vec<Cell>| {
        rows.extend(removed.drain(..).map(|cell| Row::Unified {
            cell,
            old: true,
            old_line: Some(cell.line),
            new_line: None,
        }));
        rows.extend(added.drain(..).map(|cell| Row::Unified {
            cell,
            old: false,
            old_line: None,
            new_line: Some(cell.line),
        }));
    };
    for &(left, right) in pairs {
        if left.is_some_and(Cell::changed) || right.is_some_and(Cell::changed) {
            removed.extend(left);
            added.extend(right);
            continue;
        }
        flush(rows, &mut removed, &mut added);
        rows.push(match (left, right) {
            (left, Some(right)) => Row::Unified {
                cell: right,
                old: false,
                old_line: left.map(|c| c.line),
                new_line: Some(right.line),
            },
            (Some(left), None) => Row::Unified {
                cell: left,
                old: true,
                old_line: Some(left.line),
                new_line: None,
            },
            (None, None) => continue,
        });
    }
    flush(rows, &mut removed, &mut added);
}

/// A stretch of a line with one style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub range: Range<usize>,
    pub color: Option<u32>,
    pub italic: bool,
    /// Inside a changed token.
    pub marked: bool,
    /// Inside a word the search is looking for.
    pub found: bool,
    /// Inside the selected text.
    pub selected: bool,
}

/// Syntax spans and changed-token marks folded into non-overlapping runs over `text`.
pub fn runs(
    text: &str,
    spans: &[Span],
    marks: &[Range<usize>],
    found: &[Range<usize>],
    selected: Option<Range<usize>>,
) -> Vec<Run> {
    let len = text.len();
    let valid = |r: &Range<usize>| r.start < r.end && r.end <= len;
    let marks: Vec<&Range<usize>> = marks
        .iter()
        .filter(|r| valid(r) && text.is_char_boundary(r.start) && text.is_char_boundary(r.end))
        .collect();
    let mut cuts: Vec<usize> = spans
        .iter()
        .filter(|s| valid(&s.range))
        .flat_map(|s| [s.range.start, s.range.end])
        .chain(marks.iter().flat_map(|r| [r.start, r.end]))
        .chain(
            found
                .iter()
                .filter(|r| valid(r))
                .flat_map(|r| [r.start, r.end]),
        )
        .chain(
            selected
                .iter()
                .filter(|r| valid(r))
                .flat_map(|r| [r.start, r.end]),
        )
        .chain([0, len])
        .collect();
    cuts.sort_unstable();
    cuts.dedup();

    let mut out: Vec<Run> = Vec::new();
    for window in cuts.windows(2) {
        let (start, end) = (window[0], window[1]);
        let span = spans
            .iter()
            .find(|s| s.range.start <= start && end <= s.range.end);
        let run = Run {
            range: start..end,
            color: span.map(|s| s.color),
            italic: span.is_some_and(|s| s.italic),
            marked: marks.iter().any(|r| r.start <= start && end <= r.end),
            found: found.iter().any(|r| r.start <= start && end <= r.end),
            selected: selected
                .as_ref()
                .is_some_and(|r| r.start <= start && end <= r.end),
        };
        if run.color.is_none() && !run.italic && !run.marked && !run.found && !run.selected {
            continue;
        }
        match out.last_mut() {
            Some(last)
                if last.range.end == start
                    && (
                        last.color,
                        last.italic,
                        last.marked,
                        last.found,
                        last.selected,
                    ) == (run.color, run.italic, run.marked, run.found, run.selected) =>
            {
                last.range.end = end
            }
            _ => out.push(run),
        }
    }
    out
}

/// Expands tabs to `width` columns; `map[i]` is where byte `i` of `text` lands in the result.
pub fn expand_tabs(text: &str, width: usize) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(text.len());
    let mut map = Vec::with_capacity(text.len() + 1);
    let mut column = 0;
    for (i, ch) in text.char_indices() {
        while map.len() <= i {
            map.push(out.len());
        }
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
    (out, map)
}

/// Whether a row shows a changed line (on either side).
pub fn is_change(row: &Row) -> bool {
    match *row {
        Row::Split { left, right } => {
            left.is_some_and(Cell::changed) || right.is_some_and(Cell::changed)
        }
        Row::Unified { cell, .. } => cell.changed(),
        _ => false,
    }
}

/// The rows whose text contains any of `words` (case-insensitive), for stepping through matches.
pub fn matching_rows(data: &FileData, rows: &[Row], words: &[String]) -> Vec<usize> {
    if words.is_empty() {
        return Vec::new();
    }
    let hit = |old: bool, cell: Option<Cell>| {
        cell.is_some_and(|cell| {
            let text = data.side(old).line(cell.line).0.to_lowercase();
            words.iter().any(|w| text.contains(w.as_str()))
        })
    };
    rows.iter()
        .enumerate()
        .filter(|(_, row)| match **row {
            Row::Gap { .. } | Row::Thread(_) | Row::Composer => false,
            Row::Split { left, right } => hit(true, left) || hit(false, right),
            Row::Unified { cell, old, .. } => hit(old, Some(cell)),
        })
        .map(|(ix, _)| ix)
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::single_range_in_vec_init,
    reason = "lists of one byte range are what the code under test takes"
)]
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

    fn cell(line: u32, kind: LineKind) -> Option<Cell> {
        Some(Cell { line, kind })
    }

    /// 20 old lines; line 10 replaced by two lines, nothing else changed.
    fn file() -> FileDiff {
        let old: String = (1..=20).map(|i| format!("{i}\n")).collect();
        let lines = [
            line(LineKind::Context, Some(7), Some(7)),
            line(LineKind::Context, Some(8), Some(8)),
            line(LineKind::Context, Some(9), Some(9)),
            line(LineKind::Removed, Some(10), None),
            line(LineKind::Added, None, Some(10)),
            line(LineKind::Added, None, Some(11)),
            line(LineKind::Context, Some(11), Some(12)),
            line(LineKind::Context, Some(12), Some(13)),
            line(LineKind::Context, Some(13), Some(14)),
        ];
        FileDiff {
            old_path: Some("a.txt".into()),
            new_path: Some("a.txt".into()),
            change: ChangeKind::Modified,
            added: 2,
            removed: 1,
            hunks: vec![Hunk {
                lines: lines.to_vec(),
            }],
            old_text: Some(old.into()),
            new_text: None,
            note: None,
        }
    }

    #[test]
    fn line_segments_cover_the_whole_file() {
        let segments = line_segments(&file(), 20);
        assert_eq!(
            segments,
            [
                Segment::Same {
                    old: 1,
                    new: 1,
                    len: 9
                },
                Segment::Change(vec![
                    (cell(10, LineKind::Removed), cell(10, LineKind::Added)),
                    (None, cell(11, LineKind::Added)),
                ]),
                Segment::Same {
                    old: 11,
                    new: 12,
                    len: 10
                },
            ]
        );
    }

    fn data(segments: Vec<Segment>) -> FileData {
        FileData {
            old: Side::new("a.txt", None),
            new: Side::new("a.txt", None),
            segments,
            mode_note: None,
            gutter_digits: 3,
            widest: 0,
        }
    }

    #[test]
    fn unchanged_runs_collapse_unless_expanded() {
        let data = data(line_segments(&file(), 20));
        let split = ViewOptions::default();
        let rows = layout(&data, &split, &HashSet::new());
        // 6 hidden + 3 context, the change, 3 context + 7 hidden.
        assert_eq!(
            rows[0],
            Row::Gap {
                segment: 0,
                count: 6
            }
        );
        assert_eq!(rows.len(), 1 + 3 + 2 + 3 + 1);
        assert_eq!(
            rows.last(),
            Some(&Row::Gap {
                segment: 2,
                count: 7
            })
        );

        let expanded = layout(&data, &split, &HashSet::from([0]));
        assert_eq!(expanded.len(), 9 + 2 + 3 + 1);

        let full = ViewOptions {
            full_context: true,
            ..split
        };
        assert!(
            layout(&data, &full, &HashSet::new())
                .iter()
                .all(|r| !matches!(r, Row::Gap { .. }))
        );
    }

    #[test]
    fn a_new_file_shows_in_every_mode() {
        let lines: Vec<DiffLine> = (1..=3)
            .map(|n| line(LineKind::Added, None, Some(n)))
            .collect();
        let file = FileDiff {
            old_path: None,
            new_path: Some("new.rs".into()),
            change: ChangeKind::Added,
            added: 3,
            removed: 0,
            hunks: vec![Hunk { lines }],
            old_text: None,
            new_text: Some("fn a() {}\nfn b() {}\nfn c() {}\n".into()),
            note: None,
        };
        for mode in DiffMode::ALL {
            let data = FileData::build(&file, mode);
            for unified in [false, true] {
                let options = ViewOptions {
                    unified,
                    mode,
                    ..ViewOptions::default()
                };
                let rows = layout(&data, &options, &HashSet::new());
                assert_eq!(rows.len(), 3, "mode={mode:?} unified={unified}");
            }
        }
    }

    #[test]
    fn words_mode_marks_changed_words_only_in_paired_lines() {
        let file = FileDiff {
            old_path: Some("a.txt".into()),
            new_path: Some("a.txt".into()),
            change: ChangeKind::Modified,
            added: 2,
            removed: 1,
            hunks: vec![Hunk {
                lines: vec![
                    line(LineKind::Removed, Some(1), None),
                    line(LineKind::Added, None, Some(1)),
                    line(LineKind::Added, None, Some(2)),
                ],
            }],
            old_text: Some("if a > b {\n".into()),
            new_text: Some("if a >= b {\nbrand new line\n".into()),
            note: None,
        };
        let lines = FileData::build(&file, DiffMode::Lines);
        assert!(lines.old.marks.is_empty() && lines.new.marks.is_empty());
        let words = FileData::build(&file, DiffMode::Words);
        assert_eq!(words.old.marks.get(&1), Some(&vec![5..6]));
        assert_eq!(words.new.marks.get(&1), Some(&vec![5..7]));
        assert!(!words.new.marks.contains_key(&2));
    }

    #[test]
    fn unified_puts_removals_above_additions() {
        let data = data(vec![Segment::Change(vec![
            (cell(10, LineKind::Removed), cell(10, LineKind::Added)),
            (cell(11, LineKind::Removed), cell(11, LineKind::Added)),
        ])]);
        let unified = ViewOptions {
            unified: true,
            ..ViewOptions::default()
        };
        let olds: Vec<bool> = layout(&data, &unified, &HashSet::new())
            .iter()
            .map(|r| matches!(r, Row::Unified { old: true, .. }))
            .collect();
        assert_eq!(olds, [true, true, false, false]);
    }

    #[test]
    fn structural_pairs_without_marks_are_unchanged() {
        let alignment = Alignment {
            pairs: vec![
                (Some(1), Some(1)),
                (Some(2), Some(2)),
                (None, Some(3)),
                (Some(3), Some(4)),
            ],
            old_marks: HashMap::from([(2, vec![0..1])]),
            new_marks: HashMap::from([(2, vec![0..1])]),
            language: "Rust".into(),
            status: "changed".into(),
        };
        assert_eq!(
            structural_segments(&alignment),
            [
                Segment::Same {
                    old: 1,
                    new: 1,
                    len: 1
                },
                Segment::Change(vec![
                    (cell(2, LineKind::Removed), cell(2, LineKind::Added)),
                    (None, cell(3, LineKind::Context)),
                ]),
                Segment::Same {
                    old: 3,
                    new: 4,
                    len: 1
                },
            ]
        );
    }

    #[test]
    fn runs_split_spans_at_marks() {
        let span = |range, color| Span {
            range,
            color,
            italic: false,
        };
        let runs = runs(
            "let x = 10;",
            &[span(0..3, 1), span(8..10, 2)],
            &[4..10],
            &[],
            None,
        );
        let shape: Vec<_> = runs
            .iter()
            .map(|r| (r.range.clone(), r.color, r.marked))
            .collect();
        assert_eq!(
            shape,
            [
                (0..3, Some(1), false),
                (4..8, None, true),
                (8..10, Some(2), true)
            ]
        );
    }

    #[test]
    fn found_words_split_runs_and_rows_are_listed() {
        let runs = runs("let x = 10;", &[], &[], &[4..5], None);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].found && runs[0].range == (4..5));

        let file = FileDiff {
            old_path: Some("a.txt".into()),
            new_path: Some("a.txt".into()),
            change: ChangeKind::Modified,
            added: 1,
            removed: 1,
            hunks: vec![Hunk {
                lines: vec![
                    line(LineKind::Removed, Some(1), None),
                    line(LineKind::Added, None, Some(1)),
                ],
            }],
            old_text: Some("hello old\n".into()),
            new_text: Some("Hello NEW\n".into()),
            note: None,
        };
        let data = FileData::build(&file, DiffMode::Lines);
        let rows = layout(&data, &ViewOptions::default(), &HashSet::new());
        assert_eq!(matching_rows(&data, &rows, &["new".into()]), [0]);
        assert!(matching_rows(&data, &rows, &["zzz".into()]).is_empty());
        assert!(matching_rows(&data, &rows, &[]).is_empty());
    }

    #[test]
    fn tabs_expand_with_a_byte_map() {
        let (text, map) = expand_tabs("\tab\tc", 4);
        assert_eq!(text, "    ab  c");
        assert_eq!((map[1], map[3], map[4], map[5]), (4, 6, 8, 9));
    }
}
