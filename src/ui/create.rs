//! Creating a merge request from a branch (Design #47 E, `docs/specs/new-request.md`, flow 08): the
//! request is written in place of the diff while the branch's commits and changes stay on the left.
//! From → Into, the target picked from the branch list; the description starts from the project's
//! template on the target, else the project's template setting, else a single commit's body, and
//! Templates ▾ offers the others; whether the pair merges cleanly is worked out in memory; the code
//! owners of the changed files start as the reviewers; labels and a milestone are picked from the
//! project's lists; Request | Changes (⌃⇥) shows the diff and keeps the draft. From ⌘K it opens
//! the quick way (D): the same request asked one question at a time over the branch view — Into,
//! Title, Description — ↵ next, ⇧⇥ back, ⇥ the whole form, ⌘↵ create. The one outward
//! action besides posting comments; it never pushes, so a branch only on this Mac is told to be
//! pushed first.

use super::input::{self, Edit};
use super::{ROW_RADIUS, Workspace, chip, markdown, plural, px, theme};
use crate::codeowners;
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

/// A list open under its field.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    Templates,
    Labels,
    Milestone,
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
    /// The description as the form filled it: a template read later replaces it only while it
    /// is unchanged.
    started: String,
    /// Every template on offer (name, text), the default first.
    templates: Vec<(String, String)>,
    /// The labels chosen, and the milestone (its id).
    labels: Vec<String>,
    milestone: Option<u64>,
    /// The project's labels and milestones, read as the form opens.
    all_labels: Vec<mr::Label>,
    milestones: Vec<mr::Milestone>,
    menu: Option<Menu>,
    /// The files merging into the target would leave in conflict; `None` while not known.
    conflicts: Option<Vec<String>>,
    /// Code owners of the changed files; they start as the reviewers.
    suggested: Vec<String>,
    /// The reviewers were changed by hand: another target no longer replaces them.
    reviewers_touched: bool,
    draft: bool,
    delete_branch: bool,
    squash: bool,
    focus: Focus,
    /// The target list's cursor while Into has the keyboard.
    target_at: usize,
    /// The request that already exists for this pair.
    existing: Option<u64>,
    commits: usize,
    /// The reviewers chosen.
    reviewers: Vec<String>,
    /// The description as it will read, instead of its text.
    preview: bool,
    /// Request | Changes: the diff of the pair is shown, the draft kept.
    changes: bool,
    /// Asked one question at a time over the branch view (⌘K, D) instead of in the diff's place.
    quick: bool,
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

/// The templates a new request can start from, read from the target with `file`/`files`, the
/// default first: GitLab's `.gitlab/merge_request_templates/*.md` (`Default.md` in any case is the
/// default); GitHub's `pull_request_template.md` in `.github/`, `docs/` or the root, then the files
/// of `.github/PULL_REQUEST_TEMPLATE/`. Each with its name and text.
pub fn templates(
    github: bool,
    file: impl Fn(&str) -> Option<String>,
    files: impl Fn(&str) -> Vec<String>,
) -> Vec<(String, String)> {
    let read = |dir: &str, name: String| {
        let path = if dir.is_empty() {
            name.clone()
        } else {
            format!("{dir}/{name}")
        };
        file(&path).map(|t| (name, t))
    };
    let markdown = |n: &String| n.to_lowercase().ends_with(".md");
    let mut out = Vec::new();
    if github {
        if let Some(found) = [".github", "docs", ""].into_iter().find_map(|dir| {
            files(dir)
                .into_iter()
                .find(|n| n.eq_ignore_ascii_case("pull_request_template.md"))
                .and_then(|name| read(dir, name))
        }) {
            out.push(found);
        }
        let dir = ".github/PULL_REQUEST_TEMPLATE";
        let mut names: Vec<String> = files(dir).into_iter().filter(markdown).collect();
        names.sort_by_key(|n| n.to_lowercase());
        out.extend(names.into_iter().filter_map(|n| read(dir, n)));
        return out;
    }
    let dir = ".gitlab/merge_request_templates";
    let mut names: Vec<String> = files(dir).into_iter().filter(markdown).collect();
    names.sort_by_key(|n| (!n.eq_ignore_ascii_case("default.md"), n.to_lowercase()));
    out.extend(names.into_iter().filter_map(|n| read(dir, n)));
    out
}

/// A template's name as the chip says it: `Default.md` without its extension.
fn template_name(name: &str) -> &str {
    name.strip_suffix(".md")
        .or_else(|| name.strip_suffix(".MD"))
        .unwrap_or(name)
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
            started: String::new(),
            templates: Vec::new(),
            labels: Vec::new(),
            milestone: None,
            all_labels: Vec::new(),
            milestones: Vec::new(),
            menu: None,
            conflicts: None,
            suggested: Vec::new(),
            reviewers_touched: false,
            draft: false,
            delete_branch: true,
            squash: false,
            focus: Focus::Title,
            target_at: 0,
            existing: None,
            commits: 0,
            reviewers: Vec::new(),
            preview: false,
            changes: false,
            quick: false,
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
            n.templates = match (&repo, target_tip) {
                (Some(r), Some(t)) => {
                    templates(web.github, |p| r.file_at(t, p), |d| r.files_at(t, d))
                }
                _ => Vec::new(),
            };
            if let Some((name, text)) = n.templates.first() {
                n.description = text.clone();
                n.started_from = Some(format!("from {} template", template_name(name)));
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
            n.started = n.description.clone();
        }
        self.read_create_lists(web, cx);
        cx.notify();
    }

    /// The project's labels, milestones and template setting, in the background. The template
    /// setting comes after a template file and before a commit's message.
    fn read_create_lists(&mut self, web: crate::git::WebRemote, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let (labels, milestones, template) = cx
                .background_executor()
                .spawn(async move {
                    (
                        mr::labels(&web).unwrap_or_default(),
                        mr::milestones(&web).unwrap_or_default(),
                        mr::project_template(&web).ok().flatten(),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                let Some(n) = this.newreq.as_mut() else {
                    return;
                };
                n.all_labels = labels;
                n.milestones = milestones;
                if let Some(text) = template {
                    let had_file = !n.templates.is_empty();
                    n.templates
                        .push(("Project settings".to_owned(), text.clone()));
                    if !had_file && n.description == n.started {
                        n.description = text;
                        n.started = n.description.clone();
                        n.started_from = Some("from the project's template".to_owned());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Templates ▾: the description becomes template `ix`.
    fn use_template(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(n) = self.newreq.as_mut()
            && let Some((name, text)) = n.templates.get(ix).cloned()
        {
            n.description = text;
            n.started = n.description.clone();
            n.started_from = Some(if name == "Project settings" {
                "from the project's template".to_owned()
            } else {
                format!("from {} template", template_name(&name))
            });
            n.menu = None;
            self.reset_field_edit();
        }
        cx.notify();
    }

    /// A snapshot's way in: `create-templates`, `create-labels`, `create-milestone`.
    #[cfg(feature = "snapshot")]
    pub(super) fn open_create_menu(&mut self, name: &str, cx: &mut Context<Self>) {
        let menu = match name {
            "create-templates" => Menu::Templates,
            "create-labels" => Menu::Labels,
            "create-milestone" => Menu::Milestone,
            _ => return,
        };
        self.toggle_create_menu(menu, cx);
    }

    fn toggle_create_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        if let Some(n) = self.newreq.as_mut() {
            n.menu = (n.menu != Some(menu)).then_some(menu);
        }
        cx.notify();
    }

    /// ⌘K → Create merge request: the quick way, starting at Into.
    pub(super) fn open_create_quick(&mut self, cx: &mut Context<Self>) {
        self.open_create(None, cx);
        if let Some(n) = self.newreq.as_mut() {
            n.quick = true;
            n.focus = Focus::Target;
            self.reset_field_edit();
        }
        cx.notify();
    }

    /// The request is written in the diff's place (not asked over it, D).
    pub(super) fn create_in_place(&self) -> bool {
        self.newreq.as_ref().is_some_and(|n| !n.quick)
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
        let log = match (&repo, &source, &target) {
            (Some(r), Some(s), Some(t)) => r.range_log(t.1, s.1, true).unwrap_or_default(),
            _ => Vec::new(),
        };
        let commits = log.len();
        let conflicts = match (&repo, &source, &target) {
            (Some(r), Some(s), Some(t)) => r.conflicts(t.1, s.1),
            _ => None,
        };
        // The files the request changes, commit by commit: their code owners review it.
        let ids: Vec<Oid> = log.iter().map(|c| c.id).collect();
        let mut paths: Vec<String> = repo
            .as_ref()
            .map(|r| r.changed_paths(&ids))
            .unwrap_or_default()
            .into_iter()
            .flat_map(|(_, p)| p)
            .collect();
        paths.sort_unstable();
        paths.dedup();
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
        let suggested = codeowners::suggest(&owners, &paths, self.requests.login.as_deref());
        if let Some(n) = self.newreq.as_mut() {
            n.existing = existing;
            n.commits = commits;
            n.conflicts = conflicts;
            if !n.reviewers_touched {
                n.reviewers = suggested.clone();
            }
            n.suggested = suggested;
        }
        // The diff the request would hold: under Changes, and the files on the left.
        if let (Some(s), Some(t)) = (source, target) {
            self.run_compare(t, s, true, None, cx);
        }
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
        // `@` and `:` complete in the description (#51, #9).
        if n.focus == Focus::Description && !n.changes && !n.preview {
            let mut text = n.description.clone();
            if self.field_completion_key(&mut text, key) {
                if let Some(n) = self.newreq.as_mut() {
                    n.description = text;
                }
                return cx.notify();
            }
        }
        let Some(n) = self.newreq.as_mut() else {
            return;
        };
        // The quick way: ⇥ opens the whole form where it stands, ⇧⇥ goes a question back.
        if n.quick && key.key == "tab" {
            if key.modifiers.shift {
                n.focus = match n.focus {
                    Focus::Description => Focus::Title,
                    _ => Focus::Target,
                };
            } else {
                n.quick = false;
            }
            self.reset_field_edit();
            return cx.notify();
        }
        // ⌃⇥: Request | Changes, the draft kept.
        if key.key == "tab" && key.modifiers.control && !n.quick {
            n.changes = !n.changes;
            return cx.notify();
        }
        if key.key == "escape" && n.changes {
            n.changes = false;
            return cx.notify();
        }
        if key.key == "escape" && n.menu.is_some() {
            n.menu = None;
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
            labels: n.labels.clone(),
            milestone: n.milestone,
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
        let n = self.newreq.as_ref().filter(|n| !n.quick)?;
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
        let n = self.newreq.as_ref().filter(|n| !n.quick)?;
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

    /// Beside From → Into: whether it merges cleanly, and how many commits are ahead.
    fn render_merge_state(&self, n: &NewRequest) -> gpui::Div {
        let ahead = if n.commits == 0 {
            "nothing ahead of the target".to_owned()
        } else {
            format!("{} ahead", plural(n.commits, "commit"))
        };
        let (text, color) = match &n.conflicts {
            _ if n.commits == 0 => (ahead, theme::muted()),
            Some(files) if files.is_empty() => {
                (format!("✓ merges cleanly · {ahead}"), theme::added())
            }
            Some(files) => (
                format!(
                    "⚠ {} · {ahead}",
                    match files.as_slice() {
                        [one] => format!("conflicts in {}", one.rsplit('/').next().unwrap_or(one)),
                        _ => format!("conflicts in {}", plural(files.len(), "file")),
                    }
                ),
                theme::warning(),
            ),
            None => (ahead, theme::muted()),
        };
        div()
            .flex_none()
            .text_size(px(12.))
            .text_color(color)
            .child(text)
    }

    /// A list under a field: rows of `(label, chosen, swatch)`, each a click.
    fn create_menu(
        &self,
        id: &'static str,
        rows: Vec<(String, bool, Option<u32>, Option<String>)>,
        pick: impl Fn(&mut Self, usize, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .max_h(px(240.))
            .overflow_y_scroll()
            .p(px(4.))
            .rounded(px(ROW_RADIUS))
            .border_1()
            .border_color(theme::island_border())
            .bg(theme::panel())
            .children(
                rows.into_iter()
                    .enumerate()
                    .map(|(i, (label, on, swatch, end))| {
                        let pick = pick.clone();
                        div()
                            .id((id, i))
                            .px(px(8.))
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(ROW_RADIUS))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .child(
                                div()
                                    .w(px(12.))
                                    .flex_none()
                                    .text_color(theme::accent())
                                    .child(if on { "✓" } else { "" }),
                            )
                            .children(swatch.map(|c| {
                                div()
                                    .size(px(10.))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(gpui::rgb(c))
                            }))
                            .child(div().flex_1().min_w_0().truncate().child(label))
                            .children(end.map(|e| {
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .child(e)
                            }))
                            .on_click(
                                cx.listener(move |this, _: &ClickEvent, _, cx| pick(this, i, cx)),
                            )
                    }),
            )
    }

    /// Templates ▾ open: every template; the one the description started from is ticked.
    fn render_template_menu(&self, n: &NewRequest, cx: &mut Context<Self>) -> AnyElement {
        let rows = n
            .templates
            .iter()
            .map(|(name, text)| {
                (
                    template_name(name).to_owned(),
                    *text == n.started,
                    None,
                    None,
                )
            })
            .collect();
        self.create_menu(
            "create-template-menu",
            rows,
            |this, i, cx| this.use_template(i, cx),
            cx,
        )
        .into_any_element()
    }

    /// Labels and Milestone side by side, each opening the project's list under it.
    fn render_labels_row(&self, n: &NewRequest, cx: &mut Context<Self>) -> AnyElement {
        let menu = match n.menu {
            Some(Menu::Labels) if !n.all_labels.is_empty() => Some(
                self.create_menu(
                    "create-label-menu",
                    n.all_labels
                        .iter()
                        .map(|l| (l.name.clone(), n.labels.contains(&l.name), l.color, None))
                        .collect(),
                    |this, i, cx| {
                        if let Some(n) = this.newreq.as_mut()
                            && let Some(name) = n.all_labels.get(i).map(|l| l.name.clone())
                        {
                            match n.labels.iter().position(|l| *l == name) {
                                Some(at) => {
                                    n.labels.remove(at);
                                }
                                None => n.labels.push(name),
                            }
                        }
                        cx.notify();
                    },
                    cx,
                )
                .into_any_element(),
            ),
            Some(Menu::Milestone) if !n.milestones.is_empty() => Some(
                self.create_menu(
                    "create-milestone-menu",
                    std::iter::once(("No milestone".to_owned(), n.milestone.is_none(), None, None))
                        .chain(n.milestones.iter().map(|m| {
                            (
                                m.title.clone(),
                                n.milestone == Some(m.id),
                                None,
                                m.due.clone().map(|d| format!("due {d}")),
                            )
                        }))
                        .collect(),
                    |this, i, cx| {
                        if let Some(n) = this.newreq.as_mut() {
                            n.milestone = i
                                .checked_sub(1)
                                .and_then(|i| n.milestones.get(i))
                                .map(|m| m.id);
                            n.menu = None;
                        }
                        cx.notify();
                    },
                    cx,
                )
                .into_any_element(),
            ),
            _ => None,
        };
        let field =
            |id: &'static str, menu: Menu, body: AnyElement| {
                let on = n.menu == Some(menu);
                div()
                    .id(id)
                    .flex_1()
                    .min_w_0()
                    .h(px(32.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(ROW_RADIUS))
                    .border_2()
                    .border_color(if on {
                        theme::focus()
                    } else {
                        theme::island_border()
                    })
                    .bg(theme::base())
                    .cursor_pointer()
                    .child(div().flex_1().min_w_0().overflow_hidden().child(body))
                    .child(div().text_color(theme::faint()).child("▾"))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.toggle_create_menu(menu, cx)
                    }))
            };
        let faint = |text: &str| {
            div()
                .text_color(theme::faint())
                .child(text.to_owned())
                .into_any_element()
        };
        let labels = if n.labels.is_empty() {
            faint(if n.all_labels.is_empty() {
                "No labels in this project"
            } else {
                "None"
            })
        } else {
            div()
                .flex()
                .gap(px(6.))
                .children(n.labels.iter().map(|name| {
                    let color = n
                        .all_labels
                        .iter()
                        .find(|l| l.name == *name)
                        .and_then(|l| l.color);
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .px(px(7.))
                        .h(px(20.))
                        .rounded_full()
                        .bg(theme::selected())
                        .text_size(px(12.))
                        .children(color.map(|c| div().size(px(8.)).rounded_full().bg(gpui::rgb(c))))
                        .child(name.clone())
                }))
                .into_any_element()
        };
        let milestone = match n
            .milestone
            .and_then(|id| n.milestones.iter().find(|m| m.id == id))
        {
            Some(m) => div().truncate().child(m.title.clone()).into_any_element(),
            None => faint(if n.milestones.is_empty() {
                "No open milestones"
            } else {
                "None"
            }),
        };
        let label = |text: &'static str| {
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::faint())
                .child(text.to_uppercase())
        };
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(label("Labels"))
                            .child(field("create-labels", Menu::Labels, labels)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(label("Milestone"))
                            .child(field("create-milestone", Menu::Milestone, milestone)),
                    ),
            )
            .children(menu)
            .into_any_element()
    }

    /// The form, in place of the diff.
    pub(super) fn render_create_form(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let n = self.newreq.as_ref()?;
        let github = self.web.as_ref().is_some_and(|w| w.github);
        // Built first: they need the context to themselves.
        let template_menu =
            (n.menu == Some(Menu::Templates)).then(|| self.render_template_menu(n, cx));
        let labels_row = self.render_labels_row(n, cx);
        let completion = (n.focus == Focus::Description && !n.preview)
            .then(|| self.field_completion(&n.description))
            .flatten()
            .map(|(_, choices)| self.render_completion(&choices, self.field_pick));
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
        let suggested = n.suggested.clone();
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
                            n.reviewers_touched = true;
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
                        .child(self.render_merge_state(n)),
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
                                }))
                                .children((n.templates.len() > 1).then(|| {
                                    chip(
                                        "create-templates",
                                        "Templates ▾",
                                        n.menu == Some(Menu::Templates),
                                    )
                                    .on_click(cx.listener(
                                        |this, _: &ClickEvent, _, cx| {
                                            this.toggle_create_menu(Menu::Templates, cx)
                                        },
                                    ))
                                })),
                        )
                        .children(template_menu)
                        .child(boxed(
                            "create-description",
                            Some(Focus::Description),
                            description,
                        ))
                        .children(completion),
                )
                .child(reviewers)
                .child(labels_row)
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

    /// The quick way (D): a card over the branch view asking one thing at a time.
    pub(super) fn render_create_quick(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let n = self.newreq.as_ref().filter(|n| n.quick)?;
        let steps = ["Into", "Title", "Description", "Create"];
        let at = match n.focus {
            Focus::Target => 0,
            Focus::Title => 1,
            Focus::Description => 2,
        };
        let at = if n.posting { 3 } else { at };
        let pills = div().flex().gap(px(4.)).px(px(14.)).pt(px(10.)).children(
            steps.iter().enumerate().map(|(i, step)| {
                div()
                    .h(px(22.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .text_size(px(11.))
                    .when(i == at, |s| {
                        s.bg(theme::selected()).text_color(theme::text())
                    })
                    .when(i < at, |s| s.text_color(theme::muted()))
                    .when(i > at, |s| s.text_color(theme::faint()))
                    .child(if i < at {
                        format!("✓ {step}")
                    } else {
                        format!("{} {step}", i + 1)
                    })
            }),
        );
        let (field, ask): (AnyElement, &str) = match n.focus {
            Focus::Target => (
                div()
                    .font_family(theme::CODE_FONT)
                    .child(self.field_text(&n.target, true, "main", cx))
                    .into_any_element(),
                "Into which branch?",
            ),
            Focus::Title => (
                self.field_look(
                    &n.title,
                    true,
                    "Title",
                    input::Look {
                        wrap: true,
                        ..Default::default()
                    },
                    cx.entity().downgrade(),
                )
                .into_any_element(),
                "Title",
            ),
            Focus::Description => (
                div()
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .child(n.title.clone())
                    .into_any_element(),
                "",
            ),
        };
        let question = div()
            .flex()
            .items_center()
            .gap_3()
            .px(px(14.))
            .py(px(12.))
            .border_b_1()
            .border_color(theme::island_border())
            .text_size(px(15.))
            .child(
                div()
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(format!("{} ›", super::words::say("New merge request"))),
            )
            .child(div().flex_1().min_w_0().child(field))
            .child(
                div()
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(theme::faint())
                    .child(ask),
            );
        let body: Option<AnyElement> = match n.focus {
            Focus::Target => {
                let choices = self.target_choices();
                Some(
                    div()
                        .id("quick-targets")
                        .max_h(px(260.))
                        .overflow_y_scroll()
                        .p(px(4.))
                        .children(choices.into_iter().enumerate().map(|(i, (name, local))| {
                            let pick = name.clone();
                            let default = ["main", "master"].contains(&name.as_str());
                            div()
                                .id(("quick-target", i))
                                .px(px(10.))
                                .h(px(28.))
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
                                        .child(mid(&name, 56)),
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
                        .into_any_element(),
                )
            }
            Focus::Title => None,
            Focus::Description => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .px(px(14.))
                    .py(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(theme::CODE_FONT)
                                    .child(format!("{} → {}", mid(&n.source, 44), n.target)),
                            )
                            .children(self.render_create_totals()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.))
                                    .text_color(theme::muted())
                                    .child("Description"),
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
                    .child(
                        div()
                            .id("quick-description")
                            .min_h(px(150.))
                            .max_h(px(260.))
                            .overflow_y_scroll()
                            .px_2()
                            .py(px(5.))
                            .rounded(px(ROW_RADIUS))
                            .border_2()
                            .border_color(theme::focus())
                            .bg(theme::base())
                            .child(self.field_look(
                                &n.description,
                                true,
                                "What it changes and why",
                                input::Look {
                                    wrap: true,
                                    ..Default::default()
                                },
                                cx.entity().downgrade(),
                            )),
                    )
                    .children(
                        self.field_completion(&n.description)
                            .map(|(_, choices)| self.render_completion(&choices, self.field_pick)),
                    )
                    .into_any_element(),
            ),
        };
        // What stops it, or what went wrong.
        let note = if n.local_only {
            Some(
                super::words::say(&format!(
                    "{} is only on this Mac. Push it first: git push -u origin {}",
                    n.source, n.source
                ))
                .into_owned(),
            )
        } else if let Some(iid) = n.existing {
            Some(format!(
                "{} already exists for this pair. ⌘↵ opens it.",
                super::words::number(iid)
            ))
        } else {
            n.error.clone()
        };
        let hint = |keys: &'static str, what: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(super::keys::caps(keys, super::keys::On::Plain))
                .child(what)
        };
        let foot = div()
            .flex()
            .items_center()
            .gap_4()
            .px(px(14.))
            .py(px(8.))
            .border_t_1()
            .border_color(theme::island_border())
            .text_size(px(12.))
            .text_color(theme::muted())
            .child(div().flex_1().min_w_0().truncate().child(match n.focus {
                Focus::Target => format!("From {}", mid(&n.source, 34)),
                _ => "⇥ reviewers, draft, labels…".to_owned(),
            }))
            .children((n.focus != Focus::Target).then(|| hint("⇧⇥", "back")))
            .children((n.focus != Focus::Description).then(|| hint("↵", "next")))
            .child(hint("⌘↵", "create"))
            .child(hint("esc", "cancel"));
        Some(
            div()
                .id("quick-create-backdrop")
                .absolute()
                .size_full()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(72.))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_create(cx)),
                )
                .child(
                    div()
                        .occlude()
                        .w(px(680.))
                        .flex()
                        .flex_col()
                        .rounded(px(super::ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .overflow_hidden()
                        .text_size(px(13.))
                        .child(pills)
                        .child(question)
                        .children(body)
                        .children(note.map(|text| {
                            div()
                                .px(px(14.))
                                .py(px(8.))
                                .text_size(px(12.))
                                .text_color(theme::warning())
                                .child(text)
                        }))
                        .child(foot),
                )
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
    fn the_templates_come_from_the_hosts_place_default_first() {
        let files = |dir: &str| match dir {
            ".gitlab/merge_request_templates" => vec![
                "Bug.md".to_owned(),
                "default.md".to_owned(),
                "notes.txt".to_owned(),
            ],
            "docs" => vec!["PULL_REQUEST_TEMPLATE.md".to_owned()],
            ".github/PULL_REQUEST_TEMPLATE" => vec!["release.md".to_owned()],
            _ => Vec::new(),
        };
        let file = |path: &str| Some(format!("text of {path}"));
        let names = |list: Vec<(String, String)>| -> Vec<String> {
            list.into_iter().map(|(n, _)| n).collect()
        };
        assert_eq!(
            names(templates(false, file, files)),
            ["default.md", "Bug.md"]
        );
        assert_eq!(
            templates(false, file, files)[0].1,
            "text of .gitlab/merge_request_templates/default.md"
        );
        assert_eq!(
            names(templates(true, file, files)),
            ["PULL_REQUEST_TEMPLATE.md", "release.md"]
        );
        assert!(templates(false, file, |_| Vec::new()).is_empty());
        assert_eq!(template_name("Default.md"), "Default");
    }
}
