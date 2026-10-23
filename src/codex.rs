//! Codex's chats (#18, limited). Codex has no background command that wakes it when it ends, as
//! Claude Code has, so the app is the watcher for it: on the app's beat, the session of the project
//! that a Codex running in a terminal is on — the one `codex resume <id>` named, else the newest
//! written in that terminal's folder since that Codex started (one not written yet is not taken
//! for an older one) — is written to the agents' bus as a listening chat, and taken off it when
//! that Codex quits. Codex says nothing when it closes, and its app-server keeps a session
//! loaded after the terminal is gone, so the process is the sign (the developer, 2026-10-23: two
//! sessions quit with ⌃C still listened). The comments sent to one are handed over with
//! `codex queue`, which starts a turn in that session: the brief Claude Code's watcher points to,
//! answered with the MCP tools Codex already has.

use crate::agents::{self, Answer, Bus, Chat, Kind};
use serde_json::Value;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// How far back a session may have last written and still be looked at.
const RECENT: Duration = Duration::from_secs(7 * 24 * 3600);

/// The agent's name on the bus.
const AGENT: &str = "Codex";

/// What a Codex chat cannot do yet, where it is picked (the developer, 2026-10-23: say its
/// problems rather than hide it).
pub const LIMITS: &str = "Codex is limited here: GitLance sees a Codex running in a terminal and the session it has written to since it started; sending starts a turn in that session (codex queue). Not seen: Codex in VS Code or the Codex app, nor a session before its first message. A turn sent as its terminal closes may run on unseen.";

/// `codex`'s commands that are not a session in a terminal (`codex --help`); `resume` and `fork`
/// are one, and so is a prompt.
const NOT_A_SESSION: [&str; 28] = [
    "agents",
    "exec",
    "e",
    "review",
    "login",
    "logout",
    "mcp",
    "plugin",
    "app-server",
    "remote-control",
    "app",
    "completion",
    "update",
    "doctor",
    "sandbox",
    "debug",
    "apply",
    "a",
    "queue",
    "archive",
    "delete",
    "migrate-rollouts",
    "migrate-nfs-db",
    "unarchive",
    "cloud",
    "exec-server",
    "features",
    "help",
];

/// Codex's own folder: `CODEX_HOME`, else `~/.codex`.
fn home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| Some(std::env::home_dir()?.join(".codex")))
}

/// The `codex` program; an app started from the Finder has no shell's PATH.
fn program() -> Option<PathBuf> {
    let home = std::env::home_dir()?;
    let known = [
        home.join(".local/bin/codex"),
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
    ];
    let on_path = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("codex"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    on_path.into_iter().chain(known).find(|p| p.is_file())
}

/// The sessions' rollouts written lately, `sessions/<year>/<month>/<day>/rollout-*.jsonl`, with
/// when.
fn recent_rollouts(home: &Path) -> Vec<(PathBuf, SystemTime)> {
    let now = SystemTime::now();
    let mut out = Vec::new();
    let mut dirs = vec![(home.join("sessions"), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if depth < 3 {
                if path.is_dir() {
                    dirs.push((path, depth + 1));
                }
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(at) = e.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            let fresh = now.duration_since(at).unwrap_or_default() < RECENT;
            if fresh && name.starts_with("rollout-") && name.ends_with(".jsonl") {
                out.push((path, at));
            }
        }
    }
    out
}

/// A rollout's first line: the session's id and the folder it works in.
fn meta(first: &str) -> Option<(String, PathBuf)> {
    let line: Value = serde_json::from_str(first).ok()?;
    if line["type"] != "session_meta" {
        return None;
    }
    let p = &line["payload"];
    Some((
        p["id"].as_str()?.to_owned(),
        PathBuf::from(p["cwd"].as_str()?),
    ))
}

/// The first line of `file` alone: a rollout grows to megabytes.
fn first_line(file: &Path) -> Option<String> {
    let mut line = String::new();
    std::io::BufReader::new(std::fs::File::open(file).ok()?)
        .read_line(&mut line)
        .ok()?;
    Some(line)
}

/// The sessions' names (`session_index.jsonl`, the newest line of an id wins).
fn names(index: &str) -> HashMap<String, String> {
    index
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| {
            Some((
                v["id"].as_str()?.to_owned(),
                v["thread_name"].as_str().unwrap_or_default().to_owned(),
            ))
        })
        .collect()
}

/// A session id: 36 characters, hex in five groups.
fn is_id(word: &str) -> bool {
    word.len() == 36
        && word.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// A command line, when it is Codex in a terminal: the session it resumes, when it names one.
fn session_of(command: &str) -> Option<Option<String>> {
    let mut words = command.split_whitespace();
    if Path::new(words.next()?).file_name()? != "codex" {
        return None;
    }
    let rest: Vec<&str> = words.collect();
    match rest.iter().find(|w| !w.starts_with('-')) {
        Some(w) if NOT_A_SESSION.contains(w) => None,
        Some(&"resume") => Some(rest.iter().find(|w| is_id(w)).map(|w| (*w).to_owned())),
        _ => Some(None),
    }
}

/// `ps`'s elapsed time — `mm:ss`, `hh:mm:ss` or `dd-hh:mm:ss` — in seconds.
fn elapsed(etime: &str) -> Option<u64> {
    let (days, clock) = match etime.split_once('-') {
        Some((d, rest)) => (d.parse::<u64>().ok()?, rest),
        None => (0, etime),
    };
    let mut secs = 0;
    for part in clock.split(':') {
        secs = secs * 60 + part.parse::<u64>().ok()?;
    }
    Some(days * 86_400 + secs)
}

/// A Codex running in a terminal: its folder, the session it resumed when it named one, and when
/// it started.
struct Terminal {
    folder: PathBuf,
    thread: Option<String>,
    started: SystemTime,
}

/// The Codex sessions running in terminals now. None where `ps` and `lsof` are missing (Windows).
fn terminals() -> Vec<Terminal> {
    let Ok(out) = Command::new("/bin/ps")
        .args(["-axo", "pid=,etime=,command="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let now = SystemTime::now();
    let running: Vec<(String, Option<String>, SystemTime)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (pid, rest) = l.trim_start().split_once(' ')?;
            let (etime, command) = rest.trim_start().split_once(' ')?;
            let started = now - Duration::from_secs(elapsed(etime)?);
            Some((pid.to_owned(), session_of(command.trim())?, started))
        })
        .collect();
    if running.is_empty() {
        return Vec::new();
    }
    // Their folders, in one look: `p<pid>` then `n<folder>` lines.
    let lsof = ["/usr/sbin/lsof", "/usr/bin/lsof"]
        .into_iter()
        .map(Path::new)
        .find(|p| p.is_file())
        .unwrap_or(Path::new("lsof"));
    let pids: Vec<&str> = running.iter().map(|(p, ..)| p.as_str()).collect();
    let Ok(out) = Command::new(lsof)
        .args(["-a", "-d", "cwd", "-F", "pn", "-p", &pids.join(",")])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let mut folders = HashMap::new();
    let mut pid = "";
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p;
        } else if let Some(folder) = line.strip_prefix('n') {
            folders.insert(pid.to_owned(), PathBuf::from(folder));
        }
    }
    running
        .into_iter()
        .filter_map(|(pid, thread, started)| {
            Some(Terminal {
                folder: folders.get(&pid)?.clone(),
                thread,
                started,
            })
        })
        .collect()
}

/// Codex's sessions in project `project` (its main working tree) or a worktree of it that a
/// Codex in a terminal is on now, as chats that listen.
pub fn chats(project: &Path) -> Vec<Chat> {
    let Some(home) = home() else {
        return Vec::new();
    };
    let mut terminals = terminals();
    if terminals.is_empty() {
        return Vec::new();
    }
    let mut repos: HashMap<PathBuf, Option<crate::git::Repo>> = HashMap::new();
    let mut in_project = |cwd: &Path| {
        repos
            .entry(cwd.to_owned())
            .or_insert_with(|| crate::git::Repo::open(cwd).ok())
            .as_ref()
            .is_some_and(|r| r.main_root() == project)
    };
    // The project's sessions, the newest written first.
    let mut sessions: Vec<(SystemTime, String, PathBuf)> = recent_rollouts(&home)
        .into_iter()
        .filter_map(|(file, at)| {
            let (id, cwd) = meta(&first_line(&file)?)?;
            Some((at, id, cwd))
        })
        .filter(|(_, _, cwd)| in_project(cwd))
        .collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.0));
    // One that resumed a session by name takes it first; the others the newest in their folder
    // written since they started — a Codex not written to yet is on a session not saved yet,
    // and an older one is not it.
    terminals.sort_by_key(|t| t.thread.is_none());
    let mut on: Vec<(String, PathBuf)> = Vec::new();
    for t in &terminals {
        let free = |id: &String| !on.iter().any(|(o, _)| o == id);
        let since = t.started - Duration::from_secs(2);
        let pick = match &t.thread {
            Some(id) => sessions.iter().find(|(_, s, _)| s == id && free(s)),
            None => sessions
                .iter()
                .find(|(at, s, cwd)| *cwd == t.folder && *at >= since && free(s)),
        };
        if let Some((_, id, cwd)) = pick {
            on.push((id.clone(), cwd.clone()));
        }
    }
    let names =
        names(&std::fs::read_to_string(home.join("session_index.jsonl")).unwrap_or_default());
    on.into_iter()
        .filter_map(|(session, cwd)| {
            let repo = crate::git::Repo::open(&cwd).ok()?;
            Some(Chat {
                title: names.get(&session).cloned().unwrap_or_default(),
                session,
                agent: AGENT.to_owned(),
                repo: repo.root(),
                project: project.to_owned(),
                branch: repo.head_branch().unwrap_or_default(),
                pid: 0,
                watcher: 0,
                beat: agents::now(),
            })
        })
        .collect()
}

/// One beat for project `project`: its open Codex sessions listen, the closed ones go, and what
/// was sent to one is handed to it.
pub fn beat(bus: &Bus, project: &Path) {
    let live = chats(project);
    for chat in &live {
        let _ = bus.listen(chat);
    }
    // Only the ones the app wrote: a watcher run inside Codex keeps its own.
    for gone in bus
        .chats(project)
        .into_iter()
        .filter(|c| c.agent == AGENT && c.watcher == 0)
        .filter(|c| !live.iter().any(|l| l.session == c.session))
    {
        bus.stop(&gone.session);
    }
    if live.is_empty() {
        return;
    }
    let comments = bus.comments(project);
    for chat in &live {
        let waiting = agents::waiting(&comments, &chat.session);
        if waiting.is_empty() {
            continue;
        }
        // Another window of this project may be at it this very beat.
        let Some(_claim) = bus.claim(&format!("codex-{}", chat.session)) else {
            continue;
        };
        let brief = agents::brief(&chat.repo, &waiting);
        if let Err(e) = queue(&chat.session, &brief) {
            crate::applog::line(&format!("codex: {e}"));
            continue;
        }
        for c in &waiting {
            let _ = bus.answer(
                project,
                &Answer {
                    uid: agents::uid(),
                    id: c.id,
                    batch: c.sent.as_ref().map(|s| s.batch.clone()).unwrap_or_default(),
                    kind: Kind::Delivered,
                    body: String::new(),
                    form: None,
                    at: agents::now(),
                },
            );
        }
    }
}

/// `message` into session `thread`: a turn starts in it, or after the one it is in.
fn queue(thread: &str, message: &str) -> Result<(), String> {
    let codex = program().ok_or("codex is not installed")?;
    let out = Command::new(codex)
        .args(["queue", "--thread", thread, "--message", message])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // It says "Error: …" and still exits 0.
    if !out.status.success() || said.contains("Error:") {
        return Err(said.trim().to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rollout_names_its_session_and_folder() {
        let first = r#"{"timestamp":"2026-10-23T15:12:06Z","type":"session_meta","payload":{"id":"01a1-x","session_id":"01a1-x","cwd":"/p/app","originator":"codex-tui"}}"#;
        assert_eq!(
            meta(first),
            Some(("01a1-x".to_owned(), PathBuf::from("/p/app")))
        );
        assert_eq!(meta(r#"{"type":"response_item","payload":{}}"#), None);
        assert_eq!(meta("not json"), None);
    }

    #[test]
    fn a_codex_in_a_terminal_is_told_from_its_other_commands() {
        let id = "01a16951-bcaa-77f1-82d4-6c3f73f7d8b8";
        let bin = "/Users/k/.codex/packages/standalone/releases/0.164.0/bin/codex";
        assert_eq!(session_of(bin), Some(None));
        assert_eq!(
            session_of(&format!("{bin} -m gpt-5 fix the build")),
            Some(None)
        );
        assert_eq!(
            session_of(&format!("codex resume {id}")),
            Some(Some(id.to_owned()))
        );
        assert_eq!(session_of("codex resume --last"), Some(None));
        assert_eq!(session_of("codex fork"), Some(None));
        // The app-server, a queue the app runs, a non-interactive run, another program.
        assert_eq!(
            session_of(&format!("{bin} app-server --listen unix://")),
            None
        );
        assert_eq!(
            session_of(&format!("codex queue --thread {id} --message hi")),
            None
        );
        assert_eq!(session_of("codex exec do it"), None);
        assert_eq!(session_of("/x/codex-code-mode-host"), None);
        assert_eq!(session_of("/bin/zsh -c codex"), None);
        assert!(is_id(id) && !is_id("01a16951") && !is_id(&id.replace('-', "x")));
    }

    #[test]
    fn a_codex_started_this_long_ago() {
        assert_eq!(elapsed("05:07"), Some(307));
        assert_eq!(elapsed("02:10:22"), Some(2 * 3600 + 10 * 60 + 22));
        assert_eq!(
            elapsed("86-23:45:45"),
            Some(86 * 86_400 + 23 * 3600 + 45 * 60 + 45)
        );
        assert_eq!(elapsed("soon"), None);
    }

    #[test]
    fn the_newest_name_of_a_session_wins() {
        let index = "{\"id\":\"a\",\"thread_name\":\"First\"}\n\
                     {\"id\":\"b\",\"thread_name\":\"Other\"}\n\
                     {\"id\":\"a\",\"thread_name\":\"Renamed\"}\n\
                     junk\n";
        let names = names(index);
        assert_eq!(names["a"], "Renamed");
        assert_eq!(names["b"], "Other");
    }

    #[test]
    fn only_rollouts_are_looked_at() {
        let home = std::env::temp_dir().join(format!("gitlance-codex-{}", agents::uid()));
        let day = home.join("sessions/2026/10/23");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join("rollout-2026-10-23T19-12-06-x.jsonl"), "{}").unwrap();
        std::fs::write(day.join("notes.txt"), "").unwrap();
        std::fs::write(home.join("sessions/rollout-misplaced.jsonl"), "").unwrap();
        let found: Vec<PathBuf> = recent_rollouts(&home).into_iter().map(|(f, _)| f).collect();
        assert_eq!(found, [day.join("rollout-2026-10-23T19-12-06-x.jsonl")]);
        std::fs::remove_dir_all(home).ok();
    }
}
