// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Upstream `widgets/drag_target.dart`: picking a thing up and putting it
//! somewhere.
//!
//! The family is four pieces: a [`Draggable`] that can be lifted, the
//! *feedback* it turns into while in the air, a [`DragTarget`] that decides
//! whether it will take what is offered, and the two detail records that pass
//! between them.
//!
//! What is worth knowing is mostly about **what happens to the thing while it
//! is in the air** -- where it sits relative to the finger, whether the
//! original stays behind, and which gestures count as a lift at all. Those are
//! the decisions that make a drag feel like moving an object rather than like
//! a widget appearing under a cursor.
//!
//! The half that needs a live tree -- the feedback drawn in the overlay,
//! following the pointer, and the targets under it found by hit testing -- is
//! [`crate::drag_feedback`], which is upstream's `_DragAvatar`.

use crate::render::{Axis, Offset};

/// Where the feedback sits relative to the pointer once a drag starts.
///
/// Upstream ships two functions rather than an enum, so a caller may write a
/// third; the two shipped ones are what this enum holds, with
/// [`DragAnchorStrategy::resolve`] as the body each of them has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DragAnchorStrategy {
    /// Upstream's `childDragAnchorStrategy`, the default: the feedback keeps
    /// the finger at the same place *within it* that the child had.
    ///
    /// So grabbing a card by its bottom-right corner lifts it by that corner.
    /// The thing appears to have been **picked up** -- it does not jump under
    /// the finger first, which is what would make a drag read as a
    /// replacement rather than a move.
    #[default]
    Child,
    /// Upstream's `pointerDragAnchorStrategy`: the feedback's top-left goes to
    /// the pointer.
    ///
    /// For feedback that is not a copy of the child -- a small badge showing
    /// what is being carried, say. There is no corresponding point on the
    /// child to keep, so the badge simply hangs off the fingertip.
    Pointer,
}

impl DragAnchorStrategy {
    /// Where inside the feedback the pointer sits, given where the pointer
    /// went down and where the child was.
    pub fn resolve(self, global_position: Offset, child_origin: Offset) -> Offset {
        match self {
            // Upstream's `renderObject.globalToLocal(position)`.
            DragAnchorStrategy::Child => Offset::new(
                global_position.dx - child_origin.dx,
                global_position.dy - child_origin.dy,
            ),
            DragAnchorStrategy::Pointer => Offset::ZERO,
        }
    }
}

/// Upstream `DraggableDetails`: what a finished drag reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DraggableDetails {
    /// Upstream's `wasAccepted`, **false by default**.
    ///
    /// A drag that ends over nothing has ended, and the widget still wants to
    /// know: an application that animates a rejected item back to where it
    /// came from needs the drag's end whether or not anybody took it.
    pub was_accepted: bool,
    /// The pointer's velocity when it lifted, for an application that wants to
    /// fling the item rather than drop it.
    pub velocity: Offset,
    /// Where the feedback's top-left was.
    pub offset: Offset,
}

impl DraggableDetails {
    pub fn new(offset: Offset, velocity: Offset) -> DraggableDetails {
        DraggableDetails {
            was_accepted: false,
            velocity,
            offset,
        }
    }

    pub fn accepted(mut self) -> Self {
        self.was_accepted = true;
        self
    }
}

/// Upstream `DragTargetDetails`: what is being offered, and where.
///
/// The offset is the *pointer's* position, not the feedback's, because that is
/// what a target needs to decide **which part of itself** was aimed at -- a
/// list that inserts between two rows has to know which gap the finger is
/// over, and the feedback's corner could be anywhere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragTargetDetails<T> {
    pub data: T,
    pub offset: Offset,
}

impl<T> DragTargetDetails<T> {
    pub fn new(data: T, offset: Offset) -> DragTargetDetails<T> {
        DragTargetDetails { data, offset }
    }
}

/// Upstream `Draggable`: something that can be picked up.
///
/// `Clone` because a drag in progress keeps the configuration it was started
/// with -- the axis lock and the feedback offset are consulted on every move,
/// and a drag that read them off a widget that has since rebuilt would change
/// its rules mid-gesture. See [`crate::drag_feedback::DragAvatar`].
#[derive(Clone)]
pub struct Draggable {
    pub anchor_strategy: DragAnchorStrategy,
    /// Upstream's `axis`: constrains the feedback's movement to one axis, for
    /// a list whose items reorder in a line. The finger may wander; the item
    /// does not.
    pub axis: Option<Axis>,
    /// Upstream's `affinity`: which direction a drag must *start* in for this
    /// draggable to claim it.
    ///
    /// The field a draggable inside a scrollable needs. A horizontally
    /// affine item in a vertical list claims sideways drags and lets vertical
    /// ones through to the list, so the reader can both scroll past it and
    /// swipe it -- without affinity the two gestures are the same gesture and
    /// one of them has to lose.
    pub affinity: Option<Axis>,
    /// Upstream's `maxSimultaneousDrags`. `None` is no limit; **zero is a
    /// third state and means "cannot be dragged at all"**, which is how a
    /// caller disables dragging without swapping the widget out.
    pub max_simultaneous_drags: Option<usize>,
    /// Upstream's `ignoringFeedbackPointer`, on by default: the feedback does
    /// not take hit tests.
    ///
    /// It must not, or the thing in the air would be the thing under the
    /// finger, and every drag target would be shadowed by the item being
    /// dropped on it.
    pub ignoring_feedback_pointer: bool,
    /// Upstream's `feedbackOffset`.
    pub feedback_offset: Offset,
    /// Whether this is upstream's `LongPressDraggable` -- see there.
    pub long_press: bool,
}

impl Draggable {
    pub fn new() -> Draggable {
        Draggable {
            anchor_strategy: DragAnchorStrategy::Child,
            axis: None,
            affinity: None,
            max_simultaneous_drags: None,
            ignoring_feedback_pointer: true,
            feedback_offset: Offset::ZERO,
            long_press: false,
        }
    }

    pub fn with_anchor_strategy(mut self, strategy: DragAnchorStrategy) -> Self {
        self.anchor_strategy = strategy;
        self
    }

    pub fn with_axis(mut self, axis: Axis) -> Self {
        self.axis = Some(axis);
        self
    }

    pub fn with_affinity(mut self, affinity: Axis) -> Self {
        self.affinity = Some(affinity);
        self
    }

    /// Upstream asserts `maxSimultaneousDrags == null || >= 0`, which in Rust
    /// a `usize` already guarantees -- so what is left is the meaning of
    /// zero, which is on the field.
    pub fn with_max_simultaneous_drags(mut self, max: usize) -> Self {
        self.max_simultaneous_drags = Some(max);
        self
    }

    pub fn with_feedback_offset(mut self, offset: Offset) -> Self {
        self.feedback_offset = offset;
        self
    }

    /// Whether another drag may start, given how many are already running.
    pub fn can_start_drag(&self, active_drags: usize) -> bool {
        match self.max_simultaneous_drags {
            None => true,
            Some(max) => active_drags < max,
        }
    }

    /// Where the feedback's top-left goes, for a pointer at `global_position`
    /// over a child whose top-left is at `child_origin`.
    ///
    /// The anchor is *subtracted*: the strategy says where in the feedback the
    /// pointer should be, so the feedback's corner is that far back from the
    /// pointer.
    pub fn feedback_position(&self, global_position: Offset, child_origin: Offset) -> Offset {
        let anchor = self.anchor_strategy.resolve(global_position, child_origin);
        Offset::new(
            global_position.dx - anchor.dx + self.feedback_offset.dx,
            global_position.dy - anchor.dy + self.feedback_offset.dy,
        )
    }

    /// Where the feedback ends up once [`Draggable::axis`] has had its say --
    /// movement off the axis is discarded, so the item slides in its line.
    pub fn constrain_to_axis(&self, from: Offset, to: Offset) -> Offset {
        match self.axis {
            None => to,
            Some(Axis::Horizontal) => Offset::new(to.dx, from.dy),
            Some(Axis::Vertical) => Offset::new(from.dx, to.dy),
        }
    }
}

impl Default for Draggable {
    fn default() -> Draggable {
        Draggable::new()
    }
}

/// Upstream `LongPressDraggable`: a draggable that has to be held first.
///
/// Upstream it is a subclass overriding one method -- `createRecognizer`
/// returns a long-press recogniser instead of an immediate one -- so here it
/// is a constructor rather than a type.
///
/// The reason it exists is the same one `affinity` exists for, arrived at from
/// the other side: **inside a scrollable, an immediate drag and a scroll are
/// the same gesture.** Affinity separates them by direction; a long press
/// separates them by time. A reorderable list uses the second because its
/// items move along the same axis the list scrolls, so direction cannot tell
/// the two apart.
pub struct LongPressDraggable;

impl LongPressDraggable {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Draggable {
        Draggable {
            long_press: true,
            ..Draggable::new()
        }
    }
}

/// Upstream `DragTarget`: somewhere a dragged thing may be put.
///
/// # The two questions, and why they are two
///
/// Upstream asks `onWillAcceptWithDetails` while the item is *over* the
/// target and `onAcceptWithDetails` when it is *dropped*. A target that
/// answered only at the drop could not highlight itself, and the reader would
/// be dragging blind -- so the first question exists to be asked repeatedly
/// and cheaply, and its answer is what the builder paints.
pub struct DragTarget<T> {
    /// What is currently over this target and would be accepted.
    pub candidate_data: Vec<T>,
    /// What is over it and would *not* be. Upstream hands the builder both,
    /// and the second list is what lets a target say "not this one" rather
    /// than merely failing to light up.
    pub rejected_data: Vec<T>,
}

impl<T> DragTarget<T> {
    pub fn new() -> DragTarget<T> {
        DragTarget {
            candidate_data: Vec::new(),
            rejected_data: Vec::new(),
        }
    }

    /// Sorts one offered item into the two lists.
    pub fn offer(&mut self, data: T, will_accept: bool) {
        if will_accept {
            self.candidate_data.push(data);
        } else {
            self.rejected_data.push(data);
        }
    }

    /// Whether anything at all is over this target.
    pub fn has_anything_over_it(&self) -> bool {
        !self.candidate_data.is_empty() || !self.rejected_data.is_empty()
    }

    /// Upstream's `onLeave`: everything goes when the pointer does.
    pub fn leave(&mut self) {
        self.candidate_data.clear();
        self.rejected_data.clear();
    }
}

impl<T> Default for DragTarget<T> {
    fn default() -> DragTarget<T> {
        DragTarget::new()
    }
}

// -- Swiping a thing away -----------------------------------------------------

/// Upstream `DismissDirection` (`widgets/dismissible.dart`): which way an item
/// may be swiped away.
///
/// Two of these are *pairs* -- `Horizontal` and `Vertical` allow either way
/// along their axis -- and four are single directions. `None` is a real
/// member rather than an absence, so a list can turn dismissal off per item
/// without changing which widget it builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DismissDirection {
    Vertical,
    #[default]
    Horizontal,
    /// Towards the reading direction's end -- right in a left-to-right
    /// subtree.
    EndToStart,
    StartToEnd,
    Up,
    Down,
    None,
}

impl DismissDirection {
    /// Whether this direction runs along the horizontal axis, which decides
    /// which velocity component the fling test reads.
    pub fn is_horizontal(self) -> bool {
        matches!(
            self,
            DismissDirection::Horizontal
                | DismissDirection::EndToStart
                | DismissDirection::StartToEnd
        )
    }
}

/// Upstream's `_FlingGestureKind`: what a flick at the end of a drag meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlingGestureKind {
    /// Not a fling -- too slow, too diagonal, or from a standstill. The drag's
    /// *position* decides instead.
    None,
    /// A flick the same way the item was already dragged: dismiss it.
    Forward,
    /// A flick back the other way: put it back, however far it had been
    /// dragged.
    Reverse,
}

/// Upstream `DismissUpdateDetails`: what a drag reports as it crosses the
/// line.
///
/// **Both the current and the previous answer**, which is the point:
/// upstream's own documentation says the pair is there "to catch the moment"
/// the threshold is crossed. A caller wanting to buzz the phone exactly once
/// as the item commits needs the edge, not the level, and computing the edge
/// from a stream of levels is the caller writing state the widget already had.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DismissUpdateDetails {
    pub direction: DismissDirection,
    pub reached: bool,
    pub previous_reached: bool,
    pub progress: f32,
}

impl DismissUpdateDetails {
    pub fn new(direction: DismissDirection, progress: f32) -> DismissUpdateDetails {
        DismissUpdateDetails {
            direction,
            reached: false,
            previous_reached: false,
            progress,
        }
    }

    pub fn with_reached(mut self, reached: bool, previous_reached: bool) -> Self {
        self.reached = reached;
        self.previous_reached = previous_reached;
        self
    }

    /// The moment the threshold is crossed on the way in -- what a caller
    /// wanting one buzz per commit listens for.
    pub fn just_reached(&self) -> bool {
        self.reached && !self.previous_reached
    }
}

/// Upstream `Dismissible`: an item a swipe removes.
pub struct Dismissible {
    pub direction: DismissDirection,
    /// Upstream's `dismissThresholds`, defaulting to
    /// [`Dismissible::DISMISS_THRESHOLD`] per direction.
    pub threshold: f32,
}

impl Dismissible {
    /// Upstream's `_kDismissThreshold`: **0.4, not a half**.
    ///
    /// The item commits before it is halfway gone, because by the halfway
    /// point the reader can no longer see enough of it to be sure what they
    /// are removing. Forty percent is far enough to be deliberate and near
    /// enough to still be looking at the thing.
    pub const DISMISS_THRESHOLD: f32 = 0.4;
    /// Upstream's `_kMinFlingVelocity`, in logical pixels per second.
    pub const MIN_FLING_VELOCITY: f32 = 700.0;
    /// Upstream's `_kMinFlingVelocityDelta`: how far the flick has to be
    /// *predominantly* along the axis.
    ///
    /// The second condition, and the one that matters in a list. A diagonal
    /// flick fast enough to pass the first test is usually a reader scrolling
    /// who drifted sideways; requiring the axis component to beat the other by
    /// 400 is what keeps their scroll from deleting a row.
    pub const MIN_FLING_VELOCITY_DELTA: f32 = 400.0;
    /// Upstream's `_kFlingVelocityScale`.
    pub const FLING_VELOCITY_SCALE: f32 = 1.0 / 300.0;

    pub fn new(direction: DismissDirection) -> Dismissible {
        Dismissible {
            direction,
            threshold: Dismissible::DISMISS_THRESHOLD,
        }
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Upstream's `_describeFlingGesture`.
    ///
    /// `drag_extent` is how far and which way the item has been dragged;
    /// `velocity` is the pointer's at release.
    ///
    /// **A fling from a standstill is not a fling.** Upstream's comment is
    /// worth keeping whole: released at the exact middle, "we assume that the
    /// user meant to fling it back to the center, as opposed to having wanted
    /// to drag it out one way, then fling it past the center and into and out
    /// the other side."
    pub fn describe_fling(&self, drag_extent: f32, velocity: Offset) -> FlingGestureKind {
        if drag_extent == 0.0 {
            return FlingGestureKind::None;
        }
        let (along, across) = if self.direction.is_horizontal() {
            (velocity.dx, velocity.dy)
        } else {
            (velocity.dy, velocity.dx)
        };
        // Both conditions, and in upstream's order: predominantly along the
        // axis, *and* fast enough.
        if along.abs() - across.abs() < Dismissible::MIN_FLING_VELOCITY_DELTA
            || along.abs() < Dismissible::MIN_FLING_VELOCITY
        {
            return FlingGestureKind::None;
        }
        // A flick the same way the item was already going carries it off; the
        // other way puts it back, however far it had been dragged.
        if along.signum() == drag_extent.signum() {
            FlingGestureKind::Forward
        } else {
            FlingGestureKind::Reverse
        }
    }

    /// Whether the item goes, given where the drag ended and what the flick
    /// said.
    ///
    /// A fling decides outright either way; only in its absence does the
    /// *position* matter. So a slow drag past the threshold dismisses, and a
    /// hard flick back from past the threshold does not -- the reader's last
    /// word wins over where their finger happened to stop.
    pub fn should_dismiss(&self, progress: f32, fling: FlingGestureKind) -> bool {
        match fling {
            FlingGestureKind::Forward => true,
            FlingGestureKind::Reverse => false,
            FlingGestureKind::None => progress.abs() > self.threshold,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_anchored_drag_lifts_the_thing_by_where_it_was_grabbed() {
        // Grabbing a card by its bottom-right corner lifts it by that corner.
        // It does not jump under the finger first, which is what would make a
        // drag read as a replacement rather than a move.
        let child_origin = Offset::new(100.0, 200.0);
        let grabbed_at = Offset::new(180.0, 240.0);
        let anchor = DragAnchorStrategy::Child.resolve(grabbed_at, child_origin);
        assert_eq!(anchor, Offset::new(80.0, 40.0), "80 in and 40 down");

        // So the feedback's corner stays 80 and 40 behind the finger, wherever
        // the finger goes.
        let draggable = Draggable::new();
        let placed = draggable.feedback_position(grabbed_at, child_origin);
        assert_eq!(placed, child_origin, "still exactly over the child");
    }

    #[test]
    fn a_pointer_anchored_drag_hangs_the_feedback_off_the_fingertip() {
        // For feedback that is not a copy of the child -- a badge saying what
        // is being carried. There is no corresponding point to keep.
        assert_eq!(
            DragAnchorStrategy::Pointer
                .resolve(Offset::new(180.0, 240.0), Offset::new(100.0, 200.0)),
            Offset::ZERO
        );
        let badge = Draggable::new().with_anchor_strategy(DragAnchorStrategy::Pointer);
        assert_eq!(
            badge.feedback_position(Offset::new(180.0, 240.0), Offset::new(100.0, 200.0)),
            Offset::new(180.0, 240.0),
            "its corner is at the finger"
        );
    }

    #[test]
    fn zero_simultaneous_drags_is_a_third_state_from_no_limit() {
        // Which is how a caller disables dragging without swapping the widget
        // out for a different one.
        let unlimited = Draggable::new();
        assert!(unlimited.can_start_drag(0));
        assert!(unlimited.can_start_drag(99));

        let disabled = Draggable::new().with_max_simultaneous_drags(0);
        assert!(!disabled.can_start_drag(0), "not even the first");

        let one_at_a_time = Draggable::new().with_max_simultaneous_drags(1);
        assert!(one_at_a_time.can_start_drag(0));
        assert!(!one_at_a_time.can_start_drag(1));
    }

    #[test]
    fn an_axis_locked_drag_discards_movement_off_its_line() {
        // So an item in a reorderable list slides along the list rather than
        // wandering with the finger.
        let from = Offset::new(100.0, 200.0);
        let to = Offset::new(150.0, 260.0);
        let free = Draggable::new();
        assert_eq!(free.constrain_to_axis(from, to), to);

        let vertical = Draggable::new().with_axis(Axis::Vertical);
        assert_eq!(
            vertical.constrain_to_axis(from, to),
            Offset::new(100.0, 260.0)
        );

        let horizontal = Draggable::new().with_axis(Axis::Horizontal);
        assert_eq!(
            horizontal.constrain_to_axis(from, to),
            Offset::new(150.0, 200.0)
        );
    }

    #[test]
    fn the_feedback_does_not_take_hit_tests() {
        // It must not, or the thing in the air would be the thing under the
        // finger and every drag target would be shadowed by the item being
        // dropped on it.
        assert!(Draggable::new().ignoring_feedback_pointer);
    }

    #[test]
    fn affinity_and_a_long_press_solve_the_same_problem_from_two_sides() {
        // Inside a scrollable, an immediate drag and a scroll are the same
        // gesture. Affinity separates them by direction; a long press
        // separates them by time. A reorderable list needs the second, because
        // its items move along the axis the list scrolls and direction cannot
        // tell the two apart.
        let swipeable = Draggable::new().with_affinity(Axis::Horizontal);
        assert_eq!(swipeable.affinity, Some(Axis::Horizontal));
        assert!(!swipeable.long_press);

        let reorderable = LongPressDraggable::new();
        assert!(reorderable.long_press);
        assert_eq!(reorderable.affinity, None, "time, not direction");
    }

    #[test]
    fn a_drag_that_ended_over_nothing_still_reports_its_end() {
        // An application that animates a rejected item back to where it came
        // from needs the drag's end whether or not anybody took it.
        let dropped = DraggableDetails::new(Offset::new(10.0, 20.0), Offset::new(0.0, -300.0));
        assert!(!dropped.was_accepted, "false by default");
        assert_eq!(dropped.offset, Offset::new(10.0, 20.0));
        assert!(dropped.accepted().was_accepted);
    }

    #[test]
    fn a_target_keeps_what_it_would_reject_as_well_as_what_it_would_take() {
        // The second list is what lets a target say "not this one" rather than
        // merely failing to light up.
        let mut target: DragTarget<&str> = DragTarget::new();
        assert!(!target.has_anything_over_it());

        target.offer("a photo", true);
        target.offer("a folder", false);
        assert_eq!(target.candidate_data, vec!["a photo"]);
        assert_eq!(target.rejected_data, vec!["a folder"]);
        assert!(target.has_anything_over_it());

        target.leave();
        assert!(!target.has_anything_over_it(), "both lists go together");
    }

    #[test]
    fn the_offered_position_is_the_pointers_not_the_feedbacks() {
        // A list that inserts between two rows has to know which gap the
        // finger is over, and the feedback's corner could be anywhere -- for a
        // child-anchored drag it is wherever the item happened to be grabbed.
        let details = DragTargetDetails::new("a photo", Offset::new(180.0, 240.0));
        assert_eq!(details.offset, Offset::new(180.0, 240.0));
        assert_eq!(details.data, "a photo");
    }

    #[test]
    fn a_fling_from_a_standstill_is_not_a_fling() {
        // Upstream's comment, worth keeping whole: released at the exact
        // middle, "we assume that the user meant to fling it back to the
        // center, as opposed to having wanted to drag it out one way, then
        // fling it past the center and into and out the other side."
        let item = Dismissible::new(DismissDirection::Horizontal);
        assert_eq!(
            item.describe_fling(0.0, Offset::new(2000.0, 0.0)),
            FlingGestureKind::None
        );
    }

    #[test]
    fn a_flick_must_be_fast_enough_and_predominantly_along_the_axis() {
        // The second condition is the one that matters in a list: a diagonal
        // flick fast enough to pass the first is usually a reader scrolling
        // who drifted sideways, and requiring the axis component to beat the
        // other by 400 keeps their scroll from deleting a row.
        let item = Dismissible::new(DismissDirection::Horizontal);

        // Fast and straight: a fling.
        assert_eq!(
            item.describe_fling(50.0, Offset::new(1200.0, 100.0)),
            FlingGestureKind::Forward
        );
        // Fast but diagonal: not one.
        assert_eq!(
            item.describe_fling(50.0, Offset::new(1200.0, 1100.0)),
            FlingGestureKind::None,
            "the reader was scrolling"
        );
        // Straight but slow: not one either.
        assert_eq!(
            item.describe_fling(50.0, Offset::new(500.0, 0.0)),
            FlingGestureKind::None
        );
    }

    #[test]
    fn a_flick_back_the_other_way_puts_the_item_back() {
        // However far it had been dragged -- the reader's last word wins over
        // where their finger happened to stop.
        let item = Dismissible::new(DismissDirection::Horizontal);
        let dragged_right_flicked_left = item.describe_fling(80.0, Offset::new(-1200.0, 0.0));
        assert_eq!(dragged_right_flicked_left, FlingGestureKind::Reverse);
        assert!(
            !item.should_dismiss(0.9, dragged_right_flicked_left),
            "nine tenths of the way out and it still comes back"
        );
    }

    #[test]
    fn without_a_fling_it_is_the_position_that_decides() {
        let item = Dismissible::new(DismissDirection::Horizontal);
        assert!(!item.should_dismiss(0.3, FlingGestureKind::None));
        assert!(item.should_dismiss(0.5, FlingGestureKind::None));
        // Either way along the axis.
        assert!(item.should_dismiss(-0.5, FlingGestureKind::None));
    }

    #[test]
    fn the_threshold_is_less_than_half() {
        // The item commits before it is halfway gone, because by the halfway
        // point the reader can no longer see enough of it to be sure what they
        // are removing.
        assert_eq!(Dismissible::DISMISS_THRESHOLD, 0.4);
        assert!(Dismissible::DISMISS_THRESHOLD < 0.5);
        // And a caller may move it.
        let strict = Dismissible::new(DismissDirection::Horizontal).with_threshold(0.8);
        assert!(!strict.should_dismiss(0.5, FlingGestureKind::None));
    }

    #[test]
    fn a_vertical_dismissible_reads_the_other_velocity_component() {
        // The same two conditions, about the other axis.
        let item = Dismissible::new(DismissDirection::Vertical);
        assert_eq!(
            item.describe_fling(50.0, Offset::new(100.0, 1200.0)),
            FlingGestureKind::Forward
        );
        assert_eq!(
            item.describe_fling(50.0, Offset::new(1200.0, 100.0)),
            FlingGestureKind::None,
            "a sideways flick does not dismiss a vertical item"
        );
    }

    #[test]
    fn which_directions_run_along_which_axis() {
        assert!(DismissDirection::Horizontal.is_horizontal());
        assert!(DismissDirection::StartToEnd.is_horizontal());
        assert!(DismissDirection::EndToStart.is_horizontal());
        assert!(!DismissDirection::Vertical.is_horizontal());
        assert!(!DismissDirection::Up.is_horizontal());
        assert!(!DismissDirection::Down.is_horizontal());
    }

    #[test]
    fn the_update_details_carry_the_edge_and_not_only_the_level() {
        // Upstream's own documentation says the pair is there "to catch the
        // moment" the threshold is crossed. A caller wanting to buzz the phone
        // once as the item commits needs the edge, and computing it from a
        // stream of levels is the caller keeping state the widget already had.
        let crossing =
            DismissUpdateDetails::new(DismissDirection::Horizontal, 0.45).with_reached(true, false);
        assert!(crossing.just_reached());

        let still_past =
            DismissUpdateDetails::new(DismissDirection::Horizontal, 0.6).with_reached(true, true);
        assert!(!still_past.just_reached(), "already buzzed");

        let coming_back =
            DismissUpdateDetails::new(DismissDirection::Horizontal, 0.3).with_reached(false, true);
        assert!(!coming_back.just_reached());
    }
}
