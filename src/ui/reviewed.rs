//! A reviewer's marks: which files of a diff were read. A ✓ per file, a count in the files label,
//! `v` to mark the open file and go on to the next one not yet read. Kept per commit, version
//! range or merge request on this Mac; a file that changes in a newer version loses its mark.
//! Designed in `../Design/mockups/ide-ideas/a-ideas.html` (1).

use super::px;
use super::{Header, ROW_RADIUS, Workspace, plural, theme};
use crate::git::FileDiff;
use crate::storage;
use gpui::{ClickEvent, Context, FontWeight, IntoElement, MouseButton, div, prelude::*};
use std::collections::{HashMap, HashSet};

/// How a file stands in the review.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Open,
    Done,
    /// Marked, but the file is not what it was when it was marked.
    Stale,
}

/// How far your review of a merge request got, against its head now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seen {
    /// No file of it was ever marked.
    Never,
    /// Files were marked at this head.
    Started,
    /// Files were marked at an earlier head: it was pushed since.
    Earlier,
    /// Its whole diff at this head was reviewed.
    Done,
}

/// A file marked reviewed: its fingerprint then, when (0: not known, an older mark), and the head
/// the diff was at — its version says "at v2" (X59).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mark {
    fp: u64,
    time: i64,
    head: Option<git2::Oid>,
}

impl Mark {
    fn now(fp: u64, head: Option<git2::Oid>) -> Self {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        Self { fp, time, head }
    }
}

/// Every mark ever made: per scope, per path, the file when it was marked.
#[derive(Default)]
pub struct Store {
    marks: HashMap<String, HashMap<String, Mark>>,
    /// `root|commit` of every head whose whole diff was reviewed: a branch or merge request whose tip
    /// is one of them shows ✓, and a new push (a new tip) takes it away.
    heads: HashSet<String>,
    /// `root|commit` → (files reviewed, files) of the last whole diff read up to that head: how far
    /// a branch or request got, for the lists.
    progress: HashMap<String, (u32, u32)>,
}

impl Store {
    pub fn load() -> Self {
        let mut store = Self::parse(&storage::read("reviewed.txt"));
        store.heads = storage::read("reviewed-heads.txt")
            .lines()
            .map(str::to_owned)
            .collect();
        store.progress = storage::read("reviewed-progress.txt")
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\t');
                let key = parts.next()?.to_owned();
                let done = parts.next()?.parse().ok()?;
                let total = parts.next()?.parse().ok()?;
                Some((key, (done, total)))
            })
            .collect();
        store
    }

    /// How far the whole diff up to `head` got: (files reviewed, files).
    pub fn head_progress(&self, root: &str, head: git2::Oid) -> Option<(u32, u32)> {
        self.progress.get(&Self::head_key(root, head)).copied()
    }

    fn set_progress(&mut self, root: &str, head: git2::Oid, done: u32, total: u32) {
        let key = Self::head_key(root, head);
        if self.progress.get(&key) == Some(&(done, total)) {
            return;
        }
        if done == 0 {
            self.progress.remove(&key);
        } else {
            self.progress.insert(key, (done, total));
        }
        while self.progress.len() > 500 {
            if let Some(k) = self.progress.keys().next().cloned() {
                self.progress.remove(&k);
            }
        }
        let mut lines: Vec<String> = self
            .progress
            .iter()
            .map(|(k, (d, t))| format!("{k}\t{d}\t{t}"))
            .collect();
        lines.sort_unstable();
        storage::save("reviewed-progress.txt", lines.join("\n") + "\n");
    }

    fn save_heads(&self) {
        let mut lines: Vec<&str> = self.heads.iter().map(String::as_str).collect();
        lines.sort_unstable();
        storage::save("reviewed-heads.txt", lines.join("\n") + "\n");
    }

    fn head_key(root: &str, head: git2::Oid) -> String {
        format!("{root}|{head}")
    }

    pub fn head_done(&self, root: &str, head: git2::Oid) -> bool {
        self.heads.contains(&Self::head_key(root, head))
    }

    fn set_head(&mut self, root: &str, head: git2::Oid, done: bool) {
        let key = Self::head_key(root, head);
        let changed = if done {
            self.heads.insert(key)
        } else {
            self.heads.remove(&key)
        };
        if changed {
            while self.heads.len() > 500 {
                if let Some(k) = self.heads.iter().next().cloned() {
                    self.heads.remove(&k);
                }
            }
            self.save_heads();
        }
    }

    /// Marks or unmarks many files of one scope with a single write.
    fn set_many(
        &mut self,
        scope: &str,
        files: &[(String, u64)],
        done: bool,
        head: Option<git2::Oid>,
    ) {
        let entry = self.marks.entry(scope.to_owned()).or_default();
        for (path, fp) in files {
            if done {
                entry.insert(path.clone(), Mark::now(*fp, head));
            } else {
                entry.remove(path);
            }
        }
        if entry.is_empty() {
            self.marks.remove(scope);
        }
        self.prune();
        self.save();
    }

    /// `scope path fingerprint [time head]`, tab apart; older lines have no time and head.
    fn parse(text: &str) -> Self {
        let mut marks: HashMap<String, HashMap<String, Mark>> = HashMap::new();
        for line in text.lines() {
            let mut parts = line.splitn(5, '\t');
            let (Some(scope), Some(path), Some(fp)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let time = parts.next().and_then(|t| t.parse().ok()).unwrap_or(0);
            let head = parts.next().and_then(|h| git2::Oid::from_str(h).ok());
            if let Ok(fp) = u64::from_str_radix(fp, 16) {
                marks
                    .entry(scope.to_owned())
                    .or_default()
                    .insert(path.to_owned(), Mark { fp, time, head });
            }
        }
        Self {
            marks,
            ..Self::default()
        }
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for (scope, files) in &self.marks {
            for (path, m) in files {
                let head = m.head.map(|h| h.to_string()).unwrap_or_default();
                out.push_str(&format!(
                    "{scope}\t{path}\t{:x}\t{}\t{head}\n",
                    m.fp, m.time
                ));
            }
        }
        out
    }

    fn save(&self) {
        storage::save("reviewed.txt", self.render());
    }

    fn get(&self, scope: &str, path: &str) -> Option<u64> {
        self.marks.get(scope)?.get(path).map(|m| m.fp)
    }

    /// The heads the files of `scope` were marked at, one per file marked.
    fn marked_heads(&self, scope: &str) -> Vec<Option<git2::Oid>> {
        self.marks
            .get(scope)
            .map(|m| m.values().map(|m| m.head).collect())
            .unwrap_or_default()
    }

    /// When the file was marked, and the head the diff was at then.
    fn when(&self, scope: &str, path: &str) -> Option<(i64, Option<git2::Oid>)> {
        let m = self.marks.get(scope)?.get(path)?;
        (m.time > 0).then_some((m.time, m.head))
    }

    fn set(&mut self, scope: &str, path: &str, fp: u64, head: Option<git2::Oid>) {
        self.marks
            .entry(scope.to_owned())
            .or_default()
            .insert(path.to_owned(), Mark::now(fp, head));
        self.prune();
        self.save();
    }

    fn clear(&mut self, scope: &str, path: &str) {
        if let Some(files) = self.marks.get_mut(scope) {
            files.remove(path);
            if files.is_empty() {
                self.marks.remove(scope);
            }
        }
        self.save();
    }

    /// The file does not grow without end: the newest scopes are kept (a random 300).
    fn prune(&mut self) {
        while self.marks.len() > 300 {
            if let Some(key) = self.marks.keys().next().cloned() {
                self.marks.remove(&key);
            }
        }
    }
}

/// What a file is, for the purpose of "is it the same file I read": its kind of change, its
/// lines and its contents, hashed (FNV-1a, which does not change between builds).
pub fn fingerprint(file: &FileDiff) -> u64 {
    fn feed(h: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *h ^= u64::from(*b);
            *h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    feed(
        &mut h,
        format!("{:?}{}{}", file.change, file.added, file.removed).as_bytes(),
    );
    for hunk in &file.hunks {
        for line in &hunk.lines {
            feed(
                &mut h,
                format!("{:?}{:?}{:?}", line.kind, line.old_line, line.new_line).as_bytes(),
            );
        }
    }
    feed(
        &mut h,
        file.new_text.as_deref().unwrap_or_default().as_bytes(),
    );
    feed(
        &mut h,
        file.old_text.as_deref().unwrap_or_default().as_bytes(),
    );
    h
}

impl Workspace {
    /// The key the marks of the open diff are kept under; none for the working tree, which has
    /// no stable identity.
    fn review_scope(&self) -> Option<String> {
        let root = self.root.as_ref()?.display().to_string();
        let key = match &self.diff.as_ref()?.header {
            Header::Commit(c) => format!("commit:{}", c.id),
            Header::Compare {
                request: Some(iid), ..
            } => format!("mr:{iid}"),
            Header::Compare { base, head, .. } => format!("range:{}..{}", base.1, head.1),
            Header::Versions { from, to, .. } => format!("ver:{}..{}", from.tip, to.tip),
            Header::Interdiff { .. } | Header::WorkingTree { .. } => return None,
        };
        Some(format!("{root}|{key}"))
    }

    /// Called when a diff arrives: its scope and the fingerprint of each file.
    pub(super) fn index_review(&mut self) {
        self.review_scope = self.review_scope();
        if let Some(d) = &self.diff
            && let Header::Compare {
                request: Some(iid), ..
            } = d.header
        {
            self.requests.files = Some((iid, d.files.len()));
        }
        self.fps = match (&self.review_scope, &self.diff) {
            (Some(_), Some(d)) => d.files.iter().map(fingerprint).collect(),
            _ => Vec::new(),
        };
        self.done_card = false;
    }

    pub(super) fn review_state(&self, ix: usize) -> Option<State> {
        let scope = self.review_scope.as_deref()?;
        let path = self.diff.as_ref()?.files.get(ix)?.path();
        Some(match (self.reviewed.get(scope, path), self.fps.get(ix)) {
            (Some(was), Some(now)) if was == *now => State::Done,
            (Some(_), _) => State::Stale,
            _ => State::Open,
        })
    }

    /// How many files are marked read, of how many.
    pub(super) fn review_progress(&self) -> Option<(usize, usize)> {
        self.review_scope.as_ref()?;
        let total = self.diff.as_ref()?.files.len();
        let done = (0..total)
            .filter(|&ix| self.review_state(ix) == Some(State::Done))
            .count();
        Some((done, total))
    }

    pub(super) fn set_reviewed(&mut self, ix: usize, done: bool, cx: &mut Context<Self>) {
        let (Some(scope), Some(fp)) = (self.review_scope.clone(), self.fps.get(ix).copied()) else {
            return;
        };
        let Some(path) = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(ix))
            .map(|f| f.path().to_owned())
        else {
            return;
        };
        if done {
            let head = self.viewed_rev();
            self.reviewed.set(&scope, &path, fp, head);
        } else {
            self.reviewed.clear(&scope, &path);
        }
        self.review_changed(&[ix]);
        self.sync_complete();
        cx.notify();
    }

    /// Marks or unmarks files `ixs` at once (all of them, a folder).
    pub(super) fn set_reviewed_many(&mut self, ixs: &[usize], done: bool, cx: &mut Context<Self>) {
        let (Some(scope), Some(diff)) = (self.review_scope.clone(), self.diff.as_ref()) else {
            return;
        };
        let files: Vec<(String, u64)> = ixs
            .iter()
            .filter_map(|&ix| Some((diff.files.get(ix)?.path().to_owned(), *self.fps.get(ix)?)))
            .collect();
        let head = self.viewed_rev();
        self.reviewed.set_many(&scope, &files, done, head);
        self.review_changed(ixs);
        self.sync_complete();
        cx.notify();
    }

    /// Every file of the open diff is reviewed.
    pub(super) fn all_reviewed(&self) -> bool {
        self.review_progress()
            .is_some_and(|(done, total)| total > 0 && done == total)
    }

    /// Records whether the head of the open diff is fully reviewed; the card offering what is next
    /// comes up the moment it becomes so.
    fn sync_complete(&mut self) {
        let (Some(root), Some(head)) = (self.root.clone(), self.viewed_rev()) else {
            return;
        };
        let root = root.display().to_string();
        let done = self.all_reviewed();
        // Only a whole branch or request (a compare, versions) makes its head count as reviewed;
        // one commit read is not its branch read.
        let whole = matches!(
            self.diff.as_ref().map(|d| &d.header),
            Some(Header::Compare { .. } | Header::Versions { .. })
        );
        if done && !(whole && self.reviewed.head_done(&root, head)) {
            self.done_card = true;
        }
        if !done {
            self.done_card = false;
        }
        if whole {
            self.reviewed.set_head(&root, head, done);
            if let Some((reviewed, total)) = self.review_progress() {
                self.reviewed
                    .set_progress(&root, head, reviewed as u32, total as u32);
            }
        }
    }

    /// The version of request `iid` your marks were last made at, by the versions loaded (#73).
    pub(super) fn marked_version(&self, iid: u64) -> Option<usize> {
        let root = self.root.as_ref()?.display().to_string();
        let heads = self.reviewed.marked_heads(&format!("{root}|mr:{iid}"));
        self.versions
            .iter()
            .filter(|v| heads.contains(&Some(v.tip)))
            .map(|v| v.number)
            .max()
    }

    /// How far the review of the branch or request ending at `head` got, when it started and is
    /// not done: (files reviewed, files).
    pub(super) fn head_part(&self, head: git2::Oid) -> Option<(u32, u32)> {
        let root = self.root.as_ref()?.display().to_string();
        if self.reviewed.head_done(&root, head) {
            return None;
        }
        self.reviewed
            .head_progress(&root, head)
            .filter(|&(done, total)| done > 0 && done < total)
    }

    /// How far your review of request `iid` got, against `head`, its head now (#72).
    pub(super) fn request_seen(&self, iid: u64, head: Option<git2::Oid>) -> Seen {
        let Some(root) = self.root.as_ref().map(|r| r.display().to_string()) else {
            return Seen::Never;
        };
        if head.is_some_and(|h| self.reviewed.head_done(&root, h)) {
            return Seen::Done;
        }
        let heads = self.reviewed.marked_heads(&format!("{root}|mr:{iid}"));
        if heads.is_empty() {
            Seen::Never
        } else if head.is_some() && heads.iter().all(|h| *h == head) {
            Seen::Started
        } else {
            Seen::Earlier
        }
    }

    /// Whether the whole diff of a branch or request ending at `head` was reviewed.
    pub(super) fn head_reviewed(&self, head: git2::Oid) -> bool {
        self.root
            .as_ref()
            .is_some_and(|r| self.reviewed.head_done(&r.display().to_string(), head))
    }

    /// The files under folder `path` of the tree.
    pub(super) fn files_under(&self, path: &str) -> Vec<usize> {
        let prefix = format!("{path}/");
        self.diff
            .as_ref()
            .map(|d| {
                d.files
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| f.path().starts_with(&prefix))
                    .map(|(ix, _)| ix)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// ⌥⇧V and the buttons: every file the list shows marked, or (when all are) unmarked.
    pub(super) fn toggle_all_reviewed(&mut self, cx: &mut Context<Self>) {
        let shown = self.file_order();
        let done = shown
            .iter()
            .all(|&ix| self.review_state(ix) == Some(State::Done));
        self.set_reviewed_many(&shown, !done, cx);
    }

    /// The box of a folder: empty, part-marked or all marked; a click marks or unmarks the folder.
    pub(super) fn folder_box(
        &self,
        path: &str,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        self.review_scope.as_ref()?;
        let ixs = self.files_under(path);
        let done = ixs
            .iter()
            .filter(|&&ix| self.review_state(ix) == Some(State::Done))
            .count();
        let all = done == ixs.len() && done > 0;
        let (border, fill, glyph) = if all {
            (theme::added(), Some(theme::added()), "✓")
        } else if done > 0 {
            (theme::added(), None, "–")
        } else {
            (theme::faint(), None, "")
        };
        let target = path.to_owned();
        Some(
            div()
                .id(gpui::ElementId::Name(format!("folder-box-{path}").into()))
                .flex_none()
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border_2()
                .border_color(border)
                .when_some(fill, |s, c| s.bg(c))
                .text_size(px(10.))
                .font_weight(FontWeight::BOLD)
                .text_color(if all { theme::base() } else { theme::added() })
                .cursor_pointer()
                .hover(|s| s.border_color(theme::text()))
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(move |_, cx| {
                    cx.new(|_| {
                        super::Tip(if all {
                            "Folder reviewed — click to unmark it"
                        } else {
                            "Mark the whole folder reviewed"
                        })
                    })
                    .into()
                })
                .child(glyph)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let ixs = this.files_under(&target);
                    this.set_reviewed_many(&ixs, !all, cx);
                })),
        )
    }

    /// `⇧V`: marks or unmarks the open file.
    pub(super) fn toggle_reviewed(&mut self, cx: &mut Context<Self>) {
        let ix = self.file;
        let done = self.review_state(ix) == Some(State::Done);
        self.set_reviewed(ix, !done, cx);
    }

    /// `v`: marks the open file read and opens the next one that is not.
    pub(super) fn mark_and_advance(&mut self, cx: &mut Context<Self>) {
        if self.review_scope.is_none() {
            return;
        }
        self.set_reviewed(self.file, true, cx);
        self.next_unreviewed(cx);
    }

    /// Opens the next file, in the order the list shows them, that is not marked read.
    pub(super) fn next_unreviewed(&mut self, cx: &mut Context<Self>) {
        let order = self.file_order();
        let at = order.iter().position(|&ix| ix == self.file).unwrap_or(0);
        let next = (1..=order.len())
            .map(|step| order[(at + step) % order.len().max(1)])
            .find(|&ix| self.review_state(ix) != Some(State::Done));
        if let Some(ix) = next.filter(|&ix| ix != self.file) {
            self.select_file(ix, cx);
        }
    }

    /// What a file's box says on a long hover: when it was marked, at which version (X59).
    fn review_said(&self, ix: usize, state: State) -> String {
        let when = self
            .review_scope
            .as_deref()
            .zip(self.diff.as_ref().and_then(|d| d.files.get(ix)))
            .and_then(|(scope, f)| self.reviewed.when(scope, f.path()));
        let at = when.map(|(time, head)| {
            let version = head
                .and_then(|h| self.versions.iter().find(|v| v.tip == h))
                .map(|v| format!(" at v{},", v.number))
                .unwrap_or_default();
            format!("{version} {}", super::format::ago(time))
        });
        match (state, at) {
            (State::Open, _) => "Mark as reviewed  ⇧V".into(),
            (State::Done, Some(at)) => format!("Reviewed{at} — click to unmark  ⇧V"),
            (State::Done, None) => "Reviewed — click to unmark  ⇧V".into(),
            (State::Stale, Some(at)) => {
                format!("Changed since you reviewed it{at} — mark again  ⇧V")
            }
            (State::Stale, None) => "Changed since you marked it — mark again  ⇧V".into(),
        }
    }

    /// The box at the left of a file row; a click marks or unmarks without opening the file.
    pub(super) fn review_box(
        &self,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let state = self.review_state(ix)?;
        let said: gpui::SharedString = self.review_said(ix, state).into();
        let (border, fill, glyph) = match state {
            State::Open => (theme::faint(), None, ""),
            State::Done => (theme::added(), Some(theme::added()), "✓"),
            State::Stale => (theme::warning(), None, "↻"),
        };
        Some(
            div()
                .id(("review-box", ix))
                .flex_none()
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border_2()
                .border_color(border)
                .when_some(fill, |s, c| s.bg(c))
                .text_size(px(10.))
                .font_weight(FontWeight::BOLD)
                .text_color(if state == State::Done {
                    theme::base()
                } else {
                    theme::warning()
                })
                .cursor_pointer()
                .hover(|s| s.border_color(theme::text()))
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| super::TipOwned(said.clone())).into())
                // A new mark pops in (#56, 12).
                .child(super::motion::appear(
                    div().child(glyph),
                    ("review-tick", state as usize),
                    super::motion::FAST,
                    |tick, v| tick.opacity(v),
                ))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let done = this.review_state(ix) == Some(State::Done);
                    this.set_reviewed(ix, !done, cx);
                })),
        )
    }

    /// "7 of 17 reviewed" with a bar under the files label, and Mark all / Unmark all.
    pub(super) fn render_review_progress(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let (done, total) = self.review_progress()?;
        let filtered = !self.pfilter.trim().is_empty();
        let shown = self.file_order();
        let shown_done = shown
            .iter()
            .all(|&ix| self.review_state(ix) == Some(State::Done));
        let action = |id: &'static str, label: String| {
            div()
                .id(id)
                .flex_none()
                .px(px(6.))
                .rounded(px(4.))
                .text_size(px(11.))
                .text_color(theme::muted())
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                .child(label)
        };
        let toggle = action(
            "review-all",
            match (shown_done, filtered) {
                (true, false) => "Unmark all".to_owned(),
                (true, true) => format!("Unmark {} shown", shown.len()),
                (false, false) => "Mark all".to_owned(),
                (false, true) => format!("Mark {} shown", shown.len()),
            },
        )
        .tooltip_show_delay(crate::ui::hover::LONG)
        .tooltip(|_, cx| {
            cx.new(|_| super::Tip("Mark or unmark every file in the list  ⌥⇧V"))
                .into()
        })
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_all_reviewed(cx)));
        Some(
            div()
                .flex_none()
                .px(px(14.))
                .pb(px(6.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(if done == total {
                                    theme::added()
                                } else {
                                    theme::muted()
                                })
                                .child(if done == total {
                                    format!("All {} reviewed ✓", plural(total, "file"))
                                } else {
                                    format!("{done}/{total} reviewed")
                                }),
                        )
                        .child(toggle),
                )
                .child(
                    div()
                        .h(px(4.))
                        .w_full()
                        .rounded(px(2.))
                        .bg(theme::hover())
                        // It grows to the new share (#56, 12).
                        .child(super::motion::toward_value(
                            div().h_full().rounded(px(2.)).bg(theme::added()),
                            "review-bar",
                            done as f32 / total.max(1) as f32,
                            super::motion::BASE,
                            |bar, share| bar.w(gpui::relative(share.clamp(0., 1.))),
                        )),
                ),
        )
    }

    /// The chip in the file header: mark it read, and the key.
    pub(super) fn render_review_chip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let state = self.review_state(self.file)?;
        Some(
            div()
                .id("review-chip")
                .flex_none()
                .h(px(24.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(ROW_RADIUS))
                .text_size(px(12.))
                .text_color(if state == State::Done {
                    theme::added()
                } else {
                    theme::muted()
                })
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                .child(match state {
                    State::Done => "✓ Reviewed",
                    State::Stale => "↻ Changed — mark again",
                    State::Open => "Mark reviewed",
                })
                .child(super::keys::caps(
                    if state == State::Done { "⇧V" } else { "V" },
                    super::keys::On::Plain,
                ))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    if this.review_state(this.file) == Some(State::Done) {
                        this.toggle_reviewed(cx)
                    } else {
                        this.mark_and_advance(cx)
                    }
                })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_survive_a_round_trip() {
        let mut store = Store::default();
        let head = git2::Oid::from_bytes(&[7; 20]).unwrap();
        store.marks.insert(
            "/r|mr:412".into(),
            HashMap::from([(
                "src/a.rs".to_owned(),
                Mark {
                    fp: 0xdead_beef_u64,
                    time: 1_792_149_840,
                    head: Some(head),
                },
            )]),
        );
        let again = Store::parse(&store.render());
        assert_eq!(again.get("/r|mr:412", "src/a.rs"), Some(0xdead_beef));
        assert_eq!(
            again.when("/r|mr:412", "src/a.rs"),
            Some((1_792_149_840, Some(head)))
        );
        assert_eq!(again.get("/r|mr:413", "src/a.rs"), None);
        // A line from before marks had a time still reads.
        let old = Store::parse("/r|mr:412\tsrc/a.rs\tdeadbeef\n");
        assert_eq!(old.get("/r|mr:412", "src/a.rs"), Some(0xdead_beef));
        assert_eq!(old.when("/r|mr:412", "src/a.rs"), None);
    }

    #[test]
    fn a_folder_is_marked_and_unmarked_at_once() {
        let mut store = Store::default();
        let files = [("ui/a.rs".to_owned(), 1), ("ui/b.rs".to_owned(), 2)];
        store.set_many("s", &files, true, None);
        assert_eq!(
            (store.get("s", "ui/a.rs"), store.get("s", "ui/b.rs")),
            (Some(1), Some(2))
        );
        store.set_many("s", &files[..1], false, None);
        assert_eq!(
            (store.get("s", "ui/a.rs"), store.get("s", "ui/b.rs")),
            (None, Some(2))
        );
    }

    #[test]
    fn a_reviewed_head_stops_counting_when_the_branch_moves() {
        let mut store = Store::default();
        let (old, new) = (
            git2::Oid::from_bytes(&[1; 20]).unwrap(),
            git2::Oid::from_bytes(&[2; 20]).unwrap(),
        );
        store.set_head("/r", old, true);
        assert!(store.head_done("/r", old));
        assert!(!store.head_done("/r", new));
        assert!(!store.head_done("/other", old));
        store.set_head("/r", old, false);
        assert!(!store.head_done("/r", old));
    }

    #[test]
    fn junk_lines_are_skipped() {
        let store = Store::parse("nonsense\nscope\tpath\tzz\nscope\tp\t1f\n");
        assert_eq!(store.get("scope", "p"), Some(0x1f));
        assert_eq!(store.get("scope", "path"), None);
    }
}
