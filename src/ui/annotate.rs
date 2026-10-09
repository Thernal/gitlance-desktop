//! Annotate (⌥⌘B): beside each run of lines, who wrote it and when; a click opens the commit.
//! Read from `git blame` as of the revision on screen, in the background. Designed in
//! `../Design/mockups/ide-ideas/a-ideas.html` (2).

use super::diff_view::AnnCell;
use super::{Workspace, format};
use crate::git::Repo;
use gpui::{Context, SharedString};

impl Workspace {
    pub(super) fn toggle_annotate(&mut self, cx: &mut Context<Self>) {
        self.annotate = !self.annotate;
        self.annotations.clear();
        self.refresh_annotations(cx);
        cx.notify();
    }

    /// Reads who wrote the lines of the open file, when the column is on.
    pub(super) fn refresh_annotations(&mut self, cx: &mut Context<Self>) {
        self.annot_task = None;
        if !self.annotate {
            return;
        }
        let (Some(root), Some(rev), Some(path)) =
            (self.root.clone(), self.viewed_rev(), self.current_path())
        else {
            return;
        };
        // A deleted file has no new side to annotate.
        if self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(self.file))
            .is_none_or(|f| f.new_path.is_none())
        {
            return;
        }
        self.annot_task = Some(cx.spawn(async move |this, cx| {
            let lines = cx
                .background_executor()
                .spawn(async move { Repo::open(&root)?.blame(rev, &path) })
                .await;
            this.update(cx, |this, cx| {
                if let Ok(lines) = lines {
                    let mut previous = None;
                    this.annotations = lines
                        .into_iter()
                        .map(|line| {
                            let line = line?;
                            let first = previous != Some(line.id);
                            previous = Some(line.id);
                            Some(AnnCell {
                                id: line.id,
                                label: first.then(|| {
                                    // The age first: the name may be long, the age is what scans.
                                    let ago = format::ago(line.time);
                                    SharedString::from(format!(
                                        "{} · {}",
                                        ago.trim_end_matches(" ago"),
                                        line.author
                                    ))
                                }),
                                tip: SharedString::from(format!(
                                    "{} · {} · {} · {}\nclick to open the commit",
                                    line.summary,
                                    format::short(line.id),
                                    line.author,
                                    format::ago(line.time)
                                )),
                                current: line.id == rev,
                            })
                        })
                        .map(|c| {
                            c.unwrap_or(AnnCell {
                                id: rev,
                                label: None,
                                tip: SharedString::default(),
                                current: false,
                            })
                        })
                        .collect();
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// A click on an annotation: the commit that wrote line `line`, opened in the commits list.
    pub(super) fn open_annotated_commit(&mut self, line: u32, cx: &mut Context<Self>) {
        let Some(id) = line
            .checked_sub(1)
            .and_then(|i| self.annotations.get(i as usize))
            .map(|c| c.id)
        else {
            return;
        };
        match self.commits.iter().position(|c| c.id == id) {
            Some(ix) => {
                // The list may be filtered; show it as it is without the filter.
                self.select_commit(ix, cx);
            }
            None => {
                self.error = Some(
                    format!(
                        "{} is not in this list of commits (it is older, or on another branch).",
                        format::short(id)
                    )
                    .into(),
                );
                cx.notify();
            }
        }
    }
}
