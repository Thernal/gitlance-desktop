//! The roots of Branches and Merge requests (#72): the list, the row selected previewed beside it,
//! the list's filter in the page bar (⌘F), and the list's states — loading, empty, no token, an
//! error, offline. Merge requests are grouped by what they need from you, each group a band that
//! folds (#81), and Mine counts the discussions waiting for you and lists them first. Branches list the working tree first, then
//! the local branches with how far they are from the main line and their request; a remote's
//! branches fold under its name.

use super::hover::Hover;
use super::lists::Field;
use super::nav::BranchRow;
use super::reviewed::Seen;
use super::{Workspace, discussion, format, icons, island, keys, plural, px, theme, words};
use crate::git::{ChangeKind, RefKind, Repo};
use crate::mr::{Approvals, Mr, Pipeline, Thread};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, Rgba, SharedString, Task, div, prelude::*,
};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

/// The width of a root's list.
const LIST: f32 = 460.;

/// A root's list's width: `LIST`, or a snapshot's `GITLANCE_SNAPSHOT_LIST` to see its rows
/// narrow (#79).
pub(super) fn list_width(width: f32) -> f32 {
    #[cfg(feature = "snapshot")]
    if let Some(w) = std::env::var("GITLANCE_SNAPSHOT_LIST")
        .ok()
        .and_then(|w| w.parse().ok())
    {
        return w;
    }
    width
}

/// What a request needs from you: the list's groups, in their order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Group {
    /// You are asked to review it, or started at its head.
    Asks,
    /// You marked files of it, and it was pushed since.
    Updated,
    /// Its head is reviewed.
    Reviewed,
    Others,
    Yours,
}

impl Group {
    fn heading(self) -> &'static str {
        match self {
            Group::Asks => "Needs your review",
            Group::Updated => "Updated since your review",
            Group::Reviewed => "Reviewed, nothing new",
            Group::Others => "Others",
            Group::Yours => "Yours",
        }
    }

    /// The icon and colour its band opens with: the group says how far your review is, its rows
    /// do not (#81).
    fn icon(self) -> (&'static str, Rgba) {
        match self {
            Group::Asks => ("eye", theme::accent()),
            Group::Updated => ("refresh", theme::warning()),
            Group::Reviewed => ("check", theme::added()),
            Group::Others => ("pull-request", theme::faint()),
            Group::Yours => ("user", theme::faint()),
        }
    }

    /// As `requests-list.txt` writes it.
    fn key(self) -> &'static str {
        match self {
            Group::Asks => "asks",
            Group::Updated => "updated",
            Group::Reviewed => "reviewed",
            Group::Others => "others",
            Group::Yours => "yours",
        }
    }

    fn parse(key: &str) -> Option<Group> {
        [
            Group::Asks,
            Group::Updated,
            Group::Reviewed,
            Group::Others,
            Group::Yours,
        ]
        .into_iter()
        .find(|g| g.key() == key)
    }
}

/// The groups folded in a project whose list was never folded or unfolded: those that need
/// nothing from you.
const FOLDED: [Group; 2] = [Group::Reviewed, Group::Others];

/// What the requests list keeps per project (`<repo>\t<folded groups>\t<requests opened>`, both
/// comma-separated) (#81).
const LIST_FILE: &str = "requests-list.txt";

/// The height of a group's band in the requests list.
const BAND: f32 = 28.;

/// Where request `seen` sits: yours first of all, then by how far your review got.
pub fn group(mine: bool, asks: bool, seen: Seen) -> Group {
    match seen {
        _ if mine => Group::Yours,
        Seen::Done => Group::Reviewed,
        Seen::Earlier => Group::Updated,
        Seen::Started => Group::Asks,
        Seen::Never if asks => Group::Asks,
        Seen::Never => Group::Others,
    }
}

/// A branch, as its row and preview say it.
#[derive(Clone)]
pub struct Fact {
    /// When its tip was committed, its subject and who wrote it.
    pub time: i64,
    pub summary: String,
    pub author: String,
    /// What it is counted against (the main line, or its upstream for the main line itself) and
    /// how far apart: (name, ahead, behind).
    pub against: Option<(String, usize, usize)>,
}

/// A file of the working tree, for its row and preview.
#[derive(Clone)]
pub struct TreeFile {
    pub change: ChangeKind,
    pub path: String,
    pub added: usize,
    pub removed: usize,
}

/// A request's pipeline and approvals, read for its preview, with its `updated` then.
struct Preview {
    updated: i64,
    pipeline: Option<Pipeline>,
    approvals: Option<Approvals>,
}

#[derive(Default)]
pub struct Roots {
    /// Per refname.
    pub(super) facts: HashMap<String, Fact>,
    facts_task: Option<Task<()>>,
    /// The remotes whose branches the list shows; the rest are folded to their name.
    open_remotes: HashSet<String>,
    /// The working tree's files as last read, with the fingerprint they were read at.
    tree: Option<(u64, Vec<TreeFile>)>,
    tree_task: Option<Task<()>>,
    previews: HashMap<u64, Preview>,
    preview_task: Option<Task<()>>,
    /// The requests list's scroll, and whether the next frame brings the picked row into view
    /// (after ↑ ↓ or esc back to the list).
    scroll: gpui::ScrollHandle,
    pub reveal: Cell<bool>,
    /// Project `kept_of`'s requests list: the groups folded, and the requests you have opened —
    /// a row's dot is gone once you have (#81).
    folded: HashSet<Group>,
    opened: HashSet<u64>,
    kept_of: Option<String>,
}

impl Roots {
    /// Project `repo`'s folds and opened requests, unless they are the ones held.
    pub fn load(&mut self, repo: &str) {
        if self.kept_of.as_deref() == Some(repo) {
            return;
        }
        (self.folded, self.opened) = kept_list(&crate::storage::read(LIST_FILE), repo)
            .unwrap_or_else(|| (FOLDED.into_iter().collect(), HashSet::new()));
        self.kept_of = Some(repo.to_owned());
    }

    fn keep(&self) {
        if let Some(repo) = &self.kept_of {
            let text = crate::storage::read(LIST_FILE);
            crate::storage::save(
                LIST_FILE,
                with_kept_list(&text, repo, &self.folded, &self.opened),
            );
        }
    }
}

/// Repository `repo`'s line of `requests-list.txt`: its folded groups and the requests opened;
/// none when it has no line.
fn kept_list(text: &str, repo: &str) -> Option<(HashSet<Group>, HashSet<u64>)> {
    let line = text.lines().find(|l| l.split('\t').next() == Some(repo))?;
    let mut parts = line.split('\t').skip(1);
    let mut next = || parts.next().unwrap_or_default().split(',');
    let folded = next().filter_map(Group::parse).collect();
    let opened = next().filter_map(|iid| iid.parse().ok()).collect();
    Some((folded, opened))
}

/// `text` with repository `repo`'s line written anew; other repositories' kept.
fn with_kept_list(
    text: &str,
    repo: &str,
    folded: &HashSet<Group>,
    opened: &HashSet<u64>,
) -> String {
    let mut folded: Vec<Group> = folded.iter().copied().collect();
    folded.sort();
    let mut opened: Vec<u64> = opened.iter().copied().collect();
    opened.sort();
    let line = format!(
        "{repo}\t{}\t{}",
        folded.iter().map(|g| g.key()).collect::<Vec<_>>().join(","),
        opened
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !l.is_empty() && l.split('\t').next() != Some(repo))
        .map(str::to_owned)
        .chain([line])
        .collect();
    lines.sort_unstable();
    lines.join("\n") + "\n"
}

/// The band held at the top of the requests list while its rows scroll under it, by the bands'
/// tops and the list's top: the last band above it, pushed up as far as the next one comes into
/// its room (#81).
fn held_band(tops: &[f32], top: f32) -> Option<(usize, f32)> {
    let at = tops.iter().rposition(|&t| t < top)?;
    let push = tops
        .get(at + 1)
        .map_or(0., |&next| (next - top - BAND).min(0.));
    Some((at, push))
}

impl Workspace {
    // ---- reading ------------------------------------------------------------------------

    /// Reads, in the background, when each branch's tip was made and how far it is from the
    /// main line.
    pub(super) fn read_branch_facts(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let branches = self.branches.clone();
        self.roots.facts_task = Some(cx.spawn(async move |this, cx| {
            let facts = cx
                .background_executor()
                .spawn(async move {
                    let repo = Repo::open(&root).ok()?;
                    let main = repo.default_branch();
                    let tip = |name: &str| branches.iter().find(|b| b.name == name).map(|b| b.tip);
                    let main_tip = main
                        .as_deref()
                        .and_then(|m| tip(m).or_else(|| tip(&format!("origin/{m}"))));
                    let facts: HashMap<String, Fact> = branches
                        .iter()
                        .filter_map(|b| {
                            let commit = repo.commit(b.tip).ok()?;
                            let against = match (b.kind, &main, main_tip) {
                                (RefKind::Remote, ..) => None,
                                (RefKind::Local, Some(m), _) if *m == b.name => repo.tracking(m),
                                (RefKind::Local, Some(m), Some(t)) => repo
                                    .ahead_behind(b.tip, t)
                                    .map(|(ahead, behind)| (m.clone(), ahead, behind)),
                                _ => None,
                            };
                            let fact = Fact {
                                time: commit.time,
                                summary: commit.summary,
                                author: commit.author,
                                against,
                            };
                            Some((b.refname.clone(), fact))
                        })
                        .collect();
                    Some(facts)
                })
                .await;
            this.update(cx, |this, cx| {
                if let Some(facts) = facts {
                    this.roots.facts = facts;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Reads the working tree's files again when they changed since the last read.
    pub(super) fn read_tree_files(&mut self, cx: &mut Context<Self>) {
        let hash = self.wt.hash;
        if self.wt.count == 0 || self.roots.tree.as_ref().is_some_and(|t| t.0 == hash) {
            return;
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let settings = self.settings();
        self.roots.tree_task = Some(cx.spawn(async move |this, cx| {
            let files = cx
                .background_executor()
                .spawn(async move {
                    let tree = Repo::open(&root)?.working_tree(settings, false)?;
                    anyhow::Ok(
                        tree.files
                            .iter()
                            .map(|f| TreeFile {
                                change: f.change,
                                path: f.path().to_owned(),
                                added: f.added,
                                removed: f.removed,
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                if let Ok(files) = files {
                    this.roots.tree = Some((hash, files));
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// The request picked in the list: its pipeline and approvals, once per update of it.
    pub(super) fn read_preview(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self
            .nav
            .pick_request
            .and_then(|iid| self.requests.list.iter().find(|m| m.iid == iid))
        else {
            return;
        };
        let (iid, updated) = (m.iid, m.updated);
        if self
            .roots
            .previews
            .get(&iid)
            .is_some_and(|p| p.updated == updated)
        {
            return;
        }
        let Some(remote) = self.web.clone() else {
            return;
        };
        self.roots.preview_task = Some(cx.spawn(async move |this, cx| {
            let (pipeline, approvals) = cx
                .background_executor()
                .spawn(async move {
                    (
                        crate::mr::pipeline(&remote, iid).ok().flatten(),
                        crate::mr::approvals(&remote, iid).ok(),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.roots.previews.insert(
                    iid,
                    Preview {
                        updated,
                        pipeline,
                        approvals,
                    },
                );
                cx.notify();
            })
            .ok();
        }));
    }

    // ---- what the lists hold ---------------------------------------------------------------

    /// Request `m`'s group in the list.
    pub(super) fn group_of(&self, m: &Mr) -> Group {
        let me = self.requests.login.as_deref().unwrap_or_default();
        let head = git2::Oid::from_str(&m.head).ok();
        group(
            !me.is_empty() && m.author_login == me,
            !me.is_empty() && m.reviewers.iter().any(|r| r == me),
            self.request_seen(m.iid, head),
        )
    }

    /// The discussions of request `iid` waiting for its author: open, and the last word not
    /// theirs. Read with the notifications (`notices`), so only for requests read there.
    fn waiting_threads(&self, iid: u64) -> Vec<&Thread> {
        let me = self.requests.login.as_deref().unwrap_or_default();
        self.notices
            .threads_of(iid)
            .unwrap_or_default()
            .iter()
            .filter(|t| {
                t.resolvable
                    && !t.resolved
                    && !me.is_empty()
                    && t.notes.last().is_some_and(|n| n.login != me)
            })
            .collect()
    }

    /// Branches' rows in the order the list shows them: the working tree, local branches, then
    /// the branches of each open remote; a filter lists the matching branches only.
    pub(super) fn branch_rows(&self) -> Vec<BranchRow> {
        let filter = self.bfilter.to_lowercase();
        if !filter.trim().is_empty() {
            return crate::rank::rank(self.branches.iter().enumerate(), &filter, |(_, b)| &b.name)
                .into_iter()
                .map(|(ix, _)| BranchRow::Branch(ix))
                .collect();
        }
        let mut out = vec![BranchRow::WorkingTree];
        out.extend(
            self.branches
                .iter()
                .enumerate()
                .filter(|(_, b)| b.kind == RefKind::Local)
                .map(|(ix, _)| BranchRow::Branch(ix)),
        );
        for (remote, ixs) in self.remotes() {
            if self.roots.open_remotes.contains(remote) {
                out.extend(ixs.into_iter().map(BranchRow::Branch));
            }
        }
        out
    }

    /// The remote branches, by remote.
    fn remotes(&self) -> std::collections::BTreeMap<&str, Vec<usize>> {
        let mut out: std::collections::BTreeMap<&str, Vec<usize>> = Default::default();
        for (ix, b) in self.branches.iter().enumerate() {
            if b.kind == RefKind::Remote {
                let remote = b.name.split_once('/').map_or("", |(r, _)| r);
                out.entry(remote).or_default().push(ix);
            }
        }
        out
    }

    /// "4 local · 6 on origin": the count after Branches' name in the bar.
    pub(super) fn branches_count(&self) -> String {
        let local = self
            .branches
            .iter()
            .filter(|b| b.kind == RefKind::Local)
            .count();
        let mut parts = vec![format!("{local} local")];
        parts.extend(
            self.remotes()
                .into_iter()
                .map(|(remote, ixs)| format!("{} on {remote}", ixs.len())),
        );
        parts.join(" · ")
    }

    /// The row Branches selects when nothing is: the working tree with changes, else the branch
    /// checked out.
    pub(super) fn first_branch_row(&self) -> Option<BranchRow> {
        if self.wt.count > 0 {
            return Some(BranchRow::WorkingTree);
        }
        self.branches
            .iter()
            .position(|b| b.is_head)
            .or(self.branch)
            .map(BranchRow::Branch)
    }

    /// A root list's row picked, by a click or ↑ ↓.
    pub(super) fn pick_request(&mut self, iid: u64, cx: &mut Context<Self>) {
        self.nav.pick_request = Some(iid);
        self.roots.reveal.set(true);
        self.read_preview(cx);
        cx.notify();
    }

    fn toggle_remote(&mut self, remote: String, cx: &mut Context<Self>) {
        if !self.roots.open_remotes.remove(&remote) {
            self.roots.open_remotes.insert(remote);
        }
        cx.notify();
    }

    fn toggle_group(&mut self, g: Group, cx: &mut Context<Self>) {
        if !self.roots.folded.remove(&g) {
            self.roots.folded.insert(g);
        }
        self.roots.keep();
        cx.notify();
    }

    /// Whether group `g`'s rows are folded away; a filter typed opens every group.
    fn folds(&self, g: Group) -> bool {
        self.bfilter.trim().is_empty() && self.roots.folded.contains(&g)
    }

    /// The requests the list has a row for, in its order: those of folded groups left out, but
    /// the picked one, which stays under its band.
    pub(super) fn listed_requests(&self) -> Vec<&Mr> {
        self.shown_requests()
            .into_iter()
            .filter(|m| self.nav.pick_request == Some(m.iid) || !self.folds(self.group_of(m)))
            .collect()
    }

    /// Whether you have been inside request `iid`: opened here, or files of it marked (#81).
    pub(super) fn opened_before(&self, iid: u64) -> bool {
        let head = self
            .requests
            .list
            .iter()
            .find(|m| m.iid == iid)
            .and_then(|m| git2::Oid::from_str(&m.head).ok());
        self.roots.opened.contains(&iid) || !matches!(self.request_seen(iid, head), Seen::Never)
    }

    /// Request `iid` opened: its row's dot goes for good. Requests no longer open are forgotten.
    pub(super) fn mark_opened(&mut self, iid: u64) {
        if self.roots.opened.contains(&iid) {
            return;
        }
        if !self.requests.list.is_empty() {
            let list = &self.requests.list;
            self.roots
                .opened
                .retain(|i| list.iter().any(|m| m.iid == *i));
        }
        self.roots.opened.insert(iid);
        self.roots.keep();
    }

    // ---- the pages ------------------------------------------------------------------------

    /// Merge requests' root: the list grouped, the picked one previewed; or one of its states.
    pub(super) fn render_requests_root(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.requests.connectable {
            return island()
                .flex_1()
                .bg(theme::editor())
                .child(self.render_connect(cx))
                .into_any_element();
        }
        let shown = self.shown_requests();
        let loading = self.requests.loading && self.requests.list.is_empty();
        let body = if loading {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap(px(super::gap()))
                .child(
                    island()
                        .w(px(list_width(LIST)))
                        .flex_none()
                        .p(px(16.))
                        .child(skeleton(7)),
                )
                .child(
                    island()
                        .flex_1()
                        .bg(theme::editor())
                        .px(px(28.))
                        .py(px(26.))
                        .child(skeleton(3)),
                )
                .into_any_element()
        } else if shown.is_empty() {
            island()
                .flex_1()
                .bg(theme::editor())
                .child(self.requests_empty(cx))
                .into_any_element()
        } else {
            let groups: Vec<Group> = shown.iter().map(|m| self.group_of(m)).collect();
            let picked = self
                .nav
                .pick_request
                .and_then(|iid| shown.iter().find(|m| m.iid == iid))
                .or_else(|| {
                    shown
                        .iter()
                        .zip(&groups)
                        .find(|(_, g)| !self.folds(**g))
                        .map(|(m, _)| m)
                })
                .copied();
            // Each group a band, its rows under it unless it is folded (#81).
            let mut rows: Vec<AnyElement> = Vec::new();
            let mut bands: Vec<(usize, Group, usize)> = Vec::new();
            let mut picked_row = None;
            for (n, (m, &g)) in shown.iter().zip(&groups).enumerate() {
                if n == 0 || groups[n - 1] != g {
                    let count = groups.iter().filter(|&&o| o == g).count();
                    bands.push((rows.len(), g, count));
                    rows.push(self.group_band(g, count, false, cx).into_any_element());
                }
                let is_picked = picked.is_some_and(|p| p.iid == m.iid);
                if self.folds(g) && !is_picked {
                    continue;
                }
                if is_picked {
                    picked_row = Some(rows.len());
                }
                rows.push(self.request_row(m, g, cx).into_any_element());
            }
            // The rows as the last frame laid them out, when they are these rows: where the list's
            // top is, so the band above it is held there.
            let scroll = &self.roots.scroll;
            let laid = scroll.children_count() == rows.len();
            let top = f32::from(scroll.bounds().top() - scroll.offset().y);
            let top_of = |ix: usize| scroll.bounds_for_item(ix).map(|b| f32::from(b.top()));
            if self.roots.reveal.take()
                && let Some(ix) = picked_row
            {
                // A row going up out of sight would stop under the held band: the one above it
                // comes into view instead, and with it the row.
                let under_band = laid && ix > 0 && top_of(ix).is_some_and(|t| t < top + BAND);
                scroll.scroll_to_item(if under_band { ix - 1 } else { ix });
            }
            let tops: Vec<f32> = bands.iter().filter_map(|&(ix, ..)| top_of(ix)).collect();
            let held = (laid && tops.len() == bands.len())
                .then(|| held_band(&tops, top))
                .flatten()
                .map(|(at, push)| {
                    let (_, g, count) = bands[at];
                    div()
                        .absolute()
                        .top(px(push))
                        .left_0()
                        .right_0()
                        .child(self.group_band(g, count, true, cx))
                });
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap(px(super::gap()))
                .child(
                    island()
                        .w(px(list_width(LIST)))
                        .flex_none()
                        .border_color(self.zone_border(super::zones::Zone::Branches))
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .overflow_hidden()
                                .child(self.widths.measure("requests"))
                                .child(
                                    div()
                                        .id("requests")
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .track_scroll(&self.roots.scroll)
                                        .pb(px(6.))
                                        .children(rows),
                                )
                                .children(held),
                        ),
                )
                .child(
                    island().flex_1().min_w_0().bg(theme::editor()).child(
                        div()
                            .id("request-preview")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(picked.map(|m| self.request_preview(m, cx))),
                    ),
                )
                .into_any_element()
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(super::gap()))
            .children(self.list_banner(cx))
            .child(body)
            .into_any_element()
    }

    /// No request to list: why, per filter, and the way to more.
    fn requests_empty(&self, cx: &mut Context<Self>) -> Div {
        use super::requests::Scope;
        let all = self.requests.list.len();
        let filter = self.bfilter.trim().to_owned();
        let show_all = |label: &'static str| {
            div()
                .id("empty-show-all")
                .child(bar_button(label))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.requests.scope = super::requests::Scope::All;
                    this.bfilter.clear();
                    this.field = None;
                    cx.notify();
                }))
                .into_any_element()
        };
        if !filter.is_empty() {
            return empty_state(
                "search",
                format!("Nothing matches “{filter}”"),
                words::say("The filter looks in the number, title, author and branch of each merge request.")
                    .into_owned(),
                Some(show_all("Clear the filter")),
            );
        }
        if self.requests.error.is_some() && all == 0 {
            return empty_state(
                "pull-request",
                words::say("No merge requests yet").into_owned(),
                "They come when the list can be read again.".to_owned(),
                None,
            );
        }
        match self.requests.scope {
            Scope::ReviewMe => empty_state(
                "pull-request",
                "Nothing to review".to_owned(),
                format!(
                    "Requests that ask for your review land here. {} open under All.",
                    if all == 1 {
                        "1 is".to_owned()
                    } else {
                        format!("{all} are")
                    }
                ),
                (all > 0).then(|| show_all("Show all")),
            ),
            Scope::Mine => empty_state(
                "pull-request",
                "None of yours is open".to_owned(),
                words::say(
                    "The merge requests you open land here, with what waits for you on each.",
                )
                .into_owned(),
                (all > 0).then(|| show_all("Show all")),
            ),
            Scope::All => empty_state(
                "pull-request",
                words::say("No open merge requests").into_owned(),
                words::say("When someone opens a merge request in this project, it lands here.")
                    .into_owned(),
                None,
            ),
        }
    }

    /// A group's band in the requests list: its icon, name and count, and the chevron that folds
    /// it (a click anywhere on it does); `held` is its copy held at the top of the list (#81).
    fn group_band(
        &self,
        g: Group,
        count: usize,
        held: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (icon, tone) = g.icon();
        let foldable = self.bfilter.trim().is_empty();
        let folded = self.folds(g);
        div()
            .id((if held { "group-held" } else { "group-band" }, g as usize))
            .h(px(BAND))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(16.))
            .pr(px(10.))
            .bg(theme::base())
            .border_t_1()
            .border_b_1()
            .border_color(theme::island_border())
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if folded {
                theme::muted()
            } else {
                theme::text()
            })
            .child(icons::icon(icon).size(px(14.)).text_color(tone))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(g.heading()),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(6.))
                    .rounded_full()
                    .bg(theme::hover())
                    .text_size(px(11.))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(theme::muted())
                    .child(count.to_string()),
            )
            .when(foldable, |s| {
                s.cursor_pointer()
                    .child(
                        icons::icon(if folded { "chevron-right" } else { "chevron" })
                            .size(px(14.))
                            .text_color(theme::faint()),
                    )
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_group(g, cx)),
                    )
            })
    }

    /// A request's row: unread dot, number and title, when; who, the pipeline, its discussions.
    fn request_row(&self, m: &Mr, g: Group, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let iid = m.iid;
        let picked = self.nav.pick_request == Some(iid);
        let head = git2::Oid::from_str(&m.head).ok();
        let part = head.and_then(|h| self.head_part(h));
        let pipeline = self
            .roots
            .previews
            .get(&iid)
            .and_then(|p| p.pipeline.as_ref());
        let waiting = (g == Group::Yours).then(|| self.waiting_threads(iid).len());
        let threads = self.notices.threads_of(iid).map(<[Thread]>::len);
        // A dot: a row of yours a thread waits on, another you have never opened; how far your
        // review is, its group says (#81).
        let unread = match waiting {
            Some(n) => n > 0,
            None => !self.opened_before(iid),
        };
        // Its anatomy (#79): the number, the title and the pipeline's verdict always; who (or
        // what waits for you) and when while the list has 260 px; the threads and how far your
        // review is under 360 px leave first. The tooltip holds it all.
        let room = self.widths.room("requests");
        let who = match waiting {
            Some(n) if n > 0 => Some((
                format!(
                    "{} for you",
                    if n == 1 {
                        "1 thread waits".to_owned()
                    } else {
                        format!("{n} threads wait")
                    }
                ),
                theme::warning(),
            )),
            _ => Some((
                if m.draft {
                    format!("draft · {}", m.author)
                } else {
                    m.author.clone()
                },
                theme::muted(),
            )),
        };
        let verdict = pipeline.map(super::requests::pipeline_word);
        let threads = threads.filter(|&n| n > 0 && waiting.is_none_or(|w| w == 0));
        let mut tip = vec![
            format!("{} {}", words::number(iid), m.title),
            format!("{} · {} → {}", m.author, m.source, m.target),
        ];
        if let Some((glyph, word, _)) = &verdict {
            tip.push(format!("pipeline {glyph} {word}"));
        }
        if let Some(n) = threads {
            tip.push(plural(n, "thread"));
        }
        if let Some((text, _)) = who.as_ref().filter(|_| waiting.is_some_and(|w| w > 0)) {
            tip.push(text.clone());
        }
        tip.push(format!("updated {}", super::hover::full_date(m.updated)));
        let mut meta: Vec<AnyElement> = Vec::new();
        if let Some((text, color)) = who.filter(|_| room.second()) {
            meta.push(div().text_color(color).child(text).into_any_element());
        }
        if let Some((glyph, word, color)) = verdict {
            meta.push(
                div()
                    .text_color(color)
                    .child(format!("{glyph} {word}"))
                    .into_any_element(),
            );
        }
        if let Some(n) = threads.filter(|_| room.third()) {
            meta.push(div().child(plural(n, "thread")).into_any_element());
        }
        let mut line2: Vec<AnyElement> = Vec::new();
        for (n, part) in meta.into_iter().enumerate() {
            if n > 0 {
                line2.push(
                    div()
                        .text_color(theme::faint())
                        .child("·")
                        .into_any_element(),
                );
            }
            line2.push(part);
        }
        div().px(px(6.)).py(px(1.)).child(
            div()
                .id(("mr-row", iid))
                .px(px(10.))
                .py(px(8.))
                .flex()
                .items_start()
                .gap(px(10.))
                .rounded(px(super::ROW_RADIUS))
                .cursor_pointer()
                .when(picked, |s| s.bg(theme::selected()))
                .when(!picked, |s| s.hover(|s| s.bg(theme::hover())))
                .child(unread_dot(unread))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex_none()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(words::number(iid)),
                                )
                                .child(div().flex_1().min_w_0().truncate().child(m.title.clone()))
                                .children(
                                    part.filter(|_| room.third()).map(|(done, total)| {
                                        super::lists::progress_badge(done, total)
                                    }),
                                )
                                .children(room.second().then(|| {
                                    div()
                                        .flex_none()
                                        .text_size(px(11.))
                                        .text_color(theme::faint())
                                        .child(format::ago(m.updated))
                                })),
                        )
                        .when(!line2.is_empty(), |s| {
                            s.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_size(px(12.))
                                    .text_color(theme::muted())
                                    .children(line2),
                            )
                        }),
                )
                .tip(tip.join("\n"))
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    // A second click on the picked one opens it, as ↵ does.
                    if this.nav.pick_request == Some(iid) && event.click_count() > 1 {
                        this.field = None;
                        return this.select_request(iid, cx);
                    }
                    this.zone = super::zones::Zone::Branches;
                    this.pick_request(iid, cx);
                })),
        )
    }

    /// The request picked: who and where, its title and description, how far your review is,
    /// and Open; for one of yours, the discussions waiting for you first.
    fn request_preview(&self, m: &Mr, cx: &mut Context<Self>) -> AnyElement {
        let iid = m.iid;
        let me = self.requests.login.as_deref().unwrap_or_default();
        let mine = !me.is_empty() && m.author_login == me;
        let preview = self.roots.previews.get(&iid);
        let mut pills: Vec<AnyElement> = Vec::new();
        if let Some(p) = preview.and_then(|p| p.pipeline.as_ref()) {
            let (glyph, word, color) = super::requests::pipeline_word(p);
            pills.push(pill(format!("{glyph} {word}"), color).into_any_element());
        }
        if let Some(a) = preview.and_then(|p| p.approvals.as_ref()) {
            let said = if a.required > 0 {
                Some(format!(
                    "{}/{} approved",
                    a.required - a.left.min(a.required),
                    a.required
                ))
            } else {
                (!a.by.is_empty()).then(|| format!("{} approved", a.by.len()))
            };
            pills.extend(said.map(|s| {
                pill(
                    s,
                    if a.left == 0 && !a.by.is_empty() {
                        theme::added()
                    } else {
                        theme::muted()
                    },
                )
                .into_any_element()
            }));
        }
        let props = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(theme::muted())
            .child(
                div()
                    .font_family(theme::CODE_FONT)
                    .text_color(theme::accent())
                    .child(words::number(iid)),
            )
            .child(discussion::avatar(&m.author, mine, 18.))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(if mine {
                        "yours".to_owned()
                    } else {
                        m.author.clone()
                    })
                    .child(div().text_color(theme::faint()).child("·"))
                    .child(
                        div()
                            .font_family(theme::CODE_FONT)
                            .child(format!("{} → {}", m.source, m.target)),
                    ),
            )
            .child(div().flex_1())
            .children(pills);
        let title = div()
            .text_size(px(19.))
            .line_height(px(26.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(m.title.clone());
        let description = crate::markdown::plain(&m.description);
        let description = description.trim();
        // How far your review is, and what is open.
        let head = git2::Oid::from_str(&m.head).ok();
        let mut facts: Vec<String> = Vec::new();
        match (
            self.request_seen(iid, head),
            head.and_then(|h| self.head_part(h)),
        ) {
            (Seen::Done, _) => facts.push("Reviewed at this push".to_owned()),
            (Seen::Earlier, _) => facts.push("Pushed since your review".to_owned()),
            (_, Some((done, total))) => facts.push(format!("{done} of {total} files reviewed")),
            (Seen::Started | Seen::Never, None) => {}
        }
        if let Some(threads) = self.notices.threads_of(iid) {
            let open = threads
                .iter()
                .filter(|t| t.resolvable && !t.resolved)
                .count();
            if open > 0 {
                facts.push(format!("{} open", plural(open, "discussion")));
            }
        }
        if m.draft {
            facts.push("Draft".to_owned());
        }
        facts.push(format!("updated {}", format::ago(m.updated)));
        let waiting = if mine {
            self.waiting_threads(iid)
        } else {
            Vec::new()
        };
        let first = waiting.first().and_then(|t| t.notes.last().map(|n| n.id));
        let open_label = if first.is_some() {
            "Open the first"
        } else {
            "Open"
        };
        let open_hint = if first.is_some() {
            words::say("opens the merge request at that discussion").into_owned()
        } else {
            "opens on its changes, at the first file not reviewed · ⌥↓ the next not reviewed"
                .to_owned()
        };
        let waits = (!waiting.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .pb(px(6.))
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(format!("Waiting for you · {}", waiting.len())),
                )
                .children(waiting.iter().enumerate().map(|(n, t)| {
                    let last = t.notes.last();
                    let who = last.map(|n| n.author.clone()).unwrap_or_default();
                    let gist = last.map(|n| self.one_line(&n.body)).unwrap_or_default();
                    let at = match (&t.path, t.new_line.or(t.old_line)) {
                        (Some(path), Some(line)) => {
                            format!("{}:{line}", path.rsplit('/').next().unwrap_or(path))
                        }
                        (Some(path), None) => path.rsplit('/').next().unwrap_or(path).to_owned(),
                        _ => "General".to_owned(),
                    };
                    let note = last.map(|n| n.id);
                    div()
                        .id(("waiting", n))
                        .flex()
                        .items_start()
                        .gap(px(10.))
                        .py(px(8.))
                        .border_t_1()
                        .border_color(theme::island_border())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .child(discussion::avatar(&who, false, 18.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .text_size(px(12.))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(6.))
                                        .child(div().font_weight(FontWeight::SEMIBOLD).child(who))
                                        .child(
                                            div()
                                                .text_color(theme::faint())
                                                .child(format!("on {at}")),
                                        ),
                                )
                                .child(
                                    super::hover::cut(
                                        SharedString::from(format!("waiting-text-{n}")),
                                        gist,
                                    )
                                    .text_color(theme::muted()),
                                ),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.open_at_note(iid, note, cx)
                        }))
                }))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .px(px(28.))
            .py(px(22.))
            .max_w(px(820.))
            .child(props)
            .child(title)
            .children(waits)
            .children((!description.is_empty()).then(|| {
                super::hover::cut_to(
                    SharedString::from(format!("mr-description-{iid}")),
                    description.to_owned(),
                    5,
                )
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(theme::muted())
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .children(facts.into_iter().map(|f| div().child(f))),
            )
            .child(
                div().flex().items_center().gap(px(12.)).child(
                    primary_button("request-open", open_label, "↵")
                        .tip(open_hint)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| match first {
                            Some(note) => this.open_at_note(iid, Some(note), cx),
                            None => {
                                this.field = None;
                                this.select_request(iid, cx)
                            }
                        })),
                ),
            )
            .into_any_element()
    }

    /// Opens request `iid` at note `note` (a discussion waiting for you), else as ↵ does.
    pub(super) fn open_at_note(&mut self, iid: u64, note: Option<u64>, cx: &mut Context<Self>) {
        self.field = None;
        match note {
            Some(note) => {
                self.since_on_open = Some((iid, note));
                self.set_zone(super::zones::Zone::Diff, cx);
                self.go_to(
                    crate::links::Target::Request {
                        iid,
                        note: Some(note),
                        line: None,
                    },
                    cx,
                );
            }
            None => self.select_request(iid, cx),
        }
    }

    /// ↵ on Merge requests' root: the picked request opens — for one of yours with discussions
    /// waiting, at the first of them.
    pub(super) fn open_picked_request(&mut self, iid: u64, cx: &mut Context<Self>) {
        let me = self.requests.login.clone().unwrap_or_default();
        let mine = self
            .requests
            .list
            .iter()
            .any(|m| m.iid == iid && !me.is_empty() && m.author_login == me);
        let note = mine
            .then(|| {
                self.waiting_threads(iid)
                    .first()
                    .and_then(|t| t.notes.last().map(|n| n.id))
            })
            .flatten();
        self.open_at_note(iid, note, cx);
    }

    /// Branches' root: the working tree, the local branches, the remotes folded; the row picked
    /// previewed.
    pub(super) fn render_branches_root(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.branch_rows();
        let filtering = !self.bfilter.trim().is_empty();
        let picked = self
            .nav
            .pick_branch
            .filter(|r| rows.contains(r))
            .or_else(|| self.first_branch_row().filter(|r| rows.contains(r)))
            .or(rows.first().copied());
        let mut list: Vec<AnyElement> = Vec::new();
        if filtering {
            list.extend(rows.iter().map(|row| {
                self.branch_row(*row, picked == Some(*row), true, cx)
                    .into_any_element()
            }));
            if rows.is_empty() {
                list.push(
                    div()
                        .px(px(16.))
                        .py(px(10.))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(format!("No branch matches “{}”.", self.bfilter.trim()))
                        .into_any_element(),
                );
            }
        } else {
            list.push(heading("Working tree".to_owned()).into_any_element());
            list.push(
                self.branch_row(
                    BranchRow::WorkingTree,
                    picked == Some(BranchRow::WorkingTree),
                    false,
                    cx,
                )
                .into_any_element(),
            );
            let local: Vec<usize> = self
                .branches
                .iter()
                .enumerate()
                .filter(|(_, b)| b.kind == RefKind::Local)
                .map(|(ix, _)| ix)
                .collect();
            list.push(heading(format!("Local · {}", local.len())).into_any_element());
            list.extend(local.into_iter().map(|ix| {
                let row = BranchRow::Branch(ix);
                self.branch_row(row, picked == Some(row), false, cx)
                    .into_any_element()
            }));
            for (remote, ixs) in self.remotes() {
                let open = self.roots.open_remotes.contains(remote);
                let name = remote.to_owned();
                list.push(
                    div()
                        .id(SharedString::from(format!("remote-{remote}")))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .px(px(12.))
                        .pt(px(10.))
                        .pb(px(4.))
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme::text()))
                        .child(
                            icons::icon(if open { "chevron" } else { "chevron-right" })
                                .size(px(14.))
                                .text_color(theme::faint()),
                        )
                        .child(format!(
                            "{remote} · {}{}",
                            ixs.len(),
                            if open { "" } else { " · folded" }
                        ))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.toggle_remote(name.clone(), cx)
                        }))
                        .into_any_element(),
                );
                if open {
                    list.extend(ixs.into_iter().map(|ix| {
                        let row = BranchRow::Branch(ix);
                        self.branch_row(row, picked == Some(row), false, cx)
                            .into_any_element()
                    }));
                }
            }
        }
        let local = self
            .branches
            .iter()
            .filter(|b| b.kind == RefKind::Local)
            .count();
        let alone = !filtering && local == 1 && self.wt.count == 0;
        let preview = if alone {
            let name = self
                .branches
                .iter()
                .find(|b| b.kind == RefKind::Local)
                .map(|b| b.name.clone())
                .unwrap_or_default();
            empty_state(
                "branch",
                format!("Only {name}, nothing ahead"),
                "Branches you make in your terminal or editor show up here, with what they hold and their request. GitLance only reads the repository.".to_owned(),
                None,
            )
            .into_any_element()
        } else {
            match picked {
                Some(BranchRow::WorkingTree) => self.tree_preview(cx).into_any_element(),
                Some(BranchRow::Branch(ix)) => self.branch_preview(ix, cx),
                None => div().into_any_element(),
            }
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(super::gap()))
            .child(
                island()
                    .w(px(list_width(LIST)))
                    .flex_none()
                    .relative()
                    .border_color(self.zone_border(super::zones::Zone::Branches))
                    .child(self.widths.measure("branches"))
                    .child(
                        div()
                            .id("branches")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .pb(px(6.))
                            .children(list),
                    ),
            )
            .child(
                island().flex_1().min_w_0().bg(theme::editor()).child(
                    div()
                        .id("branch-preview")
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .overflow_y_scroll()
                        .child(preview),
                ),
            )
            .into_any_element()
    }

    /// A row of Branches: the working tree's, or a branch's name, when, how far from the main
    /// line, its request. `flat` (filtered) names a remote branch with its remote. Its anatomy
    /// (#79): the name and ↑ ↓ always; its request and when while the list has 260 px; HEAD and
    /// how far your review is under 360 px leave first; its card holds it all.
    fn branch_row(
        &self,
        row: BranchRow,
        picked: bool,
        flat: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let room = self.widths.room("branches");
        let head_name = self
            .branches
            .iter()
            .find(|b| b.is_head)
            .map(|b| b.name.clone());
        let (id, icon, line1, line2): (SharedString, &'static str, AnyElement, Option<AnyElement>) =
            match row {
                BranchRow::WorkingTree => {
                    let files = self.roots.tree.as_ref().map(|t| &t.1);
                    let line1 = div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(if self.wt.count == 0 {
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Clean")
                                .into_any_element()
                        } else {
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(plural(self.wt.count, "change"))
                                .into_any_element()
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(theme::muted())
                                .child(if self.wt.count == 0 {
                                    "nothing to commit"
                                } else {
                                    "not committed"
                                }),
                        )
                        .children(head_name.filter(|_| room.second()).map(|n| {
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(format!("on {n}"))
                        }))
                        .into_any_element();
                    let line2 = files.filter(|f| !f.is_empty()).map(|f| {
                        let names: Vec<&str> = f.iter().take(2).map(|f| f.path.as_str()).collect();
                        let more = f.len().saturating_sub(2);
                        div()
                            .truncate()
                            .child(if more > 0 {
                                format!("{} and {more} more", names.join(", "))
                            } else {
                                names.join(", ")
                            })
                            .into_any_element()
                    });
                    ("branch-row-wt".into(), "files", line1, line2)
                }
                BranchRow::Branch(ix) => {
                    let b = &self.branches[ix];
                    let name = match (b.kind, flat) {
                        (RefKind::Remote, false) => {
                            b.name.split_once('/').map_or(b.name.as_str(), |(_, n)| n)
                        }
                        _ => b.name.as_str(),
                    };
                    let fact = self.roots.facts.get(&b.refname);
                    let line1 = div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            super::hover::cut(
                                SharedString::from(format!("branch-name-{ix}")),
                                name.to_owned(),
                            )
                            .flex_1()
                            .font_family(theme::CODE_FONT)
                            .when(b.kind == RefKind::Local, |s| {
                                s.font_weight(FontWeight::SEMIBOLD)
                            })
                            .when(b.kind == RefKind::Remote, |s| s.text_color(theme::muted())),
                        )
                        .children(
                            self.head_part(b.tip)
                                .filter(|_| room.third())
                                .map(|(done, total)| super::lists::progress_badge(done, total)),
                        )
                        .children((room.third() && self.head_reviewed(b.tip)).then(|| {
                            div()
                                .text_size(px(11.))
                                .text_color(theme::added())
                                .child("✓ reviewed")
                        }))
                        .children(fact.filter(|_| room.second()).map(|f| {
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(format::ago(f.time))
                        }))
                        .into_any_element();
                    let mut parts: Vec<AnyElement> = Vec::new();
                    if b.is_head && room.third() {
                        parts.push(
                            div()
                                .px(px(6.))
                                .rounded_full()
                                .border_1()
                                .border_color(theme::accent())
                                .text_size(px(10.))
                                .text_color(theme::accent())
                                .child("HEAD")
                                .into_any_element(),
                        );
                    }
                    match fact.and_then(|f| f.against.as_ref()) {
                        Some((up, 0, 0)) => parts.push(
                            div()
                                .child(format!("up to date with {up}"))
                                .into_any_element(),
                        ),
                        Some((_, ahead, behind)) => parts.push(
                            div()
                                .child(format!("↑{ahead} ↓{behind}"))
                                .into_any_element(),
                        ),
                        None => {}
                    }
                    if let Some(m) = self.request_of_branch(ix).filter(|_| room.second()) {
                        let iid = m.iid;
                        parts.push(
                            div()
                                .id(SharedString::from(format!("branch-mr-{ix}")))
                                .text_color(theme::accent())
                                .cursor_pointer()
                                .hover(|s| s.underline())
                                .child(format!("{} →", words::number(iid)))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.select_request(iid, cx)
                                }))
                                .into_any_element(),
                        );
                    }
                    let line2 = (!parts.is_empty()).then(|| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .children(parts)
                            .into_any_element()
                    });
                    (
                        SharedString::from(format!("branch-row-{ix}")),
                        "branch",
                        line1,
                        line2,
                    )
                }
            };
        let card = match row {
            BranchRow::Branch(ix) => self.branch_card(ix),
            BranchRow::WorkingTree => None,
        };
        // The working tree has no card: its tooltip names its files.
        let tree_tip = (row == BranchRow::WorkingTree).then(|| {
            let mut tip = vec![if self.wt.count == 0 {
                "Clean: nothing to commit".to_owned()
            } else {
                format!("{}, not committed", plural(self.wt.count, "change"))
            }];
            if let Some(files) = self.roots.tree.as_ref().map(|t| &t.1) {
                tip.extend(files.iter().take(12).map(|f| f.path.clone()));
                if files.len() > 12 {
                    tip.push(format!("and {} more", files.len() - 12));
                }
            }
            tip.join("\n")
        });
        div().px(px(6.)).py(px(1.)).child(
            div()
                .id(id)
                .px(px(10.))
                .py(px(7.))
                .flex()
                .items_start()
                .gap(px(10.))
                .rounded(px(super::ROW_RADIUS))
                .cursor_pointer()
                .when(picked, |s| s.bg(theme::selected()))
                .when(!picked, |s| s.hover(|s| s.bg(theme::hover())))
                .child(
                    icons::icon(icon)
                        .mt(px(2.))
                        .size(px(15.))
                        .text_color(theme::faint()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(line1)
                        .children(
                            line2.map(|l| {
                                div().text_size(px(12.)).text_color(theme::muted()).child(l)
                            }),
                        ),
                )
                .when_some(card, |s, card| s.card(card))
                .when_some(tree_tip, |s, tip| s.tip(tip))
                .when_some(
                    match row {
                        BranchRow::Branch(ix) => Some(ix),
                        BranchRow::WorkingTree => None,
                    },
                    |s, ix| {
                        s.on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                this.branch_context(ix, event.position, cx)
                            }),
                        )
                    },
                )
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    if this.nav.pick_branch == Some(row) && event.click_count() > 1 {
                        return this.open_branch_row(row, cx);
                    }
                    this.zone = super::zones::Zone::Branches;
                    this.nav.pick_branch = Some(row);
                    if row == BranchRow::WorkingTree {
                        this.read_tree_files(cx);
                    }
                    cx.notify();
                })),
        )
    }

    /// Opens a row of Branches: the working tree, or the branch.
    pub(super) fn open_branch_row(&mut self, row: BranchRow, cx: &mut Context<Self>) {
        self.field = None;
        match row {
            BranchRow::WorkingTree => self.select_working_tree(cx),
            BranchRow::Branch(ix) => self.open_branch(ix, None, cx),
        }
    }

    /// The working tree picked: what is not committed, read only, and the way to its diff.
    fn tree_preview(&self, cx: &mut Context<Self>) -> Div {
        let head = self
            .branches
            .iter()
            .find(|b| b.is_head)
            .map(|b| b.name.clone());
        let files = self
            .roots
            .tree
            .as_ref()
            .map(|t| t.1.clone())
            .unwrap_or_default();
        let props = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(theme::muted())
            .child(
                icons::icon("files")
                    .size(px(15.))
                    .text_color(theme::muted()),
            )
            .child(match &head {
                Some(n) => format!("Working tree · on {n}"),
                None => "Working tree".to_owned(),
            })
            .child(div().flex_1())
            .child(
                div()
                    .text_color(theme::faint())
                    .child("read only: commit in your editor or terminal"),
            );
        let title = div()
            .text_size(px(19.))
            .line_height(px(26.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(if self.wt.count == 0 {
                "Nothing to commit".to_owned()
            } else {
                format!("{} not committed", plural(self.wt.count, "change"))
            });
        let list = (!files.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .py(px(4.))
                .rounded(px(8.))
                .border_1()
                .border_color(theme::island_border())
                .bg(theme::panel())
                .children(files.into_iter().take(40).map(|f| {
                    let (letter, color) = change_letter(f.change);
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(12.))
                        .h(px(26.))
                        .text_size(px(13.))
                        .child(
                            div()
                                .w(px(14.))
                                .text_size(px(11.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(color)
                                .child(letter),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(f.path))
                        .child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .text_size(px(11.))
                                .child(
                                    div()
                                        .text_color(theme::added())
                                        .child(format!("+{}", f.added)),
                                )
                                .child(
                                    div()
                                        .text_color(theme::removed())
                                        .child(format!("−{}", f.removed)),
                                ),
                        )
                }))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .px(px(28.))
            .py(px(22.))
            .max_w(px(820.))
            .child(props)
            .child(title)
            .children(list)
            .children((self.wt.count > 0).then(|| {
                div()
                    .flex()
                    .child(
                        primary_button("tree-open", "Open the diff", "↵").on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.field = None;
                                this.select_working_tree(cx)
                            },
                        )),
                    )
            }))
    }

    /// A branch picked: its last commit, how far it is from the main line, its request, Open.
    fn branch_preview(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(b) = self.branches.get(ix) else {
            return div().into_any_element();
        };
        let fact = self.roots.facts.get(&b.refname);
        let request = self.request_of_branch(ix).map(|m| (m.iid, m.title.clone()));
        let props = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(theme::muted())
            .child(
                icons::icon("branch")
                    .size(px(15.))
                    .text_color(theme::muted()),
            )
            .child(match b.kind {
                RefKind::Local if b.is_head => "Local branch · checked out".to_owned(),
                RefKind::Local => "Local branch".to_owned(),
                RefKind::Remote => "Remote branch".to_owned(),
            })
            .child(div().flex_1())
            .children(fact.map(|f| {
                div()
                    .text_color(theme::faint())
                    .child(format!("last commit {}", format::ago(f.time)))
            }));
        let title = div()
            .text_size(px(19.))
            .line_height(px(26.))
            .font_weight(FontWeight::SEMIBOLD)
            .font_family(theme::CODE_FONT)
            .child(b.name.clone());
        let mut facts: Vec<String> = Vec::new();
        if let Some((against, ahead, behind)) = fact.and_then(|f| f.against.as_ref()) {
            facts.push(if (*ahead, *behind) == (0, 0) {
                format!("up to date with {against}")
            } else {
                format!(
                    "{} ahead of {against}, {} behind",
                    plural(*ahead, "commit"),
                    behind
                )
            });
        }
        let last = fact.map(|f| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(theme::island_border())
                .bg(theme::panel())
                .child(div().text_size(px(13.)).child(f.summary.clone()))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(format!(
                            "{} · {} · {}",
                            format::short(b.tip),
                            f.author,
                            format::ago(f.time)
                        )),
                )
        });
        let mut actions: Vec<AnyElement> = vec![
            primary_button("branch-open", "Open", "↵")
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.open_branch_row(BranchRow::Branch(ix), cx)
                }))
                .into_any_element(),
        ];
        if let Some((iid, _)) = &request {
            let iid = *iid;
            actions.push(
                div()
                    .id("branch-request")
                    .child(bar_button(&format!("{} →", words::number(iid))))
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.select_request(iid, cx)
                        }),
                    )
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .px(px(28.))
            .py(px(22.))
            .max_w(px(820.))
            .child(props)
            .child(title)
            .children(request.map(|(iid, title)| {
                div().text_size(px(13.)).text_color(theme::muted()).child(
                    words::say(&format!(
                        "{} is open for this branch: {title}",
                        words::number(iid)
                    ))
                    .into_owned(),
                )
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .children(facts.into_iter().map(|f| div().child(f))),
            )
            .children(last)
            .child(div().flex().items_center().gap(px(10.)).children(actions))
            .into_any_element()
    }

    /// The list's filter in the page bar: ⌘F gives it the keyboard.
    pub(super) fn bar_filter(&self, placeholder: &'static str, cx: &Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .w(px(240.))
            .child(
                self.field_box("list-filter", Field::Branches, placeholder, cx)
                    .flex_1()
                    .when(self.field != Some(Field::Branches), |s| {
                        s.child(keys::caps("⌘F", keys::On::Plain))
                    }),
            )
            .into_any_element()
    }

    /// Over a list whose host cannot be read now: offline (the VPN), or the token refused; the
    /// last list stays under it.
    pub(super) fn list_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let error = self.requests.error.as_ref()?;
        let offline = error.starts_with("Can't reach");
        let since = self
            .requests
            .listed_at
            .map(|t| format!(" The list is from {}", format::clock(t)));
        let text = match (offline, &since) {
            (true, Some(since)) => format!("{error}{since} and refreshes when it is back."),
            (true, None) => format!("{error} The list comes when it is back."),
            (false, Some(since)) => format!("{error}.{since}."),
            (false, None) => format!("{error}."),
        };
        let token = error.contains("token");
        let color = if offline {
            theme::accent()
        } else {
            theme::removed()
        };
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(14.))
                .py(px(8.))
                .rounded(px(if super::flat() {
                    0.
                } else {
                    super::ISLAND_RADIUS
                }))
                .border_1()
                .border_color(color)
                .bg(theme::panel())
                .text_size(px(13.))
                .child(
                    icons::icon(if offline { "info" } else { "alert" })
                        .size(px(16.))
                        .text_color(color),
                )
                .child(div().flex_1().min_w_0().child(text))
                .child(
                    div()
                        .id("banner-action")
                        .child(bar_button(if token { "Renew token…" } else { "Retry" }))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if token {
                                this.settings_section = super::settings::Section::Requests;
                                this.open_settings(cx);
                            } else {
                                this.load_requests(cx);
                                cx.notify();
                            }
                        })),
                )
                .into_any_element(),
        )
    }
}

/// A group's heading in a list: "Needs your review · 3".
fn heading(text: String) -> Div {
    div()
        .px(px(16.))
        .pt(px(10.))
        .pb(px(4.))
        .text_size(px(11.))
        .text_color(theme::faint())
        .child(text)
}

/// The blue dot of a row not seen yet: a request you have never opened, or one of yours a thread
/// waits on; a blank of its width otherwise.
fn unread_dot(on: bool) -> Div {
    div()
        .flex_none()
        .mt(px(6.))
        .size(px(7.))
        .rounded_full()
        .when(on, |s| s.bg(theme::accent()))
}

/// One message centred in the content: an icon, what is the case, why, and the way on.
pub(super) fn empty_state(
    icon: &'static str,
    title: String,
    text: String,
    action: Option<AnyElement>,
) -> Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .px(px(24.))
        .child(icons::icon(icon).size(px(24.)).text_color(theme::muted()))
        .child(
            div()
                .text_size(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(title),
        )
        .child(
            div()
                .max_w(px(400.))
                .text_size(px(13.))
                .line_height(px(19.))
                .text_center()
                .text_color(theme::muted())
                .child(text),
        )
        .children(action.map(|a| div().pt(px(6.)).child(a)))
}

/// Rows still loading: two grey bars each.
pub(super) fn skeleton(rows: usize) -> Div {
    const LONG: [f32; 6] = [62., 48., 70., 55., 66., 44.];
    const SHORT: [f32; 6] = [40., 52., 34., 46., 38., 50.];
    div()
        .flex()
        .flex_col()
        .gap(px(22.))
        .children((0..rows).map(|n| {
            div()
                .flex()
                .flex_col()
                .gap(px(7.))
                .child(bar(LONG[n % 6], 10.))
                .child(bar(SHORT[n % 6], 7.))
        }))
}

fn bar(percent: f32, height: f32) -> Div {
    div()
        .w(gpui::relative(percent / 100.))
        .h(px(height))
        .rounded_full()
        .bg(theme::hover())
}

/// A small rounded label in a preview's head: the pipeline, the approvals.
fn pill(text: String, color: Rgba) -> Div {
    div()
        .flex_none()
        .px(px(8.))
        .h(px(22.))
        .flex()
        .items_center()
        .rounded_full()
        .border_1()
        .border_color(color)
        .text_size(px(12.))
        .text_color(color)
        .child(text)
}

/// The filled button of a preview, with its key.
fn primary_button(id: &'static str, label: &str, key: &'static str) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(28.))
        .px(px(12.))
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(super::ROW_RADIUS))
        .bg(theme::accent())
        .text_color(theme::on_accent())
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
        .child(words::say(label).into_owned())
        .child(keys::caps(key, keys::On::Filled))
}

/// A plain outlined button.
pub(super) fn bar_button(label: &str) -> Div {
    div()
        .flex_none()
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(super::ROW_RADIUS))
        .border_1()
        .border_color(theme::island_border())
        .cursor_pointer()
        .text_size(px(13.))
        .hover(|s| s.bg(theme::hover()))
        .child(words::say(label).into_owned())
}

fn change_letter(change: ChangeKind) -> (&'static str, Rgba) {
    match change {
        ChangeKind::Added => ("A", theme::added()),
        ChangeKind::Deleted => ("D", theme::removed()),
        ChangeKind::Modified => ("M", theme::accent()),
        ChangeKind::Renamed => ("R", theme::renamed()),
        ChangeKind::Copied => ("C", theme::renamed()),
        ChangeKind::TypeChanged => ("T", theme::warning()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_grouped_by_what_it_needs_from_you() {
        assert_eq!(group(true, true, Seen::Done), Group::Yours);
        assert_eq!(group(false, true, Seen::Never), Group::Asks);
        assert_eq!(group(false, false, Seen::Never), Group::Others);
        assert_eq!(group(false, false, Seen::Started), Group::Asks);
        assert_eq!(group(false, true, Seen::Earlier), Group::Updated);
        assert_eq!(group(false, true, Seen::Done), Group::Reviewed);
        assert!(Group::Asks < Group::Updated && Group::Others < Group::Yours);
    }

    #[test]
    fn a_projects_folds_and_opened_requests_are_kept_on_its_own_line() {
        let folded = HashSet::from([Group::Others, Group::Reviewed]);
        let opened = HashSet::from([415, 398]);
        let text = with_kept_list("b\tasks\t7\n", "a", &folded, &opened);
        assert_eq!(text, "a\treviewed,others\t398,415\nb\tasks\t7\n");
        assert_eq!(kept_list(&text, "a"), Some((folded, opened)));
        assert_eq!(
            kept_list(&text, "b"),
            Some((HashSet::from([Group::Asks]), HashSet::from([7])))
        );
        // Every group unfolded, nothing opened: a line still, so the defaults do not come back.
        let none = with_kept_list(&text, "a", &HashSet::new(), &HashSet::new());
        assert_eq!(
            kept_list(&none, "a"),
            Some((HashSet::new(), HashSet::new()))
        );
        assert_eq!(kept_list(&none, "c"), None);
    }

    #[test]
    fn the_band_above_the_lists_top_is_held_until_the_next_pushes_it_up() {
        // Bands at 0, 200 and 300; the list's top at 0: nothing is above it.
        assert_eq!(held_band(&[0., 200., 300.], 0.), None);
        assert_eq!(held_band(&[0., 200., 300.], 100.), Some((0, 0.)));
        // The next band 10 px below the top: the held one goes up by what it covers.
        assert_eq!(held_band(&[0., 200., 300.], 190.), Some((0, -18.)));
        assert_eq!(held_band(&[0., 200., 300.], 250.), Some((1, 0.)));
        assert_eq!(held_band(&[0., 200., 300.], 900.), Some((2, 0.)));
    }
}
