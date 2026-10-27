//! `git fetch`, run with the `git` CLI so the user's SSH agent, credential helpers and VPN routes
//! work unchanged. The one place GitLance writes to a repository: remote-tracking refs and objects,
//! and a merge request's head fetched to `refs/gitlance/requests/<n>` when its branch is not here
//! (#114) or its head the host compares is not (#145); nothing else. The app's background fetch
//! (`ui::fetch`) and the MCP server's request reading (#150) both go through it.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(90);

/// `git fetch --all --prune` in `root`; the error is a sentence a person can act on.
pub fn all(root: &Path) -> Result<(), String> {
    git_fetch(root, &["--all", "--prune"])
}

/// Request `iid`'s head, from where the host keeps it, to `git::request_ref` (#114): a merged or
/// closed request whose branch is gone, or a fork's. GitLab may delete that ref a while after the
/// merge; then `head`, the commit the host names, is asked for itself (#145). Blocks; run it off
/// the main thread.
pub fn fetch_request(
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
pub fn fetch_branch(root: &Path, name: &str) -> Result<(), String> {
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
        all(&clone).unwrap();
        let repo = crate::git::Repo::open(&clone).unwrap();
        let tip = repo.resolve("origin/main").unwrap();
        assert_eq!(tip, repo.resolve("FETCH_HEAD").unwrap());

        std::fs::remove_dir_all(&origin).unwrap();
        assert!(all(&clone).is_err());
    }

    #[test]
    fn errors_are_put_in_plain_words() {
        assert!(explain("ssh: Could not resolve hostname git.example").contains("VPN"));
        assert!(explain("git@host: Permission denied (publickey).").contains("credentials"));
        assert_eq!(explain("\nfatal: odd\n"), "fatal: odd");
    }
}
