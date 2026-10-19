use anyhow::Result;
use git2::{
    Delta, Diff, DiffFile, DiffFindOptions, DiffOptions, FileMode, Patch, Repository, Tree,
};
use std::sync::Arc;

/// Files larger than this are listed but not diffed line by line. Above 1 MiB a file is shown
/// without highlighting (`crate::highlight`), so this only bounds memory.
const MAX_TEXT_BYTES: usize = 16 << 20;
/// Changed images up to this size are kept to be shown; larger ones stay "Binary file".
const MAX_IMAGE_BYTES: usize = 24 << 20;

/// The image formats a diff shows as images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    Ico,
    Tiff,
    Svg,
}

impl ImageKind {
    /// By the file's extension.
    pub fn of(path: &str) -> Option<Self> {
        let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match ext.as_str() {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "gif" => Self::Gif,
            "webp" => Self::Webp,
            "bmp" => Self::Bmp,
            "ico" => Self::Ico,
            "tif" | "tiff" => Self::Tiff,
            "svg" => Self::Svg,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::Webp => "WebP",
            Self::Bmp => "BMP",
            Self::Ico => "ICO",
            Self::Tiff => "TIFF",
            Self::Svg => "SVG",
        }
    }
}

/// One side of a changed image: its bytes, and its size in pixels when it could be read.
#[derive(Clone, Debug)]
pub struct ImageSide {
    pub kind: ImageKind,
    pub bytes: Arc<[u8]>,
    pub size: Option<(u32, u32)>,
}

impl ImageSide {
    fn new(kind: ImageKind, bytes: Vec<u8>) -> Self {
        let size = match kind {
            ImageKind::Svg => svg_size(&String::from_utf8_lossy(&bytes)),
            _ => imagesize::blob_size(&bytes)
                .ok()
                .map(|s| (s.width as u32, s.height as u32)),
        };
        Self {
            kind,
            bytes: bytes.into(),
            size,
        }
    }
}

/// An SVG's size: its `width` and `height`, else its `viewBox`.
pub fn svg_size(text: &str) -> Option<(u32, u32)> {
    let start = text.find("<svg")?;
    let tag = &text[start..start + text[start..].find('>')?];
    let attr = |name: &str| {
        let at = tag.find(&format!(" {name}="))? + name.len() + 2;
        let quote = tag[at..].chars().next()?;
        let rest = &tag[at + 1..];
        Some(rest[..rest.find(quote)?].to_owned())
    };
    let number = |v: &str| {
        v.trim_end_matches("px")
            .trim()
            .parse::<f32>()
            .ok()
            .map(|n| n.round() as u32)
    };
    if let (Some(w), Some(h)) = (attr("width"), attr("height"))
        && let (Some(w), Some(h)) = (number(&w), number(&h))
    {
        return Some((w, h));
    }
    let view = attr("viewBox")?;
    let parts: Vec<f32> = view
        .split([' ', ','])
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect();
    (parts.len() == 4).then(|| (parts[2].round() as u32, parts[3].round() as u32))
}

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
    /// The two sides of a changed image (`None` for a side it does not have).
    pub old_image: Option<ImageSide>,
    pub new_image: Option<ImageSide>,
    /// The local images a changed markdown file shows, as each side has them (#11).
    pub pictures: Vec<Picture>,
}

/// An image a markdown file links to by a path in the repository, read at that side's version.
#[derive(Clone, Debug)]
pub struct Picture {
    /// Of the old side.
    pub old: bool,
    /// As the document writes it.
    pub src: String,
    pub image: ImageSide,
}

/// The versions a diff compares, to read the images a markdown file links to: the trees, and
/// whether the new side is the files on disk.
#[derive(Clone, Copy, Default)]
pub(super) struct Sides<'a> {
    pub old: Option<&'a Tree<'a>>,
    pub new: Option<&'a Tree<'a>>,
    pub disk: bool,
    /// The old side is an index (the working tree's not staged changes, a rebased version's
    /// replay): a document's images are read from it instead of `old`.
    pub old_index: Option<&'a git2::Index>,
}

impl FileDiff {
    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or_default()
    }

    /// Line `line` of a side (`old`) as GitLab counts it: its kind and both counters — for an
    /// added line, the old line that comes next; for a removed one, the new line that comes next.
    /// `None` when the line is in no hunk.
    /// GitLab's two counters for `line` of one side: both for an unchanged line (in a hunk, or
    /// between hunks, shifted by what the hunks above it added and removed), one for an added or
    /// removed line.
    pub fn line_pair(&self, old: bool, line: u32) -> (Option<u32>, Option<u32>) {
        let side = |l: &DiffLine| if old { l.old_line } else { l.new_line };
        for hunk in &self.hunks {
            if let Some(l) = hunk.lines.iter().find(|l| side(l) == Some(line)) {
                return match l.kind {
                    LineKind::Context => (l.old_line, l.new_line),
                    LineKind::Added => (None, l.new_line),
                    LineKind::Removed => (l.old_line, None),
                };
            }
        }
        let shift: i64 = self
            .hunks
            .iter()
            .filter(|h| {
                h.lines
                    .iter()
                    .filter_map(side)
                    .max()
                    .is_some_and(|l| l < line)
            })
            .flat_map(|h| &h.lines)
            .map(|l| match l.kind {
                LineKind::Added => 1,
                LineKind::Removed => -1,
                LineKind::Context => 0,
            })
            .sum();
        let other = if old {
            i64::from(line) + shift
        } else {
            i64::from(line) - shift
        };
        let other = u32::try_from(other).ok().filter(|&n| n >= 1);
        if old {
            (Some(line), other)
        } else {
            (other, Some(line))
        }
    }

    pub fn position(&self, old: bool, line: u32) -> Option<DiffLine> {
        let side = |l: &DiffLine| if old { l.old_line } else { l.new_line };
        for hunk in &self.hunks {
            let Some(at) = hunk.lines.iter().position(|l| side(l) == Some(line)) else {
                continue;
            };
            let found = hunk.lines[at];
            // The counter of the other side: the next line that has it, or one past the last.
            let next = |get: fn(&DiffLine) -> Option<u32>| {
                hunk.lines[at..]
                    .iter()
                    .find_map(get)
                    .or_else(|| hunk.lines[..at].iter().rev().find_map(get).map(|n| n + 1))
                    .unwrap_or(0)
            };
            return Some(DiffLine {
                kind: found.kind,
                old_line: found.old_line.or_else(|| Some(next(|l| l.old_line))),
                new_line: found.new_line.or_else(|| Some(next(|l| l.new_line))),
            });
        }
        None
    }
}

/// How a diff is computed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffSettings {
    /// Lines that differ only in whitespace count as unchanged (`git diff -w`).
    pub ignore_whitespace: bool,
}

pub(super) fn options(settings: DiffSettings) -> DiffOptions {
    let mut opts = DiffOptions::new();
    opts.context_lines(3)
        .ignore_submodules(false)
        .ignore_whitespace(settings.ignore_whitespace);
    opts
}

pub(super) fn tree_to_tree(
    repo: &Repository,
    old: Option<&Tree<'_>>,
    new: Option<&Tree<'_>>,
    settings: DiffSettings,
) -> Result<Vec<FileDiff>> {
    let diff = repo.diff_tree_to_tree(old, new, Some(&mut options(settings)))?;
    collect(
        repo,
        diff,
        Sides {
            old,
            new,
            disk: false,
            old_index: None,
        },
    )
}

/// `sides.disk`: the new side may be a file on disk that has no blob yet.
pub(super) fn collect(
    repo: &Repository,
    mut diff: Diff<'_>,
    sides: Sides<'_>,
) -> Result<Vec<FileDiff>> {
    let workdir = sides.disk;
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

        let old = load(repo, delta.old_file(), false);
        let new = load(repo, delta.new_file(), workdir);
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
                let (_, count) = patch.hunk(h)?;
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
                hunks.push(Hunk { lines });
            }
        }

        let image = |file: DiffFile<'_>, path: &Option<String>, from_disk: bool| {
            let kind = ImageKind::of(path.as_deref()?)?;
            load_bytes(repo, file, from_disk).map(|bytes| ImageSide::new(kind, bytes))
        };
        let old_image = image(delta.old_file(), &old_path, false);
        let new_image = image(delta.new_file(), &new_path, workdir);
        let mut pictures = Vec::new();
        if let (Some(p), Content::Text(text)) = (&old_path, &old) {
            let from = match sides.old_index {
                Some(index) => Pictured::Index(index),
                None => Pictured::Tree(sides.old),
            };
            pictures.extend(read_pictures(repo, p, text, from, true));
        }
        if let (Some(p), Content::Text(text)) = (&new_path, &new) {
            let from = if sides.disk {
                Pictured::Disk
            } else {
                Pictured::Tree(sides.new)
            };
            pictures.extend(read_pictures(repo, p, text, from, false));
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
            old_image,
            new_image,
            pictures,
        });
    }
    Ok(files)
}

/// Where a side's images are read.
enum Pictured<'a> {
    Tree(Option<&'a Tree<'a>>),
    Index(&'a git2::Index),
    Disk,
}

/// The images markdown file `path` links to by a repository path, read from `from`; at most a
/// few dozen, none for another kind of file.
fn read_pictures(
    repo: &Repository,
    path: &str,
    text: &str,
    from: Pictured<'_>,
    old: bool,
) -> Vec<Picture> {
    if !crate::markdown::is_document(path) {
        return Vec::new();
    }
    let mut out: Vec<Picture> = Vec::new();
    for src in crate::markdown::images(text).into_iter().take(32) {
        if out.iter().any(|p| p.src == src) {
            continue;
        }
        let Some(at) = crate::markdown::local_path(path, &src) else {
            continue;
        };
        let Some(kind) = ImageKind::of(&at) else {
            continue;
        };
        let blob = |id: git2::Oid| {
            repo.find_blob(id)
                .ok()
                .filter(|b| b.size() <= MAX_IMAGE_BYTES)
                .map(|b| b.content().to_vec())
        };
        let bytes = match &from {
            Pictured::Disk => repo
                .workdir()
                .map(|w| w.join(&at))
                .filter(|f| std::fs::metadata(f).is_ok_and(|m| m.len() as usize <= MAX_IMAGE_BYTES))
                .and_then(|f| std::fs::read(f).ok()),
            Pictured::Tree(tree) => tree
                .and_then(|t| t.get_path(std::path::Path::new(&at)).ok())
                .and_then(|e| blob(e.id())),
            Pictured::Index(index) => index
                .get_path(std::path::Path::new(&at), 0)
                .and_then(|e| blob(e.id)),
        };
        if let Some(bytes) = bytes {
            out.push(Picture {
                old,
                src,
                image: ImageSide::new(kind, bytes),
            });
        }
    }
    out
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
            Content::TooLarge => "File too large to show (over 16 MB)",
            Content::Submodule => "Submodule",
            Content::Absent | Content::Text(_) => "",
        }
    }
}

fn load(repo: &Repository, file: DiffFile<'_>, from_disk: bool) -> Content {
    if file.id().is_zero() {
        return match (from_disk, repo.workdir(), file.path()) {
            (true, Some(dir), Some(path)) => read_file(&dir.join(path)),
            _ => Content::Absent,
        };
    }
    if file.mode() == FileMode::Commit {
        return Content::Submodule;
    }
    let Ok(blob) = repo.find_blob(file.id()) else {
        // A working-tree file whose blob git has not written.
        return match (from_disk, repo.workdir(), file.path()) {
            (true, Some(dir), Some(path)) => read_file(&dir.join(path)),
            _ => Content::Absent,
        };
    };
    if blob.is_binary() {
        Content::Binary
    } else if blob.size() > MAX_TEXT_BYTES {
        Content::TooLarge
    } else {
        Content::Text(String::from_utf8_lossy(blob.content()).into())
    }
}

/// The raw bytes of one side, for an image; `None` when absent or too large.
fn load_bytes(repo: &Repository, file: DiffFile<'_>, from_disk: bool) -> Option<Vec<u8>> {
    let from_file = || {
        let path = repo.workdir()?.join(file.path()?);
        let meta = std::fs::metadata(&path).ok()?;
        (meta.is_file() && meta.len() as usize <= MAX_IMAGE_BYTES)
            .then(|| std::fs::read(path).ok())
            .flatten()
    };
    if file.id().is_zero() {
        return from_disk.then(from_file).flatten();
    }
    match repo.find_blob(file.id()) {
        Ok(blob) => (blob.size() <= MAX_IMAGE_BYTES).then(|| blob.content().to_vec()),
        Err(_) => from_disk.then(from_file).flatten(),
    }
}

/// A file of the working tree as `Content`.
fn read_file(path: &std::path::Path) -> Content {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Content::Absent;
    };
    if !meta.is_file() {
        return Content::Absent;
    }
    if meta.len() as usize > MAX_TEXT_BYTES {
        return Content::TooLarge;
    }
    match std::fs::read(path) {
        Ok(bytes) if bytes.iter().take(8000).any(|b| *b == 0) => Content::Binary,
        Ok(bytes) => Content::Text(String::from_utf8_lossy(&bytes).into()),
        Err(_) => Content::Absent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_has_both_counters_as_gitlab_counts_them() {
        let l = |kind, old_line, new_line| DiffLine {
            kind,
            old_line,
            new_line,
        };
        use LineKind::*;
        let file = FileDiff {
            old_path: Some("a.rs".into()),
            new_path: Some("a.rs".into()),
            change: ChangeKind::Modified,
            added: 1,
            removed: 2,
            hunks: vec![Hunk {
                lines: vec![
                    l(Context, Some(4), Some(4)),
                    l(Removed, Some(5), None),
                    l(Removed, Some(6), None),
                    l(Added, None, Some(5)),
                    l(Context, Some(7), Some(6)),
                ],
            }],
            old_text: None,
            new_text: None,
            note: None,
            old_image: None,
            new_image: None,
            pictures: Vec::new(),
        };
        let pos = |old, line| file.position(old, line).map(|p| (p.old_line, p.new_line));
        assert_eq!(pos(false, 5), Some((Some(7), Some(5))));
        assert_eq!(pos(true, 6), Some((Some(6), Some(5))));
        assert_eq!(pos(false, 6), Some((Some(7), Some(6))));
        assert_eq!(file.position(false, 5).map(|p| p.kind), Some(Added));
        assert_eq!(pos(false, 40), None);
    }

    #[test]
    fn an_svg_is_sized_by_its_attributes_or_its_view_box() {
        assert_eq!(
            svg_size(r#"<svg width="24" height="16px" viewBox="0 0 48 32">"#),
            Some((24, 16))
        );
        assert_eq!(
            svg_size(r#"<?xml?><svg xmlns="x" viewBox="0 0 48 32">"#),
            Some((48, 32))
        );
        assert_eq!(svg_size("<svg>"), None);
    }

    #[test]
    fn images_are_known_by_extension() {
        assert_eq!(ImageKind::of("a/Logo.PNG"), Some(ImageKind::Png));
        assert_eq!(ImageKind::of("icon.svg"), Some(ImageKind::Svg));
        assert_eq!(ImageKind::of("main.rs"), None);
    }
}
