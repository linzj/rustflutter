//! A port of the state classes in `widgets/scrollable.dart`:
//! `ScrollableState`, `TwoDimensionalScrollable` and
//! `TwoDimensionalScrollableState`.
//!
//! `Scrollable` itself is mapped in the ledger to [`crate::scrolling::Scroll`],
//! which plays the same part -- holds the position, builds the viewport, takes
//! the gestures. What was missing is the *state*: where the scroll origin is
//! relative to the axis, how a request to show something walks outward through
//! every scrollable above it, and how two of them are put together to scroll in
//! two dimensions at once.

use crate::render::{Axis, AxisDirection};
use crate::scrollable_helpers::ScrollableDetails;
use crate::two_dimensional::DiagonalDragBehavior;

/// Upstream `ScrollPositionAlignmentPolicy`, as `ensureVisible` takes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollAlignmentPolicy {
    /// Scroll by exactly what the alignment asks for, in either direction.
    #[default]
    Explicit,
    /// Only ever scroll so the target's leading edge comes into view.
    KeepVisibleAtStart,
    /// Only ever scroll so its trailing edge does.
    KeepVisibleAtEnd,
}

/// Upstream `ScrollableState`, as much of it as does not need an element tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollableState {
    pub axis_direction: AxisDirection,
    /// Where the content is, in pixels.
    pub pixels: f32,
    /// Whether a controller was passed in. When none was, upstream builds a
    /// fallback and owns it -- and owning it is the whole difference, because
    /// what it built it must also dispose.
    pub has_controller: bool,
}

impl ScrollableState {
    pub fn new(axis_direction: AxisDirection) -> ScrollableState {
        ScrollableState {
            axis_direction,
            pixels: 0.0,
            has_controller: false,
        }
    }

    pub fn with_pixels(mut self, pixels: f32) -> Self {
        self.pixels = pixels;
        self
    }

    pub fn axis(&self) -> Axis {
        crate::render::axis_direction_to_axis(self.axis_direction)
    }

    /// Whether this scrollable owns a controller it has to dispose. Upstream's
    /// `_fallbackScrollController`.
    pub fn owns_its_controller(&self) -> bool {
        !self.has_controller
    }

    /// Upstream `deltaToScrollOrigin`: how far the content has been carried
    /// from where a scroll offset of zero would put it, as an offset in the
    /// viewport's own coordinates.
    ///
    /// The sign is the part worth having written down. Scroll offset always
    /// grows in the axis direction, but the *screen* offset only agrees with it
    /// for `down` and `right` -- a list running upwards has moved its content
    /// **negatively** in y by the same number of pixels.
    pub fn delta_to_scroll_origin(&self) -> (f32, f32) {
        match self.axis_direction {
            AxisDirection::Up => (0.0, -self.pixels),
            AxisDirection::Down => (0.0, self.pixels),
            AxisDirection::Left => (-self.pixels, 0.0),
            AxisDirection::Right => (self.pixels, 0.0),
        }
    }
}

/// One step of a walk outward through nested scrollables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnsureVisibleStep {
    /// Which scrollable this step moved, innermost first.
    pub scrollable: u64,
    /// What this step scrolls to: the caller's own render object at the
    /// innermost level, and the scrollable below it at every level after.
    pub scrolls_to_inner_scrollable: bool,
    /// Whether the original target travels along as a preference. It is
    /// recorded on the way past the first level and held fixed from there on.
    pub carries_original_target: bool,
}

/// Upstream `Scrollable.ensureVisible`, as the walk it performs.
///
/// It does not stop at the nearest scrollable. It keeps going outward through
/// every one above it, because a row inside a list inside a page needs all
/// three to move before anybody can see it.
///
/// The subtlety is `targetRenderObject`, which is recorded **once** and then
/// held fixed for the rest of the walk. Upstream's comment gives the reason and
/// links the issue: the innermost target is made as visible as it can be, and
/// only once it already is does an outer scrollable get to maximise the *inner
/// scrollable's* visibility instead. Without that, each level would re-aim at
/// something bigger and the thing the caller actually asked about could end up
/// off screen.
///
/// `chain` is the scrollables from innermost outward.
pub fn ensure_visible(chain: &[u64]) -> Vec<EnsureVisibleStep> {
    chain
        .iter()
        .enumerate()
        .map(|(index, scrollable)| EnsureVisibleStep {
            scrollable: *scrollable,
            scrolls_to_inner_scrollable: index > 0,
            carries_original_target: index > 0,
        })
        .collect()
}

/// Upstream's early returns in `ensureVisible`: nothing to wait for when there
/// was nothing to do, and no need to combine one future with itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnsureVisibleResult {
    /// No scrollable moved, or the move was instantaneous.
    Immediate,
    /// Exactly one scrollable animated.
    Single,
    /// Several animated, and the caller waits for all of them.
    Several,
}

/// Upstream's tail of `ensureVisible`.
pub fn ensure_visible_result(moved: usize, duration_ms: f32) -> EnsureVisibleResult {
    if moved == 0 || duration_ms == 0.0 {
        return EnsureVisibleResult::Immediate;
    }
    if moved == 1 {
        EnsureVisibleResult::Single
    } else {
        EnsureVisibleResult::Several
    }
}

/// Upstream `TwoDimensionalScrollable`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwoDimensionalScrollable {
    pub horizontal_details: ScrollableDetails,
    pub vertical_details: ScrollableDetails,
    /// How a drag that is going both ways at once should be resolved. Defaults
    /// to `None`, which locks to whichever axis the finger committed to first.
    pub diagonal_drag_behavior: DiagonalDragBehavior,
}

impl TwoDimensionalScrollable {
    pub fn new() -> TwoDimensionalScrollable {
        TwoDimensionalScrollable {
            horizontal_details: ScrollableDetails::horizontal(false),
            vertical_details: ScrollableDetails::vertical(false),
            diagonal_drag_behavior: DiagonalDragBehavior::None,
        }
    }

    pub fn with_diagonal(mut self, behavior: DiagonalDragBehavior) -> Self {
        self.diagonal_drag_behavior = behavior;
        self
    }

    /// Upstream's two asserts in `build`, which catch a detail that names the
    /// wrong axis -- a `verticalDetails` pointing left would otherwise build a
    /// scrollable that silently disagrees with the one beside it.
    pub fn validate(&self) -> Result<(), &'static str> {
        if crate::render::axis_direction_to_axis(self.vertical_details.direction) != Axis::Vertical
        {
            return Err("TwoDimensionalScrollable.verticalDetails are not Axis.vertical.");
        }
        if crate::render::axis_direction_to_axis(self.horizontal_details.direction)
            != Axis::Horizontal
        {
            return Err("TwoDimensionalScrollable.horizontalDetails are not Axis.horizontal.");
        }
        Ok(())
    }
}

impl Default for TwoDimensionalScrollable {
    fn default() -> Self {
        TwoDimensionalScrollable::new()
    }
}

/// Which of the two nested scrollables is which. Upstream names them in the
/// keys it builds with: `_verticalOuterScrollableKey` and
/// `_horizontalInnerScrollableKey`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollableNesting {
    /// The vertical one, built first, wrapping everything.
    VerticalOuter,
    /// The horizontal one, built inside the vertical one's viewport builder.
    HorizontalInner,
}

/// Upstream `TwoDimensionalScrollableState`.
///
/// Two ordinary `Scrollable`s, one inside the other's viewport, and **the
/// nesting order is not arbitrary**: vertical outside, horizontal inside. Each
/// is handed the other's key, which upstream labels "for gesture forwarding" --
/// a drag arriving at one has to be able to move the other, and a scrollable
/// that could only move itself would make a diagonal drag impossible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwoDimensionalScrollableState {
    pub widget: TwoDimensionalScrollable,
    /// Whether a vertical controller was passed in.
    vertical_controller_given: bool,
    horizontal_controller_given: bool,
    vertical_fallback: bool,
    horizontal_fallback: bool,
}

impl TwoDimensionalScrollableState {
    /// Upstream `initState`: a fallback controller is built for each axis that
    /// was not given one.
    pub fn init_state(
        widget: TwoDimensionalScrollable,
        vertical_controller_given: bool,
        horizontal_controller_given: bool,
    ) -> TwoDimensionalScrollableState {
        TwoDimensionalScrollableState {
            widget,
            vertical_controller_given,
            horizontal_controller_given,
            vertical_fallback: !vertical_controller_given,
            horizontal_fallback: !horizontal_controller_given,
        }
    }

    pub fn has_vertical_fallback(&self) -> bool {
        self.vertical_fallback
    }

    pub fn has_horizontal_fallback(&self) -> bool {
        self.horizontal_fallback
    }

    /// Upstream `didUpdateWidget`, which moves between owning a controller and
    /// not, in both directions.
    ///
    /// The invariant is that exactly one of the two exists at a time: a
    /// caller's controller means there is nothing to own, and no caller's
    /// controller means there has to be one to own. Upstream asserts both
    /// halves rather than only the one that would crash.
    pub fn did_update_widget(
        &mut self,
        vertical_controller_given: bool,
        horizontal_controller_given: bool,
    ) {
        if vertical_controller_given != self.vertical_controller_given {
            self.vertical_fallback = !vertical_controller_given;
            self.vertical_controller_given = vertical_controller_given;
        }
        if horizontal_controller_given != self.horizontal_controller_given {
            self.horizontal_fallback = !horizontal_controller_given;
            self.horizontal_controller_given = horizontal_controller_given;
        }
    }

    /// Which scrollable answers for an axis. Upstream's `verticalScrollable`
    /// and `horizontalScrollable`, both of which assert the key has a state --
    /// asking before the subtree is built is a mistake, not an absence.
    pub fn scrollable_for(&self, axis: Axis) -> ScrollableNesting {
        match axis {
            Axis::Vertical => ScrollableNesting::VerticalOuter,
            Axis::Horizontal => ScrollableNesting::HorizontalInner,
        }
    }

    /// Whether each scrollable is told about the other. Both are, and it is not
    /// symmetric bookkeeping for its own sake: the outer one is given the inner
    /// key and the inner one the outer key, so either can forward a gesture it
    /// cannot use.
    pub fn forwards_gestures(&self) -> bool {
        true
    }

    /// Upstream's `_TwoDimensionalScrollableScope.updateShouldNotify` returns
    /// **false**, with the reason in a comment above the class: `build` always
    /// rebuilds the scope, so anything depending on it is rebuilt through the
    /// rebuild rather than through a notification. Returning true as well would
    /// do the same work twice.
    pub fn scope_notifies_dependents() -> bool {
        false
    }

    /// Upstream `dispose`, which disposes only what it built.
    pub fn dispose(&mut self) -> (bool, bool) {
        let disposed = (self.vertical_fallback, self.horizontal_fallback);
        self.vertical_fallback = false;
        self.horizontal_fallback = false;
        disposed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- ScrollableState -------------------------------------------------------

    #[test]
    fn scroll_offset_and_screen_offset_only_agree_going_down_or_right() {
        // A list running upwards has carried its content negatively in y by the
        // same number of pixels.
        let down = ScrollableState::new(AxisDirection::Down).with_pixels(120.0);
        let up = ScrollableState::new(AxisDirection::Up).with_pixels(120.0);
        assert_eq!(down.delta_to_scroll_origin(), (0.0, 120.0));
        assert_eq!(up.delta_to_scroll_origin(), (0.0, -120.0));

        let right = ScrollableState::new(AxisDirection::Right).with_pixels(120.0);
        let left = ScrollableState::new(AxisDirection::Left).with_pixels(120.0);
        assert_eq!(right.delta_to_scroll_origin(), (120.0, 0.0));
        assert_eq!(left.delta_to_scroll_origin(), (-120.0, 0.0));
    }

    #[test]
    fn an_unscrolled_view_is_at_its_origin_whichever_way_it_runs() {
        for direction in [
            AxisDirection::Up,
            AxisDirection::Down,
            AxisDirection::Left,
            AxisDirection::Right,
        ] {
            assert_eq!(
                ScrollableState::new(direction).delta_to_scroll_origin(),
                (0.0, 0.0),
                "{direction:?}"
            );
        }
    }

    #[test]
    fn a_scrollable_owns_only_the_controller_it_had_to_build() {
        let borrowed = ScrollableState {
            has_controller: true,
            ..ScrollableState::new(AxisDirection::Down)
        };
        assert!(!borrowed.owns_its_controller());
        assert!(ScrollableState::new(AxisDirection::Down).owns_its_controller());
    }

    #[test]
    fn the_axis_direction_names_an_axis() {
        assert_eq!(
            ScrollableState::new(AxisDirection::Up).axis(),
            Axis::Vertical
        );
        assert_eq!(
            ScrollableState::new(AxisDirection::Left).axis(),
            Axis::Horizontal
        );
    }

    // -- ensureVisible -----------------------------------------------------------

    #[test]
    fn showing_something_moves_every_scrollable_above_it_not_just_the_nearest() {
        // A row inside a list inside a page needs all three to move.
        let steps = ensure_visible(&[10, 11, 12]);
        assert_eq!(steps.len(), 3);
        assert_eq!(
            steps.iter().map(|step| step.scrollable).collect::<Vec<_>>(),
            [10, 11, 12]
        );
    }

    #[test]
    fn only_the_innermost_step_aims_at_what_the_caller_actually_named() {
        // Every step after it scrolls to the scrollable below, carrying the
        // original target along as a preference. Without that, each level would
        // re-aim at something bigger and the thing asked about could end up off
        // screen.
        let steps = ensure_visible(&[10, 11, 12]);
        assert!(!steps[0].scrolls_to_inner_scrollable);
        assert!(
            !steps[0].carries_original_target,
            "there is nothing recorded yet"
        );
        assert!(steps[1].scrolls_to_inner_scrollable);
        assert!(steps[1].carries_original_target);
        assert!(steps[2].carries_original_target);
    }

    #[test]
    fn one_scrollable_is_one_step_and_none_is_none() {
        assert_eq!(ensure_visible(&[10]).len(), 1);
        assert!(ensure_visible(&[]).is_empty());
    }

    #[test]
    fn nothing_to_wait_for_when_nothing_moved_or_nothing_animated() {
        assert_eq!(
            ensure_visible_result(0, 300.0),
            EnsureVisibleResult::Immediate
        );
        assert_eq!(
            ensure_visible_result(3, 0.0),
            EnsureVisibleResult::Immediate,
            "an instantaneous jump is already done"
        );
        assert_eq!(ensure_visible_result(1, 300.0), EnsureVisibleResult::Single);
        assert_eq!(
            ensure_visible_result(3, 300.0),
            EnsureVisibleResult::Several
        );
    }

    // -- TwoDimensionalScrollable --------------------------------------------------

    #[test]
    fn a_detail_that_names_the_wrong_axis_is_caught_at_build() {
        // Otherwise it would silently disagree with the one beside it.
        let good = TwoDimensionalScrollable::new();
        assert_eq!(good.validate(), Ok(()));

        let sideways_vertical = TwoDimensionalScrollable {
            vertical_details: ScrollableDetails::horizontal(false),
            ..TwoDimensionalScrollable::new()
        };
        assert!(sideways_vertical.validate().is_err());

        let upright_horizontal = TwoDimensionalScrollable {
            horizontal_details: ScrollableDetails::vertical(false),
            ..TwoDimensionalScrollable::new()
        };
        assert!(upright_horizontal.validate().is_err());
    }

    #[test]
    fn a_reversed_axis_is_still_the_same_axis() {
        let reversed = TwoDimensionalScrollable {
            vertical_details: ScrollableDetails::vertical(true),
            horizontal_details: ScrollableDetails::horizontal(true),
            ..TwoDimensionalScrollable::new()
        };
        assert_eq!(reversed.validate(), Ok(()));
        assert_eq!(reversed.vertical_details.direction, AxisDirection::Up);
        assert_eq!(reversed.horizontal_details.direction, AxisDirection::Left);
    }

    #[test]
    fn a_diagonal_drag_locks_to_one_axis_unless_asked_otherwise() {
        assert_eq!(
            TwoDimensionalScrollable::new().diagonal_drag_behavior,
            DiagonalDragBehavior::None
        );
        assert_eq!(
            TwoDimensionalScrollable::new()
                .with_diagonal(DiagonalDragBehavior::Free)
                .diagonal_drag_behavior,
            DiagonalDragBehavior::Free
        );
    }

    // -- TwoDimensionalScrollableState -----------------------------------------------

    fn state(vertical_given: bool, horizontal_given: bool) -> TwoDimensionalScrollableState {
        TwoDimensionalScrollableState::init_state(
            TwoDimensionalScrollable::new(),
            vertical_given,
            horizontal_given,
        )
    }

    #[test]
    fn the_vertical_is_outside_and_the_horizontal_inside() {
        // Not arbitrary: the nesting decides which axis sees a gesture first.
        let state = state(false, false);
        assert_eq!(
            state.scrollable_for(Axis::Vertical),
            ScrollableNesting::VerticalOuter
        );
        assert_eq!(
            state.scrollable_for(Axis::Horizontal),
            ScrollableNesting::HorizontalInner
        );
    }

    #[test]
    fn each_scrollable_is_handed_the_other_key_for_gesture_forwarding() {
        // A scrollable that could only move itself would make a diagonal drag
        // impossible.
        assert!(state(false, false).forwards_gestures());
    }

    #[test]
    fn a_fallback_controller_is_built_only_for_an_axis_that_was_not_given_one() {
        let neither = state(false, false);
        assert!(neither.has_vertical_fallback() && neither.has_horizontal_fallback());

        let vertical_only = state(true, false);
        assert!(!vertical_only.has_vertical_fallback());
        assert!(vertical_only.has_horizontal_fallback());

        let both = state(true, true);
        assert!(!both.has_vertical_fallback() && !both.has_horizontal_fallback());
    }

    #[test]
    fn handing_a_controller_over_later_disposes_the_one_it_had_built() {
        let mut state = state(false, false);
        state.did_update_widget(true, false);
        assert!(!state.has_vertical_fallback(), "the fallback was let go");
        assert!(
            state.has_horizontal_fallback(),
            "and the other is untouched"
        );
    }

    #[test]
    fn taking_a_controller_away_later_makes_it_build_one() {
        // The invariant runs both ways: exactly one of the two exists at a time.
        let mut state = state(true, true);
        assert!(!state.has_vertical_fallback());
        state.did_update_widget(false, true);
        assert!(state.has_vertical_fallback());
    }

    #[test]
    fn it_disposes_only_what_it_built() {
        let mut owns_both = state(false, false);
        assert_eq!(owns_both.dispose(), (true, true));

        let mut owns_neither = state(true, true);
        assert_eq!(owns_neither.dispose(), (false, false));
    }

    #[test]
    fn the_scope_notifies_nobody_because_the_rebuild_already_did() {
        // build() always rebuilds it, so returning true would do the same work
        // twice.
        assert!(!TwoDimensionalScrollableState::scope_notifies_dependents());
    }
}
