//! Merge requests in the branches island: their titles and authors, a click that opens the source
//! branch, and the discussions other people left on one, in the review panel. Read-only; GitLab
//! only (see `crate::mr`).

use super::nav::Page;
use super::px;
use super::roots::Group;
use super::rows::Row;
use super::tools::ToolView;
use super::waiting::Op;
use super::{Header, Selection, Workspace, format, plural, theme};
use crate::git::{RefKind, Repo, Version};
use crate::mr::{self, Draft, Mr, Pipeline, Thread};
use gpui::{ClickEvent, Context, FontWeight, SharedString, Task, WeakEntity, div, prelude::*};

/// Which requests the list shows: All · To review · Mine in Merge requests' bar (#70, #72).
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    All,
    /// The token's user is asked to review.
    ReviewMe,
    /// The token's user wrote it.
    Mine,
}

/// The merged or the closed requests (#114), read a page at a time as the list is scrolled.
#[derive(Default)]
pub struct Past {
    /// Which they are; Opened shows the open list.
    pub of: mr::MrState,
    pub list: Vec<Mr>,
    /// The pages read, and whether another may follow.
    pub pages: u32,
    pub more: bool,
    pub loading: bool,
    pub error: Option<String>,
    task: Option<Task<()>>,
}

#[derive(Default)]
pub struct Requests {
    pub scope: Scope,
    /// The project's members, for `@` completion (#51).
    pub members: Vec<(String, String)>,
    /// Members a mention does not reach (a private project's guests): "not notified" (#51).
    pub unreached: Vec<String>,
    pub available: bool,
    /// A GitLab project without a token: the rail offers to connect instead of listing.
    pub connectable: bool,
    pub list: Vec<Mr>,
    /// The state the list shows (Open ▾, #114), and the merged or closed read so far.
    pub view: mr::MrState,
    pub past: Past,
    /// Requests open in the menu that left the open list: what became of them (merged, closed),
    /// kept until closed there.
    pub left: Vec<Mr>,
    /// Requests whose head was fetched, its branch not being here: one try each.
    pub heads_fetched: std::collections::HashSet<u64>,
    /// Requests whose base or head the host compares from was fetched, not being here: one try
    /// each (#145).
    pub sides_fetched: std::collections::HashSet<u64>,
    /// Requests read one by one (gone from the open list, or a link's): one try each.
    pub tried: std::collections::HashSet<u64>,
    pub loading: bool,
    pub error: Option<String>,
    /// When the list last came in: an error or being offline says how old the list is (#72).
    pub listed_at: Option<i64>,
    /// The request of the open branch, when it is one's source branch.
    pub current: Option<u64>,
    /// The open request and the group it was in when it was opened: ⌥↓ goes on from its place
    /// there, not from Reviewed, where reviewing it to the end moved it (#136).
    pub walk: Option<(u64, Group)>,
    /// Its discussions are being read: Discussions shimmers instead of saying there are none.
    pub reading: bool,
    /// What shows of its discussions, pending comments and approvals: `base` with the changes
    /// still `waiting` on it (#139).
    pub threads: Vec<Thread>,
    /// One's own pending comments on the current request.
    pub drafts: Vec<Draft>,
    /// The request's newest pipeline.
    pub pipeline: Option<Pipeline>,
    /// Who approved the request, and where the reviewer stands.
    pub approvals: Option<mr::Approvals>,
    /// The host's last answer for them, and the tick the read that gave it began at.
    pub base: super::waiting::Base,
    pub from: u64,
    /// The changes sent to the host and not yet read back, on any request (#139).
    pub waiting: super::waiting::Waiting,
    reload_task: Option<Task<()>>,
    /// Its description, labels, milestone and assignees, for its Overview (#8, #73).
    pub details: Option<mr::Details>,
    /// The files of a request's whole diff when it was last loaded: its Changes tab's count, on a
    /// commit of it too (#73).
    pub files: Option<(u64, usize)>,
    /// Discussions written on an earlier version: what became of their line, by thread id (#46).
    pub histories: std::collections::HashMap<String, super::outdated::LineHistory>,
    pub history_task: Option<Task<()>>,
    /// The reactions on its notes, by note id (#9).
    pub awards: super::reactions::Awards,
    pub award_task: Option<Task<()>>,
    /// The pushes GitLab kept for a request, as versions of this repository.
    pub versions: Option<(u64, Vec<Version>)>,
    /// The base and head the host compares that request from — its latest push's, as given, here
    /// or not (#145).
    pub host: Option<(u64, HostSides)>,
    version_task: Option<Task<()>>,
    /// The token's username: one's own notes are the ones it wrote, and can be edited.
    pub login: Option<String>,
    task: Option<Task<()>>,
    thread_task: Option<Task<()>>,
}

/// A request's base and head as its host compares it (GitLab: the latest version's `diff_refs`;
/// GitHub: the base branch's and the head's commits); `None` where the host gave none.
pub type HostSides = (Option<git2::Oid>, Option<git2::Oid>);

/// How a request is compared once its host's sides are known (#145).
#[derive(Debug, PartialEq, Eq)]
pub enum Sides {
    /// The sides read from this clone already give the host's diff.
    Same,
    /// The host's base and head, both here: the request is compared from them.
    Host(git2::Oid, git2::Oid),
    /// The host names a commit this clone lacks — the base, the head or both —, to fetch first.
    Missing { base: bool, head: bool },
}

/// The host's sides against the ones read from this clone, `here`: the target's tip (a merged
/// request's merge commit's first parent) and the source branch's. A target not fetched lately
/// starts the compare at an old merge base, and every target commit after it shows as the
/// request's; a fast-forward merge puts the head in the target, and nothing shows.
pub fn sides(repo: &Repo, host: HostSides, here: (git2::Oid, git2::Oid)) -> Sides {
    let lacks = |id: Option<git2::Oid>| id.is_some_and(|id| !repo.has_commit(id));
    let (base, head) = (lacks(host.0), lacks(host.1));
    if base || head {
        return Sides::Missing { base, head };
    }
    let (base, head) = (host.0.unwrap_or(here.0), host.1.unwrap_or(here.1));
    if head == here.1 && repo.start_of(base, head) == repo.start_of(here.0, here.1) {
        Sides::Same
    } else {
        Sides::Host(base, head)
    }
}

impl Requests {
    /// Request `iid` wherever it was read: the open list, the merged or closed, or the ones that
    /// left the open list while open in the menu.
    pub fn find(&self, iid: u64) -> Option<&Mr> {
        self.list
            .iter()
            .chain(&self.past.list)
            .chain(&self.left)
            .find(|m| m.iid == iid)
    }
}

impl Workspace {
    /// The list shows the merged or the closed requests, or the open ones again (#114).
    pub(super) fn set_request_view(&mut self, state: mr::MrState, cx: &mut Context<Self>) {
        self.requests.view = state;
        self.nav.pick_request = None;
        if state != mr::MrState::Opened && self.requests.past.of != state {
            self.requests.past = Past {
                of: state,
                more: true,
                ..Past::default()
            };
            self.load_past_page(cx);
        }
        cx.notify();
    }

    /// The next page of the merged or closed requests, unless one is on its way or none follows.
    pub(super) fn load_past_page(&mut self, cx: &mut Context<Self>) {
        let past = &mut self.requests.past;
        let (Some(remote), state) = (self.web.clone(), past.of) else {
            return;
        };
        if past.loading || !past.more || state == mr::MrState::Opened {
            return;
        }
        past.loading = true;
        let page = past.pages + 1;
        past.task = Some(cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { mr::list_past(&remote, state, page) })
                .await;
            this.update(cx, |this, cx| {
                let past = &mut this.requests.past;
                if past.of != state {
                    return;
                }
                past.loading = false;
                match read {
                    Ok((list, more)) => {
                        past.pages = page;
                        past.more = more;
                        past.error = None;
                        let known: std::collections::HashSet<u64> =
                            past.list.iter().map(|m| m.iid).collect();
                        past.list
                            .extend(list.into_iter().filter(|m| !known.contains(&m.iid)));
                    }
                    Err(e) => {
                        past.more = false;
                        past.error = Some(format!("{e:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// The page that could not be read, again.
    pub(super) fn retry_past_page(&mut self, cx: &mut Context<Self>) {
        let past = &mut self.requests.past;
        past.error = None;
        past.more = true;
        self.load_past_page(cx);
        cx.notify();
    }

    /// What became of requests `iids`, open in the menu but gone from the open list, or a link's
    /// that is not open: each read once, kept in `left`; a link waiting for one goes on.
    pub(super) fn read_left(&mut self, iids: Vec<u64>, cx: &mut Context<Self>) {
        let iids: Vec<u64> = iids
            .into_iter()
            .filter(|iid| self.requests.tried.insert(*iid))
            .collect();
        let Some(remote) = self.web.clone().filter(|_| !iids.is_empty()) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    iids.into_iter()
                        .filter_map(|iid| mr::one(&remote, iid).ok())
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                for m in read {
                    if this.requests.find(m.iid).is_none() {
                        this.requests.left.push(m);
                    }
                }
                this.follow_pending_link(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Reads the open merge requests of the repository's project, when a token is configured.
    pub(super) fn load_requests(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            self.requests = Requests::default();
            return;
        };
        let available = mr::available(&remote);
        self.requests.available = available;
        self.requests.connectable = !available;
        if !available {
            self.requests.list.clear();
            self.requests.error = None;
            return;
        }
        self.requests.loading = true;
        // Who the token is, once: the menu counts the requests asking for them.
        let known = self.requests.login.is_some();
        self.requests.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let login = if known {
                        None
                    } else {
                        mr::username(&remote).ok()
                    };
                    let members = mr::members(&remote).unwrap_or_default();
                    let unreached = mr::unreached(&remote).unwrap_or_default();
                    mr::list(&remote).map(|list| (list, login, (members, unreached)))
                })
                .await;
            this.update(cx, |this, cx| {
                this.requests.loading = false;
                match result {
                    Ok((list, login, (members, unreached))) => {
                        this.requests.list = list;
                        this.requests.listed_at = Some(super::inbox::now());
                        if !members.is_empty() {
                            this.requests.members = members;
                        }
                        this.requests.unreached = unreached;
                        this.requests.error = None;
                        if login.is_some() {
                            this.requests.login = login;
                        }
                    }
                    Err(e) => this.requests.error = Some(format!("{e:#}")),
                }
                if let Some(ix) = this.branch {
                    this.sync_request(ix, cx);
                }
                this.requests_listed(cx);
                this.follow_pending_link(cx);
                this.load_notices(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// The request whose source branch is branch `ix` (`feat/x` and `origin/feat/x` alike), or
    /// whose head it is.
    pub(super) fn request_of_branch(&self, ix: usize) -> Option<&Mr> {
        let b = self.branches.get(ix)?;
        let name = match b.kind {
            RefKind::Remote => b.name.split_once('/').map_or(b.name.as_str(), |(_, n)| n),
            RefKind::Local => b.name.as_str(),
            RefKind::Request => {
                let iid = b.name.rsplit('/').next()?.parse().ok()?;
                return self.requests.find(iid);
            }
        };
        self.requests.list.iter().find(|m| m.source == name)
    }

    /// Reads the discussions, pending comments and approvals of the current request again (after
    /// a change to it) and swaps them in, nothing cleared first; a read that fails leaves what
    /// shows (#139).
    pub(super) fn reload_threads(&mut self, cx: &mut Context<Self>) {
        let (Some(iid), Some(remote)) = (self.requests.current, self.web.clone()) else {
            return;
        };
        let from = self.requests.waiting.tick();
        self.requests.reload_task = Some(cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    let drafts = mr::drafts(&remote, iid).ok();
                    let approvals = mr::approvals(&remote, iid).ok();
                    mr::threads(&remote, iid).map(|t| (t, drafts, approvals))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.requests.current != Some(iid) {
                    return;
                }
                if let Ok((threads, drafts, approvals)) = read {
                    let base = super::waiting::Base {
                        threads,
                        drafts: drafts.unwrap_or_else(|| this.requests.base.drafts.clone()),
                        approvals: approvals.or_else(|| this.requests.base.approvals.clone()),
                    };
                    this.take_base(iid, from, base, cx);
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Follows the branch selection: a request opened as one (or being opened) whose source
    /// branch is branch `ix` becomes the current one and its discussions are read. A branch opened
    /// in Branches is a branch: no request rides on it (#71).
    pub(super) fn sync_request(&mut self, ix: usize, cx: &mut Context<Self>) {
        let iid = self
            .pending_compare
            .as_ref()
            .or(self.open_request.as_ref())
            .map(|r| r.2)
            .filter(|iid| {
                // Its source branch, or a branch at its head (a merged one's, #114).
                self.request_of_branch(ix).is_some_and(|m| m.iid == *iid)
                    || self
                        .requests
                        .find(*iid)
                        .zip(self.branches.get(ix))
                        .is_some_and(|(m, b)| {
                            m.state != mr::MrState::Opened && b.tip.to_string() == m.head
                        })
            });
        if iid == self.requests.current && iid.is_none() {
            return;
        }
        if iid != self.requests.current {
            self.requests.threads.clear();
            self.requests.drafts.clear();
            self.requests.base = Default::default();
            self.requests.reload_task = None;
            self.requests.pipeline = None;
            self.requests.approvals = None;
            self.requests.details = None;
            self.requests.histories.clear();
            self.requests.awards.clear();
            self.draft_at = None;
        }
        self.requests.current = iid;
        if let Some(iid) = iid {
            if self.requests.walk.is_none_or(|(walked, _)| walked != iid)
                && let Some(m) = self.requests.find(iid)
            {
                self.requests.walk = Some((iid, self.group_of(m)));
            }
            self.mark_opened(iid);
        }
        self.requests.reading = false;
        let (Some(iid), Some(remote)) = (iid, self.web.clone()) else {
            return;
        };
        let known = self.requests.login.is_some();
        self.requests.reading = true;
        let from = self.requests.waiting.tick();
        self.requests.thread_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // Drafts need a recent GitLab; without them the rest still works.
                    let drafts = mr::drafts(&remote, iid).unwrap_or_default();
                    let pipeline = mr::pipeline(&remote, iid).ok().flatten();
                    let approvals = mr::approvals(&remote, iid).ok();
                    let details = mr::details(&remote, iid).ok();
                    let login = if known {
                        None
                    } else {
                        mr::username(&remote).ok()
                    };
                    mr::threads(&remote, iid)
                        .map(|t| (t, drafts, pipeline, approvals, login, details))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.requests.current == Some(iid) {
                    this.requests.reading = false;
                    match result {
                        Ok((threads, drafts, pipeline, approvals, login, details)) => {
                            if login.is_some() {
                                this.requests.login = login;
                            }
                            this.requests.details = details;
                            this.requests.pipeline = pipeline;
                            let base = super::waiting::Base {
                                threads,
                                drafts,
                                approvals,
                            };
                            this.take_base(iid, from, base, cx);
                            // A link to a comment waits for the discussions; the diff is in by now
                            // or comes later and tries again.
                            let diff_in = this.showing_request();
                            this.resolve_spot(diff_in, cx);
                        }
                        Err(e) => this.requests.error = Some(format!("{e:#}")),
                    }
                    this.refresh_rows();
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Opens merge request `iid`: its branch in the lists and, as the diff, what the branch adds to
    /// its target — the diff the discussions and new comments belong to.
    pub(super) fn select_request(&mut self, iid: u64, cx: &mut Context<Self>) {
        let Some(m) = self.requests.find(iid).cloned() else {
            return;
        };
        let (source, target) = (m.source.clone(), m.target.clone());
        self.load_request_versions(iid, cx);
        if let Some(m) = self.requests.find(iid) {
            let (label, detail) = (m.title.clone(), super::words::number(iid));
            self.remember_place(super::recents::Key::Request(iid), label, detail);
        }
        let pick = |name: &str| {
            let remote = self.branches.iter().position(|b| {
                b.kind == RefKind::Remote && b.name.split_once('/').is_some_and(|(_, n)| n == name)
            });
            remote.or_else(|| {
                self.branches
                    .iter()
                    .position(|b| b.kind == RefKind::Local && b.name == name)
            })
        };
        self.pending_compare = None;
        // A GitHub pull request from a fork has no branch here; `pr/<n>` is where the hint below
        // fetches its head.
        let github = self.web.as_ref().is_some_and(|w| w.github);
        let head = crate::git::request_ref(iid);
        let found = pick(&source)
            // A merged or closed one's branch may have moved on since.
            .filter(|&ix| {
                m.state == mr::MrState::Opened || self.branches[ix].tip.to_string() == m.head
            })
            .or_else(|| github.then(|| pick(&format!("pr/{iid}"))).flatten())
            .or_else(|| self.branches.iter().position(|b| b.refname == head));
        let Some(src) = found else {
            // Not here: its head is fetched from the host, once, and then it opens (#114).
            if self.requests.heads_fetched.insert(iid) {
                return self.fetch_request_head(iid, github, cx);
            }
            self.error = Some(
                if github {
                    format!(
                        "#{iid}: its branch {source} is not here (a fork's, or not fetched) — fetch it: git fetch origin pull/{iid}/head:refs/remotes/origin/pr/{iid}"
                    )
                } else {
                    format!(
                        "!{iid}: the branch {source} is not here yet — fetch (Settings → Fetch in the background, or git fetch)."
                    )
                }
                .into(),
            );
            cx.notify();
            return;
        };
        self.error = None;
        let tgt = pick(&target);
        // Its page now, before the branch it is read from replaces what is loaded (#71).
        self.arrive(super::nav::Page::Request(iid));
        if let Some(tgt) = tgt {
            let side = |ix: usize| (self.branches[ix].name.clone(), self.branches[ix].tip);
            let mut base = side(tgt);
            // Merged: the target as it was, the merge commit's first parent, so the diff is what
            // the request brought, not empty.
            if let Some(parent) = m
                .merged_with
                .as_deref()
                .and_then(|sha| git2::Oid::from_str(sha).ok())
                .zip(self.root.as_deref())
                .and_then(|(id, root)| Repo::open(root).ok()?.parent_of(id).ok())
            {
                base.1 = parent;
            }
            self.pending_compare = Some((base, side(src), iid));
        }
        self.select_branch(src, None, cx);
    }

    /// Fetches request `iid`'s head (its branch is gone, or a fork's), reads the branches again
    /// and opens it; says why when the fetch fails.
    fn fetch_request_head(&mut self, iid: u64, github: bool, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let head = self
            .requests
            .find(iid)
            .map(|m| m.head.clone())
            .filter(|h| !h.is_empty());
        let news = format!("Fetching {}…", super::words::number(iid));
        self.toast_info(news.clone().into(), cx);
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    crate::git::fetch::fetch_request(&root, iid, github, head.as_deref())?;
                    Repo::open(&root)
                        .and_then(|r| r.branches())
                        .map_err(|e| format!("{e:#}"))
                })
                .await;
            this.update(cx, |this, cx| {
                this.toast_over(&news, cx);
                match read {
                    Ok(branches) => {
                        this.branches = branches;
                        this.select_request(iid, cx);
                    }
                    Err(e) => {
                        this.error = Some(
                            format!(
                                "{}: its head could not be fetched — {e}",
                                super::words::number(iid)
                            )
                            .into(),
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// ⌥⌘L: closes the request on screen, back to the list (as ⌘W on it does).
    pub(super) fn leave_request(&mut self, cx: &mut Context<Self>) {
        if let Some(iid) = self.requests.current {
            self.close_open(&super::nav::Item::Request(iid), cx);
        }
    }

    /// The request's state goes: what is loaded is its branch, as a branch.
    pub(super) fn drop_request(&mut self, cx: &mut Context<Self>) {
        self.open_request = None;
        self.pending_compare = None;
        if let Some(ix) = self.branch {
            self.sync_request(ix, cx);
        }
    }

    /// ⌥↓: the next merge request to review (`next_request`); ⌥↑: the previous one in the list,
    /// reviewed or not (to look again at what was just done).
    pub(super) fn step_request(&mut self, forward: bool, cx: &mut Context<Self>) {
        let shown = self.shown_requests();
        let n = shown.len();
        if n == 0 {
            return;
        }
        let next = if forward {
            self.next_request()
        } else {
            let at = shown
                .iter()
                .position(|m| Some(m.iid) == self.requests.current);
            Some(shown[at.map_or(n - 1, |a| (a + n - 1) % n)])
        };
        let Some(next) = next.map(|m| m.iid) else {
            self.error =
                Some("Every other merge request in the list is reviewed, or yours.".into());
            return cx.notify();
        };
        self.done_card = false;
        self.select_request(next, cx);
    }

    /// The next request to review below the open one, going round the list once: the first that
    /// is not reviewed at its head and not yours (#81). Counted from where the open one was when
    /// it was opened — reviewed to the end, it has moved down to Reviewed (#136).
    fn next_request(&self) -> Option<&Mr> {
        let mut shown = self.shown_requests();
        let walk = self
            .requests
            .walk
            .filter(|(iid, _)| Some(*iid) == self.requests.current);
        if let Some((walked, was)) = walk
            && self.requests.view == mr::MrState::Opened
        {
            shown.sort_by_key(|m| {
                let group = if m.iid == walked {
                    was
                } else {
                    self.group_of(m)
                };
                (group, std::cmp::Reverse(m.updated))
            });
        }
        let at = shown
            .iter()
            .position(|m| Some(m.iid) == self.requests.current);
        let open: Vec<bool> = shown
            .iter()
            .map(|m| matches!(self.group_of(m), Group::Asks | Group::Updated | Group::New))
            .collect();
        next_to_review(&open, at).map(|ix| shown[ix])
    }

    /// After branch `self.branch` in the branch list, the next branch of the same kind (local or
    /// remote) whose review is not done, going round; never the checked-out one.
    fn next_unreviewed_branch(&self) -> Option<usize> {
        let at = self.branch?;
        let kind = self.branches.get(at)?.kind;
        let n = self.branches.len();
        (1..n).map(|step| (at + step) % n).find(|&ix| {
            let b = &self.branches[ix];
            b.kind == kind && !b.is_head && !self.head_reviewed(b.tip)
        })
    }

    /// The card that comes up when the last file is marked: what to do next, from the keyboard.
    pub(super) fn render_done_card(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        if !self.done_card {
            return None;
        }
        let total = self.diff.as_ref()?.files.len();
        let current = self.requests.current;
        // The next merge request in request mode, else the next branch.
        let next = match current {
            Some(_) => self
                .next_request()
                .map(|m| (format!("Next: !{} {}", m.iid, m.title), Err(m.iid))),
            None => self
                .next_unreviewed_branch()
                .map(|ix| (format!("Next: {}", self.branches[ix].name), Ok(ix))),
        };
        let button = |id: &'static str, label: String, key: &'static str, primary: bool| {
            div()
                .id(id)
                .h(px(28.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(super::ROW_RADIUS))
                .cursor_pointer()
                .when(primary, |s| s.bg(theme::added()).text_color(theme::base()))
                .when(!primary, |s| s.bg(theme::hover()).text_color(theme::text()))
                .hover(|s| s.opacity(0.85))
                .child(div().min_w_0().truncate().child(label))
                .child(super::keys::caps(
                    key,
                    if primary {
                        super::keys::On::Filled
                    } else {
                        super::keys::On::Plain
                    },
                ))
        };
        Some(
            div()
                .absolute()
                .bottom(px(16.))
                .right(px(16.))
                .max_w(px(460.))
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .rounded(px(super::ISLAND_RADIUS))
                .border_2()
                .border_color(theme::added())
                .bg(theme::panel())
                .shadow_lg()
                .font_family(theme::UI_FONT)
                .text_size(px(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::added())
                                .child(match current {
                                    Some(iid) => format!(
                                        "All {} of !{iid} reviewed ✓",
                                        plural(total, "file")
                                    ),
                                    None => format!("All {} reviewed ✓", plural(total, "file")),
                                }),
                        )
                        .child(
                            div()
                                .id("done-card-close")
                                .px_1()
                                .rounded(px(4.))
                                .text_color(theme::faint())
                                .cursor_pointer()
                                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                                .child("esc")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.done_card = false;
                                    cx.notify();
                                })),
                        ),
                )
                .children(
                    self.requests
                        .approvals
                        .as_ref()
                        .filter(|a| a.can && !a.mine && current.is_some())
                        .map(|_| {
                            button(
                                "done-approve",
                                format!("Approve !{}", current.unwrap_or_default()),
                                "a",
                                false,
                            )
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_approval(cx)),
                            )
                        }),
                )
                .children(next.clone().map(|(label, to)| {
                    button(
                        "done-next",
                        label,
                        if to.is_err() { "⌥↓" } else { "" },
                        true,
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.done_card = false;
                            match to {
                                Err(iid) => this.select_request(iid, cx),
                                Ok(ix) => this.open_branch(ix, None, cx),
                            }
                        },
                    ))
                }))
                .children(current.map(|_| {
                    button("done-leave", "Back to your branch".to_owned(), "⌥⌘L", false).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.done_card = false;
                            this.leave_request(cx);
                        }),
                    )
                }))
                .children(next.is_none().then(|| {
                    div()
                        .text_color(theme::muted())
                        .child(if current.is_some() {
                            "Nothing left to review: every other merge request is reviewed too."
                        } else {
                            "Nothing left to review: every other branch is reviewed too."
                        })
                })),
        )
    }

    /// Reads the pushes GitLab kept for request `iid`; they become the versions timeline once the
    /// request's diff is open, and the latest one's base and head are what it is compared from
    /// (#145).
    fn load_request_versions(&mut self, iid: u64, cx: &mut Context<Self>) {
        self.requests.versions = None;
        self.requests.host = None;
        let (Some(remote), Some(root)) = (self.web.clone(), self.root.clone()) else {
            return;
        };
        self.requests.version_task = Some(cx.spawn(async move |this, cx| {
            let built = cx
                .background_executor()
                .spawn(async move {
                    let read = mr::versions(&remote, iid)?;
                    let oid = |sha: &str| git2::Oid::from_str(sha).ok();
                    let host: Option<HostSides> = read.last().map(|p| (oid(&p.base), oid(&p.head)));
                    let pushes: Vec<_> = read
                        .iter()
                        .filter_map(|p| Some((oid(&p.head)?, oid(&p.base), p.created)))
                        .collect();
                    anyhow::Ok((Repo::open(&root)?.versions_from_pushes(&pushes), host))
                })
                .await;
            this.update(cx, |this, cx| {
                if let Ok((list, host)) = built {
                    this.requests.versions = Some((iid, list));
                    this.requests.host = host.map(|h| (iid, h));
                    this.follow_host_sides(iid, cx);
                    this.apply_mr_versions();
                    // Two versions of it waited for them (#71).
                    this.finish_pending(cx);
                    this.compute_histories(cx);
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Request `iid` is compared from the base and head its host gives, once they are known
    /// (#145): the sides waiting for its branch to load take them, or the open request is compared
    /// again when its diff would differ. A side that is not here is fetched first, once.
    fn follow_host_sides(&mut self, iid: u64, cx: &mut Context<Self>) {
        let Some(host) = self
            .requests
            .host
            .filter(|(i, _)| *i == iid)
            .map(|(_, h)| h)
        else {
            return;
        };
        let waiting = self.pending_compare.as_ref().is_some_and(|p| p.2 == iid);
        let open = if waiting {
            self.pending_compare.clone()
        } else {
            self.open_request.clone().filter(|o| o.2 == iid)
        };
        let (Some((base, head, _)), Some(root)) = (open, self.root.clone()) else {
            return;
        };
        let Ok(repo) = Repo::open(&root) else {
            return;
        };
        match sides(&repo, host, (base.1, head.1)) {
            Sides::Same => {}
            Sides::Missing { base, head } => {
                if self.requests.sides_fetched.insert(iid) {
                    self.fetch_request_sides(iid, base, head, cx);
                }
            }
            Sides::Host(b, h) => {
                let (base, head) = ((base.0, b), (head.0, h));
                if waiting {
                    self.pending_compare = Some((base, head, iid));
                    return;
                }
                self.open_request = Some((base.clone(), head.clone(), iid));
                // The Commits tab is read again; a commit or a run picked in it stays picked.
                let id = |ix: usize| self.commits.get(ix).map(|c| c.id);
                let picked = match self.selection {
                    Selection::Commit(ix) => id(ix).map(|a| (a, a)),
                    Selection::Commits { newest, oldest } => id(newest).zip(id(oldest)),
                    _ => None,
                };
                self.show_request_commits(&base, &head);
                let at = |id: git2::Oid| self.commits.iter().position(|c| c.id == id);
                if let Some((a, b)) = picked
                    && let (Some(a), Some(b)) = (at(a), at(b))
                {
                    self.selection = match self.selection {
                        Selection::Commit(_) => Selection::Commit(a),
                        _ => Selection::Commits {
                            newest: a,
                            oldest: b,
                        },
                    };
                }
                let whole = self.request_loaded(iid)
                    || (self.diff.is_none() && self.nav_page() == Some(&Page::Request(iid)));
                if self.selection == Selection::None && whole {
                    self.run_compare(base, head, true, Some(iid), cx);
                }
                cx.notify();
            }
        }
    }

    /// Fetches what request `iid`'s host compares it from and this clone lacks — its target
    /// branch for the base, its head for the head —, then compares it from them; says why when
    /// the fetch fails.
    fn fetch_request_sides(&mut self, iid: u64, base: bool, head: bool, cx: &mut Context<Self>) {
        let (Some(root), Some(m)) = (self.root.clone(), self.requests.find(iid)) else {
            return;
        };
        let target = m.target.clone();
        let github = self.web.as_ref().is_some_and(|w| w.github);
        let sha = self
            .requests
            .host
            .and_then(|(_, h)| h.1)
            .map(|h| h.to_string());
        let number = super::words::number(iid);
        let news = if base {
            format!("Fetching {target} for {number}…")
        } else {
            format!("Fetching {number}…")
        };
        self.toast_info(news.clone().into(), cx);
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    if base {
                        crate::git::fetch::fetch_branch(&root, &target)?;
                    }
                    if head {
                        crate::git::fetch::fetch_request(&root, iid, github, sha.as_deref())?;
                    }
                    Repo::open(&root)
                        .and_then(|r| r.branches())
                        .map_err(|e| format!("{e:#}"))
                })
                .await;
            this.update(cx, |this, cx| {
                this.toast_over(&news, cx);
                match read {
                    Ok(branches) => {
                        let open = this
                            .branch
                            .and_then(|ix| this.branches.get(ix))
                            .map(|b| b.refname.clone());
                        this.branches = branches;
                        this.branch =
                            open.and_then(|r| this.branches.iter().position(|b| b.refname == r));
                        this.follow_host_sides(iid, cx);
                    }
                    Err(e) => {
                        this.error = Some(
                            format!("{number}: what the host compares it from could not be fetched — {e}")
                                .into(),
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Shows GitLab's versions of the open request in the timeline, when there is more than one.
    pub(super) fn apply_mr_versions(&mut self) {
        let Some(iid) = self.open_request.as_ref().map(|r| r.2) else {
            return;
        };
        if let Some((for_iid, list)) = &self.requests.versions
            && *for_iid == iid
            && list.len() > 1
        {
            self.versions = list.clone();
        }
    }

    /// The commits list of an open request: only what the request adds to its target.
    pub(super) fn show_request_commits(&mut self, base: &super::NamedTip, head: &super::NamedTip) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if let Ok(list) = Repo::open(&root).and_then(|r| r.range_log(base.1, head.1, true)) {
            self.commits = list;
            self.refilter();
        }
    }

    /// Is the whole request whose discussions are loaded on screen, as its page (#71)? Its keys
    /// (`a`, ⇧N, Submit review) act only then, never on one loaded under another page.
    pub(super) fn showing_request(&self) -> bool {
        self.requests.current.is_some_and(|iid| {
            self.request_loaded(iid)
                && self.nav.space == super::spaces::Space::Requests
                && self.nav_page() == Some(&super::nav::Page::Request(iid))
        })
    }

    /// The diff loaded is the whole of request `iid`, whatever page is on screen.
    pub(super) fn request_loaded(&self, iid: u64) -> bool {
        self.requests.current == Some(iid)
            && matches!(
                self.diff.as_ref().map(|d| &d.header),
                Some(Header::Compare { request: Some(i), .. }) if *i == iid
            )
    }

    /// The discussions on lines of the open file: (index, old side, line).
    pub(super) fn request_threads_here(&self) -> Vec<(usize, bool, u32)> {
        self.current_path()
            .map(|path| self.request_threads_in(&path))
            .unwrap_or_default()
    }

    /// The discussions on lines of file `path`.
    pub(super) fn request_threads_in(&self, path: &str) -> Vec<(usize, bool, u32)> {
        self.requests
            .threads
            .iter()
            .enumerate()
            .filter(|(_, t)| t.path.as_deref() == Some(path))
            .filter_map(|(ix, t)| {
                // Written on an earlier version: drawn at its line now, or not at all (#46).
                if self.thread_is_old(t) {
                    let h = self.requests.histories.get(&t.id)?;
                    return h.line_now.map(|n| (ix, false, n));
                }
                match (t.new_line, t.old_line) {
                    (Some(n), _) => Some((ix, false, n)),
                    (None, Some(o)) => Some((ix, true, o)),
                    _ => None,
                }
            })
            .collect()
    }

    /// A discussion of the request, drawn under its line: a frame with a header bar (it folds the
    /// discussion), the notes on a timeline, and a footer to reply and resolve. A click anywhere
    /// gives it the keyboard, so `r` `x` `o` act on it.
    pub(super) fn render_request_thread(
        &self,
        ix: usize,
        indent: f32,
        this: WeakEntity<Self>,
    ) -> gpui::AnyElement {
        use super::discussion::{self, Kind, Source};
        let Some(t) = self.requests.threads.get(ix) else {
            return div().into_any_element();
        };
        let open = self.mr_open(ix);
        let focused = self.thread_at == Some(ix);
        // On its way to the host: faded, and nothing to act on until it has an id (#139).
        let waits = self.thread_waits(&t.id);
        let sending = super::waiting::is_temp_thread(&t.id);
        let can_act = self.request_target().is_some() && !sending;
        let (fold, pick, reply, resolve, raw, edit) = (
            this.clone(),
            this.clone(),
            this.clone(),
            this.clone(),
            this.clone(),
            this.clone(),
        );
        let history = self.render_history(ix, this.clone());
        let selecting = self.selecting(this.clone());
        let first = &t.notes[0];
        let author = self
            .requests
            .current
            .and_then(|iid| self.requests.find(iid))
            .map(|m| m.author.clone());
        let rich = t
            .notes
            .iter()
            .any(|n| super::markdown::has_markdown(&n.body));
        let bar = discussion::bar(("thread-bar", ix), open)
            .child(discussion::source_pill(Source::GitLab))
            .children((!open).then(|| {
                div()
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child(first.author.clone())
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(open, |s| {
                        s.child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::text())
                                        .child("Discussion"),
                                )
                                .child(format!("· {}", plural(t.notes.len(), "comment"))),
                        )
                    })
                    .when(!open, |s| s.child(self.one_line(&first.body))),
            )
            .children((open && rich).then(|| {
                self.rich_switch(("thread-rich", ix))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        raw.update(cx, |this, cx| this.toggle_rich_comments(cx))
                            .ok();
                    })
            }))
            .children(t.resolvable.then(|| {
                discussion::state(t.resolved).when(waits.is_some() && !sending, |s| {
                    s.opacity(super::waiting::FADED)
                })
            }))
            .children(
                waits
                    .filter(|_| !sending)
                    .map(|word| div().flex_none().text_color(theme::muted()).child(word)),
            )
            .tooltip_show_delay(crate::ui::hover::LONG)
            // Folded, a long hover reads its start and how many replied (X59).
            .tooltip({
                let said: gpui::SharedString = if open {
                    "Collapse  O".into()
                } else {
                    let text = crate::markdown::plain(&first.body);
                    let start = match text.char_indices().nth(280) {
                        Some((at, _)) => format!("{}…", &text[..at]),
                        None => text,
                    };
                    let replies = match t.notes.len() - 1 {
                        0 => "no replies".to_owned(),
                        n => plural(n, "reply"),
                    };
                    format!("{start}\n{} · {replies}\nExpand  O", first.author).into()
                };
                move |_, cx| cx.new(|_| super::TipOwned(said.clone())).into()
            })
            // A double click folds or opens it once, as one click does.
            .on_click(move |e: &ClickEvent, _, cx| {
                if e.click_count() > 1 {
                    return;
                }
                fold.update(cx, |this, cx| {
                    this.thread_at = Some(ix);
                    this.set_thread_open(ix, !open, cx)
                })
                .ok();
            });
        // Replying: the timeline runs on from the last note to my avatar.
        let replying = self.compose.as_ref().is_some_and(|c| {
            matches!(
                c.target,
                super::comments::Target::Reply(i) | super::comments::Target::ThreadNote(i)
                    if i == ix
            )
        });
        let last = t.notes.len() - 1;
        let mine = self.my_note_in(ix);
        let notes = t.notes.iter().enumerate().map(|(n, note)| {
            let change = super::comments::Change::Note {
                thread: ix,
                note: note.id,
            };
            let edit_box = self.edit_box(change, edit.clone());
            let own = self.requests.login.as_deref() == Some(note.login.as_str());
            // Waiting on the host: faded, its time saying what for; a new one takes no actions.
            let (time, note_waits) = self.note_time(note.id, note.created);
            let can_act = can_act && !super::waiting::is_temp(note.id);
            let editable = can_act && self.compose.is_none() && own && !note_waits;
            let (hover, pills) = (edit.clone(), self.reaction_pills(note.id, edit.clone()));
            let bar = can_act
                .then(|| self.hover_bar(ix, note.id, &note.login, edit.clone()))
                .flatten();
            let note_id = note.id;
            let note_el = discussion::note(
                &note.author,
                own,
                (author.as_deref() == Some(note.author.as_str())).then_some("Author"),
                time,
                div()
                    .child(match edit_box {
                        Some(edit_box) => edit_box.into_any_element(),
                        None => super::markdown::body(
                            &note.body,
                            self.settings.rich_comments,
                            format!("md-note-{ix}-{n}"),
                            self.link_base(),
                            Some(selecting.clone()),
                        ),
                    })
                    .children(pills)
                    .into_any_element(),
                n == last && !replying,
                (bar.is_some() || editable).then(|| {
                    // `e` edits the last of one's notes in the discussion with the keyboard.
                    let key = (focused && mine == Some(note.id)).then_some("e");
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .children(bar)
                        .children(editable.then(|| {
                            super::comments::edit_button(
                                ("edit-note", note.id as usize),
                                key,
                                self.tight,
                                change,
                                edit.clone(),
                            )
                        }))
                        // One's own note goes from GitLab too, after a confirm (#76).
                        .children(
                            editable.then(|| self.delete_note_button(ix, note_id, edit.clone())),
                        )
                        .into_any_element()
                }),
                (!own).then(|| {
                    let card =
                        std::rc::Rc::new(self.person_card(&note.author, &note.login, this.clone()));
                    let build: discussion::Card = std::rc::Rc::new(move |_, cx| {
                        let c = card.clone();
                        cx.new(|_| super::hover::PersonCard::clone_of(&c)).into()
                    });
                    (build, note.created)
                }),
            );
            // Hovering a note shows its reaction bar.
            div()
                .id(("note-hover", note_id as usize))
                .when(note_waits && !sending, |s| s.opacity(super::waiting::FADED))
                .child(note_el)
                .on_hover(move |hovered: &bool, _, cx| {
                    let hovered = *hovered;
                    hover
                        .update(cx, |this, cx| {
                            if hovered && this.hover_note != Some(note_id) {
                                this.hover_note = Some(note_id);
                                cx.notify();
                            } else if !hovered && this.hover_note == Some(note_id) {
                                this.hover_note = None;
                                cx.notify();
                            }
                        })
                        .ok();
                })
        });
        // Replying: the box opens where "Reply…" was, inside the discussion.
        // The reply row: my avatar on the timeline's column, then the one input — closed, one line
        // with Resolve beside it; open, grown inside its own outline (`comments::reply_editor`).
        let my_avatar = || {
            div()
                .w(px(28.))
                .flex()
                .flex_none()
                .justify_center()
                .pt(px(5.))
                .child(discussion::avatar(&self.me, true, 24.))
        };
        let reply_row = if replying {
            self.reply_editor(reply.clone()).map(|editor| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(my_avatar())
                    .child(editor)
            })
        } else {
            can_act.then(|| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(my_avatar())
                    .child(
                        div()
                            .id(("thread-reply", ix))
                            .flex_1()
                            .h(px(34.))
                            .pl(px(10.))
                            .pr(px(6.))
                            .flex()
                            .items_center()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(theme::selected())
                            .bg(theme::base())
                            .text_color(theme::muted())
                            .cursor_text()
                            .hover(|s| s.border_color(theme::muted()))
                            .child(div().flex_1().child("Reply…"))
                            .children(
                                (!self.tight)
                                    .then(|| super::keys::caps("r", super::keys::On::Plain)),
                            )
                            .on_click(move |_, _, cx| {
                                reply.update(cx, |this, cx| this.start_reply(ix, cx)).ok();
                            }),
                    )
                    .children(t.resolvable.then(|| {
                        let (icon, label) = if t.resolved {
                            ("rotate-ccw", "Reopen")
                        } else {
                            ("check", "Resolve")
                        };
                        discussion::icon_button(
                            ("thread-resolve", ix),
                            icon,
                            label,
                            Some("x"),
                            None,
                            Kind::Plain,
                            self.tight,
                        )
                        .h(px(34.))
                        .on_click(move |_, _, cx| {
                            resolve
                                .update(cx, |this, cx| this.toggle_resolved(ix, cx))
                                .ok();
                        })
                    }))
            })
        };
        div()
            .w_full()
            .pl(px(indent))
            .pr(px(12.))
            .py(px(6.))
            .child(
                discussion::frame(Source::GitLab, focused)
                    .id(("thread", ix))
                    .when(sending, |s| s.opacity(super::waiting::FADED))
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                        pick.update(cx, |this, cx| {
                            if this.thread_at != Some(ix) {
                                this.thread_at = Some(ix);
                                cx.notify();
                            }
                        })
                        .ok();
                    })
                    .child(bar)
                    // Its notes open and fold in size and alpha (#56, 10; #110).
                    // Built only while it shows: a folded one draws no notes.
                    .children(
                        (open || self.motion.recent(&format!("thread-{ix}")))
                            .then(|| {
                                self.folding(
                                    format!("thread-body-{ix}"),
                                    &format!("thread-{ix}"),
                                    open,
                                    div()
                                        .flex()
                                        .flex_col()
                                        .children(history)
                                        .child(div().flex().flex_col().children(notes))
                                        .children(reply_row),
                                )
                            })
                            .flatten(),
                    ),
            )
            .into_any_element()
    }

    /// The discussions on lines of the files of this diff, in reading order.
    fn thread_order(&self) -> Vec<usize> {
        let Some(diff) = &self.diff else {
            return Vec::new();
        };
        let mut order: Vec<(usize, u32, usize)> = self
            .requests
            .threads
            .iter()
            .enumerate()
            .filter_map(|(ix, t)| {
                let file = diff
                    .files
                    .iter()
                    .position(|f| Some(f.path()) == t.path.as_deref())?;
                Some((file, t.new_line.or(t.old_line)?, ix))
            })
            .collect();
        order.sort_unstable();
        order.into_iter().map(|(_, _, ix)| ix).collect()
    }

    /// ⇧n / ⇧p: the next or previous discussion of the request, shown in the diff.
    pub(super) fn step_thread(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.showing_request() {
            return;
        }
        let order = self.thread_order();
        if order.is_empty() {
            return;
        }
        let at = self
            .thread_at
            .and_then(|t| order.iter().position(|&ix| ix == t));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => order.len() - 1,
            (Some(i), true) => (i + 1) % order.len(),
            (Some(i), false) => (i + order.len() - 1) % order.len(),
        };
        let ix = order[next];
        self.thread_at = Some(ix);
        if !self.mr_open(ix) {
            self.set_thread_open(ix, true, cx);
        }
        let t = &self.requests.threads[ix];
        if let Some(path) = t.path.clone() {
            let (line, old) = (t.new_line.or(t.old_line).unwrap_or(1), t.new_line.is_none());
            self.jump_to_line(&path, line, old, cx);
        }
        cx.notify();
    }

    /// The discussion `r` `x` `o` act on: the one with the keyboard (a click or ⇧N gave it), else
    /// the first open one from the top of the view.
    pub(super) fn acting_thread(&self) -> Option<usize> {
        self.thread_at.or_else(|| {
            let top = self.local_top();
            self.rows.iter().skip(top).find_map(|row| match row {
                super::rows::Row::Request(ix) if self.mr_open(*ix) => Some(*ix),
                _ => None,
            })
        })
    }

    pub(super) fn reply_focused(&mut self, cx: &mut Context<Self>) {
        // In the Inbox, r answers the selected notice.
        if self.inbox && self.zone != super::zones::Zone::Tool {
            return self.reply_in_inbox(cx);
        }
        if let Some(ix) = self
            .acting_thread()
            .filter(|_| self.request_target().is_some())
        {
            self.thread_at = Some(ix);
            self.start_reply(ix, cx);
        }
    }

    pub(super) fn resolve_focused(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .acting_thread()
            .filter(|_| self.request_target().is_some())
        {
            self.thread_at = Some(ix);
            self.toggle_resolved(ix, cx);
        }
    }

    /// Is discussion `ix` shown open? A resolved one starts folded, as in GitLab.
    pub(super) fn mr_open(&self, ix: usize) -> bool {
        let default = self.requests.threads.get(ix).is_some_and(|t| !t.resolved);
        default != self.toggled_mr.contains(&ix)
    }

    /// Opens or folds discussion `ix`.
    pub(super) fn set_thread_open(&mut self, ix: usize, open: bool, cx: &mut Context<Self>) {
        // Its notes fade in as it opens (#56, 10).
        self.mark_motion(&format!("thread-{ix}"), cx);
        if self.mr_open(ix) != open && !self.toggled_mr.remove(&ix) {
            self.toggled_mr.insert(ix);
        }
        self.refresh_rows();
        cx.notify();
    }

    /// `o`: folds or opens the discussion the keyboard is on.
    pub(super) fn toggle_focused(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self.acting_thread() {
            self.thread_at = Some(ix);
            let open = self.mr_open(ix);
            self.set_thread_open(ix, !open, cx);
        }
    }

    /// The pending comments of file `path`: (index, old side, line).
    pub(super) fn drafts_in(&self, path: &str) -> Vec<(usize, bool, u32)> {
        self.requests
            .drafts
            .iter()
            .enumerate()
            .filter(|(_, d)| d.path.as_deref() == Some(path))
            .filter_map(|(ix, d)| match (d.new_line, d.old_line) {
                (Some(n), _) => Some((ix, false, n)),
                (None, Some(o)) => Some((ix, true, o)),
                _ => None,
            })
            .collect()
    }

    /// A pending comment: a dashed frame, only its author sees it until the review is submitted.
    pub(super) fn render_draft(
        &self,
        ix: usize,
        indent: f32,
        this: WeakEntity<Self>,
    ) -> gpui::AnyElement {
        use super::discussion::{self, Kind, Source};
        let Some(d) = self.requests.drafts.get(ix) else {
            return div().into_any_element();
        };
        let id = d.id;
        let selecting = self.selecting(this.clone());
        let (delete, raw, send) = (this.clone(), this.clone(), this);
        // Waiting on the host: faded, saying what for, nothing to act on (#139).
        let waits = self.draft_waits(id);
        let acts = waits.is_none() && !super::waiting::is_temp(id);
        let one = self.sends_one_draft();
        // GitHub publishes a pending comment only with its review: there it says so (#138).
        let send = discussion::icon_button(
            ("draft-send", ix),
            "send",
            "Send this now",
            None,
            Some(if one {
                "Publishes this comment alone; the others stay pending"
            } else {
                "GitHub sends a pending comment only with its review: Submit review sends it"
            }),
            Kind::Plain,
            self.tight,
        )
        .h(px(24.))
        .px(px(8.))
        .map(|b| {
            if one {
                b.on_click(move |_, _, cx| {
                    send.update(cx, |this, cx| this.send_draft_now(id, cx)).ok();
                })
            } else {
                b.opacity(0.45).cursor_default()
            }
        });
        let bar = discussion::bar(("draft-bar", ix), true)
            .cursor_default()
            .child(discussion::source_pill(Source::Pending))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    // An agent's says who wrote it (#150).
                    .child(match self.draft_agent(id) {
                        Some(agent) => format!(
                            "· {agent} wrote it · only you see it until you submit the review"
                        ),
                        None => "· only you see it until you submit the review".to_owned(),
                    }),
            )
            .children(super::markdown::has_markdown(&d.body).then(|| {
                self.rich_switch(("draft-rich", ix))
                    .on_click(move |_, _, cx| {
                        raw.update(cx, |this, cx| this.toggle_rich_comments(cx))
                            .ok();
                    })
            }))
            .children(acts.then_some(send))
            .children(acts.then(|| {
                discussion::icon_button(
                    ("draft-delete", ix),
                    "trash",
                    "Delete",
                    None,
                    None,
                    Kind::Danger,
                    self.tight,
                )
                .h(px(24.))
                .px(px(8.))
                .on_click(move |_, _, cx| {
                    delete
                        .update(cx, |this, cx| this.discard_draft(id, cx))
                        .ok();
                })
            }));
        div()
            .w_full()
            .pl(px(indent))
            .pr(px(12.))
            .py(px(6.))
            .child(
                discussion::frame(Source::Pending, self.draft_at == Some(ix))
                    .when(waits.is_some(), |s| s.opacity(super::waiting::FADED))
                    .child(bar)
                    .child(div().child(discussion::note(
                        &self.me,
                        true,
                        None,
                        waits.unwrap_or("pending").to_owned(),
                        super::markdown::body(
                            &d.body,
                            self.settings.rich_comments,
                            format!("md-draft-{ix}"),
                            self.link_base(),
                            Some(selecting),
                        ),
                        true,
                        None,
                        None,
                    ))),
            )
            .into_any_element()
    }

    /// A change to the open request, `ops` sent with `work`; a refusal says why in the error bar
    /// (#139).
    fn request_action(
        &mut self,
        ops: Vec<Op>,
        work: impl FnOnce(crate::git::WebRemote) -> anyhow::Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if let Some(iid) = self.requests.current {
            self.send_change(iid, ops, work, Self::say_refused, cx);
        }
    }

    /// Resolves discussion `ix`, or opens it again: it folds, or opens, at once (#139).
    pub(super) fn toggle_resolved(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(t) = self.requests.threads.get(ix) else {
            return;
        };
        if super::waiting::is_temp_thread(&t.id) {
            return;
        }
        let (id, resolved) = (t.id.clone(), !t.resolved);
        let Some(iid) = self.requests.current else {
            return;
        };
        // Shown as its new state shows it.
        self.mark_motion(&format!("thread-{ix}"), cx);
        self.toggled_mr.remove(&ix);
        let op = Op::Resolve {
            thread: id.clone(),
            resolved,
        };
        self.request_action(
            vec![op],
            move |remote| mr::resolve(&remote, iid, &id, resolved),
            cx,
        );
    }

    /// `a`: approves the open request, or takes one's approval back.
    pub(super) fn toggle_approval(&mut self, cx: &mut Context<Self>) {
        let (Some(iid), Some(a)) = (self.requests.current, self.requests.approvals.clone()) else {
            return;
        };
        if !self.showing_request() {
            return;
        }
        let op = vec![Op::Approve { mine: !a.mine }];
        if a.mine {
            self.request_action(op, move |remote| mr::unapprove(&remote, iid), cx);
        } else if a.can {
            let head = self
                .requests
                .find(iid)
                .map(|m| m.head.clone())
                .unwrap_or_default();
            self.request_action(op, move |remote| mr::approve(&remote, iid, &head), cx);
        } else {
            self.error = Some("GitLab does not let you approve this merge request.".into());
            cx.notify();
        }
    }

    /// ⌥N / ⌥P: the next or previous of one's pending comments, across the files of the request.
    pub(super) fn step_draft(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.showing_request() {
            return;
        }
        let order = self.draft_order();
        if order.is_empty() {
            return;
        }
        let at = self
            .draft_at
            .and_then(|d| order.iter().position(|&(_, _, ix)| ix == d));
        let n = order.len();
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
        };
        let ix = order[next].2;
        self.draft_at = Some(ix);
        let d = &self.requests.drafts[ix];
        if let Some(path) = d.path.clone() {
            let (line, old) = (d.new_line.or(d.old_line).unwrap_or(1), d.new_line.is_none());
            self.jump_to_line(&path, line, old, cx);
        }
        cx.notify();
    }

    /// A pending comment in one line, an agent's after its name (#150).
    pub(super) fn draft_line(&self, d: &mr::Draft) -> String {
        match self.draft_agent(d.id) {
            Some(agent) => format!("{agent}: {}", self.one_line(&d.body)),
            None => self.one_line(&d.body),
        }
    }

    /// The pending comments on lines of the diff loaded, in its order: (file, line, index).
    pub(super) fn draft_order(&self) -> Vec<(usize, u32, usize)> {
        let Some(diff) = self.diff.as_ref() else {
            return Vec::new();
        };
        let mut order: Vec<(usize, u32, usize)> = self
            .requests
            .drafts
            .iter()
            .enumerate()
            .filter_map(|(ix, d)| {
                let file = diff
                    .files
                    .iter()
                    .position(|f| Some(f.path()) == d.path.as_deref())?;
                Some((file, d.new_line.or(d.old_line)?, ix))
            })
            .collect();
        order.sort_unstable();
        order
    }

    pub(super) fn discard_draft(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(iid) = self
            .requests
            .current
            .filter(|_| !super::waiting::is_temp(id))
        else {
            return;
        };
        self.request_action(
            vec![Op::DropDraft(id)],
            move |remote| mr::delete_draft(&remote, iid, id),
            cx,
        );
    }

    /// Send this now: pending comment `id` is published alone and becomes a discussion on its
    /// line; the others stay pending. A failure leaves it pending and says why (#138).
    pub(super) fn send_draft_now(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(iid) = self
            .requests
            .current
            .filter(|_| !super::waiting::is_temp(id))
        else {
            return;
        };
        let w = &mut self.requests.waiting;
        let op = Op::Publish(vec![(id, w.temp_thread(), w.temp())]);
        self.request_action(
            vec![op],
            move |remote| mr::publish_draft(&remote, iid, id),
            cx,
        );
    }

    /// Whether a pending comment can be sent alone: GitLab publishes one draft, GitHub only the
    /// whole review (#138).
    pub(super) fn sends_one_draft(&self) -> bool {
        self.web.as_ref().is_some_and(|w| !w.github)
    }

    /// Send now on a pending comment's row in a list (the Discussions window, Submit review),
    /// shown while the row is under the pointer; none on GitHub (#138).
    pub(super) fn send_now_chip(
        &self,
        id: u64,
        group: &'static str,
        cx: &Context<Self>,
    ) -> Option<gpui::Stateful<gpui::Div>> {
        self.sends_one_draft().then(|| {
            div()
                .id(("send-now", id as usize))
                .flex_none()
                .px(px(6.))
                .h(px(20.))
                .flex()
                .items_center()
                .rounded(px(super::ROW_RADIUS))
                .border_1()
                .border_color(theme::island_border())
                .bg(theme::panel())
                .text_size(px(11.))
                .text_color(theme::text())
                .cursor_pointer()
                .hover(|s| s.bg(theme::selected()))
                .invisible()
                .group_hover(group, |s| s.visible())
                .child("Send now")
                .tooltip_show_delay(super::hover::LONG)
                .tooltip(|_, cx| {
                    cx.new(|_| super::Tip("Publishes this comment alone; the others stay pending"))
                        .into()
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    this.send_draft_now(id, cx);
                }))
        })
    }

    /// Delete on one's own note, then "Delete on GitLab?" with Delete and Cancel (#76).
    fn delete_note_button(
        &self,
        thread: usize,
        note: u64,
        this: gpui::WeakEntity<Self>,
    ) -> gpui::AnyElement {
        use super::discussion::{Kind, icon_button};
        if self.confirm_note != Some(note) {
            let ask = this.clone();
            return icon_button(
                ("delete-note", note as usize),
                "trash",
                "Delete",
                None,
                None,
                Kind::Quiet,
                self.tight,
            )
            .h(px(22.))
            .px(px(6.))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                ask.update(cx, |this, cx| {
                    this.confirm_note = Some(note);
                    cx.notify()
                })
                .ok();
            })
            .into_any_element();
        }
        let (yes, no) = (this.clone(), this);
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .child(
                div()
                    .text_color(theme::removed())
                    .child(super::words::say("Delete on GitLab?").into_owned()),
            )
            .child(
                icon_button(
                    ("delete-note-yes", note as usize),
                    "trash",
                    "Delete",
                    None,
                    None,
                    Kind::Danger,
                    false,
                )
                .h(px(22.))
                .px(px(6.))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    yes.update(cx, |this, cx| this.delete_own_note(thread, note, cx))
                        .ok();
                }),
            )
            .child(
                div()
                    .id(("delete-note-no", note as usize))
                    .px(px(6.))
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .rounded(px(super::ROW_RADIUS))
                    .text_color(theme::muted())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child("Cancel")
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        no.update(cx, |this, cx| {
                            this.confirm_note = None;
                            cx.notify()
                        })
                        .ok();
                    }),
            )
            .into_any_element()
    }

    /// Deletes one's own note `note` of discussion number `thread` on GitLab or GitHub.
    fn delete_own_note(&mut self, thread: usize, note: u64, cx: &mut Context<Self>) {
        self.confirm_note = None;
        let (Some(iid), Some(discussion)) = (
            self.requests.current,
            self.requests.threads.get(thread).map(|t| t.id.clone()),
        ) else {
            return;
        };
        self.request_action(
            vec![Op::Delete { note }],
            move |remote| mr::delete_note(&remote, iid, &discussion, note),
            cx,
        );
    }

    /// The Discussions tool window: the request's discussions and pending notes — public ones only,
    /// the agent's are the Agents window's (#76); a click shows one in the diff.
    pub(super) fn discussions_tool(&self, cx: &mut Context<Self>) -> ToolView {
        let theirs: Vec<(usize, &Thread)> = if self.showing_request() {
            self.requests.threads.iter().enumerate().collect()
        } else {
            Vec::new()
        };
        let drafts: Vec<(usize, &Draft)> = if self.showing_request() {
            self.requests.drafts.iter().enumerate().collect()
        } else {
            Vec::new()
        };
        let pending = drafts.len();
        // Public only: the agent's comments are the Agents window's (#76).
        let discussions = theirs.len();
        let total = discussions + pending;
        // Group headings once there is more than one kind; the pending ones say how to walk them.
        let grouped = [discussions, pending].iter().filter(|&&n| n > 0).count() > 1;
        // Written on an earlier version: listed apart, grouped by that version (#46).
        let outdated = if self.showing_request() {
            self.outdated_by_version()
        } else {
            Vec::new()
        };
        let old: Vec<usize> = outdated.iter().flat_map(|(_, l)| l.clone()).collect();
        let theirs: Vec<(usize, &Thread)> = theirs
            .into_iter()
            .filter(|(ix, _)| !old.contains(ix))
            .collect();
        let discussions = theirs.len();
        let mut outdated_items = Vec::new();
        for (version, list) in outdated {
            let items: Vec<gpui::AnyElement> =
                list.iter().map(|&ix| self.outdated_item(ix, cx)).collect();
            outdated_items.push(
                div()
                    .flex()
                    .flex_col()
                    .child(group_heading(
                        match version {
                            Some(v) => format!("Written on v{v} · {}", list.len()),
                            None => format!("An earlier version · {}", list.len()),
                        },
                        None,
                    ))
                    .children(items),
            );
        }
        let request_items = theirs.into_iter().map(|(ix, t)| {
            let target = t.path.clone().zip(t.new_line.or(t.old_line));
            let old = t.new_line.is_none();
            let where_ = match &target {
                Some((p, n)) => format!("{}:{n}", p.rsplit('/').next().unwrap_or(p)),
                None => "General".to_owned(),
            };
            let first = &t.notes[0];
            // Faded while a change to it waits on the host (#139).
            let waits = self.thread_waits(&t.id).is_some()
                || t.notes.iter().any(|n| self.note_time(n.id, n.created).1);
            comment_item(
                ("comment-mr", ix),
                theme::warning(),
                where_,
                format!("{}: {}", first.author, self.one_line(&first.body)),
            )
            .when(waits, |s| s.opacity(super::waiting::FADED))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if let Some((path, line)) = target.clone() {
                    this.jump_to_line(&path, line, old, cx)
                }
            }))
        });
        let draft_items = drafts.into_iter().map(|(ix, d)| {
            let target = d.path.clone().zip(d.new_line.or(d.old_line));
            let old = d.new_line.is_none();
            let where_ = match &target {
                Some((p, n)) => format!("{}:{n} · pending", p.rsplit('/').next().unwrap_or(p)),
                None => "pending".to_owned(),
            };
            comment_item(
                ("comment-draft", ix),
                theme::warning(),
                where_,
                self.draft_line(d),
            )
            .relative()
            .group("pending-row")
            .when(self.draft_waits(d.id).is_some(), |s| {
                s.opacity(super::waiting::FADED)
            })
            .children(
                self.send_now_chip(d.id, "pending-row", cx)
                    .filter(|_| self.draft_waits(d.id).is_none() && !super::waiting::is_temp(d.id))
                    .map(|chip| chip.absolute().top(px(3.)).right(px(6.))),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if let Some((path, line)) = target.clone() {
                    this.jump_to_line(&path, line, old, cx)
                }
            }))
        });
        let submit = (pending > 0).then(|| {
            div()
                .id("submit-review")
                .px_2()
                .h(px(24.))
                .flex()
                .items_center()
                .rounded(px(super::ROW_RADIUS))
                .bg(theme::warning())
                .text_color(theme::base())
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .child(format!("Submit review · {pending}"))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_submit(cx)))
        });
        ToolView {
            title: "Discussions".into(),
            count: (total > 0).then(|| total.to_string()),
            actions: Vec::new(),
            body: div()
                .id("comments-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .py(px(6.))
                .children((pending > 0 && (grouped || pending > 1)).then(|| {
                    group_heading(format!("Pending · {pending}"), Some("⌥N"))
                }))
                .children(draft_items)
                .children(
                    (grouped && discussions > 0)
                        .then(|| group_heading(format!("Discussions · {discussions}"), None)),
                )
                .children(request_items)
                .children((!old.is_empty()).then(|| {
                    div()
                        .px(px(14.))
                        .pt(px(14.))
                        .pb(px(2.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child("Outdated")
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_weight(FontWeight::NORMAL)
                                .text_color(theme::faint())
                                .child("by the version they were written on"),
                        )
                }))
                .children(outdated_items)
                .children((total == 0 && self.requests.reading).then(|| {
                    div()
                        .px(px(14.))
                        .py(px(12.))
                        .child(super::roots::skeleton("discussions-loading", 3))
                }))
                .children((total == 0 && !self.requests.reading).then(|| {
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child("No comments yet. Press c to comment on a line, or hover a line and press +.")
                }))
                .into_any_element(),
            foot: submit.map(IntoElement::into_any_element),
        }
    }

    /// An outdated discussion in the Discussions window: where, its first words, whether its line
    /// changed. A click shows it — its line where it is now, else the versions compared since —,
    /// and it is resolved or reopened there, where it is read (the developer, 2026-10-23: no Show
    /// or Reopen on the row).
    fn outdated_item(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        use super::hover::Hover;
        let t = &self.requests.threads[ix];
        let line_now = self.requests.histories.get(&t.id).and_then(|h| h.line_now);
        let path = t.path.clone().unwrap_or_default();
        let place = format!(
            "{}:{}",
            path.rsplit('/').next().unwrap_or(&path),
            t.new_line.or(t.old_line).unwrap_or(0)
        );
        let first = &t.notes[0];
        let written = self.requests.histories.get(&t.id).map(|h| h.written);
        // What a click does: the line where it is now, or (gone) the versions compared.
        let shows = match (line_now, self.compare_label(ix)) {
            (Some(_), _) => "Show its line now".to_owned(),
            (None, Some(compare)) => compare,
            (None, None) => String::new(),
        };
        div()
            .id(("comment-old-item", ix))
            .flex()
            .flex_col()
            .cursor_pointer()
            .child(comment_item(
                ("comment-old", ix),
                theme::faint(),
                place,
                format!("{}: {}", first.author, self.one_line(&first.body)),
            ))
            .child(
                div()
                    .pl(px(28.))
                    .pr(px(14.))
                    .pb(px(6.))
                    .text_size(px(11.))
                    .text_color(theme::warning())
                    .child(
                        self.since_comment(ix)
                            .unwrap_or_else(|| "Written on an earlier version".to_owned()),
                    ),
            )
            .when(!shows.is_empty(), |s| s.tip(shows))
            .on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| match line_now {
                    Some(line) => this.jump_to_line(&path, line, false, cx),
                    None if written.is_some() => this.compare_since(ix, cx),
                    None => {}
                }),
            )
            .into_any_element()
    }

    /// Every comment of the open diff: the request's discussions and pending ones, and the agent's.
    /// The Discussions window's count: the request's own discussions and pending notes (#76).
    pub(super) fn comments_total(&self) -> usize {
        if self.showing_request() {
            self.requests.threads.len() + self.requests.drafts.len()
        } else {
            0
        }
    }

    /// The requests the scope and filter let through, in the order the list shows them.
    pub(super) fn shown_requests(&self) -> Vec<&Mr> {
        let filter = self.bfilter.to_lowercase();
        let words: Vec<&str> = filter.split_whitespace().collect();
        // The merged or the closed as read, newest first; the open by what they need (#114).
        let past = self.requests.view != mr::MrState::Opened;
        let list = if past {
            &self.requests.past.list
        } else {
            &self.requests.list
        };
        let mut shown: Vec<&Mr> = list
            .iter()
            .filter(|m| self.in_scope(m, self.requests.scope))
            .filter(|m| {
                let hay = format!(
                    "{} {} {} {}",
                    super::words::number(m.iid),
                    m.title,
                    m.author,
                    m.source
                )
                .to_lowercase();
                words.iter().all(|w| crate::rank::word_matches(w, &hay))
            })
            .collect();
        // By what each needs from you, as the list groups them; the latest activity first.
        if !past {
            shown.sort_by_key(|m| (self.group_of(m), std::cmp::Reverse(m.updated)));
        }
        shown
    }

    /// Whether `m` is one the scope lists, by the token's user.
    pub(super) fn in_scope(&self, m: &Mr, scope: Scope) -> bool {
        let me = self.requests.login.as_deref().unwrap_or_default();
        match scope {
            Scope::All => true,
            Scope::ReviewMe => !me.is_empty() && m.reviewers.iter().any(|r| r == me),
            Scope::Mine => !me.is_empty() && m.author_login == me,
        }
    }

    /// All · To review · Mine with their counts, in Merge requests' bar, once the token's user is
    /// known (#70, #71).
    pub(super) fn request_filters(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let loading = self.requests.loading && self.requests.list.is_empty();
        if self.requests.login.is_none() || loading {
            return None;
        }
        // The merged or the closed: counted as far as they are read (#114).
        let past = self.requests.view != mr::MrState::Opened;
        let (list, more) = if past {
            (&self.requests.past.list, self.requests.past.more)
        } else {
            (&self.requests.list, false)
        };
        let count = |scope: Scope| list.iter().filter(|m| self.in_scope(m, scope)).count();
        let filter = |scope: Scope, id: &'static str, name: &'static str| {
            super::diff_view::chip(
                id,
                format!("{name} {}{}", count(scope), if more { "+" } else { "" }),
                self.requests.scope == scope,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.requests.scope = scope;
                cx.notify();
            }))
        };
        let view = self.requests.view;
        let state = super::diff_view::chip("request-state", format!("{} ▾", view.word()), past)
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                use super::menu::{Act, Entry};
                let entries = [
                    mr::MrState::Opened,
                    mr::MrState::Merged,
                    mr::MrState::Closed,
                ]
                .into_iter()
                .map(|s| Entry::new(s.word(), Act::RequestView(s)).checked(s == view))
                .collect();
                this.open_menu(e.position(), vec![entries], cx);
            }));
        Some(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    super::diff_view::group()
                        .child(filter(Scope::All, "scope-all", "All"))
                        .child(filter(Scope::ReviewMe, "scope-review", "To review"))
                        .child(filter(Scope::Mine, "scope-mine", "Mine")),
                )
                .child(state),
        )
    }

    /// Keeps the typed token, asks GitLab who it is, and starts listing on success.
    pub(super) fn submit_token(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        let token = self.token_input.trim().to_owned();
        if token.len() < 8 || token.contains(char::is_whitespace) {
            self.gitlab_check = Some(Err("That does not look like a token.".to_owned()));
            return cx.notify();
        }
        if let Err(e) = mr::save_token(&remote, &token) {
            self.gitlab_check = Some(Err(format!("{e:#}")));
            return cx.notify();
        }
        self.gitlab_check = None;
        self.testing = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let check = remote.clone();
            let result = cx
                .background_executor()
                .spawn(async move { mr::me(&check) })
                .await;
            this.update(cx, |this, cx| {
                this.testing = false;
                match result {
                    Ok(who) => {
                        this.gitlab_check = Some(Ok(who));
                        this.token_input.clear();
                        this.field = None;
                        this.load_requests(cx);
                    }
                    Err(e) => {
                        mr::forget_token(&remote);
                        this.gitlab_check = Some(Err(format!("{e:#}")));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Merge requests' first run, a project without a token (#72): what a token is for, where to
    /// make one, and the field it is pasted into, centred in the content.
    pub(super) fn render_connect(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let Some(remote) = self.web.clone() else {
            return div().into_any_element();
        };
        let host = mr::host(&remote);
        let page = mr::token_page(&remote);
        let field = self.field_box(
            "token-field",
            super::lists::Field::Token,
            "Paste your token",
            cx,
        );
        let note = match (&self.gitlab_check, self.testing) {
            (_, true) => Some(("Checking…".to_owned(), theme::muted())),
            (Some(Err(e)), _) => Some((e.clone(), theme::removed())),
            (Some(Ok(who)), _) => Some((format!("Connected as {who}"), theme::added())),
            _ => None,
        };
        let step = |text: String| {
            div()
                .text_size(px(12.))
                .text_color(theme::muted())
                .child(text)
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .px(px(24.))
            .child(
                super::icons::icon("pull-request")
                    .size(px(24.))
                    .text_color(theme::muted()),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(super::words::say("Connect GitLab to see merge requests").into_owned()),
            )
            .child(
                div()
                    .max_w(px(420.))
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .text_center()
                    .text_color(theme::muted())
                    .child(if remote.github {
                        format!(
                            "GitLance reads the pull requests of {host} with a token of yours (the repo scope), or the GitHub CLI's once you sign in with gh auth login. It writes nothing unless you post a comment or a review."
                        )
                    } else {
                        format!(
                            "GitLance reads the merge requests of {host} with a personal access token of yours (the api scope). It stays on this Mac and writes nothing unless you post a comment or a review."
                        )
                    }),
            )
            .child(
                div()
                    .w(px(360.))
                    .pt(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("token-create")
                            .h(px(28.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(super::ROW_RADIUS))
                            .border_1()
                            .border_color(theme::island_border())
                            .text_size(px(13.))
                            .text_color(theme::accent())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .child(format!("1 · Create a token on {host} ↗"))
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&page))),
                    )
                    .child(step("2 · Paste it here and press ↵".to_owned()))
                    .child(field)
                    .children(note.map(|(text, color)| {
                        div().text_size(px(12.)).text_color(color).child(text)
                    })),
            )
            .child(
                div()
                    .pt(px(4.))
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(super::words::say("or Settings → Merge requests").into_owned()),
            )
            .into_any_element()
    }

    /// The pipeline's pill: its state, and the job that failed; a click opens it.
    pub(super) fn render_pipeline_pill(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        self.requests.pipeline.clone().map(|p| {
            let (glyph, word, color) = pipeline_word(&p);
            // A failure opens the job that failed; the rest open the pipeline.
            let url = p
                .failed
                .first()
                .map(|(_, u)| u.clone())
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| p.url.clone());
            let ago = (p.updated > 0).then(|| format::ago(p.updated));
            // A long hover: every job that failed, and when (X59).
            let host = super::words::say("GitLab").into_owned();
            let tip: SharedString = {
                let when = if p.updated > 0 {
                    format!(" · {}", super::hover::full_date(p.updated))
                } else {
                    String::new()
                };
                let jobs: Vec<&str> = p.failed.iter().map(|(name, _)| name.as_str()).collect();
                match (p.status.as_str(), jobs.is_empty()) {
                    ("failed", false) => format!(
                        "Pipeline failed{when}\nFailed: {}\nClick opens the first failed job on {host}",
                        jobs.join(", ")
                    ),
                    _ => format!("Pipeline {word}{when}\nClick opens it on {host}"),
                }
            }
            .into();
            div()
                .id("pipeline-pill")
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .h(px(22.))
                .rounded_full()
                .border_1()
                .border_color(color)
                .text_size(px(12.))
                .text_color(color)
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                .child(format!("{glyph} {word}"))
                .children(ago.map(|a| div().text_color(theme::faint()).child(a)))
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&url)))
        })
    }

    /// Approve, or who approved: `a` toggles one's approval.
    pub(super) fn render_approval_chip(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let waits = self.approval_waits();
        self.requests.approvals.clone().map(|a| {
            let count = if a.required > 0 {
                format!(" {}/{}", a.required - a.left.min(a.required), a.required)
            } else if !a.by.is_empty() && !a.mine {
                format!(" {}", a.by.len())
            } else {
                String::new()
            };
            let action = match (a.mine, a.can) {
                (true, _) => "a takes yours back",
                (false, true) => "a approves",
                (false, false) => "",
            };
            // A long hover: who approved, who asked for changes, who is still asked (X59).
            let d = self.requests.details.clone().unwrap_or_default();
            let waiting: Vec<&str> = d
                .reviewers
                .iter()
                .filter(|r| !a.by.contains(r) && !d.changes_requested.contains(r))
                .map(String::as_str)
                .collect();
            let mut lines = vec![if a.by.is_empty() {
                "No approvals yet".to_owned()
            } else {
                format!("Approved by {}", a.by.join(", "))
            }];
            if !d.changes_requested.is_empty() {
                lines.push(format!(
                    "Changes requested by {}",
                    d.changes_requested.join(", ")
                ));
            }
            if !waiting.is_empty() {
                lines.push(format!("Still asked: {}", waiting.join(", ")));
            }
            if a.left > 0 {
                lines.push(format!(
                    "{} more needed",
                    plural(a.left as usize, "approval")
                ));
            }
            if !action.is_empty() {
                lines.push(action.to_owned());
            }
            let tip: SharedString = super::words::say(&lines.join("\n")).into_owned().into();
            let (label, color) = if a.mine {
                ("✓ Approved", theme::added())
            } else if a.can {
                ("Approve", theme::text())
            } else {
                ("✓", theme::muted())
            };
            div()
                .id("approval")
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .h(px(22.))
                .rounded_full()
                .border_1()
                .border_color(if a.mine {
                    theme::added()
                } else {
                    theme::island_border()
                })
                .text_size(px(12.))
                .text_color(color)
                .when(a.mine || a.can, |s| {
                    s.cursor_pointer().hover(|s| s.bg(theme::hover()))
                })
                // Counted at once, faded until the host answers (#139).
                .when(waits, |s| s.opacity(super::waiting::FADED))
                .child(format!("{label}{count}"))
                .when(a.can && !a.mine, |s| {
                    s.child(super::keys::caps("a", super::keys::On::Plain))
                })
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_approval(cx)))
        })
    }

    /// Shows `line` of `path`: that file first when it is another one.
    pub(super) fn jump_to_line(
        &mut self,
        path: &str,
        line: u32,
        old: bool,
        cx: &mut Context<Self>,
    ) {
        self.jump_line = Some((line, old));
        if self.current_path().as_deref() == Some(path) {
            self.apply_jump(cx);
            return;
        }
        let ix = self
            .diff
            .as_ref()
            .and_then(|d| d.files.iter().position(|f| f.path() == path));
        match ix {
            Some(ix) => self.select_file(ix, cx),
            None => self.jump_line = None,
        }
    }

    /// The list row that shows `line` of the old or the new side.
    pub(super) fn row_of_line(&self, line: u32, old: bool) -> Option<usize> {
        self.rows.iter().position(|r| match r {
            Row::Split { left, right } => {
                let cell = if old { left } else { right };
                cell.is_some_and(|c| c.line == line)
            }
            Row::Unified {
                old_line, new_line, ..
            } => {
                if old {
                    *old_line == Some(line)
                } else {
                    *new_line == Some(line)
                }
            }
            _ => false,
        })
    }
}

/// After position `at` in a list (none: before its start), the next item not reviewed, going round
/// to the first item when the end is reached; never `at` itself.
/// A pipeline as a glyph, a word and its colour: "✗ lint failed" in red.
pub(super) fn pipeline_word(p: &Pipeline) -> (&'static str, String, gpui::Rgba) {
    match p.status.as_str() {
        "success" => ("✓", "passed".to_owned(), theme::added()),
        "failed" => (
            "✗",
            match p.failed.as_slice() {
                [] => "failed".to_owned(),
                [(job, _)] => format!("{job} failed"),
                [(job, _), rest @ ..] => format!("{job} +{} failed", rest.len()),
            },
            theme::removed(),
        ),
        "running" => ("●", "running".to_owned(), theme::warning()),
        "pending" | "created" | "waiting_for_resource" | "preparing" => {
            ("○", "pending".to_owned(), theme::muted())
        }
        "canceled" => ("⊘", "canceled".to_owned(), theme::muted()),
        other => ("·", other.replace('_', " "), theme::muted()),
    }
}

/// The row to review after row `at`, going round: the nearest of the lowest tier; a row of no
/// tier is never one.
fn next_to_review(open: &[bool], at: Option<usize>) -> Option<usize> {
    let n = open.len();
    let start = at.map_or(0, |a| a + 1);
    (0..n)
        .map(|step| (start + step) % n)
        .find(|&ix| Some(ix) != at && open[ix])
}

/// A group's heading in a tool window's list; `keys` says how to walk it.
pub(super) fn group_heading(text: String, keys: Option<&'static str>) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px(px(14.))
        .pt(px(10.))
        .pb(px(4.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::muted())
        .child(div().flex_1().child(text.to_uppercase()))
        .children(keys.map(|k| super::keys::caps(k, super::keys::On::Plain)))
}

/// A comment as one entry of a tool window's list: a dot in its colour, where it is, two lines of it.
pub(super) fn comment_item(
    id: (&'static str, usize),
    tone: gpui::Rgba,
    where_: String,
    body: String,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .mx(px(6.))
        .px(px(8.))
        .py(px(4.))
        .overflow_hidden()
        .rounded(px(super::ROW_RADIUS))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_none().size(px(6.)).rounded_full().bg(tone))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(theme::CODE_FONT)
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(where_),
                ),
        )
        .child(
            div()
                .line_clamp(2)
                .pl(px(14.))
                .text_size(px(12.))
                .text_color(theme::muted())
                .child(body),
        )
}

#[cfg(test)]
mod tests {
    use super::next_to_review;

    #[test]
    fn the_next_request_to_review_goes_round_to_the_top() {
        // 0 to review, 1 reviewed, 2 current, 3 reviewed: past the end, back to 0.
        assert_eq!(
            next_to_review(&[true, false, true, false], Some(2)),
            Some(0)
        );
        assert_eq!(next_to_review(&[false, true, false], Some(0)), Some(1));
        assert_eq!(next_to_review(&[false, true], None), Some(1));
        // Only the current one is left: nowhere to go.
        assert_eq!(next_to_review(&[false, true, false], Some(1)), None);
        assert_eq!(next_to_review(&[false, false, false], Some(1)), None);
    }

    #[test]
    fn the_one_below_comes_before_the_top() {
        // 0 needs review, 1 current, 2 reviewed, 3 new to you: on down to 3, not back up to 0
        // (#136).
        assert_eq!(next_to_review(&[true, true, false, true], Some(1)), Some(3));
    }

    /// `n` commits in a line from `parent`, each a file of its own; `tag` keeps two lines from
    /// the same parent apart.
    fn line(
        repo: &git2::Repository,
        parent: Option<git2::Oid>,
        n: usize,
        tag: &str,
    ) -> Vec<git2::Oid> {
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let mut out = Vec::new();
        let mut parent = parent;
        for i in 0..n {
            let mut tree = repo.treebuilder(None).unwrap();
            let blob = repo.blob(format!("{tag} {i}").as_bytes()).unwrap();
            tree.insert(format!("f{i}"), blob, 0o100644).unwrap();
            let tree = repo.find_tree(tree.write().unwrap()).unwrap();
            let parents: Vec<_> = parent
                .iter()
                .map(|p| repo.find_commit(*p).unwrap())
                .collect();
            let parents: Vec<_> = parents.iter().collect();
            let id = repo.commit(None, &sig, &sig, "c", &tree, &parents).unwrap();
            out.push(id);
            parent = Some(id);
        }
        out
    }

    #[test]
    fn a_request_is_compared_from_the_hosts_sides_when_this_clone_would_differ() {
        use super::{Sides, sides};
        let dir = tempfile::TempDir::new().unwrap();
        let git = git2::Repository::init(dir.path()).unwrap();
        // The target here is at `a`; on the host it went on to `c`, the request was rebased on
        // it (`d`), and later the target went on again (`e`).
        let main = line(&git, None, 3, "main");
        let (a, c) = (main[0], main[2]);
        let d = line(&git, Some(c), 1, "request")[0];
        let e = line(&git, Some(c), 1, "target")[0];
        let repo = crate::git::Repo::open(dir.path()).unwrap();

        // A target not fetched since: every target commit after `a` would be the request's.
        assert_eq!(sides(&repo, (Some(c), Some(d)), (a, d)), Sides::Host(c, d));
        // Up to date, or ahead: the same merge base, nothing to load again.
        assert_eq!(sides(&repo, (Some(c), Some(d)), (c, d)), Sides::Same);
        assert_eq!(sides(&repo, (Some(c), Some(d)), (e, d)), Sides::Same);
        // Merged by a fast-forward: the head is in the target, which shows nothing.
        let merged = line(&git, Some(d), 1, "merged")[0];
        assert_eq!(
            sides(&repo, (Some(c), Some(d)), (merged, d)),
            Sides::Host(c, d)
        );
        // The source branch here is behind the host's head.
        assert_eq!(sides(&repo, (Some(c), Some(e)), (c, d)), Sides::Host(c, e));
        // What the host names is not here: fetched first.
        let gone = git2::Oid::from_str("1234567890123456789012345678901234567890").unwrap();
        assert_eq!(
            sides(&repo, (Some(gone), Some(d)), (a, d)),
            Sides::Missing {
                base: true,
                head: false
            }
        );
        assert_eq!(
            sides(&repo, (Some(c), Some(gone)), (a, d)),
            Sides::Missing {
                base: false,
                head: true
            }
        );
        // The host gave no base: the target here, with the host's head.
        assert_eq!(sides(&repo, (None, Some(d)), (c, d)), Sides::Same);
    }
}
