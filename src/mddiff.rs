//! A changed markdown document read block by block (#11, Design `docs/specs/markdown-viewer.md`):
//! the blocks both versions share stay, a block edited in place is *changed* — its words marked as
//! in Words mode, a list's items one by one, a code block's lines, a table's rows and cells, a
//! quote's blocks — and the rest are added or removed. A paragraph rewritten where it stood is
//! changed too, all its words marked. The rendered page draws the rows: Unified the new version
//! with the changes in it, Split the two side by side.

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
    pair(
        &a,
        &b,
        |x, y| x.0 == y.0,
        |x, y| like(&x.0, &y.0),
        |x, y| kin(&x.0, &y.0),
    )
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
/// changed (in order), then what is left a `kin` pair standing in the same place, and the rest
/// removed or added — removed first. (change, old, new) indices.
fn pair<T>(
    old: &[T],
    new: &[T],
    same: impl Fn(&T, &T) -> bool,
    like: impl Fn(&T, &T) -> bool,
    kin: impl Fn(&T, &T) -> bool,
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
        gap(&old[i..mi], &new[j..mj], (i, j), (&like, &kin), &mut out);
        if mi < old.len() {
            out.push((Change::Same, Some(mi), Some(mj)));
        }
        (i, j) = (mi + 1, mj + 1);
    }
    out
}

/// What lies between two kept pairs: each old one changed into the first like new one after the
/// last pairing; then one left over changed into a kin one between the pairs around it (a
/// paragraph rewritten where it stood); the rest removed or added.
fn gap<T>(
    a: &[T],
    b: &[T],
    (i0, j0): (usize, usize),
    (like, kin): (&impl Fn(&T, &T) -> bool, &impl Fn(&T, &T) -> bool),
    out: &mut Vec<(Change, Option<usize>, Option<usize>)>,
) {
    let mut partner: Vec<Option<usize>> = vec![None; a.len()];
    let mut taken = vec![false; b.len()];
    let mut from = 0;
    for (x, ax) in a.iter().enumerate() {
        if let Some(y) = (from..b.len()).find(|&y| like(ax, &b[y])) {
            partner[x] = Some(y);
            taken[y] = true;
            from = y + 1;
        }
    }
    let mut lo = 0;
    for x in 0..a.len() {
        if let Some(y) = partner[x] {
            lo = y + 1;
            continue;
        }
        let hi = partner[x + 1..]
            .iter()
            .flatten()
            .next()
            .copied()
            .unwrap_or(b.len());
        if let Some(y) = (lo..hi).find(|&y| !taken[y] && kin(&a[x], &b[y])) {
            partner[x] = Some(y);
            taken[y] = true;
            lo = y + 1;
        }
    }
    let paired: Vec<bool> = partner.iter().map(Option::is_some).collect();
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

/// Blocks of one kind, so one can stand for the other rewritten in its place: paragraphs, headings
/// of one level.
fn kin(old: &Block, new: &Block) -> bool {
    match (old, new) {
        (Block::Paragraph(_), Block::Paragraph(_)) => true,
        (Block::Heading(l, _), Block::Heading(m, _)) => l == m,
        _ => false,
    }
}

/// A changed pair marked: the old with its removed words, the new with its added ones, and the
/// two read as one.
fn mark(old: &Block, new: &Block) -> (Block, Block, Block) {
    match (old, new) {
        (Block::Code { lang, text: x, .. }, Block::Code { text: y, .. }) => {
            let (o, n, m) = mark_code(x, y);
            let code = |(text, marks): (String, Vec<(usize, Mark)>)| Block::Code {
                lang: lang.clone(),
                text,
                marks,
            };
            (code(o), code(n), code(m))
        }
        (Block::Table { head: hx, rows: rx }, Block::Table { head: hy, rows: ry }) => {
            mark_table((hx, rx), (hy, ry))
        }
        (Block::Quote(xs), Block::Quote(ys)) => {
            let (o, n, m) = mark_blocks(xs, ys);
            (Block::Quote(o), Block::Quote(n), Block::Quote(m))
        }
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
            for (change, i, j) in pair(xs, ys, |a, b| a == b, text_like, |_, _| false) {
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
        // Rewritten: every word of the old gone, every word of the new come.
        let (o, n) = (whole(old, Mark::Removed), whole(new, Mark::Added));
        let mut m = o.clone();
        let at = m.text.len() + 1;
        m.text.push(' ');
        append(&mut m, new, 0..new.text.len());
        m.marks
            .extend(n.marks.iter().map(|(r, k)| (r.start + at..r.end + at, *k)));
        return (o, n, m);
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

/// `inline` with all its words marked `mark`.
fn whole(inline: &Inline, mark: Mark) -> Inline {
    let mut out = inline.clone();
    if let Some(r) = trimmed(&inline.text, 0..inline.text.len()) {
        out.marks.push((r, mark));
    }
    out
}

/// `block` added or removed whole, inside a changed block (a quote's paragraph, a table's row).
fn whole_block(block: &Block, mark: Mark) -> Block {
    match block {
        Block::Paragraph(i) => Block::Paragraph(whole(i, mark)),
        Block::Heading(l, i) => Block::Heading(*l, whole(i, mark)),
        Block::Code { lang, text, .. } => Block::Code {
            lang: lang.clone(),
            text: text.clone(),
            marks: (0..text.split('\n').count()).map(|n| (n, mark)).collect(),
        },
        Block::Quote(blocks) => Block::Quote(blocks.iter().map(|b| whole_block(b, mark)).collect()),
        Block::List { start, items } => Block::List {
            start: *start,
            items: items
                .iter()
                .map(|i| Item {
                    mark: Some(mark),
                    ..i.clone()
                })
                .collect(),
        },
        Block::Table { head, rows } => Block::Table {
            head: head.iter().map(|c| whole(c, mark)).collect(),
            rows: rows
                .iter()
                .map(|r| r.iter().map(|c| whole(c, mark)).collect())
                .collect(),
        },
        other => other.clone(),
    }
}

/// A code block's lines compared: the old text with its removed lines, the new with its added
/// ones, and the two read as one (each removed line before what replaced it).
type Code = (String, Vec<(usize, Mark)>);
fn mark_code(old: &str, new: &str) -> (Code, Code, Code) {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.split('\n').collect(), new.split('\n').collect());
    let (mut o, mut n) = (Vec::new(), Vec::new());
    let (mut lines, mut m) = (Vec::new(), Vec::new());
    for (change, i, j) in pair(&a, &b, |x, y| x == y, |_, _| false, |_, _| false) {
        match (change, i, j) {
            (Change::Removed, Some(i), _) => {
                o.push((i, Mark::Removed));
                m.push((lines.len(), Mark::Removed));
                lines.push(a[i]);
            }
            (Change::Added, _, Some(j)) => {
                n.push((j, Mark::Added));
                m.push((lines.len(), Mark::Added));
                lines.push(b[j]);
            }
            (_, _, Some(j)) => lines.push(b[j]),
            _ => {}
        }
    }
    (
        (old.to_owned(), o),
        (new.to_owned(), n),
        (lines.join("\n"), m),
    )
}

/// A table's rows compared like a document's blocks: a row edited in place has its cells marked
/// word by word, the rest are added or removed whole. Its head likewise.
fn mark_table(
    (hx, rx): (&[Inline], &[Vec<Inline>]),
    (hy, ry): (&[Inline], &[Vec<Inline>]),
) -> (Block, Block, Block) {
    // Cells compared by place; one only one side has is marked whole.
    let cells = |x: &[Inline], y: &[Inline]| {
        let (mut o, mut n, mut m) = (Vec::new(), Vec::new(), Vec::new());
        for k in 0..x.len().max(y.len()) {
            match (x.get(k), y.get(k)) {
                (Some(a), Some(b)) if a.text == b.text => {
                    o.push(a.clone());
                    n.push(b.clone());
                    m.push(b.clone());
                }
                (Some(a), Some(b)) => {
                    let (p, q, r) = mark_inline(a, b);
                    o.push(p);
                    n.push(q);
                    m.push(r);
                }
                (Some(a), None) => {
                    o.push(whole(a, Mark::Removed));
                    m.push(whole(a, Mark::Removed));
                }
                (None, Some(b)) => {
                    n.push(whole(b, Mark::Added));
                    m.push(whole(b, Mark::Added));
                }
                (None, None) => {}
            }
        }
        (o, n, m)
    };
    let (ho, hn, hm) = cells(hx, hy);
    let text = |r: &Vec<Inline>| {
        r.iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let row_like = |a: &Vec<Inline>, b: &Vec<Inline>| worddiff::marks(&text(a), &text(b)).is_some();
    let (mut o, mut n, mut m) = (Vec::new(), Vec::new(), Vec::new());
    for (change, i, j) in pair(rx, ry, |a, b| text(a) == text(b), row_like, |_, _| false) {
        match (change, i, j) {
            (Change::Same, Some(i), Some(j)) => {
                o.push(rx[i].clone());
                n.push(ry[j].clone());
                m.push(ry[j].clone());
            }
            (Change::Removed, Some(i), _) => {
                let gone: Vec<Inline> = rx[i].iter().map(|c| whole(c, Mark::Removed)).collect();
                o.push(gone.clone());
                m.push(gone);
            }
            (Change::Added, _, Some(j)) => {
                let came: Vec<Inline> = ry[j].iter().map(|c| whole(c, Mark::Added)).collect();
                n.push(came.clone());
                m.push(came);
            }
            (_, Some(i), Some(j)) => {
                let (p, q, r) = cells(&rx[i], &ry[j]);
                o.push(p);
                n.push(q);
                m.push(r);
            }
            _ => {}
        }
    }
    let table = |head, rows| Block::Table { head, rows };
    (table(ho, o), table(hn, n), table(hm, m))
}

/// A quote's blocks compared like the document's own: each changed one marked, an added or
/// removed one marked whole.
fn mark_blocks(xs: &[Block], ys: &[Block]) -> (Vec<Block>, Vec<Block>, Vec<Block>) {
    let (mut o, mut n, mut m) = (Vec::new(), Vec::new(), Vec::new());
    for (change, i, j) in pair(xs, ys, |a, b| a == b, like, kin) {
        match (change, i, j) {
            (Change::Same, Some(i), Some(j)) => {
                o.push(xs[i].clone());
                n.push(ys[j].clone());
                m.push(ys[j].clone());
            }
            (Change::Removed, Some(i), _) => {
                o.push(whole_block(&xs[i], Mark::Removed));
                m.push(whole_block(&xs[i], Mark::Removed));
            }
            (Change::Added, _, Some(j)) => {
                n.push(whole_block(&ys[j], Mark::Added));
                m.push(whole_block(&ys[j], Mark::Added));
            }
            (_, Some(i), Some(j)) => {
                let (p, q, r) = mark(&xs[i], &ys[j]);
                o.push(p);
                n.push(q);
                m.push(r);
            }
            _ => {}
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
                Same, Changed, Added, Added, Same, Changed, Removed, Removed, Added
            ]
        );
        // The rewritten paragraph shares too few words, but it stands where the old one did:
        // changed, every word marked.
        assert_eq!(rows[1].old.as_ref().map(|o| o.1), Some((3, 3)));
        let Some(Block::Paragraph(merged)) = &rows[1].merged else {
            panic!("{:?}", rows[1]);
        };
        assert_eq!(merged.marks.len(), 2);
        assert_eq!(merged.marks[0].1, Mark::Removed);
        assert_eq!(merged.marks[1].1, Mark::Added);
        // Lines of the new version, for a comment on the block.
        assert_eq!(rows[5].new.as_ref().map(|n| n.1), Some((13, 13)));
        assert_eq!(rows[8].new.as_ref().map(|n| n.1), Some((15, 17)));
    }

    #[test]
    fn a_changed_paragraph_marks_its_words() {
        let rows = diff(OLD, NEW);
        let Some(Block::Paragraph(merged)) = &rows[5].merged else {
            panic!("{:?}", rows[5]);
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
        let Some((Block::Paragraph(old), _)) = &rows[5].old else {
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
    fn a_code_block_marks_its_lines() {
        let rows = diff(
            "```rust\nlet a = 1;\nlet b = 2;\n```\n",
            "```rust\nlet a = 1;\nlet c = 3;\n```\n",
        );
        assert_eq!(changes(&rows), [Change::Changed]);
        let Some(Block::Code { text, marks, .. }) = &rows[0].merged else {
            panic!("{:?}", rows[0]);
        };
        assert_eq!(text, "let a = 1;\nlet b = 2;\nlet c = 3;");
        assert_eq!(marks, &[(1, Mark::Removed), (2, Mark::Added)]);
    }

    #[test]
    fn a_table_marks_its_cells_and_rows() {
        let rows = diff(
            "| State | Draws |\n|---|---|\n| Idle | the form |\n| Busy | a spinner |\n",
            "| State | Draws |\n|---|---|\n| Idle | the empty form |\n| Failed | the reason |\n",
        );
        assert_eq!(changes(&rows), [Change::Changed]);
        let Some(Block::Table { rows: body, .. }) = &rows[0].merged else {
            panic!("{:?}", rows[0]);
        };
        // The edited row's cell word by word; the other row replaced whole.
        assert_eq!(body.len(), 3);
        assert_eq!(body[0][1].marks.len(), 1);
        assert!(
            body[1]
                .iter()
                .all(|c| c.marks == [(0..c.text.len(), Mark::Removed)])
        );
        assert!(
            body[2]
                .iter()
                .all(|c| c.marks == [(0..c.text.len(), Mark::Added)])
        );
    }

    #[test]
    fn a_quote_marks_its_blocks() {
        let rows = diff(
            "> Keep the error in state.\n>\n> Old note.\n",
            "> Keep the error text in state.\n",
        );
        assert_eq!(changes(&rows), [Change::Changed]);
        let Some(Block::Quote(blocks)) = &rows[0].merged else {
            panic!("{:?}", rows[0]);
        };
        let Block::Paragraph(first) = &blocks[0] else {
            panic!()
        };
        assert!(first.marks.iter().any(|(_, m)| *m == Mark::Added));
        let Block::Paragraph(gone) = &blocks[1] else {
            panic!()
        };
        assert_eq!(gone.marks, [(0..gone.text.len(), Mark::Removed)]);
    }

    #[test]
    fn a_new_file_is_all_added() {
        assert_eq!(
            changes(&diff("", "# A\n\ntext\n")),
            [Change::Added, Change::Added]
        );
    }
}
