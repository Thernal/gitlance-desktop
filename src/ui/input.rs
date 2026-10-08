//! Editing the text of a one-line field from key events: the app has no text widget, so each field
//! (commit search, find in the diff, path filter, branch filter) is a `String` this edits.

use gpui::{App, Keystroke};

/// What a key press did to a field.
pub enum Edit {
    /// The text changed.
    Changed,
    /// Return was pressed; `shift` for ⇧Return.
    Enter {
        shift: bool,
    },
    Escape,
    /// Not a key a field takes.
    Ignored,
}

pub fn edit(text: &mut String, key: &Keystroke, cx: &mut App) -> Edit {
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
            "v" => {
                if let Some(pasted) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    text.push_str(&pasted.replace('\n', " "));
                }
                Edit::Changed
            }
            "backspace" => {
                text.clear();
                Edit::Changed
            }
            _ => Edit::Ignored,
        };
    }
    match key.key.as_str() {
        "backspace" => {
            text.pop();
            Edit::Changed
        }
        _ => match &key.key_char {
            Some(ch) if !key.modifiers.control && !ch.chars().any(char::is_control) => {
                text.push_str(ch);
                Edit::Changed
            }
            _ => Edit::Ignored,
        },
    }
}
