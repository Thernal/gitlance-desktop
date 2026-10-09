//! A reviewer's marks: which files of a diff were read. A ✓ per file, a count in the files label,
//! `v` to mark the open file and go on to the next one not yet read. Kept per commit, version
//! range or merge request on this Mac; a file that changes in a newer version loses its mark.
//! Designed in `../Design/mockups/ide-ideas/a-ideas.html` (1).

use super::px;
use super::{Header, ROW_RADIUS, Workspace, plural, theme};
use crate::git::FileDiff;
use crate::storage;
use gpui::{ClickEvent, Context, FontWeight, IntoElement, MouseButton, div, prelude::*};
use std::collections::HashMap;

/// How a file stands in the review.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Open,
    Done,
    /// Marked, but the file is not what it was when it was marked.
    Stale,
}

/// Every mark ever made: per scope, per path, the fingerprint of the file when it was marked.
#[derive(Default)]
pub struct Store {
    marks: HashMap<String, HashMap<String, u64>>,
}

impl Store {
    pub fn load() -> Self {
        Self::parse(&storage::read("reviewed.txt"))
    }

    fn parse(text: &str) -> Self {
        let mut marks: HashMap<String, HashMap<String, u64>> = HashMap::new();
        for line in text.lines() {
            let mut parts = line.splitn(3, '\t');
            let (Some(scope), Some(path), Some(fp)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            if let Ok(fp) = u64::from_str_radix(fp, 16) {
                marks
                    .entry(scope.to_owned())
                    .or_default()
                    .insert(path.to_owned(), fp);
            }
        }
        Self { marks }
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for (scope, files) in &self.marks {
            for (path, fp) in files {
                out.push_str(&format!("{scope}\t{path}\t{fp:x}\n"));
            }
        }
        out
    }

    fn save(&self) {
        storage::save("reviewed.txt", self.render());
    }

    fn get(&self, scope: &str, path: &str) -> Option<u64> {
        self.marks.get(scope)?.get(path).copied()
    }

    fn set(&mut self, scope: &str, path: &str, fp: u64) {
        self.marks
            .entry(scope.to_owned())
            .or_default()
            .insert(path.to_owned(), fp);
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
        self.fps = match (&self.review_scope, &self.diff) {
            (Some(_), Some(d)) => d.files.iter().map(fingerprint).collect(),
            _ => Vec::new(),
        };
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
            self.reviewed.set(&scope, &path, fp);
        } else {
            self.reviewed.clear(&scope, &path);
        }
        cx.notify();
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

    /// The box at the left of a file row; a click marks or unmarks without opening the file.
    pub(super) fn review_box(
        &self,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let state = self.review_state(ix)?;
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
                .tooltip(move |_, cx| {
                    cx.new(|_| {
                        super::Tip(match state {
                            State::Open => "Mark as reviewed  ⇧V",
                            State::Done => "Reviewed — click to unmark  ⇧V",
                            State::Stale => "Changed since you marked it — mark again  ⇧V",
                        })
                    })
                    .into()
                })
                .child(glyph)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let done = this.review_state(ix) == Some(State::Done);
                    this.set_reviewed(ix, !done, cx);
                })),
        )
    }

    /// "7 of 17 reviewed" with a bar, under the files label; nothing until one is marked.
    pub(super) fn render_review_progress(&self) -> Option<impl IntoElement + use<>> {
        let (done, total) = self.review_progress().filter(|(d, _)| *d > 0)?;
        Some(
            div()
                .flex_none()
                .px(px(14.))
                .pb(px(6.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(div().text_size(px(11.)).text_color(theme::muted()).child(
                    if done == total {
                        format!("All {} reviewed ✓", plural(total, "file"))
                    } else {
                        format!("{done} of {total} reviewed")
                    },
                ))
                .child(
                    div()
                        .h(px(4.))
                        .w_full()
                        .rounded(px(2.))
                        .bg(theme::hover())
                        .child(
                            div()
                                .h_full()
                                .rounded(px(2.))
                                .bg(theme::added())
                                .w(gpui::relative(done as f32 / total.max(1) as f32)),
                        ),
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
                .child(
                    div()
                        .text_color(theme::faint())
                        .child(if state == State::Done { "⇧V" } else { "V" }),
                )
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
        store.marks.insert(
            "/r|mr:412".into(),
            HashMap::from([("src/a.rs".to_owned(), 0xdead_beef_u64)]),
        );
        let again = Store::parse(&store.render());
        assert_eq!(again.get("/r|mr:412", "src/a.rs"), Some(0xdead_beef));
        assert_eq!(again.get("/r|mr:413", "src/a.rs"), None);
    }

    #[test]
    fn junk_lines_are_skipped() {
        let store = Store::parse("nonsense\nscope\tpath\tzz\nscope\tp\t1f\n");
        assert_eq!(store.get("scope", "p"), Some(0x1f));
        assert_eq!(store.get("scope", "path"), None);
    }
}
