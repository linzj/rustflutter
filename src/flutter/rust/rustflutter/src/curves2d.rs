// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Curves through points, and curves made of other curves.
//!
//! Upstream's `animation/curves.dart`, the part of it that is not a named
//! easing: [`Interval`] and [`Split`], which build a curve out of other curves,
//! and the `Curve2D` family, which is a curve through *space* rather than a
//! curve of one number against time.
//!
//! # Why a 2D curve is a different thing
//!
//! An easing curve answers "how far along am I" for a `t` between 0 and 1. A
//! [`Curve2D`] answers "where am I", and its `t` is a position along a path
//! rather than a fraction of a duration. What it is for is motion along a route
//! -- a card that arcs into place rather than sliding -- and the arithmetic has
//! nothing to do with easing.
//!
//! # These had no consumer, and that is not a reason
//!
//! `coverage_ledger.json` carried all six of these as `equivalent` with a note
//! reading "no consumer, waiting for the first use". That is not an
//! equivalence, and "nothing would call it" is the reasoning this line has
//! already declined twice -- `overlay.rs` was ported in full with zero
//! references, and that is what made the overlay line quick when its turn came.

use crate::animation::Curve;
use crate::render::Offset;

/// Upstream `Interval`: another curve, run over part of the time.
///
/// # It clamps at both ends rather than running early or late
///
/// Before `begin` the answer is the inner curve at 0, and after `end` it is the
/// inner curve at 1 -- `clampDouble((t - begin) / (end - begin), 0, 1)`. So a
/// widget staggered to start halfway is *at its start* for the first half,
/// which is what makes a staggered animation look like things waiting their
/// turn rather than several animations of different lengths.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub begin: f32,
    pub end: f32,
    pub curve: Curve,
}

impl Interval {
    /// Upstream asserts all four: both ends within 0 to 1, and `end >= begin`.
    /// An inverted interval has no time to run in and would divide by a
    /// negative.
    pub fn new(begin: f32, end: f32) -> Interval {
        Interval::with_curve(begin, end, Curve::Linear)
    }

    pub fn with_curve(begin: f32, end: f32, curve: Curve) -> Interval {
        debug_assert!(
            (0.0..=1.0).contains(&begin),
            "begin is a fraction of the whole"
        );
        debug_assert!((0.0..=1.0).contains(&end), "end is a fraction of the whole");
        debug_assert!(end >= begin, "an interval that ends before it begins");
        Interval { begin, end, curve }
    }

    /// Upstream's `transformInternal`.
    ///
    /// **The clamp cannot be observed in this crate**, and it is kept anyway.
    /// Upstream's `Curve.transform` *asserts* `0 <= t <= 1` and this crate's
    /// clamps, so the inner curve here already refuses an out-of-range
    /// fraction. Upstream's clamp is load-bearing; this one is the same line
    /// carried across, and removing it leaves every test green -- checked.
    pub fn transform(&self, t: f32) -> f32 {
        // A zero-length interval is a step: everything at or past it is done.
        if self.end <= self.begin {
            return if t >= self.begin { 1.0 } else { 0.0 };
        }
        let local = ((t - self.begin) / (self.end - self.begin)).clamp(0.0, 1.0);
        self.curve.transform(local)
    }
}

/// Upstream `Split`: one curve up to a point, another after it.
///
/// # The split is in *both* axes
///
/// This is the part that is easy to get wrong. At `t == split` the answer is
/// `split` -- the same number -- so the first curve is squeezed into the
/// rectangle from (0,0) to (split, split) and the second into the rest. Two
/// curves laid end to end without that rescaling would jump at the seam.
///
/// Upstream's defaults are worth keeping too: linear before the split and
/// `easeOutCubic` after it, which is the shape of something arriving.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Split {
    pub split: f32,
    pub begin_curve: Curve,
    pub end_curve: Curve,
}

impl Split {
    pub fn new(split: f32) -> Split {
        debug_assert!((0.0..=1.0).contains(&split));
        Split {
            split,
            begin_curve: Curve::Linear,
            // Upstream's default: the shape of something arriving.
            end_curve: Curve::EASE_OUT_CUBIC,
        }
    }

    pub fn with_curves(mut self, begin: Curve, end: Curve) -> Self {
        self.begin_curve = begin;
        self.end_curve = end;
        self
    }

    /// Upstream's `transform`, including its two early returns: the ends are
    /// themselves, and the split point is itself.
    pub fn transform(&self, t: f32) -> f32 {
        debug_assert!((0.0..=1.0).contains(&t));
        if t == 0.0 || t == 1.0 || t == self.split {
            return t;
        }
        if t < self.split {
            // Into the first rectangle and back out.
            let local = t / self.split;
            self.split * self.begin_curve.transform(local)
        } else {
            let local = (t - self.split) / (1.0 - self.split);
            self.split + (1.0 - self.split) * self.end_curve.transform(local)
        }
    }
}

/// Upstream `Curve2DSample`: one point on a [`Curve2D`], with the `t` it came
/// from.
///
/// The `t` rides along because [`Curve2D::generate_samples`] does not space its
/// samples evenly -- a caller that wants to know *where along the curve* a
/// sample sits cannot work it out from its position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve2DSample {
    pub t: f32,
    pub value: Offset,
}

impl Curve2DSample {
    pub fn new(t: f32, value: Offset) -> Curve2DSample {
        Curve2DSample { t, value }
    }
}

/// Upstream `Curve2D`: a parametric curve through space.
pub trait Curve2D {
    /// Where the curve is at `t`, which runs 0 to 1.
    fn transform(&self, t: f32) -> Offset;

    /// Upstream's `samplingSeed`, which exists so that sampling the same curve
    /// twice gives the same points.
    ///
    /// The subdivision below is deliberately random, and a *fresh* random
    /// sequence each time would mean a curve drawn on two frames used different
    /// points and shimmered. Upstream seeds from a control point, so the seed
    /// is a property of the curve rather than of the moment.
    fn sampling_seed(&self) -> u64 {
        0
    }

    /// Upstream's `generateSamples`: enough points to draw the curve, put where
    /// they are needed.
    ///
    /// # Flatness by triangle area, and why the subdivision is random
    ///
    /// Three points are "flat enough" when the triangle they make has an area
    /// below the tolerance -- a cheap stand-in for curvature that needs no
    /// derivatives. If they are not, the interval is split and both halves are
    /// sampled again, so **samples end up concentrated where the curve bends**
    /// and sparse where it is straight.
    ///
    /// The split point is not the midpoint. It is jittered to somewhere in the
    /// middle tenth, and upstream says why: a periodic curve sampled at exact
    /// midpoints can hit the same phase of its period every time and be judged
    /// flat while it is not -- the aliasing you get from sampling a sine at its
    /// zero crossings. The jitter is what stops that, and the seed is what
    /// keeps it repeatable.
    ///
    /// The tolerance is compared against the **squared** area, which is
    /// upstream's `(z * z) < tolerance` -- so the default 1e-10 is a much
    /// tighter bound than it looks.
    fn generate_samples(&self, start: f32, end: f32, tolerance: f32) -> Vec<Curve2DSample> {
        debug_assert!(end > start, "a sample range that does not run forwards");
        let mut rng = Jitter::new(self.sampling_seed());
        let first = Curve2DSample::new(start, self.transform(start));
        let last = Curve2DSample::new(end, self.transform(end));
        let mut samples = vec![first];
        self.subdivide(first, last, tolerance, &mut rng, &mut samples, 0);
        samples
    }

    /// The defaults upstream's named arguments carry.
    fn samples(&self) -> Vec<Curve2DSample> {
        self.generate_samples(0.0, 1.0, 1e-10)
    }

    #[doc(hidden)]
    fn subdivide(
        &self,
        p: Curve2DSample,
        q: Curve2DSample,
        tolerance: f32,
        rng: &mut Jitter,
        samples: &mut Vec<Curve2DSample>,
        depth: u32,
    ) {
        // Upstream recurses without a depth limit and relies on the tolerance
        // and on `double`'s precision to end it. `f32` has a good deal less to
        // work with, so a curve that never looks flat could recurse until the
        // interval stops shrinking. The cap is this port's, and it is stated
        // rather than tuned quietly: 24 levels is 16 million subdivisions of
        // the original interval, well past anything a screen can show.
        const MAX_DEPTH: u32 = 24;
        let t = p.t + (0.45 + 0.1 * rng.next()) * (q.t - p.t);
        let r = Curve2DSample::new(t, self.transform(t));
        if depth >= MAX_DEPTH || is_flat(p.value, q.value, r.value, tolerance) {
            samples.push(q);
        } else {
            self.subdivide(p, r, tolerance, rng, samples, depth + 1);
            self.subdivide(r, q, tolerance, rng, samples, depth + 1);
        }
    }
}

/// Upstream's `isFlat`: the area of the triangle three points make, squared and
/// compared against the tolerance.
fn is_flat(p: Offset, q: Offset, r: Offset, tolerance: f32) -> bool {
    let pr = Offset::new(p.dx - r.dx, p.dy - r.dy);
    let qr = Offset::new(q.dx - r.dx, q.dy - r.dy);
    let z = pr.dx * qr.dy - qr.dx * pr.dy;
    z * z < tolerance
}

/// The repeatable jitter the subdivision uses.
///
/// Upstream reaches for `math.Random(samplingSeed)`; a small linear
/// congruential generator here, because what is wanted is *a stable sequence
/// from a seed* and not randomness in any stronger sense.
pub struct Jitter {
    state: u64,
}

impl Jitter {
    pub fn new(seed: u64) -> Jitter {
        // A zero seed would leave a multiplicative generator at zero for ever.
        Jitter {
            state: seed.wrapping_mul(2) | 1,
        }
    }

    /// The next value, in 0 to 1.
    pub fn next(&mut self) -> f32 {
        // Numerical Recipes' constants; the top bits are the good ones.
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.state >> 33) as f32) / ((1u64 << 31) as f32)
    }
}

/// Upstream `CatmullRomSpline`: a curve that passes **through** its control
/// points.
///
/// # Through, not near
///
/// That is the whole reason to reach for one. A Bézier's control points pull
/// the curve towards them and are not on it; a Catmull-Rom's are on it, so a
/// designer handing over a path of points gets a curve through those points.
///
/// # The two invented handles
///
/// The first and last control points of the underlying spline are *handles* --
/// they steer the ends without being visited. Upstream invents them when they
/// are not given, by reflecting the second point through the first:
/// `controlPoints[0] * 2 - controlPoints[1]`. That continues the direction the
/// curve was already going in, so the ends do not kink.
///
/// # Centripetal, and the alpha that makes it so
///
/// The `alpha = 0.5` in the segment arithmetic is what makes this a
/// *centripetal* Catmull-Rom rather than a uniform one. The difference shows
/// with control points at uneven spacing: a uniform spline loops and
/// self-intersects around a tight cluster, and a centripetal one does not.
pub struct CatmullRomSpline {
    /// Four coefficients per segment, in the order upstream stores them:
    /// cubic, square, linear, constant.
    segments: Vec<[Offset; 4]>,
}

impl CatmullRomSpline {
    /// Upstream asserts at least four control points: two are the ends of the
    /// first real segment and two more are needed to steer it.
    pub fn new(control_points: &[Offset]) -> CatmullRomSpline {
        CatmullRomSpline::with_tension(control_points, 0.0, None, None)
    }

    /// `tension` runs 0 to 1, and upstream asserts it. At 1 the curve is
    /// straight between its points -- the tangents are scaled to nothing --
    /// and at 0 it is as round as it gets.
    pub fn with_tension(
        control_points: &[Offset],
        tension: f32,
        start_handle: Option<Offset>,
        end_handle: Option<Offset>,
    ) -> CatmullRomSpline {
        debug_assert!(
            control_points.len() > 3,
            "a Catmull-Rom spline needs more than three control points"
        );
        debug_assert!((0.0..=1.0).contains(&tension), "tension runs from 0 to 1");
        CatmullRomSpline {
            segments: compute_segments(control_points, tension, start_handle, end_handle),
        }
    }

    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }
}

fn compute_segments(
    control_points: &[Offset],
    tension: f32,
    start_handle: Option<Offset>,
    end_handle: Option<Offset>,
) -> Vec<[Offset; 4]> {
    if control_points.len() < 4 {
        return Vec::new();
    }
    let sub = |a: Offset, b: Offset| Offset::new(a.dx - b.dx, a.dy - b.dy);
    let add = |a: Offset, b: Offset| Offset::new(a.dx + b.dx, a.dy + b.dy);
    let mul = |a: Offset, k: f32| Offset::new(a.dx * k, a.dy * k);
    let len = |a: Offset| (a.dx * a.dx + a.dy * a.dy).sqrt();

    // The invented handles: reflect the neighbour through the end.
    let start = start_handle.unwrap_or_else(|| sub(mul(control_points[0], 2.0), control_points[1]));
    let last = control_points[control_points.len() - 1];
    let end =
        end_handle.unwrap_or_else(|| sub(mul(last, 2.0), control_points[control_points.len() - 2]));

    let mut all = Vec::with_capacity(control_points.len() + 2);
    all.push(start);
    all.extend_from_slice(control_points);
    all.push(end);

    // Centripetal. See the type's docs.
    const ALPHA: f32 = 0.5;
    let reverse_tension = 1.0 - tension;
    let mut result = Vec::new();
    for window in all.windows(4) {
        let (c0, c1, c2, c3) = (window[0], window[1], window[2], window[3]);
        let d10 = sub(c1, c0);
        let d21 = sub(c2, c1);
        let d32 = sub(c3, c2);
        let t01 = len(d10).powf(ALPHA);
        let t12 = len(d21).powf(ALPHA);
        let t23 = len(d32).powf(ALPHA);

        // Guard the divisions upstream does not: two identical control points
        // give a zero-length difference and a zero `t`, and upstream's double
        // arithmetic produces an infinity that propagates into the segment. A
        // repeated point is a thing a designer's path really contains.
        let safe = |v: f32| if v == 0.0 { f32::EPSILON } else { v };
        let m1 = mul(
            add(
                d21,
                mul(
                    sub(
                        mul(d10, 1.0 / safe(t01)),
                        mul(sub(c2, c0), 1.0 / safe(t01 + t12)),
                    ),
                    t12,
                ),
            ),
            reverse_tension,
        );
        let m2 = mul(
            add(
                d21,
                mul(
                    sub(
                        mul(d32, 1.0 / safe(t23)),
                        mul(sub(c3, c1), 1.0 / safe(t12 + t23)),
                    ),
                    t12,
                ),
            ),
            reverse_tension,
        );
        let sum = add(m1, m2);
        result.push([
            add(mul(d21, -2.0), sum),
            sub(sub(mul(d21, 3.0), m1), sum),
            m1,
            c1,
        ]);
    }
    result
}

impl Curve2D for CatmullRomSpline {
    /// Upstream's `transformInternal`: pick the segment, then evaluate its
    /// cubic at the local `t`.
    fn transform(&self, t: f32) -> Offset {
        if self.segments.is_empty() {
            return Offset::ZERO;
        }
        let count = self.segments.len();
        let (index, local) = if t < 1.0 {
            let position = t * count as f32;
            (
                (position.floor() as usize).min(count - 1),
                position - position.floor(),
            )
        } else {
            // Upstream's explicit last case: at exactly 1 the position would be
            // `count`, which is one past the end.
            (count - 1, 1.0)
        };
        let c = &self.segments[index];
        let t2 = local * local;
        Offset::new(
            c[0].dx * t2 * local + c[1].dx * t2 + c[2].dx * local + c[3].dx,
            c[0].dy * t2 * local + c[1].dy * t2 + c[2].dy * local + c[3].dy,
        )
    }

    /// Upstream's, which reads a coefficient of the first segment so that the
    /// seed is a property of this curve.
    fn sampling_seed(&self) -> u64 {
        let Some(first) = self.segments.first() else {
            return 0;
        };
        let seed = first[1];
        (((seed.dx + seed.dy) * 10000.0).round() as i64).unsigned_abs()
    }
}

/// Upstream `CatmullRomCurve`: a [`CatmullRomSpline`] used as an easing curve.
///
/// # It is a spline read as a function, and that constrains it
///
/// An easing curve has to answer exactly one value for every `t`. A spline
/// through arbitrary points need not -- it can double back, and then "the value
/// at t" has two answers. So upstream validates the control points and refuses
/// the ones that would: they must be strictly increasing in x and stay within
/// the unit square.
///
/// The endpoints (0,0) and (1,1) are **implied and must not be given**, which
/// is upstream's rule and an easy one to trip over.
pub struct CatmullRomCurve {
    spline: CatmullRomSpline,
}

impl CatmullRomCurve {
    /// Answers `None` for control points that would not make a function, rather
    /// than asserting -- upstream validates in an assert and throws with a list
    /// of what was wrong, and a caller here can see the same refusal in
    /// release.
    /// **At least two control points.** Upstream builds the spline from
    /// `[zero, ...controlPoints, (1,1)]` and `CatmullRomSpline` asserts on more
    /// than three points, so one control point trips an assertion a level down
    /// with a message about a spline the caller never mentioned. Said here.
    pub fn new(control_points: &[Offset], tension: f32) -> Option<CatmullRomCurve> {
        if control_points.len() < 2 || !CatmullRomCurve::validate(control_points) {
            return None;
        }
        // The implied endpoints, added here so a caller passes only the middle.
        let mut points = Vec::with_capacity(control_points.len() + 2);
        points.push(Offset::new(0.0, 0.0));
        points.extend_from_slice(control_points);
        points.push(Offset::new(1.0, 1.0));
        Some(CatmullRomCurve {
            spline: CatmullRomSpline::with_tension(&points, tension, None, None),
        })
    }

    /// Upstream's `validateControlPoints`, reduced to the two rules that make a
    /// spline a function: x strictly increasing, and everything inside the unit
    /// square on x.
    pub fn validate(control_points: &[Offset]) -> bool {
        if control_points.is_empty() {
            return false;
        }
        let mut previous = 0.0;
        for point in control_points {
            if !(point.dx > previous && point.dx < 1.0) {
                return false;
            }
            previous = point.dx;
        }
        true
    }

    /// The eased value at `t`.
    ///
    /// The spline is parametric, so its `t` is a position along the path rather
    /// than an x -- upstream searches for the x it wants. This does the same
    /// with a bisection, which is what upstream's `_solveForY`-style lookup
    /// amounts to.
    pub fn transform(&self, t: f32) -> f32 {
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        let (mut low, mut high) = (0.0f32, 1.0f32);
        // Twenty halvings takes the bracket below a millionth, which is finer
        // than anything downstream of an easing curve can see.
        for _ in 0..20 {
            let mid = (low + high) / 2.0;
            if self.spline.transform(mid).dx < t {
                low = mid;
            } else {
                high = mid;
            }
        }
        self.spline.transform((low + high) / 2.0).dy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> Offset {
        Offset::new(x, y)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    // -- Interval ---------------------------------------------------------------------

    #[test]
    fn an_interval_waits_and_then_finishes_early() {
        // The staggered-animation shape: at its start for the first half, at
        // its end for the last quarter.
        let interval = Interval::new(0.25, 0.75);
        assert_eq!(interval.transform(0.0), 0.0);
        assert_eq!(interval.transform(0.25), 0.0, "only just starting");
        assert!(
            close(interval.transform(0.5), 0.5),
            "halfway through its own run"
        );
        assert_eq!(interval.transform(0.75), 1.0, "done");
        assert_eq!(interval.transform(1.0), 1.0, "and stays done");
    }

    #[test]
    fn an_interval_clamps_at_both_ends_rather_than_running_early_or_late() {
        // Without the clamp the fraction goes negative before `begin` and past
        // one after `end`, and a widget staggered to start halfway would be
        // running backwards for the first half.
        let interval = Interval::new(0.5, 1.0);
        assert_eq!(interval.transform(0.0), 0.0);
        assert_eq!(interval.transform(0.25), 0.0);
        assert!(interval.transform(0.75) > 0.0);
    }

    #[test]
    fn an_interval_runs_its_inner_curve_over_its_own_stretch() {
        let eased = Interval::with_curve(0.0, 1.0, Curve::EASE_OUT_CUBIC);
        let plain = Interval::new(0.0, 1.0);
        assert!(
            eased.transform(0.5) > plain.transform(0.5),
            "ease-out is ahead at the midpoint"
        );
    }

    #[test]
    fn the_centripetal_alpha_is_what_stops_a_tight_cluster_overshooting() {
        // `alpha = 0.5` is what makes this centripetal rather than uniform, and
        // it shows with control points at uneven spacing: a tight pair between
        // two distant ones. The tangents a uniform spline computes there are
        // far too long for the short hop, so it flies out and comes back.
        //
        // The numbers are measured rather than guessed: with alpha 0.5 the
        // curve reaches y ~= 3.96 (its highest control point is at y = 3), and
        // with alpha 1.0 it reaches ~= 9.29 -- three times past the point it
        // was meant to touch.
        let uneven = vec![at(0.0, 0.0), at(10.0, 0.0), at(10.5, 3.0), at(60.0, 0.0)];
        let spline = CatmullRomSpline::new(&uneven);

        let mut highest: f32 = f32::MIN;
        for step in 0..=200 {
            highest = highest.max(spline.transform(step as f32 / 200.0).dy);
        }
        assert!(
            highest < 5.0,
            "a centripetal spline stays near the points it goes through: {highest}"
        );
        assert!(
            highest > 3.0,
            "and it does reach the one at y = 3: {highest}"
        );
    }

    #[test]
    fn an_invented_handle_is_the_neighbour_reflected_through_the_end() {
        // `controlPoints[0] * 2 - controlPoints[1]`, which continues the
        // direction the curve was already going in so the ends do not kink.
        // Pinned by handing the same value in explicitly: the two must agree
        // exactly.
        let points = square_path();
        let reflected = at(
            points[0].dx * 2.0 - points[1].dx,
            points[0].dy * 2.0 - points[1].dy,
        );
        let invented = CatmullRomSpline::new(&points);
        let explicit = CatmullRomSpline::with_tension(&points, 0.0, Some(reflected), None);

        for step in 0..=10 {
            let t = step as f32 / 10.0;
            let a = invented.transform(t);
            let b = explicit.transform(t);
            assert!(
                close(a.dx, b.dx) && close(a.dy, b.dy),
                "at {t}: {a:?} vs {b:?}"
            );
        }

        // And something that is *not* the reflection gives a different curve,
        // so the agreement above is not vacuous.
        let other = CatmullRomSpline::with_tension(&points, 0.0, Some(at(0.0, 0.0)), None);
        assert!(
            !close(invented.transform(0.15).dy, other.transform(0.15).dy),
            "a different handle really is a different curve"
        );
    }

    #[test]
    fn the_tolerance_is_compared_against_the_squared_area() {
        // Upstream's `(z * z) < tolerance`, so the default 1e-10 admits
        // triangles whose area is up to 1e-5 -- a much looser bound than the
        // number looks. Comparing the area unsquared would subdivide far
        // further for the same tolerance.
        let flat_enough = is_flat(at(0.0, 0.0), at(1.0, 0.0), at(0.5, 1e-6), 1e-10);
        assert!(flat_enough, "area 5e-7, squared 2.5e-13, under 1e-10");

        let not_flat = is_flat(at(0.0, 0.0), at(1.0, 0.0), at(0.5, 1e-3), 1e-10);
        assert!(!not_flat, "area 5e-4, squared 2.5e-7, over 1e-10");

        // The same middle case read the other way round: unsquared it would be
        // called bent, squared it is called flat. This is the pair the rule
        // turns on.
        let z = 1e-6f32;
        assert!(z * z < 1e-10, "squared: flat");
        assert!(z.abs() > 1e-10, "unsquared: would be called bent");
    }

    #[test]
    fn a_zero_length_interval_is_a_step() {
        // Upstream's assertion allows `end == begin`, and the division would be
        // by zero. A step is the only answer that is not a NaN.
        let step = Interval::new(0.5, 0.5);
        assert_eq!(step.transform(0.4), 0.0);
        assert_eq!(step.transform(0.5), 1.0);
        assert_eq!(step.transform(0.6), 1.0);
    }

    // -- Split ------------------------------------------------------------------------

    #[test]
    fn a_split_meets_itself_at_the_seam() {
        // The split is in both axes: at `t == split` the answer is `split`. Two
        // curves laid end to end without that rescaling would jump here.
        let split = Split::new(0.3);
        assert_eq!(split.transform(0.3), 0.3);
        assert!(
            close(split.transform(0.2999), 0.2999),
            "and arrives smoothly"
        );
        assert!(split.transform(0.3001) > 0.3);
    }

    #[test]
    fn a_split_keeps_the_ends_where_they_are() {
        let split = Split::new(0.7);
        assert_eq!(split.transform(0.0), 0.0);
        assert_eq!(split.transform(1.0), 1.0);
    }

    #[test]
    fn each_half_of_a_split_stays_inside_its_own_rectangle() {
        // Before the split, everything is below it; after, above.
        let split = Split::new(0.4).with_curves(Curve::EASE_OUT_CUBIC, Curve::Linear);
        for t in [0.05f32, 0.2, 0.39] {
            let v = split.transform(t);
            assert!(v >= 0.0 && v <= 0.4, "at {t}: {v}");
        }
        for t in [0.41f32, 0.6, 0.99] {
            let v = split.transform(t);
            assert!(v >= 0.4 && v <= 1.0, "at {t}: {v}");
        }
    }

    #[test]
    fn upstreams_defaults_are_linear_then_ease_out_cubic() {
        // The shape of something arriving.
        let split = Split::new(0.5);
        assert_eq!(split.begin_curve, Curve::Linear);
        assert_eq!(split.end_curve, Curve::EASE_OUT_CUBIC);
    }

    // -- CatmullRomSpline ----------------------------------------------------------------

    fn square_path() -> Vec<Offset> {
        vec![at(0.0, 0.0), at(10.0, 20.0), at(30.0, 20.0), at(40.0, 0.0)]
    }

    #[test]
    fn a_catmull_rom_spline_passes_through_its_control_points() {
        // The whole reason to reach for one: a Bezier's control points pull the
        // curve towards them and are not on it; these are on it. Including the
        // first and last, because the handles that steer the ends are invented
        // outside them rather than taken from them.
        let points = square_path();
        let spline = CatmullRomSpline::new(&points);

        let start = spline.transform(0.0);
        assert!(
            close(start.dx, points[0].dx) && close(start.dy, points[0].dy),
            "{start:?}"
        );
        let end = spline.transform(1.0);
        let last = points[points.len() - 1];
        assert!(close(end.dx, last.dx) && close(end.dy, last.dy), "{end:?}");

        // And the ones in between, at the segment boundaries.
        let inner = spline.transform(1.0 / 3.0);
        assert!(
            close(inner.dx, points[1].dx) && close(inner.dy, points[1].dy),
            "{inner:?}"
        );
    }

    #[test]
    fn n_control_points_make_n_minus_one_segments() {
        // The two invented handles bring the list to `n + 2`, and a segment
        // spans each consecutive pair of the *original* points -- so four
        // control points are three segments, not one. That is upstream's
        // `allPoints.length - 3`, and the first version of this test guessed
        // one.
        assert_eq!(CatmullRomSpline::new(&square_path()).segment_count(), 3);

        let mut longer = square_path();
        longer.push(at(50.0, 20.0));
        assert_eq!(CatmullRomSpline::new(&longer).segment_count(), 4);
    }

    #[test]
    fn a_repeated_control_point_does_not_produce_a_nan() {
        // Two identical points give a zero-length difference and a zero `t`,
        // which upstream's double arithmetic turns into an infinity that
        // propagates into the segment. A repeated point is a thing a designer's
        // path really contains.
        let repeated = vec![at(0.0, 0.0), at(10.0, 10.0), at(10.0, 10.0), at(20.0, 0.0)];
        let spline = CatmullRomSpline::with_tension(&repeated, 0.0, None, None);
        for step in 0..=10 {
            let point = spline.transform(step as f32 / 10.0);
            assert!(point.dx.is_finite() && point.dy.is_finite(), "at {step}");
        }
    }

    #[test]
    fn full_tension_pulls_the_curve_onto_the_straight_line() {
        // At tension 1 the tangents are scaled to nothing.
        let taut = CatmullRomSpline::with_tension(&square_path(), 1.0, None, None);
        let middle = taut.transform(0.5);
        // Straight from (10,20) to (30,20) is a horizontal line.
        assert!(close(middle.dy, 20.0), "{middle:?}");
        assert!(close(middle.dx, 20.0), "{middle:?}");

        let loose = CatmullRomSpline::new(&square_path());
        assert!(
            (loose.transform(0.5).dy - 20.0).abs() >= (middle.dy - 20.0).abs(),
            "and a slack one bows away from it"
        );
    }

    #[test]
    fn a_given_handle_replaces_the_invented_one() {
        let default_handles = CatmullRomSpline::new(&square_path());
        let steered =
            CatmullRomSpline::with_tension(&square_path(), 0.0, Some(at(0.0, 100.0)), None);
        assert_ne!(
            default_handles.transform(0.25).dy,
            steered.transform(0.25).dy,
            "the handle steers the end it belongs to"
        );
    }

    // -- Sampling ---------------------------------------------------------------------------

    #[test]
    fn sampling_the_same_curve_twice_gives_the_same_points() {
        // The subdivision is random, and a fresh sequence each time would mean
        // a curve drawn on two frames used different points and shimmered.
        let spline = CatmullRomSpline::new(&square_path());
        let once = spline.samples();
        let twice = spline.samples();
        assert_eq!(once.len(), twice.len());
        for (a, b) in once.iter().zip(twice.iter()) {
            assert_eq!(a.t, b.t);
        }
    }

    #[test]
    fn samples_start_and_end_where_they_were_asked_to() {
        let spline = CatmullRomSpline::new(&square_path());
        let samples = spline.generate_samples(0.25, 0.75, 1e-10);
        assert_eq!(samples.first().expect("a first").t, 0.25);
        assert!(close(samples.last().expect("a last").t, 0.75));
    }

    #[test]
    fn samples_are_in_order_and_inside_the_range() {
        let spline = CatmullRomSpline::new(&square_path());
        let samples = spline.samples();
        assert!(samples.len() > 2, "a curve needs more than its ends");
        for pair in samples.windows(2) {
            assert!(
                pair[0].t <= pair[1].t,
                "{:?} then {:?}",
                pair[0].t,
                pair[1].t
            );
        }
        assert!(samples.iter().all(|s| (0.0..=1.0).contains(&s.t)));
    }

    #[test]
    fn a_bendier_curve_gets_more_samples() {
        // The point of the flatness test: samples concentrate where the curve
        // bends and are sparse where it is straight.
        let straight =
            CatmullRomSpline::new(&[at(0.0, 0.0), at(10.0, 0.0), at(20.0, 0.0), at(30.0, 0.0)]);
        let bendy =
            CatmullRomSpline::new(&[at(0.0, 0.0), at(10.0, 60.0), at(20.0, -60.0), at(30.0, 0.0)]);
        assert!(
            bendy.samples().len() > straight.samples().len(),
            "{} vs {}",
            bendy.samples().len(),
            straight.samples().len()
        );
    }

    #[test]
    fn the_subdivision_point_is_jittered_and_not_the_midpoint() {
        // Upstream jitters into the middle tenth so that a periodic curve
        // sampled at exact midpoints cannot hit the same phase every time and
        // be judged flat while it is not.
        let mut jitter = Jitter::new(1234);
        let mut saw_variation = false;
        for _ in 0..20 {
            let f = 0.45 + 0.1 * jitter.next();
            assert!((0.45..=0.55).contains(&f), "{f}");
            if (f - 0.5).abs() > 1e-6 {
                saw_variation = true;
            }
        }
        assert!(saw_variation, "and it really varies");
    }

    #[test]
    fn a_seeded_jitter_repeats_and_a_different_seed_does_not() {
        let first: Vec<f32> = (0..5).map(|_| Jitter::new(7).next()).collect();
        assert!(
            first.windows(2).all(|w| w[0] == w[1]),
            "same seed, same first draw"
        );

        let mut a = Jitter::new(7);
        let mut b = Jitter::new(8);
        assert_ne!(a.next(), b.next());
    }

    // -- CatmullRomCurve ----------------------------------------------------------------------

    #[test]
    fn a_curve_needs_control_points_that_make_it_a_function() {
        // An easing curve answers exactly one value per t. A spline that
        // doubled back would answer two.
        assert!(CatmullRomCurve::validate(&[at(0.3, 0.8), at(0.6, 0.2)]));
        assert!(
            !CatmullRomCurve::validate(&[at(0.6, 0.8), at(0.3, 0.2)]),
            "x going backwards"
        );
        assert!(
            !CatmullRomCurve::validate(&[at(0.3, 0.5), at(0.3, 0.7)]),
            "x standing still"
        );
        assert!(
            !CatmullRomCurve::validate(&[at(1.5, 0.5)]),
            "outside the square"
        );
        assert!(!CatmullRomCurve::validate(&[]), "nothing to go through");
    }

    #[test]
    fn a_curve_refuses_control_points_it_cannot_use() {
        assert!(CatmullRomCurve::new(&[at(0.6, 0.8), at(0.3, 0.2)], 0.0).is_none());
        assert!(CatmullRomCurve::new(&[at(0.3, 0.8), at(0.6, 0.2)], 0.0).is_some());
        assert!(
            CatmullRomCurve::new(&[at(0.5, 0.5)], 0.0).is_none(),
            "one control point is not enough -- see the constructor"
        );
    }

    #[test]
    fn a_curve_starts_at_zero_and_ends_at_one() {
        // The endpoints are implied and must not be given -- upstream's rule,
        // and easy to trip over.
        let curve = CatmullRomCurve::new(&[at(0.3, 0.5), at(0.6, 0.8)], 0.0).expect("valid");
        assert_eq!(curve.transform(0.0), 0.0);
        assert_eq!(curve.transform(1.0), 1.0);
    }

    #[test]
    fn a_curve_passes_near_the_point_it_was_given() {
        // Through, within what the bisection can resolve.
        let curve = CatmullRomCurve::new(&[at(0.5, 0.9), at(0.75, 0.95)], 0.0).expect("valid");
        let at_half = curve.transform(0.5);
        assert!((at_half - 0.9).abs() < 0.02, "{at_half}");
    }

    #[test]
    fn a_curve_that_overshoots_is_allowed() {
        // Upstream permits control points whose y is outside 0 to 1 -- that is
        // how an overshooting ease is written. Only x is constrained.
        let curve = CatmullRomCurve::new(&[at(0.5, 1.4), at(0.8, 1.1)], 0.0).expect("valid");
        assert!(curve.transform(0.5) > 1.0);
    }
}
