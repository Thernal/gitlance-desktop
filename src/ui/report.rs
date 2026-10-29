//! Report a problem (#118): a tool window on the right, in every space — a title and a
//! description, what goes with them (the version always; the system, the log's last lines and the
//! window's picture ticked to begin with), the issue exactly as it goes, and Send: to the relay,
//! which files it in the project's private reports, or GitHub's new-issue page, filled in. An
//! agent's chat that hit an error drafts one (`report_problem`); it opens here, purple, and goes
//! only when sent. Below, the reports sent from here and what became of them.

use super::input::{self, Edit, Look};
use super::tools::{Tool, ToolView};
use super::{Workspace, icons, px, theme};
use crate::report::{self, Draft, Scrub, Sent};
use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, KeyDownEvent, MouseButton, SharedString, div,
    prelude::*,
};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Focus {
    #[default]
    Title,
    Description,
}

#[derive(Default)]
pub struct ReportView {
    pub draft: Draft,
    pub focus: Focus,
    /// The window as it was when the report was opened, before the report's window showed.
    picture: Option<Vec<u8>>,
    /// "macOS 15.1 · Mac15,3", read once.
    system: String,
    pub sending: bool,
    /// What the last send said: the id it was kept as, or why it was not.
    pub said: Option<Result<String, String>>,
    pub sent: Vec<Sent>,
    /// The extras have been ticked once: a later draft keeps what was unticked.
    ready: bool,
}

/// An extra of a report: its words, whether it goes, and why not when it can't.
struct Extra {
    id: &'static str,
    what: String,
    why: &'static str,
    on: bool,
    fixed: bool,
}

impl Workspace {
    /// The report gets ready to be written: the extras ticked, the system read, the window's
    /// picture taken now — before the report's own window is drawn over it.
    pub(super) fn prepare_report(&mut self) {
        let r = &mut self.report;
        if !r.ready {
            r.ready = true;
            r.draft.system = true;
            r.draft.log = true;
            r.draft.picture = true;
            r.sent = report::sent();
        }
        if r.system.is_empty() {
            r.system = report::system();
        }
        if !r.sending && !matches!(r.said, Some(Ok(_))) {
            r.picture = crate::platform::window_picture();
        }
        r.focus = Focus::Title;
        self.reset_field_edit();
    }

    /// Help → Report a problem, ⌘K: the tool window; with no repository open, GitHub's page.
    pub(super) fn open_report(&mut self, cx: &mut Context<Self>) {
        if self.root.is_none() {
            self.prepare_report();
            return self.report_in_browser(cx);
        }
        if self.tool() != Some(Tool::Report) {
            self.set_tool(Some(Tool::Report), cx);
        }
    }

    /// An agent's draft (`report_problem`, read with the agents' answers): it opens to be checked.
    pub(super) fn take_report_drafts(&mut self, cx: &mut Context<Self>) {
        let Some(bus) = crate::agents::Bus::new() else {
            return;
        };
        let Some(d) = bus.take_reports().pop() else {
            return;
        };
        self.prepare_report();
        let r = &mut self.report;
        r.said = None;
        r.draft.title = d.title;
        r.draft.description = d.description;
        r.draft.by = Some(d.by.clone());
        self.reset_field_edit();
        self.toast_info(
            format!("{} drafted a report — check it, then send it", d.by).into(),
            cx,
        );
        if self.root.is_some() {
            self.set_tool(Some(Tool::Report), cx);
        }
        cx.notify();
    }

    /// What a report must not carry here: the hosts of the remotes and of the tokens kept, the
    /// projects' names and paths.
    fn report_scrub(&self) -> Scrub {
        let mut hosts = Vec::new();
        let mut projects = Vec::new();
        if let Some(web) = &self.web {
            hosts.push(crate::mr::host(web));
            if let Some((_, path)) = web
                .base
                .split_once("://")
                .and_then(|(_, rest)| rest.split_once('/'))
            {
                projects.push(path.to_owned());
                projects.extend(path.rsplit('/').next().map(str::to_owned));
            }
        }
        if let Some(dir) = std::env::home_dir().map(|h| h.join(".config/gitlance/tokens"))
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let host = name
                    .strip_prefix("https___")
                    .or_else(|| name.strip_prefix("http___"))
                    .unwrap_or(&name);
                hosts.push(host.split('_').next().unwrap_or(host).to_owned());
            }
        }
        for path in self
            .root
            .iter()
            .chain(self.project.iter())
            .chain(&self.recent)
        {
            projects.extend(path.file_name().map(|n| n.to_string_lossy().into_owned()));
        }
        Scrub::new(hosts, projects)
    }

    /// The issue's body as it goes.
    fn report_body(&self) -> String {
        let r = &self.report;
        let log = if r.draft.log {
            crate::applog::tail(report::LOG_LINES)
        } else {
            Vec::new()
        };
        report::body(&r.draft, &self.report_scrub(), &r.system, &log)
    }

    fn report_title(&self) -> String {
        self.report_scrub().apply(self.report.draft.title.trim())
    }

    /// ⌘↵: to the relay, or GitHub's page when there is none.
    pub(super) fn send_report(&mut self, cx: &mut Context<Self>) {
        let title = self.report_title();
        if title.is_empty() {
            self.report.said = Some(Err("Give it a title.".to_owned()));
            self.report.focus = Focus::Title;
            return cx.notify();
        }
        let Some(relay) = report::relay() else {
            return self.report_in_browser(cx);
        };
        if self.report.sending {
            return;
        }
        let body = self.report_body();
        let picture = self
            .report
            .picture
            .clone()
            .filter(|_| self.report.draft.picture);
        let agent = self.report.draft.by.is_some();
        self.report.sending = true;
        self.report.said = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let t = title.clone();
            let result = cx
                .background_executor()
                .spawn(async move { report::send(&relay, &t, &body, agent, picture.as_deref()) })
                .await;
            this.update(cx, |this, cx| {
                let r = &mut this.report;
                r.sending = false;
                match result {
                    Ok(id) => {
                        r.sent.insert(
                            0,
                            Sent {
                                id: id.clone(),
                                at: crate::agents::now(),
                                title,
                                state: "received".to_owned(),
                                url: String::new(),
                            },
                        );
                        report::keep_sent(&r.sent);
                        r.said = Some(Ok(id));
                        r.draft.title.clear();
                        r.draft.description.clear();
                        r.draft.by = None;
                        r.picture = None;
                    }
                    Err(e) => {
                        crate::applog::line(&format!("report: {e:#}"));
                        r.said = Some(Err(format!("{e:#}")));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// GitHub's new-issue page of the project, filled in; the picture can't go that way, so it
    /// waits in Downloads to be dragged in.
    pub(super) fn report_in_browser(&mut self, cx: &mut Context<Self>) {
        let mut body = self.report_body();
        if self.report.draft.picture
            && let Some(png) = &self.report.picture
            && let Some(file) =
                std::env::home_dir().map(|h| h.join("Downloads/gitlance-report.png"))
            && std::fs::write(&file, png).is_ok()
        {
            body.push_str(
                "\n\n_A picture of the window is in Downloads as gitlance-report.png — drag it here._",
            );
        }
        let title = self.report_title();
        cx.open_url(&report::new_issue_url(&title, &body));
    }

    /// What became of the reports sent from here that are still on their way.
    pub(super) fn refresh_report_states(&mut self, cx: &mut Context<Self>) {
        let Some(relay) = report::relay() else {
            return;
        };
        let open: Vec<String> = self
            .report
            .sent
            .iter()
            .filter(|s| s.url.is_empty() && s.state != "closed")
            .map(|s| s.id.clone())
            .take(10)
            .collect();
        if open.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    open.into_iter()
                        .filter_map(|id| report::status(&relay, &id).ok().map(|s| (id, s)))
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                for (id, (state, url)) in read {
                    if let Some(s) = this.report.sent.iter_mut().find(|s| s.id == id) {
                        s.state = state;
                        s.url = url;
                    }
                }
                report::keep_sent(&this.report.sent);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The report has the keyboard: its window is open and focused, and it is being written.
    pub(super) fn report_typing(&self) -> bool {
        self.tool() == Some(Tool::Report)
            && self.zone == super::zones::Zone::Tool
            && !matches!(self.report.said, Some(Ok(_)))
    }

    /// Typing the report: ⇥ between title and description, ↵ from the title to the description
    /// and a new line in it, ⌘↵ sends, esc closes the window (what was written stays).
    pub(super) fn report_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if key.key == "enter" && key.modifiers.secondary() {
            return self.send_report(cx);
        }
        if key.key == "escape" {
            return self.set_tool(None, cx);
        }
        let r = &mut self.report;
        if key.key == "tab" {
            r.focus = match r.focus {
                Focus::Title => Focus::Description,
                Focus::Description => Focus::Title,
            };
            self.reset_field_edit();
            return cx.notify();
        }
        if key.key == "enter" && r.focus == Focus::Description {
            input::sync(&r.draft.description, &mut self.field_edit);
            self.field_edit.insert("\n");
            r.draft.description = self.field_edit.text().to_owned();
            return cx.notify();
        }
        let done = match r.focus {
            Focus::Title => input::edit(&mut r.draft.title, &mut self.field_edit, key, cx),
            Focus::Description => {
                input::edit_area(&mut r.draft.description, &mut self.field_edit, key, cx)
            }
        };
        if let Edit::Enter { .. } = done {
            self.report.focus = Focus::Description;
            self.reset_field_edit();
        }
        if matches!(self.report.said, Some(Err(_))) {
            self.report.said = None;
        }
        cx.notify();
    }

    fn report_focus(&mut self, focus: Focus, cx: &mut Context<Self>) {
        if self.report.focus != focus {
            self.report.focus = focus;
            self.reset_field_edit();
        }
        self.set_zone(super::zones::Zone::Tool, cx);
        cx.notify();
    }

    /// The extras a report can carry, as ticked.
    fn report_extras(&self) -> Vec<Extra> {
        let r = &self.report;
        let lines = crate::applog::tail(report::LOG_LINES).len();
        vec![
            Extra {
                id: "system",
                what: if r.system.is_empty() {
                    "The system and the machine".to_owned()
                } else {
                    r.system.clone()
                },
                why: "the system and the machine",
                on: r.draft.system,
                fixed: false,
            },
            Extra {
                id: "log",
                what: match lines {
                    0 => "The log (nothing in it yet)".to_owned(),
                    n if n < report::LOG_LINES => format!("The log, its {n} lines"),
                    n => format!("The last {n} lines of the log"),
                },
                why: "what the app met just before; paths and hosts taken out",
                on: r.draft.log && lines > 0,
                fixed: lines == 0,
            },
            Extra {
                id: "picture",
                what: if r.picture.is_some() {
                    "This window as a picture".to_owned()
                } else {
                    "This window as a picture (can't be taken here)".to_owned()
                },
                why: "the window only, as it was when this opened",
                on: r.draft.picture && r.picture.is_some(),
                fixed: r.picture.is_none(),
            },
        ]
    }

    fn toggle_extra(&mut self, id: &str, cx: &mut Context<Self>) {
        let d = &mut self.report.draft;
        match id {
            "system" => d.system = !d.system,
            "log" => d.log = !d.log,
            "picture" => d.picture = !d.picture,
            _ => {}
        }
        cx.notify();
    }

    /// The tool window's view.
    pub(super) fn report_tool(&self, cx: &mut Context<Self>) -> ToolView {
        let r = &self.report;
        let label = |text: &'static str| {
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::muted())
                .child(text)
        };
        let field =
            |id: &'static str, focus: Focus, text: &str, placeholder: &'static str, tall: bool| {
                let active = self.has_keyboard(super::typing::Input::Report) && r.focus == focus;
                div()
                    .id(id)
                    .px(px(10.))
                    .py(px(6.))
                    .when(tall, |s| s.min_h(px(120.)))
                    .flex()
                    .items_start()
                    .rounded(px(super::ROW_RADIUS))
                    .border_2()
                    .border_color(if active {
                        theme::focus()
                    } else {
                        theme::island_border()
                    })
                    .bg(theme::base())
                    .cursor_text()
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.report_focus(focus, cx)
                        }),
                    )
                    .child(self.field_look(
                        text,
                        active,
                        placeholder,
                        Look {
                            wrap: true,
                            ..Default::default()
                        },
                        cx.entity().downgrade(),
                    ))
            };
        let mut body: Vec<AnyElement> = Vec::new();
        if let Some(Ok(id)) = &r.said {
            body.push(note("check", theme::added(), format!("Sent — {id}")).into_any_element());
            body.push(
                div()
                    .child(
                        super::bar::bar_button("report-another", "Write another", "", false)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.report.said = None;
                                this.prepare_report();
                                this.report_focus(Focus::Title, cx);
                            })),
                    )
                    .into_any_element(),
            );
        } else {
            if let Some(by) = &r.draft.by {
                body.push(
                    note(
                        "bot",
                        theme::agent(),
                        format!("{by} drafted this from an error it hit."),
                    )
                    .into_any_element(),
                );
            }
            body.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(label("Title"))
                    .child(field(
                        "report-title",
                        Focus::Title,
                        &r.draft.title,
                        "In a few words",
                        false,
                    ))
                    .into_any_element(),
            );
            body.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(label("Description"))
                    .child(field(
                        "report-description",
                        Focus::Description,
                        &r.draft.description,
                        "What happened, and what you did just before",
                        true,
                    ))
                    .into_any_element(),
            );
            body.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(label("Sent with it"))
                    // The build always goes: said, not a box to tick (the developer, 2026-10-23).
                    .child(
                        div()
                            .py(px(4.))
                            .text_size(px(12.))
                            .child(report::version())
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .child("always — which build it is"),
                            ),
                    )
                    .children(self.report_extras().into_iter().map(|e| extra_row(e, cx)))
                    .into_any_element(),
            );
            body.push(self.report_preview().into_any_element());
            if let Some(Err(e)) = &r.said {
                body.push(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::removed())
                        .child(e.clone())
                        .into_any_element(),
                );
            }
        }
        if !r.sent.is_empty() {
            body.push(self.render_sent_reports(cx).into_any_element());
        }
        let writing = !matches!(r.said, Some(Ok(_)));
        let relay = report::relay().is_some();
        let foot = writing.then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .px(px(12.))
                .py(px(10.))
                .border_t_1()
                .border_color(theme::island_border())
                .child(div().text_size(px(11.)).text_color(theme::faint()).child(if relay {
                    "To the project's private reports."
                } else {
                    "The reports' relay is not set up yet: it opens GitHub's page of the project, filled in."
                }))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .when(relay, |s| {
                            s.child(
                                super::bar::bar_button("report-browser", "Open in the browser", "", false)
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.report_in_browser(cx)
                                    })),
                            )
                        })
                        .child(
                            super::bar::bar_button(
                                "report-send",
                                match (relay, r.sending) {
                                    (_, true) => "Sending…",
                                    (true, false) => "Send",
                                    (false, false) => "Open in the browser",
                                },
                                "⌘↵",
                                true,
                            )
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.send_report(cx))),
                        ),
                )
                .into_any_element()
        });
        ToolView {
            title: "Report a problem".into(),
            count: None,
            actions: Vec::new(),
            body: div()
                .id("report-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(14.))
                .p(px(12.))
                .children(body)
                .into_any_element(),
            foot,
        }
    }

    /// The issue as it goes: its title, then its body as text.
    fn report_preview(&self) -> impl IntoElement + use<> {
        let picture = self.report.draft.picture && self.report.picture.is_some();
        let mut body = self.report_body();
        if picture {
            body.push_str("\n\n+ the window's picture");
        }
        div()
            .flex()
            .flex_col()
            .rounded(px(super::ROW_RADIUS))
            .border_1()
            .border_color(theme::island_border())
            .bg(theme::panel())
            .child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .border_b_1()
                    .border_color(theme::island_border())
                    .text_size(px(11.))
                    .text_color(theme::muted())
                    .child(icons::icon("eye").size(px(13.)).text_color(theme::muted()))
                    .child("The issue as it goes")
                    .child(
                        div()
                            .text_color(theme::faint())
                            .child("· paths ~/…, hosts <host>, projects <project>"),
                    ),
            )
            .child(
                div()
                    .px(px(10.))
                    .pt(px(8.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(match self.report_title() {
                        t if t.is_empty() => "(no title yet)".to_owned(),
                        t => t,
                    }),
            )
            .child(
                div()
                    .px(px(10.))
                    .pt(px(6.))
                    .pb(px(10.))
                    .font_family(theme::code_font())
                    .text_size(px(11.))
                    .text_color(theme::muted())
                    .children(
                        body.lines()
                            .map(|l| div().min_h(px(15.)).child(SharedString::from(l.to_owned()))),
                    ),
            )
    }

    /// The reports sent from here, newest first, with what became of each.
    fn render_sent_reports(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::muted())
                    .child("Your reports"),
            )
            .children(self.report.sent.iter().take(20).enumerate().map(|(n, s)| {
                let url = s.url.clone();
                let state = match s.state.as_str() {
                    "" => "sent".to_owned(),
                    other => other.to_owned(),
                };
                let public = !url.is_empty();
                div()
                    .id(("report-sent", n))
                    .py(px(5.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(theme::island_border())
                    .text_size(px(12.))
                    .child(
                        div()
                            .flex_none()
                            .font_family(theme::code_font())
                            .text_color(theme::faint())
                            .child(s.id.clone()),
                    )
                    .child(
                        super::hover::cut(
                            gpui::SharedString::from(format!("report-title-{}", s.id)),
                            s.title.clone(),
                        )
                        .flex_1(),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::faint())
                            .child(super::format::ago(s.at)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px(px(6.))
                            .rounded_full()
                            .text_size(px(11.))
                            .bg(if public {
                                theme::info_bg()
                            } else {
                                theme::hover()
                            })
                            .text_color(if public {
                                theme::added()
                            } else {
                                theme::muted()
                            })
                            .child(if public {
                                format!("{state} ↗")
                            } else {
                                state
                            }),
                    )
                    .when(public, |s| {
                        s.cursor_pointer().hover(|s| s.bg(theme::hover())).on_click(
                            cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&url)),
                        )
                    })
            }))
    }
}

/// A line of what is said about the report: its icon in `tone`, the text.
fn note(icon: &'static str, tone: gpui::Rgba, text: String) -> gpui::Div {
    div()
        .flex()
        .items_start()
        .gap(px(8.))
        .px(px(10.))
        .py(px(8.))
        .rounded(px(super::ROW_RADIUS))
        .border_l(px(3.))
        .border_color(tone)
        .bg(gpui::Rgba { a: 0.12, ..tone })
        .text_size(px(12.))
        .child(icons::icon(icon).size(px(14.)).mt(px(1.)).text_color(tone))
        .child(div().flex_1().min_w_0().child(text))
}

/// One extra with its box; one that cannot go is greyed.
fn extra_row(e: Extra, cx: &mut Context<Workspace>) -> impl IntoElement + use<> {
    let id = e.id;
    div()
        .id(SharedString::from(format!("report-extra-{id}")))
        .py(px(4.))
        .flex()
        .items_start()
        .gap(px(8.))
        .text_size(px(12.))
        .when(!e.fixed, |s| {
            s.cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_extra(id, cx)))
        })
        .child(
            div()
                .flex_none()
                .mt(px(1.))
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border_2()
                .border_color(if e.on {
                    theme::accent()
                } else {
                    theme::faint()
                })
                .when(e.on, |s| s.bg(theme::accent()))
                .when(e.fixed, |s| s.opacity(0.55))
                .text_size(px(10.))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::base())
                .child(if e.on { "✓" } else { "" }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_color(if e.fixed && !e.on {
                            theme::faint()
                        } else {
                            theme::text()
                        })
                        .child(e.what),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child(e.why),
                ),
        )
}
