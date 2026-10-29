//! Merge, from the request's page (#164 B): in its bar beside Approve — quiet, with "N in the way",
//! while something is, its ▾ listing each reason with where it is fixed; Rebase where a rebase is
//! what is missing; "Merge when the pipeline succeeds…" while it runs, and Cancel once that is
//! set. Merge… opens the card before it goes: From → Into, what the host allows, the message. What
//! goes is the head this page shows, so a push nobody has seen is refused (`crate::merge`).

use super::discussion::{Kind, button};
use super::input::{self, Edit};
use super::keys;
use super::menu::{Act, Entry};
use super::typing::Input;
use super::{ISLAND_RADIUS, ROW_RADIUS, Workspace, icons, plural, px, theme};
use crate::merge::{self, Fix, Merge, Method, Squash, Stand};
use crate::mr::{self, MrState};
use gpui::{
    ClickEvent, Context, FontWeight, KeyDownEvent, MouseButton, Pixels, Point, SharedString, div,
    prelude::*,
};

/// The card before a merge goes.
pub struct MergeCard {
    /// The request it merges: never another one that came on the page meanwhile.
    pub iid: u64,
    pub merge: Merge,
    pub error: Option<String>,
    /// Refused for a push after the version on the page: the card offers the new one.
    pub moved: bool,
    /// Sent, and the host has not answered yet.
    pub busy: bool,
}

/// The width of the list of reasons: each says what and why on one line.
const REASONS_WIDTH: f32 = 400.;

impl Workspace {
    /// The open repository's host is GitHub: its words in what an action writes, before a frame
    /// has said them (`words::github` is the workspace drawn last).
    fn on_github(&self) -> bool {
        self.web.as_ref().is_some_and(|w| w.github)
    }

    /// A request's number as this repository's host writes it.
    fn reference(&self, iid: u64) -> String {
        format!("{}{iid}", if self.on_github() { '#' } else { '!' })
    }

    /// Where merging the request on the page stands: the host's word with what the page knows,
    /// fresher after an approval or a discussion resolved here.
    pub(super) fn merge_stand(&self) -> Option<Stand> {
        let iid = self.requests.current?;
        let m = self
            .requests
            .find(iid)
            .filter(|m| m.state == MrState::Opened)?;
        let host = self.requests.merge.as_ref()?;
        let open_threads = self
            .requests
            .threads
            .iter()
            .filter(|t| t.resolvable && !t.resolved)
            .count();
        Some(merge::stand(
            host,
            &merge::Known {
                draft: m.draft,
                approvals: self.requests.approvals.as_ref(),
                open_threads,
                pipeline: self.requests.pipeline.as_ref(),
                url: &m.url,
                target: &m.target,
            },
        ))
    }

    /// Reads where merging the current request stands again: after a rebase, or auto-merge set
    /// or cancelled.
    pub(super) fn reload_merge(&mut self, cx: &mut Context<Self>) {
        let (Some(iid), Some(remote)) = (self.requests.current, self.web.clone()) else {
            return;
        };
        self.requests.merge_task = Some(cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { mr::merge_state(&remote, iid) })
                .await;
            this.update(cx, |this, cx| {
                if this.requests.current == Some(iid) {
                    this.requests.merge_busy = None;
                    if let Ok(host) = read {
                        this.requests.merge = Some(host);
                    }
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Merge in the request's bar, as it stands; none while the host has not said.
    pub(super) fn render_merge_control(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let stand = self.merge_stand()?;
        let m = self.requests.find(self.requests.current?)?;
        let target = m.target.clone();
        let row = || div().flex_none().flex().items_center().gap(px(6.));
        let word = |text: String, color| {
            div()
                .flex_none()
                .text_size(px(12.))
                .text_color(color)
                .child(text)
        };
        if let Some(busy) = self.requests.merge_busy {
            return Some(
                word(busy.to_owned(), theme::muted())
                    .opacity(super::waiting::FADED)
                    .into_any_element(),
            );
        }
        let merge_icon = |color| icons::icon("merge").size(px(14.)).text_color(color);
        Some(match stand {
            Stand::InTheWay(blocks) => {
                let n = blocks.len();
                let tip: SharedString = blocks
                    .iter()
                    .map(|b| format!("{}: {}", b.what, b.detail))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into();
                div()
                    .id("bar-merge")
                    .flex_none()
                    .h(px(28.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(ROW_RADIUS))
                    .border_1()
                    .border_color(theme::island_border())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .child(merge_icon(theme::muted()))
                    .child("Merge")
                    .child(word(format!("{n} in the way"), theme::faint()))
                    .child(
                        icons::icon("chevron")
                            .size(px(13.))
                            .text_color(theme::faint()),
                    )
                    .tooltip_show_delay(super::hover::LONG)
                    .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                    .on_click(cx.listener(|this, e: &ClickEvent, _, cx| {
                        let at = e.position();
                        this.open_merge_reasons(Point::new(at.x - px(120.), at.y + px(18.)), cx)
                    }))
                    .into_any_element()
            }
            Stand::Checking => word(
                format!(
                    "Merge · {} is checking…",
                    if self.on_github() { "GitHub" } else { "GitLab" }
                ),
                theme::faint(),
            )
            .into_any_element(),
            Stand::Rebase(behind) => row()
                .child(
                    div()
                        .id("merge-behind")
                        .child(word(format!("{behind} behind {target}"), theme::muted()))
                        .tooltip_show_delay(super::hover::LONG)
                        .tooltip(move |_, cx| {
                            let tip = format!(
                                "{target} moved on since this branch left it. GitLab puts the commits on top of it; the pipeline runs again."
                            );
                            cx.new(|_| super::TipOwned(tip.into())).into()
                        }),
                )
                .child(
                    super::bar::bar_button("bar-rebase", "Rebase", "", true).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.rebase_request(cx)),
                    ),
                )
                .into_any_element(),
            Stand::Auto => row()
                .child(icons::icon("clock").size(px(14.)).text_color(theme::added()))
                .child(word(
                    "Merges when the pipeline succeeds".into(),
                    theme::added(),
                ))
                .child(
                    super::bar::bar_button("bar-merge-cancel", "Cancel", "", false).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_auto_merge(cx)),
                    ),
                )
                .into_any_element(),
            Stand::Running | Stand::Ready => {
                let label = match stand {
                    Stand::Running if !self.compact() => "Merge when the pipeline succeeds…",
                    _ => "Merge…",
                };
                super::bar::bar_button("bar-merge", label, "", true)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_merge(cx)))
                    .into_any_element()
            }
        })
    }

    /// Merge's ▾: each reason in the way, and where it is fixed.
    pub(super) fn open_merge_reasons(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(Stand::InTheWay(blocks)) = self.merge_stand() else {
            return;
        };
        let pipeline = self.requests.pipeline.as_ref().map(|p| p.url.clone());
        let github = self.on_github();
        let host = if github { "GitHub →" } else { "GitLab →" };
        let entries = blocks
            .into_iter()
            .map(|b| {
                let key = match &b.fix {
                    Fix::Nowhere => "",
                    Fix::Discussions => "Discussions →",
                    Fix::Page(url) if Some(url) == pipeline.as_ref() => "Pipeline →",
                    Fix::Page(_) => host,
                };
                Entry::new(b.what, Act::MergeFix(b.fix))
                    .detail(b.detail)
                    .key(key)
            })
            .collect();
        let title = if github {
            "In the way · GitHub asks again on every push"
        } else {
            "In the way · GitLab asks again on every push"
        };
        self.open_titled(at, vec![(Some(title), entries)], cx);
        if let Some(menu) = self.ctx_menu.as_mut() {
            menu.width = REASONS_WIDTH;
        }
    }

    /// Goes where a reason in the way is fixed.
    pub(super) fn go_fix(&mut self, fix: Fix, cx: &mut Context<Self>) {
        match fix {
            Fix::Nowhere => {}
            Fix::Discussions => self.set_tool(Some(super::tools::Tool::Discussions), cx),
            Fix::Page(url) => cx.open_url(&url),
        }
    }

    /// Merge… or ⌘K's Merge this merge request: the card, the project's defaults filled in; while
    /// something is in the way, the list of what.
    pub(super) fn open_merge(&mut self, cx: &mut Context<Self>) {
        let when = match self.merge_stand() {
            Some(Stand::Ready) => false,
            Some(Stand::Running) => true,
            Some(Stand::InTheWay(_)) => {
                let at = Point::new(px(self.win_width), px(84.));
                return self.open_merge_reasons(at, cx);
            }
            _ => return,
        };
        let (Some(iid), Some(host)) = (self.requests.current, self.requests.merge.as_ref()) else {
            return;
        };
        let Some(m) = self.requests.find(iid) else {
            return;
        };
        let message = format!("{} ({})", m.title, self.reference(iid));
        let merge = Merge::defaults(host, &m.head, message.clone(), when);
        self.reset_field_edit();
        self.field_edit = crate::textedit::TextEdit::new(message);
        self.merging = Some(MergeCard {
            iid,
            merge,
            error: None,
            moved: false,
            busy: false,
        });
        self.take_keyboard(Input::Merge);
        cx.notify();
    }

    /// Esc, Cancel or ×; not while the merge is on its way.
    pub(super) fn close_merge(&mut self, cx: &mut Context<Self>) {
        if self.merging.as_ref().is_some_and(|c| !c.busy) {
            self.merging = None;
            cx.notify();
        }
    }

    /// The card's keys: the message takes them, ↵ a new line; ⌘↵ merges.
    pub(super) fn merge_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let Some(card) = self.merging.as_mut() else {
            return;
        };
        if card.busy {
            return;
        }
        if key.key == "enter" && key.modifiers.secondary() {
            return self.run_merge(cx);
        }
        if key.key == "enter" {
            input::sync(&card.merge.message, &mut self.field_edit);
            self.field_edit.insert("\n");
            card.merge.message = self.field_edit.text().to_owned();
            return cx.notify();
        }
        if self.vertical_key(key, cx) {
            return;
        }
        let Some(card) = self.merging.as_mut() else {
            return;
        };
        match input::edit_area(&mut card.merge.message, &mut self.field_edit, key, cx) {
            Edit::Escape => return self.close_merge(cx),
            Edit::Changed => card.error = None,
            Edit::Selected | Edit::Enter { .. } | Edit::Ignored => {}
        }
        cx.notify();
    }

    /// Sends the merge; the card stays until the host answers, and says why it refused.
    fn run_merge(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.web.clone() else {
            return;
        };
        let Some(card) = self.merging.as_mut().filter(|c| !c.busy) else {
            return;
        };
        let iid = card.iid;
        card.busy = true;
        card.error = None;
        let merge = card.merge.clone();
        self.requests.merge_task = Some(cx.spawn(async move |this, cx| {
            let sent = merge.clone();
            let result = cx
                .background_executor()
                .spawn(async move { mr::merge(&remote, iid, &sent) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.merging = None;
                        this.merged(iid, &merge, cx);
                    }
                    Err(e) => {
                        if let Some(card) = this.merging.as_mut() {
                            let e = format!("{e:#}");
                            card.busy = false;
                            card.moved = merge::moved_on(&e);
                            card.error = Some(merge::refusal(&e));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// A push came after the version on the page: the card goes, the request is read again and
    /// its branch fetched, so the new head shows as its version.
    fn show_new_version(&mut self, cx: &mut Context<Self>) {
        self.merging = None;
        self.requests.merge = None;
        self.load_requests(cx);
        self.fetch_now(cx);
        cx.notify();
    }

    /// The host merged it, or set it to merge when the pipeline succeeds: a toast says which, and
    /// the list is read again — a request merged moves to Merged, as one merged elsewhere does.
    fn merged(&mut self, iid: u64, merge: &Merge, cx: &mut Context<Self>) {
        let here = self.requests.current == Some(iid);
        if merge.when_pipeline_succeeds {
            let text = format!("{} merges when the pipeline succeeds", self.reference(iid));
            self.toast_info(text.into(), cx);
            if let Some(host) = self.requests.merge.as_mut().filter(|_| here) {
                host.auto_set = true;
            }
            return self.reload_merge(cx);
        }
        let deletes = here
            && self
                .requests
                .merge
                .as_ref()
                .is_some_and(|h| h.deletes_itself);
        if let Some(m) = self.requests.find(iid) {
            let deleted = match merge.delete_branch || deletes {
                true => format!(" · {} deleted", m.source),
                false => String::new(),
            };
            let text = format!("{} merged into {}{deleted}", self.reference(iid), m.target);
            self.toast_info(text.into(), cx);
        }
        if here {
            self.requests.merge = None;
        }
        self.load_requests(cx);
    }

    /// Rebase: GitLab puts the commits on top of the target, and the pipeline runs again.
    fn rebase_request(&mut self, cx: &mut Context<Self>) {
        self.merge_call("Rebasing…", |remote, iid, _| mr::rebase(remote, iid), cx);
    }

    /// Cancel: it no longer merges when the pipeline succeeds.
    fn cancel_auto_merge(&mut self, cx: &mut Context<Self>) {
        self.merge_call("Cancelling…", mr::cancel_auto_merge, cx);
    }

    /// A call about merging the current request, `busy` said in Merge's place until it answers;
    /// then where it stands is read again.
    fn merge_call(
        &mut self,
        busy: &'static str,
        call: impl FnOnce(&crate::git::WebRemote, u64, &str) -> anyhow::Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let (Some(iid), Some(remote)) = (self.requests.current, self.web.clone()) else {
            return;
        };
        let node = self
            .requests
            .merge
            .as_ref()
            .map(|h| h.node.clone())
            .unwrap_or_default();
        self.requests.merge_busy = Some(busy);
        self.requests.merge_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { call(&remote, iid, &node) })
                .await;
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.requests.merge_busy = None;
                    this.error = Some(format!("{e:#}").into());
                    return cx.notify();
                }
                this.reload_merge(cx);
            })
            .ok();
        }));
        cx.notify();
    }

    /// The card before a merge goes, over everything.
    pub(super) fn render_merge_card(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let card = self.merging.as_ref()?;
        let iid = card.iid;
        let m = self.requests.find(iid)?;
        let host = self.requests.merge.clone().unwrap_or_default();
        let merge = &card.merge;
        let reference = self.reference(iid);
        let version = self
            .requests
            .versions
            .as_ref()
            .filter(|(of, _)| *of == iid)
            .and_then(|(_, v)| v.iter().find(|v| v.tip.to_string() == m.head))
            .map(|v| format!("v{}", v.number));
        let commits = self.commits.len();
        let seen = match &version {
            Some(v) => format!(" · {v}, the latest you have"),
            None => " · the latest you have".to_owned(),
        };
        let head = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .py(px(12.))
            .border_b_1()
            .border_color(theme::island_border())
            .child(icons::icon("merge").size(px(16.)).text_color(theme::text()))
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if merge.when_pipeline_succeeds {
                        format!("Merge {reference} when the pipeline succeeds")
                    } else {
                        format!("Merge {reference}")
                    }),
            )
            .child(
                div()
                    .id("merge-close")
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(ROW_RADIUS))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(icons::icon("x").size(px(14.)).text_color(theme::muted()))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_merge(cx))),
            );
        let route = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(mono(&m.source).text_color(theme::accent()))
            .child(
                icons::icon("forward")
                    .size(px(14.))
                    .text_color(theme::faint()),
            )
            .child(mono(&m.target))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::faint())
                    .child(format!("{}{seen}", plural(commits, "commit"))),
            );
        let label = |text: &'static str| {
            div()
                .w(px(112.))
                .flex_none()
                .pt(px(2.))
                .text_size(px(12.))
                .text_color(theme::muted())
                .child(text)
        };
        let field_row = || div().flex().items_start().gap(px(8.));
        let mut options = Vec::new();
        if host.github {
            let default = host.methods.first().copied();
            let chips = host.methods.iter().map(|&way| {
                let on = merge.method == way;
                div()
                    .id(SharedString::from(format!("merge-way-{}", way.key())))
                    .px(px(10.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .rounded(px(ROW_RADIUS))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .when(on, |s| {
                        s.bg(theme::selected())
                            .text_color(theme::text())
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!on, |s| {
                        s.text_color(theme::muted()).hover(|s| s.bg(theme::hover()))
                    })
                    .child(way.label())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if let Some(card) = this.merging.as_mut() {
                            card.merge.method = way;
                            cx.notify();
                        }
                    }))
            });
            options.push(
                field_row()
                    .child(label("How"))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(2.))
                                    .p(px(2.))
                                    .rounded(px(ROW_RADIUS + 2.))
                                    .border_1()
                                    .border_color(theme::island_border())
                                    .children(chips),
                            )
                            .children(default.map(|way| {
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme::faint())
                                    .child(format!(
                                        "The ways this repository allows; {} is first",
                                        way.label()
                                    ))
                            })),
                    )
                    .into_any_element(),
            );
        }
        let mut checks = Vec::new();
        if !host.github {
            let n = plural(commits.max(1), "commit");
            let squash = match host.squash {
                Squash::Never => None,
                Squash::Always => Some((true, true, "the project always squashes".to_owned())),
                Squash::Choose(default) => Some((
                    merge.squash,
                    false,
                    if merge.squash == default {
                        "the project's default".to_owned()
                    } else {
                        String::new()
                    },
                )),
            };
            if let Some((on, fixed, sub)) = squash {
                checks.push(
                    check(
                        "merge-squash",
                        on,
                        fixed,
                        format!("Squash the {n} into one"),
                        sub,
                    )
                    .when(!fixed, |s| {
                        s.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            if let Some(card) = this.merging.as_mut() {
                                card.merge.squash = !card.merge.squash;
                                cx.notify();
                            }
                        }))
                    })
                    .into_any_element(),
                );
            }
        }
        // GitHub's auto-merge deletes nothing unless the repository does.
        if !(host.github && merge.when_pipeline_succeeds && !host.deletes_itself) {
            let fixed = host.deletes_itself;
            let on = merge.delete_branch || fixed;
            let sub = if fixed {
                "the repository deletes it".to_owned()
            } else if !host.github && merge.delete_branch == host.delete_branch {
                "the project's default".to_owned()
            } else {
                String::new()
            };
            checks.push(
                check(
                    "merge-delete",
                    on,
                    fixed,
                    format!("Delete {} after", m.source),
                    sub,
                )
                .when(!fixed, |s| {
                    s.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        if let Some(card) = this.merging.as_mut() {
                            card.merge.delete_branch = !card.merge.delete_branch;
                            cx.notify();
                        }
                    }))
                })
                .into_any_element(),
            );
        }
        if !checks.is_empty() {
            options.push(
                field_row()
                    .child(label(if host.github { "" } else { "Options" }))
                    .child(div().flex().flex_col().gap(px(8.)).children(checks))
                    .into_any_element(),
            );
        }
        let active = self.has_keyboard(Input::Merge);
        let message = if host.github && merge.method == Method::Rebase {
            field_row().child(label("Commit message")).child(
                div()
                    .flex_1()
                    .pt(px(2.))
                    .text_size(px(12.))
                    .text_color(theme::faint())
                    .child("A rebase keeps each commit and its message: none to write."),
            )
        } else {
            let field = self.field_look(
                &merge.message,
                active,
                "Empty: the project's own message",
                input::Look {
                    wrap: true,
                    ..Default::default()
                },
                cx.entity().downgrade(),
            );
            field_row().child(label("Commit message")).child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .py(px(5.))
                    .min_h(px(84.))
                    .flex()
                    .items_start()
                    .rounded(px(ROW_RADIUS))
                    .border_2()
                    .border_color(if active {
                        theme::focus()
                    } else {
                        theme::island_border()
                    })
                    .bg(theme::base())
                    .cursor_text()
                    .child(field),
            )
        };
        let body =
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .px(px(16.))
                .py(px(14.))
                .child(route)
                .children(options)
                .child(message)
                .children(card.error.clone().map(|e| {
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_3()
                        .py_2()
                        .rounded(px(ROW_RADIUS))
                        .border_1()
                        .border_color(theme::removed())
                        .text_size(px(12.))
                        .text_color(theme::removed())
                        .child(div().flex_1().min_w_0().child(e))
                        .when(card.moved, |s| {
                            s.child(
                                button(
                                    "merge-new-version",
                                    "Show the new version",
                                    None,
                                    Kind::Plain,
                                )
                                .on_click(cx.listener(
                                    |this, _: &ClickEvent, _, cx| this.show_new_version(cx),
                                )),
                            )
                        })
                }));
        let after = version.unwrap_or_else(|| "this version".to_owned());
        let go = match (card.busy, merge.when_pipeline_succeeds) {
            (true, _) => "Merging…",
            (false, true) => "Merge when it succeeds",
            (false, false) => "Merge",
        };
        let foot = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .py(px(12.))
            .border_t_1()
            .border_color(theme::island_border())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .text_color(theme::faint())
                    .child(format!(
                        "Everyone sees it, and it cannot be undone. A push after {after} makes it refuse."
                    )),
            )
            .child(
                button("merge-cancel", "Cancel", Some("esc"), Kind::Quiet)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_merge(cx))),
            )
            .child(
                button(
                    "merge-go",
                    go,
                    (!card.busy).then_some("⌘↵"),
                    Kind::Primary(theme::accent()),
                )
                .when(card.busy, |s| s.opacity(super::waiting::FADED))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.run_merge(cx))),
            );
        Some(
            div()
                .id("merge-backdrop")
                .absolute()
                .size_full()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(72.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_merge(cx)),
                )
                .child(
                    div()
                        .id("merge-card")
                        .occlude()
                        .w(px(600.))
                        .max_h(gpui::relative(0.86))
                        .flex()
                        .flex_col()
                        .rounded(px(ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .text_size(px(13.))
                        .child(head)
                        .child(
                            div()
                                .id("merge-body")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .child(body),
                        )
                        .child(foot),
                ),
        )
    }
}

/// A checkbox with its label and, faint, what it is (`the project's default`); `fixed` the host's
/// rule, taking no click.
fn check(
    id: &'static str,
    on: bool,
    fixed: bool,
    text: String,
    sub: String,
) -> gpui::Stateful<gpui::Div> {
    let ink = if fixed {
        theme::faint()
    } else {
        theme::accent()
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .when(!fixed, |s| s.cursor_pointer())
        .child(
            div()
                .flex_none()
                .size(px(14.))
                .rounded(px(3.))
                .border_2()
                .border_color(if on { ink } else { theme::faint() })
                .when(on, |s| s.bg(ink))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.))
                .text_color(theme::base())
                .child(if on { "✓" } else { "" }),
        )
        .child(div().child(keys::text(&text).into_owned()))
        .when(!sub.is_empty(), |s| {
            s.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::faint())
                    .child(format!("· {sub}")),
            )
        })
}

fn mono(text: &str) -> gpui::Div {
    div()
        .font_family(super::theme::code_font())
        .text_size(px(12.))
        .child(text.to_owned())
}
