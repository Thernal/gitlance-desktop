//! Structural diffs through difftastic (`difft`), which compares syntax trees instead of lines:
//! reformatting and re-indentation are not changes, and changed tokens are marked exactly.

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Where Homebrew and Cargo put `difft` when PATH (an app launched from Finder) lacks them.
const FALLBACK_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin"];

/// Two versions of a file, line by line as difftastic aligned them.
#[derive(Debug, Default, PartialEq)]
pub struct Alignment {
    /// 1-based line numbers; `None` where a side has no counterpart.
    pub pairs: Vec<(Option<u32>, Option<u32>)>,
    /// Byte ranges of changed tokens, by 1-based line number.
    pub old_marks: HashMap<u32, Vec<Range<usize>>>,
    pub new_marks: HashMap<u32, Vec<Range<usize>>>,
    /// The language difftastic parsed, `Text` when it had no parser.
    pub language: String,
    /// `changed`, `unchanged`, `created` or `deleted`; a created or deleted file has no alignment.
    pub status: String,
}

/// `None` when difftastic is not installed.
pub fn diff(path: &str, old: &str, new: &str) -> Result<Option<Alignment>> {
    let Some(difft) = find_difft() else {
        return Ok(None);
    };
    let scratch = Scratch::new()?;
    let name = Path::new(path)
        .file_name()
        .map_or_else(|| "file".into(), |n| n.to_owned());
    let (old_path, new_path) = (
        scratch.0.join("a").join(&name),
        scratch.0.join("b").join(&name),
    );
    for (file, text) in [(&old_path, old), (&new_path, new)] {
        std::fs::create_dir_all(file.parent().expect("has a parent"))?;
        std::fs::write(file, text)?;
    }
    let output = Command::new(difft)
        .env("DFT_UNSTABLE", "yes")
        .args(["--display", "json", "--color", "never"])
        .arg(&old_path)
        .arg(&new_path)
        .output()
        .context("run difft")?;
    if !output.status.success() {
        bail!(
            "difft failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let report: Report = serde_json::from_slice(&output.stdout).context("read difft's json")?;
    Ok(Some(report.into_alignment(old, new)))
}

fn find_difft() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    on_path
        .into_iter()
        .chain(FALLBACK_DIRS.iter().map(PathBuf::from))
        .map(|dir| dir.join("difft"))
        .find(|p| p.is_file())
}

/// A private temporary directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gitlance-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Deserialize)]
struct Report {
    #[serde(default)]
    aligned_lines: Vec<(Option<u32>, Option<u32>)>,
    #[serde(default)]
    chunks: Vec<Vec<Entry>>,
    #[serde(default)]
    language: String,
    #[serde(default)]
    status: String,
}

#[derive(Deserialize)]
struct Entry {
    lhs: Option<Side>,
    rhs: Option<Side>,
}

#[derive(Deserialize)]
struct Side {
    line_number: u32,
    changes: Vec<Change>,
}

#[derive(Deserialize)]
struct Change {
    start: usize,
    end: usize,
}

impl Report {
    fn into_alignment(self, old: &str, new: &str) -> Alignment {
        let (old_count, new_count) = (line_count(old), line_count(new));
        let pairs = if matches!(self.status.as_str(), "created" | "deleted") {
            Vec::new()
        } else if self.aligned_lines.is_empty() {
            // Unchanged: difftastic sends no alignment.
            (1..=old_count.min(new_count))
                .map(|n| (Some(n), Some(n)))
                .collect()
        } else {
            self.aligned_lines
                .into_iter()
                .map(|(a, b)| {
                    (
                        a.map(|a| a + 1).filter(|&a| a <= old_count),
                        b.map(|b| b + 1).filter(|&b| b <= new_count),
                    )
                })
                .filter(|pair| *pair != (None, None))
                .collect()
        };
        let mut alignment = Alignment {
            pairs,
            language: self.language,
            status: self.status,
            ..Alignment::default()
        };
        for entry in self.chunks.into_iter().flatten() {
            for (side, marks) in [
                (entry.lhs, &mut alignment.old_marks),
                (entry.rhs, &mut alignment.new_marks),
            ] {
                let Some(side) = side else { continue };
                let ranges = marks.entry(side.line_number + 1).or_default();
                ranges.extend(side.changes.into_iter().map(|c| c.start..c.end));
                ranges.sort_by_key(|r| r.start);
            }
        }
        alignment
    }
}

fn line_count(text: &str) -> u32 {
    text.split_inclusive('\n').count() as u32
}

#[cfg(test)]
#[allow(
    clippy::single_range_in_vec_init,
    reason = "lists of one byte range are what the code under test takes"
)]
mod tests {
    use super::*;

    #[test]
    fn reads_alignment_and_marks() {
        let json = r#"{"aligned_lines":[[0,0],[1,1],[null,2],[2,3],[3,4]],
            "chunks":[[{"lhs":{"line_number":1,"changes":[{"start":4,"end":5,"content":"1","highlight":"normal"}]},
                        "rhs":{"line_number":1,"changes":[{"start":4,"end":5,"content":"2","highlight":"normal"}]}}]],
            "language":"Rust","path":"a.rs","status":"changed"}"#;
        let report: Report = serde_json::from_str(json).unwrap();
        let alignment = report.into_alignment("a\nx = 1\nb\n", "a\nx = 2\n// c\nb\n");
        assert_eq!(
            alignment.pairs,
            [
                (Some(1), Some(1)),
                (Some(2), Some(2)),
                (None, Some(3)),
                (Some(3), Some(4))
            ]
        );
        assert_eq!(alignment.old_marks[&2], [4..5]);
        assert_eq!(alignment.new_marks[&2], [4..5]);
        assert_eq!(alignment.language, "Rust");
    }

    #[test]
    fn unchanged_files_pair_every_line() {
        let report: Report =
            serde_json::from_str(r#"{"language":"Rust","path":"a.rs","status":"unchanged"}"#)
                .unwrap();
        let alignment = report.into_alignment("a\nb\n", "a\nb\n");
        assert_eq!(alignment.pairs, [(Some(1), Some(1)), (Some(2), Some(2))]);
    }

    #[test]
    fn created_files_have_no_alignment() {
        let report: Report =
            serde_json::from_str(r#"{"language":"Rust","path":"a.rs","status":"created"}"#)
                .unwrap();
        let alignment = report.into_alignment("", "a\nb\n");
        assert!(alignment.pairs.is_empty());
        assert_eq!(alignment.status, "created");
    }

    #[test]
    fn runs_difft_when_installed() {
        let Ok(Some(alignment)) = diff("m.rs", "fn a() { 1 }\n", "fn a() { 2 }\n") else {
            return; // difftastic is optional.
        };
        assert_eq!(alignment.pairs, [(Some(1), Some(1))]);
        assert!(alignment.new_marks.contains_key(&1));
    }
}
