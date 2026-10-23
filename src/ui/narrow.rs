//! Narrow windows (#80): the diff's room comes first, and what folds comes back from the edge it
//! went into, never in the content. Under 1280 px the menu folds to icons (◧ and ⌘B still open
//! it) and the diff is unified. Under 1100 the side panel folds into the same left column — this
//! page's panels as icons under a line, a click or ⌃3 opening it as a drawer over the content,
//! docked again when there is room — a list's preview goes, the bar's path is ‹ and the id,
//! secondary actions keep their icon and search its icon. Under 1000 a tool window floats over the
//! diff, and a drawer and a floating tool window are never open together.

use super::zones::Zone;
use super::{Workspace, icons, keys, px, theme};
use gpui::{AnyElement, ClickEvent, Context, SharedString, div, prelude::*};

/// Under it the menu folds and the diff is unified.
pub const MENU: f32 = 1280.;
/// Under it the side panel folds into the menu's column, and the page is compact.
pub const SIDE: f32 = 1100.;
/// Under it a tool window floats over the diff.
pub const FLOAT: f32 = 1000.;

/// The menu folded in a window `width` px wide: as `set` from 1280 px, else folded unless
/// `chosen` there.
fn menu_fold(width: f32, set: bool, chosen: Option<bool>) -> bool {
    if width < MENU {
        chosen.unwrap_or(true)
    } else {
        set
    }
}

/// What a narrow window keeps for the session: the menu's fold chosen while it is narrow, and
/// whether the side panel is open as a drawer.
#[derive(Default)]
pub struct Narrowed {
    menu: Option<bool>,
    pub drawer: bool,
}

impl Workspace {
    /// The window has crossed a step: what it folded comes back as it was.
    pub(super) fn follow_width(&mut self) {
        if self.win_width >= MENU {
            self.narrowed.menu = None;
        }
        if self.win_width >= SIDE {
            self.narrowed.drawer = false;
        }
    }

    /// Under 1280 px: the menu folds, unless opened here, and the diff is unified.
    pub(super) fn menu_narrow(&self) -> bool {
        self.win_width < MENU
    }

    /// Under 1100 px: the side panel in the menu's column, a list without its preview, the bar's
    /// path as ‹ and the id, its secondary actions and search as icons.
    pub(super) fn compact(&self) -> bool {
        self.win_width < SIDE
    }

    /// The menu folded to icons: the setting, or under 1280 px folded unless opened here.
    pub(super) fn menu_folded(&self) -> bool {
        menu_fold(self.win_width, self.layout.fold_places, self.narrowed.menu)
    }

    /// ◧ and ⌘B: the menu to icons and back — kept in `layout.txt`, or for while the window is
    /// narrow.
    pub(super) fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        // Its names fade while it folds or unfolds; its icons stay put (#56, 2).
        self.mark_motion("places-fold", cx);
        if self.menu_narrow() {
            self.narrowed.menu = Some(!self.menu_folded());
        } else {
            self.layout.fold_places = !self.layout.fold_places;
            self.save_layout();
        }
        cx.notify();
    }

    /// The side panel docked beside the page: shown, and the window wide enough.
    pub(super) fn side_docked(&self) -> bool {
        self.layout.show_sidebar && !self.compact()
    }

    /// The side panel as a drawer over the content: the window narrow, and it opened.
    pub(super) fn side_drawer(&self) -> bool {
        self.compact() && self.narrowed.drawer
    }

    /// A panel's icon or ⌃3: the drawer opens with the keyboard in `zone`; again, it closes. A
    /// floating tool window goes, as the two are never open together.
    pub(super) fn toggle_drawer(&mut self, zone: Option<Zone>, cx: &mut Context<Self>) {
        let open = !self.narrowed.drawer;
        self.narrowed.drawer = open;
        if open {
            if self.tool_floats() && self.tool().is_some() {
                self.set_tool(None, cx);
            }
            if let Some(zone) = zone {
                self.set_zone(zone, cx);
            }
        }
        self.mark_motion("drawer", cx);
        cx.notify();
    }

    /// ⌃2 ⌃3: the keyboard to a section of the side panel; while it is folded, its drawer opens
    /// there, or closes when it is open.
    pub(super) fn focus_panel(&mut self, zone: Zone, cx: &mut Context<Self>) {
        if self.compact() && !self.side_icons().is_empty() {
            return self.toggle_drawer(Some(zone), cx);
        }
        self.set_zone(zone, cx)
    }

    /// The drawer closes: esc, or what was picked in it is on screen.
    pub(super) fn close_drawer(&mut self, cx: &mut Context<Self>) {
        if self.narrowed.drawer {
            self.narrowed.drawer = false;
            self.mark_motion("drawer", cx);
            cx.notify();
        }
    }

    /// Under the spaces, below a line: this page's panels, while they are not beside the page —
    /// the window too narrow for the side panel (a click opens it as a drawer), or the panel hidden
    /// with ⌘J (a click brings it back), so a hidden panel is not lost (the developer,
    /// 2026-10-23). Icons in the folded menu; named in the open one.
    pub(super) fn render_panel_icons(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let narrow = self.compact();
        if !narrow && self.layout.show_sidebar {
            return None;
        }
        let panels = self.side_icons();
        if panels.is_empty() {
            return None;
        }
        let on = self.side_drawer();
        let named = !self.menu_folded();
        let rows = div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .flex_none()
                    .h(px(1.))
                    .mx(px(8.))
                    .my(px(8.))
                    .bg(theme::island_border()),
            )
            .children(panels.into_iter().map(|(icon, name, zone)| {
                let tip: SharedString = if narrow {
                    format!("{name}  ⌃3").into()
                } else {
                    format!("{name} — hidden  ⌘J").into()
                };
                div()
                    .id(SharedString::from(format!("panel-{name}")))
                    .h(px(32.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(10.))
                    // Where a space's icon sits, folded or open.
                    .pl(px(11.5))
                    .pr(px(8.))
                    .rounded(px(super::ROW_RADIUS))
                    .cursor_pointer()
                    .text_color(theme::muted())
                    .when(on, |s| s.bg(theme::selected()).text_color(theme::text()))
                    .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                    .child(icons::icon(icon).size(px(17.)).text_color(if on {
                        theme::text()
                    } else {
                        theme::muted()
                    }))
                    .when(named, |s| {
                        s.child(div().flex_1().min_w_0().truncate().child(name))
                            .when(!narrow, |s| s.child(keys::caps("⌘J", keys::On::Plain)))
                    })
                    .tooltip_show_delay(super::hover::LONG)
                    .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if narrow {
                            this.toggle_drawer(zone, cx)
                        } else {
                            this.show_side(zone, cx)
                        }
                    }))
                    .into_any_element()
            }));
        // Hidden a moment ago, they fade in as the panel slides away.
        Some(if self.motion.recent("side") {
            super::motion::appear(rows, "panel-icons", super::motion::BASE, |rows, v| {
                rows.opacity(v)
            })
            .into_any_element()
        } else {
            rows.into_any_element()
        })
    }

    /// The side panel hidden with ⌘J comes back, the keyboard in `zone`.
    fn show_side(&mut self, zone: Option<Zone>, cx: &mut Context<Self>) {
        self.layout.show_sidebar = true;
        self.save_layout();
        self.mark_motion("side", cx);
        if let Some(zone) = zone {
            self.set_zone(zone, cx);
        }
        cx.notify();
    }

    /// The side panel on screen: docked, or as the drawer.
    pub(super) fn side_open(&self) -> bool {
        self.side_docked() || self.side_drawer()
    }

    /// The side panel as a drawer: over the content's left edge at its width, with a shadow; it
    /// fades in.
    pub(super) fn drawer(&self, panel: impl IntoElement) -> AnyElement {
        super::motion::appear(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(px(self.layout.sidebar))
                .flex()
                .occlude()
                .shadow_lg()
                .child(panel),
            "drawer",
            super::motion::FAST,
            |drawer, v| drawer.opacity(v),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_folds_under_1280_unless_opened_there() {
        // Wide: the setting.
        assert!(!menu_fold(1480., false, None));
        assert!(menu_fold(1480., true, Some(false)));
        // Narrow: folded, whatever the setting, until ◧ or ⌘B opens it there.
        assert!(menu_fold(1200., false, None));
        assert!(!menu_fold(1200., true, Some(false)));
        assert!(!menu_fold(MENU, false, None));
    }
}
