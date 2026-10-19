//! A context menu at the pointer (right click on a commit, a file or a line; the diff's View
//! menu) and the actions its entries run: copy, open in the web or the editor, toggle a view option.

use super::px;
use super::{ISLAND_RADIUS, Opt, Workspace, row, theme};
use crate::editor;
use crate::storage::DiffMode;
use gpui::{ClipboardItem, Context, MouseButton, Pixels, Point, div, prelude::*};

#[derive(Clone)]
pub enum Act {
    Copy(String),
    OpenUrl(String),
    OpenEditor {
        path: String,
        line: u32,
    },
    Comment {
        old: bool,
        line: u32,
    },
    SetMode(DiffMode),
    Toggle(Opt),
    CreateRequest(usize),
    Annotate,
    /// Settings → Diff → Mark reviewed as I scroll past, from the View menu.
    AutoReviewed,
    /// The menu of places folds to icons, or opens.
    FoldPlaces,
    /// A tool window on the right opens or closes.
    Tool(super::tools::Tool),
    /// The project rail's menu (#49): move a project, show its folder, a window of its own, close.
    MoveProject {
        from: usize,
        to: usize,
    },
    Reveal(std::path::PathBuf),
    NewWindow(std::path::PathBuf),
    CloseProject(usize),
    /// The agent comment being written (#50 F, its ▾): sent alone, to a chat picked, or copied.
    AgentSendOnly,
    AgentChooseChat,
    AgentCopy,
}

#[derive(Clone)]
pub struct Entry {
    pub label: String,
    pub key: &'static str,
    /// `Some` for a choice: whether it is on.
    pub checked: Option<bool>,
    pub act: Act,
}

impl Entry {
    pub fn new(label: impl Into<String>, act: Act) -> Self {
        Self {
            label: label.into(),
            key: "",
            checked: None,
            act,
        }
    }

    pub fn key(mut self, key: &'static str) -> Self {
        self.key = key;
        self
    }

    pub fn checked(mut self, on: bool) -> Self {
        self.checked = Some(on);
        self
    }
}

/// Entries in groups; a rule is drawn between groups, a group may have a heading.
pub struct CtxMenu {
    pub at: Point<Pixels>,
    pub groups: Vec<Vec<Entry>>,
    pub titles: Vec<Option<&'static str>>,
}

const ROW: f32 = 28.;
/// A group's heading row.
const HEAD: f32 = 26.;
/// The room a menu keeps from the window's edges (#48: menus stay inside the window).
const EDGE: f32 = 8.;
pub(super) const WIDTH: f32 = 260.;

impl Workspace {
    pub(super) fn open_menu(
        &mut self,
        at: Point<Pixels>,
        groups: Vec<Vec<Entry>>,
        cx: &mut Context<Self>,
    ) {
        let titled = groups.into_iter().map(|g| (None, g)).collect();
        self.open_titled(at, titled, cx);
    }

    /// A menu whose groups have headings (the View menu: Changes, Show, Files, Panels).
    pub(super) fn open_titled(
        &mut self,
        at: Point<Pixels>,
        groups: Vec<(Option<&'static str>, Vec<Entry>)>,
        cx: &mut Context<Self>,
    ) {
        let (titles, groups): (Vec<_>, Vec<_>) =
            groups.into_iter().filter(|(_, g)| !g.is_empty()).unzip();
        if !groups.is_empty() {
            self.repo_menu = false;
            self.ctx_menu = Some(CtxMenu { at, groups, titles });
            cx.notify();
        }
    }

    pub(super) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.ctx_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn run_act(&mut self, act: Act, cx: &mut Context<Self>) {
        self.ctx_menu = None;
        match act {
            Act::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            Act::OpenUrl(url) => cx.open_url(&url),
            Act::OpenEditor { path, line } => self.open_in_editor(&path, line),
            Act::Comment { old, line } => self.start_comment(old, line, cx),
            Act::SetMode(mode) => self.set_mode(mode, cx),
            Act::Toggle(opt) => self.toggle(opt, cx),
            Act::CreateRequest(ix) => self.open_create(Some(ix), cx),
            Act::Annotate => self.toggle_annotate(cx),
            Act::AutoReviewed => {
                self.settings.auto_reviewed = !self.settings.auto_reviewed;
                self.settings.save();
            }
            Act::FoldPlaces => self.toggle_places(cx),
            Act::Tool(tool) => self.toggle_tool(tool, cx),
            Act::Reveal(path) => cx.reveal_path(&path),
            Act::NewWindow(path) => cx.defer(move |cx| super::open_new_window(path, cx)),
            Act::AgentSendOnly => self.add_and_send(true, cx),
            Act::AgentChooseChat => self.add_and_pick_chat(cx),
            Act::AgentCopy => self.add_and_copy(cx),
            // Need the window: `run_act_in` runs them.
            Act::MoveProject { .. } | Act::CloseProject(_) => {}
        }
        cx.notify();
    }

    /// Runs `act` from a menu click, where the window is at hand for what the shell does.
    pub(super) fn run_act_in(
        &mut self,
        act: Act,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        match act {
            Act::MoveProject { from, to } => {
                self.ctx_menu = None;
                self.with_shell(window, cx, move |s, _, cx| s.move_tab(from, to, cx));
                cx.notify();
            }
            Act::CloseProject(ix) => {
                self.ctx_menu = None;
                self.with_shell(window, cx, move |s, window, cx| s.close(ix, window, cx));
                cx.notify();
            }
            act => self.run_act(act, cx),
        }
    }

    /// Opens `path` (relative to the repository) at `line` in the chosen editor.
    pub(super) fn open_in_editor(&mut self, path: &str, line: u32) {
        let (Some(root), Some(editor)) = (&self.root, editor::pick(self.settings.editor)) else {
            self.error = Some(
                "No supported editor found. Install Android Studio, Zed or VS Code, or choose one in Settings."
                    .into(),
            );
            return;
        };
        self.error = editor.open(&root.join(path), line).err().map(Into::into);
    }

    pub(super) fn render_ctx_menu(
        &self,
        viewport: gpui::Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let menu = self.ctx_menu.as_ref()?;
        let rows: usize = menu.groups.iter().map(Vec::len).sum();
        let heads = menu.titles.iter().flatten().count();
        let height = rows as f32 * ROW + heads as f32 * HEAD + menu.groups.len() as f32 * 9. + 12.;
        // Inside the window: moved back from an edge it would cross, and scrolling when taller
        // than the window has room for.
        let room = f32::from(viewport.height) - 2. * EDGE;
        let scrolls = height > room;
        let height = height.min(room);
        let x = f32::from(menu.at.x)
            .min(f32::from(viewport.width) - WIDTH - EDGE)
            .max(EDGE);
        let y = f32::from(menu.at.y)
            .min(f32::from(viewport.height) - height - EDGE)
            .max(EDGE);
        let groups = menu.groups.iter().enumerate().map(|(g, entries)| {
            let title = menu.titles.get(g).copied().flatten();
            div()
                .flex()
                .flex_col()
                .when(g > 0, |s| {
                    s.child(div().h(px(1.)).my(px(4.)).bg(theme::island_border()))
                })
                .children(title.map(|t| {
                    div()
                        .h(px(HEAD))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme::faint())
                        .child(t.to_uppercase())
                }))
                .children(entries.iter().enumerate().map(|(i, entry)| {
                    let act = entry.act.clone();
                    row(("ctx-entry", (g * 100 + i) as u64), false)
                        .h(px(ROW))
                        .gap_2()
                        .child(div().w(px(14.)).text_color(theme::accent()).child(
                            if entry.checked == Some(true) {
                                "✓"
                            } else {
                                ""
                            },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .child(super::keys::text(&entry.label).into_owned()),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(super::keys::text(entry.key).into_owned()),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.run_act_in(act.clone(), window, cx)
                        }))
                }))
        });
        Some(
            div()
                .absolute()
                .size_full()
                .child(
                    div()
                        .id("ctx-backdrop")
                        .absolute()
                        .size_full()
                        // Modal: a press on the View button must only close the menu, not reach
                        // the button and open it again.
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.close_menu(cx)),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, _, _, cx| this.close_menu(cx)),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        // The panel takes the mouse: otherwise the backdrop behind it sees the press
                        // first, closes the menu, and the click never reaches an entry.
                        .occlude()
                        .left(px(x))
                        .top(px(y))
                        .w(px(WIDTH))
                        .h(px(height))
                        .flex()
                        .flex_col()
                        .rounded(px(ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .text_size(px(13.))
                        .overflow_hidden()
                        .child(
                            div()
                                .id("ctx-scroll")
                                .flex_1()
                                .min_h_0()
                                .p(px(6.))
                                .flex()
                                .flex_col()
                                .when(scrolls, |s| s.overflow_y_scroll())
                                .children(groups),
                        ),
                ),
        )
    }
}
