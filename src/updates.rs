//! Whether a newer GitLance is out (#135): its releases on GitHub, asked when the app starts and every 6
//! hours (`ui::run`), the newer ones kept here for the window to announce — a card once per version, then a
//! mark in the title bar (`ui/updates.rs`). Nothing is downloaded or installed: the builds are not signed,
//! so updating stays the user's (README → Releases). Settings → General turns the asking off.

use anyhow::{Result, anyhow, bail};
use semver::Version;
use serde::Deserialize;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// The project's releases, newest first.
const RELEASES: &str = "https://api.github.com/repos/Thernal/gitlance-desktop/releases?per_page=30";
/// How often a running app asks again.
pub const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// A release's notes end in its commits under this heading (`.github/release-notes.sh`).
const COMMITS: &str = "### Commits";

/// A release newer than this build.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    /// What is new, in words: the notes above its commits.
    pub notes: String,
    /// Its commits by kind, as markdown; empty for a release without them.
    pub commits: String,
    /// How many commits that is.
    pub count: usize,
    /// Its page on GitHub, with the files.
    pub url: String,
    /// When it came out (Unix seconds), when GitHub says.
    pub published: Option<i64>,
}

/// What the last check found.
#[derive(Clone, Debug, Default)]
pub struct Check {
    /// The releases newer than this build, newest first.
    pub newer: Vec<Release>,
    /// When GitHub answered, or the asking failed.
    pub at: Option<SystemTime>,
    /// Why there is no answer.
    pub error: Option<String>,
}

/// The release as GitHub gives it.
#[derive(Deserialize)]
struct Raw {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
}

/// Which version's card was shown by itself, and which one the user skipped.
#[derive(Default)]
struct Seen {
    shown: Option<Version>,
    skipped: Option<Version>,
}

static LAST: Mutex<Option<Check>> = Mutex::new(None);
static SEEN: Mutex<Option<Seen>> = Mutex::new(None);

/// This build's version.
pub fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0))
}

/// Asks GitHub (or `GITLANCE_RELEASES_FIXTURE`, a file of its answer) and keeps what it found.
/// Blocking: run it off the window's thread.
pub fn check() -> Check {
    let check = match fetch().and_then(|text| parse(&text, &current())) {
        Ok(newer) => Check {
            newer,
            at: Some(SystemTime::now()),
            error: None,
        },
        Err(e) => {
            crate::applog::line(&format!("updates: {e}"));
            Check {
                at: Some(SystemTime::now()),
                error: Some(e.to_string()),
                // What an earlier check found still holds.
                newer: last().map(|c| c.newer).unwrap_or_default(),
            }
        }
    };
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(check.clone());
    check
}

/// The last check, if one ran.
pub fn last() -> Option<Check> {
    LAST.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Whether it is time to ask: never asked, or `EVERY` since. A snapshot asks only its fixture.
pub fn due() -> bool {
    if std::env::var_os("GITLANCE_SNAPSHOT").is_some()
        && std::env::var_os("GITLANCE_RELEASES_FIXTURE").is_none()
    {
        return false;
    }
    match last().and_then(|c| c.at) {
        None => true,
        Some(at) => at.elapsed().map_or(true, |e| e >= EVERY),
    }
}

/// The newest release out, unless it is the one the user skipped.
pub fn newest() -> Option<Release> {
    let release = last()?.newer.into_iter().next()?;
    let skipped = with_seen(|s| s.skipped.clone());
    (skipped.as_ref() != Some(&release.version)).then_some(release)
}

/// The newest release whose card has not opened by itself yet; taking it counts as shown.
pub fn take_news() -> Option<Release> {
    let release = newest()?;
    with_seen(|s| {
        if s.shown.as_ref() == Some(&release.version) {
            return None;
        }
        s.shown = Some(release.version.clone());
        save(s);
        Some(release)
    })
}

/// Quiet about `version` until a newer one is out.
pub fn skip(version: &Version) {
    with_seen(|s| {
        s.skipped = Some(version.clone());
        save(s);
    })
}

/// `updates.txt`, read once.
fn with_seen<T>(f: impl FnOnce(&mut Seen) -> T) -> T {
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let seen = seen.get_or_insert_with(|| {
        let mut s = Seen::default();
        for line in crate::storage::read("updates.txt").lines() {
            match line.split_once('=') {
                Some(("shown", v)) => s.shown = Version::parse(v.trim()).ok(),
                Some(("skipped", v)) => s.skipped = Version::parse(v.trim()).ok(),
                _ => {}
            }
        }
        s
    });
    f(seen)
}

fn save(seen: &Seen) {
    let mut text = String::new();
    if let Some(v) = &seen.shown {
        text.push_str(&format!("shown={v}\n"));
    }
    if let Some(v) = &seen.skipped {
        text.push_str(&format!("skipped={v}\n"));
    }
    crate::storage::save("updates.txt", text);
}

/// GitHub's answer, as text. No token: the releases are public, and 60 asks an hour is plenty.
fn fetch() -> Result<String> {
    if let Some(file) = std::env::var_os("GITLANCE_RELEASES_FIXTURE") {
        return Ok(std::fs::read_to_string(file)?);
    }
    let agent = format!("User-Agent: GitLance/{}", env!("CARGO_PKG_VERSION"));
    let out = Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "20",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            &agent,
            "-w",
            "\n%{http_code}",
            RELEASES,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| anyhow!("curl is not installed"))?;
    if !out.status.success() {
        bail!("GitHub can't be reached");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    match code.trim() {
        "200" => Ok(body.to_owned()),
        "403" | "429" => bail!("GitHub asks to wait — too many checks from this address"),
        code => bail!("GitHub answered {code}"),
    }
}

/// The releases newer than `current`, newest first: tags `release/v<version>`, never a draft, and a
/// prerelease only to a prerelease build.
fn parse(text: &str, current: &Version) -> Result<Vec<Release>> {
    let raw: Vec<Raw> = serde_json::from_str(text)?;
    let mut newer: Vec<Release> = raw
        .into_iter()
        .filter(|r| !r.draft && (!r.prerelease || !current.pre.is_empty()))
        .filter_map(|r| {
            let version = Version::parse(r.tag_name.strip_prefix("release/v")?).ok()?;
            if &version <= current || (!version.pre.is_empty() && current.pre.is_empty()) {
                return None;
            }
            let body = r.body.unwrap_or_default().replace("\r\n", "\n");
            let (notes, commits) = split(&body);
            Some(Release {
                version,
                count: commits.lines().filter(|l| l.starts_with("- ")).count(),
                notes,
                commits,
                url: r.html_url,
                published: r
                    .published_at
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok())
                    .map(|t| t.timestamp()),
            })
        })
        .collect();
    newer.sort_by(|a, b| b.version.cmp(&a.version));
    Ok(newer)
}

/// A body as its words and its commits: the notes end where `### Commits` begins a line.
fn split(body: &str) -> (String, String) {
    let at = if body.starts_with(COMMITS) {
        Some(0)
    } else {
        body.find(&format!("\n{COMMITS}")).map(|i| i + 1)
    };
    match at {
        Some(i) => {
            let commits = body[i + COMMITS.len()..].trim().to_owned();
            (body[..i].trim().to_owned(), commits)
        }
        None => (body.trim().to_owned(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, body: &str, draft: bool, pre: bool) -> String {
        serde_json::json!({
            "tag_name": tag,
            "body": body,
            "html_url": format!("https://github.com/Thernal/gitlance-desktop/releases/tag/{tag}"),
            "draft": draft,
            "prerelease": pre,
            "published_at": "2026-10-24T09:00:00Z",
        })
        .to_string()
    }

    fn list(items: &[String]) -> String {
        format!("[{}]", items.join(","))
    }

    #[test]
    fn newer_releases_come_newest_first() {
        let text = list(&[
            release("release/v1.0.1", "", false, false),
            release("release/v1.2.0", "", false, false),
            release("release/v1.1.0", "", false, false),
            release("release/v0.2.0", "", false, false),
        ]);
        let found = parse(&text, &Version::new(1, 0, 1)).unwrap();
        let versions: Vec<String> = found.iter().map(|r| r.version.to_string()).collect();
        assert_eq!(versions, ["1.2.0", "1.1.0"]);
        assert_eq!(found[0].published, Some(1_792_832_400));
    }

    #[test]
    fn drafts_never_and_prereleases_only_to_a_prerelease() {
        let text = list(&[
            release("release/v2.0.0", "", true, false),
            release("release/v1.1.0-rc1", "", false, true),
            release("v9.0.0", "", false, false),
        ]);
        assert!(parse(&text, &Version::new(1, 0, 1)).unwrap().is_empty());
        let rc = Version::parse("1.1.0-beta").unwrap();
        let found = parse(&text, &rc).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].version.to_string(), "1.1.0-rc1");
    }

    #[test]
    fn notes_stop_where_the_commits_begin() {
        let body = "- **Fetch on by default.**\r\n\r\n### Commits\r\n\r\n#### Fixes\r\n\r\n- **fetch:** on (#134) 7625106\r\n- **ui:** two 1234567\r\n";
        let text = list(&[release("release/v1.0.2", body, false, false)]);
        let found = parse(&text, &Version::new(1, 0, 1)).unwrap();
        assert_eq!(found[0].notes, "- **Fetch on by default.**");
        assert!(found[0].commits.starts_with("#### Fixes"));
        assert_eq!(found[0].count, 2);
        // A body with no commits is all notes; one with only commits has none.
        assert_eq!(split("Just words."), ("Just words.".into(), String::new()));
        assert_eq!(split("### Commits\n- a 1").0, "");
    }
}
