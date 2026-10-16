//! `GitLance agent-watch`: run by an agent in its chat, in the background. It says the chat is
//! listening (`crate::agents`), and when the reviewer sends comments to this chat it prints one line
//! and exits — an agent wakes when its background command does. It also exits when the agent's
//! process is gone.

use crate::agents::{self, Answer, Bus, Chat, Kind};
use std::path::PathBuf;
use std::time::Duration;

/// Who is running this: the agent, its session and its process, from what it puts in a command's
/// environment.
fn whoami() -> Option<(String, String, u32, Option<PathBuf>)> {
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

/// The chat's transcript, to name it by its first message.
fn transcript(config: &std::path::Path, session: &str) -> Option<PathBuf> {
    std::fs::read_dir(config.join("projects"))
        .ok()?
        .flatten()
        .map(|dir| dir.path().join(format!("{session}.jsonl")))
        .find(|f| f.is_file())
}

fn alive(pid: u32) -> bool {
    if pid == 0 {
        return true;
    }
    crate::platform::alive(pid)
}

pub fn run() {
    let Some(bus) = Bus::new() else {
        eprintln!("gitlance agent-watch: no home folder");
        std::process::exit(1);
    };
    let Some((agent, session, pid, config)) = whoami() else {
        eprintln!(
            "gitlance agent-watch: run it from an agent's chat (Claude Code); its session id is not in the environment"
        );
        std::process::exit(1);
    };
    // Named as the app names it (`git::Repo::root`), or the two would not meet.
    let Some(root) = std::env::current_dir()
        .ok()
        .and_then(|cwd| crate::git::Repo::open(&cwd).ok())
        .map(|r| r.root())
    else {
        eprintln!("gitlance agent-watch: not in a git repository");
        std::process::exit(1);
    };
    let title = config
        .as_deref()
        .and_then(|c| transcript(c, &session))
        .map(|t| agents::title(&t))
        .unwrap_or_default();
    let mut chat = Chat {
        session: session.clone(),
        agent,
        repo: root.clone(),
        title,
        pid,
        watcher: std::process::id(),
        beat: agents::now(),
    };
    loop {
        chat.beat = agents::now();
        if bus.listen(&chat).is_err() {
            eprintln!("gitlance agent-watch: cannot write to GitLance's data folder");
            std::process::exit(1);
        }
        let comments = bus.comments(&root);
        let waiting = agents::waiting(&comments, &session);
        if !waiting.is_empty() {
            for c in &waiting {
                let batch = c.sent.as_ref().map(|s| s.batch.clone()).unwrap_or_default();
                let _ = bus.answer(
                    &root,
                    &Answer {
                        uid: agents::uid(),
                        id: c.id,
                        batch,
                        kind: Kind::Delivered,
                        body: String::new(),
                        form: None,
                        at: agents::now(),
                    },
                );
            }
            bus.stop(&session);
            println!(
                "GitLance: the reviewer sent {} review comment(s) to this chat. Call the gitlance list_comments tool, change the code for each, answer each with address_comment, then start `agent-watch` again in the background.",
                waiting.len()
            );
            return;
        }
        if !alive(pid) {
            bus.stop(&session);
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}
