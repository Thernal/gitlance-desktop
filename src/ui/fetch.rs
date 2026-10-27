//! Background fetch: on for every repository until turned off for it. It runs the `git` CLI (so the user's SSH agent,
//! credential helpers and VPN routes work unchanged), when the window gains focus and every few
//! minutes. It is the one place GitLance writes to a repository: remote-tracking refs and objects,
//! and a merge request's head fetched to `refs/gitlance/requests/<n>` when its branch is not here
//! (#114) or its head the host compares is not (#145); nothing else. The auto-refresh then sees
//! the moved refs and marks new versions.

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
    git_fetch(root, &["--all", "--prune"])
}

/// Request `iid`'s head, from where the host keeps it, to `git::request_ref` (#114): a merged or
/// closed request whose branch is gone, or a fork's. GitLab may delete that ref a while after the
/// merge; then `head`, the commit the host names, is asked for itself (#145). Blocks; run it off
/// the main thread.
pub(super) fn fetch_request(
    root: &Path,
    iid: u64,
    github: bool,
    head: Option<&str>,
) -> Result<(), String> {
    let remote = web_remote(root)?;
    let from = if github {
        format!("refs/pull/{iid}/head")
    } else {
        format!("refs/merge-requests/{iid}/head")
    };
    let to = crate::git::request_ref(iid);
    let spec = format!("+{from}:{to}");
    git_fetch(root, &[&remote, &spec]).or_else(|e| match head {
        Some(sha) => git_fetch(root, &[&remote, &format!("+{sha}:{to}")]),
        None => Err(e),
    })
}

/// Branch `name` of the project's remote, to its remote-tracking ref: a request's target whose
/// base the host compares from is not here (#145). Blocks; run it off the main thread.
pub(super) fn fetch_branch(root: &Path, name: &str) -> Result<(), String> {
    let remote = web_remote(root)?;
    let spec = format!("+refs/heads/{name}:refs/remotes/{remote}/{name}");
    git_fetch(root, &[&remote, &spec])
}

fn web_remote(root: &Path) -> Result<String, String> {
    crate::git::Repo::open(root)
        .ok()
        .and_then(|r| r.web_remote_name())
        .ok_or_else(|| "The repository has no remote".to_owned())
}

/// `git fetch --quiet` with `args` in `root`, given up after `TIMEOUT`.
fn git_fetch(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["fetch", "--quiet"])
        .args(args)
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
    /// Called when a repository opens: reads whether it is turned off and starts the timer.
    pub(super) fn start_fetching(&mut self, cx: &mut Context<Self>) {
        // A plain folder, or a repository with no remote, has nothing to fetch.
        let enabled = !self.plain
            && self.root.as_deref().is_some_and(|root| {
                storage::fetch_enabled(root)
                    && crate::git::Repo::open(root)
                        .ok()
                        .and_then(|r| r.web_remote_name())
                        .is_some()
            });
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
        self.load_notices(cx);
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
        // A long hover: the whole message, when it last fetched and from where (X59).
        let mut lines: Vec<String> = Vec::new();
        if let Some(e) = &self.fetch.error {
            lines.push(format!("Fetch failed: {e}"));
        }
        lines.push(match self.fetch.last {
            Some(t) => format!("Last fetched {}", super::hover::full_date(t)),
            None => "Not fetched yet".to_owned(),
        });
        lines.push(match &self.web {
            Some(w) => format!(
                "git fetch --all --prune: every remote, origin at {}",
                w.base
            ),
            None => "git fetch --all --prune: every remote".to_owned(),
        });
        lines.push("Every 5 minutes and when the window comes back · a click fetches now".into());
        let tip: gpui::SharedString = lines.join("\n").into();
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
                .tooltip_show_delay(super::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
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
