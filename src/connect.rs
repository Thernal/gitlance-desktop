//! Connecting a host without pasting a token (#142): a token found where git keeps it — in the
//! remote's URL, or with its credential helper — and checked for what it may do; else, when the SSH
//! key reaches GitLab, one made over SSH once the developer agrees (GitLab Shell's
//! `personal_access_token`), and renewed before it expires. It runs the system's own `git` and
//! `ssh`, so the agent, the keychain and the VPN work as they do for git; nothing is asked on the
//! terminal, no host key is accepted on anyone's behalf, and a found token is never copied.

use crate::git::WebRemote;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long a `git credential` or `ssh` question may take (ssh's own connect limit is 5 s).
const LIMIT: Duration = Duration::from_secs(10);
/// The days a token GitLance makes lasts, and how many before its end it is renewed.
pub const DAYS: u32 = 30;
const RENEW_BEFORE: i64 = 5;

/// Tokens found in git this run, by host (`https://host`), with where: `mr::token_source` gives
/// them after those given to GitLance.
fn found() -> &'static Mutex<HashMap<String, (String, &'static str)>> {
    static FOUND: OnceLock<Mutex<HashMap<String, (String, &'static str)>>> = OnceLock::new();
    FOUND.get_or_init(Default::default)
}

pub fn found_token(host: &str) -> Option<(String, &'static str)> {
    found().lock().ok()?.get(host).cloned()
}

fn keep_found(host: &str, token: Option<(String, &'static str)>) {
    if let Ok(mut found) = found().lock() {
        match token {
            Some(token) => found.insert(host.to_owned(), token),
            None => found.remove(host),
        };
    }
}

/// The token in a remote's URL: `https://oauth2:<token>@host/…`, or a lone token as its user
/// (`https://ghp_…@github.com/…`); a plain user name is none.
pub fn url_token(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split('/').next()?;
    let (user, _) = authority.rsplit_once('@')?;
    let secret = match user.split_once(':') {
        Some((_, password)) => password,
        None if ["glpat-", "ghp_", "gho_", "github_pat_"]
            .iter()
            .any(|p| user.starts_with(p)) =>
        {
            user
        }
        None => return None,
    };
    (secret.len() >= 8 && !secret.contains(char::is_whitespace)).then(|| secret.to_owned())
}

/// The password in `git credential fill`'s answer: what an HTTPS clone's helper keeps.
fn credential_password(answer: &str) -> Option<String> {
    answer
        .lines()
        .find_map(|l| l.strip_prefix("password="))
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
}

/// What git's credential helper keeps for `host` (no scheme), asked in repository `root`; it never
/// asks anyone: no terminal, no askpass, no helper's own window, and a time limit.
pub fn credential_token(root: &Path, host: &str) -> Option<String> {
    if let Some(dir) = fixture() {
        return credential_password(&std::fs::read_to_string(dir.join("credential.txt")).ok()?);
    }
    let mut git = Command::new("git");
    git.current_dir(root)
        .args(["-c", "credential.interactive=never", "credential", "fill"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "");
    let (ok, answer) = run(git, Some(&format!("protocol=https\nhost={host}\n\n")))?;
    ok.then(|| credential_password(&answer)).flatten()
}

/// Where `ssh` reaches a host: the remote's own `user@host` and port when it is an SSH remote (an
/// alias of `~/.ssh/config` resolves in ssh itself), else `git@` the host of its web address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ssh {
    pub target: String,
    pub port: Option<u16>,
}

impl Ssh {
    pub fn of(url: &str, remote: &WebRemote) -> Self {
        let url = url.trim();
        if let Some(rest) = url.strip_prefix("ssh://") {
            let authority = rest.split('/').next().unwrap_or(rest);
            let (user, at) = authority.rsplit_once('@').unwrap_or(("git", authority));
            let (host, port) = match at.rsplit_once(':') {
                Some((host, port)) => (host, port.parse().ok()),
                None => (at, None),
            };
            return Self {
                target: format!("{user}@{host}"),
                port,
            };
        }
        // `user@host:path`, as git writes an SSH remote without a scheme.
        if !url.contains("://")
            && let Some((authority, _)) = url.split_once(':')
            && authority.contains('@')
        {
            return Self {
                target: authority.to_owned(),
                port: None,
            };
        }
        let host = remote
            .base
            .split_once("://")
            .map_or(remote.base.as_str(), |(_, rest)| rest)
            .split('/')
            .next()
            .unwrap_or_default();
        let host = host.split(':').next().unwrap_or(host);
        Self {
            target: format!("git@{host}"),
            port: None,
        }
    }

    /// `ssh` asking nothing: no passphrase, no host key accepted, 5 s to connect.
    fn command(&self) -> Command {
        let mut ssh = Command::new("ssh");
        ssh.args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "StrictHostKeyChecking=yes",
            "-T",
        ]);
        if let Some(port) = self.port {
            ssh.args(["-p", &port.to_string()]);
        }
        ssh.arg(&self.target);
        ssh
    }
}

/// Who the host says the SSH key is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Known {
    GitLab(String),
    GitHub(String),
}

/// GitLab's "Welcome to GitLab, @ann!", GitHub's "Hi ann! You've successfully authenticated".
fn welcome(text: &str) -> Option<Known> {
    if let Some((_, rest)) = text.split_once("Welcome to GitLab, @") {
        let user = rest.split(['!', '\n']).next()?.trim();
        return (!user.is_empty()).then(|| Known::GitLab(user.to_owned()));
    }
    if text.contains("successfully authenticated")
        && let Some((_, rest)) = text.split_once("Hi ")
    {
        let user = rest.split(['!', '\n']).next()?.trim();
        return (!user.is_empty()).then(|| Known::GitHub(user.to_owned()));
    }
    None
}

/// Why `ssh` did not reach the host, in a few words.
fn ssh_why(text: &str) -> String {
    if text.contains("Host key verification failed") || text.contains("No ED25519 host key") {
        "SSH does not know this host key yet: connect once with git in a terminal".to_owned()
    } else if text.contains("Permission denied") {
        "SSH key not accepted".to_owned()
    } else if text.contains("Could not resolve hostname")
        || text.contains("timed out")
        || text.contains("Connection refused")
        || text.contains("No route to host")
    {
        "SSH cannot reach the host: is the VPN on?".to_owned()
    } else {
        let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        format!("ssh said: {}", first.trim())
    }
}

/// Who the host knows the SSH key as, or why it could not be asked.
pub fn whoami(ssh: &Ssh) -> Result<Known, String> {
    let answer = match fixture() {
        Some(dir) => std::fs::read_to_string(dir.join("whoami.txt"))
            .map_err(|_| "SSH key not accepted".to_owned())?,
        None => run(ssh.command(), None).ok_or("ssh did not answer")?.1,
    };
    welcome(&answer).ok_or_else(|| ssh_why(&answer))
}

/// A token GitLab Shell made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    pub token: String,
    pub expires: Option<String>,
}

/// Why a token was not made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The instance does not make tokens over SSH.
    Off,
    /// Only these scopes may be made over SSH.
    Scopes(Vec<String>),
    Other(String),
}

/// GitLab Shell's answer to `personal_access_token`: `Token:` and `Expires:`, or its refusal.
fn made(answer: &str) -> Result<Made, Refused> {
    let field = |name: &str| {
        answer
            .lines()
            .find_map(|l| l.trim().strip_prefix(name))
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    if let Some(token) = field("Token:") {
        return Ok(Made {
            token,
            expires: field("Expires:"),
        });
    }
    if let Some((_, valid)) = answer.split_once("Valid scopes are:") {
        let scopes: Vec<String> = valid
            .split(|c: char| !(c.is_ascii_lowercase() || c == '_'))
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect();
        return Err(Refused::Scopes(scopes));
    }
    let lower = answer.to_lowercase();
    if lower.contains("disallowed command")
        || lower.contains("not allowed")
        || lower.contains("disabled")
        || lower.contains("unknown command")
    {
        return Err(Refused::Off);
    }
    let first = answer.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    Err(Refused::Other(
        first.trim().trim_start_matches("remote:").trim().to_owned(),
    ))
}

/// A token for this computer made over SSH: `scope` (`api` or `read_api`), for `DAYS` days.
pub fn make(ssh: &Ssh, name: &str, scope: &str) -> Result<Made, Refused> {
    let answer = match fixture() {
        Some(dir) => std::fs::read_to_string(dir.join("made.txt"))
            .map_err(|_| Refused::Off)?
            .replace("{scope}", scope),
        None => {
            let mut ssh = ssh.command();
            ssh.args(["personal_access_token", name, scope, &DAYS.to_string()]);
            run(ssh, None)
                .ok_or_else(|| Refused::Other("ssh did not answer".to_owned()))?
                .1
        }
    };
    made(&answer)
}

/// What GitLance knows of a host between runs (`connect.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostNote {
    /// Not now, on the offer: it is not offered again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub declined: bool,
    /// The instance makes no tokens over SSH.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ssh_off: bool,
    /// The scopes it makes over SSH, when it said; every one when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made: Option<MadeNote>,
}

/// The token GitLance made for a host, so it is renewed only while it is the one in use.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MadeNote {
    pub name: String,
    pub scope: String,
    pub expires: Option<String>,
    /// Its last four characters.
    pub tail: String,
}

impl MadeNote {
    pub fn of(made: &Made, name: &str, scope: &str) -> Self {
        let tail: String = made.token.chars().rev().take(4).collect();
        Self {
            name: name.to_owned(),
            scope: scope.to_owned(),
            expires: made.expires.clone(),
            tail: tail.chars().rev().collect(),
        }
    }

    /// Whether `token` is this one.
    pub fn is(&self, token: &str) -> bool {
        !self.tail.is_empty() && token.ends_with(&self.tail)
    }

    /// Renewed now: it ends within `RENEW_BEFORE` days of `today` (days since 1970).
    pub fn due(&self, today: i64) -> bool {
        self.expires
            .as_deref()
            .and_then(day_number)
            .is_some_and(|end| end - today <= RENEW_BEFORE)
    }
}

/// `2026-11-26` as days since 1970.
fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.trim().splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    Some(crate::search::days_from_civil(y, m, d))
}

/// Today, as days since 1970.
pub fn today() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 / 86_400)
        .unwrap_or(0)
}

const NOTES: &str = "connect.json";

/// The hosts' notes, read once a run and kept as they change.
fn notes() -> &'static Mutex<HashMap<String, HostNote>> {
    static NOTES_READ: OnceLock<Mutex<HashMap<String, HostNote>>> = OnceLock::new();
    NOTES_READ.get_or_init(|| {
        Mutex::new(serde_json::from_str(&crate::storage::read(NOTES)).unwrap_or_default())
    })
}

pub fn note(host: &str) -> HostNote {
    notes()
        .lock()
        .ok()
        .and_then(|all| all.get(host).cloned())
        .unwrap_or_default()
}

pub fn set_note(host: &str, change: impl FnOnce(&mut HostNote)) {
    let Ok(mut all) = notes().lock() else {
        return;
    };
    change(all.entry(host.to_owned()).or_default());
    if let Ok(body) = serde_json::to_string_pretty(&*all) {
        crate::storage::save(NOTES, body);
    }
}

/// What looking for a way in found, for a host without a token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Looked {
    /// A token git keeps that the host took: whom it is, and where it was found.
    pub connected: Option<(String, &'static str)>,
    /// A token git keeps that the host refused, and where.
    pub refused: Option<&'static str>,
    /// Who the SSH key is, or why SSH did not say.
    pub ssh: Result<Known, String>,
}

/// Looks for a way in, for `remote` of repository `root` whose URL is `url` (Flow 1–3): a token in
/// the URL, else with the credential helper, kept for this run once the host takes it; else who
/// the SSH key is. Slow (processes, the network): off the window's thread.
pub fn look(root: &Path, url: &str, remote: &WebRemote) -> Looked {
    let host = crate::mr::api_host(remote);
    let mut refused = None;
    let candidates = [
        (url_token(url), "the remote's URL"),
        (
            credential_token(root, &crate::mr::host(remote)),
            "git's credential helper",
        ),
    ];
    for (token, from) in candidates {
        let Some(token) = token else {
            continue;
        };
        keep_found(&host, Some((token, from)));
        match crate::mr::me(remote) {
            Ok(who) => {
                return Looked {
                    connected: Some((who, from)),
                    refused: None,
                    ssh: Err(String::new()),
                };
            }
            Err(_) => {
                keep_found(&host, None);
                refused = Some(from);
            }
        }
    }
    Looked {
        connected: None,
        refused,
        // GitHub's SSH says who you are but makes no token: not asked.
        ssh: if remote.github {
            Err(String::new())
        } else {
            whoami(&Ssh::of(url, remote))
        },
    }
}

/// Runs `command` with `input` on its stdin, for `LIMIT` at most: whether it succeeded, and what
/// it wrote (stdout, then stderr).
fn run(mut command: Command, input: Option<&str>) -> Option<(bool, String)> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.unwrap_or_default().as_bytes());
    }
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < LIMIT => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let out = child.wait_with_output().ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some((status.success(), text))
}

/// `GITLANCE_SSH_FIXTURE=<dir>` answers for git's credential helper and ssh (UI checks):
/// `credential.txt`, `whoami.txt` and `made.txt` (`{scope}` the scope asked), each missing file
/// a no; a test sets its own (`TEST_FIXTURE`).
fn fixture() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    if let Some(dir) = TEST_FIXTURE.with(|f| f.borrow().clone()) {
        return Some(dir);
    }
    std::env::var_os("GITLANCE_SSH_FIXTURE").map(Into::into)
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_FIXTURE: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gitlab() -> WebRemote {
        WebRemote {
            base: "https://gitlab.example.com/g/p".into(),
            github: false,
        }
    }

    #[test]
    fn a_token_is_read_from_a_url_only_where_one_is() {
        assert_eq!(
            url_token("https://oauth2:glpat-abcdef12@gitlab.example.com/g/p.git").as_deref(),
            Some("glpat-abcdef12")
        );
        assert_eq!(
            url_token("https://ghp_1234567890@github.com/o/r.git").as_deref(),
            Some("ghp_1234567890")
        );
        assert_eq!(url_token("https://ann@gitlab.example.com/g/p.git"), None);
        assert_eq!(url_token("https://gitlab.example.com/g/p.git"), None);
        assert_eq!(url_token("git@gitlab.example.com:g/p.git"), None);
        assert_eq!(
            credential_password("protocol=https\nhost=h\nusername=ann\npassword=glpat-x1\n")
                .as_deref(),
            Some("glpat-x1")
        );
        assert_eq!(credential_password("protocol=https\nhost=h\n"), None);
    }

    #[test]
    fn ssh_reaches_the_host_as_the_remote_names_it() {
        let ssh = |url: &str| Ssh::of(url, &gitlab());
        assert_eq!(
            ssh("git@work:g/p.git"),
            Ssh {
                target: "git@work".into(),
                port: None
            }
        );
        assert_eq!(
            ssh("ssh://git@gitlab.example.com:2222/g/p.git"),
            Ssh {
                target: "git@gitlab.example.com".into(),
                port: Some(2222)
            }
        );
        assert_eq!(
            ssh("https://gitlab.example.com/g/p.git").target,
            "git@gitlab.example.com"
        );
    }

    #[test]
    fn the_hosts_answers_are_read() {
        assert_eq!(
            welcome("Welcome to GitLab, @ann!\n"),
            Some(Known::GitLab("ann".into()))
        );
        assert_eq!(
            welcome(
                "Hi ann! You've successfully authenticated, but GitHub does not provide shell access."
            ),
            Some(Known::GitHub("ann".into()))
        );
        assert_eq!(welcome("Welcome to GitLab, Anonymous!"), None);
        assert!(ssh_why("git@h: Permission denied (publickey).").contains("SSH key"));
        assert!(ssh_why("Host key verification failed.").contains("host key"));
        assert!(ssh_why("ssh: Could not resolve hostname h").contains("VPN"));

        let ok = made("Token:   glpat-abc123\nScopes:  api\nExpires: 2026-11-26\n").unwrap();
        assert_eq!(ok.token, "glpat-abc123");
        assert_eq!(ok.expires.as_deref(), Some("2026-11-26"));
        assert_eq!(
            made(
                "remote: Invalid scope: 'api'. Valid scopes are: [\"read_api\", \"read_repository\"]"
            ),
            Err(Refused::Scopes(vec![
                "read_api".into(),
                "read_repository".into()
            ]))
        );
        assert_eq!(made("Disallowed command"), Err(Refused::Off));
        assert_eq!(
            made("remote: Something broke\n"),
            Err(Refused::Other("Something broke".into()))
        );
    }

    #[test]
    fn a_made_token_is_renewed_near_its_end_and_only_while_in_use() {
        let note = MadeNote::of(
            &Made {
                token: "glpat-abcd1234".into(),
                expires: Some("2026-11-26".into()),
            },
            "GitLance (Mac)",
            "api",
        );
        assert_eq!(note.tail, "1234");
        assert!(note.is("glpat-abcd1234") && !note.is("glpat-other"));
        let end = day_number("2026-11-26").unwrap();
        assert!(!note.due(end - 6));
        assert!(note.due(end - 5) && note.due(end + 1));
        assert!(!MadeNote::default().due(end));
    }

    #[test]
    fn looking_finds_what_git_keeps_or_who_ssh_says() {
        let dir = tempfile::TempDir::new().unwrap();
        TEST_FIXTURE.with(|f| *f.borrow_mut() = Some(dir.path().to_path_buf()));
        crate::mr::TEST_FIXTURE.with(|f| *f.borrow_mut() = Some(dir.path().to_path_buf()));
        std::fs::write(dir.path().join("whoami.txt"), "Welcome to GitLab, @ann!\n").unwrap();
        let url = "git@gitlab.example.com:g/p.git";
        // Nothing in git: who SSH says.
        let looked = look(dir.path(), url, &gitlab());
        assert_eq!(looked.connected, None);
        assert_eq!(looked.ssh, Ok(Known::GitLab("ann".into())));
        // The credential helper's, which the host takes: kept for the run.
        std::fs::write(
            dir.path().join("credential.txt"),
            "username=ann\npassword=glpat-found1\n",
        )
        .unwrap();
        let looked = look(dir.path(), url, &gitlab());
        assert_eq!(
            looked.connected,
            Some(("Fixture".into(), "git's credential helper"))
        );
        assert_eq!(
            found_token("https://gitlab.example.com"),
            Some(("glpat-found1".into(), "git's credential helper"))
        );
        keep_found("https://gitlab.example.com", None);
        // Made over SSH.
        std::fs::write(
            dir.path().join("made.txt"),
            "Token:   glpat-made1\nScopes:  {scope}\nExpires: 2026-11-26\n",
        )
        .unwrap();
        let made = make(&Ssh::of(url, &gitlab()), "GitLance (Mac)", "read_api").unwrap();
        assert_eq!(made.token, "glpat-made1");
    }
}
