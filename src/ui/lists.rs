//! The lists of the window beyond the commit list: branches grouped by remote with a filter, the
//! changed files as a folder tree or a flat list with a path filter, the labels on commits, and
//! back / forward through what was looked at. Designed in review round 1 (A9, A10, A11, A13).

use super::diff_view::chip;
use super::input::{self, Edit};
use super::{Workspace, change_badge, row, theme};
use crate::git::{Deco, DecoKind, FileDiff, RefKind};
use git2::Oid;
use gpui::{
    ClickEvent, Context, KeyDownEvent, MouseButton, MouseDownEvent, ScrollStrategy, div,
    prelude::*, px,
};
use std::collections::{BTreeMap, HashSet};

// ---- back / forward ---------------------------------------------------------------------

/// Something looked at: a commit on a branch, or a range of versions of it.
#[derive(Clone, PartialEq, Eq)]
pub enum Nav {
    Commit {
        refname: String,
        id: Oid,
    },
    Versions {
        refname: String,
        from: usize,
        to: usize,
    },
}

#[derive(Default)]
pub struct History {
    entries: Vec<Nav>,
    at: usize,
}

impl History {
    pub fn can_back(&self) -> bool {
        self.at > 0
    }

    pub fn can_forward(&self) -> bool {
        self.at + 1 < self.entries.len()
    }
}

impl Workspace {
    fn current_nav(&self) -> Option<Nav> {
        let refname = self.branches.get(self.branch?)?.refname.clone();
        match self.selection {
            super::Selection::Commit(ix) => Some(Nav::Commit {
                refname,
                id: self.commits.get(ix)?.id,
            }),
            super::Selection::Versions { from, to } => Some(Nav::Versions { refname, from, to }),
            super::Selection::WorkingTree | super::Selection::None => None,
        }
    }

    /// Remembers what is shown now as the newest place in the history (dropping any "forward").
    pub(super) fn record(&mut self) {
        let Some(nav) = self.current_nav() else {
            return;
        };
        if self.history.entries.get(self.history.at) == Some(&nav) {
            return;
        }
        let keep = (self.history.at + 1).min(self.history.entries.len());
        self.history.entries.truncate(keep);
        self.history.entries.push(nav);
        self.history.at = self.history.entries.len() - 1;
    }

    pub(super) fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.history.can_back() {
            self.history.at -= 1;
            self.goto(self.history.entries[self.history.at].clone(), cx);
        }
    }

    pub(super) fn go_forward(&mut self, cx: &mut Context<Self>) {
        if self.history.can_forward() {
            self.history.at += 1;
            self.goto(self.history.entries[self.history.at].clone(), cx);
        }
    }

    fn goto(&mut self, nav: Nav, cx: &mut Context<Self>) {
        let (refname, commit) = match &nav {
            Nav::Commit { refname, id } => (refname, Some(*id)),
            Nav::Versions { refname, .. } => (refname, None),
        };
        let here = self
            .branch
            .and_then(|ix| self.branches.get(ix))
            .is_some_and(|b| &b.refname == refname);
        if !here {
            if let Some(ix) = self.branches.iter().position(|b| &b.refname == refname) {
                self.select_branch(ix, commit, cx);
            }
            return;
        }
        match nav {
            Nav::Commit { id, .. } => {
                if let Some(ix) = self.commits.iter().position(|c| c.id == id) {
                    self.select_commit(ix, cx);
                }
            }
            Nav::Versions { from, to, .. } if to < self.versions.len() => {
                self.select_versions(from, to, cx)
            }
            Nav::Versions { .. } => {}
        }
    }
}

// ---- labels on commits ------------------------------------------------------------------

/// "HEAD → main", "origin/main", "v1.2": the first label and a count of the rest.
pub fn deco_tags(decos: &[Deco]) -> Vec<gpui::AnyElement> {
    let tag = |text: String, color: gpui::Rgba| {
        div()
            .flex_none()
            .px(px(6.))
            .rounded_full()
            .border_1()
            .border_color(color)
            .text_size(px(10.))
            .text_color(color)
            .max_w(px(120.))
            .truncate()
            .child(text)
            .into_any_element()
    };
    let mut out: Vec<_> = decos
        .iter()
        .take(1)
        .map(|d| match d.kind {
            DecoKind::Head => tag(format!("HEAD → {}", d.name), theme::accent()),
            DecoKind::Local => tag(d.name.clone(), theme::accent()),
            DecoKind::Remote => tag(d.name.clone(), theme::faint()),
            DecoKind::Tag => tag(d.name.clone(), theme::warning()),
        })
        .collect();
    if decos.len() > 1 {
        out.push(tag(format!("+{}", decos.len() - 1), theme::faint()));
    }
    out
}

// ---- branches ---------------------------------------------------------------------------

enum BranchItem {
    Header(String, usize),
    Branch(usize),
}

impl Workspace {
    /// Local branches, then each remote's under its own heading; a filter flattens the list.
    fn branch_items(&self) -> Vec<BranchItem> {
        let filter = self.bfilter.to_lowercase();
        if !filter.trim().is_empty() {
            return self
                .branches
                .iter()
                .enumerate()
                .filter(|(_, b)| {
                    filter
                        .split_whitespace()
                        .all(|w| b.name.to_lowercase().contains(w))
                })
                .map(|(ix, _)| BranchItem::Branch(ix))
                .collect();
        }
        let mut out: Vec<BranchItem> = self
            .branches
            .iter()
            .enumerate()
            .filter(|(_, b)| b.kind == RefKind::Local)
            .map(|(ix, _)| BranchItem::Branch(ix))
            .collect();
        let mut remotes: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (ix, b) in self.branches.iter().enumerate() {
            if b.kind == RefKind::Remote {
                let remote = b.name.split_once('/').map_or("", |(r, _)| r);
                remotes.entry(remote).or_default().push(ix);
            }
        }
        for (remote, ixs) in remotes {
            out.push(BranchItem::Header(remote.to_owned(), ixs.len()));
            out.extend(ixs.into_iter().map(BranchItem::Branch));
        }
        out
    }

    pub(super) fn render_branches(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let filtering = !self.bfilter.trim().is_empty();
        let field = self.render_field(
            "branch-filter",
            (self.field == Some(Field::Branches)).then_some(self.bfilter.as_str()),
            &self.bfilter,
            "Filter branches",
            cx.listener(|this, _: &ClickEvent, _, cx| this.start_field(Field::Branches, cx)),
        );
        let items = self.branch_items().into_iter().map(|item| match item {
            BranchItem::Header(name, n) => div()
                .px(px(8.))
                .pt(px(8.))
                .pb(px(2.))
                .text_size(px(11.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::faint())
                .child(format!("{} · {n}", name.to_uppercase()))
                .into_any_element(),
            BranchItem::Branch(ix) => {
                let b = &self.branches[ix];
                // Under its remote's heading a name needs no prefix.
                let name = match (b.kind, filtering) {
                    (RefKind::Remote, false) => {
                        b.name.split_once('/').map_or(b.name.as_str(), |(_, n)| n)
                    }
                    _ => b.name.as_str(),
                };
                div()
                    .py(px(1.))
                    .child(
                        row(("branch", ix), self.branch == Some(ix))
                            .h(px(26.))
                            .gap_2()
                            .child(
                                div()
                                    .w(px(8.))
                                    .text_color(theme::accent())
                                    .child(if b.is_head { "●" } else { "" }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(match b.kind {
                                        RefKind::Local => theme::text(),
                                        RefKind::Remote => theme::muted(),
                                    })
                                    .child(name.to_owned()),
                            )
                            .children(self.request_of_branch(ix).map(|m| {
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(theme::accent())
                                    .child(format!("!{}", m.iid))
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                    this.branch_context(ix, event.position, cx)
                                }),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_branch(ix, None, cx);
                                this.record();
                            })),
                    )
                    .into_any_element()
            }
        });
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(field)
            .child(
                div()
                    .id("branches")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(6.))
                    .pb(px(6.))
                    .children(items),
            )
    }

    /// A one-line field like the commit search: its text or a placeholder; `active` while typing.
    pub(super) fn render_field(
        &self,
        id: &'static str,
        active: Option<&str>,
        text: &str,
        placeholder: &'static str,
        on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        let active = active.is_some();
        let text = text.to_owned();
        div()
            .id(id)
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .mx(px(6.))
            .mb(px(6.))
            .px_2()
            .h(px(26.))
            .rounded(px(super::ROW_RADIUS))
            .border_2()
            .border_color(if active {
                theme::focus()
            } else {
                theme::island_border()
            })
            .bg(theme::base())
            .cursor_text()
            .on_click(on_click)
            .child(div().text_color(theme::faint()).child("⌕"))
            .child(input::field_text(
                &text,
                active,
                self.field_all,
                placeholder,
            ))
    }

    pub(super) fn start_field(&mut self, field: Field, cx: &mut Context<Self>) {
        self.field_all = false;
        self.find.text = None;
        self.dfind = None;
        self.dterms = Default::default();
        self.refilter();
        self.recompute_matches();
        if field == Field::Files {
            self.layout.show_files = true;
        }
        self.field = Some(field);
        cx.notify();
    }

    /// Typing into the branch or path filter; escape clears it. Returns whether it took the key.
    pub(super) fn filter_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(field) = self.field else {
            return false;
        };
        let text = match field {
            Field::Branches => &mut self.bfilter,
            Field::Files => &mut self.pfilter,
            Field::Token => &mut self.token_input,
        };
        match input::edit(text, &mut self.field_all, &event.keystroke, cx) {
            Edit::Escape => {
                text.clear();
                self.field = None;
            }
            Edit::Enter { .. } if field == Field::Token => {
                self.submit_token(cx);
                return true;
            }
            Edit::Changed | Edit::Selected | Edit::Enter { .. } | Edit::Ignored => {}
        }
        cx.notify();
        true
    }
}

/// A filter field that can take the keyboard.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Branches,
    Files,
    /// The GitLab token being typed or pasted in the connect panel.
    Token,
}

// ---- changed files ----------------------------------------------------------------------

/// One row of the files island.
pub enum FileItem {
    Dir {
        path: String,
        name: String,
        depth: usize,
        count: usize,
        collapsed: bool,
    },
    File {
        ix: usize,
        depth: usize,
    },
}

#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<usize>,
}

impl Node {
    fn count(&self) -> usize {
        self.files.len() + self.dirs.values().map(Node::count).sum::<usize>()
    }
}

/// The tree's rows: folders before files, a chain of folders with nothing else folded into one
/// (`src/ui`), a collapsed folder hiding what is under it.
fn flatten(
    node: &Node,
    depth: usize,
    prefix: &str,
    collapsed: &HashSet<String>,
    out: &mut Vec<FileItem>,
) {
    for (name, child) in &node.dirs {
        let mut path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let mut shown = name.clone();
        let mut cur = child;
        while cur.files.is_empty() && cur.dirs.len() == 1 {
            let (n, c) = cur.dirs.iter().next().expect("one folder");
            path = format!("{path}/{n}");
            shown = format!("{shown}/{n}");
            cur = c;
        }
        let is_collapsed = collapsed.contains(&path);
        out.push(FileItem::Dir {
            path: path.clone(),
            name: shown,
            depth,
            count: cur.count(),
            collapsed: is_collapsed,
        });
        if !is_collapsed {
            flatten(cur, depth + 1, &path, collapsed, out);
        }
    }
    out.extend(node.files.iter().map(|&ix| FileItem::File { ix, depth }));
}

/// The rows for `files`: a tree, a flat list, or — whatever `list` says — a flat list of the files
/// whose path holds every word of `filter`.
pub fn file_items(
    files: &[FileDiff],
    list: bool,
    filter: &str,
    collapsed: &HashSet<String>,
) -> Vec<FileItem> {
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    if !words.is_empty() {
        return files
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                let path = f.path().to_lowercase();
                words.iter().all(|w| path.contains(w))
            })
            .map(|(ix, _)| FileItem::File { ix, depth: 0 })
            .collect();
    }
    if list {
        return (0..files.len())
            .map(|ix| FileItem::File { ix, depth: 0 })
            .collect();
    }
    let mut root = Node::default();
    for (ix, file) in files.iter().enumerate() {
        let mut node = &mut root;
        let mut parts: Vec<&str> = file.path().split('/').collect();
        parts.pop();
        for dir in parts {
            node = node.dirs.entry(dir.to_owned()).or_default();
        }
        node.files.push(ix);
    }
    let mut out = Vec::new();
    flatten(&root, 0, "", collapsed, &mut out);
    out
}

impl Workspace {
    pub(super) fn items(&self) -> Vec<FileItem> {
        match &self.diff {
            Some(diff) => file_items(
                &diff.files,
                self.options.list_files,
                &self.pfilter,
                &self.collapsed,
            ),
            None => Vec::new(),
        }
    }

    /// File indexes in the order the island shows them: what ← and → walk through.
    pub(super) fn file_order(&self) -> Vec<usize> {
        self.items()
            .into_iter()
            .filter_map(|item| match item {
                FileItem::File { ix, .. } => Some(ix),
                FileItem::Dir { .. } => None,
            })
            .collect()
    }

    /// What the files island shows, as keys for keyboard movement.
    pub(super) fn item_keys(&self) -> Vec<super::zones::ItemKey> {
        self.items()
            .into_iter()
            .map(|item| match item {
                FileItem::File { ix, .. } => super::zones::ItemKey::File(ix),
                FileItem::Dir { path, .. } => super::zones::ItemKey::Dir(path),
            })
            .collect()
    }

    /// Branch indexes in the order the list shows them.
    pub(super) fn branch_ixs(&self) -> Vec<usize> {
        self.branch_items()
            .into_iter()
            .filter_map(|item| match item {
                BranchItem::Branch(ix) => Some(ix),
                BranchItem::Header(..) => None,
            })
            .collect()
    }

    /// Scrolls the files island to show file `ix`.
    pub(super) fn scroll_file_into_view(&self, ix: usize) {
        let at = self
            .items()
            .iter()
            .position(|item| matches!(item, FileItem::File { ix: i, .. } if *i == ix));
        if let Some(at) = at {
            self.file_scroll.scroll_to_item(at, ScrollStrategy::Nearest);
        }
    }

    pub(super) fn set_file_view(&mut self, list: bool, cx: &mut Context<Self>) {
        if self.options.list_files != list {
            self.options.list_files = list;
            self.options.save();
            cx.notify();
        }
    }

    pub(super) fn toggle_dir(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(path) {
            self.collapsed.insert(path.to_owned());
        }
        cx.notify();
    }

    pub(super) fn render_files(
        &self,
        diff: &super::Diff,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let count = self.items().len();
        let list = self.options.list_files;
        super::island()
            .w(px(self.layout.files))
            .flex_none()
            .border_color(self.zone_border(super::zones::Zone::Files))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.set_zone(super::zones::Zone::Files, cx)),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .pr(px(8.))
                    .child(super::island_label(super::plural(diff.files.len(), "file")).flex_1())
                    .child(
                        super::diff_view::group()
                            .child(chip("files-tree", "Tree", !list).on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| this.set_file_view(false, cx),
                            )))
                            .child(chip("files-list", "List", list).on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| this.set_file_view(true, cx),
                            ))),
                    ),
            )
            .child(self.render_field(
                "file-filter",
                (self.field == Some(Field::Files)).then_some(self.pfilter.as_str()),
                &self.pfilter,
                "Filter by path  ⌘P",
                cx.listener(|this, _: &ClickEvent, _, cx| this.start_field(Field::Files, cx)),
            ))
            .child(if count == 0 {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("No changed file matches.")
                    .into_any_element()
            } else {
                uniform_list_files(self, count, cx)
            })
    }

    fn render_file_item(&self, item: &FileItem, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(diff) = &self.diff else {
            return div().into_any_element();
        };
        let tree = !self.options.list_files && self.pfilter.trim().is_empty();
        match item {
            FileItem::File { ix, depth } if tree => {
                let file = &diff.files[*ix];
                let ix = *ix;
                let name = file
                    .path()
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                div()
                    .w_full()
                    .h(px(28.))
                    .px(px(6.))
                    .py(px(1.))
                    .child(
                        row(("file", ix), self.file == ix)
                            .h(px(26.))
                            .gap_2()
                            .pl(px(8. + *depth as f32 * 14.))
                            .child(change_badge(file.change))
                            .child(div().flex_1().min_w_0().truncate().child(name))
                            .children(super::working::file_tag(diff.tags.get(file.path())))
                            .child(super::stats(file))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                    this.file_context(ix, event.position, cx)
                                }),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.select_file(ix, cx))),
                    )
                    .into_any_element()
            }
            FileItem::File { ix, .. } => super::render_file_row(
                &diff.files[*ix],
                diff.tags.get(diff.files[*ix].path()),
                *ix,
                self.file == *ix,
                cx,
            )
            .into_any_element(),
            FileItem::Dir {
                path,
                name,
                depth,
                count,
                collapsed,
            } => {
                let target = path.clone();
                div()
                    .w_full()
                    .h(px(28.))
                    .px(px(6.))
                    .py(px(1.))
                    .child(
                        row(
                            ("dir", path.len() * 31 + *depth),
                            self.fcursor.as_deref() == Some(path.as_str()),
                        )
                        .h(px(26.))
                        .gap_2()
                        .pl(px(8. + *depth as f32 * 14.))
                        .text_color(theme::muted())
                        .child(
                            super::icons::icon(if *collapsed {
                                "chevron-right"
                            } else {
                                "chevron"
                            })
                            .size(px(16.))
                            .text_color(theme::muted()),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(count.to_string()),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_dir(&target, cx))),
                    )
                    .into_any_element()
            }
        }
    }
}

/// The files list proper: one uniform list, all rows of one height.
fn uniform_list_files(
    this: &Workspace,
    count: usize,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    gpui::uniform_list(
        "files",
        count,
        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
            let items = this.items();
            range
                .filter_map(|i| items.get(i))
                .map(|item| this.render_file_item(item, cx))
                .collect::<Vec<_>>()
        }),
    )
    .track_scroll(&this.file_scroll)
    .flex_1()
    .pb(px(6.))
    .into_any_element()
}
