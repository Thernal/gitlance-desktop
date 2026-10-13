//! Line comments for a coding agent: a + in the gutter opens a composer under the line, comments sit
//! as threads under their lines, and a Review island lists them with *Copy for agent*. Comments are
//! kept on this Mac (`crate::review`), never in the reviewed repository. Designed in
//! `../Design/mockups/line-comments/a-agent-comments.html`.

use super::discussion::{self, Kind, Source};
use super::input::{self, Edit};
use super::markdown;
use super::px;
use super::rows::Row;
use super::{ROW_RADIUS, Selection, Workspace, format, theme};
use crate::markdown::Helper;
use crate::review::{self, Comment, Place};
use crate::textedit::TextEdit;
use gpui::{ClipboardItem, Context, KeyDownEvent, WeakEntity, div, prelude::*};

/// Who a comment is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// Kept on this Mac and copied for a coding agent.
    Agent,
    /// Posted to a merge request on GitLab, where everyone on it sees it.
    Request(u64),
    /// A reply to discussion number `0` of the open merge request.
    Reply(usize),
}

/// A comment being written.
pub struct Compose {
    path: String,
    old: bool,
    line: u32,
    code: String,
    /// The text, its caret and selection.
    pub edit: TextEdit,
    at: String,
    pub target: Target,
    /// Why the last attempt to post failed.
    error: Option<String>,
    posting: bool,
    /// The text is shown formatted instead of as written (⌘⇧P); typing goes on and the preview
    /// follows it.
    preview: bool,
}

impl Workspace {
    pub(super) fn current_path(&self) -> Option<String> {
        self.diff
            .as_ref()
            .and_then(|d| d.files.get(self.file))
            .map(|f| f.path().to_owned())
    }

    /// The comment marks of the open file's gutter: one per line with comments, in the colour of
    /// the most important kind there (the request's, then one's pending, then the agent's).
    pub(super) fn line_notes(&self) -> Vec<super::diff_view::LineNote> {
        use super::diff_view::{LineNote, NoteTone};
        let mut out: Vec<LineNote> = Vec::new();
        let mut add = |old: bool, line: u32, tone: NoteTone| match out
            .iter_mut()
            .find(|n| n.old == old && n.line == line)
        {
            Some(n) => {
                n.count += 1;
                let rank = |t: NoteTone| match t {
                    NoteTone::Request => 0,
                    NoteTone::Draft => 1,
                    NoteTone::Agent => 2,
                    NoteTone::Bookmark => 3,
                };
                if rank(tone) < rank(n.tone) {
                    n.tone = tone;
                }
            }
            None => out.push(LineNote {
                old,
                line,
                tone,
                count: 1,
            }),
        };
        for c in &self.comments {
            if let Some(Place::Line(n)) = self.place_in_open_file(c) {
                add(c.old, n, NoteTone::Agent);
            }
        }
        for line in self.bookmarks_here() {
            add(false, line, NoteTone::Bookmark);
        }
        if self.showing_request() {
            for (_, old, line) in self.request_threads_here() {
                add(old, line, NoteTone::Request);
            }
            for (_, old, line) in self.drafts_here() {
                add(old, line, NoteTone::Draft);
            }
        }
        out
    }

    /// A click on a line's comment mark: folds what is open there, or opens what is folded.
    pub(super) fn toggle_notes_at(&mut self, old: bool, line: u32, cx: &mut Context<Self>) {
        // A line with nothing but a bookmark: the click takes the mark off.
        let only_bookmark = self.notes.iter().any(|n| {
            n.old == old
                && n.line == line
                && n.tone == super::diff_view::NoteTone::Bookmark
                && n.count == 1
        });
        if only_bookmark {
            return self.toggle_bookmark_at(line, cx);
        }
        let mine: Vec<usize> = if self.showing_request() {
            self.request_threads_here()
                .into_iter()
                .filter(|(_, o, n)| *o == old && *n == line)
                .map(|(ix, _, _)| ix)
                .collect()
        } else {
            Vec::new()
        };
        let agent: Vec<u64> = self
            .comments
            .iter()
            .filter(|c| {
                c.old == old
                    && matches!(self.place_in_open_file(c), Some(Place::Line(n)) if n == line)
            })
            .map(|c| c.id)
            .collect();
        let any_open = mine.iter().any(|&ix| self.mr_open(ix))
            || agent.iter().any(|id| !self.toggled_agent.contains(id));
        for ix in mine {
            // Everything there ends up the other way round from how most of it was.
            if self.mr_open(ix) == any_open && !self.toggled_mr.remove(&ix) {
                self.toggled_mr.insert(ix);
            }
        }
        for id in agent {
            if any_open {
                self.toggled_agent.insert(id);
            } else {
                self.toggled_agent.remove(&id);
            }
        }
        self.refresh_rows();
        cx.notify();
    }

    /// Where `comment` sits in the open file now (`None` when it belongs to another file).
    fn place_in_open_file(&self, comment: &Comment) -> Option<Place> {
        let data = self.data.as_ref()?;
        if Some(&comment.path) != self.current_path().as_ref() {
            return None;
        }
        let side = data.side(comment.old);
        Some(review::place(comment, side.count(), |n| side.line(n).0))
    }

    /// The rows with each comment's thread, and the composer, put under their lines.
    pub(super) fn with_comments(&self, rows: Vec<Row>) -> Vec<Row> {
        let here: Vec<(u64, bool, u32)> = self
            .comments
            .iter()
            .filter_map(|c| match self.place_in_open_file(c)? {
                Place::Line(n) => Some((c.id, c.old, n)),
                Place::Outdated => None,
            })
            .collect();
        let compose = self
            .compose
            .as_ref()
            .filter(|c| Some(&c.path) == self.current_path().as_ref());
        let theirs = if self.showing_request() {
            self.request_threads_here()
        } else {
            Vec::new()
        };
        let drafts = if self.showing_request() {
            self.drafts_here()
        } else {
            Vec::new()
        };
        // The prompt of `c` hangs under the marked line, like the composer that follows it.
        let go = if compose.is_none() && self.goline.is_some() {
            self.goline_target()
        } else {
            None
        };
        if here.is_empty()
            && compose.is_none()
            && theirs.is_empty()
            && drafts.is_empty()
            && go.is_none()
        {
            return rows;
        }
        let mut out = Vec::with_capacity(rows.len() + here.len() + 1);
        for row in rows {
            out.push(row);
            let cells = match row {
                Row::Split { left, right } => {
                    [left.map(|c| (true, c.line)), right.map(|c| (false, c.line))]
                }
                Row::Unified { cell, old, .. } => [Some((old, cell.line)), None],
                _ => continue,
            };
            for (old, line) in cells.into_iter().flatten() {
                out.extend(
                    here.iter()
                        .filter(|(_, o, n)| *o == old && *n == line)
                        .map(|(id, _, _)| Row::Thread(*id)),
                );
                out.extend(
                    theirs
                        .iter()
                        .filter(|(_, o, n)| *o == old && *n == line)
                        .map(|(ix, _, _)| Row::Request(*ix)),
                );
                out.extend(
                    drafts
                        .iter()
                        .filter(|(_, o, n)| *o == old && *n == line)
                        .map(|(ix, _, _)| Row::Draft(*ix)),
                );
                if compose.is_some_and(|c| {
                    c.old == old && c.line == line && !matches!(c.target, Target::Reply(_))
                }) || go == Some((old, line))
                {
                    out.push(Row::Composer);
                }
            }
        }
        out
    }

    /// Lays the rows out again after a comment changed, keeping the scroll position.
    pub(super) fn refresh_rows(&mut self) {
        let top = self.diff_list.logical_scroll_top();
        self.relayout();
        self.diff_list.scroll_to(top);
    }

    pub(super) fn start_comment(&mut self, old: bool, line: u32, cx: &mut Context<Self>) {
        let (Some(data), Some(path)) = (&self.data, self.current_path()) else {
            return;
        };
        let code = data.side(old).line(line).0.to_owned();
        let at = match self.selection {
            Selection::Commit(ix) => self
                .commits
                .get(ix)
                .map(|c| format::short(c.id))
                .unwrap_or_default(),
            Selection::Versions { from, to } => {
                match (self.versions.get(from), self.versions.get(to)) {
                    (Some(a), Some(b)) => format!("v{}→v{}", a.number, b.number),
                    _ => String::new(),
                }
            }
            Selection::WorkingTree => "working tree".to_owned(),
            Selection::None => String::new(),
        };
        // A run of comments to one merge request does not have to pick it every time.
        let target = match self.request_target() {
            Some(iid) if self.last_request_target == Some(iid) => Target::Request(iid),
            _ => Target::Agent,
        };
        self.compose = Some(Compose {
            path,
            old,
            line,
            code,
            edit: TextEdit::default(),
            at,
            target,
            error: None,
            posting: false,
            preview: false,
        });
        self.refresh_rows();
        cx.notify();
    }

    /// Starts a reply under discussion `ix` of the open merge request.
    pub(super) fn start_reply(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(t) = self.requests.threads.get(ix) else {
            return;
        };
        let (Some(path), Some(line)) = (t.path.clone(), t.new_line.or(t.old_line)) else {
            return;
        };
        let old = t.new_line.is_none();
        self.compose = Some(Compose {
            path,
            old,
            line,
            code: String::new(),
            edit: TextEdit::default(),
            at: String::new(),
            target: Target::Reply(ix),
            error: None,
            posting: false,
            preview: false,
        });
        self.reset_field_edit();
        self.thread_at = Some(ix);
        if !self.mr_open(ix) {
            self.toggled_mr.insert(ix);
        }
        self.refresh_rows();
        cx.notify();
    }

    /// The merge request whose diff is open, when a comment can be posted to it.
    pub(super) fn request_target(&self) -> Option<u64> {
        match self.diff.as_ref()?.header {
            super::Header::Compare {
                request: Some(iid),
                since_merge_base: true,
                ..
            } if self
                .web
                .as_ref()
                .is_some_and(|w| crate::mr::token(w).is_some())
                || crate::mr::fixture_on() =>
            {
                Some(iid)
            }
            _ => None,
        }
    }

    fn toggle_target(&mut self, cx: &mut Context<Self>) {
        let request = self.request_target();
        if let Some(compose) = self.compose.as_mut()
            && !compose.posting
        {
            compose.target = match (compose.target, request) {
                (Target::Reply(ix), _) => Target::Reply(ix),
                (Target::Agent, Some(iid)) => Target::Request(iid),
                _ => Target::Agent,
            };
            compose.error = None;
            cx.notify();
        }
    }

    pub(super) fn cancel_comment(&mut self, cx: &mut Context<Self>) {
        if self.compose.take().is_some() {
            self.refresh_rows();
            cx.notify();
        }
    }

    pub(super) fn add_comment(&mut self, cx: &mut Context<Self>) {
        match self.compose.as_ref().map(|c| c.target) {
            Some(Target::Request(iid)) => return self.post_comment(iid, false, cx),
            Some(Target::Reply(ix)) => return self.post_reply(ix, cx),
            _ => {}
        }
        let Some(compose) = self.compose.take() else {
            return;
        };
        if compose.edit.text().trim().is_empty() {
            self.compose = Some(compose);
            return;
        }
        self.last_request_target = None;
        let id = self.comments.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        self.comments.push(Comment {
            id,
            path: compose.path,
            old: compose.old,
            line: compose.line,
            code: compose.code,
            body: compose.edit.text().trim().to_owned(),
            at: compose.at,
        });
        self.save_comments();
        self.copied = false;
        self.refresh_rows();
        cx.notify();
    }

    /// Posts the comment being written to merge request `iid`, on its line.
    /// With `draft`, as a pending comment that only its author sees until the review is submitted.
    pub(super) fn post_comment(&mut self, iid: u64, draft: bool, cx: &mut Context<Self>) {
        let (Some(remote), Some(compose)) = (self.web.clone(), self.compose.as_ref()) else {
            return;
        };
        let body = compose.edit.text().trim().to_owned();
        let head = match self.diff.as_ref().map(|d| &d.header) {
            Some(super::Header::Compare { head, .. }) => head.1.to_string(),
            _ => return,
        };
        let file = self
            .diff
            .as_ref()
            .and_then(|d| d.files.get(self.file))
            .map(|f| (f.new_path.clone(), f.old_path.clone()));
        if body.is_empty() || compose.posting {
            return;
        }
        let Some((new_path, old_path)) = file else {
            return;
        };
        self.last_request_target = Some(iid);
        let (old_line, new_line) = self.line_pair(compose.line, compose.old);
        let path = new_path.clone().or(old_path.clone()).unwrap_or_default();
        let old_path = old_path.or(new_path).unwrap_or_default();
        self.submit_compose(cx, move || {
            crate::mr::post_discussion(
                &remote,
                iid,
                &head,
                &crate::mr::Place {
                    path: &path,
                    old_path: &old_path,
                    new_line,
                    old_line,
                },
                &body,
                draft,
            )
        });
    }

    /// Writes the reply being composed to the discussion it answers.
    fn post_reply(&mut self, ix: usize, cx: &mut Context<Self>) {
        let (Some(remote), Some(compose), Some(thread), Some(iid)) = (
            self.web.clone(),
            self.compose.as_ref(),
            self.requests.threads.get(ix),
            self.requests.current,
        ) else {
            return;
        };
        let (body, id) = (compose.edit.text().trim().to_owned(), thread.id.clone());
        if body.is_empty() || compose.posting {
            return;
        }
        self.submit_compose(cx, move || crate::mr::reply(&remote, iid, &id, &body));
    }

    /// Runs `work` away from the interface while the composer shows "Posting…"; on success the
    /// composer closes and the request's discussions are read again, on failure the reason stays in
    /// the composer with the text.
    fn submit_compose(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
    ) {
        if let Some(c) = self.compose.as_mut() {
            c.posting = true;
            c.error = None;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { work() }).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.compose = None;
                        this.refresh_rows();
                        this.reload_threads(cx);
                    }
                    Err(e) => {
                        if let Some(c) = this.compose.as_mut() {
                            c.posting = false;
                            c.error = Some(format!("{e:#}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The old and the new line numbers GitLab wants for a line: both for an unchanged line, one
    /// for an added or removed line.
    fn line_pair(&self, line: u32, old: bool) -> (Option<u32>, Option<u32>) {
        use crate::git::LineKind;
        let single = if old {
            (Some(line), None)
        } else {
            (None, Some(line))
        };
        let Some(at) = self.row_of_line(line, old) else {
            return single;
        };
        match self.rows[at] {
            Row::Split {
                left: Some(l),
                right: Some(r),
            } if l.kind == LineKind::Context => (Some(l.line), Some(r.line)),
            Row::Unified {
                cell,
                old_line: Some(o),
                new_line: Some(n),
                ..
            } if cell.kind == LineKind::Context => (Some(o), Some(n)),
            _ => single,
        }
    }

    // ---- a comment by line number ------------------------------------------------------------

    /// `c`: a marker appears on the diff; typing a line number moves it, ↵ opens the comment there.
    pub(super) fn open_goline(&mut self, cx: &mut Context<Self>) {
        if self.data.is_none() || self.settings_open || self.compose.is_some() {
            return;
        }
        self.reset_field_edit();
        self.zone = super::zones::Zone::Diff;
        self.goline = Some(String::new());
        self.move_marker();
        cx.notify();
    }

    pub(super) fn goline_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(text) = self.goline.as_mut() else {
            return;
        };
        let key = event.keystroke.key.as_str();
        if matches!(key, "up" | "down") {
            let step: i64 = if key == "up" { -1 } else { 1 };
            if let Some((old, line)) = self.goline_target() {
                let next = (i64::from(line) + step).max(1);
                self.goline = Some(format!("{}{next}", if old { "-" } else { "" }));
                self.move_marker();
            }
            return cx.notify();
        }
        match input::edit(text, &mut self.field_edit, &event.keystroke, cx) {
            Edit::Escape => {
                self.goline = None;
                self.goline_row = None;
                self.refresh_rows();
            }
            // Digits, and a minus first for a removed line.
            Edit::Changed => {
                let keep = |(i, c): &(usize, char)| c.is_ascii_digit() || (*i == 0 && *c == '-');
                let filtered: String = text
                    .chars()
                    .enumerate()
                    .filter(keep)
                    .map(|(_, c)| c)
                    .collect();
                if filtered != *text {
                    // The caret stays after the chars it followed.
                    let caret = self.field_edit.caret();
                    let before = text[..caret].chars().enumerate().filter(keep).count();
                    self.field_edit = crate::textedit::TextEdit::new(filtered.as_str());
                    self.field_edit.place(before, false);
                    *text = filtered;
                }
                self.move_marker();
            }
            Edit::Enter { .. } => return self.finish_goline(cx),
            Edit::Selected | Edit::Ignored => {}
        }
        cx.notify();
    }

    /// The side and line the typed number stands for; with nothing typed, the first change.
    fn goline_target(&self) -> Option<(bool, u32)> {
        let text = self.goline.as_deref()?;
        if text.trim().is_empty() {
            let first = self
                .changes
                .first()
                .and_then(|&row| self.rows.get(row))
                .copied();
            return match first {
                Some(Row::Split { right: Some(c), .. }) => Some((false, c.line)),
                Some(Row::Split { left: Some(c), .. }) => Some((true, c.line)),
                Some(Row::Unified { cell, old, .. }) => Some((old, cell.line)),
                _ => None,
            };
        }
        let n = text.trim_start_matches('-').parse::<u32>().ok()?;
        Some((text.starts_with('-'), n))
    }

    /// Puts the marker on the row of the typed line and scrolls it into view.
    fn move_marker(&mut self) {
        // The prompt row moves with the marker, so the rows are laid out again first.
        self.refresh_rows();
        self.goline_row = self
            .goline_target()
            .and_then(|(old, line)| self.row_of_line(line, old));
        if let Some(at) = self.goline_row {
            // The marked line and the prompt under it.
            self.reveal_rows(at..at + 2);
        } else if let Some(gap) = self
            .goline_target()
            .and_then(|(old, line)| self.gap_of(line, old))
        {
            // Folded away: the run that holds it, which ↵ opens.
            self.reveal_rows(gap..gap + 1);
        }
    }

    /// The row of the folded run of unchanged lines that holds `line`.
    fn gap_of(&self, line: u32, old: bool) -> Option<usize> {
        let data = self.data.as_ref()?;
        self.rows.iter().position(|r| match r {
            Row::Gap { segment, .. } => matches!(
                data.segments.get(*segment),
                Some(super::rows::Segment::Same { old: o, new: n, len })
                    if (if old { *o } else { *n }) <= line && line < (if old { *o } else { *n }) + len
            ),
            _ => false,
        })
    }

    /// Opens the composer under the marked line.
    fn finish_goline(&mut self, cx: &mut Context<Self>) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let Some((old, line)) = self.goline_target() else {
            return cx.notify();
        };
        if line == 0 || line > data.side(old).count() {
            self.error = Some(
                format!(
                    "This file has {} {} lines.",
                    data.side(old).count(),
                    if old { "old" } else { "new" }
                )
                .into(),
            );
            return cx.notify();
        }
        self.goline = None;
        self.goline_row = None;
        self.reveal_line(line, old);
        self.start_comment(old, line, cx);
        if let Some(at) = self.row_of_line(line, old) {
            self.reveal_rows(at..at + 2);
        }
    }

    /// Opens the collapsed run of unchanged lines that holds `line`, if there is one.
    fn reveal_line(&mut self, line: u32, old: bool) {
        if self.row_of_line(line, old).is_some() {
            return;
        }
        let Some(data) = &self.data else {
            return;
        };
        let hit = data.segments.iter().position(|s| match s {
            super::rows::Segment::Same {
                old: o,
                new: n,
                len,
            } => {
                let start = if old { *o } else { *n };
                line >= start && line < start + len
            }
            super::rows::Segment::Change(_) => false,
        });
        if let Some(segment) = hit {
            self.expanded.insert(segment);
            self.relayout();
        }
    }

    /// The prompt of `c` under the marked line.
    pub(super) fn render_goline_row(
        &self,
        indent: f32,
        this: WeakEntity<Self>,
    ) -> gpui::AnyElement {
        let Some(text) = self.goline.as_ref() else {
            return div().into_any_element();
        };
        div()
            .w_full()
            .pl(px(indent))
            .pr(px(12.))
            .py(px(4.))
            .child(
                div()
                    .max_w(px(640.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py(px(6.))
                    .rounded(px(ROW_RADIUS))
                    .border_2()
                    .border_color(theme::focus())
                    .bg(theme::panel())
                    .font_family(theme::UI_FONT)
                    .text_size(px(12.))
                    .child(div().text_color(theme::muted()).child("Comment on line"))
                    .child(div().min_w(px(56.)).font_family(theme::CODE_FONT).child(
                        self.field_look(
                            text,
                            true,
                            "first change",
                            Default::default(),
                            this.clone(),
                        ),
                    ))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child("↵ write · ↑↓ step · −12 a removed line · esc"),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_goline(&self, this: WeakEntity<Self>) -> Option<impl IntoElement + use<>> {
        if self.goline_row.is_some() {
            return None;
        }
        let text = self.goline.as_ref()?;
        let target = self.goline_target();
        let preview = target.and_then(|(old, line)| {
            let data = self.data.as_ref()?;
            (line >= 1 && line <= data.side(old).count())
                .then(|| data.side(old).line(line).0.trim().to_owned())
        });
        let hint = match (target, &preview, self.goline_row) {
            (None, ..) if !text.is_empty() => "type a line number".to_owned(),
            (None, ..) => "no changes in this file".to_owned(),
            (Some(_), None, _) => "no such line in this file".to_owned(),
            (Some(_), Some(_), None) => "inside unchanged lines · ↵ opens them".to_owned(),
            _ => "↵ comment here".to_owned(),
        };
        Some(
            div()
                .absolute()
                .bottom(px(16.))
                .left(px(16.))
                .max_w(px(520.))
                .flex()
                .flex_col()
                .gap_1()
                .px_3()
                .py_2()
                .rounded(px(ROW_RADIUS))
                .border_2()
                .border_color(theme::focus())
                .bg(theme::panel())
                .shadow_lg()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_color(theme::muted()).child("Comment on line"))
                        .child(div().min_w(px(60.)).font_family(theme::CODE_FONT).child(
                            self.field_look(
                                text,
                                true,
                                "first change",
                                Default::default(),
                                this.clone(),
                            ),
                        ))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(hint),
                        ),
                )
                .children(preview.map(|code| {
                    div()
                        .truncate()
                        .font_family(theme::CODE_FONT)
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(code)
                }))
                .child(
                    div().text_size(px(11.)).text_color(theme::faint()).child(
                        "digits move the marker · ↑↓ step · −12 a removed line · ↵ open · esc",
                    ),
                ),
        )
    }

    pub(super) fn delete_comment(&mut self, id: u64, cx: &mut Context<Self>) {
        self.comments.retain(|c| c.id != id);
        self.save_comments();
        self.copied = false;
        self.refresh_rows();
        cx.notify();
    }

    fn save_comments(&self) {
        if let Some(root) = &self.root {
            review::save(root, &self.comments);
        }
    }

    /// Typing into the composer: a real text field (caret, selection, ⌥ words, ⌘ lines, undo),
    /// return carries a list on, ⇧↵ a plain new line; ⌘B ⌘I ⌘⇧X ⌘⇧E ⌘⇧K for markdown, ⌘⇧P to
    /// preview it, Tab for who sees it, ⌘↵ to send, ⇧⌘↵ to add it to the review, esc to cancel.
    pub(super) fn compose_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let m = key.modifiers;
        if self.compose.as_ref().is_none_or(|c| c.posting) {
            return;
        }
        if key.key == "enter" && m.platform {
            if m.shift
                && let Some(Target::Request(iid)) = self.compose.as_ref().map(|c| c.target)
            {
                return self.post_comment(iid, true, cx);
            }
            return self.add_comment(cx);
        }
        if key.key == "tab" {
            return self.toggle_target(cx);
        }
        if self.vertical_key(key, cx) {
            return;
        }
        if m.platform && !m.alt && !m.control {
            let helper = match (key.key.as_str(), m.shift) {
                ("b", false) => Some(Helper::Bold),
                ("i", false) => Some(Helper::Italic),
                ("x", true) => Some(Helper::Strike),
                ("e", true) => Some(Helper::Code),
                ("k", true) => Some(Helper::Link),
                _ => None,
            };
            if let Some(helper) = helper {
                return self.apply_helper(helper, cx);
            }
            if key.key == "p" && m.shift {
                return self.toggle_preview(cx);
            }
        }
        let Some(compose) = self.compose.as_mut() else {
            return;
        };
        match input::edit_text(&mut compose.edit, key, cx) {
            Edit::Escape => return self.cancel_comment(cx),
            Edit::Ignored => return,
            Edit::Enter { shift: true } => compose.edit.insert("\n"),
            Edit::Enter { shift: false } => crate::markdown::newline(&mut compose.edit),
            Edit::Changed | Edit::Selected => {}
        }
        // The box grows with its text.
        self.diff_list.remeasure();
        cx.notify();
    }

    /// ↑ ↓ and ⌘← ⌘→ (⇧ selecting) in a wrapped field — the composer, a review's summary, a
    /// description: by the lines as drawn. Any other key forgets the x ↑ ↓ keep. Returns whether
    /// it took the key.
    pub(super) fn vertical_key(&mut self, key: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        use input::Drawn;
        let m = key.modifiers;
        let motion = match (key.key.as_str(), m.platform, m.alt) {
            ("up", false, false) => Drawn::Up,
            ("down", false, false) => Drawn::Down,
            ("left", true, false) => Drawn::Start,
            ("right", true, false) => Drawn::End,
            _ => {
                self.field_goal = None;
                return false;
            }
        };
        if matches!(motion, Drawn::Start | Drawn::End) {
            self.field_goal = None;
        }
        let Some(layout) = self.field_lines.borrow().clone() else {
            return false;
        };
        // A card's field (the review summary, a description) is over the composer when both are open.
        let moved = if let Some(text) = self.review_summary().or_else(|| self.description_text()) {
            input::sync(&text, &mut self.field_edit);
            input::drawn(
                &mut self.field_edit,
                &layout,
                &mut self.field_goal,
                motion,
                m.shift,
            )
        } else if let Some(compose) = self.compose.as_mut() {
            input::drawn(
                &mut compose.edit,
                &layout,
                &mut self.field_goal,
                motion,
                m.shift,
            )
        } else {
            false
        };
        if moved {
            cx.notify();
        }
        moved
    }

    /// A click in the composer's text: the caret goes there; ⇧ extends the selection, a double
    /// click takes the word; a drag (`drag`) selects.
    fn compose_place(
        &mut self,
        at: usize,
        shift: bool,
        clicks: usize,
        drag: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(compose) = self.compose.as_mut() else {
            return;
        };
        compose.edit.click(at, clicks, shift || drag);
        self.field_goal = None;
        cx.notify();
    }

    /// A markdown helper of the composer's toolbar, on the selection or at the caret.
    fn apply_helper(&mut self, helper: Helper, cx: &mut Context<Self>) {
        let Some(compose) = self.compose.as_mut().filter(|c| !c.posting) else {
            return;
        };
        crate::markdown::apply(&mut compose.edit, helper);
        self.diff_list.remeasure();
        cx.notify();
    }

    fn toggle_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(compose) = self.compose.as_mut() {
            compose.preview = !compose.preview;
            self.diff_list.remeasure();
            cx.notify();
        }
    }

    /// ⌥M: comments drawn as formatted markdown, or as the text written.
    pub(super) fn toggle_rich_comments(&mut self, cx: &mut Context<Self>) {
        self.settings.rich_comments = !self.settings.rich_comments;
        self.settings.save();
        self.diff_list.remeasure();
        cx.notify();
    }

    /// A comment on one line, for a folded thread or the drawer: without its markdown when Rich.
    pub(super) fn one_line(&self, body: &str) -> String {
        if self.settings.rich_comments {
            crate::markdown::plain(body)
        } else {
            body.replace('\n', " ")
        }
    }

    /// The project's web address, which relative links in a comment are relative to.
    pub(super) fn link_base(&self) -> Option<&str> {
        self.web.as_ref().map(|w| w.base.as_str())
    }

    /// The *Raw* / *Rich* switch beside a comment that has markdown.
    pub(super) fn rich_switch(&self, id: impl Into<gpui::ElementId>) -> gpui::Stateful<gpui::Div> {
        discussion::lite(
            id,
            if self.settings.rich_comments {
                "Raw"
            } else {
                "Rich"
            },
        )
        .tooltip(|_, cx| {
            cx.new(|_| super::Tip("Markdown as written or formatted  ⌥M"))
                .into()
        })
    }

    /// The comments as markdown, copied for pasting into an agent.
    pub(super) fn copy_for_agent(&mut self, cx: &mut Context<Self>) {
        let repo = self
            .root
            .as_ref()
            .and_then(|r| r.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let comments: Vec<(Comment, Place)> = self
            .comments
            .iter()
            .map(|c| (c.clone(), self.place_of(c)))
            .collect();
        cx.write_to_clipboard(ClipboardItem::new_string(review::export(&repo, &comments)));
        self.copied = true;
        cx.notify();
    }

    /// A comment's place for the export and the list: exact in the open file, the written line
    /// elsewhere (the other files are not loaded).
    fn place_of(&self, comment: &Comment) -> Place {
        self.place_in_open_file(comment)
            .unwrap_or(Place::Line(comment.line))
    }

    /// An agent note, drawn under its line: a purple discussion frame with one note.
    pub(super) fn render_thread(
        &self,
        id: u64,
        indent: f32,
        this: WeakEntity<Self>,
    ) -> gpui::AnyElement {
        let Some(comment) = self.comments.iter().find(|c| c.id == id) else {
            return div().into_any_element();
        };
        let open = !self.toggled_agent.contains(&id);
        let (fold, delete, raw) = (this.clone(), this.clone(), this);
        let bar = discussion::bar(("agent-bar", id), open)
            .child(discussion::source_pill(Source::Agent))
            .child(div().flex_1().min_w_0().truncate().child(if open {
                format!("· stays on this Mac · at {}", comment.at)
            } else {
                self.one_line(&comment.body)
            }))
            .children((open && markdown::has_markdown(&comment.body)).then(|| {
                self.rich_switch(("agent-rich", id))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        raw.update(cx, |this, cx| this.toggle_rich_comments(cx))
                            .ok();
                    })
            }))
            .child(
                discussion::button(("delete-comment", id), "Delete", None, Kind::Danger)
                    .h(px(24.))
                    .px(px(8.))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        delete
                            .update(cx, |this, cx| this.delete_comment(id, cx))
                            .ok();
                    }),
            )
            .on_click(move |_, _, cx| {
                fold.update(cx, |this, cx| {
                    if !this.toggled_agent.remove(&id) {
                        this.toggled_agent.insert(id);
                    }
                    this.refresh_rows();
                    cx.notify();
                })
                .ok();
            });
        let body = discussion::note(
            &self.me,
            true,
            None,
            comment.at.clone(),
            markdown::body(
                &comment.body,
                self.settings.rich_comments,
                format!("md-agent-{id}"),
                None,
            ),
            true,
        );
        div()
            .w_full()
            .pl(px(indent))
            .pr(px(12.))
            .py(px(6.))
            .child(
                discussion::frame(Source::Agent, false)
                    .child(bar)
                    .children(open.then_some(body)),
            )
            .into_any_element()
    }

    /// The composer of a new comment, drawn under its line: my avatar beside the box. A reply is
    /// not drawn here but inside its discussion (`requests::render_request_thread`).
    pub(super) fn render_composer(&self, indent: f32, this: WeakEntity<Self>) -> gpui::AnyElement {
        let Some(boxed) = self.compose_box(this) else {
            return div().into_any_element();
        };
        div()
            .w_full()
            .pl(px(indent))
            .pr(px(12.))
            .py(px(6.))
            .child(
                div()
                    .max_w(px(760.))
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(discussion::avatar(&self.me, true, 28.).mt(px(3.)))
                    .child(boxed),
            )
            .into_any_element()
    }

    /// The new comment's input: the one input component (variant D) with who sees it on its edge.
    pub(super) fn compose_box(&self, this: WeakEntity<Self>) -> Option<gpui::Div> {
        let compose = self.compose.as_ref()?;
        let request = self.request_target();
        let source = match compose.target {
            Target::Agent => Source::Agent,
            Target::Request(_) | Target::Reply(_) => Source::GitLab,
        };
        let tone = source.color();
        let (cancel, add, review, audience) =
            (this.clone(), this.clone(), this.clone(), this.clone());
        let switchable = request.is_some();
        let chip = div()
            .id("compose-audience")
            .h(px(26.))
            .px(px(8.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .rounded(px(6.))
            .bg(match source {
                Source::Agent => theme::focus_bg(),
                _ => theme::warning_bg(),
            })
            .text_size(px(12.))
            .text_color(tone)
            .whitespace_nowrap()
            .child(div().size(px(7.)).rounded_full().bg(tone))
            .child(match compose.target {
                Target::Request(iid) => format!("Everyone on !{iid}"),
                _ => "Your agent".to_owned(),
            })
            .when(switchable, |s| {
                s.cursor_pointer()
                    .child(super::icons::icon("chevron").size(px(12.)).text_color(tone))
                    .tooltip(|_, cx| cx.new(|_| super::Tip("Who sees it — Tab switches")).into())
                    .on_click(move |_, _, cx| {
                        audience.update(cx, |this, cx| this.toggle_target(cx)).ok();
                    })
            });
        let primary = match (&compose.target, compose.posting) {
            (_, true) => "Posting…",
            (Target::Agent, _) => "Add for agent",
            (_, _) => "Comment",
        };
        let mut tail = vec![
            discussion::button("compose-cancel", "Cancel", None, Kind::Quiet)
                .h(px(28.))
                .px(px(10.))
                .tooltip(|_, cx| cx.new(|_| super::Tip("Esc")).into())
                .on_click(move |_, _, cx| {
                    cancel.update(cx, |this, cx| this.cancel_comment(cx)).ok();
                })
                .into_any_element(),
        ];
        if let (Target::Request(iid), false) = (compose.target, compose.posting) {
            tail.push(
                discussion::button("compose-review", "Add to review", Some("⇧⌘↵"), Kind::Plain)
                    .h(px(28.))
                    .px(px(10.))
                    .tooltip(|_, cx| {
                        cx.new(|_| super::Tip("Pending until you submit the review"))
                            .into()
                    })
                    .on_click(move |_, _, cx| {
                        review
                            .update(cx, |this, cx| this.post_comment(iid, true, cx))
                            .ok();
                    })
                    .into_any_element(),
            );
        }
        tail.push(
            discussion::button(
                "compose-add",
                primary,
                (!compose.posting).then_some("⌘↵"),
                Kind::Primary(tone),
            )
            .h(px(28.))
            .px(px(10.))
            .on_click(move |_, _, cx| {
                add.update(cx, |this, cx| this.add_comment(cx)).ok();
            })
            .into_any_element(),
        );
        let placeholder = match compose.target {
            Target::Agent => "A note for your agent — it stays on this Mac",
            _ => "Comment on the merge request — everyone on it sees it",
        };
        Some(self.open_input(
            compose,
            tone,
            placeholder,
            Some(chip.into_any_element()),
            &[Helper::Bold, Helper::Code, Helper::Link, Helper::Bullet],
            tail,
            this,
        ))
    }

    /// A reply: the discussion's own one-line field, grown inside its outline (variant D).
    pub(super) fn reply_editor(&self, this: WeakEntity<Self>) -> Option<gpui::Div> {
        let compose = self.compose.as_ref()?;
        let tone = Source::GitLab.color();
        let (cancel, send) = (this.clone(), this.clone());
        let tail = vec![
            discussion::button("reply-cancel", "Cancel", None, Kind::Quiet)
                .h(px(28.))
                .px(px(10.))
                .tooltip(|_, cx| cx.new(|_| super::Tip("Esc")).into())
                .on_click(move |_, _, cx| {
                    cancel.update(cx, |this, cx| this.cancel_comment(cx)).ok();
                })
                .into_any_element(),
            discussion::button(
                "reply-send",
                if compose.posting {
                    "Posting…"
                } else {
                    "Reply"
                },
                (!compose.posting).then_some("⌘↵"),
                Kind::Primary(tone),
            )
            .h(px(28.))
            .px(px(10.))
            .on_click(move |_, _, cx| {
                send.update(cx, |this, cx| this.add_comment(cx)).ok();
            })
            .into_any_element(),
        ];
        Some(self.open_input(
            compose,
            tone,
            "Reply…",
            None,
            &[
                Helper::Bold,
                Helper::Italic,
                Helper::Code,
                Helper::Link,
                Helper::Quote,
                Helper::Bullet,
                Helper::CodeBlock,
            ],
            tail,
            this,
        ))
    }

    /// The one input component, open: one outline in the audience's colour, the text (or its
    /// preview), and along its bottom edge `lead`, the markdown `tools`, Preview, then `tail`.
    #[allow(clippy::too_many_arguments)]
    fn open_input(
        &self,
        compose: &Compose,
        tone: gpui::Rgba,
        placeholder: &'static str,
        lead: Option<gpui::AnyElement>,
        tools: &[Helper],
        tail: Vec<gpui::AnyElement>,
        this: WeakEntity<Self>,
    ) -> gpui::Div {
        let text = if compose.preview {
            div()
                .min_h(px(66.))
                .px(px(10.))
                .py(px(7.))
                .child(if compose.edit.text().trim().is_empty() {
                    div().text_color(theme::muted()).child("Nothing to preview")
                } else {
                    div().child(markdown::body(
                        compose.edit.text(),
                        true,
                        "md-compose",
                        self.link_base(),
                    ))
                })
                .into_any_element()
        } else {
            self.render_edit_text(compose, placeholder, true, this.clone())
        };
        let tool = |h: Helper, this: WeakEntity<Self>| {
            let (icon, tip) = markdown::helper_look(h);
            div()
                .id(("md-tool", h as usize))
                .size(px(26.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(
                    super::icons::icon(icon)
                        .size(px(15.))
                        .text_color(theme::muted()),
                )
                .tooltip(move |_, cx| cx.new(|_| super::Tip(tip)).into())
                .on_click(move |_, _, cx| {
                    this.update(cx, |this, cx| this.apply_helper(h, cx)).ok();
                })
        };
        let preview = compose.preview;
        let toggle = this.clone();
        let edge = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(6.))
            .pt(px(4.))
            .pb(px(6.))
            .children(lead.map(|lead| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .mr(px(4.))
                    .child(lead)
                    .child(div().w(px(1.)).h(px(14.)).bg(theme::selected()))
            }))
            .children(tools.iter().map(|&h| tool(h, this.clone())))
            .child(
                div()
                    .id("md-preview")
                    .size(px(26.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(preview, |s| s.bg(theme::selected()))
                    .hover(|s| s.bg(theme::hover()))
                    .child(
                        super::icons::icon("eye")
                            .size(px(15.))
                            .text_color(if preview {
                                theme::text()
                            } else {
                                theme::muted()
                            }),
                    )
                    .tooltip(|_, cx| cx.new(|_| super::Tip("Preview  ⌘⇧P")).into())
                    .on_click(move |_, _, cx| {
                        toggle.update(cx, |this, cx| this.toggle_preview(cx)).ok();
                    }),
            )
            .child(div().flex_1())
            .child(div().flex().items_center().gap(px(6.)).children(tail));
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(tone)
            .bg(theme::base())
            .overflow_hidden()
            .font_family(theme::UI_FONT)
            .text_size(px(13.))
            .line_height(px(20.))
            .text_color(theme::text())
            .child(text)
            .children(compose.error.clone().map(|e| {
                div()
                    .px(px(10.))
                    .pb(px(4.))
                    .text_size(px(12.))
                    .text_color(theme::removed())
                    .child(e)
            }))
            .child(edge)
    }

    /// The composer's text as a field: the text wrapping in its box, the selection tinted, the
    /// caret drawn where the text's layout puts it; a click places the caret, a drag selects.
    fn render_edit_text(
        &self,
        compose: &Compose,
        placeholder: &'static str,
        compact: bool,
        this: WeakEntity<Self>,
    ) -> gpui::AnyElement {
        // A reply grows out of its one-line field, so its text starts where the field's did.
        let (pad_x, pad_y, min_h) = if compact {
            (10., 6., 64.)
        } else {
            (12., 10., 88.)
        };
        use gpui::{Bounds, HighlightStyle, MouseButton, StyledText, canvas, fill, point, size};
        let text = compose.edit.text();
        // A trailing space gives the caret a place on a last, empty line.
        let styled = StyledText::new(format!("{text} ")).with_highlights(
            compose.edit.selection().map(|sel| {
                (
                    sel,
                    HighlightStyle {
                        background_color: Some(theme::selection().into()),
                        ..Default::default()
                    },
                )
            }),
        );
        let layout = styled.layout().clone();
        // A card over the diff (a review's summary, a description) owns the arrows while open.
        if self.submit.is_none() && self.newreq.is_none() {
            *self.field_lines.borrow_mut() = Some(layout.clone());
        }
        let (for_caret, for_down, for_move) = (layout.clone(), layout.clone(), layout);
        let caret = compose.edit.caret();
        let (down, drag) = (this.clone(), this);
        div()
            .relative()
            .min_h(px(min_h))
            .px(px(pad_x))
            .py(px(pad_y))
            .bg(theme::base())
            .cursor_text()
            .child(div().w_full().min_w_0().child(styled))
            .children(text.is_empty().then(|| {
                div()
                    .absolute()
                    .top(px(pad_y))
                    .left(px(pad_x + 2.))
                    .text_color(theme::muted())
                    .child(placeholder)
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let at = for_caret
                            .position_for_index(caret)
                            .unwrap_or(bounds.origin + point(px(pad_x), px(pad_y)));
                        window.paint_quad(fill(
                            Bounds::new(at, size(px(2.), px(20.))),
                            theme::accent(),
                        ));
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                let at = for_down
                    .index_for_position(event.position)
                    .unwrap_or_else(|nearest| nearest);
                let (shift, clicks) = (event.modifiers.shift, event.click_count);
                down.update(cx, |this, cx| {
                    this.compose_place(at, shift, clicks, false, cx)
                })
                .ok();
            })
            .on_mouse_move(move |event, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    let at = for_move
                        .index_for_position(event.position)
                        .unwrap_or_else(|nearest| nearest);
                    drag.update(cx, |this, cx| this.compose_place(at, true, 1, true, cx))
                        .ok();
                }
            })
            .into_any_element()
    }

    /// Shows a comment: in the open file, scrolled to; in another file of the diff, that file first.
    pub(super) fn jump_to(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(path) = self
            .comments
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.path.clone())
        else {
            return;
        };
        self.jump = Some(id);
        if self.current_path().as_ref() == Some(&path) {
            self.apply_jump(cx);
            return;
        }
        let ix = self
            .diff
            .as_ref()
            .and_then(|d| d.files.iter().position(|f| f.path() == path));
        match ix {
            Some(ix) => self.select_file(ix, cx),
            None => self.jump = None,
        }
    }

    /// Scrolls to the thread of the comment asked for, once its file is laid out.
    pub(super) fn apply_jump(&mut self, cx: &mut Context<Self>) {
        if let Some((line, old)) = self.jump_line.take()
            && let Some(at) = self.row_of_line(line, old)
        {
            let thread = matches!(self.rows.get(at + 1), Some(Row::Request(_)));
            self.diff_list
                .scroll_to_reveal_item(if thread { at + 1 } else { at });
            cx.notify();
        }
        let Some(id) = self.jump.take() else {
            return;
        };
        if let Some(at) = self.rows.iter().position(|r| *r == Row::Thread(id)) {
            self.diff_list.scroll_to_reveal_item(at);
            cx.notify();
        }
    }
}
