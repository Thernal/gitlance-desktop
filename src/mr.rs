//! Merge requests of a GitLab project: the open ones with their titles, and the discussions on one.
//! Through `curl` and the token in `~/.config/gitlab-token` (so the VPN routes and proxies of the
//! machine apply, and no HTTP client is linked in). A GitHub remote's pull requests come in the same
//! shapes: every public function hands a GitHub remote to `crate::github`.

use crate::git::{DiffLine, LineKind, WebRemote};
use crate::github;
use anyhow::{Result, anyhow, bail};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

/// Where a merge request is in its life (#114): open, merged, or closed without merging.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MrState {
    #[default]
    Opened,
    Merged,
    Closed,
}

impl MrState {
    /// GitLab's word for it in a list's `state=`.
    fn query(self) -> &'static str {
        match self {
            MrState::Opened => "opened",
            MrState::Merged => "merged",
            MrState::Closed => "closed",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            MrState::Opened => "Open",
            MrState::Merged => "Merged",
            MrState::Closed => "Closed",
        }
    }
}

/// Merged and closed requests come a page of this many at a time, as the list is scrolled.
pub const PAST_PAGE: usize = 30;
/// Open requests, a request's discussions, are read a page of this many at a time, every page.
const OPEN_PAGE: usize = 100;
/// No more than this many pages of one listing.
const OPEN_PAGES: u32 = 10;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mr {
    pub iid: u64,
    pub title: String,
    pub author: String,
    pub source: String,
    pub target: String,
    pub url: String,
    /// Seconds since the epoch.
    pub updated: i64,
    pub created: i64,
    /// The source branch's head as GitLab knows it.
    pub head: String,
    pub comments: usize,
    pub draft: bool,
    /// The author's and the reviewers' usernames: whose request it is, who was asked to look.
    pub author_login: String,
    pub reviewers: Vec<String>,
    /// What the author wrote about it, as the list gives it: the root page's preview (#72).
    pub description: String,
    pub state: MrState,
    /// What it was merged as (the merge or squash commit): its first parent is the target as it
    /// was, so the diff is the request's, not what the target became since.
    pub merged_with: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// GitLab's id: a link's `#note_<id>` points at it.
    pub id: u64,
    pub author: String,
    /// The author's username, to tell one's own notes.
    pub login: String,
    pub body: String,
    pub created: i64,
}

/// A discussion on a merge request: on a line of a file, or on the request as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thread {
    /// GitLab's id of the discussion, for replying to it and resolving it.
    pub id: String,
    pub notes: Vec<Note>,
    pub path: Option<String>,
    /// 1-based line on the new side; `None` for a comment on a removed line or on no line.
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
    pub resolved: bool,
    /// A discussion on code can be resolved; a plain comment cannot.
    pub resolvable: bool,
    /// Who resolved it (name, username) and when, while it is resolved.
    pub resolved_by: Option<(String, String)>,
    pub resolved_at: i64,
    /// The commit (a version's head) the discussion was written on, when the host says (#46).
    pub commit: Option<String>,
    /// The host already knows its line is gone from the latest version (GitHub's `isOutdated`).
    pub outdated: bool,
}

/// The latest pipeline of a merge request, and when it failed, the job that did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pipeline {
    /// GitLab's word: success, failed, running, pending, canceled, skipped, manual …
    pub status: String,
    pub url: String,
    /// Seconds since the epoch, when it last changed.
    pub updated: i64,
    /// The jobs that failed, each with its page; the first is the one the pill names.
    pub failed: Vec<(String, String)>,
}

/// A comment of one's own that is not published yet: only its author sees it until the review is
/// submitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub id: u64,
    pub body: String,
    pub path: Option<String>,
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
}

/// A GitLab to-do on a merge request of this project: someone mentioned the user, asked for a
/// review, addressed them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Todo {
    pub id: u64,
    /// GitLab's word: mentioned, review_requested, directly_addressed, assigned, …
    pub action: String,
    pub iid: u64,
    pub title: String,
    pub author: String,
    /// The comment's text, when it came from one.
    pub body: String,
    /// The page it points at, often with `#note_<id>`.
    pub url: String,
    pub created: i64,
}

/// Who approved a merge request, how many approvals it needs, and where the reviewer stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Approvals {
    /// Names of those who approved.
    pub by: Vec<String>,
    /// Approvals the rules require (0 when there are no rules).
    pub required: u32,
    /// Approvals still missing.
    pub left: u32,
    /// The token's user approved it.
    pub mine: bool,
    /// The token's user may approve it (not the author on a project that forbids that, …).
    pub can: bool,
}

/// One push of the merge request's branch, as GitLab keeps it: the commit it ended on, the commit
/// the merge request was based on then, and when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Push {
    pub head: String,
    pub base: String,
    pub created: i64,
}

/// Where the token for a GitLab host comes from, best first: `GITLAB_TOKEN`, the token saved by this
/// app for that host, then the shared `~/.config/gitlab-token`.
pub fn token_source(remote: &WebRemote) -> Option<(String, &'static str)> {
    if remote.github {
        return github::token_source(remote);
    }
    let clean = |t: String| {
        let t = t.trim().to_owned();
        (!t.is_empty()).then_some(t)
    };
    if let Some(t) = std::env::var("GITLAB_TOKEN").ok().and_then(clean) {
        return Some((t, "the GITLAB_TOKEN variable"));
    }
    let home = std::env::home_dir()?;
    if let Ok((host, _)) = api(remote)
        && let Some(t) = std::fs::read_to_string(token_file(&home, &host))
            .ok()
            .and_then(clean)
    {
        return Some((t, "saved by this app"));
    }
    std::fs::read_to_string(home.join(".config/gitlab-token"))
        .ok()
        .and_then(clean)
        .map(|t| (t, "~/.config/gitlab-token"))
}

pub fn token(remote: &WebRemote) -> Option<String> {
    token_source(remote).map(|(t, _)| t)
}

/// The token this app saved for `host` (`https://host`), as written.
pub(crate) fn saved_token(host: &str) -> Option<String> {
    std::fs::read_to_string(token_file(&std::env::home_dir()?, host)).ok()
}

fn token_file(home: &std::path::Path, host: &str) -> std::path::PathBuf {
    let safe: String = host
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    home.join(".config/gitlance/tokens").join(safe)
}

/// Keeps `token` for the host of `remote`, readable by the owner only.
pub fn save_token(remote: &WebRemote, token: &str) -> Result<()> {
    let home = std::env::home_dir().ok_or_else(|| anyhow!("no home directory"))?;
    let (host, _) = api(remote)?;
    let file = token_file(&home, &host);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&file, format!("{}\n", token.trim()))?;
    crate::platform::owner_only(&file)?;
    Ok(())
}

/// Removes the token this app saved for the host of `remote` (a wrong one, after a failed test).
pub fn forget_token(remote: &WebRemote) {
    if let (Some(home), Ok((host, _))) = (std::env::home_dir(), api(remote)) {
        let _ = std::fs::remove_file(token_file(&home, &host));
    }
}

/// Where a token for this host is made, with the name and the scope filled in.
pub fn token_page(remote: &WebRemote) -> String {
    if remote.github {
        return github::token_page(remote);
    }
    let (host, _) = api(remote).unwrap_or_default();
    format!("{host}/-/user_settings/personal_access_tokens?name=GitLance&scopes=api")
}

/// The host the requests go to, without the scheme.
pub fn host(remote: &WebRemote) -> String {
    if remote.github {
        return github::host(remote);
    }
    api(remote)
        .map(|(h, _)| {
            h.split_once("://")
                .map_or(h.clone(), |(_, rest)| rest.to_owned())
        })
        .unwrap_or_default()
}

/// The name the token belongs to: proof that the host, the token and the network work.
pub fn me(remote: &WebRemote) -> Result<String> {
    if remote.github {
        return github::me(remote);
    }
    if fixture().is_some() {
        return Ok("Fixture".to_owned());
    }
    let user = call(remote, "/user", "GET", &[])?;
    Ok(format!(
        "{} (@{})",
        text(&user, "name"),
        text(&user, "username")
    ))
}

/// What the token can do and until when (GitLab: "api, read_repository · expires 2027-09-01");
/// `None` where the host does not say (GitHub).
pub fn token_info(remote: &WebRemote) -> Option<String> {
    if remote.github || fixture().is_some() {
        return None;
    }
    let me = call(remote, "/personal_access_tokens/self", "GET", &[]).ok()?;
    let scopes: Vec<&str> = me["scopes"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let mut out = scopes.join(", ");
    match me["expires_at"].as_str() {
        Some(day) => out.push_str(&format!(" · expires {day}")),
        None => out.push_str(" · never expires"),
    }
    Some(out)
}

/// Whether merge requests can be read for this remote at all.
pub fn available(remote: &WebRemote) -> bool {
    fixture().is_some() || token(remote).is_some()
}

/// Whether the checks' fixture stands in for GitLab.
pub fn fixture_on() -> bool {
    fixture().is_some()
}

/// `GITLANCE_MR_FIXTURE=<dir>` reads `mrs.json` and `discussions-<iid>.json` from a directory
/// instead of the network (for UI checks); a test sets its own (`TEST_FIXTURE`).
pub(crate) fn fixture() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    if let Some(dir) = TEST_FIXTURE.with(|f| f.borrow().clone()) {
        return Some(dir);
    }
    std::env::var_os("GITLANCE_MR_FIXTURE").map(Into::into)
}

#[cfg(test)]
thread_local! {
    /// A test's fixture, on its thread only: tests run side by side in one process.
    pub(crate) static TEST_FIXTURE: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Every open request: page after page, until one comes short (#114).
pub fn list(remote: &WebRemote) -> Result<Vec<Mr>> {
    if remote.github {
        return github::list(remote);
    }
    if let Some(dir) = fixture() {
        // `list-error.txt` stands for a host that answers with that error (#72's states).
        if dir.join("list-error.txt").exists() {
            bail!(
                "{}",
                std::fs::read_to_string(dir.join("list-error.txt"))?.trim()
            )
        }
        let value = serde_json::from_str(&std::fs::read_to_string(dir.join("mrs.json"))?)?;
        crate::avatars::harvest(&value);
        return Ok(parse_mrs(&value));
    }
    let value = get_all(remote, "merge_requests?state=opened&order_by=updated_at")?;
    crate::avatars::harvest(&value);
    Ok(parse_mrs(&value))
}

/// Every item of a listing `endpoint`: page after page of `OPEN_PAGE`, until one comes short or
/// `OPEN_PAGES` were read (#114).
fn get_all(remote: &WebRemote, endpoint: &str) -> Result<Value> {
    let join = if endpoint.contains('?') { '&' } else { '?' };
    let mut all = Vec::new();
    for page in 1..=OPEN_PAGES {
        let value = get(
            remote,
            &format!("{endpoint}{join}per_page={OPEN_PAGE}&page={page}"),
        )?;
        let items = match value {
            Value::Array(items) => items,
            other => return Ok(other),
        };
        let last = items.len() < OPEN_PAGE;
        all.extend(items);
        if last {
            break;
        }
    }
    Ok(Value::Array(all))
}

/// Page `page` (from 1) of the merged or the closed requests, newest first, and whether another
/// may follow (#114).
pub fn list_past(remote: &WebRemote, state: MrState, page: u32) -> Result<(Vec<Mr>, bool)> {
    if remote.github {
        return github::list_past(remote, state, page);
    }
    let value = match fixture() {
        // `mrs-merged.json`, `mrs-closed.json`: every one, cut into pages here.
        Some(dir) => {
            let file = dir.join(format!("mrs-{}.json", state.query()));
            let all: Vec<Value> = std::fs::read_to_string(file)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
            let from = (page as usize - 1) * PAST_PAGE;
            Value::Array(all.into_iter().skip(from).take(PAST_PAGE).collect())
        }
        None => get(
            remote,
            &format!(
                "merge_requests?state={}&order_by=updated_at&per_page={PAST_PAGE}&page={page}",
                state.query()
            ),
        )?,
    };
    crate::avatars::harvest(&value);
    let mrs = parse_mrs(&value);
    let more = mrs.len() == PAST_PAGE;
    Ok((mrs, more))
}

/// One request, whatever its state: one that left the open list, to say what became of it.
pub fn one(remote: &WebRemote, iid: u64) -> Result<Mr> {
    if remote.github {
        return github::one(remote, iid);
    }
    let value = match fixture() {
        // The merged and the closed of the fixture.
        Some(dir) => [MrState::Merged, MrState::Closed]
            .iter()
            .filter_map(|s| {
                std::fs::read_to_string(dir.join(format!("mrs-{}.json", s.query()))).ok()
            })
            .filter_map(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
            .flatten()
            .find(|m| m["iid"].as_u64() == Some(iid))
            .ok_or_else(|| anyhow!("no request {iid} in the fixture"))?,
        None => get(remote, &format!("merge_requests/{iid}"))?,
    };
    parse_mrs(&Value::Array(vec![value]))
        .pop()
        .ok_or_else(|| anyhow!("no request {iid}"))
}

pub fn threads(remote: &WebRemote, iid: u64) -> Result<Vec<Thread>> {
    if remote.github {
        return github::threads(remote, iid);
    }
    let value = match fixture() {
        Some(dir) => {
            let file = dir.join(format!("discussions-{iid}.json"));
            match std::fs::read_to_string(file) {
                Ok(text) => serde_json::from_str(&text)?,
                Err(_) => Value::Array(Vec::new()),
            }
        }
        None => get_all(remote, &format!("merge_requests/{iid}/discussions"))?,
    };
    Ok(parse_threads(&value))
}

/// One reaction on a note (#9): its id (to take one's own back), its GitLab name, who gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Award {
    pub id: u64,
    pub name: String,
    pub user: String,
}

/// The reactions on `notes` of request `iid`, by note id; a note is `(id, on the conversation)`
/// (GitHub keeps the request's own comments apart from review comments).
pub fn awards(
    remote: &WebRemote,
    iid: u64,
    notes: &[(u64, bool)],
) -> Result<std::collections::HashMap<u64, Vec<Award>>> {
    if remote.github {
        return github::awards(remote, iid, notes);
    }
    let parse = |list: &Value| -> Vec<Award> {
        list.as_array()
            .into_iter()
            .flatten()
            .map(|a| Award {
                id: a.get("id").and_then(Value::as_u64).unwrap_or(0),
                name: text(a, "name"),
                user: text(a.get("user").unwrap_or(&Value::Null), "username"),
            })
            .collect()
    };
    if let Some(dir) = fixture() {
        let map: Value = std::fs::read_to_string(dir.join(format!("awards-{iid}.json")))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        return Ok(notes
            .iter()
            .filter_map(|(id, _)| Some((*id, parse(map.get(id.to_string())?))))
            .collect());
    }
    Ok(notes
        .iter()
        .filter_map(|(id, _)| {
            let list = get(
                remote,
                &format!("merge_requests/{iid}/notes/{id}/award_emoji"),
            )
            .ok()?;
            Some((*id, parse(&list)))
        })
        .collect())
}

/// Gives note `note` the reaction `name`; its id comes back.
pub fn award(remote: &WebRemote, iid: u64, note: u64, issue: bool, name: &str) -> Result<u64> {
    if record(format!("award {iid} {note} {name}"))? {
        return Ok(note * 1000 + 1);
    }
    if remote.github {
        return github::react(remote, note, issue, name);
    }
    let value = request(
        remote,
        &format!("merge_requests/{iid}/notes/{note}/award_emoji"),
        &[("name", name.to_owned())],
    )?;
    Ok(value.get("id").and_then(Value::as_u64).unwrap_or(0))
}

/// Takes one's reaction `award` (named `name`) back from note `note`.
pub fn unaward(
    remote: &WebRemote,
    iid: u64,
    note: u64,
    issue: bool,
    name: &str,
    award: u64,
) -> Result<()> {
    if record(format!("unaward {iid} {note} {name}"))? {
        return Ok(());
    }
    if remote.github {
        return github::unreact(remote, note, issue, award);
    }
    call(
        remote,
        &format!("merge_requests/{iid}/notes/{note}/award_emoji/{award}"),
        "DELETE",
        &[],
    )?;
    Ok(())
}

/// The project's members (#51): username and name, for `@` completion.
pub fn members(remote: &WebRemote) -> Result<Vec<(String, String)>> {
    if remote.github {
        return github::members(remote);
    }
    let value = match fixture() {
        Some(dir) => match std::fs::read_to_string(dir.join("members.json")) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => Value::Array(Vec::new()),
        },
        None => get(remote, "members/all?per_page=100")?,
    };
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| (text(m, "username"), text(m, "name")))
        .filter(|(u, _)| !u.is_empty())
        .collect())
}

/// A label of a request, with its colour as 0xRRGGBB when the project gives one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    pub color: Option<u32>,
}

/// What a request's Overview says beyond its list row (#8, #73).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    pub description: String,
    pub labels: Vec<Label>,
    /// The milestone's title and its due date (`YYYY-MM-DD`), if any.
    pub milestone: Option<(String, Option<String>)>,
    pub assignees: Vec<String>,
    /// The reviewers by name (GitHub: those still asked; who approved is in the approvals).
    pub reviewers: Vec<String>,
    /// The reviewers, by name, whose last word was to request changes.
    pub changes_requested: Vec<String>,
}

/// A milestone a new request can be put in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Milestone {
    /// GitLab's id, GitHub's number: what creating a request takes.
    pub id: u64,
    pub title: String,
    pub due: Option<String>,
}

/// `#rrggbb` (or `rrggbb`) as 0xRRGGBB.
pub fn hex_color(text: &str) -> Option<u32> {
    let hex = text.trim().trim_start_matches('#');
    (hex.len() == 6)
        .then(|| u32::from_str_radix(hex, 16).ok())
        .flatten()
}

/// Request `iid`'s description, labels in their colours, milestone and assignees.
pub fn details(remote: &WebRemote, iid: u64) -> Result<Details> {
    if remote.github {
        return github::details(remote, iid);
    }
    let value = match fixture() {
        Some(dir) => match std::fs::read_to_string(dir.join(format!("mr-{iid}.json"))) {
            Ok(text) => serde_json::from_str(&text)?,
            // Without its own file, the request's row in the list, the open or the merged and
            // closed.
            Err(_) => ["mrs.json", "mrs-merged.json", "mrs-closed.json"]
                .iter()
                .filter_map(|f| std::fs::read_to_string(dir.join(f)).ok())
                .filter_map(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
                .flatten()
                .find(|m| m.get("iid").and_then(Value::as_u64) == Some(iid))
                .unwrap_or(Value::Null),
        },
        None => get(remote, &format!("merge_requests/{iid}"))?,
    };
    let colors = match fixture() {
        Some(dir) => std::fs::read_to_string(dir.join("labels.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null),
        None => get(remote, "labels?per_page=100").unwrap_or(Value::Null),
    };
    let mut details = parse_details(&value, &colors);
    // Which reviewers asked for changes: their states come from a list of their own.
    let states = match fixture() {
        Some(dir) => std::fs::read_to_string(dir.join(format!("reviewers-{iid}.json")))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null),
        None => get(remote, &format!("merge_requests/{iid}/reviewers")).unwrap_or(Value::Null),
    };
    details.changes_requested = changes_requested(&states);
    Ok(details)
}

/// The names in GitLab's reviewers list whose state is `requested_changes`.
pub fn changes_requested(states: &Value) -> Vec<String> {
    states
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| text(r, "state") == "requested_changes")
        .map(|r| match text(&r["user"], "name") {
            n if n.is_empty() => text(&r["user"], "username"),
            n => n,
        })
        .collect()
}

/// The project's labels in their colours, for a new request.
pub fn labels(remote: &WebRemote) -> Result<Vec<Label>> {
    if remote.github {
        return github::labels(remote);
    }
    let value = match fixture() {
        Some(dir) => match std::fs::read_to_string(dir.join("labels.json")) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => Value::Null,
        },
        None => get(remote, "labels?per_page=100")?,
    };
    Ok(parse_labels(&value))
}

pub fn parse_labels(value: &Value) -> Vec<Label> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| Label {
            name: text(l, "name"),
            color: hex_color(&text(l, "color")),
        })
        .filter(|l| !l.name.is_empty())
        .collect()
}

/// The project's open milestones, for a new request.
pub fn milestones(remote: &WebRemote) -> Result<Vec<Milestone>> {
    if remote.github {
        return github::milestones(remote);
    }
    let value = match fixture() {
        Some(dir) => match std::fs::read_to_string(dir.join("milestones.json")) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => Value::Null,
        },
        None => get(remote, "milestones?state=active&per_page=100")?,
    };
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let due = text(m, "due_date");
            Some(Milestone {
                id: m.get("id")?.as_u64()?,
                title: text(m, "title"),
                due: (!due.is_empty()).then_some(due),
            })
        })
        .collect())
}

/// The members a mention does not reach: in a private project, those below Reporter cannot read
/// its merge requests, so GitLab sends them nothing. GitHub: none (only collaborators are listed).
pub fn unreached(remote: &WebRemote) -> Result<Vec<String>> {
    if remote.github {
        return Ok(Vec::new());
    }
    let read = |file: &str| -> Result<Value> {
        Ok(match fixture() {
            Some(dir) => match std::fs::read_to_string(dir.join(file)) {
                Ok(text) => serde_json::from_str(&text)?,
                Err(_) => Value::Null,
            },
            None => match file {
                "project.json" => {
                    let (_, project) = api(remote)?;
                    get(remote, &format!("/projects/{project}"))?
                }
                _ => get(remote, "members/all?per_page=100")?,
            },
        })
    };
    if text(&read("project.json")?, "visibility") != "private" {
        return Ok(Vec::new());
    }
    Ok(below_reporter(&read("members.json")?))
}

/// The usernames in a members list whose access level is under Reporter (20).
pub fn below_reporter(members: &Value) -> Vec<String> {
    members
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| {
            m.get("access_level")
                .and_then(Value::as_u64)
                .is_some_and(|l| l < 20)
        })
        .map(|m| text(m, "username"))
        .filter(|u| !u.is_empty())
        .collect()
}

/// The description template set in the project's settings (GitLab's `merge_requests_template`);
/// GitHub has none apart from the files.
pub fn project_template(remote: &WebRemote) -> Result<Option<String>> {
    if remote.github {
        return Ok(None);
    }
    let value = match fixture() {
        Some(dir) => match std::fs::read_to_string(dir.join("project.json")) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => Value::Null,
        },
        None => {
            let (_, project) = api(remote)?;
            get(remote, &format!("/projects/{project}"))?
        }
    };
    let template = text(&value, "merge_requests_template");
    Ok((!template.trim().is_empty()).then_some(template))
}

pub fn parse_details(m: &Value, labels: &Value) -> Details {
    let color_of = |name: &str| {
        labels
            .as_array()
            .into_iter()
            .flatten()
            .find(|l| l.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|l| hex_color(&text(l, "color")))
    };
    Details {
        description: text(m, "description"),
        labels: m
            .get("labels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|name| Label {
                name: name.to_owned(),
                color: color_of(name),
            })
            .collect(),
        milestone: m.get("milestone").filter(|v| v.is_object()).map(|ms| {
            let due = text(ms, "due_date");
            (text(ms, "title"), (!due.is_empty()).then_some(due))
        }),
        assignees: people(m, "assignees"),
        reviewers: people(m, "reviewers"),
        changes_requested: Vec::new(),
    }
}

/// The names (else usernames) in the list `key` of `m`.
fn people(m: &Value, key: &str) -> Vec<String> {
    m.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|a| match text(a, "name") {
            n if n.is_empty() => text(a, "username"),
            n => n,
        })
        .collect()
}

/// The merge request's newest pipeline; `None` when it has none.
pub fn pipeline(remote: &WebRemote, iid: u64) -> Result<Option<Pipeline>> {
    if remote.github {
        return github::pipeline(remote, iid);
    }
    let value = match fixture() {
        Some(dir) => match std::fs::read_to_string(dir.join(format!("pipeline-{iid}.json"))) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => return Ok(None),
        },
        None => get(
            remote,
            &format!("merge_requests/{iid}/pipelines?per_page=1"),
        )?,
    };
    let Some(mut pipeline) = parse_pipeline(&value) else {
        return Ok(None);
    };
    if pipeline.status == "failed" && fixture().is_none() {
        let id = value
            .as_array()
            .and_then(|a| a.first())
            .and_then(|p| p.get("id"))
            .and_then(Value::as_u64);
        if let Some(id) = id
            && let Ok(jobs) = get(remote, &format!("pipelines/{id}/jobs?scope[]=failed"))
        {
            let jobs = parse_failed_jobs(&jobs);
            if !jobs.is_empty() {
                pipeline.failed = jobs;
            }
        }
    }
    Ok(Some(pipeline))
}

/// The pipeline in an API answer (the newest of a list, or one object).
pub fn parse_pipeline(value: &Value) -> Option<Pipeline> {
    let p = match value {
        Value::Array(list) => list.first()?,
        other => other,
    };
    let status = text(p, "status");
    (!status.is_empty()).then(|| Pipeline {
        status,
        url: text(p, "web_url"),
        updated: epoch(&text(p, "updated_at")),
        // A fixture names them; GitLab's answer is read again for its jobs.
        failed: match (p.get("failed_jobs"), p.get("failed_job")) {
            (Some(jobs), _) => parse_failed_jobs(jobs),
            (None, Some(job)) => parse_failed_jobs(&Value::Array(vec![job.clone()])),
            (None, None) => Vec::new(),
        },
    })
}

fn parse_failed_jobs(jobs: &Value) -> Vec<(String, String)> {
    jobs.as_array()
        .into_iter()
        .flatten()
        .map(|j| (text(j, "name"), text(j, "web_url")))
        .filter(|(name, _)| !name.is_empty())
        .collect()
}

/// The versions (pushes) of a merge request, oldest first.
pub fn versions(remote: &WebRemote, iid: u64) -> Result<Vec<Push>> {
    if remote.github {
        return github::versions(remote, iid);
    }
    let value = match fixture() {
        Some(dir) => match std::fs::read_to_string(dir.join(format!("versions-{iid}.json"))) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => Value::Array(Vec::new()),
        },
        None => get(remote, &format!("merge_requests/{iid}/versions"))?,
    };
    Ok(parse_versions(&value))
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
    call(
        remote,
        endpoint,
        if form.is_empty() { "GET" } else { "POST" },
        form,
    )
}

/// One call with any method. An endpoint starting with `/` is under `/api/v4`; any other is under
/// the project.
fn call(
    remote: &WebRemote,
    endpoint: &str,
    method: &str,
    form: &[(&str, String)],
) -> Result<Value> {
    let (host, project) = api(remote)?;
    let url = match endpoint.strip_prefix('/') {
        Some(rest) => format!("{host}/api/v4/{rest}"),
        None => format!("{host}/api/v4/projects/{project}/{endpoint}"),
    };
    send(remote, &url, method, form, None)
}

/// A GraphQL query or mutation; its `errors` come back as an error.
fn graphql(remote: &WebRemote, query: &str, variables: Value) -> Result<Value> {
    let (host, _) = api(remote)?;
    let body = serde_json::json!({ "query": query, "variables": variables }).to_string();
    let value = send(
        remote,
        &format!("{host}/api/graphql"),
        "POST",
        &[],
        Some(&body),
    )?;
    if let Some(first) = value
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|e| e.first())
    {
        bail!("GitLab would not take that: {}", text(first, "message"));
    }
    Ok(value)
}

/// Sends a request to `url`: form fields, or a JSON body.
fn send(
    remote: &WebRemote,
    url: &str,
    method: &str,
    form: &[(&str, String)],
    json: Option<&str>,
) -> Result<Value> {
    let token = token(remote).ok_or_else(|| {
        anyhow!("no GitLab token — add one in Settings, or put it in ~/.config/gitlab-token")
    })?;
    let (host, _) = api(remote)?;
    let mut config = format!(
        "url = \"{}\"\nheader = \"PRIVATE-TOKEN: {}\"\n",
        quote(url),
        quote(&token)
    );
    if method != "GET" {
        config.push_str(&format!("request = \"{method}\"\n"));
    }
    for (name, value) in form {
        config.push_str(&format!("data-urlencode = \"{name}={}\"\n", quote(value)));
    }
    if let Some(json) = json {
        config.push_str("header = \"Content-Type: application/json\"\n");
        config.push_str(&format!("data-raw = \"{}\"\n", quote(json)));
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
        "200" | "201" => {
            let value: Value = serde_json::from_str(body)?;
            // The people in it, for their pictures.
            crate::avatars::harvest(&value);
            Ok(value)
        }
        "204" => Ok(Value::Null),
        "401" | "403" => bail!("GitLab refused the token"),
        "404" => bail!("GitLab does not know this project (or the token cannot see it)"),
        "400" | "422" => bail!("GitLab would not take that: {}", message(body)),
        "409" => bail!("GitLab refused it: {}", message(body)),
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
    /// A comment on several lines: the first and the last, each with both counters
    /// (`FileDiff::position`). The last is the line above.
    pub range: Option<(DiffLine, DiffLine)>,
}

impl Place<'_> {
    /// `<old>_<new>..<old>_<new>` of the range, for the fixture's record.
    fn range_text(&self) -> String {
        self.range
            .map(|(a, b)| {
                let n = |l: DiffLine| {
                    format!("{}_{}", l.old_line.unwrap_or(0), l.new_line.unwrap_or(0))
                };
                format!(" range {}..{}", n(a), n(b))
            })
            .unwrap_or_default()
    }
}

/// GitLab's `line_range` fields of a comment on several lines of `path`.
fn line_range(path: &str, (start, end): (DiffLine, DiffLine)) -> Vec<(String, String)> {
    let mut form = Vec::new();
    for (end_name, l) in [("start", start), ("end", end)] {
        let key = |k: &str| format!("position[line_range][{end_name}][{k}]");
        let (old, new) = (l.old_line.unwrap_or(0), l.new_line.unwrap_or(0));
        form.push((
            key("line_code"),
            format!("{}_{old}_{new}", crate::links::path_hash(path)),
        ));
        match l.kind {
            LineKind::Added => {
                form.push((key("type"), "new".to_owned()));
                form.push((key("new_line"), new.to_string()));
            }
            LineKind::Removed => {
                form.push((key("type"), "old".to_owned()));
                form.push((key("old_line"), old.to_string()));
            }
            LineKind::Context => {
                form.push((key("old_line"), old.to_string()));
                form.push((key("new_line"), new.to_string()));
            }
        }
    }
    form
}

/// Posts `body` as a new discussion on a line of merge request `iid`, whose head the reviewer sees
/// as `head`, or keeps it pending (`draft`): then the pending comment's id comes back (#150).
/// Refuses when GitLab's head is another commit: the line numbers would not mean the same thing
/// there.
pub fn post_discussion(
    remote: &WebRemote,
    iid: u64,
    head: &str,
    place: &Place<'_>,
    body: &str,
    draft: bool,
) -> Result<Option<u64>> {
    if remote.github {
        return github::post_discussion(remote, iid, head, place, body, draft);
    }
    if let Some(dir) = fixture() {
        // UI checks: record what would have been sent.
        let file = dir.join("posted.txt");
        let mut text = std::fs::read_to_string(&file).unwrap_or_default();
        text.push_str(&format!(
            "{iid} {}{} {:?} {:?}{} {body}\n",
            if draft { "draft " } else { "" },
            place.path,
            place.new_line,
            place.old_line,
            place.range_text()
        ));
        std::fs::write(file, text)?;
        return if draft {
            fixture_draft(&dir, iid, place, body).map(Some)
        } else {
            Ok(None)
        };
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
        (if draft { "note" } else { "body" }, body.to_owned()),
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
    let range = place
        .range
        .map(|r| line_range(place.path, r))
        .unwrap_or_default();
    form.extend(range.iter().map(|(k, v)| (k.as_str(), v.clone())));
    let endpoint = if draft { "draft_notes" } else { "discussions" };
    let made = request(remote, &format!("merge_requests/{iid}/{endpoint}"), &form)?;
    Ok(made.get("id").and_then(Value::as_u64).filter(|_| draft))
}

/// A pending comment kept in the fixture's `drafts-<iid>.json`, as GitLab would list it, so the app
/// shows what was written (#150); its id, after the ones there.
fn fixture_draft(dir: &std::path::Path, iid: u64, place: &Place<'_>, body: &str) -> Result<u64> {
    let file = dir.join(format!("drafts-{iid}.json"));
    let mut drafts: Vec<Value> = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let id = drafts
        .iter()
        .filter_map(|d| d.get("id").and_then(Value::as_u64))
        .max()
        .unwrap_or(9000)
        + 1;
    drafts.push(serde_json::json!({
        "id": id,
        "note": body,
        "position": {
            "new_path": place.path,
            "old_path": place.old_path,
            "new_line": place.new_line,
            "old_line": place.old_line,
        },
    }));
    std::fs::write(file, serde_json::to_string_pretty(&drafts)?)?;
    Ok(id)
}

/// A line of the audit trail that UI checks read back: what the app would have sent.
pub(crate) fn record(line: String) -> Result<bool> {
    let Some(dir) = fixture() else {
        return Ok(false);
    };
    let file = dir.join("posted.txt");
    let mut text = std::fs::read_to_string(&file).unwrap_or_default();
    text.push_str(&line);
    text.push('\n');
    std::fs::write(file, text)?;
    Ok(true)
}

/// What a new request asks for besides its branches, title and description.
#[derive(Clone, Debug, Default)]
pub struct NewOptions {
    pub draft: bool,
    pub remove_source: bool,
    pub squash: bool,
    /// Usernames to ask for a review.
    pub reviewers: Vec<String>,
    /// Label names, and the milestone's id (GitHub: number).
    pub labels: Vec<String>,
    pub milestone: Option<u64>,
}

/// Opens a merge request from `source` into `target`. The branch must already be on the remote.
pub fn create(
    remote: &WebRemote,
    source: &str,
    target: &str,
    title: &str,
    description: &str,
    options: &NewOptions,
) -> Result<Mr> {
    let NewOptions {
        draft,
        remove_source,
        squash,
        reviewers,
        labels,
        milestone,
    } = options;
    let (draft, remove_source, squash) = (*draft, *remove_source, *squash);
    if remote.github {
        return github::create(remote, source, target, title, description, options);
    }
    let title = if draft && !title.to_lowercase().starts_with("draft:") {
        format!("Draft: {title}")
    } else {
        title.to_owned()
    };
    if record(format!(
        "create {source} {target} {draft} {remove_source} {squash} r={} l={} m={} {title}",
        reviewers.join(","),
        labels.join(","),
        milestone.map(|m| m.to_string()).unwrap_or_default()
    ))? {
        return Ok(Mr {
            iid: 999,
            title,
            author: "You".to_owned(),
            source: source.to_owned(),
            target: target.to_owned(),
            url: String::new(),
            updated: 0,
            created: 0,
            head: String::new(),
            comments: 0,
            draft,
            author_login: String::new(),
            reviewers: Vec::new(),
            description: String::new(),
            ..Default::default()
        });
    }
    let value = request(
        remote,
        "merge_requests",
        &[
            ("source_branch", source.to_owned()),
            ("target_branch", target.to_owned()),
            ("title", title),
            ("description", description.to_owned()),
            ("remove_source_branch", remove_source.to_string()),
            ("squash", squash.to_string()),
            ("labels", labels.join(",")),
        ]
        .into_iter()
        .chain(milestone.map(|id| ("milestone_id", id.to_string())))
        .chain(
            user_ids(remote, reviewers)
                .into_iter()
                .map(|id| ("reviewer_ids[]", id)),
        )
        .collect::<Vec<_>>(),
    )?;
    parse_mrs(&Value::Array(vec![value]))
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("GitLab answered without a merge request"))
}

/// GitLab's ids of the users named, as the API wants reviewers; a name it does not know is left out.
fn user_ids(remote: &WebRemote, names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter_map(|name| {
            let users = get(remote, &format!("/users?username={name}")).ok()?;
            users.get(0)?.get("id")?.as_u64().map(|id| id.to_string())
        })
        .collect()
}

/// Marks a discussion resolved or open again.
pub fn resolve(remote: &WebRemote, iid: u64, discussion: &str, resolved: bool) -> Result<()> {
    if remote.github {
        return github::resolve(remote, iid, discussion, resolved);
    }
    if record(format!("resolve {iid} {discussion} {resolved}"))? {
        return Ok(());
    }
    call(
        remote,
        &format!("merge_requests/{iid}/discussions/{discussion}"),
        "PUT",
        &[("resolved", resolved.to_string())],
    )?;
    Ok(())
}

/// Adds a reply to a discussion.
pub fn reply(remote: &WebRemote, iid: u64, discussion: &str, body: &str) -> Result<()> {
    if remote.github {
        return github::reply(remote, iid, discussion, body);
    }
    if record(format!("reply {iid} {discussion} {body}"))? {
        return Ok(());
    }
    call(
        remote,
        &format!("merge_requests/{iid}/discussions/{discussion}/notes"),
        "POST",
        &[("body", body.to_owned())],
    )?;
    Ok(())
}

/// Changes the text of one's own note `note` in discussion `discussion` of request `iid`.
pub fn edit_note(
    remote: &WebRemote,
    iid: u64,
    discussion: &str,
    note: u64,
    body: &str,
) -> Result<()> {
    if record(format!("edit {iid} {note} {body}"))? {
        return Ok(());
    }
    if remote.github {
        return github::edit_note(remote, discussion, note, body);
    }
    call(
        remote,
        &format!("merge_requests/{iid}/discussions/{discussion}/notes/{note}"),
        "PUT",
        &[("body", body.to_owned())],
    )?;
    Ok(())
}

/// Deletes one's own note `note` of discussion `discussion` on request `iid` (#76).
pub fn delete_note(remote: &WebRemote, iid: u64, discussion: &str, note: u64) -> Result<()> {
    if record(format!("delete-note {iid} {note}"))? {
        return Ok(());
    }
    if remote.github {
        return github::delete_note(remote, discussion, note);
    }
    call(
        remote,
        &format!("merge_requests/{iid}/discussions/{discussion}/notes/{note}"),
        "DELETE",
        &[],
    )?;
    Ok(())
}

/// `group/project` of a remote.
fn project_path(remote: &WebRemote) -> Option<String> {
    remote
        .base
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/'))
        .map(|(_, path)| path.to_owned())
}

/// The token's username (`GITLANCE_MR_FIXTURE`: `user.json`, else "you").
pub fn username(remote: &WebRemote) -> Result<String> {
    if remote.github {
        return github::username(remote);
    }
    let user = match fixture() {
        Some(dir) => match std::fs::read_to_string(dir.join("user.json")) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => return Ok("you".to_owned()),
        },
        None => call(remote, "/user", "GET", &[])?,
    };
    Ok(text(&user, "username"))
}

/// The user's pending to-dos on merge requests of this project, newest first.
pub fn todos(remote: &WebRemote) -> Result<Vec<Todo>> {
    if remote.github {
        return github::todos(remote);
    }
    let value = match fixture() {
        Some(dir) => match std::fs::read_to_string(dir.join("todos.json")) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => Value::Array(Vec::new()),
        },
        None => call(
            remote,
            "/todos?state=pending&type=MergeRequest&per_page=100",
            "GET",
            &[],
        )?,
    };
    Ok(parse_todos(
        &value,
        &project_path(remote).unwrap_or_default(),
    ))
}

pub fn parse_todos(value: &Value, project: &str) -> Vec<Todo> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| {
            t.get("project")
                .is_some_and(|p| text(p, "path_with_namespace") == project)
        })
        .filter_map(|t| {
            let target = t.get("target")?;
            Some(Todo {
                id: t.get("id")?.as_u64()?,
                action: text(t, "action_name"),
                iid: target.get("iid")?.as_u64()?,
                title: text(target, "title"),
                author: name(t),
                body: text(t, "body"),
                url: text(t, "target_url"),
                created: epoch(&text(t, "created_at")),
            })
        })
        .collect()
}

/// Marks to-do `id` done on GitLab.
pub fn mark_todo_done(remote: &WebRemote, id: u64) -> Result<()> {
    if remote.github {
        return github::mark_todo_done(remote, id);
    }
    if record(format!("todo-done {id}"))? {
        return Ok(());
    }
    call(remote, &format!("/todos/{id}/mark_as_done"), "POST", &[])?;
    Ok(())
}

/// Who approved request `iid`, and whether the token's user did or may.
pub fn approvals(remote: &WebRemote, iid: u64) -> Result<Approvals> {
    if remote.github {
        return github::approvals(remote, iid);
    }
    let value = match fixture() {
        Some(dir) => match std::fs::read_to_string(dir.join(format!("approvals-{iid}.json"))) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => return Ok(Approvals::default()),
        },
        None => get(remote, &format!("merge_requests/{iid}/approvals"))?,
    };
    Ok(parse_approvals(&value))
}

pub fn parse_approvals(value: &Value) -> Approvals {
    let count = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0) as u32;
    let flag = |key: &str| value.get(key).and_then(Value::as_bool).unwrap_or(false);
    Approvals {
        by: value
            .get("approved_by")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|a| a.get("user"))
            .map(|user| match text(user, "name") {
                n if n.is_empty() => text(user, "username"),
                n => n,
            })
            .collect(),
        required: count("approvals_required"),
        left: count("approvals_left"),
        mine: flag("user_has_approved"),
        can: flag("user_can_approve"),
    }
}

/// Approves request `iid` as of `head`: GitLab refuses when its head moved on since.
pub fn approve(remote: &WebRemote, iid: u64, head: &str) -> Result<()> {
    if remote.github {
        return github::submit(remote, iid, head, "", Verdict::Approve);
    }
    if record(format!("approve {iid} {head}"))? {
        return Ok(());
    }
    call(
        remote,
        &format!("merge_requests/{iid}/approve"),
        "POST",
        &[("sha", head.to_owned())],
    )?;
    Ok(())
}

/// Takes one's approval of request `iid` back.
pub fn unapprove(remote: &WebRemote, iid: u64) -> Result<()> {
    if remote.github {
        bail!(
            "GitHub cannot take an approval back — a later review (comment or changes) replaces it"
        );
    }
    if record(format!("unapprove {iid}"))? {
        return Ok(());
    }
    call(
        remote,
        &format!("merge_requests/{iid}/unapprove"),
        "POST",
        &[],
    )?;
    Ok(())
}

/// A comment on request `iid` as a whole (a review's summary).
pub fn comment(remote: &WebRemote, iid: u64, body: &str) -> Result<()> {
    if remote.github {
        return github::comment(remote, iid, body);
    }
    if record(format!("note {iid} {body}"))? {
        return Ok(());
    }
    request(
        remote,
        &format!("merge_requests/{iid}/notes"),
        &[("body", body.to_owned())],
    )?;
    Ok(())
}

/// Marks the token's user as asking for changes on request `iid` (GitLab 17 and later; an older
/// one answers with an error that says so).
pub fn request_changes(remote: &WebRemote, iid: u64) -> Result<()> {
    if record(format!("request-changes {iid}"))? {
        return Ok(());
    }
    let user = call(remote, "/user", "GET", &[])?;
    let id = user
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("GitLab did not say who the token belongs to"))?;
    let project = project_path(remote).ok_or_else(|| anyhow!("no project in {}", remote.base))?;
    let answer = graphql(
        remote,
        "mutation($project: ID!, $iid: String!, $user: UserID!) { \
         mergeRequestUpdateReviewerState(input: { projectPath: $project, iid: $iid, \
         userId: $user, state: REQUESTED_CHANGES }) { errors } }",
        serde_json::json!({
            "project": project,
            "iid": iid.to_string(),
            "user": format!("gid://gitlab/User/{id}"),
        }),
    )?;
    let errors = &answer["data"]["mergeRequestUpdateReviewerState"]["errors"];
    if let Some(first) = errors.as_array().and_then(|e| e.first()) {
        bail!(
            "GitLab would not take that: {}",
            first.as_str().unwrap_or("error")
        );
    }
    Ok(())
}

/// A review's verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Comment,
    Approve,
    RequestChanges,
}

/// Submits a review of request `iid` (whose head the reviewer sees as `head`): one's pending
/// comments when there are, the summary, the verdict. GitLab takes them as three calls; GitHub as
/// one review. `approved` says the reviewer approved already, so approving again is skipped.
pub fn submit(
    remote: &WebRemote,
    iid: u64,
    head: &str,
    drafts: bool,
    summary: &str,
    verdict: Verdict,
    approved: bool,
) -> Result<()> {
    if remote.github {
        return github::submit(remote, iid, head, summary, verdict);
    }
    if drafts {
        publish_drafts(remote, iid)?;
    }
    if !summary.is_empty() {
        comment(remote, iid, summary)?;
    }
    match verdict {
        Verdict::Approve if !approved => approve(remote, iid, head),
        Verdict::RequestChanges => request_changes(remote, iid),
        _ => Ok(()),
    }
}

/// The author's own unpublished comments on a request.
pub fn drafts(remote: &WebRemote, iid: u64) -> Result<Vec<Draft>> {
    if remote.github {
        return github::drafts(remote, iid);
    }
    let value = match fixture() {
        Some(dir) => match std::fs::read_to_string(dir.join(format!("drafts-{iid}.json"))) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => Value::Array(Vec::new()),
        },
        None => get(remote, &format!("merge_requests/{iid}/draft_notes"))?,
    };
    Ok(parse_drafts(&value))
}

/// Publishes every draft of the request at once: the review is submitted.
pub fn publish_drafts(remote: &WebRemote, iid: u64) -> Result<()> {
    if record(format!("publish {iid}"))? {
        return Ok(());
    }
    call(
        remote,
        &format!("merge_requests/{iid}/draft_notes/bulk_publish"),
        "POST",
        &[],
    )?;
    Ok(())
}

/// Publishes draft `id` of the request alone: it becomes a discussion, the others stay pending
/// (#138). GitHub publishes a pending comment only with its review.
pub fn publish_draft(remote: &WebRemote, iid: u64, id: u64) -> Result<()> {
    if remote.github {
        bail!("GitHub sends a pending comment only with its review");
    }
    if record(format!("publish-draft {iid} {id}"))? {
        return Ok(());
    }
    call(
        remote,
        &format!("merge_requests/{iid}/draft_notes/{id}/publish"),
        "PUT",
        &[],
    )?;
    Ok(())
}

pub fn delete_draft(remote: &WebRemote, iid: u64, id: u64) -> Result<()> {
    if remote.github {
        return github::delete_draft(remote, iid, id);
    }
    if record(format!("delete-draft {iid} {id}"))? {
        return Ok(());
    }
    call(
        remote,
        &format!("merge_requests/{iid}/draft_notes/{id}"),
        "DELETE",
        &[],
    )?;
    Ok(())
}

pub fn parse_drafts(value: &Value) -> Vec<Draft> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let position = d.get("position").filter(|p| !p.is_null());
            let line = |key: &str| {
                position
                    .and_then(|p| p.get(key))
                    .and_then(Value::as_u64)
                    .map(|n| n as u32)
            };
            Some(Draft {
                id: d.get("id")?.as_u64()?,
                body: text(d, "note"),
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
            })
        })
        .collect()
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
                created: epoch(&text(m, "created_at")),
                head: text(m, "sha"),
                comments: m
                    .get("user_notes_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize,
                draft: m
                    .get("draft")
                    .or_else(|| m.get("work_in_progress"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                author_login: text(m.get("author").unwrap_or(&Value::Null), "username"),
                reviewers: m
                    .get("reviewers")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|r| text(r, "username"))
                    .filter(|u| !u.is_empty())
                    .collect(),
                description: text(m, "description"),
                state: match m.get("state").and_then(Value::as_str) {
                    Some("merged") => MrState::Merged,
                    Some("closed" | "locked") => MrState::Closed,
                    _ => MrState::Opened,
                },
                merged_with: ["merge_commit_sha", "squash_commit_sha"]
                    .iter()
                    .find_map(|k| m.get(*k).and_then(Value::as_str))
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            })
        })
        .collect()
}

/// GitLab lists versions newest first; here they come oldest first.
pub fn parse_versions(value: &Value) -> Vec<Push> {
    let mut out: Vec<Push> = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let head = text(v, "head_commit_sha");
            (!head.is_empty()).then(|| Push {
                head,
                base: text(v, "base_commit_sha"),
                created: epoch(&text(v, "created_at")),
            })
        })
        .collect();
    out.reverse();
    out
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
                id: text(d, "id"),
                resolvable: !resolvable.is_empty(),
                notes: notes
                    .iter()
                    .map(|n| Note {
                        id: n.get("id").and_then(Value::as_u64).unwrap_or(0),
                        author: name(n),
                        login: text(n.get("author").unwrap_or(&Value::Null), "username"),
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
                // The last note resolved says who closed the discussion.
                resolved_by: notes
                    .iter()
                    .rev()
                    .find_map(|n| n.get("resolved_by").filter(|b| !b.is_null()))
                    .map(|b| (text(b, "name"), text(b, "username"))),
                resolved_at: notes
                    .iter()
                    .map(|n| epoch(&text(n, "resolved_at")))
                    .max()
                    .unwrap_or(0),
                commit: position
                    .map(|p| text(p, "head_sha"))
                    .filter(|s| !s.is_empty()),
                outdated: false,
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
    fn only_reviewers_who_requested_changes_are_named() {
        let states = json!([
            { "user": { "username": "ember", "name": "Ember Lind" }, "state": "requested_changes" },
            { "user": { "username": "kestrel", "name": "" }, "state": "requested_changes" },
            { "user": { "username": "juniper", "name": "Juniper Hale" }, "state": "approved" },
            { "user": { "username": "wren", "name": "Wren" }, "state": "unreviewed" }
        ]);
        assert_eq!(changes_requested(&states), ["Ember Lind", "kestrel"]);
        assert!(changes_requested(&Value::Null).is_empty());
    }

    #[test]
    fn a_request_says_its_state_and_what_it_was_merged_as() {
        let mrs = parse_mrs(&json!([
            { "iid": 1, "state": "opened", "merge_commit_sha": null },
            { "iid": 2, "state": "merged", "merge_commit_sha": "abc", "squash_commit_sha": "def" },
            { "iid": 3, "state": "merged", "merge_commit_sha": null, "squash_commit_sha": "def" },
            { "iid": 4, "state": "closed" },
            { "iid": 5, "state": "locked" }
        ]));
        let got: Vec<_> = mrs
            .iter()
            .map(|m| (m.iid, m.state, m.merged_with.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                (1, MrState::Opened, None),
                (2, MrState::Merged, Some("abc")),
                (3, MrState::Merged, Some("def")),
                (4, MrState::Closed, None),
                (5, MrState::Closed, None),
            ]
        );
    }

    #[test]
    fn only_guests_and_below_go_unreached() {
        let members = json!([
            { "username": "wren", "access_level": 10 },
            { "username": "ember", "access_level": 30 },
            { "username": "kestrel" }
        ]);
        assert_eq!(below_reporter(&members), ["wren"]);
    }

    #[test]
    fn labels_keep_their_colours_and_skip_the_nameless() {
        let labels = parse_labels(&json!([
            { "name": "auth", "color": "#428BCA" },
            { "name": "", "color": "#000000" },
            { "name": "refactor", "color": "" }
        ]));
        assert_eq!(
            labels,
            [
                Label {
                    name: "auth".to_owned(),
                    color: Some(0x428bca)
                },
                Label {
                    name: "refactor".to_owned(),
                    color: None
                }
            ]
        );
    }

    #[test]
    fn a_pipeline_is_read_from_a_list_or_an_object() {
        let list = json!([{"id": 9, "status": "failed", "web_url": "https://g/p/9", "updated_at": "2026-10-08T00:00:00Z"}, {"id": 8, "status": "success"}]);
        let p = parse_pipeline(&list).expect("a pipeline");
        assert_eq!(
            (p.status.as_str(), p.url.as_str()),
            ("failed", "https://g/p/9")
        );
        let one = json!({"status": "failed", "web_url": "u", "failed_job": {"name": "test:unit", "web_url": "j"}});
        assert_eq!(
            parse_pipeline(&one).map(|p| p.failed),
            Some(vec![("test:unit".to_owned(), "j".to_owned())])
        );
        let two = json!({"status": "failed", "failed_jobs": [{"name": "lint", "web_url": "a"}, {"name": "test", "web_url": "b"}]});
        assert_eq!(parse_pipeline(&two).map(|p| p.failed.len()), Some(2));
        assert!(parse_pipeline(&json!([])).is_none());
    }

    #[test]
    fn drafts_are_read_with_their_lines() {
        let value = json!([
            {"id": 7, "note": "Rename?", "position": {"new_path": "a.rs", "old_path": "a.rs", "new_line": 3, "old_line": null}},
            {"id": 8, "note": "General"},
            {"note": "no id"}
        ]);
        let drafts = parse_drafts(&value);
        assert_eq!(drafts.len(), 2);
        assert_eq!((drafts[0].id, drafts[0].new_line), (7, Some(3)));
        assert_eq!(drafts[0].path.as_deref(), Some("a.rs"));
        assert_eq!(drafts[1].path, None);
    }

    #[test]
    fn a_token_file_name_is_one_safe_segment() {
        let home = std::path::Path::new("/h");
        assert_eq!(
            token_file(home, "git.example.com:8443"),
            std::path::Path::new("/h/.config/gitlance/tokens/git.example.com_8443")
        );
        assert_eq!(
            token_file(home, "../x"),
            std::path::Path::new("/h/.config/gitlance/tokens/.._x")
        );
    }

    #[test]
    fn versions_come_oldest_first() {
        let value = json!([
            {"head_commit_sha": "bbb", "base_commit_sha": "000", "created_at": "2026-10-08T01:00:00Z"},
            {"head_commit_sha": "aaa", "base_commit_sha": "000", "created_at": "2026-10-08T00:00:00Z"},
            {"id": 3}
        ]);
        let pushes = parse_versions(&value);
        assert_eq!(
            pushes.iter().map(|p| p.head.as_str()).collect::<Vec<_>>(),
            ["aaa", "bbb"]
        );
    }

    #[test]
    fn details_carry_labels_in_their_colours_and_the_milestone() {
        let m = json!({
            "description": "Moves the state.",
            "labels": ["auth", "odd"],
            "milestone": { "title": "2.5", "due_date": "2026-10-31" },
            "assignees": [{ "username": "juniper", "name": "Juniper Hale" }],
            "reviewers": [{ "username": "ember", "name": "" }]
        });
        let labels = json!([{ "name": "auth", "color": "#61afef" }]);
        let d = parse_details(&m, &labels);
        assert_eq!(d.description, "Moves the state.");
        assert_eq!(d.labels[0].color, Some(0x61afef));
        assert_eq!(d.labels[1].color, None);
        assert_eq!(d.milestone, Some(("2.5".into(), Some("2026-10-31".into()))));
        assert_eq!(d.assignees, ["Juniper Hale"]);
        assert_eq!(d.reviewers, ["ember"]);
        assert_eq!(parse_details(&json!({}), &Value::Null), Details::default());
    }

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
    fn approvals_read_who_approved_and_where_the_reviewer_stands() {
        let value: Value = serde_json::from_str(
            r#"{"approvals_required": 2, "approvals_left": 1, "user_has_approved": false,
                "user_can_approve": true,
                "approved_by": [{"user": {"name": "Anna", "username": "anna"}}]}"#,
        )
        .unwrap();
        let a = parse_approvals(&value);
        assert_eq!(a.by, vec!["Anna".to_owned()]);
        assert_eq!((a.required, a.left, a.mine, a.can), (2, 1, false, true));
        assert_eq!(parse_approvals(&Value::Null), Approvals::default());
    }

    #[test]
    fn to_dos_are_kept_for_this_project_only() {
        let value: Value = serde_json::from_str(
            r#"[{"id": 1, "action_name": "mentioned", "body": "@you look",
                 "target_url": "https://g/x/app/-/merge_requests/7#note_9",
                 "created_at": "2026-10-13T10:00:00Z", "author": {"name": "Anna"},
                 "project": {"path_with_namespace": "x/app"},
                 "target": {"iid": 7, "title": "Clamp"}},
                {"id": 2, "action_name": "mentioned", "project": {"path_with_namespace": "x/other"},
                 "target": {"iid": 3, "title": "Elsewhere"}}]"#,
        )
        .unwrap();
        let todos = parse_todos(&value, "x/app");
        assert_eq!(todos.len(), 1);
        assert_eq!((todos[0].iid, todos[0].author.as_str()), (7, "Anna"));
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
        TEST_FIXTURE.with(|f| *f.borrow_mut() = Some(dir.clone()));
        let remote = WebRemote {
            base: "https://git.example.com/g/p".into(),
            github: false,
        };
        let place = Place {
            path: "a.rs",
            old_path: "a.rs",
            new_line: Some(3),
            old_line: None,
            range: None,
        };
        post_discussion(&remote, 412, "abc", &place, "Why?", false).unwrap();
        // One's own note deleted (#76).
        delete_note(&remote, 412, "d1", 7).unwrap();
        // One pending comment sent alone (#138); GitHub has no such call.
        publish_draft(&remote, 412, 9001).unwrap();
        let github = WebRemote {
            github: true,
            ..remote.clone()
        };
        assert!(publish_draft(&github, 412, 9001).is_err());
        let posted = std::fs::read_to_string(dir.join("posted.txt")).unwrap();
        assert!(posted.contains("412 a.rs Some(3) None Why?"));
        assert!(posted.contains("delete-note 412 7"));
        assert!(posted.contains("publish-draft 412 9001"));
        TEST_FIXTURE.with(|f| *f.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_range_names_its_first_and_last_lines_as_gitlab_does() {
        let l = |kind, old_line, new_line| DiffLine {
            kind,
            old_line,
            new_line,
        };
        let form = line_range(
            "a.rs",
            (
                l(LineKind::Context, Some(4), Some(4)),
                l(LineKind::Added, Some(7), Some(5)),
            ),
        );
        let get = |k: &str| {
            form.iter()
                .find(|(key, _)| key == &format!("position[line_range]{k}"))
                .map(|(_, v)| v.as_str())
        };
        let hash = crate::links::path_hash("a.rs");
        assert_eq!(
            get("[start][line_code]"),
            Some(format!("{hash}_4_4").as_str())
        );
        assert_eq!(get("[start][type]"), None);
        assert_eq!(get("[start][old_line]"), Some("4"));
        assert_eq!(
            get("[end][line_code]"),
            Some(format!("{hash}_7_5").as_str())
        );
        assert_eq!(get("[end][type]"), Some("new"));
        assert_eq!(get("[end][new_line]"), Some("5"));
        assert_eq!(get("[end][old_line]"), None);
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
