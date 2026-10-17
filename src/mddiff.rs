//! A changed markdown document read block by block (#11, Design `docs/specs/markdown-viewer.md`):
//! the blocks both versions share stay, a block edited in place is *changed* — its words marked as
//! in Words mode, a list's items one by one — and the rest are added or removed. The rendered page
//! draws the rows: Unified the new version with the changes in it, Split the two side by side.

use crate::markdown::{self, Block, Inline, Item, Mark, Span};
use crate::worddiff::{self, Piece};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Same,
    Added,
    Removed,
    Changed,
}

/// A block of the old version, of the new one, or both, with its source lines (first, last).
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub change: Change,
    /// The old block, its removed words marked.
    pub old: Option<(Block, (u32, u32))>,
    /// The new block, its added words marked.
    pub new: Option<(Block, (u32, u32))>,
    /// A changed block read as one: the new block with the removed words in it (Unified).
    pub merged: Option<Block>,
}

/// The rows of a document changed from `old` to `new`.
pub fn diff(old: &str, new: &str) -> Vec<Row> {
    let (a, b) = (markdown::parse_lines(old), markdown::parse_lines(new));
    pair(&a, &b, |x, y| x.0 == y.0, |x, y| like(&x.0, &y.0))
        .into_iter()
        .map(|(change, i, j)| {
            let (old, new) = (i.map(|i| a[i].clone()), j.map(|j| b[j].clone()));
            match (change, old, new) {
                (Change::Changed, Some((x, xl)), Some((y, yl))) => {
                    let (x, y, merged) = mark(&x, &y);
                    Row {
                        change,
                        old: Some((x, xl)),
                        new: Some((y, yl)),
                        merged: Some(merged),
                    }
                }
                (change, old, new) => Row {
                    change,
                    old,
                    new,
                    merged: None,
                },
            }
        })
        .collect()
}

/// Two sequences paired: the longest run of `same` ones kept, then between them a `like` pair is
/// changed (in order) and the rest removed or added — removed first. (change, old, new) indices.
fn pair<T>(
    old: &[T],
    new: &[T],
    same: impl Fn(&T, &T) -> bool,
    like: impl Fn(&T, &T) -> bool,
) -> Vec<(Change, Option<usize>, Option<usize>)> {
    let head = old.iter().zip(new).take_while(|(a, b)| same(a, b)).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| same(a, b))
        .count();
    let mut matches: Vec<(usize, usize)> = (0..head).map(|k| (k, k)).collect();
    let (a, b) = (&old[head..old.len() - tail], &new[head..new.len() - tail]);
    let (n, m) = (a.len(), b.len());
    // A document too large to compare block by block reads as rewritten between its ends.
    if n > 0 && m > 0 && n * m <= 1_000_000 {
        let at = |i: usize, j: usize| i * (m + 1) + j;
        let mut lcs = vec![0u32; (n + 1) * (m + 1)];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[at(i, j)] = if same(&a[i], &b[j]) {
                    lcs[at(i + 1, j + 1)] + 1
                } else {
                    lcs[at(i + 1, j)].max(lcs[at(i, j + 1)])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if same(&a[i], &b[j]) {
                matches.push((head + i, head + j));
                i += 1;
                j += 1;
            } else if lcs[at(i + 1, j)] >= lcs[at(i, j + 1)] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    matches.extend((0..tail).map(|k| (old.len() - tail + k, new.len() - tail + k)));
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    for (mi, mj) in matches.into_iter().chain([(old.len(), new.len())]) {
        gap(&old[i..mi], &new[j..mj], (i, j), &like, &mut out);
        if mi < old.len() {
            out.push((Change::Same, Some(mi), Some(mj)));
        }
        (i, j) = (mi + 1, mj + 1);
    }
    out
}

/// What lies between two kept pairs: each old one changed into the first like new one after the
/// last pairing, the rest removed or added.
fn gap<T>(
    a: &[T],
    b: &[T],
    (i0, j0): (usize, usize),
    like: &impl Fn(&T, &T) -> bool,
    out: &mut Vec<(Change, Option<usize>, Option<usize>)>,
) {
    let mut paired = vec![false; a.len()];
    let mut taken = vec![false; b.len()];
    let mut from = 0;
    for (x, ax) in a.iter().enumerate() {
        if let Some(y) = (from..b.len()).find(|&y| like(ax, &b[y])) {
            paired[x] = true;
            taken[y] = true;
            from = y + 1;
        }
    }
    let (mut x, mut y) = (0, 0);
    while x < a.len() || y < b.len() {
        if x < a.len() && !paired[x] {
            out.push((Change::Removed, Some(i0 + x), None));
            x += 1;
        } else if y < b.len() && !taken[y] {
            out.push((Change::Added, None, Some(j0 + y)));
            y += 1;
        } else {
            out.push((Change::Changed, Some(i0 + x), Some(j0 + y)));
            x += 1;
            y += 1;
        }
    }
}

/// Whether `new` is `old` edited rather than replaced: text that keeps most of its words, or a
/// block of the same kind.
fn like(old: &Block, new: &Block) -> bool {
    match (old, new) {
        (Block::Paragraph(x), Block::Paragraph(y)) => worddiff::marks(&x.text, &y.text).is_some(),
        (Block::Heading(l, x), Block::Heading(m, y)) => {
            l == m && worddiff::marks(&x.text, &y.text).is_some()
        }
        (Block::Code { lang: x, .. }, Block::Code { lang: y, .. }) => x == y,
        (Block::List { start: x, .. }, Block::List { start: y, .. }) => x.is_some() == y.is_some(),
        (Block::Table { .. }, Block::Table { .. })
        | (Block::Quote(_), Block::Quote(_))
        | (Block::Html(_), Block::Html(_)) => true,
        _ => false,
    }
}

/// A changed pair marked: the old with its removed words, the new with its added ones, and the
/// two read as one. Code, tables and quotes are not marked inside.
fn mark(old: &Block, new: &Block) -> (Block, Block, Block) {
    match (old, new) {
        (Block::Paragraph(x), Block::Paragraph(y)) => {
            let (o, n, m) = mark_inline(x, y);
            (
                Block::Paragraph(o),
                Block::Paragraph(n),
                Block::Paragraph(m),
            )
        }
        (Block::Heading(l, x), Block::Heading(k, y)) => {
            let (o, n, m) = mark_inline(x, y);
            (
                Block::Heading(*l, o),
                Block::Heading(*k, n),
                Block::Heading(*k, m),
            )
        }
        (
            Block::List {
                start: s,
                items: xs,
            },
            Block::List {
                start: t,
                items: ys,
            },
        ) => {
            let (mut o, mut n, mut m) = (Vec::new(), Vec::new(), Vec::new());
            let text_like =
                |a: &Item, b: &Item| worddiff::marks(&item_text(a), &item_text(b)).is_some();
            for (change, i, j) in pair(xs, ys, |a, b| a == b, text_like) {
                let marked = |item: &Item, mark| Item {
                    mark: Some(mark),
                    ..item.clone()
                };
                match (change, i, j) {
                    (Change::Same, Some(i), Some(j)) => {
                        o.push(xs[i].clone());
                        n.push(ys[j].clone());
                        m.push(ys[j].clone());
                    }
                    (Change::Removed, Some(i), _) => {
                        o.push(marked(&xs[i], Mark::Removed));
                        m.push(marked(&xs[i], Mark::Removed));
                    }
                    (Change::Added, _, Some(j)) => {
                        n.push(marked(&ys[j], Mark::Added));
                        m.push(marked(&ys[j], Mark::Added));
                    }
                    (_, Some(i), Some(j)) => {
                        let (a, b, c) = mark_item(&xs[i], &ys[j]);
                        o.push(a);
                        n.push(b);
                        m.push(c);
                    }
                    _ => {}
                }
            }
            (
                Block::List {
                    start: *s,
                    items: o,
                },
                Block::List {
                    start: *t,
                    items: n,
                },
                Block::List {
                    start: *t,
                    items: m,
                },
            )
        }
        _ => (old.clone(), new.clone(), new.clone()),
    }
}

/// An item's words, to tell an edited item from a new one.
fn item_text(item: &Item) -> String {
    item.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(i) | Block::Heading(_, i) => Some(i.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A changed item: its first paragraph marked; what follows it as each version has it.
fn mark_item(old: &Item, new: &Item) -> (Item, Item, Item) {
    let (Some(Block::Paragraph(x)), Some(Block::Paragraph(y))) =
        (old.blocks.first(), new.blocks.first())
    else {
        return (old.clone(), new.clone(), new.clone());
    };
    let (o, n, m) = mark_inline(x, y);
    let with = |item: &Item, first: Inline| {
        let mut item = item.clone();
        item.blocks[0] = Block::Paragraph(first);
        item
    };
    (with(old, o), with(new, n), with(new, m))
}

/// Text edited: the old with its removed phrases marked, the new with its added ones, and the new
/// with each removed phrase put back in before what replaced it.
fn mark_inline(old: &Inline, new: &Inline) -> (Inline, Inline, Inline) {
    let Some(pieces) = worddiff::pieces(&old.text, &new.text) else {
        return (old.clone(), new.clone(), new.clone());
    };
    let (mut o, mut n, mut m) = (old.clone(), new.clone(), Inline::default());
    for piece in pieces {
        match piece {
            Piece::Same(_, r) => append(&mut m, new, r),
            Piece::Change(a, b) => {
                let gone = trimmed(&old.text, a.clone());
                let came = trimmed(&new.text, b.clone());
                if let Some(t) = &gone {
                    o.marks.push((t.clone(), Mark::Removed));
                    let at = m.text.len() - a.start;
                    append(&mut m, old, a.clone());
                    m.marks.push((t.start + at..t.end + at, Mark::Removed));
                }
                if let Some(t) = &came {
                    n.marks.push((t.clone(), Mark::Added));
                    // The removed and the added phrase apart, not run together.
                    if gone.is_some()
                        && !m.text.ends_with(char::is_whitespace)
                        && !new.text[b.clone()].starts_with(char::is_whitespace)
                    {
                        m.text.push(' ');
                    }
                    let at = m.text.len() - b.start;
                    append(&mut m, new, b.clone());
                    m.marks.push((t.start + at..t.end + at, Mark::Added));
                } else if gone.is_none() {
                    append(&mut m, new, b);
                }
            }
        }
    }
    (o, n, m)
}

/// `range` of `text` without its spaces at either end; None when nothing else is left.
fn trimmed(text: &str, range: Range<usize>) -> Option<Range<usize>> {
    let piece = &text[range.clone()];
    let lead = piece.len() - piece.trim_start().len();
    let trail = piece.len() - piece.trim_end().len();
    (lead < piece.len()).then(|| range.start + lead..range.end - trail)
}

/// `range` of `from` put at the end of `to`, its styles with it.
fn append(to: &mut Inline, from: &Inline, range: Range<usize>) {
    let at = to.text.len();
    to.text.push_str(&from.text[range.clone()]);
    for s in &from.spans {
        let (start, end) = (s.range.start.max(range.start), s.range.end.min(range.end));
        if start < end {
            to.spans.push(Span {
                range: start - range.start + at..end - range.start + at,
                ..s.clone()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "# Signing in\n\nThe screen shows a spinner, an error and the form, each on its own flag.\n\n## Retrying\n\nRetry repeats the last request, whatever happened.\n\n## Known issues\n\n- The spinner can stay on after a timeout.\n";
    const NEW: &str = "# Signing in\n\nThe screen shows exactly one state at a time: idle, loading, signed in, or failed.\n\n## States\n\n| State | Draws |\n|---|---|\n| Idle | the form |\n\n## Retrying\n\nRetry repeats the last form after a failure, whatever happened.\n\n```kotlin\nvm.retry()\n```\n";

    fn changes(rows: &[Row]) -> Vec<Change> {
        rows.iter().map(|r| r.change).collect()
    }

    #[test]
    fn a_document_is_compared_block_by_block() {
        let rows = diff(OLD, NEW);
        use Change::*;
        assert_eq!(
            changes(&rows),
            [
                Same, Removed, Added, Added, Added, Same, Changed, Removed, Removed, Added
            ]
        );
        // The rewritten paragraph shares too few words: removed and added, not changed.
        assert_eq!(rows[1].old.as_ref().map(|o| o.1), Some((3, 3)));
        // Lines of the new version, for a comment on the block.
        assert_eq!(rows[6].new.as_ref().map(|n| n.1), Some((13, 13)));
        assert_eq!(rows[9].new.as_ref().map(|n| n.1), Some((15, 17)));
    }

    #[test]
    fn a_changed_paragraph_marks_its_words() {
        let rows = diff(OLD, NEW);
        let Some(Block::Paragraph(merged)) = &rows[6].merged else {
            panic!("{:?}", rows[6]);
        };
        let marked: Vec<(&str, Mark)> = merged
            .marks
            .iter()
            .map(|(r, m)| (&merged.text[r.clone()], *m))
            .collect();
        assert_eq!(
            marked,
            [
                ("request", Mark::Removed),
                ("form after a failure", Mark::Added)
            ]
        );
        assert_eq!(
            merged.text,
            "Retry repeats the last request form after a failure, whatever happened."
        );
        let Some((Block::Paragraph(old), _)) = &rows[6].old else {
            panic!()
        };
        assert_eq!(&old.text[old.marks[0].0.clone()], "request");
    }

    #[test]
    fn a_list_is_marked_item_by_item() {
        let rows = diff(
            "- one\n- two words here\n- three\n",
            "- one\n- two words there\n- four\n",
        );
        assert_eq!(changes(&rows), [Change::Changed]);
        let Some(Block::List { items, .. }) = &rows[0].merged else {
            panic!()
        };
        let marks: Vec<Option<Mark>> = items.iter().map(|i| i.mark).collect();
        assert_eq!(marks, [None, None, Some(Mark::Removed), Some(Mark::Added)]);
        let Block::Paragraph(second) = &items[1].blocks[0] else {
            panic!()
        };
        assert_eq!(second.marks.len(), 2);
    }

    #[test]
    fn a_new_file_is_all_added() {
        assert_eq!(
            changes(&diff("", "# A\n\ntext\n")),
            [Change::Added, Change::Added]
        );
    }
}
