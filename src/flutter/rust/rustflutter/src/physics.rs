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

/// How close to still is still enough.
///
/// A spring never actually arrives -- it approaches its rest position for
/// ever -- so something has to decide when it has stopped mattering, and that
/// something cannot be the simulation: a scroll position settling within a
/// pixel is done, a physics toy might not be. Upstream's `Tolerance`, with the
/// same default of a thousandth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    pub distance: f32,
    pub time: f32,
    pub velocity: f32,
}

impl Tolerance {
    pub const DEFAULT: Tolerance =
        Tolerance { distance: 1e-3, time: 1e-3, velocity: 1e-3 };
}

impl Default for Tolerance {
    fn default() -> Tolerance {
        Tolerance::DEFAULT
    }
}

/// A particle's motion, sampled by time.
///
/// Time is in seconds from the start of the simulation, position in logical
/// pixels, and velocity in logical pixels per second. Upstream's `Simulation`;
/// the tolerance that decides "done" belongs to each simulation rather than to
/// the trait, because the one that finishes on a clock has no use for it.
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

// -- Springs ------------------------------------------------------------------

/// A spring, as the three numbers that describe one.
///
/// Upstream's `SpringDescription`. The three are not independent in practice:
/// what a caller usually knows is how bouncy the result should be, which is
/// [`SpringDescription::with_damping_ratio`] -- a ratio of 1 arrives without
/// overshooting, below 1 overshoots and comes back, above 1 crawls in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringDescription {
    /// The mass hanging off it.
    pub mass: f32,
    /// How hard it pulls, per unit of displacement.
    pub stiffness: f32,
    /// How much the motion is resisted, per unit of velocity.
    pub damping: f32,
}

impl SpringDescription {
    pub const fn new(mass: f32, stiffness: f32, damping: f32) -> SpringDescription {
        SpringDescription { mass, stiffness, damping }
    }

    /// A spring described by how bouncy it is instead of by its damping.
    ///
    /// `ratio` is the damping ratio: 1 is critical damping, the fastest
    /// arrival with no overshoot; less overshoots, more is sluggish.
    pub fn with_damping_ratio(mass: f32, stiffness: f32, ratio: f32) -> SpringDescription {
        SpringDescription { mass, stiffness, damping: ratio * 2.0 * (mass * stiffness).sqrt() }
    }
}

/// Which of the three solutions a spring has.
///
/// Not a detail: it is what the shape of the motion *is*. The three cases are
/// different closed-form solutions of the same equation, and which one applies
/// is decided by the discriminant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpringType {
    CriticallyDamped,
    UnderDamped,
    OverDamped,
}

/// A mass on a spring, moving towards `end`.
///
/// Upstream's `SpringSimulation`. What it is for: an arrival whose duration
/// nobody chose. A curve needs a length in seconds, which means a spring
/// interrupted halfway has to invent one; a spring interrupted halfway is
/// simply a spring with a different starting position and velocity, which is
/// why every "settle into place" gesture upstream ends in one of these.
#[derive(Clone, Copy, Debug)]
pub struct SpringSimulation {
    end: f32,
    solution: SpringSolution,
    tolerance: Tolerance,
}

#[derive(Clone, Copy, Debug)]
enum SpringSolution {
    /// `(c1 + c2 t) e^(rt)`
    Critical { r: f32, c1: f32, c2: f32 },
    /// `c1 e^(r1 t) + c2 e^(r2 t)`
    Over { r1: f32, r2: f32, c1: f32, c2: f32 },
    /// `e^(rt) (c1 cos wt + c2 sin wt)`
    Under { w: f32, r: f32, c1: f32, c2: f32 },
}

impl SpringSimulation {
    /// A spring from `start` to `end`, starting at `velocity`.
    pub fn new(
        spring: SpringDescription,
        start: f32,
        end: f32,
        velocity: f32,
    ) -> SpringSimulation {
        SpringSimulation::with_tolerance(spring, start, end, velocity, Tolerance::DEFAULT)
    }

    pub fn with_tolerance(
        spring: SpringDescription,
        start: f32,
        end: f32,
        velocity: f32,
        tolerance: Tolerance,
    ) -> SpringSimulation {
        SpringSimulation {
            end,
            solution: SpringSolution::new(spring, start - end, velocity),
            tolerance,
        }
    }

    pub fn kind(&self) -> SpringType {
        self.solution.kind()
    }
}

impl SpringSolution {
    fn new(spring: SpringDescription, distance: f32, velocity: f32) -> SpringSolution {
        let discriminant =
            spring.damping * spring.damping - 4.0 * spring.mass * spring.stiffness;
        if discriminant > 0.0 {
            let root = discriminant.sqrt();
            let r1 = (-spring.damping - root) / (2.0 * spring.mass);
            let r2 = (-spring.damping + root) / (2.0 * spring.mass);
            let c2 = (velocity - r1 * distance) / (r2 - r1);
            SpringSolution::Over { r1, r2, c1: distance - c2, c2 }
        } else if discriminant < 0.0 {
            let w = (4.0 * spring.mass * spring.stiffness - spring.damping * spring.damping)
                .sqrt()
                / (2.0 * spring.mass);
            let r = -(spring.damping / 2.0 / spring.mass);
            SpringSolution::Under { w, r, c1: distance, c2: (velocity - r * distance) / w }
        } else {
            let r = -spring.damping / (2.0 * spring.mass);
            SpringSolution::Critical { r, c1: distance, c2: velocity - r * distance }
        }
    }

    fn kind(&self) -> SpringType {
        match self {
            SpringSolution::Critical { .. } => SpringType::CriticallyDamped,
            SpringSolution::Over { .. } => SpringType::OverDamped,
            SpringSolution::Under { .. } => SpringType::UnderDamped,
        }
    }

    fn x(&self, time: f32) -> f32 {
        match *self {
            SpringSolution::Critical { r, c1, c2 } => (c1 + c2 * time) * (r * time).exp(),
            SpringSolution::Over { r1, r2, c1, c2 } => {
                c1 * (r1 * time).exp() + c2 * (r2 * time).exp()
            }
            SpringSolution::Under { w, r, c1, c2 } => {
                (r * time).exp() * (c1 * (w * time).cos() + c2 * (w * time).sin())
            }
        }
    }

    fn dx(&self, time: f32) -> f32 {
        match *self {
            SpringSolution::Critical { r, c1, c2 } => {
                let power = (r * time).exp();
                r * (c1 + c2 * time) * power + c2 * power
            }
            SpringSolution::Over { r1, r2, c1, c2 } => {
                c1 * r1 * (r1 * time).exp() + c2 * r2 * (r2 * time).exp()
            }
            SpringSolution::Under { w, r, c1, c2 } => {
                let power = (r * time).exp();
                let cosine = (w * time).cos();
                let sine = (w * time).sin();
                power * (c2 * w * cosine - c1 * w * sine)
                    + r * power * (c2 * sine + c1 * cosine)
            }
        }
    }
}

impl Simulation for SpringSimulation {
    fn x(&self, time: f32) -> f32 {
        self.end + self.solution.x(time)
    }

    fn dx(&self, time: f32) -> f32 {
        self.solution.dx(time)
    }

    fn is_done(&self, time: f32) -> bool {
        // Both, not either: a spring at the top of its swing is momentarily
        // still and nowhere near finished, and one passing through its rest
        // position at speed is exactly at the end and not finished either.
        self.solution.x(time).abs() < self.tolerance.distance
            && self.solution.dx(time).abs() < self.tolerance.velocity
    }
}

// -- Friction -----------------------------------------------------------------

/// Something sliding to a stop, with velocity decaying exponentially.
///
/// Upstream's `FrictionSimulation`, which is what `BouncingScrollPhysics`
/// flings with -- iOS-style, where the deceleration is proportional to the
/// speed rather than shaped by a spline. `drag` is the fraction of the speed
/// that survives each second: 0.135 is upstream's iOS scrolling value.
#[derive(Clone, Copy, Debug)]
pub struct FrictionSimulation {
    drag: f32,
    drag_log: f32,
    position: f32,
    velocity: f32,
    tolerance: Tolerance,
}

impl FrictionSimulation {
    pub fn new(drag: f32, position: f32, velocity: f32) -> FrictionSimulation {
        FrictionSimulation::with_tolerance(drag, position, velocity, Tolerance::DEFAULT)
    }

    pub fn with_tolerance(
        drag: f32,
        position: f32,
        velocity: f32,
        tolerance: Tolerance,
    ) -> FrictionSimulation {
        FrictionSimulation { drag, drag_log: drag.ln(), position, velocity, tolerance }
    }

    /// Where it comes to rest.
    pub fn final_x(&self) -> f32 {
        self.position - self.velocity / self.drag_log
    }
}

impl Simulation for FrictionSimulation {
    fn x(&self, time: f32) -> f32 {
        self.position + self.velocity * self.drag.powf(time) / self.drag_log
            - self.velocity / self.drag_log
    }

    fn dx(&self, time: f32) -> f32 {
        self.velocity * self.drag.powf(time)
    }

    fn is_done(&self, time: f32) -> bool {
        self.dx(time).abs() < self.tolerance.velocity
    }
}

// -- Gravity ------------------------------------------------------------------

/// Constant acceleration, until it has fallen far enough.
///
/// Upstream's `GravitySimulation`. The simplest of the three and the one that
/// is mostly used for demonstrations, but it is also what a "throw this off
/// the screen" dismissal is.
#[derive(Clone, Copy, Debug)]
pub struct GravitySimulation {
    acceleration: f32,
    position: f32,
    velocity: f32,
    end: f32,
}

impl GravitySimulation {
    /// Falls from `position` at `velocity`, accelerating at `acceleration`,
    /// and is done once it is `end` away from zero.
    pub fn new(
        acceleration: f32,
        position: f32,
        end: f32,
        velocity: f32,
    ) -> GravitySimulation {
        GravitySimulation { acceleration, position, velocity, end: end.abs() }
    }
}

impl Simulation for GravitySimulation {
    fn x(&self, time: f32) -> f32 {
        self.position + self.velocity * time + 0.5 * self.acceleration * time * time
    }

    fn dx(&self, time: f32) -> f32 {
        self.velocity + time * self.acceleration
    }

    fn is_done(&self, time: f32) -> bool {
        self.x(time).abs() >= self.end
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

    // -- Springs --------------------------------------------------------------

    #[test]
    fn a_critically_damped_spring_arrives_without_overshooting() {
        let spring = SpringDescription::with_damping_ratio(1.0, 100.0, 1.0);
        let simulation = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
        assert_eq!(simulation.kind(), SpringType::CriticallyDamped);
        for step in 0..200 {
            let x = simulation.x(step as f32 / 100.0);
            assert!(x <= 100.0 + 1e-3, "overshot to {x}");
        }
        assert!(simulation.is_done(2.0), "should have settled by two seconds");
        assert!((simulation.x(2.0) - 100.0).abs() < 0.1);
    }

    #[test]
    fn an_underdamped_spring_overshoots_and_comes_back() {
        let spring = SpringDescription::with_damping_ratio(1.0, 100.0, 0.3);
        let simulation = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
        assert_eq!(simulation.kind(), SpringType::UnderDamped);
        let peak = (0..200)
            .map(|step| simulation.x(step as f32 / 100.0))
            .fold(f32::MIN, f32::max);
        assert!(peak > 100.0, "a bouncy spring should pass its target, not stop at {peak}");
        assert!(simulation.is_done(5.0), "and should still settle");
    }

    #[test]
    fn an_overdamped_spring_crawls_in() {
        let spring = SpringDescription::with_damping_ratio(1.0, 100.0, 2.5);
        let simulation = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
        assert_eq!(simulation.kind(), SpringType::OverDamped);
        assert!(!simulation.is_done(0.5), "sluggish is the point");
        for step in 0..400 {
            assert!(simulation.x(step as f32 / 100.0) <= 100.0 + 1e-3);
        }
    }

    #[test]
    fn a_spring_starts_where_it_was_and_at_the_speed_it_was_going() {
        let spring = SpringDescription::with_damping_ratio(1.0, 200.0, 0.8);
        let simulation = SpringSimulation::new(spring, 30.0, 100.0, -400.0);
        assert!((simulation.x(0.0) - 30.0).abs() < 1e-3);
        assert!((simulation.dx(0.0) + 400.0).abs() < 1e-2);
    }

    #[test]
    fn a_spring_is_not_done_while_it_is_passing_through() {
        // Still at the top of its swing, or moving fast through the middle:
        // neither is finished, and checking only one of position or velocity
        // would call one of them done.
        let spring = SpringDescription::with_damping_ratio(1.0, 100.0, 0.2);
        let simulation = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
        let crossing = (0..200)
            .map(|step| step as f32 / 100.0)
            .find(|time| simulation.x(*time) >= 100.0)
            .expect("an underdamped spring crosses its target");
        assert!(!simulation.is_done(crossing), "moving fast, exactly at the end");
    }

    // -- Friction -------------------------------------------------------------

    #[test]
    fn friction_slows_down_and_stops_somewhere() {
        // Upstream's iOS scrolling drag.
        let simulation = FrictionSimulation::new(0.135, 0.0, 600.0);
        assert!((simulation.dx(0.0) - 600.0).abs() < 1e-3);
        assert!(simulation.dx(1.0) < simulation.dx(0.5));
        // Exponential decay has no end, so "done" is where the tolerance puts
        // it: 600 px/s at this drag takes about six and a half seconds to fall
        // under a thousandth of a pixel per second. It has travelled all but a
        // hundredth of a pixel long before that.
        assert!(!simulation.is_done(5.0));
        assert!(simulation.is_done(8.0));
        let settled = simulation.x(5.0);
        assert!(
            (settled - simulation.final_x()).abs() < 1.0,
            "{settled} should be about {}",
            simulation.final_x()
        );
    }

    #[test]
    fn friction_the_other_way_travels_the_other_way() {
        let simulation = FrictionSimulation::new(0.135, 100.0, -600.0);
        assert!(simulation.final_x() < 100.0);
    }

    // -- Gravity --------------------------------------------------------------

    #[test]
    fn gravity_accelerates_and_finishes_at_the_edge() {
        let simulation = GravitySimulation::new(1000.0, 0.0, 500.0, 0.0);
        assert_eq!(simulation.x(0.0), 0.0);
        assert!((simulation.x(1.0) - 500.0).abs() < 1e-3, "half of a t squared");
        assert!(!simulation.is_done(0.5));
        assert!(simulation.is_done(1.0));
        assert!((simulation.dx(1.0) - 1000.0).abs() < 1e-3);
    }

    #[test]
    fn more_friction_is_a_shorter_fling() {
        let slippery = ClampingScrollSimulation::with_friction(0.0, 2000.0, 0.015);
        let sticky = ClampingScrollSimulation::with_friction(0.0, 2000.0, 0.05);
        assert!(sticky.final_x() < slippery.final_x());
        assert!(sticky.duration() < slippery.duration());
    }
}
