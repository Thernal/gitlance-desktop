//! The pieces of a discussion under a diff line, as designed in
//! `../Design/mockups/line-comments/c-conversation.html` (chosen 2026-10-12): a frame with a header
//! bar and a footer, notes laid out avatar | header | body on a timeline, buttons whose keys are
//! keycaps (`super::keys`).

use super::keys::{self, On};
use super::px;
use super::theme;
use gpui::{AnyElement, Div, ElementId, FontWeight, Rgba, SharedString, Stateful, div, prelude::*};

/// Where a discussion lives, which decides its colour.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// On the merge request, for everyone on it.
    GitLab,
    /// Kept on this Mac for a coding agent.
    Agent,
    /// One's own GitLab comment, unpublished until the review is submitted.
    Pending,
}

impl Source {
    pub fn color(self) -> Rgba {
        match self {
            Source::GitLab | Source::Pending => theme::warning(),
            Source::Agent => theme::focus(),
        }
    }

    fn tint(self) -> Rgba {
        match self {
            Source::GitLab => theme::warning_bg(),
            Source::Agent => theme::focus_bg(),
            Source::Pending => gpui::rgba(0x00000000),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Source::GitLab => "GitLab",
            Source::Agent => "For your agent",
            Source::Pending => "Pending",
        }
    }
}

/// The card of a discussion: a quiet surface with no outline (variant D, `d-calm.html`). Being
/// selected is a rail on its left edge in the source's colour, bending with the card's corners —
/// never a ring, since an outline means only "you are typing here".
pub fn frame(source: Source, selected: bool) -> Div {
    const RAIL: f32 = 3.;
    div()
        .relative()
        .w_full()
        .max_w(px(720.))
        .flex()
        .flex_col()
        .rounded(px(10.))
        .bg(theme::panel())
        // The rail takes its width out of the padding, so the text does not move.
        .when(selected, |s| {
            s.border_l(px(RAIL))
                .border_color(source.color())
                .pl(px(14. - RAIL))
        })
        .when(!selected, |s| s.pl(px(14.)))
        .pr(px(14.))
        .pt(px(10.))
        .pb(px(12.))
        .font_family(theme::UI_FONT)
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(theme::text())
}

/// The source's pill at the start of a header bar.
pub fn source_pill(source: Source) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .rounded_full()
        .text_size(px(11.))
        .line_height(px(18.))
        .font_weight(FontWeight::SEMIBOLD)
        .bg(source.tint())
        .when(source == Source::Pending, |s| {
            s.border_1().border_color(theme::warning())
        })
        .text_color(source.color())
        .child(super::words::say(source.label()).into_owned())
}

/// The head of a discussion: a chevron that folds it, then whatever the caller puts in. The whole
/// row is the fold target; it has no fill and no rule under it.
pub fn bar(id: impl Into<ElementId>, open: bool) -> Stateful<Div> {
    div()
        .id(id)
        .min_h(px(24.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .ml(px(-6.))
        .when(open, |s| s.mb(px(10.)))
        .text_size(px(12.))
        .text_color(theme::muted())
        .cursor_pointer()
        .child(
            div()
                .size(px(24.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .hover(|s| s.bg(theme::hover()))
                .child(
                    super::icons::icon(if open { "chevron" } else { "chevron-right" })
                        .size(px(16.))
                        .text_color(theme::muted()),
                ),
        )
}

/// A state chip at the end of a header bar: Unresolved, Resolved.
pub fn state(resolved: bool) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .rounded_full()
        .text_size(px(11.))
        .line_height(px(18.))
        .when(resolved, |s| {
            s.bg(theme::added_bg()).text_color(theme::added())
        })
        .when(!resolved, |s| {
            s.bg(theme::selected()).text_color(theme::text())
        })
        .child(if resolved {
            "✓ Resolved"
        } else {
            "Unresolved"
        })
}

/// A round avatar with the name's first letter, coloured by the name.
pub fn avatar(name: &str, me: bool, size: f32) -> Div {
    const HUES: [u32; 6] = [0x56b6c2, 0x98c379, 0xc678dd, 0xe5c07b, 0xe06c75, 0xd19a66];
    let color = if me {
        theme::accent()
    } else {
        let h = name
            .bytes()
            .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
        gpui::rgb(HUES[h as usize % HUES.len()])
    };
    let letter: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into());
    div()
        .size(px(size))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(color)
        .text_size(px((size * 0.42).round()))
        .font_weight(FontWeight::BOLD)
        .text_color(theme::base())
        .child(letter)
}

/// One note: avatar | name, role, time | body below the name. `last` leaves out the timeline line
/// that joins this avatar to the next.
pub fn note(
    author: &str,
    me: bool,
    role: Option<&'static str>,
    time: String,
    body: AnyElement,
    last: bool,
    action: Option<AnyElement>,
) -> Div {
    div()
        .relative()
        .flex()
        .gap(px(12.))
        .pb(px(16.))
        .children((!last).then(|| {
            div()
                .absolute()
                .left(px(13.5))
                .top(px(32.))
                .bottom(px(2.))
                .w(px(1.))
                .bg(theme::selected())
        }))
        .child(avatar(author, me, 28.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .min_w_0()
                        .child(
                            div()
                                .flex_none()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(if me {
                                    "You".to_owned()
                                } else {
                                    author.to_owned()
                                }),
                        )
                        .children(role.map(|r| {
                            div()
                                .flex_none()
                                .px(px(6.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(theme::selected())
                                .text_size(px(11.))
                                .line_height(px(16.))
                                .text_color(theme::muted())
                                .child(r)
                        }))
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child(format!("· {time}")),
                        )
                        .children(action.map(|a| div().ml_auto().flex_none().child(a))),
                )
                .child(div().min_w_0().child(body)),
        )
}

/// How a button stands.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// A bordered button.
    Plain,
    /// Text only, for Cancel.
    Quiet,
    /// Something that cannot be undone (Delete): the danger colour, a red tint under the pointer.
    Danger,
    /// Filled with the source's colour: the one action the box is for.
    Primary(Rgba),
}

/// A 32 px button with an optional keycap run after its label.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    key: Option<&str>,
    kind: Kind,
) -> Stateful<Div> {
    let filled = matches!(kind, Kind::Primary(_));
    div()
        .id(id)
        .h(px(32.))
        .px(px(12.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .rounded(px(6.))
        .text_size(px(12.))
        .cursor_pointer()
        .map(|s| match kind {
            Kind::Plain => s
                .border_1()
                .border_color(theme::selected())
                .bg(theme::hover())
                .text_color(theme::text())
                .hover(|s| s.bg(theme::selected())),
            Kind::Quiet => s
                .text_color(theme::muted())
                .hover(|s| s.bg(theme::hover()).text_color(theme::text())),
            Kind::Danger => s
                .border_1()
                .border_color(theme::removed_line())
                .text_color(theme::removed())
                .hover(|s| s.bg(theme::removed_line())),
            Kind::Primary(color) => s
                .bg(color)
                .text_color(theme::base())
                .font_weight(FontWeight::SEMIBOLD)
                .hover(|s| s.opacity(0.9)),
        })
        .child(label.into())
        .children(key.map(|k| keys::caps(k, if filled { On::Filled } else { On::Plain })))
}

/// A quiet word in a discussion's head (Raw, Delete): text that lights up under the pointer.
pub fn lite(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .px(px(6.))
        .rounded(px(6.))
        .text_size(px(12.))
        .text_color(theme::muted())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
        .child(label.into())
}
