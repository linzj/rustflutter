// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Where a list is scrolled to, and what keeps it moving.
//!
//! A scroll offset looks like a number that a drag handler can add to, and for
//! as long as the finger is down that is all it is. The moment it lifts, the
//! offset becomes something else: a value with momentum, moving on its own,
//! for a while, and stopping where physics says rather than where the finger
//! left it. Without that, content stops dead the instant the finger does --
//! which is what a list feels like when nobody has written this file, and is
//! immediately obvious next to any other application on the device.
//!
//! [`Scroll`] is both halves. It holds the offset, it clamps against the
//! content, and it owns the fling: [`Scroll::fling`] starts one and
//! [`Scroll::advance`] moves it along, once per frame, returning whether it
//! wants another.
//!
//! # What upstream splits up
//!
//! Upstream this is three objects. `ScrollPosition` holds the offset and the
//! extents; `ScrollActivity` is what is currently in charge of it -- a
//! `DragScrollActivity` while a finger is down, a `BallisticScrollActivity`
//! after it lifts, an `IdleScrollActivity` when nothing is happening; and
//! `ScrollPhysics` decides which activity comes next and with what simulation.
//! The split earns its keep there because activities are pluggable: page
//! snapping, `ScrollController.animateTo` and overscroll bouncing are each
//! another activity. Here there are two states -- dragging and flinging --
//! and one physics, so they are fields on one struct, and the day a third
//! activity is wanted is the day this becomes an enum.
//!
//! # Which way is positive
//!
//! `offset` grows as the reader goes further into the content, exactly as
//! upstream's `pixels` does, and a fling's velocity is in the same direction.
//! That is *opposite* to the finger: dragging down reveals earlier content, so
//! it decreases the offset. Handlers negate, and it is worth doing in the one
//! place they do it rather than here, because a wheel does not need negating
//! and a scrollbar drag does not either.

use std::cell::Cell;
use std::rc::Rc;

use crate::physics::{ClampingScrollSimulation, Simulation};

/// A scroll offset, its limit, and any fling in progress.
///
/// The limit lives behind an [`Rc<Cell>`](std::cell::Cell) because it is not
/// known when the offset is set: how far a list can scroll depends on how tall
/// its content turned out to be, which is settled during layout, a frame after
/// whoever holds the offset needed it. [`crate::widgets::ListView::with_extent_sink`]
/// fills it in from the other side.
#[derive(Clone)]
pub struct Scroll {
    /// How far into the content the view is, in logical pixels. Always within
    /// `0..=`[`max_extent`](Scroll::max_extent) as of the last thing that
    /// moved it.
    pub offset: f32,
    /// How far this list can scroll, filled in at layout.
    pub extent: Rc<Cell<f32>>,
    /// The fling in flight, if any.
    ballistic: Option<Ballistic>,
}

/// A fling being played out.
#[derive(Clone, Copy)]
struct Ballistic {
    simulation: ClampingScrollSimulation,
    /// When it started, in frame-clock microseconds. Not known when the fling
    /// is created -- the finger lifts between frames -- so it is taken from
    /// the first frame that advances it, which is also the first frame that
    /// could draw it. Upstream a `Ticker` does the same thing: its elapsed
    /// duration is measured from its first tick, not from `start`.
    started_micros: Option<i64>,
}

impl Default for Scroll {
    fn default() -> Scroll {
        Scroll { offset: 0.0, extent: Rc::new(Cell::new(0.0)), ballistic: None }
    }
}

impl Scroll {
    pub fn new() -> Scroll {
        Scroll::default()
    }

    /// How far this list can scroll. Zero until something has measured it.
    pub fn max_extent(&self) -> f32 {
        self.extent.get().max(0.0)
    }

    /// Records how far the list can scroll, for callers that measure the
    /// content themselves rather than handing [`extent`](Scroll::extent) to a
    /// [`ListView`](crate::widgets::ListView).
    ///
    /// Takes `&self` because a build is handed its state by shared reference
    /// and the limit is discovered during the build that lays the content out.
    pub fn set_extent(&self, extent: f32) {
        self.extent.set(extent);
    }

    /// Moves by `delta` and stays inside the content.
    ///
    /// Clamping here rather than in the viewport is what stops an overscroll
    /// from banking travel: without it, flinging past the end and dragging
    /// back would do nothing until the imaginary distance had been paid off.
    ///
    /// A drag or a wheel also ends any fling. Upstream the drag *replaces* the
    /// ballistic activity, which is the same thing said in objects: whatever
    /// the reader is doing now wins over what they did a moment ago.
    pub fn scroll_by(&mut self, delta: f32) {
        self.ballistic = None;
        self.offset = (self.offset + delta).clamp(0.0, self.max_extent());
    }

    /// Puts the offset somewhere, without any physics. For jumping to a
    /// position rather than travelling to it.
    pub fn jump_to(&mut self, offset: f32) {
        self.ballistic = None;
        self.offset = offset.clamp(0.0, self.max_extent());
    }

    /// Stops a fling where it is. What a finger touching the content does.
    pub fn stop(&mut self) {
        self.ballistic = None;
    }

    /// Whether a fling is in flight.
    pub fn is_ballistic(&self) -> bool {
        self.ballistic.is_some()
    }

    /// Starts a fling at `velocity` logical pixels per second, in offset
    /// space -- positive meaning further into the content.
    ///
    /// Does nothing when there is nowhere to go, which is upstream's
    /// `ClampingScrollPhysics.createBallisticSimulation` returning null: no
    /// velocity, or already at the end the fling is heading for. Starting one
    /// anyway would cost a run of frames that each clamp to the same number.
    pub fn fling(&mut self, velocity: f32) {
        self.ballistic = None;
        if velocity == 0.0 {
            return;
        }
        if velocity > 0.0 && self.offset >= self.max_extent() {
            return;
        }
        if velocity < 0.0 && self.offset <= 0.0 {
            return;
        }
        self.ballistic = Some(Ballistic {
            simulation: ClampingScrollSimulation::new(self.offset, velocity),
            started_micros: None,
        });
    }

    /// Moves a fling on by one frame, and says whether another is wanted.
    ///
    /// Call once per frame from a
    /// [`StatefulComponent::advance`](crate::framework::StatefulComponent::advance).
    /// Returns false when nothing is moving, which is what lets the frame loop
    /// go back to sleep.
    pub fn advance(&mut self, frame_time_micros: i64) -> bool {
        let max = self.max_extent();
        let Some(ballistic) = &mut self.ballistic else {
            return false;
        };
        let started = *ballistic.started_micros.get_or_insert(frame_time_micros);
        let elapsed = (frame_time_micros - started).max(0) as f32 / 1_000_000.0;
        let position = ballistic.simulation.x(elapsed);
        let done = ballistic.simulation.is_done(elapsed);

        let clamped = position.clamp(0.0, max);
        let moved = clamped != self.offset;
        self.offset = clamped;

        // Hitting either end ends the fling, however much of the simulation is
        // left: the content has run out, and continuing would be a run of
        // frames that each clamp to the same number. Upstream's
        // `BallisticScrollActivity` stops the same way -- `applyMoveTo`
        // returning false means the position could not go where the simulation
        // asked, and the activity goes idle.
        if done || clamped != position {
            self.ballistic = None;
            return moved;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scroll with room to move, for the tests below.
    fn scroll(extent: f32) -> Scroll {
        let scroll = Scroll::new();
        scroll.set_extent(extent);
        scroll
    }

    /// Runs frames at 60Hz until nothing is moving, and returns how many.
    fn settle(scroll: &mut Scroll) -> u32 {
        let mut frames = 0;
        let mut now = 1_000_000;
        while scroll.advance(now) {
            now += 16_667;
            frames += 1;
            assert!(frames < 600, "a fling should not last ten seconds");
        }
        frames
    }

    #[test]
    fn dragging_moves_by_the_delta_and_stops_at_the_ends() {
        let mut scroll = scroll(500.0);
        scroll.scroll_by(120.0);
        assert_eq!(scroll.offset, 120.0);
        scroll.scroll_by(-400.0);
        assert_eq!(scroll.offset, 0.0);
        scroll.scroll_by(9000.0);
        assert_eq!(scroll.offset, 500.0);
    }

    #[test]
    fn a_fling_keeps_going_after_the_finger_stops() {
        let mut scroll = scroll(5000.0);
        scroll.fling(2000.0);
        assert!(scroll.is_ballistic());

        // The first frame only starts the clock, exactly as a ticker does.
        assert!(scroll.advance(1_000_000));
        assert_eq!(scroll.offset, 0.0);

        assert!(scroll.advance(1_100_000));
        let after_a_tenth = scroll.offset;
        assert!(after_a_tenth > 100.0, "a tenth of a second in: {after_a_tenth}");

        settle(&mut scroll);
        assert!(!scroll.is_ballistic());
        // The whole 2000 px/s fling, which the simulation puts at ~647px.
        assert!(
            (scroll.offset - 647.0).abs() < 10.0,
            "should have travelled the simulation's distance, not {}",
            scroll.offset
        );
    }

    #[test]
    fn a_fling_takes_more_than_a_frame_or_two() {
        // The bug this file was written for: a swipe that moved the content
        // and then stopped dead. Whatever else changes, a fling has to be
        // something a person can watch.
        let mut scroll = scroll(5000.0);
        scroll.fling(2000.0);
        let frames = settle(&mut scroll);
        assert!(frames > 30, "a fling should last some frames, not {frames}");
    }

    #[test]
    fn a_fling_stops_at_the_end_of_the_content() {
        let mut scroll = scroll(200.0);
        scroll.fling(4000.0);
        settle(&mut scroll);
        assert_eq!(scroll.offset, 200.0);
        assert!(!scroll.is_ballistic(), "and does not keep asking for frames");
    }

    #[test]
    fn a_fling_from_the_end_does_not_start() {
        let mut scroll = scroll(200.0);
        scroll.jump_to(200.0);
        scroll.fling(3000.0);
        assert!(!scroll.is_ballistic());

        // But back the other way it does.
        scroll.fling(-3000.0);
        assert!(scroll.is_ballistic());
    }

    #[test]
    fn touching_the_content_stops_the_fling() {
        let mut scroll = scroll(5000.0);
        scroll.fling(3000.0);
        scroll.advance(1_000_000);
        scroll.advance(1_050_000);
        let caught = scroll.offset;
        assert!(caught > 0.0);

        scroll.stop();
        assert!(!scroll.is_ballistic());
        assert!(!scroll.advance(1_100_000), "a stopped fling asks for nothing");
        assert_eq!(scroll.offset, caught, "and leaves the offset where it was");
    }

    #[test]
    fn dragging_during_a_fling_takes_over() {
        let mut scroll = scroll(5000.0);
        scroll.fling(3000.0);
        scroll.advance(1_000_000);
        scroll.advance(1_050_000);
        let caught = scroll.offset;

        scroll.scroll_by(-20.0);
        assert!(!scroll.is_ballistic());
        assert_eq!(scroll.offset, caught - 20.0);
    }

    #[test]
    fn a_fling_with_no_velocity_does_nothing() {
        let mut scroll = scroll(500.0);
        scroll.fling(0.0);
        assert!(!scroll.is_ballistic());
        assert!(!scroll.advance(1_000_000));
    }

    #[test]
    fn a_fling_survives_a_late_frame() {
        // Frames are on demand and the device is not always fast. A gap does
        // not break the fling; it advances by the time that actually passed.
        let mut scroll = scroll(5000.0);
        scroll.fling(2000.0);
        scroll.advance(1_000_000);
        assert!(scroll.advance(1_300_000));
        let late = scroll.offset;

        let mut steady = self::scroll(5000.0);
        steady.fling(2000.0);
        steady.advance(1_000_000);
        for step in 1..=18 {
            steady.advance(1_000_000 + step * 16_667);
        }
        assert!(
            (late - steady.offset).abs() < 20.0,
            "a late frame should land where the steady ones did: {late} against {}",
            steady.offset
        );
    }
}
