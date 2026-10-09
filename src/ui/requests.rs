//! Merge requests in the branches island: their titles and authors, a click that opens the source
//! branch, and the discussions other people left on one, in the review panel. Read-only; GitLab
//! only (see `crate::mr`).

use super::rows::Row;
use super::{Workspace, format, plural, row, theme};
use crate::git::RefKind;
use crate::mr::{self, Mr, Thread};
use gpui::{ClickEvent, Context, FontWeight, Task, div, prelude::*, px};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    #[default]
    Branches,
    Requests,
}

#[derive(Default)]
pub struct Requests {
    pub available: bool,
    pub list: Vec<Mr>,
    pub loading: bool,
    pub error: Option<String>,
    pub side: Side,
    /// The request of the open branch, when it is one's source branch.
    pub current: Option<u64>,
    pub threads: Vec<Thread>,
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
        let side = if available {
            self.requests.side
        } else {
            Side::Branches
        };
        self.requests.available = available;
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
        let iid = self.request_of_branch(ix).map(|m| m.iid);
        if iid == self.requests.current && iid.is_none() {
            return;
        }
        if iid != self.requests.current {
            self.requests.threads.clear();
        }
        self.requests.current = iid;
        let (Some(iid), Some(remote)) = (iid, self.web.clone()) else {
            return;
        };
        self.requests.thread_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { mr::threads(&remote, iid) })
                .await;
            this.update(cx, |this, cx| {
                if this.requests.current == Some(iid) {
                    match result {
                        Ok(threads) => this.requests.threads = threads,
                        Err(e) => this.requests.error = Some(format!("{e:#}")),
                    }
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
        let Some(src) = pick(&source) else {
            self.error = Some(
                format!(
                    "!{iid}: the branch {source} is not here yet — fetch (Settings → Fetch in the background, or git fetch)."
                )
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
        self.review_open = true;
    }

    /// The requests the filter lets through, in the order the list shows them.
    pub(super) fn shown_requests(&self) -> Vec<&Mr> {
        let filter = self.bfilter.to_lowercase();
        let words: Vec<&str> = filter.split_whitespace().collect();
        self.requests
            .list
            .iter()
            .filter(|m| {
                let hay =
                    format!("!{} {} {} {}", m.iid, m.title, m.author, m.source).to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .collect()
    }

    /// The island's label: which of the two lists the rail's button chose.
    pub(super) fn render_branches_header(&self) -> impl IntoElement {
        if self.requests.available && self.requests.side == Side::Requests {
            let n = self.requests.list.len();
            let label = if self.requests.loading && n == 0 {
                "Merge requests …".to_owned()
            } else {
                format!("Merge requests · {n}")
            };
            return super::island_label(label).into_any_element();
        }
        super::island_label("Branches").into_any_element()
    }

    pub(super) fn render_requests(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let field = self.render_field(
            "branch-filter",
            (self.field == Some(super::lists::Field::Branches)).then_some(self.bfilter.as_str()),
            &self.bfilter,
            "Filter merge requests",
            cx.listener(|this, _: &ClickEvent, _, cx| {
                this.start_field(super::lists::Field::Branches, cx)
            }),
        );
        let shown = self.shown_requests();
        let note = match (&self.requests.error, shown.is_empty()) {
            (Some(e), _) => Some(e.clone()),
            (None, true) if self.requests.loading => Some("Loading merge requests…".to_owned()),
            (None, true) => Some("No open merge requests.".to_owned()),
            _ => None,
        };
        let items = shown.into_iter().map(|m| {
            let iid = m.iid;
            let current = self.requests.current == Some(iid);
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
                                .child(format!("!{iid}")),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(m.title.clone())),
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
                            format::ago(m.updated),
                        )),
                )
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.select_request(iid, cx)),
                )
        });
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(field)
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
                    .children(items.map(|i| div().py(px(1.)).child(i))),
            )
    }

    /// "!412 · Payment retry backoff" next to the branch in the title bar.
    pub(super) fn render_request_title(&self) -> Option<impl IntoElement + use<>> {
        let m = self
            .requests
            .current
            .and_then(|iid| self.requests.list.iter().find(|m| m.iid == iid))?;
        Some(
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
                        .child(format!("!{}", m.iid)),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(theme::muted())
                        .child(m.title.clone()),
                ),
        )
    }

    /// The discussions on the open branch's merge request, for the review panel.
    pub(super) fn render_request_threads(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let iid = self.requests.current?;
        let threads = &self.requests.threads;
        if threads.is_empty() {
            return None;
        }
        let open = threads.iter().filter(|t| !t.resolved).count();
        let items = threads.iter().map(|t| {
            let first = &t.notes[0];
            let where_ = match (&t.path, t.new_line.or(t.old_line)) {
                (Some(p), Some(n)) => format!("{p}:{n}"),
                (Some(p), None) => p.clone(),
                _ => "General".to_owned(),
            };
            let target = t.path.clone().zip(t.new_line.or(t.old_line));
            let old = t.new_line.is_none();
            div()
                .id((
                    "mr-thread",
                    t.notes.len() * 1000 + first.created as usize % 1000,
                ))
                .mx(px(6.))
                .my(px(1.))
                .px(px(10.))
                .py(px(8.))
                .rounded(px(super::ROW_RADIUS))
                .when(target.is_some(), |s| {
                    s.cursor_pointer().hover(|s| s.bg(theme::hover()))
                })
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .font_family(theme::CODE_FONT)
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(div().flex_1().min_w_0().truncate().child(where_))
                        .child(if t.resolved { "resolved ✓" } else { "" }),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme::accent())
                        .child(first.author.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(if t.resolved {
                            theme::muted()
                        } else {
                            theme::text()
                        })
                        .line_clamp(5)
                        .child(first.body.clone()),
                )
                .children((t.notes.len() > 1).then(|| {
                    div()
                        .pt(px(2.))
                        .text_size(px(11.))
                        .text_color(theme::muted())
                        .child(format!(
                            "{} · last by {}",
                            plural(t.notes.len() - 1, "reply"),
                            t.notes
                                .last()
                                .map(|n| n.author.as_str())
                                .unwrap_or_default()
                        ))
                }))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some((path, line)) = target.clone() {
                        this.jump_to_line(&path, line, old, cx)
                    }
                }))
        });
        Some(
            div()
                .flex_none()
                .max_h(px(420.))
                .flex()
                .flex_col()
                .border_b_1()
                .border_color(theme::island_border())
                .child(
                    super::island_label(format!(
                        "GitLab !{iid} · {} · {open} open",
                        plural(threads.len(), "thread")
                    ))
                    .text_color(theme::warning()),
                )
                .child(
                    div()
                        .id("mr-threads")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .pb(px(6.))
                        .children(items),
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
