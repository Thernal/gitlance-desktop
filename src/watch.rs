//! `GitLance agent-watch`: run by an agent in its chat, in the background. It says the chat is
//! listening (`crate::agents`), and when the reviewer sends comments to this chat it prints one line
//! and exits — an agent wakes when its background command does; the chat stays known, working.
//! It also exits, and the chat is forgotten, when the agent's process is gone.

use crate::agents::{self, Answer, Bus, Chat, Kind};
use std::path::PathBuf;
use std::time::Duration;

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
    let Some((agent, session, pid, config)) = agents::whoami() else {
        eprintln!(
            "gitlance agent-watch: run it from an agent's chat (Claude Code); its session id is not in the environment"
        );
        std::process::exit(1);
    };
    // Named as the app names them (`git::Repo::root`, `main_root`), or the two would not meet: the
    // chat works in `root`, a worktree maybe, and the project's notes are kept under `project`.
    let Some(repo) = std::env::current_dir()
        .ok()
        .and_then(|cwd| crate::git::Repo::open(&cwd).ok())
    else {
        eprintln!("gitlance agent-watch: not in a git repository");
        std::process::exit(1);
    };
    let (root, project) = (repo.root(), repo.main_root());
    let title = config
        .as_deref()
        .and_then(|c| transcript(c, &session))
        .map(|t| agents::title(&t))
        .unwrap_or_default();
    let mut chat = Chat {
        session: session.clone(),
        agent,
        repo: root.clone(),
        project: project.clone(),
        branch: repo.head_branch().unwrap_or_default(),
        title,
        pid,
        watcher: std::process::id(),
        beat: agents::now(),
    };
    loop {
        chat.beat = agents::now();
        // The branch its folder is on now: the notes of that branch come here (#77).
        if let Some(branch) = crate::git::Repo::open(&root)
            .ok()
            .and_then(|r| r.head_branch())
        {
            chat.branch = branch;
        }
        if bus.listen(&chat).is_err() {
            eprintln!("gitlance agent-watch: cannot write to GitLance's data folder");
            std::process::exit(1);
        }
        let comments = bus.comments(&project);
        let waiting = agents::waiting(&comments, &session);
        if !waiting.is_empty() {
            for c in &waiting {
                let batch = c.sent.as_ref().map(|s| s.batch.clone()).unwrap_or_default();
                let _ = bus.answer(
                    &project,
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
            // The chat stays known while it works on them: its beat goes stale and the app says
            // it is working (#77); it listens again when the agent starts this again.
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
