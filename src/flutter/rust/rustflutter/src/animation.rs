// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Animation: tickers, controllers, curves and tweens.
//!
//! An animation is a value that changes because time passed. Everything here
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

use std::time::Duration;

// -- Curves -------------------------------------------------------------------

/// Bends the flow of time. Every curve maps 0..1 to 0..1, passing through both
/// endpoints -- so an animation always starts where it started and ends where
/// it ends, however strangely it gets there.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Curve {
    #[default]
    Linear,
    /// Decelerating, matching upstream's `Curves.decelerate`.
    Decelerate,
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
            Curve::Cubic(a, b, c, d) => cubic(a, b, c, d, t),
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

/// Interpolates a colour channel by channel, in straight (non-premultiplied)
/// space. Good enough for UI, and wrong for anything with wide gamut ambitions.
#[derive(Clone, Copy, Debug)]
pub struct ColorTween {
    pub begin: crate::engine::Color,
    pub end: crate::engine::Color,
}

impl ColorTween {
    pub const fn new(
        begin: crate::engine::Color,
        end: crate::engine::Color,
    ) -> ColorTween {
        ColorTween { begin, end }
    }
}

impl Tween for ColorTween {
    type Output = crate::engine::Color;

    fn lerp(&self, t: f32) -> crate::engine::Color {
        let mix = |a: u8, b: u8| -> u8 {
            (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
        };
        crate::engine::Color::argb(
            mix(self.begin.alpha(), self.end.alpha()),
            mix(self.begin.red(), self.end.red()),
            mix(self.begin.green(), self.end.green()),
            mix(self.begin.blue(), self.end.blue()),
        )
    }
}

/// Interpolates an offset.
#[derive(Clone, Copy, Debug)]
pub struct OffsetTween {
    pub begin: crate::render::Offset,
    pub end: crate::render::Offset,
}

impl OffsetTween {
    pub const fn new(
        begin: crate::render::Offset,
        end: crate::render::Offset,
    ) -> OffsetTween {
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

/// Turns elapsed time into a value between 0 and 1.
///
/// A controller does not know what frame it is on; it is told, by
/// [`Controller::tick`], how much time has passed. That keeps it testable
/// without a clock and makes a paused animation exactly an animation that is
/// not being ticked.
#[derive(Clone, Debug)]
pub struct Controller {
    duration: Duration,
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

    /// Goes the other way from wherever it is. What a toggle wants: an
    /// interrupted animation continues from where it got to rather than
    /// snapping.
    pub fn toggle(&mut self) {
        match self.direction {
            Direction::Forward => self.reverse(),
            Direction::Reverse => self.forward(),
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

        let step = elapsed.as_secs_f32() / self.duration.as_secs_f32();
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
        self.get(name).map_or(default, |controller| controller.curved())
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
        assert!((ease_in_out.transform(0.5) - 0.5).abs() < 2e-3, "symmetric at the middle");
        assert!(ease_in_out.transform(0.25) < 0.25, "still easing in");
        assert!(ease_in_out.transform(0.75) > 0.75, "already easing out");
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
        assert!(second > first, "later bounces are closer to the ground, not further");
    }

    #[test]
    fn elastic_overshoots_in_both_directions() {
        let samples: Vec<f32> =
            (0..=100).map(|i| Curve::ELASTIC_OUT.transform(i as f32 / 100.0)).collect();
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
        assert!(controller.tick(Duration::from_millis(200)), "the arriving tick counts");
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

    #[test]
    fn looping_wraps_rather_than_stopping() {
        let mut controller =
            Controller::new(Duration::from_millis(100)).with_repeat(Repeat::Loop);
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
        assert!(animations.tick(20_000), "the frame it lands on still has to be drawn");
        assert!(!animations.tick(30_000));
        assert!(!animations.is_running());
    }

    #[test]
    fn reading_a_missing_animation_gives_the_default() {
        let animations = Animations::new();
        assert_eq!(animations.value_or("nope", 0.25), 0.25);
    }
}
