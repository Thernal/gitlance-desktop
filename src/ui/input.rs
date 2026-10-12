//! Editing the text of a one-line field from key events: the app has no text widget, so each field
//! (commit search, find in the diff, path filter, branch filter, a comment) is a `String` this edits,
//! with one "everything selected" flag (⌘A) shared by whichever field has the keyboard.

use super::px;
use super::theme;
use gpui::{App, Div, Keystroke, div, prelude::*};

/// What a key press did to a field.
pub enum Edit {
    /// The text changed.
    Changed,
    /// The text was selected (⌘A); nothing changed.
    Selected,
    /// Return was pressed; `shift` for ⇧Return.
    Enter {
        shift: bool,
    },
    Escape,
    /// Not a key a field takes.
    Ignored,
}

/// Applies `key` to `text`. `all` is whether the whole text is selected: typing, pasting or
/// deleting then replaces it, ⌘C and ⌘X take it.
pub fn edit(text: &mut String, all: &mut bool, key: &Keystroke, cx: &mut App) -> Edit {
    if key.key == "escape" {
        return Edit::Escape;
    }
    if key.key == "enter" {
        return Edit::Enter {
            shift: key.modifiers.shift,
        };
    }
    if key.modifiers.platform {
        return match key.key.as_str() {
            "a" => {
                *all = !text.is_empty();
                Edit::Selected
            }
            "c" | "x" => {
                if *all || key.key == "x" {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                }
                if key.key == "x" && *all {
                    text.clear();
                    *all = false;
                    Edit::Changed
                } else {
                    Edit::Ignored
                }
            }
            "v" => {
                if let Some(pasted) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    if std::mem::take(all) {
                        text.clear();
                    }
                    text.push_str(&pasted.replace('\n', " "));
                }
                Edit::Changed
            }
            "backspace" => {
                text.clear();
                *all = false;
                Edit::Changed
            }
            _ => Edit::Ignored,
        };
    }
    match key.key.as_str() {
        "backspace" => {
            if std::mem::take(all) {
                text.clear();
            } else {
                text.pop();
            }
            Edit::Changed
        }
        _ => match &key.key_char {
            Some(ch) if !key.modifiers.control && !ch.chars().any(char::is_control) => {
                if std::mem::take(all) {
                    text.clear();
                }
                text.push_str(ch);
                Edit::Changed
            }
            _ => Edit::Ignored,
        },
    }
}

/// The inside of a field: its text (tinted when all selected) and a caret while it has the
/// keyboard, or the placeholder.
pub fn field_text(text: &str, active: bool, all: bool, placeholder: &'static str) -> Div {
    let caret = || {
        div()
            .flex_none()
            .w(px(2.))
            .h(px(15.))
            .rounded_full()
            .bg(theme::accent())
    };
    let row = div().flex_1().min_w_0().flex().items_center();
    if text.is_empty() {
        return row
            .children(active.then(caret))
            .child(div().text_color(theme::faint()).child(placeholder));
    }
    row.child(
        div()
            .min_w_0()
            .truncate()
            .when(all && active, |s| s.bg(theme::selection()).rounded(px(3.)))
            .child(text.to_owned()),
    )
    .children((active && !all).then(caret))
}

/// Applies `key` to a multi-line `edit`: the caret and selection move (⌥ by words, ⌘ to the line's
/// or the text's ends, ⇧ selects), ⌥⌫ ⌘⌫ delete words and lines, ⌘A ⌘C ⌘X ⌘V ⌘Z ⇧⌘Z, and typing.
/// Return and esc are left to the caller.
pub fn edit_text(edit: &mut crate::textedit::TextEdit, key: &Keystroke, cx: &mut App) -> Edit {
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
                    edit.insert(&pasted);
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
