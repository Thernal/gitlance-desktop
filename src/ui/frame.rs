//! The window's frame: the root view and what surrounds the diff — the title bar, the rail, the
//! welcome, the sidebar (versions and commits), zen's line and the splitters.

use super::*;
use hover::{CutInTip, Hover};
use typing::Input;

impl Workspace {
    /// Zen's line at the foot of the diff: the keys of a review, from the keyboard alone.
    pub(super) fn render_zen_keys(&self) -> Option<impl IntoElement + use<>> {
        (self.zen.is_some() || self.motion.recent("zen")).then_some(())?;
        // Keys are split by spaces: "[ ]" is two caps.
        let pair = |spec: &'static str, what: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .children(spec.split(' ').map(|key| keys::caps(key, keys::On::Plain)))
                .child(what)
        };
        Some(
            div()
                .flex_none()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_center()
                .gap(px(16.))
                .px(px(12.))
                .py(px(6.))
                .text_size(px(12.))
                .text_color(theme::muted())
                .child(pair("[ ]", "files"))
                .child(pair("v", "reviewed, next"))
                .child(pair("⌥↓", "next request"))
                .child(pair("c", "comment"))
                .child(pair(
                    "⌥⌘.",
                    if self.zen_one {
                        "every file"
                    } else {
                        "one file at a time"
                    },
                ))
                .child(pair("⌘.", "leave zen")),
        )
        .map(|keys| self.zen_strip("zen-keys", self.zen.is_some(), 40., keys))
    }

    /// A strip over or under the page that zen takes away or brings — the page's bar, the item's
    /// line, zen's own: it folds to nothing and fades as the sides slide (#56, 16; the developer,
    /// 2026-10-23: the top went at once). `room` is the most it is tall.
    pub(super) fn zen_strip(
        &self,
        id: &'static str,
        shown: bool,
        room: f32,
        strip: impl IntoElement + 'static,
    ) -> gpui::AnyElement {
        motion::toward(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(strip),
            id,
            shown,
            self.motion.recent("zen"),
            motion::BASE,
            move |s, v| s.max_h(px(room * v)).opacity(v),
        )
        .into_any_element()
    }

    pub(super) fn size(&mut self, split: Split) -> &mut f32 {
        match split {
            Split::Sidebar => &mut self.layout.sidebar,
            Split::Tool => &mut self.layout.tool_width,
            Split::List => &mut self.layout.list,
        }
    }

    pub(super) fn start_drag(
        &mut self,
        split: Split,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if event.click_count == 2 {
            *self.size(split) = match split {
                Split::Sidebar => Layout::default().sidebar,
                Split::Tool => Layout::default().tool_width,
                Split::List => Layout::default().list,
            };
            self.save_layout();
            cx.notify();
            return;
        }
        let origin = axis(split, event.position);
        let size = *self.size(split);
        self.drag = Some(Drag {
            split,
            origin,
            size,
        });
    }

    pub(super) fn drag_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            // The button was released outside the window.
            self.end_drag(cx);
            return;
        }
        let (min, max) = drag.split.bounds();
        // In the interface's units, as the widths are: the edge stays under the pointer at any
        // interface size.
        let moved = (axis(drag.split, event.position) - drag.origin) / super::ui_zoom();
        let moved = if drag.split == Split::Tool {
            -moved
        } else {
            moved
        };
        let size = (drag.size + moved).clamp(min, max);
        *self.size(drag.split) = size;
        cx.notify();
    }

    pub(super) fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            self.save_layout();
            cx.notify();
        }
    }

    /// The gap after a pane (right of it, or below when vertical), which resizes the pane.
    pub(super) fn handle(&self, split: Split, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let active = self.drag.is_some_and(|d| d.split == split);
        let group = match split {
            Split::Sidebar => "split-sidebar",
            Split::Tool => "split-tool",
            Split::List => "split-list",
        };
        let line = div()
            .rounded_full()
            .when(active, |s| s.bg(theme::splitter()))
            .group_hover(group, |s| s.bg(theme::splitter()));
        let gap = div()
            .group(group)
            .flex_none()
            .flex()
            .items_center()
            .justify_center();
        let gap = if split.vertical() {
            gap.h(px(handle_width()))
                .w_full()
                .cursor(CursorStyle::ResizeUpDown)
                .child(line.h(px(2.)).w(px(48.)))
        } else {
            gap.w(px(handle_width()))
                .h_full()
                .cursor(CursorStyle::ResizeLeftRight)
                .child(line.w(px(2.)).h(px(48.)))
        };
        gap.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event, _, cx| this.start_drag(split, event, cx)),
        )
    }

    // ---- rendering -----------------------------------------------------------------------

    /// The title bar is the window's, never the page's (#70): the menu's fold, back and
    /// forward, the project, search and Settings — and the project's own quiet words, the fetch
    /// and the refresh status, before the gear.
    pub(super) fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let repo = self
            .root
            .as_deref()
            .map_or_else(|| "GitLance".to_owned(), crate::labels::project_name);
        // In Tabs mode the tabs take the project's place (D21).
        let tabs = self.render_title_tabs(cx);
        let tabbed = tabs.is_some();
        let repo = tabs.is_none().then_some(repo);
        div()
            .h(px(TITLE_BAR))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .pl(px(crate::platform::TITLEBAR_INSET))
            .pr(px(GAP))
            .child(
                div()
                    .flex()
                    .gap(px(2.))
                    .children(self.root.is_some().then(|| {
                        nav_button(
                            "fold-places",
                            "sidebar",
                            if self.menu_folded() {
                                "Expand the menu  ⌘B"
                            } else {
                                "Collapse the menu  ⌘B"
                            },
                            true,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(cx)))
                    }))
                    .child(
                        nav_button("go-back", "back", "Back  ⌘[", self.nav.can_back())
                            .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
                    )
                    .child(
                        nav_button(
                            "go-forward",
                            "forward",
                            "Forward  ⌘]",
                            self.nav.can_forward(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx))),
                    ),
            )
            .children(tabs)
            .children(repo.map(|repo| {
                // The repository is a menu: open, refresh and the recent repositories.
                div()
                    .id("repo-menu")
                    // Its right click, the rail's: name and colour, show, a window, close.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, e: &MouseDownEvent, _, cx| {
                            let tabs = this.tabs.borrow().clone();
                            if let Some((ix, tab)) = tabs.iter().enumerate().find(|(_, t)| t.active)
                            {
                                this.project_menu(ix, tabs.len(), tab, e.position, cx)
                            }
                        }),
                    )
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(28.))
                    .px_2()
                    .rounded(px(ROW_RADIUS))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .child(repo)
                    .child(icons::icon("chevron").text_color(theme::muted()))
                    // Open a repository (⌘T): the projects filtered as you type, a path, Browse
                    // (the developer, 2026-10-23: its old menu took no typing).
                    .on_click(
                        cx.listener(|this, _, _, cx| {
                            this.open_palette(palette::Kind::Projects, cx)
                        }),
                    )
            }))
            .child(
                // The empty part of the bar: a double click zooms the window, as in other apps.
                // Search sits in its middle (at its end beside the tabs) and gives way first when
                // the window is narrow; the buttons on the right stay.
                div()
                    .id("title-bar")
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .h_full()
                    .flex()
                    .items_center()
                    .when(tabbed, |s| s.justify_end())
                    .when(!tabbed, |s| s.justify_center())
                    .on_click(|event, window, _| {
                        if event.click_count() == 2 {
                            window.titlebar_double_click();
                        }
                    })
                    // Narrow, search is its icon (#80).
                    .children((self.root.is_some() && self.compact()).then(|| {
                        icon_button("title-search-icon", "search", "Search or run a command  ⌘K")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.search(palette::Scope::All, cx)),
                            )
                    }))
                    .children((self.root.is_some() && !self.compact()).then(|| {
                        div()
                            .id("title-search")
                            .flex_shrink(1.)
                            .min_w(px(40.))
                            .w(px(if tabbed { 220. } else { 360. }))
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(10.))
                            .overflow_hidden()
                            .rounded(px(ROW_RADIUS))
                            .border_1()
                            .border_color(theme::island_border())
                            .bg(theme::panel())
                            .text_size(px(12.))
                            .text_color(theme::faint())
                            .cursor_pointer()
                            .hover(|s| s.border_color(theme::border()))
                            .child(
                                icons::icon("search")
                                    .size(px(14.))
                                    .text_color(theme::faint()),
                            )
                            .child(
                                super::hover::cut("search-hint", "Search or run a command")
                                    .flex_1(),
                            )
                            .child(keys::caps("⌘K", keys::On::Plain))
                            // A double click here is not the bar's: the window keeps its size.
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.search(palette::Scope::All, cx)),
                            )
                    })),
            )
            // The right end never gives way.
            .child(
                div()
                    .flex_none()
                    .min_w(px(if tabbed { 0. } else { 180. }))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_3()
                    .children(self.render_update_mark(cx))
                    .children(self.render_fetch(cx))
                    .children(self.render_status(cx))
                    .child(
                        icon_button("settings-open", "settings", "Settings  ⌘,")
                            .on_click(cx.listener(|this, _, _, cx| this.open_settings(cx))),
                    ),
            )
    }

    /// The menu under the repository name, over a backdrop that closes it.
    /// The versions, newest first, for the side panel's Versions section.
    pub(super) fn version_rows(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let latest = self.versions.len() - 1;
        let (from, to) = match self.selection {
            Selection::Versions { from, to } => (Some(from), Some(to)),
            _ => (None, None),
        };
        // Who made a version is only worth a line when more than one person did.
        let several = self
            .versions
            .iter()
            .any(|v| v.author != self.versions[0].author);
        div()
            .flex()
            .flex_col()
            .children(self.versions.iter().enumerate().rev().map(|(ix, v)| {
                let role = if Some(ix) == from {
                    Some("from")
                } else if Some(ix) == to {
                    Some("to")
                } else {
                    None
                };
                let chosen = role.is_some();
                let kind = match format::reason(&v.reason) {
                    "fetch" => "fetched",
                    "unknown" => "version",
                    k => k,
                };
                let mut detail: Vec<String> = Vec::new();
                if several {
                    detail.push(v.author.clone());
                }
                detail.push(format::ago(v.time));
                match ix.checked_sub(1).and_then(|p| self.versions.get(p)) {
                    None => detail.push(plural(v.commits, "commit")),
                    Some(prev) => {
                        if prev.base != v.base {
                            detail.push(format!(
                                "onto {}",
                                v.base.map(format::short).unwrap_or_default()
                            ));
                        }
                        detail.push(match v.commits.cmp(&prev.commits) {
                            std::cmp::Ordering::Greater => {
                                format!("+{}", plural(v.commits - prev.commits, "commit"))
                            }
                            std::cmp::Ordering::Less => {
                                format!("−{}", plural(prev.commits - v.commits, "commit"))
                            }
                            std::cmp::Ordering::Equal => {
                                format!("same {}", plural(v.commits, "commit"))
                            }
                        });
                    }
                }
                // The rail: a dot per version and a line to the older one.
                let rail = div()
                    .flex_none()
                    .relative()
                    .w(px(14.))
                    .h(px(44.))
                    .when(ix > 0, |s| {
                        s.child(
                            div()
                                .absolute()
                                .left(px(6.5))
                                .top(px(18.))
                                .w(px(1.))
                                .h(px(30.))
                                .bg(theme::selected()),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(3.))
                            .top(px(13.))
                            .size(px(8.))
                            .rounded_full()
                            .bg(if chosen {
                                theme::accent()
                            } else {
                                theme::faint()
                            }),
                    );
                let item = row(("version", ix), chosen)
                    // A long hover: who pushed it, exactly when, and what moved the branch (#53).
                    .tip(format!(
                        "v{} · {} · {} · {}",
                        v.number,
                        v.author,
                        hover::full_date(v.time),
                        if v.reason.is_empty() {
                            kind
                        } else {
                            v.reason.as_str()
                        }
                    ))
                    .h(px(44.))
                    .items_start()
                    .gap_2()
                    .child(rail)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .h_full()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::accent())
                                            .child(format!("v{}", v.number)),
                                    )
                                    .child(kind.to_owned())
                                    .child(
                                        div()
                                            .font_family(theme::CODE_FONT)
                                            .text_size(px(11.))
                                            .text_color(theme::muted())
                                            .child(format::short(v.tip)),
                                    )
                                    .child(div().flex_1())
                                    .children(role.map(|r| tag(r, theme::accent())))
                                    .when(ix == latest && role.is_none(), |s| {
                                        s.child(tag("latest", theme::faint()))
                                    })
                                    .when(
                                        self.watch.new_from.is_some_and(|from| ix >= from),
                                        |s| s.child(tag("new", theme::accent())),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .cut_in_tip()
                                    .text_size(px(11.))
                                    .text_color(theme::muted())
                                    .child(detail.join(" · ")),
                            ),
                    )
                    .on_click(
                        cx.listener(move |this, event, _, cx| this.click_version(ix, event, cx)),
                    );
                div().px(px(6.)).py(px(1.)).child(item)
            }))
    }

    /// The commits of two versions, paired; a modified pair opens its interdiff.
    pub(super) fn render_pairs(&self, diff: &Diff, cx: &mut Context<Self>) -> impl IntoElement {
        island()
            .flex_1()
            .min_w_0()
            .bg(theme::editor())
            .child(island_label(
                "Commits · = unchanged  ! modified  + new  − dropped",
            ))
            .child(
                div()
                    .id("pairs")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(px(6.))
                    .pb(px(6.))
                    .children(diff.pairs.iter().enumerate().map(|(ix, p)| {
                        let (mark, color, label) = match p.kind {
                            PairKind::Unchanged => ("=", theme::muted(), "unchanged"),
                            PairKind::Modified => ("!", theme::warning(), "modified"),
                            PairKind::New => ("+", theme::added(), "new"),
                            PairKind::Dropped => ("−", theme::removed(), "dropped"),
                        };
                        let ids = match (&p.old, &p.new) {
                            (Some(o), Some(n)) => {
                                format!("{} → {}", format::short(o.id), format::short(n.id))
                            }
                            (Some(o), None) => format::short(o.id),
                            (None, Some(n)) => {
                                format!("{}   → {}", " ".repeat(7), format::short(n.id))
                            }
                            (None, None) => String::new(),
                        };
                        let summary = p
                            .new
                            .as_ref()
                            .or(p.old.as_ref())
                            .map(|c| c.summary.clone())
                            .unwrap_or_default();
                        let moved = match p.moved {
                            0 => String::new(),
                            n if n > 0 => format!("moved ↑{n}"),
                            n => format!("moved ↓{}", -n),
                        };
                        let open = p.kind == PairKind::Modified;
                        let item =
                            row(("pair", ix), false)
                                .h(px(32.))
                                .gap_3()
                                .when(!open, |s| s.cursor_default())
                                .child(
                                    div()
                                        .w(px(14.))
                                        .font_family(theme::CODE_FONT)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(color)
                                        .child(mark),
                                )
                                .child(
                                    div()
                                        .w(px(150.))
                                        .flex_none()
                                        .font_family(theme::CODE_FONT)
                                        .text_size(px(11.))
                                        .text_color(theme::muted())
                                        .child(ids),
                                )
                                .child(
                                    super::hover::cut("summary", summary)
                                        .flex_1()
                                        .when(p.kind == PairKind::Dropped, |s| {
                                            s.line_through().text_color(theme::muted())
                                        }),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(px(12.))
                                        .text_color(theme::muted())
                                        .child(if moved.is_empty() {
                                            label.to_owned()
                                        } else {
                                            format!("{label} · {moved}")
                                        }),
                                )
                                .children(open.then(|| {
                                    div()
                                        .flex_none()
                                        .text_color(theme::accent())
                                        .child("interdiff →")
                                }))
                                .when(open, |s| {
                                    s.on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_interdiff(ix, cx)
                                    }))
                                });
                        div().w_full().py(px(1.)).child(item)
                    })),
            )
    }

    pub(super) fn render_commit_row(
        &self,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let commit = &self.commits[ix];
        let selected = self.selection == Selection::Commit(ix);
        // Its anatomy (#79): the title and sha always; who and when while the panel has 260 px;
        // its labels under 360 px leave first. Its card holds them all.
        let room = self.widths.room("side-commits");
        let item = row(("commit", ix), selected)
            .h(px(62.))
            .flex_col()
            .items_start()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .line_height(px(17.))
                    .clamp_in_tip(2)
                    .child(self.summary_text(&commit.summary.clone().into())),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap_2()
                    .text_size(px(11.))
                    .text_color(theme::muted())
                    .child(
                        div()
                            .font_family(theme::CODE_FONT)
                            .child(format::short(commit.id)),
                    )
                    .when(room.third(), |s| {
                        s.child(
                            div().flex().min_w_0().flex_shrink(1.).gap_2().children(
                                self.decor
                                    .get(&commit.id)
                                    // The first label and "+N" for the rest; the card names them all.
                                    .map(|d| lists::deco_tags(d))
                                    .unwrap_or_default(),
                            ),
                        )
                    })
                    // A branch of one's own still has commits by others under it. The labels give
                    // way first; the card has the whole name.
                    .when(room.second(), |s| {
                        s.child(
                            div()
                                .flex_none()
                                .max_w(px(160.))
                                .cut_in_tip()
                                .child(commit.author.clone()),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .flex_none()
                                .whitespace_nowrap()
                                .child(format::ago(commit.time)),
                        )
                    }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.commit_context(ix, event.position, cx)
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.close_drawer(cx);
                this.select_commit(ix, cx)
            }));
        // A long hover: the commit's card (#53) — its whole message start, author, date, id, labels.
        let item = match self.commit_card(ix) {
            Some(card) => {
                let card = std::rc::Rc::new(card);
                item.card(move |_, cx| hover::CommitCard::show(&card, cx))
            }
            None => item,
        };
        div().w_full().h(px(64.)).px(px(6.)).py(px(1.)).child(item)
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The host's words (pull requests on GitHub) for everything drawn from here.
        words::set(self.web.as_ref().is_some_and(|w| w.github));
        markdown::set_me(self.requests.login.as_deref());
        self.take_error(cx);
        self.take_update_news(cx);
        // Settings closed (esc, Done, a place) while a theme was edited: its preview ends too.
        if !self.settings_open && self.theme_edit.is_some() {
            self.close_theme_edit(cx);
        }
        // Before anything is drawn: which input has the keyboard now (#168).
        self.sync_keyboard();
        let title = match &self.root {
            Some(root) => format!(
                "GitLance — {} · {}",
                crate::labels::project_name(root),
                root.display()
            ),
            None => "GitLance".to_owned(),
        };
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        // The widths the layout rules read: the window's, and last frame's width of the diff.
        if !self.zoom_applied {
            self.zoom_applied = true;
            apply_ui_zoom(self.settings.ui_zoom, window);
        }
        let win: f32 = window.viewport_size().width.into();
        if (win - self.win_width).abs() > 0.5 {
            self.win_width = win;
            self.follow_width();
        }
        // A folded section has no keyboard: it goes to the diff.
        let folded_zone = match self.zone {
            zones::Zone::Commits => self.layout.fold_commits || !self.side_open(),
            zones::Zone::Files => !self.files_zone(),
            _ => false,
        };
        if folded_zone && self.diff.is_some() {
            self.zone = zones::Zone::Diff;
        }
        let body = self.body_width.get();
        // Under 1280 px of window too (#80).
        let narrow = (body > 0. && body < 700.) || self.menu_narrow();
        // At the interface size chosen: 600 px of the design is more at 150 %.
        let tight = body > 0. && body < f32::from(px(600.));
        if narrow != self.narrow || tight != self.tight {
            self.narrow = narrow;
            self.tight = tight;
            self.relayout_all();
        }
        self.follow_scroll(cx);
        if self.char_width == 0. {
            let text = window.text_system();
            let id = text.resolve_font(&font(theme::CODE_FONT));
            self.char_width = text
                .advance(id, px(self.code_size()), 'm')
                .map_or(7.2, |size| f32::from(size.width));
        }

        div()
            .id("workspace")
            .key_context(
                // Recording a key in Settings → Keyboard: no binding may take it first.
                if self.recording.is_some() {
                    "Recording"
                } else {
                    match self.typing() {
                        None => "Workspace",
                        Some(Input::Palette | Input::FindFiles | Input::Beside) => {
                            "Workspace Typing Palette"
                        }
                        // ↑ ↓ belong to the field: they move its caret or its line. The new
                        // version's card scrolls with them (#135).
                        Some(
                            Input::Compose
                            | Input::GoLine
                            | Input::Submit
                            | Input::Report
                            | Input::UpdateCard,
                        ) => "Workspace Typing Caret",
                        Some(Input::NewRequest) if self.typing_description() => {
                            "Workspace Typing Caret"
                        }
                        Some(_) => "Workspace Typing",
                    }
                },
            )
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::previous_commit))
            .on_action(cx.listener(Self::next_commit))
            .on_action(cx.listener(Self::previous_file))
            .on_action(cx.listener(Self::next_file))
            .on_action(cx.listener(Self::previous_step))
            .on_action(cx.listener(Self::next_step))
            .on_action(cx.listener(Self::toggle_unified))
            .on_action(cx.listener(Self::cycle_mode))
            .on_action(cx.listener(|this, _: &ToggleRendered, _, cx| this.toggle_page_source(cx)))
            .on_action(
                cx.listener(|this, _: &PickTheme, _, cx| {
                    this.open_palette(palette::Kind::Theme, cx)
                }),
            )
            .on_action(cx.listener(Self::set_lines))
            .on_action(cx.listener(Self::set_words))
            .on_action(cx.listener(Self::set_structural))
            .on_action(cx.listener(Self::open_settings_action))
            .on_action(cx.listener(Self::find_action))
            .on_action(cx.listener(Self::find_commits_action))
            .on_action(cx.listener(Self::filter_files_action))
            .on_action(cx.listener(Self::go_back_action))
            .on_action(cx.listener(Self::go_forward_action))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::save_theme_key))
            .on_action(cx.listener(Self::toggle_files))
            .on_action(cx.listener(Self::toggle_requests))
            .on_action(cx.listener(Self::focus_diff))
            .on_action(cx.listener(Self::zen_one_file))
            .on_action(cx.listener(Self::report_problem))
            .on_action(cx.listener(Self::check_for_updates))
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::next_change))
            .on_action(cx.listener(Self::previous_change))
            .on_action(cx.listener(Self::next_match))
            .on_action(cx.listener(Self::previous_match))
            .on_action(cx.listener(|this, _: &CommentLine, _, cx| this.open_goline(cx)))
            .on_action(cx.listener(|this, _: &SubmitReview, _, cx| this.open_submit(cx)))
            .on_action(cx.listener(|this, _: &ApproveRequest, _, cx| this.toggle_approval(cx)))
            .on_action(cx.listener(|this, _: &SwitchTo, _, cx| {
                if this.root.is_some() {
                    this.open_palette(palette::Kind::Switch, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &NextDraft, _, cx| this.step_draft(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousDraft, _, cx| this.step_draft(false, cx)))
            .on_action(cx.listener(|this, _: &CreateRequest, _, cx| this.open_create(None, cx)))
            .on_action(
                cx.listener(|this, _: &QuickCreateRequest, _, cx| this.open_create_quick(cx)),
            )
            // Every draft to its route; with no chat listening, the card asks (#77).
            .on_action(cx.listener(|this, _: &SendToAgent, _, cx| {
                let ids = this.agent_drafts();
                if ids.is_empty() {
                    return this.open_agent_pick(cx);
                }
                this.send_agent(ids, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenIn, _, cx| this.open_in(cx)))
            .on_action(cx.listener(|this, _: &RevealInFinder, _, cx| this.reveal_target(cx)))
            .on_action(cx.listener(|this, _: &ShowShortcuts, _, cx| this.toggle_shortcuts(cx)))
            .on_action(cx.listener(|this, _: &NameProject, _, cx| {
                if let Some(root) = this.root.clone() {
                    this.open_label(labeling::Target::Project(root), cx)
                }
            }))
            .on_action(cx.listener(|this, _: &NextThread, _, cx| {
                // ⇧N: a note on the agent's form in view, else the next discussion (#10 C).
                match this.acting_agent_form() {
                    Some(id) => this.start_answer(id, cx),
                    None => this.step_thread(true, cx),
                }
            }))
            .on_action(cx.listener(|this, _: &PreviousThread, _, cx| this.step_thread(false, cx)))
            .on_action(cx.listener(|this, _: &ReplyThread, _, cx| this.reply_focused(cx)))
            .on_action(cx.listener(|this, _: &ResolveThread, _, cx| this.resolve_focused(cx)))
            .on_action(cx.listener(|this, _: &EditComment, _, cx| {
                // In the Inbox, e is done with the selected notice (an agent's: later, or resolved).
                if this.inbox && this.zone != zones::Zone::Tool {
                    this.inbox_done(cx)
                } else {
                    this.edit_focused(cx)
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleThread, _, cx| this.toggle_focused(cx)))
            .on_action(cx.listener(|this, _: &ReactNote, _, cx| this.react_focused(cx)))
            .on_action(cx.listener(|this, _: &MarkReviewed, _, cx| this.mark_and_advance(cx)))
            .on_action(cx.listener(|this, _: &ToggleBookmark, _, cx| this.toggle_bookmark(cx)))
            .on_action(cx.listener(|this, _: &ShowBookmarks, _, cx| {
                if this.palette.is_some() {
                    this.close_palette(cx);
                } else if this.diff.is_some() {
                    this.open_palette(palette::Kind::Bookmarks, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, window, cx| this.ui_zoom_by(1, window, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, window, cx| this.ui_zoom_by(-1, window, cx)))
            .on_action(
                cx.listener(|this, _: &ZoomReset, window, cx| this.set_ui_zoom(100, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ShowStructure, _, cx| {
                if this.palette.is_some() {
                    this.close_palette(cx);
                } else if this.diff.is_some() {
                    this.open_palette(palette::Kind::Structure, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenRecent, _, cx| {
                if this.palette.is_some() {
                    this.close_palette(cx);
                } else {
                    this.open_palette(palette::Kind::Recent, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleAnnotate, _, cx| this.toggle_annotate(cx)))
            .on_action(cx.listener(|this, _: &FindInFiles, _, cx| this.open_find_files(cx)))
            .on_action(cx.listener(|this, _: &ToggleReviewed, _, cx| this.toggle_reviewed(cx)))
            .on_action(cx.listener(|this, _: &ShowOverview, _, cx| {
                this.show_section(nav::Section::Overview, cx)
            }))
            .on_action(cx.listener(|this, _: &NextSection, _, cx| this.step_section(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousSection, _, cx| this.step_section(false, cx)))
            .on_action(cx.listener(|this, _: &NextOpenItem, _, cx| this.step_open_item(true, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousOpenItem, _, cx| this.step_open_item(false, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleComments, _, cx| {
                this.toggle_tool(tools::Tool::Discussions, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleAgents, _, cx| {
                this.toggle_tool(tools::Tool::Agents, cx)
            }))
            .on_action(cx.listener(|this, _: &FoldMenu, _, cx| this.toggle_menu(cx)))
            .on_action(
                cx.listener(|this, _: &GoToInbox, _, cx| this.go_space(spaces::Space::Inbox, cx)),
            )
            .on_action(
                cx.listener(|this, _: &GoToCode, _, cx| this.go_space(spaces::Space::Code, cx)),
            )
            .on_action(cx.listener(|this, _: &GoToBranches, _, cx| {
                this.go_space(spaces::Space::Branches, cx)
            }))
            .on_action(cx.listener(|this, _: &GoToMergeRequests, _, cx| {
                this.go_space(spaces::Space::Requests, cx)
            }))
            .on_action(cx.listener(|this, _: &NextZone, _, cx| this.step_zone(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousZone, _, cx| this.step_zone(false, cx)))
            .on_action(cx.listener(|this, _: &Activate, _, cx| this.activate(cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page(true, cx)))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page(false, cx)))
            .on_action(cx.listener(|this, _: &DiffStart, _, cx| this.diff_edge(false, cx)))
            .on_action(cx.listener(|this, _: &DiffEnd, _, cx| this.diff_edge(true, cx)))
            // ⌃1, the branch quick switch: search everywhere on its Branches (#72).
            .on_action(cx.listener(|this, _: &SwitchBranch, _, cx| {
                if this.root.is_some() {
                    this.search(palette::Scope::Branches, cx)
                }
            }))
            // ⌃2 ⌃3: a section of the side panel; narrow, its drawer opens there (#80).
            .on_action(cx.listener(|this, _: &FocusCommits, _, cx| {
                this.focus_panel(zones::Zone::Commits, cx)
            }))
            .on_action(
                cx.listener(|this, _: &FocusFiles, _, cx| this.focus_panel(zones::Zone::Files, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FocusDiffZone, _, cx| this.set_zone(zones::Zone::Diff, cx)),
            )
            .on_action(
                cx.listener(|this, _: &NewTab, _, cx| {
                    this.open_palette(palette::Kind::Projects, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &CloseItem, _, cx| this.close_shown(cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                this.with_shell(window, cx, |s, window, cx| {
                    let active = s.active_ix();
                    s.close(active, window, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &ReopenTab, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.reopen(window, cx))
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.step(true, window, cx))
            }))
            .on_action(cx.listener(|this, _: &PreviousTab, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.step(false, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab1, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.select(0, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab2, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.select(1, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab3, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.select(2, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab4, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.select(3, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab5, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.select(4, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab6, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.select(5, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab7, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.select(6, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab8, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.select(7, window, cx))
            }))
            .on_action(cx.listener(|this, _: &Tab9, window, cx| {
                this.with_shell(window, cx, |s, window, cx| s.select(8, window, cx))
            }))
            .on_action(cx.listener(Self::open_palette_action))
            .on_action(
                cx.listener(|this, _: &SearchFiles, _, cx| this.search(palette::Scope::Files, cx)),
            )
            .on_action(cx.listener(|this, _: &SearchCommits, _, cx| {
                this.search(palette::Scope::Commits, cx)
            }))
            .on_action(cx.listener(|this, _: &SearchBranches, _, cx| {
                this.search(palette::Scope::Branches, cx)
            }))
            .on_action(cx.listener(|this, _: &SearchRequests, _, cx| {
                this.search(palette::Scope::Requests, cx)
            }))
            .on_action(cx.listener(|this, _: &SearchActions, _, cx| {
                this.search(palette::Scope::Actions, cx)
            }))
            .on_action(cx.listener(Self::compare_action))
            .on_action(cx.listener(Self::copy_link_action))
            .on_action(cx.listener(Self::pin_tab_action))
            .on_action(cx.listener(Self::palette_up))
            .on_action(cx.listener(Self::palette_down))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.recording.is_some() {
                    return this.record_key(event, cx);
                }
                this.find_key(event, window, cx)
            }))
            .on_action(cx.listener(Self::toggle_wrap))
            .on_action(cx.listener(Self::toggle_full_context))
            .on_action(cx.listener(Self::toggle_whitespace))
            .on_action(cx.listener(Self::toggle_one_file))
            .on_action(cx.listener(Self::toggle_expand_all_action))
            .on_action(cx.listener(Self::toggle_rich_comments_action))
            .on_action(cx.listener(|this, _: &LeaveRequest, _, cx| this.leave_request(cx)))
            .on_action(
                cx.listener(|this, _: &ToggleAllReviewed, _, cx| this.toggle_all_reviewed(cx)),
            )
            .on_action(cx.listener(|this, _: &NextRequest, _, cx| this.step_request(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousRequest, _, cx| this.step_request(false, cx)))
            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().first() {
                    this.request_open(path.clone(), None, cx);
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.mention_pointer(event.position, cx);
                this.drag_move(event, cx)
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.selecting = false;
                    this.end_drag(cx)
                }),
            )
            .when_some(self.drag, |s, drag| {
                s.cursor(if drag.split.vertical() {
                    CursorStyle::ResizeUpDown
                } else {
                    CursorStyle::ResizeLeftRight
                })
            })
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme::base())
            .text_color(theme::text())
            .font_family(theme::UI_FONT)
            .text_size(px(13.))
            .child(self.render_title_bar(cx))
            .children(self.render_toasts(cx))
            .child(match self.root {
                _ if self.settings_open => self.render_settings(cx).into_any_element(),
                None => self.render_welcome(cx).into_any_element(),
                Some(_) => div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .pl(px(gap()))
                    // The stripe sits on the window's right edge; zen has none.
                    .pr(px(if self.zen.is_some() { gap() } else { 0. }))
                    .pb(px(gap()))
                    // Zen slides the rail away and back (#56, 16).
                    .children(self.render_project_rail(cx).map(|rail| {
                        motion::toward(
                            div().flex_none().flex().overflow_hidden().child(rail),
                            "rail-zen",
                            self.zen.is_none(),
                            self.motion.recent("zen"),
                            motion::BASE,
                            |rail, v| rail.max_w(px(48. * v)).opacity(v),
                        )
                    }))
                    .children(self.render_spaces(cx))
                    // The page of the space on screen (#71).
                    .child(self.render_space(cx))
                    .children(self.stripe_gap())
                    .children(self.render_stripe(cx).map(|stripe| {
                        motion::toward(
                            div().flex_none().flex().overflow_hidden().child(stripe),
                            "stripe-zen",
                            self.zen.is_none(),
                            self.motion.recent("zen"),
                            motion::BASE,
                            |stripe, v| stripe.max_w(px(44. * v)).opacity(v),
                        )
                    }))
                    .into_any_element(),
            })
            // Last, so it paints over everything.
            .children(self.render_ctx_menu(window.viewport_size(), cx))
            .children(self.render_mention_card(cx))
            .children(self.render_blame_tip())
            .children(self.render_find_files(cx))
            .children(self.render_open_ask(cx))
            .children(self.render_beside_backdrop(cx))
            .children(self.render_agent_pick(cx))
            .children(self.render_label_card(cx))
            .children(self.render_update_card(cx))
            .children(self.render_create_quick(cx))
            .children(self.render_shortcuts(cx))
            .children(self.render_palette(cx))
    }
}

/// How wide a pane's handle is: the gap between islands, a thin line between flat panes. A pane
/// that slides in and out is its width and its handle, so nothing of it stays at the end.
pub(super) fn handle_width() -> f32 {
    if flat() { 3. } else { GAP }
}

/// The pointer coordinate along the axis `split` resizes.
pub(super) fn axis(split: Split, position: Point<gpui::Pixels>) -> f32 {
    f32::from(if split.vertical() {
        position.y
    } else {
        position.x
    })
}
