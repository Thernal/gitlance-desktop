//! Keyboard focus zones: the branches, the commits, the files, the diff and the tool window on the
//! right. One of them has the
//! keyboard at a time (its island has an outline), Tab and ⇧Tab move between them, ↑ ↓ ← → work in
//! the one that has it, and ↵ goes one step in.

use super::px;
use super::{Selection, Workspace, theme};
use gpui::{Context, Rgba};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Zone {
    Branches,
    #[default]
    Commits,
    Files,
    Diff,
    /// The tool window (`super::tools`).
    Tool,
}

/// How far a key scrolls the diff.
const LINE: f32 = 60.;
const PAGE: f32 = 520.;

impl Workspace {
    /// The outline colour of a zone's island.
    pub(super) fn zone_border(&self, zone: Zone) -> Rgba {
        if self.zone == zone {
            theme::splitter()
        } else {
            theme::island_border()
        }
    }

    /// " · ⌃2": the island that has the keyboard says so in its label, and how to get back to it.
    pub(super) fn zone_tag(&self, zone: Zone) -> std::borrow::Cow<'static, str> {
        if self.zone != zone {
            return "".into();
        }
        super::keys::text(match zone {
            Zone::Branches => " · ⌃1",
            Zone::Commits => " · ⌃2",
            Zone::Files => " · ⌃3",
            Zone::Diff => " · ⌃4",
            Zone::Tool => "",
        })
    }

    pub(super) fn set_zone(&mut self, zone: Zone, cx: &mut Context<Self>) {
        if self.zone != zone {
            self.zone = zone;
            cx.notify();
        }
    }

    /// The zones that exist right now, in Tab order.
    fn zones(&self) -> Vec<Zone> {
        let mut out = Vec::new();
        // A page with no side panel and no diff: a space's list, the Inbox, Code (#71).
        let item = matches!(
            self.nav.space,
            super::spaces::Space::Branches | super::spaces::Space::Requests
        ) && !self.at_root_list();
        // A request's Overview and Commits: no side panel and no diff (#73).
        let tab_page = matches!(
            self.page_tab(),
            Some(super::nav::Section::Overview | super::nav::Section::Commits)
        );
        if !item || tab_page {
            if self.at_root_list() {
                out.push(Zone::Branches);
            }
            if self.tool_shown() && !self.settings_open {
                out.push(Zone::Tool);
            }
            return out;
        }
        // A tabbed item's commits are its Commits tab, not a section of the panel (#73, #74).
        if self.side_open() && !self.layout.fold_commits && self.side_commits() {
            out.push(Zone::Commits);
        }
        if self.diff.is_some() && !self.settings_open {
            if self.files_zone() {
                out.push(Zone::Files);
            }
            out.push(Zone::Diff);
        }
        if self.tool_shown() && !self.settings_open {
            out.push(Zone::Tool);
        }
        out
    }

    pub(super) fn step_zone(&mut self, forward: bool, cx: &mut Context<Self>) {
        let zones = self.zones();
        if zones.is_empty() {
            return;
        }
        let at = zones.iter().position(|z| *z == self.zone);
        let next = match (at, forward) {
            (Some(i), true) => (i + 1) % zones.len(),
            (Some(i), false) => (i + zones.len() - 1) % zones.len(),
            (None, _) => 0,
        };
        self.set_zone(zones[next], cx);
    }

    /// ↑ and ↓ in the zone that has the keyboard.
    pub(super) fn zone_vertical(&mut self, down: bool, cx: &mut Context<Self>) {
        // The New merge request card's From or Into list (#159).
        if self.step_branch_list(down, cx) {
            return;
        }
        // The connect panel with its field: ↑ ↓ move between the ways (#156).
        if self.requests.connectable
            && self.field == Some(super::lists::Field::Token)
            && !self.settings_open
        {
            return self.step_way(down, cx);
        }
        // The Inbox has the window: j k walk its list.
        if self.inbox && self.zone != Zone::Tool {
            return self.inbox_step(down, cx);
        }
        if self.page_tab() == Some(super::nav::Section::Commits) && self.zone != Zone::Tool {
            return self.step_commit_pick(down, cx);
        }
        match self.zone {
            Zone::Commits => self.step_commit_row(down, cx),
            Zone::Branches => self.step_pick(down, cx),
            Zone::Files => self.step_file_item(down, cx),
            Zone::Diff => self
                .diff_list
                .scroll_by(px(if down { LINE } else { -LINE })),
            Zone::Tool => match self.tool() {
                Some(super::tools::Tool::Discussions) => return self.step_thread(down, cx),
                Some(super::tools::Tool::Agents) => return self.step_agent_row(down, cx),
                _ => {}
            },
        }
        cx.notify();
    }

    /// ← and → : the neighbouring file, or in the file tree open and close folders.
    pub(super) fn zone_horizontal(&mut self, forward: bool, cx: &mut Context<Self>) {
        match self.zone {
            Zone::Files if !self.options.list_files && self.pfilter.trim().is_empty() => {
                self.tree_horizontal(forward, cx)
            }
            Zone::Branches if forward => self.set_zone(Zone::Commits, cx),
            Zone::Tool if !forward => self.set_zone(Zone::Diff, cx),
            Zone::Tool => {}
            Zone::Diff if !forward => self.set_zone(
                if self.files_zone() {
                    Zone::Files
                } else {
                    Zone::Commits
                },
                cx,
            ),
            _ => self.step_file(forward, cx),
        }
    }

    pub(super) fn step_file(&mut self, forward: bool, cx: &mut Context<Self>) {
        let order = self.file_order();
        let Some(at) = order.iter().position(|&ix| ix == self.file) else {
            return;
        };
        let to = if forward {
            (at + 1 < order.len()).then(|| at + 1)
        } else {
            at.checked_sub(1)
        };
        if let Some(to) = to {
            self.select_file(order[to], cx);
        }
    }

    /// ↵: one step in.
    pub(super) fn activate(&mut self, cx: &mut Context<Self>) {
        if self.inbox && self.zone != Zone::Tool {
            return self.inbox_open(cx);
        }
        // A space's list: what is selected in it opens (#71).
        if self.at_root_list() && self.zone == Zone::Branches {
            return self.open_pick(cx);
        }
        if self.page_tab() == Some(super::nav::Section::Commits) && self.zone != Zone::Tool {
            return self.open_commit_pick(cx);
        }
        match self.zone {
            Zone::Tool if self.tool() == Some(super::tools::Tool::Agents) => {
                self.agent_row_action(cx)
            }
            Zone::Tool | Zone::Branches => {}
            Zone::Commits => {
                if self.diff.is_some() {
                    self.set_zone(
                        if self.files_zone() {
                            Zone::Files
                        } else {
                            Zone::Diff
                        },
                        cx,
                    );
                }
            }
            Zone::Files => {
                if let Some(dir) = self.fcursor.clone() {
                    self.toggle_dir(&dir, cx);
                } else {
                    self.set_zone(Zone::Diff, cx);
                }
            }
            Zone::Diff => {
                // A folded generated file opens on ↵.
                if let Some(path) = self
                    .diff
                    .as_ref()
                    .and_then(|d| d.files.get(self.file))
                    .filter(|f| self.generated_folded(f))
                    .map(|f| f.path().to_owned())
                {
                    self.unfolded.insert(path);
                    cx.notify();
                }
            }
        }
    }

    /// PageUp, PageDown, Space: a screen of the diff.
    pub(super) fn page(&mut self, down: bool, cx: &mut Context<Self>) {
        if self.scroll_page(down, cx) {
            return;
        }
        self.diff_list
            .scroll_by(px(if down { PAGE } else { -PAGE }));
        cx.notify();
    }

    /// Home and End: the top or the bottom of the diff.
    pub(super) fn diff_edge(&mut self, end: bool, cx: &mut Context<Self>) {
        if end {
            self.diff_list.scroll_to_end();
        } else {
            self.diff_list.scroll_to(gpui::ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
        }
        cx.notify();
    }

    /// The working tree entry above the commits counts as the row before the first commit.
    fn step_commit_row(&mut self, down: bool, cx: &mut Context<Self>) {
        let has_wt = self.wt.count > 0 || self.selection == Selection::WorkingTree;
        match (self.selection, down) {
            (Selection::WorkingTree, true) if self.commit_rows() > 0 => {
                self.select_commit(self.commit_at(0), cx)
            }
            (Selection::WorkingTree, _) => {}
            (Selection::Commit(ix), false) if has_wt && self.row_of(ix) == Some(0) => {
                self.select_working_tree(cx)
            }
            (Selection::Commit(ix), _) => {
                if let Some(row) = self.row_of(ix) {
                    let to = if down {
                        (row + 1 < self.commit_rows()).then(|| row + 1)
                    } else {
                        row.checked_sub(1)
                    };
                    if let Some(to) = to {
                        self.select_commit(self.commit_at(to), cx);
                    }
                }
            }
            (_, true) if self.commit_rows() > 0 => self.select_commit(self.commit_at(0), cx),
            _ => {}
        }
    }

    /// ↑ ↓ in a space's list: the row selected, nothing opened (#71); the picked request's
    /// preview is read (#72).
    pub(super) fn step_pick(&mut self, down: bool, cx: &mut Context<Self>) {
        if !self.at_root_list() {
            return;
        }
        let step = |at: Option<usize>, n: usize| match (at, down) {
            (Some(i), true) => Some((i + 1).min(n.saturating_sub(1))),
            (Some(i), false) => Some(i.saturating_sub(1)),
            (None, _) => (n > 0).then_some(0),
        };
        if self.nav.space == super::spaces::Space::Requests {
            let shown: Vec<u64> = self.listed_requests().iter().map(|m| m.iid).collect();
            let at = self
                .nav
                .pick_request
                .and_then(|iid| shown.iter().position(|i| *i == iid));
            if let Some(iid) = step(at, shown.len()).map(|i| shown[i]) {
                self.pick_request(iid, cx);
            }
        } else {
            let rows = self.branch_rows();
            let at = self
                .nav
                .pick_branch
                .or_else(|| self.first_branch_row())
                .and_then(|r| rows.iter().position(|x| *x == r));
            self.nav.pick_branch = step(at, rows.len()).map(|i| rows[i]);
            if self.nav.pick_branch == Some(super::nav::BranchRow::WorkingTree) {
                self.read_tree_files(cx);
            }
        }
        cx.notify();
    }

    /// ↵ in a space's list: the row selected opens.
    pub(super) fn open_pick(&mut self, cx: &mut Context<Self>) {
        if self.nav.space == super::spaces::Space::Requests {
            let shown: Vec<u64> = self.listed_requests().iter().map(|m| m.iid).collect();
            let pick = self.nav.pick_request.filter(|iid| shown.contains(iid));
            if let Some(iid) = pick.or(shown.first().copied()) {
                self.open_picked_request(iid, cx);
            }
        } else {
            let rows = self.branch_rows();
            let pick = self
                .nav
                .pick_branch
                .or_else(|| self.first_branch_row())
                .filter(|r| rows.contains(r));
            if let Some(row) = pick.or(rows.first().copied()) {
                self.open_branch_row(row, cx);
            }
        }
    }

    /// ↑ ↓ over what the files island shows, folders included.
    fn step_file_item(&mut self, down: bool, cx: &mut Context<Self>) {
        let items = self.item_keys();
        let here = match &self.fcursor {
            Some(dir) => items
                .iter()
                .position(|k| matches!(k, ItemKey::Dir(d) if d == dir)),
            None => items
                .iter()
                .position(|k| matches!(k, ItemKey::File(ix) if *ix == self.file)),
        };
        let to = match (here, down) {
            (Some(i), true) => (i + 1 < items.len()).then(|| i + 1),
            (Some(i), false) => i.checked_sub(1),
            (None, _) => (!items.is_empty()).then_some(0),
        };
        match to.map(|i| items[i].clone()) {
            Some(ItemKey::File(ix)) => {
                self.fcursor = None;
                self.select_file(ix, cx);
            }
            Some(ItemKey::Dir(path)) => {
                self.fcursor = Some(path);
                cx.notify();
            }
            None => {}
        }
    }

    /// In the tree: → opens a folder or goes into the diff, ← closes a folder or goes up to it.
    fn tree_horizontal(&mut self, forward: bool, cx: &mut Context<Self>) {
        if let Some(dir) = self.fcursor.clone() {
            let open = !self.collapsed.contains(&dir);
            match (forward, open) {
                (true, false) | (false, true) => self.toggle_dir(&dir, cx),
                (true, true) => self.step_file_item(true, cx),
                (false, false) => {}
            }
            return;
        }
        if forward {
            self.set_zone(Zone::Diff, cx);
            return;
        }
        // ← on a file: the folder it is in, or its group (#22).
        if let Some(path) = self.file_key(self.file) {
            let parent = self
                .item_keys()
                .into_iter()
                .filter_map(|k| match k {
                    ItemKey::Dir(d) if path.starts_with(&format!("{d}/")) => Some(d),
                    _ => None,
                })
                .next_back();
            if parent.is_some() {
                self.fcursor = parent;
                cx.notify();
            }
        }
    }
}

#[derive(Clone)]
pub enum ItemKey {
    File(usize),
    Dir(String),
}
