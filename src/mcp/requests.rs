//! The merge request tools (#150): an agent reads a request as GitLance compares it and leaves
//! pending comments on its lines. The host is called with GitLance's own token (`crate::mr`), so
//! the agent needs none; the app is told through the Bus. Nothing is published: a pending comment
//! is seen by the reviewer alone until they submit the review, and submitting stays theirs. A
//! request the agent drafts (#152) opens in the app's New merge request card; the reviewer creates
//! it.

use crate::agents::{self, Bus, Event, Mark, RequestDraft, What};
use crate::git::{DiffSettings, FileDiff, LineKind, Repo, WebRemote};
use crate::mr;
use git2::Oid;
use serde_json::Value;
use std::fmt::Write as _;
use std::path::Path;

/// The diff lines one answer gives at most; the files past them are listed, and `paths` asks for
/// them.
const MOST_LINES: usize = 3000;

/// A request as its host compares it, read in this clone.
struct Read {
    remote: WebRemote,
    mr: mr::Mr,
    /// Where the diff starts (the sides' merge base) and its head.
    start: Oid,
    head: Oid,
    files: Vec<FileDiff>,
}

/// Request `iid`, wherever the host lists it: the open ones are a list of their own.
fn find(remote: &WebRemote, iid: u64) -> Result<mr::Mr, String> {
    mr::one(remote, iid).or_else(|e| {
        mr::list(remote)
            .ok()
            .and_then(|l| l.into_iter().find(|m| m.iid == iid))
            .ok_or_else(|| format!("!{iid} is not a merge request of this project: {e:#}"))
    })
}

/// The repository `here` is in, and its host when GitLance has a token for it.
fn host(here: &Path) -> Result<(Repo, WebRemote), String> {
    let repo =
        Repo::open(here).map_err(|_| format!("{} is not in a git repository", here.display()))?;
    let remote = repo
        .web_remote()
        .ok_or("This repository has no GitLab or GitHub remote.")?;
    if !mr::fixture_on() && mr::token(&remote).is_none() {
        return Err(format!(
            "GitLance has no token for {}; the reviewer adds one in GitLance → Settings → Merge requests.",
            mr::host(&remote)
        ));
    }
    Ok((repo, remote))
}

/// Request `iid` of the repository `here` is in, compared from the base and head of its host's
/// latest push (#145): a side this clone lacks is fetched first, once.
fn read(here: &Path, iid: u64) -> Result<Read, String> {
    let (repo, remote) = host(here)?;
    let m = find(&remote, iid)?;
    let latest = mr::versions(&remote, iid)
        .ok()
        .and_then(|p| p.last().cloned());
    let oid = |s: &str| Oid::from_str(s).ok().filter(|o| !o.is_zero());
    let head = latest
        .as_ref()
        .and_then(|p| oid(&p.head))
        .or_else(|| oid(&m.head));
    let base = latest.as_ref().and_then(|p| oid(&p.base));
    let root = repo.root();
    if head.is_none_or(|h| !repo.has_commit(h)) {
        let sha = head.map(|h| h.to_string());
        crate::git::fetch::fetch_request(&root, iid, remote.github, sha.as_deref()).map_err(
            |e| format!("The head of !{iid} is not in this clone and fetching it failed: {e}"),
        )?;
    }
    if base.is_some_and(|b| !repo.has_commit(b)) {
        crate::git::fetch::fetch_branch(&root, &m.target).map_err(|e| {
            format!("The base of !{iid} is not in this clone and fetching it failed: {e}")
        })?;
    }
    // Opened again: what was fetched is seen.
    let repo = Repo::open(here).map_err(|e| e.to_string())?;
    let head = match head {
        Some(h) => h,
        None => repo
            .resolve(&crate::git::request_ref(iid))
            .map_err(|_| format!("The head of !{iid} is not in this clone."))?,
    };
    let base = match base {
        Some(b) => b,
        // No pushes listed: the target as this clone's remote has it.
        None => {
            let remote_name = repo.web_remote_name().unwrap_or_else(|| "origin".into());
            repo.resolve(&format!("refs/remotes/{remote_name}/{}", m.target))
                .or_else(|_| repo.resolve(&m.target))
                .map_err(|_| format!("The target branch {} is not in this clone.", m.target))?
        }
    };
    if !repo.has_commit(head) || !repo.has_commit(base) {
        return Err(format!(
            "!{iid}'s commits are not in this clone; fetch them (git fetch) and ask again."
        ));
    }
    let compared = repo
        .compare(base, head, true, DiffSettings::default())
        .map_err(|e| format!("{e:#}"))?;
    Ok(Read {
        remote,
        mr: m,
        start: compared.start,
        head,
        files: compared.files,
    })
}

fn short(id: Oid) -> String {
    id.to_string()[..8].to_owned()
}

/// Line `n` of a side's text.
fn text_line(text: Option<&str>, n: Option<u32>) -> &str {
    match (text, n) {
        (Some(t), Some(n)) if n > 0 => t.lines().nth(n as usize - 1).unwrap_or_default(),
        _ => "",
    }
}

/// One file's hunks: each line with its old and new number, as add_pending_comment takes them.
fn write_file(out: &mut String, file: &FileDiff) {
    let (old, new) = (file.old_text.as_deref(), file.new_text.as_deref());
    for hunk in &file.hunks {
        let first = |get: fn(&crate::git::DiffLine) -> Option<u32>| {
            hunk.lines.iter().find_map(get).unwrap_or(0)
        };
        let _ = writeln!(
            out,
            "@@ old {} · new {} @@",
            first(|l| l.old_line),
            first(|l| l.new_line)
        );
        for l in &hunk.lines {
            let num = |n: Option<u32>| n.map_or(String::new(), |n| n.to_string());
            let (sign, line) = match l.kind {
                LineKind::Context => (' ', text_line(new, l.new_line)),
                LineKind::Added => ('+', text_line(new, l.new_line)),
                LineKind::Removed => ('-', text_line(old, l.old_line)),
            };
            let _ = writeln!(
                out,
                "{:>5} {:>5} {sign} {line}",
                num(l.old_line),
                num(l.new_line)
            );
        }
    }
}

fn lines_of(file: &FileDiff) -> usize {
    file.hunks.iter().map(|h| h.lines.len()).sum()
}

/// The pending comments the agents left on request `iid`, by id.
fn agents_drafts(bus: &Bus, project: &Path, iid: u64) -> Vec<Mark> {
    bus.marks(project)
        .into_iter()
        .filter(|m| m.iid == iid)
        .collect()
}

/// `get_request`: request `iid` as GitLance shows it — what it is, the diff the host compares,
/// its discussions and the pending comments already there. `paths` limits the diff to those files.
pub fn get_request(bus: &Bus, here: &Path, project: &Path, args: &Value) -> Result<String, String> {
    let iid = args["iid"].as_u64().ok_or("iid is missing")?;
    let paths: Vec<&str> = args["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let r = read(here, iid)?;
    let m = &r.mr;
    let (added, removed) = r
        .files
        .iter()
        .fold((0, 0), |(a, d), f| (a + f.added, d + f.removed));
    let mut out = format!(
        "# !{iid} {}\n\n{} · by {} · {} → {}\n{}\nCompared as {} compares it: {}...{} — {} files, +{added} −{removed}\n",
        m.title,
        m.state.word(),
        m.author,
        m.source,
        m.target,
        m.url,
        if r.remote.github { "GitHub" } else { "GitLab" },
        short(r.start),
        short(r.head),
        r.files.len(),
    );
    let description = m.description.trim();
    let _ = write!(
        out,
        "\n## Description\n\n{}\n",
        if description.is_empty() {
            "(none)"
        } else {
            description
        }
    );
    out.push_str(
        "\n## Changes\n\nEach line: its old number, its new number, then +, - or a space and the line. \
         Comment on a line shown here with add_pending_comment: give its new number, or for a removed line \
         its old number and old: true.\n",
    );
    let mut budget = MOST_LINES;
    let mut left = Vec::new();
    for file in &r.files {
        let path = file.path();
        if !paths.is_empty() && !paths.contains(&path) {
            continue;
        }
        let renamed = file
            .old_path
            .as_deref()
            .filter(|old| Some(*old) != file.new_path.as_deref() && file.new_path.is_some())
            .map(|old| format!(", from {old}"))
            .unwrap_or_default();
        let _ = write!(
            out,
            "\n### {path} (+{} −{}{renamed})\n",
            file.added, file.removed
        );
        let generated = paths.is_empty()
            && crate::generated::is_generated(path)
            && file.added + file.removed > crate::generated::FOLD_ABOVE;
        let lines = lines_of(file);
        if let Some(note) = &file.note {
            let _ = writeln!(out, "{note}");
        } else if generated {
            out.push_str("Generated: not shown; ask for it with paths.\n");
        } else if lines > budget {
            out.push_str("Not shown: the answer is long enough; ask for it with paths.\n");
            left.push(path.to_owned());
        } else {
            budget -= lines;
            write_file(&mut out, file);
        }
    }
    if !left.is_empty() {
        let _ = write!(
            out,
            "\n{} file(s) not shown: call get_request again with paths: {}\n",
            left.len(),
            left.join(", ")
        );
    }
    let threads = mr::threads(&r.remote, iid).unwrap_or_default();
    let _ = write!(out, "\n## Discussions ({})\n", threads.len());
    for t in &threads {
        let at = match (&t.path, t.new_line, t.old_line) {
            (Some(p), Some(n), _) => format!("{p}:{n}"),
            (Some(p), None, Some(o)) => format!("{p}:{o} (old)"),
            (Some(p), None, None) => p.clone(),
            (None, ..) => "the request".to_owned(),
        };
        let state = if t.resolved { "resolved" } else { "open" };
        let _ = write!(out, "\n- {at} · {state}\n");
        for n in &t.notes {
            let _ = writeln!(
                out,
                "  - {}: {}",
                n.author,
                n.body.trim().replace('\n', "\n    ")
            );
        }
    }
    let drafts = mr::drafts(&r.remote, iid).unwrap_or_default();
    let mine = agents_drafts(bus, project, iid);
    let _ = write!(
        out,
        "\n## Pending comments ({}) — only the reviewer sees these until they submit the review\n",
        drafts.len()
    );
    for d in &drafts {
        let at = match (&d.path, d.new_line, d.old_line) {
            (Some(p), Some(n), _) => format!("{p}:{n}"),
            (Some(p), None, Some(o)) => format!("{p}:{o} (old)"),
            _ => "the request".to_owned(),
        };
        let by = mine
            .iter()
            .find(|m| m.draft == Some(d.id))
            .map_or("the reviewer".to_owned(), |m| m.agent.clone());
        let _ = writeln!(out, "- {} · {at} · by {by}: {}", d.id, d.body.trim());
    }
    Ok(out)
}

/// The lines of each hunk on one side, as "12–30, 48–52", for a refusal.
fn shown_lines(file: &FileDiff, old: bool) -> String {
    file.hunks
        .iter()
        .filter_map(|h| {
            let side = h
                .lines
                .iter()
                .filter_map(|l| if old { l.old_line } else { l.new_line });
            let (lo, hi) = side.clone().min().zip(side.max())?;
            Some(if lo == hi {
                lo.to_string()
            } else {
                format!("{lo}–{hi}")
            })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `add_pending_comment`: a pending comment on a line of request `iid`, written as GitLance's own
/// ⇧⌘↵ writes it, from the host's diff; a line not in that diff is refused with why.
pub fn add_pending_comment(
    bus: &Bus,
    here: &Path,
    project: &Path,
    args: &Value,
) -> Result<String, String> {
    let iid = args["iid"].as_u64().ok_or("iid is missing")?;
    let path = args["path"].as_str().ok_or("path is missing")?;
    let line = args["line"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("line is missing")?;
    let body = args["body"].as_str().unwrap_or_default().trim();
    if body.is_empty() {
        return Err("body is missing".to_owned());
    }
    let old = args["old"].as_bool().unwrap_or(false);
    let start = args["start_line"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&s| s != line);
    let r = read(here, iid)?;
    let file = r
        .files
        .iter()
        .find(|f| f.new_path.as_deref() == Some(path) || f.old_path.as_deref() == Some(path))
        .ok_or_else(|| {
            let changed: Vec<&str> = r.files.iter().map(FileDiff::path).collect();
            format!(
                "{path} is not changed by !{iid}. Its changed files: {}",
                changed.join(", ")
            )
        })?;
    let side = if old { "old" } else { "new" };
    let refuse = |n: u32| {
        format!(
            "Line {n} ({side} side) of {path} is not in !{iid}'s diff, so a comment there has no place. \
             The diff shows {side} lines {} — comment on one of them{}.",
            shown_lines(file, old),
            if old {
                ""
            } else {
                ", or for a removed line give its old number with old: true"
            }
        )
    };
    let last = file.position(old, line).ok_or_else(|| refuse(line))?;
    if (old && last.kind == LineKind::Added) || (!old && last.kind == LineKind::Removed) {
        return Err(refuse(line));
    }
    let range = match start {
        Some(s) if s > line => {
            return Err(format!(
                "start_line {s} is below line {line}: a range runs from start_line down to line."
            ));
        }
        Some(s) => Some((file.position(old, s).ok_or_else(|| refuse(s))?, last)),
        None => None,
    };
    let (old_line, new_line) = file.line_pair(old, line);
    let new_path = file
        .new_path
        .clone()
        .or(file.old_path.clone())
        .unwrap_or_default();
    let old_path = file
        .old_path
        .clone()
        .or(file.new_path.clone())
        .unwrap_or_default();
    let place = mr::Place {
        path: &new_path,
        old_path: &old_path,
        new_line,
        old_line,
        range,
    };
    let id = mr::post_discussion(&r.remote, iid, &r.head.to_string(), &place, body, true)
        .map_err(|e| format!("{e:#}"))?;
    let agent = agents::agent_name();
    if let Some(id) = id {
        let mark = Mark {
            agent,
            iid,
            draft: Some(id),
            note: None,
            at: agents::now(),
        };
        bus.mark(project, &mark).map_err(|e| e.to_string())?;
    }
    let ids = id.into_iter().collect();
    bus.event(&Event::new(
        project.to_path_buf(),
        What::Pending { iid, ids },
    ))
    .map_err(|e| e.to_string())?;
    let at = match start {
        Some(s) => format!("{path}:{s}-{line}"),
        None => format!("{path}:{line}"),
    };
    let name = id.map_or_else(
        || "A pending comment".to_owned(),
        |id| format!("Pending comment {id}"),
    );
    Ok(format!(
        "{name} on {at}{} of !{iid}. Only the reviewer sees it until they submit the review; submitting, \
         approving and requesting changes are theirs.{}",
        if old { " (old side)" } else { "" },
        super::unseen(bus)
    ))
}

/// `gather_requests` (#148): the requests the agent worked on, each with its line on what it did,
/// put under Merge requests in the order given; the numbers that are not requests of this project
/// are said.
pub fn gather_requests(
    bus: &Bus,
    here: &Path,
    project: &Path,
    args: &Value,
) -> Result<String, String> {
    let asked: Vec<(u64, &str)> = args["requests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some((
                r["iid"].as_u64()?,
                r["note"].as_str().unwrap_or_default().trim(),
            ))
        })
        .collect();
    if asked.is_empty() {
        return Err("requests is missing: a list of {iid, note}".to_owned());
    }
    let (_, remote) = host(here)?;
    // The open ones in one call; another state is asked for one by one.
    let open = mr::list(&remote).unwrap_or_default();
    let mut found: Vec<(mr::Mr, &str)> = Vec::new();
    let mut missing: Vec<u64> = Vec::new();
    for (iid, note) in asked {
        if found.iter().any(|(m, _)| m.iid == iid) || missing.contains(&iid) {
            continue;
        }
        match open.iter().find(|m| m.iid == iid) {
            Some(m) => found.push((m.clone(), note)),
            None => match mr::one(&remote, iid) {
                Ok(m) => found.push((m, note)),
                Err(_) => missing.push(iid),
            },
        }
    }
    let numbers = |iids: &[u64]| {
        iids.iter()
            .map(|n| format!("!{n}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if found.is_empty() {
        return Err(format!(
            "None of {} is a merge request of this project.",
            numbers(&missing)
        ));
    }
    let agent = agents::agent_name();
    for (m, note) in &found {
        let mark = Mark {
            agent: agent.clone(),
            iid: m.iid,
            draft: None,
            note: Some((*note).to_owned()),
            at: agents::now(),
        };
        bus.mark(project, &mark).map_err(|e| e.to_string())?;
    }
    let iids = found.iter().map(|(m, _)| m.iid).collect();
    bus.event(&Event::new(project.to_path_buf(), What::Gathered { iids }))
        .map_err(|e| e.to_string())?;
    let mut out = String::from("Under Merge requests in GitLance, in this order:\n");
    for (m, note) in &found {
        let _ = write!(out, "- !{} {}", m.iid, m.title);
        if !note.is_empty() {
            let _ = write!(out, " — {note}");
        }
        out.push('\n');
    }
    if !missing.is_empty() {
        let _ = writeln!(
            out,
            "Not merge requests of this project, left out: {}.",
            numbers(&missing)
        );
    }
    Ok(format!("{}{}", out.trim_end(), super::unseen(bus)))
}

/// `draft_request` (#152): a merge request for a branch, drafted by the agent and opened in the New
/// merge request card, filled in, for the reviewer to check and create. Read here: the branch has
/// commits its target has not; one not on origin, or with commits origin has not, is said, as the
/// card says it. Nothing is created, sent or pushed.
pub fn draft_request(
    bus: &Bus,
    here: &Path,
    project: &Path,
    args: &Value,
) -> Result<String, String> {
    let repo =
        Repo::open(here).map_err(|_| format!("{} is not in a git repository", here.display()))?;
    let text = |key: &str| args[key].as_str().unwrap_or_default().trim().to_owned();
    // A side by the name the host knows: `origin/x` is `x`.
    let named = |key: &str| {
        let name = text(key);
        name.strip_prefix("origin/")
            .map(str::to_owned)
            .unwrap_or(name)
    };
    let list = |key: &str| {
        let mut out: Vec<String> = args[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|s| s.trim().trim_start_matches('@').to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        out.dedup();
        out
    };
    let title = text("title");
    if title.is_empty() {
        return Err("title is missing: the merge request's title".to_owned());
    }
    let source = match named("source") {
        s if !s.is_empty() => s,
        _ => repo
            .head_branch()
            .ok_or("source is missing: nothing is checked out here, so name the branch")?,
    };
    let target = match named("target") {
        t if !t.is_empty() => t,
        _ => repo
            .default_branch()
            .ok_or("target is missing: name the branch it merges into")?,
    };
    if source == target {
        return Err(format!(
            "{source} cannot merge into itself: name the branch it merges into."
        ));
    }
    // Each side here, on origin, or both.
    let tips = |name: &str| {
        (
            repo.resolve(&format!("refs/heads/{name}")).ok(),
            repo.resolve(&format!("refs/remotes/origin/{name}")).ok(),
        )
    };
    let (mine, pushed) = tips(&source);
    let Some(head) = mine.or(pushed) else {
        return Err(format!(
            "{source} is not a branch here: name one that is, or fetch it first."
        ));
    };
    let (target_here, target_pushed) = tips(&target);
    let Some(base) = target_pushed.or(target_here) else {
        return Err(format!(
            "{target} is not a branch here: name the branch it merges into."
        ));
    };
    let commits = repo
        .range_log(base, head, true)
        .map_err(|e| format!("{e:#}"))?
        .len();
    if commits == 0 {
        return Err(format!(
            "{source} has no commits that {target} has not: nothing to merge."
        ));
    }
    // One already open for the pair is the reviewer's to open, not to create again; asked only
    // when the host answers.
    if let Ok((_, remote)) = host(here)
        && let Ok(open) = mr::list(&remote)
        && let Some(m) = open
            .iter()
            .find(|m| m.source == source && m.target == target)
    {
        return Err(format!(
            "!{} is already open for {source} → {target} ({}): nothing to draft. Read it with get_request.",
            m.iid, m.title
        ));
    }
    let unpushed = match (mine, pushed) {
        (Some(mine), Some(pushed)) if mine != pushed => repo
            .range_log(pushed, mine, true)
            .map(|log| log.len())
            .unwrap_or(0),
        _ => 0,
    };
    let draft = RequestDraft {
        source: source.clone(),
        target: target.clone(),
        title: title.clone(),
        description: text("description"),
        reviewers: list("reviewers"),
        labels: list("labels"),
    };
    bus.event(&Event::new(project.to_path_buf(), What::NewRequest(draft)))
        .map_err(|e| e.to_string())?;
    let mut out = format!(
        "The New merge request card waits in GitLance with your draft: {source} → {target}, \"{title}\", {commits} commit(s). Nothing was created or sent: the reviewer checks it, edits it and creates it."
    );
    // GitLance never pushes: the host makes the request from what origin has.
    if pushed.is_none() {
        let _ = write!(
            out,
            " {source} is only on this Mac: the card asks for it to be pushed first, and GitLance never pushes it."
        );
    } else if unpushed > 0 {
        let _ = write!(
            out,
            " {unpushed} commit(s) of {source} are not on origin yet: the request will not have them until they are pushed."
        );
    }
    Ok(format!("{out}{}", super::unseen(bus)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A commit of flat `files` on `parent`, at `refname`.
    fn commit(
        git: &git2::Repository,
        refname: &str,
        parent: Option<Oid>,
        files: &[(&str, String)],
    ) -> Oid {
        let mut tree = git.treebuilder(None).unwrap();
        for (name, text) in files {
            tree.insert(name, git.blob(text.as_bytes()).unwrap(), 0o100644)
                .unwrap();
        }
        let tree = git.find_tree(tree.write().unwrap()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let parents: Vec<_> = parent
            .map(|p| git.find_commit(p).unwrap())
            .into_iter()
            .collect();
        let parents: Vec<_> = parents.iter().collect();
        git.commit(Some(refname), &sig, &sig, "c", &tree, &parents)
            .unwrap()
    }

    #[test]
    fn an_agent_reads_a_request_and_comments_on_its_lines_only() {
        let dir = tempfile::TempDir::new().unwrap();
        let clone = dir.path().join("clone");
        let git = git2::Repository::init(&clone).unwrap();
        git.remote("origin", "https://gitlab.example.com/g/p.git")
            .unwrap();
        let lines: Vec<String> = (1..=20).map(|n| format!("line {n}")).collect();
        let mut changed = lines.clone();
        changed[14] = "LINE 15".into();
        let base = commit(
            &git,
            "refs/remotes/origin/main",
            None,
            &[("a.rs", lines.join("\n") + "\n")],
        );
        let head = commit(
            &git,
            "refs/remotes/origin/feature",
            Some(base),
            &[("a.rs", changed.join("\n") + "\n")],
        );
        // GitLab's answers: the open request and its one push.
        let host = dir.path().join("host");
        std::fs::create_dir_all(&host).unwrap();
        let request = json!([{"iid": 7, "title": "Louder", "description": "Shouts line 15.",
            "source_branch": "feature", "target_branch": "main", "sha": head.to_string(),
            "author": {"name": "Ann", "username": "ann"}}]);
        std::fs::write(host.join("mrs.json"), request.to_string()).unwrap();
        let push = json!([{"head_commit_sha": head.to_string(),
            "base_commit_sha": base.to_string(), "created_at": "2026-10-01T10:00:00Z"}]);
        std::fs::write(host.join("versions-7.json"), push.to_string()).unwrap();
        mr::TEST_FIXTURE.with(|f| *f.borrow_mut() = Some(host.clone()));
        let bus = Bus::at(dir.path().join("agents"));
        let here = clone.as_path();

        let read = get_request(&bus, here, here, &json!({"iid": 7})).unwrap();
        assert!(read.starts_with("# !7 Louder"), "{read}");
        assert!(read.contains("Shouts line 15."));
        assert!(read.contains("   15       - line 15"), "{read}");
        assert!(read.contains("         15 + LINE 15"), "{read}");
        assert!(read.contains("## Pending comments (0)"));

        // Not in the diff: refused, with the lines that are.
        let comment = |args: Value| add_pending_comment(&bus, here, here, &args);
        let off = comment(json!({"iid": 7, "path": "a.rs", "line": 2, "body": "Hm"})).unwrap_err();
        assert!(
            off.contains("not in !7's diff") && off.contains("12–18"),
            "{off}"
        );
        let other =
            comment(json!({"iid": 7, "path": "b.rs", "line": 1, "body": "Hm"})).unwrap_err();
        assert!(other.contains("b.rs is not changed by !7"), "{other}");
        // The new line is not on the old side.
        assert!(
            comment(json!({"iid": 7, "path": "a.rs", "line": 15, "old": true, "body": "x"}))
                .is_ok()
        );
        let added = json!({"iid": 7, "path": "a.rs", "line": 15, "body": "Why shout?"});
        let ok = comment(added).unwrap();
        assert!(
            ok.starts_with("Pending comment 9002 on a.rs:15 of !7"),
            "{ok}"
        );

        // The app hears of each and tells them apart from the reviewer's.
        let marks = bus.marks(here);
        assert_eq!(
            marks.iter().map(|m| (m.iid, m.draft)).collect::<Vec<_>>(),
            [(7, Some(9001)), (7, Some(9002))]
        );
        let events = bus.events();
        assert_eq!(events.len(), 2);
        let said = |id| {
            events
                .iter()
                .any(|e| matches!(&e.what, What::Pending { iid: 7, ids } if ids == &[id]))
        };
        assert!(said(9001) && said(9002));
        let read = get_request(&bus, here, here, &json!({"iid": 7})).unwrap();
        assert!(read.contains("## Pending comments (2)"), "{read}");
        assert!(read.contains("- 9002 · a.rs:15 · by "), "{read}");
        assert!(!read.contains("by the reviewer"), "{read}");
        mr::TEST_FIXTURE.with(|f| *f.borrow_mut() = None);
    }

    #[test]
    fn gathered_requests_keep_their_order_and_the_others_are_said() {
        let dir = tempfile::TempDir::new().unwrap();
        let git = git2::Repository::init(dir.path().join("clone")).unwrap();
        git.remote("origin", "https://gitlab.example.com/g/p.git")
            .unwrap();
        // Two open, one merged; !99 is nowhere.
        let host = dir.path().join("host");
        std::fs::create_dir_all(&host).unwrap();
        let open = json!([{"iid": 7, "title": "Louder"}, {"iid": 8, "title": "Quieter"}]);
        std::fs::write(host.join("mrs.json"), open.to_string()).unwrap();
        let merged = json!([{"iid": 5, "title": "Older", "state": "merged"}]);
        std::fs::write(host.join("mrs-merged.json"), merged.to_string()).unwrap();
        mr::TEST_FIXTURE.with(|f| *f.borrow_mut() = Some(host.clone()));
        let bus = Bus::at(dir.path().join("agents"));
        let here = dir.path().join("clone");
        let gather = |args: Value| gather_requests(&bus, &here, &here, &args);

        let said = gather(json!({"requests": [
            {"iid": 8, "note": "2 pending comments"}, {"iid": 99, "note": "?"},
            {"iid": 5}, {"iid": 8, "note": "again"}, {"iid": 7, "note": "looked, nothing to say"}
        ]}))
        .unwrap();
        assert!(
            said.contains(
                "- !8 Quieter — 2 pending comments\n- !5 Older\n- !7 Louder — looked, nothing to say"
            ),
            "{said}"
        );
        assert!(said.contains("left out: !99."), "{said}");
        // The app is told, in the order given, with each one's line kept.
        let events = bus.events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].what,
            What::Gathered {
                iids: vec![8, 5, 7]
            }
        );
        let notes: Vec<_> = bus
            .marks(&here)
            .into_iter()
            .map(|m| (m.iid, m.draft, m.note.unwrap_or_default()))
            .collect();
        assert_eq!(
            notes,
            [
                (8, None, "2 pending comments".to_owned()),
                (5, None, String::new()),
                (7, None, "looked, nothing to say".to_owned())
            ]
        );
        // None at all: nothing is told.
        assert!(gather(json!({"requests": [{"iid": 99}]})).is_err());
        assert!(gather(json!({})).is_err());
        assert_eq!(bus.events().len(), 1);
        mr::TEST_FIXTURE.with(|f| *f.borrow_mut() = None);
    }

    #[test]
    fn a_drafted_request_names_its_branches_and_creates_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let clone = dir.path().join("clone");
        let git = git2::Repository::init(&clone).unwrap();
        let base = commit(
            &git,
            "refs/remotes/origin/main",
            None,
            &[("a.rs", "one\n".into())],
        );
        let pushed = commit(
            &git,
            "refs/remotes/origin/feature",
            Some(base),
            &[("a.rs", "two\n".into())],
        );
        git.reference("refs/heads/feature", pushed, true, "")
            .unwrap();
        let local = commit(
            &git,
            "refs/heads/solo",
            Some(base),
            &[("a.rs", "three\n".into())],
        );
        git.set_head("refs/heads/solo").unwrap();
        let bus = Bus::at(dir.path().join("agents"));
        let here = clone.as_path();
        let draft = |args: Value| draft_request(&bus, here, here, &args);

        let said = draft(json!({
            "source": "origin/feature",
            "target": "main",
            "title": "Say two",
            "description": "Because.",
            "reviewers": ["@ann", "bob", " "],
            "labels": ["ui"],
        }))
        .unwrap();
        assert!(
            said.starts_with("The New merge request card waits in GitLance with your draft: feature → main, \"Say two\", 1 commit(s). Nothing was created"),
            "{said}"
        );
        assert!(!said.contains("pushed"), "{said}");
        let events = bus.events();
        assert_eq!(
            events[0].what,
            What::NewRequest(RequestDraft {
                source: "feature".into(),
                target: "main".into(),
                title: "Say two".into(),
                description: "Because.".into(),
                reviewers: vec!["ann".into(), "bob".into()],
                labels: vec!["ui".into()],
            })
        );
        // The branch checked out here, only on this Mac: said, as the card says it.
        let said = draft(json!({"target": "main", "title": "Say three"})).unwrap();
        assert!(said.contains("solo → main"), "{said}");
        assert!(said.contains("solo is only on this Mac"), "{said}");
        // Commits origin has not yet.
        let ahead = commit(
            &git,
            "refs/heads/feature",
            Some(pushed),
            &[("a.rs", "four\n".into())],
        );
        assert_ne!(ahead, local);
        let said = draft(json!({"source": "feature", "target": "main", "title": "t"})).unwrap();
        assert!(
            said.contains("1 commit(s) of feature are not on origin yet"),
            "{said}"
        );
        // No title, no branch, nothing to merge, itself.
        assert!(
            draft(json!({"source": "feature"}))
                .unwrap_err()
                .contains("title")
        );
        let none = draft(json!({"source": "nope", "target": "main", "title": "t"})).unwrap_err();
        assert!(none.contains("nope is not a branch here"), "{none}");
        let same = draft(json!({"source": "main", "target": "feature", "title": "t"})).unwrap_err();
        assert!(same.contains("nothing to merge"), "{same}");
        assert!(
            draft(json!({"source": "main", "target": "origin/main", "title": "t"}))
                .unwrap_err()
                .contains("into itself")
        );
        assert_eq!(bus.events().len(), 3);
        // A request open for the pair on the host: said, not drafted again.
        git.remote("origin", "https://gitlab.example.com/g/p.git")
            .unwrap();
        let host = dir.path().join("host");
        std::fs::create_dir_all(&host).unwrap();
        let open = json!([{"iid": 9, "title": "Say two", "source_branch": "feature",
            "target_branch": "main", "sha": pushed.to_string(),
            "author": {"name": "Ann", "username": "ann"}}]);
        std::fs::write(host.join("mrs.json"), open.to_string()).unwrap();
        mr::TEST_FIXTURE.with(|f| *f.borrow_mut() = Some(host.clone()));
        let twice =
            draft(json!({"source": "feature", "target": "main", "title": "t"})).unwrap_err();
        assert!(
            twice.starts_with("!9 is already open for feature → main"),
            "{twice}"
        );
        assert_eq!(bus.events().len(), 3);
    }
}
