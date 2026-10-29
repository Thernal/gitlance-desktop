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
            Source::Agent => theme::agent(),
        }
    }

    fn tint(self) -> Rgba {
        match self {
            Source::GitLab => theme::warning_bg(),
            Source::Agent => theme::agent_bg(),
            Source::Pending => gpui::transparent_black().into(),
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
        .font_family(theme::ui_font())
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
                // The chevron turns as it opens or folds (#56, 10).
                .child(super::motion::toward(
                    super::icons::icon("chevron")
                        .size(px(16.))
                        .text_color(theme::muted()),
                    "bar-chevron",
                    open,
                    false,
                    super::motion::FAST,
                    |chevron, v| {
                        chevron.with_transformation(gpui::Transformation::rotate(gpui::radians(
                            -std::f32::consts::FRAC_PI_2 * (1. - v),
                        )))
                    },
                )),
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
    // Their picture once it is in (#89).
    if let Some(picture) = crate::avatars::picture(name) {
        return div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .overflow_hidden()
            .child(gpui::img(picture).size(px(size)).rounded_full());
    }
    const HUES: [u32; 6] = [0x56b6c2, 0x98c379, 0xc678dd, 0xe5c07b, 0xe06c75, 0xd19a66];
    // The colour from the username, when a host named them (#112): the same person looks the same
    // by name or by username; the initial from what is shown.
    let color = if me {
        theme::accent()
    } else {
        let h = crate::avatars::login_of(name)
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

/// A person's card on a long hover over their avatar and name (#51), and when they wrote.
pub type Who = (Card, i64);
/// What builds a card when it shows.
pub type Card = std::rc::Rc<dyn Fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView>;

/// One note: avatar | name, role, time | body below the name. `last` leaves out the timeline line
/// that joins this avatar to the next. `who`: the author's card and the note's time (#53).
#[allow(clippy::too_many_arguments)]
pub fn note(
    author: &str,
    me: bool,
    role: Option<&'static str>,
    time: String,
    body: AnyElement,
    last: bool,
    action: Option<AnyElement>,
    who: Option<Who>,
) -> Div {
    use super::hover::Hover;
    let key = format!("{author}-{time}");
    let (card, when) = match who {
        Some((card, when)) => (Some(card), Some(when)),
        None => (None, None),
    };
    let face = {
        let card = card.clone();
        div()
            .id(SharedString::from(format!("who-face-{key}")))
            .child(avatar(author, me, 28.))
            .when_some(card, |s, card| s.card(move |w, cx| card(w, cx)))
    };
    let name = div()
        .id(SharedString::from(format!("who-name-{key}")))
        .flex_none()
        .font_weight(FontWeight::SEMIBOLD)
        .child(if me {
            "You".to_owned()
        } else {
            author.to_owned()
        })
        .when_some(card, |s, card| s.card(move |w, cx| card(w, cx)));
    // None for a pending comment: its pill says so.
    let stamp = (!time.is_empty()).then(|| {
        div()
            .id(SharedString::from(format!("who-time-{key}")))
            .flex_none()
            .text_size(px(12.))
            .text_color(theme::muted())
            .child(format!("· {time}"))
            .when_some(when, |s, t| s.tip(super::hover::full_date(t)))
    });
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
        .child(face)
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
                        .child(name)
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
                        .children(stamp)
                        // As tall as the name's line: the reaction bar coming on a hover
                        // overhangs it rather than pushing the note down.
                        .children(action.map(|a| {
                            div()
                                .ml_auto()
                                .flex_none()
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .child(a)
                        })),
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
    shell(id, kind)
        .child(label.into())
        .children(key.map(|k| keys::caps(k, if filled { On::Filled } else { On::Plain })))
}

/// A secondary button with its icon before the label (#69, Design shell round 4, C). `bare` — a
/// comment in a narrow diff — keeps only the icon: the label, the key and `about` go to the
/// tooltip. The caller sets no tooltip of its own.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: &'static str,
    label: impl Into<SharedString>,
    key: Option<&'static str>,
    about: Option<&'static str>,
    kind: Kind,
    bare: bool,
) -> Stateful<Div> {
    const GROUP: &str = "discussion-icon-button";
    let label = label.into();
    let ink = match kind {
        Kind::Plain => theme::text(),
        Kind::Quiet => theme::muted(),
        Kind::Danger => theme::removed(),
        Kind::Primary(_) => theme::base(),
    };
    let glyph = super::icons::icon(icon)
        .size(px(if bare { 15. } else { 14. }))
        .text_color(ink)
        .when(matches!(kind, Kind::Quiet), |s| {
            s.group_hover(GROUP, |s| s.text_color(theme::text()))
        });
    let tip: Option<SharedString> = if bare {
        let named = match key {
            Some(k) if k.chars().count() == 1 => format!("{label}  {}", k.to_uppercase()),
            Some(k) => format!("{label}  {k}"),
            None => label.to_string(),
        };
        // "Add to review  ⇧⌘↵ — Pending until you submit the review"
        Some(match about {
            Some(about) => format!("{named} — {about}").into(),
            None => named.into(),
        })
    } else {
        about.map(SharedString::from)
    };
    let filled = matches!(kind, Kind::Primary(_));
    let button = shell(id, kind).group(GROUP).child(glyph).map(|s| {
        if bare {
            s.px(px(8.))
        } else {
            s.child(label)
                .children(key.map(|k| keys::caps(k, if filled { On::Filled } else { On::Plain })))
        }
    });
    match tip {
        Some(tip) => button
            .tooltip_show_delay(super::hover::LONG)
            .tooltip(move |_, cx| cx.new(|_| super::TipOwned(tip.clone())).into()),
        None => button,
    }
}

/// What every discussion button shares: its height, shape and colours by kind. A label never
/// breaks (Design shell round 4).
fn shell(id: impl Into<ElementId>, kind: Kind) -> Stateful<Div> {
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
        .whitespace_nowrap()
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
