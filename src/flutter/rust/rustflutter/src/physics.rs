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
    pub const DEFAULT: Tolerance = Tolerance {
        distance: 1e-3,
        time: 1e-3,
        velocity: 1e-3,
    };
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

        ClampingScrollSimulation {
            position,
            velocity,
            duration,
            distance,
        }
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
        SpringDescription {
            mass,
            stiffness,
            damping,
        }
    }

    /// A spring described by how bouncy it is instead of by its damping.
    ///
    /// `ratio` is the damping ratio: 1 is critical damping, the fastest
    /// arrival with no overshoot; less overshoots, more is sluggish.
    pub fn with_damping_ratio(mass: f32, stiffness: f32, ratio: f32) -> SpringDescription {
        SpringDescription {
            mass,
            stiffness,
            damping: ratio * 2.0 * (mass * stiffness).sqrt(),
        }
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
    pub fn new(spring: SpringDescription, start: f32, end: f32, velocity: f32) -> SpringSimulation {
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
        let discriminant = spring.damping * spring.damping - 4.0 * spring.mass * spring.stiffness;
        if discriminant > 0.0 {
            let root = discriminant.sqrt();
            let r1 = (-spring.damping - root) / (2.0 * spring.mass);
            let r2 = (-spring.damping + root) / (2.0 * spring.mass);
            let c2 = (velocity - r1 * distance) / (r2 - r1);
            SpringSolution::Over {
                r1,
                r2,
                c1: distance - c2,
                c2,
            }
        } else if discriminant < 0.0 {
            let w = (4.0 * spring.mass * spring.stiffness - spring.damping * spring.damping).sqrt()
                / (2.0 * spring.mass);
            let r = -(spring.damping / 2.0 / spring.mass);
            SpringSolution::Under {
                w,
                r,
                c1: distance,
                c2: (velocity - r * distance) / w,
            }
        } else {
            let r = -spring.damping / (2.0 * spring.mass);
            SpringSolution::Critical {
                r,
                c1: distance,
                c2: velocity - r * distance,
            }
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
                power * (c2 * w * cosine - c1 * w * sine) + r * power * (c2 * sine + c1 * cosine)
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

/// Numerically determines the input which produces `target` from `f`, given
/// `f`'s first derivative.
///
/// Upstream's `_newtonsMethod` (`friction_simulation.dart`), used to find when
/// a friction particle is meant to have stopped.
fn newtons_method(
    initial_guess: f32,
    target: f32,
    f: impl Fn(f32) -> f32,
    df: impl Fn(f32) -> f32,
    iterations: usize,
) -> f32 {
    let mut guess = initial_guess;
    for _ in 0..iterations {
        guess -= (f(guess) - target) / df(guess);
    }
    guess
}

/// Something sliding to a stop, with velocity decaying exponentially.
///
/// Upstream's `FrictionSimulation`, which is what `BouncingScrollPhysics`
/// flings with -- iOS-style, where the deceleration is proportional to the
/// speed rather than shaped by a spline. `drag` is the fraction of the speed
/// that survives each second: 0.135 is upstream's iOS scrolling value.
///
/// Pure exponential decay never actually stops, so upstream decides when it
/// counts as stopped: a final time found by Newton's method, past which `x`
/// has already arrived at [`Self::final_x`] and `dx` is zero. Without that
/// freeze the simulation decays for ever, which upstream's own comment on
/// `_finalTime` calls out as wrong even for a drag with no constant
/// deceleration.
#[derive(Clone, Copy, Debug)]
pub struct FrictionSimulation {
    drag: f32,
    drag_log: f32,
    position: f32,
    velocity: f32,
    /// The time at which the simulation stops. Upstream's `_finalTime`, found
    /// in the constructor with ten Newton iterations on `dx`. Each step of the
    /// iteration moves the guess by `-dx/dx' = -1/ln(drag)`, so with zero
    /// constant deceleration the answer is always `10 / |ln(drag)|` seconds
    /// -- about 4.99s for the iOS drag of 0.135.
    final_time: f32,
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
        let drag_log = drag.ln();
        // Upstream's `_finalTime`: Newton's method on `dx`, whose derivative
        // is `v * drag^t * ln(drag)` -- upstream subtracts a constant
        // deceleration here too, but this port does not have one (the
        // parameter upstream added it for is desktop scrolling, and its other
        // caller, `BouncingScrollSimulation`, passes zero). Ten iterations
        // from zero.
        let final_time = newtons_method(
            0.0,
            0.0,
            |time| velocity * drag.powf(time),
            |time| velocity * drag.powf(time) * drag_log,
            10,
        );
        FrictionSimulation {
            drag,
            drag_log,
            position,
            velocity,
            final_time,
            tolerance,
        }
    }

    /// Where it comes to rest.
    pub fn final_x(&self) -> f32 {
        self.position - self.velocity / self.drag_log
    }

    /// When it passes `x`, or infinity if it never does. Upstream's `timeAtX`.
    ///
    /// This is what lets [`crate::scroll_simulation::BouncingScrollSimulation`]
    /// join a friction run to a spring **at the exact instant the content
    /// crosses the edge** rather than at the next frame boundary, which is why
    /// an iOS bounce has no visible seam in it.
    ///
    /// The two refusals are both real: a simulation with no velocity never gets
    /// anywhere, and one asked about a point behind it or past where it stops
    /// never gets there either.
    pub fn time_at_x(&self, x: f32) -> f32 {
        if x == self.position {
            return 0.0;
        }
        let unreachable = if self.velocity > 0.0 {
            x < self.position || x > self.final_x()
        } else {
            x > self.position || x < self.final_x()
        };
        if self.velocity == 0.0 || unreachable {
            return f32::INFINITY;
        }
        (self.drag_log * (x - self.position) / self.velocity + 1.0).ln() / self.drag_log
    }

    /// A friction simulation whose drag is chosen so that it passes through
    /// `end_position` -- upstream's `FrictionSimulation.through`.
    ///
    /// Ordinary friction is given a drag and asked where it stops. This is
    /// given where it must stop and asked for the drag, which is the same
    /// equation solved the other way round. What it buys is a fling that both
    /// feels like a fling and lands exactly somewhere -- on a whole item of a
    /// wheel, in its one caller.
    ///
    /// Upstream's algebra, kept: with `v = v0 * D^t`, the time to fall from
    /// `v0` to `v1` is `(ln(v1) - ln(v0)) / ln(D)`, and solving `x(that time)
    /// = x1` for `D` gives `e^((v0 - v1) / (x0 - x1))`. Note the denominator
    /// is start *minus* end, which with the sign rule below makes the exponent
    /// negative and so the drag less than one.
    ///
    /// The three conditions upstream asserts are conditions on the caller, not
    /// choices: the two velocities must not fight each other, the simulation
    /// cannot end faster than it began, and it has to be travelling towards
    /// the end it is meant to reach.
    pub fn through(
        start_position: f32,
        end_position: f32,
        start_velocity: f32,
        end_velocity: f32,
    ) -> FrictionSimulation {
        debug_assert!(
            start_velocity == 0.0
                || end_velocity == 0.0
                || start_velocity.signum() == end_velocity.signum()
        );
        debug_assert!(start_velocity.abs() >= end_velocity.abs());
        debug_assert!((end_position - start_position).signum() == start_velocity.signum());
        FrictionSimulation::with_tolerance(
            FrictionSimulation::drag_for(
                start_position,
                end_position,
                start_velocity,
                end_velocity,
            ),
            start_position,
            start_velocity,
            Tolerance {
                velocity: end_velocity.abs(),
                ..Tolerance::DEFAULT
            },
        )
    }

    /// Upstream's `_dragFor`.
    fn drag_for(
        start_position: f32,
        end_position: f32,
        start_velocity: f32,
        end_velocity: f32,
    ) -> f32 {
        std::f32::consts::E.powf((start_velocity - end_velocity) / (start_position - end_position))
    }
}

impl FrictionSimulation {
    /// The tolerance this was built with, which
    /// [`BoundedFrictionSimulation`] needs to decide how close to a wall counts
    /// as arriving. Upstream reads the inherited `tolerance` field directly;
    /// there is no inheritance here, so it is asked for.
    pub fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

impl Simulation for FrictionSimulation {
    fn x(&self, time: f32) -> f32 {
        if time > self.final_time {
            return self.final_x();
        }
        self.position + self.velocity * self.drag.powf(time) / self.drag_log
            - self.velocity / self.drag_log
    }

    fn dx(&self, time: f32) -> f32 {
        if time > self.final_time {
            return 0.0;
        }
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
    pub fn new(acceleration: f32, position: f32, end: f32, velocity: f32) -> GravitySimulation {
        GravitySimulation {
            acceleration,
            position,
            velocity,
            end: end.abs(),
        }
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

// -- Clamping another simulation ----------------------------------------------

/// Upstream `ClampedSimulation`: another simulation with limits on what it is
/// allowed to report.
///
/// # The limits are on the *outputs*, not on the motion
///
/// This is the whole of what upstream's doc-comment spends a paragraph on, and
/// it is worth having in front of you because the class does not behave the way
/// its name suggests. A gravity simulation thrown upward past a maximum
/// position does not stop at the maximum and fall back early: it flies its full
/// arc and lands at exactly the moment it always would have, and all that
/// changes is that the *reported* position reads as pinned while the particle
/// is above the line.
///
/// Two consequences follow, and upstream names both:
///
/// * **`x` will change at a rate that does not match `dx`** while either is
///   being clamped. They are clamped independently, and a caller integrating
///   `dx` to check `x` will disagree with it.
/// * **`is_done` is not clamped at all.** It is forwarded, so a clamped
///   simulation finishes when the thing underneath finishes, not when the
///   reported position stops moving.
///
/// Neither is a wart to be fixed. A caller that wanted the motion itself
/// bounded wants [`BoundedFrictionSimulation`], which stops early on purpose.
pub struct ClampedSimulation<S: Simulation> {
    simulation: S,
    x_min: f32,
    x_max: f32,
    dx_min: f32,
    dx_max: f32,
}

impl<S: Simulation> ClampedSimulation<S> {
    /// Unbounded on every side. Upstream's constructor defaults, which are the
    /// two infinities on both pairs -- so a `ClampedSimulation` with nothing
    /// set is a pass-through, and a caller narrows the sides it cares about.
    pub fn new(simulation: S) -> ClampedSimulation<S> {
        ClampedSimulation {
            simulation,
            x_min: f32::NEG_INFINITY,
            x_max: f32::INFINITY,
            dx_min: f32::NEG_INFINITY,
            dx_max: f32::INFINITY,
        }
    }

    /// Upstream asserts `xMax >= xMin`. An inverted range has no value that
    /// satisfies it, so every answer would be wrong and a clamp would silently
    /// pick one of the two ends -- which is why upstream asserts rather than
    /// swapping them.
    pub fn with_x_range(mut self, min: f32, max: f32) -> Self {
        debug_assert!(max >= min, "x_max must not be below x_min");
        self.x_min = min;
        self.x_max = max;
        self
    }

    /// Upstream asserts `dxMax >= dxMin`, for the same reason.
    pub fn with_dx_range(mut self, min: f32, max: f32) -> Self {
        debug_assert!(max >= min, "dx_max must not be below dx_min");
        self.dx_min = min;
        self.dx_max = max;
        self
    }

    /// The simulation underneath, for a caller who wants the unclamped answer.
    pub fn inner(&self) -> &S {
        &self.simulation
    }
}

impl<S: Simulation> Simulation for ClampedSimulation<S> {
    fn x(&self, time: f32) -> f32 {
        self.simulation.x(time).clamp(self.x_min, self.x_max)
    }

    fn dx(&self, time: f32) -> f32 {
        self.simulation.dx(time).clamp(self.dx_min, self.dx_max)
    }

    /// Forwarded, deliberately. See the type's docs.
    fn is_done(&self, time: f32) -> bool {
        self.simulation.is_done(time)
    }
}

// -- Friction that stops at a wall --------------------------------------------

/// Upstream `BoundedFrictionSimulation`: a [`FrictionSimulation`] that stops
/// when it reaches either end of a range.
///
/// Unlike [`ClampedSimulation`], this one really does end early: reaching a
/// bound is a way of being finished, not merely a value the reports are pinned
/// to. That is the difference between the two, and it is why upstream has both.
///
/// The bound is checked against the tolerance rather than for equality, because
/// a particle decelerating towards a wall approaches it without arriving, and a
/// simulation that waited for exact equality would run until its own friction
/// time-out instead.
pub struct BoundedFrictionSimulation {
    friction: FrictionSimulation,
    min_x: f32,
    max_x: f32,
}

impl BoundedFrictionSimulation {
    /// Upstream asserts the initial position is already inside the range --
    /// `clampDouble(position, _minX, _maxX) == position`. A particle starting
    /// outside would be finished on its first frame at a position it never
    /// occupied, which is a caller's bug and not something to round away.
    pub fn new(
        drag: f32,
        position: f32,
        velocity: f32,
        min_x: f32,
        max_x: f32,
    ) -> BoundedFrictionSimulation {
        BoundedFrictionSimulation::with_tolerance(
            drag,
            position,
            velocity,
            min_x,
            max_x,
            Tolerance::DEFAULT,
        )
    }

    pub fn with_tolerance(
        drag: f32,
        position: f32,
        velocity: f32,
        min_x: f32,
        max_x: f32,
        tolerance: Tolerance,
    ) -> BoundedFrictionSimulation {
        debug_assert!(
            position.clamp(min_x, max_x) == position,
            "a bounded friction simulation starts inside its bounds"
        );
        BoundedFrictionSimulation {
            friction: FrictionSimulation::with_tolerance(drag, position, velocity, tolerance),
            min_x,
            max_x,
        }
    }
}

impl Simulation for BoundedFrictionSimulation {
    fn x(&self, time: f32) -> f32 {
        self.friction.x(time).clamp(self.min_x, self.max_x)
    }

    /// Not clamped, and upstream does not clamp it either: the particle's speed
    /// is the friction simulation's speed right up until it is done.
    fn dx(&self, time: f32) -> f32 {
        self.friction.dx(time)
    }

    /// Finished when the friction is finished **or** when either wall is within
    /// tolerance. The bounds are checked against the already-clamped `x`, which
    /// is upstream's `x(time)` and not `super.x(time)` -- so a particle that
    /// overshot far past a wall is still reported as done at the wall rather
    /// than at wherever the unclamped curve went.
    fn is_done(&self, time: f32) -> bool {
        let at = self.x(time);
        self.friction.is_done(time)
            || (at - self.min_x).abs() < self.friction.tolerance().distance
            || (at - self.max_x).abs() < self.friction.tolerance().distance
    }
}

// -- A spring that lands exactly ----------------------------------------------

/// Upstream `ScrollSpringSimulation`: a [`SpringSimulation`] whose `x` is
/// **exactly** the end value once it is done.
///
/// One line upstream, and the line earns its class. A spring approaches its
/// rest position without arriving, so `SpringSimulation::x` at the moment
/// `is_done` first answers true is within the tolerance of the end and not at
/// it -- a thousandth of a pixel out, by default. That is invisible on screen
/// and is not invisible to a scroll position, which stores what it was handed
/// and reports it as the resting offset for as long as nobody scrolls again.
/// The list would sit a thousandth of a pixel from the top for ever, and
/// anything comparing the offset against zero would disagree with the reader.
pub struct ScrollSpringSimulation {
    spring: SpringSimulation,
    end: f32,
}

impl ScrollSpringSimulation {
    pub fn new(
        spring: SpringDescription,
        start: f32,
        end: f32,
        velocity: f32,
    ) -> ScrollSpringSimulation {
        ScrollSpringSimulation::with_tolerance(spring, start, end, velocity, Tolerance::DEFAULT)
    }

    pub fn with_tolerance(
        spring: SpringDescription,
        start: f32,
        end: f32,
        velocity: f32,
        tolerance: Tolerance,
    ) -> ScrollSpringSimulation {
        ScrollSpringSimulation {
            spring: SpringSimulation::with_tolerance(spring, start, end, velocity, tolerance),
            end,
        }
    }
}

impl Simulation for ScrollSpringSimulation {
    fn x(&self, time: f32) -> f32 {
        if self.is_done(time) {
            self.end
        } else {
            self.spring.x(time)
        }
    }

    /// Not snapped. Upstream overrides `x` only -- the velocity at the end of a
    /// spring is already within the velocity tolerance of zero, and rounding it
    /// would hide a spring that was still moving when something declared it
    /// done.
    fn dx(&self, time: f32) -> f32 {
        self.spring.dx(time)
    }

    fn is_done(&self, time: f32) -> bool {
        self.spring.is_done(time)
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
            assert!(
                x >= previous,
                "went backwards at {step}: {x} after {previous}"
            );
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
        assert!(
            simulation.is_done(2.0),
            "should have settled by two seconds"
        );
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
        assert!(
            peak > 100.0,
            "a bouncy spring should pass its target, not stop at {peak}"
        );
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
        assert!(
            !simulation.is_done(crossing),
            "moving fast, exactly at the end"
        );
    }

    // -- Friction -------------------------------------------------------------

    #[test]
    fn friction_slows_down_and_stops_somewhere() {
        // Upstream's iOS scrolling drag.
        let simulation = FrictionSimulation::new(0.135, 0.0, 600.0);
        assert!((simulation.dx(0.0) - 600.0).abs() < 1e-3);
        assert!(simulation.dx(1.0) < simulation.dx(0.5));
        // Upstream does not let the exponential decay run for ever: ten Newton
        // iterations put a final time at 10/|ln(drag)| -- about 4.99s here --
        // past which dx() is zero and x() has arrived. A frame before it the
        // particle is still moving; at five seconds it is done, stopped
        // exactly where it was always going to be.
        assert!(!simulation.is_done(4.9));
        assert!(simulation.is_done(5.0));
        assert_eq!(simulation.dx(5.0), 0.0);
        let settled = simulation.x(5.0);
        assert!(
            (settled - simulation.final_x()).abs() < 1e-3,
            "{settled} should have arrived at {}",
            simulation.final_x()
        );
        // And it stays there, rather than decaying on towards the asymptote.
        assert_eq!(simulation.x(60.0), settled);
    }

    #[test]
    fn frictions_final_time_is_where_newton_leaves_it() {
        // Each Newton step moves the guess by 1/|ln(drag)| and there are ten
        // of them from zero, so with no constant deceleration the final time
        // is 10/|ln(drag)| -- same number of iterations as upstream, so same
        // answer, rounding aside.
        for drag in [0.135, 0.05, 0.995] {
            let simulation = FrictionSimulation::new(drag, 0.0, 600.0);
            let expected = 10.0 / drag.ln().abs();
            // A relative tolerance: f32 has barely four decimal places to
            // give at the two-thousandth second the gentle drag stops at.
            assert!(
                (simulation.final_time - expected).abs() < expected * 1e-5,
                "drag {drag}: {} should be {expected}",
                simulation.final_time
            );
        }
        // Which for the iOS drag is about five seconds, not the six and a
        // half that waiting for the decay to reach the tolerance used to take.
        assert!((FrictionSimulation::new(0.135, 0.0, 600.0).final_time - 4.99).abs() < 0.01);
    }

    #[test]
    fn friction_the_other_way_travels_the_other_way() {
        let simulation = FrictionSimulation::new(0.135, 100.0, -600.0);
        assert!(simulation.final_x() < 100.0);
        assert!(
            simulation.is_done(5.0),
            "it stops at the same time in either direction"
        );
    }

    // -- Gravity --------------------------------------------------------------

    #[test]
    fn gravity_accelerates_and_finishes_at_the_edge() {
        let simulation = GravitySimulation::new(1000.0, 0.0, 500.0, 0.0);
        assert_eq!(simulation.x(0.0), 0.0);
        assert!(
            (simulation.x(1.0) - 500.0).abs() < 1e-3,
            "half of a t squared"
        );
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

    // -- ClampedSimulation --------------------------------------------------------

    #[test]
    fn a_clamped_simulation_pins_what_it_reports_and_not_what_it_does() {
        // Upstream's own worked example: a particle thrown up past a maximum
        // returns to where it started **at the same moment it would have
        // anyway**. The clamp is on the reports.
        let up = GravitySimulation::new(-200.0, 0.0, f32::INFINITY, 100.0);
        let clamped =
            ClampedSimulation::new(GravitySimulation::new(-200.0, 0.0, f32::INFINITY, 100.0))
                .with_x_range(f32::NEG_INFINITY, 5.0);

        // At the apogee the free particle is well above the ceiling.
        let apogee = 0.5;
        assert!(up.x(apogee) > 5.0, "{}", up.x(apogee));
        assert_eq!(clamped.x(apogee), 5.0, "reported as pinned");

        // And back below it, the two agree again -- the arc was never altered.
        let late = 0.95;
        assert!(up.x(late) < 5.0);
        assert_eq!(clamped.x(late), up.x(late));
    }

    #[test]
    fn a_clamped_x_and_dx_are_allowed_to_disagree() {
        // Upstream says so in as many words: "the x value will change at a rate
        // that does not match the reported dx value while one or the other is
        // being clamped". A caller integrating dx to check x will be wrong, and
        // that is the design.
        let clamped =
            ClampedSimulation::new(GravitySimulation::new(-200.0, 0.0, f32::INFINITY, 100.0))
                .with_x_range(f32::NEG_INFINITY, 5.0);

        // Position is pinned across this interval; velocity is not.
        assert_eq!(clamped.x(0.4), 5.0);
        assert_eq!(clamped.x(0.5), 5.0);
        assert_ne!(
            clamped.dx(0.4),
            clamped.dx(0.5),
            "the particle is still slowing while its position reads as still"
        );
    }

    #[test]
    fn clamping_the_velocity_is_a_separate_pair_of_limits() {
        let clamped =
            ClampedSimulation::new(GravitySimulation::new(-200.0, 0.0, f32::INFINITY, 100.0))
                .with_dx_range(-10.0, 10.0);
        assert_eq!(clamped.dx(0.0), 10.0, "100 up, reported as 10");
        assert_eq!(clamped.dx(2.0), -10.0, "and falling fast, reported as -10");
        assert_eq!(
            clamped.x(0.5),
            clamped.inner().x(0.5),
            "the position was not asked to change"
        );
    }

    #[test]
    fn a_clamped_simulation_finishes_when_the_one_underneath_does() {
        // `isDone` is forwarded, unclamped. A caller who wanted the motion
        // itself bounded wants BoundedFrictionSimulation.
        //
        // The `end` is far away on purpose: with a near one the particle is
        // already done at every sample and the assertion compares true to true.
        // The first version of this test did that and stayed green when
        // `is_done` was made to consult the clamp.
        let inner = GravitySimulation::new(-200.0, 0.0, 1000.0, 100.0);
        let clamped = ClampedSimulation::new(GravitySimulation::new(-200.0, 0.0, 1000.0, 100.0))
            .with_x_range(f32::NEG_INFINITY, 5.0);
        for t in [0.1f32, 0.5, 1.0, 2.0] {
            assert!(!inner.is_done(t), "the particle is still flying at {t}");
            assert!(
                clamped.x(t) >= 5.0 || t > 0.9,
                "and pinned at the ceiling for most of it"
            );
            assert_eq!(clamped.is_done(t), inner.is_done(t), "at {t}");
        }
    }

    #[test]
    fn an_unclamped_clamped_simulation_is_a_pass_through() {
        // The constructor's defaults are the two infinities, so narrowing is
        // opt-in per side.
        let inner = FrictionSimulation::new(0.135, 0.0, 500.0);
        let same = ClampedSimulation::new(FrictionSimulation::new(0.135, 0.0, 500.0));
        for t in [0.0f32, 0.25, 1.0, 4.0] {
            assert_eq!(same.x(t), inner.x(t));
            assert_eq!(same.dx(t), inner.dx(t));
        }
    }

    // -- BoundedFrictionSimulation ------------------------------------------------

    #[test]
    fn a_bounded_friction_simulation_really_stops_at_the_wall() {
        // The difference from ClampedSimulation, and the reason upstream has
        // both: reaching a bound is a way of being *finished*, not merely a
        // value the reports are pinned to.
        let free = FrictionSimulation::new(0.135, 0.0, 500.0);
        let bounded = BoundedFrictionSimulation::new(0.135, 0.0, 500.0, -1000.0, 50.0);

        // Find the first moment the free particle is past the wall.
        let mut t = 0.0f32;
        while free.x(t) < 50.0 && t < 5.0 {
            t += 0.001;
        }
        assert!(t < 5.0, "the fling does reach the wall");
        assert!(!free.is_done(t), "the free one is still going");
        assert!(bounded.is_done(t), "the bounded one has arrived");
        assert_eq!(bounded.x(t), 50.0);
    }

    #[test]
    fn a_bounded_friction_simulation_that_never_reaches_a_wall_is_ordinary_friction() {
        let free = FrictionSimulation::new(0.135, 0.0, 100.0);
        let bounded = BoundedFrictionSimulation::new(0.135, 0.0, 100.0, -10_000.0, 10_000.0);
        for t in [0.0f32, 0.1, 0.5, 2.0] {
            assert_eq!(bounded.x(t), free.x(t), "at {t}");
            assert_eq!(bounded.dx(t), free.dx(t), "at {t}");
            assert_eq!(bounded.is_done(t), free.is_done(t), "at {t}");
        }
    }

    #[test]
    fn arriving_is_measured_against_the_tolerance_and_not_equality() {
        // A particle decelerating towards a wall approaches without arriving;
        // waiting for exact equality would run to the friction time-out
        // instead.
        let bounded = BoundedFrictionSimulation::with_tolerance(
            0.135,
            0.0,
            500.0,
            -1000.0,
            1000.0,
            Tolerance {
                distance: 5.0,
                ..Tolerance::DEFAULT
            },
        );
        // The fling settles a long way short of 1000, so what makes this done
        // is the *other* wall never being reached and friction running out.
        assert!(bounded.is_done(5.0));

        // With the wall placed within the (generous) tolerance of the start,
        // it is done immediately -- which is what a 5-pixel tolerance means.
        let at_the_wall = BoundedFrictionSimulation::with_tolerance(
            0.135,
            0.0,
            500.0,
            -3.0,
            1000.0,
            Tolerance {
                distance: 5.0,
                ..Tolerance::DEFAULT
            },
        );
        assert!(at_the_wall.is_done(0.0), "the wall is within tolerance");
    }

    #[test]
    fn the_velocity_of_a_bounded_simulation_is_not_clamped() {
        // Upstream overrides `x` and `isDone` and leaves `dx` alone: the
        // particle's speed is the friction simulation's speed right up until it
        // is finished.
        let free = FrictionSimulation::new(0.135, 0.0, 500.0);
        let bounded = BoundedFrictionSimulation::new(0.135, 0.0, 500.0, -1000.0, 1.0);
        assert_eq!(bounded.dx(0.0), free.dx(0.0));
        assert_eq!(bounded.dx(0.5), free.dx(0.5));
    }

    // -- ScrollSpringSimulation ---------------------------------------------------

    #[test]
    fn a_scroll_spring_lands_exactly_on_its_end_value() {
        // The whole of what the class adds. A plain spring is within the
        // tolerance of the end and not at it, and a scroll position stores what
        // it was handed: the list would rest a thousandth of a pixel from the
        // top for ever.
        let spring = SpringDescription::new(1.0, 100.0, 20.0);
        let plain = SpringSimulation::new(spring, 0.0, 300.0, 0.0);
        let scroll = ScrollSpringSimulation::new(spring, 0.0, 300.0, 0.0);

        // The first moment it is done.
        let mut t = 0.0f32;
        while !scroll.is_done(t) && t < 10.0 {
            t += 0.001;
        }
        assert!(t < 10.0, "it does settle");

        assert_eq!(scroll.x(t), 300.0, "exactly");
        assert_ne!(plain.x(t), 300.0, "and the plain one is not");
        assert!(
            (plain.x(t) - 300.0).abs() < Tolerance::DEFAULT.distance,
            "though it is within tolerance, which is why this is easy to miss"
        );
    }

    #[test]
    fn a_scroll_spring_is_an_ordinary_spring_until_it_is_done() {
        let spring = SpringDescription::new(1.0, 100.0, 20.0);
        let plain = SpringSimulation::new(spring, 0.0, 300.0, 0.0);
        let scroll = ScrollSpringSimulation::new(spring, 0.0, 300.0, 0.0);
        for t in [0.0f32, 0.05, 0.1, 0.2] {
            assert!(!scroll.is_done(t), "still moving at {t}");
            assert_eq!(scroll.x(t), plain.x(t), "at {t}");
            assert_eq!(scroll.dx(t), plain.dx(t), "at {t}");
        }
    }

    #[test]
    fn a_scroll_springs_velocity_is_not_snapped() {
        // Upstream overrides `x` only. Rounding `dx` would hide a spring that
        // was still moving when something declared it done.
        let spring = SpringDescription::new(1.0, 100.0, 20.0);
        let plain = SpringSimulation::new(spring, 0.0, 300.0, 0.0);
        let scroll = ScrollSpringSimulation::new(spring, 0.0, 300.0, 0.0);
        let mut t = 0.0f32;
        while !scroll.is_done(t) && t < 10.0 {
            t += 0.001;
        }
        assert_eq!(scroll.dx(t), plain.dx(t), "reported as it is");
    }
}
