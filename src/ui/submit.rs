//! Submitting a review of a merge request, as GitLab offers it: Comment, Approve or Request changes,
//! with an optional summary, publishing one's pending comments with it. A card over the diff that
//! lists those pending comments, each a jump to its line.

use super::input::{self, Edit};
use super::keys::{self, On};
use super::px;
use super::{ISLAND_RADIUS, ROW_RADIUS, Workspace, plural, theme};
use crate::mr;
use gpui::{ClickEvent, Context, FontWeight, KeyDownEvent, MouseButton, div, prelude::*};

/// What the review says besides its comments.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Comment,
    Approve,
    RequestChanges,
}

const VERDICTS: [Verdict; 3] = [Verdict::Comment, Verdict::Approve, Verdict::RequestChanges];

impl Verdict {
    fn label(self) -> &'static str {
        match self {
            Verdict::Comment => "Comment",
            Verdict::Approve => "Approve",
            Verdict::RequestChanges => "Request changes",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Verdict::Comment => "Publish your comments, no verdict",
            Verdict::Approve => "Publish your comments and approve",
            Verdict::RequestChanges => "Publish your comments and ask for changes before it merges",
        }
    }
}

pub struct Submit {
    summary: String,
    verdict: Verdict,
    posting: bool,
    error: Option<String>,
}

impl Workspace {
    /// ⇧⌘↵ or "Submit review": the card, on Comment.
    pub(super) fn open_submit(&mut self, cx: &mut Context<Self>) {
        if self.requests.current.is_none() || !self.showing_request() {
            return;
        }
        self.reset_field_edit();
        self.submit = Some(Submit {
            summary: String::new(),
            verdict: Verdict::Comment,
            posting: false,
            error: None,
        });
        cx.notify();
    }

    pub(super) fn close_submit(&mut self, cx: &mut Context<Self>) {
        if self.submit.take().is_some() {
            cx.notify();
        }
    }

    /// The summary's text while the card is open.
    pub(super) fn review_summary(&self) -> Option<String> {
        self.submit.as_ref().map(|s| s.summary.clone())
    }

    /// Whether the token's user approved the current request already.
    fn approved_already(&self) -> bool {
        self.requests.approvals.as_ref().is_some_and(|a| a.mine)
    }

    pub(super) fn submit_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let Some(s) = self.submit.as_mut() else {
            return;
        };
        if s.posting {
            return;
        }
        if key.key == "enter" && key.modifiers.secondary() {
            return self.run_submit(cx);
        }
        if key.key == "tab" {
            let at = VERDICTS.iter().position(|v| *v == s.verdict).unwrap_or(0);
            let step = if key.modifiers.shift { 2 } else { 1 };
            s.verdict = VERDICTS[(at + step) % VERDICTS.len()];
            s.error = None;
            return cx.notify();
        }
        if key.key == "enter" {
            input::sync(&s.summary, &mut self.field_edit);
            self.field_edit.insert("\n");
            s.summary = self.field_edit.text().to_owned();
            return cx.notify();
        }
        if self.vertical_key(key, cx) {
            return;
        }
        let Some(s) = self.submit.as_mut() else {
            return;
        };
        match input::edit_area(&mut s.summary, &mut self.field_edit, key, cx) {
            Edit::Escape => return self.close_submit(cx),
            Edit::Changed => s.error = None,
            Edit::Selected | Edit::Enter { .. } | Edit::Ignored => {}
        }
        cx.notify();
    }

    /// Publishes the pending comments, then the summary, then the verdict; the card closes when
    /// all went through and says what failed when not.
    fn run_submit(&mut self, cx: &mut Context<Self>) {
        let Some(iid) = self.requests.current else {
            return;
        };
        let Some(remote) = self.web.clone() else {
            return;
        };
        let head = self
            .requests
            .list
            .iter()
            .find(|m| m.iid == iid)
            .map(|m| m.head.clone())
            .unwrap_or_default();
        let drafts = !self.requests.drafts.is_empty();
        let approve_already = self.approved_already();
        let Some(s) = self.submit.as_mut() else {
            return;
        };
        let summary = s.summary.trim().to_owned();
        let verdict = s.verdict;
        if !drafts && summary.is_empty() && verdict == Verdict::Comment {
            s.error =
                Some("Nothing to submit: write a summary, or approve or ask for changes.".into());
            return cx.notify();
        }
        s.posting = true;
        s.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let verdict = match verdict {
                        Verdict::Comment => mr::Verdict::Comment,
                        Verdict::Approve => mr::Verdict::Approve,
                        Verdict::RequestChanges => mr::Verdict::RequestChanges,
                    };
                    mr::submit(
                        &remote,
                        iid,
                        &head,
                        drafts,
                        &summary,
                        verdict,
                        approve_already,
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.submit = None,
                    Err(e) => {
                        if let Some(s) = this.submit.as_mut() {
                            s.posting = false;
                            s.error = Some(format!("{e:#}"));
                        }
                    }
                }
                // Whatever went through (the comments, say) shows either way.
                this.reload_threads(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_submit(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let s = self.submit.as_ref()?;
        let iid = self.requests.current?;
        let title = self
            .requests
            .list
            .iter()
            .find(|m| m.iid == iid)
            .map(|m| m.title.clone())
            .unwrap_or_default();
        let approved = self.approved_already();
        let option = |v: Verdict| {
            let on = s.verdict == v;
            let hint = if v == Verdict::Approve && approved {
                "You approved it already — the comments go out alone"
            } else {
                v.hint()
            };
            div()
                .id(("verdict", v as usize))
                .flex()
                .items_start()
                .gap(px(10.))
                .px(px(10.))
                .py(px(7.))
                .rounded(px(ROW_RADIUS))
                .cursor_pointer()
                .when(on, |s| s.bg(theme::selected()))
                .hover(|s| s.bg(theme::hover()))
                .child(
                    div()
                        .mt(px(3.))
                        .size(px(14.))
                        .flex_none()
                        .rounded_full()
                        .border_2()
                        .border_color(if on { theme::accent() } else { theme::faint() })
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(
                            on.then(|| div().size(px(6.)).rounded_full().bg(theme::accent())),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(v.label()))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child(hint),
                        ),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some(s) = this.submit.as_mut() {
                        s.verdict = v;
                        s.error = None;
                        cx.notify();
                    }
                }))
        };
        let pending = self.requests.drafts.iter().enumerate().map(|(ix, d)| {
            let target = d.path.clone().zip(d.new_line.or(d.old_line));
            let old = d.new_line.is_none();
            let place = match &target {
                Some((p, n)) => format!("{}:{n}", p.rsplit('/').next().unwrap_or(p)),
                None => "General".to_owned(),
            };
            div()
                .id(("submit-draft", ix))
                .flex()
                .items_baseline()
                .gap_2()
                .px(px(10.))
                .py(px(3.))
                .rounded(px(ROW_RADIUS))
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .child(
                    div()
                        .flex_none()
                        .font_family(theme::CODE_FONT)
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(place),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(self.one_line(&d.body)),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some((path, line)) = target.clone() {
                        this.submit = None;
                        this.jump_to_line(&path, line, old, cx);
                    }
                }))
        });
        let drafts = self.requests.drafts.len();
        let go = match (s.posting, s.verdict) {
            (true, _) => "Submitting…",
            (false, Verdict::Comment) => "Submit review",
            (false, Verdict::Approve) => "Submit and approve",
            (false, Verdict::RequestChanges) => "Submit, request changes",
        };
        let field = self.field_look(
            &s.summary,
            true,
            "Summary (optional) — posted as a comment on the merge request",
            input::Look {
                wrap: true,
                ..Default::default()
            },
            cx.entity().downgrade(),
        );
        Some(
            div()
                .id("submit-backdrop")
                .absolute()
                .size_full()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(72.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_submit(cx)),
                )
                .child(
                    div()
                        .occlude()
                        .w(px(520.))
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
                                .items_baseline()
                                .gap_2()
                                .min_w_0()
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Submit review"),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(theme::faint())
                                        .child(format!("{} {title}", super::words::number(iid))),
                                ),
                        )
                        .child(
                            div()
                                .px_2()
                                .py(px(5.))
                                .min_h(px(64.))
                                .flex()
                                .items_start()
                                .rounded(px(ROW_RADIUS))
                                .border_2()
                                .border_color(theme::focus())
                                .bg(theme::base())
                                .cursor_text()
                                .child(field),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .children(VERDICTS.map(option)),
                        )
                        .children((drafts > 0).then(|| {
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .px(px(10.))
                                        .pb(px(2.))
                                        .text_size(px(11.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme::muted())
                                        .child(
                                            format!(
                                                "{} go out with it",
                                                plural(drafts, "pending comment")
                                            )
                                            .to_uppercase(),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("submit-drafts")
                                        .max_h(px(160.))
                                        .overflow_y_scroll()
                                        .children(pending),
                                )
                        }))
                        .children(s.error.clone().map(|e| {
                            div()
                                .px_3()
                                .py_2()
                                .rounded(px(ROW_RADIUS))
                                .border_1()
                                .border_color(theme::removed())
                                .text_size(px(12.))
                                .text_color(theme::removed())
                                .child(e)
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .text_size(px(11.))
                                        .text_color(theme::faint())
                                        .child(keys::caps("⇥", On::Plain))
                                        .child("switches"),
                                )
                                .child(
                                    div()
                                        .id("submit-cancel")
                                        .px_2()
                                        .h(px(28.))
                                        .flex()
                                        .items_center()
                                        .rounded(px(ROW_RADIUS))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme::hover()))
                                        .child("Cancel")
                                        .tooltip(|_, cx| cx.new(|_| super::Tip("esc")).into())
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.close_submit(cx)
                                        })),
                                )
                                .child(
                                    div()
                                        .id("submit-go")
                                        .h(px(28.))
                                        .px(px(10.))
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .rounded(px(ROW_RADIUS))
                                        .bg(match s.verdict {
                                            Verdict::Approve => theme::added(),
                                            Verdict::RequestChanges => theme::removed(),
                                            Verdict::Comment => theme::warning(),
                                        })
                                        .text_color(theme::base())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .cursor_pointer()
                                        .when(s.posting, |s| s.opacity(0.6))
                                        .hover(|s| s.opacity(0.85))
                                        .child(go)
                                        .child(keys::caps("⌘↵", On::Filled))
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.run_submit(cx)
                                        })),
                                ),
                        ),
                ),
        )
    }
}
