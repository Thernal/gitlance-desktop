//! GitLance — review commits and force-pushed branch versions without an IDE. See README.md.

// A GUI app on Windows: no console window behind it.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod agents;
mod applog;
mod avatars;
mod codeowners;
mod codex;
mod editor;
mod emoji;
mod file_icons;
mod files;
mod fswatch;
mod generated;
mod git;
mod github;
mod highlight;
mod labels;
mod links;
mod markdown;
mod mcp;
mod mddiff;
mod mr;
mod notes;
mod platform;
mod rank;
mod report;
mod review;
mod search;
mod storage;
mod structural;
mod symbols;
mod textedit;
mod themes;
mod ui;
mod updates;
mod watch;
mod worddiff;

use std::path::PathBuf;

fn main() {
    // An explicit path opens just that folder (a repository or not, #75); without one the window comes back as it was
    // left (the tabs of the last session), or offers to open one.
    // What an agent runs in its chat (`claude mcp add gitlance -- <this> mcp`): no window.
    applog::catch_panics();
    match std::env::args().nth(1).as_deref() {
        Some("mcp") => return mcp::run(),
        Some("agent-watch") => return watch::run(),
        _ => {}
    }
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .filter(|p| p.is_dir() || git::Repo::open(p).is_ok());
    ui::run(path);
}
