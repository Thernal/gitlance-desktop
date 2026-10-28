//! Connecting a host without pasting a token (#142, #156): the ways in this computer offers, each
//! with the account it signs in as — the GitLab or GitHub CLI's token and what git keeps (the
//! remote's URL, its credential helper), used where they are and never copied; one made over SSH
//! once the developer agrees (GitLab Shell's `personal_access_token`), renewed before it expires;
//! one copied once from a JetBrains IDE (`crate::ide`). It runs the system's own `git`, `ssh` and
//! CLIs, so the agent, the keychain and the VPN work as they do in a terminal; nothing is asked on
//! the terminal, no host key is accepted on anyone's behalf, and a token GitLance did not make is
//! never refreshed or revoked.

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
/// them while git is the way picked for the host.
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
        "the host did not accept it (Permission denied)".to_owned()
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
            .map_err(|_| "the host did not accept it (Permission denied)".to_owned())?,
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

/// `text` as one word of the command GitLab Shell splits as a shell does: in single quotes, a quote
/// in it closed, escaped and opened again. Bare, "GitLance (Mac)" is two words and its `(` refused:
/// "invalid command line string".
fn shell_word(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
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
            ssh.args([
                "personal_access_token",
                &shell_word(name),
                scope,
                &DAYS.to_string(),
            ]);
            run(ssh, None)
                .ok_or_else(|| Refused::Other("ssh did not answer".to_owned()))?
                .1
        }
    };
    made(&answer)
}

/// What GitLance knows of a host between runs (`connect.json`); 1.2.0's `declined` (Not now on
/// the SSH key's offer) is read and dropped: the ways list has no offer to decline.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostNote {
    /// The instance makes no tokens over SSH.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ssh_off: bool,
    /// The scopes it makes over SSH, when it said; every one when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made: Option<MadeNote>,
    /// The way in picked (#156): `cli`, `git`, `ssh`, `ide` or `pasted`; the next run goes
    /// straight to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub way: Option<String>,
    /// The IDE the token in use was copied from ("IntelliJ IDEA").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ide: Option<String>,
    /// IDE accounts (their ids) found signed in with OAuth: listed with why, never read again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub oauth: Vec<String>,
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

/// What a CLI says for a host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliSaid {
    /// It is not installed: it is not named.
    Absent,
    /// Signed in to the host: its token, and the account when it says.
    Token(String, Option<String>),
    /// Installed, not signed in to the host; the hosts it is signed in to.
    Elsewhere(Vec<String>),
}

/// What the CLIs said this run, by host (`https://host`): each read once, off the window's thread.
fn clis() -> &'static Mutex<HashMap<String, CliSaid>> {
    static CLIS: OnceLock<Mutex<HashMap<String, CliSaid>>> = OnceLock::new();
    CLIS.get_or_init(Default::default)
}

/// The CLI's token for `host` as read this run, without running it: what the window's thread asks.
pub fn cli_token_now(host: &str) -> Option<String> {
    match clis().lock().ok()?.get(host)? {
        CliSaid::Token(token, _) => Some(token.clone()),
        _ => None,
    }
}

/// What the host's CLI says (glab for GitLab, gh for GitHub): read once a run — a process, and
/// the CLI asks its host: off the window's thread.
pub fn cli(remote: &WebRemote) -> CliSaid {
    let host = crate::mr::api_host(remote);
    if let Some(said) = clis().lock().ok().and_then(|c| c.get(&host).cloned()) {
        return said;
    }
    let said = read_cli(remote);
    if let Ok(mut all) = clis().lock() {
        all.insert(host, said.clone());
    }
    said
}

/// Forgets what the CLI said for `host`: its token refused, it is read again (glab's OAuth token
/// lasts two hours, and glab refreshes it itself).
pub fn drop_cli(host: &str) {
    if let Ok(mut all) = clis().lock() {
        all.remove(host);
    }
}

/// The CLI's name: what it is called in a sentence and its program.
pub fn cli_name(github: bool) -> (&'static str, &'static str) {
    if github {
        ("The GitHub CLI", "gh")
    } else {
        ("The GitLab CLI", "glab")
    }
}

fn read_cli(remote: &WebRemote) -> CliSaid {
    let host = crate::mr::host(remote);
    let (_, name) = cli_name(remote.github);
    if let Some(dir) = fixture() {
        return match std::fs::read_to_string(dir.join(format!("{name}.txt"))) {
            Ok(text) => cli_answer(&text, &host),
            Err(_) => CliSaid::Absent,
        };
    }
    let Some(program) = program(name) else {
        return CliSaid::Absent;
    };
    let status = |args: &[&str]| {
        let mut cli = Command::new(&program);
        cli.args(["auth", "status"])
            .args(args)
            .env("GLAB_NO_PROMPT", "1")
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1");
        run(cli, None).map(|(_, text)| text).unwrap_or_default()
    };
    match cli_answer(&status(&["--hostname", &host, "--show-token"]), &host) {
        CliSaid::Token(token, user) => CliSaid::Token(token, user),
        // Not signed in to this host: where it is, without the tokens.
        _ => cli_answer(&status(&[]), &host),
    }
}

/// What `glab auth status` and `gh auth status` print (`--show-token`): the token and the account
/// of the host's first sign-in — `✓ Logged in to <host> as <user>` (gh: `account <user>`), then
/// `✓ Token: <token>` (glab: `Token found in <store>: <token>`) — else the other hosts named.
fn cli_answer(text: &str, host: &str) -> CliSaid {
    let mut at_host = false;
    let mut user = None;
    let mut elsewhere: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        // The mark before each line: ✓, X, -, !.
        let line = match line.split_once(' ') {
            Some((mark, rest))
                if mark.chars().count() == 1 && !mark.starts_with(char::is_alphanumeric) =>
            {
                rest.trim()
            }
            Some(("x" | "X", rest)) => rest.trim(),
            _ => line,
        };
        if let Some(rest) = line.strip_prefix("Logged in to ") {
            let (at, rest) = rest.split_once(' ').unwrap_or((rest, ""));
            if at.eq_ignore_ascii_case(host) {
                if at_host && user.is_some() {
                    // A second account on the host: the first is the active one.
                    break;
                }
                at_host = true;
                user = rest
                    .strip_prefix("as ")
                    .or_else(|| rest.strip_prefix("account "))
                    .and_then(|r| r.split_whitespace().next())
                    .map(str::to_owned);
            } else {
                at_host = false;
                if !elsewhere.iter().any(|h| h.eq_ignore_ascii_case(at)) {
                    elsewhere.push(at.to_owned());
                }
            }
            continue;
        }
        if at_host
            && line.starts_with("Token")
            && !line.starts_with("Token scopes")
            && let Some((_, token)) = line.rsplit_once(": ")
        {
            let token = token.trim();
            if token.len() >= 8 && !token.contains(['*', ' ']) {
                return CliSaid::Token(token.to_owned(), user);
            }
        }
    }
    CliSaid::Elsewhere(elsewhere)
}

/// A CLI's program: on the PATH, or where Homebrew and the installers put it (an app opened from
/// the Finder does not get the shell's PATH).
pub fn program(name: &str) -> Option<std::path::PathBuf> {
    let on_path: Vec<std::path::PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let known = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(Into::into);
    on_path
        .into_iter()
        .chain(known)
        .flat_map(|dir: std::path::PathBuf| [dir.join(name), dir.join(format!("{name}.exe"))])
        .find(|p| p.is_file())
}

/// How a way in gets its token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Via {
    /// The GitLab or GitHub CLI's, used where it is.
    Cli,
    /// What git keeps — in the remote's URL or with its credential helper — used where it is.
    Git(&'static str),
    /// Made with the SSH key over GitLab Shell; GitLance's own.
    Ssh,
    /// Copied once from a JetBrains IDE, with the developer's yes.
    Ide(crate::ide::Account),
    /// Made by hand on the host's page, and pasted.
    Hand,
}

impl Via {
    /// What `HostNote::way` remembers it as.
    pub fn key(&self) -> &'static str {
        match self {
            Via::Cli => "cli",
            Via::Git(_) => "git",
            Via::Ssh => "ssh",
            Via::Ide(_) => "ide",
            Via::Hand => "pasted",
        }
    }
}

/// A way in to a host, as the connect panel and Settings list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Way {
    pub via: Via,
    /// The account it signs in as (`ann`), when known before it is used.
    pub user: Option<String>,
    /// Its scopes, as the host says; empty where it does not.
    pub scopes: Vec<String>,
    /// Whether it may write — comment, approve, merge: `api` on GitLab.
    pub write: bool,
    pub expires: Option<String>,
}

impl Way {
    /// Easiest first: one there that writes (a CLI's, then git's), one made with the SSH key, one
    /// copied from an IDE; then those that only read; by hand last.
    fn rank(&self) -> (bool, bool, u8) {
        let kind = match self.via {
            Via::Cli => 0,
            Via::Git(_) => 1,
            Via::Ssh => 2,
            Via::Ide(_) => 3,
            Via::Hand => 4,
        };
        (self.via == Via::Hand, !self.write, kind)
    }
}

/// The ways in, easiest first; those of a kind keep their order (the IDE used last first).
pub fn order(ways: &mut [Way]) {
    ways.sort_by_key(Way::rank);
}

/// A way found that cannot be used, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotHere {
    pub via: Via,
    pub why: String,
}

/// What the host says of a token: the account and what it may do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Access {
    /// `Ann Example (@ann)`.
    pub who: String,
    pub user: String,
    pub scopes: Vec<String>,
    pub expires: Option<String>,
}

impl Access {
    /// The way it makes; GitHub says nothing of a token's scopes, so one there is taken to write.
    pub fn way(self, via: Via, github: bool) -> Way {
        let write = github || self.scopes.is_empty() || self.scopes.iter().any(|s| s == "api");
        Way {
            via,
            user: Some(self.user),
            scopes: self.scopes,
            write,
            expires: self.expires,
        }
    }
}

/// Asks the host who `token` is and what it may do (the network: off the window's thread); else
/// why it cannot be used — a token for cloning only, one refused, or the host not reached.
pub fn check(remote: &WebRemote, token: &str) -> Result<Access, String> {
    let host = crate::mr::host(remote);
    if let Some(dir) = fixture() {
        return fixture_check(&dir, token, &host);
    }
    let info = crate::mr::token_check_with(remote, token);
    match crate::mr::whois_with(remote, token) {
        Ok((name, user)) => {
            let (scopes, expires) = info.unwrap_or_default();
            Ok(Access {
                who: format!("{name} (@{user})"),
                user,
                scopes,
                expires,
            })
        }
        Err(e) => Err(refused(&format!("{e:#}"), info.map(|(s, _)| s), &host)),
    }
}

/// Why a token was not taken, from what the host said.
fn refused(error: &str, scopes: Option<Vec<String>>, host: &str) -> String {
    match scopes {
        Some(scopes) if !scopes.is_empty() => {
            format!("its token is for cloning only ({})", scopes.join(", "))
        }
        _ if error.contains("refused") => format!("{host} refused its token"),
        _ => error.to_owned(),
    }
}

/// The checks' answers: `tokens.txt`, a line a token — `<token> <user> <scopes,…> [<expires>]`, its
/// user `-` when the host refuses it; a token not listed is refused.
fn fixture_check(dir: &Path, token: &str, host: &str) -> Result<Access, String> {
    let text = std::fs::read_to_string(dir.join("tokens.txt")).unwrap_or_default();
    let line = text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>())
        .find(|w| w.first() == Some(&token));
    let Some(words) = line else {
        return Err(refused("refused", None, host));
    };
    let scopes: Vec<String> = words
        .get(2)
        .map(|s| s.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    match words.get(1) {
        Some(&user) if user != "-" => Ok(Access {
            who: format!("Fixture (@{user})"),
            user: user.to_owned(),
            scopes,
            expires: words.get(3).map(|e| (*e).to_owned()),
        }),
        _ => Err(refused("refused", Some(scopes), host)),
    }
}

/// Why an IDE signed in with OAuth is not offered: its token lasts two hours, and refreshing it
/// would sign the IDE out.
pub const OAUTH: &str = "signed in with OAuth, whose token lasts two hours: not copied";

/// What looking found on this computer for a host: the ways, each token checked with the host,
/// and those found that cannot be used. The SSH key's answer comes apart (`look_ssh`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Looked {
    pub ways: Vec<Way>,
    pub not_here: Vec<NotHere>,
}

/// Looks for the ways in to `remote`'s host, for repository `root` whose remote's URL is `url`:
/// the CLI's token, what git keeps (the URL's, else the credential helper's), and the JetBrains
/// IDEs signed in to the host. A token is checked with the host before it is listed; git's one the
/// host takes is kept for this run. Slow (processes, the network): off the window's thread.
pub fn look_here(root: &Path, url: &str, remote: &WebRemote) -> Looked {
    let host = crate::mr::host(remote);
    let mut looked = Looked::default();
    let (cli, git) = std::thread::scope(|s| {
        let cli = beside(s, || look_cli(remote));
        let git = beside(s, || look_git(root, url, remote));
        (cli.join().ok().flatten(), git.join().unwrap_or_default())
    });
    for found in cli.into_iter().chain(git) {
        match found {
            Ok(way) => looked.ways.push(way),
            Err(not) => looked.not_here.push(not),
        }
    }
    let oauth = note(&crate::mr::api_host(remote)).oauth;
    for account in crate::ide::accounts(&host, remote.github) {
        if oauth.contains(&account.id) {
            looked.not_here.push(NotHere {
                via: Via::Ide(account),
                why: OAUTH.to_owned(),
            });
        } else {
            looked.ways.push(Way {
                user: Some(account.user.clone()),
                via: Via::Ide(account),
                scopes: Vec::new(),
                write: true,
                expires: None,
            });
        }
    }
    order(&mut looked.ways);
    looked
}

/// Runs `work` on a thread of its own, beside the others; a test's fixture goes with it.
fn beside<'s, T: Send + 's>(
    s: &'s std::thread::Scope<'s, '_>,
    work: impl FnOnce() -> T + Send + 's,
) -> std::thread::ScopedJoinHandle<'s, T> {
    #[cfg(test)]
    let test = TEST_FIXTURE.with(|f| f.borrow().clone());
    s.spawn(move || {
        #[cfg(test)]
        TEST_FIXTURE.with(|f| *f.borrow_mut() = test);
        work()
    })
}

fn look_cli(remote: &WebRemote) -> Option<Result<Way, NotHere>> {
    let host = crate::mr::host(remote);
    let not = |why: String| Some(Err(NotHere { via: Via::Cli, why }));
    match cli(remote) {
        CliSaid::Absent => None,
        CliSaid::Token(token, _) => match check(remote, &token) {
            Ok(access) => Some(Ok(access.way(Via::Cli, remote.github))),
            Err(why) => not(why),
        },
        CliSaid::Elsewhere(hosts) if hosts.is_empty() => not(format!("not signed in to {host}")),
        CliSaid::Elsewhere(hosts) => {
            not(format!("signed in to {}, not to {host}", hosts.join(", ")))
        }
    }
}

/// What git keeps: the remote's URL's token, else its credential helper's; the first the host
/// takes is a way and kept for this run, one it refuses is said.
fn look_git(root: &Path, url: &str, remote: &WebRemote) -> Vec<Result<Way, NotHere>> {
    let api_host = crate::mr::api_host(remote);
    let mut out = Vec::new();
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
        match check(remote, &token) {
            Ok(access) => {
                keep_found(&api_host, Some((token, from)));
                out.push(Ok(access.way(Via::Git(from), remote.github)));
                return out;
            }
            Err(why) => out.push(Err(NotHere {
                via: Via::Git(from),
                why,
            })),
        }
    }
    out
}

/// Who the host knows the SSH key as, the way a token is made with it; GitHub says who you are
/// over SSH but makes no tokens, so it is not asked. Slow (ssh, the network).
pub fn look_ssh(url: &str, remote: &WebRemote) -> Result<Known, String> {
    if remote.github {
        return Err("GitHub says who you are over SSH, but makes no tokens".to_owned());
    }
    whoami(&Ssh::of(url, remote))
}

/// Hosts whose borrowed token was read this run (`borrow`).
fn borrowed() -> &'static Mutex<Vec<String>> {
    static BORROWED: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    BORROWED.get_or_init(Default::default)
}

/// Whether the way picked for the host borrows a token (the CLI's, git's) not read yet this run.
pub fn to_borrow(remote: &WebRemote) -> bool {
    let host = crate::mr::api_host(remote);
    matches!(note(&host).way.as_deref(), Some("cli" | "git"))
        && borrowed().lock().is_ok_and(|b| !b.contains(&host))
}

/// Reads the token of the way picked for the host, once a run, unchecked — the requests that
/// follow are the check: the CLI's, or what git keeps for repository `root` with remote `url`.
/// Slow (processes): off the window's thread.
pub fn borrow(root: &Path, url: &str, remote: &WebRemote) {
    let host = crate::mr::api_host(remote);
    match borrowed().lock() {
        Ok(mut done) if !done.contains(&host) => done.push(host.clone()),
        _ => return,
    }
    match note(&host).way.as_deref() {
        Some("cli") => {
            cli(remote);
        }
        Some("git") => {
            let token = url_token(url).map(|t| (t, "the remote's URL")).or_else(|| {
                credential_token(root, &crate::mr::host(remote))
                    .map(|t| (t, "git's credential helper"))
            });
            keep_found(&host, token);
        }
        _ => {}
    }
}

/// The token of a borrowed way picked for the host, as `mr::token_source` gives it: from what was
/// read this run, or — `run` — read now (the CLI; git's credential helper, from the working
/// directory), off the window's thread.
pub fn borrowed_token(remote: &WebRemote, run: bool) -> Option<(String, &'static str)> {
    let host = crate::mr::api_host(remote);
    match note(&host).way.as_deref()? {
        "cli" => {
            let token = if run {
                match cli(remote) {
                    CliSaid::Token(token, _) => Some(token),
                    _ => None,
                }
            } else {
                cli_token_now(&host)
            }?;
            Some((
                token,
                if remote.github {
                    "the GitHub CLI (gh)"
                } else {
                    "the GitLab CLI (glab)"
                },
            ))
        }
        "git" => found_token(&host).or_else(|| {
            run.then(|| {
                let token = credential_token(Path::new("."), &crate::mr::host(remote))?;
                keep_found(&host, Some((token, "git's credential helper")));
                found_token(&host)
            })
            .flatten()
        }),
        _ => None,
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

/// `GITLANCE_SSH_FIXTURE=<dir>` answers for git's credential helper, ssh, the CLIs, the IDEs and
/// the host's checks of a token (UI checks): `credential.txt`, `whoami.txt`, `made.txt` (`{scope}`
/// the scope asked), `glab.txt` / `gh.txt` (what `auth status --show-token` prints), `tokens.txt`
/// (`fixture_check`), `ide/JetBrains/<IDE><version>/options/gitlab.xml` and `keychain/<account id>`
/// (what the IDE keeps), each missing file a no; tokens saved go under its `home/`. A test sets its
/// own (`TEST_FIXTURE`).
pub(crate) fn fixture() -> Option<std::path::PathBuf> {
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
    fn a_token_name_goes_to_gitlab_shell_as_one_word() {
        assert_eq!(
            shell_word("GitLance (011570-MAC)"),
            "'GitLance (011570-MAC)'"
        );
        assert_eq!(
            shell_word("GitLance (Ann's Mac)"),
            r"'GitLance (Ann'\''s Mac)'"
        );
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
        assert!(ssh_why("git@h: Permission denied (publickey).").contains("did not accept"));
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
    fn a_clis_status_gives_its_token_and_account() {
        let glab = "gitlab.example.com\n  ✓ Logged in to gitlab.example.com as ann (/Users/ann/.config/glab-cli/config.yml)\n  ✓ Git operations for gitlab.example.com configured to use ssh protocol.\n  ✓ API calls for gitlab.example.com are made over https protocol.\n  ✓ REST API Endpoint: https://gitlab.example.com/api/v4/\n  ✓ Token found in keyring: glpat-glab12345\n";
        assert_eq!(
            cli_answer(glab, "gitlab.example.com"),
            CliSaid::Token("glpat-glab12345".into(), Some("ann".into()))
        );
        let older = "gitlab.example.com\n  ✓ Logged in to gitlab.example.com as ann (GITLAB_TOKEN)\n  ✓ Token: glpat-old12345\n";
        assert_eq!(
            cli_answer(older, "gitlab.example.com"),
            CliSaid::Token("glpat-old12345".into(), Some("ann".into()))
        );
        // gh: the host's first account is the active one.
        let gh = "github.com\n  ✓ Logged in to github.com account ann (keyring)\n  - Active account: true\n  - Git operations protocol: https\n  - Token: gho_active123\n  - Token scopes: 'gist', 'read:org', 'repo'\n\n  ✓ Logged in to github.com account ann-work (keyring)\n  - Active account: false\n  - Token: gho_other4567\n";
        assert_eq!(
            cli_answer(gh, "github.com"),
            CliSaid::Token("gho_active123".into(), Some("ann".into()))
        );
        // Masked, the token is not taken; another host is said.
        let masked = "gitlab.com\n  ✓ Logged in to gitlab.com as ann (keyring)\n  ✓ Token found: **************************\n";
        assert_eq!(
            cli_answer(masked, "gitlab.example.com"),
            CliSaid::Elsewhere(vec!["gitlab.com".into()])
        );
        assert_eq!(
            cli_answer(
                "x gitlab.example.com has not been authenticated with glab. Run `glab auth login`",
                "gitlab.example.com"
            ),
            CliSaid::Elsewhere(Vec::new())
        );
    }

    #[test]
    fn the_ways_go_easiest_first() {
        let way = |via: Via, write: bool| Way {
            via,
            user: None,
            scopes: Vec::new(),
            write,
            expires: None,
        };
        let ide = crate::ide::Account {
            product: "IntelliJ IDEA".into(),
            id: "1".into(),
            user: "ann".into(),
            github: false,
        };
        let mut ways = vec![
            way(Via::Hand, true),
            way(Via::Ide(ide.clone()), true),
            way(Via::Git("git's credential helper"), false),
            way(Via::Ssh, true),
            way(Via::Cli, true),
        ];
        order(&mut ways);
        let kinds: Vec<&str> = ways.iter().map(|w| w.via.key()).collect();
        assert_eq!(kinds, ["cli", "ssh", "ide", "git", "pasted"]);
    }

    #[test]
    fn a_token_is_checked_before_it_is_listed() {
        let dir = tempfile::TempDir::new().unwrap();
        TEST_FIXTURE.with(|f| *f.borrow_mut() = Some(dir.path().to_path_buf()));
        std::fs::write(
            dir.path().join("tokens.txt"),
            "glpat-api1 ann api,read_repository 2026-12-12\nglpat-clone1 - read_repository\n",
        )
        .unwrap();
        let ok = check(&gitlab(), "glpat-api1").unwrap();
        assert_eq!(ok.user, "ann");
        assert_eq!(ok.expires.as_deref(), Some("2026-12-12"));
        assert!(ok.way(Via::Cli, false).write);
        assert_eq!(
            check(&gitlab(), "glpat-clone1"),
            Err("its token is for cloning only (read_repository)".into())
        );
        assert_eq!(
            check(&gitlab(), "glpat-unknown"),
            Err("gitlab.example.com refused its token".into())
        );
        TEST_FIXTURE.with(|f| *f.borrow_mut() = None);
    }

    #[test]
    fn looking_lists_each_way_with_its_account_or_why_not() {
        let dir = tempfile::TempDir::new().unwrap();
        TEST_FIXTURE.with(|f| *f.borrow_mut() = Some(dir.path().to_path_buf()));
        let host = "https://gitlab.example.com";
        let url = "git@gitlab.example.com:g/p.git";
        std::fs::write(dir.path().join("whoami.txt"), "Welcome to GitLab, @ann!\n").unwrap();
        // Nothing here: no CLI named, git keeps nothing; who SSH says.
        let looked = look_here(dir.path(), url, &gitlab());
        assert_eq!(looked, Looked::default());
        assert_eq!(look_ssh(url, &gitlab()), Ok(Known::GitLab("ann".into())));
        // glab signed in elsewhere; git's token for cloning only; an IDE signed in.
        drop_cli(host);
        std::fs::write(
            dir.path().join("glab.txt"),
            "gitlab.com\n  ✓ Logged in to gitlab.com as ann (keyring)\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("credential.txt"),
            "username=ann\npassword=glpat-clone1\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tokens.txt"),
            "glpat-clone1 - read_repository\nglpat-cli1 ann api\n",
        )
        .unwrap();
        let options = dir.path().join("ide/JetBrains/IntelliJIdea2026.2/options");
        std::fs::create_dir_all(&options).unwrap();
        std::fs::write(
            options.join("gitlab.xml"),
            r#"<component name="GitLabAccounts"><![CDATA[{"accounts":[{"id":"5f2c","name":"ann","server":{"uri":"https://gitlab.example.com"}}]}]]></component>"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("keychain")).unwrap();
        std::fs::write(dir.path().join("keychain/5f2c"), "glpat-ide12345").unwrap();
        let looked = look_here(dir.path(), url, &gitlab());
        assert_eq!(looked.ways.len(), 1);
        assert!(matches!(&looked.ways[0].via, Via::Ide(a) if a.product == "IntelliJ IDEA"));
        let why: Vec<(&str, &str)> = looked
            .not_here
            .iter()
            .map(|n| (n.via.key(), n.why.as_str()))
            .collect();
        assert_eq!(
            why,
            [
                ("cli", "signed in to gitlab.com, not to gitlab.example.com"),
                ("git", "its token is for cloning only (read_repository)"),
            ]
        );
        assert_eq!(found_token(host), None);
        // glab signed in to the host: first, with its account; a way remembered gives its token.
        drop_cli(host);
        std::fs::write(
            dir.path().join("glab.txt"),
            "gitlab.example.com\n  ✓ Logged in to gitlab.example.com as ann (keyring)\n  ✓ Token found in keyring: glpat-cli1\n",
        )
        .unwrap();
        let looked = look_here(dir.path(), url, &gitlab());
        assert_eq!(looked.ways[0].via, Via::Cli);
        assert_eq!(looked.ways[0].user.as_deref(), Some("ann"));
        assert_eq!(borrowed_token(&gitlab(), false), None);
        set_note(host, |n| n.way = Some("cli".into()));
        assert_eq!(
            borrowed_token(&gitlab(), false),
            Some(("glpat-cli1".into(), "the GitLab CLI (glab)"))
        );
        set_note(host, |n| n.way = None);
        drop_cli(host);
        // Made over SSH.
        std::fs::write(
            dir.path().join("made.txt"),
            "Token:   glpat-made1\nScopes:  {scope}\nExpires: 2026-11-26\n",
        )
        .unwrap();
        let made = make(&Ssh::of(url, &gitlab()), "GitLance (Mac)", "read_api").unwrap();
        assert_eq!(made.token, "glpat-made1");
        TEST_FIXTURE.with(|f| *f.borrow_mut() = None);
    }

    #[test]
    fn a_note_from_before_the_ways_is_read() {
        let old = r#"{"https://h":{"made":{"name":"GitLance (Mac)","scope":"api","expires":"2026-11-26","tail":"1234"}}}"#;
        let notes: HashMap<String, HostNote> = serde_json::from_str(old).unwrap();
        assert_eq!(notes["https://h"].way, None);
        assert!(notes["https://h"].oauth.is_empty());
    }
}
