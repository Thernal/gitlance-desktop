//! The Code space (#75): the project's tree beside its page — the folder page with its README
//! rendered, or files as the editor's tabs — read at a revision: the working tree by default,
//! HEAD or a branch picked on the tree's chip. A plain folder, not a git repository, has only
//! this space, its files as they are on disk.

use super::menu::{Act, Entry};
use super::nav::Page;
use super::spaces::Space;
use super::{Workspace, icons, island, keys, markdown, px, theme};
use crate::files;
use crate::git::Repo;
use git2::Oid;
use gpui::{
    AnyElement, ClickEvent, Context, Div, MouseButton, MouseDownEvent, SharedString, Task, div,
    prelude::*, uniform_list,
};
use std::collections::HashSet;

/// The most files Code's tree lists.
const LIMIT: usize = 20_000;

#[derive(Default)]
pub struct Code {
    /// The revision read, by name: None the working tree, the files on disk.
    pub rev: Option<(String, Oid)>,
    /// The project's files at it, sorted.
    files: Vec<String>,
    /// What `files` were read at: the revision, None the working tree.
    read_at: Option<Option<Oid>>,
    task: Option<Task<()>>,
    /// The folders open in the tree.
    open: HashSet<String>,
    /// The files kept open as tabs, in order; `preview` is the one the next click replaces.
    pub tabs: Vec<String>,
    pub preview: Option<String>,
    /// The root's README at the revision: its path and text.
    readme: Option<(String, String)>,
    scroll: gpui::UniformListScrollHandle,
}

impl Code {
    /// The project's files as the tree lists them.
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// The folders above `path` open, so the tree shows it.
    pub fn reveal(&mut self, path: &str) {
        let mut at = 0;
        while let Some(slash) = path[at..].find('/') {
            self.open.insert(path[..at + slash].to_owned());
            at += slash + 1;
        }
    }

    /// The revision as a commit; None the working tree.
    pub fn at(&self) -> Option<Oid> {
        self.rev.as_ref().map(|(_, id)| *id)
    }

    /// The tabs in their order, the preview one where it was opened.
    fn shown_tabs(&self) -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool)> = self.tabs.iter().map(|t| (t.clone(), false)).collect();
        if let Some(p) = &self.preview {
            out.push((p.clone(), true));
        }
        out
    }
}

impl Workspace {
    /// Reads the project's files at Code's revision, once per revision (and again with `force`).
    pub(super) fn read_code_files(&mut self, force: bool, cx: &mut Context<Self>) {
        let at = self.code.at();
        if !force && self.code.read_at == Some(at) {
            return;
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let plain = self.plain;
        self.code.read_at = Some(at);
        self.code.task = Some(cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    let repo = (!plain).then(|| Repo::open(&root).ok()).flatten();
                    let files = match (&repo, at) {
                        (Some(repo), Some(rev)) => repo.files_of(rev, LIMIT),
                        (Some(repo), None) => repo.working_files(LIMIT),
                        (None, _) => files::on_disk(&root, LIMIT),
                    };
                    let readme = files::readme(&files, "").cloned().and_then(|path| {
                        let text = match (&repo, at) {
                            (Some(repo), Some(rev)) => repo.file_at(rev, &path),
                            _ => std::fs::read_to_string(root.join(&path)).ok(),
                        }?;
                        Some((path, text))
                    });
                    (files, readme)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.code.read_at == Some(at) {
                    (this.code.files, this.code.readme) = read;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Reads Code at `rev` (None: the working tree); the file open is read again there.
    fn read_code_at(&mut self, rev: Option<(String, Oid)>, cx: &mut Context<Self>) {
        self.code.rev = rev;
        self.read_code_files(false, cx);
        if let Some(Page::File(path)) = self.nav_page().cloned() {
            self.open_file_view(path, cx);
        }
        cx.notify();
    }

    /// File `path` opens in Code: as the preview tab (`preview`) the next click replaces, or kept.
    pub(super) fn open_code_file(&mut self, path: String, preview: bool, cx: &mut Context<Self>) {
        let code = &mut self.code;
        if !code.tabs.contains(&path) {
            if preview {
                code.preview = Some(path.clone());
            } else {
                if code.preview.as_ref() == Some(&path) {
                    code.preview = None;
                }
                code.tabs.push(path.clone());
            }
        }
        self.open_file_view(path, cx);
    }

    /// ⌘W in Code: the file on screen closes, its neighbour shows, else the folder page.
    pub(super) fn close_code_tab(&mut self, path: &str, cx: &mut Context<Self>) {
        let shown = self.code.shown_tabs();
        let at = shown.iter().position(|(p, _)| p == path);
        self.code.tabs.retain(|t| t != path);
        if self.code.preview.as_deref() == Some(path) {
            self.code.preview = None;
        }
        let on_screen = matches!(self.nav_page(), Some(Page::File(p)) if p == path);
        if !on_screen {
            return cx.notify();
        }
        let rest = self.code.shown_tabs();
        let next = at.and_then(|at| rest.get(at.min(rest.len().saturating_sub(1))).cloned());
        match next {
            Some((next, _)) => self.open_file_view(next, cx),
            None => {
                self.file_view = None;
                self.show(Space::Code, None, cx);
            }
        }
    }

    /// ⌃⇥ and ⌃⇧⇥ in Code: the next or the previous tab, going round.
    pub(super) fn step_code_tab(&mut self, forward: bool, cx: &mut Context<Self>) {
        let shown = self.code.shown_tabs();
        if shown.is_empty() {
            return;
        }
        let n = shown.len();
        let at = match self.nav_page() {
            Some(Page::File(path)) => shown.iter().position(|(p, _)| p == path),
            _ => None,
        };
        let to = match at {
            Some(at) => (at + if forward { 1 } else { n - 1 }) % n,
            None => 0,
        };
        let path = shown[to].0.clone();
        self.open_file_view(path, cx);
    }

    /// Code: the tree beside the folder page or the file's tabs, the tool window after them.
    pub(super) fn render_code_space(&self, cx: &mut Context<Self>) -> Div {
        let page =
            match self.nav_page() {
                Some(Page::File(_)) => div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(self.render_code_tabs(cx))
                    .child(self.render_file_view(cx).unwrap_or_else(|| {
                        island().flex_1().bg(theme::editor()).into_any_element()
                    }))
                    .into_any_element(),
                _ => self.render_folder_page(cx),
            };
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(super::gap()))
            // It slides and fades in and out with ⌘J, as the side panel does.
            .children(
                (!self.compact() && (self.layout.show_sidebar || self.motion.recent("side"))).then(
                    || {
                        let room = self.layout.sidebar + super::gap();
                        super::motion::toward(
                            div()
                                .flex()
                                .flex_none()
                                .overflow_hidden()
                                .child(self.render_code_tree(cx)),
                            "code-tree-pane",
                            self.layout.show_sidebar,
                            self.motion.recent("side"),
                            super::motion::BASE,
                            move |pane, v| pane.max_w(px(room * v)).opacity(v),
                        )
                    },
                ),
            )
            .child(page)
            .children(self.render_tool(cx))
            // Narrow, the tree opens over the page (#80).
            .children(
                self.side_drawer()
                    .then(|| self.drawer(self.render_code_tree(cx))),
            )
    }

    /// The project's tree: its name and the revision chip, Go to file, the folders and files.
    fn render_code_tree(&self, cx: &mut Context<Self>) -> Div {
        let project = self
            .root
            .as_ref()
            .and_then(|r| r.file_name())
            .map(|n| n.to_string_lossy().to_uppercase())
            .unwrap_or_default();
        let rows = files::rows(&self.code.files, &self.code.open);
        let shown = match self.nav_page() {
            Some(Page::File(path)) => Some(path.clone()),
            _ => None,
        };
        let chip = self.code_revision_label();
        let list = uniform_list(
            "code-tree",
            rows.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|ix| rows.get(ix).cloned())
                    .enumerate()
                    .map(|(n, row)| this.render_tree_row(row, shown.as_deref(), n, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.code.scroll)
        .flex_1()
        .pb(px(6.));
        island()
            .w(px(self.layout.sidebar))
            .flex_none()
            .child(
                div()
                    .flex_none()
                    .h(px(36.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::muted())
                            .child(project),
                    )
                    .child(
                        div()
                            .id("code-revision")
                            .flex_none()
                            .px(px(6.))
                            .rounded(px(4.))
                            .text_color(theme::faint())
                            .when(!self.plain, |s| {
                                s.cursor_pointer()
                                    .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                            this.open_revision_menu(e.position, cx)
                                        }),
                                    )
                            })
                            .child(chip),
                    ),
            )
            .child(
                div().flex_none().px(px(8.)).pb(px(6.)).child(
                    div()
                        .id("code-go-to-file")
                        .h(px(28.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(super::ROW_RADIUS))
                        .bg(theme::base())
                        .text_color(theme::faint())
                        .cursor_pointer()
                        .child(
                            icons::icon("search")
                                .size(px(13.))
                                .text_color(theme::faint()),
                        )
                        .child(div().flex_1().child("Go to file"))
                        .child(keys::caps("⌘P", keys::On::Plain))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.search(super::palette::Scope::Files, cx)
                        })),
                ),
            )
            .child(list)
    }

    fn render_tree_row(
        &self,
        row: files::Row,
        shown: Option<&str>,
        _n: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::hover::Hover;
        let selected = shown == Some(row.path.as_str());
        let preview = self.code.preview.as_deref() == Some(row.path.as_str());
        let path = row.path.clone();
        super::row(SharedString::from(format!("tree-{}", row.path)), selected)
            // Its whole path, whatever the depth (#79).
            .tip(row.path.clone())
            .h(px(26.))
            .pl(px(8. + row.depth as f32 * 14.))
            .gap(px(6.))
            .child(match row.folder {
                Some(open) => icons::icon(if open { "chevron" } else { "chevron-right" })
                    .size(px(12.))
                    .text_color(theme::faint()),
                None => icons::icon("file").size(px(13.)).text_color(theme::faint()),
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(preview, |s| s.italic())
                    .child(row.name.clone()),
            )
            // A right click: Reveal in Finder, Copy path, Open in.
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener({
                    let path = row.path.clone();
                    move |this, e: &gpui::MouseDownEvent, _, cx| {
                        this.tree_context(path.clone(), e.position, cx)
                    }
                }),
            )
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                if row.folder.is_some() {
                    if !this.code.open.remove(&path) {
                        this.code.open.insert(path.clone());
                    }
                    return cx.notify();
                }
                // A second click keeps it as a tab; picked in the drawer, it goes (#80).
                this.close_drawer(cx);
                this.open_code_file(path.clone(), e.click_count() < 2, cx)
            }))
            .into_any_element()
    }

    /// A file's or a folder's menu in the tree: show it in Finder, copy its path, open it.
    fn tree_context(
        &mut self,
        path: String,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let full = root.join(&path);
        let mut groups = vec![vec![
            Entry::new(super::open_in::REVEAL, Act::Reveal(full.clone())),
            Entry::new("Copy path", Act::Copy(path.clone())),
            Entry::new("Copy full path", Act::Copy(full.display().to_string())),
        ]];
        if let Some(app) = self.open_app(&full) {
            groups.push(vec![Entry::new(
                format!("Open in {}", app.name()),
                Act::OpenPathWith(full, app),
            )]);
        }
        self.open_menu(at, groups, cx);
    }

    /// What the chip says Code reads: the working tree, or the branch or commit picked.
    fn code_revision_label(&self) -> String {
        match (&self.code.rev, self.plain) {
            (_, true) => "the files on disk".to_owned(),
            (None, false) => "working tree".to_owned(),
            (Some((name, _)), false) => name.clone(),
        }
    }

    /// The chip's menu: the working tree, HEAD, the local branches.
    fn open_revision_menu(&mut self, at: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let now = self.code.at();
        let mut entries =
            vec![Entry::new("Working tree", Act::CodeRevision(None)).checked(now.is_none())];
        if let Some(head) = self.branches.iter().find(|b| b.is_head) {
            entries.push(
                Entry::new(
                    format!("HEAD · {}", head.name),
                    Act::CodeRevision(Some((head.name.clone(), head.tip))),
                )
                .checked(
                    now == Some(head.tip)
                        && self.code.rev.as_ref().is_some_and(|r| r.0 == head.name),
                ),
            );
        }
        let branches: Vec<Entry> = self
            .branches
            .iter()
            .filter(|b| b.kind == crate::git::RefKind::Local && !b.is_head)
            .map(|b| {
                Entry::new(
                    b.name.clone(),
                    Act::CodeRevision(Some((b.name.clone(), b.tip))),
                )
                .checked(self.code.rev.as_ref().is_some_and(|r| r.0 == b.name))
            })
            .collect();
        self.open_menu(at, vec![entries, branches], cx);
    }

    /// The Outline tool window: the declarations of the file read in Code; a click goes to one.
    pub(super) fn outline_tool(&self, cx: &mut Context<Self>) -> super::tools::ToolView {
        let outline = self.file_outline();
        let n = outline.len();
        let rows = outline.into_iter().map(|(line, depth, kind, name)| {
            super::row(SharedString::from(format!("outline-{line}")), false)
                .h(px(26.))
                .pl(px(10. + depth as f32 * 14.))
                .gap(px(8.))
                .text_size(px(12.))
                .child(
                    div()
                        .flex_none()
                        .font_family(theme::CODE_FONT)
                        .text_color(theme::faint())
                        .child(kind),
                )
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(line.to_string()),
                )
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.reveal_file_line(line, cx)),
                )
        });
        let empty = match &self.file_view {
            None => "Open a file to see what it declares.",
            Some(_) => "Nothing declared here that GitLance reads.",
        };
        super::tools::ToolView {
            title: "Outline".into(),
            count: (n > 0).then(|| n.to_string()),
            actions: Vec::new(),
            body: div()
                .id("outline-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(6.))
                .children(rows)
                .children((n == 0).then(|| {
                    div()
                        .px(px(8.))
                        .py(px(6.))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(empty)
                }))
                .into_any_element(),
            foot: None,
        }
    }

    /// The menu's choice: Code reads there.
    pub(super) fn set_code_revision(&mut self, rev: Option<(String, Oid)>, cx: &mut Context<Self>) {
        self.read_code_at(rev, cx);
    }

    /// The tabs over a file: the files kept open, the preview one in italics; × or ⌘W closes.
    fn render_code_tabs(&self, cx: &mut Context<Self>) -> Div {
        let shown = match self.nav_page() {
            Some(Page::File(path)) => Some(path.clone()),
            _ => None,
        };
        div()
            .flex_none()
            .h(px(34.))
            .flex()
            .items_end()
            .gap(px(2.))
            .children(self.code.shown_tabs().into_iter().map(|(path, preview)| {
                let on = shown.as_ref() == Some(&path);
                let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                let (open, close) = (path.clone(), path.clone());
                div()
                    .id(SharedString::from(format!("code-tab-{path}")))
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded_t(px(6.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .when(on, |s| s.bg(theme::editor()).text_color(theme::text()))
                    .when(!on, |s| {
                        s.text_color(theme::muted())
                            .hover(|s| s.text_color(theme::text()))
                    })
                    .child(icons::icon("file").size(px(12.)).text_color(theme::faint()))
                    .child(div().when(preview, |s| s.italic()).child(name))
                    .child(
                        div()
                            .id(SharedString::from(format!("code-tab-x-{path}")))
                            .size(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .hover(|s| s.bg(theme::hover()))
                            .child(icons::icon("x").size(px(11.)).text_color(theme::faint()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.close_code_tab(&close, cx)
                            })),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.open_file_view(open.clone(), cx)
                    }))
            }))
    }

    /// The folder page at Code's root: its README rendered, else Go to a file and the files
    /// opened lately.
    fn render_folder_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some((path, text)) = &self.code.readme else {
            return self.render_code_root(cx).into_any_element();
        };
        let body = if path.ends_with(".md") || path.ends_with(".markdown") {
            markdown::body(text, true, "folder-readme", None, None)
        } else {
            div()
                .font_family(theme::CODE_FONT)
                .whitespace_normal()
                .child(text.clone())
                .into_any_element()
        };
        island()
            .flex_1()
            .min_w_0()
            .bg(theme::editor())
            .child(
                div()
                    .flex_none()
                    .h(px(36.))
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(theme::island_border())
                    .text_size(px(12.))
                    .child(icons::icon("file").size(px(13.)).text_color(theme::faint()))
                    .child(
                        div()
                            .font_family(theme::CODE_FONT)
                            .text_color(theme::muted())
                            .child(path.clone()),
                    )
                    .child(div().flex_1())
                    .child(div().text_color(theme::faint()).child("rendered")),
            )
            .child(
                div()
                    .id("folder-page")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(24.))
                    .py(px(18.))
                    .text_size(px(13.))
                    .child(body),
            )
            .into_any_element()
    }
}
