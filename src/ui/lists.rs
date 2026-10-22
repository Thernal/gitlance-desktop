//! The lists of the window beyond the commit list: the filter fields, the changed files as a folder
//! tree or a flat list with a path filter and the labels on commits. Designed in review round 1
//! (A9, A10, A11, A13); the branches' list is Branches' root page (`roots.rs`, #72).

use super::input::{self, Edit};
use super::px;
use super::{Workspace, change_badge, row, theme};
use crate::git::{Deco, DecoKind, FileDiff};
use gpui::{
    ClickEvent, Context, KeyDownEvent, MouseButton, MouseDownEvent, ScrollStrategy, div, prelude::*,
};
use std::collections::{BTreeMap, HashSet};

// ---- labels on commits ------------------------------------------------------------------

/// "HEAD → main", "origin/main", "v1.2": the first label and a count of the rest.
pub fn deco_tags(decos: &[Deco]) -> Vec<gpui::AnyElement> {
    let tag = |text: String, color: gpui::Rgba| {
        // The pill gives way (its text is cut with …) before the time beside it does.
        div()
            .flex_shrink(1.)
            .min_w_0()
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
    // The count never gives way: it says there is more.
    if decos.len() > 1 {
        out.push(
            div()
                .flex_none()
                .px(px(6.))
                .rounded_full()
                .border_1()
                .border_color(theme::faint())
                .text_size(px(10.))
                .text_color(theme::faint())
                .child(format!("+{}", decos.len() - 1))
                .into_any_element(),
        );
    }
    out
}

impl Workspace {
    /// The one-line field of `field` in a list's head, inset from the list's edges.
    pub(super) fn render_field(
        &self,
        id: &'static str,
        field: Field,
        placeholder: &'static str,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        self.field_box(id, field, placeholder, cx)
            .mx(px(6.))
            .mb(px(6.))
    }

    /// The one-line field of `field`, like the commit search: its text or a placeholder; a click
    /// gives it the keyboard.
    pub(super) fn field_box(
        &self,
        id: &'static str,
        field: Field,
        placeholder: &'static str,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let active = self.field == Some(field);
        let (text, look) = match field {
            Field::Branches => (&self.bfilter, input::Look::default()),
            Field::Files => (&self.pfilter, input::Look::default()),
            Field::Token => (
                &self.token_input,
                input::Look {
                    mask: true,
                    ..Default::default()
                },
            ),
            Field::Keys => (&self.keys_filter, input::Look::default()),
            Field::ThemeValue => (&self.theme_value, input::Look::default()),
        };
        div()
            .id(id)
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
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
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.start_field(field, cx)))
            .child(
                super::icons::icon("search")
                    .size(px(14.))
                    .text_color(theme::faint()),
            )
            .child(self.field_look(text, active, placeholder, look, cx.entity().downgrade()))
    }

    pub(super) fn start_field(&mut self, field: Field, cx: &mut Context<Self>) {
        if self.field != Some(field) {
            self.reset_field_edit();
        }
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
        if field == Field::ThemeValue {
            match input::edit(
                &mut self.theme_value,
                &mut self.field_edit,
                &event.keystroke,
                cx,
            ) {
                Edit::Changed => self.theme_value_changed(cx),
                Edit::Enter { .. } | Edit::Escape => self.field = None,
                Edit::Selected | Edit::Ignored => {}
            }
            cx.notify();
            return true;
        }
        // A root list's filter keeps ↑ ↓ for the list it filters (#72).
        if field == Field::Branches
            && self.at_root_list()
            && matches!(event.keystroke.key.as_str(), "up" | "down")
        {
            self.step_pick(event.keystroke.key == "down", cx);
            return true;
        }
        // Typed, the keyboard filter matches by words again.
        if field == Field::Keys {
            self.keys_exact = false;
        }
        let text = match field {
            Field::Branches => &mut self.bfilter,
            Field::Files => &mut self.pfilter,
            Field::Token => &mut self.token_input,
            Field::Keys => &mut self.keys_filter,
            Field::ThemeValue => &mut self.theme_value,
        };
        match input::edit(text, &mut self.field_edit, &event.keystroke, cx) {
            Edit::Escape => {
                text.clear();
                self.field = None;
            }
            Edit::Enter { .. } if field == Field::Branches && self.at_root_list() => {
                self.open_pick(cx);
                return true;
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
    /// The filter of Settings → Keyboard.
    Keys,
    /// The value of a token in the theme editor (Settings → Appearance).
    ThemeValue,
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

    /// The Changes section's body: how far the review is, the path filter, the files.
    pub(super) fn render_changes(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let count = self.items().len();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .children(self.render_review_progress(cx))
            .child(self.render_field("file-filter", Field::Files, "Filter by path  ⌥⌘P", cx))
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
                            .when(
                                self.review_state(ix) == Some(super::reviewed::State::Done),
                                |s| s.text_color(theme::muted()),
                            )
                            .children(self.review_box(ix, cx))
                            .child(change_badge(file))
                            .child(div().flex_1().min_w_0().truncate().child(name))
                            .children(super::generated_tag(file))
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
                self.review_box(*ix, cx).map(IntoElement::into_any_element),
                self.review_state(*ix) == Some(super::reviewed::State::Done),
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
                        .children(self.folder_box(path, cx))
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

/// "3/7" beside a branch or request whose review started: files reviewed of the whole diff.
pub(super) fn progress_badge(done: u32, total: u32) -> gpui::Div {
    div()
        .flex_none()
        .px(px(5.))
        .rounded_full()
        .bg(theme::hover())
        .text_size(px(11.))
        .text_color(theme::muted())
        .child(format!("{done}/{total}"))
}
