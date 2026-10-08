//! Commit search: free words plus `author:`, `path:` and `since:` qualifiers. Every word must match
//! (AND), case-insensitively; a free word matches the message, the author, the short SHA and the
//! paths the commit touched.

use crate::git::CommitInfo;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Query {
    words: Vec<String>,
    authors: Vec<String>,
    paths: Vec<String>,
    /// Commits at or after this time (seconds since the epoch).
    since: Option<i64>,
}

impl Query {
    /// `now` is the current time in seconds, for relative `since:` values.
    pub fn parse(text: &str, now: i64) -> Self {
        let mut query = Self::default();
        for token in text.split_whitespace() {
            let lower = token.to_lowercase();
            match lower.split_once(':') {
                Some(("author", value)) if !value.is_empty() => query.authors.push(value.into()),
                Some(("path", value)) if !value.is_empty() => query.paths.push(value.into()),
                // Still being typed (`since:2`): not a filter yet.
                Some(("since", value)) => query.since = since(value, now).or(query.since),
                // A qualifier with nothing after it yet.
                Some(("author" | "path", "")) => {}
                _ => query.words.push(lower),
            }
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether the query needs the touched paths to be decided.
    pub fn needs_paths(&self) -> bool {
        !self.paths.is_empty()
    }

    /// The free words, for highlighting a match.
    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// `paths` is `None` while the touched paths are not known yet; a path condition then fails.
    pub fn matches(&self, commit: &CommitInfo, paths: Option<&[String]>) -> bool {
        let summary = commit.summary.to_lowercase();
        let message = commit.message.to_lowercase();
        let author = format!("{} {}", commit.author, commit.email).to_lowercase();
        let sha = commit.id.to_string();
        let touched =
            |word: &str| paths.is_some_and(|ps| ps.iter().any(|p| p.to_lowercase().contains(word)));
        self.words.iter().all(|w| {
            message.contains(w.as_str())
                || summary.contains(w.as_str())
                || author.contains(w.as_str())
                || sha.starts_with(w.as_str())
                || touched(w)
        }) && self.authors.iter().all(|a| author.contains(a.as_str()))
            && self.paths.iter().all(|p| touched(p))
            && self.since.is_none_or(|t| commit.time >= t)
    }
}

/// `2w`, `3d`, `12h` (relative to `now`) or `2026-09-01`.
fn since(value: &str, now: i64) -> Option<i64> {
    if let Some((year, rest)) = value.split_once('-') {
        let (month, day) = rest.split_once('-')?;
        return Some(
            days_from_civil(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?) * 86_400,
        );
    }
    let unit = value.chars().last()?;
    let count: i64 = value[..value.len() - unit.len_utf8()].parse().ok()?;
    let seconds = match unit {
        'h' => 3_600,
        'd' => 86_400,
        'w' => 7 * 86_400,
        'm' => 30 * 86_400,
        _ => return None,
    };
    Some(now - count * seconds)
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Case-insensitive byte ranges of `words` in `text`, sorted and merged.
pub fn highlights(text: &str, words: &[String]) -> Vec<std::ops::Range<usize>> {
    // Lowercasing can change byte lengths for a few scripts: only trust ASCII-stable text.
    if !text.is_ascii() {
        return Vec::new();
    }
    let lower = text.to_ascii_lowercase();
    let mut ranges: Vec<std::ops::Range<usize>> = words
        .iter()
        .filter(|w| !w.is_empty())
        .flat_map(|w| lower.match_indices(w.as_str()).map(|(i, m)| i..i + m.len()))
        .collect();
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Oid;

    fn commit(summary: &str, author: &str, time: i64) -> CommitInfo {
        CommitInfo {
            id: Oid::from_str("8a02f3c000000000000000000000000000000000").unwrap(),
            summary: summary.into(),
            message: summary.into(),
            author: author.into(),
            email: format!("{author}@example.com"),
            time,
        }
    }

    #[test]
    fn free_words_all_have_to_match() {
        let c = commit("Retry the page fetch once", "juniper", 0);
        assert!(Query::parse("retry fetch", 0).matches(&c, None));
        assert!(!Query::parse("retry banana", 0).matches(&c, None));
        assert!(Query::parse("8a02f3c", 0).matches(&c, None));
        assert!(Query::parse("", 0).is_empty());
    }

    #[test]
    fn qualifiers_narrow_by_author_path_and_time() {
        let c = commit("Cap the log", "juniper", 1_000_000);
        let paths = vec!["src/git/log.rs".to_owned()];
        assert!(Query::parse("author:jun", 0).matches(&c, None));
        assert!(!Query::parse("author:rowan", 0).matches(&c, None));
        assert!(Query::parse("path:git/log", 0).matches(&c, Some(&paths)));
        assert!(!Query::parse("path:git/log", 0).matches(&c, None));
        assert!(Query::parse("log.rs", 0).matches(&c, Some(&paths)));
        assert!(Query::parse("since:1w", 1_000_000 + 86_400).matches(&c, None));
        assert!(!Query::parse("since:1d", 1_000_000 + 3 * 86_400).matches(&c, None));
    }

    #[test]
    fn a_qualifier_being_typed_is_not_a_filter() {
        let c = commit("anything", "juniper", 0);
        for text in ["author:", "path:", "since:", "since:2"] {
            assert!(Query::parse(text, 0).matches(&c, None), "{text}");
        }
    }

    #[test]
    fn dates_parse() {
        assert_eq!(since("1970-01-02", 0), Some(86_400));
        assert_eq!(since("2026-09-01", 0), Some(1_788_220_800));
    }

    #[test]
    fn highlights_merge_overlaps() {
        let r = highlights("Retry the retry", &["retry".into(), "the".into()]);
        assert_eq!(r, [0..5, 6..9, 10..15]);
    }
}
