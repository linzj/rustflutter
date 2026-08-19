// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Upstream `material/arc.dart`: points and rectangles that travel on a
//! curve rather than in a straight line.
//!
//! Material's motion specification says a thing that moves in two dimensions
//! at once should follow an arc. A straight diagonal reads as mechanical --
//! nothing in the physical world starts and stops along a chord -- so a card
//! flying to a new position swings.
//!
//! # The shape of the arc
//!
//! [`MaterialPointArcTween`] picks a circle through both points whose centre
//! sits on the *shorter* of the two axes' extents, so the arc bulges along the
//! long axis. Which of the two cases applies is decided by comparing the
//! horizontal and vertical distance, and the interesting part is what happens
//! when neither is large: **a move that is nearly along one axis does not arc
//! at all**. Upstream's `_kOnAxisDelta` of two logical pixels is the
//! threshold, and the fallback is a plain straight interpolation -- an arc
//! between two points a pixel apart on one axis would need an enormous radius
//! and would read as a wobble.

use crate::engine::Rect;
use crate::render::Offset;

/// Upstream's `_kOnAxisDelta`: how close to an axis a move has to be before
/// it is treated as being on that axis and left straight.
pub const ON_AXIS_DELTA: f32 = 2.0;

/// Upstream `MaterialPointArcTween`: one point swinging to another.
///
/// # Not lazy
///
/// Upstream recomputes on demand behind a `_dirty` flag, because its `begin`
/// and `end` are settable and a `Tween` is expected to survive being
/// retargeted. Here the two ends are given at construction and the geometry
/// is worked out there: a tween with different ends is a different tween, and
/// the flag existed only to make the setters cheap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialPointArcTween {
    pub begin: Offset,
    pub end: Offset,
    center: Offset,
    radius: f32,
    /// `None` when the move is close enough to one axis that upstream leaves
    /// it straight -- see the module docs.
    angles: Option<(f32, f32)>,
}

impl MaterialPointArcTween {
    pub fn new(begin: Offset, end: Offset) -> MaterialPointArcTween {
        let delta = end.minus(begin);
        let delta_x = delta.dx.abs();
        let delta_y = delta.dy.abs();
        let distance = delta.distance();
        // Upstream's `c`: the corner of the axis-aligned box the two points
        // span, level with `begin` and above or below `end`.
        let corner = Offset::new(end.dx, begin.dy);

        if delta_x <= ON_AXIS_DELTA || delta_y <= ON_AXIS_DELTA {
            // Straight. The centre and radius are meaningless here and
            // upstream leaves them at whatever they were; zero says the same
            // thing without pretending to a circle.
            return MaterialPointArcTween {
                begin,
                end,
                center: Offset::ZERO,
                radius: 0.0,
                angles: None,
            };
        }

        let (center, radius, begin_angle, end_angle) = if delta_x < delta_y {
            // Taller than wide: the centre sits level with `end`, off to
            // whichever side `begin` is on, so the arc bulges vertically.
            let radius = distance * distance / corner.minus(begin).distance() / 2.0;
            let center = Offset::new(end.dx + radius * (begin.dx - end.dx).signum(), end.dy);
            let sweep = 2.0 * (distance / (2.0 * radius)).asin();
            if begin.dx < end.dx {
                (center, radius, sweep * (begin.dy - end.dy).signum(), 0.0)
            } else {
                let begin_angle = std::f32::consts::PI + sweep * (end.dy - begin.dy).signum();
                (center, radius, begin_angle, std::f32::consts::PI)
            }
        } else {
            // Wider than tall: the centre sits under or over `begin`.
            let radius = distance * distance / corner.minus(end).distance() / 2.0;
            let center = Offset::new(begin.dx, begin.dy + (end.dy - begin.dy).signum() * radius);
            let sweep = 2.0 * (distance / (2.0 * radius)).asin();
            if begin.dy < end.dy {
                let begin_angle = -std::f32::consts::FRAC_PI_2;
                (
                    center,
                    radius,
                    begin_angle,
                    begin_angle + sweep * (end.dx - begin.dx).signum(),
                )
            } else {
                let begin_angle = std::f32::consts::FRAC_PI_2;
                (
                    center,
                    radius,
                    begin_angle,
                    begin_angle + sweep * (begin.dx - end.dx).signum(),
                )
            }
        };

        MaterialPointArcTween {
            begin,
            end,
            center,
            radius,
            angles: Some((begin_angle, end_angle)),
        }
    }

    /// The centre of the circle the point travels on, or `None` for a move
    /// left straight.
    pub fn center(&self) -> Option<Offset> {
        self.angles.map(|_| self.center)
    }

    pub fn radius(&self) -> Option<f32> {
        self.angles.map(|_| self.radius)
    }

    pub fn begin_angle(&self) -> Option<f32> {
        self.angles.map(|(begin, _)| begin)
    }

    /// The angle the arc ends at.
    ///
    /// **This deliberately differs from upstream.** Upstream's `endAngle`
    /// getter returns `_beginAngle` -- a plain typo, since the field beside it
    /// is `_endAngle` and `lerp` reads the real one, so the animation is
    /// right and only the accessor lies. It is used by nothing but
    /// `toString`.
    ///
    /// Ported as the name promises rather than as written, because the
    /// alternative is a public accessor that answers a different question
    /// from its name, and nothing depends on the wrong answer. The divergence
    /// is deliberate, is the only one in this file, and there is a regression
    /// line asserting the two angles differ so it cannot be lost.
    pub fn end_angle(&self) -> Option<f32> {
        self.angles.map(|(_, end)| end)
    }

    /// Whether this move is being interpolated straight rather than swung.
    pub fn is_straight(&self) -> bool {
        self.angles.is_none()
    }

    /// Upstream's `lerp`.
    ///
    /// The endpoints are answered exactly rather than computed: an arc's ends
    /// come out of trigonometry that is right to within a rounding error, and
    /// a rounding error at `t == 1` is a thing that stops one pixel short of
    /// where it was going.
    pub fn lerp(&self, t: f32) -> Offset {
        if t == 0.0 {
            return self.begin;
        }
        if t == 1.0 {
            return self.end;
        }
        let Some((begin_angle, end_angle)) = self.angles else {
            return Offset::new(
                self.begin.dx + (self.end.dx - self.begin.dx) * t,
                self.begin.dy + (self.end.dy - self.begin.dy) * t,
            );
        };
        let angle = begin_angle + (end_angle - begin_angle) * t;
        self.center.plus(Offset::new(
            angle.cos() * self.radius,
            angle.sin() * self.radius,
        ))
    }
}

/// Which corner of a rectangle a diagonal starts or ends at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CornerId {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl CornerId {
    fn of(self, rect: Rect) -> Offset {
        match self {
            CornerId::TopLeft => Offset::new(rect.left, rect.top),
            CornerId::TopRight => Offset::new(rect.right, rect.top),
            CornerId::BottomLeft => Offset::new(rect.left, rect.bottom),
            CornerId::BottomRight => Offset::new(rect.right, rect.bottom),
        }
    }
}

/// Upstream's `_allDiagonals`, both ways round each way.
const ALL_DIAGONALS: [(CornerId, CornerId); 4] = [
    (CornerId::TopLeft, CornerId::BottomRight),
    (CornerId::BottomRight, CornerId::TopLeft),
    (CornerId::TopRight, CornerId::BottomLeft),
    (CornerId::BottomLeft, CornerId::TopRight),
];

fn rect_center(rect: Rect) -> Offset {
    let (x, y) = rect.center();
    Offset::new(x, y)
}

/// Upstream `MaterialRectArcTween`: a rectangle whose two opposite corners
/// each swing on their own arc.
///
/// **Which pair of corners is chosen is the whole idea.** Upstream picks the
/// diagonal that points most nearly along the direction the rectangle is
/// travelling -- the largest dot product with the vector between the two
/// centres. Animating a fixed pair would make a rectangle moving down-right
/// stretch and squash as its top-left ran ahead of its bottom-right; picking
/// the leading diagonal keeps the two arcs roughly parallel to the motion, so
/// the rectangle keeps its shape while it swings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialRectArcTween {
    pub begin: Rect,
    pub end: Rect,
    begin_arc: MaterialPointArcTween,
    end_arc: MaterialPointArcTween,
}

impl MaterialRectArcTween {
    pub fn new(begin: Rect, end: Rect) -> MaterialRectArcTween {
        let centers_vector = rect_center(end).minus(rect_center(begin));
        // Upstream's `_maxBy`, which keeps the *first* of equal maxima -- it
        // only replaces on a strict `>`. With a rectangle moving straight
        // down, two diagonals score the same and the tie has to break the
        // same way every time, or the arc would flip between frames.
        let mut best = ALL_DIAGONALS[0];
        let mut best_support = f32::NEG_INFINITY;
        for diagonal in ALL_DIAGONALS {
            let support = diagonal_support(begin, centers_vector, diagonal);
            if support > best_support {
                best_support = support;
                best = diagonal;
            }
        }
        MaterialRectArcTween {
            begin,
            end,
            begin_arc: MaterialPointArcTween::new(best.0.of(begin), best.0.of(end)),
            end_arc: MaterialPointArcTween::new(best.1.of(begin), best.1.of(end)),
        }
    }

    pub fn begin_arc(&self) -> MaterialPointArcTween {
        self.begin_arc
    }

    pub fn end_arc(&self) -> MaterialPointArcTween {
        self.end_arc
    }

    pub fn lerp(&self, t: f32) -> Rect {
        if t == 0.0 {
            return self.begin;
        }
        if t == 1.0 {
            return self.end;
        }
        let a = self.begin_arc.lerp(t);
        let b = self.end_arc.lerp(t);
        // Upstream's `Rect.fromPoints`, which sorts the two corners -- the
        // chosen diagonal may run either way round, and a rectangle with a
        // right edge left of its left edge is not a rectangle.
        Rect::ltrb(
            a.dx.min(b.dx),
            a.dy.min(b.dy),
            a.dx.max(b.dx),
            a.dy.max(b.dy),
        )
    }
}

/// Upstream's `_diagonalSupport`: how far the diagonal points along the
/// direction of travel, as a projection.
fn diagonal_support(begin: Rect, centers_vector: Offset, diagonal: (CornerId, CornerId)) -> f32 {
    let delta = diagonal.1.of(begin).minus(diagonal.0.of(begin));
    let length = delta.distance();
    centers_vector.dx * delta.dx / length + centers_vector.dy * delta.dy / length
}

/// Upstream `MaterialRectCenterArcTween`: a rectangle whose *centre* swings
/// while its size interpolates straight.
///
/// The difference from [`MaterialRectArcTween`] is what is being animated, not
/// how. That one arcs two corners and lets the size fall out of where they
/// land, so a rectangle that changes size does so along the arc; this one
/// swings the centre and lerps the width and height independently. Upstream
/// uses this where the size change should read as its own thing -- a hero
/// whose aspect ratio changes -- and the other where the rectangle is
/// travelling more than resizing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialRectCenterArcTween {
    pub begin: Rect,
    pub end: Rect,
    center_arc: MaterialPointArcTween,
}

impl MaterialRectCenterArcTween {
    pub fn new(begin: Rect, end: Rect) -> MaterialRectCenterArcTween {
        MaterialRectCenterArcTween {
            begin,
            end,
            center_arc: MaterialPointArcTween::new(rect_center(begin), rect_center(end)),
        }
    }

    pub fn center_arc(&self) -> MaterialPointArcTween {
        self.center_arc
    }

    pub fn lerp(&self, t: f32) -> Rect {
        if t == 0.0 {
            return self.begin;
        }
        if t == 1.0 {
            return self.end;
        }
        let center = self.center_arc.lerp(t);
        let width = self.begin.width() + (self.end.width() - self.begin.width()) * t;
        let height = self.begin.height() + (self.end.height() - self.begin.height()) * t;
        Rect::from_center(center.dx, center.dy, width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_along_one_axis_is_left_straight() {
        // An arc between two points a pixel apart on one axis would need an
        // enormous radius and would read as a wobble, so upstream's
        // two-pixel threshold leaves it alone.
        let flat = MaterialPointArcTween::new(Offset::new(0.0, 0.0), Offset::new(100.0, 1.0));
        assert!(flat.is_straight());
        assert_eq!(flat.center(), None);
        assert_eq!(flat.radius(), None);
        assert_eq!(flat.lerp(0.5), Offset::new(50.0, 0.5), "a plain lerp");

        // And exactly at the threshold: `<=` on both sides, so two pixels is
        // still on-axis.
        let at_threshold =
            MaterialPointArcTween::new(Offset::new(0.0, 0.0), Offset::new(100.0, 2.0));
        assert!(at_threshold.is_straight());
        let past = MaterialPointArcTween::new(Offset::new(0.0, 0.0), Offset::new(100.0, 2.1));
        assert!(!past.is_straight());
    }

    #[test]
    fn a_diagonal_move_swings() {
        let arc = MaterialPointArcTween::new(Offset::new(0.0, 0.0), Offset::new(100.0, 100.0));
        assert!(!arc.is_straight());
        // The midpoint of the arc is off the straight line between the ends,
        // which is the whole point of the class.
        let straight = Offset::new(50.0, 50.0);
        let swung = arc.lerp(0.5);
        assert!(
            swung.minus(straight).distance() > 1.0,
            "off the chord: {swung:?}"
        );
    }

    #[test]
    fn the_ends_are_answered_exactly_rather_than_computed() {
        // An arc's ends come out of trigonometry that is right to within a
        // rounding error, and a rounding error at t == 1 is a thing that
        // stops a pixel short of where it was going.
        let begin = Offset::new(3.0, 7.0);
        let end = Offset::new(211.0, 133.0);
        let arc = MaterialPointArcTween::new(begin, end);
        assert_eq!(arc.lerp(0.0), begin);
        assert_eq!(arc.lerp(1.0), end);
    }

    #[test]
    fn the_arc_stays_on_its_own_circle() {
        // Every point of it is `radius` from `center` -- which is what says
        // the angles, the radius and the centre agree with each other.
        let arc = MaterialPointArcTween::new(Offset::new(10.0, 20.0), Offset::new(140.0, 220.0));
        let center = arc.center().expect("a circle");
        let radius = arc.radius().expect("a radius");
        for step in 0..=10 {
            let t = step as f32 / 10.0;
            let point = arc.lerp(t);
            assert!(
                (point.minus(center).distance() - radius).abs() < 0.5,
                "t={t} strayed off the circle"
            );
        }
    }

    #[test]
    fn the_end_angle_is_the_end_angle_which_upstream_gets_wrong() {
        // Upstream's `endAngle` getter returns `_beginAngle` -- a typo, since
        // `lerp` reads the real field and only the accessor lies. Ported as
        // the name promises; this line is what keeps the deliberate
        // divergence from being lost, and it would fail the moment someone
        // "restored" upstream's body.
        let arc = MaterialPointArcTween::new(Offset::new(0.0, 0.0), Offset::new(100.0, 200.0));
        let begin = arc.begin_angle().expect("an angle");
        let end = arc.end_angle().expect("an angle");
        assert_ne!(begin, end, "an arc that sweeps has two different angles");
        // And the end angle is the one that actually places the end point.
        let placed = arc.center().unwrap().plus(Offset::new(
            end.cos() * arc.radius().unwrap(),
            end.sin() * arc.radius().unwrap(),
        ));
        assert!(placed.minus(arc.end).distance() < 0.5, "{placed:?}");
    }

    #[test]
    fn a_rectangle_picks_the_diagonal_that_points_where_it_is_going() {
        // Animating a fixed pair would make a rectangle moving down-right
        // stretch and squash as one corner ran ahead of the other.
        let begin = Rect::ltrb(0.0, 0.0, 50.0, 50.0);
        let down_right = MaterialRectArcTween::new(begin, Rect::ltrb(200.0, 200.0, 250.0, 250.0));
        // Travelling down-right, the leading diagonal is top-left to
        // bottom-right, so the first arc starts at the top-left corner.
        assert_eq!(down_right.begin_arc().begin, Offset::new(0.0, 0.0));
        assert_eq!(down_right.end_arc().begin, Offset::new(50.0, 50.0));

        // Travelling up-right, it is the bottom-left to top-right one.
        let up_right = MaterialRectArcTween::new(begin, Rect::ltrb(200.0, -200.0, 250.0, -150.0));
        assert_eq!(up_right.begin_arc().begin, Offset::new(0.0, 50.0));
        assert_eq!(up_right.end_arc().begin, Offset::new(50.0, 0.0));
    }

    #[test]
    fn a_tie_between_diagonals_breaks_the_same_way_every_time() {
        // Upstream's `_maxBy` replaces only on a strict `>`, so the first of
        // equal maxima wins. A rectangle moving straight down scores two
        // diagonals the same, and a tie that broke differently between frames
        // would flip the arc mid-flight.
        let begin = Rect::ltrb(0.0, 0.0, 50.0, 50.0);
        let straight_down = MaterialRectArcTween::new(begin, Rect::ltrb(0.0, 200.0, 50.0, 250.0));
        let again = MaterialRectArcTween::new(begin, Rect::ltrb(0.0, 200.0, 50.0, 250.0));
        assert_eq!(straight_down.begin_arc().begin, again.begin_arc().begin);
    }

    #[test]
    fn a_rectangle_arc_keeps_its_corners_the_right_way_round() {
        // The chosen diagonal may run either way, so the two arcs' points are
        // sorted rather than trusted -- a rectangle whose right edge is left
        // of its left edge is not a rectangle.
        let arc = MaterialRectArcTween::new(
            Rect::ltrb(0.0, 0.0, 50.0, 50.0),
            Rect::ltrb(200.0, 200.0, 250.0, 250.0),
        );
        for step in 0..=10 {
            let t = step as f32 / 10.0;
            let rect = arc.lerp(t);
            assert!(rect.right >= rect.left, "t={t}");
            assert!(rect.bottom >= rect.top, "t={t}");
        }
    }

    #[test]
    fn a_centre_arc_swings_the_centre_and_lerps_the_size() {
        // The difference from `MaterialRectArcTween`: what is animated, not
        // how. Here the size is a plain interpolation and only the centre
        // swings.
        let arc = MaterialRectCenterArcTween::new(
            Rect::ltrb(0.0, 0.0, 100.0, 100.0),
            Rect::ltrb(200.0, 200.0, 400.0, 300.0),
        );
        let half = arc.lerp(0.5);
        // Compared with a tolerance because the rectangle is rebuilt from a
        // centre and a size, so its edges carry the centre's rounding.
        assert!(
            (half.width() - 150.0).abs() < 0.001,
            "half way between 100 and 200"
        );
        assert!(
            (half.height() - 100.0).abs() < 0.001,
            "both rectangles are 100 tall, so the height does not move"
        );
        // And the centre is on the arc, off the straight line between the two
        // centres -- (50, 50) to (300, 250), whose midpoint is (175, 150).
        let centre = Offset::new(half.center().0, half.center().1);
        let chord = Offset::new(175.0, 150.0);
        assert!(centre.minus(chord).distance() > 1.0, "{centre:?}");
    }

    #[test]
    fn both_rectangle_tweens_answer_their_ends_exactly() {
        let begin = Rect::ltrb(1.0, 2.0, 33.0, 44.0);
        let end = Rect::ltrb(101.0, 202.0, 303.0, 404.0);
        let corners = MaterialRectArcTween::new(begin, end);
        let centre = MaterialRectCenterArcTween::new(begin, end);
        assert_eq!(corners.lerp(0.0), begin);
        assert_eq!(corners.lerp(1.0), end);
        assert_eq!(centre.lerp(0.0), begin);
        assert_eq!(centre.lerp(1.0), end);
    }
}
