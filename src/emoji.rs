//! Emoji by their GitLab names (#9): the reactions a note can get, `:name` completion, and the
//! map to GitHub's eight reactions. Every emoji GitLab knows (`assets/emoji/gitlab.tsv`, its own
//! list): the picker offers those with a group, skin tones are only drawn when someone used one.

use std::collections::HashMap;
use std::sync::LazyLock;

pub struct Emoji {
    pub name: &'static str,
    pub glyph: &'static str,
    /// GitLab's group (`Smileys & Emotion`); empty for a skin tone or a part, not offered.
    pub group: &'static str,
    /// What it shows, in words: found by a search too.
    pub about: &'static str,
}

static LIST: LazyLock<Vec<Emoji>> = LazyLock::new(|| {
    include_str!("../assets/emoji/gitlab.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut cols = l.split('\t');
            Some(Emoji {
                name: cols.next()?,
                glyph: cols.next()?,
                group: cols.next()?,
                about: cols.next().unwrap_or(""),
            })
        })
        .collect()
});

static BY_NAME: LazyLock<HashMap<&'static str, usize>> =
    LazyLock::new(|| LIST.iter().enumerate().map(|(i, e)| (e.name, i)).collect());

/// Every emoji, in GitLab's order (by group).
pub fn all() -> &'static [Emoji] {
    &LIST
}

/// The ones a picker offers, in that order.
pub fn offered() -> impl Iterator<Item = &'static Emoji> {
    LIST.iter().filter(|e| !e.group.is_empty())
}

/// GitHub's eight reactions as (its content name, GitLab name).
pub const GITHUB: &[(&str, &str)] = &[
    ("+1", "thumbsup"),
    ("-1", "thumbsdown"),
    ("laugh", "laughing"),
    ("hooray", "tada"),
    ("confused", "confused"),
    ("heart", "heart"),
    ("rocket", "rocket"),
    ("eyes", "eyes"),
];

/// The emoji of `name`, if the list has it.
pub fn get(name: &str) -> Option<&'static str> {
    BY_NAME.get(name).map(|&i| LIST[i].glyph)
}

/// What `query` finds, as (name, emoji): names that start with it, then names that hold it, then
/// those it starts a word of what they show (`tears` → 😂 joy, "face with tears of joy"). Colons
/// around it (`:tada:`) are dropped; an emoji pasted finds its own name.
pub fn find(query: &str, limit: usize) -> Vec<(&'static str, &'static str)> {
    let q = query.trim().trim_matches(':').to_lowercase();
    if q.is_empty() {
        return offered().take(limit).map(|e| (e.name, e.glyph)).collect();
    }
    if let Some(e) = LIST.iter().find(|e| e.glyph == q) {
        return vec![(e.name, e.glyph)];
    }
    let mut tiers: [Vec<&Emoji>; 3] = Default::default();
    for e in offered() {
        if e.name.starts_with(&q) {
            tiers[0].push(e);
        } else if e.name.contains(&q) {
            tiers[1].push(e);
        } else if e.about.split(' ').any(|w| w.starts_with(&q)) {
            tiers[2].push(e);
        }
    }
    tiers
        .into_iter()
        .flatten()
        .take(limit)
        .map(|e| (e.name, e.glyph))
        .collect()
}

/// `text` with each `:name:` the list has as its emoji, as GitLab and GitHub show a comment.
pub fn shortcodes(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains(':') {
        return text.into();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(':') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let glyph = after
            .find(':')
            .map(|end| &after[..end])
            .filter(|name| could_be_name(name))
            .and_then(|name| Some((name, get(name)?)));
        match glyph {
            Some((name, glyph)) => {
                out.push_str(glyph);
                rest = &after[name.len() + 1..];
            }
            None => {
                out.push(':');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.into()
}

/// Whether `name` could be a reaction GitLab has that this list does not (a custom one, or one
/// newer): the letters, digits and `_ - +` GitLab's names are made of.
pub fn could_be_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+'))
}

/// GitHub's content name for a GitLab name, when it is one of the eight.
pub fn github_name(name: &str) -> Option<&'static str> {
    GITHUB.iter().find(|(_, n)| *n == name).map(|(g, _)| *g)
}

/// The GitLab name of a GitHub content name.
pub fn from_github(content: &str) -> Option<&'static str> {
    GITHUB
        .iter()
        .find(|(g, _)| g.eq_ignore_ascii_case(content))
        .map(|(_, n)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_are_found_by_name_and_github_maps_to_its_eight() {
        assert_eq!(get("tada"), Some("🎉"));
        assert_eq!(get("thumbsup_tone3"), Some("👍🏽"));
        assert_eq!(find("thumbs", 3)[0].0, "thumbsup");
        assert!(find("mile", 99).iter().any(|(n, _)| *n == "smile"));
        // By what it shows, with colons around, or pasted.
        assert!(find("tears", 99).iter().any(|(n, _)| *n == "joy"));
        assert_eq!(find(":partying_face:", 1)[0].0, "partying_face");
        assert_eq!(find("🦄", 1), [("unicorn", "🦄")]);
        // Skin tones are drawn, not offered.
        assert!(
            find("thumbsup", 99)
                .iter()
                .all(|(n, _)| !n.contains("tone"))
        );
        assert!(offered().count() > 1500);
        assert!(could_be_name("my_team-logo"));
        assert_eq!(
            shortcodes("ship it :rocket: at 10:30 :nope: :tada::tada:"),
            "ship it 🚀 at 10:30 :nope: 🎉🎉"
        );
        assert!(!could_be_name("two words"));
        assert_eq!(github_name("thumbsup"), Some("+1"));
        assert_eq!(from_github("HOORAY"), Some("tada"));
        assert_eq!(github_name("bug"), None);
    }
}
