//! Discussions written on an earlier version (Design #46 B, `docs/specs/outdated-discussions.md`):
//! a thread keeps the commit it was written on, and the line it was written on is followed version
//! by version to the latest (`git::Repo::follow_line`) — who changed it, in which commit, what it
//! says now. The latest diff draws such a thread at its line now, saying Changed or Unchanged since
//! your comment with the line's history, and Compare with the new version; a thread whose line is
//! gone stays only in the Discussions window, grouped by the version it was written on.

use super::{Workspace, format, px, theme};
use crate::git::{Fate, Repo};
use crate::mr::Thread;
use gpui::{AnyElement, Context, FontWeight, WeakEntity, div, prelude::*};
use std::collections::HashMap;

/// One later version that changed the line.
#[derive(Clone, Debug)]
pub struct Step {
    pub version: usize,
    pub who: String,
    pub summary: String,
    pub sha: String,
    /// What the line says in that version; None when it was removed.
    pub now: Option<String>,
}

/// What became of the line a discussion was written on.
#[derive(Clone, Debug)]
pub struct LineHistory {
    /// The version it was written on.
    pub written: usize,
    /// The line as it read then.
    pub then: String,
    pub steps: Vec<Step>,
    /// Where the line is in the latest version; None when it is gone.
    pub line_now: Option<u32>,
}

impl LineHistory {
    pub fn changed(&self) -> bool {
        !self.steps.is_empty()
    }
}

impl Workspace {
    /// The latest version's head of the open request.
    fn latest_head(&self) -> Option<String> {
        self.requests
            .current
            .and_then(|iid| self.requests.list.iter().find(|m| m.iid == iid))
            .map(|m| m.head.clone())
            .filter(|h| !h.is_empty())
    }

    /// Written on an earlier version than the one shown: its line number is that version's.
    pub(super) fn thread_is_old(&self, t: &Thread) -> bool {
        if t.outdated {
            return true;
        }
        match (&t.commit, self.latest_head()) {
            (Some(c), Some(head)) => !head.starts_with(c.as_str()) && !c.starts_with(&head),
            _ => false,
        }
    }

    /// Follows the line of every discussion written on an earlier version to the latest, in the
    /// background; the threads and the versions must both be in.
    pub(super) fn compute_histories(&mut self, cx: &mut Context<Self>) {
        let (Some(root), Some(iid)) = (self.root.clone(), self.requests.current) else {
            return;
        };
        let versions: Vec<(usize, git2::Oid)> = match &self.requests.versions {
            Some((for_iid, list)) if *for_iid == iid => {
                list.iter().map(|v| (v.number, v.tip)).collect()
            }
            _ => return,
        };
        let old: Vec<(String, String, String, u32)> = self
            .requests
            .threads
            .iter()
            .filter(|t| self.thread_is_old(t))
            .filter_map(|t| {
                Some((
                    t.id.clone(),
                    t.commit.clone()?,
                    t.path.clone()?,
                    t.new_line?,
                ))
            })
            .collect();
        if old.is_empty() {
            self.requests.histories.clear();
            return;
        }
        self.requests.history_task = Some(cx.spawn(async move |this, cx| {
            let histories = cx
                .background_executor()
                .spawn(async move {
                    let Ok(repo) = Repo::open(&root) else {
                        return HashMap::new();
                    };
                    old.into_iter()
                        .filter_map(|(id, commit, path, line)| {
                            let at = versions
                                .iter()
                                .position(|(_, tip)| tip.to_string().starts_with(&commit))?;
                            Some((id, history(&repo, &versions, at, &path, line)))
                        })
                        .collect::<HashMap<_, _>>()
                })
                .await;
            this.update(cx, |this, cx| {
                if this.requests.current == Some(iid) {
                    this.requests.histories = histories;
                    this.refresh_rows();
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// The outdated discussions by the version they were written on, newest version first.
    pub(super) fn outdated_by_version(&self) -> Vec<(Option<usize>, Vec<usize>)> {
        let mut groups: Vec<(Option<usize>, Vec<usize>)> = Vec::new();
        for (ix, t) in self.requests.threads.iter().enumerate() {
            if t.path.is_none() || !self.thread_is_old(t) {
                continue;
            }
            let version = self.requests.histories.get(&t.id).map(|h| h.written);
            match groups.iter_mut().find(|(v, _)| *v == version) {
                Some((_, list)) => list.push(ix),
                None => groups.push((version, vec![ix])),
            }
        }
        groups.sort_by_key(|(v, _)| std::cmp::Reverse(*v));
        groups
    }

    /// "Changed since your comment", or not, for thread `ix`; None while it is not known.
    pub(super) fn since_comment(&self, ix: usize) -> Option<&'static str> {
        let t = self.requests.threads.get(ix)?;
        let h = self.requests.histories.get(&t.id)?;
        Some(match (h.changed(), h.line_now) {
            (_, None) => "Line removed since your comment",
            (true, _) => "Changed since your comment",
            (false, _) => "Unchanged since your comment",
        })
    }

    /// Under an outdated discussion: the line's history and Compare with the new version.
    pub(super) fn render_history(&self, ix: usize, this: WeakEntity<Self>) -> Option<AnyElement> {
        let t = self.requests.threads.get(ix)?;
        let h = self.requests.histories.get(&t.id)?.clone();
        let changed = h.changed();
        let row = |tag: String, who: String, text: Option<String>, struck: bool| {
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .child(
                    div()
                        .w(px(28.))
                        .flex_none()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::accent())
                        .child(tag),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().text_color(theme::muted()).child(who))
                        .children(text.map(|t| {
                            div()
                                .px(px(8.))
                                .py(px(2.))
                                .rounded(px(4.))
                                .bg(if struck {
                                    theme::removed_line()
                                } else {
                                    theme::added_line()
                                })
                                .font_family(theme::CODE_FONT)
                                .text_size(px(12.))
                                .text_color(theme::text())
                                .truncate()
                                .child(t)
                        })),
                )
        };
        let mut rows: Vec<AnyElement> = vec![
            row(
                format!("v{}", h.written),
                "you commented on".to_owned(),
                Some(h.then.clone()),
                changed,
            )
            .into_any_element(),
        ];
        for s in &h.steps {
            rows.push(
                row(
                    format!("v{}", s.version),
                    if s.summary.is_empty() {
                        "changed".to_owned()
                    } else {
                        format!("{} · {} {}", s.who, s.sha, s.summary)
                    },
                    Some(s.now.clone().unwrap_or_else(|| "(removed)".to_owned())),
                    s.now.is_none(),
                )
                .into_any_element(),
            );
        }
        let written = h.written;
        let path = t.path.clone();
        let line_now = h.line_now;
        Some(
            div()
                .mx(px(12.))
                .mb(px(8.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .bg(theme::base())
                .flex()
                .flex_col()
                .gap(px(8.))
                .text_size(px(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .px(px(8.))
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .rounded_full()
                                .bg(theme::warning_bg())
                                .text_color(theme::warning())
                                .child(self.since_comment(ix).unwrap_or_default()),
                        )
                        .child(
                            div()
                                .text_color(theme::faint())
                                .child(format!("written on v{written}")),
                        )
                        .child(div().flex_1())
                        .children(changed.then(|| {
                            div()
                                .id(("compare-since", ix))
                                .px(px(8.))
                                .h(px(24.))
                                .flex()
                                .items_center()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(theme::island_border())
                                .cursor_pointer()
                                .hover(|s| s.bg(theme::hover()))
                                .child("Compare with the new version")
                                .on_click(move |_, _, cx| {
                                    let path = path.clone();
                                    this.update(cx, |this, cx| {
                                        this.compare_since(written, path, line_now, cx)
                                    })
                                    .ok();
                                })
                        })),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    /// The comment's version against the latest (a discussion whose line is gone).
    pub(super) fn compare_since_written(&mut self, written: usize, cx: &mut Context<Self>) {
        self.compare_since(written, None, None, cx);
    }

    /// The comment's version against the latest, at its file and line now.
    fn compare_since(
        &mut self,
        written: usize,
        path: Option<String>,
        line: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        let from = self.versions.iter().position(|v| v.number == written);
        let to = self.versions.len().checked_sub(1);
        if let (Some(from), Some(to)) = (from, to) {
            self.select_versions(from, to, cx);
            if let (Some(path), Some(line)) = (path, line) {
                self.jump_to_line(&path, line, false, cx);
            }
        }
    }
}

/// Follows `line` of `path` from version `at` to the last of `versions`.
fn history(
    repo: &Repo,
    versions: &[(usize, git2::Oid)],
    at: usize,
    path: &str,
    line: u32,
) -> LineHistory {
    let (written, tip) = versions[at];
    let then = repo
        .file_at(tip, path)
        .and_then(|t| {
            t.lines()
                .nth(line as usize - 1)
                .map(|l| l.trim_end().to_owned())
        })
        .unwrap_or_default();
    let mut steps = Vec::new();
    let mut now = Some(line);
    for pair in versions[at..].windows(2) {
        let ((_, old), (number, new)) = (pair[0], pair[1]);
        let Some(cur) = now else { break };
        let who = || {
            repo.last_change(old, new, path)
                .map(|c| (c.author, format::short(c.id), c.summary))
                .unwrap_or_default()
        };
        match repo.follow_line(old, new, path, cur) {
            Fate::Same(n) => now = Some(n),
            Fate::Changed(n, text) => {
                let (who, sha, summary) = who();
                steps.push(Step {
                    version: number,
                    who,
                    summary,
                    sha,
                    now: Some(text),
                });
                now = Some(n);
            }
            Fate::Gone => {
                let (who, sha, summary) = who();
                steps.push(Step {
                    version: number,
                    who,
                    summary,
                    sha,
                    now: None,
                });
                now = None;
            }
        }
    }
    LineHistory {
        written,
        then,
        steps,
        line_now: now,
    }
}
