//! Editing the text of a field from key events and drawing it. A field's text is a `String` its
//! owner keeps; the caret, the selection and undo live in one `TextEdit` the workspace shares, since
//! one field has the keyboard at a time. When the owner's text changed behind its back (cleared,
//! filled from a tag), `sync` starts the edit over with the caret at the end.

use super::{Workspace, px, theme};
use crate::textedit::TextEdit;
use gpui::{
    AnyElement, App, Context, Div, Keystroke, Pixels, TextLayout, WeakEntity, div, prelude::*,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// What a key press did to a field.
pub enum Edit {
    /// The text changed.
    Changed,
    /// The caret or the selection moved; nothing changed.
    Selected,
    /// Return was pressed; `shift` for ⇧Return.
    Enter {
        shift: bool,
    },
    Escape,
    /// Not a key a field takes.
    Ignored,
}

/// A click in a field's text: where (a byte offset), ⇧, the click count, and whether it is a drag.
/// The last flag: the press was on the lower line of a soft wrap at that offset.
pub type Place = Rc<dyn Fn(usize, bool, usize, bool, bool, &mut App)>;

/// Where the caret at `index` is drawn. At a soft wrap the offset ends the upper line and starts
/// the lower one; `downstream` puts the caret at the start of the lower.
pub fn caret_point(
    layout: &TextLayout,
    index: usize,
    downstream: bool,
) -> Option<gpui::Point<Pixels>> {
    let at = layout.position_for_index(index)?;
    if downstream {
        let (height, bounds) = (layout.line_height(), layout.bounds());
        let next = gpui::point(bounds.left(), at.y + height * 1.5);
        if at.y + height < bounds.bottom()
            && layout.index_for_position(next).unwrap_or_else(|n| n) == index
        {
            return Some(gpui::point(bounds.left(), at.y + height));
        }
    }
    Some(at)
}

/// The offset nearest to `point` in a laid-out text: GPUI gives the start of the char under the
/// point, so a point past a char's middle goes after it.
pub fn nearest_index(layout: &TextLayout, point: gpui::Point<Pixels>) -> usize {
    let text = layout.text();
    let at = layout
        .index_for_position(point)
        .unwrap_or_else(|n| n)
        .min(text.len());
    let Some(c) = text[at..].chars().next().filter(|c| *c != '\n') else {
        return at;
    };
    let next = at + c.len_utf8();
    match (
        layout.position_for_index(at),
        layout.position_for_index(next),
    ) {
        (Some(a), Some(b)) if a.y == b.y && point.x - a.x > b.x - point.x => next,
        _ => at,
    }
}

/// Whether a press at height `y` that hit offset `index` was on the line below where the layout
/// puts that offset: the start of a wrapped line.
pub fn lower_line(layout: &TextLayout, index: usize, y: Pixels) -> bool {
    layout
        .position_for_index(index)
        .is_some_and(|p| p.y + layout.line_height() <= y)
}

/// Makes `edit` the edit of `text`, unless it already is.
pub fn sync(text: &str, edit: &mut TextEdit) {
    if edit.text() != text {
        *edit = TextEdit::new(text);
    }
}

/// Applies `key` to the one-line field `text`: the caret moves (⌥ by words, ⌘ to the ends, ⇧
/// selects), ⌥⌫ ⌘⌫ delete a word or the line, ⌘A ⌘C ⌘X ⌘V ⌘Z ⇧⌘Z, typing. ↑ ↓ are left to the
/// caller (lists step with them), and a pasted line break becomes a space.
pub fn edit(text: &mut String, field: &mut TextEdit, key: &Keystroke, cx: &mut App) -> Edit {
    if matches!(key.key.as_str(), "up" | "down") {
        return Edit::Ignored;
    }
    sync(text, field);
    let done = edit_with(field, key, cx, false);
    if field.text() != text {
        *text = field.text().to_owned();
    }
    done
}

/// Applies `key` to the multi-line field `text`; ↑ ↓ move between its lines. Return is the
/// caller's.
pub fn edit_area(text: &mut String, field: &mut TextEdit, key: &Keystroke, cx: &mut App) -> Edit {
    sync(text, field);
    let done = edit_with(field, key, cx, true);
    if field.text() != text {
        *text = field.text().to_owned();
    }
    done
}

/// Where a wrapped field keeps its text's last layout, for ↑ ↓ by drawn lines.
pub type Lines = Rc<RefCell<Option<TextLayout>>>;

/// How a field's text is drawn.
#[derive(Clone, Default)]
pub struct Look {
    /// Wraps in its box (a multi-line field); else one line that scrolls sideways to its caret.
    pub wrap: bool,
    /// Every char shows as a dot (a token).
    pub mask: bool,
    /// Keeps the layout of a wrapped field.
    pub lines: Option<Lines>,
}

/// A move by the lines of a wrapped field as drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Drawn {
    Up,
    Down,
    /// The start of the drawn line (⌘←).
    Start,
    /// Its end (⌘→).
    End,
}

/// Moves the caret of a wrapped field by drawn lines: ↑ ↓ to the line above or below at the x the
/// first of these presses started from (`goal`), from the first or last line to the text's start
/// or end; ⌘← ⌘→ to the ends of the caret's line. `false` when `layout` is not this text's (it was
/// drawn before the last edit).
pub fn drawn(
    edit: &mut TextEdit,
    layout: &TextLayout,
    goal: &mut Option<Pixels>,
    motion: Drawn,
    select: bool,
) -> bool {
    // The field draws its text with a trailing space.
    let len = edit.text().len();
    if layout.len() != len + 1 {
        return false;
    }
    let Some(at) = caret_point(layout, edit.caret(), edit.downstream()) else {
        return false;
    };
    let height = layout.line_height();
    let bounds = layout.bounds();
    let index = |x: Pixels, y: Pixels| nearest_index(layout, gpui::point(x, y)).min(len);
    let target = match motion {
        Drawn::Up | Drawn::Down => {
            let x = *goal.get_or_insert(at.x);
            let y = if motion == Drawn::Up {
                at.y - height * 0.5
            } else {
                at.y + height * 1.5
            };
            if y < bounds.top() {
                0
            } else if y >= bounds.bottom() {
                len
            } else {
                index(x, y)
            }
        }
        Drawn::Start => index(bounds.left(), at.y + height * 0.5),
        Drawn::End => {
            let end = index(bounds.right(), at.y + height * 0.5);
            // At a soft wrap the nearest index starts the next line: stay before it.
            match layout.position_for_index(end) {
                Some(p) if p.y > at.y && end > 0 => edit.text()[..end]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i),
                _ => end,
            }
        }
    };
    edit.go_to(target, select);
    // Aimed at a line whose start is where the layout ends the line above: stand on the line aimed at.
    let aimed = match motion {
        Drawn::Up => Some(at.y - height),
        Drawn::Down => Some(at.y + height),
        Drawn::Start => Some(at.y),
        Drawn::End => None,
    };
    if let Some(line) = aimed {
        edit.set_downstream(lower_line(layout, target, line + height * 0.5));
    }
    true
}

/// A field's text as it is edited: the selection tinted, the caret where the layout puts it; a
/// click places the caret, a double click takes a word, a drag selects. `edit` is the field's edit
/// when it is `text`'s; else the caret stands at the end. `scroll` keeps a one-line field's
/// sideways offset between frames.
pub fn text_body(
    text: &str,
    edit: &TextEdit,
    look: Look,
    scroll: Rc<Cell<f32>>,
    place: Option<Place>,
) -> AnyElement {
    use gpui::{Bounds, HighlightStyle, MouseButton, StyledText, canvas, fill, point, size};
    let (caret, selection, downstream) = if edit.text() == text {
        (edit.caret(), edit.selection(), edit.downstream())
    } else {
        (text.len(), None, false)
    };
    let dot = '•'.len_utf8();
    let to_shown = |at: usize| {
        if look.mask {
            text[..at].chars().count() * dot
        } else {
            at
        }
    };
    let owned = text.to_owned();
    let from_shown = move |at: usize| {
        if look.mask {
            owned
                .char_indices()
                .nth(at / dot)
                .map_or(owned.len(), |(i, _)| i)
        } else {
            at.min(owned.len())
        }
    };
    let shown = if look.mask {
        "•".repeat(text.chars().count())
    } else {
        text.to_owned()
    };
    let end = shown.len();
    // A trailing space gives the caret a place on a last, empty line.
    let styled = StyledText::new(format!("{shown} ")).with_highlights(selection.map(|sel| {
        (
            to_shown(sel.start)..to_shown(sel.end),
            HighlightStyle {
                background_color: Some(theme::selection().into()),
                ..Default::default()
            },
        )
    }));
    let caret = to_shown(caret);
    let layout = styled.layout().clone();
    if let Some(lines) = &look.lines {
        *lines.borrow_mut() = Some(layout.clone());
    }
    let (for_down, for_move) = (layout.clone(), layout.clone());
    let offset = if look.wrap { 0. } else { scroll.get() };
    let from_down = Rc::new(from_shown);
    let from_move = from_down.clone();
    let paint = canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let height = layout.line_height();
            let at = caret_point(&layout, caret, downstream).unwrap_or(bounds.origin);
            if !look.wrap {
                // Scroll so the caret stays in view, with no empty room after the text's end.
                let width = f32::from(bounds.size.width) - 2.;
                let x = f32::from(at.x - bounds.origin.x);
                let mut next = scroll.get();
                if x > width {
                    next += x - width;
                } else if x < 0. {
                    next += x;
                }
                if let Some(last) = layout.position_for_index(end) {
                    let room = width - f32::from(last.x - bounds.origin.x) - (next - scroll.get());
                    if room > 0. {
                        next -= room;
                    }
                }
                let next = next.max(0.);
                if (next - scroll.get()).abs() > 0.5 {
                    scroll.set(next);
                    // The text was laid out with the old offset: draw once more.
                    window.request_animation_frame();
                }
            }
            window.paint_quad(fill(
                Bounds::new(point(at.x, at.y), size(gpui::px(2.), height)),
                theme::accent(),
            ));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let (down, drag) = (place.clone(), place);
    div()
        .relative()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .cursor_text()
        .when(!look.wrap, |s| s.whitespace_nowrap())
        .child(
            div()
                .when(!look.wrap, |s| s.flex_none().ml(gpui::px(-offset)))
                .child(styled),
        )
        .child(paint)
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            if let Some(place) = &down {
                let at = nearest_index(&for_down, event.position);
                cx.stop_propagation();
                let lower = lower_line(&for_down, at, event.position.y);
                place(
                    from_down(at),
                    event.modifiers.shift,
                    event.click_count,
                    false,
                    lower,
                    cx,
                );
            }
        })
        .on_mouse_move(move |event, _, cx| {
            if let Some(place) = &drag
                && event.pressed_button == Some(MouseButton::Left)
            {
                let at = nearest_index(&for_move, event.position);
                place(from_move(at), true, 1, true, false, cx);
            }
        })
        .into_any_element()
}

/// Where a click in the field showing `text` goes: the caret to it, ⇧ or a drag selecting, a
/// double click taking the word.
fn place_field(text: &str, this: WeakEntity<Workspace>) -> Place {
    let text = text.to_owned();
    Rc::new(move |at, shift, clicks, drag, lower, cx| {
        this.update(cx, |this, cx| {
            sync(&text, &mut this.field_edit);
            this.field_goal = None;
            this.field_edit.click(at, clicks, shift || drag);
            if clicks == 1 && !shift && !drag {
                this.field_edit.set_downstream(lower);
            }
            cx.notify();
        })
        .ok();
    })
}

/// A caret standing alone: in an empty field.
fn caret() -> Div {
    div()
        .flex_none()
        .w(px(2.))
        .h(px(15.))
        .rounded_full()
        .bg(theme::accent())
}

impl Workspace {
    /// The inside of a one-line field: its text with the caret and selection while it has the
    /// keyboard (`active`), the text alone while not, or the placeholder.
    pub(super) fn field_text(
        &self,
        text: &str,
        active: bool,
        placeholder: &'static str,
        cx: &Context<Self>,
    ) -> Div {
        self.field_look(
            text,
            active,
            placeholder,
            Look::default(),
            cx.entity().downgrade(),
        )
    }

    /// `field_text` drawn as `look` says (wrapping, or masked), for a view without its context.
    pub(super) fn field_look(
        &self,
        text: &str,
        active: bool,
        placeholder: &'static str,
        look: Look,
        this: WeakEntity<Self>,
    ) -> Div {
        let row = div().flex_1().min_w_0().flex().items_center();
        if text.is_empty() {
            return row
                .children(active.then(caret))
                .child(div().text_color(theme::faint()).child(placeholder));
        }
        if !active {
            let shown = if look.mask {
                "•".repeat(text.chars().count())
            } else {
                text.to_owned()
            };
            return row.child(
                div()
                    .min_w_0()
                    .when(!look.wrap, |s| s.truncate())
                    .child(shown),
            );
        }
        let look = Look {
            lines: look.wrap.then(|| self.field_lines.clone()),
            ..look
        };
        row.child(text_body(
            text,
            &self.field_edit,
            look,
            self.field_scroll.clone(),
            Some(place_field(text, this)),
        ))
    }

    /// A field takes the keyboard: its caret starts at the end of its text.
    pub(super) fn reset_field_edit(&mut self) {
        self.field_edit = TextEdit::default();
        self.field_scroll.set(0.);
    }
}

/// Applies `key` to a multi-line `edit`: the caret and selection move (⌥ by words, ⌘ to the line's
/// or the text's ends, ⇧ selects), ⌥⌫ ⌘⌫ delete words and lines, ⌘A ⌘C ⌘X ⌘V ⌘Z ⇧⌘Z, and typing.
/// Return and esc are left to the caller.
pub fn edit_text(edit: &mut TextEdit, key: &Keystroke, cx: &mut App) -> Edit {
    edit_with(edit, key, cx, true)
}

/// `edit_text`; a one-line field (`lines` false) takes a pasted line break as a space.
fn edit_with(edit: &mut TextEdit, key: &Keystroke, cx: &mut App, lines: bool) -> Edit {
    use crate::textedit::Move;
    let m = key.modifiers;
    let movement = match (key.key.as_str(), m.platform, m.alt) {
        ("left", true, _) | ("home", ..) => Some(Move::LineStart),
        ("right", true, _) | ("end", ..) => Some(Move::LineEnd),
        ("up", true, _) => Some(Move::Start),
        ("down", true, _) => Some(Move::End),
        ("left", _, true) => Some(Move::WordLeft),
        ("right", _, true) => Some(Move::WordRight),
        ("left", ..) => Some(Move::Left),
        ("right", ..) => Some(Move::Right),
        ("up", ..) => Some(Move::Up),
        ("down", ..) => Some(Move::Down),
        _ => None,
    };
    if let Some(motion) = movement {
        edit.move_to(motion, m.shift);
        return Edit::Selected;
    }
    match key.key.as_str() {
        "escape" => return Edit::Escape,
        "enter" => return Edit::Enter { shift: m.shift },
        "backspace" => {
            edit.delete_back(m.alt, m.platform);
            return Edit::Changed;
        }
        "delete" => {
            edit.delete_forward(m.alt);
            return Edit::Changed;
        }
        _ => {}
    }
    if m.platform {
        return match key.key.as_str() {
            "a" => {
                edit.select_all();
                Edit::Selected
            }
            "c" => {
                if let Some(text) = edit.selected_text() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_owned()));
                }
                Edit::Ignored
            }
            "x" => match edit.cut() {
                Some(text) => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    Edit::Changed
                }
                None => Edit::Ignored,
            },
            "v" => match cx.read_from_clipboard().and_then(|c| c.text()) {
                Some(pasted) => {
                    edit.insert(&if lines {
                        pasted
                    } else {
                        pasted.replace('\n', " ")
                    });
                    Edit::Changed
                }
                None => Edit::Ignored,
            },
            "z" if m.shift => {
                edit.redo();
                Edit::Changed
            }
            "z" => {
                edit.undo();
                Edit::Changed
            }
            _ => Edit::Ignored,
        };
    }
    match &key.key_char {
        Some(ch) if !m.control && !ch.chars().any(char::is_control) => {
            edit.insert(ch);
            Edit::Changed
        }
        _ => Edit::Ignored,
    }
}
