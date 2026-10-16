//! Emoji by their GitLab names (#9): the reactions a note can get, `:name` completion, and the
//! map to GitHub's eight reactions. A short list of the ones people use, not the whole Unicode set.

/// (GitLab name, emoji), the most used first.
pub const ALL: &[(&str, &str)] = &[
    ("thumbsup", "👍"),
    ("thumbsdown", "👎"),
    ("heart", "❤️"),
    ("tada", "🎉"),
    ("rocket", "🚀"),
    ("eyes", "👀"),
    ("smile", "😄"),
    ("laughing", "😆"),
    ("confused", "😕"),
    ("thinking", "🤔"),
    ("pray", "🙏"),
    ("clap", "👏"),
    ("fire", "🔥"),
    ("100", "💯"),
    ("white_check_mark", "✅"),
    ("heavy_check_mark", "✔️"),
    ("x", "❌"),
    ("warning", "⚠️"),
    ("bug", "🐛"),
    ("sparkles", "✨"),
    ("star", "⭐"),
    ("bulb", "💡"),
    ("question", "❓"),
    ("exclamation", "❗"),
    ("ok_hand", "👌"),
    ("wave", "👋"),
    ("muscle", "💪"),
    ("raised_hands", "🙌"),
    ("sweat_smile", "😅"),
    ("joy", "😂"),
    ("slight_smile", "🙂"),
    ("wink", "😉"),
    ("grin", "😁"),
    ("blush", "😊"),
    ("sob", "😭"),
    ("cry", "😢"),
    ("scream", "😱"),
    ("rage", "😡"),
    ("neutral_face", "😐"),
    ("unamused", "😒"),
    ("rolling_eyes", "🙄"),
    ("sunglasses", "😎"),
    ("nerd", "🤓"),
    ("party", "🥳"),
    ("hugging", "🤗"),
    ("zipper_mouth", "🤐"),
    ("see_no_evil", "🙈"),
    ("skull", "💀"),
    ("ghost", "👻"),
    ("robot", "🤖"),
    ("coffee", "☕"),
    ("beer", "🍺"),
    ("cake", "🍰"),
    ("gift", "🎁"),
    ("trophy", "🏆"),
    ("medal", "🏅"),
    ("lock", "🔒"),
    ("key", "🔑"),
    ("hammer", "🔨"),
    ("wrench", "🔧"),
    ("gear", "⚙️"),
    ("package", "📦"),
    ("memo", "📝"),
    ("pushpin", "📌"),
    ("link", "🔗"),
    ("mag", "🔍"),
    ("chart_with_upwards_trend", "📈"),
    ("hourglass", "⌛"),
    ("zap", "⚡"),
    ("boom", "💥"),
    ("construction", "🚧"),
    ("rotating_light", "🚨"),
    ("checkered_flag", "🏁"),
    ("arrow_up", "⬆️"),
    ("arrow_down", "⬇️"),
    ("recycle", "♻️"),
    ("wastebasket", "🗑️"),
    ("seedling", "🌱"),
    ("sunny", "☀️"),
    ("rainbow", "🌈"),
    ("snowflake", "❄️"),
    ("crossed_fingers", "🤞"),
    ("handshake", "🤝"),
    ("point_up", "☝️"),
    ("point_right", "👉"),
    ("v", "✌️"),
    ("facepalm", "🤦"),
    ("shrug", "🤷"),
];

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
    ALL.iter().find(|(n, _)| *n == name).map(|(_, e)| *e)
}

/// Names that start with `query`, then those that contain it.
pub fn find(query: &str, limit: usize) -> Vec<(&'static str, &'static str)> {
    let q = query.to_lowercase();
    let mut out: Vec<_> = ALL
        .iter()
        .filter(|(n, _)| n.starts_with(&q))
        .copied()
        .collect();
    out.extend(
        ALL.iter()
            .filter(|(n, _)| !n.starts_with(&q) && n.contains(&q))
            .copied(),
    );
    out.truncate(limit);
    out
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
        assert_eq!(find("th", 3)[0].0, "thumbsup");
        assert!(find("mile", 9).iter().any(|(n, _)| *n == "smile"));
        assert_eq!(github_name("thumbsup"), Some("+1"));
        assert_eq!(from_github("HOORAY"), Some("tada"));
        assert_eq!(github_name("bug"), None);
    }
}
