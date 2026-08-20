//! Scroll physics -- a port of upstream's `widgets/scroll_physics.dart`.
//!
//! Physics answers four questions about a scroll, and the four are separate
//! because platforms disagree about them independently:
//!
//! * May the reader drag at all? (`shouldAcceptUserOffset`)
//! * How much does a drag of *n* pixels actually move the content?
//!   (`applyPhysicsToUserOffset`)
//! * What happens when a drag would go past the end?
//!   (`applyBoundaryConditions`)
//! * Where does a release fly to? (`createBallisticSimulation`)
//!
//! Android's answer to the third is "stop dead"; iOS's is "let it stretch and
//! spring back". Neither is a small tweak of the other, and both are the
//! platform's whole personality when scrolling.
//!
//! Physics **compose**: each object may have a parent, and a method that does
//! not care defers to it. That is what lets a caller say "the ambient physics,
//! but never scrollable" without knowing what the ambient physics is.
//!
//! ## What is not here
//!
//! `ScrollPosition`, the activity machinery and `applyTo`'s element-level
//! plumbing are not modelled -- see [`crate::scrolling`]. What is ported is
//! the composition, the four answers each class gives, and the friction and
//! momentum curves upstream arrived at by measurement.

use crate::physics::Tolerance;
use crate::scrolling::ScrollMetrics;

/// Upstream's `kMinFlingVelocity`.
pub const MIN_FLING_VELOCITY: f32 = 50.0;
/// Upstream's `kMaxFlingVelocity`.
pub const MAX_FLING_VELOCITY: f32 = 8000.0;

/// Upstream `ScrollDecelerationRate`: how quickly a fling gives up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollDecelerationRate {
    /// iOS's own rate, and the default.
    #[default]
    Normal,
    /// The faster rate a `CupertinoPageScaffold`'s inner lists use, where a
    /// fling that carried as far as a full-screen list's would overshoot.
    Fast,
}

/// Upstream `ScrollPhysics`: the base, and the composition point.
///
/// Upstream is a class with a `parent` and methods that delegate; this is a
/// trait with the same shape. Every method has the base answer as its default,
/// and an implementation overrides only what it changes -- which is why
/// `AlwaysScrollableScrollPhysics` and `NeverScrollableScrollPhysics` are one
/// override each.
pub trait ScrollPhysics {
    /// Upstream's `parent`. A method that does not care defers here.
    fn parent(&self) -> Option<&dyn ScrollPhysics> {
        None
    }

    /// Upstream's `shouldAcceptUserOffset`, whose base answer is **not**
    /// simply true: a scroll with nothing to scroll refuses the drag, so the
    /// gesture falls through to whatever is behind it. A list that fits on
    /// screen should not swallow a swipe meant for the page under it.
    fn should_accept_user_offset(&self, metrics: &ScrollMetrics) -> bool {
        if let Some(parent) = self.parent() {
            return parent.should_accept_user_offset(metrics);
        }
        metrics.max_scroll_extent != metrics.min_scroll_extent
    }

    /// Upstream's `allowUserScrolling`.
    fn allow_user_scrolling(&self) -> bool {
        self.parent()
            .map(|parent| parent.allow_user_scrolling())
            .unwrap_or(true)
    }

    /// Upstream's `allowImplicitScrolling`: whether the framework may scroll
    /// this on its own -- to bring a focused field into view, or for a screen
    /// reader moving through content.
    fn allow_implicit_scrolling(&self) -> bool {
        self.parent()
            .map(|parent| parent.allow_implicit_scrolling())
            .unwrap_or(true)
    }

    /// Upstream's `applyPhysicsToUserOffset`, whose base answer is the
    /// identity: a pixel dragged is a pixel moved.
    ///
    /// **The offset runs opposite to `pixels`.** Upstream's position applies
    /// it as `setPixels(pixels - offset)`, so a *positive* offset scrolls the
    /// content back towards the start. It is worth stating because every
    /// overscroll test reads backwards without it: past the end, a positive
    /// offset is the drag coming home.
    fn apply_physics_to_user_offset(&self, metrics: &ScrollMetrics, offset: f32) -> f32 {
        match self.parent() {
            Some(parent) => parent.apply_physics_to_user_offset(metrics, offset),
            None => offset,
        }
    }

    /// Upstream's `applyBoundaryConditions`: **how much of the proposed change
    /// to refuse**, not where to end up.
    ///
    /// Returning a delta rather than a position is what lets the caller tell
    /// the difference between "clamped" and "moved" -- the leftover is what
    /// becomes an overscroll notification and, on Android, the glow at the
    /// edge.
    fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, value: f32) -> f32 {
        match self.parent() {
            Some(parent) => parent.apply_boundary_conditions(metrics, value),
            None => 0.0,
        }
    }

    fn min_fling_velocity(&self) -> f32 {
        self.parent()
            .map(|parent| parent.min_fling_velocity())
            .unwrap_or(MIN_FLING_VELOCITY)
    }

    fn max_fling_velocity(&self) -> f32 {
        self.parent()
            .map(|parent| parent.max_fling_velocity())
            .unwrap_or(MAX_FLING_VELOCITY)
    }

    /// Upstream's `carriedMomentum`: how much of an existing fling survives
    /// into a new one. The base answer is none.
    fn carried_momentum(&self, existing_velocity: f32) -> f32 {
        match self.parent() {
            Some(parent) => parent.carried_momentum(existing_velocity),
            None => 0.0,
        }
    }

    /// Upstream's `dragStartDistanceMotionThreshold`, `None` by default.
    fn drag_start_distance_motion_threshold(&self) -> Option<f32> {
        self.parent()
            .and_then(|parent| parent.drag_start_distance_motion_threshold())
    }

    /// Upstream's `toleranceFor`, which scales with the device pixel ratio --
    /// a physical pixel is the smallest difference that can matter, so a
    /// denser screen deserves a tighter tolerance.
    fn tolerance_for(&self, device_pixel_ratio: f32) -> Tolerance {
        Tolerance {
            distance: 1.0 / (0.35 * device_pixel_ratio),
            time: 0.001,
            velocity: 1.0 / (0.050 * device_pixel_ratio),
        }
    }

    /// Upstream's `adjustPositionForNewDimensions`, whose base answer keeps
    /// the pixel offset it already had.
    fn adjust_position_for_new_dimensions(
        &self,
        old_position: &ScrollMetrics,
        new_position: &ScrollMetrics,
        is_scrolling: bool,
        velocity: f32,
    ) -> f32 {
        match self.parent() {
            Some(parent) => parent.adjust_position_for_new_dimensions(
                old_position,
                new_position,
                is_scrolling,
                velocity,
            ),
            None => new_position.pixels,
        }
    }
}

/// The base physics with no overrides, for composing against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BaseScrollPhysics;

impl ScrollPhysics for BaseScrollPhysics {}

/// Upstream `RangeMaintainingScrollPhysics`: keeps the reader where they were
/// when the content changes size underneath them.
///
/// This is the physics nobody notices until it is missing. A lazily-loaded
/// list that grows while the reader is halfway down must not move the words
/// they are reading, and a list that shrinks while they are overscrolled past
/// the end must not snap them somewhere arbitrary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RangeMaintainingScrollPhysics;

/// What [`RangeMaintainingScrollPhysics`] decided, for the sake of naming the
/// four cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeMaintainingDecision {
    pub maintain_overscroll: bool,
    pub enforce_boundary: bool,
}

impl RangeMaintainingScrollPhysics {
    /// The flag-setting half of upstream's
    /// `adjustPositionForNewDimensions`, which is four independent reasons to
    /// stand back.
    ///
    /// * **The position is animating.** Upstream: "don't try to adjust an
    ///   animating position, the jumping around would be distracting." Both
    ///   flags go.
    /// * **The extents did not change.** Then there is no new overscroll to
    ///   maintain and the question does not arise.
    /// * **The position was already changed by somebody else.** It may have
    ///   been moved *in expectation* of the new size, so re-deriving it would
    ///   fight whoever did that.
    /// * **The old position was already out of range.** Forcing the new one
    ///   into range would be tidying up something the reader is holding.
    ///
    /// The third case has an extra clause worth reading twice: the boundary is
    /// only relaxed when **all four extents are finite**. An infinite extent
    /// means a lazily-loaded list that does not yet know how long it is, and
    /// upstream's reasoning is the contrapositive -- if the boundaries were
    /// finite and the position still changed, somebody meant it.
    pub fn decide(
        old_position: &ScrollMetrics,
        new_position: &ScrollMetrics,
        velocity: f32,
    ) -> RangeMaintainingDecision {
        let mut maintain_overscroll = true;
        let mut enforce_boundary = true;
        if velocity != 0.0 {
            maintain_overscroll = false;
            enforce_boundary = false;
        }
        if old_position.min_scroll_extent == new_position.min_scroll_extent
            && old_position.max_scroll_extent == new_position.max_scroll_extent
        {
            maintain_overscroll = false;
        }
        if old_position.pixels != new_position.pixels {
            maintain_overscroll = false;
            if old_position.min_scroll_extent.is_finite()
                && old_position.max_scroll_extent.is_finite()
                && new_position.min_scroll_extent.is_finite()
                && new_position.max_scroll_extent.is_finite()
            {
                enforce_boundary = false;
            }
        }
        if old_position.pixels < old_position.min_scroll_extent
            || old_position.pixels > old_position.max_scroll_extent
        {
            enforce_boundary = false;
        }
        RangeMaintainingDecision {
            maintain_overscroll,
            enforce_boundary,
        }
    }
}

impl ScrollPhysics for RangeMaintainingScrollPhysics {
    fn adjust_position_for_new_dimensions(
        &self,
        old_position: &ScrollMetrics,
        new_position: &ScrollMetrics,
        _is_scrolling: bool,
        velocity: f32,
    ) -> f32 {
        let decision = Self::decide(old_position, new_position, velocity);
        if decision.maintain_overscroll {
            // Keep the same *amount* of overscroll, but only when the extents
            // shrank. Upstream's reason for that condition is the interesting
            // half: when content is **added**, holding the overscroll constant
            // would jump straight past all of it to the new maximum -- the
            // reader would never see what arrived.
            if old_position.pixels < old_position.min_scroll_extent
                && new_position.min_scroll_extent > old_position.min_scroll_extent
            {
                let old_delta = old_position.min_scroll_extent - old_position.pixels;
                return new_position.min_scroll_extent - old_delta;
            }
            if old_position.pixels > old_position.max_scroll_extent
                && new_position.max_scroll_extent < old_position.max_scroll_extent
            {
                let old_delta = old_position.pixels - old_position.max_scroll_extent;
                return new_position.max_scroll_extent + old_delta;
            }
        }
        let mut result = new_position.pixels;
        if decision.enforce_boundary {
            result = result.clamp(
                new_position.min_scroll_extent,
                new_position.max_scroll_extent,
            );
        }
        result
    }
}

/// Upstream `ClampingScrollPhysics`: Android's answer -- the content stops at
/// the edge and the leftover becomes a glow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClampingScrollPhysics;

impl ScrollPhysics for ClampingScrollPhysics {
    /// Upstream's four cases, and they are four rather than one clamp because
    /// the answer differs by **where the position already was**.
    ///
    /// If it was already at or past the edge, the whole proposed movement is
    /// refused. If it was inside and the proposal crosses out, only the part
    /// past the edge is refused -- the rest of the drag is honoured, so a
    /// flick that ends at the boundary still travels the distance it should.
    fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, value: f32) -> f32 {
        debug_assert!(
            value != metrics.pixels,
            "applyBoundaryConditions was called redundantly"
        );
        if value < metrics.pixels && metrics.pixels <= metrics.min_scroll_extent {
            // Underscroll: already at or past the start, moving further out.
            return value - metrics.pixels;
        }
        if metrics.max_scroll_extent <= metrics.pixels && metrics.pixels < value {
            // Overscroll: already at or past the end, moving further out.
            return value - metrics.pixels;
        }
        if value < metrics.min_scroll_extent && metrics.min_scroll_extent < metrics.pixels {
            // Was inside; this proposal crosses the start.
            return value - metrics.min_scroll_extent;
        }
        if metrics.pixels < metrics.max_scroll_extent && metrics.max_scroll_extent < value {
            // Was inside; this proposal crosses the end.
            return value - metrics.max_scroll_extent;
        }
        0.0
    }
}

/// Upstream `BouncingScrollPhysics`: iOS's answer -- the content stretches
/// past the edge and springs back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BouncingScrollPhysics {
    pub deceleration_rate: ScrollDecelerationRate,
}

impl BouncingScrollPhysics {
    pub fn new(deceleration_rate: ScrollDecelerationRate) -> BouncingScrollPhysics {
        BouncingScrollPhysics { deceleration_rate }
    }

    /// Upstream's `frictionFactor`.
    ///
    /// **Quadratic in how far past the edge the reader already is**, so the
    /// resistance grows faster than the distance. That is what makes an iOS
    /// overscroll feel like pulling against elastic rather than against a
    /// constant weight: the first fifty pixels are nearly free and the next
    /// fifty are not.
    pub fn friction_factor(&self, overscroll_fraction: f32) -> f32 {
        let base = match self.deceleration_rate {
            ScrollDecelerationRate::Fast => 0.26,
            ScrollDecelerationRate::Normal => 0.52,
        };
        (1.0 - overscroll_fraction).powi(2) * base
    }

    /// Upstream's `_applyFriction`.
    ///
    /// The part outside the boundary is charged friction; anything beyond that
    /// is free. So a drag that starts outside and carries the content **back
    /// in** pays only for the stretched portion, and moves normally once it is
    /// inside again.
    pub fn apply_friction(extent_outside: f32, abs_delta: f32, gamma: f32) -> f32 {
        debug_assert!(abs_delta > 0.0);
        let mut abs_delta = abs_delta;
        let mut total = 0.0;
        if extent_outside > 0.0 {
            let delta_to_limit = extent_outside / gamma;
            if abs_delta < delta_to_limit {
                return abs_delta * gamma;
            }
            total += extent_outside;
            abs_delta -= delta_to_limit;
        }
        total + abs_delta
    }
}

impl ScrollPhysics for BouncingScrollPhysics {
    /// Upstream's `applyPhysicsToUserOffset`.
    ///
    /// The `easing` flag is the asymmetry that makes the gesture feel right:
    /// **pulling further out is resisted more than letting it come back**.
    /// Remember the sign convention -- past the end, `easing` is an offset
    /// that is *positive*, because the offset runs opposite to `pixels`. The
    /// friction for an easing drag is computed at the position the drag will
    /// *end* at rather than where it started, so releasing tension is cheaper
    /// than adding it.
    ///
    /// And at the fast deceleration rate an easing drag pays **no friction at
    /// all** -- the content follows the finger home exactly.
    fn apply_physics_to_user_offset(&self, metrics: &ScrollMetrics, offset: f32) -> f32 {
        debug_assert!(offset != 0.0);
        debug_assert!(metrics.min_scroll_extent <= metrics.max_scroll_extent);
        if !metrics.out_of_range() {
            return offset;
        }
        let overscroll_past_start = (metrics.min_scroll_extent - metrics.pixels).max(0.0);
        let overscroll_past_end = (metrics.pixels - metrics.max_scroll_extent).max(0.0);
        let overscroll_past = overscroll_past_start.max(overscroll_past_end);
        let easing = (overscroll_past_start > 0.0 && offset < 0.0)
            || (overscroll_past_end > 0.0 && offset > 0.0);

        let friction = if easing {
            self.friction_factor((overscroll_past - offset.abs()) / metrics.viewport_dimension)
        } else {
            self.friction_factor(overscroll_past / metrics.viewport_dimension)
        };
        let direction = offset.signum();

        if easing && self.deceleration_rate == ScrollDecelerationRate::Fast {
            return direction * offset.abs();
        }
        direction * Self::apply_friction(overscroll_past, offset.abs(), friction)
    }

    /// Upstream returns **zero unconditionally**: nothing is ever refused,
    /// because going past the edge is the behaviour rather than an error to be
    /// clamped away.
    fn apply_boundary_conditions(&self, _metrics: &ScrollMetrics, _value: f32) -> f32 {
        0.0
    }

    /// Upstream doubles it, with its reason attached: this ballistic
    /// decelerates more slowly than the clamping one, so a fling needs to be
    /// more deliberate before it counts as one.
    fn min_fling_velocity(&self) -> f32 {
        MIN_FLING_VELOCITY * 2.0
    }

    fn max_fling_velocity(&self) -> f32 {
        match self.deceleration_rate {
            ScrollDecelerationRate::Fast => MAX_FLING_VELOCITY * 8.0,
            ScrollDecelerationRate::Normal => MAX_FLING_VELOCITY,
        }
    }

    /// Upstream's `carriedMomentum`, and it comes with its own methodology:
    /// superimpose Flutter's scroll view on the platform's, watch for the
    /// moment they stop overlapping, adjust the output at that speed, refit
    /// the power curve, repeat. The exponent 1.967 and the coefficient
    /// 0.000816 are the result of that loop rather than of any model.
    ///
    /// Note what it is **not** a function of: the velocity of the last fling.
    /// Upstream says so outright -- what matters is the speed the content
    /// still has.
    fn carried_momentum(&self, existing_velocity: f32) -> f32 {
        existing_velocity.signum() * (0.000_816 * existing_velocity.abs().powf(1.967)).min(40_000.0)
    }

    /// Upstream: "eyeballed from observation to counter the effect of an
    /// unintended scroll from the natural motion of lifting the finger."
    fn drag_start_distance_motion_threshold(&self) -> Option<f32> {
        Some(3.5)
    }
}

/// Upstream `AlwaysScrollableScrollPhysics`: one override, and it matters.
///
/// The base refuses a drag when there is nothing to scroll, which is right for
/// a list inside a page. It is wrong for a pull-to-refresh: the gesture has to
/// be accepted even though the list fits, because the gesture is what loads
/// the content that would make it not fit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AlwaysScrollableScrollPhysics;

impl ScrollPhysics for AlwaysScrollableScrollPhysics {
    fn should_accept_user_offset(&self, _metrics: &ScrollMetrics) -> bool {
        true
    }
}

/// Upstream `NeverScrollableScrollPhysics`: two overrides, and the second is
/// the one people forget.
///
/// Turning off user scrolling leaves the framework free to scroll the view
/// itself -- to reveal a focused field, or for a screen reader. A list that is
/// deliberately not scrollable because a parent scrolls it instead would then
/// scroll anyway, in the wrong axis, at the wrong moment. So implicit
/// scrolling goes too.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NeverScrollableScrollPhysics;

impl ScrollPhysics for NeverScrollableScrollPhysics {
    fn allow_user_scrolling(&self) -> bool {
        false
    }

    fn allow_implicit_scrolling(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(pixels: f32, min: f32, max: f32, viewport: f32) -> ScrollMetrics {
        ScrollMetrics {
            pixels,
            min_scroll_extent: min,
            max_scroll_extent: max,
            viewport_dimension: viewport,
        }
    }

    // -- The base ----------------------------------------------------------

    #[test]
    fn a_scroll_with_nothing_to_scroll_lets_the_gesture_through() {
        // A list that fits on screen should not swallow a swipe meant for the
        // page under it.
        let physics = BaseScrollPhysics;
        assert!(!physics.should_accept_user_offset(&metrics(0.0, 0.0, 0.0, 500.0)));
        assert!(physics.should_accept_user_offset(&metrics(0.0, 0.0, 900.0, 500.0)));
    }

    #[test]
    fn always_scrollable_accepts_the_drag_that_would_load_the_content() {
        // A pull-to-refresh gesture has to be accepted even though the list
        // fits, because the gesture is what makes it not fit.
        let physics = AlwaysScrollableScrollPhysics;
        assert!(physics.should_accept_user_offset(&metrics(0.0, 0.0, 0.0, 500.0)));
    }

    #[test]
    fn never_scrollable_turns_off_the_frameworks_scrolling_too() {
        // A list deliberately not scrollable because a parent scrolls it
        // instead would otherwise scroll anyway, in the wrong axis, when a
        // field took focus.
        let physics = NeverScrollableScrollPhysics;
        assert!(!physics.allow_user_scrolling());
        assert!(!physics.allow_implicit_scrolling());

        assert!(BaseScrollPhysics.allow_user_scrolling());
        assert!(BaseScrollPhysics.allow_implicit_scrolling());
    }

    #[test]
    fn a_denser_screen_gets_a_tighter_tolerance() {
        // A physical pixel is the smallest difference that can matter.
        let one = BaseScrollPhysics.tolerance_for(1.0);
        let three = BaseScrollPhysics.tolerance_for(3.0);
        assert!(three.distance < one.distance);
        assert!(three.velocity < one.velocity);
    }

    // -- Clamping ----------------------------------------------------------

    #[test]
    fn a_drag_from_inside_that_crosses_the_edge_is_only_refused_past_it() {
        // The rest of the drag is honoured, so a flick that ends at the
        // boundary still travels the distance it should.
        let physics = ClampingScrollPhysics;
        let at = metrics(90.0, 0.0, 100.0, 500.0);
        assert_eq!(
            physics.apply_boundary_conditions(&at, 130.0),
            30.0,
            "only the 30 past the end"
        );
    }

    #[test]
    fn a_drag_that_was_already_at_the_edge_is_refused_entirely() {
        let physics = ClampingScrollPhysics;
        let at_end = metrics(100.0, 0.0, 100.0, 500.0);
        assert_eq!(physics.apply_boundary_conditions(&at_end, 140.0), 40.0);

        let at_start = metrics(0.0, 0.0, 100.0, 500.0);
        assert_eq!(physics.apply_boundary_conditions(&at_start, -40.0), -40.0);
    }

    #[test]
    fn a_drag_that_stays_inside_is_not_refused_at_all() {
        let physics = ClampingScrollPhysics;
        let inside = metrics(50.0, 0.0, 100.0, 500.0);
        assert_eq!(physics.apply_boundary_conditions(&inside, 70.0), 0.0);
    }

    #[test]
    fn coming_back_from_out_of_range_is_never_refused() {
        // Or the reader could not undo their own overscroll.
        let physics = ClampingScrollPhysics;
        let past_end = metrics(120.0, 0.0, 100.0, 500.0);
        assert_eq!(physics.apply_boundary_conditions(&past_end, 110.0), 0.0);
    }

    // -- Bouncing ----------------------------------------------------------

    #[test]
    fn overscroll_resistance_grows_faster_than_the_distance() {
        // Which is what makes it feel like elastic rather than a weight: the
        // first fifty pixels are nearly free and the next fifty are not.
        let physics = BouncingScrollPhysics::default();
        let near = physics.friction_factor(0.1);
        let far = physics.friction_factor(0.5);
        assert!(far < near);
        assert!(
            (near - far) > (physics.friction_factor(0.5) - physics.friction_factor(0.9)),
            "and the drop is steepest early, because it is quadratic"
        );
        assert_eq!(physics.friction_factor(0.0), 0.52);
    }

    #[test]
    fn the_fast_rate_resists_twice_as_hard() {
        let normal = BouncingScrollPhysics::new(ScrollDecelerationRate::Normal);
        let fast = BouncingScrollPhysics::new(ScrollDecelerationRate::Fast);
        assert_eq!(normal.friction_factor(0.0), 0.52);
        assert_eq!(fast.friction_factor(0.0), 0.26);
    }

    #[test]
    fn pulling_further_out_is_resisted_more_than_letting_it_come_back() {
        // The asymmetry that makes the gesture feel right.
        let physics = BouncingScrollPhysics::default();
        let past_end = metrics(150.0, 0.0, 100.0, 500.0);

        // The offset runs opposite to pixels, so past the end a positive
        // offset is the drag coming home and a negative one is pulling out.
        let pulling = physics.apply_physics_to_user_offset(&past_end, -20.0);
        let easing = physics.apply_physics_to_user_offset(&past_end, 20.0);
        assert!(
            easing.abs() > pulling.abs(),
            "coming home moves further per pixel of finger: {easing} against {pulling}"
        );
    }

    #[test]
    fn a_drag_inside_the_range_is_not_touched() {
        let physics = BouncingScrollPhysics::default();
        let inside = metrics(50.0, 0.0, 100.0, 500.0);
        assert_eq!(physics.apply_physics_to_user_offset(&inside, 20.0), 20.0);
    }

    #[test]
    fn at_the_fast_rate_the_content_follows_the_finger_home_exactly() {
        let physics = BouncingScrollPhysics::new(ScrollDecelerationRate::Fast);
        let past_end = metrics(150.0, 0.0, 100.0, 500.0);
        assert_eq!(
            physics.apply_physics_to_user_offset(&past_end, 20.0),
            20.0,
            "no friction at all on the way back"
        );
        assert!(
            physics.apply_physics_to_user_offset(&past_end, -20.0).abs() < 20.0,
            "where pulling further out still pays"
        );
    }

    #[test]
    fn friction_is_charged_only_on_the_stretched_part() {
        // A drag that carries the content back inside moves normally once it
        // is in.
        let charged = BouncingScrollPhysics::apply_friction(10.0, 5.0, 0.5);
        assert_eq!(charged, 2.5, "entirely within the stretched part");

        let crossing = BouncingScrollPhysics::apply_friction(10.0, 30.0, 0.5);
        assert_eq!(crossing, 20.0, "10 outside, then 10 more at full rate");
    }

    #[test]
    fn nothing_is_ever_refused_at_the_boundary() {
        // Going past the edge is the behaviour, not an error to clamp away.
        let physics = BouncingScrollPhysics::default();
        assert_eq!(
            physics.apply_boundary_conditions(&metrics(100.0, 0.0, 100.0, 500.0), 400.0),
            0.0
        );
    }

    #[test]
    fn a_bouncing_fling_has_to_be_more_deliberate_to_count() {
        // Its ballistic decelerates more slowly than the clamping one.
        assert_eq!(
            BouncingScrollPhysics::default().min_fling_velocity(),
            MIN_FLING_VELOCITY * 2.0
        );
        assert_eq!(BaseScrollPhysics.min_fling_velocity(), MIN_FLING_VELOCITY);
    }

    #[test]
    fn momentum_carries_forward_and_is_capped() {
        // Repeated flings build speed the way iOS does, but not without limit.
        let physics = BouncingScrollPhysics::default();
        assert_eq!(physics.carried_momentum(0.0), 0.0);
        assert!(physics.carried_momentum(1000.0) > physics.carried_momentum(500.0));
        assert!(physics.carried_momentum(1e9) <= 40_000.0);
        assert!(
            physics.carried_momentum(-1000.0) < 0.0,
            "and it keeps its sign"
        );

        assert_eq!(
            BaseScrollPhysics.carried_momentum(1000.0),
            0.0,
            "where the base carries none"
        );
    }

    // -- Range maintaining -------------------------------------------------

    #[test]
    fn an_animating_position_is_left_alone_entirely() {
        // Upstream: the jumping around would be distracting.
        let old = metrics(-20.0, 0.0, 100.0, 500.0);
        let new = metrics(-20.0, 0.0, 200.0, 500.0);
        let decision = RangeMaintainingScrollPhysics::decide(&old, &new, 300.0);
        assert!(!decision.maintain_overscroll);
        assert!(!decision.enforce_boundary);
    }

    #[test]
    fn unchanged_extents_mean_there_is_no_overscroll_to_maintain() {
        let old = metrics(-20.0, 0.0, 100.0, 500.0);
        let new = metrics(-20.0, 0.0, 100.0, 500.0);
        let decision = RangeMaintainingScrollPhysics::decide(&old, &new, 0.0);
        assert!(!decision.maintain_overscroll);
    }

    #[test]
    fn a_growing_list_does_not_skip_past_what_arrived() {
        // Holding the overscroll constant when content is added would jump
        // straight to the new maximum and the reader would never see it.
        let physics = RangeMaintainingScrollPhysics;
        let old = metrics(120.0, 0.0, 100.0, 500.0);
        let new = metrics(120.0, 0.0, 300.0, 500.0);
        assert_eq!(
            physics.adjust_position_for_new_dimensions(&old, &new, false, 0.0),
            120.0,
            "it stays where it was rather than following the new end"
        );
    }

    #[test]
    fn a_shrinking_list_keeps_the_same_amount_of_overscroll() {
        // The reader is holding it stretched; the stretch should stay the
        // same length even though the content got shorter.
        let physics = RangeMaintainingScrollPhysics;
        let old = metrics(120.0, 0.0, 100.0, 500.0);
        let new = metrics(120.0, 0.0, 60.0, 500.0);
        assert_eq!(
            physics.adjust_position_for_new_dimensions(&old, &new, false, 0.0),
            80.0,
            "20 past the new end, as it was 20 past the old one"
        );
    }

    #[test]
    fn an_infinite_extent_is_the_case_where_the_boundary_stays_enforced() {
        // A lazily-loaded list does not yet know how long it is. Upstream's
        // reasoning is the contrapositive: if the boundaries were finite and
        // the position still changed, somebody meant it.
        let lazy_old = metrics(50.0, 0.0, f32::INFINITY, 500.0);
        let lazy_new = metrics(80.0, 0.0, 300.0, 500.0);
        assert!(RangeMaintainingScrollPhysics::decide(&lazy_old, &lazy_new, 0.0).enforce_boundary);

        let finite_old = metrics(50.0, 0.0, 200.0, 500.0);
        let finite_new = metrics(80.0, 0.0, 300.0, 500.0);
        assert!(
            !RangeMaintainingScrollPhysics::decide(&finite_old, &finite_new, 0.0).enforce_boundary,
            "both finite and the position moved: assume it was intentional"
        );
    }

    #[test]
    fn a_position_already_out_of_range_is_not_tidied_up() {
        // It is something the reader is holding.
        let old = metrics(-30.0, 0.0, 100.0, 500.0);
        let new = metrics(-30.0, 0.0, 100.0, 500.0);
        assert!(!RangeMaintainingScrollPhysics::decide(&old, &new, 0.0).enforce_boundary);
    }

    #[test]
    fn an_ordinary_shrink_puts_the_reader_back_inside() {
        let physics = RangeMaintainingScrollPhysics;
        let old = metrics(250.0, 0.0, 300.0, 500.0);
        let new = metrics(250.0, 0.0, 100.0, 500.0);
        assert_eq!(
            physics.adjust_position_for_new_dimensions(&old, &new, false, 0.0),
            100.0,
            "clamped to the new end"
        );
    }
}
