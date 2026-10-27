//! `open_changes` (#149): what an agent changed, opened for the reviewer as a row under Branches —
//! two refs compared, or the working tree. The refs are read here, in the agent's folder; the app
//! is told their commits, so it compares what the agent meant.

use crate::agents::{Bus, Event, What};
use crate::git::{DiffSettings, Repo};
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

pub fn open_changes(
    bus: &Bus,
    here: &Path,
    project: &Path,
    args: &Value,
) -> Result<String, String> {
    let repo =
        Repo::open(here).map_err(|_| format!("{} is not in a git repository", here.display()))?;
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
        let (count, _) = repo.working_summary().map_err(|e| format!("{e:#}"))?;
        if count == 0 {
            return Err("The working tree has no changes: nothing to open.".to_owned());
        }
        bus.event(&Event::new(
            project.to_path_buf(),
            What::WorkingTree { unstaged },
        ))
        .map_err(|e| e.to_string())?;
        return Ok(format!(
            "Under Branches in GitLance: the working tree{} — {count} path(s) changed.{}",
            if unstaged { ", not staged only" } else { "" },
            super::unseen(bus)
        ));
    }
    let base_name = text("base")
        .map(str::to_owned)
        .or_else(|| repo.default_branch())
        .ok_or("base is missing: the ref the changes are compared against")?;
    // The main line is here as a local branch, or only as origin's.
    let base = side(&repo, &base_name)
        .or_else(|e| side(&repo, &format!("origin/{base_name}")).map_err(|_| e))?;
    let head = side(&repo, text("head").unwrap_or("HEAD"))?;
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
    bus.event(&Event::new(
        project.to_path_buf(),
        What::Compare {
            base: base.0.clone(),
            base_id: base.1.to_string(),
            head: head.0.clone(),
            head_id: head.1.to_string(),
            direct,
        },
    ))
    .map_err(|e| e.to_string())?;
    Ok(format!(
        "Under Branches in GitLance: {} … {}, {} — {} file(s), +{added} −{removed}.{}",
        base.0,
        head.0,
        if direct {
            "directly"
        } else {
            "since their merge base"
        },
        compared.files.len(),
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
}
