//! Where the window is (#71). Each space keeps one page, and its stack follows from it: a page's
//! parent is the page above it (`Page::parent`) — Merge requests › !398 › a commit of it. Esc goes
//! one page up and a space's root has none above it; ⌘[ ⌘] walk every page seen, across spaces; a
//! page reached from another space by a link says where it came from (‹ Inbox). The requests and
//! branches opened stay under their space in the menu, each with the page, file and scroll it was
//! left at; × or ⌘W closes one. A project reopens on the space and pages it was left on
//! (`nav.txt`).
//!
//! The page decides what is loaded: `show` loads it when the loaded branch, selection or request
//! differ from it, and the places that open something (a list's row, ⌘K, a notification, a link)
//! say where they went through `arrive`.

use super::spaces::Space;
use super::{Header, Selection, Workspace, zones};
use git2::Oid;
use gpui::{Context, ListOffset};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Page {
    /// A file read in Code.
    File(String),
    /// A branch, by its refname: off the main line its Commits · Changes (#74), the main line its
    /// commits with one of them shown.
    Branch(String),
    /// A commit of a branch off the main line: a level under its Commits (#74).
    BranchCommit(String, Oid),
    /// A run of a branch's commits picked with shift, newest and oldest: one diff, a level under
    /// its Commits.
    BranchCommits(String, Oid, Oid),
    /// Two versions of a branch compared.
    Versions(String, usize, usize),
    /// What is not committed, against `HEAD`.
    WorkingTree,
    /// Two refs compared (⌥⌘C): what the head changed against the base.
    Compare((String, Oid), (String, Oid)),
    Request(u64),
    /// A commit of a request.
    RequestCommit(u64, Oid),
    /// A run of a request's commits picked with shift, newest and oldest.
    RequestCommits(u64, Oid, Oid),
    /// Two versions of a request compared.
    RequestVersions(u64, usize, usize),
}

impl Page {
    pub fn space(&self) -> Space {
        match self {
            Page::File(_) => Space::Code,
            Page::Branch(_)
            | Page::BranchCommit(..)
            | Page::BranchCommits(..)
            | Page::Versions(..)
            | Page::WorkingTree
            | Page::Compare(..) => Space::Branches,
            Page::Request(_)
            | Page::RequestCommit(..)
            | Page::RequestCommits(..)
            | Page::RequestVersions(..) => Space::Requests,
        }
    }

    /// The page one up; None when that is the space's root.
    pub fn parent(&self) -> Option<Page> {
        match self {
            Page::Versions(refname, ..)
            | Page::BranchCommit(refname, _)
            | Page::BranchCommits(refname, ..) => Some(Page::Branch(refname.clone())),
            Page::RequestCommit(iid, _)
            | Page::RequestCommits(iid, ..)
            | Page::RequestVersions(iid, ..) => Some(Page::Request(*iid)),
            Page::File(_)
            | Page::Branch(_)
            | Page::WorkingTree
            | Page::Compare(..)
            | Page::Request(_) => None,
        }
    }

    /// The request or branch it belongs to, kept open under its space.
    pub fn item(&self) -> Option<Item> {
        match self {
            Page::Branch(refname)
            | Page::BranchCommit(refname, _)
            | Page::BranchCommits(refname, ..)
            | Page::Versions(refname, ..) => Some(Item::Branch(refname.clone())),
            Page::Request(iid)
            | Page::RequestCommit(iid, _)
            | Page::RequestCommits(iid, ..)
            | Page::RequestVersions(iid, ..) => Some(Item::Request(*iid)),
            Page::File(_) | Page::WorkingTree | Page::Compare(..) => None,
        }
    }

    /// As `nav.txt` writes it.
    fn key(&self) -> String {
        match self {
            Page::File(path) => format!("file:{path}"),
            Page::Branch(refname) => format!("branch:{refname}"),
            Page::BranchCommit(refname, id) => format!("branch-commit:{id}:{refname}"),
            Page::BranchCommits(refname, a, b) => format!("branch-commits:{a}:{b}:{refname}"),
            Page::Versions(refname, a, b) => format!("versions:{a}:{b}:{refname}"),
            Page::WorkingTree => "working-tree".to_owned(),
            Page::Compare((base, a), (head, b)) => format!("compare:{a}:{b}:{base}:{head}"),
            Page::Request(iid) => format!("request:{iid}"),
            Page::RequestCommit(iid, id) => format!("request-commit:{iid}:{id}"),
            Page::RequestCommits(iid, a, b) => format!("request-commits:{iid}:{a}:{b}"),
            Page::RequestVersions(iid, a, b) => format!("request-versions:{iid}:{a}:{b}"),
        }
    }

    fn parse(key: &str) -> Option<Page> {
        let (kind, rest) = key.split_once(':').unwrap_or((key, ""));
        let mut parts = rest.splitn(4, ':');
        let mut next = || parts.next();
        Some(match kind {
            "file" => Page::File(rest.to_owned()),
            "branch" => Page::Branch(rest.to_owned()),
            "branch-commit" => {
                let (id, refname) = rest.split_once(':')?;
                Page::BranchCommit(refname.to_owned(), Oid::from_str(id).ok()?)
            }
            "branch-commits" => {
                let (a, b) = (Oid::from_str(next()?).ok()?, Oid::from_str(next()?).ok()?);
                Page::BranchCommits(next()?.to_owned(), a, b)
            }
            "versions" => {
                let (a, b) = (next()?.parse().ok()?, next()?.parse().ok()?);
                Page::Versions(next()?.to_owned(), a, b)
            }
            "working-tree" => Page::WorkingTree,
            "compare" => {
                let (a, b) = (Oid::from_str(next()?).ok()?, Oid::from_str(next()?).ok()?);
                Page::Compare((next()?.to_owned(), a), (next()?.to_owned(), b))
            }
            "request" => Page::Request(rest.parse().ok()?),
            "request-commit" => {
                Page::RequestCommit(next()?.parse().ok()?, Oid::from_str(next()?).ok()?)
            }
            "request-commits" => Page::RequestCommits(
                next()?.parse().ok()?,
                Oid::from_str(next()?).ok()?,
                Oid::from_str(next()?).ok()?,
            ),
            "request-versions" => Page::RequestVersions(
                next()?.parse().ok()?,
                next()?.parse().ok()?,
                next()?.parse().ok()?,
            ),
            _ => return None,
        })
    }
}

/// What stays open under a space in the menu.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Item {
    Request(u64),
    Branch(String),
}

impl Item {
    pub fn space(&self) -> Space {
        match self {
            Item::Request(_) => Space::Requests,
            Item::Branch(_) => Space::Branches,
        }
    }

    fn key(&self) -> String {
        match self {
            Item::Request(iid) => format!("request:{iid}"),
            Item::Branch(refname) => format!("branch:{refname}"),
        }
    }

    fn parse(key: &str) -> Option<Item> {
        match key.split_once(':')? {
            ("request", iid) => Some(Item::Request(iid.parse().ok()?)),
            ("branch", refname) => Some(Item::Branch(refname.to_owned())),
            _ => None,
        }
    }
}

/// A request's section tabs (#73): tabs, not levels — the request is one page, a commit of it a
/// level under Commits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Overview,
    Commits,
    Changes,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Overview, Section::Commits, Section::Changes];

    pub fn label(self) -> &'static str {
        match self {
            Section::Overview => "Overview",
            Section::Commits => "Commits",
            Section::Changes => "Changes",
        }
    }
}

/// A row of Branches' list: the working tree, or a branch (#72).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BranchRow {
    WorkingTree,
    Branch(usize),
}

/// A request or branch open under its space, and where it was left.
#[derive(Clone)]
pub struct Opened {
    pub item: Item,
    /// The page it was on.
    pub page: Page,
    /// The commit its branch page showed.
    commit: Option<Oid>,
    /// The file and scroll it was at.
    place: Option<(String, ListOffset)>,
    /// The files of its diff when it was last shown: an agent draft on one is unsent work here.
    pub paths: Vec<String>,
    /// A request's section tab (#73).
    pub section: Section,
}

/// What `nav.txt` keeps of a project.
struct Saved {
    repo: String,
    space: Space,
    pages: [Option<Page>; 4],
    open: Vec<Item>,
}

/// A page seen: ⌘[ ⌘] come back to it, with the commit a branch's page showed.
#[derive(Clone, PartialEq, Debug)]
struct Moment {
    space: Space,
    page: Option<Page>,
    commit: Option<Oid>,
}

pub struct Nav {
    pub space: Space,
    /// Each space's page; None at its root.
    pages: [Option<Page>; 4],
    time: Vec<Moment>,
    at: usize,
    /// The space a link came from, while its page is on screen (‹ Inbox).
    pub from: Option<Space>,
    pub open: Vec<Opened>,
    /// The spaces whose open items the menu folds away (Branches at first).
    pub folded: [bool; 4],
    /// A page waiting for what it needs: a request's commits or versions, the request list.
    pending: Option<Page>,
    /// A space and page to show once the request list is in (a project reopened on one).
    pending_show: Option<(Space, Option<Page>)>,
    /// The row a root list has selected: a request, a branch or the working tree.
    pub pick_request: Option<u64>,
    /// The commit a request's Commits section has selected (#73), and with shift the other end
    /// of a run of them.
    pub pick_commit: Option<Oid>,
    pub pick_to: Option<Oid>,
    pub pick_branch: Option<BranchRow>,
    /// The menu's open items picked with shift, the first the one it began from: ⌘W closes them.
    pub picked: Vec<Item>,
    /// Loading what a page needs, or coming back to a moment: no new moment, no link.
    restoring: bool,
}

impl Default for Nav {
    fn default() -> Self {
        Self {
            space: Space::Branches,
            pages: Default::default(),
            time: Vec::new(),
            at: 0,
            from: None,
            open: Vec::new(),
            folded: [false, false, true, false],
            pending: None,
            pending_show: None,
            pick_request: None,
            pick_commit: None,
            pick_branch: None,
            picked: Vec::new(),
            pick_to: None,
            restoring: false,
        }
    }
}

impl Nav {
    pub fn can_back(&self) -> bool {
        self.at > 0
    }

    pub fn can_forward(&self) -> bool {
        self.at + 1 < self.time.len()
    }

    /// A new moment; the ones ahead of it are dropped, the same one twice is one. A branch's page
    /// shown before its commit was in becomes the one with the commit.
    fn remember(&mut self, moment: Moment) {
        if let Some(last) = self.time.get_mut(self.at) {
            if *last == moment {
                return;
            }
            if last.space == moment.space && last.page == moment.page && last.commit.is_none() {
                *last = moment;
                return;
            }
        }
        let keep = (self.at + 1).min(self.time.len());
        self.time.truncate(keep);
        self.time.push(moment);
        // A long day is a long list: the oldest go.
        if self.time.len() > 200 {
            self.time.remove(0);
        }
        self.at = self.time.len() - 1;
    }

    fn opened(&self, item: &Item) -> Option<&Opened> {
        self.open.iter().find(|o| &o.item == item)
    }

    fn opened_mut(&mut self, item: &Item) -> Option<&mut Opened> {
        self.open.iter_mut().find(|o| &o.item == item)
    }

    /// `nav.txt`'s line for project `repo`: the space, each space's page, the open items.
    fn line(&self, repo: &str) -> String {
        let pages: Vec<String> = self
            .pages
            .iter()
            .map(|p| p.as_ref().map_or_else(|| "-".to_owned(), Page::key))
            .collect();
        let open: Vec<String> = self.open.iter().map(|o| o.item.key()).collect();
        format!(
            "{repo}\t{}\t{}\t{}",
            self.space.ix(),
            pages.join("\t"),
            open.join(" ")
        )
    }

    /// What `line` kept: the space, the pages and the open items.
    fn read(line: &str) -> Option<Saved> {
        let mut cols = line.split('\t');
        let repo = cols.next()?.to_owned();
        let space = Space::ALL
            .get(cols.next()?.parse::<usize>().ok()?)
            .copied()?;
        let mut pages: [Option<Page>; 4] = Default::default();
        for page in pages.iter_mut() {
            *page = Page::parse(cols.next()?);
        }
        let open = cols
            .next()
            .unwrap_or_default()
            .split(' ')
            .filter_map(Item::parse)
            .collect();
        Some(Saved {
            repo,
            space,
            pages,
            open,
        })
    }
}

const FILE: &str = "nav.txt";

impl Workspace {
    /// The page of the space on screen; None at its root.
    pub(super) fn nav_page(&self) -> Option<&Page> {
        self.nav.pages[self.nav.space.ix()].as_ref()
    }

    /// The page space `space` keeps.
    pub(super) fn page_of(&self, space: Space) -> Option<&Page> {
        self.nav.pages[space.ix()].as_ref()
    }

    /// At the root of Branches or Merge requests: their list is the page.
    pub(super) fn at_root_list(&self) -> bool {
        matches!(self.nav.space, Space::Branches | Space::Requests) && self.nav_page().is_none()
    }

    /// What is loaded, as a page of Branches or Merge requests.
    pub(super) fn loaded_page(&self) -> Option<Page> {
        let refname = self.branches.get(self.branch?)?.refname.clone();
        if let Some((_, _, iid)) = &self.open_request
            && self.requests.current == Some(*iid)
        {
            let iid = *iid;
            return Some(match self.selection {
                Selection::Commit(ix) => Page::RequestCommit(iid, self.commits.get(ix)?.id),
                Selection::Commits { newest, oldest } => Page::RequestCommits(
                    iid,
                    self.commits.get(newest)?.id,
                    self.commits.get(oldest)?.id,
                ),
                Selection::Versions { from, to } => Page::RequestVersions(iid, from, to),
                Selection::None | Selection::WorkingTree => Page::Request(iid),
            });
        }
        let tabbed = self.branch_tabbed(&refname);
        Some(
            match (self.selection, self.diff.as_ref().map(|d| &d.header)) {
                (Selection::WorkingTree, _) => Page::WorkingTree,
                (Selection::Versions { from, to }, _) => Page::Versions(refname, from, to),
                (Selection::Commit(ix), _) if tabbed => {
                    Page::BranchCommit(refname, self.commits.get(ix)?.id)
                }
                (Selection::Commits { newest, oldest }, _) => Page::BranchCommits(
                    refname,
                    self.commits.get(newest)?.id,
                    self.commits.get(oldest)?.id,
                ),
                (Selection::None, Some(Header::Compare { .. })) if self.branch_compare() => {
                    Page::Branch(refname)
                }
                (Selection::None, Some(Header::Compare { base, head, .. })) => {
                    Page::Compare(base.clone(), head.clone())
                }
                _ => Page::Branch(refname),
            },
        )
    }

    /// The commit the page shows, when it shows one.
    fn shown_commit(&self) -> Option<Oid> {
        match self.selection {
            Selection::Commit(ix) => self.commits.get(ix).map(|c| c.id),
            _ => None,
        }
    }

    /// Something was opened: its space and page come on screen, over the Inbox or Settings. From
    /// another space it was a link, and the page says where it came from.
    pub(super) fn arrive(&mut self, page: Page) {
        let space = page.space();
        if !self.nav.restoring && space != self.nav.space {
            self.nav.from = Some(self.nav.space);
        }
        self.inbox = space == Space::Inbox;
        self.settings_open = false;
        self.set_page(page);
    }

    /// The section a request opened anew starts on: Overview when you have never been inside it,
    /// else Changes (#73).
    fn first_section(&self, item: &Item) -> Section {
        match item {
            Item::Request(iid) if !self.opened_before(*iid) => Section::Overview,
            _ => Section::Changes,
        }
    }

    /// The section tabs of the page's item: a request's three (#73), a branch off the main line's
    /// Commits and Changes (#74); none for anything else.
    pub(super) fn item_sections(&self) -> &'static [Section] {
        match self.nav_page() {
            Some(
                Page::Request(_)
                | Page::RequestCommit(..)
                | Page::RequestCommits(..)
                | Page::RequestVersions(..),
            ) => &Section::ALL,
            Some(
                Page::Branch(refname)
                | Page::BranchCommit(refname, _)
                | Page::BranchCommits(refname, ..),
            ) if self.branch_tabbed(refname) => &[Section::Commits, Section::Changes],
            _ => &[],
        }
    }

    /// The section tab of the item on screen: a commit of it is under Commits, two versions
    /// compared under Changes.
    pub(super) fn page_section(&self) -> Option<Section> {
        if self.item_sections().is_empty() {
            return None;
        }
        let item = self.nav_page()?.item()?;
        Some(
            self.nav
                .opened(&item)
                .map_or(Section::Changes, |o| o.section),
        )
    }

    /// The item's tab on screen when it is the tabbed page itself, not a level under it.
    pub(super) fn page_tab(&self) -> Option<Section> {
        matches!(self.nav_page(), Some(Page::Request(_) | Page::Branch(_)))
            .then(|| self.page_section())
            .flatten()
    }

    /// Section `section` of the item on screen; from a level under it, the item's page again.
    pub(super) fn show_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if !self.item_sections().contains(&section) {
            return;
        }
        let Some(page) = self.nav_page().cloned() else {
            return;
        };
        let Some(item) = page.item() else {
            return;
        };
        if let Some(open) = self.nav.opened_mut(&item) {
            open.section = section;
        }
        let top = match &item {
            Item::Request(iid) => Page::Request(*iid),
            Item::Branch(refname) => Page::Branch(refname.clone()),
        };
        if page != top {
            self.show(page.space(), Some(top), cx);
        }
        cx.notify();
    }

    /// ⌃⇥ and ⌃⇧⇥: the next or the previous section tab, going round.
    pub(super) fn step_section(&mut self, forward: bool, cx: &mut Context<Self>) {
        // In Code they walk the files open as tabs (#75).
        if self.nav.space == Space::Code {
            return self.step_code_tab(forward, cx);
        }
        let (sections, Some(at)) = (self.item_sections(), self.page_section()) else {
            return;
        };
        let n = sections.len();
        let ix = sections.iter().position(|s| *s == at).unwrap_or(0);
        self.show_section(sections[(ix + if forward { 1 } else { n - 1 }) % n], cx);
    }

    /// `page` becomes its space's and the space the one on screen; the request or branch it is of
    /// stays open in the menu; a moment of time.
    fn set_page(&mut self, page: Page) {
        let space = page.space();
        self.keep_place();
        self.nav.space = space;
        self.nav.pages[space.ix()] = Some(page.clone());
        if let Some(item) = page.item() {
            // The commit shown is this item's only when it is the one loaded.
            let commit = self
                .shown_commit()
                .filter(|_| self.loaded_page().and_then(|p| p.item()).as_ref() == Some(&item));
            match self.nav.opened_mut(&item) {
                Some(open) => {
                    open.page = page.clone();
                    open.commit = commit.or(open.commit);
                }
                None => {
                    let section = self.first_section(&item);
                    self.nav.open.push(Opened {
                        item: item.clone(),
                        page: page.clone(),
                        commit,
                        place: None,
                        paths: Vec::new(),
                        section,
                    })
                }
            }
            // A commit of a request is a level under Commits, picked there when esc comes back to
            // it; two versions compared are Changes (#73).
            let section = match &page {
                Page::RequestCommit(_, id) | Page::BranchCommit(_, id) => {
                    self.nav.pick_commit = Some(*id);
                    self.nav.pick_to = None;
                    Some(Section::Commits)
                }
                // A run is picked there as it was, newest first.
                Page::RequestCommits(_, newest, oldest)
                | Page::BranchCommits(_, newest, oldest) => {
                    self.nav.pick_commit = Some(*newest);
                    self.nav.pick_to = Some(*oldest);
                    Some(Section::Commits)
                }
                Page::RequestVersions(..) => Some(Section::Changes),
                _ => None,
            };
            if let Some(section) = section
                && let Some(open) = self.nav.opened_mut(&item)
            {
                open.section = section;
            }
        }
        self.remember_moment();
        self.save_nav();
    }

    /// What is loaded comes on screen as its page, from wherever the window was (a ⌘K result, a
    /// link to a commit).
    pub(super) fn follow_loaded(&mut self) {
        if let Some(page) = self.loaded_page() {
            self.arrive(page);
        }
    }

    /// What is selected inside the open request or branch changed (a commit, versions): the page
    /// follows. Not while a request is still loading, nor while a page is being put back.
    pub(super) fn record(&mut self) {
        if self.pending_compare.is_some() || self.nav.restoring {
            return;
        }
        let Some(page) = self.loaded_page() else {
            return;
        };
        if page.space() == self.nav.space && page.item() == self.nav_page().and_then(Page::item) {
            self.set_page(page);
        }
    }

    fn remember_moment(&mut self) {
        if self.nav.restoring {
            return;
        }
        let moment = Moment {
            space: self.nav.space,
            page: self.nav_page().cloned(),
            commit: self
                .shown_commit()
                .filter(|_| matches!(self.nav_page(), Some(Page::Branch(_)))),
        };
        self.nav.remember(moment);
    }

    /// Before the loaded request or branch makes way: where it was, for coming back to it.
    fn keep_place(&mut self) {
        let Some(page) = self.loaded_page() else {
            return;
        };
        let Some(item) = page.item() else {
            return;
        };
        let place = self.scroll_place();
        let commit = self.shown_commit();
        let paths: Vec<String> = self
            .diff
            .as_ref()
            .map(|d| d.files.iter().map(|f| f.path().to_owned()).collect())
            .unwrap_or_default();
        if let Some(open) = self.nav.opened_mut(&item) {
            open.page = page;
            if place.is_some() {
                open.place = place;
            }
            open.commit = commit.or(open.commit);
            if !paths.is_empty() {
                open.paths = paths;
            }
        }
    }

    /// Shows `page` of `space`, loading what it needs. The menu, esc, ⌘[ ⌘] and the open items
    /// come here.
    pub(super) fn show(&mut self, space: Space, page: Option<Page>, cx: &mut Context<Self>) {
        self.keep_place();
        self.nav.space = space;
        self.nav.pages[space.ix()] = page.clone();
        self.nav.pending = None;
        self.inbox = space == Space::Inbox;
        self.settings_open = false;
        self.nav.restoring = true;
        match page {
            None => self.enter_root(space, cx),
            Some(page) => self.load_page(page, cx),
        }
        self.nav.restoring = false;
        self.remember_moment();
        self.save_nav();
        cx.notify();
    }

    /// A space's root: the Inbox's list, Code's project, the list of branches or requests.
    fn enter_root(&mut self, space: Space, cx: &mut Context<Self>) {
        match space {
            Space::Inbox => {
                self.notices.at = self.inbox_rows().first().copied().unwrap_or(0);
                self.refresh_notices(cx);
            }
            Space::Code => self.read_code_files(false, cx),
            Space::Branches => {
                self.nav.pick_branch = self.nav.pick_branch.or_else(|| self.first_branch_row());
                self.zone = zones::Zone::Branches;
                self.read_branch_facts(cx);
                self.read_tree_files(cx);
            }
            Space::Requests => {
                if self.nav.pick_request.is_none() {
                    self.nav.pick_request = self.listed_requests().first().map(|m| m.iid);
                }
                self.read_preview(cx);
                self.zone = zones::Zone::Branches;
                // A project without a token: the connect panel takes the keyboard.
                if self.requests.connectable {
                    self.start_field(super::lists::Field::Token, cx);
                }
            }
        }
    }

    /// Loads what `page` shows, unless it is what is loaded.
    fn load_page(&mut self, page: Page, cx: &mut Context<Self>) {
        let loaded = self.loaded_page();
        if page.space() == Space::Code {
            if let Page::File(path) = page
                && self.file_view.as_ref().map(|v| &v.path) != Some(&path)
            {
                self.open_file_view(path, cx);
            }
            return;
        }
        if loaded.as_ref() == Some(&page) {
            return;
        }
        let opened = page.item().and_then(|item| self.nav.opened(&item).cloned());
        let same_item =
            loaded.as_ref().and_then(Page::item) == page.item() && page.item().is_some();
        if !same_item {
            self.restore_scroll = opened.as_ref().and_then(|o| o.place.clone());
        }
        match page.clone() {
            Page::File(_) => {}
            Page::Branch(refname) if same_item && self.branch_tabbed(&refname) => {
                self.run_branch_compare(cx)
            }
            Page::BranchCommit(refname, id) => {
                if same_item && let Some(ix) = self.commits.iter().position(|c| c.id == id) {
                    self.select_commit(ix, cx);
                } else if let Some(ix) = self.branch_ix(&refname) {
                    self.select_branch(ix, Some(id), cx);
                }
            }
            Page::Branch(refname) => {
                let commit = opened.and_then(|o| o.commit);
                if same_item {
                    let ix = commit
                        .and_then(|id| self.commits.iter().position(|c| c.id == id))
                        .unwrap_or(0);
                    if !self.commits.is_empty() {
                        self.select_commit(ix, cx);
                    }
                } else if let Some(ix) = self.branch_ix(&refname) {
                    self.select_branch(ix, commit, cx);
                }
            }
            Page::Versions(refname, from, to) => {
                if same_item && to < self.versions.len() {
                    self.select_versions(from, to, cx);
                } else if let Some(ix) = self.branch_ix(&refname) {
                    self.nav.pending = Some(page);
                    self.select_branch(ix, None, cx);
                }
            }
            Page::WorkingTree => self.select_working_tree(cx),
            Page::Compare(base, head) => self.run_compare(base, head, true, None, cx),
            Page::Request(iid) => {
                if same_item {
                    if let Some((base, head, iid)) = self.open_request.clone() {
                        self.run_compare(base, head, true, Some(iid), cx);
                    }
                } else {
                    self.select_request(iid, cx);
                }
            }
            Page::RequestCommit(iid, _)
            | Page::RequestCommits(iid, ..)
            | Page::RequestVersions(iid, ..) => {
                self.nav.pending = Some(page);
                if same_item {
                    self.finish_pending(cx);
                } else {
                    self.select_request(iid, cx);
                }
            }
            Page::BranchCommits(refname, ..) => {
                self.nav.pending = Some(page);
                if same_item {
                    self.finish_pending(cx);
                } else if let Some(ix) = self.branch_ix(&refname) {
                    self.select_branch(ix, None, cx);
                }
            }
        }
    }

    fn branch_ix(&self, refname: &str) -> Option<usize> {
        self.branches.iter().position(|b| b.refname == refname)
    }

    /// What a page waited for is in (a request's commits, a branch's versions): the page is
    /// shown. Whether it was.
    pub(super) fn finish_pending(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(page) = self.nav.pending.clone() else {
            return false;
        };
        let done = match page {
            Page::RequestCommit(iid, _)
            | Page::RequestCommits(iid, ..)
            | Page::RequestVersions(iid, ..)
                if self.open_request.as_ref().map(|r| r.2) != Some(iid) =>
            {
                false
            }
            Page::RequestCommits(_, newest, oldest) | Page::BranchCommits(_, newest, oldest) => {
                let at = |id: Oid| self.commits.iter().position(|c| c.id == id);
                match (at(newest), at(oldest)) {
                    (Some(a), Some(b)) => {
                        self.select_commits(a, b, cx);
                        true
                    }
                    _ => {
                        // A push rewrote them: the item's own page.
                        self.nav.pending = None;
                        return false;
                    }
                }
            }
            Page::RequestCommit(_, id) => match self.commits.iter().position(|c| c.id == id) {
                Some(ix) => {
                    self.select_commit(ix, cx);
                    true
                }
                None => {
                    // Not among its commits any more (a push rewrote it): the whole request.
                    self.nav.pending = None;
                    return false;
                }
            },
            Page::RequestVersions(_, from, to) | Page::Versions(_, from, to)
                if to < self.versions.len() =>
            {
                self.select_versions(from, to, cx);
                true
            }
            _ => false,
        };
        if done {
            self.nav.pending = None;
        }
        done
    }

    /// Esc, once nothing smaller is open: one page up; nothing at a space's root.
    pub(super) fn go_up(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.nav_page().cloned() else {
            return;
        };
        let up = page.parent();
        if up.is_none() {
            // Back at the root: the list selects what was open.
            match (&page, page.item()) {
                (Page::WorkingTree, _) => self.nav.pick_branch = Some(BranchRow::WorkingTree),
                (_, Some(Item::Request(iid))) => {
                    self.nav.pick_request = Some(iid);
                    self.roots.reveal.set(true);
                }
                (_, Some(Item::Branch(refname))) => {
                    self.nav.pick_branch = self.branch_ix(&refname).map(BranchRow::Branch)
                }
                (_, None) => {}
            }
            self.nav.from = None;
        }
        self.show(self.nav.space, up, cx);
    }

    /// ⌘[ and ⌘]: the page seen before or after, in whatever space.
    pub(super) fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.nav.can_back() {
            self.nav.at -= 1;
            self.travel(cx);
        }
    }

    pub(super) fn go_forward(&mut self, cx: &mut Context<Self>) {
        if self.nav.can_forward() {
            self.nav.at += 1;
            self.travel(cx);
        }
    }

    fn travel(&mut self, cx: &mut Context<Self>) {
        let Some(moment) = self.nav.time.get(self.nav.at).cloned() else {
            return;
        };
        let at = self.nav.at;
        // The commit a branch's page showed then is the one it shows again.
        if let (Some(Page::Branch(refname)), Some(id)) = (&moment.page, moment.commit) {
            let item = Item::Branch(refname.clone());
            if let Some(open) = self.nav.opened_mut(&item) {
                open.commit = Some(id);
            }
            let loaded = self.loaded_page().and_then(|p| p.item()) == Some(item);
            if loaded && self.page_of(Space::Branches) == moment.page.as_ref() {
                self.nav.space = Space::Branches;
                self.nav.restoring = true;
                if let Some(ix) = self.commits.iter().position(|c| c.id == id) {
                    self.select_commit(ix, cx);
                }
                self.nav.restoring = false;
                self.inbox = false;
                self.nav.at = at;
                return cx.notify();
            }
        }
        self.nav.from = None;
        self.show(moment.space, moment.page, cx);
        // Showing it is not a new moment.
        self.nav.at = at;
    }

    /// A space's row or ⌃⌘1…4: the page it was left on; its row again, its root.
    pub(super) fn enter_space(&mut self, space: Space, cx: &mut Context<Self>) {
        let page = if space == self.nav.space && !self.settings_open {
            None
        } else {
            self.page_of(space).cloned()
        };
        self.nav.from = None;
        self.show(space, page, cx);
    }

    /// An open item's row in the menu: back to it, as it was left.
    pub(super) fn show_open(&mut self, item: &Item, cx: &mut Context<Self>) {
        let Some(page) = self.nav.opened(item).map(|o| o.page.clone()) else {
            return;
        };
        self.nav.from = None;
        self.show(item.space(), Some(page), cx);
    }

    /// × on an open item, or ⌘W on the one on screen: it leaves the menu; on screen, its space's
    /// root comes.
    pub(super) fn close_open(&mut self, item: &Item, cx: &mut Context<Self>) {
        self.nav.open.retain(|o| &o.item != item);
        self.nav.picked.retain(|o| o != item);
        let space = item.space();
        let shown = self.page_of(space).and_then(Page::item).as_ref() == Some(item);
        if shown {
            self.nav.pages[space.ix()] = None;
            if let Item::Request(iid) = item
                && self.requests.current == Some(*iid)
            {
                self.drop_request(cx);
            }
            if space == self.nav.space {
                self.nav.from = None;
                self.show(space, None, cx);
            }
        }
        self.save_nav();
        cx.notify();
    }

    /// ⌘W: closes the open items picked in the menu, else what is open on screen — a request, a
    /// branch, a file in Code.
    pub(super) fn close_shown(&mut self, cx: &mut Context<Self>) {
        if self.close_picked(cx) {
            return;
        }
        match self.nav_page().cloned() {
            Some(Page::File(path)) => self.close_code_tab(&path, cx),
            Some(page) => match page.item() {
                Some(item) => self.close_open(&item, cx),
                None => self.go_up(cx),
            },
            None => {}
        }
    }

    /// The request list came in: requests closed or merged since leave the menu, and a page
    /// waiting for the list is shown.
    pub(super) fn requests_listed(&mut self, cx: &mut Context<Self>) {
        let listed: Vec<u64> = self.requests.list.iter().map(|m| m.iid).collect();
        if self.requests.error.is_none() {
            let before = self.nav.open.len();
            self.nav.open.retain(|o| match o.item {
                Item::Request(iid) => listed.contains(&iid),
                Item::Branch(_) => true,
            });
            if before != self.nav.open.len() {
                if let Some(Item::Request(iid)) = self.page_of(Space::Requests).and_then(Page::item)
                    && !listed.contains(&iid)
                {
                    self.nav.pages[Space::Requests.ix()] = None;
                    if self.nav.space == Space::Requests {
                        self.show(Space::Requests, None, cx);
                    }
                }
                self.save_nav();
            }
        }
        if let Some((space, page)) = self.nav.pending_show.take() {
            self.show(space, page, cx);
        }
        // The list came in under its root: the first request is picked and previewed (#72).
        if self.nav.space == Space::Requests && self.at_root_list() {
            if self.nav.pick_request.is_none() {
                self.nav.pick_request = self.listed_requests().first().map(|m| m.iid);
            }
            self.read_preview(cx);
        }
    }

    /// The project just opened: the space and pages it was left on, else its checked-out branch.
    pub(super) fn restore_nav(&mut self, head: Option<usize>, cx: &mut Context<Self>) {
        let repo = self.nav_repo();
        let saved = crate::storage::read(FILE)
            .lines()
            .filter_map(Nav::read)
            .find(|saved| Some(&saved.repo) == repo.as_ref());
        self.nav = Nav::default();
        let Some(Saved {
            space, pages, open, ..
        }) = saved
        else {
            if let Some(ix) = head {
                self.open_branch(ix, None, cx);
            }
            return;
        };
        self.nav.pages = pages;
        for item in open {
            let page = match &item {
                Item::Request(iid) => Page::Request(*iid),
                Item::Branch(refname) => Page::Branch(refname.clone()),
            };
            let page = self
                .nav
                .pages
                .iter()
                .flatten()
                .find(|p| p.item().as_ref() == Some(&item))
                .cloned()
                .unwrap_or(page);
            self.nav.open.push(Opened {
                item,
                page,
                commit: None,
                place: None,
                paths: Vec::new(),
                section: Section::Changes,
            });
        }
        let page = self.nav.pages[space.ix()].clone();
        // Underneath a page that loads nothing of its own, the checked-out branch, as before:
        // ⌘K's commits and the files' revision come from it.
        if !matches!(
            page.as_ref().map(Page::space),
            Some(Space::Branches | Space::Requests)
        ) && let Some(ix) = head
        {
            self.nav.restoring = true;
            self.select_branch(ix, None, cx);
            self.nav.restoring = false;
        }
        let needs_list = matches!(page.as_ref().map(Page::space), Some(Space::Requests));
        if needs_list && self.requests.loading {
            self.nav.space = space;
            self.nav.pages[space.ix()] = None;
            self.nav.pending_show = Some((space, page));
        } else {
            self.show(space, page, cx);
        }
    }

    fn nav_repo(&self) -> Option<String> {
        self.root.as_ref().map(|r| crate::review::key(r))
    }

    fn save_nav(&self) {
        let Some(repo) = self.nav_repo() else {
            return;
        };
        let mut lines: Vec<String> = crate::storage::read(FILE)
            .lines()
            .filter(|l| l.split('\t').next() != Some(repo.as_str()))
            .map(str::to_owned)
            .collect();
        lines.push(self.nav.line(&repo));
        crate::storage::save(FILE, lines.join("\n") + "\n");
    }

    /// The open items of `space`, oldest first.
    pub(super) fn open_items(&self, space: Space) -> Vec<&Opened> {
        self.nav
            .open
            .iter()
            .filter(|o| o.item.space() == space)
            .collect()
    }

    /// Shift and a click on an open item in the menu: the items of its space from the one it began
    /// from (the first picked, else the one on screen) to this one, picked to close with ⌘W.
    pub(super) fn pick_open(&mut self, item: Item, cx: &mut Context<Self>) {
        let items: Vec<Item> = self
            .open_items(item.space())
            .into_iter()
            .map(|o| o.item.clone())
            .collect();
        let from = self
            .nav
            .picked
            .first()
            .cloned()
            .or_else(|| self.page_of(item.space()).and_then(Page::item))
            .filter(|f| items.contains(f))
            .unwrap_or_else(|| item.clone());
        if let Some(picked) = run(&items, &from, &item) {
            self.nav.picked = picked;
            cx.notify();
        }
    }

    /// ⌘W with open items picked in the menu: they all close.
    pub(super) fn close_picked(&mut self, cx: &mut Context<Self>) -> bool {
        if self.nav.picked.is_empty() {
            return false;
        }
        for item in std::mem::take(&mut self.nav.picked) {
            self.close_open(&item, cx);
        }
        true
    }

    /// Whether `item` is the one on screen.
    pub(super) fn is_shown(&self, item: &Item) -> bool {
        self.nav_page().and_then(Page::item).as_ref() == Some(item)
    }

    /// The menu's open items of a space shown or folded away.
    pub(super) fn toggle_open_items(&mut self, space: Space, cx: &mut Context<Self>) {
        self.nav.folded[space.ix()] = !self.nav.folded[space.ix()];
        cx.notify();
    }

    /// Opens branch `ix` as a page of Branches (a list's row, ⌘K, a link).
    pub(super) fn open_branch(&mut self, ix: usize, keep: Option<Oid>, cx: &mut Context<Self>) {
        let Some(refname) = self.branches.get(ix).map(|b| b.refname.clone()) else {
            return;
        };
        let item = Item::Branch(refname.clone());
        let place = (!self.is_shown(&item))
            .then(|| self.nav.opened(&item).and_then(|o| o.place.clone()))
            .flatten();
        self.arrive(Page::Branch(refname));
        self.restore_scroll = place;
        self.select_branch(ix, keep, cx);
    }
}

/// The items from `from` to `to` in `items`' order, `from` first; None when either is missing.
fn run<T: PartialEq + Clone>(items: &[T], from: &T, to: &T) -> Option<Vec<T>> {
    let a = items.iter().position(|i| i == from)?;
    let b = items.iter().position(|i| i == to)?;
    let mut out = items[a.min(b)..=a.max(b)].to_vec();
    if a > b {
        out.reverse();
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_picks_the_run_from_where_it_began() {
        let items = [1, 2, 3, 4, 5];
        assert_eq!(run(&items, &2, &4), Some(vec![2, 3, 4]));
        assert_eq!(run(&items, &4, &2), Some(vec![4, 3, 2]));
        assert_eq!(run(&items, &3, &3), Some(vec![3]));
        assert_eq!(run(&items, &9, &3), None);
    }

    #[test]
    fn a_page_knows_its_space_and_the_page_above_it() {
        let id = Oid::from_str("0123456789012345678901234567890123456789").unwrap();
        assert_eq!(Page::RequestCommit(398, id).space(), Space::Requests);
        assert_eq!(
            Page::RequestCommit(398, id).parent(),
            Some(Page::Request(398))
        );
        assert_eq!(Page::Request(398).parent(), None);
        assert_eq!(
            Page::Versions("refs/heads/x".into(), 0, 2).parent(),
            Some(Page::Branch("refs/heads/x".into()))
        );
        assert_eq!(
            Page::BranchCommit("refs/heads/x".into(), id).parent(),
            Some(Page::Branch("refs/heads/x".into()))
        );
        assert_eq!(Page::File("a/b.kt".into()).space(), Space::Code);
        assert_eq!(Page::WorkingTree.item(), None);
        assert_eq!(
            Page::RequestVersions(7, 0, 1).item(),
            Some(Item::Request(7))
        );
    }

    #[test]
    fn pages_and_open_items_survive_nav_txt() {
        let id = Oid::from_str("0123456789012345678901234567890123456789").unwrap();
        let pages = [
            Page::File("src/a b.rs".into()),
            Page::Branch("refs/heads/feature/x".into()),
            Page::BranchCommit("refs/heads/feature/x".into(), id),
            Page::Versions("refs/remotes/origin/x".into(), 1, 3),
            Page::WorkingTree,
            Page::Compare(("main".into(), id), ("origin/x".into(), id)),
            Page::Request(398),
            Page::RequestCommit(398, id),
            Page::RequestVersions(398, 0, 2),
        ];
        for page in pages {
            assert_eq!(Page::parse(&page.key()), Some(page));
        }
        let mut nav = Nav {
            space: Space::Requests,
            ..Nav::default()
        };
        nav.pages[Space::Code.ix()] = Some(Page::File("README.md".into()));
        nav.pages[Space::Requests.ix()] = Some(Page::RequestCommit(398, id));
        for item in [Item::Request(398), Item::Branch("refs/heads/main".into())] {
            nav.open.push(Opened {
                page: Page::WorkingTree,
                item,
                commit: None,
                place: None,
                paths: Vec::new(),
                section: Section::Changes,
            });
        }
        let Saved {
            repo,
            space,
            pages,
            open,
        } = Nav::read(&nav.line("_Users_me_repo")).unwrap();
        assert_eq!(repo, "_Users_me_repo");
        assert_eq!(space, Space::Requests);
        assert_eq!(
            pages[Space::Code.ix()],
            Some(Page::File("README.md".into()))
        );
        assert_eq!(pages[Space::Branches.ix()], None);
        assert_eq!(
            pages[Space::Requests.ix()],
            Some(Page::RequestCommit(398, id))
        );
        assert_eq!(
            open,
            [Item::Request(398), Item::Branch("refs/heads/main".into())]
        );
    }

    #[test]
    fn time_drops_what_was_ahead_and_keeps_one_of_a_kind() {
        let mut nav = Nav::default();
        let at = |space, page: Option<Page>| Moment {
            space,
            page,
            commit: None,
        };
        nav.remember(at(Space::Inbox, None));
        nav.remember(at(Space::Requests, Some(Page::Request(1))));
        nav.remember(at(Space::Requests, Some(Page::Request(1))));
        nav.remember(at(Space::Branches, None));
        assert_eq!(nav.time.len(), 3);
        // The branch's page, then the same with its commit in: one moment.
        let id = Oid::from_str("0123456789012345678901234567890123456789").unwrap();
        let branch = Some(Page::Branch("refs/heads/main".into()));
        nav.remember(at(Space::Branches, branch.clone()));
        nav.remember(Moment {
            commit: Some(id),
            ..at(Space::Branches, branch)
        });
        assert_eq!(nav.time.len(), 4);
        assert_eq!(nav.time[3].commit, Some(id));
        nav.time.truncate(3);
        nav.at = 2;
        nav.at = 0;
        assert!(nav.can_forward() && !nav.can_back());
        nav.remember(at(Space::Code, None));
        assert_eq!(nav.time.len(), 2);
        assert!(!nav.can_forward());
    }
}
