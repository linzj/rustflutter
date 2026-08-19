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
//! further than [`compute_pan_slop`] for its kind of pointer stops being a
//! candidate tap and becomes a drag. Which of the recognisers watching a press
//! gets to say what it was is decided by [`GestureArena`] -- a port of
//! upstream's `arena.dart`, one arena per pointer, members in the order they
//! joined, the sweep on pointer up favouring the first member still standing.

use std::rc::Rc;

use crate::render::{HitTestEntry, HitTestResult, Offset, RenderBox};

/// How far a touch has to travel before the framework is confident that it is
/// a scroll, or inversely the maximum distance it can travel before the
/// framework is confident that it is not a tap. Upstream's `kTouchSlop`.
pub const TOUCH_SLOP: f32 = 18.0;

/// The same, for pointers that go exactly where they are aimed. Upstream's
/// `kPrecisePointerHitSlop`: a mouse does not land somewhere near its target,
/// so it is not forgiven the wobble a finger is.
pub const PRECISE_POINTER_HIT_SLOP: f32 = 1.0;

/// How far a touch has to travel for the framework to be confident that the
/// gesture is a panning one -- twice the touch slop, because a drag free to
/// wander in both axes has twice the room to wobble in. Upstream's `kPanSlop`.
pub const PAN_SLOP: f32 = TOUCH_SLOP * 2.0;

/// [`PAN_SLOP`] for a precise pointer. Upstream's `kPrecisePointerPanSlop`.
pub const PRECISE_POINTER_PAN_SLOP: f32 = PRECISE_POINTER_HIT_SLOP * 2.0;

/// How far a pointer has to travel before the movement counts, by what kind of
/// pointer it is.
///
/// Upstream's `computeHitSlop` (`events.dart`): a mouse gets a single logical
/// pixel, and everything else -- touch, stylus, trackpad, unknown -- gets
/// [`TOUCH_SLOP`]. The `DeviceGestureSettings` argument is left out because
/// this port has no platform to source one from.
pub fn compute_hit_slop(kind: PointerKind) -> f32 {
    match kind {
        PointerKind::Mouse => PRECISE_POINTER_HIT_SLOP,
        _ => TOUCH_SLOP,
    }
}

/// How far a pointer has to travel before the movement counts as a pan rather
/// than a wobble: [`compute_hit_slop`] doubled, the way a free drag is judged
/// against `kPanSlop` rather than the touch slop. Upstream's `computePanSlop`
/// (`events.dart`).
pub fn compute_pan_slop(kind: PointerKind) -> f32 {
    match kind {
        PointerKind::Mouse => PRECISE_POINTER_PAN_SLOP,
        _ => PAN_SLOP,
    }
}

/// Bit in `PointerEvent::buttons` for the primary button -- the left mouse
/// button, a touch, the tip of a stylus. Upstream's `kPrimaryButton`.
pub const PRIMARY_BUTTON: i32 = 0x01;

/// Bit in `PointerEvent::buttons` for the secondary button -- the right mouse
/// button, the barrel of a stylus. Upstream's `kSecondaryButton`.
pub const SECONDARY_BUTTON: i32 = 0x02;

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

/// Upstream's `kPressTimeout` (`constants.dart`): how long a finger has to
/// rest before a recogniser may act on the press without knowing yet whether
/// it will become something else.
///
/// A fifth of the long-press timeout. Long enough that a tap that is really a
/// flick does not trigger it, short enough that a finger deliberately held
/// feels answered.
pub const PRESS_TIMEOUT_MICROS: i64 = 100_000;

/// The longest a tap waits to be the first of a double tap: measured from its
/// own lift to the second tap's press, which is where upstream starts the
/// clock -- `_registerFirstTap` calls `_startDoubleTapTimer` on the first up.
/// Upstream's `kDoubleTapTimeout`.
pub const DOUBLE_TAP_TIMEOUT_MICROS: i64 = 300_000;

/// The shortest gap that does, measured from the first tap's press to the
/// second tap's press: two "taps" closer together than this are one bounce of
/// a finger, not two taps. Upstream's `_TapTracker` starts this countdown when
/// it is created, at the down. Upstream's `kDoubleTapMinTime`.
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
        VelocityTracker {
            samples: Vec::with_capacity(VelocityTracker::HISTORY),
        }
    }

    /// Records where the pointer was at `time_micros`.
    pub fn add_position(&mut self, time_micros: i64, position: Offset) {
        if self.samples.len() == VelocityTracker::HISTORY {
            self.samples.remove(0);
        }
        self.samples.push(Sample {
            time_micros,
            position,
        });
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
            if age > VelocityTracker::HORIZON_MICROS || gap > VelocityTracker::ASSUME_STOPPED_MICROS
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
        VelocityEstimate {
            pixels_per_second: Offset::ZERO,
            offset,
            duration_micros,
        }
    }

    /// The velocity to hand to a fling, or zero if this release was not one.
    ///
    /// Both halves of upstream's `PanGestureRecognizer`: `isFlingGesture`,
    /// which wants speed *and* distance -- a slow careful drag and a twitch in
    /// place are both releases that should leave the content where it is, with
    /// the distance judged against [`compute_hit_slop`] for the pointer's kind
    /// -- and `Velocity.clampMagnitude`, which is applied along the vector so
    /// that clamping a diagonal fling does not turn it.
    pub fn fling_velocity(&self, now_micros: i64, kind: PointerKind) -> Offset {
        let estimate = self.estimate(now_micros);
        let speed_squared = estimate.pixels_per_second.distance_squared();
        let min_distance = compute_hit_slop(kind);
        let travelled_squared = estimate.offset.distance_squared();
        if speed_squared <= MIN_FLING_VELOCITY * MIN_FLING_VELOCITY
            || travelled_squared <= min_distance * min_distance
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

    let dot =
        |left: &[f32], right: &[f32]| -> f32 { left.iter().zip(right).map(|(l, r)| l * r).sum() };

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

/// Upstream `IOSScrollViewFlingVelocityTracker`: a velocity that is a weighted
/// average of recent ones rather than a curve fitted through positions.
///
/// [`VelocityTracker`] fits a least-squares quadratic through the samples,
/// which is the right answer to "how fast is this pointer moving". This one
/// answers a different question -- "how fast would the platform's own scroll
/// view have flung" -- and matches it by taking three consecutive pairwise
/// velocities and weighting them. The point is not accuracy but *agreement*:
/// a list that flings differently from the platform's own lists is noticeable
/// in a way that a list that flings slightly wrong is not.
///
/// This was in `coverage_ledger.json` as out of scope, on the grounds that iOS
/// is not a host of this port. That was wrong in a way worth naming: macOS
/// *is* a host, and [`MacOSScrollViewFlingVelocityTracker`] extends this one.
/// A class the ledger had ruled irrelevant turned out to be the base of a
/// class that is not.
pub struct IOSScrollViewFlingVelocityTracker {
    kind: PointerKind,
    /// A ring of the last [`Self::SAMPLE_SIZE`] positions, newest at `index`.
    ///
    /// Upstream keeps twenty for a calculation that reads four. Its comment
    /// says why: the extra samples are there so that `VelocityEstimate.offset`
    /// grows big enough to pass the *distance* half of the fling test, which
    /// `VerticalDragGestureRecognizer.isFlingGesture` applies. The offset and
    /// the velocity are measured over different spans on purpose.
    samples: Vec<Option<Sample>>,
    index: usize,
    last_sample_micros: Option<i64>,
}

impl IOSScrollViewFlingVelocityTracker {
    /// Upstream's `_sampleSize`.
    pub const SAMPLE_SIZE: usize = 20;

    pub fn new(kind: PointerKind) -> IOSScrollViewFlingVelocityTracker {
        IOSScrollViewFlingVelocityTracker {
            kind,
            samples: vec![None; Self::SAMPLE_SIZE],
            index: 0,
            last_sample_micros: None,
        }
    }

    pub fn kind(&self) -> PointerKind {
        self.kind
    }

    /// Upstream's `addPosition`.
    pub fn add_position(&mut self, time_micros: i64, position: Offset) {
        self.last_sample_micros = Some(time_micros);
        self.index = (self.index + 1) % Self::SAMPLE_SIZE;
        self.samples[self.index] = Some(Sample {
            time_micros,
            position,
        });
    }

    /// Upstream's `_previousVelocityAt`: the velocity between two adjacent
    /// samples, counting backwards from the newest at index zero.
    pub fn previous_velocity_at(&self, index: i32) -> Offset {
        let size = Self::SAMPLE_SIZE as i32;
        let end_index = (self.index as i32 + index).rem_euclid(size) as usize;
        let start_index = (self.index as i32 + index - 1).rem_euclid(size) as usize;
        let (Some(end), Some(start)) = (self.samples[end_index], self.samples[start_index]) else {
            return Offset::ZERO;
        };
        let dt = end.time_micros - start.time_micros;
        if dt <= 0 {
            return Offset::ZERO;
        }
        let seconds = dt as f32 / 1_000_000.0;
        Offset::new(
            (end.position.dx - start.position.dx) / seconds,
            (end.position.dy - start.position.dy) / seconds,
        )
    }

    /// The newest sample, and the oldest one still in the ring.
    fn span(&self) -> Option<(Sample, Sample)> {
        let newest = self.samples[self.index]?;
        for step in 1..=Self::SAMPLE_SIZE {
            if let Some(oldest) = self.samples[(self.index + step) % Self::SAMPLE_SIZE] {
                return Some((newest, oldest));
            }
        }
        None
    }

    fn stopped(&self, now_micros: i64) -> bool {
        match self.last_sample_micros {
            Some(last) => now_micros - last > VelocityTracker::ASSUME_STOPPED_MICROS,
            None => true,
        }
    }

    fn estimate_from(&self, velocity: Offset, now_micros: i64) -> VelocityEstimate {
        if self.stopped(now_micros) {
            return VelocityEstimate::default();
        }
        match self.span() {
            Some((newest, oldest)) => VelocityEstimate {
                pixels_per_second: velocity,
                offset: Offset::new(
                    newest.position.dx - oldest.position.dx,
                    newest.position.dy - oldest.position.dy,
                ),
                duration_micros: newest.time_micros - oldest.time_micros,
            },
            None => VelocityEstimate::default(),
        }
    }

    /// Upstream's `getVelocityEstimate`: three consecutive pairwise
    /// velocities weighted 0.6, 0.35 and 0.05 from oldest to newest.
    ///
    /// **The freshest measurement is worth almost nothing** -- a twentieth of
    /// the answer -- and the oldest of the three carries most of it. A finger
    /// leaving the glass slows or twitches in its last milliseconds, so what it
    /// was doing a moment earlier is the better description of what the reader
    /// meant.
    ///
    /// Upstream's comment is careful about what this approximates: the scroll
    /// velocity of an iOS scroll view at the moment of release, and *not* what
    /// the pan recogniser on that scroll view would report, because the scroll
    /// view slows down when the touch is released and the two therefore differ.
    pub fn velocity_estimate(&self, now_micros: i64) -> VelocityEstimate {
        let (a, b, c) = (
            self.previous_velocity_at(-2),
            self.previous_velocity_at(-1),
            self.previous_velocity_at(0),
        );
        let velocity = Offset::new(
            a.dx * 0.6 + b.dx * 0.35 + c.dx * 0.05,
            a.dy * 0.6 + b.dy * 0.35 + c.dy * 0.05,
        );
        self.estimate_from(velocity, now_micros)
    }
}

/// Upstream `MacOSScrollViewFlingVelocityTracker`: the same three samples,
/// weighted differently.
///
/// Same three samples, differently weighted: 0.15, 0.65 and 0.2 from oldest to
/// newest, where iOS uses 0.6, 0.35 and 0.05. The shapes disagree about which
/// sample to trust -- iOS leans hardest on the oldest of the three, macOS on
/// the **middle** one -- and both discount the freshest almost to nothing,
/// because that one is the finger lifting rather than the reader flinging.
/// Neither is more correct; each matches its own platform's scroll views, which
/// is the only thing either is for.
pub struct MacOSScrollViewFlingVelocityTracker {
    base: IOSScrollViewFlingVelocityTracker,
}

impl MacOSScrollViewFlingVelocityTracker {
    pub fn new(kind: PointerKind) -> MacOSScrollViewFlingVelocityTracker {
        MacOSScrollViewFlingVelocityTracker {
            base: IOSScrollViewFlingVelocityTracker::new(kind),
        }
    }

    /// Upstream's `getVelocityEstimate`.
    pub fn velocity_estimate(&self, now_micros: i64) -> VelocityEstimate {
        let (a, b, c) = (
            self.base.previous_velocity_at(-2),
            self.base.previous_velocity_at(-1),
            self.base.previous_velocity_at(0),
        );
        let velocity = Offset::new(
            a.dx * 0.15 + b.dx * 0.65 + c.dx * 0.2,
            a.dy * 0.15 + b.dy * 0.65 + c.dy * 0.2,
        );
        self.base.estimate_from(velocity, now_micros)
    }
}

impl std::ops::Deref for MacOSScrollViewFlingVelocityTracker {
    type Target = IOSScrollViewFlingVelocityTracker;

    fn deref(&self) -> &IOSScrollViewFlingVelocityTracker {
        &self.base
    }
}

impl std::ops::DerefMut for MacOSScrollViewFlingVelocityTracker {
    fn deref_mut(&mut self) -> &mut IOSScrollViewFlingVelocityTracker {
        &mut self.base
    }
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
    /// Fired when the platform takes the pointer away -- a call arriving, a
    /// system gesture starting, the window losing the touch. Upstream's
    /// `Listener.onPointerCancel`, and the reason it is separate from an up:
    /// nothing was completed, so anything that was showing progress has to
    /// unwind rather than finish.
    pub on_pointer_cancel: Option<Rc<dyn Fn(&PointerEvent)>>,
    /// Fired when the press ends without having travelled past [`TOUCH_SLOP`].
    ///
    /// Held back when this region also wants double taps: the first tap of a
    /// double tap is indistinguishable from a single one until the window has
    /// passed. See [`GestureRouter::tick`].
    pub on_tap: Option<Rc<dyn Fn(TapEvent)>>,
    /// Fired when a secondary press -- the right mouse button, a stylus barrel
    /// -- ends without having travelled past [`TOUCH_SLOP`]. Upstream's
    /// `GestureDetector.onSecondaryTap`, and like upstream the primary tap
    /// callbacks never fire for such a press: the two are different
    /// recognisers, and only the one whose button this is joins the arena.
    pub on_secondary_tap: Option<Rc<dyn Fn(TapEvent)>>,
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

    pub fn with_secondary_tap(mut self, handler: impl Fn(TapEvent) + 'static) -> Self {
        self.on_secondary_tap = Some(Rc::new(handler));
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

    pub fn with_pointer_cancel(mut self, handler: impl Fn(&PointerEvent) + 'static) -> Self {
        self.on_pointer_cancel = Some(Rc::new(handler));
        self
    }

    /// Whether anything is listening. A region with nothing attached still
    /// participates in hit testing, so it can shield what is behind it.
    pub fn is_empty(&self) -> bool {
        self.on_pointer_down.is_none()
            && self.on_pointer_move.is_none()
            && self.on_pointer_up.is_none()
            && self.on_pointer_cancel.is_none()
            && self.on_tap.is_none()
            && self.on_secondary_tap.is_none()
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
        self.on_drag_start.is_some() || self.on_drag_update.is_some() || self.on_drag_end.is_some()
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

// -- The gesture arena ----------------------------------------------------------
//
// A port of upstream's `gestures/arena.dart`. Upstream the members are
// recogniser objects; here a recogniser is one of the router's own per-kind
// states on one region, so a member is named by what it recognises and which
// region it belongs to (`index` says which: see `ActivePointer`).

/// Upstream's `GestureDisposition`: what a member is telling the arena, and
/// what the arena tells a member when it hands out the verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    Accepted,
    Rejected,
}

/// Which recogniser an arena member is. The order the variants are added in is
/// upstream's `GestureDetector` recogniser-creation order -- tap, double tap,
/// long press, then the drags -- and it decides who wins a sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Recognizer {
    /// The primary-button tap family on one region.
    Tap,
    /// `on_secondary_tap` on one region.
    SecondaryTap,
    /// `on_double_tap` on the innermost region that wants it.
    DoubleTap,
    /// `on_long_press` on one region.
    LongPress,
    /// The innermost region on the path that wants drags.
    Drag,
    /// The innermost region on the path that wants two fingers.
    Scale,
}

/// One member of an arena: a recogniser on a region.
///
/// `index` identifies the region. For [`Recognizer::Tap`],
/// [`Recognizer::DoubleTap`] and [`Recognizer::LongPress`] it is a position in
/// [`ActivePointer::taps`]; for [`Recognizer::SecondaryTap`] a position in
/// [`ActivePointer::secondary`]; for [`Recognizer::Drag`] and
/// [`Recognizer::Scale`] it is unused, because those are recorded once per
/// pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Member {
    recognizer: Recognizer,
    index: usize,
}

impl Member {
    fn of(recognizer: Recognizer) -> Member {
        Member {
            recognizer,
            index: 0,
        }
    }
}

/// One verdict the arena has handed down: upstream's `acceptGesture` and
/// `rejectGesture` calls, which are delivered exactly once per member per
/// arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Verdict {
    member: Member,
    disposition: Disposition,
}

/// Upstream's `_GestureArena`: the recognisers competing for one pointer, and
/// the state that decides when they may find out who won.
///
/// * the arena is **open** while the pointer-down event is still being
///   dispatched; a member that accepts during that time can only be recorded
///   as the *eager winner*, because later members have not had their say yet;
/// * it closes once the down event has reached everyone, and then the first
///   member to accept wins outright;
/// * **holding** it (upstream `hold`) stops the sweep on pointer up, which is
///   how a double-tap recogniser keeps a single tap from being settled before
///   the window for the second tap has closed.
#[derive(Debug)]
struct GestureArena {
    /// In join order. A member that rejects is removed, so `first` is the
    /// first member that has not rejected.
    members: Vec<Member>,
    is_open: bool,
    is_held: bool,
    has_pending_sweep: bool,
    /// The member that accepted while the arena was still open, if any.
    eager_winner: Option<Member>,
}

impl GestureArena {
    fn new() -> GestureArena {
        GestureArena {
            members: Vec::new(),
            is_open: true,
            is_held: false,
            has_pending_sweep: false,
            eager_winner: None,
        }
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
    /// Everything on the hit-test path that asked for raw pointer events, in
    /// the order it was hit. Recorded on the way down, because the tree may be
    /// rebuilt before the finger lifts.
    listeners: Vec<Target>,
    /// The innermost region under the press that wants two fingers, if any.
    /// Recorded on the way down like the other two, because by the time a
    /// second finger lands the tree may have been rebuilt.
    scale: Option<Target>,
    /// Every region on the hit-test path that wants a primary-button tap, a
    /// double tap or a long press, innermost first. One arena member each:
    /// upstream every recogniser along the path joins the arena, and the
    /// innermost one is added first, which is what makes it win the sweep.
    taps: Vec<Target>,
    /// Every region on the path that wants a secondary-button tap, innermost
    /// first. These only join when the press *is* a secondary one, the way the
    /// primary tap family only joins when it is not.
    secondary: Vec<Target>,
    /// The innermost region in `taps` that wants double taps. A double tap
    /// spans two pointers, so its recogniser cannot be per-pointer like the
    /// others; the held-back first tap is what carries it across the gap.
    double: Option<Target>,
    /// The member that won this pointer's arena, and the recognisers that
    /// lost it. A loser must not fire: winning is upstream's
    /// `acceptGesture`, losing its `rejectGesture`.
    winner: Option<Member>,
    rejected: Vec<Recognizer>,
    /// Whether the drag member has lost the arena. Kept apart from
    /// [`Self::rejected`] because the drag callbacks are spread over move and
    /// up, and "no drag may start or end" is the one thing a loss has to
    /// enforce after the fact.
    drag_rejected: bool,
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
    /// What kind of pointer this is, remembered from the down because how far
    /// it must travel to count as a drag or a fling depends on it. See
    /// [`compute_pan_slop`] and [`compute_hit_slop`].
    kind: PointerKind,
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
#[derive(Clone)]
struct PendingTap {
    target: Target,
    event: TapEvent,
    /// Which pointer's arena is being held open while this tap waits. Upstream
    /// the double-tap recogniser calls `hold` on it when the first tap lifts,
    /// and it is that hold -- not patience -- that keeps the sweep from
    /// settling the pointer as a single tap.
    pointer_id: i64,
    /// Where the finger landed, in view coordinates, and when the press
    /// started and ended. A second tap has to land near `origin`, press no
    /// later than [`DOUBLE_TAP_TIMEOUT_MICROS`] after `up_micros` -- upstream
    /// starts that clock at the first tap's lift, in `_registerFirstTap` --
    /// and no sooner than [`DOUBLE_TAP_MIN_TIME_MICROS`] after `down_micros`,
    /// where upstream's `_TapTracker` begins its countdown. The last of these
    /// is what keeps one finger bouncing from counting twice.
    origin: Offset,
    down_micros: i64,
    up_micros: i64,
}

/// Halfway between two points, which is where a two-finger gesture happens.
fn midpoint(a: Offset, b: Offset) -> Offset {
    Offset {
        dx: (a.dx + b.dx) / 2.0,
        dy: (a.dy + b.dy) / 2.0,
    }
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

/// Delivers verdicts to a pointer's recogniser states. Upstream each
/// recogniser does this itself in `acceptGesture` and `rejectGesture`: the
/// winner remembers it won, and a loser may not fire again for this pointer.
fn apply_verdicts_to(pointer: &mut ActivePointer, verdicts: Vec<Verdict>) {
    for verdict in verdicts {
        match verdict.disposition {
            Disposition::Accepted => {
                pointer.winner.get_or_insert(verdict.member);
            }
            Disposition::Rejected => {
                if !pointer.rejected.contains(&verdict.member.recognizer) {
                    pointer.rejected.push(verdict.member.recognizer);
                }
                if verdict.member.recognizer == Recognizer::Drag {
                    pointer.drag_rejected = true;
                }
            }
        }
    }
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
    /// One arena per pressed pointer, upstream's `_arenas` map. Removed as
    /// soon as the pointer's gesture has been decided, so a resolve that
    /// arrives after the fact -- a cancelled drag, say -- is a no-op, exactly
    /// as upstream's `GestureArenaEntry.resolve` on a resolved arena.
    arenas: Vec<(i64, GestureArena)>,
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
            arenas: Vec::new(),
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
                    .taps
                    .iter()
                    .any(|target| target.handlers.on_long_press.is_some())
        });
        let waiting_to_be_a_double_tap = self
            .pending_tap
            .as_ref()
            .is_some_and(|tap| now_micros - tap.up_micros < DOUBLE_TAP_TIMEOUT_MICROS);
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
                        .taps
                        .iter()
                        .any(|target| target.handlers.on_long_press.is_some())
            })
            .map(|(id, _)| *id)
            .collect();

        for id in due {
            let Some(pointer) = self.find(id) else {
                continue;
            };
            let Some(index) = pointer
                .taps
                .iter()
                .position(|target| target.handlers.on_long_press.is_some())
            else {
                continue;
            };
            let member = Member {
                recognizer: Recognizer::LongPress,
                index,
            };
            let target = pointer.taps[index].clone();
            // The long press claims the arena outright the moment its deadline
            // passes (upstream resolves accepted from `didExceedDeadline`),
            // which is also what tells the tap and the drag that they have
            // lost: a press that has become a long press is nothing else.
            let already_won = self.won_by(id, Recognizer::LongPress);
            let verdicts = self.arena_resolve(id, member, Disposition::Accepted);
            let won = already_won
                || verdicts.iter().any(|verdict| {
                    verdict.member == member && verdict.disposition == Disposition::Accepted
                });
            self.apply_verdicts(id, verdicts);
            if !won {
                continue;
            }
            let Some(pointer) = self.find(id) else {
                continue;
            };
            pointer.long_pressed = true;
            let was_pressed = pointer.pressed;
            pointer.pressed = false;
            let local = target.local_origin;
            // The press has been claimed, so whatever was showing it as held
            // stops. Upstream this is the tap recogniser losing the arena.
            if was_pressed {
                if let Some(press_change) = &target.handlers.on_press_change {
                    press_change(false);
                }
            }
            if let Some(long_press) = &target.handlers.on_long_press {
                long_press(TapEvent {
                    local_position: local,
                    pointer_id: id,
                });
            }
        }
    }

    /// Announces a held-back tap once no second tap can still arrive.
    fn flush_expired_tap(&mut self, now_micros: i64) {
        let expired = self
            .pending_tap
            .as_ref()
            .is_some_and(|tap| now_micros - tap.up_micros >= DOUBLE_TAP_TIMEOUT_MICROS);
        if !expired {
            return;
        }
        // The double-tap window has closed, so the double-tap member leaves
        // the arena it has been holding against the sweep, and the plain tap
        // waiting behind it wins by default.
        self.settle_pending_tap(Disposition::Rejected);
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

    // -- The arena manager ----------------------------------------------------
    //
    // Upstream's `GestureArenaManager`, one method to one method. Verdicts are
    // collected and returned rather than delivered on the spot, because the
    // state machine must not know what a recogniser does with its victory;
    // `apply_verdicts` is the delivery.

    /// Upstream `add`: a recogniser joins its pointer's arena, opening one if
    /// this is the first member.
    fn arena_add(&mut self, pointer: i64, member: Member) {
        let arena = match self.arenas.iter_mut().find(|(id, _)| *id == pointer) {
            Some((_, arena)) => arena,
            None => {
                self.arenas.push((pointer, GestureArena::new()));
                &mut self.arenas.last_mut().expect("just pushed").1
            }
        };
        arena.members.push(member);
    }

    /// Upstream `close`: the pointer-down event has finished dispatching, so
    /// no further members can arrive and an arena with a single member left is
    /// decided on the spot.
    fn arena_close(&mut self, pointer: i64) -> Vec<Verdict> {
        if let Some((_, arena)) = self.arenas.iter_mut().find(|(id, _)| *id == pointer) {
            arena.is_open = false;
        }
        self.try_to_resolve_arena(pointer)
    }

    /// Upstream `sweep`: the pointer has lifted, so the first member that has
    /// not rejected wins rather than anyone waiting to be convinced.
    fn arena_sweep(&mut self, pointer: i64) -> Vec<Verdict> {
        let Some(index) = self.arenas.iter().position(|(id, _)| *id == pointer) else {
            return Vec::new(); // Already resolved, or never opened.
        };
        if self.arenas[index].1.is_held {
            // Held for a long-lived member; the sweep waits for a release.
            self.arenas[index].1.has_pending_sweep = true;
            return Vec::new();
        }
        let (_, arena) = self.arenas.remove(index);
        arena
            .members
            .iter()
            .enumerate()
            .map(|(position, member)| Verdict {
                member: *member,
                disposition: if position == 0 {
                    Disposition::Accepted
                } else {
                    Disposition::Rejected
                },
            })
            .collect()
    }

    /// Upstream `hold`: stop the arena being swept.
    fn arena_hold(&mut self, pointer: i64) {
        if let Some((_, arena)) = self.arenas.iter_mut().find(|(id, _)| *id == pointer) {
            arena.is_held = true;
        }
    }

    /// Upstream `release`: let the arena be swept, and sweep it at once if a
    /// sweep was attempted while it was held.
    fn arena_release(&mut self, pointer: i64) -> Vec<Verdict> {
        let held_with_sweep = self
            .arenas
            .iter()
            .find(|(id, _)| *id == pointer)
            .is_some_and(|(_, arena)| arena.is_held && arena.has_pending_sweep);
        if let Some((_, arena)) = self.arenas.iter_mut().find(|(id, _)| *id == pointer) {
            arena.is_held = false;
        }
        if held_with_sweep {
            return self.arena_sweep(pointer);
        }
        Vec::new()
    }

    /// Upstream `_resolve`, reached through a member's `GestureArenaEntry`:
    /// the member claims the gesture or admits it is not going to happen.
    /// Resolving an arena that is already resolved is a no-op, by design --
    /// that is what keeps a cancelled drag from being told twice.
    fn arena_resolve(
        &mut self,
        pointer: i64,
        member: Member,
        disposition: Disposition,
    ) -> Vec<Verdict> {
        let Some(index) = self.arenas.iter().position(|(id, _)| *id == pointer) else {
            return Vec::new(); // Already resolved.
        };
        match disposition {
            Disposition::Accepted => {
                if self.arenas[index].1.is_open {
                    // The arena is still filling up, so an early claim can only
                    // be noted; it becomes a victory when the arena closes.
                    self.arenas[index].1.eager_winner.get_or_insert(member);
                    Vec::new()
                } else {
                    self.resolve_in_favor_of(pointer, member)
                }
            }
            Disposition::Rejected => {
                let is_open;
                {
                    let (_, arena) = &mut self.arenas[index];
                    if arena.eager_winner == Some(member) {
                        arena.eager_winner = None;
                    }
                    arena.members.retain(|standing| *standing != member);
                    is_open = arena.is_open;
                }
                let mut verdicts = vec![Verdict {
                    member,
                    disposition: Disposition::Rejected,
                }];
                if !is_open {
                    verdicts.extend(self.try_to_resolve_arena(pointer));
                }
                verdicts
            }
        }
    }

    /// Upstream `_tryToResolveArena`, called once the arena is closed: a lone
    /// survivor wins by default, an empty arena disappears, and an eager
    /// winner takes the lot.
    fn try_to_resolve_arena(&mut self, pointer: i64) -> Vec<Verdict> {
        let Some((_, arena)) = self.arenas.iter().find(|(id, _)| *id == pointer) else {
            return Vec::new();
        };
        if arena.members.len() == 1 {
            // Upstream schedules this as a microtask so the caller finishes
            // first; resolving inline is the same thing here, where nothing
            // runs in between.
            let member = arena.members[0];
            return self.resolve_by_default(pointer, member);
        }
        if arena.members.is_empty() {
            self.arenas.retain(|(id, _)| *id != pointer);
            return Vec::new();
        }
        if let Some(eager) = arena.eager_winner {
            return self.resolve_in_favor_of(pointer, eager);
        }
        Vec::new()
    }

    /// Upstream `_resolveByDefault`: the arena's only member has won.
    fn resolve_by_default(&mut self, pointer: i64, member: Member) -> Vec<Verdict> {
        let Some(index) = self.arenas.iter().position(|(id, _)| *id == pointer) else {
            return Vec::new(); // Someone resolved it first.
        };
        if self.arenas[index].1.members.first() != Some(&member) {
            return Vec::new(); // And not with this member alone.
        }
        self.arenas.remove(index);
        vec![Verdict {
            member,
            disposition: Disposition::Accepted,
        }]
    }

    /// Upstream `_resolveInFavorOf`: the arena is settled, everyone else is
    /// rejected, the loser notifications first as upstream sends them.
    fn resolve_in_favor_of(&mut self, pointer: i64, member: Member) -> Vec<Verdict> {
        let Some(index) = self.arenas.iter().position(|(id, _)| *id == pointer) else {
            return Vec::new();
        };
        let (_, arena) = self.arenas.remove(index);
        let mut verdicts: Vec<Verdict> = arena
            .members
            .iter()
            .filter(|standing| **standing != member)
            .map(|loser| Verdict {
                member: *loser,
                disposition: Disposition::Rejected,
            })
            .collect();
        verdicts.push(Verdict {
            member,
            disposition: Disposition::Accepted,
        });
        verdicts
    }

    /// The members still standing in a pointer's arena, in join order.
    fn alive_members(&self, pointer: i64) -> Vec<Member> {
        self.arenas
            .iter()
            .find(|(id, _)| *id == pointer)
            .map(|(_, arena)| arena.members.clone())
            .unwrap_or_default()
    }

    /// The standing member of `recognizer` in a pointer's arena, if any.
    fn member_of(&self, pointer: i64, recognizer: Recognizer) -> Option<Member> {
        self.arenas
            .iter()
            .find(|(id, _)| *id == pointer)
            .and_then(|(_, arena)| {
                arena
                    .members
                    .iter()
                    .find(|m| m.recognizer == recognizer)
                    .copied()
            })
    }

    /// Drops a pointer's arena without resolving it, for a down that arrives
    /// without the previous press having ended.
    fn take_arena(&mut self, pointer: i64) {
        self.arenas.retain(|(id, _)| *id != pointer);
    }

    /// Delivers verdicts to the pointer they are about: the winner is
    /// remembered, the losers are barred from firing.
    fn apply_verdicts(&mut self, pointer: i64, verdicts: Vec<Verdict>) {
        if let Some((_, pointer)) = self.active.iter_mut().find(|(id, _)| *id == pointer) {
            apply_verdicts_to(pointer, verdicts);
        }
    }

    /// Whether a recogniser has won this pointer's arena.
    fn won_by(&self, pointer: i64, recognizer: Recognizer) -> bool {
        self.active
            .iter()
            .find(|(id, _)| *id == pointer)
            .and_then(|(_, pointer)| pointer.winner)
            .is_some_and(|winner| winner.recognizer == recognizer)
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
            let Some(handlers) = entry.handlers.clone() else {
                continue;
            };
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
                hover(HoverEvent {
                    local_position: *local,
                });
                told_anyone = true;
            }
        }

        self.hovered = under
            .into_iter()
            .map(|(id, handlers, _)| (id, handlers))
            .collect();
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
            let Some(handlers) = &entry.handlers else {
                continue;
            };
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
        // The arena's membership: every region on the path that has
        // recognisers for what this press is. Which of the two tap families
        // joins is decided by the button -- upstream's tap recognisers reject
        // a pointer whose button is not theirs before it reaches the arena --
        // while drags and scales take any button.
        let mut taps: Vec<Target> = Vec::new();
        let mut secondary: Vec<Target> = Vec::new();
        for entry in &result.path {
            let Some(handlers) = entry.handlers.clone() else {
                continue;
            };
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
                    handlers: handlers.clone(),
                    id: entry.target,
                    local_origin: entry.local_position,
                });
            }
            let wants_primary = handlers.on_tap.is_some()
                || handlers.on_double_tap.is_some()
                || handlers.on_long_press.is_some();
            if event.buttons == PRIMARY_BUTTON && wants_primary {
                taps.push(Target {
                    handlers,
                    id: entry.target,
                    local_origin: entry.local_position,
                });
            } else if event.buttons == SECONDARY_BUTTON && handlers.on_secondary_tap.is_some() {
                secondary.push(Target {
                    handlers,
                    id: entry.target,
                    local_origin: entry.local_position,
                });
            }
        }
        if tap.is_none() && drag.is_none() && scale.is_none() {
            return false;
        }

        // Is this the second half of a double tap? Decided here rather than on
        // the way up, because it is the *start* of the second tap that the
        // window is measured to, and because a press that lands somewhere else
        // settles the first tap immediately instead of leaving it hanging.
        let second_tap = self.pair_with_pending_tap(&taps, event);

        // Every region on the path hears the raw pointer events -- not just
        // the one that will end up owning the gesture. That is upstream's
        // rule: a `PointerDownEvent` goes to every recogniser on the hit-test
        // path, because the arena has not decided anything yet. It is what
        // lets a finger landing on a flinging list stop it on contact rather
        // than a slop's worth of travel later, and what lets a splash start
        // under a button whose tap belongs to something else.
        let listeners: Vec<Target> = result
            .path
            .iter()
            .filter_map(|entry| {
                let handlers = entry.handlers.clone()?;
                let listens = handlers.on_pointer_down.is_some()
                    || handlers.on_pointer_move.is_some()
                    || handlers.on_pointer_up.is_some()
                    || handlers.on_pointer_cancel.is_some();
                listens.then_some(Target {
                    handlers,
                    id: entry.target,
                    local_origin: entry.local_position,
                })
            })
            .collect();

        for target in &listeners {
            if let Some(down) = &target.handlers.on_pointer_down {
                let mut local_event = *event;
                local_event.local_position = target.local_origin;
                down(&local_event);
            }
        }

        // Being *pressed* is a different question from hearing the event: it
        // is an affordance -- this is the thing you are about to activate --
        // and only one thing can be that.
        let mut pressed = false;
        if let Some(target) = &tap {
            if let Some(press_change) = &target.handlers.on_press_change {
                press_change(true);
                pressed = true;
            }
        }

        // A second down from the same pointer without an up should not leave
        // the first entry stranded, nor the arena it opened.
        self.take(event.pointer_id);
        self.take_arena(event.pointer_id);
        let double = taps
            .iter()
            .find(|t| t.handlers.on_double_tap.is_some())
            .cloned();
        let mut velocity = VelocityTracker::new();
        velocity.add_position(event.time_stamp_micros, event.position);
        self.active.push((
            event.pointer_id,
            ActivePointer {
                tap,
                drag,
                scale,
                listeners,
                taps,
                secondary,
                double,
                winner: None,
                rejected: Vec::new(),
                drag_rejected: false,
                origin: event.position,
                total: Offset::ZERO,
                past_slop: false,
                pressed,
                velocity,
                down_micros: event.time_stamp_micros,
                position: event.position,
                kind: event.kind,
                long_pressed: false,
                second_tap,
                scaling: false,
            },
        ));
        self.open_arena(event.pointer_id);
        self.begin_scale_if_two_fingers(event);
        true
    }

    /// Joins this pointer's recognisers to its arena, then closes the arena
    /// behind them. Upstream the recognisers join as the down event is
    /// dispatched along the hit-test path -- innermost region first, and
    /// within a region in the order a `GestureDetector` makes its
    /// recognisers: tap, double tap, long press, then the drags -- and the
    /// binding closes the arena when the dispatch is done. That order is not
    /// cosmetic: it is what the sweep hands the arena to.
    fn open_arena(&mut self, pointer_id: i64) {
        let members: Vec<Member> = {
            let Some((_, pointer)) = self.active.iter().find(|(id, _)| *id == pointer_id) else {
                return;
            };
            let mut members = Vec::new();
            for (index, target) in pointer.taps.iter().enumerate() {
                if target.handlers.on_tap.is_some() {
                    members.push(Member {
                        recognizer: Recognizer::Tap,
                        index,
                    });
                }
                if target.handlers.on_double_tap.is_some()
                    && pointer
                        .double
                        .as_ref()
                        .is_some_and(|double| double.is_same_region(target))
                {
                    members.push(Member {
                        recognizer: Recognizer::DoubleTap,
                        index,
                    });
                }
                if target.handlers.on_long_press.is_some() {
                    members.push(Member {
                        recognizer: Recognizer::LongPress,
                        index,
                    });
                }
            }
            for index in 0..pointer.secondary.len() {
                members.push(Member {
                    recognizer: Recognizer::SecondaryTap,
                    index,
                });
            }
            if pointer.drag.is_some() {
                members.push(Member::of(Recognizer::Drag));
            }
            if pointer.scale.is_some() {
                members.push(Member::of(Recognizer::Scale));
            }
            members
        };
        if members.is_empty() {
            return;
        }
        for member in members {
            self.arena_add(pointer_id, member);
        }
        // A lone member wins the moment the arena closes -- upstream's
        // default resolution -- which is why a region with one recogniser
        // never has to wait for a sweep.
        let verdicts = self.arena_close(pointer_id);
        self.apply_verdicts(pointer_id, verdicts);
    }

    /// Whether this press is the second tap of a double tap, settling the
    /// first one either way.
    ///
    /// Settling is an arena question: a second tap in time and place has the
    /// double-tap recogniser *win* the arena the first pointer has been
    /// holding open (upstream `_registerSecondTap` resolves both entries
    /// accepted), and a press that arrives too late or too far away has it
    /// give the arena up (upstream `_reset`), which is what finally lets the
    /// plain tap that has been waiting behind it be reported.
    fn pair_with_pending_tap(&mut self, taps: &[Target], event: &PointerEvent) -> bool {
        let Some(pending) = self.pending_tap.clone() else {
            return false;
        };
        // The two clocks start where upstream starts them: the window at the
        // first tap's lift (`_registerFirstTap` starts the double-tap timer on
        // the up), the minimum gap at the first tap's press (the `_TapTracker`
        // is created at the down and counts `kDoubleTapMinTime` from there).
        let matches = taps.iter().any(|target| {
            target.is_same_region(&pending.target)
                && event.position.minus(pending.origin).distance() <= DOUBLE_TAP_SLOP
                && event.time_stamp_micros - pending.up_micros <= DOUBLE_TAP_TIMEOUT_MICROS
                && event.time_stamp_micros - pending.down_micros >= DOUBLE_TAP_MIN_TIME_MICROS
        });
        if matches {
            // Held rather than announced: the first tap of a double tap is not
            // also a single tap.
            self.settle_pending_tap(Disposition::Accepted);
            true
        } else {
            self.settle_pending_tap(Disposition::Rejected);
            false
        }
    }

    /// Takes the pending tap out of limbo by resolving the double-tap member
    /// that has been holding its arena. If the plain tap ends up the winner of
    /// that resolution, this is the moment it is finally reported.
    fn settle_pending_tap(&mut self, disposition: Disposition) {
        let Some(tap) = self.pending_tap.take() else {
            return;
        };
        let Some(member) = self.member_of(tap.pointer_id, Recognizer::DoubleTap) else {
            return; // The arena resolved some other way; nothing is waiting.
        };
        let verdicts = self.arena_resolve(tap.pointer_id, member, disposition);
        self.arena_release(tap.pointer_id);
        self.apply_verdicts(tap.pointer_id, verdicts.clone());
        let tap_won = verdicts.iter().any(|verdict| {
            verdict.member.recognizer == Recognizer::Tap
                && verdict.disposition == Disposition::Accepted
        });
        if tap_won {
            if let Some(single) = &tap.target.handlers.on_tap {
                single(tap.event);
            }
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
                    && pointer
                        .scale
                        .as_ref()
                        .is_some_and(|t| t.is_same_region(&target))
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
        let Some(pointer) = self.find(pointer_id) else {
            return;
        };
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
        active
            .velocity
            .add_position(event.time_stamp_micros, event.position);
        if active.scaling {
            // A finger in a pinch is not dragging, tapping or long-pressing;
            // the only thing left to report is the pinch itself.
            return self.update_scale(event);
        }
        let travelled =
            (active.total.dx * active.total.dx + active.total.dy * active.total.dy).sqrt();
        let travel = event.position.minus(active.origin);
        let total = active.total;

        // The pan slop, not the touch slop: this recogniser's drag is a free
        // pan -- both axes at once, as `DragEvent` carries them -- and upstream
        // judges that against `computePanSlop` (monodrag.dart's
        // `PanGestureRecognizer.hasSufficientGlobalDistanceToAccept`), twice
        // the touch slop. A mouse's is two pixels, not thirty-six.
        let starting = !active.past_slop && travelled > compute_pan_slop(active.kind);
        if starting {
            active.past_slop = true;
        }
        let past_slop = active.past_slop;
        let was_pressed = active.pressed;
        let tap = active.tap.clone();
        let drag_target = active.drag.clone();
        let listeners = active.listeners.clone();

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

        for target in &listeners {
            if let Some(moved) = &target.handlers.on_pointer_move {
                let mut local_event = *event;
                local_event.local_position = target.local_origin.plus(travel);
                moved(&local_event);
            }
        }

        // The press has travelled, so the recognisers that were waiting to see
        // whether this was theirs know it is not: upstream every tap-flavoured
        // one resolves rejected past the slop, and the drag resolves accepted
        // and takes the arena outright, there and then.
        let mut drag_won = false;
        if starting {
            let mut verdicts = Vec::new();
            for member in self.alive_members(event.pointer_id) {
                if matches!(
                    member.recognizer,
                    Recognizer::Tap
                        | Recognizer::SecondaryTap
                        | Recognizer::DoubleTap
                        | Recognizer::LongPress
                ) {
                    verdicts.extend(self.arena_resolve(
                        event.pointer_id,
                        member,
                        Disposition::Rejected,
                    ));
                }
            }
            if drag_target.is_some() {
                verdicts.extend(self.arena_resolve(
                    event.pointer_id,
                    Member::of(Recognizer::Drag),
                    Disposition::Accepted,
                ));
            }
            self.apply_verdicts(event.pointer_id, verdicts);
            drag_won = self.won_by(event.pointer_id, Recognizer::Drag);
        }

        if let Some(target) = &drag_target {
            let drag = DragEvent {
                delta: event.delta,
                total,
                local_position: target.local_origin.plus(travel),
                pointer_id: event.pointer_id,
            };
            // `won_by` rather than "is there a drag target", because a drag
            // that lost the arena -- to a long press, say -- may not start;
            // and won at the close of a one-member arena, which is why a
            // drag-only region starts without a contest.
            if drag_won {
                if let Some(start) = &target.handlers.on_drag_start {
                    start(drag);
                }
            }
            if past_slop && !self.drag_rejected(event.pointer_id) {
                if let Some(update) = &target.handlers.on_drag_update {
                    update(drag);
                }
            }
        }
        true
    }

    /// Whether this pointer's drag member has lost its arena, in which case no
    /// drag callbacks may fire for it any more.
    fn drag_rejected(&self, pointer_id: i64) -> bool {
        self.active
            .iter()
            .find(|(id, _)| *id == pointer_id)
            .is_some_and(|(_, pointer)| pointer.drag_rejected)
    }

    /// Reports where the two fingers are now, relative to where they started.
    fn update_scale(&mut self, _event: &PointerEvent) -> bool {
        let Some(scale) = &self.scale else {
            return true;
        };
        let Some(first) = self.position_of(scale.pointers.0) else {
            return true;
        };
        let Some(second) = self.position_of(scale.pointers.1) else {
            return true;
        };

        let focal = midpoint(first, second);
        let separation = second.minus(first);
        let distance = separation.distance();
        let angle = separation.dy.atan2(separation.dx);

        let Some(scale) = &mut self.scale else {
            return true;
        };
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
        let Some(scale) = self.scale.take() else {
            return;
        };
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
        let Some(mut active) = self.take(event.pointer_id) else {
            return false;
        };
        if active.scaling {
            // Everything this finger had to say was said by the pinch.
            return true;
        }
        let travel = event.position.minus(active.origin);

        for target in &active.listeners {
            if let Some(up) = &target.handlers.on_pointer_up {
                let mut local_event = *event;
                local_event.local_position = target.local_origin.plus(travel);
                up(&local_event);
            }
        }

        if let Some(target) = &active.tap {
            if active.pressed {
                if let Some(press_change) = &target.handlers.on_press_change {
                    press_change(false);
                }
            }
        }

        // The recognisers see the up before the arena is swept -- upstream's
        // order too: the binding routes the event and only then sweeps. A
        // long press whose deadline never came gives up, the pointer being
        // gone, and so does a drag that never crossed the slop.
        let mut verdicts = Vec::new();
        for member in self.alive_members(event.pointer_id) {
            match member.recognizer {
                Recognizer::LongPress => {
                    verdicts.extend(self.arena_resolve(
                        event.pointer_id,
                        member,
                        Disposition::Rejected,
                    ));
                }
                Recognizer::Drag if !active.past_slop => {
                    verdicts.extend(self.arena_resolve(
                        event.pointer_id,
                        member,
                        Disposition::Rejected,
                    ));
                }
                _ => {}
            }
        }

        // The double tap is the one that does not give up on an up. Either
        // this was the second tap of a pair, and it wins the arena outright
        // (upstream `_registerSecondTap` resolves both entries accepted), or
        // it was the first, and it *holds* the arena against the sweep until
        // the window for a second tap has closed (upstream `_registerFirstTap`
        // calls `hold`).
        let mut held_for_double = false;
        if !active.past_slop && !active.long_pressed {
            if active.second_tap {
                if let Some(member) = self.member_of(event.pointer_id, Recognizer::DoubleTap) {
                    verdicts.extend(self.arena_resolve(
                        event.pointer_id,
                        member,
                        Disposition::Accepted,
                    ));
                }
            } else if active.double.is_some()
                && self
                    .member_of(event.pointer_id, Recognizer::DoubleTap)
                    .is_some()
            {
                let target = active.double.clone().expect("checked above");
                self.pending_tap = Some(PendingTap {
                    event: TapEvent {
                        local_position: target.local_origin.plus(travel),
                        pointer_id: event.pointer_id,
                    },
                    pointer_id: event.pointer_id,
                    target,
                    origin: active.origin,
                    down_micros: active.down_micros,
                    up_micros: event.time_stamp_micros,
                });
                self.arena_hold(event.pointer_id);
                held_for_double = true;
            }
        }

        // The sweep: pointer up, so the first member still standing wins
        // rather than anyone being convinced.
        verdicts.extend(self.arena_sweep(event.pointer_id));
        apply_verdicts_to(&mut active, verdicts);

        // What the arena decided. A tap that won is reported right here --
        // unless the double-tap recogniser has the arena on hold, in which
        // case nothing is reported until the window for a second tap closes.
        // A press that travelled is not a tap however the arena went, and
        // neither is one that was already announced as a long press.
        let won = |recognizer: Recognizer| {
            active
                .winner
                .is_some_and(|winner| winner.recognizer == recognizer)
        };
        if !held_for_double && !active.past_slop && !active.long_pressed {
            if won(Recognizer::Tap) {
                let winner = active.winner.expect("just checked");
                let target = &active.taps[winner.index];
                if let Some(tap) = &target.handlers.on_tap {
                    tap(TapEvent {
                        local_position: target.local_origin.plus(travel),
                        pointer_id: event.pointer_id,
                    });
                }
            }
            if won(Recognizer::SecondaryTap) {
                let winner = active.winner.expect("just checked");
                let target = &active.secondary[winner.index];
                if let Some(secondary) = &target.handlers.on_secondary_tap {
                    secondary(TapEvent {
                        local_position: target.local_origin.plus(travel),
                        pointer_id: event.pointer_id,
                    });
                }
            }
            if active.second_tap {
                if let Some(target) = &active.double {
                    let tap_event = TapEvent {
                        local_position: target.local_origin.plus(travel),
                        pointer_id: event.pointer_id,
                    };
                    if let Some(double) = &target.handlers.on_double_tap {
                        double(tap_event);
                    }
                }
            }
        }

        if active.past_slop && !active.drag_rejected {
            if let Some(target) = &active.drag {
                if let Some(end) = &target.handlers.on_drag_end {
                    end(DragEndEvent {
                        velocity: active
                            .velocity
                            .fling_velocity(event.time_stamp_micros, active.kind),
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
        let Some(mut active) = self.take(event.pointer_id) else {
            return false;
        };
        if active.scaling {
            return true;
        }
        for target in &active.listeners {
            if let Some(cancel) = &target.handlers.on_pointer_cancel {
                let mut local_event = *event;
                local_event.local_position = target.local_origin;
                cancel(&local_event);
            }
        }

        // The platform has taken the pointer away: every recogniser still in
        // the arena gives up. Members of an arena that already resolved --
        // a drag that had won, say -- hear nothing, which is what lets the
        // drag end with a whimper rather than not at all.
        let mut verdicts = Vec::new();
        for member in self.alive_members(event.pointer_id) {
            verdicts.extend(self.arena_resolve(event.pointer_id, member, Disposition::Rejected));
        }
        apply_verdicts_to(&mut active, verdicts);

        // A cancelled press is not a tap. Only the pressed state is unwound.
        if active.pressed {
            if let Some(target) = &active.tap {
                if let Some(press_change) = &target.handlers.on_press_change {
                    press_change(false);
                }
            }
        }
        if active.past_slop && !active.drag_rejected {
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

// -- The three roles a hit test has ------------------------------------------

/// Upstream `HitTestTarget` (`gestures/hit_test.dart`): something that can be
/// handed a pointer event because the hit test found it.
///
/// Upstream every render object is one, and so is every gesture recogniser;
/// the entry the hit test recorded carries the target, and the dispatcher
/// calls `handleEvent` on each in turn. Here the entry carries a
/// [`PointerHandlers`] instead of a target object, because this crate's
/// regions are handlers rather than objects -- so the handlers are what
/// implements this.
///
/// **The three raw changes are all that come through here.** Everything else
/// a region can be told -- a tap, a drag, a long press -- is a *recognised*
/// gesture, decided by [`GestureRouter`] from a sequence of these, and
/// delivered by it rather than by this method. That split is upstream's too:
/// `handleEvent` is the raw channel, and the recognisers sit above it.
pub trait HitTestTarget {
    /// Upstream's `handleEvent`.
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry);
}

impl HitTestTarget for PointerHandlers {
    fn handle_event(&self, event: &PointerEvent, _entry: &HitTestEntry) {
        let handler = match event.change {
            PointerChange::Down => self.on_pointer_down.as_ref(),
            PointerChange::Move => self.on_pointer_move.as_ref(),
            PointerChange::Up => self.on_pointer_up.as_ref(),
            // A cancel is not an up: nothing was completed, so anything
            // showing progress unwinds rather than finishes. See the field.
            PointerChange::Cancel => self.on_pointer_cancel.as_ref(),
            _ => None,
        };
        if let Some(handler) = handler {
            handler(event);
        }
    }
}

/// Upstream `HitTestDispatcher`: whatever takes a finished hit-test result and
/// delivers the event along it.
///
/// One method, and the interface exists so that the thing doing the
/// delivering can be replaced -- upstream's `GestureBinding` is the only
/// implementation it ships, and a test harness is the other.
pub trait HitTestDispatcher {
    /// Upstream's `dispatchEvent`.
    fn dispatch_event(&mut self, event: &PointerEvent, result: &HitTestResult);
}

impl HitTestDispatcher for GestureRouter {
    /// Delivers `event` to every target on the path, innermost first.
    ///
    /// **Innermost first, and to every one of them, not only the first.**
    /// Upstream walks the whole path for the same reason: a button inside a
    /// card is inside both, and a listener on the card that only heard about
    /// presses which missed its button would be a listener that could not
    /// count them. Stopping at the first target is what a *gesture arena* is
    /// for, and that is a different mechanism -- see [`GestureRouter`].
    fn dispatch_event(&mut self, event: &PointerEvent, result: &HitTestResult) {
        for entry in &result.path {
            if let Some(handlers) = &entry.handlers {
                handlers.handle_event(event, entry);
            }
        }
    }
}

/// Upstream `HitTestable`: something a position can be hit-tested against.
///
/// Upstream this is the binding, which forwards to the render view for the
/// view the event arrived on. Here it is the render tree itself, because that
/// is what a hit test descends.
///
/// # The two methods, and why both
///
/// Upstream is mid-migration: `hitTest(result, position)` is deprecated in
/// favour of `hitTestInView(result, position, viewId)`, because an
/// application may now have several views and an event carries the one it
/// came from. Both are here, with the deprecated one *provided* in terms of
/// the other -- which is the shape of the migration rather than a copy of its
/// current state.
///
/// This crate has one view. [`HitTestable::MAIN_VIEW_ID`] is its id, and a
/// hit test for any other view finds nothing rather than silently searching
/// the wrong tree: answering the main view's contents to a question about a
/// second view would be a wrong answer, where an empty path is merely an
/// unhelpful one.
pub trait HitTestable {
    /// The id of the only view this crate has. Upstream's
    /// `FlutterView.viewId` for the implicit view.
    const MAIN_VIEW_ID: u64 = 0;

    /// Upstream's `hitTestInView`.
    fn hit_test_in_view(&self, result: &mut HitTestResult, position: Offset, view_id: u64);

    /// Upstream's deprecated `hitTest`, which is the same question about the
    /// only view there used to be.
    fn hit_test_at(&self, result: &mut HitTestResult, position: Offset) {
        self.hit_test_in_view(result, position, Self::MAIN_VIEW_ID);
    }
}

impl<T: RenderBox + ?Sized> HitTestable for T {
    fn hit_test_in_view(&self, result: &mut HitTestResult, position: Offset, view_id: u64) {
        if view_id != Self::MAIN_VIEW_ID {
            return;
        }
        self.hit_test(position, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{
        BoxConstraints, Offset, PaintContext, RenderPointerRegion, RenderStack, Size, StackPosition,
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
                RenderPointerRegion::new(7, Sized(Size::square(40.0))).with_handlers(handlers),
                StackPosition {
                    left: Some(10.0),
                    top: Some(10.0),
                    ..Default::default()
                },
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
        // 25px: past the 18px touch slop, still short of the 36px pan slop a
        // free drag is judged against. No drag yet.
        router.dispatch(&root, &event(PointerChange::Move, 45.0, 20.0, 25.0, 0.0));
        assert!(log.borrow().is_empty());
        // Past it.
        router.dispatch(&root, &event(PointerChange::Move, 60.0, 20.0, 15.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Up, 60.0, 20.0, 0.0, 0.0));

        assert_eq!(*log.borrow(), vec!["start", "update", "end"]);
    }

    /// One event from a mouse rather than a finger: the same gesture, judged
    /// against a mouse's far smaller slop.
    fn mouse(change: PointerChange, x: f32, y: f32, dx: f32, dy: f32) -> PointerEvent {
        PointerEvent {
            kind: PointerKind::Mouse,
            ..event(change, x, y, dx, dy)
        }
    }

    #[test]
    fn a_mouse_drag_starts_almost_at_once() {
        // A mouse goes exactly where it is aimed, so upstream forgives it
        // almost nothing: kPrecisePointerPanSlop is two logical pixels, not
        // thirty-six, and a click-drag takes hold immediately.
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
        router.dispatch(&root, &mouse(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        router.dispatch(&root, &mouse(PointerChange::Move, 21.0, 20.0, 1.0, 0.0));
        assert!(
            log.borrow().is_empty(),
            "one pixel is still within the slop"
        );
        router.dispatch(&root, &mouse(PointerChange::Move, 24.0, 20.0, 3.0, 0.0));
        router.dispatch(&root, &mouse(PointerChange::Up, 24.0, 20.0, 0.0, 0.0));

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
            PointerHandlers::new()
                .with_press_change(move |pressed| sink.borrow_mut().push(pressed)),
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
            RenderPointerRegion::new(
                1,
                RenderPointerRegion::new(2, Sized(Size::square(80.0))).with_handlers(inner),
            )
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
        swipe_with(
            PointerKind::Touch,
            frames,
            per_frame,
            hold_before_lifting_micros,
        )
    }

    /// The same swipe by some other kind of pointer, whose slops may differ.
    fn swipe_with(
        kind: PointerKind,
        frames: i64,
        per_frame: f32,
        hold_before_lifting_micros: i64,
    ) -> Offset {
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
        let event_as = |change, y: f32, dy: f32, now: i64| {
            let mut event = at(change, y, dy, now);
            event.kind = kind;
            event
        };
        router.dispatch(&root, &event_as(PointerChange::Down, y, 0.0, now));
        for _ in 0..frames {
            now += 16_667;
            y -= per_frame;
            router.dispatch(&root, &event_as(PointerChange::Move, y, -per_frame, now));
        }
        now += hold_before_lifting_micros.max(16_667);
        router.dispatch(&root, &event_as(PointerChange::Up, y, 0.0, now));
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
        assert!(
            velocity.dx.abs() < 1.0,
            "and nothing sideways, not {}",
            velocity.dx
        );
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
    fn a_short_fast_flick_carries_for_a_mouse_but_not_a_finger() {
        // Twelve logical pixels in three frames, about 240 pixels a second:
        // well past kMinFlingVelocity, but not past a finger's 18px hit slop.
        // A mouse's slop is a single pixel, so the same flick carries.
        let finger = swipe(3, 4.0, 0);
        assert_eq!(
            finger,
            Offset::ZERO,
            "a finger is forgiven twelve pixels of travel"
        );
        let mouse = swipe_with(PointerKind::Mouse, 3, 4.0, 0);
        assert!(
            (mouse.dy + 240.0).abs() < 60.0,
            "the mouse flick should carry, not {}",
            mouse.dy
        );
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
            router.dispatch(
                &root,
                &at(
                    PointerChange::Move,
                    70.0 - 20.0 * step as f32,
                    -20.0,
                    1_000_000 + step * 16_667,
                ),
            );
        }
        router.dispatch(&root, &at(PointerChange::Cancel, 0.0, 0.0, 1_120_000));

        assert_eq!(
            *velocity.borrow(),
            Offset::ZERO,
            "a cancelled drag throws nothing"
        );
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
        router.dispatch(&root, &event(PointerChange::Move, 40.0, 80.0, 0.0, 40.0));
        router.dispatch(&root, &event(PointerChange::Up, 40.0, 80.0, 0.0, 0.0));

        assert_eq!(
            *dragged.borrow(),
            40.0,
            "the list should have been scrolled"
        );
        assert_eq!(
            *taps.borrow(),
            0,
            "a scroll is not a tap on the row it began on"
        );
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
        PointerEvent {
            time_stamp_micros: micros,
            ..event(change, 20.0, 20.0, 0.0, 0.0)
        }
    }

    #[test]
    fn a_press_held_still_becomes_a_long_press() {
        let presses = Rc::new(RefCell::new(0));
        let sink = presses.clone();
        let root = tree(PointerHandlers::new().with_long_press(move |_| *sink.borrow_mut() += 1));

        let mut router = GestureRouter::new();
        router.dispatch(&root, &held(PointerChange::Down, 0));
        assert!(
            router.awaits_deadline(0),
            "the clock is what decides this one"
        );

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
        router.dispatch(&root, &at(PointerChange::Move, 30.0, -40.0, 100_000));
        assert!(
            !router.awaits_deadline(100_000),
            "a scroll is not a long press"
        );
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
        // The window runs from the first tap's lift at 30ms, so it closes at
        // 330ms -- a frame before 300ms after the press it is still open.
        router.tick(300_000);
        assert!(
            log.borrow().is_empty(),
            "the clock starts at the lift, not the press"
        );
        router.tick(30_000 + DOUBLE_TAP_TIMEOUT_MICROS);
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
        // The second tap lifted at 530ms, so its own window closes at 830ms.
        router.tick(530_000 + DOUBLE_TAP_TIMEOUT_MICROS);
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
        assert_eq!(
            *log.borrow(),
            vec!["tap"],
            "the first tap was settled by the second press"
        );
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

    #[test]
    fn a_first_tap_held_down_still_leaves_the_window_for_the_second() {
        // The 300ms window runs from the first tap's lift, not its press --
        // upstream starts the clock in `_registerFirstTap`, on the up -- so a
        // reader who presses, holds for 250ms and lifts still has the whole
        // window left to tap again, not the last 50ms of it.
        let log = Rc::new(RefCell::new(Vec::new()));
        let single = log.clone();
        let double = log.clone();
        let root = tree(
            PointerHandlers::new()
                .with_tap(move |_| single.borrow_mut().push("tap"))
                .with_double_tap(move |_| double.borrow_mut().push("double")),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &held(PointerChange::Down, 0));
        router.dispatch(&root, &held(PointerChange::Up, 250_000));
        // 400ms after the press is 150ms after the lift: still waiting.
        assert!(router.awaits_deadline(400_000));
        router.dispatch(&root, &held(PointerChange::Down, 400_000));
        router.dispatch(&root, &held(PointerChange::Up, 430_000));
        assert_eq!(*log.borrow(), vec!["double"]);
    }

    #[test]
    fn the_double_tap_minimum_gap_is_measured_from_the_first_press() {
        // The 40ms minimum runs from the first tap's press -- upstream's
        // `_TapTracker` starts its countdown when it is created, at the down --
        // so a brisk 10ms tap followed by a press 45ms after the first one is
        // a double tap, even though only 35ms separates the lift from the
        // second press.
        let log = Rc::new(RefCell::new(Vec::new()));
        let single = log.clone();
        let double = log.clone();
        let root = tree(
            PointerHandlers::new()
                .with_tap(move |_| single.borrow_mut().push("tap"))
                .with_double_tap(move |_| double.borrow_mut().push("double")),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &held(PointerChange::Down, 0));
        router.dispatch(&root, &held(PointerChange::Up, 10_000));
        router.dispatch(&root, &held(PointerChange::Down, 45_000));
        router.dispatch(&root, &held(PointerChange::Up, 60_000));
        assert_eq!(*log.borrow(), vec!["double"]);
    }

    // -- The arena -------------------------------------------------------------

    #[test]
    fn two_taps_competing_for_one_pointer_and_only_one_wins() {
        let inner = Rc::new(RefCell::new(0));
        let outer = Rc::new(RefCell::new(0));
        let inner_sink = inner.clone();
        let outer_sink = outer.clone();
        let root = nested(
            PointerHandlers::new().with_tap(move |_| *inner_sink.borrow_mut() += 1),
            PointerHandlers::new().with_tap(move |_| *outer_sink.borrow_mut() += 1),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 40.0, 40.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Up, 40.0, 40.0, 0.0, 0.0));

        assert_eq!(
            *inner.borrow(),
            1,
            "the innermost tap joined first, and the sweep favours the first member"
        );
        assert_eq!(
            *outer.borrow(),
            0,
            "the outer tap lost the arena and may not fire"
        );
    }

    #[test]
    fn a_drag_wins_the_arena_past_the_slop_and_the_tap_is_rejected() {
        let taps = Rc::new(RefCell::new(0));
        let ends = Rc::new(RefCell::new(0));
        let tap_sink = taps.clone();
        let end_sink = ends.clone();
        let root = nested(
            PointerHandlers::new().with_tap(move |_| *tap_sink.borrow_mut() += 1),
            PointerHandlers::new()
                .with_drag_update(|_| {})
                .with_drag_end(move |_| *end_sink.borrow_mut() += 1),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 40.0, 40.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Move, 40.0, 80.0, 0.0, 40.0));
        // Nothing has fired yet -- but the tap has already lost, told so when
        // the drag claimed the arena, before the finger ever lifted.
        assert_eq!(*taps.borrow(), 0);
        assert_eq!(*ends.borrow(), 0);
        router.dispatch(&root, &event(PointerChange::Up, 40.0, 80.0, 0.0, 0.0));

        assert_eq!(*taps.borrow(), 0, "a rejected tap does not fire on release");
        assert_eq!(*ends.borrow(), 1, "the drag owns the gesture outright");
    }

    #[test]
    fn a_long_press_win_rejects_both_the_tap_and_the_drag() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let tap = log.clone();
        let long = log.clone();
        let update = log.clone();
        let end = log.clone();
        let root = nested(
            PointerHandlers::new()
                .with_tap(move |_| tap.borrow_mut().push("tap"))
                .with_long_press(move |_| long.borrow_mut().push("long")),
            PointerHandlers::new()
                .with_drag_update(move |_| update.borrow_mut().push("update"))
                .with_drag_end(move |_| end.borrow_mut().push("end")),
        );

        let mut router = GestureRouter::new();
        router.dispatch(&root, &at(PointerChange::Down, 70.0, 0.0, 0));
        router.tick(LONG_PRESS_TIMEOUT_MICROS);
        // The long press resolved the arena in its favour; the drag and the
        // tap were rejected with it, so neither may fire again.
        router.dispatch(&root, &at(PointerChange::Move, 40.0, -30.0, 700_000));
        router.dispatch(&root, &at(PointerChange::Up, 40.0, 0.0, 800_000));

        assert_eq!(*log.borrow(), vec!["long"]);
    }

    #[test]
    fn a_lone_recognizer_wins_as_soon_as_the_arena_closes() {
        let taps = Rc::new(RefCell::new(0));
        let sink = taps.clone();
        let root = tree(PointerHandlers::new().with_tap(move |_| *sink.borrow_mut() += 1));

        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        assert!(
            router.arenas.is_empty(),
            "a one-member arena is resolved at the close, not held for a sweep"
        );
        assert!(router.won_by(1, Recognizer::Tap));
        router.dispatch(&root, &event(PointerChange::Up, 20.0, 20.0, 0.0, 0.0));
        assert_eq!(
            *taps.borrow(),
            1,
            "and the tap is still reported on release"
        );
    }

    #[test]
    fn a_secondary_button_tap_reports_to_the_secondary_handler_only() {
        let primary = Rc::new(RefCell::new(0));
        let secondary = Rc::new(RefCell::new(0));
        let primary_sink = primary.clone();
        let secondary_sink = secondary.clone();
        let root = tree(
            PointerHandlers::new()
                .with_tap(move |_| *primary_sink.borrow_mut() += 1)
                .with_secondary_tap(move |_| *secondary_sink.borrow_mut() += 1),
        );

        let mut router = GestureRouter::new();
        let mut right = event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0);
        right.buttons = SECONDARY_BUTTON;
        assert!(router.dispatch(&root, &right));
        let mut right = event(PointerChange::Up, 20.0, 20.0, 0.0, 0.0);
        right.buttons = SECONDARY_BUTTON;
        router.dispatch(&root, &right);

        assert_eq!(*secondary.borrow(), 1);
        assert_eq!(
            *primary.borrow(),
            0,
            "the primary tap never joins for another button's press"
        );
    }

    #[test]
    fn a_secondary_tap_loses_to_a_drag_past_the_slop() {
        let secondary = Rc::new(RefCell::new(0));
        let ends = Rc::new(RefCell::new(0));
        let secondary_sink = secondary.clone();
        let end_sink = ends.clone();
        let root = nested(
            PointerHandlers::new().with_secondary_tap(move |_| *secondary_sink.borrow_mut() += 1),
            PointerHandlers::new()
                .with_drag_update(|_| {})
                .with_drag_end(move |_| *end_sink.borrow_mut() += 1),
        );

        let mut router = GestureRouter::new();
        let mut right = event(PointerChange::Down, 40.0, 40.0, 0.0, 0.0);
        right.buttons = SECONDARY_BUTTON;
        router.dispatch(&root, &right);
        let mut right = event(PointerChange::Move, 40.0, 80.0, 0.0, 40.0);
        right.buttons = SECONDARY_BUTTON;
        router.dispatch(&root, &right);
        let mut right = event(PointerChange::Up, 40.0, 80.0, 0.0, 0.0);
        right.buttons = SECONDARY_BUTTON;
        router.dispatch(&root, &right);

        assert_eq!(*secondary.borrow(), 0, "the secondary tap lost the arena");
        assert_eq!(*ends.borrow(), 1, "the drag won it");
    }

    #[test]
    fn an_eager_accept_is_only_a_promise_until_the_arena_closes() {
        let mut router = GestureRouter::new();
        let tap = Member {
            recognizer: Recognizer::Tap,
            index: 0,
        };
        let drag = Member::of(Recognizer::Drag);
        router.arena_add(9, tap);
        router.arena_add(9, drag);
        // The arena is still open -- later members have not had their say --
        // so accepting now can only be noted, not settled.
        assert!(
            router
                .arena_resolve(9, tap, Disposition::Accepted)
                .is_empty()
        );
        // Closing keeps the promise: the eager winner takes the arena and
        // everyone else is told they lost.
        let verdicts = router.arena_close(9);
        assert_eq!(
            verdicts,
            vec![
                Verdict {
                    member: drag,
                    disposition: Disposition::Rejected
                },
                Verdict {
                    member: tap,
                    disposition: Disposition::Accepted
                },
            ]
        );
        assert!(router.arenas.is_empty(), "a resolved arena is discarded");
    }

    #[test]
    fn a_sweep_attempted_on_a_held_arena_runs_when_it_is_released() {
        let mut router = GestureRouter::new();
        let tap = Member {
            recognizer: Recognizer::Tap,
            index: 0,
        };
        let long_press = Member {
            recognizer: Recognizer::LongPress,
            index: 0,
        };
        router.arena_add(5, tap);
        router.arena_add(5, long_press);
        router.arena_close(5);
        router.arena_hold(5);
        // Pointer up with the arena held: the sweep waits, which is why a
        // held-back single tap reports late rather than never.
        assert!(router.arena_sweep(5).is_empty());
        let verdicts = router.arena_release(5);
        assert_eq!(
            verdicts,
            vec![
                Verdict {
                    member: tap,
                    disposition: Disposition::Accepted
                },
                Verdict {
                    member: long_press,
                    disposition: Disposition::Rejected
                },
            ],
            "the release runs the pending sweep, first member first"
        );
        assert!(router.arenas.is_empty());
    }

    // -- Scale ----------------------------------------------------------------

    /// One finger's event, at a position and a moment.
    fn finger(pointer_id: i64, change: PointerChange, x: f32, y: f32, micros: i64) -> PointerEvent {
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
        assert_eq!(
            events.first().map(|s| s.scale),
            Some(1.0),
            "a pinch starts at one"
        );
        let last = events.last().expect("an update");
        assert!(
            (last.scale - 2.0).abs() < 0.01,
            "fingers twice as far apart: {}",
            last.scale
        );
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
        assert!(
            (last.focal_point.dy - 50.0).abs() < 0.01,
            "the focal point followed them"
        );
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
        assert_eq!(
            *log.borrow(),
            vec![true],
            "entering is one event, not one per move"
        );
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
        router.dispatch(
            &root,
            &PointerEvent {
                change: PointerChange::Remove,
                ..hover_at(40.0, 40.0)
            },
        );
        assert_eq!(
            *log.borrow(),
            vec!["inner:true", "outer:true", "inner:false", "outer:false"],
            "the pointer left the window, so nothing is hovered"
        );
    }

    #[test]
    fn a_press_is_not_a_hover() {
        let root =
            tree(PointerHandlers::new().with_hover_change(|_| {
                panic!("a finger touching the screen is not hovering over it")
            }));
        let mut router = GestureRouter::new();
        router.dispatch(&root, &event(PointerChange::Down, 20.0, 20.0, 0.0, 0.0));
        router.dispatch(&root, &event(PointerChange::Up, 20.0, 20.0, 0.0, 0.0));
    }

    /// A box of a fixed size that records itself under `id`.
    fn region(id: u64, handlers: PointerHandlers) -> crate::render::RenderPointerRegion {
        crate::render::RenderPointerRegion::new(
            id,
            crate::render::RenderConstrainedBox::tight(100.0, 100.0),
        )
        .with_handlers(handlers)
    }

    fn event_at(change: PointerChange, x: f32, y: f32) -> PointerEvent {
        event(change, x, y, 0.0, 0.0)
    }

    #[test]
    fn only_the_raw_changes_reach_a_targets_handle_event() {
        // Everything else a region can be told -- a tap, a drag, a long press
        // -- is a *recognised* gesture the router decides from a sequence of
        // these. `handle_event` is the raw channel, as upstream's is.
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let make = |label: &'static str,
                    sink: std::rc::Rc<std::cell::RefCell<Vec<&'static str>>>| {
            move |_: &PointerEvent| sink.borrow_mut().push(label)
        };
        let handlers = PointerHandlers::new()
            .with_pointer_down(make("down", seen.clone()))
            .with_pointer_move(make("move", seen.clone()))
            .with_pointer_up(make("up", seen.clone()))
            .with_pointer_cancel(make("cancel", seen.clone()));
        let entry = HitTestEntry {
            target: 1,
            local_position: Offset::ZERO,
            handlers: None,
        };

        for change in [
            PointerChange::Down,
            PointerChange::Move,
            PointerChange::Up,
            PointerChange::Cancel,
        ] {
            handlers.handle_event(&event_at(change, 0.0, 0.0), &entry);
        }
        assert_eq!(*seen.borrow(), vec!["down", "move", "up", "cancel"]);

        // And a hover or an add is not one of them: those are the router's,
        // which tracks which regions the mouse is inside.
        seen.borrow_mut().clear();
        for change in [
            PointerChange::Hover,
            PointerChange::Add,
            PointerChange::Remove,
            PointerChange::PanZoomStart,
        ] {
            handlers.handle_event(&event_at(change, 0.0, 0.0), &entry);
        }
        assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
    }

    #[test]
    fn a_cancel_is_not_an_up() {
        // Nothing was completed, so anything showing progress unwinds rather
        // than finishes -- which is why the two are separate callbacks and a
        // target that only listens for one hears nothing of the other.
        let ups = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = ups.clone();
        let handlers =
            PointerHandlers::new().with_pointer_up(move |_| counter.set(counter.get() + 1));
        let entry = HitTestEntry {
            target: 1,
            local_position: Offset::ZERO,
            handlers: None,
        };
        handlers.handle_event(&event_at(PointerChange::Cancel, 0.0, 0.0), &entry);
        assert_eq!(ups.get(), 0);
        handlers.handle_event(&event_at(PointerChange::Up, 0.0, 0.0), &entry);
        assert_eq!(ups.get(), 1);
    }

    #[test]
    fn a_dispatch_reaches_every_target_on_the_path_not_only_the_first() {
        // A button inside a card is inside both, and a listener on the card
        // that only heard about presses which missed its button would be one
        // that could not count them. Stopping at the first is what a gesture
        // *arena* is for, which is a different mechanism.
        let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let outer_sink = heard.clone();
        let inner_sink = heard.clone();
        let outer = std::rc::Rc::new(
            PointerHandlers::new().with_pointer_down(move |_| outer_sink.borrow_mut().push("card")),
        );
        let inner = std::rc::Rc::new(
            PointerHandlers::new()
                .with_pointer_down(move |_| inner_sink.borrow_mut().push("button")),
        );

        let mut result = HitTestResult::new();
        // Innermost first, which is the order a hit test records in.
        result.add_with_handlers(2, Offset::ZERO, Some(inner));
        result.add_with_handlers(1, Offset::ZERO, Some(outer));

        let mut router = GestureRouter::new();
        router.dispatch_event(&event_at(PointerChange::Down, 0.0, 0.0), &result);
        assert_eq!(*heard.borrow(), vec!["button", "card"]);
    }

    #[test]
    fn a_hit_test_for_another_view_finds_nothing_rather_than_the_wrong_tree() {
        // This crate has one view. Answering its contents to a question about
        // a second view would be a wrong answer, where an empty path is only
        // an unhelpful one.
        let tree = region(7, PointerHandlers::new().with_pointer_down(|_| {}));
        let mut tree = crate::render::RenderRef::new(tree);
        tree.layout(crate::render::BoxConstraints::tight(100.0, 100.0));

        let mut main = HitTestResult::new();
        tree.hit_test_in_view(
            &mut main,
            Offset::new(50.0, 50.0),
            <crate::render::RenderRef as HitTestable>::MAIN_VIEW_ID,
        );
        assert!(!main.path.is_empty(), "the only view finds its own tree");

        let mut second = HitTestResult::new();
        tree.hit_test_in_view(&mut second, Offset::new(50.0, 50.0), 1);
        assert!(second.path.is_empty(), "and no other view finds anything");
    }

    #[test]
    fn the_deprecated_hit_test_is_the_same_question_about_the_only_view() {
        // Upstream is mid-migration and provides one in terms of the other;
        // this pins that the provided method really does route to the main
        // view rather than being a second implementation that could drift.
        let tree = region(7, PointerHandlers::new().with_pointer_down(|_| {}));
        let mut tree = crate::render::RenderRef::new(tree);
        tree.layout(crate::render::BoxConstraints::tight(100.0, 100.0));

        let mut old = HitTestResult::new();
        tree.hit_test_at(&mut old, Offset::new(50.0, 50.0));
        let mut new = HitTestResult::new();
        tree.hit_test_in_view(
            &mut new,
            Offset::new(50.0, 50.0),
            <crate::render::RenderRef as HitTestable>::MAIN_VIEW_ID,
        );
        assert_eq!(
            old.path
                .iter()
                .map(|entry| entry.target)
                .collect::<Vec<_>>(),
            new.path
                .iter()
                .map(|entry| entry.target)
                .collect::<Vec<_>>()
        );
    }

    /// A finger moving at a steady speed, sampled every 10ms.
    fn steady(speed_per_second: f32, samples: usize) -> Vec<(i64, Offset)> {
        (0..samples)
            .map(|step| {
                let micros = step as i64 * 10_000;
                (
                    micros,
                    Offset::new(0.0, speed_per_second * micros as f32 / 1_000_000.0),
                )
            })
            .collect()
    }

    #[test]
    fn the_platform_fling_trackers_weight_the_freshest_sample_almost_to_nothing() {
        // Both discount the last measurement heavily -- iOS to a twentieth,
        // macOS to a fifth -- because that one is the finger lifting rather
        // than the reader flinging. The two disagree about which of the other
        // samples to trust, and neither is more correct: each matches its own
        // platform's scroll views, which is all either is for.
        let mut ios = IOSScrollViewFlingVelocityTracker::new(PointerKind::Touch);
        let mut macos = MacOSScrollViewFlingVelocityTracker::new(PointerKind::Touch);
        for (micros, position) in steady(1000.0, 6) {
            ios.add_position(micros, position);
            macos.add_position(micros, position);
        }
        let now = 50_000;
        // At a steady speed every weighting agrees, which is the sanity check.
        assert!((ios.velocity_estimate(now).pixels_per_second.dy - 1000.0).abs() < 1.0);
        assert!((macos.velocity_estimate(now).pixels_per_second.dy - 1000.0).abs() < 1.0);

        // Now stop dead on the very last sample. iOS barely notices; macOS
        // notices more, but neither reports anything like a stop.
        let mut ios = IOSScrollViewFlingVelocityTracker::new(PointerKind::Touch);
        let mut macos = MacOSScrollViewFlingVelocityTracker::new(PointerKind::Touch);
        for (micros, position) in steady(1000.0, 5) {
            ios.add_position(micros, position);
            macos.add_position(micros, position);
        }
        let last = Offset::new(0.0, 1000.0 * 40_000.0 / 1_000_000.0);
        ios.add_position(50_000, last);
        macos.add_position(50_000, last);

        let ios_dy = ios.velocity_estimate(50_000).pixels_per_second.dy;
        let macos_dy = macos.velocity_estimate(50_000).pixels_per_second.dy;
        assert!(
            (ios_dy - 950.0).abs() < 1.0,
            "iOS kept 95% of the speed, got {ios_dy}"
        );
        assert!(
            (macos_dy - 800.0).abs() < 1.0,
            "macOS kept 80% of it, got {macos_dy}"
        );
        assert!(macos_dy < ios_dy, "macOS listens to the last sample more");
    }

    #[test]
    fn a_pause_before_lifting_means_no_fling_at_all() {
        // The same rule the fitted tracker uses, and for the same reason: a
        // finger that stopped and rested is not throwing anything.
        let mut ios = IOSScrollViewFlingVelocityTracker::new(PointerKind::Touch);
        for (micros, position) in steady(1000.0, 5) {
            ios.add_position(micros, position);
        }
        let resting = 40_000 + VelocityTracker::ASSUME_STOPPED_MICROS + 1;
        assert_eq!(
            ios.velocity_estimate(resting).pixels_per_second,
            Offset::ZERO
        );
    }

    #[test]
    fn the_offset_is_measured_over_far_more_samples_than_the_velocity() {
        // Upstream keeps twenty samples for a calculation that reads four, and
        // its comment says why: the offset has to grow big enough to pass the
        // *distance* half of the fling test. The two halves are deliberately
        // measured over different spans.
        let mut ios = IOSScrollViewFlingVelocityTracker::new(PointerKind::Touch);
        for (micros, position) in steady(1000.0, 12) {
            ios.add_position(micros, position);
        }
        let estimate = ios.velocity_estimate(110_000);
        assert!(
            estimate.offset.dy > 100.0,
            "eleven intervals of travel, not three: {}",
            estimate.offset.dy
        );
        assert_eq!(estimate.duration_micros, 110_000);
    }

    #[test]
    fn a_tracker_with_nothing_in_it_reports_nothing_rather_than_dividing_by_zero() {
        let empty = IOSScrollViewFlingVelocityTracker::new(PointerKind::Touch);
        assert_eq!(empty.velocity_estimate(0).pixels_per_second, Offset::ZERO);
        assert_eq!(empty.previous_velocity_at(0), Offset::ZERO);

        // And one sample is still not two.
        let mut one = IOSScrollViewFlingVelocityTracker::new(PointerKind::Touch);
        one.add_position(0, Offset::ZERO);
        assert_eq!(one.previous_velocity_at(0), Offset::ZERO);
    }

    #[test]
    fn two_samples_at_the_same_instant_are_not_infinitely_fast() {
        let mut ios = IOSScrollViewFlingVelocityTracker::new(PointerKind::Touch);
        ios.add_position(0, Offset::ZERO);
        ios.add_position(0, Offset::new(0.0, 50.0));
        assert_eq!(ios.previous_velocity_at(0), Offset::ZERO);
    }

    #[test]
    fn the_ring_wraps_and_the_macos_tracker_shares_it() {
        // The macOS tracker is upstream's subclass of the iOS one, so it has
        // the same ring and the same addPosition; only the weighting differs.
        let mut macos = MacOSScrollViewFlingVelocityTracker::new(PointerKind::Mouse);
        assert_eq!(macos.kind(), PointerKind::Mouse);
        for (micros, position) in steady(500.0, IOSScrollViewFlingVelocityTracker::SAMPLE_SIZE + 5)
        {
            macos.add_position(micros, position);
        }
        let now = (IOSScrollViewFlingVelocityTracker::SAMPLE_SIZE as i64 + 4) * 10_000;
        assert!((macos.velocity_estimate(now).pixels_per_second.dy - 500.0).abs() < 1.0);
    }
}
