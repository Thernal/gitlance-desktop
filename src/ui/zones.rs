//! Keyboard focus zones: the branches, the commits, the files and the diff. One of them has the
//! keyboard at a time (its island has an outline), Tab and ⇧Tab move between them, ↑ ↓ ← → work in
//! the one that has it, and ↵ goes one step in.

use super::{Selection, Workspace, requests, theme};
use gpui::{Context, Rgba, px};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Zone {
    Branches,
    #[default]
    Commits,
    Files,
    Diff,
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
    pub(super) fn zone_tag(&self, zone: Zone) -> &'static str {
        if self.zone != zone {
            return "";
        }
        match zone {
            Zone::Branches => " · ⌃1",
            Zone::Commits => " · ⌃2",
            Zone::Files => " · ⌃3",
            Zone::Diff => " · ⌃4",
        }
    }

    pub(super) fn set_zone(&mut self, zone: Zone, cx: &mut Context<Self>) {
        // In a narrow window the files list is a popover: it opens with the zone and closes with it.
        let popover = zone == Zone::Files && self.compact();
        if self.zone != zone || self.files_popover != popover {
            self.zone = zone;
            self.files_popover = popover;
            cx.notify();
        }
    }

    /// The zones that exist right now, in Tab order.
    fn zones(&self) -> Vec<Zone> {
        let mut out = Vec::new();
        if self.layout.show_sidebar {
            out.push(Zone::Commits);
        }
        if self.diff.is_some() && !self.settings_open {
            if self.files_zone() {
                out.push(Zone::Files);
            }
            out.push(Zone::Diff);
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
        match self.zone {
            Zone::Commits => self.step_commit_row(down, cx),
            Zone::Branches => self.step_branch(down, cx),
            Zone::Files => self.step_file_item(down, cx),
            Zone::Diff => self
                .diff_list
                .scroll_by(px(if down { LINE } else { -LINE })),
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
        match self.zone {
            Zone::Branches => self.close_picker(cx),
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
            Zone::Diff => {}
        }
    }

    /// PageUp, PageDown, Space: a screen of the diff.
    pub(super) fn page(&mut self, down: bool, cx: &mut Context<Self>) {
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

    /// The branch (or merge request) before or after the open one, in the order the list shows.
    fn step_branch(&mut self, down: bool, cx: &mut Context<Self>) {
        if self.requests.available && self.requests.side == requests::Side::Requests {
            let shown: Vec<u64> = self.shown_requests().iter().map(|m| m.iid).collect();
            let at = self
                .requests
                .current
                .and_then(|iid| shown.iter().position(|i| *i == iid));
            let to = match (at, down) {
                (Some(i), true) => (i + 1 < shown.len()).then(|| i + 1),
                (Some(i), false) => i.checked_sub(1),
                (None, _) => (!shown.is_empty()).then_some(0),
            };
            if let Some(iid) = to.map(|i| shown[i]) {
                self.select_request(iid, cx);
            }
            return;
        }
        let order = self.branch_order();
        let at = self
            .branch
            .and_then(|b| order.iter().position(|ix| *ix == b));
        let to = match (at, down) {
            (Some(i), true) => (i + 1 < order.len()).then(|| i + 1),
            (Some(i), false) => i.checked_sub(1),
            (None, _) => (!order.is_empty()).then_some(0),
        };
        if let Some(ix) = to.map(|i| order[i]) {
            self.select_branch(ix, None, cx);
            self.record();
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
        // ← on a file: the folder it is in.
        let path = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(self.file))
            .map(|f| f.path().to_owned());
        if let Some(path) = path {
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

    fn branch_order(&self) -> Vec<usize> {
        self.branch_ixs()
    }
}

#[derive(Clone)]
pub enum ItemKey {
    File(usize),
    Dir(String),
}
