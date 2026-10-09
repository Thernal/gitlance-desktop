//! The ⌘K palette: one field that jumps to a commit, a file, a branch or a command, and the same
//! overlay as the picker for comparing any two refs (⌘⇧C).

use super::input::{self, Edit};
use super::{Diff, Header, ISLAND_RADIUS, Selection, Workspace, format, island_label, theme};
use crate::git::{NamedRef, Repo};
use git2::Oid;
use gpui::{Action, Context, KeyDownEvent, MouseButton, Window, div, prelude::*, px};
use std::sync::Arc;

/// What the palette is for.
#[derive(Clone)]
pub enum Kind {
    Jump,
    /// Picking the first side of a comparison.
    Base,
    /// Picking the second side, the first being `.0`.
    Head((String, Oid)),
}

#[derive(Clone, Copy)]
pub enum Cmd {
    Open,
    Refresh,
    Settings,
    Back,
    Forward,
    FindInDiff,
    FindCommits,
    FilterFiles,
    ToggleSidebar,
    ToggleFiles,
    ToggleRequests,
    FocusDiff,
    Unified,
    CycleMode,
    Wrap,
    FullContext,
    Whitespace,
    Compare,
    NewTab,
    CreateRequest,
    Shortcuts,
    Comments,
}

const COMMANDS: &[(&str, &str, Cmd)] = &[
    ("Compare two refs…", "⌘⇧C", Cmd::Compare),
    ("Create merge request…", "⌥⌘M", Cmd::CreateRequest),
    ("Keyboard shortcuts", "?", Cmd::Shortcuts),
    ("Show or hide the comments", "⌘⇧R", Cmd::Comments),
    ("Find in the diff", "⌘F", Cmd::FindInDiff),
    ("Search commits", "⌘⇧F", Cmd::FindCommits),
    ("Filter files by path", "⌘P", Cmd::FilterFiles),
    ("Refresh", "⌘R", Cmd::Refresh),
    ("Open a repository…", "⌘O", Cmd::Open),
    ("Back", "⌘[", Cmd::Back),
    ("Forward", "⌘]", Cmd::Forward),
    ("Toggle split / unified", "⌥U", Cmd::Unified),
    ("Cycle lines, words, structural", "⌥D", Cmd::CycleMode),
    ("Toggle wrap", "⌥Z", Cmd::Wrap),
    ("Toggle full file", "⌥E", Cmd::FullContext),
    ("Toggle ignore whitespace", "⌥W", Cmd::Whitespace),
    ("Hide or show the branches", "⌥⌘1", Cmd::ToggleSidebar),
    ("Hide or show the files", "⌥⌘2", Cmd::ToggleFiles),
    ("Branches or merge requests", "⌥⌘3", Cmd::ToggleRequests),
    ("Diff only", "⌘.", Cmd::FocusDiff),
    ("New tab", "⌘T", Cmd::NewTab),
    ("Settings", "⌘,", Cmd::Settings),
];

#[derive(Clone)]
enum Pick {
    Commit(Oid),
    File(usize),
    Branch(usize),
    Command(Cmd),
    Ref(String, Oid),
}

#[derive(Clone)]
struct Item {
    group: &'static str,
    label: String,
    detail: String,
    key: &'static str,
    pick: Pick,
}

pub struct Palette {
    kind: Kind,
    query: String,
    at: usize,
    all: bool,
    items: Vec<Item>,
    refs: Vec<NamedRef>,
}

/// The words of `query` all occur in `hay`, ignoring case.
fn matches(words: &[String], hay: &str) -> bool {
    let hay = hay.to_lowercase();
    words.iter().all(|w| hay.contains(w.as_str()))
}

impl Workspace {
    pub(super) fn open_palette(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let refs = match (&kind, &self.root) {
            (Kind::Jump, _) | (_, None) => Vec::new(),
            (_, Some(root)) => Repo::open(root)
                .and_then(|r| r.compare_refs())
                .unwrap_or_default(),
        };
        self.field = None;
        self.dfind = None;
        self.find.text = None;
        self.ctx_menu = None;
        self.repo_menu = false;
        self.palette = Some(Palette {
            kind,
            query: String::new(),
            at: 0,
            all: false,
            items: Vec::new(),
            refs,
        });
        self.refresh_palette();
        cx.notify();
    }

    pub(super) fn close_palette(&mut self, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            cx.notify();
        }
    }

    fn refresh_palette(&mut self) {
        let Some(palette) = &self.palette else {
            return;
        };
        let items = self.palette_items(palette);
        if let Some(p) = &mut self.palette {
            p.at = p.at.min(items.len().saturating_sub(1));
            p.items = items;
        }
    }

    fn palette_items(&self, p: &Palette) -> Vec<Item> {
        let query = p.query.trim_start();
        if !matches!(p.kind, Kind::Jump) {
            let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
            let group = if matches!(p.kind, Kind::Base) {
                "Base"
            } else {
                "Compare with"
            };
            let mut items: Vec<Item> = p
                .refs
                .iter()
                .filter(|r| matches(&words, &r.name))
                .take(14)
                .map(|r| Item {
                    group,
                    label: r.name.clone(),
                    detail: format!(
                        "{} {}",
                        if r.tag { "tag" } else { "branch" },
                        format::short(r.tip)
                    ),
                    key: "",
                    pick: Pick::Ref(r.name.clone(), r.tip),
                })
                .collect();
            // Anything git can name: a commit id, HEAD~3, a ref the list does not show.
            if !query.is_empty()
                && !p.refs.iter().any(|r| r.name == query.trim())
                && let Some(id) = self
                    .root
                    .as_ref()
                    .and_then(|root| Repo::open(root).ok()?.resolve(query).ok())
            {
                items.insert(
                    0,
                    Item {
                        group,
                        label: query.trim().to_owned(),
                        detail: format!("commit {}", format::short(id)),
                        key: "",
                        pick: Pick::Ref(query.trim().to_owned(), id),
                    },
                );
            }
            return items;
        }

        let (scope, rest) = match query.chars().next() {
            Some(c @ ('>' | '#' | '@')) => (Some(c), &query[1..]),
            _ => (None, query),
        };
        let words: Vec<String> = rest.split_whitespace().map(str::to_lowercase).collect();
        let cap = |small: usize| if scope.is_some() { 14 } else { small };
        let mut items = Vec::new();
        if scope.is_none_or(|s| s == '>') {
            items.extend(
                COMMANDS
                    .iter()
                    .filter(|(label, ..)| matches(&words, label))
                    .take(cap(5))
                    .map(|&(label, key, cmd)| Item {
                        group: "Commands",
                        label: label.to_owned(),
                        detail: String::new(),
                        key,
                        pick: Pick::Command(cmd),
                    }),
            );
        }
        if scope.is_none_or(|s| s == '#') && !words.is_empty() | scope.is_some() {
            items.extend(
                self.branches
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| matches(&words, &b.name))
                    .take(cap(3))
                    .map(|(ix, b)| Item {
                        group: "Branches",
                        label: b.name.clone(),
                        detail: format::short(b.tip),
                        key: "",
                        pick: Pick::Branch(ix),
                    }),
            );
        }
        if scope.is_none_or(|s| s == '@')
            && !words.is_empty() | scope.is_some()
            && let Some(diff) = &self.diff
        {
            items.extend(
                diff.files
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| matches(&words, f.path()))
                    .take(cap(3))
                    .map(|(ix, f)| Item {
                        group: "Files in this diff",
                        label: f.path().to_owned(),
                        detail: String::new(),
                        key: "",
                        pick: Pick::File(ix),
                    }),
            );
        }
        if scope.is_none() && !words.is_empty() {
            items.extend(
                self.commits
                    .iter()
                    .filter(|c| {
                        matches(&words, &c.summary)
                            || matches(&words, &c.author)
                            || c.id.to_string().starts_with(&words[0])
                    })
                    .take(4)
                    .map(|c| Item {
                        group: "Commits",
                        label: c.summary.clone(),
                        detail: format!("{} · {}", format::short(c.id), c.author),
                        key: "",
                        pick: Pick::Commit(c.id),
                    }),
            );
            // Jumping to what was typed beats the commands when something real matched.
            items.sort_by_key(|i| (i.group == "Commands") as u8);
        }
        items
    }

    pub(super) fn palette_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(palette) = &mut self.palette else {
            return;
        };
        match input::edit(&mut palette.query, &mut palette.all, &event.keystroke, cx) {
            Edit::Changed => {
                palette.at = 0;
                self.refresh_palette();
            }
            Edit::Enter { .. } => {
                let at = palette.at;
                self.run_palette_item(at, window, cx);
                return;
            }
            Edit::Escape => {
                // A second pick goes back to the first.
                if matches!(palette.kind, Kind::Head(_)) {
                    palette.kind = Kind::Base;
                    palette.query.clear();
                    self.refresh_palette();
                } else {
                    self.palette = None;
                }
            }
            Edit::Selected | Edit::Ignored => {}
        }
        cx.notify();
    }

    /// Sets the field's text, as typing it would (UI checks).
    #[cfg(feature = "snapshot")]
    pub(super) fn type_in_palette(&mut self, text: &str) {
        if let Some(p) = &mut self.palette {
            p.query = text.to_owned();
        }
        self.refresh_palette();
    }

    pub(super) fn palette_step(&mut self, down: bool, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.palette
            && !p.items.is_empty()
        {
            let n = p.items.len();
            p.at = if down {
                (p.at + 1) % n
            } else {
                (p.at + n - 1) % n
            };
            cx.notify();
        }
    }

    pub(super) fn run_palette_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.palette.as_ref().and_then(|p| p.items.get(ix)).cloned() else {
            return;
        };
        match item.pick {
            Pick::Ref(name, id) => {
                let kind = self.palette.as_ref().map(|p| p.kind.clone());
                match kind {
                    Some(Kind::Base) => {
                        if let Some(p) = &mut self.palette {
                            p.kind = Kind::Head((name, id));
                            p.query.clear();
                            p.at = 0;
                        }
                        self.refresh_palette();
                    }
                    Some(Kind::Head(base)) => {
                        self.palette = None;
                        self.run_compare(base, (name, id), true, None, cx);
                    }
                    _ => self.palette = None,
                }
            }
            Pick::Commit(id) => {
                self.palette = None;
                if let Some(ix) = self.commits.iter().position(|c| c.id == id) {
                    self.select_commit(ix, cx);
                }
            }
            Pick::File(ix) => {
                self.palette = None;
                self.select_file(ix, cx);
            }
            Pick::Branch(ix) => {
                self.palette = None;
                self.select_branch(ix, None, cx);
            }
            Pick::Command(cmd) => {
                self.palette = None;
                cx.notify();
                window.dispatch_action(command_action(cmd), cx);
            }
        }
        cx.notify();
    }

    /// Shows what `head` changed against `base`; `since_merge_base` for `base...head`.
    pub(super) fn run_compare(
        &mut self,
        base: (String, Oid),
        head: (String, Oid),
        since_merge_base: bool,
        request: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.clear_diff();
        self.selection = Selection::None;
        let settings = self.settings();
        self.load_diff(None, cx, move || {
            let c = Repo::open(&root)?.compare(base.1, head.1, since_merge_base, settings)?;
            Ok(Diff {
                header: Header::Compare {
                    base,
                    head,
                    since_merge_base,
                    start: c.start,
                    commits: c.commits,
                    request,
                },
                files: Arc::new(c.files),
                pairs: Vec::new(),
                tags: Default::default(),
            })
        });
        cx.notify();
    }

    pub(super) fn render_palette(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let p = self.palette.as_ref()?;
        let placeholder = match &p.kind {
            Kind::Jump => "Search commits, files, branches, commands   > # @",
            Kind::Base => "Compare — pick the base: a branch, a tag or a commit",
            Kind::Head(_) => "Compare — pick what to compare with it",
        };
        let title = match &p.kind {
            Kind::Jump => None,
            Kind::Base => Some("Compare · step 1 of 2".to_owned()),
            Kind::Head((name, _)) => Some(format!("Compare · {name} … ?")),
        };
        let mut last = "";
        let mut rows = Vec::new();
        for (ix, item) in p.items.iter().enumerate() {
            if item.group != last {
                last = item.group;
                rows.push(
                    div()
                        .px(px(10.))
                        .pt(px(8.))
                        .pb(px(2.))
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(item.group.to_uppercase())
                        .into_any_element(),
                );
            }
            let selected = ix == p.at;
            rows.push(
                super::row(("palette-item", ix), selected)
                    .h(px(28.))
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(item.label.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(item.detail.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(item.key),
                    )
                    .on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.run_palette_item(ix, window, cx)
                        }),
                    )
                    .into_any_element(),
            );
        }
        let empty = p.items.is_empty().then(|| {
            div()
                .px(px(14.))
                .py(px(12.))
                .text_color(theme::muted())
                .child(match &p.kind {
                    Kind::Jump if p.query.is_empty() => "Type to search.",
                    Kind::Jump => "Nothing matches.",
                    _ => "No branch, tag or commit by that name.",
                })
        });
        Some(
            div()
                .id("palette-backdrop")
                .absolute()
                .size_full()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(72.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_palette(cx)),
                )
                .child(
                    div()
                        .occlude()
                        .w(px(640.))
                        .flex()
                        .flex_col()
                        .rounded(px(ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .overflow_hidden()
                        .children(title.map(island_label))
                        .child(
                            div()
                                .h(px(44.))
                                .px(px(14.))
                                .flex()
                                .items_center()
                                .child(input::field_text(&p.query, true, p.all, placeholder)),
                        )
                        .child(div().h(px(1.)).bg(theme::island_border()))
                        .child(
                            div()
                                .id("palette-list")
                                .max_h(px(480.))
                                .overflow_y_scroll()
                                .pb(px(6.))
                                .px(px(6.))
                                .children(rows)
                                .children(empty),
                        ),
                ),
        )
    }
}

fn command_action(cmd: Cmd) -> Box<dyn Action> {
    use super::*;
    match cmd {
        Cmd::Open => Box::new(Open),
        Cmd::Refresh => Box::new(Refresh),
        Cmd::Settings => Box::new(OpenSettings),
        Cmd::Back => Box::new(GoBack),
        Cmd::Forward => Box::new(GoForward),
        Cmd::FindInDiff => Box::new(Find),
        Cmd::FindCommits => Box::new(FindCommits),
        Cmd::FilterFiles => Box::new(FilterFiles),
        Cmd::ToggleSidebar => Box::new(ToggleSidebar),
        Cmd::ToggleFiles => Box::new(ToggleFiles),
        Cmd::ToggleRequests => Box::new(ToggleRequests),
        Cmd::FocusDiff => Box::new(FocusDiff),
        Cmd::Unified => Box::new(ToggleUnified),
        Cmd::CycleMode => Box::new(CycleMode),
        Cmd::Wrap => Box::new(ToggleWrap),
        Cmd::FullContext => Box::new(ToggleFullContext),
        Cmd::Whitespace => Box::new(ToggleWhitespace),
        Cmd::Compare => Box::new(Compare),
        Cmd::NewTab => Box::new(NewTab),
        Cmd::CreateRequest => Box::new(CreateRequest),
        Cmd::Shortcuts => Box::new(ShowShortcuts),
        Cmd::Comments => Box::new(ToggleComments),
    }
}
