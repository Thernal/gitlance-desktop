//! Row anatomy (#79): a row shows what it always shows on one line, cut; the rest by priority as
//! its list has room — the third-priority parts leave under 360 px, the second under 260 px; and
//! its tooltip holds everything. A list is a container: a row reacts to its list's width, not the
//! window's. Each list measures itself as it is drawn (`measure`) and its rows read the width at
//! the next frame (`room`).

use gpui::{IntoElement, Styled, canvas};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// How much of a row its list has room for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Room {
    /// Under 260 px: what the row always shows.
    Least,
    /// Under 360 px: the second-priority parts too.
    Some,
    /// Every part.
    All,
}

impl Room {
    /// The room a list `width` px wide gives; not measured yet (0), all of it.
    pub fn of(width: f32) -> Room {
        if width <= 0. || width >= 360. {
            Room::All
        } else if width >= 260. {
            Room::Some
        } else {
            Room::Least
        }
    }

    /// Room for the second-priority parts.
    pub fn second(self) -> bool {
        self >= Room::Some
    }

    /// Room for the third-priority parts.
    pub fn third(self) -> bool {
        self == Room::All
    }
}

/// The lists' widths as last drawn, by name.
#[derive(Clone, Default)]
pub struct Widths(Rc<RefCell<HashMap<&'static str, f32>>>);

impl Widths {
    /// List `list`'s width as last drawn; 0 before it is.
    pub fn width(&self, list: &'static str) -> f32 {
        self.0.borrow().get(list).copied().unwrap_or(0.)
    }

    pub fn room(&self, list: &'static str) -> Room {
        Room::of(self.width(list))
    }

    /// Laid over list `list` (its parent `relative`): measures it as it is drawn; a new width
    /// draws the window again, so its rows take it.
    pub fn measure(&self, list: &'static str) -> impl IntoElement {
        let widths = self.0.clone();
        canvas(
            move |bounds, window, _| {
                let width = f32::from(bounds.size.width);
                let old = widths.borrow_mut().insert(list, width);
                if old.is_none_or(|old| (old - width).abs() > 0.5) {
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
}

/// What folds open and shut, its height as last drawn open, by name (#110): a fold grows to it.
#[derive(Clone, Default)]
pub struct Heights(Rc<RefCell<HashMap<String, f32>>>);

impl Heights {
    /// `key`'s height as last drawn; 0 before it is.
    pub fn get(&self, key: &str) -> f32 {
        self.0.borrow().get(key).copied().unwrap_or(0.)
    }

    /// Laid over the content `key` (its parent `relative`): measures it as it is drawn; a new
    /// height draws the window again.
    pub fn measure(&self, key: String) -> impl IntoElement {
        let heights = self.0.clone();
        canvas(
            move |bounds, window, _| {
                let height = f32::from(bounds.size.height);
                let old = heights.borrow_mut().insert(key.clone(), height);
                if old.is_none_or(|old| (old - height).abs() > 0.5) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_third_leaves_first_then_the_second() {
        assert_eq!(Room::of(420.), Room::All);
        assert_eq!(Room::of(360.), Room::All);
        assert_eq!(Room::of(300.), Room::Some);
        assert_eq!(Room::of(260.), Room::Some);
        assert_eq!(Room::of(220.), Room::Least);
        // Not drawn yet: everything, until the list says otherwise.
        assert_eq!(Room::of(0.), Room::All);
        assert!(Room::Some.second() && !Room::Some.third());
        assert!(!Room::Least.second());
    }
}
