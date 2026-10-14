//! What the open repository's host calls things: GitLab's merge requests (`!12`), GitHub's pull
//! requests (`#12`). The workspace being drawn says which (`set`); the app writes GitLab's words,
//! and text shown goes through `say` — inside `keys::text`, so every tooltip, menu, placeholder,
//! toast and settings line that shows a shortcut also says the host's words.

use std::borrow::Cow;
use std::cell::Cell;

thread_local! {
    static GITHUB: Cell<bool> = const { Cell::new(false) };
}

/// Called as a workspace draws: its host's words from here on.
pub fn set(github: bool) {
    GITHUB.with(|g| g.set(github));
}

pub fn github() -> bool {
    GITHUB.with(Cell::get)
}

/// `text` in the host's words.
pub fn say(text: &str) -> Cow<'_, str> {
    if !github()
        || !(text.contains("erge request")
            || text.contains("erge Request")
            || text.contains("GitLab"))
    {
        return Cow::Borrowed(text);
    }
    Cow::Owned(github_words(text))
}

fn github_words(text: &str) -> String {
    text.replace("merge request", "pull request")
        .replace("Merge request", "Pull request")
        .replace("Merge Request", "Pull Request")
        .replace("GitLab", "GitHub")
}

/// A request's number as the host writes it: `!12` on GitLab, `#12` on GitHub.
pub fn number(iid: u64) -> String {
    format!("{}{iid}", if github() { '#' } else { '!' })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_says_pull_request() {
        assert_eq!(
            github_words("Create merge request on GitLab · Merge requests"),
            "Create pull request on GitHub · Pull requests"
        );
        set(false);
        assert_eq!(say("Merge requests"), "Merge requests");
        assert_eq!(number(12), "!12");
        set(true);
        assert_eq!(say("Merge requests · 3"), "Pull requests · 3");
        assert_eq!(number(12), "#12");
        set(false);
    }
}
