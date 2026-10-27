//! `open_changes` (#149): what an agent changed, opened for the reviewer as a row under Branches —
//! two refs compared, or the working tree. The refs are read here, in the agent's folder; the app
//! is told their commits, so it compares what the agent meant. `annotate_changes` (#151): the
//! agent's notes on lines of those changes, with a summary, which `open_changes` can carry too.

use crate::agents::{Bus, Event, What};
use crate::git::{DiffSettings, Repo};
use crate::notes::Note;
use serde_json::Value;
use std::path::Path;

/// A side as the agent named it, `HEAD` as the branch it stands for, and its commit.
fn side(repo: &Repo, name: &str) -> Result<(String, git2::Oid), String> {
    let id = repo.resolve(name).map_err(|_| {
        format!("{name} is not a branch, tag or commit here: name one that is, or fetch it first.")
    })?;
    let name = if name.trim() == "HEAD" {
        repo.head_branch()
            .unwrap_or_else(|| id.to_string()[..8].to_owned())
    } else {
        name.trim().to_owned()
    };
    Ok((name, id))
}

/// The changes an agent names: the event that opens them, how they are said, the paths they
/// change, and where a note on a path reads its line — the working tree in this folder, or the
/// head's commit.
struct Changes {
    what: What,
    said: String,
    paths: Vec<String>,
    head: Option<git2::Oid>,
}

fn changes(repo: &Repo, here: &Path, project: &Path, args: &Value) -> Result<Changes, String> {
    let text = |key: &str| args[key].as_str().map(str::trim).filter(|s| !s.is_empty());
    if args["working_tree"].as_bool().unwrap_or(false) {
        // GitLance shows the project's main working tree; a linked worktree's changes are not
        // in it.
        if here != project {
            return Err(format!(
                "This folder is a worktree of {}; GitLance shows that project's own working tree, not this one. \
                 Commit the work and open it as a comparison (base and head) instead.",
                project.display()
            ));
        }
        let unstaged = args["unstaged"].as_bool().unwrap_or(false);
        let tree = repo
            .working_tree(DiffSettings::default(), unstaged)
            .map_err(|e| format!("{e:#}"))?;
        if tree.files.is_empty() {
            return Err("The working tree has no changes: nothing to open.".to_owned());
        }
        return Ok(Changes {
            what: What::WorkingTree { unstaged },
            said: format!(
                "the working tree{} — {} path(s) changed",
                if unstaged { ", not staged only" } else { "" },
                tree.files.len()
            ),
            paths: tree.files.iter().map(|f| f.path().to_owned()).collect(),
            head: None,
        });
    }
    let base_name = text("base")
        .map(str::to_owned)
        .or_else(|| repo.default_branch())
        .ok_or("base is missing: the ref the changes are compared against")?;
    // The main line is here as a local branch, or only as origin's.
    let base = side(repo, &base_name)
        .or_else(|e| side(repo, &format!("origin/{base_name}")).map_err(|_| e))?;
    let head = side(repo, text("head").unwrap_or("HEAD"))?;
    if base.1 == head.1 {
        return Err(format!(
            "{} and {} are the same commit: nothing to compare. Name the head that has the changes.",
            base.0, head.0
        ));
    }
    let direct = args["direct"].as_bool().unwrap_or(false);
    let compared = repo
        .compare(base.1, head.1, !direct, DiffSettings::default())
        .map_err(|e| format!("{e:#}"))?;
    let (added, removed) = compared
        .files
        .iter()
        .fold((0, 0), |(a, d), f| (a + f.added, d + f.removed));
    Ok(Changes {
        said: format!(
            "{} … {}, {} — {} file(s), +{added} −{removed}",
            base.0,
            head.0,
            if direct {
                "directly"
            } else {
                "since their merge base"
            },
            compared.files.len(),
        ),
        what: What::Compare {
            base: base.0,
            base_id: base.1.to_string(),
            head: head.0,
            head_id: head.1.to_string(),
            direct,
        },
        paths: compared.files.iter().map(|f| f.path().to_owned()).collect(),
        head: Some(head.1),
    })
}

/// The agent's notes on lines of `changes` (#151): each on a path they change and a line the file
/// has there, its text kept to find it again; refused with why, naming the paths that changed.
fn notes(repo: &Repo, here: &Path, changes: &Changes, args: &Value) -> Result<Vec<Note>, String> {
    let given = args["notes"].as_array().cloned().unwrap_or_default();
    let mut notes = Vec::with_capacity(given.len());
    for n in &given {
        let path = n["path"].as_str().unwrap_or_default().trim().to_owned();
        let body = n["text"].as_str().unwrap_or_default().trim().to_owned();
        let line = n["line"].as_u64().unwrap_or(0) as u32;
        if body.is_empty() {
            return Err(format!("The note on {path}:{line} has no text."));
        }
        if !changes.paths.contains(&path) {
            let mut listed: Vec<&str> = changes.paths.iter().take(30).map(String::as_str).collect();
            if changes.paths.len() > listed.len() {
                listed.push("…");
            }
            return Err(format!(
                "{path} is not one of the changed files. They are:\n{}",
                listed.join("\n")
            ));
        }
        let text = match changes.head {
            Some(head) => repo.file_at(head, &path),
            None => std::fs::read_to_string(here.join(&path)).ok(),
        }
        .unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let Some(code) = (line as usize).checked_sub(1).and_then(|i| lines.get(i)) else {
            return Err(format!(
                "{path} has no line {line}: it has {} line(s) in these changes.",
                lines.len()
            ));
        };
        notes.push(Note {
            path,
            line,
            code: (*code).to_owned(),
            body,
            ..Default::default()
        });
    }
    Ok(notes)
}

/// The summary the agent gave its notes, trimmed.
fn summary(args: &Value) -> String {
    args["summary"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

pub fn open_changes(
    bus: &Bus,
    here: &Path,
    project: &Path,
    args: &Value,
) -> Result<String, String> {
    let repo =
        Repo::open(here).map_err(|_| format!("{} is not in a git repository", here.display()))?;
    let changes = changes(&repo, here, project, args)?;
    // Notes with them, when the agent wrote some: they open with their notes on (#151).
    let notes = notes(&repo, here, &changes, args)?;
    let summary = summary(args);
    let with = match notes.len() {
        0 => String::new(),
        n => format!(", with {n} note(s)"),
    };
    let what = if notes.is_empty() && summary.is_empty() {
        changes.what
    } else {
        What::Notes {
            changes: Box::new(changes.what),
            summary,
            notes,
            open: true,
        }
    };
    bus.event(&Event::new(project.to_path_buf(), what))
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "Under Branches in GitLance: {}{with}.{}",
        changes.said,
        super::unseen(bus)
    ))
}

/// `annotate_changes` (#151): notes on lines of the working tree or of a branch, and a summary,
/// kept in GitLance beside the diff; nothing opens and nothing goes to GitLab or GitHub.
pub fn annotate_changes(
    bus: &Bus,
    here: &Path,
    project: &Path,
    args: &Value,
) -> Result<String, String> {
    let repo =
        Repo::open(here).map_err(|_| format!("{} is not in a git repository", here.display()))?;
    let changes = changes(&repo, here, project, args)?;
    let notes = notes(&repo, here, &changes, args)?;
    let summary = summary(args);
    if notes.is_empty() && summary.is_empty() {
        return Err("No notes and no summary: give notes (path, line, text) or a summary.".into());
    }
    let files: std::collections::BTreeSet<&str> = notes.iter().map(|n| n.path.as_str()).collect();
    let said = format!(
        "{} note(s) on {} file(s) of {}",
        notes.len(),
        files.len(),
        changes.said
    );
    let what = What::Notes {
        changes: Box::new(changes.what),
        summary,
        notes,
        open: false,
    };
    bus.event(&Event::new(project.to_path_buf(), what))
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "Kept in GitLance beside the diff: {said}. The reviewer sees them there and can answer one as a comment; nothing was sent to GitLab or GitHub.{}",
        super::unseen(bus)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn changes_are_opened_by_their_commits_and_nothing_empty_is() {
        let dir = tempfile::TempDir::new().unwrap();
        let here = dir.path().join("clone");
        let git = git2::Repository::init(&here).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        std::fs::write(here.join("a.rs"), "one\n").unwrap();
        let mut index = git.index().unwrap();
        index.add_path(Path::new("a.rs")).unwrap();
        index.write().unwrap();
        let tree = git.find_tree(index.write_tree().unwrap()).unwrap();
        let main = git
            .commit(Some("HEAD"), &sig, &sig, "one", &tree, &[])
            .unwrap();
        let name = git.head().unwrap().shorthand().unwrap().to_owned();
        let mut more = git.treebuilder(Some(&tree)).unwrap();
        more.insert("b.rs", git.blob(b"two\n").unwrap(), 0o100644)
            .unwrap();
        let more = git.find_tree(more.write().unwrap()).unwrap();
        let parent = git.find_commit(main).unwrap();
        let feature = git
            .commit(
                Some("refs/heads/feature"),
                &sig,
                &sig,
                "two",
                &more,
                &[&parent],
            )
            .unwrap();
        let bus = Bus::at(dir.path().join("agents"));
        let open = |args: Value| open_changes(&bus, &here, &here, &args);

        let said = open(json!({"base": name, "head": "feature"})).unwrap();
        assert!(
            said.starts_with(&format!("Under Branches in GitLance: {name} … feature, since their merge base — 1 file(s), +1 −0.")),
            "{said}"
        );
        let events = bus.events();
        assert_eq!(
            events[0].what,
            What::Compare {
                base: name.clone(),
                base_id: main.to_string(),
                head: "feature".into(),
                head_id: feature.to_string(),
                direct: false,
            }
        );
        // What is not here, or the same commit twice, is said and not opened.
        assert!(
            open(json!({"base": "nope"}))
                .unwrap_err()
                .contains("nope is not")
        );
        let same = open(json!({"base": name})).unwrap_err();
        assert!(same.contains("same commit"), "{same}");
        assert!(
            open(json!({"working_tree": true}))
                .unwrap_err()
                .contains("no changes")
        );
        std::fs::write(here.join("a.rs"), "one\nmore\n").unwrap();
        let said = open(json!({"working_tree": true, "unstaged": true})).unwrap();
        assert!(
            said.contains("the working tree, not staged only — 1 path(s)"),
            "{said}"
        );
        let events = bus.events();
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .any(|e| e.what == What::WorkingTree { unstaged: true })
        );
    }

    #[test]
    fn notes_sit_on_lines_of_the_changes_and_are_refused_elsewhere() {
        let dir = tempfile::TempDir::new().unwrap();
        let here = dir.path().join("clone");
        let git = git2::Repository::init(&here).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        std::fs::write(here.join("a.rs"), "one\n").unwrap();
        let mut index = git.index().unwrap();
        index.add_path(Path::new("a.rs")).unwrap();
        index.write().unwrap();
        let tree = git.find_tree(index.write_tree().unwrap()).unwrap();
        let main = git
            .commit(Some("HEAD"), &sig, &sig, "one", &tree, &[])
            .unwrap();
        let name = git.head().unwrap().shorthand().unwrap().to_owned();
        let mut more = git.treebuilder(Some(&tree)).unwrap();
        more.insert("b.rs", git.blob(b"two\nthree\n").unwrap(), 0o100644)
            .unwrap();
        let more = git.find_tree(more.write().unwrap()).unwrap();
        let parent = git.find_commit(main).unwrap();
        git.commit(
            Some("refs/heads/feature"),
            &sig,
            &sig,
            "two",
            &more,
            &[&parent],
        )
        .unwrap();
        let bus = Bus::at(dir.path().join("agents"));
        let annotate = |args: Value| annotate_changes(&bus, &here, &here, &args);
        let on = |notes: Value| json!({"base": name, "head": "feature", "notes": notes});

        let said = annotate(json!({
            "base": name,
            "head": "feature",
            "summary": "Adds b.",
            "notes": [
                {"path": "b.rs", "line": 2, "text": "Why three."},
                {"path": "b.rs", "line": 1, "text": "Why two."},
            ],
        }))
        .unwrap();
        assert!(
            said.starts_with("Kept in GitLance beside the diff: 2 note(s) on 1 file(s) of "),
            "{said}"
        );
        assert!(said.contains("nothing was sent"), "{said}");
        let events = bus.events();
        let What::Notes {
            changes,
            summary,
            notes,
            open,
        } = &events[0].what
        else {
            panic!("{:?}", events[0].what);
        };
        assert!(matches!(**changes, What::Compare { ref head, .. } if head == "feature"));
        assert_eq!((summary.as_str(), *open), ("Adds b.", false));
        assert_eq!(
            notes
                .iter()
                .map(|n| (n.line, n.code.as_str()))
                .collect::<Vec<_>>(),
            [(2, "three"), (1, "two")]
        );
        // A file the changes leave alone, a line the file has not, a note with no text, nothing.
        let away = annotate(on(json!([{"path": "a.rs", "line": 1, "text": "x"}]))).unwrap_err();
        assert!(
            away.contains("not one of the changed files") && away.contains("b.rs"),
            "{away}"
        );
        let past = annotate(on(json!([{"path": "b.rs", "line": 9, "text": "x"}]))).unwrap_err();
        assert!(past.contains("has no line 9: it has 2"), "{past}");
        let blank = annotate(on(json!([{"path": "b.rs", "line": 1, "text": " "}]))).unwrap_err();
        assert!(blank.contains("no text"), "{blank}");
        assert!(annotate(on(json!([]))).unwrap_err().contains("No notes"));
        assert_eq!(bus.events().len(), 1);

        // The working tree's lines are read from the file; opened with them, it opens.
        std::fs::write(here.join("a.rs"), "one\nmore\n").unwrap();
        let said = open_changes(
            &bus,
            &here,
            &here,
            &json!({
                "working_tree": true,
                "notes": [{"path": "a.rs", "line": 2, "text": "Added."}],
            }),
        )
        .unwrap();
        assert!(said.contains("with 1 note(s)."), "{said}");
        let events = bus.events();
        assert!(events.iter().any(|e| matches!(
            &e.what,
            What::Notes { changes, notes, open: true, .. }
                if **changes == (What::WorkingTree { unstaged: false }) && notes[0].code == "more"
        )));
    }
}
