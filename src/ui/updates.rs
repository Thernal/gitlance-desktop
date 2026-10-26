//! A newer GitLance, said in the window (#135; the check is `crate::updates`): a card over the window
//! the first time a check finds a version — what is new in it and in every release in between, each
//! one's commits folded under it, Download (its page), Skip this version, Later —, then a mark in the
//! title bar that opens it again. The releases scroll between the card's head and its buttons, a fade
//! at their foot while more is below; ↑ ↓ (j k), space, ⇧space, Home and End scroll them. GitLance →
//! Check for Updates…, ⌘K and Settings → General ask at once. Nothing is downloaded or installed.

use super::{ISLAND_RADIUS, ROW_RADIUS, Workspace, format, icons, keys, markdown, px, theme};
use crate::updates::{self, Release};
use gpui::{
    ClickEvent, Context, FontWeight, KeyDownEvent, MouseButton, ScrollHandle, SharedString, canvas,
    div, linear_color_stop, linear_gradient, point, prelude::*,
};
use std::cell::Cell;
use std::rc::Rc;

/// How far ↑ ↓ scroll the releases.
const STEP: f32 = 48.;

/// The card's state: the releases whose commits are unfolded, and their scroll.
#[derive(Default)]
pub struct UpdateCard {
    unfolded: Vec<semver::Version>,
    scroll: ScrollHandle,
    /// More of the releases below what shows: the fade at their foot. Set as they are drawn.
    more: Rc<Cell<bool>>,
}

impl Workspace {
    /// Each frame: a version a check found and no card showed yet opens the card — unless
    /// something is being written or another card is up, when it waits.
    pub(super) fn take_update_news(&mut self, cx: &mut Context<Self>) {
        let busy = self.update_card.is_some()
            || self.shortcuts
            || self.label_edit.is_some()
            || self.palette.is_some()
            || self.field.is_some()
            || self.compose.is_some();
        if !busy && updates::take_news().is_some() {
            self.update_card = Some(UpdateCard::default());
            cx.notify();
        }
    }

    pub(super) fn open_update_card(&mut self, cx: &mut Context<Self>) {
        self.update_card = Some(UpdateCard::default());
        cx.notify();
    }

    /// A snapshot's card with the newest release's commits unfolded.
    #[cfg(feature = "snapshot")]
    pub(super) fn unfold_update_commits(&mut self) {
        let newest = updates::last().and_then(|c| c.newer.first().map(|r| r.version.clone()));
        if let (Some(card), Some(version)) = (self.update_card.as_mut(), newest) {
            card.unfolded.push(version);
        }
    }

    /// The card's keys: esc closes it, the rest scroll the releases.
    pub(super) fn update_card_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(card) = self.update_card.as_ref() else {
            return;
        };
        let s = &card.scroll;
        let (y, max) = (f32::from(s.offset().y), f32::from(s.max_offset().y));
        let page = f32::from(s.bounds().size.height) * 0.85;
        let shift = event.keystroke.modifiers.shift;
        let to = match event.keystroke.key.as_str() {
            "escape" => return self.close_update_card(cx),
            "down" | "j" => y - f32::from(px(STEP)),
            "up" | "k" => y + f32::from(px(STEP)),
            "space" if shift => y + page,
            "space" | "pagedown" => y - page,
            "pageup" => y + page,
            "home" => 0.,
            "end" => -max,
            _ => return,
        };
        s.set_offset(point(s.offset().x, gpui::px(to.clamp(-max, 0.))));
        cx.notify();
    }

    pub(super) fn close_update_card(&mut self, cx: &mut Context<Self>) {
        self.update_card = None;
        cx.notify();
    }

    /// GitLance → Check for Updates…, ⌘K, Settings' Check now: asks GitHub now and opens the card,
    /// a skipped version too, or says this one is the newest.
    pub(super) fn check_updates_now(&mut self, cx: &mut Context<Self>) {
        if self.update_checking {
            return;
        }
        self.update_checking = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let check = cx
                .background_executor()
                .spawn(async { updates::check() })
                .await;
            this.update(cx, |this, cx| {
                this.update_checking = false;
                match (&check.error, check.newer.is_empty()) {
                    (_, false) => this.open_update_card(cx),
                    (Some(e), true) => {
                        this.error = Some(format!("Could not ask GitHub for releases: {e}").into())
                    }
                    (None, true) => this.toast_info(
                        format!("GitLance {} is the newest", updates::current()).into(),
                        cx,
                    ),
                }
                cx.refresh_windows();
            })
            .ok();
        })
        .detach();
    }

    /// The title bar's mark of a newer version (not one skipped): a click opens the card.
    pub(super) fn render_update_mark(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let release = updates::newest()?;
        let tip: SharedString = format!(
            "GitLance {} is out — what is new, and the download",
            release.version
        )
        .into();
        Some(
            div()
                .id("update-mark")
                .flex_none()
                .h(px(24.))
                .flex()
                .items_center()
                .gap(px(5.))
                .px_2()
                .rounded(px(ROW_RADIUS))
                .text_size(px(11.))
                .text_color(theme::accent())
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .tooltip_show_delay(super::hover::LONG)
                .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into())
                .child(
                    icons::icon("update")
                        .size(px(14.))
                        .text_color(theme::accent()),
                )
                .child(release.version.to_string())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_update_card(cx))),
        )
    }

    /// Settings → General, under Check for updates: this build, what the last check said, and
    /// Check now.
    pub(super) fn render_update_status(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let check = updates::last();
        let said = if self.update_checking {
            "checking…".to_owned()
        } else {
            match &check {
                None => "not checked yet".to_owned(),
                Some(c) => {
                    let when =
                        c.at.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| format::ago(d.as_secs() as i64))
                            .unwrap_or_default();
                    match (c.newer.first(), &c.error) {
                        (Some(r), _) => format!("{} is out · checked {when}", r.version),
                        (None, Some(e)) => format!("{e} · {when}"),
                        (None, None) => format!("the newest · checked {when}"),
                    }
                }
            }
        };
        let newer = check.is_some_and(|c| !c.newer.is_empty());
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(theme::muted())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(format!("{} — {said}", crate::report::version())),
            )
            .when(newer, |s| {
                s.child(
                    text_button("update-whats-new", "What's new").on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.open_update_card(cx)),
                    ),
                )
            })
            .child(
                text_button("update-check-now", "Check now").on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.check_updates_now(cx)),
                ),
            )
    }

    /// The card: the newest version, then what is new in each release newer than this build.
    pub(super) fn render_update_card(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let card = self.update_card.as_ref()?;
        let newer = updates::last().map(|c| c.newer).unwrap_or_default();
        let newest = newer.first()?.clone();
        let current = updates::current();
        let selecting = self.selecting(cx.entity().downgrade());
        let releases = newer.iter().map(|r| {
            let open = card.unfolded.contains(&r.version);
            let version = r.version.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .pb(px(14.))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(r.version.to_string()),
                        )
                        .children(r.published.map(|t| {
                            div()
                                .text_size(px(11.))
                                .text_color(theme::faint())
                                .child(format::ago(t))
                        })),
                )
                .child(if r.notes.is_empty() {
                    div()
                        .text_size(px(12.))
                        .text_color(theme::faint())
                        .child("No notes for this release.")
                        .into_any_element()
                } else {
                    markdown::body(
                        &r.notes,
                        true,
                        format!("update-notes-{}", r.version),
                        None,
                        Some(selecting.clone()),
                    )
                })
                .when(r.count > 0, |s| {
                    s.child(
                        div()
                            .id(SharedString::from(format!("update-commits-{}", r.version)))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme::text()))
                            .child(
                                icons::icon(if open { "chevron" } else { "chevron-right" })
                                    .size(px(14.))
                                    .text_color(theme::muted()),
                            )
                            .child(super::plural(r.count, "commit"))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if let Some(card) = this.update_card.as_mut() {
                                    if let Some(ix) =
                                        card.unfolded.iter().position(|v| *v == version)
                                    {
                                        card.unfolded.remove(ix);
                                    } else {
                                        card.unfolded.push(version.clone());
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .when(open, |s| {
                        s.child(div().pl(px(18.)).text_size(px(12.)).child(markdown::body(
                            &r.commits,
                            true,
                            format!("update-commits-{}", r.version),
                            None,
                            Some(selecting.clone()),
                        )))
                    })
                })
        });
        let since = newest
            .published
            .map(|t| format!(" · out {}", format::ago(t)))
            .unwrap_or_default();
        Some(
            div()
                .id("update-backdrop")
                .absolute()
                .size_full()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(72.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_update_card(cx)),
                )
                .child(
                    div()
                        .occlude()
                        .w(px(620.))
                        .max_h(gpui::relative(0.86))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .p_4()
                        .rounded(px(ISLAND_RADIUS))
                        .border_1()
                        .border_color(theme::island_border())
                        .bg(theme::panel())
                        .shadow_lg()
                        .text_size(px(13.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(format!("GitLance {} is out", newest.version)),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme::muted())
                                        .child(format!("You have {current}{since}")),
                                ),
                        )
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .border_t_1()
                                .border_color(theme::island_border())
                                .child(
                                    div()
                                        .id("update-releases")
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .track_scroll(&card.scroll)
                                        .pt(px(12.))
                                        .children(releases),
                                )
                                .children(card.more.get().then(fade))
                                .child(watch_more(card.scroll.clone(), card.more.clone())),
                        )
                        .child(self.render_update_buttons(&newest, cx)),
                ),
        )
    }

    /// Skip this version on the left; Later and Download on the right.
    fn render_update_buttons(
        &self,
        newest: &Release,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let url = newest.url.clone();
        let skipped = newest.version.clone();
        div()
            .pt(px(10.))
            .border_t_1()
            .border_color(theme::island_border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        button("update-skip", "Skip this version", false).on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                updates::skip(&skipped);
                                this.close_update_card(cx);
                            },
                        )),
                    )
                    .child(div().flex_1())
                    .child(
                        button("update-later", "Later", false)
                            .child(keys::caps("esc", keys::On::Plain))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.close_update_card(cx)
                            })),
                    )
                    .child(
                        button("update-download", "Download", true)
                            .child(
                                icons::icon("external")
                                    .size(px(13.))
                                    .text_color(theme::base()),
                            )
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.open_url(&url);
                                this.close_update_card(cx);
                            })),
                    ),
            )
    }
}

/// The fade at the foot of the releases while more is below.
fn fade() -> impl IntoElement {
    let ground = theme::panel();
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .h(px(40.))
        .bg(linear_gradient(
            180.,
            linear_color_stop(gpui::Rgba { a: 0., ..ground }, 0.),
            linear_color_stop(ground, 1.),
        ))
}

/// Laid over the releases (their parent `relative`): whether more of them is below, kept in `more`
/// as they are drawn; a change draws the window again, so the fade comes or goes.
fn watch_more(scroll: ScrollHandle, more: Rc<Cell<bool>>) -> impl IntoElement {
    canvas(
        move |_, window, _| {
            let max = f32::from(scroll.max_offset().y);
            let below = max > 1. && f32::from(scroll.offset().y) > 1. - max;
            if more.replace(below) != below {
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A button of the card, as the naming card's.
fn button(id: &'static str, label: &'static str, primary: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(30.))
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(6.))
        .text_size(px(12.))
        .cursor_pointer()
        .when(primary, |s| {
            s.bg(theme::accent())
                .text_color(theme::base())
                .font_weight(FontWeight::SEMIBOLD)
        })
        .when(!primary, |s| {
            s.text_color(theme::muted())
                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
        })
        .child(label)
}

/// A quiet word that acts, in a settings line.
fn text_button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .px(px(8.))
        .h(px(24.))
        .flex()
        .items_center()
        .rounded(px(ROW_RADIUS))
        .text_color(theme::accent())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(label)
}
