//! Micro animations (#56, Design `tokens.css`: `--motion-fast`, `--motion-base`, `--motion-ease`),
//! one helper for the app. Something that flips between two states (a fold, a width) moves on a
//! critically damped spring keyed by a stable id: it does not move when first drawn, and a second
//! flip midway turns it back from where it is. Something that appears (a menu, a card, a toast)
//! plays once from its first frame. Nothing waits for a motion: the state changes at once, only
//! the drawing follows. All of it stops while GPUI's reduce motion is set — macOS Reduce motion,
//! Settings → Appearance → Animations off, or a snapshot, which draws the end state.

use gpui::{Animation, AnimationExt, App, ElementId, IntoElement, SpringAnimation, SpringConfig};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::{Duration, Instant};

/// `--motion-fast`: tooltips, cards, menus, pills, chevrons.
pub const FAST: Duration = Duration::from_millis(120);
/// `--motion-base`: panes — side sections, the menu of spaces, tool windows, zen.
pub const BASE: Duration = Duration::from_millis(200);

/// Settings → Appearance → Animations, shared by every window.
static ON: AtomicBool = AtomicBool::new(true);

/// Keeps Settings' choice and sets GPUI's reduce motion from it, the system's and a snapshot's.
pub fn set(animations: bool, cx: &mut App) {
    ON.store(animations, Relaxed);
    refresh(cx);
}

/// Reads the system's Reduce motion again (the app's beat calls it).
pub fn refresh(cx: &mut App) {
    // A snapshot draws the end state, unless it is asked to show motion midway.
    let snapshot = cfg!(feature = "snapshot")
        && std::env::var_os("GITLANCE_SNAPSHOT").is_some()
        && std::env::var_os("GITLANCE_SNAPSHOT_MOTION").is_none();
    cx.set_reduce_motion(!ON.load(Relaxed) || snapshot || crate::platform::reduce_motion());
}

/// `--motion-ease`, cubic-bezier(0.2, 0, 0.2, 1), at time `t` (0…1).
pub fn ease(t: f32) -> f32 {
    bezier((0.2, 0.0), (0.2, 1.0), t.clamp(0., 1.))
}

/// The CSS cubic Bézier through (0, 0), `a`, `b`, (1, 1): its height where its x is `t`.
fn bezier(a: (f32, f32), b: (f32, f32), t: f32) -> f32 {
    let at = |p1: f32, p2: f32, s: f32| {
        let r = 1. - s;
        3. * r * r * s * p1 + 3. * r * s * s * p2 + s * s * s
    };
    // The curve's parameter where x is t, by bisection: x grows with it.
    let (mut lo, mut hi) = (0f32, 1f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.;
        if at(a.0, b.0, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(a.1, b.1, (lo + hi) / 2.)
}

/// A critically damped spring within 4 % of its target after `duration`: x(t) = (1 + ωt)·e^(−ωt).
pub fn spring(duration: Duration) -> SpringConfig {
    let w = 5.0 / duration.as_secs_f32();
    SpringConfig::new(w * w, 2. * w, 1.)
}

/// `element` on its way to on (1) or off (0); `draw` gets where it is, 0…1. Drawn the first time
/// it is where it goes, unless `entering`: then it comes from the other end (a pane just opened,
/// one just closed and drawn on its way out).
pub fn toward<E: IntoElement + 'static>(
    element: E,
    id: impl Into<ElementId>,
    on: bool,
    entering: bool,
    duration: Duration,
    draw: impl FnOnce(E, f32) -> E + 'static,
) -> impl IntoElement {
    let target = if on { 1f32 } else { 0. };
    let mut spring = SpringAnimation::new(spring(duration)).to(target);
    if entering {
        spring = spring.from(1. - target);
    }
    element.with_spring(id, spring, move |e, v| draw(e, v.clamp(0., 1.)))
}

/// `element` on its way to `value` (a share, a position); `draw` gets where it is.
pub fn toward_value<E: IntoElement + 'static>(
    element: E,
    id: impl Into<ElementId>,
    value: f32,
    duration: Duration,
    draw: impl FnOnce(E, f32) -> E + 'static,
) -> impl IntoElement {
    element.with_spring(id, SpringAnimation::new(spring(duration)).to(value), draw)
}

/// When things last opened or closed, to draw them coming and going: a pane closed stays drawn on
/// its way out until its time is up.
#[derive(Default)]
pub struct Marks(HashMap<String, Instant>);

impl Marks {
    pub fn mark(&mut self, key: impl Into<String>) {
        self.0.retain(|_, at| at.elapsed() < Duration::from_secs(2));
        self.0.insert(key.into(), Instant::now());
    }

    /// Changed within its motion's time.
    pub fn recent(&self, key: &str) -> bool {
        self.0
            .get(key)
            .is_some_and(|at| at.elapsed() < BASE + Duration::from_millis(80))
    }
}

/// `element` coming in: `draw` gets 0 at its first frame, 1 after `duration`, on the curve.
pub fn appear<E: IntoElement + 'static>(
    element: E,
    id: impl Into<ElementId>,
    duration: Duration,
    draw: impl Fn(E, f32) -> E + 'static,
) -> impl IntoElement {
    element.with_animation(id, Animation::new(duration).with_easing(ease), draw)
}

/// A quiet shimmer in place of "Loading…" (#56, 18): `lines` bars of code-like lengths whose
/// light comes and goes; with reduce motion they stand still.
pub fn shimmer(id: impl Into<ElementId>, lines: usize) -> gpui::AnyElement {
    use gpui::{div, prelude::*};
    const WIDTHS: [f32; 7] = [0.62, 0.48, 0.71, 0.36, 0.55, 0.67, 0.42];
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(super::px(10.))
        .children((0..lines).map(|n| {
            div()
                .h(super::px(10.))
                .w(gpui::relative(WIDTHS[n % WIDTHS.len()]))
                .rounded(super::px(4.))
                .bg(super::theme::hover())
        }))
        .with_animation(
            id,
            Animation::new(Duration::from_millis(1400))
                .repeat()
                .with_easing(gpui::pulsating_between(0.45, 1.)),
            |bars, v| bars.opacity(v),
        )
        .into_any_element()
}

impl super::Workspace {
    /// Folder `dir` of a tree opened: the rows it shows fade in. A tree's rows keep one height
    /// (a uniform list), so only their alpha moves (#110).
    pub(super) fn tree_opening(&mut self, dir: &str, cx: &mut gpui::Context<Self>) {
        let generation = self.tree_opened.as_ref().map_or(0, |(_, g)| g + 1);
        self.tree_opened = Some((dir.to_owned(), generation));
        self.mark_motion("tree-open", cx);
    }

    /// Row `path` of a tree, fading in when a folder just opened above it.
    pub(super) fn tree_row(&self, path: &str, row: gpui::AnyElement) -> gpui::AnyElement {
        use gpui::{div, prelude::*};
        match &self.tree_opened {
            Some((dir, generation))
                if self.motion.recent("tree-open")
                    && path
                        .strip_prefix(dir.as_str())
                        .is_some_and(|r| r.starts_with('/')) =>
            {
                appear(
                    div().child(row),
                    gpui::SharedString::from(format!("tree-in-{generation}-{path}")),
                    BASE,
                    |row, v| row.opacity(v),
                )
                .into_any_element()
            }
            _ => row,
        }
    }

    /// Something that folds open and shut (#110; the developer, 2026-10-23: every fold moves, in
    /// size and in alpha): it grows from nothing to the height its content was last drawn at,
    /// fading in, and back the same way. `moving` names the mark its toggle set
    /// (`mark_motion`); shut and still, it is not drawn. Open and still, it takes whatever height
    /// its content has.
    pub(super) fn folding(
        &self,
        key: impl Into<String>,
        moving: &str,
        open: bool,
        content: impl IntoElement + 'static,
    ) -> Option<gpui::AnyElement> {
        use gpui::{div, prelude::*};
        let moving = self.motion.recent(moving);
        if !open && !moving {
            return None;
        }
        let key: String = key.into();
        let height = self.heights.get(&key);
        let inner = div()
            .relative()
            .flex_none()
            .child(content)
            .child(self.heights.measure(key.clone()));
        Some(
            toward(
                div().flex_none().overflow_hidden().child(inner),
                gpui::SharedString::from(format!("folding-{key}")),
                open,
                moving,
                BASE,
                move |fold, v| {
                    // Not yet measured, it starts from nothing; open and still, it is free.
                    let fold = if v >= 0.999 {
                        fold
                    } else {
                        fold.max_h(super::px(height * v))
                    };
                    fold.opacity(v)
                },
            )
            .into_any_element(),
        )
    }
}

/// `from` to `to` at `v`.
pub fn mix(from: f32, to: f32, v: f32) -> f32 {
    from + (to - from) * v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_starts_slow_ends_settled_and_leads_the_clock() {
        assert!(ease(0.).abs() < 1e-4 && (ease(1.) - 1.).abs() < 1e-4);
        // An ease-out: past three quarters of the way at half the time.
        assert!(ease(0.5) > 0.75, "{}", ease(0.5));
        let mut last = 0.;
        for i in 1..=20 {
            let v = ease(i as f32 / 20.);
            assert!(v >= last);
            last = v;
        }
    }

    #[test]
    fn turned_around_midway_it_goes_back_from_where_it_is() {
        // Folding, then unfolding a moment later: no jump, and it heads back at once.
        let config = spring(BASE);
        let rest = gpui::SpringState {
            position: 1.,
            velocity: 0.,
        };
        let midway = config.step(rest, 0., 0.05);
        assert!(
            midway.position > 0.1 && midway.position < 0.9,
            "{}",
            midway.position
        );
        let next = config.step(midway, 1., 1. / 120.);
        assert!((next.position - midway.position).abs() < 0.1);
        let later = config.step(midway, 1., 0.1);
        assert!(later.position > midway.position);
        let end = config.step(midway, 1., BASE.as_secs_f32() * 2.);
        assert!((end.position - 1.).abs() < 0.02, "{}", end.position);
    }

    #[test]
    fn a_mark_lasts_its_motion() {
        let mut marks = Marks::default();
        assert!(!marks.recent("side"));
        marks.mark("side");
        assert!(marks.recent("side") && !marks.recent("tool"));
        marks.0.insert("side".into(), Instant::now() - BASE * 2);
        assert!(!marks.recent("side"));
    }

    #[test]
    fn the_spring_is_critically_damped_and_settles_in_its_time() {
        let config = spring(BASE);
        let (_, zeta) = config.canonical();
        assert!((zeta - 1.).abs() < 1e-3);
        let start = gpui::SpringState {
            position: 0.,
            velocity: 0.,
        };
        let end = config.step(start, 1., BASE.as_secs_f32());
        assert!(
            end.position > 0.95 && end.position <= 1.0001,
            "{}",
            end.position
        );
    }
}
