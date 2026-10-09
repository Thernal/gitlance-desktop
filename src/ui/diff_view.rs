//! Drawing one row of a file diff, and the small controls above it.

use super::px;
use super::rows::{self, Cell, FileData, Row, Side};
use super::select::Sel;
use super::{DIFF_ROW, ROW_RADIUS, theme};
use crate::git::LineKind;
use crate::search;
use crate::storage::MarkStyle;
use gpui::{
    AnyElement, App, FontStyle, HighlightStyle, MouseButton, Pixels, Point, Rgba, SharedString,
    StyledText, UnderlineStyle, div, prelude::*,
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
    /// The selected text.
    pub sel: Option<Sel>,
    /// The lines of this file that have comments.
    pub notes: &'a [LineNote],
    /// The height of a line of code, in pixels (the zoom applied).
    pub row: f32,
    /// The annotation column is on; its cells (empty until blame is read).
    pub annot: Option<&'a [AnnCell]>,
    /// The file is added or deleted as a whole: its lines keep the gutter bar but no fill.
    pub whole: bool,
}

impl RowStyle<'_> {
    /// The width taken by line numbers (and signs) in one row.
    pub fn chrome(&self, unified: bool) -> f32 {
        let annot = if self.annot.is_some() { ANNOT_W } else { 0. };
        if unified {
            2. * self.gutter + SIGN_WIDTH + annot
        } else {
            2. * self.gutter + 1. + annot
        }
    }
}

/// What the annotation column says about one line of the new side.
#[derive(Clone, Debug)]
pub struct AnnCell {
    pub id: git2::Oid,
    /// "Anna · 3 d" on the first line of a run by the same commit; nothing on the rest.
    pub label: Option<SharedString>,
    /// The commit's title, hash and age, for the tooltip.
    pub tip: SharedString,
    /// This line comes from the commit being viewed.
    pub current: bool,
}

/// The width of the annotation column.
pub const ANNOT_W: f32 = 172.;

/// Whose comment sits on a line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteTone {
    /// A discussion on the merge request (GitLab).
    Request,
    /// A pending comment of one's own on the request.
    Draft,
    /// A comment kept for the agent.
    Agent,
    /// A line marked with F3.
    Bookmark,
}

/// A line that has comments: drawn as a mark in its gutter.
#[derive(Clone, Copy, Debug)]
pub struct LineNote {
    pub old: bool,
    pub line: u32,
    pub tone: NoteTone,
    pub count: usize,
}

/// A callback about a diff line: (on the removed side, line number, …).
type OnLine = Box<dyn Fn(bool, u32, &mut App)>;
/// … with the byte under the pointer, the click count and whether ⇧ was held.
type OnPress = Box<dyn Fn(bool, u32, usize, usize, bool, &mut App)>;
/// … with the byte under the pointer.
type OnDrag = Box<dyn Fn(bool, u32, usize, &mut App)>;
/// … with a window position.
type OnContext = Box<dyn Fn(bool, u32, Point<Pixels>, &mut App)>;

/// What a row reports back.
pub struct Events {
    /// A gutter's + was clicked.
    pub comment: OnLine,
    /// A gutter's comment mark was clicked: open or fold what is on that line.
    pub toggle: OnLine,
    /// An annotation was clicked: open the commit that wrote the line.
    pub annotate: OnLine,
    /// The mouse went down on a line.
    pub press: OnPress,
    /// The pointer moved over a line with the button down.
    pub drag: OnDrag,
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
            .h(px(style.row))
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
            .min_h(px(style.row))
            .flex()
            .group("diff-row")
            .child(half(&data.old, left, true, style, events.clone()))
            .child(div().w(px(1.)).flex_none().bg(theme::border()))
            .child(half(&data.new, right, false, style, events))
            .into_any_element(),
        // Drawn by the workspace, which owns the comments.
        Row::Thread(_) | Row::Request(_) | Row::Draft(_) | Row::Composer => {
            div().into_any_element()
        }
        Row::Unified {
            cell,
            old,
            old_line,
            new_line,
        } => {
            let (line_bg, gutter_bg) = tints(cell.kind);
            let line_bg = line_bg.filter(|_| !style.whole);
            let sign = match cell.kind {
                LineKind::Added => "+",
                LineKind::Removed => "−",
                LineKind::Context => "",
            };
            div()
                .w_full()
                .min_h(px(style.row))
                .flex()
                .when_some(line_bg, |s, bg| s.bg(bg))
                .group("diff-row")
                .children(annot_col(style, new_line, events.clone()))
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
    let line_bg = line_bg.filter(|_| !style.whole);
    half.when_some(line_bg, |s, bg| s.bg(bg))
        .children(if old {
            None
        } else {
            annot_col(style, Some(cell.line), events.clone())
        })
        .child(gutter(
            Some(cell.line),
            gutter_bg,
            style,
            Some((old, cell.line, events.clone())),
        ))
        .child(code(side, cell, old, style, events))
}

/// The annotation column beside a new-side line: who wrote it; a click opens the commit.
fn annot_col(style: RowStyle<'_>, line: Option<u32>, events: OnEvents) -> Option<AnyElement> {
    let cells = style.annot?;
    let n = line?;
    let cell = cells.get(n.checked_sub(1)? as usize);
    let col = div()
        .w(px(ANNOT_W))
        .flex_none()
        .px(px(6.))
        .text_size(px(11.));
    let Some(cell) = cell else {
        return Some(col.into_any_element());
    };
    let tip = cell.tip.clone();
    Some(
        col.id(("annot", u64::from(n)))
            .truncate()
            .text_color(if cell.current {
                theme::accent()
            } else {
                theme::muted()
            })
            .cursor_pointer()
            .hover(|s| s.text_color(theme::text()))
            .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
            .children(cell.label.clone())
            .on_click(move |_, _, cx| (events.annotate)(false, n, cx))
            .into_any_element(),
    )
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
        .text_color(theme::muted())
        .children(line.map(|l| l.to_string()))
        .children(comment.clone().and_then(|(old, line, events)| {
            let note = style
                .notes
                .iter()
                .find(|n| n.old == old && n.line == line)
                .copied()?;
            let tone = match note.tone {
                NoteTone::Request => theme::warning(),
                NoteTone::Draft => theme::renamed(),
                NoteTone::Agent => theme::focus(),
                NoteTone::Bookmark => theme::renamed(),
            };
            Some(
                div()
                    .id(("comment-mark", u64::from(line) * 2 + u64::from(old)))
                    .absolute()
                    .left(px(2.))
                    .top(px(1.))
                    .h(px(18.))
                    .px(px(3.))
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .rounded(px(ROW_RADIUS))
                    .text_size(px(10.))
                    .text_color(tone)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(
                        super::icons::icon(if note.tone == NoteTone::Bookmark {
                            "bookmark"
                        } else {
                            "message"
                        })
                        .size(px(13.))
                        .text_color(tone),
                    )
                    .on_click(move |_, _, cx| (events.toggle)(old, line, cx)),
            )
        }))
        .children(comment.and_then(|(old, line, events)| {
            if style.notes.iter().any(|n| n.old == old && n.line == line) {
                return None;
            }
            Some(
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
                    .on_click(move |_, _, cx| (events.comment)(old, line, cx)),
            )
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
    let picked = style
        .sel
        .filter(|s| s.old == old)
        .and_then(|s| s.in_line(cell.line, text.len()));
    let runs = rows::runs(text, spans, marks, &found, picked);
    let original = text.to_owned();
    let (text, map) = rows::expand_tabs(text, TAB_WIDTH);
    let highlights: Vec<_> = runs
        .into_iter()
        .map(|run| {
            (
                map[run.range.start]..map[run.range.end],
                HighlightStyle {
                    color: run.color.map(theme::code),
                    font_style: run.italic.then_some(FontStyle::Italic),
                    ..if run.selected {
                        HighlightStyle {
                            background_color: Some(theme::selection().into()),
                            ..Default::default()
                        }
                    } else if run.found && style.strong {
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
    // Where a pointer position falls in the line's own text: the shaped text knows its columns, the
    // map undoes the tab expansion.
    let layout = text.layout().clone();
    let at: Rc<dyn Fn(Point<Pixels>) -> usize> = {
        let map = Rc::new((map, original));
        Rc::new(move |position| {
            let expanded = layout.index_for_position(position).unwrap_or_else(|e| e);
            to_original(&map.0, &map.1, expanded)
        })
    };
    let line = div().pl_2();
    let line = if style.wrap {
        line.child(text)
    } else {
        line.relative()
            .left(px(-style.offset))
            .whitespace_nowrap()
            .child(text)
    };
    let line_no = cell.line;
    let (press, drag, context) = (events.clone(), events.clone(), events);
    let (at_press, at_drag) = (at.clone(), at);
    div()
        .id("code")
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .cursor_text()
        .child(line)
        // A press puts the caret (or takes a word, or the line), a drag selects, a right click
        // opens the menu.
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            (press.press)(
                old,
                line_no,
                at_press(event.position),
                event.click_count,
                event.modifiers.shift,
                cx,
            )
        })
        .on_mouse_move(move |event, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                (drag.drag)(old, line_no, at_drag(event.position), cx)
            }
        })
        .on_mouse_down(MouseButton::Right, move |event, _, cx| {
            (context.context)(old, line_no, event.position, cx)
        })
}

/// The byte of the original text that tab-expanded byte `expanded` came from.
fn to_original(map: &[usize], text: &str, expanded: usize) -> usize {
    if expanded >= map[text.len()] {
        return text.len();
    }
    text.char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| map[i] <= expanded)
        .last()
        .unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_in_expanded_text_maps_back_to_the_original_byte() {
        let (_, map) = rows::expand_tabs("\tab\tc", 4);
        // "    ab  c": the tab is columns 0..4, a=4, b=5, tab=6..8, c=8.
        assert_eq!(to_original(&map, "\tab\tc", 0), 0);
        assert_eq!(to_original(&map, "\tab\tc", 3), 0);
        assert_eq!(to_original(&map, "\tab\tc", 4), 1);
        assert_eq!(to_original(&map, "\tab\tc", 6), 3);
        assert_eq!(to_original(&map, "\tab\tc", 99), 5);
    }
}
