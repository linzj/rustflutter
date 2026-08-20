//! A port of `widgets/page_transitions_builder.dart`.
//!
//! How a page arrives. Each platform gets one of these off the theme, so the
//! same `MaterialPageRoute` slides in on one platform and zooms in on another
//! without the route knowing anything about it.
//!
//! The two builders here are Android's older answers -- `FadeUpwards` matches
//! Android O and `OpenUpwards` matches Android P -- and putting them side by
//! side is the useful part: they solve the same problem with opposite habits.

use crate::animation::Curve;
use crate::engine::Color;

/// Upstream `PageTransitionsBuilder`.
///
/// An abstract class whose only required member is `buildTransitions`; the rest
/// are defaults a subclass may take as they are. Note that
/// `reverseTransitionDuration` defaults to `transitionDuration` rather than to
/// its own constant, so a subclass that lengthens the entrance gets a matching
/// exit for free and has to say so only if it wants them to differ.
pub trait PageTransitionsBuilder {
    /// Upstream's 300ms default.
    fn transition_duration_ms(&self) -> u32 {
        300
    }

    fn reverse_transition_duration_ms(&self) -> u32 {
        self.transition_duration_ms()
    }

    /// Upstream's `delegatedTransition`, which lets a route describe how the
    /// route *underneath* it should move. Defaults to none: most transitions
    /// only animate the page arriving.
    fn has_delegated_transition(&self) -> bool {
        false
    }

    /// The transform applied to the arriving page at `t`.
    fn primary(&self, t: f32) -> PageTransitionFrame;

    /// The transform applied to the page being left, at the secondary
    /// animation's `t`. Most transitions leave it alone.
    fn secondary(&self, _t: f32) -> PageTransitionFrame {
        PageTransitionFrame::identity()
    }
}

/// One frame of a page transition, as fractions of the page's own size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageTransitionFrame {
    /// Vertical offset as a fraction of the page height; negative is upward.
    pub translation_y: f32,
    /// The page's own opacity.
    pub opacity: f32,
    /// How much of the page, from the bottom, is revealed. 1.0 is all of it.
    pub reveal: f32,
    /// The opacity of the black scrim over the page.
    pub scrim_opacity: f32,
}

impl PageTransitionFrame {
    pub fn identity() -> PageTransitionFrame {
        PageTransitionFrame {
            translation_y: 0.0,
            opacity: 1.0,
            reveal: 1.0,
            scrim_opacity: 0.0,
        }
    }
}

/// Upstream `FadeUpwardsPageTransitionsBuilder`, matching Android O.
///
/// The page rises from a quarter of the screen below its final place while
/// fading in. What is worth noticing is that the two halves are driven by
/// **different curves** -- `fastOutSlowIn` for the movement and `easeIn` for
/// the fade -- rather than one curve applied to both. The page is most of the
/// way to its position while still most of the way transparent, so it does not
/// look like it is sliding into view so much as settling into it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FadeUpwardsPageTransitionsBuilder;

impl FadeUpwardsPageTransitionsBuilder {
    /// A quarter of the screen below the top.
    pub const BEGIN_OFFSET: f32 = 0.25;

    pub fn new() -> FadeUpwardsPageTransitionsBuilder {
        FadeUpwardsPageTransitionsBuilder
    }
}

impl PageTransitionsBuilder for FadeUpwardsPageTransitionsBuilder {
    fn primary(&self, t: f32) -> PageTransitionFrame {
        let position = Curve::FAST_OUT_SLOW_IN.transform(t);
        let opacity = Curve::EASE_IN.transform(t);
        PageTransitionFrame {
            translation_y: FadeUpwardsPageTransitionsBuilder::BEGIN_OFFSET * (1.0 - position),
            opacity,
            reveal: 1.0,
            scrim_opacity: 0.0,
        }
    }
}

/// Upstream `OpenUpwardsPageTransitionsBuilder`, matching Android P.
///
/// The opposite habit to [`FadeUpwardsPageTransitionsBuilder`]: **one curve for
/// everything**, and two animations instead of one, because the old page moves
/// too.
///
/// The new page is not faded in at all -- it is *revealed*, through a clip
/// rectangle growing from the bottom. Upstream lays the page out at full height
/// inside an `OverflowBox` and then shrinks the box around it, so the content
/// never squashes: what changes is how much of it you can see. It also slides
/// up 5% while that happens, and the old page slides up 2.5% -- the same
/// direction, half as far, so the page underneath follows rather than being
/// pushed. A scrim darkens it to a quarter black.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenUpwardsPageTransitionsBuilder;

impl OpenUpwardsPageTransitionsBuilder {
    /// The new page's slide, as a fraction of its height.
    pub const PRIMARY_TRANSLATION: f32 = 0.05;
    /// The old page's, in the same direction and half as far.
    pub const SECONDARY_TRANSLATION: f32 = -0.025;
    pub const SCRIM_COLOR: Color = Color(0xFF00_0000);
    pub const SCRIM_END_OPACITY: f32 = 0.25;

    /// `Cubic(0.20, 0.00, 0.00, 1.00)`. The second control point sits at x = 0,
    /// which drags nearly all of the motion into the first part of the
    /// animation -- the page flies open and then eases the last of the way.
    pub const CURVE: Curve = Curve::Cubic(0.20, 0.00, 0.00, 1.00);

    pub fn new() -> OpenUpwardsPageTransitionsBuilder {
        OpenUpwardsPageTransitionsBuilder
    }

    /// Upstream uses `curve.flipped` as the reverse curve, so the transition
    /// reversed is the mirror of itself rather than the same shape run
    /// backwards.
    pub fn reverse_at(t: f32) -> f32 {
        1.0 - OpenUpwardsPageTransitionsBuilder::CURVE.transform(1.0 - t)
    }
}

impl PageTransitionsBuilder for OpenUpwardsPageTransitionsBuilder {
    fn has_delegated_transition(&self) -> bool {
        // It moves the page underneath, but through its own secondary
        // animation rather than by handing one to the route below.
        false
    }

    fn primary(&self, t: f32) -> PageTransitionFrame {
        let curved = OpenUpwardsPageTransitionsBuilder::CURVE.transform(t);
        PageTransitionFrame {
            translation_y: OpenUpwardsPageTransitionsBuilder::PRIMARY_TRANSLATION * (1.0 - curved),
            // Never faded: the page is whole from the first frame, and only
            // partly in view.
            opacity: 1.0,
            reveal: curved,
            scrim_opacity: OpenUpwardsPageTransitionsBuilder::SCRIM_END_OPACITY * curved,
        }
    }

    fn secondary(&self, t: f32) -> PageTransitionFrame {
        let curved = OpenUpwardsPageTransitionsBuilder::CURVE.transform(t);
        PageTransitionFrame {
            translation_y: OpenUpwardsPageTransitionsBuilder::SECONDARY_TRANSLATION * curved,
            opacity: 1.0,
            reveal: 1.0,
            scrim_opacity: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FADE: FadeUpwardsPageTransitionsBuilder = FadeUpwardsPageTransitionsBuilder;
    const OPEN: OpenUpwardsPageTransitionsBuilder = OpenUpwardsPageTransitionsBuilder;

    #[test]
    fn the_exit_matches_the_entrance_unless_a_subclass_says_otherwise() {
        // Which is why reverseTransitionDuration defaults to
        // transitionDuration rather than to its own number.
        assert_eq!(FADE.transition_duration_ms(), 300);
        assert_eq!(FADE.reverse_transition_duration_ms(), 300);

        struct Slow;
        impl PageTransitionsBuilder for Slow {
            fn transition_duration_ms(&self) -> u32 {
                800
            }
            fn primary(&self, _t: f32) -> PageTransitionFrame {
                PageTransitionFrame::identity()
            }
        }
        assert_eq!(
            Slow.reverse_transition_duration_ms(),
            800,
            "lengthening one lengthened both"
        );
    }

    #[test]
    fn most_transitions_leave_the_page_underneath_alone() {
        assert_eq!(FADE.secondary(0.5), PageTransitionFrame::identity());
        assert!(!FADE.has_delegated_transition());
    }

    // -- Fade upwards -----------------------------------------------------------

    #[test]
    fn the_page_starts_a_quarter_of_a_screen_low_and_ends_in_place() {
        let start = FADE.primary(0.0);
        assert_eq!(start.translation_y, 0.25);
        assert_eq!(start.opacity, 0.0);

        let end = FADE.primary(1.0);
        assert!(end.translation_y.abs() < 1e-6);
        assert!((end.opacity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_movement_and_the_fade_run_on_different_curves() {
        // Which is the point of using two: the page is most of the way to its
        // place while still most of the way transparent, so it settles into
        // view rather than sliding into it.
        let half = FADE.primary(0.5);
        let position_progress = 1.0 - half.translation_y / 0.25;
        assert!(
            position_progress > half.opacity + 0.2,
            "position {position_progress}, opacity {}",
            half.opacity
        );
    }

    #[test]
    fn the_page_is_never_revealed_through_a_clip_it_is_all_there() {
        for step in 0..=10 {
            let frame = FADE.primary(step as f32 / 10.0);
            assert_eq!(frame.reveal, 1.0);
            assert_eq!(frame.scrim_opacity, 0.0);
        }
    }

    // -- Open upwards -----------------------------------------------------------

    #[test]
    fn the_new_page_is_revealed_rather_than_faded_in() {
        // It is whole from the first frame; what changes is how much of it is
        // in view. Upstream gets that from an OverflowBox at full height inside
        // a shrinking clip, so the content never squashes.
        for step in 0..=10 {
            assert_eq!(OPEN.primary(step as f32 / 10.0).opacity, 1.0);
        }
        assert_eq!(OPEN.primary(0.0).reveal, 0.0);
        assert!((OPEN.primary(1.0).reveal - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_old_page_follows_the_new_one_the_same_way_and_half_as_far() {
        // Not pushed aside -- it drifts after it.
        let new_start = OPEN.primary(0.0).translation_y;
        let old_end = OPEN.secondary(1.0).translation_y;
        assert_eq!(new_start, 0.05);
        assert!((old_end + 0.025).abs() < 1e-6);
        assert_eq!(
            new_start / 2.0,
            -old_end,
            "half as far, and both of them upward"
        );
    }

    #[test]
    fn the_scrim_darkens_the_old_page_to_a_quarter_black() {
        assert_eq!(OPEN.primary(0.0).scrim_opacity, 0.0);
        assert!((OPEN.primary(1.0).scrim_opacity - 0.25).abs() < 1e-6);
        assert_eq!(
            OpenUpwardsPageTransitionsBuilder::SCRIM_COLOR,
            Color(0xFF00_0000)
        );
    }

    #[test]
    fn one_curve_drives_everything_this_transition_does() {
        // The opposite habit to the fade, which uses two.
        let t = 0.35;
        let curved = OpenUpwardsPageTransitionsBuilder::CURVE.transform(t);
        let primary = OPEN.primary(t);
        assert!((primary.reveal - curved).abs() < 1e-6);
        assert!((primary.scrim_opacity - 0.25 * curved).abs() < 1e-6);
        assert!((primary.translation_y - 0.05 * (1.0 - curved)).abs() < 1e-6);
        assert!((OPEN.secondary(t).translation_y + 0.025 * curved).abs() < 1e-6);
    }

    #[test]
    fn the_curve_puts_half_the_motion_in_the_first_fifth_of_the_time() {
        // Its second control point sits at x = 0, which drags the motion
        // forward: the page flies open and then eases the last of the way.
        let curve = OpenUpwardsPageTransitionsBuilder::CURVE;
        assert!(
            curve.transform(0.2) > 0.45,
            "at a fifth of the way through: {}",
            curve.transform(0.2)
        );
        assert!(curve.transform(0.5) > 0.85);
        assert!(
            curve.transform(0.9) > 0.99,
            "and is almost finished long before the end"
        );
    }

    #[test]
    fn both_curves_still_start_at_nothing_and_end_at_everything() {
        for curve in [
            OpenUpwardsPageTransitionsBuilder::CURVE,
            Curve::FAST_OUT_SLOW_IN,
            Curve::EASE_IN,
        ] {
            assert!(curve.transform(0.0).abs() < 1e-4, "{curve:?}");
            assert!((curve.transform(1.0) - 1.0).abs() < 1e-4, "{curve:?}");
        }
    }

    #[test]
    fn reversing_mirrors_the_curve_rather_than_running_its_shape_backwards() {
        // A front-loaded curve run backwards would be front-loaded on the way
        // out too, which would look like the page snapping shut.
        let curve = OpenUpwardsPageTransitionsBuilder::CURVE;
        let forward_early = curve.transform(0.2);
        let reverse_early = OpenUpwardsPageTransitionsBuilder::reverse_at(0.2);
        assert!(forward_early > 0.45);
        assert!(
            reverse_early < 0.05,
            "the mirror is slow where the original is fast: {reverse_early}"
        );
        assert!(OpenUpwardsPageTransitionsBuilder::reverse_at(0.0).abs() < 1e-4);
        assert!((OpenUpwardsPageTransitionsBuilder::reverse_at(1.0) - 1.0).abs() < 1e-4);
    }
}
