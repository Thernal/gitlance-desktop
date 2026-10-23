//! Errors as desktop notifications: toasts stacked at the top right, newest at the bottom, each with
//! ✕ and gone after a few seconds. Code still reports an error by setting `Workspace::error`; each
//! frame moves it here. A deleted draft's toast says so and offers Undo instead (#76).

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
    /// The draft it deleted: Undo puts it back (#76).
    undo: Option<crate::review::Comment>,
    /// News, not an error: no alert mark (#77).
    info: bool,
    /// Dismissed: drawn on its way out until it is removed (#56, 8).
    leaving: bool,
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
        self.toast_with(text, None, false, cx);
    }

    /// News that is not an error: a chat began or stopped listening (#77).
    pub(super) fn toast_info(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.toast_with(text, None, true, cx);
    }

    /// The news of `text` is over: its toast goes now.
    pub(super) fn toast_over(&mut self, text: &str, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self
            .toasts
            .list
            .iter()
            .filter(|t| t.text.as_ref() == text)
            .map(|t| t.id)
            .collect();
        for id in ids {
            self.dismiss(id, cx);
        }
    }

    /// "Draft deleted · Money.kt:23" with Undo, which puts `comment` back.
    pub(super) fn toast_deleted(
        &mut self,
        comment: crate::review::Comment,
        cx: &mut Context<Self>,
    ) {
        let name = comment.path.rsplit('/').next().unwrap_or(&comment.path);
        let text = format!("Draft deleted · {name}:{}", comment.line).into();
        self.toast_with(text, Some(comment), true, cx);
        cx.notify();
    }

    fn toast_with(
        &mut self,
        text: SharedString,
        undo: Option<crate::review::Comment>,
        info: bool,
        cx: &mut Context<Self>,
    ) {
        // Errors go in the app's log, for a report to carry (#118).
        if !info {
            crate::applog::line(&text);
        }
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
            undo,
            info,
            leaving: false,
            _timer: timer,
        });
        let over = self.toasts.list.len().saturating_sub(MOST);
        self.toasts.list.drain(..over);
    }

    /// Undo on toast `id`: its draft is back where it was.
    fn undo(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(comment) = self
            .toasts
            .list
            .iter_mut()
            .find(|t| t.id == id)
            .and_then(|t| t.undo.take())
        else {
            return;
        };
        self.comments.push(comment);
        self.comments.sort_by_key(|c| c.id);
        self.save_comments();
        self.refresh_rows();
        self.dismiss(id, cx);
    }

    /// It slides out, then it is gone.
    fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(toast) = self
            .toasts
            .list
            .iter_mut()
            .find(|t| t.id == id && !t.leaving)
        else {
            return;
        };
        toast.leaving = true;
        toast._timer = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(super::motion::BASE + Duration::from_millis(40))
                .await;
            this.update(cx, |this, cx| {
                this.toasts.list.retain(|t| t.id != id);
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    /// The stack, over everything else, under the title bar's right end.
    pub(super) fn render_toasts(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if self.toasts.list.is_empty() {
            return None;
        }
        let toasts = self.toasts.list.iter().map(|t| {
            let id = t.id;
            let leaving = t.leaving;
            let undo = t.undo.is_some();
            let info = t.info;
            let toast = div()
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
                .children((!info).then(|| {
                    icons::icon("alert")
                        .size(px(16.))
                        .mt(px(2.))
                        .text_color(theme::removed())
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(theme::text())
                        .child(super::keys::text(&t.text).into_owned()),
                )
                .children(undo.then(|| {
                    div()
                        .id(("toast-undo", id as usize))
                        .flex_none()
                        .px(px(6.))
                        .rounded(px(6.))
                        .text_color(theme::accent())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .child("Undo")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.undo(id, cx)))
                }))
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
                );
            // In from the right as it fades in, and out the same way (#56, 8).
            super::motion::toward(
                toast,
                ("toast-motion", id as usize),
                !leaving,
                true,
                super::motion::BASE,
                |toast, v| toast.relative().left(px(24. * (1. - v))).opacity(v),
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
