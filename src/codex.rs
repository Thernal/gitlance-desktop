//! Codex's chats (#18). Codex has no background command that wakes it when it ends, as Claude
//! Code has, so the app is the watcher for it: on the app's beat, a Codex session open now in the
//! project — its rollout in `~/.codex/sessions` held open by Codex, as its app-server keeps a
//! loaded session's file — is written to the agents' bus as a listening chat, one that closed is
//! taken off it, and the comments sent to one are handed over with `codex queue`, which starts a
//! turn in that session: the brief Claude Code's watcher points to, answered with the MCP tools
//! Codex already has.

use crate::agents::{self, Answer, Bus, Chat, Kind};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// How far back a session may have last written and still be looked at.
const RECENT: Duration = Duration::from_secs(7 * 24 * 3600);

/// The agent's name on the bus.
const AGENT: &str = "Codex";

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

/// The sessions' rollouts written lately: `sessions/<year>/<month>/<day>/rollout-*.jsonl`.
fn recent_rollouts(home: &Path) -> Vec<PathBuf> {
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
            let fresh = e
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|at| now.duration_since(at).unwrap_or_default() < RECENT);
            if fresh && name.starts_with("rollout-") && name.ends_with(".jsonl") {
                out.push(path);
            }
        }
    }
    out
}

/// Which of `files` a process holds open: Codex keeps a loaded session's rollout open, a closed
/// one's not. None where `lsof` is missing (Windows).
fn held_open(files: &[PathBuf]) -> HashSet<PathBuf> {
    if files.is_empty() {
        return HashSet::new();
    }
    let lsof = ["/usr/sbin/lsof", "/usr/bin/lsof"]
        .into_iter()
        .map(Path::new)
        .find(|p| p.is_file())
        .unwrap_or(Path::new("lsof"));
    let Ok(out) = Command::new(lsof)
        .args(["-F", "n", "--"])
        .args(files)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return HashSet::new();
    };
    // `p<pid>` then `n<name>` lines; it exits 1 when some file is open nowhere.
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix('n'))
        .map(PathBuf::from)
        .collect()
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

/// Codex's sessions open now in project `project` (its main working tree) or a worktree of it,
/// as chats that listen.
pub fn chats(project: &Path) -> Vec<Chat> {
    let Some(home) = home() else {
        return Vec::new();
    };
    let rollouts = recent_rollouts(&home);
    let open = held_open(&rollouts);
    let names =
        names(&std::fs::read_to_string(home.join("session_index.jsonl")).unwrap_or_default());
    let mut chats = Vec::new();
    for file in rollouts.iter().filter(|f| open.contains(*f)) {
        let Some((session, cwd)) = std::fs::read_to_string(file)
            .ok()
            .and_then(|t| t.lines().next().and_then(meta))
        else {
            continue;
        };
        let Ok(repo) = crate::git::Repo::open(&cwd) else {
            continue;
        };
        if repo.main_root() != project {
            continue;
        }
        chats.push(Chat {
            title: names.get(&session).cloned().unwrap_or_default(),
            session,
            agent: AGENT.to_owned(),
            repo: repo.root(),
            project: project.to_owned(),
            branch: repo.head_branch().unwrap_or_default(),
            pid: 0,
            watcher: 0,
            beat: agents::now(),
        });
    }
    chats
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
        let found = recent_rollouts(&home);
        assert_eq!(found, [day.join("rollout-2026-10-23T19-12-06-x.jsonl")]);
        std::fs::remove_dir_all(home).ok();
    }
}
