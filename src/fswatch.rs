//! What the file system says changed under a working tree (#87), so git is asked only then: a
//! status of a large tree (an Android project with its build outputs) costs a second of CPU, and
//! asked every two seconds it kept a core busy. Changes in what git ignores, and in `.git` but its
//! index, `HEAD` and refs, do not count. Where the platform cannot watch a whole tree cheaply
//! (`platform::WATCHES_TREES`), or the watch fails, every look counts as a change.

use notify::{EventHandler, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Changes {
    dirty: Arc<AtomicBool>,
    watcher: Option<notify::RecommendedWatcher>,
}

impl Changes {
    /// Starts watching the working tree at `root`; the first look always counts.
    pub fn watch(root: &Path) -> Changes {
        let dirty = Arc::new(AtomicBool::new(true));
        let watcher = crate::platform::WATCHES_TREES
            .then(|| {
                let handler = Handler {
                    dirty: dirty.clone(),
                    root: root.to_path_buf(),
                    repo: git2::Repository::open(root).ok(),
                };
                let mut watcher = notify::recommended_watcher(handler).ok()?;
                watcher.watch(root, RecursiveMode::Recursive).ok()?;
                Some(watcher)
            })
            .flatten();
        Changes { dirty, watcher }
    }

    /// Whether something changed since the last look; without a watch, always.
    pub fn take(&self) -> bool {
        self.watcher.is_none() || self.dirty.swap(false, Ordering::Relaxed)
    }
}

struct Handler {
    dirty: Arc<AtomicBool>,
    root: PathBuf,
    /// For what git ignores; none, everything counts.
    repo: Option<git2::Repository>,
}

impl EventHandler for Handler {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        let counts = match &event {
            // A watch that lost track (an overflow): look again.
            Err(_) => true,
            Ok(event) => event.paths.is_empty() || event.paths.iter().any(|p| self.counts(p)),
        };
        if counts {
            self.dirty.store(true, Ordering::Relaxed);
        }
    }
}

impl Handler {
    fn counts(&self, path: &Path) -> bool {
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return true;
        };
        if let Ok(git) = rel.strip_prefix(".git") {
            return matters_in_git(git);
        }
        match &self.repo {
            Some(repo) => !repo.is_path_ignored(rel).unwrap_or(false),
            None => true,
        }
    }
}

/// A change inside `.git` that moves what the working tree is compared with.
fn matters_in_git(rel: &Path) -> bool {
    rel == Path::new("index") || rel == Path::new("HEAD") || rel.starts_with("refs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_index_head_and_refs_count_inside_git() {
        assert!(matters_in_git(Path::new("index")));
        assert!(matters_in_git(Path::new("HEAD")));
        assert!(matters_in_git(Path::new("refs/heads/main")));
        assert!(!matters_in_git(Path::new("objects/ab/cdef")));
        assert!(!matters_in_git(Path::new("logs/HEAD")));
        assert!(!matters_in_git(Path::new("index.lock")));
    }
}
