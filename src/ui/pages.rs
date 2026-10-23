//! The page of the space on screen (#71), under its bar (`bar.rs`): the Inbox, Code's project or
//! a file, the list of branches or requests at their space's root (`roots.rs`, #72), else a
//! branch's or request's panes with its identity over the diff.
//! Code is the project's tree beside its folder page or a file's tabs (`code.rs`, #75).

use super::nav::Section;
use super::spaces::Space;
use super::{Split, Workspace, gap, island, keys, motion, px, theme};
use gpui::{AnyElement, ClickEvent, Context, div, prelude::*};

impl Workspace {
    pub(super) fn render_space(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match (self.nav.space, self.nav_page()) {
            (Space::Inbox, _) => div()
                .relative()
                .flex_1()
                .min_h_0()
                .flex()
                .child(self.render_inbox(cx))
                .children(self.render_tool(cx)),
            // The project's tree beside its folder page or a file's tabs (#75).
            (Space::Code, _) => self.render_code_space(cx),
            (Space::Branches, None) => div()
                .relative()
                .flex_1()
                .min_h_0()
                .flex()
                .child(self.render_branches_root(cx))
                .children(self.render_tool(cx)),
            (Space::Requests, None) => div()
                .relative()
                .flex_1()
                .min_h_0()
                .flex()
                .child(self.render_requests_root(cx))
                .children(self.render_tool(cx)),
            // A request's Overview and Commits, a branch's Commits, are pages of their own
            // (#73, #74).
            (Space::Requests | Space::Branches, Some(_))
                if let Some(tab @ (Section::Overview | Section::Commits)) = self.page_tab() =>
            {
                self.render_tab_page(tab, cx)
            }
            (Space::Branches | Space::Requests, Some(_)) => self.render_item(cx),
        };
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            // One bar over every page; zen has its own line instead.
            .children(self.render_page_bar(cx).map(|bar| {
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(bar)
                    .child(div().h(px(gap())))
            }))
            .children(
                self.diff
                    .as_ref()
                    .filter(|_| self.zen.is_some())
                    .map(|diff| {
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .child(self.render_zen_head(diff))
                            .child(div().h(px(gap())))
                    }),
            )
            .child(body)
            .into_any_element()
    }

    /// A branch's or a request's page under its bar: the side panel; the item's identity over the
    /// diff; the tool window.
    fn render_item(&self, cx: &mut Context<Self>) -> gpui::Div {
        let identity = self
            .diff
            .as_ref()
            .filter(|_| self.zen.is_none())
            .map(|diff| {
                if let Some(head) = self.render_create_header(cx) {
                    head
                } else if let Some(head) = self.render_request_line(cx) {
                    head
                } else if let Some(head) = self.render_branch_line() {
                    head
                } else {
                    self.render_header(diff, cx).into_any_element()
                }
            });
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .children(
                self.render_side(cx)
                    .filter(|_| !self.compact())
                    .map(|side| {
                        // It slides in and out at its width (#56, 4, 16).
                        let room = self.layout.sidebar + 12.;
                        motion::toward(
                            div()
                                .flex()
                                .flex_none()
                                .overflow_hidden()
                                .child(side)
                                .child(self.handle(Split::Sidebar, cx)),
                            "side-pane",
                            self.layout.show_sidebar,
                            self.motion.recent("side"),
                            motion::BASE,
                            // It fades as it narrows (the developer, 2026-10-23).
                            move |pane, v| pane.max_w(px(room * v)).opacity(v),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .children(identity.map(|identity| {
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .child(identity)
                            .child(div().h(px(gap())))
                    }))
                    .child(self.render_diff(cx))
                    .children(self.render_zen_keys()),
            )
            .children(self.render_tool(cx))
            // Narrow, the side panel opens over the content (#80).
            .children(
                self.side_drawer()
                    .then(|| self.render_side(cx))
                    .flatten()
                    .map(|side| self.drawer(side)),
            )
    }

    /// Code's folder page when it has no README: Go to a file, and the files opened lately.
    pub(super) fn render_code_root(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let recent: Vec<String> = self
            .recents
            .iter()
            .filter_map(|e| match &e.key {
                super::recents::Key::File(path) => Some(path.clone()),
                _ => None,
            })
            .take(8)
            .collect();
        island().flex_1().items_center().bg(theme::editor()).child(
            div()
                .w(px(440.))
                .mt(px(72.))
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .id("code-go-to-file")
                        .h(px(30.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(super::ROW_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .text_color(theme::faint())
                        .cursor_pointer()
                        .hover(|s| s.border_color(theme::border()))
                        .child(
                            super::icons::icon("search")
                                .size(px(14.))
                                .text_color(theme::faint()),
                        )
                        .child(div().flex_1().child("Go to a file"))
                        .child(keys::caps("⌘P", keys::On::Plain))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.search(super::palette::Scope::Files, cx)
                        })),
                )
                .children((!recent.is_empty()).then(|| {
                    div()
                        .mt_2()
                        .text_size(px(11.))
                        .text_color(theme::faint())
                        .child("Opened lately")
                }))
                .children(recent.into_iter().enumerate().map(|(ix, path)| {
                    let open = path.clone();
                    super::row(("code-recent", ix), false)
                        .h(px(28.))
                        .gap_2()
                        .child(
                            super::icons::icon("file")
                                .size(px(14.))
                                .text_color(theme::faint()),
                        )
                        .child(div().flex_1().min_w_0().truncate().child(path))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.open_file_view(open.clone(), cx)
                        }))
                })),
        )
    }
}
