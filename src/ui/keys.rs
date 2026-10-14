//! Keys on controls, drawn as keycaps: one small bordered cap per key after a button's label, so a
//! key never reads as part of the label ("Cancel esc" did). Decided in
//! `../Design/mockups/line-comments/key-hints.html` (option 1); Esc, which the whole app shares, is
//! told in tooltips instead.

use super::px;
use super::theme;
use crate::platform;
use gpui::{Div, FontWeight, Rgba, div, prelude::*};
use std::borrow::Cow;

/// The app writes shortcuts the macOS way (`⇧⌘↵`). On Windows and Linux they are shown in that
/// OS's words (`Ctrl+Shift+Enter`), as `platform::pc_keys` binds them: ⌘ is Ctrl, ⌘1…⌘9 are Alt.
fn is_modifier(c: char) -> bool {
    matches!(c, '⌘' | '⇧' | '⌥' | '⌃')
}

/// A piece of text: plain, or one shortcut's caps in Windows and Linux words.
enum Piece {
    Plain(char),
    Keys(Vec<String>),
}

/// `text` cut into plain characters and shortcuts — a run of modifiers and the key after it
/// (one character, or F1…F12). "⌘-click" has no key: the modifier alone.
fn pieces(text: &str) -> Vec<Piece> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_modifier(chars[i]) {
            out.push(Piece::Plain(chars[i]));
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && is_modifier(chars[i]) {
            i += 1;
        }
        let mods = &chars[start..i];
        let key: String = match chars.get(i) {
            Some('F') if chars.get(i + 1).is_some_and(char::is_ascii_digit) => {
                let from = i;
                i += 1;
                while chars.get(i).is_some_and(char::is_ascii_digit) {
                    i += 1;
                }
                chars[from..i].iter().collect()
            }
            Some('-') if chars.get(i + 1).is_some_and(|c| c.is_alphabetic()) => String::new(),
            Some(&c) if !c.is_whitespace() => {
                i += 1;
                c.to_string()
            }
            _ => String::new(),
        };
        out.push(Piece::Keys(pc_caps(mods, &key)));
    }
    out
}

/// The caps of one shortcut on Windows and Linux: Ctrl, Alt, Shift, then the key.
fn pc_caps(mods: &[char], key: &str) -> Vec<String> {
    let tab = mods == ['⌘'] && matches!(key.as_bytes(), [b'1'..=b'9']);
    let mut caps = Vec::new();
    if !tab && mods.iter().any(|&c| c == '⌘' || c == '⌃') {
        caps.push("Ctrl".to_owned());
    }
    if tab || mods.contains(&'⌥') {
        caps.push("Alt".to_owned());
    }
    if mods.contains(&'⇧') {
        caps.push("Shift".to_owned());
    }
    if !key.is_empty() {
        caps.push(match key {
            "↵" => "Enter".to_owned(),
            "⌫" => "Backspace".to_owned(),
            "⌦" => "Delete".to_owned(),
            "⇥" => "Tab".to_owned(),
            "−" => "-".to_owned(),
            k => k.to_uppercase(),
        });
    }
    caps
}

/// `text` with every shortcut in it in Windows and Linux words.
fn pc_text(text: &str) -> String {
    pieces(text)
        .into_iter()
        .map(|piece| match piece {
            Piece::Plain(c) => c.to_string(),
            Piece::Keys(caps) => caps.join("+"),
        })
        .collect()
}

/// `text` with every shortcut in it in this OS's words: as written on macOS, `Ctrl+Shift+Enter`
/// for `⇧⌘↵` on Windows and Linux. Every shortcut shown to the user goes through this.
/// It also says the host's words (`words::say`): GitHub's pull requests.
pub fn text(text: &str) -> Cow<'_, str> {
    let said = super::words::say(text);
    if platform::MAC || !said.chars().any(is_modifier) {
        said
    } else {
        Cow::Owned(pc_text(&said))
    }
}

/// The caps of a shortcut on Windows and Linux: each modifier and the key their own caps, plain
/// words (`Tab`, `esc`) one cap each.
fn pc_split(spec: &str) -> Vec<String> {
    let mut caps = Vec::new();
    let mut word = String::new();
    for piece in pieces(spec) {
        match piece {
            Piece::Plain(c) if c.is_whitespace() => {
                if !word.is_empty() {
                    caps.extend(split(&std::mem::take(&mut word)));
                }
            }
            Piece::Plain(c) => word.push(c),
            Piece::Keys(keys) => {
                if !word.is_empty() {
                    caps.extend(split(&std::mem::take(&mut word)));
                }
                caps.extend(keys);
            }
        }
    }
    if !word.is_empty() {
        caps.extend(split(&word));
    }
    caps
}

/// The caps of a shortcut as written in the app (`⇧⌘↵`, `⌥M`, `r`, `F3`, `Tab`): each modifier its
/// own cap, the rest one cap, a single letter upper-cased.
pub fn split(spec: &str) -> Vec<String> {
    let mut caps = Vec::new();
    let mut rest = String::new();
    for c in spec.chars() {
        if matches!(c, '⌘' | '⇧' | '⌥' | '⌃') && rest.is_empty() {
            caps.push(c.to_string());
        } else if !c.is_whitespace() {
            rest.push(c);
        }
    }
    if !rest.is_empty() {
        caps.push(if rest.chars().count() == 1 {
            rest.to_uppercase()
        } else {
            rest
        });
    }
    caps
}

/// How a cap sits on what is under it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum On {
    /// A plain surface: a field, a quiet button.
    Plain,
    /// A filled primary button: the caps take the button's ink.
    Filled,
}

/// The keycaps of `spec`, for the end of a button or a field.
pub fn caps(spec: &str, on: On) -> Div {
    let (ink, border, fill): (Rgba, Rgba, Rgba) = match on {
        On::Plain => (theme::text(), theme::selected(), theme::panel()),
        On::Filled => (
            theme::base(),
            gpui::rgba(0x17191e4d),
            gpui::rgba(0x17191e2e),
        ),
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.))
        .children(
            if platform::MAC {
                split(spec)
            } else {
                pc_split(spec)
            }
            .into_iter()
            .map(move |key| {
                div()
                    .min_w(px(18.))
                    .h(px(18.))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_b_2()
                    .border_color(border)
                    .bg(fill)
                    .text_size(px(11.))
                    .line_height(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(ink)
                    .child(key)
            }),
        )
}

#[cfg(test)]
mod tests {
    use super::{pc_split, pc_text, split};

    #[test]
    fn modifiers_are_caps_of_their_own() {
        assert_eq!(split("⇧⌘↵"), ["⇧", "⌘", "↵"]);
        assert_eq!(split("⌥M"), ["⌥", "M"]);
        assert_eq!(split("r"), ["R"]);
        assert_eq!(split("Tab"), ["Tab"]);
        assert_eq!(split("⌘F3"), ["⌘", "F3"]);
    }

    #[test]
    fn windows_and_linux_spell_shortcuts_out() {
        assert_eq!(pc_text("Post  ⇧⌘↵"), "Post  Ctrl+Shift+Enter");
        assert_eq!(pc_text("⌥⌘1 and ⌘1…9"), "Ctrl+Alt+1 and Alt+1…9");
        assert_eq!(pc_text("⌃2, ⌘F3, ⌥↓"), "Ctrl+2, Ctrl+F3, Alt+↓");
        assert_eq!(
            pc_text("⌘+ and ⌘− zoom, ⌘0"),
            "Ctrl++ and Ctrl+- zoom, Ctrl+0"
        );
        assert_eq!(pc_text("⌘-click opens it"), "Ctrl-click opens it");
        assert_eq!(pc_text("⌘[ ⌘]"), "Ctrl+[ Ctrl+]");
        assert_eq!(pc_text("no keys ↵"), "no keys ↵");
        assert_eq!(pc_split("⇧⌘↵"), ["Ctrl", "Shift", "Enter"]);
        assert_eq!(pc_split("⌘+"), ["Ctrl", "+"]);
        assert_eq!(pc_split("Tab"), ["Tab"]);
    }
}
