//! The keyboard card: every key of the app in one place, grouped by what one is doing. `?` opens
//! it; esc or a click closes it. Designed in round 2 of the review (`../Design/review/round-2.html`, A29).

use super::{ISLAND_RADIUS, Workspace, theme};
use gpui::{ClickEvent, Context, FontWeight, MouseButton, div, prelude::*, px};

/// (group, [(keys, what it does)]).
const GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Move",
        &[
            ("Tab ⇧Tab", "next / previous island"),
            ("⌃1 ⌃2 ⌃3 ⌃4", "branches, commits, files, diff"),
            ("j k", "next / previous commit"),
            ("h l  ← →", "previous / next file"),
            ("n p", "next / previous change"),
            ("⇧n ⇧p", "next / previous discussion"),
            ("Space ⇧Space", "page down / up"),
            ("⌘[ ⌘]", "back / forward"),
        ],
    ),
    (
        "Comment",
        &[
            ("c", "comment by line number"),
            ("↑ ↓ in it", "step the line"),
            ("Tab in the box", "agent / merge request"),
            ("⌘↵", "add or post"),
            ("⇧⌘↵", "keep pending · submit review"),
            ("r  x", "reply to / resolve a discussion"),
        ],
    ),
    (
        "Find",
        &[
            ("⌘K", "search everything"),
            ("⌘F", "find in the diff"),
            ("⌘⇧F", "search commits"),
            ("⌘P", "filter files"),
            ("⌘⇧C", "compare two refs"),
        ],
    ),
    (
        "View",
        &[
            ("⌥U ⌥D ⌥Z", "split, mode, wrap"),
            ("⌥E ⌥W", "full file, whitespace"),
            ("⌥⌘1 ⌥⌘2 ⌥⌘3", "branches, files, merge requests"),
            ("⌘.", "the diff alone"),
            ("⌘T ⌘W ⌘1…9", "tabs"),
            ("⌥⌘M", "new merge request"),
            ("?", "this card"),
        ],
    ),
];

impl Workspace {
    pub(super) fn toggle_shortcuts(&mut self, cx: &mut Context<Self>) {
        self.shortcuts = !self.shortcuts;
        cx.notify();
    }

    pub(super) fn render_shortcuts(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        if !self.shortcuts {
            return None;
        }
        let groups = GROUPS.iter().map(|(title, rows)| {
            div()
                .w(px(344.))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::faint())
                        .child(title.to_uppercase()),
                )
                .children(rows.iter().map(|(keys, what)| {
                    div()
                        .flex()
                        .gap_3()
                        .child(
                            div()
                                .w(px(124.))
                                .flex_none()
                                .font_family(theme::CODE_FONT)
                                .text_size(px(12.))
                                .text_color(theme::accent())
                                .child(*keys),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(theme::muted())
                                .child(*what),
                        )
                }))
        });
        Some(
            div()
                .id("shortcuts-backdrop")
                .absolute()
                .size_full()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(72.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.shortcuts = false;
                        cx.notify()
                    }),
                )
                .child(
                    div()
                        .occlude()
                        .w(px(760.))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .p_4()
                        .rounded(px(ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .text_size(px(13.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Keyboard"),
                                )
                                .child(
                                    div()
                                        .id("shortcuts-close")
                                        .px_2()
                                        .rounded(px(super::ROW_RADIUS))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme::hover()))
                                        .text_color(theme::muted())
                                        .child("Close  esc")
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.shortcuts = false;
                                            cx.notify()
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_x(px(24.))
                                .gap_y(px(16.))
                                .children(groups),
                        ),
                ),
        )
    }
}
