//! The project's files as Code's tree lists them (#75): a plain folder's files on disk — a
//! repository's come from git (`Repo::working_files`, `Repo::files_of`) — and the rows of the tree
//! for the folders open.

use std::collections::HashSet;
use std::path::Path;

/// Folders never listed: git's own, and what tools rebuild.
const SKIPPED: [&str; 5] = [".git", "node_modules", "target", ".gradle", ".DS_Store"];

/// A plain folder's files, relative, `/`-separated and sorted, up to `limit`; the names its
/// top `.gitignore` lists by name (`build/`, `*.log`, `secrets.txt`) are left out.
pub fn on_disk(root: &Path, limit: usize) -> Vec<String> {
    let ignored =
        Ignore::parse(&std::fs::read_to_string(root.join(".gitignore")).unwrap_or_default());
    let mut out = Vec::new();
    let mut dirs = vec![String::new()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(root.join(&dir)) else {
            continue;
        };
        let mut names: Vec<(String, bool)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let is_dir = e.file_type().ok()?.is_dir();
                Some((name, is_dir))
            })
            .filter(|(name, is_dir)| {
                !SKIPPED.contains(&name.as_str()) && !ignored.skips(name, *is_dir)
            })
            .collect();
        names.sort();
        for (name, is_dir) in names {
            let path = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if is_dir {
                dirs.push(path);
            } else if out.len() < limit {
                out.push(path);
            }
        }
        if out.len() >= limit {
            break;
        }
    }
    out.sort();
    out
}

/// What a top `.gitignore` says by name: plain names, folders (`name/`) and extensions (`*.ext`);
/// anything with more pattern than that is not read.
#[derive(Default)]
struct Ignore {
    names: HashSet<String>,
    dirs: HashSet<String>,
    exts: HashSet<String>,
}

impl Ignore {
    fn parse(text: &str) -> Self {
        let mut out = Self::default();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            let line = line.trim_start_matches('/');
            if let Some(ext) = line.strip_prefix("*.").filter(|e| is_plain(e)) {
                out.exts.insert(ext.to_owned());
            } else if let Some(dir) = line.strip_suffix('/').filter(|d| is_plain(d)) {
                out.dirs.insert(dir.to_owned());
            } else if is_plain(line) {
                out.names.insert(line.to_owned());
            }
        }
        out
    }

    fn skips(&self, name: &str, dir: bool) -> bool {
        self.names.contains(name)
            || (dir && self.dirs.contains(name))
            || (!dir
                && name
                    .rsplit_once('.')
                    .is_some_and(|(_, ext)| self.exts.contains(ext)))
    }
}

/// A name with no pattern in it.
fn is_plain(s: &str) -> bool {
    !s.is_empty() && !s.contains(['*', '?', '[', '/'])
}

/// A row of Code's tree: a folder (open or not) or a file, at its depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    pub name: String,
    pub depth: usize,
    /// A folder, and whether it is open.
    pub folder: Option<bool>,
}

/// The tree's rows for `files` (sorted, `/`-separated) with the folders in `open` unfolded:
/// each level its folders first, then its files.
pub fn rows(files: &[String], open: &HashSet<String>) -> Vec<Row> {
    let mut out = Vec::new();
    level(files, "", 0, open, &mut out);
    out
}

fn level(files: &[String], dir: &str, depth: usize, open: &HashSet<String>, out: &mut Vec<Row>) {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let mut folders: Vec<&str> = Vec::new();
    let mut here: Vec<&str> = Vec::new();
    for f in files.iter().filter_map(|f| f.strip_prefix(&prefix)) {
        match f.split_once('/') {
            Some((folder, _)) => {
                if folders.last() != Some(&folder) {
                    folders.push(folder);
                }
            }
            None => here.push(f),
        }
    }
    folders.dedup();
    for folder in folders {
        let mut path = format!("{prefix}{folder}");
        let mut name = folder.to_owned();
        // A folder that holds one folder and nothing else is one row with it, `main/kotlin/a`,
        // as Android Studio shows them; it opens as its last.
        while let Some(only) = only_folder(files, &path) {
            path = format!("{path}/{only}");
            name = format!("{name}/{only}");
        }
        let is_open = open.contains(&path);
        out.push(Row {
            path: path.clone(),
            name,
            depth,
            folder: Some(is_open),
        });
        if is_open {
            let inside: Vec<String> = files
                .iter()
                .filter(|f| f.starts_with(&format!("{path}/")))
                .cloned()
                .collect();
            level(&inside, &path, depth + 1, open, out);
        }
    }
    for name in here {
        out.push(Row {
            path: format!("{prefix}{name}"),
            name: name.to_owned(),
            depth,
            folder: None,
        });
    }
}

/// The one folder `dir` holds, when it holds nothing else.
fn only_folder<'a>(files: &'a [String], dir: &str) -> Option<&'a str> {
    let inside = format!("{dir}/");
    let mut only: Option<&str> = None;
    for f in files.iter().filter_map(|f| f.strip_prefix(inside.as_str())) {
        let (first, _) = f.split_once('/')?;
        match only {
            Some(o) if o != first => return None,
            _ => only = Some(first),
        }
    }
    only
}

/// The README a folder page renders: the first of the usual names among `files` directly in
/// `dir`.
pub fn readme<'a>(files: &'a [String], dir: &str) -> Option<&'a String> {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    [
        "README.md",
        "readme.md",
        "Readme.md",
        "README.markdown",
        "README",
        "README.txt",
    ]
    .iter()
    .find_map(|name| files.iter().find(|f| **f == format!("{prefix}{name}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_tree_lists_folders_first_and_opens_what_is_open() {
        let all = files(&[
            ".gitlab-ci.yml",
            "README.md",
            "core/a.kt",
            "core/net/b.kt",
            "money/Money.kt",
        ]);
        let names = |open: &[&str]| -> Vec<(String, usize)> {
            let open = open.iter().map(|s| s.to_string()).collect();
            rows(&all, &open)
                .into_iter()
                .map(|r| (r.name, r.depth))
                .collect()
        };
        assert_eq!(
            names(&[]),
            [
                ("core", 0),
                ("money", 0),
                (".gitlab-ci.yml", 0),
                ("README.md", 0)
            ]
            .map(|(n, d)| (n.to_owned(), d))
        );
        assert_eq!(
            names(&["core"]),
            [
                ("core", 0),
                ("net", 1),
                ("a.kt", 1),
                ("money", 0),
                (".gitlab-ci.yml", 0),
                ("README.md", 0)
            ]
            .map(|(n, d)| (n.to_owned(), d))
        );
    }

    #[test]
    fn a_folder_with_only_a_folder_in_it_is_one_row_with_it() {
        let all = files(&[
            "app/src/main/kotlin/a/B.kt",
            "app/src/main/kotlin/a/C.kt",
            "app/build.gradle",
            "lib/x/y/Z.kt",
        ]);
        let open = ["app", "app/src/main/kotlin/a"].map(String::from).into();
        let rows: Vec<(String, usize)> = rows(&all, &open)
            .into_iter()
            .map(|r| (r.name, r.depth))
            .collect();
        assert_eq!(
            rows,
            [
                ("app", 0),
                ("src/main/kotlin/a", 1),
                ("B.kt", 2),
                ("C.kt", 2),
                ("build.gradle", 1),
                ("lib/x/y", 0),
            ]
            .map(|(n, d)| (n.to_owned(), d))
        );
    }

    #[test]
    fn a_gitignore_is_read_by_name() {
        let ignore = Ignore::parse("# build\nbuild/\n*.log\n/secrets.txt\nsrc/**/gen\n!keep.log\n");
        assert!(ignore.skips("build", true));
        assert!(!ignore.skips("build", false));
        assert!(ignore.skips("debug.log", false));
        assert!(ignore.skips("secrets.txt", false));
        assert!(!ignore.skips("gen", true));
        assert!(!ignore.skips("Money.kt", false));
    }

    #[test]
    fn a_folder_page_finds_its_readme() {
        let all = files(&["docs/README.md", "README", "src/main.rs"]);
        assert_eq!(readme(&all, "").map(String::as_str), Some("README"));
        assert_eq!(
            readme(&all, "docs").map(String::as_str),
            Some("docs/README.md")
        );
        assert_eq!(readme(&all, "src"), None);
    }

    #[test]
    fn a_plain_folder_is_walked_without_what_it_ignores() {
        let dir = std::env::temp_dir().join(format!("gitlance-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for f in [
            "notes/a.md",
            "build/out.bin",
            "README.md",
            "x.log",
            ".git/config",
        ] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        std::fs::write(dir.join(".gitignore"), "build/\n*.log\n").unwrap();
        assert_eq!(
            on_disk(&dir, 100),
            files(&[".gitignore", "README.md", "notes/a.md"])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
