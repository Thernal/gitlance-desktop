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
    /// A link or a number copied, and a toast saying what (#162): `said` is `Link to !398`.
    CopySaid {
        text: String,
        said: String,
    },
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
    /// The menu of spaces folds to icons, or opens.
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
    /// A narrow composer's folded buttons (#129): Send all, Add to review, a markdown helper,
    /// Preview.
    AgentSendAll,
    AddToReview(u64),
    ComposeHelper(crate::markdown::Helper),
    ComposePreview,
    /// The theme being edited is built on this one (its extends ▾).
    ThemeBase(String),
    /// Code reads at a branch or commit, by name; None the working tree (#75).
    CodeRevision(Option<(String, git2::Oid)>),
    /// Open in's ▾ (#78): the page's file in this app, this once; its app kept for files with
    /// this extension (or let go); Settings, where the editor is chosen.
    OpenWith(super::open_in::App),
    /// A file or folder of Code's tree in this app (its right click).
    OpenPathWith(std::path::PathBuf, super::open_in::App),
    /// A note for the agent on a folder of Code's tree (its right click, #97).
    FolderNote(String),
    /// The card naming a project and giving it a hue, or naming a chat (`labeling.rs`).
    NameProject(std::path::PathBuf),
    NameChat {
        session: String,
        agent: String,
    },
    /// Code's tabs (A, 2026-10-23), by key — a folder's ends with a slash: one shown, a preview
    /// kept, one closed, every one but one (or all) closed.
    CodeTab(String),
    KeepCodeTab(String),
    CloseCodeTab(String),
    CloseCodeTabs(Option<String>),
    CloseCodeTabsRight(String),
    PinCodeTab(String),
    /// The requests list shows the open, the merged or the closed (#114).
    RequestView(crate::mr::MrState),
    AlwaysOpen(String),
    EditorSettings,
    /// Settings → Appearance → Fonts and size (#24): the interface's face or (`code`) the code's,
    /// None its default; Other… lists every face installed.
    Face {
        code: bool,
        face: Option<String>,
    },
    OtherFaces(bool),
    /// Set up's Code colours (#143): a code theme, None following the window.
    CodeTheme(Option<String>),
}

#[derive(Clone)]
pub struct Entry {
    pub label: String,
    pub key: &'static str,
    /// `Some` for a choice: whether it is on.
    pub checked: Option<bool>,
    /// Shown dimmed, taking no click: its label says why (a branch only on this Mac, #162).
    pub off: bool,
    /// Its label drawn in this face (a face's menu, #24).
    pub face: Option<gpui::SharedString>,
    pub act: Act,
}

impl Entry {
    pub fn new(label: impl Into<String>, act: Act) -> Self {
        Self {
            label: label.into(),
            key: "",
            checked: None,
            off: false,
            face: None,
            act,
        }
    }

    /// What cannot be done here, and why: `Copy link — only on this Mac`.
    pub fn off(label: impl Into<String>) -> Self {
        Self {
            off: true,
            ..Self::new(label, Act::Copy(String::new()))
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

    pub fn face(mut self, face: gpui::SharedString) -> Self {
        self.face = Some(face);
        self
    }
}

/// Entries in groups; a rule is drawn between groups, a group may have a heading.
#[derive(Clone)]
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
            self.ctx_menu = Some(CtxMenu { at, groups, titles });
            self.menu_leaving = None;
            cx.notify();
        }
    }

    pub(super) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.ctx_menu.is_some() {
            self.let_menu_go(cx);
            cx.notify();
        }
    }

    /// The menu closes; it fades on its way out (#56, 5).
    fn let_menu_go(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = self.ctx_menu.take() {
            self.menu_leaving = Some(menu);
            self.mark_motion("menu", cx);
        }
    }

    pub(super) fn run_act(&mut self, act: Act, cx: &mut Context<Self>) {
        self.let_menu_go(cx);
        match act {
            Act::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            Act::CopySaid { text, said } => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.toast_info(format!("{said} copied").into(), cx);
            }
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
            Act::FoldPlaces => self.toggle_menu(cx),
            Act::Tool(tool) => self.toggle_tool(tool, cx),
            Act::Reveal(path) => cx.reveal_path(&path),
            Act::NewWindow(path) => cx.defer(move |cx| super::open_new_window(path, cx)),
            Act::AgentSendOnly => self.add_and_send(true, cx),
            Act::AgentChooseChat => self.add_and_pick_chat(cx),
            Act::AgentCopy => self.add_and_copy(cx),
            Act::AgentSendAll => self.add_and_send(false, cx),
            Act::AddToReview(iid) => self.post_comment(iid, true, cx),
            Act::ComposeHelper(helper) => self.apply_helper(helper, cx),
            Act::ComposePreview => self.toggle_preview(cx),
            Act::ThemeBase(id) => self.set_theme_base(&id, cx),
            Act::CodeRevision(rev) => self.set_code_revision(rev, cx),
            Act::OpenWith(app) => self.open_with_app(app),
            Act::OpenPathWith(path, app) => self.open_path_with(&path, app),
            Act::RequestView(state) => self.set_request_view(state, cx),
            Act::FolderNote(path) => {
                self.open_folder_page(path.clone(), cx);
                self.start_folder_comment(&path, cx);
            }
            Act::CodeTab(key) => self.open_code_tab(&key, cx),
            Act::KeepCodeTab(key) => self.keep_code_tab(&key, cx),
            Act::CloseCodeTab(key) => self.close_code_tab(&key, cx),
            Act::CloseCodeTabs(keep) => self.close_code_tabs(keep, cx),
            Act::CloseCodeTabsRight(key) => self.close_code_tabs_right(key, cx),
            Act::PinCodeTab(key) => self.toggle_pin(&key, cx),
            Act::NameProject(path) => self.open_label(super::labeling::Target::Project(path), cx),
            Act::NameChat { session, agent } => {
                self.open_label(super::labeling::Target::Chat { session, agent }, cx)
            }
            Act::AlwaysOpen(ext) => self.always_open(ext),
            Act::EditorSettings => self.open_settings(cx),
            Act::Face { code, face } => self.choose_face(code, face, cx),
            Act::OtherFaces(code) => self.open_all_faces(code, cx),
            Act::CodeTheme(id) => self.choose_code_theme(id, cx),
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
                self.let_menu_go(cx);
                self.with_shell(window, cx, move |s, _, cx| s.move_tab(from, to, cx));
                cx.notify();
            }
            Act::CloseProject(ix) => {
                self.let_menu_go(cx);
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
    ) -> Option<gpui::AnyElement> {
        let Some(menu) = self.ctx_menu.as_ref() else {
            // Closed a moment ago: it fades out, taking no clicks.
            let menu = self
                .menu_leaving
                .as_ref()
                .filter(|_| self.motion.recent("menu"))?;
            let (panel, _) = self.menu_panel(menu, viewport, false, cx);
            return Some(
                super::motion::toward(
                    panel,
                    "menu-leaving",
                    false,
                    true,
                    super::motion::FAST,
                    |panel, v| panel.opacity(v),
                )
                .into_any_element(),
            );
        };
        let (panel, y) = self.menu_panel(menu, viewport, true, cx);
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
                // It drops 4 px into place as it fades in (#56, 5).
                .child(super::motion::appear(
                    panel,
                    "menu-coming",
                    super::motion::FAST,
                    move |panel, v| panel.top(px(y - 4. * (1. - v))).opacity(v),
                ))
                .into_any_element(),
        )
    }

    /// The menu's panel at its place, and its top; `live`, its entries take clicks.
    fn menu_panel(
        &self,
        menu: &CtxMenu,
        viewport: gpui::Size<Pixels>,
        live: bool,
        cx: &mut Context<Self>,
    ) -> (gpui::Div, f32) {
        let rows: usize = menu.groups.iter().map(Vec::len).sum();
        let heads = menu.titles.iter().flatten().count();
        let height = rows as f32 * ROW + heads as f32 * HEAD + menu.groups.len() as f32 * 9. + 12.;
        // Inside the window: moved back from an edge it would cross, and scrolling when taller
        // than the window has room for.
        // The pointer and the window in the interface's own units: `px` scales by the interface
        // size, so at 110 % a menu placed in window pixels landed a tenth too far (#87).
        let zoom = super::ui_zoom();
        let viewport = gpui::size(viewport.width / zoom, viewport.height / zoom);
        let at = gpui::point(menu.at.x / zoom, menu.at.y / zoom);
        let room = f32::from(viewport.height) - 2. * EDGE;
        let scrolls = height > room;
        let height = height.min(room);
        let x = f32::from(at.x)
            .min(f32::from(viewport.width) - WIDTH - EDGE)
            .max(EDGE);
        let y = f32::from(at.y)
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
                    let off = entry.off;
                    // One that cannot be done takes no hover: `row` gives it none as "selected",
                    // and its fill is taken away.
                    row(("ctx-entry", (g * 100 + i) as u64), off)
                        .h(px(ROW))
                        .gap_2()
                        .when(off, |s| {
                            s.bg(gpui::transparent_black())
                                .text_color(theme::faint())
                                .cursor_default()
                        })
                        .child(div().w(px(14.)).text_color(theme::accent()).child(
                            if entry.checked == Some(true) {
                                "✓"
                            } else {
                                ""
                            },
                        ))
                        .child(
                            super::hover::cut(
                                "label",
                                super::keys::text(&entry.label).into_owned(),
                            )
                            .when_some(entry.face.clone(), |s, face| s.font_family(face))
                            .flex_1(),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(super::keys::text(entry.key).into_owned()),
                        )
                        .when(live && !off, |s| {
                            s.on_click(cx.listener(move |this, _, window, cx| {
                                this.run_act_in(act.clone(), window, cx)
                            }))
                        })
                }))
        });
        let panel = div()
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
            );
        (panel, y)
    }
}
