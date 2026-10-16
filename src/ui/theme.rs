//! One Dark for the window chrome; code colors live in `crate::highlight::palette`.
//!
//! Each colour names the token it mirrors in `../Design/tokens.css`, the map themes will be read through.

use gpui::{Hsla, Rgba, rgb, rgba};

pub const UI_FONT: &str = ".SystemUIFont";
pub const CODE_FONT: &str = "Menlo";

/// The window behind the islands. `--background-01`
pub fn base() -> Rgba {
    rgb(0x17191e)
}

/// List islands; the diff island uses `editor`. `--background-02`
pub fn panel() -> Rgba {
    rgb(0x21252b)
}

/// `--border-01`
pub fn island_border() -> Rgba {
    rgba(0xffffff0a)
}

/// `--background-03`
pub fn editor() -> Rgba {
    rgb(crate::highlight::palette::BACKGROUND)
}

/// `--border-02`
pub fn border() -> Rgba {
    rgb(0x181a1f)
}

/// `--background-04`
pub fn hover() -> Rgba {
    rgb(0x2c313a)
}

/// `--interactive-03-hover`
pub fn selected() -> Rgba {
    rgb(0x3a3f4b)
}

/// `--content-01`
pub fn text() -> Rgba {
    rgb(crate::highlight::palette::FOREGROUND)
}

/// `--content-02`
pub fn muted() -> Rgba {
    rgb(0x9da3ae)
}

/// `--content-03`, 4.5:1 on every surface.
pub fn faint() -> Rgba {
    rgb(0x979ea9)
}

/// `--interactive-01`
pub fn accent() -> Rgba {
    rgb(crate::highlight::palette::BLUE)
}

/// `--success`
pub fn added() -> Rgba {
    rgb(crate::highlight::palette::GREEN)
}

/// Red ink — deletions, errors, Request changes. `--danger`: One Dark's red raised to 4.5:1; the
/// diff's red tints stay One Dark's (`removed_line`, `removed_word`, `removed_gutter`).
pub fn removed() -> Rgba {
    rgb(0xec7f87)
}

/// `--brand-yellow-500`
pub fn renamed() -> Rgba {
    rgb(crate::highlight::palette::YELLOW)
}

/// `--attention`
pub fn warning() -> Rgba {
    rgb(crate::highlight::palette::ORANGE)
}

/// The tint behind a search match. `--attention-bg`
pub fn warning_bg() -> Rgba {
    rgba(0xd19a6633)
}

/// The focus ring of a text field. `--focus-ring`
pub fn focus() -> Rgba {
    rgb(crate::highlight::palette::PURPLE)
}

/// Comments for a coding agent, apart from the focus ring and the code's keyword purple. `--agent`
pub fn agent() -> Rgba {
    rgb(0xd18ee6)
}

/// The tint behind an agent note's pill.
pub fn agent_bg() -> Rgba {
    rgba(0xd18ee62e)
}

/// The tint behind a "Resolved" chip. `--success-bg`
pub fn added_bg() -> Rgba {
    rgba(0x98c37924)
}

/// The tint behind the new-commits pill. `--info-bg`
pub fn info_bg() -> Rgba {
    rgba(0x61afef24)
}

/// The tint behind selected lines of a diff.
pub fn selection() -> Rgba {
    rgba(0x61afef38)
}

/// A pane splitter under the pointer or being dragged.
pub fn splitter() -> Rgba {
    rgba(0x61afef99)
}

/// `--diff-added-01`
pub fn added_line() -> Rgba {
    rgba(0x98c3791f)
}

/// `--diff-removed-01`
pub fn removed_line() -> Rgba {
    rgba(0xe06c751f)
}

pub fn scrollbar() -> Rgba {
    rgba(0xabb2bf40)
}

/// A changed token inside an added line. `--diff-added-03`
pub fn added_word() -> Rgba {
    rgba(0x98c3792e)
}

/// A changed token inside a removed line. `--diff-removed-03`
pub fn removed_word() -> Rgba {
    rgba(0xe06c752e)
}

/// `--diff-added-02`
pub fn added_gutter() -> Rgba {
    rgba(0x98c37933)
}

/// `--diff-removed-02`
pub fn removed_gutter() -> Rgba {
    rgba(0xe06c7533)
}

/// A code colour of `crate::highlight::palette`.
pub fn code(color: u32) -> Hsla {
    rgb(color).into()
}
