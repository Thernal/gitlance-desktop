//! Line comments for a coding agent: kept per repository on this Mac (never in the repository),
//! attached to a file and to the text of a line rather than its number, so a rebase does not
//! detach them, and exported as markdown to paste into an agent.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub path: String,
    /// On the removed side of the diff.
    pub old: bool,
    /// The line number when it was written; the text below decides where it belongs now.
    pub line: u32,
    /// The commented line, as it was.
    pub code: String,
    /// A comment on several lines: the ones above `line`, as they were. The comment follows its
    /// last line; the range is that many lines above it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub above: Vec<String>,
    pub body: String,
    /// Where it was written: a commit or a version range, for the reader of the export.
    pub at: String,
    /// The commit its line was read from, so the line can be followed when it changes (#46).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Where it is in its life with an agent.
    #[serde(default)]
    pub state: State,
    /// The chat it was sent to, once sent (`crate::agents`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent: Option<Sent>,
    /// What the agent said about it since.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<Reply>,
    /// The reviewer's answer to the agent's last question, to go with the next send (#10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Answered>,
    /// Written on a GitLab / GitHub discussion (#50 G): that discussion as the agent should read
    /// it. Kept on this Mac, never posted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// The branch of the page it was written on: it goes to the chat working on that branch (#77).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Put away once it was read, addressed or resolved: out of the lists and off its line (#76).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
    /// Written as an answer to the agent's own note on this line (#151): that note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<String>,
}

/// A small form an agent asks with (#10 C, Design `design/next/agent-answer/kinds.html`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Form {
    pub kind: FormKind,
    /// The choices (one, many, plan, files, code, scale, line, text's suggestions, order's items).
    #[serde(default)]
    pub options: Vec<FormOption>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormKind {
    /// One of the options.
    #[default]
    One,
    /// Any of them.
    Many,
    /// Yes or no: the first two options (a default pair when none are given).
    Confirm,
    /// Steps to approve, each on or off.
    Plan,
    /// Versions of code, one to pick, stacked.
    Code,
    /// Files to touch, each on or off.
    Files,
    /// A name or text: suggestions to pick, or one's own in the note.
    Text,
    /// The items in the order they should go.
    Order,
    /// How far: one of a range of scopes.
    Scale,
    /// Which line: one of the lines given.
    Line,
}

impl FormKind {
    /// Whether several options can be on at once.
    pub fn many(self) -> bool {
        matches!(self, FormKind::Many | FormKind::Plan | FormKind::Files)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// For code: the lines of this version.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub code: Vec<String>,
    /// On to begin with (plan steps, files).
    #[serde(default)]
    pub on: bool,
}

impl Form {
    /// The options as drawn: Confirm's default pair when the agent gave none.
    pub fn choices(&self) -> Vec<FormOption> {
        if self.kind == FormKind::Confirm && self.options.len() < 2 {
            return ["Yes", "No"]
                .map(|l| FormOption {
                    label: l.to_owned(),
                    ..Default::default()
                })
                .to_vec();
        }
        self.options.clone()
    }
}

/// The reviewer's answer: the options picked (for Order, the new order), "your call", a note.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answered {
    #[serde(default)]
    pub picks: Vec<usize>,
    #[serde(default)]
    pub your_call: bool,
    #[serde(default)]
    pub note: String,
    /// Text's own value, typed in the form's field instead of a suggestion (#10 C).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
}

impl Answered {
    /// The answer in words, for the agent: what was picked, then the note.
    pub fn words(&self, form: Option<&Form>) -> String {
        let mut parts = Vec::new();
        if self.your_call {
            parts.push("Your call — decide yourself.".to_owned());
        } else if !self.value.trim().is_empty() {
            parts.push(format!("Use: {}", self.value.trim()));
        } else if let Some(form) = form {
            let choices = form.choices();
            let picked: Vec<String> = self
                .picks
                .iter()
                .filter_map(|&i| choices.get(i).map(|c| c.label.clone()))
                .collect();
            if !picked.is_empty() {
                parts.push(match form.kind {
                    FormKind::Order => format!("In this order: {}", picked.join(" → ")),
                    k if k.many() => format!("Picked: {}", picked.join("; ")),
                    _ => format!("Picked: {}", picked[0]),
                });
            }
        }
        if !self.note.trim().is_empty() {
            parts.push(self.note.trim().to_owned());
        }
        parts.join("\n")
    }
}

/// A comment's life with an agent: written, sent to a chat, done by the agent, confirmed by the
/// reviewer. Reopening puts it back to a draft, to be sent again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Draft,
    Sent,
    /// The agent says it is done; the reviewer has not looked yet.
    Addressed,
    Resolved,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sent {
    /// The chat's session id; empty for "whichever chat asks" (a new chat).
    pub session: String,
    /// What the reviewer saw the chat as, for the comment's line.
    pub title: String,
    /// The batch it went in: answers name it, so an old answer never touches a resent comment.
    pub batch: String,
    /// The chat's watcher has handed it over.
    #[serde(default)]
    pub delivered: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    /// Unique, so an answer applied twice is applied once.
    pub uid: String,
    pub body: String,
    /// The form the agent asked with (#10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<Form>,
    /// Seconds since the epoch.
    pub at: i64,
    /// The reviewer's answer to its form, kept when the agent asked again: an earlier question
    /// still shows what was picked (the developer, 2026-10-23: its options went).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered: Option<Answered>,
}

impl Comment {
    /// Still for the agent: not done by it, not confirmed by the reviewer.
    pub fn open(&self) -> bool {
        matches!(self.state, State::Draft | State::Sent)
    }

    /// `path:line`, or `path:first-last` for several lines, its last line now on `line`.
    pub fn at_line(&self, line: u32) -> String {
        match self.above.len() as u32 {
            0 => format!("{}:{line}", self.path),
            n => format!("{}:{}-{line}", self.path, line.saturating_sub(n)),
        }
    }

    /// A note on a whole folder (#97): its path ends with a slash, it has no line.
    pub fn is_folder(&self) -> bool {
        self.path.ends_with('/')
    }

    /// The commented code as it was: the lines above, then the line.
    pub fn quoted(&self) -> String {
        let mut lines: Vec<&str> = self.above.iter().map(String::as_str).collect();
        lines.push(&self.code);
        lines.join("\n").trim_end().to_owned()
    }
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
    find_line(comment.line, &comment.code, count, line)
}

/// Line `at` reading `code`, found again in a side of `count` lines: there if it still reads the
/// same, else the one line that does; ambiguous or missing means outdated. An agent's notes on its
/// changes are found the same way (#151).
pub fn find_line<'a>(at: u32, code: &str, count: u32, line: impl Fn(u32) -> &'a str) -> Place {
    if at >= 1 && at <= count && line(at) == code {
        return Place::Line(at);
    }
    let mut found = (1..=count).filter(|&n| line(n) == code);
    match (found.next(), found.next()) {
        (Some(only), None) if !code.trim().is_empty() => Place::Line(only),
        _ => Place::Outdated,
    }
}

/// The repository's name in the data folder's file names.
pub fn key(root: &Path) -> String {
    root.display()
        .to_string()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

fn store(root: &Path) -> String {
    format!("comments/{}.json", key(root))
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
        if comment.is_folder() {
            out.push_str(&format!(
                "\n## {} (the folder and everything in it)\n\n{}\n\n_Written at {}._\n",
                comment.path,
                comment.body.trim(),
                comment.at
            ));
            continue;
        }
        let at = match place {
            Place::Line(n) => comment.at_line(*n),
            Place::Outdated => format!("{} (the line has changed since)", comment.path),
        };
        let side = if comment.old { " (removed line)" } else { "" };
        out.push_str(&format!(
            "\n## {at}{side}\n\n```\n{}\n```\n\n{}\n\n_Written at {}._\n",
            comment.quoted(),
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
            ..Default::default()
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
    fn a_comment_on_several_lines_names_and_quotes_them_all() {
        let mut c = comment(12, "    x");
        c.above = vec!["fn f() {".into(), "    let y = 2;".into()];
        assert_eq!(c.at_line(14), "src/a.rs:12-14");
        assert_eq!(c.quoted(), "fn f() {\n    let y = 2;\n    x");
        let export = export("repo", &[(c, Place::Line(14))]);
        assert!(export.contains("## src/a.rs:12-14"));
    }

    #[test]
    fn comments_round_trip_as_json() {
        let c = vec![comment(2, "let x = 1;")];
        let body = serde_json::to_string_pretty(&c).unwrap();
        assert_eq!(serde_json::from_str::<Vec<Comment>>(&body).unwrap(), c);
    }
}
