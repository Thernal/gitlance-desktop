//! Settings → Appearance → Fonts and size (#24 A): the interface's face and the code's, each
//! picked from a menu that draws every face in itself — a few installed ones, and Other… for all
//! of them in ⌘K's window —, the code's size in points, the interface size, and a line saying
//! where the narrow layout folds at that size (`narrow.rs`: the steps grow with it).

use super::diff_view::{self, chip};
use super::menu::{Act, Entry};
use super::palette::Kind;
use super::settings::item;
use super::{ROW_RADIUS, Workspace, icons, narrow, px, theme};
use gpui::{AnyElement, ClickEvent, Context, Pixels, Point, div, prelude::*};

/// Faces a face's menu offers when they are installed; Other… lists every one.
const UI_FACES: [&str; 5] = [
    "Helvetica Neue",
    "Avenir Next",
    "Inter",
    "Georgia",
    "Verdana",
];
const CODE_FACES: [&str; 5] = [
    "SF Mono",
    "Monaco",
    "JetBrains Mono",
    "Fira Code",
    "Courier New",
];

/// The code's size in points at code size `zoom` (%), as the diff draws it.
pub(super) fn code_points(zoom: u16) -> u16 {
    (super::CODE_SIZE * f32::from(zoom) / 100.).round() as u16
}

/// The code size (%) that draws the code at `points`.
fn zoom_for(points: u16) -> u16 {
    (f32::from(points) * 100. / super::CODE_SIZE).round() as u16
}

/// Where the menu folds, the side panel becomes a drawer and a tool window floats, in window
/// pixels, at interface size `ui_zoom` (%).
fn fold_widths(ui_zoom: u16) -> [u32; 3] {
    [narrow::MENU, narrow::SIDE, narrow::FLOAT]
        .map(|w| (w * f32::from(ui_zoom) / 100.).round() as u32)
}

/// `1472` as `1 472`.
fn grouped(n: u32) -> String {
    match n {
        0..1000 => n.to_string(),
        _ => format!("{} {:03}", n / 1000, n % 1000),
    }
}

/// The line under the rows: where the layout folds at this size.
fn fold_line(ui_zoom: u16) -> String {
    let [menu, side, float] = fold_widths(ui_zoom).map(grouped);
    match ui_zoom {
        100 => format!(
            "At 100 % the menu folds under {menu} px, the side panel under {side}, a tool window floats under {float}; at a larger size they come sooner."
        ),
        _ => format!(
            "At {ui_zoom} % the narrow layout comes {}, as the text needs {} room: the menu folds under {menu} px (1 280 at 100 %), the side panel under {side}, a tool window floats under {float}.",
            if ui_zoom > 100 { "sooner" } else { "later" },
            if ui_zoom > 100 { "more" } else { "less" },
        ),
    }
}

/// A face's menu: its default (None), those of `common` installed, and the chosen one.
fn short_list(common: &[&str], installed: &[String], chosen: Option<&str>) -> Vec<Option<String>> {
    let mut faces = vec![None];
    faces.extend(
        common
            .iter()
            .filter(|f| installed.iter().any(|i| i == *f))
            .map(|f| Some((*f).to_owned())),
    );
    if let Some(chosen) = chosen
        && !faces.iter().flatten().any(|f| f == chosen)
    {
        faces.push(Some(chosen.to_owned()));
    }
    faces
}

/// Every face installed, by family name; the system's hidden ones (`.SF NS`) left out.
pub(super) fn installed(cx: &gpui::App) -> Vec<String> {
    cx.text_system()
        .all_font_names()
        .into_iter()
        .filter(|f| !f.starts_with('.'))
        .collect()
}

/// A face's name as a list says it: the interface's default is the system's.
pub(super) fn face_label(code: bool, face: Option<&str>) -> String {
    match (face, code) {
        (Some(face), _) => face.to_owned(),
        (None, false) => "System".to_owned(),
        (None, true) => theme::CODE_FONT.to_owned(),
    }
}

/// The family `face` is drawn in.
pub(super) fn face_family(code: bool, face: Option<&str>) -> gpui::SharedString {
    match (face, code) {
        (Some(face), _) => face.to_owned().into(),
        (None, false) => theme::UI_FONT.into(),
        (None, true) => theme::CODE_FONT.into(),
    }
}

impl Workspace {
    /// The section: Interface and Code with their faces, the code's size, the interface size, and
    /// where the layout folds.
    pub(super) fn render_fonts(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(super::appearance::section(
                "Fonts and size",
                "The interface and the code each have their own face. Every face installed can be chosen.",
            ))
            .child(div().flex().flex_col().children(self.font_rows(cx)))
            .child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(ROW_RADIUS))
                    .bg(theme::info_bg())
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(fold_line(self.settings.ui_zoom)),
            )
            .into_any_element()
    }

    /// Interface and its face, Code with its face and size, Interface size (here, and Set up's
    /// Look).
    pub(super) fn font_rows(&self, cx: &mut Context<Self>) -> [AnyElement; 3] {
        let faces = theme::faces();
        let s = self.settings;
        let ui_zoom =
            diff_view::group()
                .child(chip("ui-zoom-out", "−", false).on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| this.ui_zoom_by(-1, window, cx)),
                ))
                .child(
                    chip(
                        "ui-zoom-reset",
                        format!("{} %", s.ui_zoom),
                        s.ui_zoom != 100,
                    )
                    .on_click(cx.listener(
                        |this, _: &ClickEvent, window, cx| this.set_ui_zoom(100, window, cx),
                    )),
                )
                .child(chip("ui-zoom-in", "+", false).on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| this.ui_zoom_by(1, window, cx)),
                ));
        let points = code_points(s.zoom);
        let code_size = diff_view::group()
            .child(chip("code-size-down", "−", false).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.set_zoom(zoom_for(points.saturating_sub(1)), cx)
                },
            )))
            .child(
                chip("code-size-reset", format!("{points} pt"), s.zoom != 100)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.set_zoom(100, cx))),
            )
            .child(chip("code-size-up", "+", false).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| this.set_zoom(zoom_for(points + 1), cx),
            )));
        let code = div()
            .flex()
            .items_center()
            .gap_2()
            .child(self.face_picker(true, faces.code.as_deref(), cx))
            .child(code_size);
        [
            item(
                "Interface",
                "",
                self.face_picker(false, faces.ui.as_deref(), cx),
            )
            .into_any_element(),
            item("Code", "Grows with the interface size.", code).into_any_element(),
            item(
                "Interface size",
                "⌘+ and ⌘− change it from anywhere; ⌘0 back to 100 %",
                ui_zoom,
            )
            .into_any_element(),
        ]
    }

    /// The face in use, drawn in itself; a click opens the face's menu.
    fn face_picker(
        &self,
        code: bool,
        face: Option<&str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .id(if code { "face-code" } else { "face-ui" })
            .min_w(px(190.))
            .h(px(28.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(ROW_RADIUS))
            .bg(theme::hover())
            .cursor_pointer()
            .hover(|s| s.bg(theme::selected()))
            .child(
                div()
                    .flex_1()
                    .text_size(px(13.))
                    .font_family(face_family(code, face))
                    .child(face_label(code, face)),
            )
            .child(
                icons::icon("chevron")
                    .size(px(12.))
                    .text_color(theme::muted()),
            )
            .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                this.open_faces(code, ev.position(), cx)
            }))
    }

    /// A face's menu: the default, a few installed faces, each in itself; Other… for all.
    pub(super) fn open_faces(&mut self, code: bool, at: Point<Pixels>, cx: &mut Context<Self>) {
        let faces = theme::faces();
        let chosen = if code { faces.code } else { faces.ui };
        let common: &[&str] = if code { &CODE_FACES } else { &UI_FACES };
        let entries = short_list(common, &installed(cx), chosen.as_deref())
            .into_iter()
            .map(|face| {
                Entry::new(
                    face_label(code, face.as_deref()),
                    Act::Face {
                        code,
                        face: face.clone(),
                    },
                )
                .checked(face == chosen)
                .face(face_family(code, face.as_deref()))
            })
            .collect();
        self.open_titled(
            at,
            vec![
                (Some("Each in its own type"), entries),
                (
                    None,
                    vec![Entry::new("Other…", Act::OtherFaces(code)).key("any installed face")],
                ),
            ],
            cx,
        );
    }

    /// Every face installed, in ⌘K's window.
    pub(super) fn open_all_faces(&mut self, code: bool, cx: &mut Context<Self>) {
        self.open_palette(Kind::Face { code }, cx);
    }

    /// Draws the interface or the code in `face` (None: the default) and keeps it.
    pub(super) fn choose_face(&mut self, code: bool, face: Option<String>, cx: &mut Context<Self>) {
        let mut faces = theme::faces();
        match code {
            true => faces.code = face,
            false => faces.ui = face,
        }
        theme::choose_faces(&faces);
        // The code's width is measured again in the new face as the window draws.
        self.relayout_all();
        cx.refresh_windows();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Settings;

    #[test]
    fn the_codes_size_in_points_goes_back_and_forth_through_its_percent() {
        assert_eq!(code_points(100), 12);
        for points in code_points(Settings::ZOOM_MIN)..=code_points(Settings::ZOOM_MAX) {
            let zoom = zoom_for(points).clamp(Settings::ZOOM_MIN, Settings::ZOOM_MAX);
            assert_eq!(code_points(zoom), points, "{points} pt");
        }
    }

    #[test]
    fn the_layout_folds_sooner_at_a_larger_interface_size() {
        assert_eq!(fold_widths(100), [1280, 1100, 1000]);
        assert_eq!(fold_widths(115), [1472, 1265, 1150]);
        assert!(fold_line(115).contains("under 1 472 px (1 280 at 100 %)"));
        assert!(fold_line(115).contains("the side panel under 1 265"));
        assert!(fold_line(90).contains("comes later"));
    }

    #[test]
    fn a_faces_menu_has_its_default_the_installed_ones_and_the_chosen() {
        let installed = ["Georgia".to_owned(), "Optima".to_owned()];
        assert_eq!(
            short_list(&UI_FACES, &installed, None),
            [None, Some("Georgia".to_owned())]
        );
        assert_eq!(
            short_list(&UI_FACES, &installed, Some("Optima")),
            [None, Some("Georgia".to_owned()), Some("Optima".to_owned())]
        );
        assert_eq!(short_list(&UI_FACES, &installed, Some("Georgia")).len(), 2);
    }
}
