//! Line comments for a coding agent: a + in the gutter opens a composer under the line, comments sit
//! as threads under their lines, and a Review island lists them with *Copy for agent*. Comments are
//! kept on this Mac (`crate::review`), never in the reviewed repository. Designed in
//! `../Design/mockups/line-comments/a-agent-comments.html`.

use super::discussion::{self, Kind, Source};
use super::input::{self, Edit};
use super::markdown;
use super::px;
use super::rows::{FileData, Row};
use super::waiting::Op;
use super::{ROW_RADIUS, Selection, Workspace, format, theme};
use crate::markdown::Helper;
use crate::review::{self, Comment, Place, State};
use crate::textedit::TextEdit;
use gpui::{ClipboardItem, Context, KeyDownEvent, WeakEntity, div, prelude::*};

/// The quiet Edit on one's own note: a pencil, alone in a narrow diff (#69).
pub(super) fn edit_button(
    id: impl Into<gpui::ElementId>,
    key: Option<&'static str>,
    bare: bool,
    change: Change,
    this: WeakEntity<Workspace>,
) -> gpui::AnyElement {
    discussion::icon_button(id, "pencil", "Edit", key, None, Kind::Quiet, bare)
        .h(px(22.))
        .px(px(6.))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.start_edit(change, cx)).ok();
        })
        .into_any_element()
}

/// Who a comment is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// Kept on this Mac and copied for a coding agent.
    Agent,
    /// Posted to a merge request on GitLab, where everyone on it sees it.
    Request(u64),
    /// A reply to discussion number `0` of the open merge request.
    Reply(usize),
    /// A new text for one's own comment.
    Edit(Change),
    /// A purple note on discussion `0` of the request, kept on this Mac for the agent (#50 G).
    ThreadNote(usize),
    /// The reviewer's note answering the agent's question on comment `0` (#10).
    Answer(u64),
    /// Text's own value on the agent's form of comment `0` (#10 C).
    AnswerValue(u64),
    /// Written in the Inbox: a reply to the selected notice's discussion, or a comment on its
    /// request when it has none (`Workspace::inbox_view.reply`).
    InboxReply,
}

/// The comment an edit changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    /// A comment for the agent, by its id.
    Agent(u64),
    /// Note `note` of discussion number `thread` of the open merge request.
    Note { thread: usize, note: u64 },
}

/// A comment being written.
pub struct Compose {
    path: String,
    old: bool,
    /// The first line of a comment on several lines; it sits under its last, `line`.
    first: Option<u32>,
    line: u32,
    code: String,
    /// The text, its caret and selection.
    pub edit: TextEdit,
    at: String,
    pub target: Target,
    /// Why the last attempt to post failed.
    error: Option<String>,
    /// The text is shown formatted instead of as written (⌘⇧P); typing goes on and the preview
    /// follows it.
    preview: bool,
    /// The completion list's selected row, and where a list was closed with esc (#51, #9).
    pick: usize,
    closed: Option<usize>,
    /// Written on a document read as a page: its file in the diff, the composer under the block
    /// (#11).
    page: Option<usize>,
    /// Written on a file read in Code, at its commit (None: the working tree) — a note on a plain
    /// line, under it in the file (#75).
    file: Option<Option<git2::Oid>>,
    /// The agent's note it answers, carried into the comment (#151).
    pub(super) answers: Option<String>,
}

impl Compose {
    /// The discussion of the open request a reply, an edit or a note for the agent is in.
    pub(super) fn thread(&self) -> Option<usize> {
        match self.target {
            Target::Reply(ix) | Target::ThreadNote(ix) => Some(ix),
            Target::Edit(Change::Note { thread, .. }) => Some(thread),
            _ => None,
        }
    }

    /// That discussion moved to `ix` in the list.
    pub(super) fn set_thread(&mut self, ix: usize) {
        match &mut self.target {
            Target::Reply(i) | Target::ThreadNote(i) => *i = ix,
            Target::Edit(Change::Note { thread, .. }) => *thread = ix,
            _ => {}
        }
    }
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
        self.current_path()
            .map(|path| self.line_notes_in(&path, self.data.as_deref()))
            .unwrap_or_default()
    }

    /// The comment marks of file `path`'s gutter, `data` its built diff.
    pub(super) fn line_notes_in(
        &self,
        path: &str,
        data: Option<&FileData>,
    ) -> Vec<super::diff_view::LineNote> {
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
            if let Some(Place::Line(n)) = data.and_then(|data| self.place_in_file(c, path, data)) {
                add(c.old, n, NoteTone::Agent);
            }
        }
        for line in self.bookmarks_in(path) {
            add(false, line, NoteTone::Bookmark);
        }
        if self.showing_request() {
            for (_, old, line) in self.request_threads_in(path) {
                add(old, line, NoteTone::Request);
            }
            for (_, old, line) in self.drafts_in(path) {
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
        self.place_in_file(comment, &self.current_path()?, self.data.as_ref()?)
    }

    /// Where `comment` sits in file `path` built as `data`: by its line's text, else, its line
    /// changed since, where that line went (`agent_lines`); nowhere once archived (#76).
    fn place_in_file(&self, comment: &Comment, path: &str, data: &FileData) -> Option<Place> {
        if comment.archived {
            return None;
        }
        match place_in(comment, path, data)? {
            Place::Outdated => Some(
                self.agent_lines
                    .get(&comment.id)
                    .map_or(Place::Outdated, |&n| Place::Line(n)),
            ),
            found => Some(found),
        }
    }

    /// Follows the line of every agent comment whose text no longer matches, from the commit it
    /// was written on to the diff shown, in the background (#46).
    pub(super) fn map_agent_comments(&mut self, cx: &mut Context<Self>) {
        self.agent_lines.clear();
        let (Some(root), Some(head), Some(diff)) =
            (self.root.clone(), self.shown_head(), self.diff.as_ref())
        else {
            return;
        };
        let paths: Vec<String> = diff.files.iter().map(|f| f.path().to_owned()).collect();
        let asks: Vec<(u64, String, String, u32)> = self
            .comments
            .iter()
            .filter(|c| !c.old && paths.contains(&c.path))
            .filter_map(|c| {
                let commit = c.commit.clone()?;
                (!head.to_string().starts_with(&commit))
                    .then(|| (c.id, commit, c.path.clone(), c.line))
            })
            .collect();
        if asks.is_empty() {
            return;
        }
        self.agent_task = Some(cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let Ok(repo) = crate::git::Repo::open(&root) else {
                        return std::collections::HashMap::new();
                    };
                    asks.into_iter()
                        .filter_map(|(id, commit, path, line)| {
                            let from = git2::Oid::from_str(&commit).ok()?;
                            match repo.follow_line(from, head, &path, line) {
                                crate::git::Fate::Same(n) | crate::git::Fate::Changed(n, _) => {
                                    Some((id, n))
                                }
                                crate::git::Fate::Gone => None,
                            }
                        })
                        .collect()
                })
                .await;
            this.update(cx, |this, cx| {
                this.agent_lines = found;
                this.refresh_rows();
                cx.notify();
            })
            .ok();
        }));
    }

    /// The rows with each comment's thread, and the composer, put under their lines.
    pub(super) fn with_comments(&self, rows: Vec<Row>) -> Vec<Row> {
        match (self.current_path(), &self.data) {
            (Some(path), Some(data)) => self.with_comments_in(&path, data, rows, true),
            _ => rows,
        }
    }

    /// The rows of file `path` with its comments under their lines; the `c` prompt only in the
    /// `active` file, the one the keyboard works in.
    pub(super) fn with_comments_in(
        &self,
        path: &str,
        data: &FileData,
        rows: Vec<Row>,
        active: bool,
    ) -> Vec<Row> {
        let here: Vec<(u64, bool, u32)> = self
            .comments
            .iter()
            .filter_map(|c| match self.place_in_file(c, path, data)? {
                Place::Line(n) => Some((c.id, c.old, n)),
                Place::Outdated => None,
            })
            .collect();
        // A note written on Code's file is drawn there, not in a diff (#75).
        let compose = self
            .compose
            .as_ref()
            .filter(|c| c.path == path && c.file.is_none());
        let theirs = if self.showing_request() {
            self.request_threads_in(path)
        } else {
            self.since_thread_in(path)
        };
        let drafts = if self.showing_request() {
            self.drafts_in(path)
        } else {
            Vec::new()
        };
        // An agent's notes on its changes, on the new side (#151).
        let notes = self.notes_in(path, data);
        // The prompt of `c` hangs under the marked line, like the composer that follows it.
        let go = if active && compose.is_none() && self.goline.is_some() {
            self.goline_target()
        } else {
            None
        };
        if here.is_empty()
            && compose.is_none()
            && theirs.is_empty()
            && drafts.is_empty()
            && notes.is_empty()
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
                // Why the line is as it is comes before what is said about it.
                if !old {
                    out.extend(
                        notes
                            .iter()
                            .filter(|(_, n)| *n == line)
                            .map(|(id, _)| Row::Note(*id)),
                    );
                }
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
                    c.old == old
                        && c.line == line
                        && !matches!(
                            c.target,
                            Target::Reply(_)
                                | Target::Edit(_)
                                | Target::ThreadNote(_)
                                | Target::Answer(_)
                                | Target::AnswerValue(_)
                                | Target::InboxReply
                        )
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
        // With all files in one scroll the comment may be in any of them; only rows that changed
        // are replaced, so the view stays where it is.
        if self.stack.is_some() {
            return self.relayout_stack();
        }
        let top = self.diff_list.logical_scroll_top();
        self.relayout();
        self.diff_list.scroll_to(top);
    }

    pub(super) fn start_comment(&mut self, old: bool, line: u32, cx: &mut Context<Self>) {
        self.start_comment_on(old, None, line, cx);
    }

    /// Where a comment is written, for the reader of the export: a commit or a version range.
    fn comment_at(&self) -> String {
        match self.selection {
            Selection::Commit(ix) => self
                .commits
                .get(ix)
                .map(|c| format::short(c.id))
                .unwrap_or_default(),
            Selection::Commits { newest, oldest } => {
                match (self.commits.get(oldest), self.commits.get(newest)) {
                    (Some(a), Some(b)) => {
                        format!("{}..{}", format::short(a.id), format::short(b.id))
                    }
                    _ => String::new(),
                }
            }
            Selection::Versions { from, to } => {
                match (self.versions.get(from), self.versions.get(to)) {
                    (Some(a), Some(b)) => format!("v{}→v{}", a.number, b.number),
                    _ => String::new(),
                }
            }
            Selection::WorkingTree => "working tree".to_owned(),
            Selection::None => String::new(),
        }
    }

    /// Who a new comment is for: a run of comments to one merge request does not have to pick it
    /// every time.
    fn comment_target(&self) -> Target {
        match self.request_target() {
            Some(iid) if self.last_request_target == Some(iid) => Target::Request(iid),
            _ => Target::Agent,
        }
    }

    /// A comment on a block of the document file `ix` read as a page: on the source line `line`
    /// the block starts on, written under the block (#11).
    pub(super) fn start_page_comment(
        &mut self,
        ix: usize,
        old: bool,
        line: u32,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self.diff.as_ref().and_then(|d| d.files.get(ix)) else {
            return;
        };
        let text = if old { &file.old_text } else { &file.new_text };
        let code = text
            .as_deref()
            .and_then(|t| t.lines().nth(line as usize - 1))
            .unwrap_or_default()
            .to_owned();
        let path = file.path().to_owned();
        self.compose = Some(Compose {
            path,
            old,
            first: None,
            line,
            code,
            edit: TextEdit::default(),
            at: self.comment_at(),
            target: self.comment_target(),
            error: None,
            preview: false,
            pick: 0,
            closed: None,
            page: Some(ix),
            file: None,
            answers: None,
        });
        self.reset_field_edit();
        self.remeasure_pages();
        cx.notify();
    }

    /// A note on line `line` of the file read in Code: for the agent, kept on the path, the line
    /// and its text, written under the line (#75).
    pub(super) fn start_file_comment(&mut self, line: u32, cx: &mut Context<Self>) {
        self.start_file_comment_on(None, line, cx)
    }

    /// A note on lines `first`…`line` of the file read in Code (#104), under the last.
    fn start_file_comment_on(&mut self, first: Option<u32>, line: u32, cx: &mut Context<Self>) {
        let Some(view) = &self.file_view else {
            return;
        };
        let Some(code) = view.line(line) else {
            return;
        };
        self.compose = Some(Compose {
            path: view.path.clone(),
            old: false,
            first: first.filter(|&f| f < line),
            line,
            code: code.to_owned(),
            edit: TextEdit::default(),
            at: view.at().to_owned(),
            target: Target::Agent,
            error: None,
            preview: false,
            pick: 0,
            closed: None,
            page: None,
            file: Some(view.rev()),
            answers: None,
        });
        self.reset_field_edit();
        cx.notify();
    }

    /// How much of its foot the field being written has room for (`room_for`). Its own width
    /// decides, wherever it is — the diff, a file in Code, a folder's page —, or the narrow diff's.
    pub(super) fn compose_room(&self) -> Room {
        room_for(self.widths.width("composer"), self.tight)
    }

    /// Under L: the secondary buttons keep only their icon, named in the tooltip, the keycaps go,
    /// the primary keeps its label (Design, 2026-10-21, C · icons below a width).
    pub(super) fn compose_narrow(&self) -> bool {
        self.compose_room() != Room::L
    }

    /// A note for the agent on folder `path` of Code (#97): kept on the path with a slash and no
    /// line, written on the folder's page.
    pub(super) fn start_folder_comment(&mut self, path: &str, cx: &mut Context<Self>) {
        let at = match &self.code.rev {
            Some((_, id)) => super::format::short(*id),
            None => "working tree".to_owned(),
        };
        self.compose = Some(Compose {
            path: format!("{path}/"),
            old: false,
            first: None,
            line: 0,
            code: String::new(),
            edit: TextEdit::default(),
            at,
            target: Target::Agent,
            error: None,
            preview: false,
            pick: 0,
            closed: None,
            page: None,
            file: Some(self.code.at()),
            answers: None,
        });
        self.reset_field_edit();
        cx.notify();
    }

    /// The folder a note for the agent is being written on (#97).
    pub(super) fn folder_compose(&self) -> Option<&str> {
        let c = self.compose.as_ref().filter(|c| c.file.is_some())?;
        c.path.strip_suffix('/')
    }

    /// Whether folder `path` has a note for the agent not yet resolved or put away (#97).
    pub(super) fn folder_note_waits(&self, path: &str) -> bool {
        self.comments.iter().any(|c| {
            c.path.strip_suffix('/') == Some(path) && !c.archived && c.state != State::Resolved
        })
    }

    /// The lines selected in the file read in Code, first and last (1-based); a selection that
    /// ends at the start of a line does not take that line (#104).
    fn file_selected_lines(&self) -> Option<(u32, u32)> {
        let view = self.file_view.as_ref()?;
        let sel = self
            .note_sel
            .as_ref()
            .filter(|s| s.note.as_ref() == format!("file-lines-{}", view.path))?;
        if sel.anchor == sel.head {
            return None;
        }
        let ((first, _), (last, at)) = sel.ends();
        let last = if at == 0 && last > first {
            last - 1
        } else {
            last
        };
        Some((first as u32 + 1, last as u32 + 1))
    }

    /// The composer written on Code's file: its path and line.
    pub(super) fn file_compose(&self) -> Option<(&str, u32)> {
        let c = self.compose.as_ref().filter(|c| c.file.is_some())?;
        Some((&c.path, c.line))
    }

    /// The composer written on a page: its file, side and line.
    pub(super) fn page_compose(&self) -> Option<(usize, bool, u32)> {
        let c = self.compose.as_ref()?;
        Some((c.page?, c.old, c.line))
    }

    /// The lines selected in the diff, first and last, on their side; a selection that ends at the
    /// start of a line does not take that line.
    fn selected_lines(&self) -> Option<(bool, u32, u32)> {
        let sel = self.sel.filter(|s| !s.is_empty())?;
        let (start, end) = sel.ends();
        let last = if end.byte == 0 && end.line > start.line {
            end.line - 1
        } else {
            end.line
        };
        Some((sel.old, start.line, last))
    }

    /// A comment on line `line`, or on `first`..=`line`.
    fn start_comment_on(
        &mut self,
        old: bool,
        first: Option<u32>,
        line: u32,
        cx: &mut Context<Self>,
    ) {
        let (Some(data), Some(path)) = (&self.data, self.current_path()) else {
            return;
        };
        let code = data.side(old).line(line).0.to_owned();
        let (at, target) = (self.comment_at(), self.comment_target());
        self.compose = Some(Compose {
            path,
            old,
            first: first.filter(|&f| f < line),
            line,
            code,
            edit: TextEdit::default(),
            at,
            target,
            error: None,
            preview: false,
            pick: 0,
            closed: None,
            page: None,
            file: None,
            answers: None,
        });
        self.refresh_rows();
        cx.notify();
    }

    /// Adds the agent comment being written to the drafts, then sends it alone or with all.
    pub(super) fn add_and_send(&mut self, only: bool, cx: &mut Context<Self>) {
        let Some(new) = self.add_agent_comment(cx) else {
            return;
        };
        let ids = if only { vec![new] } else { self.agent_drafts() };
        self.send_agent(ids, cx);
    }

    /// Adds the agent comment being written; its id, when it was added.
    fn add_agent_comment(&mut self, cx: &mut Context<Self>) -> Option<u64> {
        let before = self.comments.iter().map(|c| c.id).max().unwrap_or(0);
        self.add_comment(cx);
        self.comments
            .iter()
            .map(|c| c.id)
            .max()
            .filter(|&id| id > before)
    }

    /// 🤖 on a discussion's hover bar: the purple note for the agent on it, written in its reply
    /// field (#50 G, as ⇥ there).
    pub(super) fn start_thread_note(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.start_reply(ix, cx);
        if let Some(c) = self.compose.as_mut()
            && c.target == Target::Reply(ix)
        {
            c.target = Target::ThreadNote(ix);
        }
        cx.notify();
    }

    /// ▾ → Choose the chat…: adds it and asks where it goes.
    pub(super) fn add_and_pick_chat(&mut self, cx: &mut Context<Self>) {
        if let Some(new) = self.add_agent_comment(cx) {
            self.open_agent_pick_for(vec![new], cx);
        }
    }

    /// ▾ → Copy as markdown: adds it and copies every open comment for an agent.
    pub(super) fn add_and_copy(&mut self, cx: &mut Context<Self>) {
        if self.add_agent_comment(cx).is_some() {
            self.copy_for_agent(cx);
        }
    }

    /// The note answering the agent on comment `id` (⇥ Add a note under its form).
    pub(super) fn start_answer(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(c) = self.comments.iter().find(|c| c.id == id) else {
            return;
        };
        let note = c
            .answer
            .as_ref()
            .map(|a| a.note.clone())
            .unwrap_or_default();
        self.compose = Some(Compose {
            path: c.path.clone(),
            old: c.old,
            first: None,
            line: c.line,
            code: c.code.clone(),
            edit: TextEdit::new(note),
            at: String::new(),
            target: Target::Answer(id),
            error: None,
            preview: false,
            pick: 0,
            closed: None,
            page: None,
            file: None,
            answers: None,
        });
        self.reset_field_edit();
        self.refresh_rows();
        cx.notify();
    }

    /// Text's own value on the agent's form of comment `id`, in a field of its own.
    pub(super) fn start_answer_value(&mut self, id: u64, cx: &mut Context<Self>) {
        let value = self
            .comments
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.answer.as_ref())
            .map(|a| a.value.clone())
            .unwrap_or_default();
        self.start_answer(id, cx);
        if let Some(c) = self.compose.as_mut() {
            c.target = Target::AnswerValue(id);
            c.edit = TextEdit::new(value);
        }
    }

    /// The typed value replaces any suggestion picked; it goes to the drafts like a pick.
    fn save_answer_value(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(compose) = self.compose.take() else {
            return;
        };
        if let Some(c) = self.comments.iter_mut().find(|c| c.id == id) {
            let a = c.answer.get_or_insert_with(Default::default);
            a.value = compose.edit.text().trim().to_owned();
            if !a.value.is_empty() {
                a.picks.clear();
                a.your_call = false;
            }
            c.state = State::Draft;
        }
        self.save_comments();
        self.refresh_rows();
        cx.notify();
    }

    /// The answer's note goes to the drafts with the comment, for the next send.
    fn save_answer_note(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(compose) = self.compose.take() else {
            return;
        };
        if let Some(c) = self.comments.iter_mut().find(|c| c.id == id) {
            let a = c.answer.get_or_insert_with(Default::default);
            a.note = compose.edit.text().trim().to_owned();
            c.state = State::Draft;
        }
        self.save_comments();
        self.refresh_rows();
        cx.notify();
        // Written in the Inbox: it goes to the chat now (#116).
        if self.inbox && self.zone != super::zones::Zone::Tool {
            self.inbox_answered(id, cx);
        }
    }

    /// A purple note on a GitLab / GitHub discussion: a comment for the agent on its line, with
    /// the discussion to read; never posted (#50 G).
    fn add_thread_note(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(compose) = self.compose.take() else {
            return;
        };
        let body = compose.edit.text().trim().to_owned();
        let Some(t) = self.requests.threads.get(ix) else {
            return;
        };
        if body.is_empty() {
            self.compose = Some(compose);
            return;
        }
        let thread = t
            .notes
            .iter()
            .map(|n| format!("> {}: {}", n.author, n.body.replace('\n', " ")))
            .collect::<Vec<_>>()
            .join("\n");
        let code = self
            .data
            .as_ref()
            .map(|d| d.side(compose.old).line(compose.line).0.to_owned())
            .unwrap_or_default();
        let id = self.comments.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        self.comments.push(Comment {
            id,
            path: compose.path,
            old: compose.old,
            line: compose.line,
            code,
            body,
            at: compose.at,
            thread: Some(thread),
            ..Default::default()
        });
        self.save_comments();
        self.refresh_rows();
        cx.notify();
    }

    /// Starts a reply under discussion `ix` of the open merge request.
    pub(super) fn start_reply(&mut self, ix: usize, cx: &mut Context<Self>) {
        // One still on its way to the host has no id to reply to yet (#139).
        let Some(t) = self
            .requests
            .threads
            .get(ix)
            .filter(|t| !super::waiting::is_temp_thread(&t.id))
        else {
            return;
        };
        let (Some(path), Some(line)) = (t.path.clone(), t.new_line.or(t.old_line)) else {
            return;
        };
        let old = t.new_line.is_none();
        self.compose = Some(Compose {
            path,
            old,
            first: None,
            line,
            code: String::new(),
            edit: TextEdit::default(),
            at: String::new(),
            target: Target::Reply(ix),
            error: None,
            preview: false,
            pick: 0,
            closed: None,
            page: None,
            file: None,
            answers: None,
        });
        self.reset_field_edit();
        self.thread_at = Some(ix);
        if !self.mr_open(ix) {
            self.toggled_mr.insert(ix);
        }
        self.refresh_rows();
        cx.notify();
    }

    /// The Inbox's composer: a reply to discussion `discussion` of request `iid` (on `path` and
    /// `line` when it is on code), or with no discussion a comment on the request. A comment
    /// being written elsewhere is kept: the Inbox then waits for it.
    pub(super) fn start_inbox_reply(
        &mut self,
        iid: u64,
        discussion: Option<String>,
        spot: Option<(String, u32)>,
        cx: &mut Context<Self>,
    ) {
        if self
            .compose
            .as_ref()
            .is_some_and(|c| c.target != Target::InboxReply)
        {
            return;
        }
        let (path, line) = spot.unwrap_or_default();
        self.compose = Some(Compose {
            path,
            old: false,
            first: None,
            line,
            code: String::new(),
            edit: TextEdit::default(),
            at: String::new(),
            target: Target::InboxReply,
            error: None,
            preview: false,
            pick: 0,
            closed: None,
            page: None,
            file: None,
            answers: None,
        });
        self.reset_field_edit();
        self.inbox_view.reply = Some((iid, discussion));
        cx.notify();
    }

    /// Posts the Inbox's reply: to its discussion, or as a comment on the request.
    fn post_inbox_reply(&mut self, cx: &mut Context<Self>) {
        let (Some(compose), Some((iid, discussion))) =
            (self.compose.as_ref(), self.inbox_view.reply.clone())
        else {
            return;
        };
        let body = compose.edit.text().trim().to_owned();
        if body.is_empty() {
            return;
        }
        let w = &mut self.requests.waiting;
        let op = match discussion.clone() {
            Some(thread) => Op::Reply {
                thread,
                note: w.temp(),
                body: body.clone(),
            },
            None => Op::Discussion {
                thread: w.temp_thread(),
                note: w.temp(),
                body: body.clone(),
                at: None,
            },
        };
        self.submit_compose(iid, vec![op], cx, move |remote| match discussion {
            Some(id) => crate::mr::reply(&remote, iid, &id, &body),
            None => crate::mr::comment(&remote, iid, &body),
        });
    }

    /// Edit: the composer in place of one's own comment, its text in it.
    pub(super) fn start_edit(&mut self, change: Change, cx: &mut Context<Self>) {
        let (path, old, line, text) = match change {
            Change::Agent(id) => {
                let Some(c) = self.comments.iter().find(|c| c.id == id) else {
                    return;
                };
                if !self.toggled_agent.contains(&id) && c.state == State::Resolved {
                    self.toggled_agent.insert(id);
                }
                (c.path.clone(), c.old, c.line, c.body.clone())
            }
            Change::Note { thread, note } => {
                let Some(t) = self.requests.threads.get(thread) else {
                    return;
                };
                let Some(n) = t
                    .notes
                    .iter()
                    .find(|n| n.id == note && !super::waiting::is_temp(n.id))
                else {
                    return;
                };
                self.thread_at = Some(thread);
                if !self.mr_open(thread) {
                    self.toggled_mr.insert(thread);
                }
                let old = t.new_line.is_none();
                let line = t.new_line.or(t.old_line).unwrap_or(0);
                (
                    t.path.clone().unwrap_or_default(),
                    old,
                    line,
                    n.body.clone(),
                )
            }
        };
        let mut edit = TextEdit::new(text);
        edit.move_to(crate::textedit::Move::End, false);
        self.compose = Some(Compose {
            path,
            old,
            first: None,
            line,
            code: String::new(),
            edit,
            at: String::new(),
            target: Target::Edit(change),
            error: None,
            preview: false,
            pick: 0,
            closed: None,
            page: None,
            file: None,
            answers: None,
        });
        self.reset_field_edit();
        self.refresh_rows();
        cx.notify();
    }

    /// One's own last note in discussion `ix`, when the request can take changes.
    pub(super) fn my_note_in(&self, ix: usize) -> Option<u64> {
        let login = self.requests.login.as_deref()?;
        self.request_target()?;
        let t = self.requests.threads.get(ix)?;
        t.notes
            .iter()
            .rev()
            .find(|n| n.login == login)
            .map(|n| n.id)
    }

    /// `e`: edit one's own last note in the discussion with the keyboard.
    pub(super) fn edit_focused(&mut self, cx: &mut Context<Self>) {
        if self.compose.is_some() {
            return;
        }
        let Some(thread) = self.acting_thread() else {
            return;
        };
        match self.my_note_in(thread) {
            Some(note) => self.start_edit(Change::Note { thread, note }, cx),
            None => {
                self.error = Some("Nothing of yours to edit in this discussion".into());
                cx.notify();
            }
        }
    }

    /// Saves an edit: an agent's comment here, a note on GitLab or GitHub.
    fn save_edit(&mut self, change: Change, cx: &mut Context<Self>) {
        let Some(compose) = self.compose.as_ref() else {
            return;
        };
        let body = compose.edit.text().trim().to_owned();
        if body.is_empty() {
            return;
        }
        match change {
            Change::Agent(id) => {
                if let Some(c) = self.comments.iter_mut().find(|c| c.id == id) {
                    c.body = body;
                }
                self.compose = None;
                self.save_comments();
                self.copied = false;
                self.refresh_rows();
                cx.notify();
            }
            Change::Note { thread, note } => {
                let (Some(t), Some(iid)) =
                    (self.requests.threads.get(thread), self.requests.current)
                else {
                    return;
                };
                let id = t.id.clone();
                let op = Op::Edit {
                    note,
                    body: body.clone(),
                };
                self.submit_compose(iid, vec![op], cx, move |remote| {
                    crate::mr::edit_note(&remote, iid, &id, note, &body)
                });
            }
        }
    }

    /// The editor of an edit, when `change` is being edited.
    pub(super) fn edit_box(&self, change: Change, this: WeakEntity<Self>) -> Option<gpui::Div> {
        let compose = self
            .compose
            .as_ref()
            .filter(|c| c.target == Target::Edit(change))?;
        let tone = match change {
            Change::Agent(_) => Source::Agent.color(),
            Change::Note { .. } => Source::GitLab.color(),
        };
        let (cancel, save) = (this.clone(), this.clone());
        let tail = vec![
            discussion::button("edit-cancel", "Cancel", None, Kind::Quiet)
                .h(px(28.))
                .px(px(10.))
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(|_, cx| cx.new(|_| super::Tip("Esc")).into())
                .on_click(move |_, _, cx| {
                    cancel.update(cx, |this, cx| this.cancel_comment(cx)).ok();
                })
                .into_any_element(),
            discussion::button(
                "edit-save",
                "Save",
                (!self.compose_narrow()).then_some("⌘↵"),
                Kind::Primary(tone),
            )
            .h(px(28.))
            .px(px(10.))
            .on_click(move |_, _, cx| {
                save.update(cx, |this, cx| this.add_comment(cx)).ok();
            })
            .into_any_element(),
        ];
        Some(self.open_input(
            compose,
            tone,
            "The comment…",
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
                .is_some_and(|w| crate::mr::token_now(w).is_some())
                || crate::mr::fixture_on() =>
            {
                Some(iid)
            }
            _ => None,
        }
    }

    /// The request the open composer can post to: none for a note on a file or folder in Code,
    /// which is the agent's only, whatever request's diff is loaded behind it (#130).
    fn compose_request(&self) -> Option<u64> {
        self.request_target()
            .filter(|_| self.compose.as_ref().is_some_and(|c| c.file.is_none()))
    }

    fn toggle_target(&mut self, cx: &mut Context<Self>) {
        let request = self.compose_request();
        if let Some(compose) = self.compose.as_mut() {
            compose.target = match (compose.target, request) {
                // ⇥ in a discussion's reply: a note for the agent instead, and back (#50 G).
                (Target::Reply(ix), _) => Target::ThreadNote(ix),
                (Target::ThreadNote(ix), _) => Target::Reply(ix),
                (Target::Answer(id), _) => Target::Answer(id),
                (Target::AnswerValue(id), _) => Target::AnswerValue(id),
                (Target::Edit(change), _) => Target::Edit(change),
                (Target::InboxReply, _) => Target::InboxReply,
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
            self.remeasure_pages();
            cx.notify();
        }
    }

    pub(super) fn add_comment(&mut self, cx: &mut Context<Self>) {
        match self.compose.as_ref().map(|c| c.target) {
            Some(Target::Request(iid)) => return self.post_comment(iid, false, cx),
            Some(Target::Reply(ix)) => return self.post_reply(ix, cx),
            Some(Target::Edit(change)) => return self.save_edit(change, cx),
            Some(Target::Answer(id)) => return self.save_answer_note(id, cx),
            Some(Target::AnswerValue(id)) => return self.save_answer_value(id, cx),
            Some(Target::ThreadNote(ix)) => return self.add_thread_note(ix, cx),
            Some(Target::InboxReply) => return self.post_inbox_reply(cx),
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
        let above = match (compose.first, &self.data, &self.file_view) {
            // On a file read in Code: its own lines, whatever diff is loaded (#104).
            (Some(first), _, Some(view)) if compose.file.is_some() => (first..compose.line)
                .filter_map(|n| view.line(n).map(str::to_owned))
                .collect(),
            (Some(first), Some(data), _) if compose.file.is_none() => (first..compose.line)
                .map(|n| data.side(compose.old).line(n).0.to_owned())
                .collect(),
            _ => Vec::new(),
        };
        self.comments.push(Comment {
            id,
            path: compose.path,
            old: compose.old,
            line: compose.line,
            code: compose.code,
            above,
            body: compose.edit.text().trim().to_owned(),
            at: compose.at,
            commit: match compose.file {
                Some(rev) => rev.map(|id| id.to_string()),
                None => self.shown_head().map(|id| id.to_string()),
            },
            branch: self.page_branch(),
            answers: compose.answers,
            ..Default::default()
        });
        self.save_comments();
        self.copied = false;
        self.refresh_rows();
        self.remeasure_pages();
        cx.notify();
    }

    /// Posts the comment being written to merge request `iid`, on its line.
    /// With `draft`, as a pending comment that only its author sees until the review is submitted.
    pub(super) fn post_comment(&mut self, iid: u64, draft: bool, cx: &mut Context<Self>) {
        let Some(compose) = self.compose.as_ref() else {
            return;
        };
        let body = compose.edit.text().trim().to_owned();
        let head = match self.diff.as_ref().map(|d| &d.header) {
            Some(super::Header::Compare { head, .. }) => head.1.to_string(),
            _ => return,
        };
        // On a page the file is the page's, and its counters come from its hunks (#11).
        let at_file = compose.page.unwrap_or(self.file);
        let file = self.diff.as_ref().and_then(|d| d.files.get(at_file));
        let pair = file
            .filter(|_| compose.page.is_some())
            .map(|f| f.line_pair(compose.old, compose.line));
        let file = file.map(|f| (f.new_path.clone(), f.old_path.clone()));
        if body.is_empty() {
            return;
        }
        let Some((new_path, old_path)) = file else {
            return;
        };
        self.last_request_target = Some(iid);
        let (old_line, new_line) =
            pair.unwrap_or_else(|| self.line_pair(compose.line, compose.old));
        let range = compose.first.and_then(|first| {
            Some((
                self.diff_line(first, compose.old)?,
                self.diff_line(compose.line, compose.old)?,
            ))
        });
        let path = new_path.clone().or(old_path.clone()).unwrap_or_default();
        let old_path = old_path.or(new_path).unwrap_or_default();
        // Shown at once where it was written: a pending comment, or a discussion (#139).
        let w = &mut self.requests.waiting;
        let op = if draft {
            Op::Draft(crate::mr::Draft {
                id: w.temp(),
                body: body.clone(),
                path: Some(path.clone()),
                new_line,
                old_line,
            })
        } else {
            Op::Discussion {
                thread: w.temp_thread(),
                note: w.temp(),
                body: body.clone(),
                at: Some(super::waiting::At {
                    path: path.clone(),
                    new_line,
                    old_line,
                }),
            }
        };
        self.submit_compose(iid, vec![op], cx, move |remote| {
            crate::mr::post_discussion(
                &remote,
                iid,
                &head,
                &crate::mr::Place {
                    path: &path,
                    old_path: &old_path,
                    new_line,
                    old_line,
                    range,
                },
                &body,
                draft,
            )
            .map(|_| ())
        });
    }

    /// Writes the reply being composed to the discussion it answers.
    fn post_reply(&mut self, ix: usize, cx: &mut Context<Self>) {
        let (Some(compose), Some(thread), Some(iid)) = (
            self.compose.as_ref(),
            self.requests.threads.get(ix),
            self.requests.current,
        ) else {
            return;
        };
        let (body, id) = (compose.edit.text().trim().to_owned(), thread.id.clone());
        if body.is_empty() || super::waiting::is_temp_thread(&id) {
            return;
        }
        let op = Op::Reply {
            thread: id.clone(),
            note: self.requests.waiting.temp(),
            body: body.clone(),
        };
        self.submit_compose(iid, vec![op], cx, move |remote| {
            crate::mr::reply(&remote, iid, &id, &body)
        });
    }

    /// Sends what the composer holds, as `ops` on request `iid`, with `work`: the composer closes
    /// and the comment shows at once, faded (#139). Refused, the composer comes back with its text
    /// and the reason when nothing else is being written and its place still shows; else its text
    /// goes to the clipboard and the error bar says why.
    fn submit_compose(
        &mut self,
        iid: u64,
        ops: Vec<Op>,
        cx: &mut Context<Self>,
        work: impl FnOnce(crate::git::WebRemote) -> anyhow::Result<()> + Send + 'static,
    ) {
        if self.web.is_none() {
            return;
        }
        let Some(mut compose) = self.compose.take() else {
            return;
        };
        let reply = self.inbox_view.reply.take();
        // A reply or an edit comes back to its discussion, wherever it moved.
        let thread = compose
            .thread()
            .and_then(|ix| self.requests.threads.get(ix))
            .map(|t| t.id.clone());
        self.refresh_rows();
        self.remeasure_pages();
        let refused = move |this: &mut Self, why: String, cx: &mut Context<Self>| {
            let place = match &thread {
                Some(id) => this.requests.threads.iter().position(|t| &t.id == id),
                None => Some(usize::MAX),
            };
            let shows = match compose.target {
                Target::InboxReply => {
                    this.inbox
                        && this.inbox_view.reply.is_none()
                        && reply.as_ref().is_some_and(|(i, _)| {
                            this.notices
                                .list
                                .get(this.notices.at)
                                .is_some_and(|n| n.iid == *i)
                        })
                }
                Target::Request(_) => {
                    this.requests.current == Some(iid)
                        && (compose.page.is_some()
                            || this.current_path().as_deref() == Some(compose.path.as_str()))
                }
                _ => this.requests.current == Some(iid),
            };
            match place {
                Some(ix) if this.compose.is_none() && shows => {
                    if ix != usize::MAX {
                        compose.set_thread(ix);
                    }
                    compose.error = Some(why);
                    if compose.target == Target::InboxReply {
                        this.inbox_view.reply = reply;
                    }
                    this.compose = Some(compose);
                    this.reset_field_edit();
                    this.refresh_rows();
                    this.remeasure_pages();
                }
                _ => {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        compose.edit.text().to_owned(),
                    ));
                    this.error =
                        Some(format!("Not sent — {why}. Its text is on the clipboard.").into());
                }
            }
            cx.notify();
        };
        self.send_change(iid, ops, work, refused, cx);
    }

    /// A line of the open file with both of GitLab's counters: from its hunk, or, outside the
    /// hunks (all lines shown), an unchanged line.
    fn diff_line(&self, line: u32, old: bool) -> Option<crate::git::DiffLine> {
        let file = self.diff.as_ref()?.files.get(self.file)?;
        file.position(old, line)
            .or_else(|| match self.line_pair(line, old) {
                (Some(o), Some(n)) => Some(crate::git::DiffLine {
                    kind: crate::git::LineKind::Context,
                    old_line: Some(o),
                    new_line: Some(n),
                }),
                _ => None,
            })
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

    /// `c`: on the lines selected in the diff, the composer at once; else a marker appears on the
    /// diff, typing a line number moves it, ↵ opens the comment there.
    pub(super) fn open_goline(&mut self, cx: &mut Context<Self>) {
        // On a folder's page in Code: a note for the agent on the folder (#97).
        if let Some(super::nav::Page::Folder(path)) = self.nav_page().cloned()
            && self.nav.space == super::spaces::Space::Code
            && !self.settings_open
            && self.compose.is_none()
        {
            return self.start_folder_comment(&path, cx);
        }
        // On a file read in Code with lines selected: a note on them (#104).
        if self.code_file_shown()
            && !self.settings_open
            && self.compose.is_none()
            && let Some((first, last)) = self.file_selected_lines()
        {
            self.note_sel = None;
            return self.start_file_comment_on(Some(first), last, cx);
        }
        // On a file read in Code: the number of the line to write a note on (#75).
        if self.code_file_shown() && !self.settings_open && self.compose.is_none() {
            self.reset_field_edit();
            self.goline = Some(String::new());
            return cx.notify();
        }
        if self.data.is_none() || self.settings_open || self.compose.is_some() {
            return;
        }
        if let Some((old, first, last)) = self.selected_lines() {
            self.zone = super::zones::Zone::Diff;
            return self.start_comment_on(old, Some(first), last, cx);
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
        if self.code_file_shown() {
            let line = self
                .goline
                .as_deref()
                .and_then(|t| t.parse::<u32>().ok())
                .unwrap_or(0);
            let count = self.file_view.as_ref().map_or(0, |v| v.line_count());
            if line == 0 || line as usize > count {
                self.error = Some(format!("This file has {count} lines.").into());
                return cx.notify();
            }
            self.goline = None;
            return self.start_file_comment(line, cx);
        }
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

    pub(super) fn save_comments(&self) {
        if let Some(root) = self.project.as_ref().or(self.root.as_ref()) {
            review::save(root, &self.comments);
        }
    }

    /// Typing into the composer: a real text field (caret, selection, ⌥ words, ⌘ lines, undo),
    /// return carries a list on, ⇧↵ a plain new line; ⌘B ⌘I ⌘⇧X ⌘⇧E ⌘⇧K for markdown, ⌘⇧P to
    /// preview it, Tab for who sees it, ⌘↵ to send, ⇧⌘↵ to add it to the review, esc to cancel.
    pub(super) fn compose_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let m = key.modifiers;
        if self.compose.is_none() {
            return;
        }
        if key.key == "enter" && m.secondary() {
            let target = self.compose.as_ref().map(|c| c.target);
            if m.shift
                && let Some(Target::Request(iid)) = target
            {
                return self.post_comment(iid, true, cx);
            }
            // An agent comment: ⇧⌘↵ adds it and sends all drafts, ⌥⌘↵ sends this one only.
            if target == Some(Target::Agent) && (m.shift || m.alt) {
                return self.add_and_send(m.alt, cx);
            }
            return self.add_comment(cx);
        }
        if self.completion_key(key, cx) {
            return;
        }
        if key.key == "tab" {
            return self.toggle_target(cx);
        }
        if self.vertical_key(key, cx) {
            return;
        }
        // ⌘B ⌘I ⇧⌘X … (Ctrl on Windows and Linux), no other modifier.
        if m.secondary() && m.number_of_modifiers() == 1 + u8::from(m.shift) {
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

    /// Puts `text` at the composer's caret (a reply that mentions the author).
    pub(super) fn insert_into_compose(&mut self, text: &str) {
        if let Some(c) = self.compose.as_mut() {
            c.edit.insert(text);
        }
    }

    /// The list for the token being typed in the composer, if one is open.
    pub(super) fn compose_completion(&self) -> Option<(usize, Vec<super::complete::Choice>)> {
        let c = self.compose.as_ref().filter(|c| !c.preview)?;
        self.completion(c.edit.text(), c.edit.caret(), c.closed)
    }

    /// ↑ ↓ ↵ ⇥ esc while a completion list is open; whether it took the key.
    fn completion_key(&mut self, key: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let Some((at, choices)) = self.compose_completion() else {
            return false;
        };
        if key.modifiers.number_of_modifiers() > 0 {
            return false;
        }
        let Some(c) = self.compose.as_mut() else {
            return false;
        };
        match key.key.as_str() {
            "down" => c.pick = (c.pick + 1).min(choices.len() - 1),
            "up" => c.pick = c.pick.saturating_sub(1),
            "enter" | "tab" => {
                let choice = &choices[c.pick.min(choices.len() - 1)];
                let caret = c.edit.caret();
                let end = at + choice.insert.len();
                c.edit.replace(at..caret, &choice.insert, end..end);
                c.pick = 0;
                self.diff_list.remeasure();
            }
            "escape" => c.closed = Some(at),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// ↑ ↓ and ⌘← ⌘→ (⇧ selecting) in a wrapped field — the composer, a review's summary, a
    /// description: by the lines as drawn. Any other key forgets the x ↑ ↓ keep. Returns whether
    /// it took the key.
    pub(super) fn vertical_key(&mut self, key: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        use input::Drawn;
        let m = key.modifiers;
        let motion = match (key.key.as_str(), crate::platform::line(&m), m.alt) {
            ("up", false, false) => Drawn::Up,
            ("down", false, false) => Drawn::Down,
            ("left", true, false) => Drawn::Start,
            ("right", true, false) => Drawn::End,
            // Windows and Linux: Home and End by the lines as drawn, as ⌘← ⌘→ on macOS.
            ("home", false, false) if !crate::platform::MAC && !m.secondary() => Drawn::Start,
            ("end", false, false) if !crate::platform::MAC && !m.secondary() => Drawn::End,
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
        lower: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(compose) = self.compose.as_mut() else {
            return;
        };
        // Clicked, it has the keys again, and a filter that had them lets go.
        self.compose_aside = false;
        self.field = None;
        compose.edit.click(at, clicks, shift || drag);
        if clicks == 1 && !shift && !drag {
            compose.edit.set_downstream(lower);
        }
        self.field_goal = None;
        cx.notify();
    }

    /// A markdown helper of the composer's toolbar, on the selection or at the caret.
    pub(super) fn apply_helper(&mut self, helper: Helper, cx: &mut Context<Self>) {
        let Some(compose) = self.compose.as_mut() else {
            return;
        };
        crate::markdown::apply(&mut compose.edit, helper);
        self.diff_list.remeasure();
        cx.notify();
    }

    pub(super) fn toggle_preview(&mut self, cx: &mut Context<Self>) {
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

    /// A comment on one line, for a folded thread or a tool window: without its markdown when Rich.
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
        .tooltip_show_delay(crate::ui::hover::LONG)
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
        // What the agent still has to do: not the comments it did or the reviewer resolved.
        let comments: Vec<(Comment, Place)> = self
            .comments
            .iter()
            .filter(|c| c.open())
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
        // A resolved comment starts folded; a click flips either.
        let resolved = comment.state == State::Resolved;
        let open = self.toggled_agent.contains(&id) == resolved;
        let selecting = self.selecting(this.clone());
        let (fold, delete, raw) = (this.clone(), this.clone(), this.clone());
        let (resolve, reopen, editing, asking) = (this.clone(), this.clone(), this.clone(), this);
        // A chat by the name it was given, else its title; without one, by its agent.
        let chat = |s: &crate::review::Sent| {
            let title = crate::labels::chat(&s.session).unwrap_or_else(|| s.title.clone());
            if title.is_empty() {
                self.agent_chats
                    .iter()
                    .find(|c| c.session == s.session)
                    .map_or_else(|| "its chat".to_owned(), |c| c.agent.clone())
            } else {
                format!("“{title}”")
            }
        };
        let status = match (&comment.state, &comment.sent) {
            (State::Draft, _) => "draft".to_owned(),
            (State::Sent, Some(s)) if s.session.is_empty() => "sent to a new chat".to_owned(),
            (State::Sent, Some(s)) if s.delivered => format!("with {}", chat(s)),
            (State::Sent, Some(s)) => format!("sent to {} · waits for it to listen", chat(s)),
            (State::Sent, None) => "sent".to_owned(),
            (State::Addressed, _) => "the agent says it is done — check it".to_owned(),
            (State::Resolved, _) => "resolved".to_owned(),
        };
        let bar = discussion::bar(("agent-bar", id), open)
            .child(discussion::source_pill(Source::Agent))
            .child(div().flex_1().min_w_0().truncate().child(if open {
                format!("· {status}")
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
            .children((comment.state == State::Addressed).then(|| {
                discussion::button(
                    ("resolve-agent", id),
                    "Resolve",
                    None,
                    Kind::Primary(theme::agent()),
                )
                .h(px(24.))
                .px(px(8.))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    resolve
                        .update(cx, |this, cx| this.resolve_agent_comment(id, cx))
                        .ok();
                })
            }))
            .children(
                matches!(comment.state, State::Addressed | State::Resolved).then(|| {
                    discussion::icon_button(
                        ("reopen-agent", id),
                        "rotate-ccw",
                        "Reopen",
                        None,
                        None,
                        Kind::Plain,
                        self.tight,
                    )
                    .h(px(24.))
                    .px(px(8.))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        reopen
                            .update(cx, |this, cx| this.reopen_agent_comment(id, cx))
                            .ok();
                    })
                }),
            )
            .child(
                discussion::icon_button(
                    ("delete-comment", id),
                    "trash",
                    "Delete",
                    None,
                    None,
                    Kind::Danger,
                    self.tight,
                )
                .h(px(24.))
                .px(px(8.))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    delete
                        .update(cx, |this, cx| this.delete_comment(id, cx))
                        .ok();
                }),
            )
            // A double click folds or opens it once, as one click does.
            .on_click(move |e: &gpui::ClickEvent, _, cx| {
                if e.click_count() > 1 {
                    return;
                }
                fold.update(cx, |this, cx| {
                    this.mark_motion(&format!("agent-thread-{id}"), cx);
                    if !this.toggled_agent.remove(&id) {
                        this.toggled_agent.insert(id);
                    }
                    this.refresh_rows();
                    cx.notify();
                })
                .ok();
            });
        let change = Change::Agent(id);
        let edit_box = self.edit_box(change, editing.clone());
        let body = discussion::note(
            &self.me,
            true,
            None,
            comment.at.clone(),
            match edit_box {
                Some(edit_box) => edit_box.into_any_element(),
                None => markdown::body(
                    &comment.body,
                    self.settings.rich_comments,
                    format!("md-agent-{id}"),
                    None,
                    Some(selecting.clone()),
                ),
            },
            comment.replies.is_empty(),
            (self.compose.is_none())
                .then(|| edit_button(("edit-agent", id), None, self.tight, change, editing)),
            None,
        );
        // The last question is answered under it while the comment is open; an earlier one, or
        // the last once it is done, shows its form as it was answered.
        let last = comment.replies.len().saturating_sub(1);
        let asking_now = comment.open();
        let replies = comment.replies.iter().enumerate().map(|(ix, reply)| {
            let record = reply
                .form
                .as_ref()
                .filter(|_| ix != last || !asking_now)
                .map(|form| {
                    let answered = if ix == last {
                        comment.answer.as_ref().or(reply.answered.as_ref())
                    } else {
                        reply.answered.as_ref()
                    };
                    self.render_form_record(form, answered)
                });
            div()
                .flex()
                .flex_col()
                .child(discussion::note(
                    "Agent",
                    false,
                    Some("agent"),
                    super::format::ago(reply.at),
                    markdown::body(
                        &reply.body,
                        self.settings.rich_comments,
                        format!("md-agent-{id}-{ix}"),
                        None,
                        Some(selecting.clone()),
                    ),
                    ix + 1 == comment.replies.len(),
                    None,
                    None,
                ))
                .children(record)
        });
        // The agent asked with a form: answered here, the pick going to the drafts (#10 C).
        let form = self.render_agent_form(id, asking.clone());
        let answering = self.compose.as_ref().is_some_and(
            |c| matches!(c.target, Target::Answer(i) | Target::AnswerValue(i) if i == id),
        );
        let answer_box = answering
            .then(|| self.reply_editor(asking))
            .flatten()
            .map(|editor| div().ml(px(52.)).mr(px(12.)).mb(px(10.)).child(editor));
        let on_thread = comment.thread.is_some().then(|| {
            div()
                .mx(px(12.))
                .mb(px(6.))
                .text_size(px(11.))
                .text_color(theme::faint())
                .child("🔒 On a discussion")
        });
        let notes = div()
            .flex()
            .flex_col()
            .children(on_thread)
            .child(body)
            .children(replies)
            .children(form)
            .children(answer_box);
        div()
            .w_full()
            .pl(px(indent))
            .pr(px(12.))
            .py(px(6.))
            .child(
                discussion::frame(Source::Agent, false)
                    .child(bar)
                    // Its notes open and fold in size and alpha (#110).
                    .children(self.folding(
                        format!("agent-thread-body-{id}"),
                        &format!("agent-thread-{id}"),
                        open,
                        notes,
                    )),
            )
            .into_any_element()
    }

    /// The composer of a new comment, drawn under its line: my avatar beside the box. A reply is
    /// not drawn here but inside its discussion (`requests::render_request_thread`).
    pub(super) fn render_composer(&self, indent: f32, this: WeakEntity<Self>) -> gpui::AnyElement {
        let Some(boxed) = self.compose_box(this) else {
            return div().into_any_element();
        };
        // Under 520 px of row the avatar and the indent give way and the box takes the row
        // (Design shell rounds 4 and 6, #129).
        let row = self.widths.width("composer-row");
        let roomy = row == 0. || row >= 520.;
        div()
            .relative()
            .w_full()
            .child(self.widths.measure("composer-row"))
            .pl(px(if roomy { indent } else { 8. }))
            .pr(px(if roomy { 12. } else { 8. }))
            .py(px(6.))
            .child(
                div()
                    // The box at most 760 px, as drawn, so a wide diff reaches L.
                    .max_w(px(800.))
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .children(roomy.then(|| discussion::avatar(&self.me, true, 28.).mt(px(3.))))
                    .child(boxed),
            )
            .into_any_element()
    }

    /// The new comment's input: the one input component (variant D) with who sees it on its edge.
    pub(super) fn compose_box(&self, this: WeakEntity<Self>) -> Option<gpui::Div> {
        let compose = self.compose.as_ref()?;
        let request = self.compose_request();
        let source = match compose.target {
            Target::Agent
            | Target::Edit(Change::Agent(_))
            | Target::ThreadNote(_)
            | Target::Answer(_)
            | Target::AnswerValue(_) => Source::Agent,
            Target::Request(_)
            | Target::Reply(_)
            | Target::Edit(Change::Note { .. })
            | Target::InboxReply => Source::GitLab,
        };
        let tone = source.color();
        let (cancel, add, review, switch) =
            (this.clone(), this.clone(), this.clone(), this.clone());
        let switchable = request.is_some();
        let narrow = self.compose_narrow();
        let audience = match compose.target {
            Target::Request(iid) => format!("Everyone on !{iid}"),
            // The chat it will go to, by its page's branch (#77).
            Target::Agent => match self.route_of(self.page_branch().as_deref()) {
                Some(chat) if !chat.title.is_empty() => format!("To: {}", chat.title),
                Some(chat) => format!("To: {}", chat.agent),
                None => "Your agent".to_owned(),
            },
            _ => "Your agent".to_owned(),
        };
        // Narrow, who it goes to is its colour's dot, the words in the tooltip.
        let said: gpui::SharedString = if switchable {
            format!("{audience} — Tab switches who sees it").into()
        } else {
            audience.clone().into()
        };
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
                Source::Agent => theme::agent_bg(),
                _ => theme::warning_bg(),
            })
            .text_size(px(12.))
            .text_color(tone)
            .whitespace_nowrap()
            .child(div().size(px(7.)).rounded_full().bg(tone))
            .when(!narrow, |s| s.child(audience))
            .tooltip_show_delay(crate::ui::hover::LONG)
            .tooltip(move |_, cx| cx.new(|_| super::TipOwned(said.clone())).into())
            .when(switchable, |s| {
                s.cursor_pointer()
                    .child(super::icons::icon("chevron").size(px(12.)).text_color(tone))
                    .on_click(move |_, _, cx| {
                        switch.update(cx, |this, cx| this.toggle_target(cx)).ok();
                    })
            });
        // The foot by its room (Design shell round 6, #129): at S "Add to drafts" is "Add" (named in
        // full in its tooltip), and the other ways to send fold into its own ▾.
        let room = self.compose_room();
        let folded = room == Room::S;
        let primary = match &compose.target {
            Target::Agent if room == Room::S => "Add",
            Target::Agent => "Add to drafts",
            _ => "Comment",
        };
        // An agent comment's bar (#50 F): Send all beside Add to drafts, Send this only in ▾.
        let drafts = self.agent_drafts().len();
        let compose_agent = compose.target == Target::Agent;
        let send_label = if drafts > 0 {
            format!("Send all {}", drafts + 1)
        } else {
            "Send".to_owned()
        };
        let review_iid = match compose.target {
            Target::Request(iid) => Some(iid),
            _ => None,
        };
        let (send_all, send_one, fold_menu) = (this.clone(), this.clone(), this.clone());
        let mut tail = vec![
            // Narrow, Cancel is its × icon, named in the tooltip.
            if narrow {
                discussion::icon_button(
                    "compose-cancel",
                    "x",
                    "Cancel",
                    Some("esc"),
                    None,
                    Kind::Quiet,
                    true,
                )
            } else {
                discussion::button("compose-cancel", "Cancel", None, Kind::Quiet)
                    .tooltip_show_delay(crate::ui::hover::LONG)
                    .tooltip(|_, cx| cx.new(|_| super::Tip("Esc")).into())
            }
            .h(px(28.))
            .px(px(if narrow { 6. } else { 10. }))
            .on_click(move |_, _, cx| {
                cancel.update(cx, |this, cx| this.cancel_comment(cx)).ok();
            })
            .into_any_element(),
        ];
        // The secondary buttons are one weight, bare as Cancel and ▾ are; only the primary is
        // filled.
        if let Some(iid) = review_iid.filter(|_| !folded) {
            tail.push(
                discussion::icon_button(
                    "compose-review",
                    "layers",
                    "Add to review",
                    Some("⇧⌘↵"),
                    Some("Pending until you submit the review"),
                    Kind::Quiet,
                    narrow,
                )
                .h(px(28.))
                .px(px(if narrow { 6. } else { 10. }))
                .on_click(move |_, _, cx| {
                    review
                        .update(cx, |this, cx| this.post_comment(iid, true, cx))
                        .ok();
                })
                .into_any_element(),
            );
        }
        if compose_agent && !folded {
            tail.push(
                discussion::icon_button(
                    "compose-send-all",
                    "send",
                    send_label.clone(),
                    Some("⇧⌘↵"),
                    Some("Adds it and sends every draft to your agent's chat"),
                    Kind::Quiet,
                    narrow,
                )
                .h(px(28.))
                .px(px(if narrow { 6. } else { 10. }))
                .on_click(move |_, _, cx| {
                    send_all
                        .update(cx, |this, cx| this.add_and_send(false, cx))
                        .ok();
                })
                .into_any_element(),
            );
            // ▾: Send this only, the chat it goes to, Copy as markdown (#50 F).
            tail.push(
                discussion::icon_button(
                    "compose-more",
                    "chevron",
                    "Send this only, choose the chat, copy",
                    None,
                    None,
                    Kind::Quiet,
                    true,
                )
                .h(px(28.))
                .px(px(6.))
                .on_click(move |event, _, cx| {
                    let at = event.position();
                    send_one
                        .update(cx, |this, cx| {
                            use super::menu::{Act, Entry};
                            this.open_menu(
                                at,
                                vec![
                                    vec![
                                        Entry::new("Send this only", Act::AgentSendOnly).key("⌥⌘↵"),
                                        Entry::new("Choose the chat…", Act::AgentChooseChat),
                                    ],
                                    vec![Entry::new("Copy as markdown", Act::AgentCopy)],
                                ],
                                cx,
                            )
                        })
                        .ok();
                })
                .into_any_element(),
            );
        }
        // At S, what folded: into the primary's own ▾.
        let more = {
            use super::menu::{Act, Entry};
            match (folded, compose.target) {
                (true, Target::Agent) => vec![
                    vec![
                        Entry::new(send_label, Act::AgentSendAll).key("⇧⌘↵"),
                        Entry::new("Send this only", Act::AgentSendOnly).key("⌥⌘↵"),
                        Entry::new("Choose the chat…", Act::AgentChooseChat),
                    ],
                    vec![Entry::new("Copy as markdown", Act::AgentCopy)],
                ],
                (true, Target::Request(iid)) => vec![vec![
                    Entry::new("Add to review", Act::AddToReview(iid)).key("⇧⌘↵"),
                ]],
                _ => Vec::new(),
            }
        };
        let add_button = discussion::button(
            "compose-add",
            primary,
            (!narrow).then_some("⌘↵"),
            Kind::Primary(tone),
        )
        .h(px(28.))
        .px(px(10.))
        .when(narrow, |s| {
            s.tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(move |_, cx| {
                    cx.new(|_| {
                        super::Tip(if compose_agent {
                            "Add to drafts  ⌘↵"
                        } else {
                            "Comment  ⌘↵"
                        })
                    })
                    .into()
                })
        })
        .on_click(move |_, _, cx| {
            add.update(cx, |this, cx| this.add_comment(cx)).ok();
        });
        tail.push(if more.is_empty() {
            add_button.into_any_element()
        } else {
            div()
                .flex()
                .flex_none()
                .gap(px(1.))
                .child(add_button.rounded_r(px(0.)))
                .child(
                    discussion::icon_button(
                        "compose-add-more",
                        "chevron",
                        if compose_agent {
                            "Send all, send this only, choose the chat, copy"
                        } else {
                            "Add to review"
                        },
                        None,
                        None,
                        Kind::Primary(tone),
                        true,
                    )
                    .h(px(28.))
                    .px(px(6.))
                    .rounded_l(px(0.))
                    .on_click(move |event, _, cx| {
                        let (at, more) = (event.position(), more.clone());
                        fold_menu
                            .update(cx, |this, cx| this.open_menu(at, more, cx))
                            .ok();
                    }),
                )
                .into_any_element()
        });
        let placeholder = match compose.target {
            Target::Agent => "Note for your agent",
            _ => "Comment",
        };
        let input = self.open_input(
            compose,
            tone,
            placeholder,
            Some(chip.into_any_element()),
            &[Helper::Bold, Helper::Code, Helper::Link, Helper::Bullet],
            tail,
            this,
        );
        // Several lines: which, above the box.
        Some(match compose.first {
            Some(first) => div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .font_family(theme::UI_FONT)
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(format!("Lines {first}–{}", compose.line)),
                )
                .child(input),
            None => input,
        })
    }

    /// A reply: the discussion's own one-line field, grown inside its outline (variant D).
    pub(super) fn reply_editor(&self, this: WeakEntity<Self>) -> Option<gpui::Div> {
        let compose = self.compose.as_ref()?;
        // ⇥ turned it into a note for the agent: purple, and it stays on this Mac (#50 G).
        let agent = matches!(
            compose.target,
            Target::ThreadNote(_) | Target::Answer(_) | Target::AnswerValue(_)
        );
        let tone = if agent {
            Source::Agent.color()
        } else {
            Source::GitLab.color()
        };
        let (cancel, send) = (this.clone(), this.clone());
        // The same foot as a new comment's (#129): narrow, Cancel is its ×, who sees it its dot.
        let room = self.compose_room();
        let narrow = room != Room::L;
        let tail = vec![
            if narrow {
                discussion::icon_button(
                    "reply-cancel",
                    "x",
                    "Cancel",
                    Some("esc"),
                    None,
                    Kind::Quiet,
                    true,
                )
            } else {
                discussion::button("reply-cancel", "Cancel", None, Kind::Quiet)
                    .tooltip_show_delay(crate::ui::hover::LONG)
                    .tooltip(|_, cx| cx.new(|_| super::Tip("Esc")).into())
            }
            .h(px(28.))
            .px(px(if narrow { 6. } else { 10. }))
            .on_click(move |_, _, cx| {
                cancel.update(cx, |this, cx| this.cancel_comment(cx)).ok();
            })
            .into_any_element(),
            discussion::button(
                "reply-send",
                if agent && self.inbox && matches!(compose.target, Target::Answer(_)) {
                    "Send answer"
                } else if agent {
                    "Add to drafts"
                } else {
                    "Reply"
                },
                (!narrow).then_some("⌘↵"),
                Kind::Primary(tone),
            )
            .h(px(28.))
            .px(px(10.))
            .on_click(move |_, _, cx| {
                send.update(cx, |this, cx| this.add_comment(cx)).ok();
            })
            .into_any_element(),
        ];
        // Which of the two the field writes, and ⇥ to switch, on a discussion (#50 G).
        let lead = matches!(compose.target, Target::Reply(_) | Target::ThreadNote(_)).then(|| {
            let said = if agent {
                "🔒 For your agent"
            } else {
                "Reply on the merge request"
            };
            div()
                .id("reply-audience")
                .h(px(26.))
                .px(px(8.))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .rounded(px(6.))
                .bg(if agent {
                    theme::agent_bg()
                } else {
                    theme::warning_bg()
                })
                .text_size(px(12.))
                .text_color(tone)
                .whitespace_nowrap()
                .map(|s| {
                    if narrow {
                        // Narrow, its colour's dot; the words and ⇥ in the tooltip.
                        s.child(div().size(px(7.)).rounded_full().bg(tone))
                            .tooltip_show_delay(crate::ui::hover::LONG)
                            .tooltip(move |_, cx| {
                                cx.new(|_| super::TipOwned(format!("{said} — ⇥ switches").into()))
                                    .into()
                            })
                    } else {
                        s.child(said)
                            .child(super::keys::caps("⇥", super::keys::On::Plain))
                    }
                })
                .into_any_element()
        });
        Some(self.open_input(
            compose,
            tone,
            match (compose.target, room) {
                (Target::ThreadNote(_), _) => "Note for your agent",
                (Target::Answer(_), Room::S) => "Note with your answer",
                (Target::Answer(_), _) => "A note for the agent with your answer",
                (Target::AnswerValue(_), _) => "Your own value",
                _ => "Reply…",
            },
            lead,
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
                        None,
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
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| super::Tip(tip)).into())
                .on_click(move |_, _, cx| {
                    this.update(cx, |this, cx| this.apply_helper(h, cx)).ok();
                })
        };
        let preview = compose.preview;
        let toggle = this.clone();
        // One row at every width (Design shell round 6, #129): who sees it and the writing helpers
        // on the left, the buttons on the right; at S the helpers fold into T, a menu of them.
        let folded = self.compose_room() == Room::S;
        let format = {
            use super::menu::{Act, Entry};
            let helpers: Vec<Entry> = tools
                .iter()
                .map(|&h| {
                    let (_, tip) = markdown::helper_look(h);
                    let (name, key) = tip.split_once("  ").unwrap_or((tip, ""));
                    let name = name.split(" — ").next().unwrap_or(name);
                    Entry::new(name, Act::ComposeHelper(h)).key(key)
                })
                .collect();
            vec![
                helpers,
                vec![
                    Entry::new("Preview", Act::ComposePreview)
                        .key("⌘⇧P")
                        .checked(preview),
                ],
            ]
        };
        let open_format = this.clone();
        let helpers = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .children(lead.map(|lead| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .mr(px(4.))
                    .child(lead)
                    .children((!folded).then(|| div().w(px(1.)).h(px(14.)).bg(theme::selected())))
            }))
            .children(folded.then(|| {
                div()
                    .id("md-format")
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
                        super::icons::icon("type")
                            .size(px(15.))
                            .text_color(theme::muted()),
                    )
                    .tooltip_show_delay(crate::ui::hover::LONG)
                    .tooltip(|_, cx| {
                        cx.new(|_| super::Tip("Format: bold, code, link, list, preview"))
                            .into()
                    })
                    .on_click(move |event, _, cx| {
                        let (at, format) = (event.position(), format.clone());
                        open_format
                            .update(cx, |this, cx| this.open_menu(at, format, cx))
                            .ok();
                    })
            }))
            .children(
                tools
                    .iter()
                    .filter(|_| !folded)
                    .map(|&h| tool(h, this.clone())),
            )
            .when(!folded, |s| {
                s.child(
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
                        .tooltip_show_delay(crate::ui::hover::LONG)
                        .tooltip(|_, cx| cx.new(|_| super::Tip("Preview  ⌘⇧P")).into())
                        .on_click(move |_, _, cx| {
                            toggle.update(cx, |this, cx| this.toggle_preview(cx)).ok();
                        }),
                )
            });
        let edge = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(6.))
            .pt(px(4.))
            .pb(px(6.))
            .child(helpers)
            .child(div().flex_1().min_w_0())
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .children(tail),
            );
        div()
            .relative()
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
            // Its own width decides its buttons' (`compose_narrow`).
            .child(self.widths.measure("composer"))
            .child(text)
            .children(
                self.compose_completion()
                    .filter(|_| {
                        self.compose
                            .as_ref()
                            .is_some_and(|c| std::ptr::eq(c, compose))
                    })
                    .map(|(_, choices)| self.render_completion(&choices, compose.pick)),
            )
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
        let (caret, downstream) = (compose.edit.caret(), compose.edit.downstream());
        // Stepped aside, it shows no caret: the field that has the keys does.
        let focused = !self.compose_stepped_aside();
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
                    // Bounded on the right, so a long hint wraps rather than being cut (#129).
                    .right(px(pad_x))
                    .text_color(theme::muted())
                    .child(placeholder)
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        if !focused {
                            return;
                        }
                        let at = input::caret_point(&for_caret, caret, downstream)
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
                let at = input::nearest_index(&for_down, event.position);
                let (shift, clicks) = (event.modifiers.shift, event.click_count);
                let lower = input::lower_line(&for_down, at, event.position.y);
                down.update(cx, |this, cx| {
                    this.compose_place(at, shift, clicks, false, lower, cx)
                })
                .ok();
            })
            .on_mouse_move(move |event, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    let at = input::nearest_index(&for_move, event.position);
                    drag.update(cx, |this, cx| {
                        this.compose_place(at, true, 1, true, false, cx)
                    })
                    .ok();
                }
            })
            .into_any_element()
    }

    /// Whether the open composer has stepped aside for a field still open (a filter, a find).
    pub(super) fn compose_stepped_aside(&self) -> bool {
        self.compose_aside
            && (self.field.is_some() || self.dfind.is_some() || self.find.text.is_some())
    }

    /// Shows a comment: in the open file, scrolled to; in another file of the diff, that file first.
    pub(super) fn jump_to(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some((path, line, old)) = self
            .comments
            .iter()
            .find(|c| c.id == id)
            .map(|c| (c.path.clone(), c.line, c.old))
        else {
            return;
        };
        // A folder's note: the folder's page, where it is written (#97).
        if let Some(folder) = path.strip_suffix('/') {
            return self.open_folder_page(folder.to_owned(), cx);
        }
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
            // Not in the diff open: the file in Code at its line, the note drawn under it there
            // (the developer, 2026-10-23: a note asking you led nowhere).
            None if !old && self.root.is_some() => {
                self.jump = None;
                self.file_jump = Some((path.clone(), line));
                self.nav.space = super::spaces::Space::Code;
                self.open_code_file(path, false, cx);
            }
            None => self.jump = None,
        }
    }

    /// Scrolls to the line and the comments under it that a jump asked for, once its file is laid
    /// out: the line, opened first when it is folded away, with the discussions or the pending
    /// comment below it in view.
    pub(super) fn apply_jump(&mut self, cx: &mut Context<Self>) {
        if let Some((line, old)) = self.jump_line.take() {
            self.reveal_line(line, old);
            if std::mem::take(&mut self.jump_whole) {
                self.reveal_function(line, old);
            }
            if let Some(at) = self.row_of_line(line, old) {
                let under = self.rows[at + 1..]
                    .iter()
                    .take_while(|r| {
                        matches!(
                            r,
                            Row::Request(_) | Row::Draft(_) | Row::Thread(_) | Row::Note(_)
                        )
                    })
                    .count();
                self.reveal_rows(at..at + 1 + under);
                cx.notify();
            }
        }
        let Some(id) = self.jump.take() else {
            return;
        };
        if let Some(at) = self.rows.iter().position(|r| *r == Row::Thread(id)) {
            self.reveal_rows(at.saturating_sub(1)..at + 1);
            cx.notify();
        }
    }
}

/// Where `comment` sits in file `path`, built as `data` (`None` when it belongs to another file).
fn place_in(comment: &Comment, path: &str, data: &FileData) -> Option<Place> {
    if comment.path != path {
        return None;
    }
    let side = data.side(comment.old);
    Some(review::place(comment, side.count(), |n| side.line(n).0))
}

/// What a composer's foot shows, by its box's width (Design shell round 6, 2026-10-24, #129). The
/// foot is one row at each: what does not fit folds, nothing wraps and nothing is cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Room {
    /// 720 px and up: who sees it in words, the helpers, every button with its label and keys.
    L,
    /// The secondary buttons are their icons, who sees it its dot, the keys go.
    M,
    /// Under 440 px: the helpers fold into T, Send and ▾ into the primary's own ▾; the hint is
    /// short. (Drawn at 420; a request's audience chip with its ▾ needs 423 for M's row.)
    S,
}

/// The room of a box `width` wide; 0 is not measured yet. A tight diff is never L.
pub(super) fn room_for(width: f32, tight: bool) -> Room {
    match width {
        w if w > 0. && w < 440. => Room::S,
        w if tight || (w > 0. && w < 720.) => Room::M,
        _ => Room::L,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_composer_folds_its_foot_by_its_own_width() {
        assert_eq!(room_for(0., false), Room::L);
        assert_eq!(room_for(760., false), Room::L);
        assert_eq!(room_for(630., false), Room::M);
        assert_eq!(room_for(440., false), Room::M);
        assert_eq!(room_for(439., false), Room::S);
        assert_eq!(room_for(136., false), Room::S);
        assert_eq!(room_for(900., true), Room::M);
        assert_eq!(room_for(300., true), Room::S);
    }
}
