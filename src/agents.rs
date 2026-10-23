//! GitLance and the coding agents' chats on this Mac: how a review reaches a chat and the agent's
//! answers come back. Everything goes through files in the data folder's `agents/`, so the app,
//! the MCP server (`crate::mcp`) and a chat's watcher (`crate::watch`) need not run at the same
//! time:
//!
//! - `chats/<session>.json` — a chat that listens: its watcher writes it and keeps it fresh.
//! - `answers/<repo>.jsonl` — what agents did with the comments, appended by the MCP server and
//!   applied by the app (each answer once, by its uid). Only the app writes the comments.
//!
//! A comment sent to a chat says so itself (`review::Sent`); the watcher hands over the comments
//! of its chat that are not delivered yet, then exits so the agent wakes.

use crate::review::{self, Comment, Reply, Sent, State};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// How long a watcher may go quiet and still count as listening.
pub const FRESH: i64 = 12;

/// A chat whose watcher runs (or ran).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chat {
    /// The agent's session id.
    pub session: String,
    /// "Claude Code", "Codex", …
    pub agent: String,
    /// The folder it works in: the project's main working tree, or a worktree of it.
    pub repo: PathBuf,
    /// The project's main working tree, where its notes are kept (#77); empty from an older
    /// watcher, which worked in the main one.
    #[serde(default)]
    pub project: PathBuf,
    /// The branch checked out in its folder: the notes of that branch go to it (#77).
    #[serde(default)]
    pub branch: String,
    /// Its first message, to tell chats apart.
    pub title: String,
    /// The agent's process, and the watcher's.
    pub pid: u32,
    pub watcher: u32,
    /// When the watcher last said it is there, seconds since the epoch.
    pub beat: i64,
}

/// What an agent did with a comment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub uid: String,
    pub id: u64,
    /// The batch the comment was in when the agent answered.
    pub batch: String,
    pub kind: Kind,
    #[serde(default)]
    pub body: String,
    /// A reply asking with a form (#10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<review::Form>,
    pub at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The chat's watcher handed the comment over.
    Delivered,
    /// A question or remark; the comment stays open.
    Reply,
    /// The agent says it is done.
    Addressed,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// A string no other answer or batch has.
pub fn uid() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{nanos:x}-{:x}", std::process::id())
}

/// The folder all of this lives in.
#[derive(Clone, Debug)]
pub struct Bus {
    dir: PathBuf,
}

impl Bus {
    /// The data folder's `agents/`.
    pub fn new() -> Option<Self> {
        Some(Self::at(crate::platform::data_dir()?.join("agents")))
    }

    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn chat_file(&self, session: &str) -> PathBuf {
        self.dir
            .join("chats")
            .join(format!("{}.json", safe(session)))
    }

    fn answers_file(&self, root: &Path) -> PathBuf {
        self.dir
            .join("answers")
            .join(format!("{}.jsonl", review::key(root)))
    }

    /// The comments file the app keeps, read as it is now.
    pub fn comments_file(&self, root: &Path) -> PathBuf {
        self.dir
            .parent()
            .unwrap_or(&self.dir)
            .join("comments")
            .join(format!("{}.json", review::key(root)))
    }

    /// Says the chat is listening (again).
    pub fn listen(&self, chat: &Chat) -> std::io::Result<()> {
        let file = self.chat_file(&chat.session);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Written beside and renamed, so a reader never sees half a file.
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string(chat).unwrap_or_default())?;
        std::fs::rename(tmp, file)
    }

    pub fn stop(&self, session: &str) {
        let _ = std::fs::remove_file(self.chat_file(session));
    }

    /// The chats that listened in project `root` (its main working tree), in any of its
    /// worktrees, newest first; a chat whose watcher stopped saying so is still there until the
    /// watcher removes it, and `listening` tells them apart.
    pub fn chats(&self, root: &Path) -> Vec<Chat> {
        let Ok(dir) = std::fs::read_dir(self.dir.join("chats")) else {
            return Vec::new();
        };
        let mut chats: Vec<Chat> = dir
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
            .filter(|c: &Chat| {
                c.project == root || (c.project.as_os_str().is_empty() && c.repo == root)
            })
            .collect();
        chats.sort_by_key(|c| std::cmp::Reverse(c.beat));
        chats
    }

    pub fn answer(&self, root: &Path, answer: &Answer) -> std::io::Result<()> {
        let file = self.answers_file(root);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut line = serde_json::to_string(answer).unwrap_or_default();
        line.push('\n');
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)?
            .write_all(line.as_bytes())
    }

    pub fn answers(&self, root: &Path) -> Vec<Answer> {
        std::fs::read_to_string(self.answers_file(root))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// When the answers last changed, to read them only then.
    pub fn answers_stamp(&self, root: &Path) -> Option<std::time::SystemTime> {
        std::fs::metadata(self.answers_file(root))
            .and_then(|m| m.modified())
            .ok()
    }

    /// The comments as the app saved them, with the agents' answers since applied.
    pub fn comments(&self, root: &Path) -> Vec<Comment> {
        let mut comments: Vec<Comment> = std::fs::read_to_string(self.comments_file(root))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        apply(&mut comments, &self.answers(root));
        comments
    }
}

impl Chat {
    pub fn listening(&self) -> bool {
        now() - self.beat <= FRESH
    }
}

fn safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Applies `answers` to `comments`; an answer already applied, or about an earlier sending of
/// its comment, changes nothing. Returns whether anything changed.
pub fn apply(comments: &mut [Comment], answers: &[Answer]) -> bool {
    let mut changed = false;
    for answer in answers {
        let Some(c) = comments.iter_mut().find(|c| c.id == answer.id) else {
            continue;
        };
        let Some(sent) = c.sent.as_mut().filter(|s| s.batch == answer.batch) else {
            continue;
        };
        match answer.kind {
            Kind::Delivered => {
                if !sent.delivered {
                    sent.delivered = true;
                    changed = true;
                }
            }
            Kind::Reply | Kind::Addressed => {
                if c.replies.iter().any(|r| r.uid == answer.uid) {
                    continue;
                }
                if !answer.body.trim().is_empty() {
                    c.replies.push(Reply {
                        uid: answer.uid.clone(),
                        body: answer.body.trim().to_owned(),
                        form: answer.form.clone(),
                        at: answer.at,
                    });
                    // A new question: the last answer was to the one before.
                    if answer.kind == Kind::Reply {
                        c.answer = None;
                    }
                }
                if answer.kind == Kind::Addressed && c.state == State::Sent {
                    c.state = State::Addressed;
                }
                changed = true;
            }
        }
    }
    changed
}

/// Marks `ids` as sent to `session` in one new batch; returns the batch.
pub fn send(comments: &mut [Comment], ids: &[u64], session: &str, title: &str) -> String {
    let batch = uid();
    for c in comments.iter_mut().filter(|c| ids.contains(&c.id)) {
        c.state = State::Sent;
        c.sent = Some(Sent {
            session: session.to_owned(),
            title: title.to_owned(),
            batch: batch.clone(),
            delivered: false,
        });
    }
    batch
}

/// Where a note on `branch` goes (#77): the chat picked for that branch (`chosen`) while it
/// listens, else the chat whose folder is on that branch, else the project's default — the newest
/// listening chat in the main working tree `main`, else the newest listening one. `chats` newest
/// first.
pub fn route<'a>(
    branch: Option<&str>,
    chosen: Option<&str>,
    chats: &'a [Chat],
    main: &Path,
) -> Option<&'a Chat> {
    let listening = || chats.iter().filter(|c| c.listening());
    chosen
        .and_then(|s| listening().find(|c| c.session == s))
        .or_else(|| branch.and_then(|b| listening().find(|c| c.branch == b)))
        .or_else(|| listening().find(|c| c.repo == main))
        .or_else(|| listening().next())
}

/// Who is running this: the agent, its session and its process, from what it puts in a command's
/// environment (the chat's watcher and the MCP server, #77).
pub fn whoami() -> Option<(String, String, u32, Option<PathBuf>)> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    if let Some(session) = var("CLAUDE_CODE_SESSION_ID") {
        let pid = var("CLAUDE_PID").and_then(|p| p.parse().ok()).unwrap_or(0);
        let config = var("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| Some(std::env::home_dir()?.join(".claude")));
        return Some(("Claude Code".into(), session, pid, config));
    }
    if let Some(session) = var("CODEX_SESSION_ID").or_else(|| var("CODEX_THREAD_ID")) {
        return Some(("Codex".into(), session, 0, None));
    }
    None
}

/// The comments waiting for `session`: sent to it and not handed over yet.
pub fn waiting<'a>(comments: &'a [Comment], session: &str) -> Vec<&'a Comment> {
    comments
        .iter()
        .filter(|c| c.state == State::Sent)
        .filter(|c| {
            c.sent
                .as_ref()
                .is_some_and(|s| s.session == session && !s.delivered)
        })
        .collect()
}

/// Claude Code's chats in `root` that are not listening: from its transcripts, touched in the last
/// day, newest first, as (session, title). Each config folder (`~/.claude`, `~/.claude-*`) counts.
pub fn claude_chats(root: &Path) -> Vec<(String, String)> {
    let Some(home) = std::env::home_dir() else {
        return Vec::new();
    };
    let folder = claude_folder(root);
    let mut found: Vec<(std::time::SystemTime, String, PathBuf)> = Vec::new();
    for config in std::fs::read_dir(&home).into_iter().flatten().flatten() {
        let name = config.file_name().to_string_lossy().into_owned();
        if name != ".claude" && !name.starts_with(".claude-") {
            continue;
        }
        let dir = config.path().join("projects").join(&folder);
        for file in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = file.path();
            if path.extension().is_none_or(|x| x != "jsonl") {
                continue;
            }
            let Ok(modified) = file.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            let day = std::time::Duration::from_secs(24 * 3600);
            if modified.elapsed().is_ok_and(|age| age > day) {
                continue;
            }
            let session = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            found.push((modified, session, path));
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found
        .into_iter()
        .take(8)
        .map(|(_, session, path)| (session, title(&path)))
        .collect()
}

/// Where Claude Code keeps the transcripts of chats started in `dir`: the path with every
/// character but letters and digits made `-`.
pub fn claude_folder(dir: &Path) -> String {
    dir.display()
        .to_string()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
}

/// A chat's first message, shortened: what the person asked it first.
pub fn title(transcript: &Path) -> String {
    let text = std::fs::read_to_string(transcript).unwrap_or_default();
    for line in text.lines().take(400) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value["type"] != "user" {
            continue;
        }
        let content = &value["message"]["content"];
        let said = match content {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(parts) => parts
                .iter()
                .filter(|p| p["type"] == "text")
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join(" "),
            _ => String::new(),
        };
        let said = said.split_whitespace().collect::<Vec<_>>().join(" ");
        // Commands, hooks and reminders are written in tags; the person's words are not.
        if said.is_empty() || said.starts_with('<') || said.starts_with("Caveat:") {
            continue;
        }
        return shorten(&said, 60);
    }
    "A chat without messages yet".to_owned()
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max - 1).collect();
    format!("{}…", cut.trim_end())
}

/// The command an agent runs for GitLance's MCP server: this program with `mcp`.
pub fn server_command() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "gitlance".into());
    format!("\"{exe}\" mcp")
}

/// Claude Code's config folders on this machine: `~/.claude` and every `~/.claude-*` profile
/// (`CLAUDE_CONFIG_DIR`), each with its own MCP servers.
pub fn claude_profiles() -> Vec<PathBuf> {
    let Some(home) = std::env::home_dir() else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = std::fs::read_dir(&home)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name == ".claude" || name.starts_with(".claude-"))
                && e.path().join("projects").is_dir()
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// The `claude` program: on the PATH, or where its installers put it (an app opened from the
/// Finder does not get the shell's PATH).
fn claude_program() -> Option<PathBuf> {
    let home = std::env::home_dir()?;
    let known = [
        home.join(".local/bin/claude"),
        home.join(".claude/local/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ];
    let on_path = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("claude"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    on_path.into_iter().chain(known).find(|p| p.is_file())
}

/// Adds GitLance's MCP server to every Claude Code profile, for every project (user scope),
/// replacing an older entry. Returns what happened, per profile.
pub fn connect_claude() -> Result<String, String> {
    let claude = claude_program().ok_or("Claude Code's `claude` was not found.")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let profiles = claude_profiles();
    let mut done = Vec::new();
    for profile in &profiles {
        let env = |c: &mut std::process::Command| {
            // `~/.claude` is the default profile: no variable names it.
            if profile.file_name().is_some_and(|n| n != ".claude") {
                c.env("CLAUDE_CONFIG_DIR", profile);
            }
        };
        let mut remove = std::process::Command::new(&claude);
        remove.args(["mcp", "remove", "-s", "user", "gitlance"]);
        env(&mut remove);
        let _ = remove.output();
        let mut add = std::process::Command::new(&claude);
        add.args(["mcp", "add", "-s", "user", "gitlance", "--"])
            .arg(&exe)
            .arg("mcp");
        env(&mut add);
        let out = add.output().map_err(|e| e.to_string())?;
        let name = profile
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if out.status.success() {
            done.push(name);
        } else {
            return Err(format!(
                "{name}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
    }
    if done.is_empty() {
        return Err("No Claude Code profile found (~/.claude).".into());
    }
    Ok(format!(
        "Connected: {}. New chats start the watcher; open chats need a restart or “listen to GitLance”.",
        done.join(", ")
    ))
}

/// The comments of `root` the agent should work on, as markdown with their ids: what it was sent,
/// placed on the lines of the working tree as it is now.
pub fn brief(root: &Path, comments: &[&Comment]) -> String {
    if comments.is_empty() {
        return "No review comments are waiting. The reviewer sends them from GitLance.".to_owned();
    }
    let mut out = format!(
        "{} review comment(s) from GitLance. Change the code for each, then call address_comment with its id and one line on what you did (or reply_comment to ask the reviewer). Line numbers are of the working tree now.\n",
        comments.len()
    );
    for c in comments {
        let at = if c.old {
            format!("{} (a removed line)", c.path)
        } else {
            let text = std::fs::read_to_string(root.join(&c.path)).unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            match review::place(c, lines.len() as u32, |n| lines[n as usize - 1]) {
                review::Place::Line(n) => c.at_line(n),
                review::Place::Outdated => format!("{} (the line has changed since)", c.path),
            }
        };
        out.push_str(&format!(
            "\n## Comment {} — {at}\n\n```\n{}\n```\n\n{}\n",
            c.id,
            c.quoted(),
            c.body.trim()
        ));
        if let Some(thread) = &c.thread {
            out.push_str(&format!(
                "\nThis note is on a discussion of the merge request (the reviewer's own, not posted):\n{thread}\n"
            ));
        }
        for r in &c.replies {
            out.push_str(&format!("\n> Your earlier answer: {}\n", r.body));
        }
        if let Some(a) = &c.answer {
            let form = c.replies.last().and_then(|r| r.form.as_ref());
            out.push_str(&format!("\nThe reviewer answered:\n{}\n", a.words(form)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(session: &str, repo: &str, branch: &str, listening: bool) -> Chat {
        Chat {
            session: session.into(),
            repo: repo.into(),
            project: "/p".into(),
            branch: branch.into(),
            beat: if listening { now() } else { now() - 3600 },
            ..Default::default()
        }
    }

    #[test]
    fn a_note_goes_to_the_chat_on_its_branch_else_the_default() {
        let main = Path::new("/p");
        // Newest first: a chat in a worktree on feature/x, one in the main tree on main.
        let chats = [
            chat("w", "/p-x", "feature/x", true),
            chat("m", "/p", "main", true),
            chat("gone", "/p-y", "feature/y", false),
        ];
        let to = |branch, chosen| route(branch, chosen, &chats, main).map(|c| c.session.as_str());
        assert_eq!(to(Some("feature/x"), None), Some("w"));
        assert_eq!(to(Some("main"), None), Some("m"));
        // No chat on its branch (its chat is gone): the main tree's.
        assert_eq!(to(Some("feature/y"), None), Some("m"));
        assert_eq!(to(None, None), Some("m"));
        // Picked with Change…, while it listens.
        assert_eq!(to(Some("feature/x"), Some("m")), Some("m"));
        assert_eq!(to(Some("feature/x"), Some("gone")), Some("w"));
        // None listens: nowhere.
        assert_eq!(route(Some("main"), None, &chats[2..], main), None);
    }

    fn comment(id: u64) -> Comment {
        Comment {
            id,
            path: "src/a.rs".into(),
            line: 2,
            code: "let x = 1;".into(),
            body: "Clamp this.".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_question_with_a_form_is_kept_and_the_answer_goes_with_the_next_send() {
        let form: review::Form = serde_json::from_value(serde_json::json!({
            "kind": "one",
            "options": [{ "label": "Only after a failure" }, { "label": "In both cases" }]
        }))
        .unwrap();
        let mut comments = vec![comment(1)];
        let batch = send(&mut comments, &[1], "s1", "Fix login");
        let ask = Answer {
            uid: "u1".into(),
            id: 1,
            batch,
            kind: Kind::Reply,
            body: "When should lastForm be cleared?".into(),
            form: Some(form),
            at: 2,
        };
        assert!(apply(&mut comments, &[ask]));
        assert_eq!(
            comments[0].replies[0].form.as_ref().unwrap().options.len(),
            2
        );
        comments[0].answer = Some(review::Answered {
            picks: vec![1],
            note: "and on sign-out".into(),
            ..Default::default()
        });
        comments[0].thread = Some("> Ember: is this cleared?".into());
        let text = brief(Path::new("/nope"), &[&comments[0]]);
        assert!(text.contains("The reviewer answered:\nPicked: In both cases\nand on sign-out"));
        assert!(text.contains("> Ember: is this cleared?"));
        let confirm = review::Form {
            kind: review::FormKind::Confirm,
            ..Default::default()
        };
        assert_eq!(confirm.choices().len(), 2);
    }

    #[test]
    fn a_sent_comment_waits_for_its_chat_until_delivered() {
        let mut comments = vec![comment(1), comment(2)];
        let batch = send(&mut comments, &[1], "s1", "Fix login");
        assert_eq!(waiting(&comments, "s1").len(), 1);
        assert!(waiting(&comments, "s2").is_empty());
        let delivered = Answer {
            uid: uid(),
            id: 1,
            batch,
            kind: Kind::Delivered,
            body: String::new(),
            form: None,
            at: 0,
        };
        assert!(apply(&mut comments, std::slice::from_ref(&delivered)));
        assert!(waiting(&comments, "s1").is_empty());
        assert!(!apply(&mut comments, &[delivered]));
    }

    #[test]
    fn an_answer_counts_once_and_only_for_its_sending() {
        let mut comments = vec![comment(1)];
        let first = send(&mut comments, &[1], "s1", "");
        let done = Answer {
            uid: "u1".into(),
            id: 1,
            batch: first,
            kind: Kind::Addressed,
            body: "Clamped to 0..100.".into(),
            form: None,
            at: 5,
        };
        apply(&mut comments, std::slice::from_ref(&done));
        apply(&mut comments, std::slice::from_ref(&done));
        assert_eq!(comments[0].state, State::Addressed);
        assert_eq!(comments[0].replies.len(), 1);
        // Reopened and sent again: the old answer is about the first sending.
        comments[0].state = State::Draft;
        send(&mut comments, &[1], "s1", "");
        let mut again = done.clone();
        again.uid = "u2".into();
        apply(&mut comments, &[again]);
        assert_eq!(comments[0].state, State::Sent);
    }

    #[test]
    fn chats_listen_while_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Bus::at(dir.path().join("agents"));
        let root = PathBuf::from("/r");
        let chat = Chat {
            session: "s1".into(),
            agent: "Claude Code".into(),
            repo: root.clone(),
            beat: now(),
            ..Default::default()
        };
        bus.listen(&chat).unwrap();
        let chats = bus.chats(&root);
        assert_eq!(chats.len(), 1);
        assert!(chats[0].listening());
        assert!(bus.chats(Path::new("/other")).is_empty());
        bus.stop("s1");
        assert!(bus.chats(&root).is_empty());
    }

    #[test]
    fn answers_reach_the_comments_the_app_saved() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Bus::at(dir.path().join("agents"));
        let root = PathBuf::from("/r");
        let mut comments = vec![comment(1)];
        let batch = send(&mut comments, &[1], "s1", "");
        let file = bus.comments_file(&root);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, serde_json::to_string(&comments).unwrap()).unwrap();
        let answer = Answer {
            uid: uid(),
            id: 1,
            batch,
            kind: Kind::Addressed,
            body: "Done.".into(),
            form: None,
            at: 1,
        };
        bus.answer(&root, &answer).unwrap();
        assert_eq!(bus.comments(&root)[0].state, State::Addressed);
    }

    #[test]
    fn a_transcript_is_named_by_the_first_thing_said() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.jsonl");
        std::fs::write(
            &file,
            [
                r#"{"type":"summary","summary":"x"}"#,
                r#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#,
                r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Fix the   login flow on mobile"}]}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        assert_eq!(title(&file), "Fix the login flow on mobile");
        assert_eq!(
            claude_folder(Path::new("/Users/a/My.Repo")),
            "-Users-a-My-Repo"
        );
    }
}
