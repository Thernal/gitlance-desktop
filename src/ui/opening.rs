//! Opening a repository while one is open: in this tab, or a new one. A card asks (↵ takes the
//! highlighted answer, ⇥ or ← → switch it, esc cancels) unless Settings → General says which, and
//! "Remember my choice" (off until ticked) writes the answer there. Every way of opening one comes
//! here: ⌘O, the recent list, a folder dropped on the window, a link into another project.

use super::keys::{self, On};
use super::px;
use super::{ISLAND_RADIUS, ROW_RADIUS, Workspace, theme};
use crate::links::Target;
use crate::storage::OpenIn;
use gpui::{ClickEvent, Context, FontWeight, KeyDownEvent, MouseButton, div, prelude::*};
use std::path::PathBuf;

/// The question being asked.
pub struct OpenAsk {
    path: PathBuf,
    /// A link's place in the repository, to go to once it is open.
    then: Option<Target>,
    new_tab: bool,
    remember: bool,
}

impl Workspace {
    /// Opens the repository at `path`, here or in a new tab as Settings says or the card answers;
    /// `then` is where a link wants to go in it.
    pub(super) fn request_open(
        &mut self,
        path: PathBuf,
        then: Option<Target>,
        cx: &mut Context<Self>,
    ) {
        if self.root.as_ref() == Some(&path) {
            return self.open_here(path, then, cx);
        }
        // Open in another tab already: that tab comes on screen.
        let other = self
            .shell
            .as_ref()
            .and_then(gpui::WeakEntity::upgrade)
            .filter(|shell| shell.read(cx).tab_of(&path).is_some());
        if other.is_some() {
            return self.open_in_new_tab(path, then, cx);
        }
        if self.root.is_none() {
            return self.open_here(path, then, cx);
        }
        match self.settings.open_in {
            OpenIn::ThisTab => self.open_here(path, then, cx),
            OpenIn::NewTab => self.open_in_new_tab(path, then, cx),
            OpenIn::Ask => {
                self.open_ask = Some(OpenAsk {
                    path,
                    then,
                    new_tab: self.settings.last_new_tab,
                    remember: false,
                });
                cx.notify();
            }
        }
    }

    fn open_here(&mut self, path: PathBuf, then: Option<Target>, cx: &mut Context<Self>) {
        self.pending_link = then;
        if self.root.as_ref() == Some(&path) {
            self.follow_pending_link(cx);
        } else {
            self.open_repo(path, None, cx);
        }
    }

    /// A new tab of this window, after the current event (the shell reads this workspace).
    pub(super) fn open_in_new_tab(
        &mut self,
        path: PathBuf,
        then: Option<Target>,
        cx: &mut Context<Self>,
    ) {
        let Some(shell) = self.shell.as_ref().and_then(gpui::WeakEntity::upgrade) else {
            return self.open_here(path, then, cx);
        };
        let window = self.window;
        cx.defer(move |cx| {
            window
                .update(cx, |_, window, cx| {
                    let ws = shell.update(cx, |s, cx| s.open_tab(path, window, cx));
                    ws.update(cx, |ws, _| ws.pending_link = then);
                })
                .ok();
        });
    }

    /// Answers the card: in a new tab or here, remembered when asked to.
    fn answer_open(&mut self, new_tab: bool, cx: &mut Context<Self>) {
        let Some(ask) = self.open_ask.take() else {
            return;
        };
        self.settings.last_new_tab = new_tab;
        if ask.remember {
            self.settings.open_in = if new_tab {
                OpenIn::NewTab
            } else {
                OpenIn::ThisTab
            };
        }
        self.settings.save();
        if new_tab {
            self.open_in_new_tab(ask.path, ask.then, cx);
        } else {
            self.open_here(ask.path, ask.then, cx);
        }
        cx.notify();
    }

    pub(super) fn open_ask_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(ask) = self.open_ask.as_mut() else {
            return;
        };
        match event.keystroke.key.as_str() {
            "escape" => self.open_ask = None,
            "enter" => {
                let new_tab = ask.new_tab;
                return self.answer_open(new_tab, cx);
            }
            "tab" | "left" | "right" => ask.new_tab = !ask.new_tab,
            "space" => ask.remember = !ask.remember,
            _ => return,
        }
        cx.notify();
    }

    pub(super) fn render_open_ask(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let ask = self.open_ask.as_ref()?;
        let name = crate::labels::project_name(&ask.path);
        let current = self
            .root
            .as_deref()
            .map(crate::labels::project_name)
            .unwrap_or_default();
        let choice = |id: &'static str, label: &'static str, hint: String, new_tab: bool| {
            let on = ask.new_tab == new_tab;
            div()
                .id(id)
                .flex_1()
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .rounded(px(ROW_RADIUS))
                .border_2()
                .border_color(if on {
                    theme::focus()
                } else {
                    theme::island_border()
                })
                .when(on, |s| s.bg(theme::selected()))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(label),
                        )
                        .children(on.then(|| keys::caps("↵", On::Plain))),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(hint),
                )
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.answer_open(new_tab, cx)),
                )
        };
        let remember = ask.remember;
        Some(
            div()
                .id("open-ask-backdrop")
                .absolute()
                .size_full()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(120.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.open_ask = None;
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .occlude()
                        .w(px(480.))
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
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("Open {name}")),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(choice(
                                    "open-this-tab",
                                    "This tab",
                                    format!("In place of {current}"),
                                    false,
                                ))
                                .child(choice(
                                    "open-new-tab",
                                    "New tab",
                                    format!("Next to {current}"),
                                    true,
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .id("open-remember")
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .cursor_pointer()
                                        .child(
                                            div()
                                                .size(px(14.))
                                                .rounded(px(3.))
                                                .border_2()
                                                .border_color(if remember {
                                                    theme::accent()
                                                } else {
                                                    theme::faint()
                                                })
                                                .when(remember, |s| s.bg(theme::accent()))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(10.))
                                                .text_color(theme::base())
                                                .child(if remember { "✓" } else { "" }),
                                        )
                                        .child("Remember my choice")
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            if let Some(a) = this.open_ask.as_mut() {
                                                a.remember = !a.remember;
                                                cx.notify();
                                            }
                                        })),
                                )
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .text_size(px(11.))
                                        .text_color(theme::faint())
                                        .child(keys::caps("⇥", On::Plain))
                                        .child("switches")
                                        .child(keys::caps("Space", On::Plain))
                                        .child("remembers"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child("Settings → General can change it later."),
                        ),
                ),
        )
    }
}
