//! `show` (#154): the agent points the reviewer at one place — a file and a line, a commit, a merge
//! request at one of its comments. The place is checked here, in the agent's folder; the app only
//! says it in a toast whose Show goes there.

use crate::agents::{Bus, Event, Place, What};
use crate::git::Repo;
use serde_json::Value;
use std::path::Path;

pub fn show(bus: &Bus, here: &Path, project: &Path, args: &Value) -> Result<String, String> {
    let text = |key: &str| args[key].as_str().map(str::trim).filter(|s| !s.is_empty());
    let request = args["request"].as_u64();
    let asked = [
        text("path").is_some(),
        text("commit").is_some(),
        request.is_some(),
    ];
    if asked.iter().filter(|&&a| a).count() != 1 {
        return Err("Name one place: a path (with a line), a commit, or a request.".to_owned());
    }
    let line = args["line"].as_u64().and_then(|n| u32::try_from(n).ok());
    let (place, said) = if let Some(path) = text("path") {
        file(here, project, path, line)?
    } else if let Some(commit) = text("commit") {
        let repo = Repo::open(here)
            .map_err(|_| format!("{} is not in a git repository", here.display()))?;
        let id = repo.resolve(commit).map_err(|_| {
            format!(
                "{commit} is not a commit, branch or tag here: name one that is, or fetch it first."
            )
        })?;
        let summary = repo.commit(id).map(|c| c.summary).unwrap_or_default();
        let id = id.to_string();
        let said = format!("commit {} {summary}", &id[..7]);
        (Place::Commit { id }, said)
    } else {
        let comment = args["comment"].as_u64();
        super::requests::request_place(here, request.unwrap_or_default(), comment)?
    };
    bus.event(&Event::new(project.to_path_buf(), What::Point { place }))
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "Pointed the reviewer at {said}: a note in GitLance offers to show it.{}",
        super::unseen(bus)
    ))
}

/// A file of the project's working tree, as GitLance's Code reads it: named from the root, an
/// absolute path or `path:line` taken too; its line one the file has.
fn file(
    here: &Path,
    project: &Path,
    path: &str,
    line: Option<u32>,
) -> Result<(Place, String), String> {
    let (path, line) = match (line, path.rsplit_once(':')) {
        (None, Some((p, n))) if n.parse::<u32>().is_ok() => (p, n.parse().ok()),
        _ => (path, line),
    };
    let path = [here, project]
        .iter()
        .find_map(|root| Path::new(path).strip_prefix(root).ok())
        .and_then(Path::to_str)
        .unwrap_or(path)
        .trim_start_matches("./")
        .to_owned();
    let Ok(text) = std::fs::read(project.join(&path)) else {
        if here != project && here.join(&path).is_file() {
            return Err(format!(
                "{path} is in this worktree only; GitLance shows {}'s own working tree. Commit it and point at the commit instead.",
                project.display()
            ));
        }
        return Err(format!("{path} is not a file of this project."));
    };
    if let Some(line) = line {
        let lines = text.split(|&b| b == b'\n').count() - usize::from(text.ends_with(b"\n"));
        if line == 0 || line as usize > lines {
            return Err(format!(
                "{path} has {lines} lines; line {line} is not one of them."
            ));
        }
    }
    let said = match line {
        Some(line) => format!("{path}:{line}"),
        None => path.clone(),
    };
    Ok((Place::File { path, line }, said))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_named_from_the_root_and_its_line_is_one_it_has() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/nav.rs"), "a\nb\nc\n").unwrap();
        let place = |path: &str, line| file(root, root, path, line).map(|(p, _)| p);
        let at = |line| Place::File {
            path: "src/nav.rs".into(),
            line,
        };
        assert_eq!(place("src/nav.rs:3", None), Ok(at(Some(3))));
        let whole = root.join("src/nav.rs");
        assert_eq!(place(whole.to_str().unwrap(), Some(2)), Ok(at(Some(2))));
        assert_eq!(place("./src/nav.rs", None), Ok(at(None)));
        assert!(
            place("src/nav.rs", Some(4))
                .unwrap_err()
                .contains("has 3 lines")
        );
        assert!(
            place("src/gone.rs", None)
                .unwrap_err()
                .contains("not a file")
        );
    }
}
