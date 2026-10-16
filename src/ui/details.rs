//! A merge request's details (Design #8 D, `docs/specs/overview.md`): the Details tool window — the
//! facts on top as GitLab's sidebar has them, the description as markdown under them — opening by
//! itself the first time a request is opened (kept in `details-seen.txt`), and the request's own
//! header row over the panes: its title, who and where, the pipeline, approvals and Submit review.

use super::tools::{Tool, ToolView, tool_button};
use super::{ROW_RADIUS, Workspace, format, markdown, px, theme};
use crate::storage;
use gpui::{AnyElement, ClickEvent, Context, FontWeight, Rgba, div, prelude::*};

const SEEN: &str = "details-seen.txt";

impl Workspace {
    /// Opens Details the first time request `iid` of this project is opened; afterwards it is as
    /// the reviewer left it.
    pub(super) fn details_first_time(&mut self, iid: u64, cx: &mut Context<Self>) {
        let Some(base) = self.web.as_ref().map(|w| w.base.clone()) else {
            return;
        };
        let key = format!("{base}|{iid}");
        let seen = storage::read(SEEN);
        if seen.lines().any(|l| l == key) {
            return;
        }
        storage::save(SEEN, format!("{seen}{key}\n"));
        if self.tool().is_none() && self.submit.is_none() {
            self.set_tool(Some(Tool::Details), cx);
        }
    }

    /// The Details tool window.
    pub(super) fn details_tool(&self, cx: &mut Context<Self>) -> ToolView {
        let m = self
            .requests
            .current
            .and_then(|iid| self.requests.list.iter().find(|m| m.iid == iid));
        let Some(m) = m else {
            return ToolView {
                title: "Request details".into(),
                count: None,
                actions: Vec::new(),
                body: div().into_any_element(),
                foot: None,
            };
        };
        let d = self.requests.details.clone().unwrap_or_default();
        let approved = self
            .requests
            .approvals
            .as_ref()
            .map(|a| a.by.clone())
            .unwrap_or_default();
        // Who reviews: those asked, then whoever approved without being asked (GitHub drops an
        // approving reviewer from the asked ones).
        let mut reviewers = d.reviewers.clone();
        for name in &approved {
            if !reviewers.contains(name) {
                reviewers.push(name.clone());
            }
        }
        let fact = |label: &'static str, value: AnyElement| {
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .child(
                    div()
                        .w(px(84.))
                        .flex_none()
                        .text_color(theme::muted())
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_wrap()
                        .gap_x(px(10.))
                        .gap_y(px(4.))
                        .child(value),
                )
        };
        let person = |name: String, state: Option<(&'static str, Rgba)>| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(super::discussion::avatar(&name, false, 18.))
                .child(name)
                .children(state.map(|(word, color)| div().text_color(color).child(word)))
        };
        let mut facts: Vec<AnyElement> = Vec::new();
        facts.push(
            fact(
                "Author",
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(px(8.))
                    .child(person(m.author.clone(), None))
                    .child(div().text_color(theme::faint()).child(format!(
                        "opened {} · updated {}",
                        format::ago(m.created),
                        format::ago(m.updated)
                    )))
                    .into_any_element(),
            )
            .into_any_element(),
        );
        if !reviewers.is_empty() {
            facts.push(
                fact(
                    "Reviewers",
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .children(reviewers.into_iter().map(|r| {
                            let state = if approved.contains(&r) {
                                ("✓ approved", theme::added())
                            } else {
                                ("reviewing", theme::muted())
                            };
                            person(r, Some(state))
                        }))
                        .into_any_element(),
                )
                .into_any_element(),
            );
        }
        if !d.assignees.is_empty() {
            facts.push(
                fact(
                    "Assignee",
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .children(d.assignees.iter().cloned().map(|a| person(a, None)))
                        .into_any_element(),
                )
                .into_any_element(),
            );
        }
        if !d.labels.is_empty() {
            facts.push(
                fact(
                    "Labels",
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .children(d.labels.iter().map(|l| {
                            let color = l.color.map_or(theme::muted(), gpui::rgb);
                            div()
                                .px(px(8.))
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .rounded_full()
                                .border_1()
                                .border_color(Rgba { a: 0.45, ..color })
                                .bg(Rgba { a: 0.18, ..color })
                                .text_size(px(11.))
                                .text_color(theme::text())
                                .child(l.name.clone())
                        }))
                        .into_any_element(),
                )
                .into_any_element(),
            );
        }
        if let Some((title, due)) = &d.milestone {
            facts.push(
                fact(
                    "Milestone",
                    div()
                        .child(match due {
                            Some(due) => format!("{title} · due {due}"),
                            None => title.clone(),
                        })
                        .into_any_element(),
                )
                .into_any_element(),
            );
        }
        if let Some(p) = &self.requests.pipeline {
            let (word, color) = match p.status.as_str() {
                "success" => ("✓ passed".to_owned(), theme::added()),
                "failed" => (
                    p.failed.as_ref().map_or("✗ failed".to_owned(), |(job, _)| {
                        format!("✗ {job} failed")
                    }),
                    theme::removed(),
                ),
                "running" => ("● running".to_owned(), theme::warning()),
                other => (other.replace('_', " "), theme::muted()),
            };
            facts.push(
                fact(
                    "Pipeline",
                    div()
                        .flex()
                        .gap_2()
                        .child(div().text_color(color).child(word))
                        .children((p.updated > 0).then(|| {
                            div()
                                .text_color(theme::faint())
                                .child(format::ago(p.updated))
                        }))
                        .into_any_element(),
                )
                .into_any_element(),
            );
        }
        if let Some(a) = &self.requests.approvals {
            let text = if a.required > 0 {
                format!("{} of {}", a.required - a.left.min(a.required), a.required)
            } else if a.by.is_empty() {
                "none yet".to_owned()
            } else {
                a.by.join(", ")
            };
            facts.push(fact("Approvals", div().child(text).into_any_element()).into_any_element());
        }
        let description = if d.description.trim().is_empty() {
            div()
                .text_color(theme::muted())
                .child("No description.")
                .into_any_element()
        } else {
            markdown::body(
                &d.description,
                true,
                format!("details-{}", m.iid),
                self.web.as_ref().map(|w| w.base.as_str()),
                None,
            )
        };
        let url = m.url.clone();
        ToolView {
            title: "Request details".into(),
            count: None,
            actions: vec![
                tool_button(
                    "details-web",
                    super::words::say("Open in GitLab ↗").into_owned(),
                )
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&url)))
                .into_any_element(),
            ],
            body: div()
                .id("details-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(px(14.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .text_size(px(12.))
                .children(facts)
                .child(div().h(px(1.)).my(px(4.)).bg(theme::island_border()))
                .child(div().text_size(px(13.)).child(description))
                .into_any_element(),
            foot: None,
        }
    }

    /// The header row of a merge request: its title, author and branches, then the pipeline,
    /// approvals and Submit review.
    pub(super) fn render_request_header(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self
            .requests
            .current
            .filter(|_| self.showing_request())
            .and_then(|iid| self.requests.list.iter().find(|m| m.iid == iid))?;
        let pending = self.requests.drafts.len();
        Some(
            super::island()
                .flex_none()
                .flex_row()
                .items_center()
                .gap_3()
                .px_4()
                .py(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .min_w_0()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(theme::accent())
                                        .child(super::words::number(m.iid)),
                                )
                                .child(div().min_w_0().truncate().child(m.title.clone()))
                                .children(m.draft.then(|| {
                                    div()
                                        .flex_none()
                                        .text_size(px(12.))
                                        .text_color(theme::warning())
                                        .child("Draft")
                                })),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .min_w_0()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child(super::discussion::avatar(&m.author, false, 16.))
                                .child(div().flex_none().child(m.author.clone()))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .font_family(theme::CODE_FONT)
                                        .child(format!("{} → {}", m.source, m.target)),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(theme::faint())
                                        .child(format!("· updated {}", format::ago(m.updated))),
                                ),
                        ),
                )
                .children(self.render_pipeline_pill(cx))
                .children(self.render_approval_chip(cx))
                .child(
                    div()
                        .id("header-submit")
                        .flex_none()
                        .px(px(10.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(ROW_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .child(if pending > 0 {
                            format!("Submit review · {pending}")
                        } else {
                            "Submit review".to_owned()
                        })
                        .child(super::keys::caps("⇧⌘↵", super::keys::On::Plain))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_submit(cx))),
                )
                .into_any_element(),
        )
    }
}
