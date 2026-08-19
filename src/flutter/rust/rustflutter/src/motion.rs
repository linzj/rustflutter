// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Upstream `material/motion.dart`: the durations and easings the Material
//! specification names.
//!
//! Both are generated upstream from the Material token database, and both are
//! namespaces rather than types -- upstream declares them
//! `abstract final class`, which is Dart for "this is a bag of constants and
//! you may not instantiate it". Unit structs with associated constants are the
//! same statement here.
//!
//! What they are *for* is worth a line, because a table of sixteen durations
//! looks like indecision. It is the opposite: a design system that names its
//! durations can change how fast an entire application feels by moving one
//! number, and a control that reaches for [`Durations::MEDIUM2`] rather than
//! writing `300` is a control that will follow.

use crate::animation::Curve;

/// Upstream `Durations`, in microseconds -- this crate's clock unit
/// throughout, so a caller never converts.
///
/// The four bands are the specification's, and the reason there are four
/// rather than one scale is that the right duration depends on how far a
/// thing moves and how much of the screen it covers: a checkbox filling in
/// (`SHORT`) and a full-screen transition (`EXTRALONG`) are not the same event
/// slowed down.
pub struct Durations;

impl Durations {
    pub const SHORT1: i64 = 50_000;
    pub const SHORT2: i64 = 100_000;
    pub const SHORT3: i64 = 150_000;
    pub const SHORT4: i64 = 200_000;
    pub const MEDIUM1: i64 = 250_000;
    pub const MEDIUM2: i64 = 300_000;
    pub const MEDIUM3: i64 = 350_000;
    pub const MEDIUM4: i64 = 400_000;
    pub const LONG1: i64 = 450_000;
    pub const LONG2: i64 = 500_000;
    pub const LONG3: i64 = 550_000;
    pub const LONG4: i64 = 600_000;
    pub const EXTRALONG1: i64 = 700_000;
    pub const EXTRALONG2: i64 = 800_000;
    pub const EXTRALONG3: i64 = 900_000;
    pub const EXTRALONG4: i64 = 1_000_000;

    /// All sixteen in order, for a caller that wants to walk the scale.
    pub const ALL: [i64; 16] = [
        Durations::SHORT1,
        Durations::SHORT2,
        Durations::SHORT3,
        Durations::SHORT4,
        Durations::MEDIUM1,
        Durations::MEDIUM2,
        Durations::MEDIUM3,
        Durations::MEDIUM4,
        Durations::LONG1,
        Durations::LONG2,
        Durations::LONG3,
        Durations::LONG4,
        Durations::EXTRALONG1,
        Durations::EXTRALONG2,
        Durations::EXTRALONG3,
        Durations::EXTRALONG4,
    ];
}

/// Upstream `Easing`: the named curves of the Material specification.
///
/// The three families are worth telling apart, because the names do not say
/// it outright:
///
/// * **`STANDARD`** is the everyday one -- a thing that starts and stops on
///   screen.
/// * **`*_ACCELERATE`** is for something *leaving*: it starts slow and speeds
///   up off the edge, so the eye is not asked to follow it out.
/// * **`*_DECELERATE`** is for something *arriving*: it enters fast and
///   settles, so the eye catches it where it stops rather than where it
///   started.
///
/// `EMPHASIZED` is the same three with more contrast, for the one movement on
/// screen that is the point of the change. `LEGACY` is Material 2's set, kept
/// because a theme may still be asking for it.
pub struct Easing;

impl Easing {
    pub const EMPHASIZED_ACCELERATE: Curve = Curve::Cubic(0.3, 0.0, 0.8, 0.15);
    pub const EMPHASIZED_DECELERATE: Curve = Curve::Cubic(0.05, 0.7, 0.1, 1.0);
    /// Upstream spells this as a cubic rather than reusing `Curves.linear`,
    /// and the cubic `(0, 0, 1, 1)` *is* the straight line -- kept as written
    /// so the table reads as one table.
    pub const LINEAR: Curve = Curve::Cubic(0.0, 0.0, 1.0, 1.0);
    pub const STANDARD: Curve = Curve::Cubic(0.2, 0.0, 0.0, 1.0);
    pub const STANDARD_ACCELERATE: Curve = Curve::Cubic(0.3, 0.0, 1.0, 1.0);
    pub const STANDARD_DECELERATE: Curve = Curve::Cubic(0.0, 0.0, 0.0, 1.0);
    pub const LEGACY_DECELERATE: Curve = Curve::Cubic(0.0, 0.0, 0.2, 1.0);
    pub const LEGACY_ACCELERATE: Curve = Curve::Cubic(0.4, 0.0, 1.0, 1.0);
    pub const LEGACY: Curve = Curve::Cubic(0.4, 0.0, 0.2, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_duration_scale_only_ever_grows() {
        // Sixteen names over four bands, and the point of naming them is that
        // a design can move one number. A scale that was not monotonic would
        // make "longer" a lie.
        for pair in Durations::ALL.windows(2) {
            assert!(pair[1] > pair[0], "{pair:?}");
        }
        assert_eq!(Durations::SHORT1, 50_000, "50ms, in this crate's micros");
        assert_eq!(Durations::EXTRALONG4, 1_000_000, "one second");
    }

    #[test]
    fn every_easing_starts_at_nothing_and_ends_at_everything() {
        // A curve that did not would make a control jump at one end of its
        // animation, which is the one thing an easing must never do.
        for curve in [
            Easing::EMPHASIZED_ACCELERATE,
            Easing::EMPHASIZED_DECELERATE,
            Easing::LINEAR,
            Easing::STANDARD,
            Easing::STANDARD_ACCELERATE,
            Easing::STANDARD_DECELERATE,
            Easing::LEGACY_DECELERATE,
            Easing::LEGACY_ACCELERATE,
            Easing::LEGACY,
        ] {
            assert!(curve.transform(0.0).abs() < 0.001, "{curve:?} at zero");
            assert!(
                (curve.transform(1.0) - 1.0).abs() < 0.001,
                "{curve:?} at one"
            );
        }
    }

    #[test]
    fn accelerate_leaves_slowly_and_decelerate_arrives_quickly() {
        // Which is the whole difference, and what decides which one a
        // transition should use: something leaving should not ask the eye to
        // follow it out, and something arriving should be caught where it
        // stops.
        assert!(
            Easing::STANDARD_ACCELERATE.transform(0.25) < 0.25,
            "behind the line early on"
        );
        assert!(
            Easing::STANDARD_DECELERATE.transform(0.25) > 0.25,
            "ahead of it"
        );
    }

    #[test]
    fn upstreams_linear_cubic_really_is_the_straight_line() {
        // Upstream spells it as a cubic rather than reusing `Curves.linear`,
        // so it is worth checking that the spelling is the same thing.
        for step in 0..=10 {
            let t = step as f32 / 10.0;
            assert!((Easing::LINEAR.transform(t) - t).abs() < 0.001, "t={t}");
        }
    }

    #[test]
    fn emphasized_is_the_standard_pair_with_more_contrast() {
        // The reason there are two families rather than one: emphasized is
        // for the single movement that is the point of the change, so it
        // pulls further from the straight line than standard does.
        let early = 0.2;
        assert!(
            Easing::EMPHASIZED_ACCELERATE.transform(early)
                < Easing::STANDARD_ACCELERATE.transform(early),
            "emphasized leaves more slowly still"
        );
    }
}
