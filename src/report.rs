//! Reporting a problem from the app (#118): what goes with a report — the version always; the
//! system, the log's last lines and the window's picture while they are ticked —, said with nothing
//! private (paths under `~`, hosts, project names, tokens and e-mail addresses named for what they
//! are), and the two ways it goes: the relay, which files it in the project's private reports
//! (the app holds no key), or GitHub's new-issue page of the project, filled in.

use anyhow::{Result, anyhow, bail};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The relay's address (`relay/`); `GITLANCE_REPORT_URL` stands in for it.
const RELAY: &str = "https://gitlance-reports.h-humbat0.workers.dev";
/// Where a report goes when it is posted by hand.
const NEW_ISSUE: &str = "https://github.com/Thernal/gitlance-desktop/issues/new";
/// The log lines a report carries.
pub const LOG_LINES: usize = 200;
/// GitHub refuses longer new-issue addresses; the log is cut first to fit.
const URL_MAX: usize = 7_500;

/// The relay's address, when there is one.
pub fn relay() -> Option<String> {
    std::env::var("GITLANCE_REPORT_URL")
        .ok()
        .or_else(|| (!RELAY.is_empty()).then(|| RELAY.to_owned()))
        .map(|u| u.trim_end_matches('/').to_owned())
        .filter(|u| u.starts_with("https://") || u.starts_with("http://127.0.0.1"))
}

/// "GitLance 0.2.0 (7c5758f)": which build it is.
pub fn version() -> String {
    format!(
        "GitLance {} ({})",
        env!("CARGO_PKG_VERSION"),
        env!("GITLANCE_COMMIT")
    )
}

/// The system and the machine: "macOS 15.1 · Mac15,3".
pub fn system() -> String {
    let run = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .stderr(Stdio::null())
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .filter(|s| !s.is_empty())
    };
    if cfg!(target_os = "macos") {
        let os = run("sw_vers", &["-productVersion"]).unwrap_or_default();
        let model = run("sysctl", &["-n", "hw.model"]).unwrap_or_default();
        format!("macOS {os} · {model}")
    } else if cfg!(target_os = "linux") {
        let os = std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|t| {
                t.lines()
                    .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                    .map(|v| v.trim_matches('"').to_owned())
            })
            .unwrap_or_else(|| "Linux".to_owned());
        format!("{os} · {}", std::env::consts::ARCH)
    } else {
        format!("{} · {}", std::env::consts::OS, std::env::consts::ARCH)
    }
}

/// What a report must not carry, and what it says instead.
#[derive(Clone, Debug, Default)]
pub struct Scrub {
    home: Option<String>,
    /// Private hosts (`gitlab.example.com`), longest first.
    hosts: Vec<String>,
    /// Projects' names and paths (`payments-service`, `acme/payments-service`), longest first.
    projects: Vec<String>,
}

impl Scrub {
    /// `hosts` and `projects` as known here; the public hosts stay as they are.
    pub fn new(hosts: Vec<String>, projects: Vec<String>) -> Self {
        let mut hosts: Vec<String> = hosts
            .into_iter()
            .filter(|h| !h.is_empty() && h != "github.com" && h != "gitlab.com")
            .collect();
        let mut projects: Vec<String> = projects.into_iter().filter(|p| p.len() >= 3).collect();
        for list in [&mut hosts, &mut projects] {
            list.sort_by_key(|s| std::cmp::Reverse(s.len()));
            list.dedup();
        }
        Self {
            home: std::env::home_dir().map(|h| h.display().to_string()),
            hosts,
            projects,
        }
    }

    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_owned();
        if let Some(home) = self.home.as_deref().filter(|h| h.len() > 1) {
            out = out.replace(home, "~");
        }
        for host in &self.hosts {
            out = out.replace(host.as_str(), "<host>");
        }
        for project in &self.projects {
            out = out.replace(project.as_str(), "<project>");
        }
        use std::sync::LazyLock;
        static TOKEN: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(
                r"glpat-[A-Za-z0-9_\-]{10,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}",
            )
            .unwrap()
        });
        static EMAIL: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").unwrap()
        });
        let out = TOKEN.replace_all(&out, "<token>");
        EMAIL.replace_all(&out, "<email>").into_owned()
    }
}

/// A report as written, and what goes with it.
#[derive(Clone, Debug, Default)]
pub struct Draft {
    pub title: String,
    pub description: String,
    pub system: bool,
    pub log: bool,
    pub picture: bool,
    /// Drafted by an agent's chat (`report_problem`): who.
    pub by: Option<String>,
}

/// The issue's body as it goes: the description, then the build (and the system), the log folded.
/// The picture is the relay's to add: it stores it beside the issue.
pub fn body(draft: &Draft, scrub: &Scrub, system: &str, log: &[String]) -> String {
    let mut out = scrub.apply(draft.description.trim());
    out.push_str("\n\n---\n");
    out.push_str(&version());
    if draft.system && !system.is_empty() {
        out.push_str(" · ");
        out.push_str(system);
    }
    if let Some(by) = &draft.by {
        out.push_str(&format!(
            "\n\n_Drafted by {by} (an agent) from an error it hit; checked and sent by the reporter._"
        ));
    }
    if draft.log && !log.is_empty() {
        out.push_str(&format!(
            "\n\n<details><summary>Log, the last {} lines</summary>\n\n```\n{}\n```\n</details>",
            log.len(),
            scrub.apply(&log.join("\n"))
        ));
    }
    out
}

/// What makes two reports the same problem, for the relay to count them as one: the title's
/// words and the version.
pub fn fingerprint(title: &str) -> String {
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let mut hash = sha1_smol::Sha1::new();
    hash.update(words.join(" ").as_bytes());
    hash.update(env!("CARGO_PKG_VERSION").as_bytes());
    hash.digest().to_string()[..12].to_owned()
}

/// GitHub's new-issue page of the project, filled in; a long log is cut from its start so the
/// address stays one GitHub takes.
pub fn new_issue_url(title: &str, body: &str) -> String {
    let encode = |s: &str| -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
    let url = |body: &str| format!("{NEW_ISSUE}?title={}&body={}", encode(title), encode(body));
    let mut body = body.to_owned();
    loop {
        let whole = url(&body);
        if whole.len() <= URL_MAX {
            return whole;
        }
        // The log's oldest line goes first; with no log left, the end of the text.
        let start = body.find("```\n").map(|at| at + 4);
        let line = start.and_then(|s| {
            let end = s + body[s..].find('\n')? + 1;
            (!body[s..end].starts_with("```")).then_some((s, end))
        });
        match line {
            Some((s, end)) => body.replace_range(s..end, ""),
            None => {
                let mut at = body.len().saturating_sub(whole.len() - URL_MAX + 3);
                while !body.is_char_boundary(at) {
                    at -= 1;
                }
                body.truncate(at);
                body.push('…');
            }
        }
    }
}

/// This installation's id, made once: the relay counts reports by it, not by person.
pub fn install_id() -> String {
    let Some(file) = crate::platform::data_dir().map(|d| d.join("install-id.txt")) else {
        return "unknown".to_owned();
    };
    if let Ok(id) = std::fs::read_to_string(&file) {
        let id = id.trim();
        if id.len() >= 16 {
            return id.to_owned();
        }
    }
    let mut hash = sha1_smol::Sha1::new();
    hash.update(crate::agents::uid().as_bytes());
    hash.update(format!("{:?}", std::env::home_dir()).as_bytes());
    let id = hash.digest().to_string()[..20].to_owned();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&file, &id);
    id
}

/// A report sent from here: its id at the relay, when, its title.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sent {
    pub id: String,
    pub at: i64,
    pub title: String,
    /// What became of it, as the relay last said: received, moved to the project, a duplicate.
    #[serde(default)]
    pub state: String,
    /// Where it is public, once it is.
    #[serde(default)]
    pub url: String,
}

fn sent_file() -> Option<PathBuf> {
    crate::platform::data_dir().map(|d| d.join("reports.json"))
}

/// The reports sent from here, newest first.
pub fn sent() -> Vec<Sent> {
    sent_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn keep_sent(list: &[Sent]) {
    if let (Some(file), Ok(text)) = (sent_file(), serde_json::to_string_pretty(list)) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, text);
    }
}

/// `body` (JSON) to `url` with curl, its answer as JSON; nothing secret goes, so on the command
/// line is fine, the body on stdin. When nothing reached the relay — its name, a connection, TLS
/// (curl 6, 7, 35) — it is sent once more a second later: the first send after launch was seen to
/// fail so (2026-10-23), and a report that never arrived cannot arrive twice.
fn post(url: &str, body: &Value) -> Result<Value> {
    let out = post_once(url, body)?;
    if matches!(out.status.code(), Some(6 | 7 | 35)) {
        unreached(&out);
        std::thread::sleep(std::time::Duration::from_secs(1));
        return answer(post_once(url, body)?);
    }
    answer(out)
}

fn post_once(url: &str, body: &Value) -> Result<std::process::Output> {
    let mut child = Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "60",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
            "-w",
            "\n%{http_code}",
            url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| anyhow!("curl is not installed"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(body.to_string().as_bytes())?;
    }
    Ok(child.wait_with_output()?)
}

/// curl's own reason in the app's log, when the relay could not be reached.
fn unreached(out: &std::process::Output) {
    let reason = String::from_utf8_lossy(&out.stderr);
    crate::applog::line(&format!(
        "report: curl {}: {}",
        out.status.code().unwrap_or(-1),
        reason.trim()
    ));
}

fn get(url: &str) -> Result<Value> {
    let out = Command::new("curl")
        .args(["-sS", "--max-time", "20", "-w", "\n%{http_code}", url])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| anyhow!("curl is not installed"))?;
    answer(out)
}

fn answer(out: std::process::Output) -> Result<Value> {
    if !out.status.success() {
        unreached(&out);
        bail!("The reports can't be reached");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    let value: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    match code.trim().parse::<u16>().unwrap_or(0) {
        200..=299 => Ok(value),
        429 => bail!(
            "Too many reports from here today — try again tomorrow, or open it in the browser"
        ),
        503 => bail!("The reports are closed for now — open it in the browser instead"),
        code => bail!(
            "{}",
            value["error"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("The reports answered {code}"))
        ),
    }
}

/// Hands the report to the relay; its id there.
pub fn send(
    relay: &str,
    title: &str,
    body: &str,
    by_agent: bool,
    picture: Option<&[u8]>,
) -> Result<String> {
    let payload = json!({
        "install": install_id(),
        "version": env!("CARGO_PKG_VERSION"),
        "commit": env!("GITLANCE_COMMIT"),
        "title": title,
        "body": body,
        "fingerprint": fingerprint(title),
        "agent": by_agent,
        "picture": picture.map(|p| base64::engine::general_purpose::STANDARD.encode(p)),
    });
    let value = post(&format!("{relay}/v1/report"), &payload)?;
    value["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("The reports did not say what they kept it as"))
}

/// What became of report `id`: its state, and its public address once it has one.
pub fn status(relay: &str, id: &str) -> Result<(String, String)> {
    let value = get(&format!("{relay}/v1/report/{id}?install={}", install_id()))?;
    Ok((
        value["state"].as_str().unwrap_or_default().to_owned(),
        value["url"].as_str().unwrap_or_default().to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_private_goes() {
        let scrub = Scrub {
            home: Some("/Users/kestrel".into()),
            hosts: vec!["gitlab.example.com".into()],
            projects: vec!["acme/payments-service".into(), "payments-service".into()],
        };
        let said = scrub.apply(
            "open /Users/kestrel/Projects/payments-service failed at https://gitlab.example.com/acme/payments-service \
             with glpat-abcdefghijklmnop for kestrel@example.com",
        );
        assert_eq!(
            said,
            "open ~/Projects/<project> failed at https://<host>/<project> with <token> for <email>"
        );
    }

    #[test]
    fn public_hosts_stay() {
        let scrub = Scrub::new(vec!["github.com".into(), "gitlab.com".into()], vec![]);
        assert_eq!(scrub.apply("github.com/x"), "github.com/x");
    }

    #[test]
    fn the_body_carries_what_is_ticked() {
        let scrub = Scrub::default();
        let mut draft = Draft {
            title: "t".into(),
            description: "It broke.".into(),
            ..Default::default()
        };
        let log = vec!["12:00 one".to_owned()];
        let bare = body(&draft, &scrub, "macOS 15.1 · Mac15,3", &log);
        assert!(bare.starts_with("It broke.\n\n---\nGitLance "));
        assert!(!bare.contains("macOS") && !bare.contains("<details>"));
        draft.system = true;
        draft.log = true;
        draft.by = Some("Claude Code".into());
        let full = body(&draft, &scrub, "macOS 15.1 · Mac15,3", &log);
        assert!(full.contains(" · macOS 15.1 · Mac15,3"));
        assert!(full.contains("Drafted by Claude Code"));
        assert!(full.contains("```\n12:00 one\n```"));
    }

    #[test]
    fn the_same_words_are_the_same_problem() {
        assert_eq!(
            fingerprint("Empty diff, after ⌘R"),
            fingerprint("empty  diff after ⌘r")
        );
        assert_ne!(fingerprint("Empty diff"), fingerprint("Wrong diff"));
    }

    #[test]
    fn a_long_report_still_fits_the_address() {
        let log: String = (0..2000).map(|n| format!("line {n}\n")).collect();
        let body = format!("text\n\n```\n{log}```\n");
        let url = new_issue_url("A title", &body);
        assert!(url.len() <= URL_MAX);
        assert!(url.contains("title=A%20title"));
        assert!(url.contains("line%201999"), "the newest lines are kept");
    }
}
