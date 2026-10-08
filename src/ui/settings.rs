//! The Settings page: how changed words are marked, which mode diffs open in, and whether the
//! window refreshes itself. Designed in `../Design/mockups/settings/a-general.html`.

use super::diff_view::{self, chip};
use super::{GAP, ISLAND_RADIUS, Workspace, button, island, theme};
use crate::editor::{self, Editor};
use crate::storage::{Appearance, DiffMode, MarkStyle};
use gpui::{ClickEvent, Context, FontWeight, SharedString, div, prelude::*, px};

impl Workspace {
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.find.text = None;
        self.refilter();
        self.settings_open = true;
        cx.notify();
    }

    fn change_settings(&mut self, change: impl FnOnce(&mut Self), cx: &mut Context<Self>) {
        change(self);
        self.settings.save();
        cx.notify();
    }

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let s = self.settings;
        let marks = diff_view::group().children(MarkStyle::ALL.map(|style| {
            chip(style_id(style), style.label(), s.mark_style == style).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.change_settings(|this| this.settings.mark_style = style, cx)
                },
            ))
        }));
        let modes = diff_view::group().children(DiffMode::ALL.map(|mode| {
            chip(mode_id(mode), mode.label(), s.default_mode == mode).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.change_settings(|this| this.settings.default_mode = mode, cx);
                    // The page's choice is what the open diff shows too.
                    this.set_mode(mode, cx);
                },
            ))
        }));
        let installed: Vec<Editor> = Editor::ALL.into_iter().filter(|e| e.installed()).collect();
        let current = editor::pick(s.editor);
        let editors = diff_view::group().children(installed.iter().map(|&e| {
            chip(e.key(), e.label(), current == Some(e)).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.change_settings(|this| this.settings.editor = Some(e), cx)
                },
            ))
        }));
        let appearance = diff_view::group().children(Appearance::ALL.map(|a| {
            chip(a.label(), a.label(), s.appearance == a).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.change_settings(|this| this.settings.appearance = a, cx)
                },
            ))
        }));
        let refresh = switch("refresh", s.auto_refresh).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.change_settings(
                    |this| this.settings.auto_refresh = !this.settings.auto_refresh,
                    cx,
                )
            },
        ));

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .px(px(GAP))
            .pb(px(GAP))
            .child(
                island()
                    .id("settings")
                    .flex_1()
                    .bg(theme::editor())
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(760.))
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap_5()
                            .p(px(24.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(div().flex_1().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child("Settings"))
                                    .child(button("settings-done", "Done", "esc").on_click(cx.listener(
                                        |this, _: &ClickEvent, _, cx| {
                                            this.settings_open = false;
                                            cx.notify();
                                        },
                                    ))),
                            )
                            .child(heading("Diff"))
                            .child(item(
                                "Changed words",
                                "How the words that changed inside a changed line are marked, in Words and Structural modes.",
                                marks,
                            ))
                            .child(
                                div()
                                    .rounded(px(ISLAND_RADIUS))
                                    .overflow_hidden()
                                    .bg(theme::editor())
                                    .border_1()
                                    .border_color(theme::island_border())
                                    .child(diff_view::mark_preview(s.mark_style)),
                            )
                            .child(item(
                                "Default mode",
                                "What a diff opens in. Lines, Words and Structural are also in the toolbar and the View menu.",
                                modes,
                            ))
                            .child(heading("General"))
                            .child(item(
                                "Appearance",
                                "Follow the system, or stay dark. Only the dark theme exists so far, so both look the same until a light one is designed.",
                                appearance,
                            ))
                            .child(item(
                                "Open files in",
                                "The editor behind “Open in …” on a file or a line. It opens the file in the working tree.",
                                if installed.is_empty() {
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme::faint())
                                        .child("Android Studio, Zed or VS Code not found")
                                        .into_any_element()
                                } else {
                                    editors.into_any_element()
                                },
                            ))
                            .child(heading("Repository"))
                            .child(item(
                                "Refresh when the repository changes",
                                "Re-reads refs and history when something outside the app commits, amends or rebases. Never writes to the repository.",
                                refresh,
                            ))
                            .child(item(
                                "Fetch in the background",
                                "Brings other people’s force pushes in as versions. It writes remote-tracking refs, so it will be opt-in. Not built yet.",
                                div()
                                    .px(px(6.))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(theme::faint())
                                    .text_size(px(10.))
                                    .text_color(theme::faint())
                                    .child("later"),
                            )),
                    ),
            )
    }
}

fn style_id(style: MarkStyle) -> &'static str {
    match style {
        MarkStyle::Tinted => "mark-tinted",
        MarkStyle::Underlined => "mark-underlined",
    }
}

fn mode_id(mode: DiffMode) -> &'static str {
    match mode {
        DiffMode::Lines => "default-lines",
        DiffMode::Words => "default-words",
        DiffMode::Structural => "default-structural",
    }
}

fn heading(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

/// A labelled row: the name and what it does on the left, the control on the right.
fn item(title: &'static str, detail: &'static str, control: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_4()
        .py(px(10.))
        .border_b_1()
        .border_color(theme::island_border())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(title)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(detail),
                ),
        )
        .child(div().flex_none().child(control))
}

fn switch(id: &'static str, on: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w(px(34.))
        .h(px(20.))
        .rounded_full()
        .flex()
        .items_center()
        .px(px(2.))
        .cursor_pointer()
        .bg(if on { theme::accent() } else { theme::hover() })
        .when(on, |s| s.justify_end())
        .child(div().size(px(16.)).rounded_full().bg(if on {
            theme::base()
        } else {
            theme::muted()
        }))
}
