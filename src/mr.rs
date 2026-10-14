//! Merge requests of a GitLab project: the open ones with their titles, and the discussions on one.
//! Through `curl` and the token in `~/.config/gitlab-token` (so the VPN routes and proxies of the
//! machine apply, and no HTTP client is linked in). A GitHub remote's pull requests come in the same
//! shapes: every public function hands a GitHub remote to `crate::github`.

use crate::git::WebRemote;
use crate::github;
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
    pub created: i64,
    /// The source branch's head as GitLab knows it.
    pub head: String,
    pub comments: usize,
    pub draft: bool,
    /// The author's and the reviewers' usernames: whose request it is, who was asked to look.
    pub author_login: String,
    pub reviewers: Vec<String>,
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
}

/// The latest pipeline of a merge request, and when it failed, the job that did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pipeline {
    /// GitLab's word: success, failed, running, pending, canceled, skipped, manual …
    pub status: String,
    pub url: String,
    /// Seconds since the epoch, when it last changed.
    pub updated: i64,
    /// The first failed job: its name and its page.
    pub failed: Option<(String, String)>,
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

/// Whether merge requests can be read for this remote at all.
pub fn available(remote: &WebRemote) -> bool {
    fixture().is_some() || token(remote).is_some()
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
    if remote.github {
        return github::list(remote);
    }
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
        None => get(
            remote,
            &format!("merge_requests/{iid}/discussions?per_page=100"),
        )?,
    };
    Ok(parse_threads(&value))
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
    if pipeline.status == "failed" && pipeline.failed.is_none() && fixture().is_none() {
        let id = value
            .as_array()
            .and_then(|a| a.first())
            .and_then(|p| p.get("id"))
            .and_then(Value::as_u64);
        if let Some(id) = id
            && let Ok(jobs) = get(remote, &format!("pipelines/{id}/jobs?scope[]=failed"))
        {
            pipeline.failed = parse_failed_job(&jobs);
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
        failed: p.get("failed_job").and_then(|j| {
            let name = text(j, "name");
            (!name.is_empty()).then(|| (name, text(j, "web_url")))
        }),
    })
}

fn parse_failed_job(jobs: &Value) -> Option<(String, String)> {
    let job = jobs.as_array()?.first()?;
    Some((text(job, "name"), text(job, "web_url")))
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
        "200" | "201" => Ok(serde_json::from_str(body)?),
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
    draft: bool,
) -> Result<()> {
    if remote.github {
        return github::post_discussion(remote, iid, head, place, body, draft);
    }
    if let Some(dir) = fixture() {
        // UI checks: record what would have been sent.
        let file = dir.join("posted.txt");
        let mut text = std::fs::read_to_string(&file).unwrap_or_default();
        text.push_str(&format!(
            "{iid} {}{} {:?} {:?} {body}\n",
            if draft { "draft " } else { "" },
            place.path,
            place.new_line,
            place.old_line
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
    let endpoint = if draft { "draft_notes" } else { "discussions" };
    request(remote, &format!("merge_requests/{iid}/{endpoint}"), &form)?;
    Ok(())
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

/// Opens a merge request from `source` into `target`. The branch must already be on the remote.
pub fn create(
    remote: &WebRemote,
    source: &str,
    target: &str,
    title: &str,
    description: &str,
    draft: bool,
    remove_source: bool,
) -> Result<Mr> {
    if remote.github {
        return github::create(remote, source, target, title, description, draft);
    }
    let title = if draft && !title.to_lowercase().starts_with("draft:") {
        format!("Draft: {title}")
    } else {
        title.to_owned()
    };
    if record(format!(
        "create {source} {target} {draft} {remove_source} {title}"
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
        ],
    )?;
    parse_mrs(&Value::Array(vec![value]))
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("GitLab answered without a merge request"))
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
    fn a_pipeline_is_read_from_a_list_or_an_object() {
        let list = json!([{"id": 9, "status": "failed", "web_url": "https://g/p/9", "updated_at": "2026-10-08T00:00:00Z"}, {"id": 8, "status": "success"}]);
        let p = parse_pipeline(&list).expect("a pipeline");
        assert_eq!(
            (p.status.as_str(), p.url.as_str()),
            ("failed", "https://g/p/9")
        );
        let one = json!({"status": "failed", "web_url": "u", "failed_job": {"name": "test:unit", "web_url": "j"}});
        assert_eq!(
            parse_pipeline(&one).and_then(|p| p.failed),
            Some(("test:unit".to_owned(), "j".to_owned()))
        );
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
        post_discussion(&remote, 412, "abc", &place, "Why?", false).unwrap();
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
