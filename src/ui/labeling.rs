//! Naming a project and giving it a hue, naming a chat (`crate::labels`; the developer,
//! 2026-10-23): a card over the window, opened from a project's right click — the rail, the menu's
//! project row, the title bar's tabs or project — ⌘K, or a chat's right click in the Agents
//! window. ↵ keeps it, esc leaves it as it was.

use super::lists::Field;
use super::{Workspace, keys, px, theme};
use gpui::{
    Bounds, ClickEvent, Context, DragMoveEvent, MouseButton, MouseDownEvent, Pixels, div,
    prelude::*,
};
use std::path::PathBuf;

/// What is being named.
#[derive(Clone)]
pub enum Target {
    Project(PathBuf),
    Chat { session: String, agent: String },
}

/// The card's state; the name being typed is `Workspace::label_name`.
pub struct LabelEdit {
    target: Target,
    /// The project's hue, None for the one it is given.
    hue: Option<f32>,
    /// What it is called without a name of its own: the folder's, the chat's first message.
    fallback: String,
}

/// A drag along the hue strip.
struct HueDrag;

impl Workspace {
    /// Opens the card for `target`, with its name and hue as they are.
    pub(super) fn open_label(&mut self, target: Target, cx: &mut Context<Self>) {
        let (name, hue, fallback) = match &target {
            Target::Project(path) => {
                let own = crate::labels::project(path);
                let folder = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (own.name, own.hue, folder)
            }
            Target::Chat { session, agent } => {
                let first = self
                    .agent_chats
                    .iter()
                    .find(|c| &c.session == session)
                    .map(|c| c.title.clone())
                    .unwrap_or_default();
                let own = crate::labels::chat(session);
                // The title shown is already the name given; without one, the first message.
                let fallback = if own.is_some() {
                    agent.clone()
                } else {
                    first.clone()
                };
                (own.unwrap_or_default(), None, fallback)
            }
        };
        self.label_name = name;
        self.label_edit = Some(LabelEdit {
            target,
            hue,
            fallback,
        });
        self.start_field(Field::Label, cx);
    }

    /// ↵ or Save: the name and hue kept; a name equal to the folder's is none of its own.
    pub(super) fn save_label(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.label_edit.take() else {
            return;
        };
        let name = self.label_name.trim().to_owned();
        match edit.target {
            Target::Project(path) => {
                let name = if name == edit.fallback {
                    String::new()
                } else {
                    name
                };
                crate::labels::set_project(
                    &path,
                    crate::labels::Project {
                        name,
                        hue: edit.hue,
                    },
                );
            }
            Target::Chat { session, .. } => {
                crate::labels::set_chat(&session, &name);
                self.refresh_chats();
            }
        }
        self.close_label(cx);
    }

    /// Esc or Cancel: nothing kept.
    pub(super) fn close_label(&mut self, cx: &mut Context<Self>) {
        self.label_edit = None;
        self.label_name.clear();
        if self.field == Some(Field::Label) {
            self.field = None;
        }
        // The rail and the tabs read the names on their next draw.
        cx.notify();
    }

    fn set_label_hue(&mut self, hue: Option<f32>, cx: &mut Context<Self>) {
        if let Some(edit) = self.label_edit.as_mut() {
            edit.hue = hue;
            cx.notify();
        }
    }

    /// The card: what is named, its mark as it will look, the name, and a project's hue.
    pub(super) fn render_label_card(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let edit = self.label_edit.as_ref()?;
        let project = match &edit.target {
            Target::Project(path) => Some(path.clone()),
            Target::Chat { .. } => None,
        };
        let shown = if self.label_name.trim().is_empty() {
            edit.fallback.clone()
        } else {
            self.label_name.trim().to_owned()
        };
        let (title, place, placeholder) = match &edit.target {
            Target::Project(path) => (
                "Name and colour",
                super::format::home_relative(path),
                "The folder's name",
            ),
            Target::Chat { agent, .. } => ("Name this chat", agent.clone(), "Its first message"),
        };
        // A project's mark as it will look: its name's initials on its hue, or the one it is given.
        let mark = project.as_ref().map(|_| {
            let tint = match edit.hue {
                Some(h) => super::rail::hue_at(h),
                None => super::rail::hue(&edit.fallback),
            };
            super::rail::project_mark(&super::rail::initials(&shown), tint, 34.)
        });
        let field = div()
            .id("label-name")
            .flex_1()
            .min_w_0()
            .h(px(32.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(super::ROW_RADIUS))
            .border_2()
            .border_color(if self.field == Some(Field::Label) {
                theme::focus()
            } else {
                theme::island_border()
            })
            .bg(theme::base())
            .cursor_text()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.start_field(Field::Label, cx)))
            .child(self.field_look(
                &self.label_name,
                self.field == Some(Field::Label),
                placeholder,
                Default::default(),
                cx.entity().downgrade(),
            ));
        let hue = project.map(|_| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child("Colour"),
                        )
                        .child(chip(
                            "label-hue-auto",
                            "Automatic",
                            edit.hue.is_none(),
                            cx.listener(|this, _: &ClickEvent, _, cx| this.set_label_hue(None, cx)),
                        )),
                )
                .child(self.hue_strip(edit.hue, cx))
        });
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .h(px(30.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(6.))
                .text_size(px(12.))
                .cursor_pointer()
                .when(primary, |s| {
                    s.bg(theme::accent())
                        .text_color(theme::base())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .when(!primary, |s| {
                    s.text_color(theme::muted())
                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                })
                .child(label)
        };
        let reset_project = edit.hue.is_some() || !self.label_name.is_empty();
        Some(
            div()
                .absolute()
                .size_full()
                .child(
                    div()
                        .id("label-backdrop")
                        .absolute()
                        .size_full()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.close_label(cx)),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(90.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .occlude()
                                .w(px(400.))
                                .p(px(16.))
                                .flex()
                                .flex_col()
                                .gap(px(14.))
                                .rounded(px(super::ISLAND_RADIUS))
                                .border_1()
                                .border_color(theme::island_border())
                                .bg(theme::panel())
                                .shadow_lg()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .child(title),
                                        )
                                        .child(
                                            super::hover::cut("place", place)
                                                .text_size(px(11.))
                                                .text_color(theme::faint()),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(10.))
                                        .children(mark)
                                        .child(field),
                                )
                                .children(hue)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .when(reset_project, |s| {
                                            s.child(button("label-reset", "Reset", false).on_click(
                                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                                    this.label_name.clear();
                                                    this.set_label_hue(None, cx);
                                                }),
                                            ))
                                        })
                                        .child(div().flex_1())
                                        .child(button("label-cancel", "Cancel", false).on_click(
                                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                                this.close_label(cx)
                                            }),
                                        ))
                                        .child(
                                            button("label-save", "Save", true)
                                                .child(keys::caps("↵", keys::On::Filled))
                                                .on_click(cx.listener(
                                                    |this, _: &ClickEvent, _, cx| {
                                                        this.save_label(cx)
                                                    },
                                                )),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The wheel as a strip: a press sets the hue, a drag follows the pointer.
    fn hue_strip(&self, hue: Option<f32>, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui::{linear_color_stop as stop, linear_gradient};
        let area: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>> = Default::default();
        let found = area.clone();
        let at = |b: Bounds<Pixels>, x: Pixels| -> f32 {
            let w = f32::from(b.size.width).max(1.);
            (f32::from(x - b.origin.x) / w).clamp(0., 1.) * 360.
        };
        div()
            .id("label-hue")
            .relative()
            .h(px(18.))
            .w_full()
            .flex()
            .cursor_crosshair()
            .children((0..6).map(|k| {
                let k = k as f32;
                div()
                    .flex_1()
                    .h_full()
                    .when(k == 0., |d| d.rounded_l(px(4.)))
                    .when(k == 5., |d| d.rounded_r(px(4.)))
                    .bg(linear_gradient(
                        90.,
                        stop(super::rail::hue_at(k * 60.), 0.),
                        stop(super::rail::hue_at((k + 1.) * 60.), 1.),
                    ))
            }))
            .child(
                gpui::canvas(move |b, _, _| found.set(Some(b)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .children(hue.map(|h| {
                div()
                    .absolute()
                    .top(px(-3.))
                    .bottom(px(-3.))
                    .left(gpui::relative(h / 360.))
                    .ml(px(-3.))
                    .w(px(6.))
                    .rounded(px(3.))
                    .border_2()
                    .border_color(gpui::white())
                    .shadow_sm()
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener({
                    let area = area.clone();
                    move |this, e: &MouseDownEvent, _, cx| {
                        if let Some(b) = area.get() {
                            this.set_label_hue(Some(at(b, e.position.x)), cx);
                        }
                    }
                }),
            )
            .on_drag(HueDrag, |_, _, _, cx| cx.new(|_| super::hover::NoTip))
            .on_drag_move(cx.listener(move |this, e: &DragMoveEvent<HueDrag>, _, cx| {
                this.set_label_hue(Some(at(e.bounds, e.event.position.x)), cx);
            }))
    }
}

/// A small choice that is on or off.
fn chip(
    id: &'static str,
    label: &'static str,
    on: bool,
    click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(22.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .text_size(px(11.))
        .cursor_pointer()
        .border_1()
        .border_color(if on {
            theme::accent()
        } else {
            theme::island_border()
        })
        .text_color(if on { theme::text() } else { theme::muted() })
        .hover(|s| s.bg(theme::hover()))
        .child(label)
        .on_click(click)
}
