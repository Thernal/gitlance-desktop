//! Creating a merge request from a branch (Design #47 E, `docs/specs/new-request.md`, flow 08): the
//! request is written in place of the diff while the branch's commits and changes stay on the left.
//! From → Into, the target picked from the branch list; the description starts from the project's
//! template on the target (or a single commit's body); reviewers are suggested from the code owners
//! of the changed files; Request | Changes (⌃⇥) shows the diff and keeps the draft. The one outward
//! action besides posting comments; it never pushes, so a branch only on this Mac is told to be
//! pushed first.

use super::input::{self, Edit};
use super::{ROW_RADIUS, Workspace, chip, markdown, plural, px, theme};
use crate::codeowners::{self, Rule};
use crate::git::{RefKind, Repo};
use crate::mr;
use git2::Oid;
use gpui::{AnyElement, ClickEvent, Context, FontWeight, KeyDownEvent, div, prelude::*};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Target,
    Title,
    Description,
}

pub struct NewRequest {
    source: String,
    /// The branch is not on origin, so the host cannot open a request from it.
    local_only: bool,
    target: String,
    title: String,
    description: String,
    /// Where the description started from: "Default.md template", "the commit's message".
    started_from: Option<String>,
    draft: bool,
    delete_branch: bool,
    squash: bool,
    focus: Focus,
    /// The target list's cursor while Into has the keyboard.
    target_at: usize,
    /// The request that already exists for this pair.
    existing: Option<u64>,
    commits: usize,
    /// CODEOWNERS of the target, for the suggested reviewers; the ones chosen.
    owners: Vec<Rule>,
    reviewers: Vec<String>,
    /// The description as it will read, instead of its text.
    preview: bool,
    /// Request | Changes: the diff of the pair is shown, the draft kept.
    changes: bool,
    error: Option<String>,
    posting: bool,
}

/// `text` cut in the middle to `n` characters: long branch names keep both ends.
pub fn mid(text: &str, n: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= n || n < 5 {
        return text.to_owned();
    }
    let head = n.div_ceil(2) - 1;
    let tail = n - head - 1;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// The template a new request starts from, read from the target with `file`/`files`: GitLab's
/// `.gitlab/merge_request_templates/Default.md` (any case), GitHub's `pull_request_template.md` in
/// `.github/`, `docs/` or the root. Its name and text.
pub fn template(
    github: bool,
    file: impl Fn(&str) -> Option<String>,
    files: impl Fn(&str) -> Vec<String>,
) -> Option<(String, String)> {
    if github {
        for dir in [".github", "docs", ""] {
            if let Some(name) = files(dir)
                .into_iter()
                .find(|n| n.eq_ignore_ascii_case("pull_request_template.md"))
            {
                let path = if dir.is_empty() {
                    name.clone()
                } else {
                    format!("{dir}/{name}")
                };
                return file(&path).map(|t| (name, t));
            }
        }
        return None;
    }
    let dir = ".gitlab/merge_request_templates";
    let name = files(dir)
        .into_iter()
        .find(|n| n.eq_ignore_ascii_case("default.md"))?;
    file(&format!("{dir}/{name}")).map(|t| (format!("{name} template"), t))
}

impl Workspace {
    /// Opens the form for branch `ix` (the open one when `None`).
    pub(super) fn open_create(&mut self, ix: Option<usize>, cx: &mut Context<Self>) {
        let (Some(root), Some(web)) = (self.root.clone(), self.web.clone()) else {
            return;
        };
        if !self.requests.available {
            self.error = Some(
                super::words::say("Creating a merge request needs a GitLab token — connect GitLab from Review requests in the menu on the left.")
                    .into_owned()
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
            .find(|t| self.remote_tip(t).is_some())
            .unwrap_or("main")
            .to_owned();
        let repo = Repo::open(&root).ok();
        let commit = repo.as_ref().and_then(|r| r.commit(tip).ok());
        let title = commit
            .as_ref()
            .map(|c| c.summary.clone())
            .unwrap_or_default();
        self.field = None;
        self.find.text = None;
        self.palette = None;
        self.ctx_menu = None;
        // The picker it was opened from closes with it.
        self.picker_open = false;
        self.bfilter.clear();
        self.reset_field_edit();
        self.newreq = Some(NewRequest {
            source,
            local_only: !on_origin,
            target,
            title,
            description: String::new(),
            started_from: None,
            draft: false,
            delete_branch: true,
            squash: false,
            focus: Focus::Title,
            target_at: 0,
            existing: None,
            commits: 0,
            owners: Vec::new(),
            reviewers: Vec::new(),
            preview: false,
            changes: false,
            error: None,
            posting: false,
        });
        self.refresh_create(cx);
        // The description: the target's template, else the one commit's message body.
        let target_tip = self
            .newreq
            .as_ref()
            .and_then(|n| self.remote_tip(&n.target));
        if let Some(n) = self.newreq.as_mut() {
            let found = match (&repo, target_tip) {
                (Some(r), Some(t)) => {
                    template(web.github, |p| r.file_at(t, p), |d| r.files_at(t, d))
                }
                _ => None,
            };
            if let Some((name, text)) = found {
                n.description = text;
                n.started_from = Some(format!("from {name}"));
            } else if n.commits == 1
                && let Some(body) = commit
                    .as_ref()
                    .and_then(|c| c.message.split_once('\n'))
                    .map(|(_, b)| b.trim().to_owned())
                    .filter(|b| !b.is_empty())
            {
                n.description = body;
                n.started_from = Some("from the commit's message".to_owned());
            }
        }
        cx.notify();
    }

    /// The tip of `name` on origin (`origin/name`).
    fn remote_tip(&self, name: &str) -> Option<Oid> {
        self.branches
            .iter()
            .find(|b| {
                b.kind == RefKind::Remote && b.name.split_once('/').is_some_and(|(_, s)| s == name)
            })
            .map(|b| b.tip)
    }

    /// Recomputes what the form says about the pair: commits, an existing request, the owners.
    fn refresh_create(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(n) = self.newreq.as_ref() else {
            return;
        };
        let github = self.web.as_ref().is_some_and(|w| w.github);
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
        let repo = Repo::open(&root).ok();
        let commits = match (&repo, &source, &target) {
            (Some(r), Some(s), Some(t)) => {
                r.range_log(t.1, s.1, true).map(|l| l.len()).unwrap_or(0)
            }
            _ => 0,
        };
        let places = if github {
            codeowners::PLACES_GITHUB
        } else {
            codeowners::PLACES_GITLAB
        };
        let owners = match (&repo, &target) {
            (Some(r), Some(t)) => places
                .iter()
                .find_map(|p| r.file_at(t.1, p))
                .map(|text| codeowners::parse(&text))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        if let Some(n) = self.newreq.as_mut() {
            n.existing = existing;
            n.commits = commits;
            n.owners = owners;
        }
        // The diff the request would hold: under Changes, and the files on the left.
        if let (Some(s), Some(t)) = (source, target) {
            self.run_compare(t, s, true, None, cx);
        }
    }

    /// Code owners of the files the request changes, without the one writing it.
    fn suggested_reviewers(&self) -> Vec<String> {
        let Some(n) = self.newreq.as_ref() else {
            return Vec::new();
        };
        let paths: Vec<String> = self
            .diff
            .as_ref()
            .map(|d| d.files.iter().map(|f| f.path().to_owned()).collect())
            .unwrap_or_default();
        codeowners::suggest(&n.owners, &paths, self.requests.login.as_deref())
    }

    /// The branches Into offers for what is typed: on origin first (the default first), then
    /// those only on this Mac. Each with whether it is only local.
    fn target_choices(&self) -> Vec<(String, bool)> {
        let Some(n) = self.newreq.as_ref() else {
            return Vec::new();
        };
        let typed = n.target.to_lowercase();
        let mut remote: Vec<String> = self
            .branches
            .iter()
            .filter(|b| b.kind == RefKind::Remote)
            .filter_map(|b| b.name.split_once('/').map(|(_, s)| s.to_owned()))
            .filter(|s| s != "HEAD" && *s != n.source)
            .collect();
        remote.sort_by_key(|s| (!["main", "master"].contains(&s.as_str()), s.clone()));
        remote.dedup();
        let local: Vec<String> = self
            .branches
            .iter()
            .filter(|b| b.kind == RefKind::Local && b.name != n.source)
            .filter(|b| !remote.contains(&b.name))
            .map(|b| b.name.clone())
            .collect();
        let mut all: Vec<(String, bool)> = remote.into_iter().map(|s| (s, false)).collect();
        all.extend(local.into_iter().map(|s| (s, true)));
        // An exact match is the choice already made: everything is offered again.
        if all.iter().any(|(s, _)| s.to_lowercase() == typed) {
            return all;
        }
        all.retain(|(s, _)| s.to_lowercase().contains(&typed));
        all
    }

    /// The description has the keyboard: ↑ ↓ move between its lines.
    pub(super) fn typing_description(&self) -> bool {
        self.description_text().is_some()
    }

    /// The description's text while it has the keyboard.
    pub(super) fn description_text(&self) -> Option<String> {
        self.newreq
            .as_ref()
            .filter(|n| n.focus == Focus::Description && !n.changes && !n.preview)
            .map(|n| n.description.clone())
    }

    pub(super) fn close_create(&mut self, cx: &mut Context<Self>) {
        if self.newreq.take().is_some() {
            cx.notify();
        }
    }

    fn pick_target(&mut self, name: String, cx: &mut Context<Self>) {
        if let Some(n) = self.newreq.as_mut() {
            n.target = name;
            n.focus = Focus::Title;
            n.target_at = 0;
            n.error = None;
        }
        self.reset_field_edit();
        self.refresh_create(cx);
        cx.notify();
    }

    pub(super) fn newreq_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let Some(n) = self.newreq.as_mut() else {
            return;
        };
        if n.posting {
            return;
        }
        if key.key == "enter" && key.modifiers.secondary() {
            return self.submit_create(cx);
        }
        // ⌃⇥: Request | Changes, the draft kept.
        if key.key == "tab" && key.modifiers.control {
            n.changes = !n.changes;
            return cx.notify();
        }
        if key.key == "escape" && n.changes {
            n.changes = false;
            return cx.notify();
        }
        if n.changes {
            return;
        }
        if key.key == "d" && key.modifiers.secondary() {
            n.draft = !n.draft;
            return cx.notify();
        }
        if key.key == "p" && key.modifiers.secondary() && key.modifiers.shift {
            n.preview = !n.preview;
            return cx.notify();
        }
        // Into's list: ↑ ↓ walk it, ↵ chooses.
        if n.focus == Focus::Target {
            let choices = self.target_choices();
            let Some(n) = self.newreq.as_mut() else {
                return;
            };
            match key.key.as_str() {
                "down" => {
                    n.target_at = (n.target_at + 1).min(choices.len().saturating_sub(1));
                    return cx.notify();
                }
                "up" => {
                    n.target_at = n.target_at.saturating_sub(1);
                    return cx.notify();
                }
                "enter" => {
                    if let Some((name, _)) = choices.get(n.target_at).cloned() {
                        return self.pick_target(name, cx);
                    }
                }
                _ => {}
            }
        }
        if self.vertical_key(key, cx) {
            return;
        }
        let Some(n) = self.newreq.as_mut() else {
            return;
        };
        if key.key == "tab" {
            n.focus = match (n.focus, key.modifiers.shift) {
                (Focus::Target, false) | (Focus::Description, true) => Focus::Title,
                (Focus::Title, false) | (Focus::Target, true) => Focus::Description,
                (Focus::Description, false) | (Focus::Title, true) => Focus::Target,
            };
            self.reset_field_edit();
            return cx.notify();
        }
        if key.key == "enter" && n.focus == Focus::Description {
            input::sync(&n.description, &mut self.field_edit);
            self.field_edit.insert("\n");
            n.description = self.field_edit.text().to_owned();
            return cx.notify();
        }
        let text = match n.focus {
            Focus::Target => &mut n.target,
            Focus::Title => &mut n.title,
            Focus::Description => &mut n.description,
        };
        let edit = if n.focus == Focus::Description {
            input::edit_area
        } else {
            input::edit
        };
        match edit(text, &mut self.field_edit, key, cx) {
            Edit::Escape => return self.close_create(cx),
            Edit::Enter { .. } => {
                if n.focus == Focus::Title {
                    n.focus = Focus::Description;
                }
            }
            Edit::Changed => {
                n.error = None;
                if n.focus == Focus::Target {
                    n.target_at = 0;
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
        let (source, target, title, description) = (
            n.source.clone(),
            n.target.trim().to_owned(),
            n.title.trim().to_owned(),
            n.description.trim().to_owned(),
        );
        let options = mr::NewOptions {
            draft: n.draft,
            remove_source: n.delete_branch,
            squash: n.squash,
            reviewers: n.reviewers.clone(),
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    mr::create(&remote, &source, &target, &title, &description, &options)
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

    /// Request | Changes is on Changes: the diff of the pair stands in the pane.
    pub(super) fn create_shows_changes(&self) -> bool {
        self.newreq.as_ref().is_some_and(|n| n.changes)
    }

    /// The header row while a request is being written: its name, the pair, totals, Create.
    pub(super) fn render_create_header(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let n = self.newreq.as_ref()?;
        let blocked = n.local_only || n.posting;
        let go = if n.posting {
            "Creating…".to_owned()
        } else if let Some(iid) = n.existing {
            format!("Open {}", super::words::number(iid))
        } else {
            "Create".to_owned()
        };
        Some(
            super::island()
                .flex_none()
                .flex_row()
                .items_center()
                .gap_3()
                .px_4()
                .py(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(super::words::say("New merge request").into_owned()),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_family(theme::CODE_FONT)
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child(format!("{} → {}", mid(&n.source, 52), n.target)),
                        ),
                )
                .children(self.render_create_totals())
                .child(
                    div()
                        .id("create-cancel")
                        .flex_none()
                        .px(px(10.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(ROW_RADIUS))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .child("Cancel")
                        .child(super::keys::caps("esc", super::keys::On::Plain))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_create(cx))),
                )
                .child(
                    div()
                        .id("create-go")
                        .flex_none()
                        .px(px(10.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(ROW_RADIUS))
                        .font_weight(FontWeight::SEMIBOLD)
                        .bg(theme::accent())
                        .text_color(theme::base())
                        .when(blocked, |s| s.opacity(0.45))
                        .when(!blocked, |s| s.cursor_pointer())
                        .child(go)
                        .child(super::keys::caps("⌘↵", super::keys::On::Filled))
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.submit_create(cx)),
                        ),
                )
                .into_any_element(),
        )
    }

    /// "3 commits · 8 files · +46 −12" from the diff of the pair.
    fn render_create_totals(&self) -> Option<gpui::Div> {
        let n = self.newreq.as_ref()?;
        let d = self
            .diff
            .as_ref()
            .filter(|d| matches!(d.header, super::Header::Compare { .. }))?;
        let (a, r) = d
            .files
            .iter()
            .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
        Some(
            div()
                .flex_none()
                .flex()
                .gap_2()
                .text_size(px(12.))
                .text_color(theme::muted())
                .child(format!(
                    "{} · {} ·",
                    plural(n.commits, "commit"),
                    plural(d.files.len(), "file")
                ))
                .child(div().text_color(theme::added()).child(format!("+{a}")))
                .child(div().text_color(theme::removed()).child(format!("−{r}"))),
        )
    }

    /// The Request | Changes switch at the top of the pane.
    pub(super) fn render_create_switch(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let n = self.newreq.as_ref()?;
        let files = self.diff.as_ref().map_or(0, |d| d.files.len());
        let set = |changes: bool| {
            cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                if let Some(n) = this.newreq.as_mut() {
                    n.changes = changes;
                }
                cx.notify();
            })
        };
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_3()
                .px(px(12.))
                .py(px(8.))
                .border_b_1()
                .border_color(theme::island_border())
                .child(
                    super::diff_view::group()
                        .child(chip("create-request", "Request", !n.changes).on_click(set(false)))
                        .child(
                            chip("create-changes", format!("Changes · {files}"), n.changes)
                                .on_click(set(true)),
                        ),
                )
                .children(n.changes.then(|| {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(div().size(px(7.)).rounded_full().bg(theme::warning()))
                        .child("Draft kept")
                }))
                .child(div().flex_1())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(super::keys::caps("⌃⇥", super::keys::On::Plain))
                        .child("switches"),
                )
                .into_any_element(),
        )
    }

    /// The form, in place of the diff.
    pub(super) fn render_create_form(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let n = self.newreq.as_ref()?;
        let github = self.web.as_ref().is_some_and(|w| w.github);
        let label = |text: &'static str| {
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::faint())
                .child(text.to_uppercase())
        };
        let boxed = |id: &'static str, focus: Option<Focus>, body: AnyElement| {
            let on = focus.is_some_and(|f| n.focus == f);
            div()
                .id(id)
                .px_2()
                .py(px(5.))
                .min_w_0()
                .rounded(px(ROW_RADIUS))
                .border_2()
                .border_color(if on {
                    theme::focus()
                } else {
                    theme::island_border()
                })
                .bg(theme::base())
                .when(focus.is_some(), |s| s.cursor_text())
                .child(body)
                .when_some(focus, |s, focus| {
                    s.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if let Some(n) = this.newreq.as_mut()
                            && n.focus != focus
                        {
                            n.focus = focus;
                            n.preview = false;
                            this.reset_field_edit();
                            cx.notify();
                        }
                    }))
                })
        };
        let check = |id: &'static str, on: bool, text: &'static str| {
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
                .child(super::keys::text(text).into_owned())
        };
        let note = if n.local_only {
            Some((
                super::words::say(&format!(
                    "{} is only on this Mac, and GitLab cannot open a request from it. Push it first: git push -u origin {}",
                    n.source, n.source
                ))
                .into_owned(),
                true,
            ))
        } else {
            n.existing.map(|iid| {
                (
                    super::keys::text(&format!(
                        "{} already exists for {} → {}. ⌘↵ opens it.",
                        super::words::number(iid),
                        n.source,
                        n.target
                    ))
                    .into_owned(),
                    false,
                )
            })
        };
        // Into's list, under the field while it has the keyboard.
        let choices = if n.focus == Focus::Target {
            self.target_choices()
        } else {
            Vec::new()
        };
        let list = (!choices.is_empty()).then(|| {
            div()
                .id("create-targets")
                .max_h(px(220.))
                .overflow_y_scroll()
                .p(px(4.))
                .rounded(px(ROW_RADIUS))
                .border_1()
                .border_color(theme::island_border())
                .bg(theme::panel())
                .children(choices.into_iter().enumerate().map(|(i, (name, local))| {
                    let pick = name.clone();
                    let default = ["main", "master"].contains(&name.as_str());
                    div()
                        .id(("create-target", i))
                        .px(px(8.))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(ROW_RADIUS))
                        .cursor_pointer()
                        .when(i == n.target_at, |s| s.bg(theme::selected()))
                        .hover(|s| s.bg(theme::hover()))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(theme::CODE_FONT)
                                .child(mid(&name, 48)),
                        )
                        .children(default.then(|| {
                            div()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child("default")
                        }))
                        .children(local.then(|| {
                            div()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child("not on origin")
                        }))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.pick_target(pick.clone(), cx)
                        }))
                }))
        });
        let suggested = self.suggested_reviewers();
        let mut people: Vec<String> = n.reviewers.clone();
        for s in &suggested {
            if !people.contains(s) {
                people.push(s.clone());
            }
        }
        let reviewers = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(label("Reviewers"))
            .children(people.into_iter().enumerate().map(|(i, who)| {
                let on = n.reviewers.contains(&who);
                let name = who.clone();
                div()
                    .id(("create-reviewer", i))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(26.))
                    .pl(px(4.))
                    .pr(px(8.))
                    .rounded_full()
                    .cursor_pointer()
                    .when(on, |s| s.bg(theme::selected()))
                    .when(!on, |s| s.border_1().border_color(theme::island_border()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(super::discussion::avatar(&who, false, 18.))
                    .child(who)
                    .child(
                        div()
                            .text_color(theme::faint())
                            .child(if on { "×" } else { "+" }),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if let Some(n) = this.newreq.as_mut() {
                            match n.reviewers.iter().position(|r| *r == name) {
                                Some(at) => {
                                    n.reviewers.remove(at);
                                }
                                None => n.reviewers.push(name.clone()),
                            }
                        }
                        cx.notify();
                    }))
            }))
            .child(div().text_size(px(11.)).text_color(theme::faint()).child(
                if suggested.is_empty() {
                    "no code owners for these files"
                } else {
                    "suggested: code owners of the changed files"
                },
            ));
        let description: AnyElement = if n.preview {
            div()
                .min_h(px(200.))
                .child(if n.description.trim().is_empty() {
                    div()
                        .text_color(theme::faint())
                        .child("Nothing to preview")
                        .into_any_element()
                } else {
                    markdown::body(
                        &n.description,
                        true,
                        "create-preview",
                        self.web.as_ref().map(|w| w.base.as_str()),
                        None,
                    )
                })
                .into_any_element()
        } else {
            div()
                .min_h(px(200.))
                .flex()
                .items_start()
                .child(self.field_look(
                    &n.description,
                    n.focus == Focus::Description,
                    "What it changes and why",
                    input::Look {
                        wrap: true,
                        ..Default::default()
                    },
                    cx.entity().downgrade(),
                ))
                .into_any_element()
        };
        let set_preview = |preview: bool| {
            cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                if let Some(n) = this.newreq.as_mut() {
                    n.preview = preview;
                }
                cx.notify();
            })
        };
        Some(
            div()
                .id("create-form")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(px(16.))
                .py(px(14.))
                .flex()
                .flex_col()
                .gap_3()
                .max_w(px(920.))
                .text_size(px(13.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div().flex_1().min_w_0().child(boxed(
                                "create-source",
                                None,
                                div()
                                    .truncate()
                                    .font_family(theme::CODE_FONT)
                                    .child(mid(&n.source, 36))
                                    .into_any_element(),
                            )),
                        )
                        .child(div().text_color(theme::faint()).child("→"))
                        .child(
                            div().flex_1().min_w_0().child(boxed(
                                "create-target-field",
                                Some(Focus::Target),
                                div()
                                    .font_family(theme::CODE_FONT)
                                    .child(self.field_text(
                                        &n.target,
                                        n.focus == Focus::Target,
                                        "main",
                                        cx,
                                    ))
                                    .into_any_element(),
                            )),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(if n.commits > 0 {
                                    theme::added()
                                } else {
                                    theme::muted()
                                })
                                .child(if n.commits == 0 {
                                    "nothing ahead of the target".to_owned()
                                } else {
                                    format!("{} ahead", plural(n.commits, "commit"))
                                }),
                        ),
                )
                .children(list)
                .children(note.map(|(text, warn)| {
                    div()
                        .px_2()
                        .py(px(5.))
                        .rounded(px(ROW_RADIUS))
                        .border_1()
                        .border_color(if warn {
                            theme::warning()
                        } else {
                            theme::island_border()
                        })
                        .text_size(px(12.))
                        .text_color(if warn {
                            theme::warning()
                        } else {
                            theme::muted()
                        })
                        .child(text)
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(label("Title"))
                        .child(boxed(
                            "create-title",
                            Some(Focus::Title),
                            self.field_look(
                                &n.title,
                                n.focus == Focus::Title,
                                "Title",
                                input::Look {
                                    wrap: true,
                                    ..Default::default()
                                },
                                cx.entity().downgrade(),
                            )
                            .into_any_element(),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().flex_1().child(label("Description")))
                                .child(
                                    super::diff_view::group()
                                        .child(
                                            chip("create-write", "Write", !n.preview)
                                                .on_click(set_preview(false)),
                                        )
                                        .child(
                                            chip("create-preview", "Preview", n.preview)
                                                .on_click(set_preview(true)),
                                        ),
                                )
                                .children(n.started_from.clone().map(|from| {
                                    div()
                                        .px(px(8.))
                                        .h(px(22.))
                                        .flex()
                                        .items_center()
                                        .rounded_full()
                                        .bg(theme::info_bg())
                                        .text_size(px(11.))
                                        .text_color(theme::muted())
                                        .child(from)
                                })),
                        )
                        .child(boxed(
                            "create-description",
                            Some(Focus::Description),
                            description,
                        )),
                )
                .child(reviewers)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_x(px(20.))
                        .gap_y(px(8.))
                        .text_color(theme::muted())
                        .child(
                            check("create-draft", n.draft, "Draft  ⌘D").on_click(cx.listener(
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
                                "Delete the branch after merge",
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
                        // GitHub squashes when merging, not when opening.
                        .children((!github).then(|| {
                            check("create-squash", n.squash, "Squash").on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    if let Some(n) = this.newreq.as_mut() {
                                        n.squash = !n.squash;
                                    }
                                    cx.notify();
                                },
                            ))
                        })),
                )
                .children(n.error.clone().map(|e| {
                    div()
                        .text_size(px(12.))
                        .text_color(theme::removed())
                        .child(e)
                }))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_branch_names_keep_both_ends() {
        assert_eq!(mid("main", 10), "main");
        let cut = mid("feature/login-state-sealed-interface-and-error-text", 20);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.starts_with("feature/l") && cut.ends_with("-text") && cut.contains('…'));
    }

    #[test]
    fn the_template_comes_from_the_hosts_place() {
        let files = |dir: &str| match dir {
            ".gitlab/merge_request_templates" => vec!["Bug.md".to_owned(), "default.md".to_owned()],
            "docs" => vec!["PULL_REQUEST_TEMPLATE.md".to_owned()],
            _ => Vec::new(),
        };
        let file = |path: &str| Some(format!("text of {path}"));
        assert_eq!(
            template(false, file, files),
            Some((
                "default.md template".to_owned(),
                "text of .gitlab/merge_request_templates/default.md".to_owned()
            ))
        );
        assert_eq!(
            template(true, file, files),
            Some((
                "PULL_REQUEST_TEMPLATE.md".to_owned(),
                "text of docs/PULL_REQUEST_TEMPLATE.md".to_owned()
            ))
        );
        assert_eq!(template(false, file, |_| Vec::new()), None);
    }
}
