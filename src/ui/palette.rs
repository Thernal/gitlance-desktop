//! The ⌘K palette: one field that jumps to a commit, a file, a branch or a command, and the same
//! overlay as the picker for comparing any two refs (⌘⇧C).

use super::input::{self, Edit};
use super::px;
use super::{Diff, Header, ISLAND_RADIUS, Selection, Workspace, format, island_label, theme};
use crate::git::{NamedRef, Repo};
use git2::Oid;
use gpui::{Action, Context, KeyDownEvent, MouseButton, Window, div, prelude::*};
use std::sync::Arc;

/// What the palette is for.
#[derive(Clone)]
pub enum Kind {
    Jump,
    /// The places opened lately, newest first (⌘E).
    Recent,
    /// The functions and types the open file changes (⌘⇧O).
    Structure,
    /// The lines marked with F3 in the open diff (⌘F3).
    Bookmarks,
    /// Switching to a branch or a merge request (⌘B), with how far each one's review got.
    Switch,
    /// Picking the first side of a comparison.
    Base,
    /// Picking the second side, the first being `.0`.
    Head((String, Oid)),
    /// The themes (#12 B): ↑↓ tries each on the whole window, ↵ keeps, esc goes back.
    Theme,
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
    Rendered,
    Theme,
    Wrap,
    FullContext,
    Whitespace,
    OneFile,
    ExpandAll,
    RichComments,
    LeaveRequest,
    AllReviewed,
    NextRequest,
    PreviousRequest,
    Compare,
    NewTab,
    CreateRequest,
    SendToAgent,
    Shortcuts,
    Comments,
    Details,
    FindInFiles,
    Annotate,
    Recent,
    Structure,
    Bookmarks,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Switch,
    Notifications,
}

const COMMANDS: &[(&str, &str, Cmd)] = &[
    ("Switch to a branch or a merge request…", "⌘B", Cmd::Switch),
    (
        "Notifications — mentions, reviews, replies",
        "⌘⇧N",
        Cmd::Notifications,
    ),
    ("Compare two refs…", "⌘⇧C", Cmd::Compare),
    ("Create merge request…", "⌥⌘M", Cmd::CreateRequest),
    (
        "Send the comments to an agent's chat…",
        "⇧⌘A",
        Cmd::SendToAgent,
    ),
    ("Keyboard shortcuts", "?", Cmd::Shortcuts),
    ("Show or hide the Discussions window", "⌘⇧R", Cmd::Comments),
    ("Show or hide the request's details", "⌘I", Cmd::Details),
    ("Find in the diff", "⌘F", Cmd::FindInDiff),
    ("Search in all files of the diff", "⌘⌥F", Cmd::FindInFiles),
    ("Annotate — who wrote each line", "⌥⌘B", Cmd::Annotate),
    ("Recent places", "⌘E", Cmd::Recent),
    (
        "Changes in this file — functions and types",
        "⌘⇧O",
        Cmd::Structure,
    ),
    ("Bookmarks", "⌘F3", Cmd::Bookmarks),
    ("Zoom in — bigger interface", "⌘+", Cmd::ZoomIn),
    ("Zoom out — smaller interface", "⌘−", Cmd::ZoomOut),
    ("Actual size", "⌘0", Cmd::ZoomReset),
    (
        "Changes in this file — functions and types",
        "⌘⇧O",
        Cmd::Structure,
    ),
    ("Search commits", "⌘⇧F", Cmd::FindCommits),
    ("Filter files by path", "⌘P", Cmd::FilterFiles),
    ("Refresh", "⌘R", Cmd::Refresh),
    ("Open a repository…", "⌘O", Cmd::Open),
    ("Back", "⌘[", Cmd::Back),
    ("Forward", "⌘]", Cmd::Forward),
    ("Toggle split / unified", "⌥U", Cmd::Unified),
    ("Cycle lines, words, structural", "⌥D", Cmd::CycleMode),
    (
        "Markdown files rendered, or their source",
        "M",
        Cmd::Rendered,
    ),
    ("Theme — try each on the whole window", "", Cmd::Theme),
    ("Toggle wrap", "⌥Z", Cmd::Wrap),
    ("Toggle full file", "⌥E", Cmd::FullContext),
    ("Toggle ignore whitespace", "⌥W", Cmd::Whitespace),
    (
        "All files in one scroll, or one file at a time",
        "⌥A",
        Cmd::OneFile,
    ),
    (
        "Expand all files, or collapse them all",
        "⌥⇧A",
        Cmd::ExpandAll,
    ),
    ("Comments as markdown or raw text", "⌥M", Cmd::RichComments),
    (
        "Leave the merge request — back to your branch",
        "⌥⌘L",
        Cmd::LeaveRequest,
    ),
    (
        "Mark all files reviewed, or unmark all",
        "⌥⇧V",
        Cmd::AllReviewed,
    ),
    (
        "Next merge request not reviewed yet",
        "⌥↓",
        Cmd::NextRequest,
    ),
    ("Previous merge request", "⌥↑", Cmd::PreviousRequest),
    ("Hide or show the side panel", "⌥⌘1", Cmd::ToggleSidebar),
    ("Fold or open Changes", "⌥⌘2", Cmd::ToggleFiles),
    ("Branches or merge requests", "⌥⌘3", Cmd::ToggleRequests),
    ("Diff only", "⌘.", Cmd::FocusDiff),
    ("New tab", "⌘T", Cmd::NewTab),
    ("Settings", "⌘,", Cmd::Settings),
];

#[derive(Clone)]
enum Pick {
    Mark { path: String, line: u32 },
    Symbol { old: bool, line: u32 },
    Recent(super::recents::Key),
    Commit(Oid),
    File(usize),
    Branch(usize),
    Request(u64),
    Link(crate::links::Link),
    Command(Cmd),
    Ref(String, Oid),
    Theme(String),
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
    /// ⌘B: only what is not reviewed yet (⇥ switches).
    open_only: bool,
    items: Vec<Item>,
    refs: Vec<NamedRef>,
}

/// The words of `query` all occur in `hay`, ignoring case.
fn matches(words: &[String], hay: &str) -> bool {
    let hay = hay.to_lowercase();
    words.iter().all(|w| hay.contains(w.as_str()))
}

impl Workspace {
    /// ⌘B's list: the merge requests (when the project has them), then the local branches; each
    /// says how far its review got.
    fn switch_items(&self, query: &str, open_only: bool) -> Vec<Item> {
        let state = |head: Option<Oid>| match head {
            Some(h) if self.head_reviewed(h) => (true, "✓ reviewed".to_owned()),
            Some(h) => match self.head_part(h) {
                Some((done, total)) => (false, format!("{done}/{total} reviewed")),
                None => (false, "not reviewed".to_owned()),
            },
            None => (false, String::new()),
        };
        let requests = crate::rank::rank(self.requests.list.iter(), query, |m| &m.title)
            .into_iter()
            .chain(
                // `!12` or a branch name finds a request too.
                self.requests.list.iter().filter(|m| {
                    let q = query.trim().trim_start_matches('!');
                    !q.is_empty()
                        && (m.iid.to_string() == q || m.source.contains(q))
                        && crate::rank::score(query, &m.title).is_none()
                }),
            )
            .filter_map(|m| {
                let (done, said) = state(Oid::from_str(&m.head).ok());
                (!(open_only && done)).then(|| Item {
                    group: "Merge requests",
                    label: format!("{} {}", super::words::number(m.iid), m.title),
                    detail: format!("{} · {said}", m.source),
                    key: "",
                    pick: Pick::Request(m.iid),
                })
            });
        let branches = crate::rank::rank(
            self.branches
                .iter()
                .enumerate()
                .filter(|(_, b)| b.kind == crate::git::RefKind::Local),
            query,
            |(_, b)| &b.name,
        )
        .into_iter()
        .filter_map(|(ix, b)| {
            let (done, said) = state(Some(b.tip));
            (!(open_only && done)).then(|| Item {
                group: "Branches",
                label: b.name.clone(),
                detail: format!("{} · {said}", format::short(b.tip)),
                key: "",
                pick: Pick::Branch(ix),
            })
        });
        requests.take(10).chain(branches.take(10)).collect()
    }

    pub(super) fn open_palette(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let refs = match (&kind, &self.root) {
            (
                Kind::Jump
                | Kind::Recent
                | Kind::Structure
                | Kind::Bookmarks
                | Kind::Switch
                | Kind::Theme,
                _,
            )
            | (_, None) => Vec::new(),
            (_, Some(root)) => Repo::open(root)
                .and_then(|r| r.compare_refs())
                .unwrap_or_default(),
        };
        self.field = None;
        self.dfind = None;
        self.find.text = None;
        self.ctx_menu = None;
        self.repo_menu = false;
        self.reset_field_edit();
        let theme = matches!(kind, Kind::Theme);
        self.palette = Some(Palette {
            kind,
            query: String::new(),
            at: 0,
            open_only: false,
            items: Vec::new(),
            refs,
        });
        self.refresh_palette();
        // The themes open on the one in use.
        if theme && let Some(p) = &mut self.palette {
            let current = theme::choice().window;
            p.at = p
                .items
                .iter()
                .position(|i| matches!(&i.pick, Pick::Theme(id) if *id == current))
                .unwrap_or(0);
        }
        cx.notify();
    }

    pub(super) fn close_palette(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.palette.take() {
            // A theme being tried goes back to the chosen one.
            if matches!(p.kind, Kind::Theme) {
                theme::restore();
                cx.refresh_windows();
            }
            cx.notify();
        }
    }

    /// The theme under the selection, drawn on the whole window for now.
    fn try_theme(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &self.palette else {
            return;
        };
        if let Some(Pick::Theme(id)) = p.items.get(p.at).map(|i| &i.pick) {
            let mut choice = theme::choice();
            choice.window = id.clone();
            theme::preview(&choice);
            cx.refresh_windows();
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
        if matches!(p.kind, Kind::Theme) {
            let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
            let current = theme::choice().window;
            return theme::all()
                .into_iter()
                .filter(|t| matches(&words, &t.label))
                .map(|t| Item {
                    group: if t.built_in { "Themes" } else { "Your themes" },
                    detail: if t.id == current {
                        "current".to_owned()
                    } else if !t.problems.is_empty() {
                        super::plural(t.problems.len(), "problem")
                    } else if t.light() {
                        "light".to_owned()
                    } else {
                        String::new()
                    },
                    label: t.label,
                    key: "",
                    pick: Pick::Theme(t.id),
                })
                .collect();
        }
        if matches!(p.kind, Kind::Bookmarks) {
            let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
            return self
                .bookmark_list()
                .into_iter()
                .filter(|m| matches(&words, &format!("{} {}", m.path, m.text)))
                .map(|m| Item {
                    group: "Bookmarks",
                    label: format!(
                        "{}:{}",
                        m.path.rsplit('/').next().unwrap_or(&m.path),
                        m.line
                    ),
                    detail: m.text.clone(),
                    key: "",
                    pick: Pick::Mark {
                        path: m.path,
                        line: m.line,
                    },
                })
                .collect();
        }
        if matches!(p.kind, Kind::Structure) {
            let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
            let Some(file) = self.diff.as_ref().and_then(|d| d.files.get(self.file)) else {
                return Vec::new();
            };
            let name = file
                .path()
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            let group: &'static str = Box::leak(format!("Changes in {name}").into_boxed_str());
            return crate::symbols::changed(file)
                .into_iter()
                .filter(|s| matches(&words, &s.name))
                .map(|s| Item {
                    group,
                    label: format!("{}  {} {}", s.mark, s.kind, s.name),
                    detail: format!("{}line {}", if s.old { "old " } else { "" }, s.line),
                    key: "",
                    pick: Pick::Symbol {
                        old: s.old,
                        line: s.line,
                    },
                })
                .collect();
        }
        if matches!(p.kind, Kind::Switch) {
            return self.switch_items(query, p.open_only);
        }
        if matches!(p.kind, Kind::Recent) {
            let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
            return self
                .recents
                .iter()
                .filter(|r| matches(&words, &format!("{} {}", r.label, r.detail)))
                .take(14)
                .map(|r| Item {
                    group: "Recent",
                    label: r.label.clone(),
                    detail: r.detail.clone(),
                    key: "",
                    pick: Pick::Recent(r.key.clone()),
                })
                .collect();
        }
        if !matches!(p.kind, Kind::Jump) {
            let group = if matches!(p.kind, Kind::Base) {
                "Base"
            } else {
                "Compare with"
            };
            let mut ranked = crate::rank::rank(&p.refs, query, |r| &r.name);
            // A stale local branch comes after its upstream: that is the one meant.
            for i in 0..ranked.len() {
                if let Some(st) = &ranked[i].stale
                    && let Some(j) = ranked[i..].iter().position(|r| r.name == st.upstream)
                {
                    let upstream = ranked.remove(i + j);
                    ranked.insert(i, upstream);
                }
            }
            let mut items: Vec<Item> = ranked
                .into_iter()
                .take(14)
                .map(|r| Item {
                    group,
                    label: r.name.clone(),
                    detail: match &r.stale {
                        Some(st) => format!(
                            "branch {} · ↓{} behind {}",
                            format::short(r.tip),
                            st.behind,
                            st.upstream
                        ),
                        None => format!(
                            "{} {}",
                            if r.tag { "tag" } else { "branch" },
                            format::short(r.tip)
                        ),
                    },
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

        // A pasted GitLab link is one place to go.
        if let Some(link) = crate::links::parse(query) {
            return vec![Item {
                group: "Link",
                label: format!("Open {}", link.describe()),
                detail: link.project().to_owned(),
                key: "↵",
                pick: Pick::Link(link),
            }];
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
                crate::rank::rank(self.branches.iter().enumerate(), rest, |(_, b)| &b.name)
                    .into_iter()
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
                crate::rank::rank(diff.files.iter().enumerate(), rest, |(_, f)| {
                    f.path().rsplit('/').next().unwrap_or_default()
                })
                .into_iter()
                .chain(
                    // A word that only matches the folders of a path.
                    diff.files.iter().enumerate().filter(|(_, f)| {
                        matches(&words, f.path())
                            && crate::rank::score(
                                rest,
                                f.path().rsplit('/').next().unwrap_or_default(),
                            )
                            .is_none()
                    }),
                )
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
        if event.keystroke.key == "tab" && matches!(palette.kind, Kind::Switch) {
            palette.open_only = !palette.open_only;
            palette.at = 0;
            self.refresh_palette();
            return cx.notify();
        }
        match input::edit(
            &mut palette.query,
            &mut self.field_edit,
            &event.keystroke,
            cx,
        ) {
            Edit::Changed => {
                palette.at = 0;
                self.refresh_palette();
                if matches!(self.palette.as_ref().map(|p| &p.kind), Some(Kind::Theme)) {
                    self.try_theme(cx);
                }
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
                    return self.close_palette(cx);
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
            if matches!(p.kind, Kind::Theme) {
                self.try_theme(cx);
            }
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
            Pick::Recent(key) => {
                self.palette = None;
                self.open_recent(key, cx);
            }
            Pick::Mark { path, line } => {
                self.palette = None;
                self.jump_to_line(&path, line, false, cx);
            }
            Pick::Symbol { old, line } => {
                self.palette = None;
                if let Some(path) = self.current_path() {
                    self.jump_to_line(&path, line, old, cx);
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
                if self.requests.current.is_some() {
                    self.leave_request(cx);
                }
                self.select_branch(ix, None, cx);
            }
            Pick::Request(iid) => {
                self.palette = None;
                self.select_request(iid, cx);
            }
            Pick::Link(link) => {
                self.palette = None;
                self.open_link(link, cx);
            }
            Pick::Command(cmd) => {
                self.palette = None;
                cx.notify();
                window.dispatch_action(command_action(cmd), cx);
            }
            Pick::Theme(id) => {
                self.palette = None;
                let mut choice = theme::choice();
                choice.window = id;
                self.choose_theme(choice, cx);
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
            let repo = Repo::open(&root)?;
            let c = repo.compare(base.1, head.1, since_merge_base, settings)?;
            // Only a local branch that still points where it was picked can be stale.
            let stale = repo
                .stale(&base.0)
                .filter(|_| repo.resolve(&base.0).ok() == Some(base.1));
            Ok(Diff {
                header: Header::Compare {
                    base,
                    head,
                    since_merge_base,
                    start: c.start,
                    commits: c.commits,
                    request,
                    stale,
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
            Kind::Recent => "Recent places — files, commits, merge requests",
            Kind::Structure => "Functions and types this file changes",
            Kind::Bookmarks => "Bookmarks — lines marked with F3",
            Kind::Switch if p.open_only => "Switch to — only what is not reviewed yet   ⇥ all",
            Kind::Switch => "Switch to a branch or a merge request   ⇥ only not reviewed",
            Kind::Base => "Compare — pick the base: a branch, a tag or a commit",
            Kind::Head(_) => "Compare — pick what to compare with it",
            Kind::Theme => "Theme — ↑↓ tries it on the whole window, ↵ keeps, esc goes back",
        };
        let title = match &p.kind {
            Kind::Jump
            | Kind::Recent
            | Kind::Structure
            | Kind::Bookmarks
            | Kind::Switch
            | Kind::Theme => None,
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
                        .child(super::words::say(item.group).to_uppercase())
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
                            .child(super::keys::text(&item.label).into_owned()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(item.detail.clone()),
                    )
                    .children({
                        // A command whose key was changed in Settings → Keyboard shows the new one.
                        let key = match item.pick {
                            Pick::Command(cmd) => cx
                                .global::<super::keymap::Keymap>()
                                .shown(command_action(cmd).as_ref())
                                .unwrap_or_else(|| item.key.to_owned()),
                            _ => item.key.to_owned(),
                        };
                        (!key.is_empty()).then(|| super::keys::caps(&key, super::keys::On::Plain))
                    })
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
                    Kind::Recent => "Nothing opened yet.",
                    Kind::Bookmarks => "No bookmarks yet: click a line and press F3.",
                    Kind::Structure => "No function or type of this file is changed (or the language is not known).",
                    Kind::Jump if p.query.is_empty() => "Type to search.",
                    Kind::Jump => "Nothing matches.",
                    Kind::Switch if p.open_only => "Nothing left to review here.",
                    Kind::Switch => "No branch or merge request by that name.",
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
                                .child(self.field_text(&p.query, true, placeholder, cx)),
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
        Cmd::OneFile => Box::new(ToggleOneFile),
        Cmd::ExpandAll => Box::new(ToggleExpandAll),
        Cmd::RichComments => Box::new(ToggleRichComments),
        Cmd::Rendered => Box::new(ToggleRendered),
        Cmd::Theme => Box::new(PickTheme),
        Cmd::LeaveRequest => Box::new(LeaveRequest),
        Cmd::AllReviewed => Box::new(ToggleAllReviewed),
        Cmd::NextRequest => Box::new(NextRequest),
        Cmd::PreviousRequest => Box::new(PreviousRequest),
        Cmd::Compare => Box::new(Compare),
        Cmd::NewTab => Box::new(NewTab),
        Cmd::CreateRequest => Box::new(CreateRequest),
        Cmd::SendToAgent => Box::new(super::SendToAgent),
        Cmd::Shortcuts => Box::new(ShowShortcuts),
        Cmd::Comments => Box::new(ToggleComments),
        Cmd::Details => Box::new(ToggleDetails),
        Cmd::FindInFiles => Box::new(FindInFiles),
        Cmd::Annotate => Box::new(ToggleAnnotate),
        Cmd::Recent => Box::new(OpenRecent),
        Cmd::Structure => Box::new(ShowStructure),
        Cmd::Bookmarks => Box::new(ShowBookmarks),
        Cmd::ZoomIn => Box::new(ZoomIn),
        Cmd::ZoomOut => Box::new(ZoomOut),
        Cmd::ZoomReset => Box::new(ZoomReset),
        Cmd::Switch => Box::new(SwitchTo),
        Cmd::Notifications => Box::new(ToggleNotifications),
    }
}
