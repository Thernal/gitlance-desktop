//! The right edge of the redesign (Design `design/shell/`, #49): an icon stripe that opens one tool
//! window at a time beside the diff — Discussions, Agents, Notifications — and Submit review in the
//! same place. The same icon or esc closes it; which one is open and its width are kept in
//! `layout.txt`. No stripe until a repository is open, none in zen.

use super::zones::Zone;
use super::{ROW_RADIUS, Split, Tip, TipOwned, Workspace, icons, island, keys, px, theme};
use gpui::{AnyElement, ClickEvent, Context, FontWeight, Rgba, SharedString, div, prelude::*};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// A merge request's facts and description (#8); only while one is shown.
    Details,
    Discussions,
    Agents,
    Notifications,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Details => "details",
            Tool::Discussions => "discussions",
            Tool::Agents => "agents",
            Tool::Notifications => "notifications",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [
            Tool::Details,
            Tool::Discussions,
            Tool::Agents,
            Tool::Notifications,
        ]
        .into_iter()
        .find(|t| t.name() == name)
    }

    fn icon(self) -> &'static str {
        match self {
            Tool::Details => "info",
            Tool::Discussions => "message",
            Tool::Agents => "bot",
            Tool::Notifications => "bell",
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Tool::Details => "Request details  ⌘I",
            Tool::Discussions => "Discussions  ⌘⇧D",
            Tool::Agents => "Agents  ⇧⌘A",
            Tool::Notifications => "Notifications  ⌘⇧N",
        }
    }
}

/// What a tool window shows: a title with a faint count, buttons in its header, the body (which
/// scrolls itself) and an optional foot.
pub struct ToolView {
    pub title: SharedString,
    pub count: Option<String>,
    pub actions: Vec<AnyElement>,
    pub body: AnyElement,
    pub foot: Option<AnyElement>,
}

impl Workspace {
    /// The tool window open now; Notifications only where requests can be read.
    pub(super) fn tool(&self) -> Option<Tool> {
        self.root.as_ref()?;
        let tool = Tool::parse(&self.layout.tool)?;
        match tool {
            Tool::Notifications if !self.requests.available => None,
            Tool::Details if !self.showing_request() => None,
            tool => Some(tool),
        }
    }

    /// The window on the right is drawn: a tool, or Submit review.
    pub(super) fn tool_shown(&self) -> bool {
        self.zen.is_none() && (self.submit.is_some() || self.tool().is_some())
    }

    /// Opens `tool` (and gives it the keyboard), or closes the one open.
    pub(super) fn set_tool(&mut self, tool: Option<Tool>, cx: &mut Context<Self>) {
        let name = tool.map_or("", Tool::name);
        // Opened or closed, it slides (#56, 3); one tool for another is no motion.
        let was = self.tool();
        if was.is_some() != tool.is_some() {
            self.last_tool = was.or(tool);
            self.mark_motion("tool", cx);
        }
        if self.layout.tool != name {
            self.layout.tool = name.to_owned();
            self.layout.save();
        }
        match tool {
            Some(Tool::Notifications) => self.refresh_notices(cx),
            Some(Tool::Agents) => {
                self.refresh_chats();
            }
            _ => {}
        }
        if tool.is_some() {
            self.set_zone(Zone::Tool, cx);
        } else if self.zone == Zone::Tool {
            self.set_zone(
                if self.diff.is_some() {
                    Zone::Diff
                } else {
                    Zone::Commits
                },
                cx,
            );
        }
        cx.notify();
    }

    /// Under 1 280 px the tool window floats over the diff instead of taking its width.
    pub(super) fn tool_floats(&self) -> bool {
        self.win_width < 1280.
    }

    pub(super) fn toggle_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        let open = self.tool() == Some(tool);
        self.set_tool((!open).then_some(tool), cx);
    }

    /// The icon stripe on the right edge.
    pub(super) fn render_stripe(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let zen = self.zen.is_some() && !self.motion.recent("zen");
        if self.root.is_none() || zen || self.settings_open {
            return None;
        }
        let open = self.tool();
        // The long hover says what the count is (#53): "Discussions · 3 comments".
        let button = |tool: Tool, badge: Option<(String, Rgba)>, dot: bool, what: String| {
            let id = tool.name();
            let on = open == Some(tool);
            div()
                .id(id)
                .relative()
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(ROW_RADIUS))
                .group(id)
                .cursor_pointer()
                .when(on, |s| s.bg(theme::selected()))
                .hover(|s| s.bg(theme::hover()))
                .child(
                    icons::icon(tool.icon())
                        .text_color(if on { theme::text() } else { theme::muted() })
                        .group_hover(id, |s| s.text_color(theme::text())),
                )
                .children(badge.map(|(text, color)| {
                    div()
                        .absolute()
                        .right(px(1.))
                        .bottom(px(1.))
                        .min_w(px(14.))
                        .h(px(14.))
                        .px(px(3.))
                        .rounded_full()
                        .bg(color)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(9.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::base())
                        .child(text)
                }))
                .children(dot.then(|| {
                    div()
                        .absolute()
                        .top(px(6.))
                        .right(px(6.))
                        .size(px(7.))
                        .rounded_full()
                        .bg(theme::accent())
                }))
                .tooltip_show_delay(crate::ui::hover::LONG)
                .tooltip(move |_, cx| {
                    let tip = keys::text(tool.tip()).into_owned();
                    let tip = if what.is_empty() {
                        tip
                    } else {
                        format!("{tip}  ·  {what}")
                    };
                    cx.new(|_| TipOwned(tip.into())).into()
                })
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_tool(tool, cx)),
                )
        };
        let count = |n: usize, color: Rgba| (n > 0).then(|| (n.min(99).to_string(), color));
        let drafts = self.agent_drafts().len();
        Some(
            div()
                .flex_none()
                .w(px(44.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .pt(px(2.))
                .children(
                    self.showing_request()
                        .then(|| button(Tool::Details, None, false, String::new())),
                )
                .child(button(
                    Tool::Discussions,
                    count(self.comments_total(), theme::accent()),
                    false,
                    super::plural(self.comments_total(), "comment"),
                ))
                .child(button(
                    Tool::Agents,
                    count(drafts, theme::agent()),
                    false,
                    super::plural(drafts, "draft"),
                ))
                .children(self.requests.available.then(|| {
                    let unread = self.notices.unread();
                    button(
                        Tool::Notifications,
                        None,
                        unread > 0,
                        format!("{unread} new"),
                    )
                })),
        )
    }

    /// The tool window, after the splitter that resizes it.
    pub(super) fn render_tool(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        // Closed a moment ago, it is drawn on its way out (#56, 3).
        let shown = self.tool_shown();
        let leaving = !shown && self.zen.is_none() && self.motion.recent("tool");
        if !shown && !leaving {
            return None;
        }
        let view = match self.submit_tool(cx) {
            Some(view) => view,
            None => match self.tool().or(self.last_tool.filter(|_| leaving))? {
                Tool::Details => self.details_tool(cx),
                Tool::Discussions => self.discussions_tool(cx),
                Tool::Agents => self.agents_tool(cx),
                Tool::Notifications => self.notifications_tool(cx),
            },
        };
        let submitting = self.submit.is_some();
        let close = div()
            .id("tool-close")
            .size(px(26.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(ROW_RADIUS))
            .group("tool-close")
            .cursor_pointer()
            .hover(|s| s.bg(theme::hover()))
            .child(
                icons::icon("x")
                    .size(px(15.))
                    .text_color(theme::muted())
                    .group_hover("tool-close", |s| s.text_color(theme::text())),
            )
            .tooltip_show_delay(crate::ui::hover::LONG)
            .tooltip(|_, cx| cx.new(|_| Tip("Hide  esc")).into())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if submitting {
                    this.close_submit(cx)
                } else {
                    this.set_tool(None, cx)
                }
            }));
        // A narrow window has no room beside the diff: the window floats over its right edge.
        let over = self.tool_floats();
        let room = self.layout.tool_width + 12.;
        let pane = div()
            .flex_none()
            .flex()
            .overflow_hidden()
            .when(over, |s| {
                s.absolute()
                    .top(px(0.))
                    .bottom(px(0.))
                    .right(px(0.))
                    .shadow_lg()
            })
            .when(!over, |s| s.child(self.handle(Split::Tool, cx)))
            .child(
                island()
                    .w(px(self.layout.tool_width))
                    .h_full()
                    .border_color(self.zone_border(Zone::Tool))
                    .child(
                        div()
                            .flex_none()
                            .h(px(38.))
                            .pl(px(14.))
                            .pr(px(8.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .border_b_1()
                            .border_color(theme::island_border())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .flex_none()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(view.title),
                                    )
                                    .children(view.count.map(|c| {
                                        div().truncate().text_color(theme::faint()).child(c)
                                    })),
                            )
                            .children(view.actions)
                            .child(close),
                    )
                    .child(div().flex_1().min_h_0().flex().flex_col().child(view.body))
                    .children(view.foot.map(|foot| {
                        div()
                            .flex_none()
                            .px(px(14.))
                            .py(px(10.))
                            .border_t_1()
                            .border_color(theme::island_border())
                            .child(foot)
                    })),
            );
        // It comes out of the stripe and goes back into it; its content shows once it is half out.
        Some(super::motion::toward(
            pane,
            "tool-pane",
            shown,
            self.motion.recent("tool"),
            super::motion::BASE,
            move |pane, v| {
                pane.max_w(px(room * v))
                    .opacity(((v - 0.4) / 0.6).clamp(0., 1.))
            },
        ))
    }

    /// The gap between the page and the stripe, a tool window open or not: opening one moves
    /// nothing outside it, the header above included (#68).
    pub(super) fn stripe_gap(&self) -> Option<impl IntoElement + use<>> {
        (self.root.is_some() && self.zen.is_none() && !self.settings_open)
            .then(|| div().flex_none().w(px(super::gap())))
    }
}

/// A small button in a tool window's header or foot.
pub fn tool_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .h(px(24.))
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .rounded(px(ROW_RADIUS))
        .text_size(px(12.))
        .text_color(theme::muted())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
        .child(label.into())
}
