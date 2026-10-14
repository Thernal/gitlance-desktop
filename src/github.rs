//! Pull requests of a GitHub repository (github.com or GitHub Enterprise), in the shapes `crate::mr`
//! gives the app for GitLab merge requests: `mr` hands every call on a GitHub remote to this
//! module. Through `curl` as there, with GitHub's REST API and its GraphQL API where REST has no
//! answer (review threads and their resolving, the pending review, force pushes).
//!
//! GitHub's model differs in three places, and the app says so where it shows: a reviewer's
//! unpublished comments are one pending review, submitted once with a verdict; an approval cannot
//! be taken back, only replaced by a later review; and only comments on lines resolve.

use crate::git::WebRemote;
use crate::mr::{Approvals, Draft, Mr, Note, Pipeline, Place, Push, Thread, Todo, Verdict};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

/// `https://api.github.com` (or `https://host/api/v3`), the GraphQL endpoint, and `owner/repo`.
fn api(remote: &WebRemote) -> Result<(String, String, String)> {
    let rest = remote
        .base
        .split_once("://")
        .map(|(_, r)| r)
        .ok_or_else(|| anyhow!("unsupported remote {}", remote.base))?;
    let (host, path) = rest
        .split_once('/')
        .ok_or_else(|| anyhow!("no repository in {}", remote.base))?;
    if host == "github.com" {
        Ok((
            "https://api.github.com".into(),
            "https://api.github.com/graphql".into(),
            path.to_owned(),
        ))
    } else {
        Ok((
            format!("https://{host}/api/v3"),
            format!("https://{host}/api/graphql"),
            path.to_owned(),
        ))
    }
}

/// The web host, without the scheme.
pub fn host(remote: &WebRemote) -> String {
    remote
        .base
        .split_once("://")
        .and_then(|(_, r)| r.split_once('/'))
        .map(|(h, _)| h.to_owned())
        .unwrap_or_default()
}

fn owner_name(remote: &WebRemote) -> Result<(String, String)> {
    let (_, _, path) = api(remote)?;
    let (owner, name) = path
        .split_once('/')
        .ok_or_else(|| anyhow!("no repository in {}", remote.base))?;
    Ok((owner.to_owned(), name.to_owned()))
}

/// Where the token comes from, best first: `GITHUB_TOKEN` / `GH_TOKEN`, the token saved by this app
/// for the host, then the GitHub CLI's own (`gh auth token`).
pub fn token_source(remote: &WebRemote) -> Option<(String, &'static str)> {
    let clean = |t: String| {
        let t = t.trim().to_owned();
        (!t.is_empty()).then_some(t)
    };
    for var in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Some(t) = std::env::var(var).ok().and_then(clean) {
            return Some((
                t,
                if var == "GITHUB_TOKEN" {
                    "the GITHUB_TOKEN variable"
                } else {
                    "the GH_TOKEN variable"
                },
            ));
        }
    }
    if let Some(t) = crate::mr::saved_token(&format!("https://{}", host(remote))).and_then(clean) {
        return Some((t, "saved by this app"));
    }
    let gh = gh_program()?;
    let out = Command::new(gh)
        .args(["auth", "token", "--hostname", &host(remote)])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        .and_then(clean)
        .map(|t| (t, "the GitHub CLI (gh)"))
}

/// The GitHub CLI: on the PATH, or where Homebrew puts it (an app opened from the Finder does not
/// get the shell's PATH).
fn gh_program() -> Option<std::path::PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("gh"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let known = ["/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"].map(Into::into);
    on_path.into_iter().chain(known).find(|p| p.is_file())
}

/// Where a token is made: a classic token with the `repo` scope reads and reviews pull requests
/// of every repository the account can see, private ones too.
pub fn token_page(remote: &WebRemote) -> String {
    format!(
        "https://{}/settings/tokens/new?scopes=repo,read:user&description=GitLance",
        host(remote)
    )
}

/// `GITLANCE_MR_FIXTURE=<dir>` reads GitHub's answers from `<dir>/github/`.
fn fixture() -> Option<std::path::PathBuf> {
    std::env::var_os("GITLANCE_MR_FIXTURE").map(|d| std::path::PathBuf::from(d).join("github"))
}

fn read_fixture(name: &str) -> Option<Result<Value>> {
    let dir = fixture()?;
    Some(match std::fs::read_to_string(dir.join(name)) {
        Ok(text) => serde_json::from_str(&text).map_err(Into::into),
        Err(_) => Ok(Value::Null),
    })
}

/// A value inside a curl config file's double quotes.
fn quote(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// One REST call: `endpoint` under the repository unless it starts with `/`.
fn rest(remote: &WebRemote, method: &str, endpoint: &str, body: Option<&Value>) -> Result<Value> {
    let (base, _, repo) = api(remote)?;
    let url = match endpoint.strip_prefix('/') {
        Some(rest) => format!("{base}/{rest}"),
        None => format!("{base}/repos/{repo}/{endpoint}"),
    };
    send(remote, &url, method, body)
}

fn get(remote: &WebRemote, endpoint: &str) -> Result<Value> {
    rest(remote, "GET", endpoint, None)
}

/// A GraphQL query or mutation on the repository (`$owner`, `$name` and `$number` are filled in);
/// its `errors` come back as an error.
fn graphql(remote: &WebRemote, query: &str, mut variables: Value) -> Result<Value> {
    let (_, endpoint, _) = api(remote)?;
    let (owner, name) = owner_name(remote)?;
    if let Some(vars) = variables.as_object_mut() {
        vars.entry("owner").or_insert(json!(owner));
        vars.entry("name").or_insert(json!(name));
    }
    let value = send(
        remote,
        &endpoint,
        "POST",
        Some(&json!({ "query": query, "variables": variables })),
    )?;
    if let Some(first) = value["errors"].as_array().and_then(|e| e.first()) {
        bail!(
            "GitHub would not take that: {}",
            first["message"].as_str().unwrap_or("error")
        );
    }
    Ok(value["data"].clone())
}

fn send(remote: &WebRemote, url: &str, method: &str, body: Option<&Value>) -> Result<Value> {
    let token = token_source(remote).map(|(t, _)| t).ok_or_else(|| {
        anyhow!(
            "no GitHub token — add one in Settings, or sign in with the GitHub CLI (gh auth login)"
        )
    })?;
    let mut config = format!(
        "url = \"{}\"\nheader = \"Authorization: Bearer {}\"\nheader = \"Accept: application/vnd.github+json\"\nheader = \"X-GitHub-Api-Version: 2022-11-28\"\nuser-agent = \"GitLance\"\n",
        quote(url),
        quote(&token)
    );
    if method != "GET" {
        config.push_str(&format!("request = \"{method}\"\n"));
    }
    if let Some(body) = body {
        config.push_str("header = \"Content-Type: application/json\"\n");
        config.push_str(&format!("data-raw = \"{}\"\n", quote(&body.to_string())));
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
        bail!("Can't reach {} — is the network on?", host(remote));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or((&text, ""));
    let message = || {
        serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                let main = v["message"].as_str()?.to_owned();
                let detail = v["errors"]
                    .as_array()
                    .and_then(|e| e.first())
                    .and_then(|e| e["message"].as_str().or(e.as_str()))
                    .map(|d| format!(": {d}"))
                    .unwrap_or_default();
                Some(format!("{main}{detail}"))
            })
            .unwrap_or_else(|| body.chars().take(160).collect())
    };
    match code.trim() {
        "200" | "201" => Ok(serde_json::from_str(body).unwrap_or(Value::Null)),
        "204" => Ok(Value::Null),
        "401" => bail!("GitHub refused the token"),
        "403" => bail!("GitHub refused it: {}", message()),
        "404" => bail!("GitHub does not know this repository (or the token cannot see it)"),
        "422" | "400" => bail!("GitHub would not take that: {}", message()),
        other => bail!("GitHub answered {other}"),
    }
}

/// Seconds since the epoch of GitHub's `2026-10-08T12:34:56Z`.
fn epoch(value: &Value) -> i64 {
    crate::mr::epoch(value.as_str().unwrap_or_default())
}

fn s(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// A user's name, else their login.
fn who(user: &Value) -> String {
    match user["name"].as_str().filter(|n| !n.is_empty()) {
        Some(name) => name.to_owned(),
        None => s(&user["login"]),
    }
}

pub fn me(remote: &WebRemote) -> Result<String> {
    if fixture().is_some() {
        return Ok("Fixture".to_owned());
    }
    let user = get(remote, "/user")?;
    Ok(format!("{} (@{})", who(&user), s(&user["login"])))
}

pub fn username(remote: &WebRemote) -> Result<String> {
    let user = match read_fixture("user.json") {
        Some(v) => v?,
        None => get(remote, "/user")?,
    };
    Ok(match s(&user["login"]) {
        l if l.is_empty() => "you".to_owned(),
        l => l,
    })
}

pub fn list(remote: &WebRemote) -> Result<Vec<Mr>> {
    let value = match read_fixture("pulls.json") {
        Some(v) => v?,
        None => get(
            remote,
            "pulls?state=open&sort=updated&direction=desc&per_page=100",
        )?,
    };
    Ok(parse_pulls(&value))
}

pub fn parse_pulls(value: &Value) -> Vec<Mr> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(parse_pull)
        .collect()
}

fn parse_pull(p: &Value) -> Option<Mr> {
    Some(Mr {
        iid: p["number"].as_u64()?,
        title: s(&p["title"]),
        author: who(&p["user"]),
        source: s(&p["head"]["ref"]),
        target: s(&p["base"]["ref"]),
        url: s(&p["html_url"]),
        updated: epoch(&p["updated_at"]),
        created: epoch(&p["created_at"]),
        head: s(&p["head"]["sha"]),
        comments: (p["comments"].as_u64().unwrap_or(0) + p["review_comments"].as_u64().unwrap_or(0))
            as usize,
        draft: p["draft"].as_bool().unwrap_or(false),
        author_login: s(&p["user"]["login"]),
        reviewers: p["requested_reviewers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| s(&r["login"]))
            .filter(|l| !l.is_empty())
            .collect(),
    })
}

const THREADS: &str = "query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) { pullRequest(number: $number) {
    reviewThreads(first: 100) { nodes {
      id isResolved path line originalLine diffSide
      resolvedBy { login ... on User { name } }
      comments(first: 100) { nodes {
        databaseId body createdAt
        author { login ... on User { name } }
        pullRequestReview { state }
      } }
    } }
  } }
}";

/// The review threads (published and pending comments apart) of pull request `number`.
fn review_threads(remote: &WebRemote, number: u64) -> Result<Value> {
    match read_fixture(&format!("threads-{number}.json")) {
        Some(v) => v,
        None => graphql(remote, THREADS, json!({ "number": number })),
    }
}

/// Lines and sides of a thread: GitHub's `line` (or, outdated, `originalLine`) on its side.
fn thread_lines(t: &Value) -> (Option<u32>, Option<u32>) {
    let line = t["line"]
        .as_u64()
        .or_else(|| t["originalLine"].as_u64())
        .map(|n| n as u32);
    if t["diffSide"] == "LEFT" {
        (None, line)
    } else {
        (line, None)
    }
}

fn pending(comment: &Value) -> bool {
    comment["pullRequestReview"]["state"] == "PENDING"
}

pub fn threads(remote: &WebRemote, number: u64) -> Result<Vec<Thread>> {
    let data = review_threads(remote, number)?;
    let issue = match read_fixture(&format!("comments-{number}.json")) {
        Some(v) => v?,
        None => get(remote, &format!("issues/{number}/comments?per_page=100"))?,
    };
    Ok(parse_threads(&data, &issue))
}

pub fn parse_threads(data: &Value, issue_comments: &Value) -> Vec<Thread> {
    let note = |c: &Value| Note {
        id: c["databaseId"].as_u64().unwrap_or(0),
        author: who(&c["author"]),
        login: s(&c["author"]["login"]),
        body: s(&c["body"]),
        created: epoch(&c["createdAt"]),
    };
    let mut out: Vec<Thread> = data["repository"]["pullRequest"]["reviewThreads"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let notes: Vec<Note> = t["comments"]["nodes"]
                .as_array()?
                .iter()
                .filter(|c| !pending(c))
                .map(note)
                .collect();
            let first = notes.first()?.id;
            let (new_line, old_line) = thread_lines(t);
            let resolved = t["isResolved"].as_bool().unwrap_or(false);
            Some(Thread {
                // The thread's node id resolves it; its first comment's id is what replies answer.
                id: format!("{}|{first}", s(&t["id"])),
                notes,
                path: t["path"].as_str().map(str::to_owned),
                new_line,
                old_line,
                resolved,
                resolvable: true,
                resolved_by: resolved
                    .then(|| &t["resolvedBy"])
                    .filter(|b| !b.is_null())
                    .map(|b| (who(b), s(&b["login"]))),
                resolved_at: 0,
            })
        })
        .collect();
    out.extend(
        issue_comments
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let id = c["id"].as_u64()?;
                Some(Thread {
                    id: format!("issue|{id}"),
                    notes: vec![Note {
                        id,
                        author: who(&c["user"]),
                        login: s(&c["user"]["login"]),
                        body: s(&c["body"]),
                        created: epoch(&c["created_at"]),
                    }],
                    path: None,
                    new_line: None,
                    old_line: None,
                    resolved: false,
                    resolvable: false,
                    resolved_by: None,
                    resolved_at: 0,
                })
            }),
    );
    out
}

/// One's own comments in the pending review, not published yet.
pub fn drafts(remote: &WebRemote, number: u64) -> Result<Vec<Draft>> {
    Ok(parse_drafts(&review_threads(remote, number)?))
}

pub fn parse_drafts(data: &Value) -> Vec<Draft> {
    data["repository"]["pullRequest"]["reviewThreads"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|t| {
            let (new_line, old_line) = thread_lines(t);
            t["comments"]["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| pending(c))
                .filter_map(move |c| {
                    Some(Draft {
                        id: c["databaseId"].as_u64()?,
                        body: s(&c["body"]),
                        path: t["path"].as_str().map(str::to_owned),
                        new_line,
                        old_line,
                    })
                })
        })
        .collect()
}

/// The checks of the pull request's head: failed when one failed (named, with its page), running
/// while one runs, passed when all passed; `None` when there are none.
pub fn pipeline(remote: &WebRemote, number: u64) -> Result<Option<Pipeline>> {
    let (runs, statuses, url) = match read_fixture(&format!("checks-{number}.json")) {
        Some(v) => {
            let v = v?;
            (v["check_runs"].clone(), v["statuses"].clone(), s(&v["url"]))
        }
        None => {
            let pull = get(remote, &format!("pulls/{number}"))?;
            let sha = s(&pull["head"]["sha"]);
            let runs = get(remote, &format!("commits/{sha}/check-runs?per_page=100"))?;
            let status = get(remote, &format!("commits/{sha}/status"))?;
            (
                runs["check_runs"].clone(),
                status["statuses"].clone(),
                format!("{}/checks", s(&pull["html_url"])),
            )
        }
    };
    Ok(parse_checks(&runs, &statuses, url))
}

pub fn parse_checks(runs: &Value, statuses: &Value, url: String) -> Option<Pipeline> {
    // (name, state, page, time): state is passed, failed, running or skipped.
    let mut all: Vec<(String, &str, String, i64)> = Vec::new();
    for r in runs.as_array().into_iter().flatten() {
        let state = match (r["status"].as_str(), r["conclusion"].as_str()) {
            (Some("completed"), Some("success" | "neutral")) => "passed",
            (Some("completed"), Some("skipped")) => "skipped",
            (Some("completed"), _) => "failed",
            _ => "running",
        };
        let time = epoch(&r["completed_at"]).max(epoch(&r["started_at"]));
        all.push((s(&r["name"]), state, s(&r["html_url"]), time));
    }
    for st in statuses.as_array().into_iter().flatten() {
        let state = match st["state"].as_str() {
            Some("success") => "passed",
            Some("pending") => "running",
            _ => "failed",
        };
        all.push((
            s(&st["context"]),
            state,
            s(&st["target_url"]),
            epoch(&st["updated_at"]),
        ));
    }
    if all.is_empty() {
        return None;
    }
    let failed = all
        .iter()
        .find(|c| c.1 == "failed")
        .map(|c| (c.0.clone(), c.2.clone()));
    let status = if failed.is_some() {
        "failed"
    } else if all.iter().any(|c| c.1 == "running") {
        "running"
    } else if all.iter().all(|c| c.1 == "skipped") {
        "skipped"
    } else {
        "success"
    };
    Some(Pipeline {
        status: status.to_owned(),
        url,
        updated: all.iter().map(|c| c.3).max().unwrap_or(0),
        failed,
    })
}

const VERSIONS: &str = "query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) { pullRequest(number: $number) {
    createdAt baseRefOid headRefOid
    timelineItems(first: 100, itemTypes: [HEAD_REF_FORCE_PUSHED_EVENT]) { nodes {
      ... on HeadRefForcePushedEvent { createdAt beforeCommit { oid } afterCommit { oid } }
    } }
  } }
}";

/// The versions of a pull request, oldest first: the head before each force push, then the head.
pub fn versions(remote: &WebRemote, number: u64) -> Result<Vec<Push>> {
    let data = match read_fixture(&format!("versions-{number}.json")) {
        Some(v) => v?,
        None => graphql(remote, VERSIONS, json!({ "number": number }))?,
    };
    Ok(parse_versions(&data))
}

pub fn parse_versions(data: &Value) -> Vec<Push> {
    let pr = &data["repository"]["pullRequest"];
    let base = s(&pr["baseRefOid"]);
    let head = s(&pr["headRefOid"]);
    if head.is_empty() {
        return Vec::new();
    }
    let mut since = epoch(&pr["createdAt"]);
    let mut out = Vec::new();
    for e in pr["timelineItems"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let before = s(&e["beforeCommit"]["oid"]);
        if !before.is_empty() {
            out.push(Push {
                head: before,
                base: base.clone(),
                created: since,
            });
        }
        since = epoch(&e["createdAt"]);
    }
    out.push(Push {
        head,
        base,
        created: since,
    });
    out
}

const APPROVALS: &str = "query($owner: String!, $name: String!, $number: Int!) {
  viewer { login }
  repository(owner: $owner, name: $name) { pullRequest(number: $number) {
    author { login }
    latestOpinionatedReviews(first: 50) { nodes { state author { login ... on User { name } } } }
  } }
}";

/// Who approved: each reviewer's latest opinion. GitHub's required count lives in branch rules
/// that only admins read, so it is 0 here.
pub fn approvals(remote: &WebRemote, number: u64) -> Result<Approvals> {
    let data = match read_fixture(&format!("approvals-{number}.json")) {
        Some(v) => v?,
        None => graphql(remote, APPROVALS, json!({ "number": number }))?,
    };
    Ok(parse_approvals(&data))
}

pub fn parse_approvals(data: &Value) -> Approvals {
    let viewer = s(&data["viewer"]["login"]);
    let pr = &data["repository"]["pullRequest"];
    let approved: Vec<&Value> = pr["latestOpinionatedReviews"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["state"] == "APPROVED")
        .collect();
    Approvals {
        by: approved.iter().map(|r| who(&r["author"])).collect(),
        required: 0,
        left: 0,
        mine: approved
            .iter()
            .any(|r| r["author"]["login"] == viewer.as_str()),
        can: !viewer.is_empty() && s(&pr["author"]["login"]) != viewer,
    }
}

const PENDING: &str = "query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) { pullRequest(number: $number) {
    id
    reviews(states: PENDING, first: 10) { nodes { id viewerDidAuthor } }
  } }
}";

/// The pull request's node id, and one's own pending review, if any.
fn pending_review(remote: &WebRemote, number: u64) -> Result<(String, Option<String>)> {
    let data = graphql(remote, PENDING, json!({ "number": number }))?;
    let pr = &data["repository"]["pullRequest"];
    let review = pr["reviews"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["viewerDidAuthor"] == true)
        .map(|r| s(&r["id"]));
    Ok((s(&pr["id"]), review))
}

/// The head GitHub has for the pull request, refusing when the reviewer sees another one: the
/// line numbers would not mean the same thing there.
fn check_head(remote: &WebRemote, number: u64, head: &str) -> Result<String> {
    let pull = get(remote, &format!("pulls/{number}"))?;
    let theirs = s(&pull["head"]["sha"]);
    if theirs.is_empty() {
        bail!("GitHub has no head for this pull request");
    }
    if !theirs.starts_with(head) && !head.starts_with(&theirs) {
        bail!(
            "The branch here is not the version on GitHub ({}) — fetch first",
            &theirs[..7.min(theirs.len())]
        );
    }
    Ok(theirs)
}

/// A comment on a line: published at once, or added to one's pending review (`draft`).
pub fn post_discussion(
    remote: &WebRemote,
    number: u64,
    head: &str,
    place: &Place<'_>,
    body: &str,
    draft: bool,
) -> Result<()> {
    let (line, side) = match (place.new_line, place.old_line) {
        (Some(n), _) => (n, "RIGHT"),
        (None, Some(o)) => (o, "LEFT"),
        (None, None) => bail!("a comment needs a line"),
    };
    if crate::mr::record(format!(
        "{number} {}{} {line} {side} {body}",
        if draft { "draft " } else { "" },
        place.path
    ))? {
        return Ok(());
    }
    let sha = check_head(remote, number, head)?;
    if !draft {
        rest(
            remote,
            "POST",
            &format!("pulls/{number}/comments"),
            Some(
                &json!({ "body": body, "commit_id": sha, "path": place.path, "line": line, "side": side }),
            ),
        )?;
        return Ok(());
    }
    let (pr, review) = pending_review(remote, number)?;
    let review = match review {
        Some(id) => id,
        None => {
            let data = graphql(
                remote,
                "mutation($pr: ID!, $sha: GitObjectID!) { addPullRequestReview(input: { pullRequestId: $pr, commitOID: $sha }) { pullRequestReview { id } } }",
                json!({ "pr": pr, "sha": sha }),
            )?;
            s(&data["addPullRequestReview"]["pullRequestReview"]["id"])
        }
    };
    graphql(
        remote,
        "mutation($review: ID!, $path: String!, $line: Int!, $side: DiffSide!, $body: String!) { addPullRequestReviewThread(input: { pullRequestReviewId: $review, path: $path, line: $line, side: $side, body: $body }) { thread { id } } }",
        json!({ "review": review, "path": place.path, "line": line, "side": side, "body": body }),
    )?;
    Ok(())
}

/// The thread's node id and its first comment's id (`parse_threads`), or an issue comment's.
fn thread_ids(thread: &str) -> (&str, &str) {
    thread.split_once('|').unwrap_or((thread, ""))
}

pub fn reply(remote: &WebRemote, number: u64, thread: &str, body: &str) -> Result<()> {
    if crate::mr::record(format!("reply {number} {thread} {body}"))? {
        return Ok(());
    }
    match thread_ids(thread) {
        ("issue", _) => comment(remote, number, body),
        (_, first) => {
            rest(
                remote,
                "POST",
                &format!("pulls/{number}/comments/{first}/replies"),
                Some(&json!({ "body": body })),
            )?;
            Ok(())
        }
    }
}

pub fn resolve(remote: &WebRemote, number: u64, thread: &str, resolved: bool) -> Result<()> {
    if crate::mr::record(format!("resolve {number} {thread} {resolved}"))? {
        return Ok(());
    }
    let (id, _) = thread_ids(thread);
    if id == "issue" {
        bail!("Only comments on lines resolve on GitHub");
    }
    let mutation = if resolved {
        "mutation($id: ID!) { resolveReviewThread(input: { threadId: $id }) { thread { id } } }"
    } else {
        "mutation($id: ID!) { unresolveReviewThread(input: { threadId: $id }) { thread { id } } }"
    };
    graphql(remote, mutation, json!({ "id": id }))?;
    Ok(())
}

/// A comment on the pull request as a whole.
pub fn comment(remote: &WebRemote, number: u64, body: &str) -> Result<()> {
    if crate::mr::record(format!("note {number} {body}"))? {
        return Ok(());
    }
    rest(
        remote,
        "POST",
        &format!("issues/{number}/comments"),
        Some(&json!({ "body": body })),
    )?;
    Ok(())
}

/// Submits a review: one's pending comments, if any, with the summary and the verdict, in one go.
pub fn submit(
    remote: &WebRemote,
    number: u64,
    head: &str,
    summary: &str,
    verdict: Verdict,
) -> Result<()> {
    let event = match verdict {
        Verdict::Comment => "COMMENT",
        Verdict::Approve => "APPROVE",
        Verdict::RequestChanges => "REQUEST_CHANGES",
    };
    // GitHub wants words with a request for changes, and with a bare comment.
    let body = match (summary.trim(), verdict) {
        ("", Verdict::RequestChanges) => "Changes requested.".to_owned(),
        (text, _) => text.to_owned(),
    };
    if crate::mr::record(format!("submit {number} {event} {body}"))? {
        return Ok(());
    }
    let (_, review) = pending_review(remote, number)?;
    match review {
        Some(id) => {
            graphql(
                remote,
                "mutation($id: ID!, $event: PullRequestReviewEvent!, $body: String) { submitPullRequestReview(input: { pullRequestReviewId: $id, event: $event, body: $body }) { pullRequestReview { id } } }",
                json!({ "id": id, "event": event, "body": body }),
            )?;
        }
        None => {
            let sha = check_head(remote, number, head)?;
            if verdict == Verdict::Comment && body.is_empty() {
                bail!("Nothing to submit");
            }
            rest(
                remote,
                "POST",
                &format!("pulls/{number}/reviews"),
                Some(&json!({ "commit_id": sha, "event": event, "body": body })),
            )?;
        }
    }
    Ok(())
}

pub fn delete_draft(remote: &WebRemote, number: u64, id: u64) -> Result<()> {
    if crate::mr::record(format!("delete-draft {number} {id}"))? {
        return Ok(());
    }
    rest(remote, "DELETE", &format!("pulls/comments/{id}"), None)?;
    Ok(())
}

/// Opens a pull request from `source` into `target`; the branch must already be on GitHub.
pub fn create(
    remote: &WebRemote,
    source: &str,
    target: &str,
    title: &str,
    description: &str,
    draft: bool,
) -> Result<Mr> {
    if crate::mr::record(format!("create {source} {target} {draft} {title}"))? {
        return Ok(Mr {
            iid: 999,
            title: title.to_owned(),
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
    let value = rest(
        remote,
        "POST",
        "pulls",
        Some(
            &json!({ "title": title, "head": source, "base": target, "body": description, "draft": draft }),
        ),
    )?;
    parse_pull(&value).ok_or_else(|| anyhow!("GitHub answered without a pull request"))
}

/// The user's unread notifications on pull requests of this repository, newest first.
pub fn todos(remote: &WebRemote) -> Result<Vec<Todo>> {
    let value = match read_fixture("notifications.json") {
        Some(v) => v?,
        None => get(remote, "notifications?participating=true&per_page=50")?,
    };
    Ok(parse_notifications(&value, &remote.base))
}

pub fn parse_notifications(value: &Value, base: &str) -> Vec<Todo> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["subject"]["type"] == "PullRequest")
        .filter_map(|n| {
            let api_url = s(&n["subject"]["url"]);
            let number: u64 = api_url.rsplit('/').next()?.parse().ok()?;
            Some(Todo {
                id: s(&n["id"]).parse().ok()?,
                // GitHub's reasons in GitLab's words, where they mean the same.
                action: match s(&n["reason"]).as_str() {
                    "mention" | "team_mention" => "mentioned".to_owned(),
                    "review_requested" => "review_requested".to_owned(),
                    other => other.to_owned(),
                },
                iid: number,
                title: s(&n["subject"]["title"]),
                author: String::new(),
                body: String::new(),
                url: format!("{base}/pull/{number}"),
                created: epoch(&n["updated_at"]),
            })
        })
        .collect()
}

/// Marks a notification read.
pub fn mark_todo_done(remote: &WebRemote, id: u64) -> Result<()> {
    if crate::mr::record(format!("todo-done {id}"))? {
        return Ok(());
    }
    rest(
        remote,
        "PATCH",
        &format!("/notifications/threads/{id}"),
        None,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(base: &str) -> WebRemote {
        WebRemote {
            base: base.into(),
            github: true,
        }
    }

    #[test]
    fn github_com_and_enterprise_have_their_api() {
        let (rest, graphql, repo) = api(&remote("https://github.com/Thernal/gitlance")).unwrap();
        assert_eq!(rest, "https://api.github.com");
        assert_eq!(graphql, "https://api.github.com/graphql");
        assert_eq!(repo, "Thernal/gitlance");
        let (rest, graphql, _) = api(&remote("https://git.corp.example/a/b")).unwrap();
        assert_eq!(rest, "https://git.corp.example/api/v3");
        assert_eq!(graphql, "https://git.corp.example/api/graphql");
    }

    #[test]
    fn pulls_read_as_requests() {
        let value = json!([{
            "number": 12, "title": "Add retries", "draft": true,
            "user": {"login": "ann"}, "html_url": "https://github.com/o/r/pull/12",
            "head": {"ref": "retry", "sha": "abc"}, "base": {"ref": "main"},
            "created_at": "2026-10-01T10:00:00Z", "updated_at": "2026-10-02T10:00:00Z",
            "requested_reviewers": [{"login": "bob"}]
        }]);
        let prs = parse_pulls(&value);
        assert_eq!(prs.len(), 1);
        let p = &prs[0];
        assert_eq!(
            (p.iid, p.source.as_str(), p.target.as_str()),
            (12, "retry", "main")
        );
        assert_eq!((p.head.as_str(), p.author_login.as_str()), ("abc", "ann"));
        assert!(p.draft);
        assert_eq!(p.reviewers, ["bob"]);
    }

    fn threads_data() -> Value {
        json!({"repository": {"pullRequest": {"reviewThreads": {"nodes": [
            {"id": "T1", "isResolved": true, "path": "src/a.rs", "line": 7, "diffSide": "RIGHT",
             "resolvedBy": {"login": "bob", "name": "Bob"},
             "comments": {"nodes": [
                {"databaseId": 101, "body": "Clamp it", "createdAt": "2026-10-01T10:00:00Z",
                 "author": {"login": "ann", "name": "Ann"}, "pullRequestReview": {"state": "COMMENTED"}},
                {"databaseId": 102, "body": "Done", "createdAt": "2026-10-01T11:00:00Z",
                 "author": {"login": "bob"}, "pullRequestReview": {"state": "COMMENTED"}}
             ]}},
            {"id": "T2", "isResolved": false, "path": "src/b.rs", "line": null, "originalLine": 3,
             "diffSide": "LEFT",
             "comments": {"nodes": [
                {"databaseId": 201, "body": "mine, unpublished", "createdAt": "2026-10-01T12:00:00Z",
                 "author": {"login": "me"}, "pullRequestReview": {"state": "PENDING"}}
             ]}}
        ]}}}})
    }

    #[test]
    fn threads_and_pending_comments_come_apart() {
        let issue = json!([{"id": 9, "body": "LGTM", "user": {"login": "carl"},
                            "created_at": "2026-10-01T09:00:00Z"}]);
        let threads = parse_threads(&threads_data(), &issue);
        assert_eq!(threads.len(), 2);
        let t = &threads[0];
        assert_eq!(t.id, "T1|101");
        assert_eq!((t.new_line, t.old_line), (Some(7), None));
        assert!(t.resolved && t.resolvable);
        assert_eq!(t.resolved_by, Some(("Bob".into(), "bob".into())));
        assert_eq!(t.notes.len(), 2);
        assert_eq!(threads[1].id, "issue|9");
        assert!(!threads[1].resolvable);
        let drafts = parse_drafts(&threads_data());
        assert_eq!(drafts.len(), 1);
        assert_eq!(
            (drafts[0].id, drafts[0].new_line, drafts[0].old_line),
            (201, None, Some(3))
        );
    }

    #[test]
    fn checks_add_up_to_one_state() {
        let runs = json!([
            {"name": "build", "status": "completed", "conclusion": "success", "html_url": "u1"},
            {"name": "test", "status": "completed", "conclusion": "failure", "html_url": "u2"}
        ]);
        let p = parse_checks(&runs, &json!([]), "page".into()).unwrap();
        assert_eq!(p.status, "failed");
        assert_eq!(p.failed, Some(("test".into(), "u2".into())));
        let running = json!([{"name": "build", "status": "in_progress"}]);
        assert_eq!(
            parse_checks(&running, &json!([]), String::new())
                .unwrap()
                .status,
            "running"
        );
        assert!(parse_checks(&json!([]), &json!([]), String::new()).is_none());
    }

    #[test]
    fn force_pushes_make_versions() {
        let data = json!({"repository": {"pullRequest": {
            "createdAt": "2026-10-01T10:00:00Z", "baseRefOid": "base", "headRefOid": "c3",
            "timelineItems": {"nodes": [
                {"createdAt": "2026-10-02T10:00:00Z", "beforeCommit": {"oid": "c1"}, "afterCommit": {"oid": "c2"}},
                {"createdAt": "2026-10-03T10:00:00Z", "beforeCommit": {"oid": "c2"}, "afterCommit": {"oid": "c3"}}
            ]}
        }}});
        let heads: Vec<String> = parse_versions(&data).into_iter().map(|p| p.head).collect();
        assert_eq!(heads, ["c1", "c2", "c3"]);
    }

    #[test]
    fn approvals_are_each_reviewers_latest_opinion() {
        let data = json!({"viewer": {"login": "me"}, "repository": {"pullRequest": {
            "author": {"login": "ann"},
            "latestOpinionatedReviews": {"nodes": [
                {"state": "APPROVED", "author": {"login": "me", "name": "Me"}},
                {"state": "CHANGES_REQUESTED", "author": {"login": "bob"}}
            ]}
        }}});
        let a = parse_approvals(&data);
        assert_eq!(a.by, ["Me"]);
        assert!(a.mine && a.can);
    }

    #[test]
    fn notifications_on_pulls_become_todos() {
        let value = json!([
            {"id": "77", "reason": "review_requested", "updated_at": "2026-10-01T10:00:00Z",
             "subject": {"type": "PullRequest", "title": "Add retries",
                         "url": "https://api.github.com/repos/o/r/pulls/12"}},
            {"id": "78", "reason": "mention",
             "subject": {"type": "Issue", "title": "x", "url": "https://api.github.com/repos/o/r/issues/3"}}
        ]);
        let todos = parse_notifications(&value, "https://github.com/o/r");
        assert_eq!(todos.len(), 1);
        assert_eq!((todos[0].id, todos[0].iid), (77, 12));
        assert_eq!(todos[0].action, "review_requested");
        assert_eq!(todos[0].url, "https://github.com/o/r/pull/12");
    }

    /// Reads a public repository's pull requests from the real GitHub: every query's shape.
    /// `GITHUB_TOKEN=… cargo test live_reads -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_reads() {
        let r = remote("https://github.com/zed-industries/zed");
        let prs = list(&r).unwrap();
        assert!(!prs.is_empty());
        println!(
            "{} open pull requests; first #{} {}",
            prs.len(),
            prs[0].iid,
            prs[0].title
        );
        // Pull requests with fresh comments on lines: the newest line comments say which.
        let recent = get(&r, "pulls/comments?sort=created&direction=desc&per_page=30").unwrap();
        let mut numbers: Vec<u64> = Vec::new();
        for c in recent.as_array().unwrap() {
            let n = s(&c["pull_request_url"])
                .rsplit('/')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            if !numbers.contains(&n) {
                numbers.push(n);
            }
        }
        let mut seen_thread = false;
        for p in prs.iter().filter(|p| numbers.contains(&p.iid)).take(4) {
            let threads = threads(&r, p.iid).unwrap();
            let lines = threads.iter().filter(|t| t.path.is_some()).count();
            let versions = versions(&r, p.iid).unwrap();
            let checks = pipeline(&r, p.iid).unwrap();
            let approvals = approvals(&r, p.iid).unwrap();
            println!(
                "#{}: {} threads ({lines} on lines), {} versions, checks {:?}, approved by {:?}",
                p.iid,
                threads.len(),
                versions.len(),
                checks.map(|c| c.status),
                approvals.by
            );
            assert_eq!(
                versions.last().map(|v| v.head.as_str()),
                Some(p.head.as_str())
            );
            seen_thread |= lines > 0;
        }
        assert!(
            seen_thread,
            "no review thread on the pull requests with line comments"
        );
        assert!(!username(&r).unwrap().is_empty());
    }
}
