//! Search everywhere (#52, Design `docs/specs/command-palette.md`): one window with a tab per
//! scope — All ⌘K, Files ⌘P, Commits ⌘⇧C, Branches ⌘⇧B, Requests ⌘⇧R, Actions ⌘⇧P —, each
//! named with what it found; ⇥ ⇧⇥ move between them keeping the text, `>` typed moves to Actions.
//! The same overlay picks the refs of a comparison (⌥⌘C), a theme, a recent place, a bookmark.

use super::input::{self, Edit};
use super::px;
use super::spaces::Space;
use super::{
    Diff, Header, ISLAND_RADIUS, ROW_RADIUS, Selection, Workspace, format, island_label, theme,
};
use crate::git::{NamedRef, Repo};
use git2::Oid;
use gpui::{Action, Context, KeyDownEvent, MouseButton, Window, div, prelude::*};
use std::sync::Arc;

/// What the palette is for.
/// A tab of search everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    Files,
    Commits,
    Branches,
    Requests,
    Actions,
}

impl Scope {
    pub const ALL: [Scope; 6] = [
        Scope::All,
        Scope::Files,
        Scope::Commits,
        Scope::Branches,
        Scope::Requests,
        Scope::Actions,
    ];

    fn label(self) -> &'static str {
        match self {
            Scope::All => "All",
            Scope::Files => "Files",
            Scope::Commits => "Commits",
            Scope::Branches => "Branches",
            Scope::Requests => "Requests",
            Scope::Actions => "Actions",
        }
    }

    fn action(self) -> Box<dyn Action> {
        use super::*;
        match self {
            Scope::All => Box::new(SearchEverywhere),
            Scope::Files => Box::new(SearchFiles),
            Scope::Commits => Box::new(SearchCommits),
            Scope::Branches => Box::new(SearchBranches),
            Scope::Requests => Box::new(SearchRequests),
            Scope::Actions => Box::new(SearchActions),
        }
    }

    /// Its key as shipped, for the footer when the keymap does not say another.
    fn key(self) -> &'static str {
        match self {
            Scope::All => "⌘K",
            Scope::Files => "⌘P",
            Scope::Commits => "⌘⇧C",
            Scope::Branches => "⌘⇧B",
            Scope::Requests => "⌘⇧R",
            Scope::Actions => "⌘⇧P",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Scope::All => "Search everywhere — files, commits, branches, requests, actions",
            Scope::Files => "Files — this diff's first, then the repository's",
            Scope::Commits => "Commits — by words of the message, the author or the id",
            Scope::Branches => "Branches",
            Scope::Requests => "Merge requests — by title or number",
            Scope::Actions => "Actions — everything the app does, with its key",
        }
    }
}

#[derive(Clone)]
pub enum Kind {
    /// Search everywhere, on one tab.
    Search(Scope),
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
    /// Open a repository (⌘T, the rail's +): the ones opened before, filtered as you type, a path
    /// typed, and Browse for the system's picker (the developer, 2026-10-23: not a new tab's
    /// welcome page, which had no way back).
    Projects,
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
    OpenIn,
    Reveal,
    Shortcuts,
    Comments,
    Agents,
    FoldMenu,
    Details,
    NextSection,
    FindInFiles,
    Annotate,
    Recent,
    Structure,
    Bookmarks,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Switch,
    Space(Space),
}

const COMMANDS: &[(&str, &str, Cmd)] = &[
    ("Inbox", "⌃⌘1", Cmd::Space(Space::Inbox)),
    ("Code", "⌃⌘2", Cmd::Space(Space::Code)),
    ("Branches", "⌃⌘3", Cmd::Space(Space::Branches)),
    ("Merge requests", "⌃⌘4", Cmd::Space(Space::Requests)),
    ("Switch to a branch or a merge request…", "", Cmd::Switch),
    ("Compare two refs…", "⌥⌘C", Cmd::Compare),
    ("Create merge request…", "⌥⌘M", Cmd::CreateRequest),
    (
        "Send the comments to an agent's chat…",
        "⌥⌘A",
        Cmd::SendToAgent,
    ),
    ("Show or hide the Agents window", "⇧⌘A", Cmd::Agents),
    (
        "Open in its app — the file or the folder",
        "⌥⌘O",
        Cmd::OpenIn,
    ),
    ("Reveal in Finder", "⌥⌘R", Cmd::Reveal),
    ("Fold the menu to icons", "⌘B", Cmd::FoldMenu),
    ("Keyboard shortcuts", "?", Cmd::Shortcuts),
    ("Show or hide the Discussions window", "⌘⇧D", Cmd::Comments),
    ("Show the request's overview", "⌘I", Cmd::Details),
    ("Next section of the request", "⌃⇥", Cmd::NextSection),
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
    ("Filter files by path", "⌥⌘P", Cmd::FilterFiles),
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
    ("Hide or show the side panel", "⌘J", Cmd::ToggleSidebar),
    ("Fold or open Changes", "⌥⌘2", Cmd::ToggleFiles),
    ("Branches or merge requests", "⌥⌘3", Cmd::ToggleRequests),
    ("Diff only", "⌘.", Cmd::FocusDiff),
    ("New tab", "⌘T", Cmd::NewTab),
    ("Settings", "⌘,", Cmd::Settings),
];

#[derive(Clone)]
enum Pick {
    Mark {
        path: String,
        line: u32,
    },
    Symbol {
        old: bool,
        line: u32,
    },
    Recent(super::recents::Key),
    Commit(Oid),
    File(usize),
    Branch(usize),
    Request(u64),
    /// A file of the repository the diff does not change.
    RepoFile(String),
    Link(crate::links::Link),
    Command(Cmd),
    Ref(String, Oid),
    Theme(String),
    /// A repository to open in a tab of its own, or go to when one has it.
    Project(std::path::PathBuf),
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
    /// The tab `>` moved away from: ⌫ in the empty field goes back to it.
    from: Option<Scope>,
    /// What each tab found for the text, in `Scope::ALL`'s order (with text typed).
    counts: Option<[usize; 6]>,
    /// The repository's files, read when search everywhere opened.
    files: Arc<Vec<String>>,
    query: String,
    at: usize,
    /// ⌘B: only what is not reviewed yet (⇥ switches).
    open_only: bool,
    items: Vec<Item>,
    refs: Vec<NamedRef>,
}

/// Open a repository's list: a folder typed as a path, the ones opened before ranked by the
/// typed words (the one on screen marked), and Browse.
fn project_items(
    query: &str,
    recent: &[std::path::PathBuf],
    here: Option<&std::path::Path>,
) -> Vec<Item> {
    let mut items = Vec::new();
    let typed = query.trim();
    let expanded = match typed.strip_prefix('~') {
        Some(rest) => std::env::home_dir().map(|h| h.join(rest.trim_start_matches('/'))),
        None => Some(std::path::PathBuf::from(typed)),
    };
    if typed.starts_with(['/', '~'])
        && let Some(path) = expanded.filter(|p| p.is_dir())
    {
        items.push(Item {
            group: "Folder",
            label: path.display().to_string(),
            detail: String::new(),
            key: "↵",
            pick: Pick::Project(path),
        });
    }
    let named: Vec<(String, &std::path::PathBuf)> = recent
        .iter()
        .map(|p| {
            let name = p.file_name().map_or_else(
                || p.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            (name, p)
        })
        .collect();
    let ranked: Vec<&(String, &std::path::PathBuf)> = if typed.is_empty() {
        named.iter().collect()
    } else {
        crate::rank::rank(named.iter(), typed, |(name, _)| name.as_str())
    };
    items.extend(ranked.into_iter().map(|(name, path)| Item {
        group: "Opened before",
        label: name.clone(),
        detail: if here == Some(path.as_path()) {
            "on screen".to_owned()
        } else {
            super::format::home_relative(path)
        },
        key: "",
        pick: Pick::Project((*path).clone()),
    }));
    items.push(Item {
        group: "On disk",
        label: "Browse…".to_owned(),
        detail: String::new(),
        key: "⌘O",
        pick: Pick::Command(Cmd::Open),
    });
    items
}

/// A file's name, which leads, and its folder, which follows it fainter — as IDEs list files.
fn name_and_folder(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((folder, name)) => (name.to_owned(), folder.to_owned()),
        None => (path.to_owned(), String::new()),
    }
}

/// The words of `query` all occur in `hay`, ignoring case — or as the starts of its words.
fn matches(words: &[String], hay: &str) -> bool {
    words.iter().all(|w| crate::rank::word_matches(w, hay))
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
                        && (m.iid.to_string() == q || crate::rank::word_matches(q, &m.source))
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
                Kind::Search(_)
                | Kind::Recent
                | Kind::Structure
                | Kind::Bookmarks
                | Kind::Switch
                | Kind::Theme
                | Kind::Projects,
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
        let files = match (&kind, &self.root) {
            // Code's own: the tree it shows, a plain folder's files on disk (#75).
            (Kind::Search(_), Some(_)) if !self.code.files().is_empty() => {
                Arc::new(self.code.files().to_vec())
            }
            (Kind::Search(_), Some(root)) if self.plain => {
                Arc::new(crate::files::on_disk(root, 200_000))
            }
            (Kind::Search(_), Some(root)) => Arc::new(
                Repo::open(root)
                    .map(|r| r.all_files(200_000))
                    .unwrap_or_default(),
            ),
            _ => Arc::default(),
        };
        self.palette = Some(Palette {
            kind,
            from: None,
            counts: None,
            files,
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
        // Every tab's count, while something is typed; All's is what the others found (X56).
        let counts = match palette.kind {
            Kind::Search(_) if !palette.query.trim().is_empty() => {
                let mut counts = [0; 6];
                for (i, scope) in Scope::ALL.into_iter().enumerate() {
                    if scope != Scope::All {
                        counts[i] = self.scope_items(scope, palette).len();
                    }
                }
                let all = Scope::ALL
                    .iter()
                    .position(|&s| s == Scope::All)
                    .unwrap_or(0);
                counts[all] = counts.iter().sum();
                Some(counts)
            }
            _ => None,
        };
        if let Some(p) = &mut self.palette {
            p.at = p.at.min(items.len().saturating_sub(1));
            p.items = items;
            p.counts = counts;
        }
    }

    /// Opens search everywhere on `scope`, or moves to that tab when it is open (the text stays);
    /// the same key again closes it.
    pub(super) fn search(&mut self, scope: Scope, cx: &mut Context<Self>) {
        match self.palette.as_mut() {
            Some(p) if matches!(p.kind, Kind::Search(s) if s == scope) => self.close_palette(cx),
            Some(p) if matches!(p.kind, Kind::Search(_)) => {
                p.kind = Kind::Search(scope);
                p.from = None;
                p.at = 0;
                self.refresh_palette();
                cx.notify();
            }
            _ => self.open_palette(Kind::Search(scope), cx),
        }
    }

    /// The next tab (or the one before), the text kept.
    fn step_scope(&mut self, back: bool, cx: &mut Context<Self>) {
        let Some(p) = &mut self.palette else {
            return;
        };
        let Kind::Search(scope) = p.kind else {
            return;
        };
        let at = Scope::ALL.iter().position(|s| *s == scope).unwrap_or(0);
        let n = Scope::ALL.len();
        p.kind = Kind::Search(Scope::ALL[if back { (at + n - 1) % n } else { (at + 1) % n }]);
        p.from = None;
        p.at = 0;
        self.refresh_palette();
        cx.notify();
    }

    /// What tab `scope` lists for the palette's text.
    fn scope_items(&self, scope: Scope, p: &Palette) -> Vec<Item> {
        let query = p.query.trim();
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        match scope {
            Scope::All => self.all_items(query, &p.files),
            Scope::Actions => COMMANDS
                .iter()
                .filter(|(label, ..)| matches(&words, label))
                .map(|&(label, key, cmd)| Item {
                    group: "Actions",
                    label: label.to_owned(),
                    detail: String::new(),
                    key,
                    pick: Pick::Command(cmd),
                })
                .collect(),
            Scope::Branches => {
                crate::rank::rank(self.branches.iter().enumerate(), query, |(_, b)| &b.name)
                    .into_iter()
                    .take(60)
                    .map(|(ix, b)| Item {
                        group: "Branches",
                        label: b.name.clone(),
                        detail: format::short(b.tip),
                        key: "",
                        pick: Pick::Branch(ix),
                    })
                    .collect()
            }
            Scope::Commits => self
                .commits
                .iter()
                .filter(|c| {
                    words.is_empty()
                        || matches(&words, &c.summary)
                        || matches(&words, &c.author)
                        || c.id.to_string().starts_with(&words[0])
                })
                .take(60)
                .map(|c| Item {
                    group: "Commits",
                    label: c.summary.clone(),
                    detail: format!("{} · {}", format::short(c.id), c.author),
                    key: "",
                    pick: Pick::Commit(c.id),
                })
                .collect(),
            Scope::Requests => {
                let number = query.trim_start_matches(['!', '#']);
                self.requests
                    .list
                    .iter()
                    .filter(|m| {
                        words.is_empty()
                            || matches(&words, &m.title)
                            || matches(&words, &m.author)
                            || m.iid.to_string() == number
                    })
                    .take(60)
                    .map(|m| Item {
                        group: "Merge requests",
                        label: format!("{} {}", super::words::number(m.iid), m.title),
                        detail: m.author.clone(),
                        key: "",
                        pick: Pick::Request(m.iid),
                    })
                    .collect()
            }
            Scope::Files => self.file_items(query, &p.files),
        }
    }

    fn palette_items(&self, p: &Palette) -> Vec<Item> {
        let query = p.query.trim_start();
        if matches!(p.kind, Kind::Projects) {
            return project_items(query, &crate::storage::recent(), self.root.as_deref());
        }
        if matches!(p.kind, Kind::Theme) {
            let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
            let current = theme::choice().window;
            return theme::all()
                .into_iter()
                .filter(|t| t.window && matches(&words, &t.label))
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
        if let Kind::Search(scope) = p.kind {
            return self.scope_items(scope, p);
        }
        let group = if matches!(p.kind, Kind::Base) {
            "Base"
        } else {
            "Compare with"
        };
        {
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
            items
        }
    }

    /// The All tab: a few of every kind, a pasted link as one place, `#` `@` narrowing.
    /// The Files tab's finds: this diff's files first, then the repository's.
    fn file_items(&self, query: &str, files: &[String]) -> Vec<Item> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let name = |path: &str| path.rsplit('/').next().unwrap_or_default().to_owned();
        let mut items: Vec<Item> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        if let Some(diff) = &self.diff {
            for (ix, f) in crate::rank::rank(diff.files.iter().enumerate(), query, |(_, f)| {
                f.path().rsplit('/').next().unwrap_or_default()
            })
            .into_iter()
            .chain(diff.files.iter().enumerate().filter(|(_, f)| {
                !words.is_empty()
                    && matches(&words, f.path())
                    && crate::rank::score(query, &name(f.path())).is_none()
            })) {
                if seen.insert(f.path().to_owned()) {
                    let (label, detail) = name_and_folder(f.path());
                    items.push(Item {
                        group: "In this diff",
                        label,
                        detail,
                        key: "",
                        pick: Pick::File(ix),
                    });
                }
            }
        }
        if !words.is_empty() {
            let ranked = crate::rank::rank(files.iter(), query, |f| {
                f.rsplit('/').next().unwrap_or_default()
            });
            items.extend(
                ranked
                    .into_iter()
                    .chain(files.iter().filter(|f| {
                        matches(&words, f) && crate::rank::score(query, &name(f)).is_none()
                    }))
                    .filter(|f| seen.insert((*f).clone()))
                    .take(60)
                    .map(|f| {
                        let (label, detail) = name_and_folder(f);
                        Item {
                            group: "In the repository",
                            label,
                            detail,
                            key: "",
                            pick: Pick::RepoFile(f.clone()),
                        }
                    }),
            );
        }
        items
    }

    fn all_items(&self, query: &str, files: &[String]) -> Vec<Item> {
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
        // The Files tab's first finds: this diff's, then the repository's (X56).
        if scope.is_none_or(|s| s == '@') && !words.is_empty() | scope.is_some() {
            items.extend(
                self.file_items(rest, files)
                    .into_iter()
                    .take(cap(3))
                    .map(|i| Item {
                        group: "Files",
                        ..i
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
            let number = rest.trim_start_matches(['!', '#']);
            items.extend(
                self.requests
                    .list
                    .iter()
                    .filter(|m| matches(&words, &m.title) || m.iid.to_string() == number)
                    .take(3)
                    .map(|m| Item {
                        group: "Merge requests",
                        label: format!("{} {}", super::words::number(m.iid), m.title),
                        detail: m.author.clone(),
                        key: "",
                        pick: Pick::Request(m.iid),
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
        if event.keystroke.key == "tab" && matches!(palette.kind, Kind::Search(_)) {
            return self.step_scope(event.keystroke.modifiers.shift, cx);
        }
        // ⌫ in the empty field goes back from Actions to the tab ">" left.
        if event.keystroke.key == "backspace"
            && palette.query.is_empty()
            && let Some(from) = palette.from.take()
        {
            palette.kind = Kind::Search(from);
            self.refresh_palette();
            return cx.notify();
        }
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
                // ">" typed first moves to Actions, shown as a chip in the field.
                if let Kind::Search(scope) = palette.kind
                    && scope != Scope::Actions
                    && palette.query.starts_with('>')
                {
                    palette.query = palette.query[1..].trim_start().to_owned();
                    palette.kind = Kind::Search(Scope::Actions);
                    palette.from = Some(scope);
                    self.reset_field_edit();
                }
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
                        let head = (name, id);
                        self.arrive(super::nav::Page::Compare(base.clone(), head.clone()));
                        self.run_compare(base, head, true, None, cx);
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
            // A result opens in its space (#71): a commit or a file of what is loaded on its page.
            Pick::Commit(id) => {
                self.palette = None;
                if let Some(ix) = self.commits.iter().position(|c| c.id == id) {
                    self.select_commit(ix, cx);
                    self.follow_loaded();
                }
            }
            // In Code a file opens there, as a tab, changed or not (#75).
            Pick::File(ix) if self.nav.space == super::spaces::Space::Code => {
                self.palette = None;
                if let Some(path) = self
                    .diff
                    .as_ref()
                    .and_then(|d| d.files.get(ix))
                    .map(|f| f.path().to_owned())
                {
                    self.open_file_view(path, cx);
                }
            }
            Pick::File(ix) => {
                self.palette = None;
                self.select_file(ix, cx);
                self.follow_loaded();
            }
            Pick::Branch(ix) => {
                self.palette = None;
                self.open_branch(ix, None, cx);
            }
            Pick::Request(iid) => {
                self.palette = None;
                self.select_request(iid, cx);
            }
            Pick::RepoFile(path) => {
                self.palette = None;
                // Not changed here: read in Code (D18, #71).
                self.open_file_view(path, cx);
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
            // A tab of its own, as + means; one that has it already comes on screen.
            Pick::Project(path) => {
                self.palette = None;
                match self.root {
                    None => self.request_open(path, None, cx),
                    Some(_) => self.open_in_new_tab(path, None, cx),
                }
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
        // The whole request is its page; a commit or versions of it were (#71).
        if request.is_some() {
            self.record();
        }
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
            Kind::Search(scope) => scope.placeholder(),
            Kind::Recent => "Recent places — files, commits, merge requests",
            Kind::Structure => "Functions and types this file changes",
            Kind::Bookmarks => "Bookmarks — lines marked with F3",
            Kind::Switch if p.open_only => "Switch to — only what is not reviewed yet   ⇥ all",
            Kind::Switch => "Switch to a branch or a merge request   ⇥ only not reviewed",
            Kind::Base => "Compare — pick the base: a branch, a tag or a commit",
            Kind::Head(_) => "Compare — pick what to compare with it",
            Kind::Theme => "Theme — ↑↓ tries it on the whole window, ↵ keeps, esc goes back",
            Kind::Projects => "Open a repository — type to filter, or a path",
        };
        let title = match &p.kind {
            Kind::Search(_)
            | Kind::Recent
            | Kind::Structure
            | Kind::Bookmarks
            | Kind::Switch
            | Kind::Theme => None,
            Kind::Projects => Some("Open a repository".to_owned()),
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
            // A file or a repository: its name, then its folder right after it, cut first.
            let file = matches!(
                item.pick,
                Pick::File(_) | Pick::RepoFile(_) | Pick::Project(_)
            );
            // A file's icon before it (#95).
            let path = match &item.pick {
                Pick::File(f) => self
                    .diff
                    .as_ref()
                    .and_then(|d| d.files.get(*f))
                    .map(|f| f.path().to_owned()),
                Pick::RepoFile(path) => Some(path.clone()),
                _ => None,
            };
            rows.push(
                super::row(("palette-item", ix), selected)
                    .h(px(28.))
                    .gap_3()
                    .children(path.map(|p| super::icons::file_icon(&p, 16.)))
                    .child(
                        div()
                            .when(!file, |s| s.flex_1())
                            .when(file, |s| s.flex_none().max_w(px(320.)))
                            .min_w_0()
                            .truncate()
                            .child(super::keys::text(&item.label).into_owned()),
                    )
                    .child(
                        div()
                            .when(!file, |s| s.flex_none())
                            .when(file, |s| s.flex_1().min_w_0().truncate())
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
                    Kind::Projects => "None by that name. Browse… finds it on disk.",
                    Kind::Bookmarks => "No bookmarks yet: click a line and press F3.",
                    Kind::Structure => "No function or type of this file is changed (or the language is not known).",
                    Kind::Search(_) if p.query.is_empty() => "Type to search.",
                    Kind::Search(_) => "Nothing matches.",
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
                // It fades in as it grows from 98 % (#56, 6).
                .child(super::motion::appear(
                    div()
                        .occlude()
                        .flex()
                        .flex_col()
                        .rounded(px(ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .overflow_hidden()
                        .children(title.map(island_label))
                        .children(self.render_scopes(p, cx))
                        .child(
                            div()
                                .h(px(44.))
                                .px(px(14.))
                                .flex()
                                .items_center()
                                .gap_2()
                                .children(p.from.is_some().then(|| {
                                    div()
                                        .flex_none()
                                        .px(px(8.))
                                        .h(px(22.))
                                        .flex()
                                        .items_center()
                                        .rounded(px(ROW_RADIUS))
                                        .bg(theme::info_bg())
                                        .text_size(px(12.))
                                        .text_color(theme::accent())
                                        .child("> Actions")
                                }))
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
                        )
                        .children(self.render_search_foot(p, cx)),
                    "palette-coming",
                    super::motion::FAST,
                    |panel, v| panel.w(px(super::motion::mix(627., 640., v))).opacity(v),
                )),
        )
    }

    /// The tabs of search everywhere: names, and what each found in a badge; one that found
    /// nothing fades.
    fn render_scopes(&self, p: &Palette, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let Kind::Search(on) = p.kind else {
            return None;
        };
        Some(
            div()
                .flex()
                .gap(px(2.))
                .px(px(10.))
                .border_b_1()
                .border_color(theme::island_border())
                .children(Scope::ALL.into_iter().enumerate().map(|(i, scope)| {
                    let count = p.counts.map(|c| c[i]);
                    let none = count == Some(0);
                    let lit = scope == on;
                    let tab = div()
                        .id(("search-scope", i))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(10.))
                        .pt(px(10.))
                        .pb(px(8.))
                        .border_b_2()
                        .text_size(px(13.))
                        .text_color(if lit { theme::text() } else { theme::muted() })
                        .when(lit, |s| s.font_weight(gpui::FontWeight::SEMIBOLD))
                        .when(none && !lit, |s| s.opacity(0.6))
                        .cursor_pointer()
                        .child(super::words::say(scope.label()).into_owned())
                        .children(count.map(|n| {
                            div()
                                .min_w(px(18.))
                                .px(px(5.))
                                .rounded_full()
                                .flex()
                                .justify_center()
                                .text_size(px(11.))
                                .bg(if lit {
                                    theme::info_bg()
                                } else {
                                    theme::hover()
                                })
                                .text_color(if lit { theme::accent() } else { theme::faint() })
                                .child(n.to_string())
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| this.search(scope, cx)));
                    // The underline goes from the tab left to the tab come to (#56, 6).
                    super::motion::toward(
                        tab,
                        ("search-scope-line", i),
                        lit,
                        false,
                        super::motion::FAST,
                        |tab, v| {
                            tab.border_color(gpui::Rgba {
                                a: v,
                                ..theme::accent()
                            })
                        },
                    )
                })),
        )
        .map(|d| d.into_any_element())
    }

    /// The tab on with its key, and how to move.
    fn render_search_foot(&self, p: &Palette, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let Kind::Search(on) = p.kind else {
            return None;
        };
        let key = cx
            .global::<super::keymap::Keymap>()
            .shown(on.action().as_ref())
            .unwrap_or_else(|| on.key().to_owned());
        Some(
            div()
                .flex()
                .gap(px(14.))
                .px(px(14.))
                .py(px(8.))
                .border_t_1()
                .border_color(theme::island_border())
                .text_size(px(11.))
                .text_color(theme::muted())
                .child(format!(
                    "{} · {}",
                    super::words::say(on.label()),
                    super::keys::text(&key)
                ))
                .child("⇥ next tab · > actions")
                .child(div().flex_1())
                .child("↵ open · esc")
                .into_any_element(),
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
        // From ⌘K, the quick way: one question at a time (#47 D).
        Cmd::CreateRequest => Box::new(super::QuickCreateRequest),
        Cmd::SendToAgent => Box::new(super::SendToAgent),
        Cmd::OpenIn => Box::new(super::OpenIn),
        Cmd::Reveal => Box::new(super::RevealInFinder),
        Cmd::Shortcuts => Box::new(ShowShortcuts),
        Cmd::Comments => Box::new(ToggleComments),
        Cmd::Agents => Box::new(ToggleAgents),
        Cmd::FoldMenu => Box::new(FoldMenu),
        Cmd::Details => Box::new(ShowOverview),
        Cmd::NextSection => Box::new(NextSection),
        Cmd::FindInFiles => Box::new(FindInFiles),
        Cmd::Annotate => Box::new(ToggleAnnotate),
        Cmd::Recent => Box::new(OpenRecent),
        Cmd::Structure => Box::new(ShowStructure),
        Cmd::Bookmarks => Box::new(ShowBookmarks),
        Cmd::ZoomIn => Box::new(ZoomIn),
        Cmd::ZoomOut => Box::new(ZoomOut),
        Cmd::ZoomReset => Box::new(ZoomReset),
        Cmd::Switch => Box::new(SwitchTo),
        Cmd::Space(Space::Inbox) => Box::new(super::GoToInbox),
        Cmd::Space(Space::Code) => Box::new(super::GoToCode),
        Cmd::Space(Space::Branches) => Box::new(super::GoToBranches),
        Cmd::Space(Space::Requests) => Box::new(super::GoToMergeRequests),
    }
}
