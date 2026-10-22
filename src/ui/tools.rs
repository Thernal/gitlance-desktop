//! The right edge of the redesign (Design `design/shell/`, #49): an icon stripe that opens one tool
//! window at a time beside the page — only what is about it (#76): Code Outline · Agents, Branches
//! Agents, Merge requests Discussions · Agents, the Inbox nothing — and Submit review in the same
//! place (the bell's Notifications window retired into the Inbox, #72). The same icon or esc closes it; which one is open and its width are kept in
//! `layout.txt`. No stripe until a repository is open, none in zen.

use super::zones::Zone;
use super::{ROW_RADIUS, Split, Tip, TipOwned, Workspace, icons, island, keys, px, theme};
use gpui::{AnyElement, ClickEvent, Context, FontWeight, Rgba, SharedString, div, prelude::*};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// The declarations of the file read in Code (#76).
    Outline,
    Discussions,
    Agents,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Outline => "outline",
            Tool::Discussions => "discussions",
            Tool::Agents => "agents",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Tool::Outline, Tool::Discussions, Tool::Agents]
            .into_iter()
            .find(|t| t.name() == name)
    }

    fn icon(self) -> &'static str {
        match self {
            Tool::Outline => "list",
            Tool::Discussions => "message",
            Tool::Agents => "bot",
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Tool::Outline => "Outline",
            Tool::Discussions => "Discussions  ⌘⇧D",
            Tool::Agents => "Agents  ⇧⌘A",
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
    /// The tool windows of the space on screen (#76): Code Outline · Agents, Branches Agents,
    /// a request's page Discussions · Agents, the Inbox none.
    fn space_tools(&self) -> &'static [Tool] {
        use super::spaces::Space;
        match self.nav.space {
            Space::Inbox => &[],
            Space::Code => &[Tool::Outline, Tool::Agents],
            Space::Branches => &[Tool::Agents],
            Space::Requests if self.nav_page().is_some() => &[Tool::Discussions, Tool::Agents],
            Space::Requests => &[Tool::Agents],
        }
    }

    /// The tool window open now, when its space has it; a request's details are its Overview
    /// (#73).
    pub(super) fn tool(&self) -> Option<Tool> {
        self.root.as_ref()?;
        let tool = Tool::parse(&self.layout.tool)?;
        self.space_tools().contains(&tool).then_some(tool)
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
        if tool == Some(Tool::Agents) {
            self.refresh_chats();
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
        let button = |tool: Tool, badge: Option<(String, Rgba)>, what: String| {
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
                .children(self.space_tools().iter().map(|&tool| match tool {
                    Tool::Outline => button(tool, None, String::new()),
                    Tool::Discussions => button(
                        tool,
                        count(self.comments_total(), theme::accent()),
                        super::plural(self.comments_total(), "comment"),
                    ),
                    Tool::Agents => button(
                        tool,
                        count(drafts, theme::agent()),
                        super::plural(drafts, "draft"),
                    ),
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
                Tool::Outline => self.outline_tool(cx),
                Tool::Discussions => self.discussions_tool(cx),
                Tool::Agents => self.agents_tool(cx),
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
