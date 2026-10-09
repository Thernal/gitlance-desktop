//! Auto-refresh: notice that the repository changed under the window (a commit, an amend, a rebase,
//! a checkout made elsewhere) and re-read it, without moving what the user is looking at. It only
//! reads — fetching is a separate, opt-in feature.

use super::{COMMIT_LIMIT, Selection, Workspace, theme};
use crate::git::{self, BranchRef, CommitInfo, Repo, Version};
use gpui::{ClickEvent, Context, Rgba, Task, div, prelude::*, px};
use std::path::PathBuf;
use std::time::Duration;

/// How often the git directory is checked.
const POLL: Duration = Duration::from_millis(1500);
/// A change is acted on once it has been still for this long, so a rebase is one refresh.
const SETTLE: Duration = Duration::from_millis(300);
/// How long "Repository changed" stays in the title bar.
const FLASH: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    UpToDate,
    Changed,
    Reloaded,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::UpToDate => "Up to date",
            Status::Changed => "Repository changed",
            Status::Reloaded => "Reloaded",
        }
    }

    fn color(self) -> Rgba {
        match self {
            Status::UpToDate => theme::added(),
            Status::Changed => theme::accent(),
            Status::Reloaded => theme::warning(),
        }
    }
}

/// What a refresh found on the open branch, held back until the user asks for it.
pub struct Pending {
    commits: Vec<CommitInfo>,
    versions: Vec<Version>,
    /// New commits on top of the ones shown; `None` when the branch was rewritten.
    new: Option<usize>,
}

pub struct Watch {
    git_dir: Option<PathBuf>,
    fingerprint: u64,
    pub status: Status,
    pub pending: Option<Pending>,
    /// Versions from this index on arrived since the user last looked.
    pub new_from: Option<usize>,
    task: Option<Task<()>>,
    flash: Option<Task<()>>,
}

impl Default for Watch {
    fn default() -> Self {
        Self {
            git_dir: None,
            fingerprint: 0,
            status: Status::UpToDate,
            pending: None,
            new_from: None,
            task: None,
            flash: None,
        }
    }
}

impl Watch {
    /// A repository was opened: watch its git directory from this state.
    pub fn watch(&mut self, git_dir: PathBuf, fingerprint: u64) {
        self.git_dir = Some(git_dir);
        self.fingerprint = fingerprint;
        self.status = Status::UpToDate;
        self.forget();
    }

    /// The user moved to another branch: what was found on the old one no longer applies.
    pub fn forget(&mut self) {
        self.pending = None;
        self.new_from = None;
        self.task = None;
    }
}

impl Workspace {
    /// Starts the polling loop for the life of the window.
    pub(super) fn start_watching(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let Ok(job) = this.update(cx, |this, _| this.watch_job()) else {
                    return;
                };
                let Some((dir, known)) = job else {
                    continue;
                };
                let seen = {
                    let dir = dir.clone();
                    cx.background_executor()
                        .spawn(async move { git::fingerprint(&dir) })
                        .await
                };
                if seen == known {
                    continue;
                }
                cx.background_executor().timer(SETTLE).await;
                let settled = cx
                    .background_executor()
                    .spawn(async move { git::fingerprint(&dir) })
                    .await;
                // Still changing: the next tick looks again.
                if settled == seen
                    && this
                        .update(cx, |this, cx| this.repository_changed(seen, cx))
                        .is_err()
                {
                    return;
                }
            }
        })
        .detach();
    }

    fn watch_job(&self) -> Option<(PathBuf, u64)> {
        if !self.settings.auto_refresh {
            return None;
        }
        Some((self.watch.git_dir.clone()?, self.watch.fingerprint))
    }

    fn set_status(&mut self, status: Status, cx: &mut Context<Self>) {
        self.watch.status = status;
        self.watch.flash = (status != Status::UpToDate).then(|| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(FLASH).await;
                this.update(cx, |this, cx| {
                    this.watch.status = Status::UpToDate;
                    cx.notify();
                })
                .ok();
            })
        });
        cx.notify();
    }

    /// Re-reads the branches and the open branch's history and versions in the background.
    fn repository_changed(&mut self, fingerprint: u64, cx: &mut Context<Self>) {
        self.watch.fingerprint = fingerprint;
        let Some(root) = self.root.clone() else {
            return;
        };
        self.set_status(Status::Changed, cx);
        let refname = self
            .branch
            .and_then(|ix| self.branches.get(ix))
            .map(|b| b.refname.clone());
        self.watch.task = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let repo = Repo::open(&root)?;
                    let branches = repo.branches()?;
                    let tip = refname
                        .as_ref()
                        .and_then(|r| branches.iter().find(|b| &b.refname == r))
                        .map(|b| b.tip);
                    let open = match (&refname, tip) {
                        (Some(r), Some(tip)) => Some((
                            r.clone(),
                            repo.versions(r).unwrap_or_default(),
                            repo.log(tip, COMMIT_LIMIT)?,
                        )),
                        _ => None,
                    };
                    anyhow::Ok((branches, refname, open, repo.decorations()))
                })
                .await;
            this.update(cx, |this, cx| match loaded {
                Ok((branches, refname, open, decor)) => {
                    this.decor = decor;
                    this.apply_reload(branches, refname, open, cx)
                }
                // Mid-rebase the repository can be unreadable for a moment; the next change retries.
                Err(_) => this.set_status(Status::UpToDate, cx),
            })
            .ok();
        }));
    }

    fn apply_reload(
        &mut self,
        branches: Vec<BranchRef>,
        refname: Option<String>,
        open: Option<(String, Vec<Version>, Vec<CommitInfo>)>,
        cx: &mut Context<Self>,
    ) {
        self.branches = branches;
        let Some(refname) = refname else {
            return self.set_status(Status::UpToDate, cx);
        };
        let Some((_, versions, commits)) = open else {
            // The open branch is gone: fall back to the checked-out one, and say so.
            self.error =
                Some(format!("{refname} no longer exists — showing the current branch.").into());
            let ix = self
                .branches
                .iter()
                .position(|b| b.is_head)
                .or((!self.branches.is_empty()).then_some(0));
            self.clear_branch();
            if let Some(ix) = ix {
                self.select_branch(ix, None, cx);
            }
            return self.set_status(Status::Reloaded, cx);
        };
        self.branch = self.branches.iter().position(|b| b.refname == refname);

        let old_first = self.commits.first().map(|c| c.id);
        let first_same = old_first == commits.first().map(|c| c.id);
        if first_same && versions.len() == self.versions.len() {
            self.watch.pending = None;
            return self.set_status(Status::UpToDate, cx);
        }
        let new = old_first.and_then(|id| commits.iter().position(|c| c.id == id));
        if first_same {
            // Only the reflog moved: a new version of the same history.
            let seen = self.versions.len();
            self.mark_new_versions(versions.len());
            self.versions = versions;
            self.follow_latest_version(seen, cx);
            return self.set_status(Status::Reloaded, cx);
        }
        self.watch.pending = Some(Pending {
            commits,
            versions,
            new,
        });
        self.set_status(Status::UpToDate, cx);
    }

    fn mark_new_versions(&mut self, len: usize) {
        if len > self.versions.len() {
            self.watch.new_from = Some(self.versions.len());
        }
    }

    /// Shows what a refresh found: the new commits at the top, the selection and scroll kept.
    pub(super) fn apply_pending(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.watch.pending.take() else {
            return;
        };
        let selected = match self.selection {
            Selection::Commit(ix) => self.commits.get(ix).map(|c| c.id),
            _ => None,
        };
        let seen = self.versions.len();
        self.mark_new_versions(pending.versions.len());
        self.versions = pending.versions;
        self.commits = pending.commits;
        self.refilter();
        self.index_paths(cx);
        match (self.selection, selected) {
            (Selection::Commit(_), Some(id)) => {
                match self.commits.iter().position(|c| c.id == id) {
                    // The commit is still there, further down: only its index moved.
                    Some(ix) => self.selection = Selection::Commit(ix),
                    None => self.select_commit(0, cx),
                }
            }
            (Selection::Versions { from, to }, _) if to < self.versions.len() => {
                self.selection = Selection::Versions { from, to };
            }
            (Selection::WorkingTree, _) => {}
            _ => self.clear_diff(),
        }
        self.follow_latest_version(seen, cx);
        self.set_status(Status::Reloaded, cx);
    }

    /// A version comparison that ended at the latest version moves on to a newer one, in place:
    /// same file, same scroll position, and a notice that can undo it. Commits never need this —
    /// a commit's diff cannot change.
    fn follow_latest_version(&mut self, seen: usize, cx: &mut Context<Self>) {
        let Selection::Versions { from, to } = self.selection else {
            return;
        };
        let latest = self.versions.len().saturating_sub(1);
        if seen == 0 || to + 1 != seen || latest <= to {
            return;
        }
        let file = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(self.file))
            .map(|f| f.path().to_owned());
        if let Some(path) = file.clone() {
            self.restore_scroll = Some((path, self.diff_list.logical_scroll_top()));
        }
        let undo = self.selection;
        self.load_versions(from, latest, file, cx);
        // `load_versions` clears the notice of the diff it replaces; this one belongs to the new.
        self.notice = Some(super::Notice {
            text: format!(
                "v{} arrived — this comparison now ends at v{}.",
                self.versions[latest].number, self.versions[latest].number
            ),
            undo,
        });
    }

    /// Goes back to the comparison a notice replaced, at the same place in the file.
    pub(super) fn undo_notice(&mut self, cx: &mut Context<Self>) {
        let Some(notice) = self.notice.take() else {
            return;
        };
        let Selection::Versions { from, to } = notice.undo else {
            return;
        };
        let file = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(self.file))
            .map(|f| f.path().to_owned());
        if let Some(path) = file.clone() {
            self.restore_scroll = Some((path, self.diff_list.logical_scroll_top()));
        }
        self.load_versions(from, to, file, cx);
    }

    /// The title bar's quiet status.
    pub(super) fn render_status(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.settings.auto_refresh || self.root.is_none() {
            return None;
        }
        let status = self.watch.status;
        // Quiet while everything is fine: a status that is always on stops being read.
        if status == Status::UpToDate {
            return None;
        }
        Some(
            // A click refreshes, like ⌘R.
            div()
                .id("status")
                .flex()
                .items_center()
                .gap_2()
                .h(px(24.))
                .px_2()
                .rounded(px(super::ROW_RADIUS))
                .text_size(px(11.))
                .text_color(theme::muted())
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                .child(div().size(px(7.)).rounded_full().bg(status.color()))
                .child(status.label())
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.refresh(&super::Refresh, window, cx)
                })),
        )
    }

    /// "↑ 2 new commits · Show", above the commit list.
    pub(super) fn render_pill(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let pending = self.watch.pending.as_ref()?;
        let text = match pending.new {
            Some(1) => "1 new commit".to_owned(),
            Some(n) => format!("{n} new commits"),
            None => "The branch was rewritten".to_owned(),
        };
        Some(
            div()
                .id("pill")
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .mx(px(6.))
                .mb(px(6.))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(super::ROW_RADIUS))
                .bg(theme::info_bg())
                .text_size(px(11.))
                .text_color(theme::accent())
                .cursor_pointer()
                .hover(|s| s.bg(theme::selected()))
                .child("↑")
                .child(div().flex_1().child(text))
                .child(div().text_color(theme::muted()).child("Show"))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.apply_pending(cx))),
        )
    }
}
