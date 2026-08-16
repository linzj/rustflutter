// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Pointer events and the recognisers that turn them into gestures.
//!
//! The shell hands over a stream of pointer events in physical pixels. This
//! module converts them to logical pixels, hit-tests each one against the
//! render tree that was painted last, and decides which of them add up to a
//! tap or a drag.
//!
//! # Where the callbacks live
//!
//! A widget tree is rebuilt every frame, so a handler cannot be stored beside
//! the widget that declared it -- by the time the pointer arrives, that widget
//! is gone. It lives on the *render object* instead ([`crate::render::
//! RenderPointerRegion`]), which is what hit testing finds, and it is an
//! `Rc<dyn Fn>` rather than a `FnMut` so that finding it does not require a
//! mutable borrow of the tree that is being searched.
//!
//! What the handler mutates is a [`crate::framework::StateHandle`] it captured
//! at build time. That is the whole loop: build captures a handle, the pointer
//! finds the handler, the handler calls `set_state`, the element is marked
//! dirty, the next frame rebuilds that subtree.
//!
//! # What is recognised
//!
//! Tap and drag, with the arbitration rule that matters: a press that moves
//! further than [`TOUCH_SLOP`] stops being a candidate tap and becomes a drag.
//! Upstream this is a `GestureArena` where several recognisers compete and one
//! wins; with two recognisers the arena collapses into a single distance test,
//! and pretending otherwise would be more machinery than decision.

use std::rc::Rc;

use crate::render::{HitTestResult, Offset, RenderBox};

/// How far a pointer may move and still count as a tap, in logical pixels.
/// Upstream's `kTouchSlop`.
pub const TOUCH_SLOP: f32 = 18.0;

/// What happened to a pointer. Mirrors `flutter::PointerData::Change`.
/// What a pointer event carries besides a position.
///
/// The shell sends a scroll as a hover with a signal rather than as its own
/// change, so the signal has to be looked at first or a wheel turn reads as a
/// mouse being moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SignalKind {
    #[default]
    None,
    Scroll,
    ScrollInertiaCancel,
    Scale,
    /// A signal code the shell sent that this build does not know.
    Unknown,
}

impl SignalKind {
    pub fn from_raw(raw: i32) -> SignalKind {
        match raw {
            0 => SignalKind::None,
            1 => SignalKind::Scroll,
            2 => SignalKind::ScrollInertiaCancel,
            3 => SignalKind::Scale,
            _ => SignalKind::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerChange {
    Cancel,
    Add,
    Remove,
    Hover,
    Down,
    Move,
    Up,
    PanZoomStart,
    PanZoomUpdate,
    PanZoomEnd,
    /// A change code the shell sent that this build does not know.
    Unknown,
}

impl PointerChange {
    // Called only from the C ABI in app.rs, which is compiled out under
    // cfg(test) -- see engine_test_stubs.rs for why.
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn from_code(code: i32) -> PointerChange {
        match code {
            0 => PointerChange::Cancel,
            1 => PointerChange::Add,
            2 => PointerChange::Remove,
            3 => PointerChange::Hover,
            4 => PointerChange::Down,
            5 => PointerChange::Move,
            6 => PointerChange::Up,
            7 => PointerChange::PanZoomStart,
            8 => PointerChange::PanZoomUpdate,
            9 => PointerChange::PanZoomEnd,
            _ => PointerChange::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Touch,
    Mouse,
    Stylus,
    InvertedStylus,
    Trackpad,
    Unknown,
}

impl PointerKind {
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn from_code(code: i32) -> PointerKind {
        match code {
            0 => PointerKind::Touch,
            1 => PointerKind::Mouse,
            2 => PointerKind::Stylus,
            3 => PointerKind::InvertedStylus,
            4 => PointerKind::Trackpad,
            _ => PointerKind::Unknown,
        }
    }
}

/// One pointer event, in logical pixels relative to the view.
#[derive(Clone, Copy, Debug)]
pub struct PointerEvent {
    pub view_id: i64,
    pub device: i64,
    pub pointer_id: i64,
    pub change: PointerChange,
    pub kind: PointerKind,
    pub signal_kind: SignalKind,
    /// Bitmask; bit 0 is the primary button.
    pub buttons: i32,
    pub time_stamp_micros: i64,
    /// Where the pointer is, in the view's logical coordinates.
    pub position: Offset,
    /// Movement since the previous event, in logical pixels.
    pub delta: Offset,
    pub scroll_delta: Offset,
    pub pressure: f64,
    /// Where the pointer is in the coordinates of the object that was hit.
    /// Equal to `position` until a hit test fills it in.
    pub local_position: Offset,
}

impl PointerEvent {
    pub fn is_down(&self) -> bool {
        self.change == PointerChange::Down
    }

    pub fn is_up(&self) -> bool {
        self.change == PointerChange::Up
    }
}

/// A completed tap.
#[derive(Clone, Copy, Debug)]
pub struct TapEvent {
    /// Where the finger came up, in the target's local coordinates.
    pub local_position: Offset,
    pub pointer_id: i64,
}

/// A wheel or trackpad scroll.
///
/// Carries where as well as how far, because the two things a scroll can mean
/// need different halves of it: a list moves by `delta` and does not care
/// where the pointer was, while a zoom has to keep whatever is under the
/// pointer under it and cannot be written without `local_position`.
#[derive(Clone, Copy, Debug)]
pub struct ScrollEvent {
    /// How far to scroll, in logical pixels. Positive `dy` means the content
    /// should move up -- the direction the reader is going, not the direction
    /// the finger went.
    pub delta: Offset,
    /// Where the pointer was, in the target's local coordinates.
    pub local_position: Offset,
}

/// A drag in progress.
#[derive(Clone, Copy, Debug)]
pub struct DragEvent {
    /// Movement since the last drag event.
    pub delta: Offset,
    /// Movement since the press began.
    pub total: Offset,
    pub local_position: Offset,
    pub pointer_id: i64,
}

/// A drag that has ended, and how fast it was going when it did.
///
/// The velocity is the only reason this is its own event: everything else
/// about the end of a drag is already known from the last update, and the
/// speed at the moment of release is not -- it has to be estimated from the
/// events before it, because the last two positions on their own are noise.
/// Upstream's `DragEndDetails`.
#[derive(Clone, Copy, Debug)]
pub struct DragEndEvent {
    /// How fast the pointer was moving when it lifted, in logical pixels per
    /// second, or zero when the release was not a fling. See
    /// [`VelocityTracker`] for what "not a fling" means.
    pub velocity: Offset,
    /// Movement since the press began.
    pub total: Offset,
    pub local_position: Offset,
    pub pointer_id: i64,
}

/// Two fingers moving with respect to each other: a pinch, a spread, a twist,
/// or the pair of them sliding together.
///
/// Upstream's `ScaleUpdateDetails`, minus the axis-separated scales. The focal
/// point is the point the gesture is happening *around* -- the midpoint of the
/// fingers -- and it is what a zoom has to keep still: scaling about the
/// centre of the screen while the reader pinches a corner moves the thing they
/// were looking at out from under them.
#[derive(Clone, Copy, Debug)]
pub struct ScaleEvent {
    /// The midpoint of the pointers, in view coordinates.
    pub focal_point: Offset,
    /// The same point in the target's local coordinates.
    pub local_focal_point: Offset,
    /// How far the focal point moved since the last event. A two-finger drag
    /// is a scale gesture whose scale stays at 1.
    pub focal_delta: Offset,
    /// The distance between the pointers, over what it was when the gesture
    /// started. 1.0 at the start, greater when spreading.
    pub scale: f32,
    /// How far the line between the pointers has turned since the start, in
    /// radians, anticlockwise.
    pub rotation: f32,
    /// How many pointers are taking part.
    pub pointer_count: usize,
}

/// A mouse moving over a region without pressing anything.
#[derive(Clone, Copy, Debug)]
pub struct HoverEvent {
    /// Where the pointer is, in the region's local coordinates.
    pub local_position: Offset,
}

// -- Timings ------------------------------------------------------------------
//
// Upstream's `gestures/constants.dart`. They are durations there and
// microseconds here because that is the clock the shell hands over.

/// How long a press has to be held before it is a long press.
pub const LONG_PRESS_TIMEOUT_MICROS: i64 = 500_000;

/// The longest gap between the start of two taps that still makes a double
/// tap.
pub const DOUBLE_TAP_TIMEOUT_MICROS: i64 = 300_000;

/// The shortest gap that does. Two "taps" closer together than this are one
/// bounce of a finger, not two taps.
pub const DOUBLE_TAP_MIN_TIME_MICROS: i64 = 40_000;

/// How far apart two taps may land and still be a double tap.
pub const DOUBLE_TAP_SLOP: f32 = 100.0;

// -- Velocity -----------------------------------------------------------------

/// Slower than this, in logical pixels per second, and a release is not a
/// fling. Upstream's `kMinFlingVelocity`.
pub const MIN_FLING_VELOCITY: f32 = 50.0;

/// Faster than this and it is treated as this. Upstream's
/// `kMaxFlingVelocity`: a fast flick can estimate at tens of thousands of
/// pixels a second, which is a scroll nobody asked for.
pub const MAX_FLING_VELOCITY: f32 = 8000.0;

/// What a pointer's recent movement says about how fast it is going.
///
/// `offset` and the velocity are separate answers to separate questions: how
/// far the pointer travelled over the samples that were used, and the slope
/// fitted through them. A flick that travels far but stops dead before lifting
/// has a large offset and no velocity, and it is not a fling.
#[derive(Clone, Copy, Debug, Default)]
pub struct VelocityEstimate {
    /// Logical pixels per second.
    pub pixels_per_second: Offset,
    /// How far the pointer moved across the samples the estimate used.
    pub offset: Offset,
    /// How long those samples span.
    pub duration_micros: i64,
}

/// One position, and when the pointer was there.
#[derive(Clone, Copy)]
struct Sample {
    time_micros: i64,
    position: Offset,
}

/// Estimates how fast a pointer is moving, from where it has been.
///
/// A port of upstream's `VelocityTracker`, including the numbers, which are
/// the part that matters: the last two positions are far too noisy to
/// differentiate -- a finger leaving the glass usually slows or twitches --
/// so a least-squares quadratic is fitted through the recent samples and its
/// first derivative at the end is the velocity.
///
/// The rules around the fit are as load-bearing as the fit:
///
/// * only the last [`Self::HORIZON_MICROS`] of movement counts, because
///   anything older describes a different gesture;
/// * a gap longer than [`Self::ASSUME_STOPPED_MICROS`] between two samples
///   ends the run -- the pointer stopped, and what happened before the pause
///   is not what it is doing now;
/// * the same gap before the release means the velocity is zero, which is what
///   stops "drag somewhere, hold still, lift" from flinging;
/// * fewer than [`Self::MIN_SAMPLES`] samples is not enough to fit anything.
///
/// What is not ported is upstream's confidence value -- the coefficient of
/// determination of the fit. Nothing consults it: neither `DragGestureRecognizer`
/// nor anything downstream of it, and computing a number to ignore it would be
/// worse than not having it.
pub struct VelocityTracker {
    /// Newest last. Never longer than [`Self::HISTORY`]; upstream uses a ring
    /// buffer of the same length, walked backwards from its write cursor.
    samples: Vec<Sample>,
}

impl VelocityTracker {
    /// How many samples to keep. Upstream's `_historySize`.
    const HISTORY: usize = 20;
    /// How far back an estimate may look. Upstream's `_horizonMilliseconds`.
    const HORIZON_MICROS: i64 = 100_000;
    /// A gap this long means the pointer stopped. Upstream's
    /// `_assumePointerMoveStoppedMilliseconds`.
    const ASSUME_STOPPED_MICROS: i64 = 40_000;
    /// Fewer samples than this and there is nothing to fit. Upstream's
    /// `_minSampleSize`.
    const MIN_SAMPLES: usize = 3;

    pub fn new() -> VelocityTracker {
        VelocityTracker { samples: Vec::with_capacity(VelocityTracker::HISTORY) }
    }

    /// Records where the pointer was at `time_micros`.
    pub fn add_position(&mut self, time_micros: i64, position: Offset) {
        if self.samples.len() == VelocityTracker::HISTORY {
            self.samples.remove(0);
        }
        self.samples.push(Sample { time_micros, position });
    }

    /// Estimates the velocity as of `now_micros`, which is when the pointer
    /// lifted.
    ///
    /// Upstream measures the same interval with a `Stopwatch` started at the
    /// last `addPosition`, because a `PointerUpEvent` may be handled some time
    /// after it happened. The release's own timestamp is the better clock when
    /// there is one, and there always is one here: it comes from the same
    /// `MotionEvent` -- or the same `WM_POINTER` -- as the positions did.
    pub fn estimate(&self, now_micros: i64) -> VelocityEstimate {
        let Some(newest) = self.samples.last() else {
            return VelocityEstimate::default();
        };

        // Has the pointer moved recently? A finger that came to rest before it
        // lifted is not flinging anything, whatever it was doing before that.
        if now_micros - newest.time_micros > VelocityTracker::ASSUME_STOPPED_MICROS {
            return VelocityEstimate::default();
        }

        // Backwards from the newest sample for as long as the motion is
        // continuous. Times are in milliseconds and negative -- ages -- so the
        // fitted derivative at zero is the velocity at the newest sample.
        let mut times: Vec<f32> = Vec::new();
        let mut xs: Vec<f32> = Vec::new();
        let mut ys: Vec<f32> = Vec::new();
        let mut oldest = newest;
        let mut previous = newest;
        for sample in self.samples.iter().rev() {
            let age = newest.time_micros - sample.time_micros;
            let gap = (sample.time_micros - previous.time_micros).abs();
            previous = sample;
            if age > VelocityTracker::HORIZON_MICROS
                || gap > VelocityTracker::ASSUME_STOPPED_MICROS
            {
                break;
            }
            oldest = sample;
            times.push(-(age as f32) / 1000.0);
            xs.push(sample.position.dx);
            ys.push(sample.position.dy);
        }

        let offset = newest.position.minus(oldest.position);
        let duration_micros = newest.time_micros - oldest.time_micros;

        if times.len() >= VelocityTracker::MIN_SAMPLES {
            if let (Some(fit_x), Some(fit_y)) =
                (fit_quadratic(&times, &xs), fit_quadratic(&times, &ys))
            {
                return VelocityEstimate {
                    // The fit is in pixels per millisecond.
                    pixels_per_second: Offset::new(fit_x[1] * 1000.0, fit_y[1] * 1000.0),
                    offset,
                    duration_micros,
                };
            }
        }

        // Something was recorded, but not enough of it to say how fast.
        VelocityEstimate { pixels_per_second: Offset::ZERO, offset, duration_micros }
    }

    /// The velocity to hand to a fling, or zero if this release was not one.
    ///
    /// Both halves of upstream's `PanGestureRecognizer`: `isFlingGesture`,
    /// which wants speed *and* distance -- a slow careful drag and a twitch in
    /// place are both releases that should leave the content where it is --
    /// and `Velocity.clampMagnitude`, which is applied along the vector so
    /// that clamping a diagonal fling does not turn it.
    pub fn fling_velocity(&self, now_micros: i64) -> Offset {
        let estimate = self.estimate(now_micros);
        let speed_squared = estimate.pixels_per_second.distance_squared();
        let travelled_squared = estimate.offset.distance_squared();
        if speed_squared <= MIN_FLING_VELOCITY * MIN_FLING_VELOCITY
            || travelled_squared <= TOUCH_SLOP * TOUCH_SLOP
        {
            return Offset::ZERO;
        }
        if speed_squared > MAX_FLING_VELOCITY * MAX_FLING_VELOCITY {
            let scale = MAX_FLING_VELOCITY / speed_squared.sqrt();
            return estimate.pixels_per_second.scaled(scale);
        }
        estimate.pixels_per_second
    }
}

impl Default for VelocityTracker {
    fn default() -> Self {
        VelocityTracker::new()
    }
}

/// Fits `y = c0 + c1*t + c2*t^2` through the samples and returns the
/// coefficients, or `None` if they do not determine one.
///
/// Upstream's `LeastSquaresSolver.solve(2)`, which solves it by Gram-Schmidt:
/// build the Vandermonde matrix A of the times, decompose it into an
/// orthonormal Q and an upper triangular R, and back-substitute R b = Qᵀ y.
/// Doing it this way rather than by normal equations is what keeps it stable
/// when the times are close together, which they always are -- sixteen
/// milliseconds apart, squared, is a matrix that barely differs from singular.
///
/// Upstream's solver also takes per-sample weights; its only caller passes 1
/// for every sample, so they are left out here rather than multiplied through.
fn fit_quadratic(times: &[f32], values: &[f32]) -> Option<[f32; 3]> {
    const N: usize = 3; // degree 2, so three coefficients
    let m = times.len();
    if m < N {
        return None;
    }

    // A, column i being the times raised to the i-th power.
    let mut a = [vec![0.0f32; m], vec![0.0f32; m], vec![0.0f32; m]];
    for h in 0..m {
        a[0][h] = 1.0;
        for i in 1..N {
            a[i][h] = a[i - 1][h] * times[h];
        }
    }

    let dot = |left: &[f32], right: &[f32]| -> f32 {
        left.iter().zip(right).map(|(l, r)| l * r).sum()
    };

    // Q and R, by Gram-Schmidt.
    let mut q = [vec![0.0f32; m], vec![0.0f32; m], vec![0.0f32; m]];
    let mut r = [[0.0f32; N]; N];
    for j in 0..N {
        q[j].copy_from_slice(&a[j]);
        for i in 0..j {
            let projection = dot(&q[j], &q[i]);
            // Split because `i` is always below `j`: the row being built and
            // the row being subtracted from it are two rows of the same array.
            let (earlier, from) = q.split_at_mut(j);
            for (value, basis) in from[0].iter_mut().zip(&earlier[i]) {
                *value -= projection * basis;
            }
        }
        let norm = dot(&q[j], &q[j]).sqrt();
        if norm < 0.000001 {
            // The times are linearly dependent -- every sample at the same
            // instant, which happens when a device reports a whole batch with
            // one timestamp. There is no line through them.
            return None;
        }
        let inverse = 1.0 / norm;
        for value in q[j].iter_mut() {
            *value *= inverse;
        }
        for i in 0..N {
            r[i][j] = if i < j { 0.0 } else { dot(&q[j], &a[i]) };
        }
    }

    // R b = Qᵀ y, from the bottom up because R is upper triangular.
    let mut coefficients = [0.0f32; N];
    for i in (0..N).rev() {
        coefficients[i] = dot(&q[i], values);
        for j in (i + 1..N).rev() {
            coefficients[i] -= r[i][j] * coefficients[j];
        }
        coefficients[i] /= r[i][i];
    }
    Some(coefficients)
}

/// The callbacks a [`crate::render::RenderPointerRegion`] can carry.
///
/// Every one is `Rc<dyn Fn>`: hit testing walks the tree behind a shared
/// reference, so a handler cannot ask for `&mut`. Capture a `StateHandle` and
/// call `set_state` instead.
#[derive(Default, Clone)]
pub struct PointerHandlers {
    pub on_pointer_down: Option<Rc<dyn Fn(&PointerEvent)>>,
    pub on_pointer_move: Option<Rc<dyn Fn(&PointerEvent)>>,
    pub on_pointer_up: Option<Rc<dyn Fn(&PointerEvent)>>,
    /// Fired when the press ends without having travelled past [`TOUCH_SLOP`].
    ///
    /// Held back when this region also wants double taps: the first tap of a
    /// double tap is indistinguishable from a single one until the window has
    /// passed. See [`GestureRouter::tick`].
    pub on_tap: Option<Rc<dyn Fn(TapEvent)>>,
    /// Fired when a press is held still for [`LONG_PRESS_TIMEOUT_MICROS`],
    /// before the finger lifts. A press that becomes a long press is no longer
    /// a tap.
    pub on_long_press: Option<Rc<dyn Fn(TapEvent)>>,
    /// Fired on the second of two taps in the same place, close enough
    /// together in time.
    pub on_double_tap: Option<Rc<dyn Fn(TapEvent)>>,
    /// Fired once, when a press first travels past [`TOUCH_SLOP`].
    pub on_drag_start: Option<Rc<dyn Fn(DragEvent)>>,
    pub on_drag_update: Option<Rc<dyn Fn(DragEvent)>>,
    /// Fired when the pointer lifts, or the platform cancels, after a drag has
    /// started. Carries the release velocity; see [`DragEndEvent`].
    pub on_drag_end: Option<Rc<dyn Fn(DragEndEvent)>>,
    /// Fired when a press begins or ends over this region, so a button can
    /// show that it is being held.
    pub on_press_change: Option<Rc<dyn Fn(bool)>>,
    /// Fired for a wheel or trackpad scroll over this region.
    pub on_scroll: Option<Rc<dyn Fn(ScrollEvent)>>,
    /// Fired when a second finger lands on this region.
    pub on_scale_start: Option<Rc<dyn Fn(ScaleEvent)>>,
    /// Fired as the fingers move. Scale and rotation are measured from where
    /// they were when the gesture started.
    pub on_scale_update: Option<Rc<dyn Fn(ScaleEvent)>>,
    /// Fired when the gesture stops being a two-finger one.
    pub on_scale_end: Option<Rc<dyn Fn(ScaleEvent)>>,
    /// Fired when a mouse moves onto or off this region.
    ///
    /// Nothing on a touch screen ever calls it: a finger is either touching or
    /// not there, so there is no hovering to report. Upstream's `MouseRegion`
    /// onEnter/onExit, and the same caveat applies -- a region that appears
    /// under a stationary pointer is entered upstream by the mouse tracker's
    /// post-frame check, and here only by the next thing the mouse does.
    pub on_hover_change: Option<Rc<dyn Fn(bool)>>,
    /// Fired as a mouse moves inside this region.
    pub on_hover: Option<Rc<dyn Fn(HoverEvent)>>,
}

impl PointerHandlers {
    pub fn new() -> PointerHandlers {
        PointerHandlers::default()
    }

    pub fn with_tap(mut self, handler: impl Fn(TapEvent) + 'static) -> Self {
        self.on_tap = Some(Rc::new(handler));
        self
    }

    pub fn with_long_press(mut self, handler: impl Fn(TapEvent) + 'static) -> Self {
        self.on_long_press = Some(Rc::new(handler));
        self
    }

    pub fn with_double_tap(mut self, handler: impl Fn(TapEvent) + 'static) -> Self {
        self.on_double_tap = Some(Rc::new(handler));
        self
    }

    pub fn with_hover_change(mut self, handler: impl Fn(bool) + 'static) -> Self {
        self.on_hover_change = Some(Rc::new(handler));
        self
    }

    pub fn with_hover(mut self, handler: impl Fn(HoverEvent) + 'static) -> Self {
        self.on_hover = Some(Rc::new(handler));
        self
    }

    pub fn with_scale_start(mut self, handler: impl Fn(ScaleEvent) + 'static) -> Self {
        self.on_scale_start = Some(Rc::new(handler));
        self
    }

    pub fn with_scale_update(mut self, handler: impl Fn(ScaleEvent) + 'static) -> Self {
        self.on_scale_update = Some(Rc::new(handler));
        self
    }

    pub fn with_scale_end(mut self, handler: impl Fn(ScaleEvent) + 'static) -> Self {
        self.on_scale_end = Some(Rc::new(handler));
        self
    }

    pub fn with_press_change(mut self, handler: impl Fn(bool) + 'static) -> Self {
        self.on_press_change = Some(Rc::new(handler));
        self
    }

    pub fn with_scroll(mut self, handler: impl Fn(ScrollEvent) + 'static) -> Self {
        self.on_scroll = Some(Rc::new(handler));
        self
    }

    pub fn with_drag_start(mut self, handler: impl Fn(DragEvent) + 'static) -> Self {
        self.on_drag_start = Some(Rc::new(handler));
        self
    }

    pub fn with_drag_update(mut self, handler: impl Fn(DragEvent) + 'static) -> Self {
        self.on_drag_update = Some(Rc::new(handler));
        self
    }

    pub fn with_drag_end(mut self, handler: impl Fn(DragEndEvent) + 'static) -> Self {
        self.on_drag_end = Some(Rc::new(handler));
        self
    }

    pub fn with_pointer_down(mut self, handler: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_down = Some(Rc::new(handler));
        self
    }

    pub fn with_pointer_move(mut self, handler: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_move = Some(Rc::new(handler));
        self
    }

    pub fn with_pointer_up(mut self, handler: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_up = Some(Rc::new(handler));
        self
    }

    /// Whether anything is listening. A region with nothing attached still
    /// participates in hit testing, so it can shield what is behind it.
    pub fn is_empty(&self) -> bool {
        self.on_pointer_down.is_none()
            && self.on_pointer_move.is_none()
            && self.on_pointer_up.is_none()
            && self.on_tap.is_none()
            && self.on_long_press.is_none()
            && self.on_double_tap.is_none()
            && self.on_drag_start.is_none()
            && self.on_drag_update.is_none()
            && self.on_drag_end.is_none()
            && self.on_press_change.is_none()
            && self.on_scroll.is_none()
            && self.on_hover_change.is_none()
            && self.on_hover.is_none()
            && !self.wants_scale()
    }

    /// Whether this region wants two-finger gestures. What tells an image that
    /// can be pinched apart from the list it is sitting in, the same way
    /// [`PointerHandlers::wants_drag`] does for scrolling.
    pub fn wants_scale(&self) -> bool {
        self.on_scale_start.is_some()
            || self.on_scale_update.is_some()
            || self.on_scale_end.is_some()
    }

    /// Whether this region wants drags. What tells a scrollable ancestor apart
    /// from a button on the way out of a hit test.
    pub fn wants_drag(&self) -> bool {
        self.on_drag_start.is_some()
            || self.on_drag_update.is_some()
            || self.on_drag_end.is_some()
    }
}

/// One end of a gesture: what was hit, and where it was hit.
#[derive(Clone)]
struct Target {
    handlers: Rc<PointerHandlers>,
    /// Which region this is, from the hit test. Zero means the region has no
    /// identity, and two of those are never the same one.
    ///
    /// Identity matters between *frames*: whether the finger that just landed
    /// is on the same thing the last tap was, and whether the second finger is
    /// on the same thing as the first. The handlers cannot answer that -- they
    /// are rebuilt every frame, so the same region is a different `Rc` each
    /// time. Upstream never asks the question, because there a recogniser is
    /// an object that outlives the frame; here the widget-assigned id is what
    /// outlives it.
    id: u64,
    /// Where the press began, in this target's coordinates.
    local_origin: Offset,
}

impl Target {
    /// Whether two targets are the same region, across frames.
    fn is_same_region(&self, other: &Target) -> bool {
        (self.id != 0 && self.id == other.id) || Rc::ptr_eq(&self.handlers, &other.handlers)
    }
}

/// What a single pressed pointer is doing.
///
/// Two targets, not one. A press inside a scrolling list of buttons is both a
/// press on a button and the beginning of a possible scroll, and those are
/// different objects: the innermost thing that can be tapped gets the tap, and
/// the innermost thing that can be dragged gets the drag. Recording only the
/// innermost hit -- which is what this did at first -- means a list whose rows
/// are tappable cannot be scrolled at all, because every press lands on a row.
struct ActivePointer {
    tap: Option<Target>,
    drag: Option<Target>,
    /// The innermost region under the press that wants two fingers, if any.
    /// Recorded on the way down like the other two, because by the time a
    /// second finger lands the tree may have been rebuilt.
    scale: Option<Target>,
    /// Where the press began, in view coordinates.
    origin: Offset,
    /// Movement since the press began.
    total: Offset,
    /// Set once the press travels past the slop.
    ///
    /// Separate from whether anything is being dragged: travelling past the
    /// slop always means the press is no longer a tap, even when nothing
    /// underneath wants a drag. Sliding a finger off a button cancels it
    /// whether or not the button happens to be inside a list.
    past_slop: bool,
    /// Whether the tap target has been told it is pressed.
    pressed: bool,
    /// Where this pointer has been, so that the release can say how fast it
    /// was going. Fed by every move, whether or not anything is being dragged:
    /// what the press turns out to be is not known until it is over.
    velocity: VelocityTracker,
    /// When the finger landed, and where it is now. The first is the long
    /// press deadline's starting point; the second is what a second finger
    /// needs in order to work out a focal point.
    down_micros: i64,
    position: Offset,
    /// Set once this press has been announced as a long press, which is also
    /// what stops it from becoming a tap.
    long_pressed: bool,
    /// Set when this press landed close enough, and soon enough, after a tap
    /// on the same region: if it ends as a tap it is a double tap.
    second_tap: bool,
    /// Set while this pointer is part of a two-finger gesture. Its drag has
    /// been ended by then and must not be resumed.
    scaling: bool,
}

/// A tap that has happened but has not been announced yet, because the region
/// it landed on also wants double taps and this might be the first of two.
///
/// Upstream a `DoubleTapGestureRecognizer` *holds* the arena open for
/// `kDoubleTapTimeout` instead of rejecting, which is why a `GestureDetector`
/// with both callbacks reports a single tap late. This is that hold.
struct PendingTap {
    target: Target,
    event: TapEvent,
    /// Where the finger landed, in view coordinates, and when the press
    /// started and ended. A second tap has to land near `origin`, start no
    /// later than [`DOUBLE_TAP_TIMEOUT_MICROS`] after `down_micros`, and no
    /// sooner than [`DOUBLE_TAP_MIN_TIME_MICROS`] after `up_micros` -- the
    /// last of which is what keeps one finger bouncing from counting twice.
    origin: Offset,
    down_micros: i64,
    up_micros: i64,
}

/// Halfway between two points, which is where a two-finger gesture happens.
fn midpoint(a: Offset, b: Offset) -> Offset {
    Offset { dx: (a.dx + b.dx) / 2.0, dy: (a.dy + b.dy) / 2.0 }
}

/// Brings an angle back into -pi..pi.
///
/// Two angles measured with `atan2` can differ by nearly a full turn when the
/// fingers cross the half-turn line, and reporting a rotation of six radians
/// for a wrist that moved a degree would spin whatever is listening.
fn normalise_angle(radians: f32) -> f32 {
    let turn = std::f32::consts::PI * 2.0;
    let mut angle = radians % turn;
    if angle > std::f32::consts::PI {
        angle -= turn;
    } else if angle < -std::f32::consts::PI {
        angle += turn;
    }
    angle
}

/// A two-finger gesture in progress.
struct ActiveScale {
    handlers: Rc<PointerHandlers>,
    /// The pointers taking part, in the order they landed.
    pointers: (i64, i64),
    /// Where the focal point was in the target's coordinates when it started,
    /// and where it was last time, so an update can report how far it moved.
    local_origin: Offset,
    initial_focal: Offset,
    last_focal: Offset,
    /// The distance and angle between the fingers at the start. Everything is
    /// reported relative to these, which is what makes `scale` start at one.
    initial_distance: f32,
    initial_angle: f32,
}

/// Routes pointer events to the render tree and recognises gestures.
///
/// One per view. It holds no borrow of the tree between calls; each dispatch is
/// given the tree that was painted last.
pub struct GestureRouter {
    active: Vec<(i64, ActivePointer)>,
    /// A tap that has happened and is waiting to see whether a second one
    /// follows it.
    pending_tap: Option<PendingTap>,
    /// The two-finger gesture in progress, if there is one.
    scale: Option<ActiveScale>,
    /// Which regions the mouse is currently inside, innermost first, and what
    /// to tell them when it leaves.
    ///
    /// The *whole* path rather than the innermost region: a button inside a
    /// card is inside both, and both want to know. Upstream's `MouseTracker`
    /// keeps the same set for the same reason.
    hovered: Vec<(u64, Rc<PointerHandlers>)>,
}

impl GestureRouter {
    pub fn new() -> GestureRouter {
        GestureRouter {
            active: Vec::new(),
            pending_tap: None,
            scale: None,
            hovered: Vec::new(),
        }
    }

    /// Moves the clock forward, and returns whether anything is still waiting
    /// on it.
    ///
    /// Two gestures are decided by time passing rather than by an event
    /// arriving: a long press fires while the finger is still down and doing
    /// nothing, and a single tap on a region that also wants double taps
    /// cannot be announced until the window for a second tap has closed.
    /// Neither can be driven by pointer events, because the defining feature
    /// of both is that no pointer event happens.
    ///
    /// Upstream both are `Timer`s. Frames here are on demand, so the host
    /// calls this once a frame and asks for another frame while this returns
    /// true -- see `WidgetHost::begin_frame`.
    pub fn tick(&mut self, now_micros: i64) -> bool {
        self.fire_due_long_presses(now_micros);
        self.flush_expired_tap(now_micros);
        self.awaits_deadline(now_micros)
    }

    /// Whether some gesture is still waiting for the clock.
    pub fn awaits_deadline(&self, now_micros: i64) -> bool {
        let waiting_to_be_a_long_press = self.active.iter().any(|(_, pointer)| {
            !pointer.long_pressed
                && !pointer.past_slop
                && !pointer.scaling
                && pointer
                    .tap
                    .as_ref()
                    .is_some_and(|target| target.handlers.on_long_press.is_some())
        });
        let waiting_to_be_a_double_tap = self
            .pending_tap
            .as_ref()
            .is_some_and(|tap| now_micros - tap.down_micros < DOUBLE_TAP_TIMEOUT_MICROS);
        waiting_to_be_a_long_press || waiting_to_be_a_double_tap
    }

    /// Announces every press that has been held long enough.
    fn fire_due_long_presses(&mut self, now_micros: i64) {
        let due: Vec<i64> = self
            .active
            .iter()
            .filter(|(_, pointer)| {
                !pointer.long_pressed
                    && !pointer.past_slop
                    && !pointer.scaling
                    && now_micros - pointer.down_micros >= LONG_PRESS_TIMEOUT_MICROS
                    && pointer
                        .tap
                        .as_ref()
                        .is_some_and(|target| target.handlers.on_long_press.is_some())
            })
            .map(|(id, _)| *id)
            .collect();

        for id in due {
            let Some(pointer) = self.find(id) else { continue };
            pointer.long_pressed = true;
            let was_pressed = pointer.pressed;
            pointer.pressed = false;
            let Some(target) = pointer.tap.clone() else { continue };
            let local = target.local_origin;
            // The press has been claimed, so whatever was showing it as held
            // stops. Upstream this is the tap recogniser losing the arena.
            if was_pressed {
                if let Some(press_change) = &target.handlers.on_press_change {
                    press_change(false);
                }
            }
            if let Some(long_press) = &target.handlers.on_long_press {
                long_press(TapEvent { local_position: local, pointer_id: id });
            }
        }
    }

    /// Announces a held-back tap once no second tap can still arrive.
    fn flush_expired_tap(&mut self, now_micros: i64) {
        let expired = self
            .pending_tap
            .as_ref()
            .is_some_and(|tap| now_micros - tap.down_micros >= DOUBLE_TAP_TIMEOUT_MICROS);
        if !expired {
            return;
        }
        self.announce_pending_tap();
    }

    /// Gives up on a second tap arriving and reports the first as a single
    /// one.
    fn announce_pending_tap(&mut self) {
        let Some(tap) = self.pending_tap.take() else { return };
        if let Some(handler) = &tap.target.handlers.on_tap {
            handler(tap.event);
        }
    }

    /// How many pointers are currently pressed. Used by tests.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    fn take(&mut self, pointer_id: i64) -> Option<ActivePointer> {
        let index = self.active.iter().position(|(id, _)| *id == pointer_id)?;
        Some(self.active.remove(index).1)
    }

    fn find(&mut self, pointer_id: i64) -> Option<&mut ActivePointer> {
        self.active
            .iter_mut()
            .find(|(id, _)| *id == pointer_id)
            .map(|(_, pointer)| pointer)
    }

    /// Dispatches one event against `root`, the render tree painted last frame.
    ///
    /// Returns whether anything handled it, which is what a caller would use to
    /// decide whether to let the platform have the event instead.
    pub fn dispatch(&mut self, root: &dyn RenderBox, event: &PointerEvent) -> bool {
        // A scroll arrives as a hover carrying a signal rather than as its own
        // change, which is the shell's encoding, so it has to be checked before
        // the change is.
        if event.signal_kind == SignalKind::Scroll {
            return self.on_scroll(root, event);
        }
        match event.change {
            PointerChange::Down => self.on_down(root, event),
            PointerChange::Move => self.on_move(event),
            PointerChange::Up => self.on_up(event),
            // A mouse leaving the window is a Remove with nothing pressed:
            // whatever it was hovering, it is not hovering it now.
            PointerChange::Cancel | PointerChange::Remove => {
                self.leave_all();
                self.on_cancel(event)
            }
            PointerChange::Hover => self.on_hover(root, event),
            // Add and the pan-zoom family have no recogniser yet. They are
            // accepted so the shell's contract holds.
            _ => false,
        }

    }

    /// Tells the regions under the mouse that it is there, and the ones it has
    /// left that it is gone.
    fn on_hover(&mut self, root: &dyn RenderBox, event: &PointerEvent) -> bool {
        let mut result = HitTestResult::new();
        root.hit_test(event.position, &mut result);

        let mut under: Vec<(u64, Rc<PointerHandlers>, Offset)> = Vec::new();
        for entry in &result.path {
            let Some(handlers) = entry.handlers.clone() else { continue };
            if handlers.on_hover_change.is_some() || handlers.on_hover.is_some() {
                under.push((entry.target, handlers, entry.local_position));
            }
        }

        // Left: everything that was under the mouse and no longer is. Told
        // first, so a region cannot see two things highlighted at once.
        let still_here: Vec<u64> = under.iter().map(|(id, _, _)| *id).collect();
        let left: Vec<Rc<PointerHandlers>> = self
            .hovered
            .iter()
            .filter(|(id, _)| !still_here.contains(id))
            .map(|(_, handlers)| Rc::clone(handlers))
            .collect();
        for handlers in &left {
            if let Some(changed) = &handlers.on_hover_change {
                changed(false);
            }
        }

        let was_here: Vec<u64> = self.hovered.iter().map(|(id, _)| *id).collect();
        let mut told_anyone = !left.is_empty();
        for (id, handlers, local) in &under {
            if !was_here.contains(id) {
                if let Some(changed) = &handlers.on_hover_change {
                    changed(true);
                    told_anyone = true;
                }
            }
            if let Some(hover) = &handlers.on_hover {
                hover(HoverEvent { local_position: *local });
                told_anyone = true;
            }
        }

        self.hovered =
            under.into_iter().map(|(id, handlers, _)| (id, handlers)).collect();
        // Only when something was actually told. A mouse crossing a window
        // with nothing hoverable in it, or sitting still inside a region that
        // only cares about being entered, must not cost a frame per pixel.
        told_anyone
    }

    /// Gives a scroll to the innermost region under the pointer that wants one.
    ///
    /// Innermost-first, so a scrollable inside a scrollable takes the wheel;
    /// falling outward is what lets the page scroll when the wheel is over a
    /// row that does not scroll itself.
    fn on_scroll(&mut self, root: &dyn RenderBox, event: &PointerEvent) -> bool {
        let mut result = HitTestResult::new();
        root.hit_test(event.position, &mut result);
        for entry in &result.path {
            let Some(handlers) = &entry.handlers else { continue };
            if let Some(scroll) = &handlers.on_scroll {
                scroll(ScrollEvent {
                    delta: event.scroll_delta,
                    local_position: entry.local_position,
                });
                return true;
            }
        }
        false
    }

    /// The mouse has gone: nothing is hovered any more.
    fn leave_all(&mut self) {
        for (_, handlers) in std::mem::take(&mut self.hovered) {
            if let Some(changed) = &handlers.on_hover_change {
                changed(false);
            }
        }
    }

    fn on_down(&mut self, root: &dyn RenderBox, event: &PointerEvent) -> bool {
        let mut result = HitTestResult::new();
        root.hit_test(event.position, &mut result);

        // Innermost first. The first region that listens at all takes the tap;
        // the first that wants drags takes the drag. They are usually different
        // objects -- a row inside a list -- and looking for them separately is
        // what lets a list of buttons scroll.
        let mut tap: Option<Target> = None;
        let mut drag: Option<Target> = None;
        let mut scale: Option<Target> = None;
        for entry in &result.path {
            let Some(handlers) = entry.handlers.clone() else { continue };
            if drag.is_none() && handlers.wants_drag() {
                drag = Some(Target {
                    handlers: handlers.clone(),
                    id: entry.target,
                    local_origin: entry.local_position,
                });
            }
            if scale.is_none() && handlers.wants_scale() {
                scale = Some(Target {
                    handlers: handlers.clone(),
                    id: entry.target,
                    local_origin: entry.local_position,
                });
            }
            if tap.is_none() {
                tap = Some(Target {
                    handlers,
                    id: entry.target,
                    local_origin: entry.local_position,
                });
            }
            if tap.is_some() && drag.is_some() && scale.is_some() {
                break;
            }
        }
        if tap.is_none() && drag.is_none() && scale.is_none() {
            return false;
        }

        // Is this the second half of a double tap? Decided here rather than on
        // the way up, because it is the *start* of the second tap that the
        // window is measured to, and because a press that lands somewhere else
        // settles the first tap immediately instead of leaving it hanging.
        let second_tap = self.pair_with_pending_tap(&tap, event);

        let mut pressed = false;
        if let Some(target) = &tap {
            let mut local_event = *event;
            local_event.local_position = target.local_origin;
            if let Some(down) = &target.handlers.on_pointer_down {
                down(&local_event);
            }
            if let Some(press_change) = &target.handlers.on_press_change {
                press_change(true);
                pressed = true;
            }
        }

        // The scrollable under the row hears about the press too. It has to:
        // a finger landing on a list that is still flinging stops it, and that
        // has to happen on contact rather than a slop's worth of travel later,
        // or a list cannot be caught. Upstream a `PointerDownEvent` goes to
        // every recogniser on the hit-test path -- the arena has not decided
        // anything yet -- and this is the two-target version of the same rule.
        if let Some(target) = &drag {
            let already_told =
                tap.as_ref().is_some_and(|t| Rc::ptr_eq(&t.handlers, &target.handlers));
            if !already_told {
                if let Some(down) = &target.handlers.on_pointer_down {
                    let mut local_event = *event;
                    local_event.local_position = target.local_origin;
                    down(&local_event);
                }
            }
        }

        // A second down from the same pointer without an up should not leave
        // the first entry stranded.
        self.take(event.pointer_id);
        let mut velocity = VelocityTracker::new();
        velocity.add_position(event.time_stamp_micros, event.position);
        self.active.push((
            event.pointer_id,
            ActivePointer {
                tap,
                drag,
                scale,
                origin: event.position,
                total: Offset::ZERO,
                past_slop: false,
                pressed,
                velocity,
                down_micros: event.time_stamp_micros,
                position: event.position,
                long_pressed: false,
                second_tap,
                scaling: false,
            },
        ));
        self.begin_scale_if_two_fingers(event);
        true
    }

    /// Whether this press is the second tap of a double tap, settling the
    /// first one either way.
    fn pair_with_pending_tap(&mut self, tap: &Option<Target>, event: &PointerEvent) -> bool {
        let Some(pending) = &self.pending_tap else { return false };
        let matches = tap.as_ref().is_some_and(|target| {
            target.is_same_region(&pending.target)
                && event.position.minus(pending.origin).distance() <= DOUBLE_TAP_SLOP
                && event.time_stamp_micros - pending.down_micros <= DOUBLE_TAP_TIMEOUT_MICROS
                && event.time_stamp_micros - pending.up_micros >= DOUBLE_TAP_MIN_TIME_MICROS
        });
        if matches {
            // Held rather than announced: the first tap of a double tap is not
            // also a single tap.
            self.pending_tap = None;
            true
        } else {
            self.announce_pending_tap();
            false
        }
    }

    /// Starts a two-finger gesture if this press is the second finger on a
    /// region that wants one.
    fn begin_scale_if_two_fingers(&mut self, event: &PointerEvent) {
        if self.scale.is_some() {
            // Already two fingers in. A third is ignored, which is what
            // upstream's two-pointer arithmetic amounts to as well.
            return;
        }
        let Some(target) = self
            .find(event.pointer_id)
            .and_then(|pointer| pointer.scale.clone())
        else {
            return;
        };
        // The other finger has to be on the same region -- two fingers on two
        // different images are two drags, not a pinch.
        let Some((other_id, other_position)) = self
            .active
            .iter()
            .find(|(id, pointer)| {
                *id != event.pointer_id
                    && pointer.scale.as_ref().is_some_and(|t| t.is_same_region(&target))
            })
            .map(|(id, pointer)| (*id, pointer.position))
        else {
            return;
        };

        let focal = midpoint(other_position, event.position);
        let separation = event.position.minus(other_position);
        let scale = ActiveScale {
            handlers: Rc::clone(&target.handlers),
            pointers: (other_id, event.pointer_id),
            // The focal point starts between the fingers, which is not where
            // either press landed, so the local origin is the first finger's
            // plus however far the focal point is from it.
            local_origin: target.local_origin.plus(focal.minus(event.position)),
            initial_focal: focal,
            last_focal: focal,
            initial_distance: separation.distance().max(f32::EPSILON),
            initial_angle: separation.dy.atan2(separation.dx),
        };
        let details = ScaleEvent {
            focal_point: focal,
            local_focal_point: scale.local_origin,
            focal_delta: Offset::ZERO,
            scale: 1.0,
            rotation: 0.0,
            pointer_count: 2,
        };
        if let Some(start) = &target.handlers.on_scale_start {
            start(details);
        }
        self.scale = Some(scale);

        // Whatever the fingers were doing on their own, they are not doing it
        // any more. Upstream the scale recogniser winning the arena is what
        // rejects the drag; the drag hears about that as a cancel, which is a
        // drag end with no velocity -- flinging a list because a pinch started
        // on it is not what anyone asked for.
        for id in [other_id, event.pointer_id] {
            self.end_drag_for_scale(id);
        }
    }

    /// Takes a pointer out of whatever drag it was in, because a two-finger
    /// gesture has claimed it.
    fn end_drag_for_scale(&mut self, pointer_id: i64) {
        let Some(pointer) = self.find(pointer_id) else { return };
        pointer.scaling = true;
        pointer.long_pressed = true; // No long press out of a pinch either.
        let past_slop = pointer.past_slop;
        let total = pointer.total;
        let was_pressed = pointer.pressed;
        pointer.pressed = false;
        let drag = pointer.drag.take();
        let tap = pointer.tap.clone();

        if was_pressed {
            if let Some(target) = &tap {
                if let Some(press_change) = &target.handlers.on_press_change {
                    press_change(false);
                }
            }
        }
        if past_slop {
            if let Some(target) = &drag {
                if let Some(end) = &target.handlers.on_drag_end {
                    end(DragEndEvent {
                        velocity: Offset::ZERO,
                        total,
                        local_position: target.local_origin,
                        pointer_id,
                    });
                }
            }
        }
    }

    fn on_move(&mut self, event: &PointerEvent) -> bool {
        let Some(active) = self.find(event.pointer_id) else {
            return false;
        };
        active.total = active.total.plus(event.delta);
        active.position = event.position;
        active.velocity.add_position(event.time_stamp_micros, event.position);
        if active.scaling {
            // A finger in a pinch is not dragging, tapping or long-pressing;
            // the only thing left to report is the pinch itself.
            return self.update_scale(event);
        }
        let travelled =
            (active.total.dx * active.total.dx + active.total.dy * active.total.dy).sqrt();
        let travel = event.position.minus(active.origin);
        let total = active.total;

        let starting = !active.past_slop && travelled > TOUCH_SLOP;
        if starting {
            active.past_slop = true;
        }
        let past_slop = active.past_slop;
        let was_pressed = active.pressed;
        let tap = active.tap.clone();
        let drag_target = active.drag.clone();

        // Past the slop the press is no longer a tap candidate, so the pressed
        // state comes back off -- the same thing a button does when a finger
        // slides off it. This is also what stops a scroll from selecting the row
        // it started on.
        if starting && was_pressed {
            if let Some(target) = &tap {
                if let Some(press_change) = &target.handlers.on_press_change {
                    press_change(false);
                }
            }
            if let Some(active) = self.find(event.pointer_id) {
                active.pressed = false;
            }
        }

        if let Some(target) = &tap {
            if let Some(moved) = &target.handlers.on_pointer_move {
                let mut local_event = *event;
                local_event.local_position = target.local_origin.plus(travel);
                moved(&local_event);
            }
        }

        if let Some(target) = &drag_target {
            let drag = DragEvent {
                delta: event.delta,
                total,
                local_position: target.local_origin.plus(travel),
                pointer_id: event.pointer_id,
            };
            if starting {
                if let Some(start) = &target.handlers.on_drag_start {
                    start(drag);
                }
            }
            if past_slop {
                if let Some(update) = &target.handlers.on_drag_update {
                    update(drag);
                }
            }
        }
        true
    }

    /// Reports where the two fingers are now, relative to where they started.
    fn update_scale(&mut self, _event: &PointerEvent) -> bool {
        let Some(scale) = &self.scale else { return true };
        let Some(first) = self.position_of(scale.pointers.0) else { return true };
        let Some(second) = self.position_of(scale.pointers.1) else { return true };

        let focal = midpoint(first, second);
        let separation = second.minus(first);
        let distance = separation.distance();
        let angle = separation.dy.atan2(separation.dx);

        let Some(scale) = &mut self.scale else { return true };
        let details = ScaleEvent {
            focal_point: focal,
            // The focal point is reported in the target's coordinates by
            // moving the origin by however far the focal point has moved: the
            // target has not moved under the fingers, so the two deltas are
            // the same.
            local_focal_point: scale.local_origin.plus(focal.minus(scale.initial_focal)),
            focal_delta: focal.minus(scale.last_focal),
            scale: distance / scale.initial_distance,
            rotation: normalise_angle(angle - scale.initial_angle),
            pointer_count: 2,
        };
        scale.last_focal = focal;
        let handlers = Rc::clone(&scale.handlers);
        if let Some(update) = &handlers.on_scale_update {
            update(details);
        }
        true
    }

    fn position_of(&self, pointer_id: i64) -> Option<Offset> {
        self.active
            .iter()
            .find(|(id, _)| *id == pointer_id)
            .map(|(_, pointer)| pointer.position)
    }

    /// Ends the two-finger gesture if `pointer_id` was one of its fingers.
    ///
    /// One finger leaving ends the gesture rather than demoting it to a drag.
    /// Upstream a `ScaleGestureRecognizer` does keep going with one pointer,
    /// but it owns the pan as well; here the drag recogniser is a separate
    /// thing that was told to stop, and handing the gesture back to it
    /// mid-flight would move the content by however far the fingers happened
    /// to be apart.
    fn end_scale_if_involved(&mut self, pointer_id: i64) {
        let involved = self
            .scale
            .as_ref()
            .is_some_and(|scale| scale.pointers.0 == pointer_id || scale.pointers.1 == pointer_id);
        if !involved {
            return;
        }
        let Some(scale) = self.scale.take() else { return };
        let details = ScaleEvent {
            focal_point: scale.last_focal,
            local_focal_point: scale
                .local_origin
                .plus(scale.last_focal.minus(scale.initial_focal)),
            focal_delta: Offset::ZERO,
            scale: 1.0,
            rotation: 0.0,
            pointer_count: 1,
        };
        if let Some(end) = &scale.handlers.on_scale_end {
            end(details);
        }
        // The finger that is still down stays claimed: it may not resume the
        // drag it was doing before the pinch.
        for id in [scale.pointers.0, scale.pointers.1] {
            if let Some(pointer) = self.find(id) {
                pointer.scaling = true;
            }
        }
    }

    fn on_up(&mut self, event: &PointerEvent) -> bool {
        self.end_scale_if_involved(event.pointer_id);
        let Some(active) = self.take(event.pointer_id) else {
            return false;
        };
        if active.scaling {
            // Everything this finger had to say was said by the pinch.
            return true;
        }
        let travel = event.position.minus(active.origin);

        if let Some(target) = &active.tap {
            let local = target.local_origin.plus(travel);
            if active.pressed {
                if let Some(press_change) = &target.handlers.on_press_change {
                    press_change(false);
                }
            }
            if let Some(up) = &target.handlers.on_pointer_up {
                let mut local_event = *event;
                local_event.local_position = local;
                up(&local_event);
            }
            // A press that travelled is not a tap, however short the travel,
            // and neither is one that was already announced as a long press.
            if !active.past_slop && !active.long_pressed {
                let tap_event = TapEvent { local_position: local, pointer_id: event.pointer_id };
                if active.second_tap {
                    if let Some(double) = &target.handlers.on_double_tap {
                        double(tap_event);
                    }
                } else if target.handlers.on_double_tap.is_some() {
                    // This region wants double taps, so a single one cannot be
                    // reported until it is clear that no second tap is coming.
                    // `tick` announces it when the window closes.
                    self.pending_tap = Some(PendingTap {
                        target: target.clone(),
                        event: tap_event,
                        origin: active.origin,
                        down_micros: active.down_micros,
                        up_micros: event.time_stamp_micros,
                    });
                } else if let Some(tap) = &target.handlers.on_tap {
                    tap(tap_event);
                }
            }
        }

        if active.past_slop {
            if let Some(target) = &active.drag {
                if let Some(end) = &target.handlers.on_drag_end {
                    end(DragEndEvent {
                        velocity: active.velocity.fling_velocity(event.time_stamp_micros),
                        total: active.total,
                        local_position: target.local_origin.plus(travel),
                        pointer_id: event.pointer_id,
                    });
                }
            }
        }
        true
    }

    fn on_cancel(&mut self, event: &PointerEvent) -> bool {
        self.end_scale_if_involved(event.pointer_id);
        let Some(active) = self.take(event.pointer_id) else {
            return false;
        };
        if active.scaling {
            return true;
        }
        // A cancelled press is not a tap. Only the pressed state is unwound.
        if active.pressed {
            if let Some(target) = &active.tap {
                if let Some(press_change) = &target.handlers.on_press_change {
                    press_change(false);
                }
            }
        }
        if active.past_slop {
            if let Some(target) = &active.drag {
                if let Some(end) = &target.handlers.on_drag_end {
                    // No velocity: a cancelled drag is the platform taking the
                    // gesture away -- a call arriving, a system gesture
                    // starting -- and flinging the content after that is not
                    // what anyone asked for.
                    end(DragEndEvent {
                        velocity: Offset::ZERO,
                        total: active.total,
                        local_position: target.local_origin,
                        pointer_id: event.pointer_id,
                    });
                }
            }
        }
        true
    }
}

impl Default for GestureRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{
        BoxConstraints, Offset, PaintContext, RenderPointerRegion, RenderStack, Size,
        StackPosition,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Sized(Size);

    impl RenderBox for Sized {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            constraints.constrain(self.0)
        }
        fn size(&self) -> Size {
            self.0
        }
        fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
    }

    fn event(change: PointerChange, x: f32, y: f32, dx: f32, dy: f32) -> PointerEvent {
        PointerEvent {
            view_id: 0,
            device: 0,
            pointer_id: 1,
            change,
            kind: PointerKind::Touch,
            signal_kind: SignalKind::None,
            buttons: 1,
            time_stamp_micros: 0,
            position: Offset::new(x, y),
            delta: Offset::new(dx, dy),
            scroll_delta: Offset::ZERO,
            pressure: 1.0,
            local_position: Offset::new(x, y),
        }
    }

    /// A 40x40 region at (10, 10) inside a 100x100 stack.
    fn tree(handlers: PointerHandlers) -> RenderStack {
        let mut stack = RenderStack::new()
            .push(Sized(Size::square(100.0)))
            .push_positioned(
                RenderPointerRegion::new(7, Sized(Size::square(40.0)))
                    .with_handlers(handlers),
                StackPosition { left: Some(10.0), top: Some(10.0), ..Default::default() },
            );
        stack.layout(BoxConstraints::tight(100.0, 100.0));
        stack
    }

    #[test]
    fn a_press_and_release_in_place_is_a_tap() {
        let taps = Rc::new(RefCell::new(Vec::new()));
        let sink = taps.clone();
        let root = tree(PointerHandlers::new().with_tap(move |tap| {
            sink.borrow_mut().push(tap.local_position);
        }));

        let mut router = GestureRouter::new();
        assert!(router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0)));
        assert_eq!(router.active_count(), 1);
        assert!(router.dispatch(&root, &event(PointerChange::Up, 20.0, 20.0, 0.0, 0.0)));
        assert_eq!(router.active_count(), 0);

        // One tap, reported in the region's own coordinates.
        assert_eq!(taps.borrow().len(), 1);
        assert_eq!(taps.borrow()[0], Offset::new(10.0, 10.0));
    }

    #[test]
    fn a_press_outside_the_region_is_not_a_tap() {
        let taps = Rc::new(RefCell::new(0));
        let sink = taps.clone();
        let root = tree(PointerHandlers::new().with_tap(move |_| {
            *sink.borrow_mut() += 1;
        }));

        let mut router = GestureRouter::new();
        assert!(!router.dispatch(&root, &event(PointerChange::Down, 80.0, 80.0, 0.0, 0.0)));
        router.dispatch(&root, &event(PointerChange::Up, 80.0, 80.0, 0.0, 0.0));
        assert_eq!(*taps.borrow(), 0);
    }

    #[test]
    fn moving_past_the_slop_turns_the_press_into_a_drag() {
        let log = Rc::new(RefCell::new(Vec::<&'static str>::new()));
        let starts = log.clone();
        let updates = log.clone();
        let ends = log.clone();
        let taps = log.clone();
        let root = tree(
            PointerHandlers::new()
                .with_drag_start(move |_| starts.borrow_mut().push("start"))
                .with_drag_update(move |_| updates.borrow_mut().push("update"))
                .with_drag_end(move |_| ends.borrow_mut().push("end"))
                .with_tap(move |_| taps.borrow_mut().push("tap")),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        // Below the slop: no drag yet.
        router.dispatch(&root, &event(PointerChange::Move, 25.0, 20.0, 5.0, 0.0));
        assert!(log.borrow().is_empty());
        // Past it.
        router.dispatch(&root, &event(PointerChange::Move, 45.0, 20.0, 20.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Up, 45.0, 20.0, 0.0, 0.0));

        assert_eq!(*log.borrow(), vec!["start", "update", "end"]);
    }

    #[test]
    fn a_small_wobble_still_counts_as_a_tap() {
        let taps = Rc::new(RefCell::new(0));
        let sink = taps.clone();
        let root = tree(PointerHandlers::new().with_tap(move |_| *sink.borrow_mut() += 1));

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Move, 23.0, 22.0, 3.0, 2.0));
        router.dispatch(&root, &event(PointerChange::Up, 23.0, 22.0, 0.0, 0.0));
        assert_eq!(*taps.borrow(), 1);
    }

    #[test]
    fn the_pressed_state_goes_on_and_off() {
        let states = Rc::new(RefCell::new(Vec::new()));
        let sink = states.clone();
        let root = tree(
            PointerHandlers::new().with_press_change(move |pressed| sink.borrow_mut().push(pressed)),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        assert_eq!(*states.borrow(), vec![true]);
        router.dispatch(&root, &event(PointerChange::Up, 20.0, 20.0, 0.0, 0.0));
        assert_eq!(*states.borrow(), vec![true, false]);
    }

    #[test]
    fn sliding_off_clears_the_pressed_state_without_tapping() {
        let states = Rc::new(RefCell::new(Vec::new()));
        let taps = Rc::new(RefCell::new(0));
        let state_sink = states.clone();
        let tap_sink = taps.clone();
        let root = tree(
            PointerHandlers::new()
                .with_press_change(move |pressed| state_sink.borrow_mut().push(pressed))
                .with_tap(move |_| *tap_sink.borrow_mut() += 1),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Move, 60.0, 20.0, 40.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Up, 60.0, 20.0, 0.0, 0.0));

        assert_eq!(*states.borrow(), vec![true, false]);
        assert_eq!(*taps.borrow(), 0);
    }

    /// A row inside a scrolling list: the row takes the tap, the list takes the
    /// drag. Recording only the innermost hit means the list can never be
    /// scrolled, because every press lands on a row.
    fn nested(inner: PointerHandlers, outer: PointerHandlers) -> RenderStack {
        let mut stack = RenderStack::new().push(
            RenderPointerRegion::new(1, RenderPointerRegion::new(2, Sized(Size::square(80.0)))
                .with_handlers(inner))
                .with_handlers(outer),
        );
        stack.layout(BoxConstraints::tight(100.0, 100.0));
        stack
    }

    /// One event of a vertical swipe, at a stated moment.
    ///
    /// Velocity is the one thing in this file that cannot be tested without a
    /// clock: every other recogniser reads positions, and this one reads how
    /// long they took.
    fn at(change: PointerChange, y: f32, dy: f32, micros: i64) -> PointerEvent {
        PointerEvent {
            time_stamp_micros: micros,
            ..event(change, 40.0, y, 0.0, dy)
        }
    }

    /// Swipes upwards from (40, 70) at `per_frame` logical pixels every 60Hz
    /// frame, and returns the velocity the release reported.
    fn swipe(frames: i64, per_frame: f32, hold_before_lifting_micros: i64) -> Offset {
        let velocity = Rc::new(RefCell::new(Offset::ZERO));
        let sink = velocity.clone();
        // The 80x80 region, so that a swipe has somewhere to start: the
        // press is hit-tested and the moves after it are routed by pointer.
        let root = nested(
            PointerHandlers::new(),
            PointerHandlers::new()
                .with_drag_update(|_| {})
                .with_drag_end(move |end| *sink.borrow_mut() = end.velocity),
        );

        let mut router = GestureRouter::new();
        let mut y = 70.0;
        let mut now = 1_000_000;
        router.dispatch(&root, &at(PointerChange::Down, y, 0.0, now));
        for _ in 0..frames {
            now += 16_667;
            y -= per_frame;
            router.dispatch(&root, &at(PointerChange::Move, y, -per_frame, now));
        }
        now += hold_before_lifting_micros.max(16_667);
        router.dispatch(&root, &at(PointerChange::Up, y, 0.0, now));
        *velocity.borrow()
    }

    #[test]
    fn a_flick_reports_how_fast_it_was_going() {
        // 20 logical pixels every 60Hz frame is 1200 a second, upwards.
        let velocity = swipe(10, 20.0, 0);
        assert!(
            (velocity.dy + 1200.0).abs() < 60.0,
            "should be about -1200 px/s, not {}",
            velocity.dy
        );
        assert!(velocity.dx.abs() < 1.0, "and nothing sideways, not {}", velocity.dx);
    }

    #[test]
    fn a_slow_drag_is_not_a_fling() {
        // Under kMinFlingVelocity: far enough to be a drag, too slow to throw.
        let velocity = swipe(40, 0.5, 0);
        assert_eq!(velocity, Offset::ZERO);
    }

    #[test]
    fn a_drag_that_stops_before_lifting_is_not_a_fling() {
        // The gesture everyone makes without thinking about it: swipe, hold
        // still to read what arrived, lift. The content must stay put.
        let velocity = swipe(10, 20.0, 120_000);
        assert_eq!(velocity, Offset::ZERO);
    }

    #[test]
    fn a_twitch_in_place_is_not_a_fling() {
        // Fast enough, but it went nowhere -- a finger settling on the glass.
        // Upstream wants distance as well as speed for the same reason.
        let velocity = swipe(3, 4.0, 0);
        assert_eq!(velocity, Offset::ZERO);
    }

    #[test]
    fn an_impossibly_fast_flick_is_clamped() {
        let velocity = swipe(6, 400.0, 0);
        assert!(
            (velocity.dy + MAX_FLING_VELOCITY).abs() < 1.0,
            "should be held at the maximum, not {}",
            velocity.dy
        );
    }

    #[test]
    fn a_cancelled_drag_does_not_fling() {
        let velocity = Rc::new(RefCell::new(Offset::new(f32::NAN, f32::NAN)));
        let sink = velocity.clone();
        // The 80x80 region, so that a swipe has somewhere to start: the
        // press is hit-tested and the moves after it are routed by pointer.
        let root = nested(
            PointerHandlers::new(),
            PointerHandlers::new()
                .with_drag_update(|_| {})
                .with_drag_end(move |end| *sink.borrow_mut() = end.velocity),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &at(PointerChange::Down, 70.0, 0.0, 1_000_000));
        for step in 1..=6 {
            router.dispatch(&root, &at(
                PointerChange::Move,
                70.0 - 20.0 * step as f32,
                -20.0,
                1_000_000 + step * 16_667,
            ));
        }
        router.dispatch(&root, &at(PointerChange::Cancel, 0.0, 0.0, 1_120_000));

        assert_eq!(*velocity.borrow(), Offset::ZERO, "a cancelled drag throws nothing");
    }

    #[test]
    fn a_press_reaches_the_scrollable_as_well_as_the_row() {
        // What lets a list be caught mid-fling: the finger lands on a row, and
        // the list above it hears about it anyway -- before the slop, because
        // stopping a fling a slop's worth of travel late is not stopping it.
        let row = Rc::new(RefCell::new(0));
        let list = Rc::new(RefCell::new(0));
        let row_sink = row.clone();
        let list_sink = list.clone();

        let root = nested(
            PointerHandlers::new().with_pointer_down(move |_| *row_sink.borrow_mut() += 1),
            PointerHandlers::new()
                .with_pointer_down(move |_| *list_sink.borrow_mut() += 1)
                .with_drag_update(|_| {}),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 40.0, 40.0, 0.0, 0.0));

        assert_eq!(*row.borrow(), 1);
        assert_eq!(*list.borrow(), 1);
    }

    #[test]
    fn a_region_that_is_both_row_and_list_is_only_told_once() {
        let downs = Rc::new(RefCell::new(0));
        let sink = downs.clone();
        let root = tree(
            PointerHandlers::new()
                .with_pointer_down(move |_| *sink.borrow_mut() += 1)
                .with_drag_update(|_| {}),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));

        assert_eq!(*downs.borrow(), 1);
    }

    #[test]
    fn a_drag_reaches_the_scrollable_above_the_row_it_started_on() {
        let taps = Rc::new(RefCell::new(0));
        let dragged = Rc::new(RefCell::new(0.0_f32));
        let tap_sink = taps.clone();
        let drag_sink = dragged.clone();

        let root = nested(
            PointerHandlers::new().with_tap(move |_| *tap_sink.borrow_mut() += 1),
            PointerHandlers::new()
                .with_drag_update(move |drag| *drag_sink.borrow_mut() += drag.delta.dy),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 40.0, 40.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Move, 40.0, 70.0, 0.0, 30.0));
        router.dispatch(&root, &event(PointerChange::Up, 40.0, 70.0, 0.0, 0.0));

        assert_eq!(*dragged.borrow(), 30.0, "the list should have been scrolled");
        assert_eq!(*taps.borrow(), 0, "a scroll is not a tap on the row it began on");
    }

    #[test]
    fn a_press_that_does_not_travel_still_taps_the_row() {
        let taps = Rc::new(RefCell::new(0));
        let dragged = Rc::new(RefCell::new(0.0_f32));
        let tap_sink = taps.clone();
        let drag_sink = dragged.clone();

        let root = nested(
            PointerHandlers::new().with_tap(move |_| *tap_sink.borrow_mut() += 1),
            PointerHandlers::new()
                .with_drag_update(move |drag| *drag_sink.borrow_mut() += drag.delta.dy),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 40.0, 40.0, 0.0, 0.0));
        // Inside the slop, so it is a tap and not a scroll.
        router.dispatch(&root, &event(PointerChange::Move, 40.0, 44.0, 0.0, 4.0));
        router.dispatch(&root, &event(PointerChange::Up, 40.0, 44.0, 0.0, 0.0));

        assert_eq!(*taps.borrow(), 1);
        assert_eq!(*dragged.borrow(), 0.0);
    }

    #[test]
    fn a_scroll_finds_the_innermost_region_that_wants_one() {
        let inner = Rc::new(RefCell::new(0.0_f32));
        let outer = Rc::new(RefCell::new(0.0_f32));
        let inner_sink = inner.clone();
        let outer_sink = outer.clone();

        let root = nested(
            PointerHandlers::new().with_scroll(move |s| *inner_sink.borrow_mut() += s.delta.dy),
            PointerHandlers::new().with_scroll(move |s| *outer_sink.borrow_mut() += s.delta.dy),
        );

        let mut router = GestureRouter::new();
        let mut wheel = event(PointerChange::Hover, 40.0, 40.0, 0.0, 0.0);
        wheel.signal_kind = SignalKind::Scroll;
        wheel.scroll_delta = Offset::new(0.0, 53.0);
        assert!(router.dispatch(&root, &wheel));

        assert_eq!(*inner.borrow(), 53.0);
        assert_eq!(*outer.borrow(), 0.0, "the inner one took it");
    }

    #[test]
    fn a_scroll_falls_outward_when_the_row_under_it_does_not_scroll() {
        let outer = Rc::new(RefCell::new(0.0_f32));
        let sink = outer.clone();

        let root = nested(
            // A tappable row, which is not a scrollable.
            PointerHandlers::new().with_tap(|_| {}),
            PointerHandlers::new().with_scroll(move |s| *sink.borrow_mut() += s.delta.dy),
        );

        let mut router = GestureRouter::new();
        let mut wheel = event(PointerChange::Hover, 40.0, 40.0, 0.0, 0.0);
        wheel.signal_kind = SignalKind::Scroll;
        wheel.scroll_delta = Offset::new(0.0, -20.0);
        assert!(router.dispatch(&root, &wheel));

        assert_eq!(*outer.borrow(), -20.0);
    }

    #[test]
    fn a_cancelled_press_is_not_a_tap() {
        let taps = Rc::new(RefCell::new(0));
        let sink = taps.clone();
        let root = tree(PointerHandlers::new().with_tap(move |_| *sink.borrow_mut() += 1));

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Cancel, 20.0, 20.0, 0.0, 0.0));
        assert_eq!(*taps.borrow(), 0);
        assert_eq!(router.active_count(), 0);
    }

    #[test]
    fn an_up_without_a_down_is_ignored() {
        let root = tree(PointerHandlers::new().with_tap(|_| panic!("should not fire")));
        let mut router = GestureRouter::new();
        assert!(!router.dispatch(&root, &event(PointerChange::Up, 20.0, 20.0, 0.0, 0.0)));
    }

    // -- Long press -----------------------------------------------------------

    /// One event of a press held in place, at a stated moment.
    fn held(change: PointerChange, micros: i64) -> PointerEvent {
        PointerEvent { time_stamp_micros: micros, ..event(change, 20.0, 20.0, 0.0, 0.0) }
    }

    #[test]
    fn a_press_held_still_becomes_a_long_press() {
        let presses = Rc::new(RefCell::new(0));
        let sink = presses.clone();
        let root = tree(PointerHandlers::new().with_long_press(move |_| *sink.borrow_mut() += 1));

        let mut router = GestureRouter::new();
        router.dispatch(&root, &held(PointerChange::Down, 0));
        assert!(router.awaits_deadline(0), "the clock is what decides this one");

        // A frame before the deadline, and one after it.
        assert!(router.tick(400_000));
        assert_eq!(*presses.borrow(), 0);
        router.tick(LONG_PRESS_TIMEOUT_MICROS);
        assert_eq!(*presses.borrow(), 1);
    }

    #[test]
    fn a_long_press_fires_once_and_is_not_also_a_tap() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let long = log.clone();
        let tap = log.clone();
        let root = tree(
            PointerHandlers::new()
                .with_long_press(move |_| long.borrow_mut().push("long"))
                .with_tap(move |_| tap.borrow_mut().push("tap")),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &held(PointerChange::Down, 0));
        router.tick(600_000);
        router.tick(900_000);
        router.dispatch(&root, &held(PointerChange::Up, 1_000_000));
        assert_eq!(*log.borrow(), vec!["long"]);
    }

    #[test]
    fn a_press_that_travels_is_not_a_long_press() {
        let presses = Rc::new(RefCell::new(0));
        let sink = presses.clone();
        // The long press is on the inner region, which is what a press is
        // reported to; the drag is on the outer one, the way a row inside a
        // list is arranged.
        let root = nested(
            PointerHandlers::new().with_long_press(move |_| *sink.borrow_mut() += 1),
            PointerHandlers::new().with_drag_update(|_| {}),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &at(PointerChange::Down, 70.0, 0.0, 0));
        router.dispatch(&root, &at(PointerChange::Move, 40.0, -30.0, 100_000));
        assert!(!router.awaits_deadline(100_000), "a scroll is not a long press");
        router.tick(700_000);
        assert_eq!(*presses.borrow(), 0);
    }

    #[test]
    fn a_press_that_lifts_early_is_not_a_long_press() {
        let presses = Rc::new(RefCell::new(0));
        let sink = presses.clone();
        let root = tree(PointerHandlers::new().with_long_press(move |_| *sink.borrow_mut() += 1));

        let mut router = GestureRouter::new();
        router.dispatch(&root, &held(PointerChange::Down, 0));
        router.dispatch(&root, &held(PointerChange::Up, 200_000));
        router.tick(900_000);
        assert_eq!(*presses.borrow(), 0);
    }

    // -- Double tap -----------------------------------------------------------

    /// Taps at `micros`, holding the finger down for 30ms.
    fn tap_at(router: &mut GestureRouter, root: &dyn RenderBox, micros: i64) {
        router.dispatch(root, &held(PointerChange::Down, micros));
        router.dispatch(root, &held(PointerChange::Up, micros + 30_000));
    }

    #[test]
    fn two_taps_close_together_are_a_double_tap() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let single = log.clone();
        let double = log.clone();
        let root = tree(
            PointerHandlers::new()
                .with_tap(move |_| single.borrow_mut().push("tap"))
                .with_double_tap(move |_| double.borrow_mut().push("double")),
        );

        let mut router = GestureRouter::new();
        tap_at(&mut router, &root, 0);
        // Nothing yet: this could still be the first half of a double tap, and
        // reporting it now would mean reporting both.
        assert!(log.borrow().is_empty());
        tap_at(&mut router, &root, 150_000);
        assert_eq!(*log.borrow(), vec!["double"]);
    }

    #[test]
    fn a_lone_tap_arrives_late_rather_than_never() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let single = log.clone();
        let root = tree(
            PointerHandlers::new()
                .with_tap(move |_| single.borrow_mut().push("tap"))
                .with_double_tap(|_| panic!("only one tap happened")),
        );

        let mut router = GestureRouter::new();
        tap_at(&mut router, &root, 0);
        assert!(log.borrow().is_empty());
        router.tick(200_000);
        assert!(log.borrow().is_empty(), "the window has not closed yet");
        router.tick(DOUBLE_TAP_TIMEOUT_MICROS);
        assert_eq!(*log.borrow(), vec!["tap"]);
    }

    #[test]
    fn a_tap_on_a_region_that_wants_no_double_tap_is_not_delayed() {
        let taps = Rc::new(RefCell::new(0));
        let sink = taps.clone();
        let root = tree(PointerHandlers::new().with_tap(move |_| *sink.borrow_mut() += 1));

        let mut router = GestureRouter::new();
        tap_at(&mut router, &root, 0);
        assert_eq!(*taps.borrow(), 1, "nothing was waiting on a second tap");
    }

    #[test]
    fn two_taps_far_apart_in_time_are_two_taps() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let single = log.clone();
        let root = tree(
            PointerHandlers::new()
                .with_tap(move |_| single.borrow_mut().push("tap"))
                .with_double_tap(|_| panic!("half a second apart is not a double tap")),
        );

        let mut router = GestureRouter::new();
        tap_at(&mut router, &root, 0);
        router.tick(DOUBLE_TAP_TIMEOUT_MICROS);
        tap_at(&mut router, &root, 500_000);
        router.tick(500_000 + DOUBLE_TAP_TIMEOUT_MICROS);
        assert_eq!(*log.borrow(), vec!["tap", "tap"]);
    }

    #[test]
    fn two_taps_in_different_places_are_two_taps() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let single = log.clone();
        // The 80x80 region: two taps have to land more than a hundred logical
        // pixels apart to be different taps, which does not fit in the 40x40
        // one. That slop is deliberately generous upstream -- a finger aiming
        // at the same spot twice in a hurry misses it by a lot.
        let root = nested(
            PointerHandlers::new()
                .with_tap(move |_| single.borrow_mut().push("tap"))
                .with_double_tap(|_| panic!("the second tap landed elsewhere")),
            PointerHandlers::new(),
        );

        let mut router = GestureRouter::new();
        let corner = |change, x: f32, y: f32, micros| PointerEvent {
            time_stamp_micros: micros,
            ..event(change, x, y, 0.0, 0.0)
        };
        router.dispatch(&root, &corner(PointerChange::Down, 3.0, 3.0, 0));
        router.dispatch(&root, &corner(PointerChange::Up, 3.0, 3.0, 30_000));
        // Still inside the window, diagonally across the region: about 106
        // pixels away.
        router.dispatch(&root, &corner(PointerChange::Down, 78.0, 78.0, 100_000));
        router.dispatch(&root, &corner(PointerChange::Up, 78.0, 78.0, 130_000));
        assert_eq!(*log.borrow(), vec!["tap"], "the first tap was settled by the second press");
    }

    #[test]
    fn two_taps_survive_the_tree_being_rebuilt_between_them() {
        // The bug this test is written for: the two taps of a double tap are
        // separated by frames, and every frame builds a fresh set of handlers.
        // Anything that identified a region by the identity of its handlers
        // saw two different regions and reported two single taps.
        let log = Rc::new(RefCell::new(Vec::new()));
        let make = |log: Rc<RefCell<Vec<&'static str>>>| {
            let single = log.clone();
            let double = log;
            tree(
                PointerHandlers::new()
                    .with_tap(move |_| single.borrow_mut().push("tap"))
                    .with_double_tap(move |_| double.borrow_mut().push("double")),
            )
        };

        let mut router = GestureRouter::new();
        let first = make(log.clone());
        tap_at(&mut router, &first, 0);
        drop(first);

        let second = make(log.clone());
        tap_at(&mut router, &second, 150_000);
        assert_eq!(*log.borrow(), vec!["double"]);
    }

    #[test]
    fn a_second_tap_too_soon_is_one_bounce_of_a_finger() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let single = log.clone();
        let root = tree(
            PointerHandlers::new()
                .with_tap(move |_| single.borrow_mut().push("tap"))
                .with_double_tap(|_| panic!("20ms apart is a bounce, not two taps")),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &held(PointerChange::Down, 0));
        router.dispatch(&root, &held(PointerChange::Up, 10_000));
        router.dispatch(&root, &held(PointerChange::Down, 30_000));
        router.dispatch(&root, &held(PointerChange::Up, 40_000));
        router.tick(DOUBLE_TAP_TIMEOUT_MICROS * 2);
        assert_eq!(*log.borrow(), vec!["tap", "tap"]);
    }

    // -- Scale ----------------------------------------------------------------

    /// One finger's event, at a position and a moment.
    fn finger(
        pointer_id: i64,
        change: PointerChange,
        x: f32,
        y: f32,
        micros: i64,
    ) -> PointerEvent {
        PointerEvent {
            pointer_id,
            time_stamp_micros: micros,
            ..event(change, x, y, 0.0, 0.0)
        }
    }

    /// The 80x80 region with scale handlers that report into `sink`.
    fn pinchable(sink: Rc<RefCell<Vec<ScaleEvent>>>) -> RenderStack {
        let start = sink.clone();
        let update = sink.clone();
        let end = sink;
        nested(
            PointerHandlers::new(),
            PointerHandlers::new()
                .with_scale_start(move |s| start.borrow_mut().push(s))
                .with_scale_update(move |s| update.borrow_mut().push(s))
                .with_scale_end(move |s| end.borrow_mut().push(s)),
        )
    }

    #[test]
    fn two_fingers_spreading_report_a_scale_above_one() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let root = pinchable(events.clone());
        let mut router = GestureRouter::new();

        router.dispatch(&root, &finger(1, PointerChange::Down, 30.0, 40.0, 0));
        router.dispatch(&root, &finger(2, PointerChange::Down, 50.0, 40.0, 10_000));
        // 20 apart to start with, 40 apart after both fingers move outwards.
        router.dispatch(&root, &finger(1, PointerChange::Move, 20.0, 40.0, 20_000));
        router.dispatch(&root, &finger(2, PointerChange::Move, 60.0, 40.0, 30_000));

        let events = events.borrow();
        assert_eq!(events.first().map(|s| s.scale), Some(1.0), "a pinch starts at one");
        let last = events.last().expect("an update");
        assert!((last.scale - 2.0).abs() < 0.01, "fingers twice as far apart: {}", last.scale);
        // The focal point has not moved: the fingers moved apart symmetrically.
        assert!((last.focal_point.dx - 40.0).abs() < 0.01);
    }

    #[test]
    fn two_fingers_moving_together_are_a_scale_of_one_that_travels() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let root = pinchable(events.clone());
        let mut router = GestureRouter::new();

        router.dispatch(&root, &finger(1, PointerChange::Down, 30.0, 40.0, 0));
        router.dispatch(&root, &finger(2, PointerChange::Down, 50.0, 40.0, 10_000));
        router.dispatch(&root, &finger(1, PointerChange::Move, 30.0, 50.0, 20_000));
        router.dispatch(&root, &finger(2, PointerChange::Move, 50.0, 50.0, 30_000));

        let events = events.borrow();
        let last = events.last().expect("an update");
        assert!((last.scale - 1.0).abs() < 0.01, "no pinch, only a move");
        assert!((last.focal_point.dy - 50.0).abs() < 0.01, "the focal point followed them");
    }

    #[test]
    fn turning_two_fingers_reports_a_rotation() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let root = pinchable(events.clone());
        let mut router = GestureRouter::new();

        // Horizontal to start with, vertical after: a quarter turn.
        router.dispatch(&root, &finger(1, PointerChange::Down, 30.0, 40.0, 0));
        router.dispatch(&root, &finger(2, PointerChange::Down, 50.0, 40.0, 10_000));
        router.dispatch(&root, &finger(1, PointerChange::Move, 40.0, 30.0, 20_000));
        router.dispatch(&root, &finger(2, PointerChange::Move, 40.0, 50.0, 30_000));

        let events = events.borrow();
        let last = events.last().expect("an update");
        assert!(
            (last.rotation.abs() - std::f32::consts::FRAC_PI_2).abs() < 0.01,
            "a quarter turn, not {}",
            last.rotation
        );
    }

    #[test]
    fn a_pinch_ends_when_a_finger_leaves() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let root = pinchable(events.clone());
        let mut router = GestureRouter::new();

        router.dispatch(&root, &finger(1, PointerChange::Down, 30.0, 40.0, 0));
        router.dispatch(&root, &finger(2, PointerChange::Down, 50.0, 40.0, 10_000));
        router.dispatch(&root, &finger(2, PointerChange::Up, 50.0, 40.0, 20_000));
        assert_eq!(events.borrow().last().map(|s| s.pointer_count), Some(1));

        // The finger still down does not start a fresh pinch or a drag with
        // what is left.
        let before = events.borrow().len();
        router.dispatch(&root, &finger(1, PointerChange::Move, 10.0, 40.0, 30_000));
        assert_eq!(events.borrow().len(), before);
    }

    #[test]
    fn a_pinch_cancels_the_drag_it_started_as() {
        let ended = Rc::new(RefCell::new(Vec::new()));
        let sink = ended.clone();
        let root = nested(
            PointerHandlers::new(),
            PointerHandlers::new()
                .with_drag_update(|_| {})
                .with_drag_end(move |end| sink.borrow_mut().push(end.velocity))
                .with_scale_update(|_| {}),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &finger(1, PointerChange::Down, 30.0, 70.0, 0));
        // Past the slop, so a drag is under way. The delta is what the router
        // measures travel with -- see `on_move` -- so a move event without one
        // is a pointer that went nowhere.
        router.dispatch(
            &root,
            &PointerEvent {
                delta: Offset::new(0.0, -50.0),
                ..finger(1, PointerChange::Move, 30.0, 20.0, 16_000)
            },
        );
        router.dispatch(&root, &finger(2, PointerChange::Down, 50.0, 20.0, 32_000));

        assert_eq!(
            *ended.borrow(),
            vec![Offset::ZERO],
            "the drag ended, and a cancelled drag does not fling"
        );
    }

    // -- Hover ----------------------------------------------------------------

    /// A mouse at a position, hovering rather than pressing.
    fn hover_at(x: f32, y: f32) -> PointerEvent {
        PointerEvent {
            kind: PointerKind::Mouse,
            buttons: 0,
            ..event(PointerChange::Hover, x, y, 0.0, 0.0)
        }
    }

    #[test]
    fn a_mouse_entering_and_leaving_is_reported_once_each() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let sink = log.clone();
        let root = tree(PointerHandlers::new().with_hover_change(move |inside| {
            sink.borrow_mut().push(inside);
        }));

        let mut router = GestureRouter::new();
        // Outside the 40x40 region at (10,10), then inside it twice, then out.
        router.dispatch(&root, &hover_at(90.0, 90.0));
        assert!(log.borrow().is_empty());
        router.dispatch(&root, &hover_at(20.0, 20.0));
        router.dispatch(&root, &hover_at(30.0, 30.0));
        assert_eq!(*log.borrow(), vec![true], "entering is one event, not one per move");
        router.dispatch(&root, &hover_at(90.0, 90.0));
        assert_eq!(*log.borrow(), vec![true, false]);
    }

    #[test]
    fn moving_inside_a_region_reports_where() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        let root = tree(PointerHandlers::new().with_hover(move |hover| {
            sink.borrow_mut().push(hover.local_position);
        }));

        let mut router = GestureRouter::new();
        router.dispatch(&root, &hover_at(20.0, 20.0));
        // Local to the region, which starts at (10, 10).
        assert_eq!(seen.borrow().len(), 1);
        assert_eq!(seen.borrow()[0], Offset::new(10.0, 10.0));
    }

    #[test]
    fn a_button_inside_a_card_hovers_both() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let inner = log.clone();
        let outer = log.clone();
        let root = nested(
            PointerHandlers::new().with_hover_change(move |inside| {
                inner.borrow_mut().push(format!("inner:{inside}"));
            }),
            PointerHandlers::new().with_hover_change(move |inside| {
                outer.borrow_mut().push(format!("outer:{inside}"));
            }),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &hover_at(40.0, 40.0));
        assert_eq!(*log.borrow(), vec!["inner:true", "outer:true"]);
        router.dispatch(&root, &PointerEvent {
            change: PointerChange::Remove,
            ..hover_at(40.0, 40.0)
        });
        assert_eq!(
            *log.borrow(),
            vec!["inner:true", "outer:true", "inner:false", "outer:false"],
            "the pointer left the window, so nothing is hovered"
        );
    }

    #[test]
    fn a_press_is_not_a_hover() {
        let root = tree(PointerHandlers::new().with_hover_change(|_| {
            panic!("a finger touching the screen is not hovering over it")
        }));
        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Up, 20.0, 20.0, 0.0, 0.0));
    }
}
