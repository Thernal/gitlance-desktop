//! Merge requests in the branches island: their titles and authors, a click that opens the source
//! branch, and the discussions other people left on one, in the review panel. Read-only; GitLab
//! only (see `crate::mr`).

use super::px;
use super::rows::Row;
use super::tools::ToolView;
use super::{Header, Selection, Workspace, format, island, plural, row, theme};
use crate::git::{RefKind, Repo, Version};
use crate::mr::{self, Draft, Mr, Pipeline, Thread};
use gpui::{ClickEvent, Context, FontWeight, SharedString, Task, WeakEntity, div, prelude::*};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    #[default]
    Branches,
    Requests,
}

#[derive(Default)]
pub struct Requests {
    pub available: bool,
    /// A GitLab project without a token: the rail offers to connect instead of listing.
    pub connectable: bool,
    pub list: Vec<Mr>,
    pub loading: bool,
    pub error: Option<String>,
    pub side: Side,
    /// The request of the open branch, when it is one's source branch.
    pub current: Option<u64>,
    pub threads: Vec<Thread>,
    /// One's own pending comments on the current request.
    pub drafts: Vec<Draft>,
    /// The request's newest pipeline.
    pub pipeline: Option<Pipeline>,
    /// Who approved the request, and where the reviewer stands.
    pub approvals: Option<mr::Approvals>,
    /// The pushes GitLab kept for a request, as versions of this repository.
    pub versions: Option<(u64, Vec<Version>)>,
    version_task: Option<Task<()>>,
    /// The request left with ✕: its branch shows as a plain branch until a request is picked again.
    pub dismissed: Option<u64>,
    /// The token's username: one's own notes are the ones it wrote, and can be edited.
    pub login: Option<String>,
    task: Option<Task<()>>,
    thread_task: Option<Task<()>>,
}

impl Workspace {
    /// Reads the open merge requests of the repository's project, when a token is configured.
    pub(super) fn load_requests(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            self.requests = Requests::default();
            return;
        };
        let available = mr::available(&remote);
        let connectable = !available;
        let side = if available || connectable {
            self.requests.side
        } else {
            Side::Branches
        };
        self.requests.available = available;
        self.requests.connectable = connectable;
        self.requests.side = side;
        if !available {
            self.requests.list.clear();
            self.requests.error = None;
            return;
        }
        self.requests.loading = true;
        self.requests.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { mr::list(&remote) })
                .await;
            this.update(cx, |this, cx| {
                this.requests.loading = false;
                match result {
                    Ok(list) => {
                        this.requests.list = list;
                        this.requests.error = None;
                    }
                    Err(e) => this.requests.error = Some(format!("{e:#}")),
                }
                if let Some(ix) = this.branch {
                    this.sync_request(ix, cx);
                }
                this.follow_pending_link(cx);
                this.load_notices(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// The request whose source branch is branch `ix` (`feat/x` and `origin/feat/x` alike).
    pub(super) fn request_of_branch(&self, ix: usize) -> Option<&Mr> {
        let b = self.branches.get(ix)?;
        let name = match b.kind {
            RefKind::Remote => b.name.split_once('/').map_or(b.name.as_str(), |(_, n)| n),
            RefKind::Local => b.name.as_str(),
        };
        self.requests.list.iter().find(|m| m.source == name)
    }

    /// Reads the discussions of the current request again (after posting to it).
    pub(super) fn reload_threads(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self.branch {
            self.requests.current = None;
            self.sync_request(ix, cx);
        }
    }

    /// Follows the branch selection: the request of the branch becomes the current one and its
    /// discussions are read.
    pub(super) fn sync_request(&mut self, ix: usize, cx: &mut Context<Self>) {
        let dismissed = self.requests.dismissed;
        let iid = self
            .request_of_branch(ix)
            .map(|m| m.iid)
            .filter(|iid| dismissed != Some(*iid));
        if iid == self.requests.current && iid.is_none() {
            return;
        }
        if iid != self.requests.current {
            self.requests.threads.clear();
            self.requests.drafts.clear();
            self.requests.pipeline = None;
            self.requests.approvals = None;
            self.draft_at = None;
        }
        self.requests.current = iid;
        let (Some(iid), Some(remote)) = (iid, self.web.clone()) else {
            return;
        };
        let known = self.requests.login.is_some();
        self.requests.thread_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // Drafts need a recent GitLab; without them the rest still works.
                    let drafts = mr::drafts(&remote, iid).unwrap_or_default();
                    let pipeline = mr::pipeline(&remote, iid).ok().flatten();
                    let approvals = mr::approvals(&remote, iid).ok();
                    let login = if known {
                        None
                    } else {
                        mr::username(&remote).ok()
                    };
                    mr::threads(&remote, iid).map(|t| (t, drafts, pipeline, approvals, login))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.requests.current == Some(iid) {
                    match result {
                        Ok((threads, drafts, pipeline, approvals, login)) => {
                            if login.is_some() {
                                this.requests.login = login;
                            }
                            this.requests.threads = threads;
                            this.requests.drafts = drafts;
                            this.requests.pipeline = pipeline;
                            this.requests.approvals = approvals;
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
        let Some((source, target)) = self
            .requests
            .list
            .iter()
            .find(|m| m.iid == iid)
            .map(|m| (m.source.clone(), m.target.clone()))
        else {
            return;
        };
        self.requests.dismissed = None;
        self.load_request_versions(iid, cx);
        if let Some(m) = self.requests.list.iter().find(|m| m.iid == iid) {
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
        let found = pick(&source).or_else(|| github.then(|| pick(&format!("pr/{iid}"))).flatten());
        let Some(src) = found else {
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
        if let Some(tgt) = pick(&target) {
            let side = |ix: usize| (self.branches[ix].name.clone(), self.branches[ix].tip);
            self.pending_compare = Some((side(tgt), side(src), iid));
        }
        self.select_branch(src, None, cx);
        self.record();
    }

    /// ✕ on the request (⌥⌘L): back to the checked-out branch, as a plain branch, with the branches
    /// in the rail.
    pub(super) fn leave_request(&mut self, cx: &mut Context<Self>) {
        let Some(iid) = self.requests.current else {
            return;
        };
        self.requests.dismissed = Some(iid);
        self.requests.side = Side::Branches;
        self.pending_compare = None;
        self.close_picker(cx);
        match self.branches.iter().position(|b| b.is_head).or(self.branch) {
            Some(ix) => self.select_branch(ix, None, cx),
            None => self.sync_request(0, cx),
        }
        cx.notify();
    }

    /// ⌥↓: the next merge request, in list order, that is not reviewed yet; ⌥↑: the previous one,
    /// reviewed or not (to look again at what was just done).
    pub(super) fn step_request(&mut self, forward: bool, cx: &mut Context<Self>) {
        let shown = self.shown_requests();
        let n = shown.len();
        if n == 0 {
            return;
        }
        let at = shown
            .iter()
            .position(|m| Some(m.iid) == self.requests.current);
        let next = if forward {
            let reviewed: Vec<bool> = shown
                .iter()
                .map(|m| git2::Oid::from_str(&m.head).is_ok_and(|h| self.head_reviewed(h)))
                .collect();
            next_unreviewed(&reviewed, at).map(|ix| shown[ix])
        } else {
            Some(shown[at.map_or(n - 1, |a| (a + n - 1) % n)])
        };
        let Some(next) = next.map(|m| m.iid) else {
            self.error = Some("Every other merge request in the list is reviewed.".into());
            return cx.notify();
        };
        self.done_card = false;
        self.select_request(next, cx);
    }

    /// The next request after the open one, in list order, whose head is not reviewed yet.
    fn next_unreviewed_request(&self) -> Option<&Mr> {
        let shown = self.shown_requests();
        let at = shown
            .iter()
            .position(|m| Some(m.iid) == self.requests.current)?;
        (1..shown.len())
            .map(|step| shown[(at + step) % shown.len()])
            .find(|m| git2::Oid::from_str(&m.head).map_or(true, |head| !self.head_reviewed(head)))
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
                .next_unreviewed_request()
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
                                Ok(ix) => this.select_branch(ix, None, cx),
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
    /// request's diff is open.
    fn load_request_versions(&mut self, iid: u64, cx: &mut Context<Self>) {
        self.requests.versions = None;
        let (Some(remote), Some(root)) = (self.web.clone(), self.root.clone()) else {
            return;
        };
        self.requests.version_task = Some(cx.spawn(async move |this, cx| {
            let built = cx
                .background_executor()
                .spawn(async move {
                    let pushes: Vec<_> = mr::versions(&remote, iid)?
                        .iter()
                        .filter_map(|p| {
                            Some((
                                git2::Oid::from_str(&p.head).ok()?,
                                git2::Oid::from_str(&p.base).ok(),
                                p.created,
                            ))
                        })
                        .collect();
                    anyhow::Ok(Repo::open(&root)?.versions_from_pushes(&pushes))
                })
                .await;
            this.update(cx, |this, cx| {
                if let Ok(list) = built {
                    this.requests.versions = Some((iid, list));
                    this.apply_mr_versions();
                    cx.notify();
                }
            })
            .ok();
        }));
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

    /// Is the open diff the one of the request whose discussions are loaded?
    pub(super) fn showing_request(&self) -> bool {
        matches!(
            self.diff.as_ref().map(|d| &d.header),
            Some(Header::Compare { request: Some(iid), .. }) if Some(*iid) == self.requests.current
        )
    }

    /// The requests list is open and no request is shown (nor loading): the diff of the branch
    /// behind it would read as the request's, so the right side waits for a choice.
    pub(super) fn choosing_request(&self) -> bool {
        self.picker_open
            && self.requests.side == Side::Requests
            && self.requests.available
            && self.open_request.is_none()
            && !self.showing_request()
    }

    /// The right side while a request is being chosen.
    pub(super) fn render_choose_request(&self) -> impl IntoElement + use<> {
        let key = |k: &'static str, what: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(super::keys::caps(k, super::keys::On::Plain))
                .child(what)
        };
        island()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .bg(theme::editor())
            .text_color(theme::muted())
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child(super::words::say("Choose a merge request").into_owned()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .text_size(px(12.))
                    .child(key("↑↓", "move"))
                    .child(key("↵", "open"))
                    .child(key("esc", "back to the branch")),
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
            .filter_map(|(ix, t)| match (t.new_line, t.old_line) {
                (Some(n), _) => Some((ix, false, n)),
                (None, Some(o)) => Some((ix, true, o)),
                _ => None,
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
        let can_act = self.request_target().is_some();
        let (fold, pick, reply, resolve, raw, edit) = (
            this.clone(),
            this.clone(),
            this.clone(),
            this.clone(),
            this.clone(),
            this.clone(),
        );
        let selecting = self.selecting(this);
        let first = &t.notes[0];
        let author = self
            .requests
            .current
            .and_then(|iid| self.requests.list.iter().find(|m| m.iid == iid))
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
            .children(t.resolvable.then(|| discussion::state(t.resolved)))
            .tooltip(move |_, cx| {
                cx.new(|_| super::Tip(if open { "Collapse  O" } else { "Expand  O" }))
                    .into()
            })
            .on_click(move |_, _, cx| {
                fold.update(cx, |this, cx| {
                    this.thread_at = Some(ix);
                    this.set_thread_open(ix, !open, cx)
                })
                .ok();
            });
        // Replying: the timeline runs on from the last note to my avatar.
        let replying = self
            .compose
            .as_ref()
            .is_some_and(|c| c.target == super::comments::Target::Reply(ix));
        let last = t.notes.len() - 1;
        let mine = self.my_note_in(ix);
        let notes = t.notes.iter().enumerate().map(|(n, note)| {
            let change = super::comments::Change::Note {
                thread: ix,
                note: note.id,
            };
            let edit_box = self.edit_box(change, edit.clone());
            let own = self.requests.login.as_deref() == Some(note.login.as_str());
            let editable = can_act && self.compose.is_none() && own;
            discussion::note(
                &note.author,
                own,
                (author.as_deref() == Some(note.author.as_str())).then_some("Author"),
                format::ago(note.created),
                match edit_box {
                    Some(edit_box) => edit_box.into_any_element(),
                    None => super::markdown::body(
                        &note.body,
                        self.settings.rich_comments,
                        format!("md-note-{ix}-{n}"),
                        self.link_base(),
                        Some(selecting.clone()),
                    ),
                },
                n == last && !replying,
                editable.then(|| {
                    // `e` edits the last of one's notes in the discussion with the keyboard.
                    let key = (focused && mine == Some(note.id)).then_some("e");
                    super::comments::edit_button(
                        ("edit-note", note.id as usize),
                        key,
                        change,
                        edit.clone(),
                    )
                }),
            )
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
                            .child(super::keys::caps("r", super::keys::On::Plain))
                            .on_click(move |_, _, cx| {
                                reply.update(cx, |this, cx| this.start_reply(ix, cx)).ok();
                            }),
                    )
                    .children(t.resolvable.then(|| {
                        discussion::button(
                            ("thread-resolve", ix),
                            if t.resolved { "Reopen" } else { "Resolve" },
                            Some("x"),
                            Kind::Plain,
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
                    .when(open, |s| {
                        s.child(div().flex().flex_col().children(notes))
                            .children(reply_row)
                    }),
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
        let (delete, raw) = (this.clone(), this);
        let bar = discussion::bar(("draft-bar", ix), true)
            .cursor_default()
            .child(discussion::source_pill(Source::Pending))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child("· only you see it until you submit the review"),
            )
            .children(super::markdown::has_markdown(&d.body).then(|| {
                self.rich_switch(("draft-rich", ix))
                    .on_click(move |_, _, cx| {
                        raw.update(cx, |this, cx| this.toggle_rich_comments(cx))
                            .ok();
                    })
            }))
            .child(
                discussion::button(("draft-delete", ix), "Delete", None, Kind::Danger)
                    .h(px(24.))
                    .px(px(8.))
                    .on_click(move |_, _, cx| {
                        delete
                            .update(cx, |this, cx| this.discard_draft(id, cx))
                            .ok();
                    }),
            );
        div()
            .w_full()
            .pl(px(indent))
            .pr(px(12.))
            .py(px(6.))
            .child(
                discussion::frame(Source::Pending, self.draft_at == Some(ix))
                    .child(bar)
                    .child(div().child(discussion::note(
                        &self.me,
                        true,
                        None,
                        "pending".to_owned(),
                        super::markdown::body(
                            &d.body,
                            self.settings.rich_comments,
                            format!("md-draft-{ix}"),
                            self.link_base(),
                            Some(selecting),
                        ),
                        true,
                        None,
                    ))),
            )
            .into_any_element()
    }

    /// Runs a GitLab call away from the interface; afterwards the request is read again.
    fn request_action(
        &mut self,
        work: impl FnOnce(crate::git::WebRemote) -> anyhow::Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { work(remote) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.error = None;
                        this.reload_threads(cx);
                    }
                    Err(e) => this.error = Some(format!("{e:#}").into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn toggle_resolved(&mut self, ix: usize, cx: &mut Context<Self>) {
        let (Some(t), Some(iid)) = (self.requests.threads.get(ix), self.requests.current) else {
            return;
        };
        let (id, resolved) = (t.id.clone(), !t.resolved);
        self.request_action(move |remote| mr::resolve(&remote, iid, &id, resolved), cx);
    }

    /// `a`: approves the open request, or takes one's approval back.
    pub(super) fn toggle_approval(&mut self, cx: &mut Context<Self>) {
        let (Some(iid), Some(a)) = (self.requests.current, self.requests.approvals.clone()) else {
            return;
        };
        if !self.showing_request() {
            return;
        }
        if a.mine {
            self.request_action(move |remote| mr::unapprove(&remote, iid), cx);
        } else if a.can {
            let head = self
                .requests
                .list
                .iter()
                .find(|m| m.iid == iid)
                .map(|m| m.head.clone())
                .unwrap_or_default();
            self.request_action(move |remote| mr::approve(&remote, iid, &head), cx);
        } else {
            self.error = Some("GitLab does not let you approve this merge request.".into());
            cx.notify();
        }
    }

    /// ⌥N / ⌥P: the next or previous of one's pending comments, across the files of the request.
    pub(super) fn step_draft(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(diff) = self.diff.as_ref().filter(|_| self.showing_request()) else {
            return;
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
        if order.is_empty() {
            return;
        }
        order.sort_unstable();
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

    fn discard_draft(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(iid) = self.requests.current else {
            return;
        };
        self.request_action(move |remote| mr::delete_draft(&remote, iid, id), cx);
    }

    /// "!412 · 3" on the Commits label: back to the diff of the whole request after one commit.
    pub(super) fn render_request_chip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (base, head, iid) = self.open_request.clone()?;
        let selected = self.selection == Selection::None && self.showing_request();
        let n = self.commits.len();
        Some(
            super::commit_chip("request-row", selected)
                .child(super::words::number(iid))
                .tooltip(|_, cx| cx.new(|_| super::Tip("The whole merge request")).into())
                .child(div().text_color(theme::warning()).child(n.to_string()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.run_compare(base.clone(), head.clone(), true, Some(iid), cx)
                })),
        )
    }

    /// The Discussions tool window: every comment on the open diff, the request's in orange and the
    /// agent's in purple; a click shows it in the diff.
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
        let (discussions, agents) = (theirs.len(), self.comments.len());
        let total = discussions + pending + agents;
        // Group headings once there is more than one kind; the pending ones say how to walk them.
        let grouped = [discussions, pending, agents]
            .iter()
            .filter(|&&n| n > 0)
            .count()
            > 1;
        let request_items = theirs.into_iter().map(|(ix, t)| {
            let target = t.path.clone().zip(t.new_line.or(t.old_line));
            let old = t.new_line.is_none();
            let where_ = match &target {
                Some((p, n)) => format!("{}:{n}", p.rsplit('/').next().unwrap_or(p)),
                None => "General".to_owned(),
            };
            let first = &t.notes[0];
            comment_item(
                ("comment-mr", ix),
                theme::warning(),
                where_,
                format!("{}: {}", first.author, self.one_line(&first.body)),
            )
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
                self.one_line(&d.body),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if let Some((path, line)) = target.clone() {
                    this.jump_to_line(&path, line, old, cx)
                }
            }))
        });
        let agent_items = self.comments.iter().map(|c| {
            let id = c.id;
            comment_item(
                ("comment-agent", id as usize),
                theme::agent(),
                format!(
                    "{}:{}",
                    c.path.rsplit('/').next().unwrap_or(&c.path),
                    c.line
                ),
                self.one_line(&c.body),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.jump_to(id, cx)))
        });
        let copy = (!self.comments.is_empty()).then(|| {
            div()
                .id("copy-for-agent")
                .px_2()
                .h(px(24.))
                .flex()
                .items_center()
                .rounded(px(super::ROW_RADIUS))
                .border_1()
                .border_color(theme::agent())
                .text_color(theme::agent())
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(if self.copied {
                    "Copied ✓".to_owned()
                } else {
                    format!(
                        "Copy for agent · {}",
                        self.comments.iter().filter(|c| c.open()).count()
                    )
                })
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.copy_for_agent(cx)))
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
                .children(
                    (grouped && agents > 0)
                        .then(|| group_heading(format!("For your agent · {agents}"), None)),
                )
                .children(agent_items)
                .children((total == 0).then(|| {
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child("No comments yet. Press c to comment on a line, or hover a line and press +.")
                }))
                .into_any_element(),
            foot: (copy.is_some() || submit.is_some()).then(|| {
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .children(copy)
                    .children(submit)
                    .into_any_element()
            }),
        }
    }

    /// Every comment of the open diff: the request's discussions and pending ones, and the agent's.
    pub(super) fn comments_total(&self) -> usize {
        let theirs = if self.showing_request() {
            self.requests.threads.len() + self.requests.drafts.len()
        } else {
            0
        };
        theirs + self.comments.len()
    }

    /// The requests the filter lets through, in the order the list shows them.
    pub(super) fn shown_requests(&self) -> Vec<&Mr> {
        let filter = self.bfilter.to_lowercase();
        let words: Vec<&str> = filter.split_whitespace().collect();
        let mut shown: Vec<&Mr> = self
            .requests
            .list
            .iter()
            .filter(|m| {
                let hay = format!(
                    "{} {} {} {}",
                    super::words::number(m.iid),
                    m.title,
                    m.author,
                    m.source
                )
                .to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .collect();
        if self.settings.mr_by_updated {
            shown.sort_by_key(|m| std::cmp::Reverse(m.updated));
        } else {
            shown.sort_by_key(|m| std::cmp::Reverse(m.created));
        }
        shown
    }

    /// Newest first, or most recently updated first.
    fn set_mr_order(&mut self, by_updated: bool, cx: &mut Context<Self>) {
        self.settings.mr_by_updated = by_updated;
        self.settings.save();
        cx.notify();
    }

    /// The island's label: which of the two lists the rail's button chose.
    pub(super) fn render_branches_header(&self) -> impl IntoElement {
        if self.requests.connectable && self.requests.side == Side::Requests {
            return super::island_label(format!(
                "Connect GitLab{}",
                self.zone_tag(super::zones::Zone::Branches)
            ))
            .into_any_element();
        }
        if self.requests.available && self.requests.side == Side::Requests {
            let n = self.requests.list.len();
            let label = if self.requests.loading && n == 0 {
                super::words::say("Merge requests …").into_owned()
            } else {
                super::words::say(&format!("Merge requests · {n}")).into_owned()
            };
            return super::island_label(format!(
                "{label}{}",
                self.zone_tag(super::zones::Zone::Branches)
            ))
            .into_any_element();
        }
        super::island_label(format!(
            "Branches{}",
            self.zone_tag(super::zones::Zone::Branches)
        ))
        .into_any_element()
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

    /// The sidebar's top island for a GitLab project without a token.
    pub(super) fn render_connect(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let Some(remote) = self.web.clone() else {
            return div().into_any_element();
        };
        let host = mr::host(&remote);
        let page = mr::token_page(&remote);
        let field = self.render_field(
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
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_2()
            .px(px(8.))
            .child(
                div()
                    .px(px(6.))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(if remote.github {
                        format!(
                            "See the pull requests of {host}: comment, reply, resolve and review here. GitLance needs a token with the repo scope — or sign in once with the GitHub CLI (gh auth login) and it uses that. It stays on this Mac."
                        )
                    } else {
                        format!(
                            "See the merge requests of {host}: comment, reply and resolve here. GitLance needs a personal access token with the api scope. It stays on this Mac."
                        )
                    }),
            )
            .child(
                div()
                    .id("token-create")
                    .mx(px(6.))
                    .px_2()
                    .py(px(4.))
                    .rounded(px(super::ROW_RADIUS))
                    .border_1()
                    .border_color(theme::island_border())
                    .text_size(px(12.))
                    .text_color(theme::accent())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(format!("1 · Create a token on {host} ↗"))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&page))),
            )
            .child(
                div()
                    .px(px(6.))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("2 · Paste it here and press ↵"),
            )
            .child(field)
            .children(note.map(|(text, color)| {
                div()
                    .px(px(6.))
                    .text_size(px(12.))
                    .text_color(color)
                    .child(text)
            }))
            .into_any_element()
    }

    pub(super) fn render_requests(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let field = self.render_field(
            "branch-filter",
            super::lists::Field::Branches,
            "Filter merge requests",
            cx,
        );
        let shown = self.shown_requests();
        let note = match (&self.requests.error, shown.is_empty()) {
            (Some(e), _) => Some(e.clone()),
            (None, true) if self.requests.loading => {
                Some(super::words::say("Loading merge requests…").into_owned())
            }
            (None, true) => Some(super::words::say("No open merge requests.").into_owned()),
            _ => None,
        };
        let items = shown.into_iter().map(|m| {
            let iid = m.iid;
            let current = self.requests.current == Some(iid);
            let head = git2::Oid::from_str(&m.head).ok();
            let reviewed = head.is_some_and(|h| self.head_reviewed(h));
            let part = head.and_then(|h| self.head_part(h));
            row(("mr", iid), current)
                .h(px(50.))
                .flex_col()
                .items_start()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme::accent())
                                .child(super::words::number(iid)),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(m.title.clone()))
                        .children(reviewed.then(|| {
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(theme::added())
                                .child("✓ reviewed")
                        }))
                        .children(
                            part.map(|(done, total)| super::lists::progress_badge(done, total)),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(theme::muted())
                        .child(format!(
                            "{}{} · {}{} · {} · {}",
                            if m.draft { "draft · " } else { "" },
                            m.author,
                            m.source,
                            if matches!(m.target.as_str(), "main" | "master") {
                                String::new()
                            } else {
                                format!(" → {}", m.target)
                            },
                            plural(m.comments, "comment"),
                            if self.settings.mr_by_updated {
                                format!("updated {}", format::ago(m.updated))
                            } else {
                                format!("opened {}", format::ago(m.created))
                            },
                        )),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.select_request(iid, cx);
                    this.close_picker(cx);
                }))
        });
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(field)
            .child({
                let by_updated = self.settings.mr_by_updated;
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px(px(14.))
                    .pb(px(4.))
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child("Sort")
                    .child(
                        super::diff_view::group()
                            .child(
                                super::diff_view::chip("mr-by-created", "Newest", !by_updated)
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.set_mr_order(false, cx)
                                    })),
                            )
                            .child(
                                super::diff_view::chip("mr-by-updated", "Updated", by_updated)
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.set_mr_order(true, cx)
                                    })),
                            ),
                    )
            })
            .child(
                div()
                    .id("requests")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(6.))
                    .pb(px(6.))
                    .children(note.map(|n| {
                        div()
                            .px(px(8.))
                            .py(px(6.))
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(n)
                    }))
                    .children(items.map(|i| div().py(px(1.)).child(i)))
                    .child(
                        div().py(px(1.)).child(
                            row("new-request", false)
                                .h(px(32.))
                                .gap_2()
                                .text_color(theme::warning())
                                .child(super::words::say("＋ New merge request…").into_owned())
                                .child(div().flex_1())
                                .child(super::keys::caps("⌥⌘M", super::keys::On::Plain))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.open_create(None, cx)
                                })),
                        ),
                    ),
            )
    }

    /// "!412 · Payment retry backoff" next to the branch in the title bar.
    pub(super) fn render_request_title(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let m = self
            .requests
            .current
            .and_then(|iid| self.requests.list.iter().find(|m| m.iid == iid))?;
        let pill = self.requests.pipeline.clone().map(|p| {
            let (glyph, word, color) = match p.status.as_str() {
                "success" => ("✓", "passed".to_owned(), theme::added()),
                "failed" => (
                    "✗",
                    p.failed
                        .as_ref()
                        .map_or("failed".to_owned(), |(job, _)| format!("{job} failed")),
                    theme::removed(),
                ),
                "running" => ("●", "running".to_owned(), theme::warning()),
                "pending" | "created" | "waiting_for_resource" | "preparing" => {
                    ("○", "pending".to_owned(), theme::muted())
                }
                "canceled" => ("⊘", "canceled".to_owned(), theme::muted()),
                other => ("·", other.replace('_', " "), theme::muted()),
            };
            // A failure opens the job that failed; the rest open the pipeline.
            let url = p
                .failed
                .as_ref()
                .map(|(_, u)| u.clone())
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| p.url.clone());
            let ago = (p.updated > 0).then(|| format::ago(p.updated));
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
                .tooltip(|_, cx| cx.new(|_| super::Tip("Open the pipeline on GitLab")).into())
                .child(format!("{glyph} {word}"))
                .children(ago.map(|a| div().text_color(theme::faint()).child(a)))
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&url)))
        });
        let approval = self.requests.approvals.clone().map(|a| {
            let count = if a.required > 0 {
                format!(" {}/{}", a.required - a.left.min(a.required), a.required)
            } else if !a.by.is_empty() && !a.mine {
                format!(" {}", a.by.len())
            } else {
                String::new()
            };
            let tip: SharedString = match (a.by.is_empty(), a.mine, a.can) {
                (_, true, _) => format!("Approved by {} — a takes yours back", a.by.join(", ")),
                (true, false, true) => "Approve this merge request  a".to_owned(),
                (false, false, true) => format!("Approved by {} — a approves", a.by.join(", ")),
                (true, false, false) => "No approvals yet".to_owned(),
                (false, false, false) => format!("Approved by {}", a.by.join(", ")),
            }
            .into();
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
                .child(format!("{label}{count}"))
                .when(a.can && !a.mine, |s| {
                    s.child(super::keys::caps("a", super::keys::On::Plain))
                })
                .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_approval(cx)))
        });
        Some(
            div()
                .flex_1()
                .flex()
                .items_center()
                .gap_3()
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .max_w(px(480.))
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme::accent())
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(super::words::number(m.iid)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(theme::muted())
                                .child(m.title.clone()),
                        ),
                )
                .children(pill)
                .children(approval)
                .child(
                    div()
                        .id("leave-request")
                        .flex_none()
                        .size(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .text_color(theme::faint())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                        .child("✕")
                        .tooltip(|_, cx| {
                            cx.new(|_| {
                                super::Tip("Leave the merge request — back to your branch  ⌥⌘L")
                            })
                            .into()
                        })
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.leave_request(cx)),
                        ),
                ),
        )
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
fn next_unreviewed(reviewed: &[bool], at: Option<usize>) -> Option<usize> {
    let n = reviewed.len();
    let start = at.map_or(0, |a| a + 1);
    (0..n)
        .map(|step| (start + step) % n)
        .find(|&ix| Some(ix) != at && !reviewed[ix])
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
    use super::next_unreviewed;

    #[test]
    fn the_next_unreviewed_request_goes_round_to_the_top() {
        // 0 open, 1 reviewed, 2 current, 3 reviewed: past the end, back to 0.
        assert_eq!(
            next_unreviewed(&[false, true, false, true], Some(2)),
            Some(0)
        );
        assert_eq!(next_unreviewed(&[true, false, true], Some(0)), Some(1));
        assert_eq!(next_unreviewed(&[true, false], None), Some(1));
        // Only the current one is left: nowhere to go.
        assert_eq!(next_unreviewed(&[true, false, true], Some(1)), None);
    }
}
