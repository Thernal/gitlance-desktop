//! Background fetch: opt-in per repository. It runs the `git` CLI (so the user's SSH agent,
//! credential helpers and VPN routes work unchanged), when the window gains focus and every few
//! minutes. It is the one place GitLance writes to a repository: remote-tracking refs and objects,
//! nothing else. The auto-refresh then sees the moved refs and marks new versions.

use super::px;
use super::{Workspace, format, theme};
use crate::storage;
use gpui::{ClickEvent, Context, Task, div, prelude::*};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const INTERVAL: Duration = Duration::from_secs(300);
/// A focus gain fetches only when the last fetch is older than this.
const FOCUS_AFTER: i64 = 60;
const TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Default)]
pub struct Fetch {
    pub enabled: bool,
    running: bool,
    /// Seconds since the epoch of the last successful fetch.
    last: Option<i64>,
    error: Option<String>,
    task: Option<Task<()>>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `git fetch --all --prune` in `root`; the error is a sentence a person can act on.
fn run(root: &Path) -> Result<(), String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["fetch", "--all", "--prune", "--quiet"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "git is not installed".to_owned())?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                return Err("The remote did not answer in time — is the VPN on?".to_owned());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(e.to_string()),
        }
    };
    if status.success() {
        return Ok(());
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = std::io::Read::read_to_string(&mut pipe, &mut stderr);
    }
    Err(explain(&stderr))
}

fn explain(stderr: &str) -> String {
    let lower = stderr.to_lowercase();
    if [
        "could not resolve",
        "timed out",
        "connection refused",
        "network is unreachable",
        "no route",
    ]
    .iter()
    .any(|p| lower.contains(p))
    {
        "Can't reach the remote — is the VPN on?".to_owned()
    } else if [
        "permission denied",
        "authentication failed",
        "could not read username",
    ]
    .iter()
    .any(|p| lower.contains(p))
    {
        "The remote refused the credentials".to_owned()
    } else {
        stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("git fetch failed")
            .trim()
            .to_owned()
    }
}

impl Workspace {
    /// Called when a repository opens: reads the opt-in and starts the timer.
    pub(super) fn start_fetching(&mut self, cx: &mut Context<Self>) {
        let enabled = self.root.as_deref().is_some_and(storage::fetch_enabled);
        self.fetch = Fetch {
            enabled,
            ..Fetch::default()
        };
        if enabled {
            self.fetch.task = Some(cx.spawn(async move |this, cx| {
                loop {
                    if this.update(cx, |this, cx| this.fetch_now(cx)).is_err() {
                        return;
                    }
                    cx.background_executor().timer(INTERVAL).await;
                }
            }));
        }
        cx.notify();
    }

    pub(super) fn set_fetch_enabled(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(root) = &self.root {
            storage::set_fetch_enabled(root, on);
        }
        self.start_fetching(cx);
    }

    /// The window came back to the front.
    pub(super) fn window_activated(&mut self, cx: &mut Context<Self>) {
        if self.fetch.enabled && self.fetch.last.is_none_or(|t| now() - t > FOCUS_AFTER) {
            self.fetch_now(cx);
        }
    }

    pub(super) fn fetch_now(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if !self.fetch.enabled || self.fetch.running {
            return;
        }
        self.fetch.running = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { run(&root) })
                .await;
            this.update(cx, |this, cx| {
                this.fetch.running = false;
                match result {
                    Ok(()) => {
                        this.fetch.last = Some(now());
                        this.fetch.error = None;
                    }
                    Err(e) => this.fetch.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The title bar's word on the fetch: when it last ran, or why it failed.
    pub(super) fn render_fetch(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.fetch.enabled {
            return None;
        }
        let (text, color) = match (&self.fetch.error, self.fetch.running, self.fetch.last) {
            (Some(e), _, _) => (format!("Fetch failed: {e}  ·  Retry"), theme::removed()),
            (None, true, _) => ("Fetching…".to_owned(), theme::muted()),
            (None, false, Some(t)) => (format!("Fetched {}", format::ago(t)), theme::muted()),
            (None, false, None) => ("Fetch waiting".to_owned(), theme::muted()),
        };
        Some(
            div()
                .id("fetch-status")
                .flex_none()
                .max_w(px(420.))
                .truncate()
                .h(px(24.))
                .flex()
                .items_center()
                .px_2()
                .rounded(px(super::ROW_RADIUS))
                .text_size(px(11.))
                .text_color(color)
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(text)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.fetch_now(cx))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
            .args(args)
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn a_fetch_brings_in_the_remote_and_a_missing_remote_says_why() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (origin, clone) = (tmp.path().join("origin"), tmp.path().join("clone"));
        std::fs::create_dir(&origin).unwrap();
        git(&origin, &["init", "-q", "-b", "main"]);
        git(&origin, &["commit", "-q", "--allow-empty", "-m", "one"]);
        let status = Command::new("git")
            .args(["clone", "-q"])
            .arg(&origin)
            .arg(&clone)
            .status()
            .unwrap();
        assert!(status.success());

        git(&origin, &["commit", "-q", "--allow-empty", "-m", "two"]);
        run(&clone).unwrap();
        let repo = crate::git::Repo::open(&clone).unwrap();
        let tip = repo.resolve("origin/main").unwrap();
        assert_eq!(tip, repo.resolve("FETCH_HEAD").unwrap());

        std::fs::remove_dir_all(&origin).unwrap();
        assert!(run(&clone).is_err());
    }

    #[test]
    fn errors_are_put_in_plain_words() {
        assert!(explain("ssh: Could not resolve hostname git.example").contains("VPN"));
        assert!(explain("git@host: Permission denied (publickey).").contains("credentials"));
        assert_eq!(explain("\nfatal: odd\n"), "fatal: odd");
    }
}
