//! Versions of a rewritten branch, the way a merge request has versions per push.
//!
//! A ref's reflog records every tip it had on this machine: commits, amends, rebases, resets and
//! fetches. Consecutive tips that only fast-forward belong to one version; a tip that does not
//! descend from the previous one (amend, rebase, force push) starts the next.

use super::diff::{self, DiffSettings, FileDiff};
use anyhow::{Context as _, Result};
use git2::{IndexConflict, Oid, Repository};
use std::path::PathBuf;

/// Refs a branch is usually based on, best first.
const BASE_CANDIDATES: &[&str] = &[
    "refs/remotes/origin/HEAD",
    "refs/remotes/origin/main",
    "refs/remotes/origin/master",
    "refs/heads/main",
    "refs/heads/master",
];

/// The stage bits of an index entry's flags.
const INDEX_STAGE_MASK: u16 = 0x3000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    /// 1-based, oldest first.
    pub number: usize,
    pub tip: Oid,
    /// Where the branch forks from its base branch; `None` for a root commit.
    pub base: Option<Oid>,
    /// Commits from `base` (exclusive) to `tip`.
    pub commits: usize,
    /// When this machine last moved the ref to `tip`, seconds since the epoch.
    pub time: i64,
    /// The reflog message of that move: `commit (amend): …`, `rebase (finish): …`, `fetch: forced-update`.
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct VersionDiff {
    pub files: Vec<FileDiff>,
    /// The base moved between the versions, so the older one was rebased in memory first.
    pub rebased: bool,
    /// Paths whose in-memory rebase conflicted; they are compared against the older version as is.
    pub conflicts: Vec<String>,
}

pub(super) fn versions(repo: &Repository, refname: &str) -> Result<Vec<Version>> {
    let current = repo
        .find_reference(refname)
        .with_context(|| format!("no ref {refname}"))?
        .target();
    let reflog = repo.reflog(refname)?;

    // Oldest first; entries whose commits were pruned are skipped.
    let mut moves: Vec<(Oid, i64, String)> = reflog
        .iter()
        .rev()
        .filter(|e| !e.id_new().is_zero() && repo.find_commit(e.id_new()).is_ok())
        .map(|e| {
            (
                e.id_new(),
                e.committer().when().seconds(),
                String::from_utf8_lossy(e.message_bytes().unwrap_or_default()).into_owned(),
            )
        })
        .collect();
    if let Some(tip) = current
        && moves.last().is_none_or(|(id, ..)| *id != tip)
    {
        // A ref updated without a reflog entry still has a current version.
        let time = repo.find_commit(tip)?.committer().when().seconds();
        moves.push((tip, time, String::new()));
    }

    let mut ends: Vec<(Oid, i64, String)> = Vec::new();
    for next in moves {
        match ends.last_mut() {
            Some(last) if last.0 == next.0 => *last = next,
            Some(last) if repo.graph_descendant_of(next.0, last.0)? => *last = next,
            _ => ends.push(next),
        }
    }

    let targets: Vec<Oid> = BASE_CANDIDATES
        .iter()
        .filter_map(|name| repo.find_reference(name).ok()?.resolve().ok()?.target())
        .collect();
    ends.into_iter()
        .enumerate()
        .map(|(i, (tip, time, reason))| {
            let base = base_of(repo, tip, &targets)?;
            Ok(Version {
                number: i + 1,
                tip,
                base,
                commits: count_commits(repo, tip, base)?,
                time,
                reason,
            })
        })
        .collect()
}

/// The newest fork point of `tip` with any base candidate; the first parent when `tip` is
/// itself on a base branch or none exists.
fn base_of(repo: &Repository, tip: Oid, targets: &[Oid]) -> Result<Option<Oid>> {
    let mut best: Option<Oid> = None;
    for &target in targets {
        let Ok(fork) = repo.merge_base(tip, target) else {
            continue;
        };
        if fork == tip {
            continue;
        }
        best = match best {
            Some(b) if !repo.graph_descendant_of(fork, b)? => Some(b),
            _ => Some(fork),
        };
    }
    if best.is_none() {
        let commit = repo.find_commit(tip)?;
        best = commit.parent_ids().next();
    }
    Ok(best)
}

fn count_commits(repo: &Repository, tip: Oid, base: Option<Oid>) -> Result<usize> {
    let mut walk = repo.revwalk()?;
    walk.push(tip)?;
    if let Some(base) = base {
        walk.hide(base)?;
    }
    Ok(walk.count())
}

/// `repo` must be a private handle: the in-memory object backend added here stays on it.
pub(super) fn version_diff(
    repo: &Repository,
    from: &Version,
    to: &Version,
    settings: DiffSettings,
) -> Result<VersionDiff> {
    let tree = |id: Oid| repo.find_commit(id).and_then(|c| c.tree());
    let from_tip = tree(from.tip)?;
    let to_tip = tree(to.tip)?;

    let (Some(from_base), Some(to_base)) = (from.base, to.base) else {
        return plain(repo, &from_tip, &to_tip, settings);
    };
    if from_base == to_base {
        return plain(repo, &from_tip, &to_tip, settings);
    }

    let odb = repo.odb()?;
    let _mempack = odb.add_new_mempack_backend(1000)?;

    // `from` replayed onto `to`'s base: to.base + (from.tip − from.base).
    let mut index = repo.merge_trees(&tree(from_base)?, &tree(to_base)?, &from_tip, None)?;
    let conflicted: Vec<IndexConflict> = index.conflicts()?.collect::<Result<_, _>>()?;
    let mut conflicts = Vec::with_capacity(conflicted.len());
    for conflict in conflicted {
        let Some(entry) = [&conflict.their, &conflict.our, &conflict.ancestor]
            .into_iter()
            .flatten()
            .next()
        else {
            continue;
        };
        let path = PathBuf::from(String::from_utf8_lossy(&entry.path).into_owned());
        index.conflict_remove(&path)?;
        if let Some(mut their) = conflict.their {
            their.flags &= !INDEX_STAGE_MASK;
            index.add(&their)?;
        }
        conflicts.push(path.to_string_lossy().into_owned());
    }

    let mut opts = diff::options(settings);
    opts.reverse(true);
    let changes = repo.diff_tree_to_index(Some(&to_tip), Some(&index), Some(&mut opts))?;
    let mut files = diff::collect(repo, changes)?;
    for file in &mut files {
        if conflicts.iter().any(|c| c == file.path()) {
            file.note.get_or_insert_with(|| {
                "Rebase conflict: compared with the older version as it was".to_owned()
            });
        }
    }
    Ok(VersionDiff {
        files,
        rebased: true,
        conflicts,
    })
}

fn plain(
    repo: &Repository,
    from: &git2::Tree<'_>,
    to: &git2::Tree<'_>,
    settings: DiffSettings,
) -> Result<VersionDiff> {
    Ok(VersionDiff {
        files: diff::tree_to_tree(repo, Some(from), Some(to), settings)?,
        rebased: false,
        conflicts: Vec::new(),
    })
}
