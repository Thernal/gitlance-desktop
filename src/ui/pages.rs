//! The page of the space on screen (#71): the Inbox, Code's project or a file, the list of
//! branches or requests at their space's root, else a branch's or request's header and panes.
//! The roots are interim until their pages come: Code's is the files opened lately and Go to a
//! file (#75), the lists are today's, docked as a pane beside what is chosen (#72).

use super::spaces::Space;
use super::{Split, Workspace, gap, island, keys, motion, px, theme};
use gpui::{AnyElement, ClickEvent, Context, FontWeight, div, prelude::*};

/// The width of a root's list.
const LIST: f32 = 420.;

impl Workspace {
    pub(super) fn render_space(&self, cx: &mut Context<Self>) -> AnyElement {
        let column = div().flex_1().min_w_0().min_h_0().flex().flex_col();
        match (self.nav.space, self.nav_page()) {
            (Space::Inbox, _) => column
                .child(self.render_inbox_header(cx))
                .child(div().h(px(gap())))
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(self.render_inbox(cx))
                        .children(self.render_tool(cx)),
                ),
            (Space::Code, None) => column.child(self.render_code_root(cx)),
            (Space::Code, Some(_)) => column.child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_file_view(cx).unwrap_or_else(|| {
                        island().flex_1().bg(theme::editor()).into_any_element()
                    }))
                    .children(self.render_tool(cx)),
            ),
            (Space::Branches | Space::Requests, None) => column.child(self.render_root_list(cx)),
            (Space::Branches | Space::Requests, Some(_)) => self.render_item(column, cx),
        }
        .into_any_element()
    }

    /// A branch's or a request's page: its header, then the side panel, the diff and the tool window.
    fn render_item(&self, column: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        column
            .children(self.diff.as_ref().map(|diff| {
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(if self.zen.is_some() {
                        self.render_zen_head(diff).into_any_element()
                    } else if let Some(head) = self.render_create_header(cx) {
                        head
                    } else if let Some(head) = self.render_request_header(cx) {
                        head
                    } else {
                        self.render_header(diff, cx).into_any_element()
                    })
                    .child(div().h(px(gap())))
            }))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .children(self.render_side(cx).map(|side| {
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
                            move |pane, v| pane.max_w(px(room * v)),
                        )
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(self.render_diff(cx))
                            .children(self.render_zen_keys()),
                    )
                    .children(self.render_tool(cx)),
            )
    }

    /// Branches' or Merge requests' root: the list, and beside it what to do with it.
    fn render_root_list(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let requests = self.nav.space == Space::Requests;
        let list = if requests && self.requests.connectable {
            self.render_connect(cx).into_any_element()
        } else if requests {
            self.render_requests(cx).into_any_element()
        } else {
            self.render_branches(cx).into_any_element()
        };
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(gap()))
            .child(
                island()
                    .flex_none()
                    .w(px(LIST))
                    .border_color(self.zone_border(super::zones::Zone::Branches))
                    .child(self.render_branches_header(cx))
                    .child(list),
            )
            .child(if requests {
                self.render_choose_request().into_any_element()
            } else {
                self.render_choose_branch(cx).into_any_element()
            })
            .children(self.render_tool(cx))
    }

    /// Beside the list of branches: how to open one, and the working tree when it has changes.
    fn render_choose_branch(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let key = |k: &'static str, what: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(keys::caps(k, keys::On::Plain))
                .child(what)
        };
        let changes = self.wt.count;
        island()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .bg(theme::editor())
            .text_color(theme::muted())
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child("Choose a branch"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .text_size(px(12.))
                    .child(key("↑↓", "move"))
                    .child(key("↵", "open")),
            )
            .children((changes > 0).then(|| {
                div()
                    .id("root-working-tree")
                    .mt_2()
                    .px(px(10.))
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(super::ROW_RADIUS))
                    .border_1()
                    .border_color(theme::island_border())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                    .child(
                        super::icons::icon("files")
                            .size(px(14.))
                            .text_color(theme::muted()),
                    )
                    .child(format!(
                        "Working tree · {}",
                        super::plural(changes, "change")
                    ))
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.select_working_tree(cx)),
                    )
            }))
    }

    /// Code's root until it has the project's tree: Go to a file, and the files opened lately.
    fn render_code_root(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let name = self
            .root
            .as_ref()
            .and_then(|r| r.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
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
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name),
                )
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
