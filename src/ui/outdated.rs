//! Discussions written on an earlier version (Design #46 B, `docs/specs/outdated-discussions.md`):
//! a thread keeps the commit it was written on, and the line it was written on is followed version
//! by version to the latest (`git::Repo::follow_line`) — who changed it, in which commit, what it
//! says now. The latest diff draws such a thread at its line now, saying Changed or Unchanged since
//! the comment — "your comment" only when it is yours, else its author is named — with the line's
//! history, and Compare v1 → v3; a thread whose line is gone stays only in the Discussions window,
//! grouped by the version it was written on.

use super::{Selection, Workspace, format, px, theme};
use crate::git::{Fate, Repo};
use crate::mr::Thread;
use crate::worddiff::Piece;
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, StrikethroughStyle, StyledText, WeakEntity,
    div, prelude::*,
};
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
    /// Where the line is in that version.
    pub line: Option<u32>,
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

    /// The versions to step through: the one it was written on (at `line` then), each that
    /// changed the line, and the latest (`latest`) when the line did not change in it.
    pub fn stops(&self, line: Option<u32>, latest: usize) -> Vec<(usize, Option<u32>)> {
        let mut stops = vec![(self.written, line)];
        stops.extend(self.steps.iter().map(|s| (s.version, s.line)));
        if stops.last().is_some_and(|(v, _)| *v < latest) {
            stops.push((latest, self.line_now));
        }
        stops
    }
}

/// A comparison opened from an outdated discussion: the versions that changed its line, to step
/// through (#46 C, its stepper v3 → v5 → v7).
#[derive(Clone, Debug)]
pub struct Since {
    pub iid: u64,
    /// The discussion it was opened from.
    pub thread: String,
    pub path: String,
    /// Version numbers, oldest first, with where the line is in each (`None`: gone).
    pub stops: Vec<(usize, Option<u32>)>,
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

    /// The commit discussion `t` was written on: the host's word, else (an older GitLab answer
    /// without `head_sha`) the version pushed last before its first note.
    fn written_commit(&self, t: &Thread) -> Option<String> {
        if let Some(c) = &t.commit {
            return Some(c.clone());
        }
        let iid = self.requests.current?;
        let when = t.notes.first()?.created;
        match &self.requests.versions {
            Some((for_iid, list)) if *for_iid == iid && list.len() > 1 => list
                .iter()
                .rev()
                .find(|v| v.time <= when)
                .map(|v| v.tip.to_string()),
            _ => None,
        }
    }

    /// Written on an earlier version than the one shown: its line number is that version's.
    pub(super) fn thread_is_old(&self, t: &Thread) -> bool {
        if t.outdated {
            return true;
        }
        match (self.written_commit(t), self.latest_head()) {
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
                    self.written_commit(t)?,
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
                    this.open_since_on_arrival(cx);
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

    /// Whether thread `t` began with a comment of yours: only then is it "your comment".
    fn yours(&self, t: &Thread) -> bool {
        let me = self.requests.login.as_deref().unwrap_or_default();
        !me.is_empty() && t.notes.first().is_some_and(|n| n.login == me)
    }

    /// "Changed since your comment" (or this comment, another's), or not, for thread `ix`; None
    /// while it is not known.
    pub(super) fn since_comment(&self, ix: usize) -> Option<String> {
        let t = self.requests.threads.get(ix)?;
        let h = self.requests.histories.get(&t.id)?;
        let whose = if self.yours(t) {
            "your comment"
        } else {
            "this comment"
        };
        Some(match (h.changed(), h.line_now) {
            (_, None) => format!("Line removed since {whose}"),
            (true, _) => format!("Changed since {whose}"),
            (false, _) => format!("Unchanged since {whose}"),
        })
    }

    /// The button that compares thread `ix`'s version with the latest: "Compare v1 → v3".
    pub(super) fn compare_label(&self, ix: usize) -> Option<String> {
        let t = self.requests.threads.get(ix)?;
        let written = self.requests.histories.get(&t.id)?.written;
        let latest = self.versions.last()?.number;
        Some(format!("Compare v{written} → v{latest}"))
    }

    /// Under an outdated discussion: the line's history and Compare with the new version.
    pub(super) fn render_history(&self, ix: usize, this: WeakEntity<Self>) -> Option<AnyElement> {
        let t = self.requests.threads.get(ix)?;
        let h = self.requests.histories.get(&t.id)?.clone();
        let changed = h.changed();
        let row = |tag: String, who: String, text: Option<AnyElement>, struck: bool| {
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
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(t)
                        })),
                )
        };
        // Who wrote the comment, named unless it is you.
        let who = if self.yours(t) {
            "you commented on".to_owned()
        } else {
            let author = t.notes.first().map_or("", |n| n.author.as_str());
            format!("{author} commented on")
        };
        let compare = self.compare_label(ix).unwrap_or_default();
        let mut rows: Vec<AnyElement> = vec![
            row(
                format!("v{}", h.written),
                who,
                Some(h.then.clone().into_any_element()),
                changed,
            )
            .into_any_element(),
        ];
        // Each version's line against the one before it, the changed words marked (#46 B).
        let mut before = h.then.clone();
        for s in &h.steps {
            let text = match &s.now {
                Some(now) => {
                    let marked = word_line(&before, now);
                    before = now.clone();
                    marked
                }
                None => "(removed)".into_any_element(),
            };
            rows.push(
                row(
                    format!("v{}", s.version),
                    if s.summary.is_empty() {
                        "changed".to_owned()
                    } else {
                        format!("{} · {} {}", s.who, s.sha, s.summary)
                    },
                    Some(text),
                    s.now.is_none(),
                )
                .into_any_element(),
            );
        }
        let written = h.written;
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
                                .flex_none()
                                .child(compare)
                                .on_click(move |_, _, cx| {
                                    this.update(cx, |this, cx| this.compare_since(ix, cx)).ok();
                                })
                        })),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    /// Compare with the new version, for discussion `ix`: its version against the latest, at its
    /// line now with the whole function around it (or, the line gone, where it was), and a stepper
    /// through the versions that changed it.
    pub(super) fn compare_since(&mut self, ix: usize, cx: &mut Context<Self>) {
        let (Some(t), Some(iid)) = (self.requests.threads.get(ix), self.requests.current) else {
            return;
        };
        let (Some(h), Some(path)) = (self.requests.histories.get(&t.id), t.path.clone()) else {
            return;
        };
        let Some(latest) = self.versions.last().map(|v| v.number) else {
            return;
        };
        let stops = h.stops(t.new_line.or(t.old_line), latest);
        let last = stops.len() - 1;
        let thread = t.id.clone();
        self.since = Some(Since {
            iid,
            thread,
            path,
            stops,
        });
        self.go_since(0, last, cx);
    }

    /// The stepper: versions `a` → `b` of the stops, at the line in `b`.
    fn go_since(&mut self, a: usize, b: usize, cx: &mut Context<Self>) {
        let Some(since) = self.since.clone() else {
            return;
        };
        let index = |number: usize| self.versions.iter().position(|v| v.number == number);
        let (Some(&(va, la)), Some(&(vb, lb))) = (since.stops.get(a), since.stops.get(b)) else {
            return;
        };
        let (Some(from), Some(to)) = (index(va), index(vb)) else {
            return;
        };
        // The comment's file once the comparison is in, at its line: where it is in `b`, or gone
        // there, where it was in `a` on the old side; the whole function around it.
        let at = lb.map(|l| (l, false)).or(la.map(|l| (l, true)));
        self.jump_line = at;
        self.jump_whole = at.is_some();
        self.version_tab = super::VersionTab::Files;
        self.load_versions(from, to, Some(since.path.clone()), cx);
    }

    /// The stepper's comparison of file `path` shown: its discussion pinned on the line, in the
    /// later version or, gone there, in the earlier one (#46 C).
    pub(super) fn since_thread_in(&self, path: &str) -> Vec<(usize, bool, u32)> {
        let Some(since) = self.since.as_ref().filter(|s| s.path == path) else {
            return Vec::new();
        };
        let Selection::Versions { from, to } = self.selection else {
            return Vec::new();
        };
        let number = |i: usize| self.versions.get(i).map(|v| v.number);
        let stop = |n: Option<usize>| since.stops.iter().find(|(v, _)| Some(*v) == n);
        let (Some(&(_, a)), Some(&(_, b))) = (stop(number(from)), stop(number(to))) else {
            return Vec::new();
        };
        let ix = self
            .requests
            .threads
            .iter()
            .position(|t| t.id == since.thread);
        let at = b.map(|l| (false, l)).or(a.map(|l| (true, l)));
        ix.zip(at)
            .map(|(ix, (old, line))| vec![(ix, old, line)])
            .unwrap_or_default()
    }

    /// Over the diff while it is one of the stepper's comparisons: the versions that changed the
    /// comment's line, and the way back to all changes.
    pub(super) fn render_since_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let since = self.since.as_ref()?;
        let Selection::Versions { from, to } = self.selection else {
            return None;
        };
        let shown = (
            self.versions.get(from)?.number,
            self.versions.get(to)?.number,
        );
        let last = since.stops.len() - 1;
        let mut pairs = vec![(0, last)];
        if last > 1 {
            pairs.extend((0..last).map(|i| (i, i + 1)));
        }
        let number = |i: usize| since.stops[i].0;
        if !pairs.iter().any(|&(a, b)| (number(a), number(b)) == shown) {
            return None;
        }
        let iid = since.iid;
        let yours = self
            .requests
            .threads
            .iter()
            .find(|t| t.id == since.thread)
            .is_some_and(|t| self.yours(t));
        let (line, whose) = if yours {
            ("Your comment's line", "your comment")
        } else {
            ("The commented line", "the comment")
        };
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px(px(12.))
                .py(px(6.))
                .border_b_1()
                .border_color(theme::island_border())
                .text_size(px(12.))
                .child(div().text_color(theme::warning()).child(line))
                .child(
                    super::diff_view::group().children(pairs.into_iter().map(|(a, b)| {
                        let label = if (a, b) == (0, last) {
                            format!("v{} → v{} · {whose}", number(a), number(b))
                        } else {
                            format!("v{} → v{}", number(a), number(b))
                        };
                        let on = (number(a), number(b)) == shown;
                        super::chip(("since-step", a * 100 + b), label, on).on_click(cx.listener(
                            move |this, _: &gpui::ClickEvent, _, cx| this.go_since(a, b, cx),
                        ))
                    })),
                )
                .child(div().flex_1())
                .child(
                    super::chip("since-all", "All changes", false).on_click(cx.listener(
                        move |this, _: &gpui::ClickEvent, _, cx| {
                            this.since = None;
                            this.select_request(iid, cx);
                        },
                    )),
                )
                .into_any_element(),
        )
    }

    /// Shows the unchanged lines of the function around `line` (the side `old`), so a comparison
    /// reads the whole function (#46 C).
    pub(super) fn reveal_function(&mut self, line: u32, old: bool) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let side = data.side(old);
        let Some((start, end)) = crate::symbols::enclosing(side.count(), line, |n| side.line(n).0)
        else {
            return;
        };
        let mut grew = false;
        for (ix, segment) in data.segments.iter().enumerate() {
            if let super::rows::Segment::Same {
                old: o,
                new: n,
                len,
            } = *segment
            {
                let first = if old { o } else { n };
                if first <= end && first + len > start {
                    grew |= self.expanded.insert(ix);
                }
            }
        }
        if grew {
            self.relayout();
        }
    }

    /// A notification opened on comment `note`: once the histories are in, an outdated one whose
    /// line changed opens on the diff since it (#46, the inbox way in).
    fn open_since_on_arrival(&mut self, cx: &mut Context<Self>) {
        let Some((iid, note)) = self.since_on_open else {
            return;
        };
        if self.requests.current != Some(iid) {
            return;
        }
        self.since_on_open = None;
        let ix = self
            .requests
            .threads
            .iter()
            .position(|t| t.notes.iter().any(|n| n.id == note));
        if let Some(ix) = ix
            && self
                .requests
                .histories
                .get(&self.requests.threads[ix].id)
                .is_some_and(LineHistory::changed)
        {
            self.compare_since(ix, cx);
        }
    }
}

/// `new` with what changed from `old` marked: removed words struck, added ones tinted, both in
/// place; a line rewritten past recognition reads whole.
fn word_line(old: &str, new: &str) -> AnyElement {
    let Some(pieces) = crate::worddiff::pieces(old, new) else {
        return new.to_owned().into_any_element();
    };
    let (text, runs) = joined(old, new, &pieces);
    let style = |gone: bool| HighlightStyle {
        background_color: Some(if gone {
            theme::removed_word().into()
        } else {
            theme::added_word().into()
        }),
        strikethrough: gone.then_some(StrikethroughStyle {
            thickness: px(1.),
            color: Some(theme::removed().into()),
        }),
        ..Default::default()
    };
    StyledText::new(text)
        .with_highlights(runs.into_iter().map(move |(r, gone)| (r, style(gone))))
        .into_any_element()
}

/// One text from `old` and `new` read together: what stayed, then for each change what went and
/// what came. The ranges of what went (`true`) and what came (`false`) in it.
fn joined(old: &str, new: &str, pieces: &[Piece]) -> (String, Vec<(std::ops::Range<usize>, bool)>) {
    let mut text = String::new();
    let mut runs = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Same(_, n) => text.push_str(&new[n.clone()]),
            Piece::Change(o, n) => {
                for (part, gone) in [(&old[o.clone()], true), (&new[n.clone()], false)] {
                    if !part.is_empty() {
                        let at = text.len();
                        text.push_str(part);
                        runs.push((at..text.len(), gone));
                    }
                }
            }
        }
    }
    (text, runs)
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
                    line: Some(n),
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
                    line: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn step(version: usize, line: Option<u32>) -> Step {
        Step {
            version,
            who: String::new(),
            summary: String::new(),
            sha: String::new(),
            now: line.map(|_| String::new()),
            line,
        }
    }

    #[test]
    fn the_stepper_stops_where_the_line_changed_and_at_the_latest() {
        let h = LineHistory {
            written: 3,
            then: String::new(),
            steps: vec![step(5, Some(50)), step(6, Some(52))],
            line_now: Some(55),
        };
        assert_eq!(
            h.stops(Some(48), 7),
            [(3, Some(48)), (5, Some(50)), (6, Some(52)), (7, Some(55))]
        );
        // Changed in the latest itself: no extra stop.
        assert_eq!(h.stops(Some(48), 6).len(), 3);
    }

    #[test]
    fn a_changed_line_reads_once_with_both_sides_of_each_change() {
        let (old, new) = (
            "val result = api.login(form)",
            "val result = runCatching { api.login(form) }",
        );
        let pieces = crate::worddiff::pieces(old, new).expect("alike lines");
        let (text, runs) = joined(old, new, &pieces);
        assert!(text.contains("runCatching"));
        assert!(runs.iter().all(|(_, gone)| !gone));
        let added: Vec<&str> = runs.iter().map(|(r, _)| &text[r.clone()]).collect();
        assert!(added.iter().any(|a| a.contains("runCatching")));
    }
}
