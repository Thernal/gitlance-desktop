//! Drawing one row of a file diff, and the small controls above it.

use super::rows::{self, Cell, FileData, Row, Side};
use super::{DIFF_ROW, ROW_RADIUS, theme};
use crate::git::LineKind;
use gpui::{
    AnyElement, App, FontStyle, HighlightStyle, Rgba, SharedString, StyledText, div, prelude::*, px,
};

const TAB_WIDTH: usize = 4;
/// Width of the `+`/`−` column in a unified diff.
const SIGN_WIDTH: f32 = 16.;

/// How rows are drawn right now.
#[derive(Clone, Copy)]
pub struct RowStyle {
    pub wrap: bool,
    /// Horizontal scroll, in pixels, when not wrapping.
    pub offset: f32,
    /// Width of one line-number column.
    pub gutter: f32,
}

impl RowStyle {
    /// The width taken by line numbers (and signs) in one row.
    pub fn chrome(&self, unified: bool) -> f32 {
        if unified {
            2. * self.gutter + SIGN_WIDTH
        } else {
            2. * self.gutter + 1.
        }
    }
}

pub fn row(
    data: &FileData,
    row: Row,
    style: RowStyle,
    on_expand: impl Fn(usize, &mut App) + 'static,
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
            .child(half(&data.old, left, true, style))
            .child(div().w(px(1.)).flex_none().bg(theme::border()))
            .child(half(&data.new, right, false, style))
            .into_any_element(),
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
                .child(gutter(old_line, gutter_bg, style))
                .child(gutter(new_line, gutter_bg, style))
                .child(
                    div()
                        .w(px(SIGN_WIDTH))
                        .flex_none()
                        .text_color(theme::faint())
                        .child(sign),
                )
                .child(code(data.side(old), cell, old, style))
                .into_any_element()
        }
    }
}

/// One side of a split row; an empty side is shaded.
fn half(side: &Side, cell: Option<Cell>, old: bool, style: RowStyle) -> impl IntoElement {
    let half = div().flex_1().min_w_0().flex();
    let Some(cell) = cell else {
        return half.bg(theme::panel());
    };
    let (line_bg, gutter_bg) = tints(cell.kind);
    half.when_some(line_bg, |s, bg| s.bg(bg))
        .child(gutter(Some(cell.line), gutter_bg, style))
        .child(code(side, cell, old, style))
}

fn gutter(line: Option<u32>, bg: Option<Rgba>, style: RowStyle) -> impl IntoElement {
    div()
        .w(px(style.gutter))
        .flex_none()
        .flex()
        .justify_end()
        .pr_2()
        .when_some(bg, |s, bg| s.bg(bg))
        .text_color(theme::faint())
        .children(line.map(|l| l.to_string()))
}

fn code(side: &Side, cell: Cell, old: bool, style: RowStyle) -> impl IntoElement {
    let (text, spans, marks) = side.line(cell.line);
    let runs = rows::runs(text, spans, marks);
    let (text, map) = rows::expand_tabs(text, TAB_WIDTH);
    let mark = if old {
        theme::removed_word()
    } else {
        theme::added_word()
    };
    let highlights: Vec<_> = runs
        .into_iter()
        .map(|run| {
            (
                map[run.range.start]..map[run.range.end],
                HighlightStyle {
                    color: run.color.map(theme::code),
                    font_style: run.italic.then_some(FontStyle::Italic),
                    background_color: run.marked.then(|| mark.into()),
                    ..Default::default()
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
    div().flex_1().min_w_0().overflow_hidden().child(line)
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
