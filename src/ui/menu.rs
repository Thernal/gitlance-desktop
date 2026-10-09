//! A context menu at the pointer (right click on a commit, a file or a line; the diff's View
//! menu) and the actions its entries run: copy, open in the web or the editor, toggle a view option.

use super::{ISLAND_RADIUS, Opt, Workspace, row, theme};
use crate::editor;
use crate::storage::DiffMode;
use gpui::{ClipboardItem, Context, MouseButton, Pixels, Point, div, prelude::*, px};

#[derive(Clone)]
pub enum Act {
    Copy(String),
    OpenUrl(String),
    OpenEditor { path: String, line: u32 },
    Comment { old: bool, line: u32 },
    SetMode(DiffMode),
    Toggle(Opt),
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

/// Entries in groups; a rule is drawn between groups.
pub struct CtxMenu {
    pub at: Point<Pixels>,
    pub groups: Vec<Vec<Entry>>,
}

const ROW: f32 = 28.;
pub(super) const WIDTH: f32 = 260.;

impl Workspace {
    pub(super) fn open_menu(
        &mut self,
        at: Point<Pixels>,
        groups: Vec<Vec<Entry>>,
        cx: &mut Context<Self>,
    ) {
        let groups: Vec<_> = groups.into_iter().filter(|g| !g.is_empty()).collect();
        if !groups.is_empty() {
            self.repo_menu = false;
            self.ctx_menu = Some(CtxMenu { at, groups });
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
        }
        cx.notify();
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
        let height = rows as f32 * ROW + menu.groups.len() as f32 * 9. + 12.;
        let x = f32::from(menu.at.x)
            .min(f32::from(viewport.width) - WIDTH - 8.)
            .max(8.);
        let y = f32::from(menu.at.y)
            .min(f32::from(viewport.height) - height - 8.)
            .max(8.);
        let groups = menu.groups.iter().enumerate().map(|(g, entries)| {
            div()
                .flex()
                .flex_col()
                .when(g > 0, |s| {
                    s.child(div().h(px(1.)).my(px(4.)).bg(theme::island_border()))
                })
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
                        .child(div().flex_1().truncate().child(entry.label.clone()))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(entry.key),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.run_act(act.clone(), cx)))
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
                        .p(px(6.))
                        .flex()
                        .flex_col()
                        .rounded(px(ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .text_size(px(13.))
                        .children(groups),
                ),
        )
    }
}
