//! A port of `material/page_transitions_theme.dart` and
//! `material/predictive_back_page_transitions_builder.dart`.
//!
//! The rest of the page-transition family, which tick 55 started with the
//! abstract base and Android's two older answers. These are the newer ones and
//! the table that picks between them.

use crate::engine::Color;
use crate::page_transitions_builder::{PageTransitionFrame, PageTransitionsBuilder};
use crate::scroll_plumbing::ScrollPlatform;

/// Upstream `FadeForwardsPageTransitionsBuilder`, matching Android U.
///
/// Its duration carries the third admitted approximation this port has found,
/// and the most explicit of them: *"Eyeballed on a physical Pixel 9 running
/// Android 16. This does not match the actual value used by native Android,
/// which is 800ms, because native Android is using Material 3 Expressive
/// springs that are not currently supported by Flutter."*
///
/// It names the number it is not, says why it cannot be that number, and says
/// the gap is temporary. Compare the overscroll pair, which admitted the
/// mystery but could not name what the right answer would have been.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FadeForwardsPageTransitionsBuilder {
    /// The colour behind the two pages while they cross.
    ///
    /// It exists to stop a black flash: for the moment when one page is fading
    /// out and the other fading in, **neither is opaque**, and with nothing
    /// behind them the reader sees through to nothing. Defaults to the colour
    /// scheme's surface.
    pub background_color: Option<Color>,
}

impl FadeForwardsPageTransitionsBuilder {
    pub const TRANSITION_MILLISECONDS: u32 = 450;
    /// What native Android actually uses, recorded because upstream records it.
    pub const NATIVE_ANDROID_MILLISECONDS: u32 = 800;

    pub fn new() -> FadeForwardsPageTransitionsBuilder {
        FadeForwardsPageTransitionsBuilder {
            background_color: None,
        }
    }

    pub fn with_background(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Whether the duration is upstream's own value or the platform's.
    pub fn matches_native_duration() -> bool {
        FadeForwardsPageTransitionsBuilder::TRANSITION_MILLISECONDS
            == FadeForwardsPageTransitionsBuilder::NATIVE_ANDROID_MILLISECONDS
    }
}

impl PageTransitionsBuilder for FadeForwardsPageTransitionsBuilder {
    fn transition_duration_ms(&self) -> u32 {
        FadeForwardsPageTransitionsBuilder::TRANSITION_MILLISECONDS
    }

    fn primary(&self, t: f32) -> PageTransitionFrame {
        PageTransitionFrame {
            // Forwards: the arriving page comes in from the trailing side.
            translation_y: 0.0,
            opacity: t,
            reveal: 1.0,
            scrim_opacity: 0.0,
        }
    }

    fn secondary(&self, t: f32) -> PageTransitionFrame {
        PageTransitionFrame {
            translation_y: 0.0,
            // And the leaving page fades out as it goes, which is why the
            // background colour is needed at all.
            opacity: 1.0 - t,
            reveal: 1.0,
            scrim_opacity: 0.0,
        }
    }
}

/// Upstream `ZoomPageTransitionsBuilder`, matching Android Q.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomPageTransitionsBuilder {
    /// Whether to animate a **snapshot** of the routes rather than the routes
    /// themselves.
    ///
    /// Upstream names the cost as plainly as the benefit: with this on,
    /// *"animations that occur on the entering/exiting route while the route
    /// animation plays may appear frozen -- unless they are a hero animation or
    /// something that is drawn in a separate overlay."*
    ///
    /// Which is what a snapshot is: **a photograph, and anything moving inside
    /// it stops moving.** The trade is one rasterisation against a whole
    /// subtree repainted every frame of the transition, and for most pages the
    /// photograph wins.
    pub allow_snapshotting: bool,
    /// Separately settable, because the entering route is the one the reader is
    /// about to touch -- a frozen page they are arriving at is worse than a
    /// frozen one they are leaving.
    pub allow_enter_route_snapshotting: bool,
}

impl ZoomPageTransitionsBuilder {
    pub fn new() -> ZoomPageTransitionsBuilder {
        ZoomPageTransitionsBuilder {
            allow_snapshotting: true,
            allow_enter_route_snapshotting: true,
        }
    }

    pub fn without_snapshotting() -> ZoomPageTransitionsBuilder {
        ZoomPageTransitionsBuilder {
            allow_snapshotting: false,
            allow_enter_route_snapshotting: false,
        }
    }

    /// Whether the route arriving is drawn from a snapshot. Both flags have to
    /// agree.
    pub fn snapshots_entering_route(&self) -> bool {
        self.allow_snapshotting && self.allow_enter_route_snapshotting
    }

    pub fn snapshots_exiting_route(&self) -> bool {
        self.allow_snapshotting
    }
}

impl Default for ZoomPageTransitionsBuilder {
    fn default() -> Self {
        ZoomPageTransitionsBuilder::new()
    }
}

impl PageTransitionsBuilder for ZoomPageTransitionsBuilder {
    fn primary(&self, t: f32) -> PageTransitionFrame {
        PageTransitionFrame {
            translation_y: 0.0,
            opacity: t,
            reveal: 1.0,
            scrim_opacity: 0.0,
        }
    }
}

/// Which builder a [`PageTransitionsTheme`] resolved to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedTransition {
    PredictiveBack,
    Cupertino,
    Zoom,
    FadeForwards,
    OpenUpwards,
    FadeUpwards,
}

/// Upstream `PageTransitionsTheme`: the table from platform to transition.
#[derive(Clone, Debug, PartialEq)]
pub struct PageTransitionsTheme {
    builders: Vec<(ScrollPlatform, ResolvedTransition)>,
}

impl PageTransitionsTheme {
    /// Upstream's `_defaultBuilders`. Note what is **not** in it: there is no
    /// entry for Fuchsia, and the lookup falls back rather than failing.
    pub fn new() -> PageTransitionsTheme {
        PageTransitionsTheme {
            builders: vec![
                (ScrollPlatform::Android, ResolvedTransition::PredictiveBack),
                (ScrollPlatform::IOS, ResolvedTransition::Cupertino),
                (ScrollPlatform::MacOS, ResolvedTransition::Cupertino),
                (ScrollPlatform::Windows, ResolvedTransition::Zoom),
                (ScrollPlatform::Linux, ResolvedTransition::Zoom),
            ],
        }
    }

    pub fn with_builders(
        builders: Vec<(ScrollPlatform, ResolvedTransition)>,
    ) -> PageTransitionsTheme {
        PageTransitionsTheme { builders }
    }

    pub fn builders(&self) -> &[(ScrollPlatform, ResolvedTransition)] {
        &self.builders
    }

    /// Upstream `buildTransitions` and `delegatedTransition` both fall back to
    /// the zoom transition when the platform is not in the table.
    ///
    /// A fallback rather than an assert: a new platform, or one an application
    /// deliberately did not configure, gets something reasonable rather than
    /// nothing. The choice of zoom is the neutral one -- it belongs to no
    /// platform in particular.
    pub fn resolve(&self, platform: ScrollPlatform) -> ResolvedTransition {
        self.builders
            .iter()
            .find(|(candidate, _)| *candidate == platform)
            .map(|(_, transition)| *transition)
            .unwrap_or(ResolvedTransition::Zoom)
    }

    /// Upstream's `_all`, whose comment says exactly what it is for: *"Map the
    /// builders to a list with one PageTransitionsBuilder per platform for the
    /// operator == overload."*
    ///
    /// Comparing the maps directly would compare their key sets; what actually
    /// matters is whether the two themes answer the same for every platform.
    /// A theme listing four platforms and one listing five can be identical in
    /// effect, and this is how that is noticed.
    pub fn all_platforms(&self) -> Vec<ResolvedTransition> {
        [
            ScrollPlatform::Android,
            ScrollPlatform::Fuchsia,
            ScrollPlatform::IOS,
            ScrollPlatform::Linux,
            ScrollPlatform::MacOS,
            ScrollPlatform::Windows,
        ]
        .iter()
        .map(|platform| self.resolve(*platform))
        .collect()
    }

    /// Two themes are the same when they answer the same everywhere.
    pub fn same_as(&self, other: &PageTransitionsTheme) -> bool {
        self.all_platforms() == other.all_platforms()
    }
}

impl Default for PageTransitionsTheme {
    fn default() -> Self {
        PageTransitionsTheme::new()
    }
}

/// Upstream `PredictiveBackPageTransitionsBuilder`, matching Android U.
///
/// The transition that follows the finger during a system back gesture, and
/// the one decision worth reading is that **it only runs for an actual
/// gesture.** A button press or a programmatic pop falls back to
/// [`FadeForwardsPageTransitionsBuilder`].
///
/// Which is right, and not merely conservative: "predictive" means the
/// animation is tracking a drag that has not finished, and there is nothing to
/// track when a button was pressed. Running it anyway would be a shape with no
/// input driving it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PredictiveBackPageTransitionsBuilder {
    /// The scrim behind the transition on a platform that does not support the
    /// gesture at all. Defaults to the fallback builder's own background.
    pub fallback_color: Option<Color>,
}

impl PredictiveBackPageTransitionsBuilder {
    pub fn new() -> PredictiveBackPageTransitionsBuilder {
        PredictiveBackPageTransitionsBuilder {
            fallback_color: None,
        }
    }

    /// Upstream borrows the fade-forwards duration rather than choosing its
    /// own, so a gesture that is abandoned and a button press that follows look
    /// like the same transition.
    pub fn transition_duration_ms() -> u32 {
        FadeForwardsPageTransitionsBuilder::TRANSITION_MILLISECONDS
    }

    /// Upstream's branch in the builder callback.
    pub fn resolve(&self, pop_gesture_in_progress: bool) -> ResolvedTransition {
        if pop_gesture_in_progress {
            ResolvedTransition::PredictiveBack
        } else {
            ResolvedTransition::FadeForwards
        }
    }
}

/// Upstream `PredictiveBackFullscreenPageTransitionsBuilder`.
///
/// The same gesture, for a route that covers the screen rather than sitting in
/// a shared-element arrangement. Its fallback is the **zoom** transition rather
/// than fade-forwards, which is the whole difference: a full-screen route
/// arriving without a gesture is the ordinary case the zoom was written for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PredictiveBackFullscreenPageTransitionsBuilder {
    pub fallback_color: Option<Color>,
}

impl PredictiveBackFullscreenPageTransitionsBuilder {
    pub fn new() -> PredictiveBackFullscreenPageTransitionsBuilder {
        PredictiveBackFullscreenPageTransitionsBuilder {
            fallback_color: None,
        }
    }

    pub fn resolve(&self, pop_gesture_in_progress: bool) -> ResolvedTransition {
        if pop_gesture_in_progress {
            ResolvedTransition::PredictiveBack
        } else {
            ResolvedTransition::Zoom
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- FadeForwards ----------------------------------------------------------

    #[test]
    fn the_duration_is_an_eyeballed_approximation_and_says_so() {
        // Upstream names the number it is not, why it cannot be that number,
        // and that the gap is temporary.
        assert_eq!(
            FadeForwardsPageTransitionsBuilder::TRANSITION_MILLISECONDS,
            450
        );
        assert_eq!(
            FadeForwardsPageTransitionsBuilder::NATIVE_ANDROID_MILLISECONDS,
            800
        );
        assert!(!FadeForwardsPageTransitionsBuilder::matches_native_duration());
    }

    #[test]
    fn there_is_a_moment_when_neither_page_is_opaque() {
        // Which is what the background colour is for: without it the reader
        // sees through to nothing.
        let builder = FadeForwardsPageTransitionsBuilder::new();
        let arriving = builder.primary(0.5).opacity;
        let leaving = builder.secondary(0.5).opacity;
        assert!(arriving < 1.0 && leaving < 1.0, "{arriving}, {leaving}");
        assert!(
            builder.background_color.is_none(),
            "and it defaults to the surface"
        );
    }

    #[test]
    fn the_ends_are_still_exact() {
        let builder = FadeForwardsPageTransitionsBuilder::new();
        assert_eq!(builder.primary(0.0).opacity, 0.0);
        assert_eq!(builder.primary(1.0).opacity, 1.0);
        assert_eq!(builder.secondary(0.0).opacity, 1.0);
        assert_eq!(builder.secondary(1.0).opacity, 0.0);
    }

    // -- Zoom -------------------------------------------------------------------

    #[test]
    fn a_snapshot_is_a_photograph_and_anything_moving_inside_it_stops() {
        // Upstream names the cost as plainly as the benefit.
        let default = ZoomPageTransitionsBuilder::new();
        assert!(default.snapshots_entering_route());
        assert!(default.snapshots_exiting_route());

        let live = ZoomPageTransitionsBuilder::without_snapshotting();
        assert!(!live.snapshots_entering_route());
        assert!(!live.snapshots_exiting_route());
    }

    #[test]
    fn the_arriving_page_can_be_left_live_while_the_leaving_one_is_frozen() {
        // A frozen page the reader is arriving at is worse than a frozen one
        // they are leaving.
        let mut builder = ZoomPageTransitionsBuilder::new();
        builder.allow_enter_route_snapshotting = false;
        assert!(!builder.snapshots_entering_route());
        assert!(builder.snapshots_exiting_route());
    }

    #[test]
    fn turning_snapshotting_off_entirely_beats_the_per_route_flag() {
        let mut builder = ZoomPageTransitionsBuilder::new();
        builder.allow_snapshotting = false;
        assert!(!builder.snapshots_entering_route());
    }

    // -- The table ----------------------------------------------------------------

    #[test]
    fn each_platform_gets_the_transition_it_is_used_to() {
        let theme = PageTransitionsTheme::new();
        assert_eq!(
            theme.resolve(ScrollPlatform::Android),
            ResolvedTransition::PredictiveBack
        );
        assert_eq!(
            theme.resolve(ScrollPlatform::IOS),
            ResolvedTransition::Cupertino
        );
        assert_eq!(
            theme.resolve(ScrollPlatform::MacOS),
            ResolvedTransition::Cupertino
        );
        assert_eq!(
            theme.resolve(ScrollPlatform::Windows),
            ResolvedTransition::Zoom
        );
        assert_eq!(
            theme.resolve(ScrollPlatform::Linux),
            ResolvedTransition::Zoom
        );
    }

    #[test]
    fn a_platform_nobody_listed_falls_back_rather_than_failing() {
        // Fuchsia is not in the default table at all. The fallback is the zoom
        // transition, which belongs to no platform in particular.
        let theme = PageTransitionsTheme::new();
        assert!(
            !theme
                .builders()
                .iter()
                .any(|(platform, _)| *platform == ScrollPlatform::Fuchsia)
        );
        assert_eq!(
            theme.resolve(ScrollPlatform::Fuchsia),
            ResolvedTransition::Zoom
        );
    }

    #[test]
    fn two_themes_are_the_same_when_they_answer_the_same_everywhere() {
        // Comparing the maps would compare their key sets; what matters is the
        // answer per platform. A theme listing four platforms and one listing
        // five can be identical in effect.
        let implicit = PageTransitionsTheme::with_builders(vec![(
            ScrollPlatform::Android,
            ResolvedTransition::Zoom,
        )]);
        let explicit = PageTransitionsTheme::with_builders(vec![
            (ScrollPlatform::Android, ResolvedTransition::Zoom),
            (ScrollPlatform::Fuchsia, ResolvedTransition::Zoom),
            (ScrollPlatform::IOS, ResolvedTransition::Zoom),
            (ScrollPlatform::Linux, ResolvedTransition::Zoom),
            (ScrollPlatform::MacOS, ResolvedTransition::Zoom),
            (ScrollPlatform::Windows, ResolvedTransition::Zoom),
        ]);
        assert_ne!(
            implicit.builders().len(),
            explicit.builders().len(),
            "different tables"
        );
        assert!(implicit.same_as(&explicit), "and the same theme");
    }

    #[test]
    fn a_theme_that_differs_anywhere_is_a_different_theme() {
        let default = PageTransitionsTheme::new();
        let all_zoom = PageTransitionsTheme::with_builders(vec![(
            ScrollPlatform::Android,
            ResolvedTransition::Zoom,
        )]);
        assert!(!default.same_as(&all_zoom));
    }

    // -- Predictive back -------------------------------------------------------------

    #[test]
    fn predictive_back_runs_only_for_an_actual_gesture() {
        // "Predictive" means tracking a drag that has not finished, and there
        // is nothing to track when a button was pressed.
        let builder = PredictiveBackPageTransitionsBuilder::new();
        assert_eq!(builder.resolve(true), ResolvedTransition::PredictiveBack);
        assert_eq!(builder.resolve(false), ResolvedTransition::FadeForwards);
    }

    #[test]
    fn the_fullscreen_one_falls_back_to_zoom_instead() {
        // Which is the whole difference between the two: a full-screen route
        // arriving without a gesture is the ordinary case the zoom was written
        // for.
        let fullscreen = PredictiveBackFullscreenPageTransitionsBuilder::new();
        assert_eq!(fullscreen.resolve(true), ResolvedTransition::PredictiveBack);
        assert_eq!(fullscreen.resolve(false), ResolvedTransition::Zoom);

        assert_ne!(
            fullscreen.resolve(false),
            PredictiveBackPageTransitionsBuilder::new().resolve(false)
        );
    }

    #[test]
    fn an_abandoned_gesture_and_a_button_press_look_like_the_same_transition() {
        // Because the predictive builder borrows the fade-forwards duration
        // rather than choosing its own.
        assert_eq!(
            PredictiveBackPageTransitionsBuilder::transition_duration_ms(),
            FadeForwardsPageTransitionsBuilder::new().transition_duration_ms()
        );
    }

    #[test]
    fn the_scrim_defaults_to_whatever_the_fallback_builder_uses() {
        assert!(
            PredictiveBackPageTransitionsBuilder::new()
                .fallback_color
                .is_none()
        );
        assert!(
            PredictiveBackFullscreenPageTransitionsBuilder::new()
                .fallback_color
                .is_none()
        );
    }
}
