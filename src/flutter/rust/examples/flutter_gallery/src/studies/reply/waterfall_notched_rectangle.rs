// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/reply/waterfall_notched_rectangle.dart` (flutter/
//! gallery @ d12640d): the smooth circular notch the mobile bottom app bar
//! wraps around the docked FAB.
//!
//! The Bezier math is upstream's, formula for formula (the derivation it cites
//! is at https://goo.gl/Ufzrqn). One divergence: upstream closes the notch
//! with `Path.arcToPoint`, and the framework's `RenderPath` has no arc verb,
//! so the arc is sampled into line segments ([`ARC_STEPS`] of them) -- at the
//! sizes this is drawn (a ~68px circle) the chords are sub-pixel.

use rustflutter::engine::Rect;
use rustflutter::painting::RenderPath;

/// How the arc is sampled; see the module header.
const ARC_STEPS: usize = 24;

/// The six control points upstream computes, in absolute coordinates:
/// p0..p2 drive segment A (Bezier from the host edge to the arc), p3 is the
/// arc's far end, and p4..p5 mirror p1..p0 for segment C.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotchPoints {
    pub p: [(f32, f32); 6],
    pub notch_radius: f32,
    /// The guest circle's center; the arc runs around it.
    pub center: (f32, f32),
}

/// Upstream's `getOuterPath`, as the point math rather than a path: `None`
/// when there is no notch (no guest, or no overlap), which upstream answers
/// with a plain rectangle.
pub fn notch_points(host: Rect, guest: Option<Rect>) -> Option<NotchPoints> {
    let guest = guest?;
    if !overlaps(host, guest) {
        return None;
    }

    // The guest's shape is a circle bounded by the guest rectangle. So the
    // guest's radius is half the guest width.
    let notch_radius = guest.width() / 2.0;

    // We build a path for the notch from 3 segments:
    // Segment A - a Bezier curve from the host's top edge to segment B.
    // Segment B - an arc with radius notchRadius.
    // Segment C - a Bezier curve from segment B back to the host's top edge.

    // s1, s2 are the two knobs controlling the behavior of the bezier curve.
    const S1: f32 = 21.0;
    const S2: f32 = 6.0;

    let r = notch_radius;
    let a = -1.0 * r - S2;
    let b = host.top - (guest.top + guest.height() / 2.0);

    let n2 = (b * b * r * r * (a * a + b * b - r * r)).sqrt();
    let p2x_a = ((a * r * r) - n2) / (a * a + b * b);
    let p2x_b = ((a * r * r) + n2) / (a * a + b * b);
    let p2y_a = (r * r - p2x_a * p2x_a).sqrt();
    let p2y_b = (r * r - p2x_b * p2x_b).sqrt();

    let mut p = [(0.0, 0.0); 6];

    // p0, p1, and p2 are the control points for segment A.
    p[0] = (a - S1, b);
    p[1] = (a, b);
    let cmp = if b < 0.0 { -1.0 } else { 1.0 };
    p[2] = if cmp * p2y_a > cmp * p2y_b {
        (p2x_a, p2y_a)
    } else {
        (p2x_b, p2y_b)
    };

    // p3, p4, and p5 are the control points for segment B, which is a mirror
    // of segment A around the y axis.
    p[3] = (-1.0 * p[2].0, p[2].1);
    p[4] = (-1.0 * p[1].0, p[1].1);
    p[5] = (-1.0 * p[0].0, p[0].1);

    // Translate all points back to the absolute coordinate system.
    let center = (
        guest.left + guest.width() / 2.0,
        guest.top + guest.height() / 2.0,
    );
    for point in &mut p {
        point.0 += center.0;
        point.1 += center.1;
    }

    Some(NotchPoints {
        p,
        notch_radius,
        center,
    })
}

/// Upstream's `Rect.overlaps`.
fn overlaps(a: Rect, b: Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Upstream's `WaterfallNotchedRectangle.getOuterPath`: the host rectangle
/// with the notch cut into its top edge, or the plain rectangle when there is
/// no guest to notch around.
pub fn outer_path(host: Rect, guest: Option<Rect>) -> RenderPath {
    let mut path = RenderPath::new();
    let Some(notch) = notch_points(host, guest) else {
        path.add_rect(host);
        return path;
    };

    let p = notch.p;
    path.move_to(host.left, host.top);
    path.line_to(p[0].0, p[0].1);
    path.quadratic_to(p[1].0, p[1].1, p[2].0, p[2].1);
    // Upstream: `arcToPoint(p[3], radius: Radius.circular(notchRadius),
    // clockwise: false)` -- the arc of the guest circle from p2 round its
    // bottom to p3. Sampled, per the module header.
    let (cx, cy) = notch.center;
    let start = (p[2].0 - cx).atan2(p[2].1 - cy);
    let end = (p[3].0 - cx).atan2(p[3].1 - cy);
    // Counterclockwise in Flutter's y-down plane means the angle grows from
    // start to end through the bottom of the circle (+y).
    let mut sweep = end - start;
    while sweep <= 0.0 {
        sweep += std::f32::consts::TAU;
    }
    for step in 1..=ARC_STEPS {
        let angle = start + sweep * step as f32 / ARC_STEPS as f32;
        path.line_to(
            cx + notch.notch_radius * angle.cos(),
            cy + notch.notch_radius * angle.sin(),
        );
    }
    path.quadratic_to(p[4].0, p[4].1, p[5].0, p[5].1);
    path.line_to(host.right, host.top);
    path.line_to(host.right, host.bottom);
    path.line_to(host.left, host.bottom);
    path.close();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The geometry the mobile layout notches around: a bar across the bottom
    /// and the FAB's circle straddling its top edge, inflated by the 6px
    /// notch margin.
    fn host() -> Rect {
        Rect::ltrb(0.0, 0.0, 400.0, 56.0)
    }

    fn guest() -> Rect {
        Rect::xywh(200.0 - 34.0, -34.0, 68.0, 68.0)
    }

    #[test]
    fn no_guest_is_a_plain_rectangle() {
        assert!(notch_points(host(), None).is_none());
    }

    #[test]
    fn no_overlap_is_a_plain_rectangle() {
        let away = Rect::xywh(0.0, -500.0, 68.0, 68.0);
        assert!(notch_points(host(), Some(away)).is_none());
    }

    #[test]
    fn the_control_points_are_upstreams_formulas() {
        // Worked out from upstream's formulas for this geometry: the guest
        // circle (r = 34) is centered on the bar's top edge, so b = 0, n2 = 0
        // and p2 = (r^2 / a, sqrt(r^2 - p2x^2)) = (-28.9, 17.91). The points
        // below are absolute (translated by the guest's center, (200, 0)).
        let notch = notch_points(host(), Some(guest())).expect("a notch");
        let p2y = (34.0_f32 * 34.0 - 28.9 * 28.9).sqrt();

        let close = |actual: (f32, f32), expect: (f32, f32)| {
            assert!(
                (actual.0 - expect.0).abs() < 1e-3,
                "x: {} vs {}",
                actual.0,
                expect.0
            );
            assert!(
                (actual.1 - expect.1).abs() < 1e-3,
                "y: {} vs {}",
                actual.1,
                expect.1
            );
        };
        close(notch.p[0], (139.0, 0.0)); // a - s1 = -61
        close(notch.p[1], (160.0, 0.0)); // a = -40
        close(notch.p[2], (200.0 - 28.9, p2y));
        // Segment B mirrors segment A around the guest's vertical axis.
        close(notch.p[3], (200.0 + 28.9, p2y));
        close(notch.p[4], (240.0, 0.0));
        close(notch.p[5], (261.0, 0.0));
        assert_eq!(notch.notch_radius, 34.0);
        assert_eq!(notch.center, (200.0, 0.0));
    }

    #[test]
    fn the_notch_wraps_the_bottom_of_the_guest_circle() {
        let notch = notch_points(host(), Some(guest())).expect("a notch");
        // p2 and p3 sit on the guest circle, and below its center: the dip
        // into the bar.
        for point in [notch.p[2], notch.p[3]] {
            let dx = point.0 - notch.center.0;
            let dy = point.1 - notch.center.1;
            let distance = (dx * dx + dy * dy).sqrt();
            assert!((distance - notch.notch_radius).abs() < 1e-2);
            assert!(
                point.1 > notch.center.1,
                "the notch dips below the FAB center"
            );
        }
        // And the bezier ends sit on the host's top edge.
        assert_eq!(notch.p[0].1, host().top);
        assert_eq!(notch.p[5].1, host().top);
    }
}
