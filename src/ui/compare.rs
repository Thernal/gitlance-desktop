//! A comparison's commits (#21, B): Commits · N over Changes in the side panel of a comparison's
//! own page, newest first. A click narrows the diff to that commit's own — against its first
//! parent, a merge's too —, a ⇧-click to a run of them (the oldest's parent against the newest),
//! as a branch's Commits picks; "a41f9c0 only" over the diff says so, and esc or its × bring the
//! whole comparison back. The page stays the comparison's: picking is a filter, not a level.

use super::hover::{CutInTip, Hover};
use super::nav::Page;
use super::{Diff, Header, Selection, Workspace, format, icons, px, row, theme};
use crate::git::{CommitInfo, Repo};
use gpui::{AnyElement, ClickEvent, Context, div, prelude::*};
use std::sync::Arc;

/// Rows of the list before it scrolls.
const ROWS: usize = 8;
/// A row's height.
const ROW: f32 = 28.;

/// A comparison's own commits, read with it.
pub(super) struct CompareCommits {
    /// The comparison they are of, as its page names it.
    pub page: Page,
    /// Newest first; None while they are read.
    pub list: Option<Vec<CommitInfo>>,
    /// The run whose diff is shown — the newest's index and the oldest's —; None the whole.
    pub picked: Option<(usize, usize)>,
    /// Where a ⇧-click's run starts: the commit clicked last.
    anchor: Option<usize>,
}

impl CompareCommits {
    /// The commits picked, newest first.
    fn picked_ids(&self) -> Option<(git2::Oid, git2::Oid)> {
        let (newest, oldest) = self.picked?;
        let list = self.list.as_ref()?;
        Some((list.get(newest)?.id, list.get(oldest)?.id))
    }
}

impl Workspace {
    /// The comparison's commits, while its page is on screen and they are read.
    pub(super) fn shown_compare(&self) -> Option<&CompareCommits> {
        self.compare_commits
            .as_ref()
            .filter(|c| c.list.as_ref().is_some_and(|l| !l.is_empty()))
            .filter(|c| self.nav_page() == Some(&c.page))
    }

    /// The comparison's page while a run of its commits narrows its diff.
    pub(super) fn narrowed_compare(&self) -> Option<&Page> {
        self.shown_compare()
            .filter(|c| c.picked.is_some() && self.selection == Selection::None)
            .map(|c| &c.page)
    }

    /// A comparison of `page` shown whole: its commits are read, unless they are these already
    /// (esc back from a pick); none picked. Any other diff (`None`) has none.
    pub(super) fn read_compare_commits(&mut self, page: Option<Page>, cx: &mut Context<Self>) {
        let sides = match &page {
            Some(Page::Compare(base, head, since)) => Some((base.1, head.1, *since)),
            _ => None,
        };
        let (Some(page), Some((base, head, since)), Some(root)) = (page, sides, self.root.clone())
        else {
            self.compare_commits = None;
            return;
        };
        if let Some(c) = self.compare_commits.as_mut().filter(|c| c.page == page) {
            c.picked = None;
            return;
        }
        self.compare_commits = Some(CompareCommits {
            page: page.clone(),
            list: None,
            picked: None,
            anchor: None,
        });
        cx.spawn(async move |this, cx| {
            let list = cx
                .background_executor()
                .spawn(
                    async move { Repo::open(&root).and_then(|r| r.range_log(base, head, since)) },
                )
                .await
                .unwrap_or_default();
            this.update(cx, |this, cx| {
                if let Some(c) = this.compare_commits.as_mut().filter(|c| c.page == page) {
                    c.list = Some(list);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// A click on commit `ix`: its diff alone; with ⇧ the run from the one clicked last to it.
    pub(super) fn pick_compare_commit(&mut self, ix: usize, span: bool, cx: &mut Context<Self>) {
        let Some(c) = self.compare_commits.as_mut() else {
            return;
        };
        c.picked = Some(run_to(c.anchor, ix, span));
        if !span || c.anchor.is_none() {
            c.anchor = Some(ix);
        }
        let keep = self.current_path();
        self.load_compare_pick(keep, cx);
    }

    /// ↑ ↓ in the comparison's commits: the one above or below the one picked.
    pub(super) fn step_compare_pick(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(to) = self
            .shown_compare()
            .and_then(|c| step_to(c.picked, down, c.list.as_ref().map_or(0, Vec::len)))
        else {
            return;
        };
        self.pick_compare_commit(to, false, cx);
        self.compare_scroll
            .scroll_to_item(to, gpui::ScrollStrategy::Nearest);
    }

    /// Shows the run picked: what the newest holds against the oldest's parent (one commit
    /// against its first parent), under the comparison's own page; `keep` stays on that file.
    pub(super) fn load_compare_pick(&mut self, keep: Option<String>, cx: &mut Context<Self>) {
        let (Some(root), Some((newest, oldest))) = (
            self.root.clone(),
            self.compare_commits.as_ref().and_then(|c| c.picked_ids()),
        ) else {
            return;
        };
        self.clear_diff();
        let settings = self.settings();
        self.load_diff(keep, cx, move || {
            let repo = Repo::open(&root)?;
            // A first commit has no parent: its diff is all it adds.
            let base = repo.parent_of(oldest).unwrap_or(oldest);
            let (files, start, commits) = if newest == oldest {
                (repo.commit_diff(newest, settings)?, base, 1)
            } else {
                let c = repo.compare(base, newest, false, settings)?;
                (c.files, c.start, c.commits)
            };
            Ok(Diff {
                header: Header::Compare {
                    base: (format!("{}^", format::short(oldest)), base),
                    head: (format::short(newest), newest),
                    since_merge_base: false,
                    start,
                    commits,
                    request: None,
                    stale: None,
                },
                files: Arc::new(files),
                pairs: Vec::new(),
                tags: Default::default(),
            })
        });
        cx.notify();
    }

    /// esc or the pill's ×: the whole comparison again. Whether a run was picked.
    pub(super) fn unpick_compare(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(Page::Compare(base, head, since)) = self.narrowed_compare().cloned() else {
            return false;
        };
        self.run_compare(base, head, since, None, cx);
        true
    }

    /// The title of a comparison narrowed to one of its commits: the commit's subject.
    pub(super) fn compare_title(&self) -> Option<String> {
        self.narrowed_compare()?;
        let c = self.compare_commits.as_ref()?;
        match c.picked? {
            (newest, oldest) if newest == oldest => {
                Some(c.list.as_ref()?.get(newest)?.summary.clone())
            }
            _ => None,
        }
    }

    /// The Commits section's line when it is folded: how many are picked, else the newest.
    pub(super) fn compare_summary(&self) -> Option<String> {
        let c = self.shown_compare()?;
        let list = c.list.as_ref()?;
        Some(match c.picked {
            Some((newest, oldest)) => format!("{} of {}", oldest - newest + 1, list.len()),
            None if list.len() > 1 => format!("{} +{}", list[0].summary, list.len() - 1),
            None => list[0].summary.clone(),
        })
    }

    /// The comparison's commits: a row each — its id, subject and age, its card on a long hover
    /// —, at most `ROWS` before the list scrolls, and how they are picked.
    pub(super) fn render_compare_commits(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let n = self.shown_compare()?.list.as_ref()?.len();
        Some(
            div()
                .flex()
                .flex_col()
                .child(
                    gpui::uniform_list(
                        "compare-commits",
                        n,
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|ix| this.render_compare_row(ix, cx))
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.compare_scroll)
                    .h(px(n.min(ROWS) as f32 * ROW)),
                )
                .child(
                    div()
                        .px(px(14.))
                        .pt(px(2.))
                        .pb(px(8.))
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(super::keys::text(
                            "a click shows one · ⇧-click a span · esc all",
                        )),
                )
                .into_any_element(),
        )
    }

    fn render_compare_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some((c, info)) = self
            .compare_commits
            .as_ref()
            .and_then(|c| Some((c, c.list.as_ref()?.get(ix)?)))
        else {
            return div().h(px(ROW)).into_any_element();
        };
        let picked = c
            .picked
            .is_some_and(|(newest, oldest)| (newest..=oldest).contains(&ix));
        let card = std::rc::Rc::new(self.card_of(info.clone()));
        let item = row(("compare-commit", ix), picked)
            .h(px(ROW - 2.))
            .gap_2()
            .child(
                icons::icon("commit")
                    .size(px(13.))
                    .text_color(theme::faint()),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(theme::code_font())
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(format::short(info.id)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .cut_in_tip()
                    .child(info.summary.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(format::ago(info.time)),
            )
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                // Picked in the drawer, it goes and the diff is on screen (#80).
                this.close_drawer(cx);
                this.pick_compare_commit(ix, e.modifiers().shift, cx)
            }))
            .card(move |_, cx| super::hover::CommitCard::show(&card, cx));
        div()
            .w_full()
            .h(px(ROW))
            .px(px(6.))
            .py(px(1.))
            .child(item)
            .into_any_element()
    }

    /// Over the narrowed diff: "a41f9c0 only" (a run its ends), its × the whole comparison.
    pub(super) fn render_compare_only(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.narrowed_compare()?;
        let (newest, oldest) = self.compare_commits.as_ref()?.picked_ids()?;
        let what = if newest == oldest {
            format::short(newest)
        } else {
            format!("{} … {}", format::short(oldest), format::short(newest))
        };
        Some(
            div()
                .flex_none()
                .h(px(24.))
                .pl(px(8.))
                .pr(px(4.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded_full()
                .bg(theme::selected())
                .text_size(px(12.))
                .child(
                    icons::icon("commit")
                        .size(px(13.))
                        .text_color(theme::muted()),
                )
                .child(div().font_family(theme::code_font()).child(what))
                .child("only")
                .child(
                    div()
                        .id("compare-whole")
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .child(icons::icon("x").size(px(12.)).text_color(theme::muted()))
                        .tip(super::keys::text("The whole comparison  esc").into_owned())
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.unpick_compare(cx);
                        })),
                )
                .into_any_element(),
        )
    }
}

/// The run a click on `ix` picks — the newest's index and the oldest's —: with ⇧ from `anchor`,
/// the one clicked last, to it; else it alone.
fn run_to(anchor: Option<usize>, ix: usize, span: bool) -> (usize, usize) {
    match (span, anchor) {
        (true, Some(at)) => (at.min(ix), at.max(ix)),
        _ => (ix, ix),
    }
}

/// ↑ ↓ from the run `picked` in a list of `len`, newest first: the one commit to pick — the first
/// when none is, the one past the run's end the key points to, which stops at the list's ends.
fn step_to(picked: Option<(usize, usize)>, down: bool, len: usize) -> Option<usize> {
    let last = len.checked_sub(1)?;
    match (picked, down) {
        (None, true) => Some(0),
        (None, false) => None,
        (Some((newest, _)), false) => Some(newest.saturating_sub(1)),
        (Some((_, oldest)), true) => Some((oldest + 1).min(last)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_picks_one_and_a_shift_click_the_run_from_the_last_one() {
        assert_eq!(run_to(None, 2, false), (2, 2));
        assert_eq!(run_to(Some(1), 3, false), (3, 3));
        // Newest first: the run's newest is the smaller index, whichever was clicked first.
        assert_eq!(run_to(Some(1), 3, true), (1, 3));
        assert_eq!(run_to(Some(3), 1, true), (1, 3));
        assert_eq!(run_to(None, 2, true), (2, 2));
    }

    #[test]
    fn the_arrows_pick_the_next_commit_past_the_run() {
        assert_eq!(step_to(None, true, 5), Some(0));
        assert_eq!(step_to(None, false, 5), None);
        assert_eq!(step_to(Some((1, 3)), true, 5), Some(4));
        assert_eq!(step_to(Some((1, 3)), false, 5), Some(0));
        assert_eq!(step_to(Some((4, 4)), true, 5), Some(4));
        assert_eq!(step_to(Some((0, 0)), false, 5), Some(0));
        assert_eq!(step_to(None, true, 0), None);
    }
}
