//! The project rail on the window's left edge (Design flow 02, #49): a mark per open repository,
//! ⌘1…9 in its order, the unread count on the ones you are not in, + to open another. A
//! right-click moves a project up or down, shows its folder, opens it in a window of its own or
//! closes it; a drag reorders. Settings → Appearance picks the rail (default), tabs or the menu.

use super::menu::{Act, Entry};
use super::shell::TabInfo;
use super::{TipOwned, Workspace, icons, px, theme};
use crate::storage::Projects;
use gpui::{
    ClickEvent, Context, FontWeight, MouseButton, MouseDownEvent, Rgba, Window, div, prelude::*,
};

/// A project being dragged along the rail: its index.
#[derive(Clone, Copy)]
pub struct DragProject(pub usize);

/// What follows the pointer while a project is dragged: its mark.
pub struct DragMark {
    mark: String,
    hue: Rgba,
}

impl Render for DragMark {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        project_mark(&self.mark, self.hue, 34.)
    }
}

/// Up to two letters for a repository: the first of its first two words.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(['-', '_', '.', ' '])
        .filter(|w| !w.is_empty())
        .collect();
    let letters: String = match words.as_slice() {
        [] => "?".to_owned(),
        [one] => one.chars().take(2).collect(),
        [a, b, ..] => a.chars().take(1).chain(b.chars().take(1)).collect(),
    };
    letters.to_uppercase()
}

/// A colour of its own per repository name, from the theme's code colours.
pub fn hue(name: &str) -> Rgba {
    let sum = name
        .bytes()
        .fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    theme::code([3, 2, 1, 5, 8, 4, 7][sum % 7]).into()
}

/// The rounded square with a project's initials on a tint of its colour.
pub fn project_mark(mark: &str, hue: Rgba, size: f32) -> gpui::Div {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size * 0.29))
        .bg(Rgba { a: 0.35, ..hue })
        .text_color(theme::text())
        .text_size(px(size * 0.35))
        .font_weight(FontWeight::BOLD)
        .child(mark.to_owned())
}

impl Workspace {
    /// The rail; None unless Settings chose it, and in zen.
    pub(super) fn render_project_rail(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        if super::projects() != Projects::Rail || self.zen.is_some() {
            return None;
        }
        let tabs = self.tabs.borrow().clone();
        let n = tabs.len();
        let items = tabs.into_iter().enumerate().map(|(ix, tab)| {
            let mark = initials(&tab.title);
            let tint = hue(&tab.title);
            let (drag_mark, drag_hue) = (mark.clone(), tint);
            // A long hover: the project's name, where it is, its unread count and key (#53).
            let place = tab
                .root
                .as_ref()
                .map(|r| format!(" · {}", super::format::home_relative(r)))
                .unwrap_or_default();
            let unread = match tab.unread {
                0 => String::new(),
                n => format!(" · {n} new"),
            };
            let tip = if ix < 9 {
                format!("{}{place}{unread}  ⌘{}", tab.title, ix + 1)
            } else {
                format!("{}{place}{unread}", tab.title)
            };
            let info = tab.clone();
            div()
                .id(("project", ix))
                .relative()
                .size(px(38.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(12.))
                .cursor_pointer()
                .when(tab.active, |s| s.border_2().border_color(theme::accent()))
                .when(!tab.active, |s| s.hover(|s| s.bg(theme::hover())))
                .drag_over::<DragProject>(|s, _, _, _| s.bg(theme::selected()))
                .child(project_mark(&mark, tint, 30.))
                // The project you are in has a bar at the rail's edge.
                .children(tab.active.then(|| {
                    div()
                        .absolute()
                        .left(px(-7.))
                        .top(px(9.))
                        .w(px(3.))
                        .h(px(18.))
                        .rounded_full()
                        .bg(theme::text())
                }))
                .children((!tab.active && tab.unread > 0).then(|| {
                    div()
                        .absolute()
                        .top(px(-4.))
                        .right(px(-4.))
                        .min_w(px(16.))
                        .h(px(16.))
                        .px(px(4.))
                        .rounded_full()
                        .bg(theme::removed())
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::base())
                        .child(tab.unread.min(99).to_string())
                }))
                .children((!tab.active && tab.unread == 0 && tab.news).then(|| {
                    div()
                        .absolute()
                        .top(px(-1.))
                        .right(px(-1.))
                        .size(px(8.))
                        .rounded_full()
                        .bg(theme::accent())
                }))
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| TipOwned(tip.clone().into())).into())
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.with_shell(window, cx, move |s, window, cx| s.select(ix, window, cx))
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.project_menu(ix, n, &info, event.position, cx)
                    }),
                )
                .on_drag(DragProject(ix), move |_, _, _, cx| {
                    cx.new(|_| DragMark {
                        mark: drag_mark.clone(),
                        hue: drag_hue,
                    })
                })
                .on_drop(cx.listener(move |this, dragged: &DragProject, window, cx| {
                    let from = dragged.0;
                    this.with_shell(window, cx, move |s, _, cx| s.move_tab(from, ix, cx));
                }))
        });
        Some(
            div()
                .w(px(48.))
                .flex_none()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .pt(px(4.))
                .children(items)
                .child(
                    div()
                        .id("project-add")
                        .size(px(34.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(10.))
                        .border_1()
                        .border_dashed()
                        .border_color(theme::selected())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .child(icons::icon("open").size(px(16.)).text_color(theme::muted()))
                        .tooltip_show_delay(crate::ui::hover::LONG)
                        .tooltip(|_, cx| {
                            cx.new(|_| {
                                TipOwned(
                                    super::keys::text("Open a repository  ⌘T")
                                        .into_owned()
                                        .into(),
                                )
                            })
                            .into()
                        })
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.with_shell(window, cx, |s, window, cx| s.new_tab(window, cx))
                        })),
                ),
        )
    }

    /// The right-click menu of project `ix` of `n`.
    fn project_menu(
        &mut self,
        ix: usize,
        n: usize,
        tab: &TabInfo,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let mut moves = Vec::new();
        if ix > 0 {
            moves.push(Entry::new(
                "Move up",
                Act::MoveProject {
                    from: ix,
                    to: ix - 1,
                },
            ));
        }
        if ix + 1 < n {
            moves.push(Entry::new(
                "Move down",
                Act::MoveProject {
                    from: ix,
                    to: ix + 1,
                },
            ));
        }
        let mut open = Vec::new();
        if let Some(root) = tab.root.clone() {
            open.push(Entry::new(
                if crate::platform::MAC {
                    "Reveal in Finder"
                } else {
                    "Show the folder"
                },
                Act::Reveal(root.clone()),
            ));
            open.push(Entry::new("Open in a new window", Act::NewWindow(root)));
        }
        let close = vec![Entry::new("Close", Act::CloseProject(ix))];
        self.open_menu(at, vec![moves, open, close], cx);
    }

    /// The projects in the menu of places (Settings → Appearance → Menu): one row each.
    pub(super) fn menu_projects(&self) -> Vec<(usize, TabInfo)> {
        if super::projects() != Projects::Menu {
            return Vec::new();
        }
        self.tabs.borrow().iter().cloned().enumerate().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_takes_the_first_letters_of_the_first_two_words() {
        assert_eq!(initials("payments-service"), "PS");
        assert_eq!(initials("Desktop"), "DE");
        assert_eq!(initials("gitlance_desktop.app"), "GD");
        assert_eq!(initials(""), "?");
        assert_eq!(hue("payments-service"), hue("payments-service"));
    }
}
