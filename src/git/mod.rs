//! Read-only access to a repository: branches, history, commit diffs and the versions of a
//! rewritten branch. Nothing here changes the index, the working tree, refs, reflogs or objects.

mod diff;
mod versions;

#[cfg(test)]
mod tests;

pub use diff::{ChangeKind, FileDiff, LineKind};
#[cfg(test)]
pub use diff::{DiffLine, Hunk};
pub use versions::{Version, VersionDiff};

use anyhow::{Context as _, Result};
use git2::{BranchType, Oid, Repository, Sort};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Local,
    Remote,
}

#[derive(Clone, Debug)]
pub struct BranchRef {
    /// Short name: `main`, `origin/main`.
    pub name: String,
    /// Full name: `refs/heads/main`.
    pub refname: String,
    pub kind: RefKind,
    pub tip: Oid,
    pub is_head: bool,
}

#[derive(Clone, Debug)]
pub struct CommitInfo {
    pub id: Oid,
    pub summary: String,
    pub message: String,
    pub author: String,
    pub email: String,
    /// Author time, seconds since the epoch.
    pub time: i64,
}

pub struct Repo {
    inner: Repository,
}

impl Repo {
    /// Opens the repository that contains `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let inner = Repository::discover(path)
            .with_context(|| format!("no git repository at {}", path.display()))?;
        Ok(Self { inner })
    }

    /// The working directory, or the git directory of a bare repository.
    pub fn root(&self) -> PathBuf {
        self.inner
            .workdir()
            .unwrap_or_else(|| self.inner.path())
            .to_path_buf()
    }

    /// Local branches first, the checked-out one at the top; remote-tracking refs after them.
    pub fn branches(&self) -> Result<Vec<BranchRef>> {
        let mut out = Vec::new();
        for item in self.inner.branches(None)? {
            let (branch, kind) = item?;
            let reference = branch.get();
            let (Ok(refname), Some(tip)) = (reference.name(), reference.target()) else {
                // Symbolic refs such as `origin/HEAD` have no direct target.
                continue;
            };
            out.push(BranchRef {
                name: lossy(reference.shorthand_bytes()),
                refname: refname.to_owned(),
                kind: match kind {
                    BranchType::Local => RefKind::Local,
                    BranchType::Remote => RefKind::Remote,
                },
                tip,
                is_head: branch.is_head(),
            });
        }
        out.sort_by(|a, b| {
            (a.kind == RefKind::Remote, !a.is_head, &a.name).cmp(&(
                b.kind == RefKind::Remote,
                !b.is_head,
                &b.name,
            ))
        });
        Ok(out)
    }

    /// Up to `limit` commits reachable from `tip`, newest first.
    pub fn log(&self, tip: Oid, limit: usize) -> Result<Vec<CommitInfo>> {
        let mut walk = self.inner.revwalk()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        walk.push(tip)?;
        walk.take(limit).map(|id| self.commit(id?)).collect()
    }

    pub fn commit(&self, id: Oid) -> Result<CommitInfo> {
        let commit = self.inner.find_commit(id)?;
        let author = commit.author();
        Ok(CommitInfo {
            id,
            summary: commit.summary_bytes().map(lossy).unwrap_or_default(),
            message: lossy(commit.message_bytes()).trim_end().to_owned(),
            author: lossy(author.name_bytes()),
            email: lossy(author.email_bytes()),
            time: author.when().seconds(),
        })
    }

    /// What a commit changed against its first parent; a root commit against the empty tree.
    pub fn commit_diff(&self, id: Oid) -> Result<Vec<FileDiff>> {
        let commit = self.inner.find_commit(id)?;
        let new = commit.tree()?;
        let old = match commit.parent_count() {
            0 => None,
            _ => Some(commit.parent(0)?.tree()?),
        };
        diff::tree_to_tree(&self.inner, old.as_ref(), Some(&new))
    }

    /// The versions of a branch, oldest first, read from the ref's reflog.
    pub fn versions(&self, refname: &str) -> Result<Vec<Version>> {
        versions::versions(&self.inner, refname)
    }

    /// What changed from version `from` to version `to`, without the noise of a rebase.
    pub fn version_diff(&self, from: &Version, to: &Version) -> Result<VersionDiff> {
        // The virtual rebase writes merged blobs; a private handle with an in-memory object
        // backend keeps them out of the repository.
        let scratch = Repository::open(self.inner.path())?;
        versions::version_diff(&scratch, from, to)
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
