// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Animation: tickers, controllers, curves and tweens.
//!
//! An animation is a value that changes because time passes. Everything here
//! exists to make that one sentence work without any part of it polling: a
//! [`Ticker`] is advanced by the frame that is already happening, a
//! [`Controller`] turns elapsed time into a number between 0 and 1, a
//! [`Curve`] bends that number, and a [`Tween`] maps it onto whatever the
//! caller actually wanted.
//!
//! # Where the frames come from
//!
//! Frames are on demand: the engine goes idle after the last one unless
//! something asks for another. So an animation that is running has to keep
//! asking, and one that is finished has to stop -- otherwise a static page
//! burns a core forever. [`Controller::tick`] returns whether it is still
//! going, and [`Animations::tick`] does the asking for a whole set at once.
//!
//! Upstream the same job is split between `Ticker`, `AnimationController`,
//! `Curve` and `Tween`, with a `TickerProvider` mixed into the state. The
//! shapes here are the same; what is missing is the global ticker muting that
//! upstream uses to freeze animations in tests.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

// -- Curves -------------------------------------------------------------------

/// Bends the flow of time. Every curve maps 0..1 to 0..1, passing through both
/// endpoints -- so an animation always starts where it started and ends where
/// it ends, however strangely it gets there.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Curve {
    #[default]
    Linear,
    /// Decelerating, matching upstream's `Curves.decelerate`: `1 - (1 - t)^2`.
    Decelerate,
    /// `t^2`: starts slowly and ends fast.
    ///
    /// **Upstream has no name for this one.** It is what
    /// `FlippedCurve(Curves.decelerate)` computes, and it is here because
    /// [`Curve::flipped`] answers a closed form per variant rather than
    /// wrapping -- without somewhere to send it, decelerate had no flip and
    /// quietly answered itself.
    Accelerate,
    /// A cubic Bezier easing, given its two control points.
    ///
    /// This is what almost every named curve upstream is -- `Curves.easeIn` is
    /// `Cubic(0.42, 0.0, 1.0, 1.0)` and nothing more -- so the named ones
    /// below are constants rather than variants.
    Cubic(f32, f32, f32, f32),
    /// Overshoots, comes back, and does it again, smaller each time. The
    /// number is the period of the oscillation.
    ElasticIn(f32),
    ElasticOut(f32),
    ElasticInOut(f32),
    /// Two cubics joined at a midpoint. Upstream's `ThreePointCubic`, which
    /// is a separate class rather than a curve you could approximate with one
    /// cubic: iOS's page transition is one, and its whole character is the
    /// join.
    ThreePointCubic(ThreePointCubic),
    /// Bounces, like something dropped.
    BounceIn,
    BounceOut,
    BounceInOut,
}

/// Upstream's `Curves`, as constants.
///
/// The coefficients are copied rather than derived: they are a design
/// decision, and a curve that is nearly `easeInOut` reads as a mistake next to
/// one that is.
impl Curve {
    /// Material's usual choice for a change in place.
    pub const EASE: Curve = Curve::Cubic(0.25, 0.1, 0.25, 1.0);
    /// Slow in. What something entering the screen should use.
    pub const EASE_IN: Curve = Curve::Cubic(0.42, 0.0, 1.0, 1.0);
    /// Slow out. What something leaving should use.
    pub const EASE_OUT: Curve = Curve::Cubic(0.0, 0.0, 0.58, 1.0);
    /// Slow at both ends. The safe default.
    pub const EASE_IN_OUT: Curve = Curve::Cubic(0.42, 0.0, 0.58, 1.0);

    /// Material's standard easing: quick to leave, slow to arrive.
    pub const FAST_OUT_SLOW_IN: Curve = Curve::Cubic(0.4, 0.0, 0.2, 1.0);
    pub const SLOW_MIDDLE: Curve = Curve::Cubic(0.15, 0.85, 0.85, 0.15);

    /// Starts at the linear rate and eases out. iOS's choice for a page that
    /// is being covered.
    pub const LINEAR_TO_EASE_OUT: Curve = Curve::Cubic(0.35, 0.91, 0.33, 0.97);
    /// [`Curve::LINEAR_TO_EASE_OUT`] run the other way, and **not** its
    /// `flipped`: upstream spells both out, because the pair is a design
    /// decision and a reversal that merely looked right would read as one.
    pub const EASE_IN_TO_LINEAR: Curve = Curve::Cubic(0.67, 0.03, 0.65, 0.09);

    /// The iOS page transition, and the reason [`ThreePointCubic`] exists: a
    /// quick ease in, then a long slow ease out, joined at a point neither
    /// cubic alone can put there.
    pub const FAST_EASE_IN_TO_SLOW_EASE_OUT: Curve = Curve::ThreePointCubic(ThreePointCubic::new(
        (0.056, 0.024),
        (0.108, 0.3085),
        (0.198, 0.541),
        (0.3655, 1.0),
        (0.5465, 0.989),
    ));

    pub const EASE_IN_SINE: Curve = Curve::Cubic(0.47, 0.0, 0.745, 0.715);
    pub const EASE_IN_QUAD: Curve = Curve::Cubic(0.55, 0.085, 0.68, 0.53);
    pub const EASE_IN_CUBIC: Curve = Curve::Cubic(0.55, 0.055, 0.675, 0.19);
    pub const EASE_IN_QUART: Curve = Curve::Cubic(0.895, 0.03, 0.685, 0.22);
    pub const EASE_IN_QUINT: Curve = Curve::Cubic(0.755, 0.05, 0.855, 0.06);
    pub const EASE_IN_EXPO: Curve = Curve::Cubic(0.95, 0.05, 0.795, 0.035);
    pub const EASE_IN_CIRC: Curve = Curve::Cubic(0.6, 0.04, 0.98, 0.335);
    /// Backs up before setting off.
    pub const EASE_IN_BACK: Curve = Curve::Cubic(0.6, -0.28, 0.735, 0.045);

    pub const EASE_OUT_SINE: Curve = Curve::Cubic(0.39, 0.575, 0.565, 1.0);
    pub const EASE_OUT_QUAD: Curve = Curve::Cubic(0.25, 0.46, 0.45, 0.94);
    pub const EASE_OUT_CUBIC: Curve = Curve::Cubic(0.215, 0.61, 0.355, 1.0);
    pub const EASE_OUT_QUART: Curve = Curve::Cubic(0.165, 0.84, 0.44, 1.0);
    pub const EASE_OUT_QUINT: Curve = Curve::Cubic(0.23, 1.0, 0.32, 1.0);
    pub const EASE_OUT_EXPO: Curve = Curve::Cubic(0.19, 1.0, 0.22, 1.0);
    pub const EASE_OUT_CIRC: Curve = Curve::Cubic(0.075, 0.82, 0.165, 1.0);
    /// Overshoots and comes back. For an arrival that should feel physical.
    pub const EASE_OUT_BACK: Curve = Curve::Cubic(0.175, 0.885, 0.32, 1.275);

    pub const EASE_IN_OUT_SINE: Curve = Curve::Cubic(0.445, 0.05, 0.55, 0.95);
    pub const EASE_IN_OUT_QUAD: Curve = Curve::Cubic(0.455, 0.03, 0.515, 0.955);
    pub const EASE_IN_OUT_CUBIC: Curve = Curve::Cubic(0.645, 0.045, 0.355, 1.0);
    /// Upstream's `Curves.easeInOutCubicEmphasized`: Material 3's emphasized
    /// easing, and the curve a navigation destination's label fades and slides
    /// on (see
    /// [`crate::navigation_destinations::destination_label_animation`]).
    ///
    /// A `ThreePointCubic` rather than a cubic, and the shape says why. It
    /// barely moves for the first tenth of its time, then covers a quarter to
    /// two thirds of the distance between t=0.15 and t=0.20, and is 95% of the
    /// way there by the half-way point -- so **the entire second half is the
    /// last five per cent arriving**. The join sits at (0.166, 0.4), in the
    /// middle of that burst: 40% of the distance in 17% of the time. A single
    /// cubic through (0,0) and (1,1) cannot both hesitate that long and then
    /// accelerate that hard.
    pub const EASE_IN_OUT_CUBIC_EMPHASIZED: Curve = Curve::ThreePointCubic(ThreePointCubic::new(
        (0.05, 0.0),
        (0.133333, 0.06),
        (0.166666, 0.4),
        (0.208333, 0.82),
        (0.25, 1.0),
    ));
    pub const EASE_IN_OUT_QUART: Curve = Curve::Cubic(0.77, 0.0, 0.175, 1.0);
    pub const EASE_IN_OUT_QUINT: Curve = Curve::Cubic(0.86, 0.0, 0.07, 1.0);
    pub const EASE_IN_OUT_EXPO: Curve = Curve::Cubic(1.0, 0.0, 0.0, 1.0);
    pub const EASE_IN_OUT_CIRC: Curve = Curve::Cubic(0.785, 0.135, 0.15, 0.86);
    pub const EASE_IN_OUT_BACK: Curve = Curve::Cubic(0.68, -0.55, 0.265, 1.55);

    /// The elastic curves at upstream's default period.
    pub const ELASTIC_IN: Curve = Curve::ElasticIn(0.4);
    pub const ELASTIC_OUT: Curve = Curve::ElasticOut(0.4);
    pub const ELASTIC_IN_OUT: Curve = Curve::ElasticInOut(0.4);

    // The names the rest of this crate was written against, from when the
    // curves were a handful of hand-fitted polynomials. They are the cubics
    // above now, which is what upstream always meant by them.
    #[allow(non_upper_case_globals)]
    pub const EaseIn: Curve = Curve::EASE_IN;
    #[allow(non_upper_case_globals)]
    pub const EaseOut: Curve = Curve::EASE_OUT;
    #[allow(non_upper_case_globals)]
    pub const EaseInOut: Curve = Curve::EASE_IN_OUT;
    #[allow(non_upper_case_globals)]
    pub const EaseOutBack: Curve = Curve::EASE_OUT_BACK;

    /// Transforms `t`, which is clamped to 0..1 first.
    pub fn transform(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        // The endpoints are exact, whatever the shape says. Upstream's
        // `Curve.transform` does the same and asserts that the shape agrees to
        // within rounding: an elastic curve is a decaying sine, and at t=0 it
        // evaluates to a thousandth rather than to nothing, which would be an
        // animation that starts by jumping.
        if t == 0.0 || t == 1.0 {
            return t;
        }
        let tau = std::f32::consts::PI * 2.0;
        match self {
            Curve::Linear => t,
            Curve::Decelerate => {
                let inverted = 1.0 - t;
                1.0 - inverted * inverted
            }
            Curve::Accelerate => t * t,
            Curve::Cubic(a, b, c, d) => cubic(a, b, c, d, t),
            Curve::ThreePointCubic(shape) => shape.transform(t),
            Curve::ElasticIn(period) => {
                let s = period / 4.0;
                let t = t - 1.0;
                -(2f32.powf(10.0 * t)) * ((t - s) * tau / period).sin()
            }
            Curve::ElasticOut(period) => {
                let s = period / 4.0;
                2f32.powf(-10.0 * t) * ((t - s) * tau / period).sin() + 1.0
            }
            Curve::ElasticInOut(period) => {
                let s = period / 4.0;
                let t = 2.0 * t - 1.0;
                if t < 0.0 {
                    -0.5 * 2f32.powf(10.0 * t) * ((t - s) * tau / period).sin()
                } else {
                    2f32.powf(-10.0 * t) * ((t - s) * tau / period).sin() * 0.5 + 1.0
                }
            }
            Curve::BounceIn => 1.0 - bounce(1.0 - t),
            Curve::BounceOut => bounce(t),
            Curve::BounceInOut => {
                if t < 0.5 {
                    (1.0 - bounce(1.0 - t * 2.0)) * 0.5
                } else {
                    bounce(t * 2.0 - 1.0) * 0.5 + 0.5
                }
            }
        }
    }

    /// The curve run backwards: `1 - curve(1 - t)`.
    ///
    /// Upstream's `FlippedCurve`, and what a reversing animation wants -- an
    /// ease-in played in reverse should still start slowly, which means easing
    /// out. For a cubic it is exact rather than approximate: the reverse of a
    /// Bezier easing with control points (a, b) and (c, d) is the one with
    /// (1-c, 1-d) and (1-a, 1-b).
    pub fn flipped(self) -> Curve {
        match self {
            Curve::Cubic(a, b, c, d) => Curve::Cubic(1.0 - c, 1.0 - d, 1.0 - a, 1.0 - b),
            Curve::ThreePointCubic(shape) => Curve::ThreePointCubic(shape.flipped()),
            Curve::Decelerate => Curve::Accelerate,
            Curve::Accelerate => Curve::Decelerate,
            Curve::BounceIn => Curve::BounceOut,
            Curve::BounceOut => Curve::BounceIn,
            Curve::ElasticIn(period) => Curve::ElasticOut(period),
            Curve::ElasticOut(period) => Curve::ElasticIn(period),
            other => other,
        }
    }
}

/// Where a cubic Bezier easing is at `t`.
///
/// The curve is a parametric Bezier through (0,0) and (1,1) with control
/// points (a,b) and (c,d), so the answer is not a formula in `t`: the
/// parameter that puts the curve at time `t` has to be found first. Upstream
/// bisects, and so does this, to the same error bound -- which is loose
/// because a curve is being sampled for a pixel position, not solved.
/// Upstream `ThreePointCubic`: two cubic Beziers joined at a midpoint.
///
/// Each segment is an ordinary cubic through its own two endpoints, and the
/// midpoint is where the first hands over to the second. It is a separate
/// shape rather than a cubic with cleverer control points because a single
/// cubic through (0,0) and (1,1) cannot both leave quickly and arrive slowly
/// to the degree iOS asks for -- the join is the whole point.
///
/// Points are `(x, y)` pairs rather than `Offset`s, which keeps this file
/// free of a dependency on the engine's geometry for the sake of ten numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreePointCubic {
    /// The first control point of the first curve.
    pub a1: (f32, f32),
    /// The second control point of the first curve. The line through this and
    /// the midpoint is tangent to the curve arriving at the midpoint.
    pub b1: (f32, f32),
    /// Where the first curve ends and the second begins.
    pub midpoint: (f32, f32),
    /// The first control point of the second curve.
    pub a2: (f32, f32),
    /// The second control point of the second curve.
    pub b2: (f32, f32),
}

impl ThreePointCubic {
    pub const fn new(
        a1: (f32, f32),
        b1: (f32, f32),
        midpoint: (f32, f32),
        a2: (f32, f32),
        b2: (f32, f32),
    ) -> ThreePointCubic {
        ThreePointCubic {
            a1,
            b1,
            midpoint,
            a2,
            b2,
        }
    }

    /// Where the curve is at `t`.
    ///
    /// Each half is evaluated as an ordinary cubic in its **own** unit square
    /// and then scaled back out. That is why the control points are divided by
    /// the segment's extent: a cubic only knows how to run from (0,0) to
    /// (1,1), so the segment is normalised, solved, and put back.
    pub fn transform(self, t: f32) -> f32 {
        let first = t < self.midpoint.0;
        let scale_x = if first {
            self.midpoint.0
        } else {
            1.0 - self.midpoint.0
        };
        let scale_y = if first {
            self.midpoint.1
        } else {
            1.0 - self.midpoint.1
        };
        let scaled_t = (t - if first { 0.0 } else { self.midpoint.0 }) / scale_x;
        if first {
            cubic(
                self.a1.0 / scale_x,
                self.a1.1 / scale_y,
                self.b1.0 / scale_x,
                self.b1.1 / scale_y,
                scaled_t,
            ) * scale_y
        } else {
            cubic(
                (self.a2.0 - self.midpoint.0) / scale_x,
                (self.a2.1 - self.midpoint.1) / scale_y,
                (self.b2.0 - self.midpoint.0) / scale_x,
                (self.b2.1 - self.midpoint.1) / scale_y,
                scaled_t,
            ) * scale_y
                + self.midpoint.1
        }
    }

    /// The shape run backwards.
    ///
    /// Exact, for the same reason a cubic's is: mirroring every point through
    /// (0.5, 0.5) and reversing their order gives `1 - curve(1 - t)`. The two
    /// segments swap, so the second curve's control points become the first's.
    pub fn flipped(self) -> ThreePointCubic {
        let mirror = |point: (f32, f32)| (1.0 - point.0, 1.0 - point.1);
        ThreePointCubic {
            a1: mirror(self.b2),
            b1: mirror(self.a2),
            midpoint: mirror(self.midpoint),
            a2: mirror(self.b1),
            b2: mirror(self.a1),
        }
    }
}

fn cubic(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    const ERROR_BOUND: f32 = 0.001;
    fn evaluate(a: f32, b: f32, m: f32) -> f32 {
        3.0 * a * (1.0 - m) * (1.0 - m) * m + 3.0 * b * (1.0 - m) * m * m + m * m * m
    }
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let mut start = 0.0f32;
    let mut end = 1.0f32;
    // Bounded rather than `loop`: bisection converges in about ten steps, and
    // control points that make the curve non-monotonic must not be able to
    // hang a frame looking for a parameter that is not there.
    for _ in 0..30 {
        let midpoint = (start + end) / 2.0;
        let estimate = evaluate(a, c, midpoint);
        if (t - estimate).abs() < ERROR_BOUND {
            return evaluate(b, d, midpoint);
        }
        if estimate < t {
            start = midpoint;
        } else {
            end = midpoint;
        }
    }
    evaluate(b, d, (start + end) / 2.0)
}

/// Upstream's `_bounce`: four parabolic arcs, each a quarter the height of the
/// one before.
fn bounce(t: f32) -> f32 {
    if t < 1.0 / 2.75 {
        7.5625 * t * t
    } else if t < 2.0 / 2.75 {
        let t = t - 1.5 / 2.75;
        7.5625 * t * t + 0.75
    } else if t < 2.5 / 2.75 {
        let t = t - 2.25 / 2.75;
        7.5625 * t * t + 0.9375
    } else {
        let t = t - 2.625 / 2.75;
        7.5625 * t * t + 0.984375
    }
}

// -- Tweens -------------------------------------------------------------------

/// Maps 0..1 onto a range of some type.
pub trait Tween {
    type Output;

    fn lerp(&self, t: f32) -> Self::Output;
}

/// Interpolates between two numbers.
#[derive(Clone, Copy, Debug)]
pub struct FloatTween {
    pub begin: f32,
    pub end: f32,
}

impl FloatTween {
    pub const fn new(begin: f32, end: f32) -> FloatTween {
        FloatTween { begin, end }
    }
}

impl Tween for FloatTween {
    type Output = f32;

    fn lerp(&self, t: f32) -> f32 {
        self.begin + (self.end - self.begin) * t
    }
}

impl Animatable for FloatTween {
    type Output = f32;

    fn transform(&self, t: f32) -> f32 {
        self.lerp(t)
    }
}

/// Interpolates a colour channel by channel, in straight (non-premultiplied)
/// space. Good enough for UI, and wrong for anything with wide gamut ambitions.
#[derive(Clone, Copy, Debug)]
pub struct ColorTween {
    pub begin: crate::engine::Color,
    pub end: crate::engine::Color,
}

impl ColorTween {
    pub const fn new(begin: crate::engine::Color, end: crate::engine::Color) -> ColorTween {
        ColorTween { begin, end }
    }
}

impl Tween for ColorTween {
    type Output = crate::engine::Color;

    fn lerp(&self, t: f32) -> crate::engine::Color {
        let mix = |a: u8, b: u8| -> u8 {
            (a as f32 + (b as f32 - a as f32) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        crate::engine::Color::argb(
            mix(self.begin.alpha(), self.end.alpha()),
            mix(self.begin.red(), self.end.red()),
            mix(self.begin.green(), self.end.green()),
            mix(self.begin.blue(), self.end.blue()),
        )
    }
}

impl Animatable for ColorTween {
    type Output = crate::engine::Color;

    fn transform(&self, t: f32) -> crate::engine::Color {
        self.lerp(t)
    }
}

/// Interpolates an offset.
#[derive(Clone, Copy, Debug)]
pub struct OffsetTween {
    pub begin: crate::render::Offset,
    pub end: crate::render::Offset,
}

impl OffsetTween {
    pub const fn new(begin: crate::render::Offset, end: crate::render::Offset) -> OffsetTween {
        OffsetTween { begin, end }
    }
}

impl Tween for OffsetTween {
    type Output = crate::render::Offset;

    fn lerp(&self, t: f32) -> crate::render::Offset {
        crate::render::Offset::new(
            self.begin.dx + (self.end.dx - self.begin.dx) * t,
            self.begin.dy + (self.end.dy - self.begin.dy) * t,
        )
    }
}

impl Animatable for OffsetTween {
    type Output = crate::render::Offset;

    fn transform(&self, t: f32) -> crate::render::Offset {
        self.lerp(t)
    }
}

// -- Controller ---------------------------------------------------------------

/// Which way a running animation is going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Reverse,
}

/// What happens when a controller reaches the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Repeat {
    /// Stop.
    #[default]
    Once,
    /// Jump back to the start and go again.
    Loop,
    /// Turn around.
    PingPong,
}

/// Upstream `AnimationBehavior`: what an animation does when the reader has
/// asked the platform to stop animating things.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AnimationBehavior {
    /// Shorten the animation when `AccessibilityFeatures.disableAnimations` is
    /// set. `AnimationController`'s default.
    #[default]
    Normal,
    /// Run as written, whatever the setting says.
    ///
    /// **`AnimationController.unbounded`'s default**, and upstream says why:
    /// it is the repeating case, "in order to prevent them from flashing
    /// rapidly on the screen if the widget does not take the
    /// [AccessibilityFeatures.disableAnimations] flag into account". So the
    /// setting is honoured by default *except* where honouring it would look
    /// worse than ignoring it.
    Preserve,
}

impl AnimationBehavior {
    /// Upstream's `_enableAnimations`.
    ///
    /// The flag arrives as an argument rather than being read from a binding,
    /// because this crate has no bridge for it: nothing reports
    /// `disableAnimations` from the platform yet. Taking it as a parameter is
    /// what keeps both arms of this reachable -- a switch that can only ever
    /// answer one way is not a switch, it is a constant with a longer name.
    pub fn enables_animations(self, disable_animations: bool) -> bool {
        match self {
            AnimationBehavior::Normal => !disable_animations,
            AnimationBehavior::Preserve => true,
        }
    }

    /// Upstream's `scale` in `_animateToInternal`: **five per cent, not zero.**
    ///
    /// Its comment gives the reason, and it is not about taste: "Ideally, the
    /// framework would be able to handle zero duration animations; however,
    /// the common pattern of an eternally repeating animation might cause an
    /// endless loop if it weren't delayed for at least one frame. Instead,
    /// it's run at 5% of the normal duration to limit most animations to a
    /// single frame."
    pub const DISABLED_DURATION_SCALE: f32 = 0.05;

    /// Upstream's `scale` in `fling`: the velocity is multiplied, not the
    /// duration divided.
    ///
    /// A spring has no duration to shorten -- it runs until it settles -- so
    /// the way to make one arrive at once is to throw it two hundred times as
    /// hard. Same intent as [`AnimationBehavior::DISABLED_DURATION_SCALE`],
    /// opposite arithmetic, because the two mechanisms have nothing to divide
    /// in common.
    pub const DISABLED_FLING_VELOCITY_SCALE: f32 = 200.0;

    /// What to multiply a duration by.
    pub fn duration_scale(self, disable_animations: bool) -> f32 {
        if self.enables_animations(disable_animations) {
            1.0
        } else {
            AnimationBehavior::DISABLED_DURATION_SCALE
        }
    }

    /// What to multiply a fling's velocity by.
    pub fn fling_velocity_scale(self, disable_animations: bool) -> f32 {
        if self.enables_animations(disable_animations) {
            1.0
        } else {
            AnimationBehavior::DISABLED_FLING_VELOCITY_SCALE
        }
    }
}

/// Turns elapsed time into a value between 0 and 1.
///
/// A controller does not know what frame it is on; it is told, by
/// [`Controller::tick`], how much time has passed. That keeps it testable
/// without a clock and makes a paused animation exactly an animation that is
/// not being ticked.
#[derive(Clone, Debug)]
pub struct Controller {
    duration: Duration,
    /// Upstream's `animationBehavior`, and the flag it is weighed against.
    /// The flag lives here rather than in a binding because nothing bridges
    /// it; a caller that knows the setting sets it.
    behavior: AnimationBehavior,
    disable_animations: bool,
    value: f32,
    direction: Direction,
    repeat: Repeat,
    running: bool,
    curve: Curve,
}

impl Controller {
    pub fn new(duration: Duration) -> Controller {
        Controller {
            duration,
            behavior: AnimationBehavior::Normal,
            disable_animations: false,
            value: 0.0,
            direction: Direction::Forward,
            repeat: Repeat::Once,
            running: false,
            curve: Curve::Linear,
        }
    }

    pub fn with_curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }

    pub fn with_repeat(mut self, repeat: Repeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// Starts from wherever it is, going forwards.
    pub fn forward(&mut self) {
        self.direction = Direction::Forward;
        self.running = self.value < 1.0 || self.repeat != Repeat::Once;
        if !self.running {
            // Already at the end and not repeating: nothing to run, but the
            // caller asked for forward, so make that true.
            self.value = 1.0;
        }
    }

    /// Starts from wherever it is, going backwards.
    pub fn reverse(&mut self) {
        self.direction = Direction::Reverse;
        self.running = self.value > 0.0 || self.repeat != Repeat::Once;
        if !self.running {
            self.value = 0.0;
        }
    }

    /// The status this controller would report: upstream's `status`, derived
    /// rather than stored.
    ///
    /// Upstream keeps `_status` as a field written by `_checkStatusChanged`;
    /// here it falls out of the value and the direction last asked for. The
    /// wrapper's `derived_status` says the same thing from outside, and this
    /// exists so that [`Controller::toggle`] can ask without one.
    pub fn status(&self) -> AnimationStatus {
        if self.running {
            return match self.direction {
                Direction::Forward => AnimationStatus::Forward,
                Direction::Reverse => AnimationStatus::Reverse,
            };
        }
        match (self.value, self.direction) {
            (value, _) if value >= 1.0 => AnimationStatus::Completed,
            (value, _) if value <= 0.0 => AnimationStatus::Dismissed,
            (_, Direction::Forward) => AnimationStatus::Forward,
            (_, Direction::Reverse) => AnimationStatus::Reverse,
        }
    }

    /// Goes the other way from wherever it is. What a toggle wants: an
    /// interrupted animation continues from where it got to rather than
    /// snapping.
    ///
    /// # It asks the status, not the direction, and the two disagree at rest
    ///
    /// Upstream is `_direction = isForwardOrCompleted ? reverse : forward`.
    /// This port asked `self.direction` instead, which is the direction last
    /// *requested* and outlives the run that requested it. The two agree while
    /// something is moving and part company at the ends:
    ///
    /// **A fresh controller sits at 0.0 with the direction still forward.**
    /// Its status is `dismissed`, whose aim is away from completion, so
    /// upstream's toggle runs it **forward**. Reading the direction gave
    /// `reverse` -- and reversing something already at zero plays nothing at
    /// all. The first toggle of a controller nobody had run yet did nothing.
    pub fn toggle(&mut self) {
        if self.status().is_forward_or_completed() {
            self.reverse();
        } else {
            self.forward();
        }
    }

    /// Restarts from the beginning.
    pub fn restart(&mut self) {
        self.value = 0.0;
        self.direction = Direction::Forward;
        self.running = true;
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// The raw 0..1 value, before the curve.
    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn set_value(&mut self, value: f32) {
        self.value = value.clamp(0.0, 1.0);
    }

    /// The value after the curve. This is what a build should read.
    pub fn curved(&self) -> f32 {
        self.curve.transform(self.value)
    }

    /// Maps the curved value through a tween.
    pub fn animate<T: Tween>(&self, tween: &T) -> T::Output {
        tween.lerp(self.curved())
    }

    /// Advances by `elapsed`. Returns whether the animation is still running,
    /// which the caller uses to decide whether to ask for another frame.
    /// Upstream's `animationBehavior` constructor argument.
    pub fn with_behavior(mut self, behavior: AnimationBehavior) -> Controller {
        self.behavior = behavior;
        self
    }

    /// What `SemanticsBinding.instance.disableAnimations` would answer.
    ///
    /// Set by whoever knows, since nothing bridges the platform's
    /// accessibility features into this crate yet.
    pub fn with_disable_animations(mut self, disable_animations: bool) -> Controller {
        self.disable_animations = disable_animations;
        self
    }

    pub fn behavior(&self) -> AnimationBehavior {
        self.behavior
    }

    pub fn tick(&mut self, elapsed: Duration) -> bool {
        if !self.running {
            return false;
        }
        if self.duration.is_zero() {
            self.value = match self.direction {
                Direction::Forward => 1.0,
                Direction::Reverse => 0.0,
            };
            self.running = false;
            return false;
        }

        // Upstream scales the *simulation's* duration rather than the step,
        // which comes to the same thing: a fifth of a twentieth of the way
        // through per tick is twenty times as far.
        let scaled =
            self.duration.as_secs_f32() * self.behavior.duration_scale(self.disable_animations);
        let step = elapsed.as_secs_f32() / scaled;
        match self.direction {
            Direction::Forward => self.value += step,
            Direction::Reverse => self.value -= step,
        }

        // Handle the ends. Which end counts depends on the direction: a forward
        // animation sitting at exactly 0 has not finished, it has not started,
        // and treating "at an end" as direction-free stops it on its first
        // tick. A single tick can also overshoot by more than a whole cycle if
        // the app was suspended, so looping wraps rather than subtracting once.
        let reached_end = match self.direction {
            Direction::Forward => self.value >= 1.0,
            Direction::Reverse => self.value <= 0.0,
        };
        if reached_end {
            match self.repeat {
                Repeat::Once => {
                    self.value = self.value.clamp(0.0, 1.0);
                    self.running = false;
                }
                Repeat::Loop => {
                    self.value = self.value.rem_euclid(1.0);
                }
                Repeat::PingPong => {
                    // Fold back on itself: 1.2 becomes 0.8, -0.2 becomes 0.2.
                    let folded = self.value.rem_euclid(2.0);
                    self.value = if folded > 1.0 { 2.0 - folded } else { folded };
                    self.direction = match self.direction {
                        Direction::Forward => Direction::Reverse,
                        Direction::Reverse => Direction::Forward,
                    };
                }
            }
        }
        // True even on the tick that finished, because this answer is also what
        // marks the widget for rebuilding: a tick that moved the value has to
        // be drawn, and the tick that moves it to exactly the end is the one
        // that matters most. Reporting `self.running` here left every animation
        // showing its second-to-last frame for ever -- a fade-out that stopped
        // at ninety-eight per cent leaves a ghost on the screen.
        //
        // The next tick returns false at the top, so this costs one frame and
        // stops.
        true
    }

    /// Whether the animation is at rest at either end.
    pub fn is_settled(&self) -> bool {
        !self.running && (self.value <= 0.0 || self.value >= 1.0)
    }
}

// -- A set of them ------------------------------------------------------------

/// A named collection of controllers, ticked together.
///
/// A screen usually has several animations that all want the same frames. This
/// keeps the "is anything still running" question in one place, which is the
/// question that decides whether to ask for another frame.
#[derive(Clone, Debug, Default)]
pub struct Animations {
    entries: Vec<(&'static str, Controller)>,
    /// When the last tick happened, so the next one knows how long it has been.
    last_frame_micros: Option<i64>,
}

impl Animations {
    pub fn new() -> Animations {
        Animations::default()
    }

    pub fn insert(&mut self, name: &'static str, controller: Controller) {
        match self.entries.iter_mut().find(|(key, _)| *key == name) {
            Some((_, existing)) => *existing = controller,
            None => self.entries.push((name, controller)),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Controller> {
        self.entries
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, controller)| controller)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Controller> {
        self.entries
            .iter_mut()
            .find(|(key, _)| *key == name)
            .map(|(_, controller)| controller)
    }

    /// The curved value of `name`, or `default` if there is no such animation.
    /// Reading a missing animation is a caller mistake, but a build that fails
    /// to draw is worse than one that draws the resting state.
    pub fn value_or(&self, name: &str, default: f32) -> f32 {
        self.get(name)
            .map_or(default, |controller| controller.curved())
    }

    /// Advances every controller to `frame_time_micros`.
    ///
    /// Returns whether anything is still running. Pass the frame time straight
    /// from [`crate::app::FrameContext`]; the first call establishes the
    /// baseline and advances nothing.
    pub fn tick(&mut self, frame_time_micros: i64) -> bool {
        let elapsed = match self.last_frame_micros {
            Some(previous) if frame_time_micros > previous => {
                Duration::from_micros((frame_time_micros - previous) as u64)
            }
            // First frame, or a clock that went backwards. Either way there is
            // no sane elapsed time, so advance nothing and start counting.
            _ => Duration::ZERO,
        };
        self.last_frame_micros = Some(frame_time_micros);

        let mut running = false;
        for (_, controller) in self.entries.iter_mut() {
            running |= controller.tick(elapsed);
        }
        running
    }

    /// Whether anything is running, without advancing time.
    pub fn is_running(&self) -> bool {
        self.entries.iter().any(|(_, c)| c.is_running())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_three_point_cubic_passes_exactly_through_its_join() {
        // Which is the only reason the shape exists: a single cubic through
        // (0,0) and (1,1) cannot be made to pass through an arbitrary interior
        // point with prescribed tangents on both sides of it.
        let shape = match Curve::FAST_EASE_IN_TO_SLOW_EASE_OUT {
            Curve::ThreePointCubic(shape) => shape,
            other => panic!("expected a three-point cubic, got {other:?}"),
        };
        let (mid_x, mid_y) = shape.midpoint;
        assert!(
            (shape.transform(mid_x) - mid_y).abs() < 1e-3,
            "at {mid_x} the curve is {} rather than {mid_y}",
            shape.transform(mid_x)
        );
        assert_eq!(shape.transform(0.0), 0.0);
        assert!((shape.transform(1.0) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn the_ios_page_curve_is_most_of_the_way_there_by_a_fifth_of_the_time() {
        // Fast in, slow out: 54% of the distance in 20% of the duration. That
        // is what makes an iOS page feel like it has already arrived while it
        // is still settling, and it is not a shape a plain ease-out reaches.
        let curve = Curve::FAST_EASE_IN_TO_SLOW_EASE_OUT;
        assert!(curve.transform(0.198) > 0.5);
        assert!(
            Curve::EASE_OUT.transform(0.198) < 0.5,
            "where the nearest single cubic is not even halfway"
        );
        assert!(curve.transform(0.6) > 0.95, "and then it crawls home");
    }

    #[test]
    fn flipping_a_three_point_cubic_is_exact_rather_than_approximate() {
        // Mirroring every control point through (0.5, 0.5) and reversing their
        // order gives 1 - curve(1 - t), which is what a reversing animation
        // wants. The two segments swap places, so the claim is not obvious.
        let curve = Curve::FAST_EASE_IN_TO_SLOW_EASE_OUT;
        let flipped = curve.flipped();
        for step in 1..20 {
            let t = step as f32 / 20.0;
            let expected = 1.0 - curve.transform(1.0 - t);
            assert!(
                (flipped.transform(t) - expected).abs() < 2e-3,
                "at {t}: {} against {expected}",
                flipped.transform(t)
            );
        }
    }

    #[test]
    fn the_two_ios_linear_curves_are_spelled_out_and_are_not_each_others_flip() {
        // Upstream declares both rather than deriving one, and the numbers say
        // why: a reversal that merely looked right would read as one.
        assert_ne!(
            Curve::LINEAR_TO_EASE_OUT.flipped(),
            Curve::EASE_IN_TO_LINEAR
        );
        assert_eq!(
            Curve::LINEAR_TO_EASE_OUT,
            Curve::Cubic(0.35, 0.91, 0.33, 0.97)
        );
        assert_eq!(
            Curve::EASE_IN_TO_LINEAR,
            Curve::Cubic(0.67, 0.03, 0.65, 0.09)
        );
    }

    use super::*;
    use crate::engine::Color;

    #[test]
    fn curves_pass_through_both_ends() {
        for curve in [
            Curve::Linear,
            Curve::EaseIn,
            Curve::EaseOut,
            Curve::EaseInOut,
            Curve::EaseOutBack,
            Curve::Decelerate,
            Curve::EASE,
            Curve::FAST_OUT_SLOW_IN,
            Curve::EASE_IN_OUT_CUBIC,
            Curve::EASE_IN_BACK,
            Curve::BounceIn,
            Curve::BounceOut,
            Curve::BounceInOut,
            Curve::ELASTIC_IN,
            Curve::ELASTIC_OUT,
            Curve::ELASTIC_IN_OUT,
        ] {
            assert!((curve.transform(0.0) - 0.0).abs() < 1e-5, "{curve:?} at 0");
            assert!((curve.transform(1.0) - 1.0).abs() < 1e-5, "{curve:?} at 1");
        }
    }

    #[test]
    fn curves_clamp_out_of_range_input() {
        assert_eq!(Curve::Linear.transform(-1.0), 0.0);
        assert_eq!(Curve::Linear.transform(2.0), 1.0);
    }

    #[test]
    fn a_cubic_matches_the_curve_it_names() {
        // Sampled against upstream's Curves.easeInOut, which is this cubic.
        // The bisection's error bound is a thousandth, so the tolerance is the
        // solver's rather than the curve's.
        let ease_in_out = Curve::EASE_IN_OUT;
        assert!(
            (ease_in_out.transform(0.5) - 0.5).abs() < 2e-3,
            "symmetric at the middle"
        );
        assert!(ease_in_out.transform(0.25) < 0.25, "still easing in");
        assert!(ease_in_out.transform(0.75) > 0.75, "already easing out");
    }

    #[test]
    fn every_curve_flips_to_what_the_definition_says() {
        // `flipped` answers a closed form per variant rather than wrapping,
        // which is exact where the family is closed under flipping and wrong
        // the moment it is not. This holds every variant to
        // `flipped(t) == 1 - curve(1 - t)`, which is the whole of what
        // upstream's `FlippedCurve` computes -- so a variant added later
        // without a flip of its own is caught here rather than in a widget.
        let curves = [
            Curve::Linear,
            Curve::Decelerate,
            Curve::Accelerate,
            Curve::EASE_IN,
            Curve::EASE_OUT,
            Curve::EASE_IN_OUT_CUBIC_EMPHASIZED,
            Curve::ElasticIn(0.4),
            Curve::ElasticOut(0.4),
            Curve::ElasticInOut(0.4),
            Curve::BounceIn,
            Curve::BounceOut,
            Curve::BounceInOut,
        ];
        for curve in curves {
            for step in 0..=20 {
                let t = step as f32 / 20.0;
                let want = 1.0 - curve.transform(1.0 - t);
                let got = curve.flipped().transform(t);
                assert!(
                    (want - got).abs() < 2e-3,
                    "{curve:?} at {t}: flipped gave {got}, the definition says {want}"
                );
            }
        }
    }

    #[test]
    fn decelerating_run_backwards_is_accelerating() {
        // Not itself. `decelerate` is `1 - (1 - t)^2`, so its flip is `t^2` --
        // a curve that starts slowly and ends fast, which is the opposite
        // shape. Upstream has no name for it and writes
        // `FlippedCurve(Curves.decelerate)`.
        assert_eq!(Curve::Decelerate.flipped(), Curve::Accelerate);
        assert_eq!(Curve::Accelerate.flipped(), Curve::Decelerate);
        assert_ne!(
            Curve::Decelerate.flipped(),
            Curve::Decelerate,
            "it is not its own flip"
        );

        // t squared, and the halfway point is where the two differ most.
        assert!((Curve::Accelerate.transform(0.5) - 0.25).abs() < 1e-5);
        assert!((Curve::Decelerate.transform(0.5) - 0.75).abs() < 1e-5);
        assert!(
            (Curve::Accelerate.transform(0.5) - Curve::Decelerate.transform(0.5)).abs() > 0.49,
            "half a unit apart in the middle"
        );
    }

    #[test]
    fn the_emphasized_easing_leaves_late_and_arrives_early() {
        // Material 3's emphasized curve is two cubics joined at (0.25, 1): it
        // reaches its destination a quarter of the way through and spends the
        // rest settling. That is why it is a ThreePointCubic and not a cubic.
        let curve = Curve::EASE_IN_OUT_CUBIC_EMPHASIZED;
        // Slow for the first tenth, then a burst: a quarter to two thirds of
        // the way between t=0.15 and t=0.20.
        assert!(curve.transform(0.1) < 0.1, "{}", curve.transform(0.1));
        assert!(curve.transform(0.15) < 0.3);
        assert!(curve.transform(0.2) > 0.6, "{}", curve.transform(0.2));

        // Already 95% there at the half-way point, and the whole second half
        // is the last five per cent arriving.
        assert!(curve.transform(0.5) > 0.94, "{}", curve.transform(0.5));
        assert!(
            curve.transform(1.0) - curve.transform(0.5) < 0.06,
            "half the time for a twentieth of the distance"
        );

        // The join is at the midpoint upstream names -- 40% of the distance
        // in 17% of the time, in the middle of the burst.
        assert!((curve.transform(0.166_666) - 0.4).abs() < 0.02);
        assert_eq!(curve.transform(0.0), 0.0);
        assert_eq!(curve.transform(1.0), 1.0);
    }

    #[test]
    fn a_flipped_cubic_is_the_curve_run_backwards() {
        // What FlippedCurve means: flipped(t) == 1 - curve(1 - t). For a cubic
        // the flip is another cubic, which is why this is exact rather than
        // approximated.
        let curve = Curve::EASE_IN_BACK;
        let flipped = curve.flipped();
        for step in 0..=10 {
            let t = step as f32 / 10.0;
            let expected = 1.0 - curve.transform(1.0 - t);
            assert!(
                (flipped.transform(t) - expected).abs() < 5e-3,
                "at {t}: {} against {expected}",
                flipped.transform(t)
            );
        }
    }

    #[test]
    fn a_bounce_lands_and_bounces_smaller() {
        // Three landings on the way, each lower than the last bounce's peak.
        let peak = |from: i32, to: i32| {
            (from..=to)
                .map(|i| Curve::BounceOut.transform(i as f32 / 100.0))
                .fold(f32::MIN, f32::max)
        };
        let first = peak(30, 45);
        let second = peak(70, 85);
        assert!(first < 1.0 && second < 1.0);
        assert!(
            second > first,
            "later bounces are closer to the ground, not further"
        );
    }

    #[test]
    fn elastic_overshoots_in_both_directions() {
        let samples: Vec<f32> = (0..=100)
            .map(|i| Curve::ELASTIC_OUT.transform(i as f32 / 100.0))
            .collect();
        assert!(samples.iter().any(|v| *v > 1.0), "never overshot");
        assert!(samples.iter().any(|v| *v < 1.0), "never came back");
    }

    #[test]
    fn ease_in_starts_slower_than_linear() {
        assert!(Curve::EaseIn.transform(0.25) < 0.25);
        assert!(Curve::EaseOut.transform(0.25) > 0.25);
    }

    #[test]
    fn back_overshoots_before_settling() {
        // The point of the curve: somewhere past the middle it goes above 1.
        let overshot = (60..=95)
            .map(|i| Curve::EaseOutBack.transform(i as f32 / 100.0))
            .any(|v| v > 1.0);
        assert!(overshot);
    }

    #[test]
    fn a_controller_runs_to_the_end_and_stops() {
        let mut controller = Controller::new(Duration::from_millis(100));
        controller.forward();
        assert!(controller.tick(Duration::from_millis(50)));
        assert!((controller.value() - 0.5).abs() < 1e-5);
        // The tick that arrives at the end still reports that it did
        // something: that answer is what marks the widget for rebuilding, and
        // this is the tick whose value is the one to draw.
        assert!(controller.tick(Duration::from_millis(60)));
        assert_eq!(controller.value(), 1.0);
        assert!(!controller.is_running());
        // Only the tick after it is idle.
        assert!(!controller.tick(Duration::from_millis(16)));
    }

    #[test]
    fn reverse_runs_back_to_zero() {
        let mut controller = Controller::new(Duration::from_millis(100));
        controller.set_value(1.0);
        controller.reverse();
        assert!(
            controller.tick(Duration::from_millis(200)),
            "the arriving tick counts"
        );
        assert_eq!(controller.value(), 0.0);
        assert!(!controller.tick(Duration::from_millis(16)));
    }

    #[test]
    fn toggle_continues_from_where_it_got_to() {
        let mut controller = Controller::new(Duration::from_millis(100));
        controller.forward();
        controller.tick(Duration::from_millis(30));
        assert!((controller.value() - 0.3).abs() < 1e-5);

        // Interrupted a third of the way in: it should come back from 0.3, not
        // snap to 1 first.
        controller.toggle();
        assert_eq!(controller.direction(), Direction::Reverse);
        controller.tick(Duration::from_millis(10));
        assert!((controller.value() - 0.2).abs() < 1e-5);
    }

    // -- Which way the animation is aiming, tick 318 -----------------------

    /// An animation that reports a status and nothing else, so the four
    /// states can be walked without driving a controller into each.
    struct FixedStatusAnimation {
        status: AnimationStatus,
    }

    impl Animation for FixedStatusAnimation {
        fn value(&self) -> f32 {
            0.0
        }

        fn status(&self) -> AnimationStatus {
            self.status
        }

        fn add_listener(&self, _listener: AnimationListener) {}
        fn remove_listener(&self, _listener: &AnimationListener) {}
    }

    #[test]
    fn the_aim_cuts_the_four_states_along_the_other_diagonal() {
        // Not the negation of is_animating, and not of is_dismissed: forward
        // pairs with completed, reverse with dismissed.
        assert!(AnimationStatus::Forward.is_forward_or_completed());
        assert!(AnimationStatus::Completed.is_forward_or_completed());
        assert!(!AnimationStatus::Reverse.is_forward_or_completed());
        assert!(!AnimationStatus::Dismissed.is_forward_or_completed());

        // The two moving states fall on opposite sides, though both are
        // equally "animating".
        assert_eq!(
            AnimationStatus::Forward.is_animating(),
            AnimationStatus::Reverse.is_animating()
        );
        assert_ne!(
            AnimationStatus::Forward.is_forward_or_completed(),
            AnimationStatus::Reverse.is_forward_or_completed()
        );
    }

    #[test]
    fn a_first_toggle_of_a_controller_nobody_ran_plays_it_forward() {
        // It sits at 0.0 with the direction still forward, so reading the
        // direction says "reverse" -- and reversing something already at zero
        // plays nothing at all. The status is `dismissed`, whose aim is away
        // from completion, so the toggle runs it forward.
        let mut controller = Controller::new(Duration::from_millis(100));
        assert_eq!(controller.direction(), Direction::Forward, "never run");
        assert_eq!(controller.status(), AnimationStatus::Dismissed);

        controller.toggle();
        assert_eq!(controller.direction(), Direction::Forward);
        controller.tick(Duration::from_millis(50));
        assert!(
            controller.value() > 0.0,
            "it moved, rather than sitting at zero"
        );
    }

    #[test]
    fn an_animation_run_backwards_aims_the_other_way() {
        // `ReverseAnimation` maps the four states across exactly the diagonal
        // this predicate cuts along, so its aim is always the opposite of its
        // parent's -- which is what makes it worth asking here rather than
        // asking the status by hand at every call site.
        for status in [
            AnimationStatus::Dismissed,
            AnimationStatus::Forward,
            AnimationStatus::Reverse,
            AnimationStatus::Completed,
        ] {
            let parent = Rc::new(FixedStatusAnimation { status });
            let reversed = ReverseAnimation::new(parent.clone());
            assert_eq!(
                parent.is_forward_or_completed(),
                !reversed.is_forward_or_completed(),
                "{status:?}"
            );
        }

        // And one absolute reading, so the pairing above cannot be satisfied
        // by both sides being wrong together.
        assert!(
            !AlwaysStoppedAnimation { value: 0.5 }.is_forward_or_completed(),
            "a stopped animation reports dismissed, whose aim is away"
        );
    }

    #[test]
    fn a_toggle_at_the_far_end_comes_back() {
        let mut controller = Controller::new(Duration::from_millis(100));
        controller.forward();
        controller.tick(Duration::from_millis(100));
        assert_eq!(controller.status(), AnimationStatus::Completed);

        controller.toggle();
        assert_eq!(controller.direction(), Direction::Reverse);
        controller.tick(Duration::from_millis(50));
        assert!(controller.value() < 1.0);
    }

    #[test]
    fn a_controller_stopped_part_way_keeps_the_direction_it_was_sent_in() {
        // Mid-flight the status and the direction agree, which is why the
        // interrupted-toggle case above was never wrong.
        let mut controller = Controller::new(Duration::from_millis(100));
        controller.forward();
        controller.tick(Duration::from_millis(30));
        assert_eq!(controller.status(), AnimationStatus::Forward);
        assert!(controller.status().is_forward_or_completed());
    }

    #[test]
    fn looping_wraps_rather_than_stopping() {
        let mut controller = Controller::new(Duration::from_millis(100)).with_repeat(Repeat::Loop);
        controller.forward();
        assert!(controller.tick(Duration::from_millis(250)));
        // 2.5 cycles in: a quarter of the way through the third.
        assert!((controller.value() - 0.5).abs() < 1e-4);
        assert!(controller.is_running());
    }

    #[test]
    fn ping_pong_turns_around() {
        let mut controller =
            Controller::new(Duration::from_millis(100)).with_repeat(Repeat::PingPong);
        controller.forward();
        controller.tick(Duration::from_millis(120));
        assert_eq!(controller.direction(), Direction::Reverse);
        assert!((controller.value() - 0.8).abs() < 1e-4);
    }

    #[test]
    fn a_zero_duration_animation_finishes_immediately() {
        let mut controller = Controller::new(Duration::ZERO);
        controller.forward();
        assert!(!controller.tick(Duration::from_millis(1)));
        assert_eq!(controller.value(), 1.0);
    }

    #[test]
    fn tweens_hit_both_ends() {
        let float = FloatTween::new(10.0, 20.0);
        assert_eq!(float.lerp(0.0), 10.0);
        assert_eq!(float.lerp(1.0), 20.0);
        assert_eq!(float.lerp(0.5), 15.0);

        let color = ColorTween::new(Color::rgb(0, 0, 0), Color::rgb(255, 255, 255));
        assert_eq!(color.lerp(0.0), Color::rgb(0, 0, 0));
        assert_eq!(color.lerp(1.0), Color::rgb(255, 255, 255));
        assert_eq!(color.lerp(0.5), Color::rgb(128, 128, 128));
    }

    #[test]
    fn the_first_tick_of_a_set_advances_nothing() {
        let mut animations = Animations::new();
        let mut controller = Controller::new(Duration::from_millis(100));
        controller.forward();
        animations.insert("fade", controller);

        // The first frame only establishes the baseline: there is no previous
        // frame to measure from, and guessing would jump the animation.
        animations.tick(1_000_000);
        assert_eq!(animations.get("fade").unwrap().value(), 0.0);

        animations.tick(1_050_000);
        assert!((animations.get("fade").unwrap().value() - 0.5).abs() < 1e-4);
    }

    #[test]
    fn a_set_reports_when_everything_has_settled() {
        let mut animations = Animations::new();
        let mut controller = Controller::new(Duration::from_millis(10));
        controller.forward();
        animations.insert("a", controller);

        animations.tick(0);
        assert!(animations.tick(5_000));
        assert!(
            animations.tick(20_000),
            "the frame it lands on still has to be drawn"
        );
        assert!(!animations.tick(30_000));
        assert!(!animations.is_running());
    }

    #[test]
    fn reading_a_missing_animation_gives_the_default() {
        let animations = Animations::new();
        assert_eq!(animations.value_or("nope", 0.25), 0.25);
    }
}

// -- The Animation object graph (upstream animation/animation.dart, ---------------
//    tween.dart, tween_sequence.dart, animation_controller.dart)

/// Upstream `AnimationStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationStatus {
    Dismissed,
    Forward,
    Reverse,
    Completed,
}

impl AnimationStatus {
    pub fn is_dismissed(&self) -> bool {
        *self == AnimationStatus::Dismissed
    }

    pub fn is_completed(&self) -> bool {
        *self == AnimationStatus::Completed
    }

    pub fn is_animating(&self) -> bool {
        matches!(self, AnimationStatus::Forward | AnimationStatus::Reverse)
    }

    /// Upstream's `isForwardOrCompleted`: "whether the current aim of the
    /// animation is toward completion."
    ///
    /// # It is about the aim, not about the picture
    ///
    /// The two moving statuses fall on opposite sides, and neither matches
    /// what is on the screen at the moment it is asked:
    ///
    /// * **Forward is true from its first frame**, when the thing is barely
    ///   there at all.
    /// * **Reverse is false from its first frame**, when the thing is still
    ///   fully drawn.
    ///
    /// That is the point. Upstream's `MagnifierController.shown` is written on
    /// this, so a magnifier part-way through its exit already counts as not
    /// shown -- and a second `show` during that exit replaces it rather than
    /// deciding one is already up. Reading the pixels instead would have the
    /// answer flip in the middle of each animation.
    ///
    /// Note it is not the negation of [`AnimationStatus::is_animating`] nor of
    /// [`AnimationStatus::is_dismissed`]: it cuts the four states along the
    /// other diagonal, pairing forward with completed and reverse with
    /// dismissed.
    pub fn is_forward_or_completed(&self) -> bool {
        matches!(self, AnimationStatus::Forward | AnimationStatus::Completed)
    }
}

/// One listener on an animation: its value callback, and optionally its
/// status callback -- upstream's separate listener lists folded into one
/// registration.
#[derive(Clone)]
pub struct AnimationListener {
    pub on_value: Rc<dyn Fn()>,
    pub on_status: Option<Rc<dyn Fn(AnimationStatus)>>,
}

/// Upstream `Animation<T>`: a value that changes over time, telling its
/// listeners when it does and its status listeners when the direction of
/// time itself does. The crate's tick loop is the clock.
pub trait Animation {
    fn value(&self) -> f32;
    fn status(&self) -> AnimationStatus;

    fn add_listener(&self, listener: AnimationListener);
    fn remove_listener(&self, listener: &AnimationListener);

    /// Whether `value` can change again -- upstream `isListening`'s twin
    /// from the other side: a stopped animation never tells.
    fn is_animating(&self) -> bool {
        self.status().is_animating()
    }

    /// Upstream `Animation.isForwardOrCompleted`, which is the same question
    /// asked of the animation's status. See
    /// [`AnimationStatus::is_forward_or_completed`].
    fn is_forward_or_completed(&self) -> bool {
        self.status().is_forward_or_completed()
    }
}

/// Upstream `AlwaysStoppedAnimation`.
pub struct AlwaysStoppedAnimation {
    pub value: f32,
}

impl Animation for AlwaysStoppedAnimation {
    fn value(&self) -> f32 {
        self.value
    }

    fn status(&self) -> AnimationStatus {
        AnimationStatus::Dismissed
    }

    fn add_listener(&self, _listener: AnimationListener) {}
    fn remove_listener(&self, _listener: &AnimationListener) {}
}

/// The listener bookkeeping every composed animation shares -- upstream's
/// `AnimationLocalListenersMixin`/`AnimationLocalStatusListenersMixin` and
/// the lazy/eager attach mixins in one: listeners held here, attach and
/// detach driven by the first add and the last remove.
pub(crate) struct AnimationListeners {
    listeners: RefCell<Vec<AnimationListener>>,
}

impl AnimationListeners {
    pub fn new() -> AnimationListeners {
        AnimationListeners {
            listeners: RefCell::new(Vec::new()),
        }
    }

    pub fn add(&self, listener: AnimationListener) {
        self.listeners.borrow_mut().push(listener);
    }

    pub fn remove(&self, listener: &AnimationListener) {
        self.listeners
            .borrow_mut()
            .retain(|existing| !Rc::ptr_eq(&existing.on_value, &listener.on_value));
    }

    pub fn notify_value(&self) {
        for listener in self.listeners.borrow().clone() {
            (listener.on_value)();
        }
    }

    pub fn notify_status(&self, status: AnimationStatus) {
        for listener in self.listeners.borrow().clone() {
            if let Some(on_status) = listener.on_status {
                on_status(status);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.listeners.borrow().is_empty()
    }

    /// Upstream's `clearListeners`, which `dispose` calls.
    pub fn clear(&self) {
        self.listeners.borrow_mut().clear();
    }
}

/// Upstream `AnimationController`: the one animation nothing else drives.
///
/// [`Controller`] is the same clock as a plain value -- it is ticked, it
/// answers a number, and whoever holds it reads that number in their own
/// build. This is that controller behind shared ownership and with the
/// listener half attached, which is what the rest of the object graph needs:
/// a [`CurvedAnimation`], a [`ProxyAnimation`] and every transition take an
/// `Rc<dyn Animation>` parent, and until this existed the only things that
/// could be one were `AlwaysStoppedAnimation` and a test fixture.
///
/// The tick still comes from outside -- upstream's `Ticker`/`TickerProvider`
/// pair is not ported (`widgets/ticker_provider.dart`), so the owning widget
/// calls [`AnimationController::tick`] from its own
/// [`advance`](crate::framework::StatefulComponent::advance), the same place
/// every other animation in the crate is driven from.
pub struct AnimationController {
    controller: RefCell<Controller>,
    listeners: AnimationListeners,
    last_status: Cell<AnimationStatus>,
    /// The ticker this controller drives itself from, when it was given a
    /// provider -- upstream's `AnimationController(vsync:)`, which likewise
    /// builds its ticker in the constructor and starts it on `forward`.
    ticker: RefCell<Option<crate::ticker::Ticker>>,
}

impl AnimationController {
    pub fn new(duration: Duration) -> Rc<AnimationController> {
        Rc::new(AnimationController {
            controller: RefCell::new(Controller::new(duration)),
            listeners: AnimationListeners::new(),
            last_status: Cell::new(AnimationStatus::Dismissed),
            ticker: RefCell::new(None),
        })
    }

    /// Upstream `AnimationController(vsync:)`: the controller makes its
    /// ticker from the provider and runs itself off it, so a caller who has
    /// wired the provider into their `advance` never ticks this by hand.
    ///
    /// The ticker's callback holds a weak reference back, so the controller
    /// owning its ticker and the ticker calling its controller is not a
    /// cycle. Upstream's callback is given the elapsed time since the ticker
    /// started; [`Controller::tick`] takes the step since the last frame, so
    /// the difference is kept here.
    pub fn with_vsync(
        self: &Rc<Self>,
        vsync: &dyn crate::ticker::TickerProvider,
    ) -> crate::ticker::Ticker {
        let weak = Rc::downgrade(self);
        let last = Cell::new(Duration::ZERO);
        let ticker = vsync.create_ticker(Rc::new(move |elapsed: Duration| {
            let Some(controller) = weak.upgrade() else {
                return;
            };
            let step = elapsed.saturating_sub(last.get());
            last.set(elapsed);
            controller.tick(step);
            if !controller.is_running() {
                // Landed. Upstream's `_tick` stops its ticker on the frame
                // the simulation finishes, which is what takes the animation
                // out of the frame schedule. `Controller::tick` answers true
                // on that frame -- it is the frame that draws the end -- so
                // the question to ask is whether it is still running, not
                // what the tick said.
                controller.stop();
            }
        }));
        *self.ticker.borrow_mut() = Some(ticker.clone());
        ticker
    }

    /// Starts this controller's ticker, if it has one and it is not already
    /// running -- upstream's `_ticker!.start()` inside `_animateToInternal`.
    fn start_ticker(&self) {
        if let Some(ticker) = self.ticker.borrow().as_ref() {
            if self.controller.borrow().is_running() && !ticker.is_active() {
                ticker.start();
            }
        }
    }

    /// The same controller, curved. The curve is read on every `value`, so
    /// changing it mid-flight changes what is drawn from the next frame --
    /// upstream reaches the same effect through a `CurvedAnimation` wrapper.
    pub fn with_curve(self: Rc<Self>, curve: Curve) -> Rc<Self> {
        let curved = self.controller.borrow().clone().with_curve(curve);
        *self.controller.borrow_mut() = curved;
        self
    }

    pub fn with_repeat(self: Rc<Self>, repeat: Repeat) -> Rc<Self> {
        let repeating = self.controller.borrow().clone().with_repeat(repeat);
        *self.controller.borrow_mut() = repeating;
        self
    }

    /// The raw 0..1 value, before the curve -- upstream's
    /// `AnimationController.value`, which no curve is applied to either.
    pub fn raw_value(&self) -> f32 {
        self.controller.borrow().value()
    }

    pub fn set_value(&self, value: f32) {
        self.controller.borrow_mut().set_value(value);
        self.announce();
    }

    pub fn is_running(&self) -> bool {
        self.controller.borrow().is_running()
    }

    pub fn forward(&self) {
        self.controller.borrow_mut().forward();
        self.start_ticker();
        self.announce();
    }

    pub fn reverse(&self) {
        self.controller.borrow_mut().reverse();
        self.start_ticker();
        self.announce();
    }

    pub fn toggle(&self) {
        self.controller.borrow_mut().toggle();
        self.start_ticker();
        self.announce();
    }

    pub fn restart(&self) {
        self.controller.borrow_mut().restart();
        self.start_ticker();
        self.announce();
    }

    pub fn stop(&self) {
        self.controller.borrow_mut().stop();
        if let Some(ticker) = self.ticker.borrow().as_ref() {
            ticker.stop(false);
        }
        self.announce();
    }

    /// Advances the clock and tells the listeners. Returns whether another
    /// frame is wanted, exactly as [`Controller::tick`] does.
    pub fn tick(&self, elapsed: Duration) -> bool {
        let wants_frame = self.controller.borrow_mut().tick(elapsed);
        if wants_frame {
            self.listeners.notify_value();
        }
        self.announce();
        wants_frame
    }

    /// The status this controller is in now, derived rather than stored:
    /// running says which way, and a stopped controller is at one end or
    /// parked between them.
    ///
    /// Upstream keeps `_status` as a field written by `_checkStatusChanged`,
    /// so a controller stopped mid-flight keeps saying `forward`. Here the
    /// same answer falls out of the direction it was last sent in, which is
    /// what `Controller::direction` holds.
    fn derived_status(&self) -> AnimationStatus {
        // One copy of the rule, on the controller itself, because
        // [`Controller::toggle`] has to ask it from the inside.
        self.controller.borrow().status()
    }

    /// Tells the status listeners, if the status moved. Upstream's
    /// `_checkStatusChanged`.
    fn announce(&self) {
        let status = self.derived_status();
        if status != self.last_status.get() {
            self.last_status.set(status);
            self.listeners.notify_status(status);
        }
    }
}

impl Animation for AnimationController {
    fn value(&self) -> f32 {
        self.controller.borrow().curved()
    }

    fn status(&self) -> AnimationStatus {
        self.derived_status()
    }

    fn add_listener(&self, listener: AnimationListener) {
        self.listeners.add(listener);
    }

    fn remove_listener(&self, listener: &AnimationListener) {
        self.listeners.remove(listener);
    }

    /// Upstream `AnimationController.isAnimating` overrides the status-derived
    /// default with `_ticker.isActive`: a controller stopped in the middle is
    /// not animating even though its status still says which way it was
    /// going. This is that override.
    fn is_animating(&self) -> bool {
        self.controller.borrow().is_running()
    }
}

/// Upstream `ProxyAnimation`: an animation that forwards everything to
/// another, which can be swapped underneath -- the seam the persistent
/// headers' snap and the route transitions drive.
pub struct ProxyAnimation {
    inner: RefCell<Option<Rc<dyn Animation>>>,
    listeners: AnimationListeners,
    last_status: Cell<AnimationStatus>,
}

impl ProxyAnimation {
    pub fn new() -> ProxyAnimation {
        ProxyAnimation {
            inner: RefCell::new(None),
            listeners: AnimationListeners::new(),
            last_status: Cell::new(AnimationStatus::Dismissed),
        }
    }

    /// Upstream `ProxyAnimation.parent`'s setter: the old inner stops being
    /// told, the new one starts, and a status change across the swap is
    /// announced.
    pub fn set_parent(&self, parent: Option<Rc<dyn Animation>>) {
        *self.inner.borrow_mut() = parent;
        let status = self
            .inner
            .borrow()
            .as_ref()
            .map_or(AnimationStatus::Dismissed, |inner| inner.status());
        if status != self.last_status.get() {
            self.last_status.set(status);
            self.listeners.notify_status(status);
        }
    }

    pub fn parent(&self) -> Option<Rc<dyn Animation>> {
        self.inner.borrow().clone()
    }
}

impl Animation for ProxyAnimation {
    fn value(&self) -> f32 {
        self.inner
            .borrow()
            .as_ref()
            .map_or(0.0, |inner| inner.value())
    }

    fn status(&self) -> AnimationStatus {
        self.inner
            .borrow()
            .as_ref()
            .map_or(AnimationStatus::Dismissed, |inner| inner.status())
    }

    fn add_listener(&self, listener: AnimationListener) {
        self.listeners.add(listener);
    }

    fn remove_listener(&self, listener: &AnimationListener) {
        self.listeners.remove(listener);
    }
}

/// Upstream `ReverseAnimation`: the same value, time flowing the other
/// way -- completed becomes dismissed, forward becomes reverse.
pub struct ReverseAnimation {
    parent: Rc<dyn Animation>,
}

impl ReverseAnimation {
    pub fn new(parent: Rc<dyn Animation>) -> ReverseAnimation {
        ReverseAnimation { parent }
    }
}

impl Animation for ReverseAnimation {
    fn value(&self) -> f32 {
        1.0 - self.parent.value()
    }

    fn status(&self) -> AnimationStatus {
        match self.parent.status() {
            AnimationStatus::Dismissed => AnimationStatus::Completed,
            AnimationStatus::Forward => AnimationStatus::Reverse,
            AnimationStatus::Reverse => AnimationStatus::Forward,
            AnimationStatus::Completed => AnimationStatus::Dismissed,
        }
    }

    fn add_listener(&self, _listener: AnimationListener) {}
    fn remove_listener(&self, _listener: &AnimationListener) {}
}

/// Upstream `CurvedAnimation`: the parent's value through a curve, with
/// the curve's own flipping at the half and the ends clamped.
pub struct CurvedAnimation {
    parent: Rc<dyn Animation>,
    curve: Curve,
    reverse_curve: Option<Curve>,
}

impl CurvedAnimation {
    pub fn new(parent: Rc<dyn Animation>, curve: Curve) -> CurvedAnimation {
        CurvedAnimation {
            parent,
            curve,
            reverse_curve: None,
        }
    }

    pub fn with_reverse_curve(mut self, curve: Curve) -> CurvedAnimation {
        self.reverse_curve = Some(curve);
        self
    }
}

impl Animation for CurvedAnimation {
    fn value(&self) -> f32 {
        let t = self.parent.value();
        let curve = match (self.parent.status(), self.reverse_curve) {
            (AnimationStatus::Reverse, Some(reverse)) => reverse,
            _ => self.curve,
        };
        // Outside 0..1 the curve clamps to its ends, exactly as
        // `CurvedAnimation.transform` does.
        if t <= 0.0 {
            return curve.transform(0.0);
        }
        if t >= 1.0 {
            return curve.transform(1.0);
        }
        curve.transform(t)
    }

    fn status(&self) -> AnimationStatus {
        self.parent.status()
    }

    fn add_listener(&self, _listener: AnimationListener) {}
    fn remove_listener(&self, _listener: &AnimationListener) {}
}

/// Upstream `AnimationMean`: the average of two.
pub struct AnimationMean {
    left: Rc<dyn Animation>,
    right: Rc<dyn Animation>,
}

impl AnimationMean {
    pub fn new(left: Rc<dyn Animation>, right: Rc<dyn Animation>) -> AnimationMean {
        AnimationMean { left, right }
    }
}

impl Animation for AnimationMean {
    fn value(&self) -> f32 {
        (self.left.value() + self.right.value()) / 2.0
    }

    fn status(&self) -> AnimationStatus {
        self.left.status()
    }

    fn add_listener(&self, _listener: AnimationListener) {}
    fn remove_listener(&self, _listener: &AnimationListener) {}
}

/// Upstream `AnimationMax`: the larger of two.
pub struct AnimationMax {
    left: Rc<dyn Animation>,
    right: Rc<dyn Animation>,
}

impl AnimationMax {
    pub fn new(left: Rc<dyn Animation>, right: Rc<dyn Animation>) -> AnimationMax {
        AnimationMax { left, right }
    }
}

impl Animation for AnimationMax {
    fn value(&self) -> f32 {
        self.left.value().max(self.right.value())
    }

    fn status(&self) -> AnimationStatus {
        if self.left.value() > self.right.value() {
            self.left.status()
        } else {
            self.right.status()
        }
    }

    fn add_listener(&self, _listener: AnimationListener) {}
    fn remove_listener(&self, _listener: &AnimationListener) {}
}

/// Upstream `AnimationMin`: the smaller of two.
pub struct AnimationMin {
    left: Rc<dyn Animation>,
    right: Rc<dyn Animation>,
}

impl AnimationMin {
    pub fn new(left: Rc<dyn Animation>, right: Rc<dyn Animation>) -> AnimationMin {
        AnimationMin { left, right }
    }
}

impl Animation for AnimationMin {
    fn value(&self) -> f32 {
        self.left.value().min(self.right.value())
    }

    fn status(&self) -> AnimationStatus {
        if self.left.value() < self.right.value() {
            self.left.status()
        } else {
            self.right.status()
        }
    }

    fn add_listener(&self, _listener: AnimationListener) {}
    fn remove_listener(&self, _listener: &AnimationListener) {}
}

// -- Animatable and the tween remainder (upstream tween.dart) -----------------------

/// Upstream `Animatable`: anything that can map a double to a value. A
/// `Tween` is the two-ended one; a curve or a chained sequence are the
/// others.
pub trait Animatable {
    type Output;

    fn transform(&self, t: f32) -> Self::Output;
}

// Upstream `Animatable.chain` is spelled through
// `ChainedAnimatable::evaluate` here: inner first, outer of that.

/// Upstream `_ChainedEvaluation`: the inner animatable's value feeds the
/// outer's parameter. Rust's associated types make the generic spelling
/// awkward; the working chain composes two animatables whose types the
/// caller names.
pub struct ChainedAnimatable<I, O> {
    pub inner_output: std::marker::PhantomData<I>,
    pub outer_output: std::marker::PhantomData<O>,
}

impl<I, O> ChainedAnimatable<I, O> {
    /// Upstream `Animatable.chain`'s arithmetic: inner first, outer of
    /// that.
    pub fn evaluate<A, B>(inner: &A, outer: &B, t: f32) -> O
    where
        A: Animatable<Output = f32>,
        B: Animatable<Output = O>,
    {
        outer.transform(inner.transform(t))
    }
}

/// Upstream `CurveTween`: the curve as an animatable.
#[derive(Clone, Copy)]
pub struct CurveTween {
    pub curve: Curve,
}

impl Animatable for CurveTween {
    type Output = f32;

    fn transform(&self, t: f32) -> f32 {
        self.curve.transform(t)
    }
}

/// Upstream `ReverseTween`: the tween read back to front.
#[derive(Clone, Copy)]
pub struct ReverseTween<T: Tween> {
    pub tween: T,
}

impl<T: Tween> Animatable for ReverseTween<T> {
    type Output = T::Output;

    fn transform(&self, t: f32) -> T::Output {
        self.tween.lerp(1.0 - t)
    }
}

/// Upstream `StepTween`: a tween that snaps to whole steps.
#[derive(Clone, Copy)]
pub struct StepTween {
    pub begin: f32,
    pub end: f32,
}

impl Animatable for StepTween {
    type Output = i32;

    fn transform(&self, t: f32) -> i32 {
        let value = self.begin + (self.end - self.begin) * t;
        // Dart's lerpDouble followed by .round(); the half rounds away
        // from zero there.
        value.round() as i32
    }
}

/// Upstream `IntTween`.
#[derive(Clone, Copy)]
pub struct IntTween {
    pub begin: i32,
    pub end: i32,
}

impl Animatable for IntTween {
    type Output = i32;

    fn transform(&self, t: f32) -> i32 {
        (self.begin as f32 + (self.end as f32 - self.begin as f32) * t).round() as i32
    }
}

/// Upstream `SizeTween`.
#[derive(Clone, Copy)]
pub struct SizeTween {
    pub begin: (f32, f32),
    pub end: (f32, f32),
}

impl Animatable for SizeTween {
    type Output = (f32, f32);

    fn transform(&self, t: f32) -> (f32, f32) {
        (
            self.begin.0 + (self.end.0 - self.begin.0) * t,
            self.begin.1 + (self.end.1 - self.begin.1) * t,
        )
    }
}

/// Upstream `RectTween`.
#[derive(Clone, Copy)]
pub struct RectTween {
    pub begin: crate::engine::Rect,
    pub end: crate::engine::Rect,
}

impl Animatable for RectTween {
    type Output = crate::engine::Rect;

    fn transform(&self, t: f32) -> crate::engine::Rect {
        crate::engine::Rect::ltrb(
            self.begin.left + (self.end.left - self.begin.left) * t,
            self.begin.top + (self.end.top - self.begin.top) * t,
            self.begin.right + (self.end.right - self.begin.right) * t,
            self.begin.bottom + (self.end.bottom - self.begin.bottom) * t,
        )
    }
}

/// Upstream `ConstantTween`: every t answers the same value.
#[derive(Clone, Copy)]
pub struct ConstantTween<T: Copy> {
    pub value: T,
}

impl<T: Copy> Animatable for ConstantTween<T> {
    type Output = T;

    fn transform(&self, _t: f32) -> T {
        self.value
    }
}

// -- Tween sequences (upstream tween_sequence.dart) ---------------------------------

/// Upstream `TweenSequenceItem`: one weight's worth of one tween.
pub struct TweenSequenceItem<T: Tween> {
    pub tween: Option<T>,
    pub weight: f32,
}

impl<T: Tween> TweenSequenceItem<T> {
    /// Upstream `TweenSequenceItem.tween`.
    pub fn tween(tween: T, weight: f32) -> TweenSequenceItem<T> {
        TweenSequenceItem {
            tween: Some(tween),
            weight,
        }
    }

    /// Upstream `TweenSequenceItem.weight`-only: a gap.
    pub fn gap(weight: f32) -> TweenSequenceItem<T> {
        TweenSequenceItem {
            tween: None,
            weight,
        }
    }
}

/// Upstream `TweenSequence`: the timeline split into weighted segments,
/// each a tween over its own local 0..1.
pub struct TweenSequence<T: Tween + Clone> {
    items: Vec<TweenSequenceItem<T>>,
    total_weight: f32,
}

impl<T: Tween + Clone> TweenSequence<T> {
    pub fn new(items: Vec<TweenSequenceItem<T>>) -> TweenSequence<T> {
        let total_weight = items.iter().map(|item| item.weight).sum();
        TweenSequence {
            items,
            total_weight,
        }
    }

    /// The tween's answer and the item's local t, upstream's
    /// `_evaluate`.
    fn locate(&self, t: f32) -> (Option<&T>, f32) {
        let position = t.clamp(0.0, 1.0) * self.total_weight;
        let mut covered = 0.0;
        for item in &self.items {
            if position <= covered + item.weight {
                let local = if item.weight == 0.0 {
                    1.0
                } else {
                    ((position - covered) / item.weight).clamp(0.0, 1.0)
                };
                return (item.tween.as_ref(), local);
            }
            covered += item.weight;
        }
        (None, 1.0)
    }
}

impl<T: Tween + Clone> Animatable for TweenSequence<T> {
    type Output = T::Output;

    fn transform(&self, t: f32) -> T::Output {
        // The gap case needs an output value; the port's contract is that
        // a sequence holds no gaps unless the caller made the tween types
        // agree on a default. A gap answers the last tween's end, the same
        // value the timeline holds through it.
        let (tween, local) = self.locate(t);
        match tween {
            Some(tween) => tween.lerp(local),
            None => {
                unreachable!("a sequence with a gap needs its tween's end; hold a tween instead")
            }
        }
    }
}

/// Upstream `FlippedTweenSequence`: the sequence turned through half a turn.
///
/// # Both axes, not just time
///
/// `1 - super.transform(1 - t)`. The inner `1 - t` reads the sequence back to
/// front; the **outer `1 -`** turns its values upside down as well, and the
/// two together are a rotation rather than a reflection. Upstream's own
/// sentence is "flips the tween both horizontally and vertically".
///
/// Reversing only time is the natural half to write and gives an animation
/// that plays its segments in reverse order while each one still runs the
/// direction it always did -- a rewind that travels the wrong path.
///
/// # It is `f32`-valued because the vertical flip needs a number
///
/// Upstream declares `extends TweenSequence<double>` and says the result "has
/// to be a double between 0.0 and 1.0". `1 - x` has no meaning for a colour or
/// an offset, and no sense at all outside the unit interval -- flipping a
/// sequence running 0 to 20 gives values down at -19.
///
/// This port was generic over the tween's output, which is exactly what made
/// the outer `1 -` impossible to write, so it had been left out. The
/// restriction is upstream's and it is what makes the operation definable.
pub struct FlippedTweenSequence<T: Tween<Output = f32> + Clone> {
    pub sequence: TweenSequence<T>,
}

impl<T: Tween<Output = f32> + Clone> Animatable for FlippedTweenSequence<T> {
    type Output = f32;

    fn transform(&self, t: f32) -> f32 {
        1.0 - self.sequence.transform(1.0 - t)
    }
}

/// Upstream `AnimationStyle`: the curve and duration a widget's own
/// animation takes when the caller did not say (material's M3 default).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationStyle {
    pub curve: Option<Curve>,
    pub reverse_curve: Option<Curve>,
    pub duration: Option<Duration>,
    pub reverse_duration: Option<Duration>,
}

impl AnimationStyle {
    pub const NO_ANIMATION: AnimationStyle = AnimationStyle {
        curve: None,
        reverse_curve: None,
        duration: Some(Duration::from_millis(0)),
        reverse_duration: Some(Duration::from_millis(0)),
    };

    pub fn at_most(&self, other: &AnimationStyle) -> AnimationStyle {
        AnimationStyle {
            curve: self.curve.or(other.curve),
            reverse_curve: self.reverse_curve.or(other.reverse_curve),
            duration: self.duration.or(other.duration),
            reverse_duration: self.reverse_duration.or(other.reverse_duration),
        }
    }
}

impl Default for AnimationStyle {
    fn default() -> AnimationStyle {
        AnimationStyle {
            curve: None,
            reverse_curve: None,
            duration: None,
            reverse_duration: None,
        }
    }
}

#[cfg(test)]
mod animation_graph_tests {
    use super::*;

    struct FixedAnimation {
        value: f32,
        status: AnimationStatus,
    }
    impl Animation for FixedAnimation {
        fn value(&self) -> f32 {
            self.value
        }
        fn status(&self) -> AnimationStatus {
            self.status
        }
        fn add_listener(&self, _listener: AnimationListener) {}
        fn remove_listener(&self, _listener: &AnimationListener) {}
    }

    fn fixed(value: f32, status: AnimationStatus) -> Rc<dyn Animation> {
        Rc::new(FixedAnimation { value, status })
    }

    #[test]
    fn a_reversed_animation_flips_value_and_status() {
        let forward = fixed(0.25, AnimationStatus::Forward);
        let reversed = ReverseAnimation::new(forward);
        assert_eq!(reversed.value(), 0.75);
        assert_eq!(reversed.status(), AnimationStatus::Reverse);

        let dismissed = fixed(0.0, AnimationStatus::Dismissed);
        assert_eq!(
            ReverseAnimation::new(dismissed).status(),
            AnimationStatus::Completed
        );
    }

    #[test]
    fn a_curved_animation_bends_and_clamps() {
        let parent = fixed(0.5, AnimationStatus::Forward);
        let curved = CurvedAnimation::new(parent, Curve::Cubic(0.0, 0.0, 1.0, 1.0));
        // A straight cubic at 0.5 is 0.5.
        assert!((curved.value() - 0.5).abs() < 1e-4);

        // Outside 0..1 the curve clamps to its ends.
        let before = CurvedAnimation::new(
            fixed(-0.5, AnimationStatus::Forward),
            Curve::Cubic(0.0, 0.0, 1.0, 1.0),
        );
        assert_eq!(before.value(), 0.0);
        let after = CurvedAnimation::new(
            fixed(1.5, AnimationStatus::Forward),
            Curve::Cubic(0.0, 0.0, 1.0, 1.0),
        );
        assert_eq!(after.value(), 1.0);
    }

    #[test]
    fn mean_max_and_min_combine_two() {
        let left = fixed(0.25, AnimationStatus::Forward);
        let right = fixed(0.75, AnimationStatus::Forward);
        assert_eq!(
            AnimationMean::new(Rc::clone(&left), Rc::clone(&right)).value(),
            0.5
        );
        assert_eq!(
            AnimationMax::new(Rc::clone(&left), Rc::clone(&right)).value(),
            0.75
        );
        assert_eq!(AnimationMin::new(left, right).value(), 0.25);
    }

    #[test]
    fn a_proxy_animation_announces_status_across_the_swap() {
        let proxy = Rc::new(ProxyAnimation::new());
        let heard = Rc::new(std::cell::Cell::new(0));
        let listener = AnimationListener {
            on_value: Rc::new(|| {}),
            on_status: {
                let heard = Rc::clone(&heard);
                Some(Rc::new(move |_status| heard.set(heard.get() + 1)))
            },
        };
        proxy.add_listener(listener);
        proxy.set_parent(Some(fixed(0.5, AnimationStatus::Completed)));
        assert_eq!(heard.get(), 1);
        // Same status again: silent.
        proxy.set_parent(Some(fixed(0.8, AnimationStatus::Completed)));
        assert_eq!(heard.get(), 1);
        proxy.set_parent(Some(fixed(0.8, AnimationStatus::Forward)));
        assert_eq!(heard.get(), 2);
    }

    #[test]
    fn a_tween_sequence_walks_its_weights() {
        let sequence = TweenSequence::new(vec![
            TweenSequenceItem::tween(
                FloatTween {
                    begin: 0.0,
                    end: 10.0,
                },
                1.0,
            ),
            TweenSequenceItem::tween(
                FloatTween {
                    begin: 10.0,
                    end: 20.0,
                },
                3.0,
            ),
        ]);
        // The first quarter is the first tween; the rest, the second.
        assert_eq!(Animatable::transform(&sequence, 0.0), 0.0);
        assert_eq!(Animatable::transform(&sequence, 0.25), 10.0);
        assert_eq!(Animatable::transform(&sequence, 0.625), 15.0);
        assert_eq!(Animatable::transform(&sequence, 1.0), 20.0);
    }

    #[test]
    fn a_flipped_sequence_turns_through_half_a_turn_rather_than_playing_backwards() {
        // A sequence over the unit interval, which is the only domain the
        // vertical half of the flip makes sense on: a quarter of the time
        // spent going nowhere, then three quarters climbing to 1.
        let held_then_climbing = || {
            TweenSequence::new(vec![
                TweenSequenceItem::tween(
                    FloatTween {
                        begin: 0.0,
                        end: 0.0,
                    },
                    1.0,
                ),
                TweenSequenceItem::tween(
                    FloatTween {
                        begin: 0.0,
                        end: 1.0,
                    },
                    3.0,
                ),
            ])
        };
        let plain = held_then_climbing();
        assert_eq!(Animatable::transform(&plain, 0.125), 0.0, "held");
        assert_eq!(Animatable::transform(&plain, 1.0), 1.0);

        let flipped = FlippedTweenSequence {
            sequence: held_then_climbing(),
        };

        // Both ends still land where an animation's ends must land. Reversing
        // time alone would put 1 at t=0 and 0 at t=1 -- an animation that
        // starts finished.
        assert_eq!(Animatable::transform(&flipped, 0.0), 0.0);
        assert_eq!(Animatable::transform(&flipped, 1.0), 1.0);

        // The pause has moved to the end, which is the whole point: the
        // shape is reversed, the direction of travel is not.
        assert_eq!(
            Animatable::transform(&flipped, 0.875),
            1.0,
            "held at its destination for the last eighth"
        );

        // And it is the rotation, not either half on its own.
        for step in 0..=8 {
            let t = step as f32 / 8.0;
            let want = 1.0 - Animatable::transform(&plain, 1.0 - t);
            assert!(
                (Animatable::transform(&flipped, t) - want).abs() < 1e-5,
                "at {t}"
            );
        }
    }

    #[test]
    fn a_step_tween_snaps() {
        let stepped = StepTween {
            begin: 0.0,
            end: 3.0,
        };
        assert_eq!(Animatable::transform(&stepped, 0.4), 1);
        assert_eq!(Animatable::transform(&stepped, 0.9), 3);
    }

    #[test]
    fn chained_animatables_evaluate_inner_then_outer() {
        let inner = CurveTween {
            curve: Curve::Linear,
        };
        let outer = FloatTween {
            begin: 0.0,
            end: 100.0,
        };
        assert_eq!(
            ChainedAnimatable::<f32, f32>::evaluate(&inner, &outer, 0.5),
            50.0
        );
    }
}

// -- Changing horses in midstream ---------------------------------------------

/// Which way the two trains have to cross before the hop is allowed.
///
/// Upstream's private `_TrainHoppingMode`, decided once at construction from
/// which train is ahead. It is not re-decided later, and that is the point: the
/// mode is the answer to "which way round were they when we started", and a hop
/// happens the moment they meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainHoppingMode {
    /// The next train started **below** the current one, so hop when it catches
    /// up from below.
    Minimize,
    /// The next train started **above**, so hop when it comes down to meet.
    Maximize,
}

/// Upstream `TrainHoppingAnimation`: follows one animation until a second one
/// crosses it, then follows that one instead.
///
/// # What it is for
///
/// A value that must not jump. Upstream's use is a route transition whose
/// driving animation is replaced mid-flight -- switching to the new one
/// immediately would show a visible step, so this waits until the two agree on
/// a value and switches *there*, where the change costs nothing.
///
/// # It hops once, and then it is an ordinary animation
///
/// After the hop the next train is dropped and there is nothing left to hop to.
/// Upstream never takes a third; a caller that wants another chain builds
/// another hopper.
///
/// # The degenerate case is handled at construction
///
/// If the two trains already agree, upstream switches immediately in the
/// constructor and never records a mode at all -- so the mode is `None` exactly
/// when there is no next train, which is what its final assert says.
pub struct TrainHoppingAnimation {
    current: RefCell<Rc<dyn Animation>>,
    next: RefCell<Option<Rc<dyn Animation>>>,
    mode: Cell<Option<TrainHoppingMode>>,
    last_value: Cell<Option<f32>>,
    last_status: Cell<Option<AnimationStatus>>,
    on_switched_train: RefCell<Option<Rc<dyn Fn()>>>,
    listeners: AnimationListeners,
}

impl TrainHoppingAnimation {
    /// Upstream's constructor, including the two things it settles before
    /// anything is listening.
    pub fn new(
        current: Rc<dyn Animation>,
        next: Option<Rc<dyn Animation>>,
    ) -> Rc<TrainHoppingAnimation> {
        let mut current = current;
        let mut next = next;
        let mut mode = None;
        if let Some(candidate) = next.clone() {
            if current.value() == candidate.value() {
                // Already met: hop now and never record a mode.
                current = candidate;
                next = None;
            } else if current.value() > candidate.value() {
                mode = Some(TrainHoppingMode::Maximize);
            } else {
                mode = Some(TrainHoppingMode::Minimize);
            }
        }
        debug_assert!(
            mode.is_some() || next.is_none(),
            "a next train without a mode has no rule for when to hop"
        );
        Rc::new(TrainHoppingAnimation {
            current: RefCell::new(current),
            next: RefCell::new(next),
            mode: Cell::new(mode),
            last_value: Cell::new(None),
            last_status: Cell::new(None),
            on_switched_train: RefCell::new(None),
            listeners: AnimationListeners::new(),
        })
    }

    pub fn with_on_switched_train(self: &Rc<Self>, on_switched: impl Fn() + 'static) -> Rc<Self> {
        *self.on_switched_train.borrow_mut() = Some(Rc::new(on_switched));
        Rc::clone(self)
    }

    /// Upstream's `currentTrain`. `None` after [`dispose`](Self::dispose).
    pub fn current_train(&self) -> Rc<dyn Animation> {
        Rc::clone(&self.current.borrow())
    }

    pub fn mode(&self) -> Option<TrainHoppingMode> {
        self.mode.get()
    }

    pub fn has_next_train(&self) -> bool {
        self.next.borrow().is_some()
    }

    /// Upstream's `_valueChangeHandler`, which is where everything happens.
    ///
    /// Both trains' value listeners run this, and the order inside it is
    /// upstream's and matters: hop first, *then* read the value, so the value
    /// reported is the new train's. Reading first would report the old train's
    /// last value one final time after the switch.
    ///
    /// The `onSwitchedTrain` callback fires **after** the value listeners, not
    /// at the moment of the hop. A listener told "we switched" before the value
    /// went out would look at a value belonging to the old train.
    pub fn pump(&self) {
        let mut hopped = false;
        let next = self.next.borrow().clone();
        if let Some(next) = next {
            let mode = self.mode.get().expect("a next train always has a mode");
            let current_value = self.current.borrow().value();
            let hop = match mode {
                TrainHoppingMode::Minimize => next.value() <= current_value,
                TrainHoppingMode::Maximize => next.value() >= current_value,
            };
            if hop {
                *self.current.borrow_mut() = next;
                *self.next.borrow_mut() = None;
                hopped = true;
                let status = self.current.borrow().status();
                self.note_status(status);
            }
        }
        let value = self.current.borrow().value();
        if self.last_value.get() != Some(value) {
            self.listeners.notify_value();
            self.last_value.set(Some(value));
        }
        if hopped {
            let callback = self.on_switched_train.borrow().clone();
            if let Some(callback) = callback {
                callback();
            }
        }
    }

    /// Upstream's `_statusChangeHandler`, whose whole content is the guard: a
    /// status that has not changed is not announced.
    ///
    /// It is needed because the hop calls this by hand with the new train's
    /// status, and that will usually be the status the old train already had --
    /// a listener told the animation started forward twice would, for instance,
    /// play an entry sound twice.
    pub fn note_status(&self, status: AnimationStatus) {
        if self.last_status.get() != Some(status) {
            self.listeners.notify_status(status);
            self.last_status.set(Some(status));
        }
    }

    /// Upstream's `dispose`, which lets go of both trains and every listener.
    /// After it the object is not usable, as upstream's doc says in as many
    /// words.
    pub fn dispose(&self) {
        *self.next.borrow_mut() = None;
        self.listeners.clear();
    }
}

impl Animation for TrainHoppingAnimation {
    fn value(&self) -> f32 {
        self.current.borrow().value()
    }

    fn status(&self) -> AnimationStatus {
        self.current.borrow().status()
    }

    fn add_listener(&self, listener: AnimationListener) {
        self.listeners.add(listener);
    }

    fn remove_listener(&self, listener: &AnimationListener) {
        self.listeners.remove(listener);
    }
}

#[cfg(test)]
mod train_hopping_tests {
    use super::*;

    /// An animation whose value the test sets by hand.
    struct Dial {
        value: Cell<f32>,
        status: Cell<AnimationStatus>,
        listeners: AnimationListeners,
    }

    impl Dial {
        fn at(value: f32) -> Rc<Dial> {
            Rc::new(Dial {
                value: Cell::new(value),
                status: Cell::new(AnimationStatus::Forward),
                listeners: AnimationListeners::new(),
            })
        }

        fn set(&self, value: f32) {
            self.value.set(value);
            self.listeners.notify_value();
        }
    }

    impl Animation for Dial {
        fn value(&self) -> f32 {
            self.value.get()
        }

        fn status(&self) -> AnimationStatus {
            self.status.get()
        }

        fn add_listener(&self, listener: AnimationListener) {
            self.listeners.add(listener);
        }

        fn remove_listener(&self, listener: &AnimationListener) {
            self.listeners.remove(listener);
        }
    }

    #[test]
    fn the_mode_is_decided_by_which_train_is_ahead() {
        let behind = TrainHoppingAnimation::new(Dial::at(0.8), Some(Dial::at(0.2)));
        assert_eq!(
            behind.mode(),
            Some(TrainHoppingMode::Maximize),
            "next is below"
        );

        let ahead = TrainHoppingAnimation::new(Dial::at(0.2), Some(Dial::at(0.8)));
        assert_eq!(
            ahead.mode(),
            Some(TrainHoppingMode::Minimize),
            "next is above"
        );
    }

    #[test]
    fn trains_that_already_agree_hop_before_anyone_is_listening() {
        // And record no mode at all, because there is nothing left to wait for.
        let next = Dial::at(0.5);
        let hopper =
            TrainHoppingAnimation::new(Dial::at(0.5), Some(Rc::clone(&next) as Rc<dyn Animation>));
        assert_eq!(hopper.mode(), None);
        assert!(!hopper.has_next_train());
        assert!(Rc::ptr_eq(
            &hopper.current_train(),
            &(Rc::clone(&next) as Rc<dyn Animation>)
        ));
    }

    #[test]
    fn one_train_alone_has_no_mode_and_nothing_to_do() {
        let hopper = TrainHoppingAnimation::new(Dial::at(0.3), None);
        assert_eq!(hopper.mode(), None);
        assert_eq!(hopper.value(), 0.3);
        hopper.pump();
        assert_eq!(hopper.value(), 0.3);
    }

    #[test]
    fn it_hops_the_moment_the_trains_meet() {
        // The point of the whole class: switching before they meet would show a
        // visible step in whatever the value drives.
        let current = Dial::at(0.2);
        let next = Dial::at(0.8);
        let hopper = TrainHoppingAnimation::new(
            Rc::clone(&current) as Rc<dyn Animation>,
            Some(Rc::clone(&next) as Rc<dyn Animation>),
        );

        next.value.set(0.5);
        hopper.pump();
        assert!(hopper.has_next_train(), "0.5 is still above 0.2");
        assert_eq!(
            hopper.value(),
            0.2,
            "and the value is still the old train's"
        );

        next.value.set(0.2);
        hopper.pump();
        assert!(!hopper.has_next_train(), "they met");
        assert_eq!(hopper.value(), 0.2, "at the same value, so nothing jumped");

        // And from here it follows the new train, not the old.
        current.value.set(0.9);
        next.value.set(0.4);
        hopper.pump();
        assert_eq!(hopper.value(), 0.4);
    }

    #[test]
    fn maximize_waits_for_the_next_train_to_come_down() {
        let next = Dial::at(0.2);
        let hopper =
            TrainHoppingAnimation::new(Dial::at(0.8), Some(Rc::clone(&next) as Rc<dyn Animation>));
        next.value.set(0.7);
        hopper.pump();
        assert!(hopper.has_next_train(), "0.7 is still below 0.8");

        next.value.set(0.9);
        hopper.pump();
        assert!(!hopper.has_next_train());
        assert_eq!(hopper.value(), 0.9);
    }

    #[test]
    fn it_hops_once_and_then_it_is_an_ordinary_animation() {
        let next = Dial::at(0.8);
        let hopper =
            TrainHoppingAnimation::new(Dial::at(0.2), Some(Rc::clone(&next) as Rc<dyn Animation>));
        next.value.set(0.1);
        hopper.pump();
        assert!(!hopper.has_next_train());

        // Whatever the old train does now, there is nothing to hop back to.
        for value in [0.0, 0.5, 1.0] {
            next.value.set(value);
            hopper.pump();
            assert_eq!(hopper.value(), value);
        }
    }

    #[test]
    fn listeners_hear_the_new_trains_value_and_not_the_old_ones_again() {
        // Which is why the hop happens before the value is read.
        let heard = Rc::new(RefCell::new(Vec::new()));
        let next = Dial::at(0.8);
        let hopper =
            TrainHoppingAnimation::new(Dial::at(0.2), Some(Rc::clone(&next) as Rc<dyn Animation>));
        let recorder = Rc::clone(&heard);
        let reader = Rc::clone(&hopper);
        hopper.add_listener(AnimationListener {
            on_value: Rc::new(move || recorder.borrow_mut().push(reader.value())),
            on_status: None,
        });

        next.value.set(0.1);
        hopper.pump();
        assert_eq!(*heard.borrow(), vec![0.1], "the new train's, at once");
    }

    #[test]
    fn the_switched_callback_comes_after_the_value_went_out() {
        // A listener told "we switched" before the value went out would be
        // looking at a value belonging to the old train.
        let order = Rc::new(RefCell::new(Vec::new()));
        let next = Dial::at(0.8);
        let hopper =
            TrainHoppingAnimation::new(Dial::at(0.2), Some(Rc::clone(&next) as Rc<dyn Animation>));
        let switched = Rc::clone(&order);
        let hopper = hopper.with_on_switched_train(move || switched.borrow_mut().push("switched"));
        let valued = Rc::clone(&order);
        hopper.add_listener(AnimationListener {
            on_value: Rc::new(move || valued.borrow_mut().push("value")),
            on_status: None,
        });

        next.value.set(0.1);
        hopper.pump();
        assert_eq!(*order.borrow(), vec!["value", "switched"]);
    }

    #[test]
    fn the_switched_callback_does_not_fire_without_a_hop() {
        let order = Rc::new(RefCell::new(0));
        let current = Dial::at(0.2);
        let hopper = TrainHoppingAnimation::new(
            Rc::clone(&current) as Rc<dyn Animation>,
            Some(Dial::at(0.8)),
        );
        let counter = Rc::clone(&order);
        let hopper = hopper.with_on_switched_train(move || *counter.borrow_mut() += 1);
        current.value.set(0.3);
        hopper.pump();
        assert_eq!(*order.borrow(), 0);
    }

    #[test]
    fn a_value_that_did_not_change_is_not_announced() {
        let heard = Rc::new(RefCell::new(0));
        let current = Dial::at(0.4);
        let hopper = TrainHoppingAnimation::new(Rc::clone(&current) as Rc<dyn Animation>, None);
        let counter = Rc::clone(&heard);
        hopper.add_listener(AnimationListener {
            on_value: Rc::new(move || *counter.borrow_mut() += 1),
            on_status: None,
        });

        hopper.pump();
        assert_eq!(*heard.borrow(), 1, "the first read is news");
        hopper.pump();
        hopper.pump();
        assert_eq!(*heard.borrow(), 1, "and the same value is not");
        current.value.set(0.5);
        hopper.pump();
        assert_eq!(*heard.borrow(), 2);
    }

    #[test]
    fn a_status_that_did_not_change_is_not_announced_either() {
        // The hop calls the status handler by hand with the new train's status,
        // which will usually be the status the old train already had.
        let heard = Rc::new(RefCell::new(Vec::new()));
        let hopper = TrainHoppingAnimation::new(Dial::at(0.5), None);
        let recorder = Rc::clone(&heard);
        hopper.add_listener(AnimationListener {
            on_value: Rc::new(|| {}),
            on_status: Some(Rc::new(move |status| recorder.borrow_mut().push(status))),
        });

        hopper.note_status(AnimationStatus::Forward);
        hopper.note_status(AnimationStatus::Forward);
        assert_eq!(*heard.borrow(), vec![AnimationStatus::Forward], "once");
        hopper.note_status(AnimationStatus::Completed);
        assert_eq!(heard.borrow().len(), 2);
    }

    #[test]
    fn a_hop_between_trains_of_the_same_status_says_nothing() {
        let heard = Rc::new(RefCell::new(0));
        let next = Dial::at(0.8);
        let hopper =
            TrainHoppingAnimation::new(Dial::at(0.2), Some(Rc::clone(&next) as Rc<dyn Animation>));
        hopper.note_status(AnimationStatus::Forward);
        let counter = Rc::clone(&heard);
        hopper.add_listener(AnimationListener {
            on_value: Rc::new(|| {}),
            on_status: Some(Rc::new(move |_| *counter.borrow_mut() += 1)),
        });

        next.value.set(0.1);
        hopper.pump();
        assert_eq!(*heard.borrow(), 0, "both trains were going forward");
    }

    #[test]
    fn disposing_lets_go_of_the_next_train_and_every_listener() {
        let heard = Rc::new(RefCell::new(0));
        let current = Dial::at(0.2);
        let hopper = TrainHoppingAnimation::new(
            Rc::clone(&current) as Rc<dyn Animation>,
            Some(Dial::at(0.8)),
        );
        let counter = Rc::clone(&heard);
        hopper.add_listener(AnimationListener {
            on_value: Rc::new(move || *counter.borrow_mut() += 1),
            on_status: None,
        });

        hopper.dispose();
        assert!(!hopper.has_next_train());
        current.value.set(0.9);
        hopper.pump();
        assert_eq!(*heard.borrow(), 0);
    }
}

#[cfg(test)]
mod animation_style_direction_tests {
    use super::*;

    fn near() -> AnimationStyle {
        AnimationStyle {
            curve: Some(Curve::Linear),
            reverse_curve: Some(Curve::EASE_IN),
            duration: Some(Duration::from_millis(100)),
            reverse_duration: Some(Duration::from_millis(200)),
        }
    }

    fn far() -> AnimationStyle {
        AnimationStyle {
            curve: Some(Curve::EASE_OUT),
            reverse_curve: Some(Curve::EASE_IN_OUT),
            duration: Some(Duration::from_millis(300)),
            reverse_duration: Some(Duration::from_millis(400)),
        }
    }

    #[test]
    fn the_nearer_style_wins_every_field_and_not_just_the_first() {
        // Written with both sides fully set: a field set on one side only
        // shows that *something* comes through and not which side it came
        // from. `tools/order_sweep.py` found all four of these by swapping the
        // sides and watching nothing fail.
        let merged = near().at_most(&far());
        assert_eq!(merged.curve, Some(Curve::Linear));
        assert_eq!(merged.reverse_curve, Some(Curve::EASE_IN));
        assert_eq!(merged.duration, Some(Duration::from_millis(100)));
        assert_eq!(merged.reverse_duration, Some(Duration::from_millis(200)));
    }

    #[test]
    fn the_other_way_round_gives_the_other_answer_everywhere() {
        let merged = far().at_most(&near());
        assert_eq!(merged.curve, Some(Curve::EASE_OUT));
        assert_eq!(merged.duration, Some(Duration::from_millis(300)));
        assert_eq!(merged.reverse_duration, Some(Duration::from_millis(400)));
    }

    #[test]
    fn a_field_only_the_far_style_has_still_comes_through() {
        let sparse = AnimationStyle {
            curve: Some(Curve::Linear),
            ..AnimationStyle::default()
        };
        let merged = sparse.at_most(&far());
        assert_eq!(merged.curve, Some(Curve::Linear));
        assert_eq!(merged.duration, Some(Duration::from_millis(300)));
    }
}

#[cfg(test)]
mod animation_behavior_tests {
    use super::{AnimationBehavior, Controller};
    use std::time::Duration;

    #[test]
    fn only_the_normal_behavior_listens_to_the_setting() {
        assert!(AnimationBehavior::Normal.enables_animations(false));
        assert!(!AnimationBehavior::Normal.enables_animations(true));
        // Preserve ignores the argument, which is the whole of what it is for.
        assert!(AnimationBehavior::Preserve.enables_animations(false));
        assert!(AnimationBehavior::Preserve.enables_animations(true));
    }

    #[test]
    fn a_shortened_animation_is_shortened_and_not_skipped() {
        // Five per cent rather than zero, and upstream says why: a zero
        // duration risks an endless loop for an eternally repeating animation,
        // so this limits most of them to a single frame instead.
        assert_eq!(AnimationBehavior::DISABLED_DURATION_SCALE, 0.05);
        assert!(AnimationBehavior::DISABLED_DURATION_SCALE > 0.0);
        assert_eq!(AnimationBehavior::Normal.duration_scale(true), 0.05);
        assert_eq!(AnimationBehavior::Normal.duration_scale(false), 1.0);
        assert_eq!(AnimationBehavior::Preserve.duration_scale(true), 1.0);
    }

    #[test]
    fn and_a_spring_is_thrown_harder_instead_of_run_shorter() {
        // Opposite arithmetic for the same intent: a spring has no duration to
        // divide, so the velocity is multiplied.
        assert_eq!(AnimationBehavior::DISABLED_FLING_VELOCITY_SCALE, 200.0);
        assert_eq!(AnimationBehavior::Normal.fling_velocity_scale(true), 200.0);
        assert_eq!(AnimationBehavior::Normal.fling_velocity_scale(false), 1.0);
        assert_eq!(AnimationBehavior::Preserve.fling_velocity_scale(true), 1.0);
        // One shrinks and the other grows, so they cannot be the same number
        // reached two ways.
        assert!(AnimationBehavior::Normal.duration_scale(true) < 1.0);
        assert!(AnimationBehavior::Normal.fling_velocity_scale(true) > 1.0);
    }

    #[test]
    fn the_scale_really_reaches_the_clock() {
        // Through `tick`, not through the predicate: a controller told the
        // reader wants no animations covers in one frame what it would have
        // taken twenty to cover.
        let mut ordinary = Controller::new(Duration::from_millis(1000));
        ordinary.forward();
        ordinary.tick(Duration::from_millis(50));
        assert!(
            (ordinary.value() - 0.05).abs() < 1e-5,
            "{}",
            ordinary.value()
        );

        let mut hurried =
            Controller::new(Duration::from_millis(1000)).with_disable_animations(true);
        hurried.forward();
        hurried.tick(Duration::from_millis(50));
        assert!(hurried.value() >= 1.0, "{}", hurried.value());
    }

    #[test]
    fn and_preserve_keeps_its_pace_with_the_setting_on() {
        let mut kept = Controller::new(Duration::from_millis(1000))
            .with_behavior(AnimationBehavior::Preserve)
            .with_disable_animations(true);
        kept.forward();
        kept.tick(Duration::from_millis(50));
        assert!((kept.value() - 0.05).abs() < 1e-5, "{}", kept.value());
        // Which is a different answer from Normal under the same setting --
        // otherwise this test would hold with the behaviour ignored entirely.
        let mut hurried =
            Controller::new(Duration::from_millis(1000)).with_disable_animations(true);
        hurried.forward();
        hurried.tick(Duration::from_millis(50));
        assert_ne!(kept.value(), hurried.value());
    }

    #[test]
    fn a_controller_animates_normally_until_told_otherwise() {
        assert_eq!(
            Controller::new(Duration::from_millis(1)).behavior(),
            AnimationBehavior::Normal
        );
        assert_eq!(AnimationBehavior::default(), AnimationBehavior::Normal);
    }
}
