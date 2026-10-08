//! One Dark for the window chrome; code colors live in `crate::highlight::palette`.

use gpui::{Hsla, Rgba, rgb, rgba};

pub const UI_FONT: &str = ".SystemUIFont";
pub const CODE_FONT: &str = "Menlo";

/// The window behind the islands.
pub fn base() -> Rgba {
    rgb(0x17191e)
}

/// List islands; the diff island uses `editor`.
pub fn panel() -> Rgba {
    rgb(0x21252b)
}

pub fn island_border() -> Rgba {
    rgba(0xffffff0a)
}

pub fn editor() -> Rgba {
    rgb(crate::highlight::palette::BACKGROUND)
}

pub fn border() -> Rgba {
    rgb(0x181a1f)
}

pub fn hover() -> Rgba {
    rgb(0x2c313a)
}

pub fn selected() -> Rgba {
    rgb(0x3a3f4b)
}

pub fn text() -> Rgba {
    rgb(crate::highlight::palette::FOREGROUND)
}

pub fn muted() -> Rgba {
    rgb(0x7f848e)
}

pub fn faint() -> Rgba {
    rgb(0x5c6370)
}

pub fn accent() -> Rgba {
    rgb(crate::highlight::palette::BLUE)
}

pub fn added() -> Rgba {
    rgb(crate::highlight::palette::GREEN)
}

pub fn removed() -> Rgba {
    rgb(crate::highlight::palette::RED)
}

pub fn renamed() -> Rgba {
    rgb(crate::highlight::palette::YELLOW)
}

pub fn warning() -> Rgba {
    rgb(crate::highlight::palette::ORANGE)
}

/// A pane splitter under the pointer or being dragged.
pub fn splitter() -> Rgba {
    rgba(0x61afef99)
}

pub fn added_line() -> Rgba {
    rgba(0x98c3791f)
}

pub fn removed_line() -> Rgba {
    rgba(0xe06c751f)
}

pub fn added_gutter() -> Rgba {
    rgba(0x98c37933)
}

pub fn removed_gutter() -> Rgba {
    rgba(0xe06c7533)
}

pub fn code(color: u32) -> Hsla {
    rgb(color).into()
}
