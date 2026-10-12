//! How well a name matches what was typed, so the best match comes first: typing `develop` puts
//! `develop` and `origin/develop` before `feature/DCT-0555-sync-develop`. Every word must appear
//! (case-insensitively); the whole query then decides the tier.

/// A lower score is a better match; `None` when some word is missing.
pub fn score(query: &str, hay: &str) -> Option<(u8, usize)> {
    let hay_l = hay.to_lowercase();
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    if !words.iter().all(|w| hay_l.contains(w)) {
        return None;
    }
    let last = hay_l.rsplit('/').next().unwrap_or(&hay_l);
    let segments: Vec<&str> = hay_l.split(['/', '-', '_', '.', ' ']).collect();
    let tier = if hay_l == query {
        0
    } else if last == query {
        1
    } else if hay_l.starts_with(query.as_str()) || last.starts_with(query.as_str()) {
        2
    } else if segments.contains(&query.as_str()) {
        3
    } else if segments.iter().any(|s| s.starts_with(query.as_str())) {
        4
    } else {
        5
    };
    Some((tier, hay.len()))
}

/// `items` that match `query`, best first; equal scores keep their order.
pub fn rank<T>(
    items: impl IntoIterator<Item = T>,
    query: &str,
    hay: impl Fn(&T) -> &str,
) -> Vec<T> {
    let mut scored: Vec<((u8, usize), T)> = items
        .into_iter()
        .filter_map(|item| Some((score(query, hay(&item))?, item)))
        .collect();
    scored.sort_by_key(|(s, _)| *s);
    scored.into_iter().map(|(_, item)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_itself_beats_names_that_contain_it() {
        let names = [
            "feature/DCT-0555-sync-develop",
            "origin/feature/ST-37965-analysis-of-develop-changes",
            "origin/develop",
            "develop-old",
            "develop",
            "feature/developer-tools",
        ];
        let ranked = rank(names, "develop", |n| n);
        assert_eq!(
            ranked,
            [
                "develop",
                "origin/develop",
                "develop-old",
                // The branch's own name starts with it.
                "feature/developer-tools",
                "feature/DCT-0555-sync-develop",
                "origin/feature/ST-37965-analysis-of-develop-changes",
            ]
        );
    }

    #[test]
    fn every_word_must_match() {
        assert!(score("pfm history", "self/pfm-responsive-02-history").is_some());
        assert!(score("pfm analytics", "self/pfm-responsive-02-history").is_none());
        assert_eq!(score("DEVELOP", "develop"), Some((0, 7)));
    }
}
