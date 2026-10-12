//! Keys on controls, drawn as keycaps: one small bordered cap per key after a button's label, so a
//! key never reads as part of the label ("Cancel esc" did). Decided in
//! `../Design/mockups/line-comments/key-hints.html` (option 1); Esc, which the whole app shares, is
//! told in tooltips instead.

use super::px;
use super::theme;
use gpui::{Div, FontWeight, Rgba, div, prelude::*};

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
        .children(split(spec).into_iter().map(move |key| {
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
        }))
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn modifiers_are_caps_of_their_own() {
        assert_eq!(split("⇧⌘↵"), ["⇧", "⌘", "↵"]);
        assert_eq!(split("⌥M"), ["⌥", "M"]);
        assert_eq!(split("r"), ["R"]);
        assert_eq!(split("Tab"), ["Tab"]);
        assert_eq!(split("⌘F3"), ["⌘", "F3"]);
    }
}
