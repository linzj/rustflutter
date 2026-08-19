// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! What a recogniser hands its callbacks (upstream `gestures/tap_and_drag.dart`,
//! `gestures/multitap.dart`, `gestures/force_press.dart`).
//!
//! Nine data classes, each the argument of one callback. They are together
//! because they are the same kind of thing and because the shapes rhyme: a
//! global position, the same point in the receiving box's coordinates, and
//! then whatever that particular gesture knows.
//!
//! # Why a tap and a drag share a family
//!
//! A text field needs both, and needs them from *one* recogniser. Two
//! separate ones fight in the arena, and the tap loses -- so selecting a word
//! by double-tapping and then dragging to extend the selection cannot be
//! built out of a tap recogniser and a drag recogniser. `TapAndDrag` is
//! upstream's answer, and `consecutive_tap_count` is the field that makes it
//! work: every callback carries how many taps in a row this is, so the
//! handler can tell a drag after one tap from a drag after two.
//!
//! # Recorded divergences
//!
//! * `localPosition` defaults to the global position where upstream's
//!   constructors allow it to. That is upstream's rule and not a shortcut:
//!   before anything has transformed the event, the two are the same point.
//! * `Duration` is `i64` microseconds here, which is what this crate's
//!   [`PointerEvent`](crate::gestures::PointerEvent) timestamps are.
//! * The recognisers these belong to are not ported; this is the half that is
//!   data. Upstream's own `debugFillProperties` on all nine is the
//!   diagnostics tree, P10.

use crate::gestures::PointerKind;
use crate::render::Offset;

/// Upstream `PositionedGestureDetails`: the two positions every one of these
/// carries.
///
/// Upstream is an interface the nine implement; here it is a trait with the
/// two getters, so that something drawing a gesture can take any of them.
pub trait PositionedGestureDetails {
    fn global_position(&self) -> Offset;
    fn local_position(&self) -> Offset;
}

/// Upstream `TapDragDownDetails`: the finger landed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapDragDownDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub kind: Option<PointerKind>,
    /// How many taps in a row this is, counting from one. The field that
    /// makes a combined tap-and-drag recogniser usable at all.
    pub consecutive_tap_count: u32,
}

impl TapDragDownDetails {
    pub fn new(global_position: Offset, consecutive_tap_count: u32) -> TapDragDownDetails {
        TapDragDownDetails {
            global_position,
            local_position: global_position,
            kind: None,
            consecutive_tap_count,
        }
    }

    pub fn with_local_position(mut self, local_position: Offset) -> Self {
        self.local_position = local_position;
        self
    }

    pub fn with_kind(mut self, kind: PointerKind) -> Self {
        self.kind = Some(kind);
        self
    }
}

/// Upstream `TapDragUpDetails`: the finger lifted without having dragged.
///
/// Upstream requires the kind here where the other four leave it optional --
/// by the time a tap has completed, the platform has certainly said what
/// touched the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapDragUpDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub kind: PointerKind,
    pub consecutive_tap_count: u32,
}

impl TapDragUpDetails {
    pub fn new(
        global_position: Offset,
        kind: PointerKind,
        consecutive_tap_count: u32,
    ) -> TapDragUpDetails {
        TapDragUpDetails {
            global_position,
            local_position: global_position,
            kind,
            consecutive_tap_count,
        }
    }

    pub fn with_local_position(mut self, local_position: Offset) -> Self {
        self.local_position = local_position;
        self
    }
}

/// Upstream `TapDragStartDetails`: the finger moved far enough that this is a
/// drag rather than a tap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapDragStartDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    /// When the platform said this happened, rather than when the framework
    /// noticed. Upstream carries it so that a drag started from a queued
    /// event is timed by the event and not by the frame that read it.
    pub source_time_stamp_micros: Option<i64>,
    pub kind: Option<PointerKind>,
    pub consecutive_tap_count: u32,
}

impl TapDragStartDetails {
    pub fn new(global_position: Offset, consecutive_tap_count: u32) -> TapDragStartDetails {
        TapDragStartDetails {
            global_position,
            local_position: global_position,
            source_time_stamp_micros: None,
            kind: None,
            consecutive_tap_count,
        }
    }

    pub fn with_source_time_stamp(mut self, micros: i64) -> Self {
        self.source_time_stamp_micros = Some(micros);
        self
    }
}

/// Upstream `TapDragUpdateDetails`: the drag moved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapDragUpdateDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub source_time_stamp_micros: Option<i64>,
    /// How far it moved since the last update.
    pub delta: Offset,
    /// The same movement along the one axis a single-axis recogniser cares
    /// about. Absent for a pan, which has no single axis.
    pub primary_delta: Option<f32>,
    pub kind: Option<PointerKind>,
    /// How far the drag has come *in total*, which is what a selection needs:
    /// the delta says how much to scroll and this says where the selection
    /// now reaches.
    pub offset_from_origin: Offset,
    pub local_offset_from_origin: Offset,
    pub consecutive_tap_count: u32,
}

impl TapDragUpdateDetails {
    pub fn new(
        global_position: Offset,
        offset_from_origin: Offset,
        consecutive_tap_count: u32,
    ) -> TapDragUpdateDetails {
        TapDragUpdateDetails {
            global_position,
            local_position: global_position,
            source_time_stamp_micros: None,
            delta: Offset::ZERO,
            primary_delta: None,
            kind: None,
            offset_from_origin,
            local_offset_from_origin: offset_from_origin,
            consecutive_tap_count,
        }
    }

    pub fn with_delta(mut self, delta: Offset) -> Self {
        self.delta = delta;
        self
    }

    pub fn with_primary_delta(mut self, primary_delta: f32) -> Self {
        self.primary_delta = Some(primary_delta);
        self
    }
}

/// Upstream `TapDragEndDetails`: the drag finished.
///
/// The position defaults to the origin here, as upstream's does: a drag ends
/// where the finger left, and a recogniser that has lost the pointer -- to a
/// cancel, or to the arena -- has no position to report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapDragEndDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    /// How fast it was going when it ended, in pixels a second. What a fling
    /// is made of.
    pub velocity: Offset,
    pub primary_velocity: Option<f32>,
    pub consecutive_tap_count: u32,
}

impl TapDragEndDetails {
    pub fn new(consecutive_tap_count: u32) -> TapDragEndDetails {
        TapDragEndDetails {
            global_position: Offset::ZERO,
            local_position: Offset::ZERO,
            velocity: Offset::ZERO,
            primary_velocity: None,
            consecutive_tap_count,
        }
    }

    pub fn with_velocity(mut self, velocity: Offset) -> Self {
        self.velocity = velocity;
        self
    }

    pub fn with_primary_velocity(mut self, primary_velocity: f32) -> Self {
        self.primary_velocity = Some(primary_velocity);
        self
    }
}

/// Upstream `SerialTapDownDetails`: one tap of a run of them, landing.
///
/// A serial tap recogniser reports every tap as it happens rather than
/// waiting to see how many there will be -- which is what a text field wants,
/// because the first tap should place the caret immediately and the second
/// should turn that into a word selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SerialTapDownDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub kind: PointerKind,
    /// Which buttons were down, as the pointer event's bitmask.
    pub buttons: i32,
    /// Which tap of the run this is, counting from one. Upstream asserts it
    /// is positive: there is no zeroth tap.
    pub count: u32,
}

impl SerialTapDownDetails {
    pub fn new(global_position: Offset, kind: PointerKind, count: u32) -> SerialTapDownDetails {
        debug_assert!(count > 0, "there is no zeroth tap");
        SerialTapDownDetails {
            global_position,
            local_position: global_position,
            kind,
            buttons: 0,
            count,
        }
    }

    pub fn with_buttons(mut self, buttons: i32) -> Self {
        self.buttons = buttons;
        self
    }
}

/// Upstream `SerialTapCancelDetails`: the tap that was counted did not
/// happen after all.
///
/// The only one of the nine with no position, and upstream does not give it
/// one: a cancel is about a tap that is being taken back, and where it would
/// have been is not information anybody acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialTapCancelDetails {
    pub count: u32,
}

impl SerialTapCancelDetails {
    pub fn new(count: u32) -> SerialTapCancelDetails {
        debug_assert!(count > 0, "there is no zeroth tap");
        SerialTapCancelDetails { count }
    }
}

/// Upstream `SerialTapUpDetails`: one tap of a run of them, completing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SerialTapUpDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub kind: Option<PointerKind>,
    pub count: u32,
}

impl SerialTapUpDetails {
    pub fn new(global_position: Offset, count: u32) -> SerialTapUpDetails {
        debug_assert!(count > 0, "there is no zeroth tap");
        SerialTapUpDetails {
            global_position,
            local_position: global_position,
            kind: None,
            count,
        }
    }
}

/// Upstream `ForcePressDetails`: how hard the screen is being pressed.
///
/// Only a screen that measures pressure reports this at all. The pressure is
/// normalised by the recogniser against the device's own range, so that the
/// same number means the same push on hardware that reports different scales.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForcePressDetails {
    pub global_position: Offset,
    pub local_position: Offset,
    pub pressure: f32,
}

impl ForcePressDetails {
    pub fn new(global_position: Offset, pressure: f32) -> ForcePressDetails {
        ForcePressDetails {
            global_position,
            local_position: global_position,
            pressure,
        }
    }

    pub fn with_local_position(mut self, local_position: Offset) -> Self {
        self.local_position = local_position;
        self
    }
}

macro_rules! positioned {
    ($($details:ty),* $(,)?) => {
        $(
            impl PositionedGestureDetails for $details {
                fn global_position(&self) -> Offset {
                    self.global_position
                }

                fn local_position(&self) -> Offset {
                    self.local_position
                }
            }
        )*
    };
}

// Every one but `SerialTapCancelDetails`, which upstream leaves out of the
// interface for the reason recorded on it.
positioned!(
    TapDragDownDetails,
    TapDragUpDetails,
    TapDragStartDetails,
    TapDragUpdateDetails,
    TapDragEndDetails,
    SerialTapDownDetails,
    SerialTapUpDetails,
    ForcePressDetails,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_local_position_starts_as_the_global_one() {
        // Upstream's rule and not a shortcut: before anything has transformed
        // the event, the two are the same point. Defaulting to the origin
        // instead would put every untransformed gesture at the top-left.
        let point = Offset::new(12.0, 34.0);
        assert_eq!(TapDragDownDetails::new(point, 1).local_position, point);
        assert_eq!(
            SerialTapDownDetails::new(point, PointerKind::Touch, 1).local_position,
            point
        );
        assert_eq!(ForcePressDetails::new(point, 0.5).local_position, point);
        // And a transform, when there is one, replaces only the local half.
        let transformed =
            TapDragDownDetails::new(point, 1).with_local_position(Offset::new(2.0, 4.0));
        assert_eq!(transformed.global_position, point);
        assert_eq!(transformed.local_position, Offset::new(2.0, 4.0));
    }

    #[test]
    fn every_tap_drag_callback_carries_how_many_taps_in_a_row_it_is() {
        // The field the combined recogniser exists for. Without it a handler
        // cannot tell a drag after one tap from a drag after two, which is
        // exactly the difference between moving the caret and extending a
        // word selection.
        assert_eq!(
            TapDragDownDetails::new(Offset::ZERO, 2).consecutive_tap_count,
            2
        );
        assert_eq!(
            TapDragUpDetails::new(Offset::ZERO, PointerKind::Mouse, 2).consecutive_tap_count,
            2
        );
        assert_eq!(
            TapDragStartDetails::new(Offset::ZERO, 3).consecutive_tap_count,
            3
        );
        assert_eq!(
            TapDragUpdateDetails::new(Offset::ZERO, Offset::ZERO, 3).consecutive_tap_count,
            3
        );
        assert_eq!(TapDragEndDetails::new(3).consecutive_tap_count, 3);
    }

    #[test]
    fn an_update_carries_both_the_step_and_the_total() {
        // Two different questions, and a handler needs both: the delta says
        // how far to scroll this frame, the offset from origin says where the
        // selection now reaches. Deriving either from the other means keeping
        // a running total in the handler, which is what upstream saves it
        // from.
        let details = TapDragUpdateDetails::new(Offset::new(30.0, 0.0), Offset::new(25.0, 0.0), 1)
            .with_delta(Offset::new(5.0, 0.0));
        assert_eq!(details.delta, Offset::new(5.0, 0.0));
        assert_eq!(details.offset_from_origin, Offset::new(25.0, 0.0));
        assert_ne!(details.delta, details.offset_from_origin);
    }

    #[test]
    fn a_pan_has_no_primary_delta_and_a_single_axis_drag_does() {
        // The primary is the movement along the one axis a single-axis
        // recogniser watches. A pan has no such axis, so upstream leaves it
        // null rather than picking one.
        let pan = TapDragUpdateDetails::new(Offset::ZERO, Offset::ZERO, 1);
        assert_eq!(pan.primary_delta, None);
        assert_eq!(pan.with_primary_delta(5.0).primary_delta, Some(5.0));
    }

    #[test]
    fn a_drag_that_ended_without_a_position_reports_the_origin() {
        // Upstream's default, and it means something: a recogniser that lost
        // the pointer -- to a cancel, or to the arena -- has no position to
        // report, and the origin is what it says instead.
        let ended = TapDragEndDetails::new(1);
        assert_eq!(ended.global_position, Offset::ZERO);
        assert_eq!(ended.velocity, Offset::ZERO);
        assert_eq!(ended.primary_velocity, None);
        let flung = TapDragEndDetails::new(1)
            .with_velocity(Offset::new(0.0, -800.0))
            .with_primary_velocity(-800.0);
        assert_eq!(flung.primary_velocity, Some(-800.0));
    }

    #[test]
    fn a_start_can_say_when_the_platform_thought_it_happened() {
        // So that a drag started from a queued event is timed by the event
        // and not by the frame that got round to reading it.
        assert_eq!(
            TapDragStartDetails::new(Offset::ZERO, 1).source_time_stamp_micros,
            None
        );
        assert_eq!(
            TapDragStartDetails::new(Offset::ZERO, 1)
                .with_source_time_stamp(16_000)
                .source_time_stamp_micros,
            Some(16_000)
        );
    }

    #[test]
    fn a_serial_tap_counts_from_one() {
        // Upstream asserts it: there is no zeroth tap, and a count starting
        // at zero would make the first tap look like a cancel of nothing.
        assert_eq!(
            SerialTapDownDetails::new(Offset::ZERO, PointerKind::Touch, 1).count,
            1
        );
        assert_eq!(SerialTapCancelDetails::new(2).count, 2);
        assert_eq!(SerialTapUpDetails::new(Offset::ZERO, 3).count, 3);
    }

    #[test]
    fn a_cancel_is_the_one_of_the_nine_with_no_position() {
        // Upstream leaves it out of the positioned interface deliberately: a
        // cancel is about a tap being taken back, and where it would have
        // been is not something anybody acts on.
        //
        // The check is that it compiles without the trait -- asserted here by
        // taking every *other* one through it.
        let positioned: Vec<Box<dyn PositionedGestureDetails>> = vec![
            Box::new(TapDragDownDetails::new(Offset::new(1.0, 0.0), 1)),
            Box::new(TapDragUpDetails::new(
                Offset::new(2.0, 0.0),
                PointerKind::Touch,
                1,
            )),
            Box::new(TapDragStartDetails::new(Offset::new(3.0, 0.0), 1)),
            Box::new(TapDragUpdateDetails::new(
                Offset::new(4.0, 0.0),
                Offset::ZERO,
                1,
            )),
            Box::new(TapDragEndDetails::new(1)),
            Box::new(SerialTapDownDetails::new(
                Offset::new(6.0, 0.0),
                PointerKind::Touch,
                1,
            )),
            Box::new(SerialTapUpDetails::new(Offset::new(7.0, 0.0), 1)),
            Box::new(ForcePressDetails::new(Offset::new(8.0, 0.0), 0.5)),
        ];
        assert_eq!(positioned.len(), 8);
        assert_eq!(
            positioned
                .iter()
                .map(|details| details.global_position().dx)
                .collect::<Vec<_>>(),
            vec![1.0, 2.0, 3.0, 4.0, 0.0, 6.0, 7.0, 8.0]
        );
    }

    #[test]
    fn an_up_knows_what_touched_the_screen_and_a_down_may_not() {
        // Upstream requires the kind on `TapDragUpDetails` and leaves it
        // optional on the other four. By the time a tap has completed the
        // platform has certainly said what it was.
        let up = TapDragUpDetails::new(Offset::ZERO, PointerKind::Stylus, 1);
        assert_eq!(up.kind, PointerKind::Stylus);
        assert_eq!(TapDragDownDetails::new(Offset::ZERO, 1).kind, None);
        assert_eq!(
            TapDragDownDetails::new(Offset::ZERO, 1)
                .with_kind(PointerKind::Mouse)
                .kind,
            Some(PointerKind::Mouse)
        );
    }
}
