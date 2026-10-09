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
        if here.is_empty() && compose.is_none() {
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
                if compose.is_some_and(|c| c.old == old && c.line == line) {
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

    /// The merge request whose diff is open, when a comment can be posted to it.
    pub(super) fn request_target(&self) -> Option<u64> {
        match self.diff.as_ref()?.header {
            super::Header::Compare {
                request: Some(iid),
                since_merge_base: true,
                ..
            } if self.web.is_some() && crate::mr::token().is_some() || crate::mr::fixture_on() => {
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
        if let Some(Compose {
            target: Target::Request(iid),
            ..
        }) = self.compose
        {
            return self.post_comment(iid, cx);
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
    fn post_comment(&mut self, iid: u64, cx: &mut Context<Self>) {
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
        if let Some(c) = self.compose.as_mut() {
            c.posting = true;
            c.error = None;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
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
                    )
                })
                .await;
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

    /// `c`: type a line number, then the comment.
    pub(super) fn open_goline(&mut self, cx: &mut Context<Self>) {
        if self.data.is_none() || self.settings_open || self.compose.is_some() {
            return;
        }
        self.field_all = false;
        self.zone = super::zones::Zone::Diff;
        self.goline = Some(String::new());
        cx.notify();
    }

    pub(super) fn goline_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(text) = self.goline.as_mut() else {
            return;
        };
        match input::edit(text, &mut self.field_all, &event.keystroke, cx) {
            Edit::Escape => self.goline = None,
            // Digits, and a minus first for a removed line.
            Edit::Changed => {
                let filtered: String = text
                    .chars()
                    .enumerate()
                    .filter(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == '-'))
                    .map(|(_, c)| c)
                    .collect();
                *text = filtered;
            }
            Edit::Enter { .. } => return self.finish_goline(cx),
            Edit::Selected | Edit::Ignored => {}
        }
        cx.notify();
    }

    /// Opens the composer under the line that was typed; with nothing typed, under the first change.
    fn finish_goline(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.goline.take() else {
            return;
        };
        let Some(data) = self.data.clone() else {
            return;
        };
        let (old, line) = if text.trim().is_empty() {
            let first = self
                .changes
                .first()
                .and_then(|&row| self.rows.get(row))
                .copied();
            match first {
                Some(Row::Split { right: Some(c), .. }) => (false, c.line),
                Some(Row::Split { left: Some(c), .. }) => (true, c.line),
                Some(Row::Unified { cell, old, .. }) => (old, cell.line),
                _ => return cx.notify(),
            }
        } else {
            let old = text.starts_with('-');
            let Ok(n) = text.trim_start_matches('-').parse::<u32>() else {
                return cx.notify();
            };
            (old, n)
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

    pub(super) fn render_goline(&self) -> Option<impl IntoElement + use<>> {
        let text = self.goline.as_ref()?;
        Some(
            div()
                .absolute()
                .bottom(px(16.))
                .left(px(16.))
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
                        .child(
                            div()
                                .min_w(px(60.))
                                .font_family(theme::CODE_FONT)
                                .child(input::field_text(text, true, self.field_all, "number")),
                        ),
                )
                .child(div().text_size(px(11.)).text_color(theme::faint()).child(
                    "↵ opens the comment · −12 for a removed line · empty: the first change · esc",
                )),
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
        let to_request = matches!(compose.target, Target::Request(_));
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
                    .child(
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
                            ),
                    )
                    .child(if compose.body.is_empty() {
                        div().text_color(theme::faint()).child(if to_request {
                            "Comment on the merge request — everyone on it will see it  ⌘↵ posts"
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
                                        (Target::Request(_), true) => "Posting…".to_owned(),
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
    fn jump_to(&mut self, id: u64, cx: &mut Context<Self>) {
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
            self.diff_list.scroll_to_reveal_item(at);
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
