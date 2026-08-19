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
}
