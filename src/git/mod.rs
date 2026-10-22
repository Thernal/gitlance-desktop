//! Read-only access to a repository: branches, history, commit diffs and the versions of a
//! rewritten branch. Nothing here changes the index, the working tree, refs, reflogs or objects.

mod diff;
mod versions;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub use diff::Hunk;
pub use diff::{ChangeKind, DiffLine, DiffSettings, FileDiff, ImageKind, ImageSide, LineKind};
pub use versions::{PairCommit, PairKind, RangePair, Version, VersionDiff};

use anyhow::{Context as _, Result};
use git2::{BranchType, Oid, Repository, Sort};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where a repository lives on the web: GitLab, GitHub or similar, from its clone URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebRemote {
    /// `https://host/group/project`, without a trailing `.git`.
    pub base: String,
    pub github: bool,
}

impl WebRemote {
    /// `git@host:group/project.git`, `ssh://git@host:port/group/project.git` and
    /// `https://[user@]host/group/project.git` all give `https://host/group/project`.
    pub fn parse(url: &str) -> Option<Self> {
        let url = url.trim();
        let (host, path) = if let Some(rest) = url.split_once("://").map(|(_, r)| r) {
            let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
            let (host, path) = rest.split_once('/')?;
            (host.split(':').next()?, path)
        } else {
            // scp-like: user@host:path
            let rest = url.rsplit_once('@').map_or(url, |(_, r)| r);
            rest.split_once(':')?
        };
        let path = path.trim_matches('/').trim_end_matches(".git");
        if host.is_empty() || !path.contains('/') {
            return None;
        }
        Some(Self {
            base: format!("https://{host}/{path}"),
            github: host.contains("github"),
        })
    }

    pub fn name(&self) -> &'static str {
        if self.github { "GitHub" } else { "GitLab" }
    }

    pub fn commit(&self, sha: &str) -> String {
        if self.github {
            format!("{}/commit/{sha}", self.base)
        } else {
            format!("{}/-/commit/{sha}", self.base)
        }
    }

    /// The page of `path` at revision `sha`, at `line`.
    pub fn blob(&self, sha: &str, path: &str, line: u32) -> String {
        if self.github {
            format!("{}/blob/{sha}/{path}#L{line}", self.base)
        } else {
            format!("{}/-/blob/{sha}/{path}#L{line}", self.base)
        }
    }

    /// `branch` as `main` or `origin/main`; a remote prefix is dropped.
    pub fn branch(&self, branch: &str) -> String {
        let name = branch.split_once('/').map_or(branch, |(_, rest)| rest);
        let name = if self.is_remote_prefix(branch) {
            name
        } else {
            branch
        };
        if self.github {
            format!("{}/tree/{name}", self.base)
        } else {
            format!("{}/-/tree/{name}", self.base)
        }
    }

    fn is_remote_prefix(&self, branch: &str) -> bool {
        branch.starts_with("origin/") || branch.starts_with("upstream/")
    }
}

/// A label on a commit: where a branch, a remote branch or a tag points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deco {
    pub name: String,
    pub kind: DecoKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecoKind {
    /// The checked-out branch: shown as `HEAD → name`.
    Head,
    Local,
    Remote,
    Tag,
}

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

/// Who wrote a line, and in which commit.
#[derive(Clone, Debug)]
pub struct BlameLine {
    pub id: Oid,
    pub author: String,
    pub time: i64,
    pub summary: String,
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

/// A branch or a tag, to compare.
#[derive(Clone, Debug)]
pub struct NamedRef {
    pub name: String,
    pub tip: Oid,
    pub tag: bool,
    /// A local branch behind its upstream.
    pub stale: Option<Stale>,
}

/// A local branch its upstream has moved past: comparing against it shows everyone else's work too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stale {
    /// `origin/develop`.
    pub upstream: String,
    pub tip: Oid,
    /// Commits the upstream has that the local branch does not.
    pub behind: usize,
}

/// Uncommitted work against `HEAD`.
pub struct WorkingTree {
    pub files: Vec<FileDiff>,
    /// Per path: `staged`, `partly staged`, `not staged` or `untracked`.
    pub tags: HashMap<String, &'static str>,
}

pub struct Comparison {
    pub files: Vec<FileDiff>,
    /// Where the comparison starts: the base itself or its merge base with the head.
    pub start: Oid,
    /// Commits the head has beyond `start`.
    pub commits: usize,
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

    /// The text of `path` in commit `rev`, read from the objects (no checkout); None when it is not
    /// there or not text. The new request's template and CODEOWNERS come from here.
    pub fn file_at(&self, rev: Oid, path: &str) -> Option<String> {
        let tree = self.inner.find_commit(rev).ok()?.tree().ok()?;
        let entry = tree.get_path(Path::new(path)).ok()?;
        let blob = self.inner.find_blob(entry.id()).ok()?;
        std::str::from_utf8(blob.content()).ok().map(str::to_owned)
    }

    /// What became of line `line` (1-based) of `path` from commit `old` to commit `new` (#46).
    pub fn follow_line(&self, old: Oid, new: Oid, path: &str, line: u32) -> Fate {
        let (Some(before), Some(after)) = (self.file_at(old, path), self.file_at(new, path)) else {
            return Fate::Gone;
        };
        follow(&before, &after, line)
    }

    /// The newest commit between `from` (left out) and `to` that changed `path`.
    pub fn last_change(&self, from: Oid, to: Oid, path: &str) -> Option<CommitInfo> {
        let commits = self.range_log(from, to, false).ok()?;
        let ids: Vec<Oid> = commits.iter().map(|c| c.id).collect();
        let touched = self.changed_paths(&ids);
        let id = touched
            .iter()
            .find(|(_, paths)| paths.iter().any(|p| p == path))
            .map(|(id, _)| *id)?;
        commits.into_iter().find(|c| c.id == id)
    }

    /// Every file of `HEAD`'s tree, by path (at most `limit`): search everywhere's Files tab.
    pub fn all_files(&self, limit: usize) -> Vec<String> {
        let Ok(tree) = self.inner.head().and_then(|h| h.peel_to_tree()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let _ = tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if out.len() >= limit {
                return git2::TreeWalkResult::Abort;
            }
            if entry.kind() == Some(git2::ObjectType::Blob)
                && let Ok(name) = entry.name()
            {
                out.push(format!("{dir}{name}"));
            }
            git2::TreeWalkResult::Ok
        });
        out
    }

    /// The names of the files directly in folder `dir` of commit `rev`.
    pub fn files_at(&self, rev: Oid, dir: &str) -> Vec<String> {
        let names = || -> Option<Vec<String>> {
            let tree = self.inner.find_commit(rev).ok()?.tree().ok()?;
            let sub = if dir.is_empty() {
                tree
            } else {
                let entry = tree.get_path(Path::new(dir)).ok()?;
                self.inner.find_tree(entry.id()).ok()?
            };
            Some(
                sub.iter()
                    .filter(|e| e.kind() == Some(git2::ObjectType::Blob))
                    .filter_map(|e| e.name().map(str::to_owned).ok())
                    .collect(),
            )
        };
        names().unwrap_or_default()
    }

    /// `user.name` from git's config: whose avatar the composer shows.
    pub fn user_name(&self) -> Option<String> {
        self.inner.config().ok()?.get_string("user.name").ok()
    }

    /// The working directory, or the git directory of a bare repository.
    pub fn root(&self) -> PathBuf {
        self.inner
            .workdir()
            .unwrap_or_else(|| self.inner.path())
            .to_path_buf()
    }

    /// The git directory, where refs and reflogs live.
    pub fn git_dir(&self) -> PathBuf {
        self.inner.path().to_path_buf()
    }

    /// The web address of the repository's remote (`origin`, else the first one), when it has one.
    pub fn web_remote(&self) -> Option<WebRemote> {
        let remotes = self.inner.remotes().ok()?;
        let names: Vec<&str> = remotes.iter().flatten().flatten().collect();
        let name = names
            .iter()
            .copied()
            .find(|n| *n == "origin")
            .or_else(|| names.first().copied())?;
        let remote = self.inner.find_remote(name).ok()?;
        WebRemote::parse(remote.url().ok()?)
    }

    /// The branches, remote branches and tags that point at each commit.
    pub fn decorations(&self) -> HashMap<Oid, Vec<Deco>> {
        let mut out: HashMap<Oid, Vec<Deco>> = HashMap::new();
        let head = self
            .inner
            .head()
            .ok()
            .and_then(|h| h.name().ok().map(str::to_owned));
        let Ok(refs) = self.inner.references() else {
            return out;
        };
        for reference in refs.flatten() {
            let (Ok(name), Ok(commit)) = (reference.name(), reference.peel_to_commit()) else {
                continue;
            };
            let short = lossy(reference.shorthand_bytes());
            let kind = if name.starts_with("refs/heads/") {
                if head.as_deref() == Some(name) {
                    DecoKind::Head
                } else {
                    DecoKind::Local
                }
            } else if name.starts_with("refs/remotes/") {
                // `origin/HEAD` is only a pointer to another remote branch.
                if short.ends_with("/HEAD") {
                    continue;
                }
                DecoKind::Remote
            } else if name.starts_with("refs/tags/") {
                DecoKind::Tag
            } else {
                continue;
            };
            out.entry(commit.id())
                .or_default()
                .push(Deco { name: short, kind });
        }
        for decos in out.values_mut() {
            decos.sort_by_key(|d| d.kind as u8);
        }
        out
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

    /// The files merging `source` into `target` would leave in conflict (empty: it merges
    /// cleanly), merged in memory; `None` when the two cannot be read.
    pub fn conflicts(&self, target: Oid, source: Oid) -> Option<Vec<String>> {
        let tree = |id: Oid| self.inner.find_commit(id).and_then(|c| c.tree()).ok();
        let base = self.inner.merge_base(target, source).ok()?;
        // Merged blobs go to memory, never to the repository's objects.
        let odb = self.inner.odb().ok()?;
        let _mempack = odb.add_new_mempack_backend(1000).ok()?;
        let index = self
            .inner
            .merge_trees(&tree(base)?, &tree(target)?, &tree(source)?, None)
            .ok()?;
        let mut paths: Vec<String> = index
            .conflicts()
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|c| c.our.or(c.their).or(c.ancestor))
            .map(|e| String::from_utf8_lossy(&e.path).into_owned())
            .collect();
        paths.dedup();
        Some(paths)
    }

    /// The paths each of `ids` touched against its first parent, without reading file contents.
    pub fn changed_paths(&self, ids: &[Oid]) -> Vec<(Oid, Vec<String>)> {
        let mut opts = git2::DiffOptions::new();
        opts.context_lines(0).skip_binary_check(true);
        ids.iter()
            .map(|&id| {
                let paths = (|| -> Result<Vec<String>> {
                    let commit = self.inner.find_commit(id)?;
                    let new = commit.tree()?;
                    let old = match commit.parent_count() {
                        0 => None,
                        _ => Some(commit.parent(0)?.tree()?),
                    };
                    let diff =
                        self.inner
                            .diff_tree_to_tree(old.as_ref(), Some(&new), Some(&mut opts))?;
                    Ok(diff
                        .deltas()
                        .flat_map(|d| [d.old_file().path(), d.new_file().path()])
                        .flatten()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect())
                })();
                (id, paths.unwrap_or_default())
            })
            .collect()
    }

    /// What a commit changed against its first parent; a root commit against the empty tree.
    /// What commit `id` changed against its first parent: (files, lines added, lines removed).
    pub fn commit_stat(&self, id: Oid) -> Result<(usize, usize, usize)> {
        let commit = self.inner.find_commit(id)?;
        let old = match commit.parent(0) {
            Ok(parent) => Some(parent.tree()?),
            Err(_) => None,
        };
        let diff = self
            .inner
            .diff_tree_to_tree(old.as_ref(), Some(&commit.tree()?), None)?;
        let stats = diff.stats()?;
        Ok((stats.files_changed(), stats.insertions(), stats.deletions()))
    }

    pub fn commit_diff(&self, id: Oid, settings: DiffSettings) -> Result<Vec<FileDiff>> {
        let commit = self.inner.find_commit(id)?;
        let new = commit.tree()?;
        let old = match commit.parent_count() {
            0 => None,
            _ => Some(commit.parent(0)?.tree()?),
        };
        diff::tree_to_tree(&self.inner, old.as_ref(), Some(&new), settings)
    }

    /// The changes of the working tree against `HEAD` (staged, not staged and untracked files);
    /// with `only_unstaged` only what changed since the last `git add`. Read-only.
    pub fn working_tree(&self, settings: DiffSettings, only_unstaged: bool) -> Result<WorkingTree> {
        if self.inner.workdir().is_none() {
            return Ok(WorkingTree {
                files: Vec::new(),
                tags: HashMap::new(),
            });
        }
        let head = self.inner.head().ok().and_then(|h| h.peel_to_tree().ok());
        let opts = |settings| {
            let mut o = diff::options(settings);
            o.include_untracked(true)
                .recurse_untracked_dirs(true)
                .show_untracked_content(true);
            o
        };
        // A document's images: on the old side from HEAD, or for the not staged changes alone
        // from the index they are compared with; on the new side from the disk.
        let sides = diff::Sides {
            old: head.as_ref(),
            new: None,
            disk: true,
            old_index: None,
        };
        let index = self.inner.index()?;
        let unstaged = diff::collect(
            &self.inner,
            self.inner
                .diff_index_to_workdir(None, Some(&mut opts(settings)))?,
            diff::Sides {
                old_index: Some(&index),
                ..sides
            },
        )?;
        let staged = self.inner.diff_tree_to_index(
            head.as_ref(),
            None,
            Some(&mut diff::options(settings)),
        )?;
        let staged_paths: std::collections::HashSet<String> = staged
            .deltas()
            .filter_map(|d| {
                d.new_file()
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
            })
            .collect();
        let untracked: std::collections::HashSet<String> = self
            .inner
            .statuses(Some(
                git2::StatusOptions::new()
                    .include_untracked(true)
                    .recurse_untracked_dirs(true)
                    .include_ignored(false),
            ))?
            .iter()
            .filter(|e| e.status().contains(git2::Status::WT_NEW))
            .filter_map(|e| e.path().ok().map(str::to_owned))
            .collect();
        let unstaged_paths: std::collections::HashSet<String> =
            unstaged.iter().map(|f| f.path().to_owned()).collect();

        let mut tags = HashMap::new();
        for path in staged_paths.iter().chain(&unstaged_paths) {
            let tag = match (
                staged_paths.contains(path),
                unstaged_paths.contains(path),
                untracked.contains(path),
            ) {
                (_, _, true) => "untracked",
                (true, true, _) => "partly staged",
                (true, false, _) => "staged",
                _ => "not staged",
            };
            tags.insert(path.clone(), tag);
        }
        let files = if only_unstaged {
            unstaged
        } else {
            diff::collect(
                &self.inner,
                self.inner
                    .diff_tree_to_workdir_with_index(head.as_ref(), Some(&mut opts(settings)))?,
                sides,
            )?
        };
        Ok(WorkingTree { files, tags })
    }

    /// How many paths changed in the working tree and a fingerprint of them (their status, size and
    /// modification time), to notice an edit without diffing; `(0, 0)` for a bare repository.
    pub fn working_summary(&self) -> Result<(usize, u64)> {
        use std::hash::{Hash, Hasher};
        let Some(dir) = self.inner.workdir() else {
            return Ok((0, 0));
        };
        let statuses = self.inner.statuses(Some(
            git2::StatusOptions::new()
                .include_untracked(true)
                .recurse_untracked_dirs(true)
                .include_ignored(false),
        ))?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut count = 0;
        for entry in statuses.iter() {
            count += 1;
            entry.path_bytes().hash(&mut hasher);
            entry.status().bits().hash(&mut hasher);
            if count <= 2000
                && let Ok(path) = entry.path()
                && let Ok(meta) = std::fs::metadata(dir.join(path))
            {
                meta.len().hash(&mut hasher);
                if let Ok(time) = meta.modified() {
                    time.hash(&mut hasher);
                }
            }
        }
        Ok((count, hasher.finish()))
    }

    /// Every branch and tag with the commit it points at, for picking a side to compare.
    pub fn compare_refs(&self) -> Result<Vec<NamedRef>> {
        let mut out: Vec<NamedRef> = self
            .branches()?
            .into_iter()
            .map(|b| NamedRef {
                stale: (b.kind == RefKind::Local)
                    .then(|| self.stale(&b.name))
                    .flatten(),
                name: b.name,
                tip: b.tip,
                tag: false,
            })
            .collect();
        for name in self.inner.tag_names(None)?.iter().flatten().flatten() {
            let tip = self
                .inner
                .revparse_single(&format!("refs/tags/{name}"))
                .and_then(|o| o.peel_to_commit())
                .map(|c| c.id());
            if let Ok(tip) = tip {
                out.push(NamedRef {
                    name: name.to_owned(),
                    tip,
                    tag: true,
                    stale: None,
                });
            }
        }
        Ok(out)
    }

    /// The project's main line, as a local branch name: what `origin/HEAD` points at, else `main`
    /// or `master`, here or on origin.
    pub fn default_branch(&self) -> Option<String> {
        let pointed = self
            .inner
            .find_reference("refs/remotes/origin/HEAD")
            .ok()
            .and_then(|r| r.symbolic_target().ok().flatten().map(str::to_owned))
            .and_then(|t| t.strip_prefix("refs/remotes/origin/").map(str::to_owned));
        pointed.or_else(|| {
            ["main", "master"]
                .into_iter()
                .find(|name| {
                    self.inner.find_branch(name, BranchType::Local).is_ok()
                        || self
                            .inner
                            .find_branch(&format!("origin/{name}"), BranchType::Remote)
                            .is_ok()
                })
                .map(str::to_owned)
        })
    }

    /// Commits `a` has that `b` has not, and the other way round.
    pub fn ahead_behind(&self, a: Oid, b: Oid) -> Option<(usize, usize)> {
        self.inner.graph_ahead_behind(a, b).ok()
    }

    /// A local branch's upstream and how far apart they are: (upstream, ahead, behind).
    pub fn tracking(&self, name: &str) -> Option<(String, usize, usize)> {
        let local = self.inner.find_branch(name, BranchType::Local).ok()?;
        let upstream = local.upstream().ok()?;
        let (tip, up) = (local.get().target()?, upstream.get().target()?);
        let (ahead, behind) = self.inner.graph_ahead_behind(tip, up).ok()?;
        Some((lossy(upstream.get().shorthand_bytes()), ahead, behind))
    }

    pub fn stale(&self, name: &str) -> Option<Stale> {
        let local = self.inner.find_branch(name, BranchType::Local).ok()?;
        let upstream = local.upstream().ok()?;
        let (tip, up) = (local.get().target()?, upstream.get().target()?);
        let (_, behind) = self.inner.graph_ahead_behind(tip, up).ok()?;
        (behind > 0).then(|| Stale {
            upstream: lossy(upstream.get().shorthand_bytes()),
            tip: up,
            behind,
        })
    }

    /// A branch, tag, `HEAD~3` or (part of) a commit id as a commit.
    /// Whether `commit` is `tip` or one of its ancestors.
    pub fn contains(&self, tip: Oid, commit: Oid) -> bool {
        tip == commit || self.inner.graph_descendant_of(tip, commit).unwrap_or(false)
    }

    /// The first parent of commit `id`.
    pub fn parent_of(&self, id: Oid) -> Result<Oid> {
        Ok(self.inner.find_commit(id)?.parent_id(0)?)
    }

    pub fn resolve(&self, spec: &str) -> Result<Oid> {
        Ok(self
            .inner
            .revparse_single(spec.trim())?
            .peel_to_commit()?
            .id())
    }

    /// The changes from `base` to `head`; with `since_merge_base` from where the two diverged,
    /// so only what `head` added shows (`base...head`).
    pub fn compare(
        &self,
        base: Oid,
        head: Oid,
        since_merge_base: bool,
        settings: DiffSettings,
    ) -> Result<Comparison> {
        let start = if since_merge_base {
            self.inner.merge_base(base, head).unwrap_or(base)
        } else {
            base
        };
        let old = self.inner.find_commit(start)?.tree()?;
        let new = self.inner.find_commit(head)?.tree()?;
        let files = diff::tree_to_tree(&self.inner, Some(&old), Some(&new), settings)?;
        let mut walk = self.inner.revwalk()?;
        walk.push(head)?;
        walk.hide(start)?;
        Ok(Comparison {
            files,
            start,
            commits: walk.count(),
        })
    }

    /// Who last wrote each line of `path` as of `rev`: one entry per line, 1-based line `n` at `n - 1`.
    pub fn blame(&self, rev: Oid, path: &str) -> Result<Vec<Option<BlameLine>>> {
        let mut opts = git2::BlameOptions::new();
        opts.newest_commit(rev);
        let blame = self.inner.blame_file(Path::new(path), Some(&mut opts))?;
        Ok(self.blame_lines(&blame))
    }

    /// The same for the working tree's `path`: a line not committed yet is `None`.
    pub fn blame_working(&self, path: &str) -> Result<Vec<Option<BlameLine>>> {
        let dir = self
            .inner
            .workdir()
            .context("a bare repository has no working tree")?;
        let text = std::fs::read(dir.join(path))?;
        let head = self.inner.blame_file(Path::new(path), None)?;
        Ok(self.blame_lines(&head.blame_buffer(&text)?))
    }

    fn blame_lines(&self, blame: &git2::Blame<'_>) -> Vec<Option<BlameLine>> {
        let mut summaries: std::collections::HashMap<Oid, String> =
            std::collections::HashMap::new();
        let mut out: Vec<Option<BlameLine>> = Vec::new();
        for hunk in blame.iter() {
            let id = hunk.final_commit_id();
            // Lines of a buffer that no commit has yet.
            if id.is_zero() {
                let end = hunk.final_start_line().saturating_sub(1) + hunk.lines_in_hunk();
                if out.len() < end {
                    out.resize(end, None);
                }
                continue;
            }
            let summary = summaries
                .entry(id)
                .or_insert_with(|| {
                    self.inner
                        .find_commit(id)
                        .ok()
                        .map(|c| c.summary_bytes().map(lossy).unwrap_or_default())
                        .unwrap_or_default()
                })
                .clone();
            let sig = hunk.final_signature();
            let line = BlameLine {
                id,
                author: sig
                    .as_ref()
                    .map(|s| lossy(s.name_bytes()))
                    .unwrap_or_default(),
                time: sig.as_ref().map_or(0, |s| s.when().seconds()),
                summary,
            };
            let start = hunk.final_start_line().saturating_sub(1);
            let end = start + hunk.lines_in_hunk();
            if out.len() < end {
                out.resize(end, None);
            }
            for slot in &mut out[start..end] {
                *slot = Some(line.clone());
            }
        }
        out
    }

    /// The commits `head` has on top of `base` (or of their merge base), newest first.
    pub fn range_log(
        &self,
        base: Oid,
        head: Oid,
        since_merge_base: bool,
    ) -> Result<Vec<CommitInfo>> {
        let start = if since_merge_base {
            self.inner.merge_base(base, head).unwrap_or(base)
        } else {
            base
        };
        let mut walk = self.inner.revwalk()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        walk.push(head)?;
        walk.hide(start)?;
        walk.map(|id| self.commit(id?)).collect()
    }

    /// Versions from a code host's record of pushes: `(head, base, seconds)`, oldest first.
    pub fn versions_from_pushes(&self, pushes: &[(Oid, Option<Oid>, i64)]) -> Vec<Version> {
        versions::from_pushes(&self.inner, pushes)
    }

    /// The versions of a branch, oldest first, read from the ref's reflog.
    pub fn versions(&self, refname: &str) -> Result<Vec<Version>> {
        versions::versions(&self.inner, refname)
    }

    /// What changed from version `from` to version `to`, without the noise of a rebase.
    pub fn version_diff(
        &self,
        from: &Version,
        to: &Version,
        settings: DiffSettings,
    ) -> Result<VersionDiff> {
        // The virtual rebase writes merged blobs; a private handle with an in-memory object
        // backend keeps them out of the repository.
        let scratch = Repository::open(self.inner.path())?;
        versions::version_diff(&scratch, from, to, settings)
    }

    /// The commits of two versions, paired the way `git range-diff` pairs them.
    pub fn range_pairs(&self, from: &Version, to: &Version) -> Result<Vec<RangePair>> {
        versions::range_pairs(&self.inner, from, to)
    }

    /// What one commit changed against its counterpart in another version.
    pub fn interdiff(&self, old: Oid, new: Oid, settings: DiffSettings) -> Result<VersionDiff> {
        let scratch = Repository::open(self.inner.path())?;
        versions::interdiff(&scratch, old, new, settings)
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A value that changes whenever a ref, `HEAD` or a reflog under `git_dir` does: file sizes and
/// modification times, hashed. Reads metadata only.
/// What became of a line from one text to the next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fate {
    /// The same text, at this line now.
    Same(u32),
    /// Rewritten: its line now and what it says.
    Changed(u32, String),
    /// Removed, or its file is gone.
    Gone,
}

/// Follows `line` of `before` into `after` through their line diff.
pub fn follow(before: &str, after: &str, line: u32) -> Fate {
    let mut opts = git2::DiffOptions::new();
    opts.context_lines(0);
    let Ok(patch) = git2::Patch::from_buffers(
        before.as_bytes(),
        None,
        after.as_bytes(),
        None,
        Some(&mut opts),
    ) else {
        return Fate::Same(line);
    };
    let mut delta: i64 = 0;
    for h in 0..patch.num_hunks() {
        let Ok((hunk, count)) = patch.hunk(h) else {
            continue;
        };
        let (old_start, old_lines) = (hunk.old_start(), hunk.old_lines());
        let new_lines = hunk.new_lines() as i64;
        if old_lines == 0 {
            // Only added lines, after `old_start`.
            if line <= old_start {
                break;
            }
            delta += new_lines;
            continue;
        }
        if line < old_start {
            break;
        }
        if line < old_start + old_lines {
            // Removed here: the added line in the same place, if any, is what it became.
            let k = (line - old_start) as usize;
            let added: Vec<(u32, String)> = (0..count)
                .filter_map(|l| patch.line_in_hunk(h, l).ok())
                .filter(|l| l.origin() == '+')
                .filter_map(|l| {
                    let text = String::from_utf8_lossy(l.content()).trim_end().to_owned();
                    l.new_lineno().map(|n| (n, text))
                })
                .collect();
            return match added.get(k).or_else(|| added.last()) {
                Some((n, text)) => Fate::Changed(*n, text.clone()),
                None => Fate::Gone,
            };
        }
        delta += new_lines - old_lines as i64;
    }
    Fate::Same((line as i64 + delta).max(1) as u32)
}

pub fn fingerprint(git_dir: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    fn visit(path: &Path, hasher: &mut impl Hasher) {
        let Ok(meta) = std::fs::metadata(path) else {
            return;
        };
        path.hash(hasher);
        meta.len().hash(hasher);
        meta.modified().ok().hash(hasher);
        if meta.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(path)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect();
            entries.sort();
            for entry in entries {
                visit(&entry, hasher);
            }
        }
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for name in ["HEAD", "packed-refs", "refs", "logs"] {
        visit(&git_dir.join(name), &mut hasher);
    }
    hasher.finish()
}
