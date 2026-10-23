//! Submitting a review of a merge request, as GitLab offers it: Comment, Approve or Request changes,
//! with an optional summary, publishing one's pending comments with it. It opens in the tool window
//! on the right (Design flow 05 step 9), which stays while the diff scrolls: what goes out (the
//! pending comments, each a jump to its line), the verdict with what it does, the summary.

use super::input::{self, Edit};
use super::keys::{self, On};
use super::px;
use super::tools::ToolView;
use super::{ROW_RADIUS, Workspace, icons, plural, theme};
use crate::mr;
use gpui::{ClickEvent, Context, FontWeight, KeyDownEvent, div, prelude::*};

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
            Verdict::Comment => "no verdict",
            Verdict::Approve => "approves",
            Verdict::RequestChanges => "blocks merging",
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
        // `@` and `:` complete in the summary too (#51, #9).
        let mut summary = s.summary.clone();
        if self.field_completion_key(&mut summary, key) {
            if let Some(s) = self.submit.as_mut() {
                s.summary = summary;
            }
            return cx.notify();
        }
        let Some(s) = self.submit.as_mut() else {
            return;
        };
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
            .find(iid)
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

    /// Submit review, in the tool window while it is open.
    pub(super) fn submit_tool(&self, cx: &mut Context<Self>) -> Option<ToolView> {
        let s = self.submit.as_ref()?;
        let iid = self.requests.current?;
        let title = self
            .requests
            .find(iid)
            .map(|m| m.title.clone())
            .unwrap_or_default();
        // What Approve does to the approvals: "1 / 2 → 2 / 2" where rules count them.
        let approve = match &self.requests.approvals {
            Some(a) if a.mine => "approved already".to_owned(),
            Some(a) if a.required > 0 => {
                let have = a.required.saturating_sub(a.left);
                format!(
                    "{have} / {} → {} / {}",
                    a.required,
                    (have + 1).min(a.required),
                    a.required
                )
            }
            _ => Verdict::Approve.hint().to_owned(),
        };
        let option = |v: Verdict| {
            let on = s.verdict == v;
            let hint = if v == Verdict::Approve {
                approve.clone()
            } else {
                v.hint().to_owned()
            };
            div()
                .id(("verdict", v as usize))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .py(px(6.))
                .rounded(px(ROW_RADIUS))
                .cursor_pointer()
                .when(on, |s| s.bg(theme::selected()))
                .hover(|s| s.bg(theme::hover()))
                .child(
                    div()
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
                .child(div().flex_1().child(v.label()))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(hint),
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
                .px(px(6.))
                .py(px(2.))
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
                // The window stays: the comment is checked in the diff beside it.
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some((path, line)) = target.clone() {
                        this.jump_to_line(&path, line, old, cx);
                    }
                }))
        });
        let drafts = self.requests.drafts.len();
        let agent_drafts = self.agent_drafts().len();
        let go = match (s.posting, s.verdict) {
            (true, _) => "Submitting…",
            (false, Verdict::Comment) => "Submit review",
            (false, Verdict::Approve) => "Submit and approve",
            (false, Verdict::RequestChanges) => "Submit, request changes",
        };
        let field = self.field_look(
            &s.summary,
            true,
            "A summary, if you want one…",
            input::Look {
                wrap: true,
                ..Default::default()
            },
            cx.entity().downgrade(),
        );
        let body = div()
            .id("submit-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(14.))
            .py(px(12.))
            .flex()
            .flex_col()
            .gap_3()
            .text_size(px(13.))
            .child(
                div()
                    .truncate()
                    .text_size(px(12.))
                    .text_color(theme::faint())
                    .child(format!("{} {title}", super::words::number(iid))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px(px(4.))
                    .py(px(8.))
                    .rounded(px(ROW_RADIUS))
                    .bg(theme::base())
                    .child(
                        div()
                            .px(px(6.))
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(if drafts == 0 {
                                "No pending comments — the verdict and the summary go out alone."
                                    .to_owned()
                            } else {
                                format!(
                                    "{} — pending, only you see them until now",
                                    plural(drafts, "comment")
                                )
                            }),
                    )
                    .children(pending),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(VERDICTS.map(option)),
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
            .children(
                self.field_completion(&s.summary)
                    .map(|(_, choices)| self.render_completion(&choices, self.field_pick)),
            )
            .children((agent_drafts > 0).then(|| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .text_color(theme::muted())
                    .child(icons::icon("bot").size(px(14.)).text_color(theme::agent()))
                    .child(format!(
                        "Your {} not part of the review — they go to your agent, not to {}.",
                        if agent_drafts == 1 {
                            "agent draft is".to_owned()
                        } else {
                            format!("{agent_drafts} agent drafts are")
                        },
                        if super::words::github() {
                            "GitHub"
                        } else {
                            "GitLab"
                        },
                    ))
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
            }));
        let foot = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(11.))
                    .text_color(theme::faint())
                    .child(keys::caps("⇥", On::Plain))
                    .child("verdict"),
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
                    .tooltip_show_delay(crate::ui::hover::LONG)
                    .tooltip(|_, cx| cx.new(|_| super::Tip("esc")).into())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_submit(cx))),
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
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.run_submit(cx))),
            );
        Some(ToolView {
            title: "Submit review".into(),
            count: None,
            actions: Vec::new(),
            body: body.into_any_element(),
            foot: Some(foot.into_any_element()),
        })
    }
}
