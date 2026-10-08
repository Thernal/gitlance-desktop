use anyhow::Result;
use git2::{
    Delta, Diff, DiffFile, DiffFindOptions, DiffOptions, FileMode, Patch, Repository, Tree,
};
use std::sync::Arc;

/// Files larger than this are listed but not diffed line by line.
const MAX_TEXT_BYTES: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
    TypeChanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// One line of a hunk. Its text is the matching line of the file's old or new text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// 1-based.
    pub old_line: Option<u32>,
    /// 1-based.
    pub new_line: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Hunk {
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug)]
pub struct FileDiff {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub change: ChangeKind,
    pub added: usize,
    pub removed: usize,
    pub hunks: Vec<Hunk>,
    /// Whole old and new contents, for highlighting; `None` when absent, binary or too large.
    pub old_text: Option<Arc<str>>,
    pub new_text: Option<Arc<str>>,
    /// Why the content is not shown (binary, too large, submodule), or a caveat about it.
    pub note: Option<String>,
}

impl FileDiff {
    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or_default()
    }
}

pub(super) fn options() -> DiffOptions {
    let mut opts = DiffOptions::new();
    opts.context_lines(3).ignore_submodules(false);
    opts
}

pub(super) fn tree_to_tree(
    repo: &Repository,
    old: Option<&Tree<'_>>,
    new: Option<&Tree<'_>>,
) -> Result<Vec<FileDiff>> {
    let diff = repo.diff_tree_to_tree(old, new, Some(&mut options()))?;
    collect(repo, diff)
}

pub(super) fn collect(repo: &Repository, mut diff: Diff<'_>) -> Result<Vec<FileDiff>> {
    diff.find_similar(Some(DiffFindOptions::new().renames(true)))?;
    let mut files = Vec::with_capacity(diff.deltas().len());
    for idx in 0..diff.deltas().len() {
        let Some(patch) = Patch::from_diff(&diff, idx)? else {
            continue;
        };
        let delta = patch.delta();
        let change = match delta.status() {
            Delta::Added | Delta::Untracked => ChangeKind::Added,
            Delta::Deleted => ChangeKind::Deleted,
            Delta::Renamed => ChangeKind::Renamed,
            Delta::Copied => ChangeKind::Copied,
            Delta::Typechange => ChangeKind::TypeChanged,
            _ => ChangeKind::Modified,
        };
        let path = |file: DiffFile<'_>| file.path().map(|p| p.to_string_lossy().into_owned());
        let old_path = (change != ChangeKind::Added)
            .then(|| path(delta.old_file()))
            .flatten();
        let new_path = (change != ChangeKind::Deleted)
            .then(|| path(delta.new_file()))
            .flatten();

        let old = load(repo, delta.old_file());
        let new = load(repo, delta.new_file());
        let note = if delta.flags().is_binary() {
            Some(Content::Binary)
        } else {
            [old.clone(), new.clone()]
                .into_iter()
                .find(|c| !matches!(c, Content::Text(_) | Content::Absent))
        }
        .map(|c| c.reason().to_owned());

        let (_, added, removed) = patch.line_stats()?;
        let mut hunks = Vec::new();
        if note.is_none() {
            for h in 0..patch.num_hunks() {
                let (hunk, count) = patch.hunk(h)?;
                let mut lines = Vec::with_capacity(count);
                for l in 0..count {
                    let line = patch.line_in_hunk(h, l)?;
                    let kind = match line.origin() {
                        ' ' => LineKind::Context,
                        '+' => LineKind::Added,
                        '-' => LineKind::Removed,
                        _ => continue,
                    };
                    lines.push(DiffLine {
                        kind,
                        old_line: line.old_lineno(),
                        new_line: line.new_lineno(),
                    });
                }
                hunks.push(Hunk {
                    old_start: hunk.old_start(),
                    new_start: hunk.new_start(),
                    lines,
                });
            }
        }

        files.push(FileDiff {
            old_path,
            new_path,
            change,
            added,
            removed,
            hunks,
            old_text: old.text(),
            new_text: new.text(),
            note,
        });
    }
    Ok(files)
}

#[derive(Clone)]
enum Content {
    Absent,
    Text(Arc<str>),
    Binary,
    TooLarge,
    Submodule,
}

impl Content {
    fn text(self) -> Option<Arc<str>> {
        match self {
            Content::Text(text) => Some(text),
            _ => None,
        }
    }

    fn reason(&self) -> &'static str {
        match self {
            Content::Binary => "Binary file",
            Content::TooLarge => "File too large to show",
            Content::Submodule => "Submodule",
            Content::Absent | Content::Text(_) => "",
        }
    }
}

fn load(repo: &Repository, file: DiffFile<'_>) -> Content {
    if file.id().is_zero() {
        return Content::Absent;
    }
    if file.mode() == FileMode::Commit {
        return Content::Submodule;
    }
    let Ok(blob) = repo.find_blob(file.id()) else {
        return Content::Absent;
    };
    if blob.is_binary() {
        Content::Binary
    } else if blob.size() > MAX_TEXT_BYTES {
        Content::TooLarge
    } else {
        Content::Text(String::from_utf8_lossy(blob.content()).into())
    }
}
