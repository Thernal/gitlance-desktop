//! Line comments for a coding agent: a + in the gutter opens a composer under the line, comments sit
//! as threads under their lines, and a Review island lists them with *Copy for agent*. Comments are
//! kept on this Mac (`crate::review`), never in the reviewed repository. Designed in
//! `../Design/mockups/line-comments/a-agent-comments.html`.

use super::input::{self, Edit};
use super::rows::Row;
use super::{GAP, ROW_RADIUS, Selection, Workspace, format, island, island_label, theme};
use crate::review::{self, Comment, Place};
use gpui::{
    ClickEvent, ClipboardItem, Context, FontWeight, KeyDownEvent, WeakEntity, div, prelude::*, px,
};

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
    pub body: String,
    at: String,
    target: Target,
    /// Why the last attempt to post failed.
    error: Option<String>,
    posting: bool,
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
                if compose.is_some_and(|c| c.old == old && c.line == line)
                    || go == Some((old, line))
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
        self.compose = Some(Compose {
            path,
            old,
            line,
            code,
            body: String::new(),
            at,
            target: Target::Agent,
            error: None,
            posting: false,
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
            body: String::new(),
            at: String::new(),
            target: Target::Reply(ix),
            error: None,
            posting: false,
        });
        self.field_all = false;
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
        if compose.body.trim().is_empty() {
            self.compose = Some(compose);
            return;
        }
        let id = self.comments.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        self.comments.push(Comment {
            id,
            path: compose.path,
            old: compose.old,
            line: compose.line,
            code: compose.code,
            body: compose.body.trim().to_owned(),
            at: compose.at,
        });
        self.save_comments();
        self.copied = false;
        self.review_open = true;
        self.refresh_rows();
        cx.notify();
    }

    /// Posts the comment being written to merge request `iid`, on its line.
    /// With `draft`, as a pending comment that only its author sees until the review is submitted.
    pub(super) fn post_comment(&mut self, iid: u64, draft: bool, cx: &mut Context<Self>) {
        let (Some(remote), Some(compose)) = (self.web.clone(), self.compose.as_ref()) else {
            return;
        };
        let body = compose.body.trim().to_owned();
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
        let (body, id) = (compose.body.trim().to_owned(), thread.id.clone());
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
                        this.review_open = true;
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
        self.field_all = false;
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
        match input::edit(text, &mut self.field_all, &event.keystroke, cx) {
            Edit::Escape => {
                self.goline = None;
                self.goline_row = None;
                self.refresh_rows();
            }
            // Digits, and a minus first for a removed line.
            Edit::Changed => {
                let filtered: String = text
                    .chars()
                    .enumerate()
                    .filter(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == '-'))
                    .map(|(_, c)| c)
                    .collect();
                *text = filtered;
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
            self.diff_list.scroll_to_reveal_item(at + 1);
        }
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
            self.diff_list.scroll_to_reveal_item(at + 1);
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
    pub(super) fn render_goline_row(&self, indent: f32) -> gpui::AnyElement {
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
                        input::field_text(text, true, self.field_all, "first change"),
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

    pub(super) fn render_goline(&self) -> Option<impl IntoElement + use<>> {
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
                            input::field_text(text, true, self.field_all, "first change"),
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

    /// Typing into the composer: text, ⌘A, backspace, return for a new line, ⌘↵ to add, esc to cancel.
    pub(super) fn compose_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let Some(compose) = self.compose.as_mut() else {
            return;
        };
        if key.key == "enter" && key.modifiers.platform {
            // ⇧⌘↵ on a merge request keeps the comment pending until the review is submitted.
            if key.modifiers.shift
                && let Some(Target::Request(iid)) = self.compose.as_ref().map(|c| c.target)
            {
                return self.post_comment(iid, true, cx);
            }
            return self.add_comment(cx);
        }
        if key.key == "tab" {
            return self.toggle_target(cx);
        }
        if key.key == "enter" {
            if std::mem::take(&mut self.field_all) {
                compose.body.clear();
            }
            compose.body.push('\n');
        } else {
            match input::edit(&mut compose.body, &mut self.field_all, key, cx) {
                Edit::Escape => return self.cancel_comment(cx),
                Edit::Ignored => return,
                Edit::Changed | Edit::Selected | Edit::Enter { .. } => {}
            }
        }
        // The box grows with its text.
        self.diff_list.remeasure();
        cx.notify();
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

    /// One comment's thread, drawn under its line.
    pub(super) fn render_thread(
        &self,
        id: u64,
        indent: f32,
        this: WeakEntity<Self>,
    ) -> gpui::AnyElement {
        let Some(comment) = self.comments.iter().find(|c| c.id == id) else {
            return div().into_any_element();
        };
        if self.toggled_agent.contains(&id) {
            return div()
                .w_full()
                .pl(px(indent))
                .pr(px(12.))
                .py(px(2.))
                .child(
                    div()
                        .id(("agent-folded", id))
                        .max_w(px(640.))
                        .h(px(26.))
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(ROW_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .font_family(theme::UI_FONT)
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme::focus())
                                .child("▸ You → agent"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(comment.body.replace('\n', " ")),
                        )
                        .on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| {
                                this.toggled_agent.remove(&id);
                                this.refresh_rows();
                                cx.notify();
                            })
                            .ok();
                        }),
                )
                .into_any_element();
        }
        div()
            .w_full()
            .pl(px(indent))
            .pr(px(12.))
            .py(px(4.))
            .child(
                div()
                    .max_w(px(640.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .rounded(px(ROW_RADIUS))
                    .border_1()
                    .border_color(theme::island_border())
                    .bg(theme::panel())
                    .font_family(theme::UI_FONT)
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(div().text_color(theme::focus()).child("You → agent"))
                            .child(div().flex_1().child(format!("· {}", comment.at)))
                            .child(
                                div()
                                    .id(("delete-comment", id))
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(theme::removed()))
                                    .child("Delete")
                                    .on_click(move |_, _, cx| {
                                        this.update(cx, |this, cx| this.delete_comment(id, cx))
                                            .ok();
                                    }),
                            ),
                    )
                    .child(comment.body.clone()),
            )
            .into_any_element()
    }

    /// The box for a new comment, drawn under its line.
    pub(super) fn render_composer(&self, indent: f32, this: WeakEntity<Self>) -> gpui::AnyElement {
        let Some(compose) = &self.compose else {
            return div().into_any_element();
        };
        let (add, cancel, pick_agent, pick_request) =
            (this.clone(), this.clone(), this.clone(), this);
        let request = self.request_target();
        let reply = matches!(compose.target, Target::Reply(_));
        let to_request = matches!(compose.target, Target::Request(_) | Target::Reply(_));
        // Agent notes are purple, what goes to GitLab is orange: the two never look alike.
        let tone = if to_request {
            theme::warning()
        } else {
            theme::focus()
        };
        let target_chip = |id: &'static str, label: String, on: bool, color: gpui::Rgba| {
            div()
                .id(id)
                .px_2()
                .py(px(1.))
                .rounded(px(ROW_RADIUS))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .border_1()
                .border_color(if on { color } else { theme::island_border() })
                .text_color(if on { color } else { theme::muted() })
                .hover(|s| s.bg(theme::hover()))
                .child(label)
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
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded(px(ROW_RADIUS))
                    .border_2()
                    .border_color(tone)
                    .bg(theme::panel())
                    .font_family(theme::UI_FONT)
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .child(if reply {
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::warning())
                            .child("Reply on GitLab")
                    } else {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                target_chip(
                                    "target-agent",
                                    "For your agent".to_owned(),
                                    !to_request,
                                    theme::focus(),
                                )
                                .on_click(move |_, _, cx| {
                                    pick_agent
                                        .update(cx, |this, cx| {
                                            if let Some(c) = this.compose.as_mut() {
                                                c.target = Target::Agent;
                                                c.error = None;
                                            }
                                            cx.notify();
                                        })
                                        .ok();
                                }),
                            )
                            .children(request.map(|iid| {
                                target_chip(
                                    "target-request",
                                    format!("On !{iid} · GitLab"),
                                    to_request,
                                    theme::warning(),
                                )
                                .on_click(move |_, _, cx| {
                                    pick_request
                                        .update(cx, |this, cx| {
                                            if let Some(c) = this.compose.as_mut() {
                                                c.target = Target::Request(iid);
                                                c.error = None;
                                            }
                                            cx.notify();
                                        })
                                        .ok();
                                })
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .child(if request.is_some() {
                                        "Tab switches"
                                    } else {
                                        ""
                                    }),
                            )
                    })
                    .child(if compose.body.is_empty() {
                        div().text_color(theme::faint()).child(if reply {
                            "Reply — everyone on the merge request will see it  ⌘↵ sends"
                        } else if to_request {
                            "Comment on the merge request — everyone on it will see it  ⌘↵ posts · ⇧⌘↵ keeps it pending"
                        } else {
                            "Comment for your agent — stays on this Mac  ⌘↵ adds it"
                        })
                    } else {
                        div()
                            .flex()
                            .items_end()
                            .child(
                                div()
                                    .when(self.field_all, |s| {
                                        s.bg(theme::selection()).rounded(px(3.))
                                    })
                                    .child(compose.body.clone()),
                            )
                            .children((!self.field_all).then(|| {
                                div()
                                    .flex_none()
                                    .w(px(2.))
                                    .h(px(15.))
                                    .rounded_full()
                                    .bg(theme::accent())
                            }))
                    })
                    .children(compose.error.clone().map(|e| {
                        div()
                            .text_size(px(11.))
                            .text_color(theme::removed())
                            .child(e)
                    }))
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
                                    .text_color(theme::faint())
                                    .child(format!("on {}:{}", compose.path, compose.line)),
                            )
                            .child(
                                div()
                                    .id("compose-cancel")
                                    .px_2()
                                    .rounded(px(ROW_RADIUS))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme::hover()))
                                    .child("Cancel  esc")
                                    .on_click(move |_, _, cx| {
                                        cancel.update(cx, |this, cx| this.cancel_comment(cx)).ok();
                                    }),
                            )
                            .child(
                                div()
                                    .id("compose-add")
                                    .px_3()
                                    .rounded(px(ROW_RADIUS))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .bg(tone)
                                    .text_color(theme::base())
                                    .cursor_pointer()
                                    .child(match (&compose.target, compose.posting) {
                                        (Target::Request(_) | Target::Reply(_), true) => {
                                            "Posting…".to_owned()
                                        }
                                        (Target::Reply(_), false) => "Reply  ⌘↵".to_owned(),
                                        (Target::Request(iid), false) => {
                                            format!("Post to !{iid}  ⌘↵")
                                        }
                                        (Target::Agent, _) => "Add for agent  ⌘↵".to_owned(),
                                    })
                                    .on_click(move |_, _, cx| {
                                        add.update(cx, |this, cx| this.add_comment(cx)).ok();
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The Review island: every comment of the repository, and Copy for agent.
    pub(super) fn render_review(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let threads = self.render_request_threads(cx);
        let items = self.comments.iter().map(|c| {
            let id = c.id;
            let place = self.place_of(c);
            let where_ = match place {
                Place::Line(n) => format!("{}:{n}", c.path),
                Place::Outdated => format!("{} · outdated", c.path),
            };
            div()
                .id(("review-item", id))
                .mx(px(6.))
                .my(px(1.))
                .px(px(10.))
                .py(px(8.))
                .rounded(px(ROW_RADIUS))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(
                    div()
                        .font_family(theme::CODE_FONT)
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .truncate()
                        .child(where_),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(if place == Place::Outdated {
                            theme::muted()
                        } else {
                            theme::text()
                        })
                        .child(c.body.clone()),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.jump_to(id, cx)))
        });
        let count = self.comments.len();
        island()
            .w(px(320.))
            .flex_none()
            .ml(px(GAP))
            .children(threads)
            .child(
                island_label(format!(
                    "For your agent · {count} comment{}",
                    if count == 1 { "" } else { "s" }
                ))
                .text_color(theme::focus()),
            )
            .child(
                div()
                    .id("review-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(items)
                    .when(count == 0, |s| {
                        s.child(
                            div()
                                .px_4()
                                .py_4()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child("Hover a line in the diff and press + to write a comment for your agent."),
                        )
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .border_t_1()
                    .border_color(theme::island_border())
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child("Kept on this Mac, not in the repository. Copy them and paste into your agent."),
                    )
                    .child(
                        div()
                            .id("copy-for-agent")
                            .flex()
                            .justify_center()
                            .py_1()
                            .rounded(px(8.))
                            .bg(if count == 0 { theme::hover() } else { theme::accent() })
                            .text_color(if count == 0 { theme::faint() } else { theme::base() })
                            .font_weight(FontWeight::SEMIBOLD)
                            .when(count > 0, |s| s.cursor_pointer())
                            .child(if self.copied { "Copied ✓" } else { "Copy for agent" })
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                if !this.comments.is_empty() {
                                    this.copy_for_agent(cx)
                                }
                            })),
                    ),
            )
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
