//! The keyboard card: every key of the app in one place, grouped by what one is doing. `?` opens
//! it; esc or a click closes it. Designed in round 2 of the review (`../Design/review/round-2.html`, A29).

use super::px;
use super::{ISLAND_RADIUS, Workspace, theme};
use gpui::{ClickEvent, Context, FontWeight, MouseButton, div, prelude::*};
use std::sync::atomic::AtomicBool;

/// The card was shown at launch in this run already (a new tab does not show it again).
pub static SHOWN: AtomicBool = AtomicBool::new(false);

/// (group, [(keys, what it does)]).
const GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Move",
        &[
            ("⌃⌘1 ⌃⌘2 ⌃⌘3 ⌃⌘4", "Inbox, Code, Branches, Merge requests"),
            ("Tab ⇧Tab", "next / previous island"),
            ("⌃1", "switch to a branch"),
            ("⌃2 ⌃3 ⌃4", "commits, files, diff"),
            ("j k", "next / previous commit"),
            ("h l  ← →", "previous / next file"),
            (
                "[  ]",
                "on a commit: the one before / after it; else the file",
            ),
            ("n p", "next / previous change"),
            (
                "v  ⇧v",
                "mark the file reviewed and go on · toggle the mark",
            ),
            ("⇧n ⇧p", "next / previous discussion"),
            ("⌥⇧V", "mark all files reviewed / unmark all"),
            ("⌥↓ ⌥↑", "next unreviewed / previous merge request"),
            ("Space ⇧Space", "page down / up"),
            ("esc", "one page up · ⌘W closes what is open"),
            ("⌘[ ⌘]", "back / forward, across spaces"),
        ],
    ),
    (
        "Comment",
        &[
            ("c", "comment by line number"),
            ("↑ ↓ in it", "step the line"),
            ("Tab in the box", "agent / merge request"),
            ("⌘↵", "add or post"),
            (
                "⇧⌘↵",
                "keep pending · submit review (comment, approve, request changes)",
            ),
            ("⌥N ⌥P", "next / previous of your pending comments"),
            ("a", "approve the merge request · take the approval back"),
            (
                "⌘⇧N",
                "the Inbox: mentions, review requests, replies (j k ↵ e)",
            ),
            ("r  x  o", "reply to, resolve, fold a discussion"),
            ("e", "edit your comment in the discussion"),
            ("⇧e", "react to the discussion's newest note"),
            ("⌘⇧D", "show or hide the Discussions window · esc hides it"),
            ("⌥M", "comments as markdown / raw text"),
            ("⌥⌘A", "send the comments for your agent to its chat"),
            ("⇧⌘A", "show or hide the Agents window"),
            ("⌘B ⌘I ⌘⇧X ⌘⇧E ⌘⇧K", "bold, italic, strike, code, link"),
            ("⌘⇧P in the box", "preview the markdown"),
        ],
    ),
    (
        "Find",
        &[
            ("⌘K", "search everywhere · ⇥ the next tab, > actions"),
            ("⌘P  ⌘⇧C  ⌘⇧B", "find a file · a commit · a branch"),
            ("⌘⇧R  ⌘⇧P", "find a merge request · an action"),
            ("⌘F", "find in the diff · filter a space's list"),
            ("⌘⌥F", "search all files of the diff"),
            ("⌘⇧F", "search commits"),
            ("⌥⌘P", "filter the changed files by path"),
            ("⌥⌘C", "compare two refs"),
            ("⌥⇧⌘C", "copy the link to this page"),
            ("⌘E", "recent places"),
            ("⌘⇧O", "functions and types this file changes"),
            ("F3  ⌘F3", "bookmark a line · list the bookmarks"),
        ],
    ),
    (
        "View",
        &[
            ("⌥U ⌥D ⌥Z", "split, mode, wrap"),
            ("⌥E ⌥W", "full file, whitespace"),
            ("⌥⌘B", "annotate: who wrote each line"),
            ("⌘J ⌥⌘2 ⌥⌘3", "side panel, fold Changes, Merge requests"),
            ("⌘B", "fold the menu to icons"),
            ("⌘.", "zen: the diff alone, the keys of a review below"),
            ("⌘+  ⌘−  ⌘0", "interface bigger, smaller, actual size"),
            ("⌘T ⇧⌘W ⌘1…9", "tabs"),
            ("⌥⌘M", "new merge request"),
            ("⌥⌘O ⌥⌘R", "the file or folder in its app · in Finder"),
            ("⌥⌘L", "leave the merge request, back to your branch"),
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
                                .child(super::keys::text(keys).into_owned()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(theme::muted())
                                .child(super::words::say(what).into_owned()),
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
                        // Taller than the window, the groups scroll; the switch stays in view.
                        .max_h(gpui::relative(0.86))
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
                        .child(self.render_card_presets(cx))
                        .children(self.render_changed_keys(cx))
                        .child(
                            div()
                                .id("shortcut-groups")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .flex()
                                .flex_wrap()
                                .gap_x(px(24.))
                                .gap_y(px(16.))
                                .children(groups),
                        )
                        .child(self.render_launch_switch()),
                ),
        )
    }

    /// Which keys yours start from — GitLance's, Android Studio's or VS Code's (#54): chosen here
    /// when the card greets the first launch, as Settings → Keyboard does later.
    fn render_card_presets(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        use super::keymap::{self, Keymap};
        let preset = cx.global::<Keymap>().preset();
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(theme::muted())
            .child("Your keys start from")
            .child(super::diff_view::group().children(
                keymap::Preset::ALL.into_iter().enumerate().map(|(i, p)| {
                    super::diff_view::chip(("card-preset", i), p.label(), p == preset).on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            keymap::set_preset(p, cx);
                            this.refresh_menus(cx);
                            cx.notify();
                        }),
                    )
                }),
            ))
            .child(
                div()
                    .text_color(theme::faint())
                    .child("· Settings → Keyboard changes any of them"),
            )
    }

    /// The keys changed in Settings → Keyboard, above the shipped ones the card lists.
    fn render_changed_keys(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let changes = cx.global::<super::keymap::Keymap>().changes();
        (!changes.is_empty()).then(|| {
            div()
                .px_3()
                .py_2()
                .rounded(px(super::ROW_RADIUS))
                .bg(theme::hover())
                .text_size(px(12.))
                .text_color(theme::muted())
                .child(format!(
                    "Changed by you: {} — the list below shows the shipped keys.",
                    changes
                        .iter()
                        .map(|(label, keys)| {
                            let keys: Vec<String> =
                                keys.iter().map(|k| super::keymap::symbols(k)).collect();
                            if keys.is_empty() {
                                format!("{label} (no key)")
                            } else {
                                format!("{label} {}", super::keys::text(&keys.join(" ")))
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" · ")
                ))
        })
    }

    /// How to bring the card back.
    fn render_launch_switch(&self) -> impl IntoElement + use<> {
        div()
            .pt(px(8.))
            .border_t_1()
            .border_color(theme::island_border())
            .text_size(px(12.))
            .text_color(theme::faint())
            .child(
                super::keys::text("? or ⌘/ shows this again, and Help → Keyboard Shortcuts")
                    .into_owned(),
            )
    }
}
