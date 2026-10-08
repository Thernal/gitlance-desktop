//! Drawing one row of a file diff, and the small controls above it.

use super::rows::{self, Cell, FileData, Row, Side};
use super::{DIFF_ROW, ROW_RADIUS, theme};
use crate::git::LineKind;
use crate::search;
use crate::storage::MarkStyle;
use gpui::{
    AnyElement, App, FontStyle, HighlightStyle, MouseButton, Pixels, Point, Rgba, SharedString,
    StyledText, UnderlineStyle, div, prelude::*, px,
};
use std::rc::Rc;

const TAB_WIDTH: usize = 4;
/// Width of the `+`/`−` column in a unified diff.
const SIGN_WIDTH: f32 = 16.;

/// How rows are drawn right now.
#[derive(Clone, Copy)]
pub struct RowStyle<'a> {
    pub wrap: bool,
    /// Horizontal scroll, in pixels, when not wrapping.
    pub offset: f32,
    /// Width of one line-number column.
    pub gutter: f32,
    /// How changed words are drawn.
    pub marks: MarkStyle,
    /// Words the commit search is looking for; highlighted wherever they appear.
    pub terms: &'a [String],
    /// This row is the one the find bar is on: its matches are drawn solid, not tinted.
    pub strong: bool,
    /// The selected lines: (on the removed side, first line, last line).
    pub sel: Option<(bool, u32, u32)>,
}

impl RowStyle<'_> {
    /// The width taken by line numbers (and signs) in one row.
    pub fn chrome(&self, unified: bool) -> f32 {
        if unified {
            2. * self.gutter + SIGN_WIDTH
        } else {
            2. * self.gutter + 1.
        }
    }
}

/// A callback about a diff line: (on the removed side, line number, …).
type OnLine = Box<dyn Fn(bool, u32, &mut App)>;
/// … with whether ⇧ was held.
type OnPress = Box<dyn Fn(bool, u32, bool, &mut App)>;
/// … with a window position.
type OnContext = Box<dyn Fn(bool, u32, Point<Pixels>, &mut App)>;

/// What a row reports back.
pub struct Events {
    /// A gutter's + was clicked.
    pub comment: OnLine,
    /// The mouse went down on a line; whether ⇧ was held.
    pub press: OnPress,
    /// The pointer moved over a line with the button down.
    pub drag: OnLine,
    /// A right click on a line, at a window position.
    pub context: OnContext,
}

pub type OnEvents = Rc<Events>;

pub fn row(
    data: &FileData,
    row: Row,
    style: RowStyle<'_>,
    on_expand: impl Fn(usize, &mut App) + 'static,
    events: OnEvents,
) -> AnyElement {
    match row {
        Row::Gap { segment, count } => div()
            .id(("gap", segment))
            .h(px(DIFF_ROW))
            .my(px(2.))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .rounded(px(ROW_RADIUS))
            .bg(theme::hover())
            .text_color(theme::muted())
            .cursor_pointer()
            .hover(|s| s.bg(theme::selected()).text_color(theme::text()))
            .child("↕")
            .child(format!(
                "{count} unchanged line{}",
                if count == 1 { "" } else { "s" }
            ))
            .on_click(move |_, _, cx| on_expand(segment, cx))
            .into_any_element(),
        Row::Split { left, right } => div()
            .w_full()
            .min_h(px(DIFF_ROW))
            .flex()
            .group("diff-row")
            .child(half(&data.old, left, true, style, events.clone()))
            .child(div().w(px(1.)).flex_none().bg(theme::border()))
            .child(half(&data.new, right, false, style, events))
            .into_any_element(),
        // Drawn by the workspace, which owns the comments.
        Row::Thread(_) | Row::Composer => div().into_any_element(),
        Row::Unified {
            cell,
            old,
            old_line,
            new_line,
        } => {
            let (line_bg, gutter_bg) = tints(cell.kind);
            let sign = match cell.kind {
                LineKind::Added => "+",
                LineKind::Removed => "−",
                LineKind::Context => "",
            };
            div()
                .w_full()
                .min_h(px(DIFF_ROW))
                .flex()
                .when_some(line_bg, |s, bg| s.bg(bg))
                .group("diff-row")
                .child(gutter(old_line, gutter_bg, style, None))
                .child(gutter(
                    new_line,
                    gutter_bg,
                    style,
                    Some((old, cell.line, events.clone())),
                ))
                .child(
                    div()
                        .w(px(SIGN_WIDTH))
                        .flex_none()
                        .text_color(theme::faint())
                        .child(sign),
                )
                .child(code(data.side(old), cell, old, style, events))
                .into_any_element()
        }
    }
}

/// One side of a split row; an empty side is shaded.
fn half(
    side: &Side,
    cell: Option<Cell>,
    old: bool,
    style: RowStyle<'_>,
    events: OnEvents,
) -> impl IntoElement + use<> {
    let half = div().flex_1().min_w_0().flex();
    let Some(cell) = cell else {
        return half.bg(theme::panel());
    };
    let (line_bg, gutter_bg) = tints(cell.kind);
    half.when_some(line_bg, |s, bg| s.bg(bg))
        .child(gutter(
            Some(cell.line),
            gutter_bg,
            style,
            Some((old, cell.line, events.clone())),
        ))
        .child(code(side, cell, old, style, events))
}

/// A line-number column; `comment` adds a + that appears on hover and starts a comment on that line.
fn gutter(
    line: Option<u32>,
    bg: Option<Rgba>,
    style: RowStyle<'_>,
    comment: Option<(bool, u32, OnEvents)>,
) -> impl IntoElement + use<> {
    div()
        .w(px(style.gutter))
        .flex_none()
        .relative()
        .flex()
        .justify_end()
        .pr_2()
        .when_some(bg, |s, bg| s.bg(bg))
        .text_color(theme::faint())
        .children(line.map(|l| l.to_string()))
        .children(comment.map(|(old, line, events)| {
            div()
                .id(("comment-plus", u64::from(line) * 2 + u64::from(old)))
                .absolute()
                .left(px(3.))
                .top(px(1.))
                .size(px(18.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(ROW_RADIUS))
                .bg(theme::accent())
                .text_color(theme::base())
                .cursor_pointer()
                .invisible()
                .group_hover("diff-row", |s| s.visible())
                .child("+")
                .on_click(move |_, _, cx| (events.comment)(old, line, cx))
        }))
}

fn code(
    side: &Side,
    cell: Cell,
    old: bool,
    style: RowStyle<'_>,
    events: OnEvents,
) -> impl IntoElement + use<> {
    let (text, spans, marks) = side.line(cell.line);
    let found = search::highlights(text, style.terms);
    let runs = rows::runs(text, spans, marks, &found);
    let (text, map) = rows::expand_tabs(text, TAB_WIDTH);
    let highlights: Vec<_> = runs
        .into_iter()
        .map(|run| {
            (
                map[run.range.start]..map[run.range.end],
                HighlightStyle {
                    color: run.color.map(theme::code),
                    font_style: run.italic.then_some(FontStyle::Italic),
                    ..if run.found && style.strong {
                        HighlightStyle {
                            color: Some(theme::base().into()),
                            background_color: Some(theme::warning().into()),
                            ..Default::default()
                        }
                    } else if run.found {
                        HighlightStyle {
                            background_color: Some(theme::warning_bg().into()),
                            ..Default::default()
                        }
                    } else if run.marked {
                        word_mark(style.marks, old)
                    } else {
                        HighlightStyle::default()
                    }
                },
            )
        })
        .collect();
    let text = StyledText::new(text).with_highlights(highlights);
    let line = div().pl_2();
    let line = if style.wrap {
        line.child(text)
    } else {
        line.relative()
            .left(px(-style.offset))
            .whitespace_nowrap()
            .child(text)
    };
    let selected = style
        .sel
        .is_some_and(|(o, lo, hi)| o == old && (lo..=hi).contains(&cell.line));
    let line_no = cell.line;
    let (press, drag, context) = (events.clone(), events.clone(), events);
    div()
        .id("code")
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .when(selected, |s| s.bg(theme::selection()))
        .child(line)
        // A press selects the line, a drag extends the selection, a right click opens the menu.
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            (press.press)(old, line_no, event.modifiers.shift, cx)
        })
        .on_mouse_move(move |event, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                (drag.drag)(old, line_no, cx)
            }
        })
        .on_mouse_down(MouseButton::Right, move |event, _, cx| {
            (context.context)(old, line_no, event.position, cx)
        })
}

/// How a changed word is drawn: a stronger tint, or an underline in the line's own hue.
fn word_mark(marks: MarkStyle, removed: bool) -> HighlightStyle {
    let (tint, ink) = if removed {
        (theme::removed_word(), theme::removed())
    } else {
        (theme::added_word(), theme::added())
    };
    match marks {
        MarkStyle::Tinted => HighlightStyle {
            background_color: Some(tint.into()),
            ..Default::default()
        },
        MarkStyle::Underlined => HighlightStyle {
            underline: Some(UnderlineStyle {
                thickness: px(2.),
                color: Some(ink.into()),
                wavy: false,
            }),
            ..Default::default()
        },
    }
}

/// Two sample lines, for the Settings page: what a changed word looks like in `marks`.
pub fn mark_preview(marks: MarkStyle) -> impl IntoElement {
    let line = |text: &'static str, removed: bool, changed: &'static str| {
        let at = text.find(changed).unwrap_or(0);
        let (line_bg, _) = tints(if removed {
            LineKind::Removed
        } else {
            LineKind::Added
        });
        div()
            .w_full()
            .h(px(DIFF_ROW))
            .px_3()
            .when_some(line_bg, |s, bg| s.bg(bg))
            .font_family(theme::CODE_FONT)
            .text_size(px(super::CODE_SIZE))
            .line_height(px(DIFF_ROW))
            .child(
                StyledText::new(text)
                    .with_highlights([(at..at + changed.len(), word_mark(marks, removed))]),
            )
    };
    div()
        .py(px(4.))
        .child(line("    if commits.len() > MAX_COMMITS {", true, ">"))
        .child(line("    if commits.len() >= MAX_COMMITS {", false, ">="))
}

fn tints(kind: LineKind) -> (Option<Rgba>, Option<Rgba>) {
    match kind {
        LineKind::Context => (None, None),
        LineKind::Added => (Some(theme::added_line()), Some(theme::added_gutter())),
        LineKind::Removed => (Some(theme::removed_line()), Some(theme::removed_gutter())),
    }
}

/// A small toggle in the diff toolbar.
pub fn chip(
    id: &'static str,
    label: impl Into<SharedString>,
    active: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py(px(2.))
        .rounded(px(ROW_RADIUS))
        .text_size(px(12.))
        .cursor_pointer()
        .when(active, |s| {
            s.bg(theme::selected()).text_color(theme::text())
        })
        .when(!active, |s| {
            s.text_color(theme::muted())
                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
        })
        .child(label.into())
}

/// Chips that belong together, on a shared rounded track.
pub fn group() -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(ROW_RADIUS + 2.))
        .bg(theme::base())
}
