//! Creating a merge request from a branch: a card with the target, title, description and two
//! switches. The one outward action besides posting comments; it never pushes, so a branch that is
//! only on this Mac is told to be pushed first. Designed in `../Design/mockups/mr-create/`.

use super::input::{self, Edit};
use super::{ISLAND_RADIUS, ROW_RADIUS, Workspace, plural, theme};
use crate::git::{RefKind, Repo};
use crate::mr;
use gpui::{ClickEvent, Context, FontWeight, KeyDownEvent, MouseButton, div, prelude::*, px};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Target,
    Title,
    Description,
}

pub struct NewRequest {
    source: String,
    /// The branch is not on origin, so GitLab cannot open a request from it.
    local_only: bool,
    target: String,
    title: String,
    description: String,
    draft: bool,
    delete_branch: bool,
    focus: Focus,
    /// The request that already exists for this pair.
    existing: Option<u64>,
    commits: usize,
    error: Option<String>,
    posting: bool,
}

impl Workspace {
    /// Opens the card for branch `ix` (the open one when `None`).
    pub(super) fn open_create(&mut self, ix: Option<usize>, cx: &mut Context<Self>) {
        let (Some(root), Some(remote)) = (self.root.clone(), self.web.clone()) else {
            return;
        };
        if remote.github || !self.requests.available {
            self.error = Some(
                "Creating a merge request needs a GitLab token — connect GitLab from the merge requests button on the left."
                    .into(),
            );
            return cx.notify();
        }
        let Some(branch) = ix.or(self.branch).and_then(|i| self.branches.get(i)) else {
            return;
        };
        let source = match branch.kind {
            RefKind::Remote => branch
                .name
                .split_once('/')
                .map_or(branch.name.clone(), |(_, n)| n.to_owned()),
            RefKind::Local => branch.name.clone(),
        };
        let tip = branch.tip;
        let on_origin = self.branches.iter().any(|b| {
            b.kind == RefKind::Remote && b.name.split_once('/').is_some_and(|(_, n)| n == source)
        });
        let target = ["main", "master"]
            .into_iter()
            .find(|t| {
                self.branches.iter().any(|b| {
                    b.kind == RefKind::Remote
                        && b.name.split_once('/').is_some_and(|(_, n)| n == *t)
                })
            })
            .unwrap_or("main")
            .to_owned();
        let title = Repo::open(&root)
            .and_then(|r| r.commit(tip))
            .map(|c| c.summary)
            .unwrap_or_default();
        self.field = None;
        self.find.text = None;
        self.palette = None;
        self.ctx_menu = None;
        // The picker it was opened from closes with it.
        self.picker_open = false;
        self.bfilter.clear();
        self.field_all = false;
        self.newreq = Some(NewRequest {
            source,
            local_only: !on_origin,
            target,
            title,
            description: String::new(),
            draft: false,
            delete_branch: true,
            focus: Focus::Title,
            existing: None,
            commits: 0,
            error: None,
            posting: false,
        });
        self.refresh_create(cx);
        cx.notify();
    }

    /// Recomputes what the card says about the pair: how many commits, and whether a request exists.
    fn refresh_create(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(n) = self.newreq.as_ref() else {
            return;
        };
        let find = |name: &str| {
            self.branches
                .iter()
                .find(|b| {
                    b.kind == RefKind::Remote
                        && b.name.split_once('/').is_some_and(|(_, s)| s == name)
                })
                .or_else(|| {
                    self.branches
                        .iter()
                        .find(|b| b.kind == RefKind::Local && b.name == name)
                })
                .map(|b| (b.name.clone(), b.tip))
        };
        let (source, target) = (find(&n.source), find(&n.target));
        let existing = self
            .requests
            .list
            .iter()
            .find(|m| m.source == n.source && m.target == n.target)
            .map(|m| m.iid);
        let commits = match (&source, &target) {
            (Some(s), Some(t)) => Repo::open(&root)
                .and_then(|r| r.range_log(t.1, s.1, true))
                .map(|l| l.len())
                .unwrap_or(0),
            _ => 0,
        };
        if let Some(n) = self.newreq.as_mut() {
            n.existing = existing;
            n.commits = commits;
        }
        // The diff the request would hold is what shows behind the card.
        if let (Some(s), Some(t)) = (source, target) {
            self.run_compare(t, s, true, None, cx);
        }
    }

    pub(super) fn close_create(&mut self, cx: &mut Context<Self>) {
        if self.newreq.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn newreq_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let Some(n) = self.newreq.as_mut() else {
            return;
        };
        if n.posting {
            return;
        }
        if key.key == "enter" && key.modifiers.platform {
            return self.submit_create(cx);
        }
        if key.key == "d" && key.modifiers.platform {
            n.draft = !n.draft;
            return cx.notify();
        }
        if key.key == "tab" {
            n.focus = match (n.focus, key.modifiers.shift) {
                (Focus::Target, false) | (Focus::Description, true) => Focus::Title,
                (Focus::Title, false) | (Focus::Target, true) => Focus::Description,
                (Focus::Description, false) | (Focus::Title, true) => Focus::Target,
            };
            self.field_all = false;
            return cx.notify();
        }
        if key.key == "enter" && n.focus == Focus::Description {
            if std::mem::take(&mut self.field_all) {
                n.description.clear();
            }
            n.description.push('\n');
            return cx.notify();
        }
        let text = match n.focus {
            Focus::Target => &mut n.target,
            Focus::Title => &mut n.title,
            Focus::Description => &mut n.description,
        };
        match input::edit(text, &mut self.field_all, key, cx) {
            Edit::Escape => return self.close_create(cx),
            Edit::Enter { .. } => {
                // Return finishes a field; on the last one it asks for the same as ⌘↵.
                match n.focus {
                    Focus::Target => n.focus = Focus::Title,
                    Focus::Title => n.focus = Focus::Description,
                    Focus::Description => {}
                }
            }
            Edit::Changed => {
                n.error = None;
                if n.focus == Focus::Target {
                    self.refresh_create(cx);
                }
            }
            Edit::Selected | Edit::Ignored => {}
        }
        cx.notify();
    }

    pub(super) fn submit_create(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        let Some(n) = self.newreq.as_mut() else {
            return;
        };
        if let Some(iid) = n.existing {
            self.newreq = None;
            self.select_request(iid, cx);
            return;
        }
        if n.local_only || n.posting {
            return;
        }
        if n.title.trim().is_empty() {
            n.error = Some("A merge request needs a title.".to_owned());
            return cx.notify();
        }
        n.posting = true;
        n.error = None;
        let (source, target, title, description, draft, delete) = (
            n.source.clone(),
            n.target.trim().to_owned(),
            n.title.trim().to_owned(),
            n.description.trim().to_owned(),
            n.draft,
            n.delete_branch,
        );
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    mr::create(
                        &remote,
                        &source,
                        &target,
                        &title,
                        &description,
                        draft,
                        delete,
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(request) => {
                        let iid = request.iid;
                        this.newreq = None;
                        this.requests.list.insert(0, request);
                        this.requests.side = super::requests::Side::Requests;
                        this.layout.show_sidebar = true;
                        this.select_request(iid, cx);
                    }
                    Err(e) => {
                        if let Some(n) = this.newreq.as_mut() {
                            n.posting = false;
                            n.error = Some(format!("{e:#}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_create(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let n = self.newreq.as_ref()?;
        let host = self.web.as_ref().map(mr::host).unwrap_or_default();
        let tone = theme::warning();
        let field = |id: &'static str, label: &'static str, focus: Focus, body: gpui::Div| {
            let on = n.focus == focus;
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::faint())
                        .child(label.to_uppercase()),
                )
                .child(
                    div()
                        .id(id)
                        .px_2()
                        .py(px(5.))
                        .rounded(px(ROW_RADIUS))
                        .border_2()
                        .border_color(if on {
                            theme::focus()
                        } else {
                            theme::island_border()
                        })
                        .bg(theme::base())
                        .cursor_text()
                        .child(body)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if let Some(n) = this.newreq.as_mut() {
                                n.focus = focus;
                                this.field_all = false;
                                cx.notify();
                            }
                        })),
                )
        };
        let caret = || {
            div()
                .flex_none()
                .w(px(2.))
                .h(px(15.))
                .rounded_full()
                .bg(theme::accent())
        };
        let multi = |text: &str, on: bool| {
            if text.is_empty() {
                return div().min_h(px(64.)).flex().children(on.then(caret)).child(
                    div()
                        .text_color(theme::faint())
                        .child("What it changes and why"),
                );
            }
            div()
                .min_h(px(64.))
                .flex()
                .flex_col()
                .children(text.split('\n').enumerate().map(|(i, line)| {
                    let last = i == text.matches('\n').count();
                    div()
                        .min_h(px(17.))
                        .flex()
                        .child(line.to_owned())
                        .children((on && last).then(caret))
                }))
        };
        let check = |id: &'static str, on: bool, label: &'static str, hint: &'static str| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .child(
                    div()
                        .size(px(14.))
                        .rounded(px(3.))
                        .border_2()
                        .border_color(if on { theme::accent() } else { theme::faint() })
                        .when(on, |s| s.bg(theme::accent()))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.))
                        .text_color(theme::base())
                        .child(if on { "✓" } else { "" }),
                )
                .child(label)
                .child(
                    div()
                        .text_color(theme::faint())
                        .text_size(px(11.))
                        .child(hint),
                )
        };
        let note = if n.local_only {
            Some((
                format!(
                    "{} is only on this Mac, and GitLab cannot open a request from it. Push it first: git push -u origin {}",
                    n.source, n.source
                ),
                true,
            ))
        } else {
            n.existing.map(|iid| {
                (
                    format!(
                        "!{iid} already exists for {} → {}. Press ↵ or ⌘↵ to open it.",
                        n.source, n.target
                    ),
                    false,
                )
            })
        };
        let blocked = n.local_only || n.posting;
        let go_label = if n.posting {
            "Creating…".to_owned()
        } else if let Some(iid) = n.existing {
            format!("Open !{iid}  ⌘↵")
        } else {
            "Create on GitLab  ⌘↵".to_owned()
        };
        Some(
            div()
                .id("create-backdrop")
                .absolute()
                .size_full()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(72.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_create(cx)),
                )
                .child(
                    div()
                        .occlude()
                        .w(px(520.))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .p_4()
                        .rounded(px(ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .text_size(px(13.))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("New merge request"),
                                )
                                .child(div().text_color(theme::faint()).child(host)),
                        )
                        .child(
                            div()
                                .flex()
                                // Both columns carry a label, so the two boxes stand on one line.
                                .items_end()
                                .gap_2()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.))
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme::faint())
                                                .child("FROM"),
                                        )
                                        .child(
                                            div()
                                                .px_2()
                                                .py(px(5.))
                                                .border_2()
                                                .border_color(theme::island_border())
                                                .rounded(px(ROW_RADIUS))
                                                .bg(theme::base())
                                                .font_family(theme::CODE_FONT)
                                                .child(n.source.clone()),
                                        ),
                                )
                                .child(div().pb(px(8.)).text_color(theme::faint()).child("→"))
                                .child(div().flex_1().child(field(
                                    "create-target",
                                    "Into",
                                    Focus::Target,
                                    div().font_family(theme::CODE_FONT).child(input::field_text(
                                        &n.target,
                                        n.focus == Focus::Target,
                                        self.field_all,
                                        "main",
                                    )),
                                ))),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child(if n.commits == 0 {
                                    "no commits ahead of the target".to_owned()
                                } else {
                                    plural(n.commits, "commit")
                                })
                                .children(match &self.diff {
                                    Some(d)
                                        if matches!(d.header, super::Header::Compare { .. }) =>
                                    {
                                        let (a, r) = d
                                            .files
                                            .iter()
                                            .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
                                        Some(
                                            div()
                                                .flex()
                                                .gap_3()
                                                .child(plural(d.files.len(), "file"))
                                                .child(
                                                    div()
                                                        .text_color(theme::added())
                                                        .child(format!("+{a}")),
                                                )
                                                .child(
                                                    div()
                                                        .text_color(theme::removed())
                                                        .child(format!("−{r}")),
                                                ),
                                        )
                                    }
                                    _ => None,
                                }),
                        )
                        .child(field(
                            "create-title",
                            "Title",
                            Focus::Title,
                            input::field_text(
                                &n.title,
                                n.focus == Focus::Title,
                                self.field_all,
                                "Title",
                            ),
                        ))
                        .child(field(
                            "create-description",
                            "Description",
                            Focus::Description,
                            multi(&n.description, n.focus == Focus::Description),
                        ))
                        .child(
                            check(
                                "create-draft",
                                n.draft,
                                "Draft",
                                "cannot be merged until marked ready  ⌘D",
                            )
                            .on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    if let Some(n) = this.newreq.as_mut() {
                                        n.draft = !n.draft;
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            check(
                                "create-delete",
                                n.delete_branch,
                                "Delete the branch when merged",
                                "",
                            )
                            .on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    if let Some(n) = this.newreq.as_mut() {
                                        n.delete_branch = !n.delete_branch;
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                        .children(note.map(|(text, warn)| {
                            div()
                                .px_2()
                                .py(px(5.))
                                .rounded(px(ROW_RADIUS))
                                .border_1()
                                .border_color(if warn { tone } else { theme::island_border() })
                                .text_size(px(12.))
                                .text_color(if warn { tone } else { theme::muted() })
                                .child(text)
                        }))
                        .children(n.error.clone().map(|e| {
                            div()
                                .text_size(px(12.))
                                .text_color(theme::removed())
                                .child(e)
                        }))
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .id("create-cancel")
                                        .px_2()
                                        .rounded(px(ROW_RADIUS))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme::hover()))
                                        .child("Cancel  esc")
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.close_create(cx)
                                        })),
                                )
                                .child(
                                    div()
                                        .id("create-go")
                                        .px_3()
                                        .py(px(2.))
                                        .rounded(px(ROW_RADIUS))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .bg(tone)
                                        .text_color(theme::base())
                                        .when(blocked, |s| s.opacity(0.45))
                                        .when(!blocked, |s| s.cursor_pointer())
                                        .child(go_label)
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.submit_create(cx)
                                        })),
                                ),
                        ),
                ),
        )
    }
}
