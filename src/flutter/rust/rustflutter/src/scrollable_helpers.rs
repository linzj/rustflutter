// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The pieces a scrollable needs that are not the scrollable (upstream
//! `widgets/scrollable_helpers.dart`).
//!
//! Three unrelated things share the file upstream, and they share it here for
//! the same reason -- they are each too small for one of their own:
//!
//! * [`ScrollableDetails`], the description a scrollable hands to whatever is
//!   asking about it.
//! * [`EdgeDraggingAutoScroller`], which scrolls a list while something is
//!   being dragged near its edge.
//! * [`ScrollIntent`] and [`ScrollAction`], which is how a keyboard scrolls.
//!
//! # Recorded divergences
//!
//! * `ScrollableDetails.controller` and `.physics` are one field here rather
//!   than two, and it is not on this struct: upstream's `ScrollController`,
//!   `ScrollPosition`, `ScrollPhysics` and `Scrollable` are all the crate's
//!   [`Scroll`](crate::scrolling::Scroll), and a details object carrying two
//!   handles to the same object twice would be describing the crate's shape
//!   as upstream's. What is left is what actually varies independently: the
//!   direction and the decoration clip.
//! * Upstream's auto-scroller drives itself with an `async` loop that awaits
//!   each animation and then looks again. There is no executor here, so the
//!   loop is turned inside out: [`EdgeDraggingAutoScroller::step`] is one
//!   turn of it, called once a frame by whoever owns the drag -- the same
//!   shape as every other `advance` in this crate.
//! * Upstream's `ScrollAction` is a `ContextAction` and finds the scrollable
//!   through the context. An [`Action`](crate::actions::Action) here is a
//!   plain callback with no context, so the scrollable is named when the
//!   action is built -- the same wiring [`Slider::wired`](crate::components::Slider::wired)
//!   uses, and the reason the primary-scroll-controller fallback upstream
//!   reaches for has nothing to fall back to.

use crate::actions::{Action, Intent};
use crate::animation::Curve;
use crate::engine::Rect;
use crate::framework::StateHandle;
use crate::painting::ClipBehavior;
use crate::render::{Axis, AxisDirection, Offset, axis_direction_to_axis};
use crate::scrolling::{Scroll, ScrollMetrics};

/// Upstream `ScrollableDetails`: which way a scrollable runs, for whatever is
/// asking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollableDetails {
    pub direction: AxisDirection,
    /// Upstream's `decorationClipBehavior`, which clips the decorators around
    /// a scroll view rather than the view itself. Upstream also keeps a
    /// deprecated `clipBehavior` alias for the same field; the alias is not
    /// carried over, because there is nothing here that was written against
    /// the old name.
    pub decoration_clip_behavior: Option<ClipBehavior>,
}

impl ScrollableDetails {
    pub fn new(direction: AxisDirection) -> ScrollableDetails {
        ScrollableDetails {
            direction,
            decoration_clip_behavior: None,
        }
    }

    /// Upstream `ScrollableDetails.vertical`. Reversed means the content
    /// starts at the bottom and grows upwards, which is `up` and not "down,
    /// backwards".
    pub fn vertical(reverse: bool) -> ScrollableDetails {
        ScrollableDetails::new(if reverse {
            AxisDirection::Up
        } else {
            AxisDirection::Down
        })
    }

    /// Upstream `ScrollableDetails.horizontal`.
    pub fn horizontal(reverse: bool) -> ScrollableDetails {
        ScrollableDetails::new(if reverse {
            AxisDirection::Left
        } else {
            AxisDirection::Right
        })
    }

    pub fn with_decoration_clip_behavior(mut self, behavior: ClipBehavior) -> Self {
        self.decoration_clip_behavior = Some(behavior);
        self
    }

    /// Upstream `copyWith`.
    pub fn copy_with(
        &self,
        direction: Option<AxisDirection>,
        decoration_clip_behavior: Option<ClipBehavior>,
    ) -> ScrollableDetails {
        ScrollableDetails {
            direction: direction.unwrap_or(self.direction),
            decoration_clip_behavior: decoration_clip_behavior.or(self.decoration_clip_behavior),
        }
    }

    pub fn axis(&self) -> Axis {
        axis_direction_to_axis(self.direction)
    }
}

/// Upstream `ScrollIncrementType`: how far one keyboard scroll goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ScrollIncrementType {
    /// One line, which upstream fixes at 50 logical pixels.
    #[default]
    Line,
    /// Most of a screenful -- upstream's four fifths of the viewport, so that
    /// a page down leaves a strip of what was there to read against.
    Page,
}

/// Upstream `ScrollIncrementDetails`: what a scrollable is asked when it is
/// allowed to decide its own increment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollIncrementDetails {
    pub increment_type: ScrollIncrementType,
    pub metrics: ScrollMetrics,
}

impl ScrollIncrementDetails {
    pub fn new(
        increment_type: ScrollIncrementType,
        metrics: ScrollMetrics,
    ) -> ScrollIncrementDetails {
        ScrollIncrementDetails {
            increment_type,
            metrics,
        }
    }

    /// Upstream's `_calculateScrollIncrement` default, which is what a
    /// scrollable with no `incrementCalculator` of its own uses.
    pub fn default_increment(&self) -> f32 {
        match self.increment_type {
            ScrollIncrementType::Line => 50.0,
            ScrollIncrementType::Page => 0.8 * self.metrics.viewport_dimension,
        }
    }
}

/// Upstream `ScrollIntent`: scroll, by this much, that way.
///
/// The crate's intents are one closed enum, so this is the constructor for
/// the variant rather than a type of its own -- the same shape
/// [`RequestFocusAction`](crate::actions::RequestFocusAction) and its
/// neighbours have.
pub struct ScrollIntent;

impl ScrollIntent {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(direction: AxisDirection, increment_type: ScrollIncrementType) -> Intent {
        Intent::Scroll {
            direction,
            increment_type,
        }
    }

    /// A one-line scroll, which is upstream's default `type`.
    pub fn line(direction: AxisDirection) -> Intent {
        ScrollIntent::new(direction, ScrollIncrementType::Line)
    }

    pub fn page(direction: AxisDirection) -> Intent {
        ScrollIntent::new(direction, ScrollIncrementType::Page)
    }
}

/// Upstream `ScrollAction`.
pub struct ScrollAction;

impl ScrollAction {
    /// Upstream `ScrollAction.getDirectionalIncrement`.
    ///
    /// Nothing happens when the intent's axis is not the scrollable's: a left
    /// arrow in a vertical list is not a small upward scroll, it is a key for
    /// something else.
    pub fn directional_increment(
        axis_direction: AxisDirection,
        metrics: ScrollMetrics,
        direction: AxisDirection,
        increment_type: ScrollIncrementType,
    ) -> f32 {
        if axis_direction_to_axis(direction) != axis_direction_to_axis(axis_direction) {
            return 0.0;
        }
        let increment = ScrollIncrementDetails::new(increment_type, metrics).default_increment();
        if direction == axis_direction {
            increment
        } else {
            -increment
        }
    }

    /// Upstream's animation: a tenth of a second, eased at both ends.
    pub const DURATION_MICROS: i64 = 100_000;

    /// Builds the action for the scrollable reached through `handle`.
    ///
    /// `axis_direction` is which way that scrollable runs, which upstream
    /// reads off the `ScrollableState` it found in the context.
    pub fn new<S: 'static>(
        handle: StateHandle<S>,
        scroll: fn(&mut S) -> &mut Scroll,
        axis_direction: AxisDirection,
    ) -> Action {
        Action::callback(move |intent| {
            let Intent::Scroll {
                direction,
                increment_type,
            } = intent
            else {
                return None;
            };
            let (direction, increment_type) = (*direction, *increment_type);
            handle.set_state(move |state| {
                let scroll = scroll(state);
                let increment = ScrollAction::directional_increment(
                    axis_direction,
                    scroll.metrics(),
                    direction,
                    increment_type,
                );
                if increment == 0.0 {
                    return;
                }
                let target = (scroll.offset + increment).clamp(0.0, scroll.max_extent().max(0.0));
                scroll.animate_to(
                    target,
                    ScrollAction::DURATION_MICROS,
                    Curve::Cubic(0.42, 0.0, 0.58, 1.0),
                );
            });
            None
        })
    }
}

/// Upstream `EdgeDraggingAutoScroller`: scrolls a list while something is
/// dragged near its edge.
///
/// The behaviour a reorderable list needs and cannot get from the drag alone:
/// a finger held at the bottom of a list has stopped moving, so nothing else
/// would ever ask the list to scroll, and the item being dragged can never
/// reach anything below the fold.
pub struct EdgeDraggingAutoScroller {
    /// Which way the list it is scrolling runs.
    axis_direction: AxisDirection,
    /// Upstream's `velocityScalar`: how many screenfuls a second the scroll
    /// runs at, which becomes the duration of each step.
    velocity_scalar: f32,
    /// The dragged thing's box in the scrollable's coordinates, as
    /// [`start_auto_scroll_if_necessary`](EdgeDraggingAutoScroller::start_auto_scroll_if_necessary)
    /// last saw it.
    drag_target: Option<Rect>,
    scrolling: bool,
}

impl EdgeDraggingAutoScroller {
    /// Upstream's largest single step, which is what keeps a drag held far
    /// past the edge from scrolling arbitrarily fast.
    pub const OVER_DRAG_MAX: f32 = 20.0;

    pub fn new(axis_direction: AxisDirection, velocity_scalar: f32) -> EdgeDraggingAutoScroller {
        EdgeDraggingAutoScroller {
            axis_direction,
            velocity_scalar,
            drag_target: None,
            scrolling: false,
        }
    }

    pub fn scrolling(&self) -> bool {
        self.scrolling
    }

    /// Upstream `startAutoScrollIfNecessary`. The rectangle is remembered
    /// whether or not this starts anything, because a scroll already running
    /// picks the new one up on its next step -- which is upstream's comment
    /// on the early return.
    pub fn start_auto_scroll_if_necessary(&mut self, drag_target: Rect) {
        self.drag_target = Some(drag_target);
        self.scrolling = true;
    }

    /// Upstream `stopAutoScroll`.
    pub fn stop_auto_scroll(&mut self) {
        self.scrolling = false;
        self.drag_target = None;
    }

    /// How far the offset should move this step, or nothing if the drag is
    /// not near enough an edge for the list to follow it.
    ///
    /// Upstream's `_scroll`, less the awaiting. `viewport` is the
    /// scrollable's own box in the same coordinates the drag target is in.
    pub fn next_offset(&self, scroll: &Scroll, viewport: Rect) -> Option<f32> {
        let target = self.drag_target?;
        if !self.scrolling {
            return None;
        }
        let axis = axis_direction_to_axis(self.axis_direction);
        let extent = |offset: Offset| match axis {
            Axis::Horizontal => offset.dx,
            Axis::Vertical => offset.dy,
        };
        let viewport_start = extent(Offset::new(viewport.left, viewport.top));
        let viewport_end = viewport_start
            + match axis {
                Axis::Horizontal => viewport.width(),
                Axis::Vertical => viewport.height(),
            };
        let proxy_start = extent(Offset::new(target.left, target.top));
        let proxy_end = extent(Offset::new(target.right, target.bottom));

        let pixels = scroll.offset;
        let min = 0.0;
        let max = scroll.max_extent();
        // Which end of the drag pushes which way depends on the axis
        // direction: in a reversed list, running off the far end scrolls
        // backwards.
        let (backwards_when_past_end, _) = match self.axis_direction {
            AxisDirection::Up | AxisDirection::Left => (true, ()),
            AxisDirection::Down | AxisDirection::Right => (false, ()),
        };
        let new_offset = if backwards_when_past_end {
            if proxy_end > viewport_end && pixels > min {
                Some((pixels - (proxy_end - viewport_end).min(Self::OVER_DRAG_MAX)).max(min))
            } else if proxy_start < viewport_start && pixels < max {
                Some((pixels + (viewport_start - proxy_start).min(Self::OVER_DRAG_MAX)).min(max))
            } else {
                None
            }
        } else if proxy_start < viewport_start && pixels > min {
            Some((pixels - (viewport_start - proxy_start).min(Self::OVER_DRAG_MAX)).max(min))
        } else if proxy_end > viewport_end && pixels < max {
            Some((pixels + (proxy_end - viewport_end).min(Self::OVER_DRAG_MAX)).min(max))
        } else {
            None
        };
        // Upstream's last guard: a step of less than a pixel is not a scroll,
        // and taking it would spin the loop forever at the end of the travel.
        match new_offset {
            Some(offset) if (offset - pixels).abs() >= 1.0 => Some(offset),
            _ => None,
        }
    }

    /// One turn of upstream's loop: works out the next offset and starts the
    /// animation towards it. Returns whether it did anything, so a caller can
    /// stop asking.
    pub fn step(&mut self, scroll: &mut Scroll, viewport: Rect) -> bool {
        let Some(offset) = self.next_offset(scroll, viewport) else {
            self.scrolling = false;
            return false;
        };
        let duration_micros = (1_000_000.0 / self.velocity_scalar).round() as i64;
        scroll.animate_to(offset, duration_micros.max(1), Curve::Linear);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(pixels: f32, max: f32, viewport: f32) -> ScrollMetrics {
        ScrollMetrics {
            pixels,
            min_scroll_extent: 0.0,
            max_scroll_extent: max,
            viewport_dimension: viewport,
        }
    }

    #[test]
    fn reversing_a_scrollable_changes_the_direction_not_a_flag() {
        // Upstream's named constructors: reversed vertical is `up`, not
        // "down, backwards". Everything downstream reads the direction, so a
        // reverse flag kept beside it would have to be applied by each of
        // them, and one of them would forget.
        assert_eq!(
            ScrollableDetails::vertical(false).direction,
            AxisDirection::Down
        );
        assert_eq!(
            ScrollableDetails::vertical(true).direction,
            AxisDirection::Up
        );
        assert_eq!(
            ScrollableDetails::horizontal(false).direction,
            AxisDirection::Right
        );
        assert_eq!(
            ScrollableDetails::horizontal(true).direction,
            AxisDirection::Left
        );
        // Both directions on an axis are that axis.
        assert_eq!(ScrollableDetails::vertical(true).axis(), Axis::Vertical);
        assert_eq!(ScrollableDetails::vertical(false).axis(), Axis::Vertical);
    }

    #[test]
    fn a_page_is_most_of_the_viewport_and_a_line_is_a_fixed_fifty() {
        // The line increment does not scale with the viewport and the page
        // one does; and the page is four fifths, not all of it, so that a
        // strip of what was on screen stays to read against.
        let small =
            ScrollIncrementDetails::new(ScrollIncrementType::Line, metrics(0.0, 500.0, 300.0));
        let large =
            ScrollIncrementDetails::new(ScrollIncrementType::Line, metrics(0.0, 500.0, 900.0));
        assert_eq!(small.default_increment(), 50.0);
        assert_eq!(large.default_increment(), 50.0);

        let page =
            ScrollIncrementDetails::new(ScrollIncrementType::Page, metrics(0.0, 500.0, 300.0));
        assert_eq!(page.default_increment(), 240.0);
        assert!(page.default_increment() < 300.0);
    }

    #[test]
    fn a_key_for_the_other_axis_scrolls_nothing() {
        // A left arrow in a vertical list is not a small upward scroll; it is
        // a key for something else, and returning zero is what lets the next
        // handler have it.
        assert_eq!(
            ScrollAction::directional_increment(
                AxisDirection::Down,
                metrics(0.0, 500.0, 300.0),
                AxisDirection::Left,
                ScrollIncrementType::Line,
            ),
            0.0
        );
        // Along the axis, and with it: forwards.
        assert_eq!(
            ScrollAction::directional_increment(
                AxisDirection::Down,
                metrics(0.0, 500.0, 300.0),
                AxisDirection::Down,
                ScrollIncrementType::Line,
            ),
            50.0
        );
        // Along the axis, against it: backwards by the same amount.
        assert_eq!(
            ScrollAction::directional_increment(
                AxisDirection::Down,
                metrics(0.0, 500.0, 300.0),
                AxisDirection::Up,
                ScrollIncrementType::Line,
            ),
            -50.0
        );
    }

    #[test]
    fn a_reversed_list_scrolls_the_other_way_for_the_same_key() {
        // The increment is measured against the list's own direction, not the
        // screen's. In an `up` list a down arrow moves the offset backwards,
        // which is what makes a reversed chat log scroll the way it looks.
        assert_eq!(
            ScrollAction::directional_increment(
                AxisDirection::Up,
                metrics(0.0, 500.0, 300.0),
                AxisDirection::Down,
                ScrollIncrementType::Line,
            ),
            -50.0
        );
    }

    fn scroll_at(offset: f32, extent: f32, viewport: f32) -> Scroll {
        let mut scroll = Scroll::new();
        scroll.set_extent(extent, viewport);
        scroll.jump_to(offset);
        scroll
    }

    /// A 100-tall viewport at the origin.
    const VIEWPORT: Rect = Rect::ltrb(0.0, 0.0, 50.0, 100.0);

    #[test]
    fn a_drag_in_the_middle_of_the_list_scrolls_nothing() {
        let mut scroller = EdgeDraggingAutoScroller::new(AxisDirection::Down, 1.0);
        scroller.start_auto_scroll_if_necessary(Rect::ltrb(0.0, 40.0, 50.0, 60.0));
        let scroll = scroll_at(200.0, 500.0, 100.0);
        assert_eq!(scroller.next_offset(&scroll, VIEWPORT), None);
    }

    #[test]
    fn a_drag_past_an_edge_scrolls_towards_it_by_at_most_the_cap() {
        // Held far past the bottom, the step is capped: upstream's
        // `overDragMax` is what keeps a finger parked off-screen from
        // scrolling arbitrarily fast.
        let mut scroller = EdgeDraggingAutoScroller::new(AxisDirection::Down, 1.0);
        scroller.start_auto_scroll_if_necessary(Rect::ltrb(0.0, 80.0, 50.0, 400.0));
        let scroll = scroll_at(200.0, 500.0, 100.0);
        assert_eq!(
            scroller.next_offset(&scroll, VIEWPORT),
            Some(200.0 + EdgeDraggingAutoScroller::OVER_DRAG_MAX)
        );

        // Just past it, the step is the overhang itself rather than the cap.
        scroller.start_auto_scroll_if_necessary(Rect::ltrb(0.0, 80.0, 50.0, 105.0));
        assert_eq!(scroller.next_offset(&scroll, VIEWPORT), Some(205.0));
    }

    #[test]
    fn a_list_already_at_its_end_does_not_scroll_further() {
        let mut scroller = EdgeDraggingAutoScroller::new(AxisDirection::Down, 1.0);
        scroller.start_auto_scroll_if_necessary(Rect::ltrb(0.0, 80.0, 50.0, 400.0));
        let at_end = scroll_at(400.0, 400.0, 100.0);
        assert_eq!(scroller.next_offset(&at_end, VIEWPORT), None);
        // And one at the top does not scroll backwards.
        scroller.start_auto_scroll_if_necessary(Rect::ltrb(0.0, -300.0, 50.0, 20.0));
        let at_start = scroll_at(0.0, 400.0, 100.0);
        assert_eq!(scroller.next_offset(&at_start, VIEWPORT), None);
    }

    #[test]
    fn a_reversed_list_reads_the_same_overhang_the_other_way() {
        // In an `up` list, running off the *far* end scrolls backwards. Both
        // branches of upstream's switch are here because a reversed list gets
        // them the wrong way round otherwise, and nothing else would notice.
        let target = Rect::ltrb(0.0, 80.0, 50.0, 400.0);
        let scroll = scroll_at(200.0, 500.0, 100.0);

        let mut down = EdgeDraggingAutoScroller::new(AxisDirection::Down, 1.0);
        down.start_auto_scroll_if_necessary(target);
        let mut up = EdgeDraggingAutoScroller::new(AxisDirection::Up, 1.0);
        up.start_auto_scroll_if_necessary(target);

        assert_eq!(down.next_offset(&scroll, VIEWPORT), Some(220.0));
        assert_eq!(up.next_offset(&scroll, VIEWPORT), Some(180.0));
    }

    #[test]
    fn a_step_of_less_than_a_pixel_is_not_a_step() {
        // Upstream's last guard. Without it the loop keeps asking for a
        // scroll it has already arrived at, and never stops.
        let mut scroller = EdgeDraggingAutoScroller::new(AxisDirection::Down, 1.0);
        scroller.start_auto_scroll_if_necessary(Rect::ltrb(0.0, 80.0, 50.0, 100.5));
        let scroll = scroll_at(200.0, 500.0, 100.0);
        assert_eq!(scroller.next_offset(&scroll, VIEWPORT), None);
    }

    #[test]
    fn stopping_forgets_the_drag_and_a_step_that_does_nothing_stops_itself() {
        let mut scroller = EdgeDraggingAutoScroller::new(AxisDirection::Down, 1.0);
        scroller.start_auto_scroll_if_necessary(Rect::ltrb(0.0, 80.0, 50.0, 400.0));
        assert!(scroller.scrolling());
        let mut scroll = scroll_at(200.0, 500.0, 100.0);
        assert!(scroller.step(&mut scroll, VIEWPORT));
        assert!(scroller.scrolling());

        // A drag that has come back inside stops the scroll rather than
        // leaving it running at zero.
        scroller.start_auto_scroll_if_necessary(Rect::ltrb(0.0, 40.0, 50.0, 60.0));
        assert!(!scroller.step(&mut scroll, VIEWPORT));
        assert!(!scroller.scrolling());

        scroller.start_auto_scroll_if_necessary(Rect::ltrb(0.0, 80.0, 50.0, 400.0));
        scroller.stop_auto_scroll();
        assert!(!scroller.scrolling());
        assert_eq!(scroller.next_offset(&scroll, VIEWPORT), None);
    }

    #[test]
    fn a_scroll_intent_is_a_line_unless_it_says_otherwise() {
        // `Intent` is not `PartialEq` -- one of its variants carries a
        // callback -- so the variant is read out rather than compared.
        let Intent::Scroll {
            direction,
            increment_type,
        } = ScrollIntent::line(AxisDirection::Down)
        else {
            panic!("a scroll intent should be a scroll intent");
        };
        assert_eq!(direction, AxisDirection::Down);
        assert_eq!(increment_type, ScrollIncrementType::Line);

        let Intent::Scroll { increment_type, .. } = ScrollIntent::page(AxisDirection::Down) else {
            unreachable!()
        };
        assert_eq!(increment_type, ScrollIncrementType::Page);
        assert_eq!(
            ScrollIntent::line(AxisDirection::Down).action_name(),
            "Scroll"
        );
    }
}
