//! Line comments for a coding agent: a + in the gutter opens a composer under the line, comments sit
//! as threads under their lines, and a Review island lists them with *Copy for agent*. Comments are
//! kept on this Mac (`crate::review`), never in the reviewed repository. Designed in
//! `../Design/mockups/line-comments/a-agent-comments.html`.

use super::rows::Row;
use super::{GAP, ROW_RADIUS, Selection, Workspace, format, island, island_label, theme};
use crate::review::{self, Comment, Place};
use gpui::{
    ClickEvent, ClipboardItem, Context, FontWeight, KeyDownEvent, WeakEntity, div, prelude::*, px,
};

/// A comment being written.
pub struct Compose {
    path: String,
    old: bool,
    line: u32,
    code: String,
    pub body: String,
    at: String,
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
            Selection::None => String::new(),
        };
        self.compose = Some(Compose {
            path,
            old,
            line,
            code,
            body: String::new(),
            at,
        });
        self.refresh_rows();
        cx.notify();
    }

    pub(super) fn cancel_comment(&mut self, cx: &mut Context<Self>) {
        if self.compose.take().is_some() {
            self.refresh_rows();
            cx.notify();
        }
    }

    pub(super) fn add_comment(&mut self, cx: &mut Context<Self>) {
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

    /// Typing into the composer: text, backspace, enter for a new line, ⌘↵ to add, esc to cancel.
    pub(super) fn compose_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let Some(compose) = self.compose.as_mut() else {
            return;
        };
        if key.key == "escape" {
            return self.cancel_comment(cx);
        }
        if key.modifiers.platform {
            match key.key.as_str() {
                "enter" => return self.add_comment(cx),
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                        compose.body.push_str(&text);
                    }
                }
                "backspace" => compose.body.clear(),
                _ => return,
            }
        } else {
            match key.key.as_str() {
                "backspace" => {
                    compose.body.pop();
                }
                "enter" => compose.body.push('\n'),
                _ => match &key.key_char {
                    Some(ch) if !key.modifiers.control && !ch.chars().any(char::is_control) => {
                        compose.body.push_str(ch)
                    }
                    _ => return,
                },
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
                            .child(div().child("You"))
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
        let (add, cancel) = (this.clone(), this);
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
                    .border_1()
                    .border_color(theme::focus())
                    .bg(theme::panel())
                    .font_family(theme::UI_FONT)
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .child(if compose.body.is_empty() {
                        div()
                            .text_color(theme::faint())
                            .child("Comment for your agent…  ⌘↵ adds it")
                    } else {
                        div().child(format!("{}▏", compose.body))
                    })
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
                                    .bg(theme::accent())
                                    .text_color(theme::base())
                                    .cursor_pointer()
                                    .child("Add comment  ⌘↵")
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
            .child(island_label(format!(
                "Review · {count} comment{}",
                if count == 1 { "" } else { "s" }
            )))
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
        let Some(id) = self.jump.take() else {
            return;
        };
        if let Some(at) = self.rows.iter().position(|r| *r == Row::Thread(id)) {
            self.diff_list.scroll_to_reveal_item(at);
            cx.notify();
        }
    }
}
