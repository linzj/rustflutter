// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Smoothing pointer events onto the frame clock (upstream
//! `gestures/resampler.dart`, and the small pieces around it in
//! `gestures/binding.dart`, `gestures/drag.dart`,
//! `gestures/pointer_signal_resolver.dart`).
//!
//! A touchscreen samples the finger on its own clock, which is not the
//! display's. Left alone, the positions arrive in clumps -- two in one frame,
//! none in the next -- and a finger moving smoothly looks like it stutters.
//! The resampler fixes the frame time and asks where the finger *was* then,
//! interpolating between the two real samples on either side.
//!
//! # Recorded divergences
//!
//! * Upstream's resampler builds real `PointerEvent`s of the right subtype --
//!   a hover for an untracked pointer, a move for a tracked one -- because it
//!   feeds them back into the same dispatch. Here it answers positions and
//!   the kind of event they are, and the caller builds the event, because
//!   this crate's `PointerEvent` is one struct with a `change` field rather
//!   than a family.
//! * Upstream reports an error through `FlutterError` when a signal callback
//!   throws. There is nothing to throw here, so
//!   [`PointerSignalResolver::resolve`] simply runs the callback.

use std::collections::VecDeque;

use crate::gestures::PointerEvent;
use crate::render::Offset;

/// Upstream `SamplingClock`: the clock the resampler measures against.
///
/// Upstream's exists so a test can replace it -- resampling is entirely about
/// time, and a test that could not control the clock could not check any of
/// it. The same reason it is a type here rather than a call to the system.
pub struct SamplingClock {
    now_micros: i64,
}

impl Default for SamplingClock {
    fn default() -> SamplingClock {
        SamplingClock::new()
    }
}

impl SamplingClock {
    pub fn new() -> SamplingClock {
        SamplingClock { now_micros: 0 }
    }

    /// Upstream `now`.
    pub fn now_micros(&self) -> i64 {
        self.now_micros
    }

    /// Moves the clock, which is what upstream's test clock does and what the
    /// frame loop does to the real one.
    pub fn set_now_micros(&mut self, now_micros: i64) {
        self.now_micros = now_micros;
    }
}

/// What one resampled sample is: where the pointer was, and whether it was
/// down at the time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResampledPosition {
    pub position: Offset,
    /// The movement since the last sample handed out, which is what a drag
    /// recogniser adds up.
    pub delta: Offset,
    pub is_down: bool,
}

/// Upstream `PointerEventResampler`: one pointer's events, held back and
/// handed out on the frame clock.
#[derive(Default)]
pub struct PointerEventResampler {
    queued_events: VecDeque<PointerEvent>,
    /// The two real samples the interpolation runs between: the last one at
    /// or before the sample time, and the first one after it.
    last: Option<PointerEvent>,
    next: Option<PointerEvent>,
    position: Offset,
    is_tracked: bool,
    is_down: bool,
}

impl PointerEventResampler {
    pub fn new() -> PointerEventResampler {
        PointerEventResampler::default()
    }

    /// Upstream `addEvent`.
    pub fn add_event(&mut self, event: PointerEvent) {
        self.queued_events.push_back(event);
    }

    /// Upstream `hasPendingEvents`.
    pub fn has_pending_events(&self) -> bool {
        !self.queued_events.is_empty()
    }

    pub fn is_tracked(&self) -> bool {
        self.is_tracked
    }

    pub fn is_down(&self) -> bool {
        self.is_down
    }

    /// Upstream's `_positionAt`: where the pointer was at `sample_time`.
    ///
    /// The interpolation only runs when the next sample is genuinely after
    /// the sample time *and* after the last one. Both guards matter: without
    /// the first the pointer would be dragged backwards past a sample it has
    /// already reached, and without the second two samples with the same
    /// timestamp would divide by zero.
    pub fn position_at(&self, sample_time_micros: i64) -> Offset {
        let Some(next) = &self.next else {
            return Offset::ZERO;
        };
        let next_time = next.time_stamp_micros;
        let last_time = self.last.as_ref().map_or(0, |last| last.time_stamp_micros);
        if next_time > sample_time_micros && next_time > last_time {
            let interval = (next_time - last_time) as f32;
            let scalar = (sample_time_micros - last_time) as f32 / interval;
            let last_position = self
                .last
                .as_ref()
                .map_or(Offset::ZERO, |last| last.position);
            return Offset::new(
                last_position.dx + (next.position.dx - last_position.dx) * scalar,
                last_position.dy + (next.position.dy - last_position.dy) * scalar,
            );
        }
        next.position
    }

    /// Upstream's `_processPointerEvents`: picks the two samples the
    /// interpolation runs between, without taking anything off the queue.
    fn process_pointer_events(&mut self, sample_time_micros: i64) {
        for event in &self.queued_events {
            if event.time_stamp_micros <= sample_time_micros || self.last.is_none() {
                self.last = Some(event.clone());
                self.next = Some(event.clone());
                continue;
            }
            let next_time = self.next.as_ref().map_or(0, |next| next.time_stamp_micros);
            if next_time < sample_time_micros {
                self.next = Some(event.clone());
                break;
            }
        }
    }

    /// Upstream `sample`: everything due by `sample_time`, then the
    /// interpolated position for the frame.
    ///
    /// `next_sample_time` is the *following* frame's time. Upstream needs it
    /// because a press that lands between the two frames should still be
    /// delivered this frame -- waiting a frame to report a touch is what
    /// makes an interface feel slow.
    pub fn sample(
        &mut self,
        sample_time_micros: i64,
        next_sample_time_micros: i64,
        mut handle: impl FnMut(PointerEvent),
    ) -> Option<ResampledPosition> {
        self.process_pointer_events(sample_time_micros);
        self.dequeue_until(sample_time_micros, next_sample_time_micros, &mut handle);
        if !self.is_tracked {
            return None;
        }
        // Upstream's `_samplePointerPosition`: a move only where the position
        // actually changed, so a still finger produces no events at all.
        let position = self.position_at(sample_time_micros);
        if position == self.position || self.next.is_none() {
            return None;
        }
        let delta = Offset::new(
            position.dx - self.position.dx,
            position.dy - self.position.dy,
        );
        self.position = position;
        Some(ResampledPosition {
            position,
            delta,
            is_down: self.is_down,
        })
    }

    /// Upstream's `_dequeueAndSampleNonHoverOrMovePointerEventsUntil`.
    ///
    /// Moves and hovers are what the interpolation replaces, so they are
    /// swallowed; everything else -- down, up, cancel -- is a fact rather
    /// than a position and goes straight through, with its timestamp moved to
    /// the frame's.
    fn dequeue_until(
        &mut self,
        sample_time_micros: i64,
        next_sample_time_micros: i64,
        handle: &mut impl FnMut(PointerEvent),
    ) {
        let mut end_time = sample_time_micros;
        for event in &self.queued_events {
            if event.time_stamp_micros > sample_time_micros {
                if event.time_stamp_micros >= next_sample_time_micros {
                    break;
                }
                // An up or a remove between the frames extends the window:
                // the finger has left, and holding that back to the next
                // frame would leave a gesture hanging.
                if matches!(
                    event.change,
                    crate::gestures::PointerChange::Up | crate::gestures::PointerChange::Remove
                ) {
                    end_time = event.time_stamp_micros;
                    continue;
                }
                if !matches!(
                    event.change,
                    crate::gestures::PointerChange::Move | crate::gestures::PointerChange::Hover
                ) {
                    break;
                }
            }
        }

        while let Some(event) = self.queued_events.front() {
            if event.time_stamp_micros > end_time {
                break;
            }
            let event = self.queued_events.pop_front().expect("just looked at it");
            let was_tracked = self.is_tracked;
            match event.change {
                crate::gestures::PointerChange::Add => self.is_tracked = true,
                crate::gestures::PointerChange::Remove => {
                    self.is_tracked = false;
                    self.is_down = false;
                }
                crate::gestures::PointerChange::Down => {
                    self.is_tracked = true;
                    self.is_down = true;
                }
                crate::gestures::PointerChange::Up | crate::gestures::PointerChange::Cancel => {
                    self.is_down = false
                }
                _ => {}
            }
            // Upstream: a pointer that has only just started being tracked
            // has no history to interpolate from, so its position is taken
            // as it is rather than blended with a stale one.
            let position = self.position_at(sample_time_micros);
            if self.is_tracked && !was_tracked {
                self.position = position;
            }
            if matches!(
                event.change,
                crate::gestures::PointerChange::Move | crate::gestures::PointerChange::Hover
            ) {
                // Swallowed: this is exactly what the interpolation replaces.
                continue;
            }
            let mut delivered = event;
            delivered.position = self.position;
            delivered.time_stamp_micros = sample_time_micros;
            handle(delivered);
        }
    }

    /// Upstream `stop`: hand out everything still queued, unresampled, and
    /// forget the pointer.
    ///
    /// Called when resampling is switched off or the pointer is gone. Holding
    /// events back for a frame that will never come is how a gesture gets
    /// stuck.
    pub fn stop(&mut self, mut handle: impl FnMut(PointerEvent)) {
        while let Some(event) = self.queued_events.pop_front() {
            handle(event);
        }
        self.last = None;
        self.next = None;
        self.position = Offset::ZERO;
        self.is_tracked = false;
        self.is_down = false;
    }
}

/// Upstream `PointerSignalResolver`: which one thing gets a scroll.
///
/// A scroll lands on every region under the pointer, and only one of them
/// should act on it -- a list inside a page should take the wheel, and the
/// page should not scroll too. The rule is first come: the innermost region
/// registers first, because the hit path is innermost-first, and everything
/// after it is ignored.
#[derive(Default)]
pub struct PointerSignalResolver {
    first_registered: Option<Box<dyn FnOnce()>>,
    registered: bool,
}

impl PointerSignalResolver {
    pub fn new() -> PointerSignalResolver {
        PointerSignalResolver::default()
    }

    /// Upstream `register`.
    pub fn register(&mut self, callback: impl FnOnce() + 'static) {
        if self.registered {
            return;
        }
        self.registered = true;
        self.first_registered = Some(Box::new(callback));
    }

    /// Whether anybody wants this signal.
    pub fn has_registrant(&self) -> bool {
        self.registered
    }

    /// Upstream `resolve`: runs the first registrant, and answers whether
    /// there was one.
    ///
    /// Upstream's false case tells the platform it may take its own default
    /// action -- a wheel over a web view that nothing in the application
    /// wanted should still scroll the page. The answer here is that same
    /// permission, for a caller that has a platform to tell.
    pub fn resolve(&mut self) -> bool {
        self.registered = false;
        match self.first_registered.take() {
            Some(callback) => {
                callback();
                true
            }
            None => false,
        }
    }
}

/// Upstream `Drag`: what a recogniser hands back when it starts a drag.
///
/// The recogniser stops being involved once the drag is under way -- from
/// then on the events go straight to this, which is what lets a drag survive
/// the recogniser being rebuilt out from under it.
pub trait Drag {
    /// Upstream `update`.
    fn update(&mut self, delta: Offset);
    /// Upstream `end`.
    fn end(&mut self, velocity: Offset);
    /// Upstream `cancel`.
    fn cancel(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gestures::{PointerChange, PointerKind, SignalKind};

    fn at(change: PointerChange, x: f32, micros: i64) -> PointerEvent {
        PointerEvent {
            view_id: 0,
            device: 0,
            pointer_id: 1,
            change,
            kind: PointerKind::Touch,
            signal_kind: SignalKind::None,
            buttons: 1,
            time_stamp_micros: micros,
            position: Offset::new(x, 0.0),
            delta: Offset::ZERO,
            scroll_delta: Offset::ZERO,
            pressure: 1.0,
            local_position: Offset::new(x, 0.0),
        }
    }

    /// A pointer down at 0 and moves at 10 and 20 milliseconds, which is the
    /// ordinary case: the touchscreen's clock and the display's do not line
    /// up.
    fn moving() -> PointerEventResampler {
        let mut resampler = PointerEventResampler::new();
        resampler.add_event(at(PointerChange::Down, 0.0, 0));
        resampler.add_event(at(PointerChange::Move, 10.0, 10_000));
        resampler.add_event(at(PointerChange::Move, 20.0, 20_000));
        resampler
    }

    #[test]
    fn the_position_a_frame_reports_is_the_point_between_two_samples() {
        // The whole purpose. Without it the finger jumps ten pixels on one
        // frame and none on the next, which is what a stutter is.
        let resampler = {
            let mut resampler = moving();
            resampler.sample(5_000, 21_000, |_| {});
            resampler
        };
        assert_eq!(
            resampler.position_at(5_000),
            Offset::new(5.0, 0.0),
            "half way between the sample at 0 and the one at 10ms"
        );
        assert_eq!(resampler.position_at(2_500), Offset::new(2.5, 0.0));
    }

    #[test]
    fn the_delivered_event_carries_the_interpolated_position_not_its_own() {
        // The down happened at zero and the frame is at five milliseconds, so
        // what the framework is told is where the finger was at the frame --
        // and the event is stamped with the frame's time too. Delivering the
        // event's own position would undo the resampling for the one event
        // that starts the gesture.
        let mut resampler = moving();
        let mut delivered = Vec::new();
        let sampled = resampler.sample(5_000, 21_000, |event| {
            delivered.push((event.change, event.position.dx, event.time_stamp_micros))
        });
        assert_eq!(delivered, vec![(PointerChange::Down, 5.0, 5_000)]);
        // And no separate sample: the down already carried the position, so
        // reporting it again would be a move of nothing.
        assert!(sampled.is_none());
    }

    #[test]
    fn later_frames_report_the_movement_since_the_last_one() {
        // The delta is measured against the last position handed out, not the
        // last real event: a drag recogniser adds these up, and deltas
        // measured against anything else would drift.
        let mut resampler = moving();
        resampler.sample(5_000, 6_000, |_| {});
        let second = resampler.sample(7_500, 8_500, |_| {}).expect("a sample");
        assert_eq!(second.position, Offset::new(7.5, 0.0));
        assert_eq!(second.delta, Offset::new(2.5, 0.0));
        assert!(second.is_down);
    }

    #[test]
    fn a_frame_past_the_last_sample_gets_the_last_sample() {
        // No extrapolation: guessing where a finger went after it stopped
        // reporting is how a flick overshoots.
        let mut resampler = moving();
        let mut delivered = Vec::new();
        resampler.sample(30_000, 40_000, |event| delivered.push(event.position.dx));
        assert_eq!(delivered, vec![20.0]);
        assert_eq!(resampler.position_at(50_000), Offset::new(20.0, 0.0));
    }

    #[test]
    fn a_still_finger_produces_no_sample_at_all() {
        // The position did not change, so there is nothing to report -- and
        // reporting it anyway would wake every drag recogniser on every frame
        // for a finger that has not moved.
        let mut resampler = PointerEventResampler::new();
        resampler.add_event(at(PointerChange::Down, 7.0, 0));
        resampler.sample(1_000, 2_000, |_| {});
        assert!(
            resampler.sample(2_000, 3_000, |_| {}).is_none(),
            "nothing moved between the two frames"
        );
    }

    #[test]
    fn two_samples_with_the_same_timestamp_do_not_divide_by_zero() {
        // The second guard in `_positionAt`. A touchscreen that reports two
        // positions in the same microsecond is not hypothetical, and without
        // the guard the interval is zero and the position is a NaN that then
        // travels through every gesture recogniser.
        let mut resampler = PointerEventResampler::new();
        resampler.add_event(at(PointerChange::Down, 0.0, 1_000));
        resampler.add_event(at(PointerChange::Move, 10.0, 1_000));
        resampler.sample(1_000, 2_000, |_| {});
        assert!(resampler.position_at(1_000).dx.is_finite());
        assert!(resampler.position_at(500).dx.is_finite());
    }

    #[test]
    fn a_press_between_the_frames_waits_and_only_a_release_does_not() {
        // Only an up or a remove extends the window, which is easy to
        // read as an oversight and is not. The resampler samples in the
        // past, so a press at 12ms has not happened yet at a frame timed
        // 10ms, and delivering it would report a touch before it
        // occurred. A release is the other way round: the pointer is
        // gone, there is nothing left to interpolate towards, and
        // holding it back strands the gesture for a frame.
        let mut resampler = PointerEventResampler::new();
        resampler.add_event(at(PointerChange::Down, 3.0, 12_000));
        let mut delivered = Vec::new();
        resampler.sample(10_000, 16_000, |event| delivered.push(event.change));
        assert!(delivered.is_empty(), "the press waits for its frame");
        assert!(resampler.has_pending_events());

        // The next frame reaches it, stamped with that frame's time.
        let mut delivered = Vec::new();
        resampler.sample(16_000, 22_000, |event| {
            delivered.push((event.change, event.time_stamp_micros))
        });
        assert_eq!(delivered, vec![(PointerChange::Down, 16_000)]);
    }

    #[test]
    fn an_event_past_the_next_frame_waits_for_it() {
        let mut resampler = PointerEventResampler::new();
        resampler.add_event(at(PointerChange::Down, 3.0, 20_000));
        let mut delivered = Vec::new();
        resampler.sample(10_000, 16_000, |event| delivered.push(event.change));
        assert!(delivered.is_empty());
        assert!(resampler.has_pending_events());
    }

    #[test]
    fn a_release_between_the_frames_extends_the_window() {
        // The finger has left. Holding that back to the next frame leaves a
        // gesture hanging for a frame with nothing arriving to end it.
        let mut resampler = PointerEventResampler::new();
        resampler.add_event(at(PointerChange::Down, 0.0, 0));
        resampler.add_event(at(PointerChange::Up, 0.0, 12_000));
        let mut delivered = Vec::new();
        resampler.sample(10_000, 16_000, |event| delivered.push(event.change));
        assert_eq!(delivered, vec![PointerChange::Down, PointerChange::Up]);
        assert!(!resampler.is_down());
    }

    #[test]
    fn moves_are_swallowed_because_they_are_what_the_interpolation_replaces() {
        // Every queued move is consumed and none is delivered; what comes out
        // is the interpolated position instead. Passing them through as well
        // would deliver the stutter and the smoothing.
        let mut resampler = moving();
        let mut delivered = Vec::new();
        resampler.sample(30_000, 40_000, |event| delivered.push(event.change));
        assert_eq!(delivered, vec![PointerChange::Down]);
        assert!(!resampler.has_pending_events(), "all three were consumed");
    }

    #[test]
    fn stopping_hands_out_everything_unresampled_and_forgets_the_pointer() {
        // Holding events back for a frame that will never come is how a
        // gesture gets stuck.
        let mut resampler = moving();
        let mut delivered = Vec::new();
        resampler.stop(|event| delivered.push((event.change, event.time_stamp_micros)));
        assert_eq!(
            delivered,
            vec![
                (PointerChange::Down, 0),
                (PointerChange::Move, 10_000),
                (PointerChange::Move, 20_000)
            ],
            "with their own timestamps, not a frame's"
        );
        assert!(!resampler.has_pending_events());
        assert!(!resampler.is_tracked());
    }

    #[test]
    fn the_first_registrant_takes_the_signal_and_the_rest_are_ignored() {
        // A scroll lands on every region under the pointer, and only one
        // should act: a list inside a page takes the wheel and the page does
        // not scroll too. The hit path is innermost-first, so first come is
        // innermost wins.
        let taken = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut resolver = PointerSignalResolver::new();
        let inner = std::rc::Rc::clone(&taken);
        resolver.register(move || inner.set(1));
        let outer = std::rc::Rc::clone(&taken);
        resolver.register(move || outer.set(2));
        assert!(resolver.resolve());
        assert_eq!(taken.get(), 1, "the innermost registrant, not the last");
    }

    #[test]
    fn a_signal_nobody_wanted_says_so() {
        // Upstream's false is permission for the platform to do its own
        // thing -- a wheel over a web view that the application ignored
        // should still scroll the page.
        let mut resolver = PointerSignalResolver::new();
        assert!(!resolver.has_registrant());
        assert!(!resolver.resolve());
        // And resolving clears, so the next signal starts over.
        resolver.register(|| {});
        assert!(resolver.resolve());
        assert!(!resolver.resolve());
    }

    #[test]
    fn the_sampling_clock_is_settable_because_resampling_is_all_about_time() {
        // Upstream's exists so a test can replace it; a test that could not
        // control the clock could not check any of this.
        let mut clock = SamplingClock::new();
        assert_eq!(clock.now_micros(), 0);
        clock.set_now_micros(16_000);
        assert_eq!(clock.now_micros(), 16_000);
    }
}
