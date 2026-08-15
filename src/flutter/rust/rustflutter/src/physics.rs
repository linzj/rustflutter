// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Simulations: where something goes after the finger lets go.
//!
//! A drag is not an animation. It has no duration and no curve, because the
//! reader is holding it -- the content is wherever the finger put it. What
//! happens *after* the finger lifts is the animation, and it is not a curve
//! either: nobody chose how long it should take. It is a particle with a
//! starting position and a starting velocity, slowing down, and the only
//! question is where physics leaves it.
//!
//! That is what a simulation is: a function from time to position, plus the
//! velocity at that time and a way to say when it has stopped mattering. It
//! knows nothing about scrolling, widgets or frames.
//!
//! # Which physics
//!
//! Upstream picks by platform, in `ScrollBehavior.getScrollPhysics`: iOS and
//! macOS get `BouncingScrollPhysics`, everything else -- Android, Windows,
//! Linux, Fuchsia -- gets `ClampingScrollPhysics`. Both platforms this runs on
//! are in the second group, so [`ClampingScrollSimulation`] is the only one
//! here. The bouncing one is a friction simulation handed off to a spring at
//! the edges, and is worth writing the day there is an iOS host to want it.

/// A particle's motion, sampled by time.
///
/// Time is in seconds from the start of the simulation, position in logical
/// pixels, and velocity in logical pixels per second. Upstream's `Simulation`,
/// minus the tolerance: the one simulation below finishes on a clock rather
/// than by converging, so there is nothing for a tolerance to decide.
pub trait Simulation {
    /// Where the particle is at `time`.
    fn x(&self, time: f32) -> f32;

    /// How fast it is going at `time`.
    fn dx(&self, time: f32) -> f32;

    /// Whether it has finished. A simulation that is done stays where `x` last
    /// put it.
    fn is_done(&self, time: f32) -> bool;
}

/// A fling that decelerates the way Android's does.
///
/// This is a port of upstream's `ClampingScrollSimulation`, which is in turn a
/// port of `SplineOverScroller.fling` from Android's `OverScroller.java` -- the
/// same curve `RecyclerView` flings with. Travelling exactly as far as the
/// platform does is the whole point: a list that keeps going too long feels
/// slippery and one that stops too soon feels like it is dragging, and neither
/// is something to guess at when the platform has already decided.
///
/// The one deliberate difference from Android, which upstream also makes: this
/// is *ballistic*, meaning deceleration depends only on the current velocity
/// and not on how long ago the fling started. That is what lets a fling be
/// interrupted and restarted from wherever it got to. Android's version moves
/// slightly faster at the start and slightly slower at the end, and arrives at
/// the same place a little later.
#[derive(Clone, Copy, Debug)]
pub struct ClampingScrollSimulation {
    /// Where the fling started, in logical pixels.
    position: f32,
    /// How fast it started, in logical pixels per second.
    velocity: f32,
    /// How long the whole fling lasts, in seconds.
    duration: f32,
    /// How far it travels in total, signed like the velocity.
    distance: f32,
}

/// See `DECELERATION_RATE` in `OverScroller.java`. Not a `const` because `ln`
/// is not available at compile time; it is two logarithms once per fling.
fn deceleration_rate() -> f32 {
    0.78f32.ln() / 0.9f32.ln()
}

/// See `INFLEXION`.
const INFLEXION: f32 = 0.35;

/// See `mPhysicalCoeff`: gravity, scaled by a "look and feel" constant, in
/// logical pixels per second squared.
const PHYSICAL_COEFF: f32 = 9.80665 // g, in metres per second^2
    * 39.37 // inches per metre
    * 160.0 // logical pixels per inch
    * 0.84; // "look and feel tuning"

/// See `mFlingFriction`. The value that makes the distance match Android's.
pub const DEFAULT_FRICTION: f32 = 0.015;

impl ClampingScrollSimulation {
    /// A fling from `position` at `velocity` logical pixels per second.
    pub fn new(position: f32, velocity: f32) -> ClampingScrollSimulation {
        ClampingScrollSimulation::with_friction(position, velocity, DEFAULT_FRICTION)
    }

    /// The same, with the friction to travel a different distance. More
    /// friction is a shorter, sooner-finished fling.
    pub fn with_friction(position: f32, velocity: f32, friction: f32) -> ClampingScrollSimulation {
        let rate = deceleration_rate();

        // See getSplineFlingDuration(). The Android version's value is in
        // milliseconds; this is the same number in seconds, finishing a little
        // sooner so that the total distance comes out the same.
        let reference_velocity = friction * PHYSICAL_COEFF / INFLEXION;
        let duration = if velocity == 0.0 {
            0.0
        } else {
            let android = (velocity.abs() / reference_velocity).powf(1.0 / (rate - 1.0));
            rate * INFLEXION * android
        };

        // See getSplineFlingDistance(), which computes this the long way round
        // -- exp(log(v/v0) * rate / (rate - 1)) times friction times the
        // coefficient -- and arrives at the same number. Signed, so that a
        // fling upwards travels upwards.
        let distance = velocity * duration / rate;

        ClampingScrollSimulation { position, velocity, duration, distance }
    }

    /// How long this fling lasts, in seconds. Nothing needs it to animate; it
    /// is here because a test that asserts on where a fling ends has to know
    /// when the end is.
    pub fn duration(&self) -> f32 {
        self.duration
    }

    /// Where the fling finishes.
    pub fn final_x(&self) -> f32 {
        self.position + self.distance
    }
}

impl Simulation for ClampingScrollSimulation {
    fn x(&self, time: f32) -> f32 {
        if self.duration <= 0.0 {
            return self.position;
        }
        let t = (time / self.duration).clamp(0.0, 1.0);
        self.position + self.distance * (1.0 - (1.0 - t).powf(deceleration_rate()))
    }

    fn dx(&self, time: f32) -> f32 {
        if self.duration <= 0.0 {
            return 0.0;
        }
        let t = (time / self.duration).clamp(0.0, 1.0);
        self.velocity * (1.0 - t).powf(deceleration_rate() - 1.0)
    }

    fn is_done(&self, time: f32) -> bool {
        time >= self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fling_starts_where_it_was_and_at_the_speed_it_was_going() {
        let fling = ClampingScrollSimulation::new(100.0, 2000.0);
        assert_eq!(fling.x(0.0), 100.0);
        assert!((fling.dx(0.0) - 2000.0).abs() < 1.0);
    }

    #[test]
    fn a_fling_slows_down_and_stops() {
        let fling = ClampingScrollSimulation::new(0.0, 2000.0);
        let early = fling.dx(0.05);
        let late = fling.dx(fling.duration() * 0.9);
        assert!(early > late, "{early} should be faster than {late}");
        assert!(late >= 0.0);
        assert!(fling.is_done(fling.duration()));
        assert!(!fling.is_done(fling.duration() * 0.99));
    }

    #[test]
    fn a_fling_ends_where_the_distance_says() {
        let fling = ClampingScrollSimulation::new(0.0, 2000.0);
        let travelled = fling.x(fling.duration());
        assert!((travelled - fling.final_x()).abs() < 0.5);
        // Held to the platform's number rather than to itself: Android's
        // spline takes a 2000 px/s fling about 647 logical pixels, over about
        // three quarters of a second. A change here means the curve stopped
        // matching the platform, which is the only thing this simulation is
        // for.
        assert!(
            (travelled - 647.0).abs() < 10.0,
            "a 2000 px/s fling should travel about 647px, not {travelled}"
        );
        assert!(
            (fling.duration() - 0.76).abs() < 0.05,
            "and should take about 0.76s, not {}",
            fling.duration()
        );
    }

    #[test]
    fn a_fling_never_goes_backwards() {
        let fling = ClampingScrollSimulation::new(0.0, 1200.0);
        let mut previous = f32::NEG_INFINITY;
        for step in 0..60 {
            let x = fling.x(step as f32 / 60.0);
            assert!(x >= previous, "went backwards at {step}: {x} after {previous}");
            previous = x;
        }
    }

    #[test]
    fn a_fling_the_other_way_travels_the_other_way() {
        let up = ClampingScrollSimulation::new(500.0, -2000.0);
        assert!(up.final_x() < 500.0);
        assert!((up.final_x() - (500.0 - 647.0)).abs() < 10.0);
    }

    #[test]
    fn a_fling_with_no_velocity_goes_nowhere() {
        // Not reachable through `Scroll`, which will not start one, but a
        // simulation that divided by its own zero duration would be a NaN
        // loose in a layout.
        let still = ClampingScrollSimulation::new(42.0, 0.0);
        assert_eq!(still.x(0.0), 42.0);
        assert_eq!(still.x(1.0), 42.0);
        assert_eq!(still.dx(0.0), 0.0);
        assert!(still.is_done(0.0));
    }

    #[test]
    fn more_friction_is_a_shorter_fling() {
        let slippery = ClampingScrollSimulation::with_friction(0.0, 2000.0, 0.015);
        let sticky = ClampingScrollSimulation::with_friction(0.0, 2000.0, 0.05);
        assert!(sticky.final_x() < slippery.final_x());
        assert!(sticky.duration() < slippery.duration());
    }
}
