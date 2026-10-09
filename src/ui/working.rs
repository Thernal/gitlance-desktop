//! The working tree against `HEAD`: a pinned entry above the commits, kept current while an agent
//! works. Read-only — GitLance never stages or commits.

use super::{Diff, Header, Selection, WorkingState, Workspace, row, theme};
use crate::git::Repo;
use gpui::{Context, FontWeight, div, prelude::*, px};
use std::sync::Arc;
use std::time::Duration;

/// How often the working tree is looked at.
const POLL: Duration = Duration::from_secs(2);

impl Workspace {
    /// Starts looking at the working tree of the open repository.
    pub(super) fn start_working_poll(&mut self, cx: &mut Context<Self>) {
        self.wt = WorkingState {
            only_unstaged: self.wt.only_unstaged,
            ..WorkingState::default()
        };
        self.wt.task = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some(root)) = this.read_with(cx, |this, _| this.root.clone()) else {
                    return;
                };
                let summary = cx
                    .background_executor()
                    .spawn(async move { Repo::open(&root).and_then(|r| r.working_summary()) })
                    .await;
                if let Ok((count, hash)) = summary
                    && this
                        .update(cx, |this, cx| this.working_summary_arrived(count, hash, cx))
                        .is_err()
                {
                    return;
                }
                cx.background_executor().timer(POLL).await;
            }
        }));
    }

    fn working_summary_arrived(&mut self, count: usize, hash: u64, cx: &mut Context<Self>) {
        let changed = hash != self.wt.hash;
        self.wt.count = count;
        self.wt.hash = hash;
        if !changed {
            return;
        }
        if self.selection == Selection::WorkingTree {
            // Reload in place: the selected file stays.
            let keep = self.current_path();
            self.load_working_tree(keep, cx);
        }
        cx.notify();
    }

    pub(super) fn select_working_tree(&mut self, cx: &mut Context<Self>) {
        self.load_working_tree(None, cx);
    }

    pub(super) fn load_working_tree(&mut self, keep: Option<String>, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let only_unstaged = self.wt.only_unstaged;
        self.clear_diff();
        self.selection = Selection::WorkingTree;
        let settings = self.settings();
        self.load_diff(keep, cx, move || {
            let tree = Repo::open(&root)?.working_tree(settings, only_unstaged)?;
            Ok(Diff {
                header: Header::WorkingTree { only_unstaged },
                files: Arc::new(tree.files),
                pairs: Vec::new(),
                tags: tree.tags,
            })
        });
        cx.notify();
    }

    pub(super) fn set_only_unstaged(&mut self, on: bool, cx: &mut Context<Self>) {
        self.wt.only_unstaged = on;
        let keep = self.current_path();
        self.load_working_tree(keep, cx);
    }

    /// The pinned entry above the commit list.
    pub(super) fn render_working_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let selected = self.selection == Selection::WorkingTree;
        (self.wt.count > 0 || selected).then(|| {
            div().w_full().px(px(6.)).pb(px(4.)).child(
                row("working-tree", selected)
                    .h(px(34.))
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Working tree"),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(theme::warning())
                            .child(format!(
                                "{} {}",
                                self.wt.count,
                                if self.wt.count == 1 {
                                    "change"
                                } else {
                                    "changes"
                                }
                            )),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.select_working_tree(cx))),
            )
        })
    }
}

/// How a working-tree file is tagged in the file list.
pub(super) fn tag_color(tag: &str) -> gpui::Rgba {
    match tag {
        "staged" => theme::added(),
        "untracked" => theme::renamed(),
        _ => theme::warning(),
    }
}

/// The small word after a file's name.
pub(super) fn file_tag(tag: Option<&&'static str>) -> Option<gpui::Div> {
    tag.map(|t| {
        div()
            .flex_none()
            .text_size(px(10.))
            .text_color(tag_color(t))
            .child(*t)
    })
}
