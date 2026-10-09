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
