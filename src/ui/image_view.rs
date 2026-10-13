//! A changed image shown as an image, as GitLab does: the two sides next to each other with their
//! sizes (2-up), the new one wiped over the old (Swipe), or laid over it (Onion skin). An SVG can
//! switch to its source, the text diff.

use super::diff_view::{chip, group};
use super::px;
use super::{Workspace, theme};
use crate::git::{ChangeKind, FileDiff, ImageKind, ImageSide};
use gpui::{
    Bounds, ClickEvent, Context, FontWeight, ImageFormat, MouseButton, ObjectFit, Pixels, div, img,
    prelude::*,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// The box an image is drawn in at most, before the interface zoom.
const MAX_W: f32 = 720.;
const MAX_H: f32 = 520.;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    TwoUp,
    Swipe,
    Onion,
}

impl Mode {
    const ALL: [Mode; 3] = [Mode::TwoUp, Mode::Swipe, Mode::Onion];

    fn label(self) -> &'static str {
        match self {
            Mode::TwoUp => "2-up",
            Mode::Swipe => "Swipe",
            Mode::Onion => "Onion skin",
        }
    }
}

/// How images are being looked at, and the decoded images kept between frames.
pub struct Images {
    pub mode: Mode,
    /// An SVG shows its text diff.
    pub source: bool,
    /// Where the swipe is, or how opaque the new image is: 0 … 1.
    pub mix: f32,
    /// Where the swipe or onion box was drawn, for the mouse.
    area: Rc<Cell<Option<Bounds<Pixels>>>>,
    cache: RefCell<HashMap<usize, Arc<gpui::Image>>>,
}

impl Default for Images {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            source: false,
            mix: 0.5,
            area: Rc::default(),
            cache: RefCell::default(),
        }
    }
}

impl Images {
    /// The image of one side, decoded once per set of bytes.
    fn image(&self, side: &ImageSide) -> Arc<gpui::Image> {
        let key = Arc::as_ptr(&side.bytes) as *const u8 as usize;
        let mut cache = self.cache.borrow_mut();
        if cache.len() > 24 {
            cache.clear();
        }
        cache
            .entry(key)
            .or_insert_with(|| {
                Arc::new(gpui::Image::from_bytes(
                    format(side.kind),
                    side.bytes.to_vec(),
                ))
            })
            .clone()
    }
}

fn format(kind: ImageKind) -> ImageFormat {
    match kind {
        ImageKind::Png => ImageFormat::Png,
        ImageKind::Jpeg => ImageFormat::Jpeg,
        ImageKind::Gif => ImageFormat::Gif,
        ImageKind::Webp => ImageFormat::Webp,
        ImageKind::Bmp => ImageFormat::Bmp,
        ImageKind::Ico => ImageFormat::Ico,
        ImageKind::Tiff => ImageFormat::Tiff,
        ImageKind::Svg => ImageFormat::Svg,
    }
}

/// `12.3 KB`.
pub fn bytes(n: usize) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.1} KB", n as f64 / 1024.),
        n => format!("{:.1} MB", n as f64 / (1024. * 1024.)),
    }
}

/// `PNG · 640 × 480 · 12.3 KB`.
fn describe(side: &ImageSide) -> String {
    let size = side
        .size
        .map(|(w, h)| format!(" · {w} × {h}"))
        .unwrap_or_default();
    format!("{}{size} · {}", side.kind.label(), bytes(side.bytes.len()))
}

/// What changed between the sides: `640 × 480 → 800 × 600 · +1.8 KB`.
pub fn change(old: &ImageSide, new: &ImageSide) -> String {
    let size = match (old.size, new.size) {
        (Some(a), Some(b)) if a != b => format!("{} × {} → {} × {}", a.0, a.1, b.0, b.1),
        (Some(_), Some(_)) => "same size".to_owned(),
        _ => String::new(),
    };
    let delta = new.bytes.len() as i64 - old.bytes.len() as i64;
    let weight = match delta {
        0 => "same weight".to_owned(),
        d if d > 0 => format!("+{}", bytes(d as usize)),
        d => format!("−{}", bytes((-d) as usize)),
    };
    [size, weight]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The size to draw an image of `size` at: as it is, or scaled down into the box.
fn fit(size: Option<(u32, u32)>, max_w: f32, max_h: f32) -> (f32, f32) {
    let (w, h) = size.map_or((max_w, max_h), |(w, h)| (w.max(1) as f32, h.max(1) as f32));
    let scale = (max_w / w).min(max_h / h).min(1.);
    (w * scale, h * scale)
}

impl Workspace {
    /// Whether the open file is drawn as images (not as its text diff).
    pub(super) fn shows_image(&self, file: &FileDiff) -> bool {
        (file.old_image.is_some() || file.new_image.is_some())
            && !(self.images.source && is_svg(file))
    }

    pub(super) fn render_image(&self, file: &FileDiff, cx: &mut Context<Self>) -> gpui::AnyElement {
        let both = file.old_image.is_some() && file.new_image.is_some();
        let mode = if both { self.images.mode } else { Mode::TwoUp };
        let modes = group().children(Mode::ALL.map(|m| {
            chip(m.label(), m.label(), mode == m)
                .when(!both && m != Mode::TwoUp, |s| s.opacity(0.4))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.images.mode = m;
                    cx.notify();
                }))
        }));
        let summary = match (&file.old_image, &file.new_image) {
            (Some(old), Some(new)) => change(old, new),
            (None, Some(new)) => format!("Added · {}", describe(new)),
            (Some(old), None) => format!("Deleted · {}", describe(old)),
            (None, None) => String::new(),
        };
        let toolbar = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .py(px(8.))
            .border_b_1()
            .border_color(theme::island_border())
            .child(modes)
            .children(is_svg(file).then(|| source_switch(false, cx)))
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(summary),
            );
        let body = match (mode, &file.old_image, &file.new_image) {
            (Mode::Swipe | Mode::Onion, Some(old), Some(new)) => {
                self.render_overlay(mode, old, new, cx).into_any_element()
            }
            _ => self.render_two_up(file).into_any_element(),
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div()
                    .id("image-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .flex()
                    .justify_center()
                    .child(body),
            )
            .into_any_element()
    }

    /// The sides next to each other, each with its kind, size and weight.
    fn render_two_up(&self, file: &FileDiff) -> impl IntoElement + use<> {
        let half = MAX_W / 2. + 40.;
        let side = |image: Option<&ImageSide>, before: bool| {
            let tone = if before {
                theme::removed()
            } else {
                theme::added()
            };
            let label = match (before, file.change) {
                (true, _) => "Before",
                (false, _) => "After",
            };
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .w(px(half))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tone)
                        .child(label),
                )
                .child(match image {
                    Some(s) => {
                        let (w, h) = fit(s.size, half - 16., MAX_H);
                        frame(tone)
                            .child(
                                img(self.images.image(s))
                                    .w(px(w))
                                    .h(px(h))
                                    .object_fit(ObjectFit::Contain),
                            )
                            .into_any_element()
                    }
                    None => div()
                        .w(px(half - 16.))
                        .h(px(120.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .border_1()
                        .border_dashed()
                        .border_color(theme::island_border())
                        .text_size(px(12.))
                        .text_color(theme::faint())
                        .child(if before {
                            "Not there before"
                        } else {
                            "Deleted"
                        })
                        .into_any_element(),
                })
                .children(image.map(|s| {
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(describe(s))
                }))
        };
        let added = file.change == ChangeKind::Added;
        let deleted = file.change == ChangeKind::Deleted;
        div()
            .flex()
            .items_start()
            .gap_4()
            .when(!added, |s| s.child(side(file.old_image.as_ref(), true)))
            .when(!deleted, |s| s.child(side(file.new_image.as_ref(), false)))
    }

    /// The new side over the old: wiped in from the left up to the handle (Swipe), or faded in
    /// (Onion skin). A drag in the box or on the bar moves it.
    fn render_overlay(
        &self,
        mode: Mode,
        old: &ImageSide,
        new: &ImageSide,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // One box for both, sized by the larger side.
        let size = match (old.size, new.size) {
            (Some(a), Some(b)) => Some((a.0.max(b.0), a.1.max(b.1))),
            (a, b) => a.or(b),
        };
        let (w, h) = fit(size, MAX_W, MAX_H);
        let mix = self.images.mix;
        let picture = |side: &ImageSide| {
            img(self.images.image(side))
                .absolute()
                .top_0()
                .left_0()
                .w(px(w))
                .h(px(h))
                .object_fit(ObjectFit::Contain)
        };
        let area = self.images.area.clone();
        let (down, drag) = (cx.entity().downgrade(), cx.entity().downgrade());
        let set = |this: &mut Workspace, x: Pixels, cx: &mut Context<Workspace>| {
            if let Some(b) = this.images.area.get() {
                let at = f32::from(x - b.origin.x) / f32::from(b.size.width).max(1.);
                this.images.mix = at.clamp(0., 1.);
                cx.notify();
            }
        };
        let stage = div()
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .rounded(px(4.))
            .overflow_hidden()
            .bg(theme::panel())
            .border_1()
            .border_color(theme::island_border())
            .child(picture(old))
            .child(match mode {
                Mode::Swipe => div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .h(px(h))
                    .w(px(w * mix))
                    .overflow_hidden()
                    .child(picture(new))
                    .into_any_element(),
                _ => picture(new).opacity(mix).into_any_element(),
            })
            .when(mode == Mode::Swipe, |s| {
                s.child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(w * mix - 1.))
                        .w(px(2.))
                        .h(px(h))
                        .bg(theme::accent()),
                )
            })
            .child(
                gpui::canvas(move |bounds, _, _| area.set(Some(bounds)), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
            .cursor_col_resize()
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                down.update(cx, |this, cx| set(this, event.position.x, cx))
                    .ok();
            })
            .on_mouse_move(move |event, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    drag.update(cx, |this, cx| set(this, event.position.x, cx))
                        .ok();
                }
            });
        let (left, right) = match mode {
            Mode::Swipe => ("After ←", "→ Before"),
            _ => ("Before", "After"),
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .child(stage)
            .child(
                div()
                    .w(px(w.max(240.)))
                    .flex()
                    .justify_between()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(left)
                    .child(format!("{}%", (mix * 100.).round()))
                    .child(right),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::faint())
                    .child("Drag across the image to move it"),
            )
    }
}

pub(super) fn is_svg(file: &FileDiff) -> bool {
    [&file.old_image, &file.new_image]
        .into_iter()
        .flatten()
        .any(|s| s.kind == ImageKind::Svg)
}

/// The border an image sits in, in its side's colour.
fn frame(tone: gpui::Rgba) -> gpui::Div {
    div()
        .p(px(8.))
        .rounded(px(6.))
        .border_1()
        .border_color(tone)
        .bg(theme::panel())
}

/// Image | Source, for an SVG.
pub(super) fn source_switch(source: bool, cx: &mut Context<Workspace>) -> impl IntoElement {
    group()
        .child(chip("svg-image", "Image", !source).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.images.source = false;
                cx.notify();
            },
        )))
        .child(chip("svg-source", "Source", source).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.images.source = true;
                cx.notify();
            },
        )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(size: (u32, u32), len: usize) -> ImageSide {
        ImageSide {
            kind: ImageKind::Png,
            bytes: vec![0; len].into(),
            size: Some(size),
        }
    }

    #[test]
    fn the_change_says_size_and_weight() {
        assert_eq!(
            change(&side((640, 480), 2048), &side((800, 600), 4096)),
            "640 × 480 → 800 × 600 · +2.0 KB"
        );
        assert_eq!(
            change(&side((16, 16), 300), &side((16, 16), 200)),
            "same size · −100 B"
        );
    }

    #[test]
    fn a_large_image_is_scaled_down_a_small_one_kept() {
        assert_eq!(fit(Some((1440, 520)), 720., 520.), (720., 260.));
        assert_eq!(fit(Some((32, 32)), 720., 520.), (32., 32.));
    }
}
