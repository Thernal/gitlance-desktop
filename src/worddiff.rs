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
    fn multibyte_text_stays_on_char_boundaries() {
        let (old, new) = ("let s = \"héllo wörld\";", "let s = \"héllo wörld!\";");
        let (_, b) = marks(old, new).expect("an edit");
        assert_eq!(texts(new, &b), ["!"]);
    }
}
