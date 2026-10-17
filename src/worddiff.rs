//! Word-level marks inside a changed line: which words of the removed line and of the added line
//! differ. Pure text in, byte ranges out.

use std::ops::Range;

/// Lines longer than this are never compared word by word.
const MAX_LINE: usize = 1000;
/// Below this share of common words, two lines are a rewrite, not an edit.
const MIN_SHARED: f32 = 0.5;

/// The changed byte ranges of the old line and of the new line.
pub type LineMarks = (Vec<Range<usize>>, Vec<Range<usize>>);

/// Changed byte ranges of `old` and of `new`; `None` when the lines have too little in common
/// (a rewritten line is not made readable by marks) or are too long.
pub fn marks(old: &str, new: &str) -> Option<LineMarks> {
    if old.len() > MAX_LINE || new.len() > MAX_LINE {
        return None;
    }
    let (a, b) = (words(old), words(new));
    let (keep_a, keep_b) = common(old, &a, new, &b);

    let weight = |text: &str, words: &[Range<usize>]| {
        words
            .iter()
            .filter(|w| !text[(*w).clone()].trim().is_empty())
            .count()
    };
    let shared = a
        .iter()
        .zip(&keep_a)
        .filter(|(w, keep)| **keep && !old[(*w).clone()].trim().is_empty())
        .count();
    let longest = weight(old, &a).max(weight(new, &b));
    if longest == 0 || (shared as f32) < MIN_SHARED * longest as f32 {
        return None;
    }
    Some((changed(old, &a, &keep_a), changed(new, &b, &keep_b)))
}

/// One piece of two texts read together: the same in both, or a change — what the old text had
/// there and what the new one has (either may be empty). The ranges follow each other in each text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Same(Range<usize>, Range<usize>),
    Change(Range<usize>, Range<usize>),
}

/// `old` and `new` cut into what stayed and what changed, a phrase at a time: a space or a lone
/// short word between two changes goes into the change, so a rewritten clause reads as one
/// removal and one addition. `None` as for `marks`.
pub fn pieces(old: &str, new: &str) -> Option<Vec<Piece>> {
    marks(old, new)?;
    let (a, b) = (words(old), words(new));
    let (keep_a, keep_b) = common(old, &a, new, &b);
    // Word by word: Some((i, j)) for a pair in the common part, else one side's word.
    enum Op {
        Same(usize, usize),
        Old(usize),
        New(usize),
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && !keep_a[i] {
            ops.push(Op::Old(i));
            i += 1;
        } else if j < b.len() && !keep_b[j] {
            ops.push(Op::New(j));
            j += 1;
        } else {
            ops.push(Op::Same(i, j));
            i += 1;
            j += 1;
        }
    }
    let small = |i: usize| {
        let w = old[a[i].clone()].trim();
        w.chars().count() <= 3
    };
    let is_change = |k: usize| !matches!(ops[k], Op::Same(..));
    let mut out: Vec<Piece> = Vec::new();
    let (mut at_a, mut at_b) = (0, 0);
    let mut k = 0;
    while k < ops.len() {
        if !is_change(k) {
            let Op::Same(i, j) = ops[k] else {
                unreachable!()
            };
            match out.last_mut() {
                Some(Piece::Same(x, y)) => {
                    x.end = a[i].end;
                    y.end = b[j].end;
                }
                _ => out.push(Piece::Same(a[i].clone(), b[j].clone())),
            }
            at_a = a[i].end;
            at_b = b[j].end;
            k += 1;
            continue;
        }
        // A change, and whatever small same words lie between it and the next change.
        let mut end = k;
        loop {
            while end < ops.len() && is_change(end) {
                end += 1;
            }
            let mut gap = end;
            while gap < ops.len() && matches!(ops[gap], Op::Same(i, _) if small(i)) && gap - end < 3
            {
                gap += 1;
            }
            if gap > end && gap < ops.len() && is_change(gap) {
                end = gap;
            } else {
                break;
            }
        }
        let (start_a, start_b) = (at_a, at_b);
        for op in &ops[k..end] {
            match *op {
                Op::Same(i, j) => {
                    at_a = a[i].end;
                    at_b = b[j].end;
                }
                Op::Old(i) => at_a = a[i].end,
                Op::New(j) => at_b = b[j].end,
            }
        }
        out.push(Piece::Change(start_a..at_a, start_b..at_b));
        k = end;
    }
    Some(out)
}

/// Runs of word characters, runs of whitespace, runs of operator characters and single others.
pub fn words(text: &str) -> Vec<Range<usize>> {
    #[derive(PartialEq)]
    enum Class {
        Word,
        Space,
        /// Operator characters stick together (`>=`, `::`, `&&`).
        Operator,
        /// Brackets, quotes, commas: one at a time.
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c.is_whitespace() {
            Class::Space
        } else if "<>=!&|+-*/%^~?:".contains(c) {
            Class::Operator
        } else {
            Class::Other
        }
    };
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut last: Option<Class> = None;
    for (i, c) in text.char_indices() {
        let class = class(c);
        match (&last, out.last_mut()) {
            (Some(prev), Some(range)) if *prev == class && class != Class::Other => {
                range.end = i + c.len_utf8()
            }
            _ => out.push(i..i + c.len_utf8()),
        }
        last = Some(class);
    }
    out
}

/// Longest common subsequence of the two word lists; for each word, whether it is in it.
fn common(old: &str, a: &[Range<usize>], new: &str, b: &[Range<usize>]) -> (Vec<bool>, Vec<bool>) {
    let (n, m) = (a.len(), b.len());
    let eq = |i: usize, j: usize| old[a[i].clone()] == new[b[j].clone()];
    // lcs[i][j]: length for the suffixes a[i..], b[j..].
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if eq(i, j) {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut keep_a, mut keep_b) = (vec![false; n], vec![false; m]);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if eq(i, j) {
            keep_a[i] = true;
            keep_b[j] = true;
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (keep_a, keep_b)
}

/// The words not in the common part, as ranges; neighbours (and neighbours split only by
/// whitespace) merge, and a range of nothing but whitespace is dropped.
fn changed(text: &str, words: &[Range<usize>], keep: &[bool]) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    for (word, _) in words.iter().zip(keep).filter(|(_, keep)| !**keep) {
        match out.last_mut() {
            Some(last) if text[last.end..word.start].trim().is_empty() => last.end = word.end,
            _ => out.push(word.clone()),
        }
    }
    out.retain(|r| !text[r.clone()].trim().is_empty());
    for r in &mut out {
        let piece = &text[r.clone()];
        let lead = piece.len() - piece.trim_start().len();
        let trail = piece.len() - piece.trim_end().len();
        *r = r.start + lead..r.end - trail;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts<'a>(text: &'a str, ranges: &[Range<usize>]) -> Vec<&'a str> {
        ranges.iter().map(|r| &text[r.clone()]).collect()
    }

    #[test]
    fn marks_only_the_words_that_changed() {
        let (old, new) = ("if len > MAX {", "if len >= MAX {");
        let (a, b) = marks(old, new).expect("an edit");
        assert_eq!(texts(old, &a), [">"]);
        assert_eq!(texts(new, &b), [">="]);
    }

    #[test]
    fn a_rewritten_line_has_no_marks() {
        assert!(marks("// retry once", "let retries = 1;").is_none());
    }

    #[test]
    fn long_lines_are_not_compared() {
        let long = "word ".repeat(300);
        assert!(marks(&long, &format!("{long}x")).is_none());
    }

    #[test]
    fn added_words_merge_into_one_range() {
        let (old, new) = ("call(x)", "call(a, b, x)");
        let (a, b) = marks(old, new).expect("an edit");
        assert!(a.is_empty());
        assert_eq!(texts(new, &b), ["a, b,"]);
    }

    #[test]
    fn a_rewritten_clause_is_one_change() {
        let (old, new) = (
            "Retry repeats the last request, whatever happened.",
            "Retry repeats the last form after a failure, whatever happened.",
        );
        let pieces = pieces(old, new).expect("an edit");
        let changes: Vec<(&str, &str)> = pieces
            .iter()
            .filter_map(|p| match p {
                Piece::Change(x, y) => Some((&old[x.clone()], &new[y.clone()])),
                Piece::Same(..) => None,
            })
            .collect();
        assert_eq!(changes, [("request", "form after a failure")]);
        // Read together, the pieces are the two texts.
        let (mut a, mut b) = (String::new(), String::new());
        for p in &pieces {
            let (Piece::Same(x, y) | Piece::Change(x, y)) = p;
            a.push_str(&old[x.clone()]);
            b.push_str(&new[y.clone()]);
        }
        assert_eq!((a.as_str(), b.as_str()), (old, new));
    }

    #[test]
    fn multibyte_text_stays_on_char_boundaries() {
        let (old, new) = ("let s = \"héllo wörld\";", "let s = \"héllo wörld!\";");
        let (_, b) = marks(old, new).expect("an edit");
        assert_eq!(texts(new, &b), ["!"]);
    }
}
