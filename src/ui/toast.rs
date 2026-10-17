//! Errors as desktop notifications: toasts stacked at the top right, newest at the bottom, each with
//! ✕ and gone after a few seconds. Code still reports an error by setting `Workspace::error`; each
//! frame moves it here.

use super::{Tip, Workspace, icons, px, theme};
use gpui::{ClickEvent, Context, SharedString, Task, div, prelude::*};
use std::time::Duration;

/// How long a toast stays.
const SHOWN: Duration = Duration::from_secs(6);
/// The most toasts on screen; an older one goes when another comes.
const MOST: usize = 4;

struct Toast {
    id: u64,
    text: SharedString,
    /// Dismisses it when `SHOWN` is up; dropping it (✕, the same text again) cancels that.
    _timer: Task<()>,
}

#[derive(Default)]
pub struct Toasts {
    list: Vec<Toast>,
    next: u64,
}

impl Workspace {
    /// Moves a reported error into a toast.
    pub(super) fn take_error(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.error.take() {
            self.toast(text, cx);
        }
    }

    /// Shows `text`; the same text already showing starts over at the bottom instead.
    fn toast(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.toasts.list.retain(|t| t.text != text);
        let id = self.toasts.next;
        self.toasts.next += 1;
        let timer = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SHOWN).await;
            this.update(cx, |this, cx| this.dismiss(id, cx)).ok();
        });
        self.toasts.list.push(Toast {
            id,
            text,
            _timer: timer,
        });
        let over = self.toasts.list.len().saturating_sub(MOST);
        self.toasts.list.drain(..over);
    }

    fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.toasts.list.retain(|t| t.id != id);
        cx.notify();
    }

    /// The stack, over everything else, under the title bar's right end.
    pub(super) fn render_toasts(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if self.toasts.list.is_empty() {
            return None;
        }
        let toasts = self.toasts.list.iter().map(|t| {
            let id = t.id;
            div()
                .id(("toast", id as usize))
                .occlude()
                .w(px(360.))
                .flex()
                .items_start()
                .gap(px(10.))
                .pl(px(12.))
                .pr(px(6.))
                .py(px(10.))
                .rounded(px(10.))
                .border_1()
                .border_color(theme::island_border())
                .bg(theme::panel())
                .shadow_lg()
                .text_size(px(13.))
                .line_height(px(19.))
                .child(
                    icons::icon("alert")
                        .size(px(16.))
                        .mt(px(2.))
                        .text_color(theme::removed()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(theme::text())
                        .child(super::keys::text(&t.text).into_owned()),
                )
                .child(
                    div()
                        .id(("toast-close", id as usize))
                        .flex_none()
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .child(icons::icon("x").size(px(14.)).text_color(theme::muted()))
                        .tooltip_show_delay(crate::ui::hover::LONG)
                        .tooltip(|_, cx| cx.new(|_| Tip("Dismiss")).into())
                        .on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| this.dismiss(id, cx)),
                        ),
                )
        });
        Some(
            gpui::deferred(
                div()
                    .absolute()
                    .top(px(52.))
                    .right(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(toasts),
            )
            .with_priority(2),
        )
    }
}
