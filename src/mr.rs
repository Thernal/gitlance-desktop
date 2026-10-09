//! Merge requests of a GitLab project: the open ones with their titles, and the discussions on one.
//! Read-only, through `curl` and the token in `~/.config/gitlab-token` (so the VPN routes and
//! proxies of the machine apply, and no HTTP client is linked in). GitHub is not supported yet.

use crate::git::WebRemote;
use anyhow::{Result, anyhow, bail};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mr {
    pub iid: u64,
    pub title: String,
    pub author: String,
    pub source: String,
    pub target: String,
    pub url: String,
    /// Seconds since the epoch.
    pub updated: i64,
    pub comments: usize,
    pub draft: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub author: String,
    pub body: String,
    pub created: i64,
}

/// A discussion on a merge request: on a line of a file, or on the request as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thread {
    pub notes: Vec<Note>,
    pub path: Option<String>,
    /// 1-based line on the new side; `None` for a comment on a removed line or on no line.
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
    pub resolved: bool,
}

/// The token file, when there is one.
pub fn token() -> Option<String> {
    let path = std::env::home_dir()?.join(".config/gitlab-token");
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Whether merge requests can be read for this remote at all.
pub fn available(remote: &WebRemote) -> bool {
    fixture().is_some() || (!remote.github && token().is_some())
}

/// Whether the checks' fixture stands in for GitLab.
pub fn fixture_on() -> bool {
    fixture().is_some()
}

/// `GITLANCE_MR_FIXTURE=<dir>` reads `mrs.json` and `discussions-<iid>.json` from a directory
/// instead of the network (for UI checks).
fn fixture() -> Option<std::path::PathBuf> {
    std::env::var_os("GITLANCE_MR_FIXTURE").map(Into::into)
}

pub fn list(remote: &WebRemote) -> Result<Vec<Mr>> {
    let value = match fixture() {
        Some(dir) => serde_json::from_str(&std::fs::read_to_string(dir.join("mrs.json"))?)?,
        None => get(
            remote,
            "merge_requests?state=opened&order_by=updated_at&per_page=100",
        )?,
    };
    Ok(parse_mrs(&value))
}

pub fn threads(remote: &WebRemote, iid: u64) -> Result<Vec<Thread>> {
    let value = match fixture() {
        Some(dir) => {
            let file = dir.join(format!("discussions-{iid}.json"));
            match std::fs::read_to_string(file) {
                Ok(text) => serde_json::from_str(&text)?,
                Err(_) => Value::Array(Vec::new()),
            }
        }
        None => get(
            remote,
            &format!("merge_requests/{iid}/discussions?per_page=100"),
        )?,
    };
    Ok(parse_threads(&value))
}

/// `https://host`, and the project path with `/` as `%2F`.
fn api(remote: &WebRemote) -> Result<(String, String)> {
    let rest = remote
        .base
        .strip_prefix("https://")
        .or_else(|| remote.base.strip_prefix("http://"))
        .ok_or_else(|| anyhow!("unsupported remote {}", remote.base))?;
    let (host, path) = rest
        .split_once('/')
        .ok_or_else(|| anyhow!("no project in {}", remote.base))?;
    let scheme = if remote.base.starts_with("http://") {
        "http"
    } else {
        "https"
    };
    Ok((format!("{scheme}://{host}"), path.replace('/', "%2F")))
}

fn get(remote: &WebRemote, endpoint: &str) -> Result<Value> {
    request(remote, endpoint, &[])
}

/// A value inside a curl config file's double quotes.
fn quote(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// One call to the API: a GET, or a POST of `form` fields when there are any.
fn request(remote: &WebRemote, endpoint: &str, form: &[(&str, String)]) -> Result<Value> {
    let token = token().ok_or_else(|| anyhow!("no token in ~/.config/gitlab-token"))?;
    let (host, project) = api(remote)?;
    let url = format!("{host}/api/v4/projects/{project}/{endpoint}");
    let mut config = format!(
        "url = \"{}\"\nheader = \"PRIVATE-TOKEN: {}\"\n",
        quote(&url),
        quote(&token)
    );
    if !form.is_empty() {
        config.push_str("request = \"POST\"\n");
        for (name, value) in form {
            config.push_str(&format!("data-urlencode = \"{name}={}\"\n", quote(value)));
        }
    }
    // The token goes in on stdin, not on the command line, where `ps` would show it.
    let mut child = Command::new("curl")
        .args(["-sS", "--max-time", "25", "-K", "-", "-w", "\n%{http_code}"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| anyhow!("curl is not installed"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(config.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("Can't reach {host} — is the VPN on?");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or((&text, ""));
    match code.trim() {
        "200" | "201" => Ok(serde_json::from_str(body)?),
        "401" | "403" => bail!("GitLab refused the token"),
        "404" => bail!("GitLab does not know this project (or the token cannot see it)"),
        "400" => bail!("GitLab would not take the comment there: {}", message(body)),
        other => bail!("GitLab answered {other}"),
    }
}

/// The `message` GitLab sends with an error, as text.
fn message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").map(|m| m.to_string()))
        .unwrap_or_else(|| body.chars().take(160).collect())
}

/// Where a comment goes in a merge request's diff.
pub struct Place<'a> {
    pub path: &'a str,
    pub old_path: &'a str,
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
}

/// Posts `body` as a new discussion on a line of merge request `iid`, whose head the reviewer sees
/// as `head`. Refuses when GitLab's head is another commit: the line numbers would not mean the
/// same thing there.
pub fn post_discussion(
    remote: &WebRemote,
    iid: u64,
    head: &str,
    place: &Place<'_>,
    body: &str,
) -> Result<()> {
    if let Some(dir) = fixture() {
        // UI checks: record what would have been sent.
        let file = dir.join("posted.txt");
        let mut text = std::fs::read_to_string(&file).unwrap_or_default();
        text.push_str(&format!(
            "{iid} {} {:?} {:?} {body}\n",
            place.path, place.new_line, place.old_line
        ));
        std::fs::write(file, text)?;
        return Ok(());
    }
    let mr = get(remote, &format!("merge_requests/{iid}"))?;
    let refs = mr.get("diff_refs").filter(|r| !r.is_null());
    let sha = |key: &str| refs.map(|r| text(r, key)).unwrap_or_default();
    let (base, theirs, start) = (sha("base_sha"), sha("head_sha"), sha("start_sha"));
    if theirs.is_empty() {
        bail!("GitLab has no diff for this merge request yet");
    }
    if !theirs.starts_with(head) && !head.starts_with(&theirs) {
        bail!(
            "The branch here is not the version on GitLab ({}) — fetch first",
            &theirs[..7.min(theirs.len())]
        );
    }
    let mut form = vec![
        ("body", body.to_owned()),
        ("position[position_type]", "text".to_owned()),
        ("position[base_sha]", base),
        ("position[head_sha]", theirs),
        ("position[start_sha]", start),
        ("position[new_path]", place.path.to_owned()),
        ("position[old_path]", place.old_path.to_owned()),
    ];
    if let Some(n) = place.new_line {
        form.push(("position[new_line]", n.to_string()));
    }
    if let Some(n) = place.old_line {
        form.push(("position[old_line]", n.to_string()));
    }
    request(remote, &format!("merge_requests/{iid}/discussions"), &form)?;
    Ok(())
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn name(value: &Value) -> String {
    let who = value.get("author").unwrap_or(&Value::Null);
    let name = text(who, "name");
    if name.is_empty() {
        text(who, "username")
    } else {
        name
    }
}

pub fn parse_mrs(value: &Value) -> Vec<Mr> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(Mr {
                iid: m.get("iid")?.as_u64()?,
                title: text(m, "title"),
                author: name(m),
                source: text(m, "source_branch"),
                target: text(m, "target_branch"),
                url: text(m, "web_url"),
                updated: epoch(&text(m, "updated_at")),
                comments: m
                    .get("user_notes_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize,
                draft: m
                    .get("draft")
                    .or_else(|| m.get("work_in_progress"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

pub fn parse_threads(value: &Value) -> Vec<Thread> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let raw = d.get("notes")?.as_array()?;
            let notes: Vec<&Value> = raw
                .iter()
                .filter(|n| n.get("system").and_then(Value::as_bool) != Some(true))
                .collect();
            let first = notes.first()?;
            let position = first.get("position").filter(|p| !p.is_null());
            let line = |key: &str| {
                position
                    .and_then(|p| p.get(key))
                    .and_then(Value::as_u64)
                    .map(|n| n as u32)
            };
            let resolvable: Vec<bool> = notes
                .iter()
                .filter(|n| n.get("resolvable").and_then(Value::as_bool) == Some(true))
                .map(|n| n.get("resolved").and_then(Value::as_bool) == Some(true))
                .collect();
            Some(Thread {
                notes: notes
                    .iter()
                    .map(|n| Note {
                        author: name(n),
                        body: text(n, "body"),
                        created: epoch(&text(n, "created_at")),
                    })
                    .collect(),
                path: position
                    .map(|p| {
                        let new = text(p, "new_path");
                        if new.is_empty() {
                            text(p, "old_path")
                        } else {
                            new
                        }
                    })
                    .filter(|p| !p.is_empty()),
                new_line: line("new_line"),
                old_line: line("old_line"),
                resolved: !resolvable.is_empty() && resolvable.iter().all(|r| *r),
            })
        })
        .collect()
}

/// `2026-10-08T12:34:56.789Z` (or with an offset) as seconds since the epoch; 0 if unreadable.
pub fn epoch(stamp: &str) -> i64 {
    let (date, time) = stamp.split_once('T').unwrap_or((stamp, "00:00:00"));
    let mut d = date.split('-').filter_map(|p| p.parse::<i64>().ok());
    let (Some(y), Some(m), Some(day)) = (d.next(), d.next(), d.next()) else {
        return 0;
    };
    let clock: String = time
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ':')
        .collect();
    let mut t = clock.split(':').filter_map(|p| p.parse::<i64>().ok());
    let (h, mi, s) = (
        t.next().unwrap_or(0),
        t.next().unwrap_or(0),
        t.next().unwrap_or(0),
    );
    // Howard Hinnant's civil-to-days.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    // An offset such as +03:00 after the clock.
    let offset = time
        .rfind(['+', '-'])
        .filter(|i| *i > 0)
        .and_then(|i| {
            let sign = if time.as_bytes()[i] == b'+' { 1 } else { -1 };
            let mut p = time[i + 1..]
                .split(':')
                .filter_map(|x| x.parse::<i64>().ok());
            Some(sign * (p.next()? * 3600 + p.next().unwrap_or(0) * 60))
        })
        .unwrap_or(0);
    days * 86400 + h * 3600 + mi * 60 + s - offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn timestamps_become_epoch_seconds() {
        assert_eq!(epoch("1970-01-02T00:00:00Z"), 86400);
        assert_eq!(epoch("2026-10-08T00:00:00.123Z"), 1_791_417_600);
        assert_eq!(epoch("2026-10-08T03:00:00+03:00"), 1_791_417_600);
        assert_eq!(epoch("nonsense"), 0);
    }

    #[test]
    fn merge_requests_are_read_with_their_titles() {
        let value = json!([{
            "iid": 412, "title": "Payment retry backoff", "source_branch": "feat/backoff",
            "target_branch": "main", "web_url": "https://git.example/g/p/-/merge_requests/412",
            "updated_at": "2026-10-08T00:00:00Z", "user_notes_count": 3, "draft": true,
            "author": {"name": "Anna", "username": "anna"}
        }, {"title": "no iid"}]);
        let mrs = parse_mrs(&value);
        assert_eq!(mrs.len(), 1);
        assert_eq!(mrs[0].iid, 412);
        assert_eq!(mrs[0].author, "Anna");
        assert_eq!((mrs[0].comments, mrs[0].draft), (3, true));
        assert_eq!(mrs[0].source, "feat/backoff");
    }

    #[test]
    fn discussions_become_threads_without_system_notes() {
        let value = json!([
            {"notes": [{"system": true, "body": "added 1 commit"}]},
            {"notes": [
                {"system": false, "body": "Why a loop?", "author": {"name": "Boris"},
                 "created_at": "2026-10-08T00:00:00Z", "resolvable": true, "resolved": false,
                 "position": {"new_path": "src/a.rs", "old_path": "src/a.rs", "new_line": 12, "old_line": null}},
                {"system": false, "body": "Fixed", "author": {"name": "Anna"},
                 "created_at": "2026-10-08T01:00:00Z", "resolvable": true, "resolved": false}
            ]},
            {"notes": [{"system": false, "body": "LGTM", "author": {"name": "Carl"},
                 "resolvable": false, "position": null}]}
        ]);
        let threads = parse_threads(&value);
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].path.as_deref(), Some("src/a.rs"));
        assert_eq!(threads[0].new_line, Some(12));
        assert_eq!(threads[0].notes.len(), 2);
        assert!(!threads[0].resolved);
        assert_eq!(threads[1].path, None);
    }

    #[test]
    fn a_value_is_quoted_for_a_curl_config() {
        assert_eq!(
            quote("say \"hi\"\nback\\slash"),
            "say \\\"hi\\\"\\nback\\\\slash"
        );
    }

    #[test]
    fn a_comment_is_recorded_in_the_fixture() {
        let dir = std::env::temp_dir().join(format!("gitlance-mr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: only this test sets the variable.
        unsafe { std::env::set_var("GITLANCE_MR_FIXTURE", &dir) };
        let remote = WebRemote {
            base: "https://git.example.com/g/p".into(),
            github: false,
        };
        let place = Place {
            path: "a.rs",
            old_path: "a.rs",
            new_line: Some(3),
            old_line: None,
        };
        post_discussion(&remote, 412, "abc", &place, "Why?").unwrap();
        let posted = std::fs::read_to_string(dir.join("posted.txt")).unwrap();
        assert!(posted.contains("412 a.rs Some(3) None Why?"));
        unsafe { std::env::remove_var("GITLANCE_MR_FIXTURE") };
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_api_address_is_the_host_and_an_encoded_project() {
        let remote = WebRemote {
            base: "https://git.example.com/group/sub/project".into(),
            github: false,
        };
        assert_eq!(
            api(&remote).unwrap(),
            (
                "https://git.example.com".into(),
                "group%2Fsub%2Fproject".into()
            )
        );
    }
}
