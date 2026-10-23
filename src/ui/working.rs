//! The working tree against `HEAD`: a pinned entry above the commits, kept current while an agent
//! works. Read-only — GitLance never stages or commits.

use super::px;
use super::{Diff, Header, Selection, WorkingState, Workspace, theme};
use crate::git::Repo;
use gpui::{Context, div, prelude::*};
use std::sync::Arc;
use std::time::Duration;

/// How often the working tree is looked at.
const POLL: Duration = Duration::from_secs(2);
/// The longest wait while nothing changes.
const IDLE: Duration = Duration::from_secs(8);
/// How often a tab behind another looks.
const BEHIND: Duration = Duration::from_secs(10);
/// Git is asked at least this often, whatever the file system said.
const RECHECK: Duration = Duration::from_secs(60);

impl Workspace {
    /// Starts looking at the working tree of the open repository.
    pub(super) fn start_working_poll(&mut self, cx: &mut Context<Self>) {
        self.wt = WorkingState {
            only_unstaged: self.wt.only_unstaged,
            ..WorkingState::default()
        };
        let shown = self.on_screen.clone();
        let changes = self
            .root
            .as_deref()
            .filter(|_| !self.plain)
            .map(|root| std::rc::Rc::new(crate::fswatch::Changes::watch(root)));
        self.wt.changes = changes.clone();
        self.wt.task = Some(cx.spawn(async move |this, cx| {
            // Unchanged polls in a row: the next waits longer, up to `IDLE` (#87).
            let mut still = 0u32;
            let mut asked = std::time::Instant::now();
            let mut first = true;
            loop {
                // A tab behind another looks seldom; it looks again as it comes on screen.
                if !shown.get() {
                    cx.background_executor().timer(BEHIND).await;
                    if !shown.get() {
                        continue;
                    }
                }
                let Ok(Some((root, plain))) =
                    this.read_with(cx, |this, _| this.root.clone().map(|r| (r, this.plain)))
                else {
                    return;
                };
                // Git is asked when the file system says something changed, and at least once a
                // minute (#87).
                let changed = changes.as_ref().is_none_or(|c| c.take());
                let due = first || changed || asked.elapsed() >= RECHECK;
                first = false;
                // A plain folder has no git to ask; the agents' answers still come (#75).
                let summary = match plain {
                    true => Err(anyhow::anyhow!("not a git repository")),
                    false if !due => Err(anyhow::anyhow!("nothing changed")),
                    false => {
                        asked = std::time::Instant::now();
                        cx.background_executor()
                            .spawn(
                                async move { Repo::open(&root).and_then(|r| r.working_summary()) },
                            )
                            .await
                    }
                };
                if let Ok((count, hash)) = summary {
                    match this.update(cx, |this, cx| {
                        let changed = hash != this.wt.hash;
                        this.working_summary_arrived(count, hash, cx);
                        changed
                    }) {
                        Ok(true) => still = 0,
                        Ok(false) => still = still.saturating_add(1),
                        Err(_) => return,
                    }
                }
                // The agents' answers ride on the same beat, and pictures that came in.
                if this
                    .update(cx, |this, cx| {
                        this.poll_agents(cx);
                        if crate::avatars::take_fresh() {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    return;
                }
                cx.background_executor()
                    .timer((POLL * (1 + still.min(3))).min(IDLE))
                    .await;
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
        self.blames.working_changed();
        if self.nav.space == super::spaces::Space::Branches && self.at_root_list() {
            self.read_tree_files(cx);
        }
        // Code's tree of the working tree follows it (#75).
        if self.nav.space == super::spaces::Space::Code && self.code.at().is_none() {
            self.read_code_files(true, cx);
        }
        if self.selection == Selection::WorkingTree {
            // Reload in place: the selected file stays.
            let keep = self.current_path();
            self.load_working_tree(keep, cx);
        }
        cx.notify();
    }

    pub(super) fn select_working_tree(&mut self, cx: &mut Context<Self>) {
        self.arrive(super::nav::Page::WorkingTree);
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

    /// The working tree as a chip on the Commits label: how many changes, one click to open.
    pub(super) fn render_working_chip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let selected = self.selection == Selection::WorkingTree;
        (self.wt.count > 0 || selected).then(|| {
            super::commit_chip("working-tree", selected)
                .child("WT")
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(|_, cx| cx.new(|_| super::Tip("Working tree against HEAD")).into())
                .child(
                    div()
                        .text_color(theme::warning())
                        .child(self.wt.count.to_string()),
                )
                // In the Commits heading: picking it does not fold the section.
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.select_working_tree(cx)
                }))
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
