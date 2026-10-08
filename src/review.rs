//! Line comments for a coding agent: kept per repository on this Mac (never in the repository),
//! attached to a file and to the text of a line rather than its number, so a rebase does not
//! detach them, and exported as markdown to paste into an agent.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub path: String,
    /// On the removed side of the diff.
    pub old: bool,
    /// The line number when it was written; the text below decides where it belongs now.
    pub line: u32,
    /// The commented line, as it was.
    pub code: String,
    pub body: String,
    /// Where it was written: a commit or a version range, for the reader of the export.
    pub at: String,
}

/// Where a comment sits in the file as it is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// On this 1-based line.
    Line(u32),
    /// The line is gone or changed beyond recognition.
    Outdated,
}

/// Finds `comment`'s line in a side of a file: where it was if it still reads the same, else the one
/// line that does; ambiguous or missing means outdated.
pub fn place<'a>(comment: &Comment, count: u32, line: impl Fn(u32) -> &'a str) -> Place {
    if comment.line >= 1 && comment.line <= count && line(comment.line) == comment.code {
        return Place::Line(comment.line);
    }
    let mut found = (1..=count).filter(|&n| line(n) == comment.code);
    match (found.next(), found.next()) {
        (Some(only), None) if !comment.code.trim().is_empty() => Place::Line(only),
        _ => Place::Outdated,
    }
}

fn store(root: &Path) -> String {
    let name: String = root
        .display()
        .to_string()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    format!("comments/{name}.json")
}

pub fn load(root: &Path) -> Vec<Comment> {
    serde_json::from_str(&crate::storage::read(&store(root))).unwrap_or_default()
}

pub fn save(root: &Path, comments: &[Comment]) {
    if let Ok(body) = serde_json::to_string_pretty(comments) {
        crate::storage::save(&store(root), body);
    }
}

/// The comments as markdown for an agent: the file and line, the code, then what to change.
/// `places` gives each comment's current line, `None` when it is outdated.
pub fn export(repo: &str, comments: &[(Comment, Place)]) -> String {
    let mut out = format!(
        "# Review of {repo}\n\nAddress each comment below. Each names a file and line, quotes the line as the reviewer saw it, and says what to change.\n"
    );
    for (comment, place) in comments {
        let at = match place {
            Place::Line(n) => format!("{}:{n}", comment.path),
            Place::Outdated => format!("{} (the line has changed since)", comment.path),
        };
        let side = if comment.old { " (removed line)" } else { "" };
        out.push_str(&format!(
            "\n## {at}{side}\n\n```\n{}\n```\n\n{}\n\n_Written at {}._\n",
            comment.code.trim_end(),
            comment.body.trim(),
            comment.at
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(line: u32, code: &str) -> Comment {
        Comment {
            id: 1,
            path: "src/a.rs".into(),
            old: false,
            line,
            code: code.into(),
            body: "Clamp this.".into(),
            at: "c41d7be".into(),
        }
    }

    fn lines<'a>(text: &'a [&'a str]) -> impl Fn(u32) -> &'a str {
        move |n| text[n as usize - 1]
    }

    #[test]
    fn a_comment_stays_where_its_line_is_unchanged() {
        let text = ["a", "let x = 1;", "b"];
        assert_eq!(
            place(&comment(2, "let x = 1;"), 3, lines(&text)),
            Place::Line(2)
        );
    }

    #[test]
    fn a_comment_follows_its_line_when_the_file_shifts() {
        let text = ["new", "a", "let x = 1;", "b"];
        assert_eq!(
            place(&comment(2, "let x = 1;"), 4, lines(&text)),
            Place::Line(3)
        );
    }

    #[test]
    fn a_changed_or_repeated_line_is_outdated() {
        let text = ["let x = 2;", "y", "y"];
        assert_eq!(
            place(&comment(1, "let x = 1;"), 3, lines(&text)),
            Place::Outdated
        );
        // Two candidates: guessing would attach the comment to the wrong one.
        assert_eq!(place(&comment(9, "y"), 3, lines(&text)), Place::Outdated);
        assert_eq!(place(&comment(1, "  "), 3, lines(&text)), Place::Outdated);
    }

    #[test]
    fn the_export_names_file_line_code_and_comment() {
        let c = comment(2, "let x = 1;");
        let text = export(
            "payments",
            &[(c.clone(), Place::Line(2)), (c, Place::Outdated)],
        );
        assert!(text.contains("## src/a.rs:2\n"));
        assert!(text.contains("```\nlet x = 1;\n```"));
        assert!(text.contains("Clamp this."));
        assert!(text.contains("(the line has changed since)"));
    }

    #[test]
    fn comments_round_trip_as_json() {
        let c = vec![comment(2, "let x = 1;")];
        let body = serde_json::to_string_pretty(&c).unwrap();
        assert_eq!(serde_json::from_str::<Vec<Comment>>(&body).unwrap(), c);
    }
}
