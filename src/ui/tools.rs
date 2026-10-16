//! The right edge of the redesign (Design `design/shell/`, #49): an icon stripe that opens one tool
//! window at a time beside the diff — Discussions, Agents, Notifications — and Submit review in the
//! same place. The same icon or esc closes it; which one is open and its width are kept in
//! `layout.txt`. No stripe until a repository is open, none in zen.

use super::zones::Zone;
use super::{GAP, ROW_RADIUS, Split, Tip, TipOwned, Workspace, icons, island, keys, px, theme};
use gpui::{AnyElement, ClickEvent, Context, FontWeight, Rgba, SharedString, div, prelude::*};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Discussions,
    Agents,
    Notifications,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Discussions => "discussions",
            Tool::Agents => "agents",
            Tool::Notifications => "notifications",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Tool::Discussions, Tool::Agents, Tool::Notifications]
            .into_iter()
            .find(|t| t.name() == name)
    }

    fn icon(self) -> &'static str {
        match self {
            Tool::Discussions => "message",
            Tool::Agents => "bot",
            Tool::Notifications => "bell",
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Tool::Discussions => "Discussions  ⌘⇧R",
            Tool::Agents => "Agents",
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
        (tool != Tool::Notifications || self.requests.available).then_some(tool)
    }

    /// The window on the right is drawn: a tool, or Submit review.
    pub(super) fn tool_shown(&self) -> bool {
        self.zen.is_none() && (self.submit.is_some() || self.tool().is_some())
    }

    /// Opens `tool` (and gives it the keyboard), or closes the one open.
    pub(super) fn set_tool(&mut self, tool: Option<Tool>, cx: &mut Context<Self>) {
        let name = tool.map_or("", Tool::name);
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

    pub(super) fn toggle_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        let open = self.tool() == Some(tool);
        self.set_tool((!open).then_some(tool), cx);
    }

    /// The icon stripe on the right edge.
    pub(super) fn render_stripe(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if self.root.is_none() || self.zen.is_some() || self.settings_open {
            return None;
        }
        let open = self.tool();
        let button = |tool: Tool, badge: Option<(String, Rgba)>, dot: bool| {
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
                .tooltip(move |_, cx| {
                    cx.new(|_| TipOwned(keys::text(tool.tip()).into_owned().into()))
                        .into()
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
                .child(button(
                    Tool::Discussions,
                    count(self.comments_total(), theme::accent()),
                    false,
                ))
                .child(button(Tool::Agents, count(drafts, theme::agent()), false))
                .children(
                    self.requests
                        .available
                        .then(|| button(Tool::Notifications, None, self.notices.unread() > 0)),
                ),
        )
    }

    /// The tool window, after the splitter that resizes it.
    pub(super) fn render_tool(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.tool_shown() {
            return None;
        }
        let view = match self.submit_tool(cx) {
            Some(view) => view,
            None => match self.tool()? {
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
            .tooltip(|_, cx| cx.new(|_| Tip("Hide  esc")).into())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if submitting {
                    this.close_submit(cx)
                } else {
                    this.set_tool(None, cx)
                }
            }));
        Some(
            div()
                .flex_none()
                .flex()
                .child(self.handle(Split::Tool, cx))
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
                ),
        )
    }

    /// The gap between the diff and the stripe when no tool window is open.
    pub(super) fn stripe_gap(&self) -> Option<impl IntoElement + use<>> {
        (!self.tool_shown() && self.root.is_some() && self.zen.is_none() && !self.settings_open)
            .then(|| div().flex_none().w(px(GAP)))
    }
}

/// A small button in a tool window's header or foot.
pub fn tool_button(id: &'static str, label: impl Into<SharedString>) -> gpui::Stateful<gpui::Div> {
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
