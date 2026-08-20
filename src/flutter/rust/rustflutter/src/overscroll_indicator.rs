//! A port of `widgets/overscroll_indicator.dart`.
//!
//! What a scroll view does when it runs out of content. Two answers ship, and
//! they are answers to the same question rather than variants of one design:
//! the glow paints something *over* the content, and the stretch deforms the
//! content itself. `ScrollBehavior` picks between them by platform, and gives a
//! glow to exactly the platforms whose physics do not already stretch.
//!
//! Both are faithful ports of Android's originals, and both carry an admitted
//! mystery -- a constant that had to be there for the result to match the
//! platform, with upstream saying in as many words that it does not know why.

use crate::animation::Curve;
use crate::engine::Color;
use crate::physics::{Simulation, SpringDescription, SpringSimulation};
use crate::render::{Axis, AxisDirection};

/// Upstream `OverscrollIndicatorNotification`.
///
/// Two of its three fields are mutable, which is unusual for a notification and
/// is the point of the class: it travels up the tree and is *written into* on
/// the way. An ancestor can veto the indication outright, or move where it is
/// drawn -- an app bar sets `paint_offset` so the glow appears below it rather
/// than underneath it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverscrollIndicatorNotification {
    /// Whether the indication is for the leading (top or left) edge.
    pub leading: bool,
    /// How far from the edge a [`GlowingOverscrollIndicator`] should draw. A
    /// negative value is not useful, since the glow would be clipped.
    ///
    /// This has no effect on a [`StretchingOverscrollIndicator`]: there is
    /// nothing to offset when the content itself is what moves.
    pub paint_offset: f32,
    accepted: bool,
}

impl OverscrollIndicatorNotification {
    pub fn new(leading: bool) -> OverscrollIndicatorNotification {
        OverscrollIndicatorNotification {
            leading,
            paint_offset: 0.0,
            accepted: true,
        }
    }

    /// Whether this overscroll will be indicated at all. Defaults to true, so
    /// an ancestor that does not care needs to do nothing.
    pub fn accepted(&self) -> bool {
        self.accepted
    }

    /// Upstream `disallowIndicator`. There is no matching "allow": the veto is
    /// one-way, so one ancestor's objection cannot be overruled by another.
    pub fn disallow_indicator(&mut self) {
        self.accepted = false;
    }

    /// Upstream `debugFillDescription`.
    pub fn describe_side(&self) -> &'static str {
        if self.leading {
            "leading edge"
        } else {
            "trailing edge"
        }
    }
}

/// Upstream `_GlowState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlowState {
    #[default]
    Idle,
    /// A fling arrived at the edge and its energy is being absorbed.
    Absorb,
    /// A finger is dragging past the edge.
    Pull,
    /// The glow is fading back out.
    Recede,
}

/// A `begin`/`end` pair, as upstream's mutable `Tween`s are used here: both
/// ends are rewritten on every event so the next phase starts from wherever the
/// last one had got to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Span {
    begin: f32,
    end: f32,
}

impl Span {
    fn at(&self, t: f32) -> f32 {
        self.begin + (self.end - self.begin) * t
    }
}

/// Upstream `_GlowController`: the substance of [`GlowingOverscrollIndicator`].
#[derive(Clone, Debug, PartialEq)]
pub struct GlowController {
    pub color: Color,
    pub axis: Axis,
    state: GlowState,
    opacity: Span,
    size: Span,
    /// The animation controller's own 0..1, driven here by hand.
    t: f32,
    duration_ms: f32,
    pull_distance: f32,
    displacement: f32,
    displacement_target: f32,
    pull_recede_pending: bool,
    notifications: usize,
}

impl GlowController {
    pub const RECEDE_TIME_MS: f32 = 600.0;
    pub const PULL_TIME_MS: f32 = 167.0;
    pub const PULL_HOLD_TIME_MS: f32 = 167.0;
    pub const PULL_DECAY_TIME_MS: f32 = 2000.0;

    pub const MAX_OPACITY: f32 = 0.5;
    pub const PULL_OPACITY_GLOW_FACTOR: f32 = 0.8;
    pub const VELOCITY_GLOW_FACTOR: f32 = 0.00006;
    /// `(3/4)(2 - sqrt(3))`, the arc's width-to-height ratio.
    pub const WIDTH_TO_HEIGHT_FACTOR: f32 = 0.75 * (2.0 - 1.732_050_8);

    /// Absorbed velocities are clamped into this range, in logical pixels per
    /// second: below the floor there would be nothing to see, and above the
    /// ceiling the glow would be the same anyway.
    pub const MIN_VELOCITY: f32 = 100.0;
    pub const MAX_VELOCITY: f32 = 10000.0;

    pub fn new(color: Color, axis: Axis) -> GlowController {
        GlowController {
            color,
            axis,
            state: GlowState::Idle,
            opacity: Span::default(),
            size: Span::default(),
            t: 0.0,
            duration_ms: 0.0,
            pull_distance: 0.0,
            displacement: 0.5,
            displacement_target: 0.5,
            pull_recede_pending: false,
            notifications: 0,
        }
    }

    pub fn state(&self) -> GlowState {
        self.state
    }

    pub fn duration_ms(&self) -> f32 {
        self.duration_ms
    }

    pub fn notification_count(&self) -> usize {
        self.notifications
    }

    /// Where along the cross axis the glow is centred, as a fraction. It chases
    /// the finger rather than jumping to it.
    pub fn displacement(&self) -> f32 {
        self.displacement
    }

    pub fn displacement_target(&self) -> f32 {
        self.displacement_target
    }

    /// Whether a pull is holding open a timer that will start the slow decay.
    /// Letting go is not required: a finger that simply stops moving still
    /// fades the glow out, after `PULL_HOLD_TIME_MS`.
    pub fn is_pull_recede_pending(&self) -> bool {
        self.pull_recede_pending
    }

    /// Upstream drives these through a `Curves.decelerate` on the controller.
    fn curved(&self) -> f32 {
        Curve::Decelerate.transform(self.t)
    }

    pub fn glow_opacity(&self) -> f32 {
        self.opacity.at(self.curved())
    }

    pub fn glow_size(&self) -> f32 {
        self.size.at(self.curved())
    }

    /// Upstream `absorbImpact`: a fling that hit the edge.
    ///
    /// The glow it produces is a function of **velocity** -- how hard the
    /// content arrived. Compare [`GlowController::pull`], which is a function
    /// of distance.
    pub fn absorb_impact(&mut self, velocity: f32) {
        debug_assert!(velocity >= 0.0, "an absorbed velocity is a speed");
        self.pull_recede_pending = false;
        let velocity = velocity.clamp(GlowController::MIN_VELOCITY, GlowController::MAX_VELOCITY);

        // A second impact does not restart from nothing: only an idle glow
        // begins at the fixed 0.3.
        self.opacity.begin = if self.state == GlowState::Idle {
            0.3
        } else {
            self.glow_opacity()
        };
        self.opacity.end = (velocity * GlowController::VELOCITY_GLOW_FACTOR)
            .clamp(self.opacity.begin, GlowController::MAX_OPACITY);
        self.size.begin = self.glow_size();
        self.size.end = (0.025 + 7.5e-7 * velocity * velocity).min(1.0);
        self.duration_ms = (0.15 + velocity * 0.02).round();
        self.t = 0.0;
        // The glow from an impact is always centred; only a finger has a
        // position along the cross axis.
        self.displacement = 0.5;
        self.state = GlowState::Absorb;
    }

    /// Upstream `pull`: a finger dragging past the edge.
    ///
    /// `overscroll` is positive whichever edge it is, `extent` is the viewport
    /// along the main axis, and `cross_axis_offset` / `cross_extent` say where
    /// across the viewport the finger is.
    pub fn pull(
        &mut self,
        overscroll: f32,
        extent: f32,
        cross_axis_offset: f32,
        cross_extent: f32,
    ) {
        self.pull_recede_pending = false;
        // Upstream's comment on the 200 is worth keeping verbatim: "This factor
        // is magic. Not clear why we need it to match Android."
        self.pull_distance += overscroll / 200.0;

        self.opacity.begin = self.glow_opacity();
        self.opacity.end = (self.glow_opacity()
            + overscroll / extent * GlowController::PULL_OPACITY_GLOW_FACTOR)
            .min(GlowController::MAX_OPACITY);

        let height = extent.min(cross_extent * GlowController::WIDTH_TO_HEIGHT_FACTOR);
        self.size.begin = self.glow_size();
        // The max is not a safety net: a glow never shrinks while it is being
        // pulled. Only a recede takes it back down.
        self.size.end =
            (1.0 - 1.0 / (0.7 * (self.pull_distance * height).sqrt())).max(self.glow_size());

        self.displacement_target = cross_axis_offset / cross_extent;
        self.duration_ms = GlowController::PULL_TIME_MS;
        if self.state != GlowState::Pull {
            self.t = 0.0;
            self.state = GlowState::Pull;
        } else if self.t >= 1.0 {
            // A drag that keeps going after the 167ms animation finished still
            // has to repaint, and there is no animation left to do it.
            self.notifications += 1;
        }
        self.pull_recede_pending = true;
    }

    /// Upstream `scrollEnd`, which acts only on a pull -- an absorb is already
    /// on its way to receding by itself.
    pub fn scroll_end(&mut self) {
        if self.state == GlowState::Pull {
            self.recede(GlowController::RECEDE_TIME_MS);
        }
    }

    /// The pull-hold timer firing: the finger stopped moving, so the glow
    /// starts its long decay.
    pub fn pull_hold_elapsed(&mut self) {
        if !self.pull_recede_pending {
            return;
        }
        self.pull_recede_pending = false;
        self.recede(GlowController::PULL_DECAY_TIME_MS);
    }

    /// Upstream `_recede`.
    pub fn recede(&mut self, duration_ms: f32) {
        if self.state == GlowState::Recede || self.state == GlowState::Idle {
            return;
        }
        self.pull_recede_pending = false;
        self.opacity.begin = self.glow_opacity();
        self.opacity.end = 0.0;
        self.size.begin = self.glow_size();
        self.size.end = 0.0;
        self.duration_ms = duration_ms;
        self.t = 0.0;
        self.state = GlowState::Recede;
    }

    /// Drives the controller. `t` runs 0..1 over `duration_ms`.
    pub fn set_t(&mut self, t: f32) {
        self.t = t.clamp(0.0, 1.0);
        self.notifications += 1;
        if self.t >= 1.0 {
            self.change_phase();
        }
    }

    /// Upstream `_changePhase`, which runs only when the animation completes.
    fn change_phase(&mut self) {
        match self.state {
            GlowState::Absorb => self.recede(GlowController::RECEDE_TIME_MS),
            GlowState::Recede => {
                self.state = GlowState::Idle;
                // The pull distance is accumulated across a whole gesture and
                // only reset once the glow is fully gone -- so a reader who
                // lets go and grabs again mid-fade continues from where they
                // were rather than starting over.
                self.pull_distance = 0.0;
            }
            GlowState::Pull | GlowState::Idle => {}
        }
    }

    pub fn pull_distance(&self) -> f32 {
        self.pull_distance
    }

    /// The cross-axis chase. The half-life is one frame at sixty hertz, so the
    /// glow closes half the remaining distance to the finger every frame --
    /// which is fast enough to look attached to it and slow enough not to snap.
    pub fn tick_displacement(&mut self, elapsed_ms: f32) {
        let half_time_ms = 1000.0 / 60.0;
        self.displacement = self.displacement_target
            - (self.displacement_target - self.displacement) * (-elapsed_ms / half_time_ms).exp2();
        self.notifications += 1;
    }

    /// Whether the chase has arrived, at which point upstream stops the ticker.
    pub fn displacement_settled(&self) -> bool {
        (self.displacement_target - self.displacement).abs() < 1e-3
    }

    /// Upstream `paint`, as its geometry.
    ///
    /// The arc is a **circle** of radius one and a half times the viewport's
    /// width, squashed in Y by the glow's size and clipped to a rectangle only
    /// as tall as the arc. A circle that much wider than the viewport presents
    /// only its shallow top -- which is the shape Android's glow has.
    ///
    /// Returns `None` at zero opacity: upstream returns before painting rather
    /// than drawing something invisible.
    pub fn paint_geometry(&self, width: f32, height: f32) -> Option<GlowGeometry> {
        if self.glow_opacity() == 0.0 {
            return None;
        }
        let base_glow_scale = if width > height { height / width } else { 1.0 };
        let arc_height = height.min(width * GlowController::WIDTH_TO_HEIGHT_FACTOR);
        Some(GlowGeometry {
            radius: width * 3.0 / 2.0,
            arc_height,
            scale_y: self.glow_size() * base_glow_scale,
            center_x: (width / 2.0) * (0.5 + self.displacement),
            center_y: arc_height - width * 3.0 / 2.0,
            opacity: self.glow_opacity(),
        })
    }
}

/// What [`GlowController::paint_geometry`] works out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlowGeometry {
    pub radius: f32,
    pub arc_height: f32,
    pub scale_y: f32,
    pub center_x: f32,
    pub center_y: f32,
    pub opacity: f32,
}

/// Upstream `GlowingOverscrollIndicator`.
#[derive(Clone, Debug, PartialEq)]
pub struct GlowingOverscrollIndicator {
    pub show_leading: bool,
    pub show_trailing: bool,
    pub axis_direction: AxisDirection,
    pub color: Color,
    pub child: Option<u64>,
}

impl GlowingOverscrollIndicator {
    pub fn new(axis_direction: AxisDirection, color: Color) -> GlowingOverscrollIndicator {
        GlowingOverscrollIndicator {
            show_leading: true,
            show_trailing: true,
            axis_direction,
            color,
            child: None,
        }
    }

    pub fn with_edges(mut self, leading: bool, trailing: bool) -> Self {
        self.show_leading = leading;
        self.show_trailing = trailing;
        self
    }

    pub fn axis(&self) -> Axis {
        crate::render::axis_direction_to_axis(self.axis_direction)
    }

    /// Whether an overscroll at this edge is indicated at all, given both the
    /// widget's own configuration and any ancestor's veto.
    pub fn shows(&self, notification: &OverscrollIndicatorNotification) -> bool {
        notification.accepted()
            && if notification.leading {
                self.show_leading
            } else {
                self.show_trailing
            }
    }

    /// Upstream's `debugFillProperties` description of the two flags.
    pub fn describe_edges(&self) -> &'static str {
        match (self.show_leading, self.show_trailing) {
            (true, true) => "both sides",
            (true, false) => "leading side only",
            (false, true) => "trailing side only",
            (false, false) => "neither side (!)",
        }
    }
}

/// Upstream `_StretchController`: the substance of
/// [`StretchingOverscrollIndicator`].
///
/// Where the glow paints an arc whose size it computes directly, the stretch
/// runs a real spring -- Android's `EdgeEffect` constants, ported as they are.
#[derive(Debug)]
pub struct StretchController {
    overscroll: f32,
    /// Set when a pull interrupts a running animation, and added to what the
    /// pull computes, so the stretch does not jump back to zero and out again.
    interrupted_overscroll: f32,
    simulation: Option<SpringSimulation>,
    time: f32,
    notifications: usize,
}

impl StretchController {
    pub const MIN_OVERSCROLL: f32 = -1.0;
    pub const MAX_OVERSCROLL: f32 = 1.0;

    pub const EXPONENTIAL_SCALAR: f32 = std::f32::consts::E / 0.33;
    pub const STRETCH_INTENSITY: f32 = 0.016;

    pub const FLING_VELOCITY_FRICTION: f32 = 1.0 / 6000.0;
    pub const ABSORB_IMPACT_VELOCITY_FRICTION: f32 = 1.0 / 3000.0;
    pub const MAX_FLING_VELOCITY: f32 = 0.5;
    pub const MAX_ABSORB_IMPACT_VELOCITY: f32 = 1.25;

    /// Ported directly from Android's `EdgeEffect.java`.
    pub const NATURAL_FREQUENCY: f32 = 24.657;
    pub const DAMPING_RATIO: f32 = 0.98;

    /// Upstream's own account of this constant is the interesting part: using
    /// Android's numbers as they stand produced an animation "noticeably faster
    /// than the native Android behavior", the reason "is unknown", and 0.8 was
    /// arrived at by eyeballing it against the platform.
    ///
    /// It is applied to time. Upstream notes the mathematically equivalent
    /// alternative -- scaling the natural frequency and the initial velocity by
    /// the same factor -- and in fact does exactly that, squaring it into the
    /// stiffness and multiplying it into the velocity.
    pub const TIME_CORRECTION_FACTOR: f32 = 0.8;

    pub const STIFFNESS: f32 =
        StretchController::NATURAL_FREQUENCY * StretchController::NATURAL_FREQUENCY;

    pub fn spring() -> SpringDescription {
        SpringDescription::with_damping_ratio(
            1.0,
            StretchController::STIFFNESS
                * StretchController::TIME_CORRECTION_FACTOR
                * StretchController::TIME_CORRECTION_FACTOR,
            StretchController::DAMPING_RATIO,
        )
    }

    pub fn new() -> StretchController {
        StretchController {
            overscroll: 0.0,
            interrupted_overscroll: 0.0,
            simulation: None,
            time: 0.0,
            notifications: 0,
        }
    }

    /// How far the content is stretched, in the range -1..1.
    pub fn overscroll(&self) -> f32 {
        self.overscroll
    }

    pub fn set_overscroll(&mut self, value: f32) {
        let clamped = value.clamp(
            StretchController::MIN_OVERSCROLL,
            StretchController::MAX_OVERSCROLL,
        );
        if clamped != self.overscroll {
            self.notifications += 1;
        }
        self.overscroll = clamped;
    }

    pub fn is_animating(&self) -> bool {
        self.simulation.is_some()
    }

    pub fn notification_count(&self) -> usize {
        self.notifications
    }

    pub fn interrupted_overscroll(&self) -> f32 {
        self.interrupted_overscroll
    }

    fn stretch_simulation(&self, velocity: f32) -> SpringSimulation {
        SpringSimulation::new(
            StretchController::spring(),
            self.overscroll,
            0.0,
            velocity * StretchController::TIME_CORRECTION_FACTOR,
        )
    }

    /// Upstream `absorbImpact`: a fling arriving at the edge.
    pub fn absorb_impact(&mut self, velocity: f32) {
        if velocity == 0.0 {
            return;
        }
        let scaled = (velocity * StretchController::ABSORB_IMPACT_VELOCITY_FRICTION).clamp(
            -StretchController::MAX_ABSORB_IMPACT_VELOCITY,
            StretchController::MAX_ABSORB_IMPACT_VELOCITY,
        );
        self.animate(self.stretch_simulation(scaled));
    }

    /// Upstream `scrollEnd`: the finger left. The velocity is **negated** here
    /// and not in `absorb_impact`, because these are opposite events -- one is
    /// content arriving at the edge, the other is a hand leaving it.
    pub fn scroll_end(&mut self, velocity: f32) {
        if velocity == 0.0 && self.overscroll == 0.0 {
            return;
        }
        let scaled = (-(velocity * StretchController::FLING_VELOCITY_FRICTION)).clamp(
            -StretchController::MAX_FLING_VELOCITY,
            StretchController::MAX_FLING_VELOCITY,
        );
        // A spring already running is left alone: it is already going home.
        if self.simulation.is_none() {
            self.animate(self.stretch_simulation(scaled));
        }
    }

    pub fn animate(&mut self, simulation: SpringSimulation) {
        self.simulation = Some(simulation);
        self.time = 0.0;
    }

    /// Advances a running animation. Returns whether one is still going.
    pub fn tick(&mut self, seconds: f32) -> bool {
        let Some(simulation) = &self.simulation else {
            return false;
        };
        self.time += seconds;
        let value = simulation.x(self.time);
        let done = simulation.is_done(self.time);
        self.set_overscroll(value);
        if done {
            // Upstream guards this cleanup on the controller still being the
            // active one, because a later pull may have replaced it -- a stale
            // completion must not reach into shared state.
            self.simulation = None;
            self.set_overscroll(0.0);
            self.interrupted_overscroll = 0.0;
            return false;
        }
        true
    }

    /// Upstream `pull`. `normalized_overscroll` is the overscroll in pixels
    /// divided by the viewport's main-axis extent, and keeps its sign.
    ///
    /// The intensity is **linear plus exponential**, and the two halves take
    /// turns. At the start the exponential term dominates -- its slope there is
    /// `EXPONENTIAL_SCALAR` times the linear one, so about nine times as much
    /// movement per pixel -- and then it saturates at a constant, leaving only
    /// the slow linear term still growing. A drag that has gone a long way is
    /// barely moving the edge any further, which is what resistance feels
    /// like.
    pub fn pull(&mut self, normalized_overscroll: f32) {
        if self.simulation.is_some() {
            // Capture where the animation had got to, so the pull continues
            // from there rather than from zero.
            self.interrupted_overscroll = self.overscroll;
            self.simulation = None;
        }

        let distance = normalized_overscroll.abs();
        let linear = StretchController::STRETCH_INTENSITY * distance;
        let exponential = StretchController::STRETCH_INTENSITY
            * (1.0 - (-distance * StretchController::EXPONENTIAL_SCALAR).exp());
        let sign = if normalized_overscroll == 0.0 {
            0.0
        } else {
            normalized_overscroll.signum()
        };
        self.set_overscroll(sign * (linear + exponential) + self.interrupted_overscroll);
    }
}

impl Default for StretchController {
    fn default() -> Self {
        StretchController::new()
    }
}

/// Upstream `StretchingOverscrollIndicator`.
#[derive(Clone, Debug, PartialEq)]
pub struct StretchingOverscrollIndicator {
    pub axis_direction: AxisDirection,
    /// Defaults to hard-edge clipping: the stretch moves content past the
    /// viewport's bounds, and without a clip it would draw outside them.
    pub clip_behavior_is_none: bool,
    pub child: Option<u64>,
}

impl StretchingOverscrollIndicator {
    pub fn new(axis_direction: AxisDirection) -> StretchingOverscrollIndicator {
        StretchingOverscrollIndicator {
            axis_direction,
            clip_behavior_is_none: false,
            child: None,
        }
    }

    pub fn axis(&self) -> Axis {
        crate::render::axis_direction_to_axis(self.axis_direction)
    }

    /// What upstream hands `StretchEffect`. The controller's overscroll is
    /// negated, and negated again for a reversed axis -- so the same physical
    /// gesture stretches the same way whichever direction the list runs.
    pub fn stretch_strength(&self, overscroll: f32) -> f32 {
        let strength = -overscroll;
        if matches!(self.axis_direction, AxisDirection::Up | AxisDirection::Left) {
            -strength
        } else {
            strength
        }
    }

    /// Upstream clips only when there is a stretch **and** the viewport is
    /// smaller than the screen along the main axis, with the reason written
    /// down: if the viewport takes up the whole screen, there is nowhere for
    /// the overflow to be seen anyway, and a clip is a layer that costs
    /// something.
    pub fn clips(&self, stretch: f32, viewport_dimension: f32, screen_dimension: f32) -> bool {
        !self.clip_behavior_is_none && stretch != 0.0 && viewport_dimension != screen_dimension
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUE: Color = Color(0xFF00_00FF);

    fn glow() -> GlowController {
        GlowController::new(BLUE, Axis::Vertical)
    }

    /// Runs a controller's animation to its end.
    fn finish(controller: &mut GlowController) {
        controller.set_t(1.0);
    }

    // -- The notification ------------------------------------------------------

    #[test]
    fn an_ancestor_writes_into_the_notification_as_it_passes() {
        // Which is what the mutable fields are for: this is the only channel
        // back from an ancestor to the widget that sent it.
        let mut notification = OverscrollIndicatorNotification::new(true);
        assert!(notification.accepted(), "indicated unless somebody objects");
        assert_eq!(notification.paint_offset, 0.0);

        notification.paint_offset = 56.0;
        notification.disallow_indicator();
        assert!(!notification.accepted());
        assert_eq!(notification.describe_side(), "leading edge");
        assert_eq!(
            OverscrollIndicatorNotification::new(false).describe_side(),
            "trailing edge"
        );
    }

    #[test]
    fn an_indicator_needs_both_its_own_permission_and_nobodys_objection() {
        let indicator = GlowingOverscrollIndicator::new(AxisDirection::Down, BLUE);
        let leading = OverscrollIndicatorNotification::new(true);
        assert!(indicator.shows(&leading));

        let trailing_only = indicator.clone().with_edges(false, true);
        assert!(!trailing_only.shows(&leading));
        assert!(trailing_only.shows(&OverscrollIndicatorNotification::new(false)));

        let mut vetoed = OverscrollIndicatorNotification::new(true);
        vetoed.disallow_indicator();
        assert!(!indicator.shows(&vetoed));
    }

    #[test]
    fn an_indicator_that_shows_on_neither_side_says_so_with_an_exclamation() {
        let indicator = GlowingOverscrollIndicator::new(AxisDirection::Right, BLUE);
        assert_eq!(indicator.describe_edges(), "both sides");
        assert_eq!(indicator.axis(), Axis::Horizontal);
        assert_eq!(
            indicator.with_edges(false, false).describe_edges(),
            "neither side (!)"
        );
    }

    // -- The glow: two different events ---------------------------------------

    #[test]
    fn an_impact_glow_comes_from_speed_and_a_pull_glow_from_distance() {
        // Which is why both entry points exist rather than one taking an
        // amount: they are told different things by the scroll view.
        let mut fast = glow();
        fast.absorb_impact(4000.0);
        let mut slow = glow();
        slow.absorb_impact(1000.0);
        finish(&mut fast);
        finish(&mut slow);
        assert!(fast.glow_size() > slow.glow_size());

        let mut far = glow();
        far.pull(100.0, 800.0, 50.0, 400.0);
        let mut near = glow();
        near.pull(10.0, 800.0, 50.0, 400.0);
        finish(&mut far);
        finish(&mut near);
        assert!(far.glow_opacity() > near.glow_opacity());
    }

    #[test]
    fn a_crawl_into_the_edge_glows_like_the_slowest_worth_drawing() {
        // And an impossibly fast one glows like the fastest worth drawing.
        let mut crawl = glow();
        crawl.absorb_impact(1.0);
        let mut floor = glow();
        floor.absorb_impact(GlowController::MIN_VELOCITY);
        assert_eq!(crawl.duration_ms(), floor.duration_ms());

        let mut absurd = glow();
        absurd.absorb_impact(1.0e9);
        let mut ceiling = glow();
        ceiling.absorb_impact(GlowController::MAX_VELOCITY);
        assert_eq!(absurd.duration_ms(), ceiling.duration_ms());
    }

    #[test]
    fn a_second_impact_carries_on_from_where_the_first_had_got_to() {
        // Only an idle glow starts from the fixed 0.3.
        let mut controller = glow();
        controller.absorb_impact(5000.0);
        controller.set_t(0.5);
        let mid = controller.glow_opacity();
        assert!(mid > 0.0);

        controller.absorb_impact(5000.0);
        assert_eq!(
            controller.glow_opacity(),
            mid,
            "the new span begins where the old one stood"
        );
    }

    #[test]
    fn the_glow_never_shrinks_while_it_is_being_pulled() {
        // The max in the size computation is the rule, not a safety net: only a
        // recede takes the glow back down.
        let mut controller = glow();
        controller.pull(200.0, 800.0, 200.0, 400.0);
        finish(&mut controller);
        let grown = controller.glow_size();
        assert!(grown > 0.0);

        // A tiny further pull computes a smaller target, and is ignored.
        controller.pull(0.001, 800.0, 200.0, 400.0);
        assert!(controller.glow_size() >= grown);
    }

    #[test]
    fn an_impact_recedes_by_itself_and_a_pull_waits_to_be_told() {
        // An impact is over the moment it lands; a pull is a finger that is
        // still there.
        let mut absorbed = glow();
        absorbed.absorb_impact(3000.0);
        finish(&mut absorbed);
        assert_eq!(absorbed.state(), GlowState::Recede);

        let mut pulled = glow();
        pulled.pull(50.0, 800.0, 200.0, 400.0);
        finish(&mut pulled);
        assert_eq!(pulled.state(), GlowState::Pull, "still being held");

        pulled.scroll_end();
        assert_eq!(pulled.state(), GlowState::Recede);
        assert_eq!(pulled.duration_ms(), GlowController::RECEDE_TIME_MS);
    }

    #[test]
    fn a_finger_that_merely_stops_moving_still_fades_the_glow_out() {
        // Letting go is not required -- and the decay it gets is the slow one,
        // twelve times longer than the one a released drag gets.
        let mut controller = glow();
        controller.pull(50.0, 800.0, 200.0, 400.0);
        assert!(controller.is_pull_recede_pending());

        controller.pull_hold_elapsed();
        assert_eq!(controller.state(), GlowState::Recede);
        assert_eq!(controller.duration_ms(), GlowController::PULL_DECAY_TIME_MS);
        assert!(controller.duration_ms() > GlowController::RECEDE_TIME_MS);
    }

    #[test]
    fn a_further_pull_cancels_the_pending_fade() {
        let mut controller = glow();
        controller.pull(50.0, 800.0, 200.0, 400.0);
        controller.pull(50.0, 800.0, 200.0, 400.0);
        assert!(
            controller.is_pull_recede_pending(),
            "re-armed, not left over"
        );
        assert_eq!(controller.state(), GlowState::Pull);
    }

    #[test]
    fn a_finished_recede_goes_idle_and_only_then_forgets_the_pull() {
        // The pull distance survives the whole gesture, so a reader who lets go
        // and grabs again mid-fade carries on rather than starting over.
        let mut controller = glow();
        controller.pull(400.0, 800.0, 200.0, 400.0);
        assert!(controller.pull_distance() > 0.0);

        controller.scroll_end();
        assert!(
            controller.pull_distance() > 0.0,
            "still remembered while fading"
        );

        finish(&mut controller);
        assert_eq!(controller.state(), GlowState::Idle);
        assert_eq!(controller.pull_distance(), 0.0);
    }

    #[test]
    fn receding_something_that_is_already_receding_does_nothing() {
        let mut controller = glow();
        controller.pull(50.0, 800.0, 200.0, 400.0);
        controller.recede(GlowController::RECEDE_TIME_MS);
        controller.set_t(0.5);
        let midway = controller.glow_opacity();

        controller.recede(GlowController::PULL_DECAY_TIME_MS);
        assert_eq!(controller.duration_ms(), GlowController::RECEDE_TIME_MS);
        assert_eq!(controller.glow_opacity(), midway, "not restarted");

        let mut idle = glow();
        idle.recede(GlowController::RECEDE_TIME_MS);
        assert_eq!(idle.state(), GlowState::Idle);
    }

    #[test]
    fn a_drag_that_outlives_its_own_animation_still_repaints() {
        // The 167ms pull animation finishes while the finger is still moving,
        // and there is then nothing animating to schedule a frame.
        let mut controller = glow();
        controller.pull(50.0, 800.0, 200.0, 400.0);
        finish(&mut controller);
        let before = controller.notification_count();

        controller.pull(50.0, 800.0, 200.0, 400.0);
        assert!(
            controller.notification_count() > before,
            "said so itself, since nothing else would"
        );
    }

    // -- The glow's geometry ---------------------------------------------------

    #[test]
    fn an_impact_is_centred_and_a_pull_is_where_the_finger_is() {
        let mut impact = glow();
        impact.absorb_impact(3000.0);
        assert_eq!(impact.displacement(), 0.5);

        let mut pull = glow();
        pull.pull(50.0, 800.0, 100.0, 400.0);
        assert_eq!(pull.displacement_target(), 0.25);
    }

    #[test]
    fn the_glow_chases_the_finger_across_the_axis_rather_than_jumping() {
        let mut controller = glow();
        controller.pull(50.0, 800.0, 400.0, 400.0);
        assert_eq!(controller.displacement_target(), 1.0);
        assert_eq!(controller.displacement(), 0.5, "not there yet");

        // One frame at sixty hertz is the half-life.
        controller.tick_displacement(1000.0 / 60.0);
        assert!((controller.displacement() - 0.75).abs() < 1e-4);

        controller.tick_displacement(1000.0 / 60.0);
        assert!((controller.displacement() - 0.875).abs() < 1e-4);

        for _ in 0..40 {
            controller.tick_displacement(1000.0 / 60.0);
        }
        assert!(controller.displacement_settled());
    }

    #[test]
    fn an_invisible_glow_is_not_painted_at_all() {
        let controller = glow();
        assert_eq!(controller.glow_opacity(), 0.0);
        assert!(controller.paint_geometry(400.0, 800.0).is_none());
    }

    #[test]
    fn the_arc_is_a_circle_far_wider_than_the_viewport() {
        // A circle that much wider shows only its shallow top, which is the
        // shape Android's glow has.
        let mut controller = glow();
        controller.absorb_impact(5000.0);
        finish(&mut controller);
        let geometry = controller.paint_geometry(400.0, 800.0).unwrap();

        assert_eq!(geometry.radius, 600.0, "one and a half viewport widths");
        assert!(
            (geometry.arc_height - 80.4).abs() < 0.1,
            "clipped to a band about a fifth of the width: {}",
            geometry.arc_height
        );
        assert!(
            geometry.arc_height < geometry.radius / 7.0,
            "so only the shallow top of the circle is inside it"
        );
        assert!(
            geometry.center_y < 0.0,
            "and its centre is far above the band it is drawn in"
        );
    }

    #[test]
    fn an_idle_glow_cannot_be_hit_softly_enough_to_start_dimmer_than_zero_point_three() {
        // The opacity is clamped with its own *begin* as the floor, so an idle
        // glow -- which begins at the fixed 0.3 -- looks the same for every
        // impact below 5000 px/s. Velocity only begins to tell above that.
        let mut gentle = glow();
        gentle.absorb_impact(500.0);
        let mut firm = glow();
        firm.absorb_impact(4900.0);
        finish(&mut gentle);
        finish(&mut firm);
        assert_eq!(gentle.glow_opacity(), 0.3);
        assert_eq!(firm.glow_opacity(), 0.3);

        let mut hard = glow();
        hard.absorb_impact(6000.0);
        finish(&mut hard);
        assert!(hard.glow_opacity() > 0.3, "and now it does");

        let mut slammed = glow();
        slammed.absorb_impact(GlowController::MAX_VELOCITY);
        finish(&mut slammed);
        assert_eq!(
            slammed.glow_opacity(),
            GlowController::MAX_OPACITY,
            "with a ceiling of its own"
        );
    }

    #[test]
    fn a_wide_viewport_scales_the_glow_down_to_fit_its_height() {
        let mut tall = glow();
        tall.absorb_impact(5000.0);
        finish(&mut tall);
        let portrait = tall.paint_geometry(400.0, 800.0).unwrap();
        let landscape = tall.paint_geometry(800.0, 400.0).unwrap();
        assert!(landscape.scale_y < portrait.scale_y);
    }

    // -- The stretch -----------------------------------------------------------

    #[test]
    fn the_edge_resists_more_the_further_it_is_pulled() {
        // The exponential half saturates, leaving only the slow linear term.
        let step = |from: f32, to: f32| {
            let mut a = StretchController::new();
            a.pull(from);
            let mut b = StretchController::new();
            b.pull(to);
            b.overscroll() - a.overscroll()
        };
        let first = step(0.0, 0.05);
        let later = step(0.5, 0.55);
        assert!(
            first > later * 4.0,
            "first {first}, later {later} -- the same drag moves far less"
        );
    }

    #[test]
    fn the_exponential_half_dominates_at_the_start() {
        // Its slope there is EXPONENTIAL_SCALAR times the linear one.
        let mut controller = StretchController::new();
        controller.pull(0.001);
        let measured = controller.overscroll() / 0.001;
        let linear_only = StretchController::STRETCH_INTENSITY;
        let expected = linear_only * (1.0 + StretchController::EXPONENTIAL_SCALAR);
        assert!(
            (measured - expected).abs() < 1e-3,
            "{measured} vs {expected}"
        );
        assert!(measured > linear_only * 9.0);
    }

    #[test]
    fn the_stretch_keeps_the_sign_of_the_pull() {
        let mut up = StretchController::new();
        up.pull(0.2);
        let mut down = StretchController::new();
        down.pull(-0.2);
        assert!(up.overscroll() > 0.0);
        assert_eq!(down.overscroll(), -up.overscroll());
    }

    #[test]
    fn the_stretch_is_held_inside_the_unit_range() {
        let mut controller = StretchController::new();
        controller.set_overscroll(50.0);
        assert_eq!(controller.overscroll(), StretchController::MAX_OVERSCROLL);
        controller.set_overscroll(-50.0);
        assert_eq!(controller.overscroll(), StretchController::MIN_OVERSCROLL);
    }

    #[test]
    fn a_pull_interrupting_a_spring_continues_from_where_it_was() {
        // Otherwise the stretch would snap back to nothing and out again under
        // a finger that never lifted.
        let mut controller = StretchController::new();
        controller.pull(0.5);
        let pulled = controller.overscroll();
        controller.scroll_end(0.0);
        assert!(controller.is_animating());

        controller.tick(0.01);
        let mid_flight = controller.overscroll();
        assert!(mid_flight != 0.0);

        controller.pull(0.0);
        assert!(!controller.is_animating());
        assert_eq!(
            controller.interrupted_overscroll(),
            mid_flight,
            "captured at the moment of interruption"
        );
        assert_eq!(
            controller.overscroll(),
            mid_flight,
            "and a zero pull leaves it exactly there"
        );
        assert!(pulled > 0.0);
    }

    #[test]
    fn a_spring_already_going_home_is_not_restarted() {
        let mut controller = StretchController::new();
        controller.pull(0.5);
        controller.scroll_end(1000.0);
        controller.tick(0.01);
        let after_one_tick = controller.overscroll();

        controller.scroll_end(9999.0);
        controller.tick(0.0);
        assert_eq!(
            controller.overscroll(),
            after_one_tick,
            "the second release did nothing"
        );
    }

    #[test]
    fn an_impact_and_a_release_read_velocity_in_opposite_directions() {
        // One is content arriving at the edge; the other is a hand leaving it.
        let mut impact = StretchController::new();
        impact.absorb_impact(1000.0);
        impact.tick(0.001);
        let impact_sign = impact.overscroll().signum();

        let mut release = StretchController::new();
        release.scroll_end(1000.0);
        release.tick(0.001);
        let release_sign = release.overscroll().signum();

        assert_eq!(impact_sign, -release_sign, "same number, opposite stretch");
    }

    #[test]
    fn a_still_edge_with_no_velocity_is_left_alone() {
        let mut impact = StretchController::new();
        impact.absorb_impact(0.0);
        assert!(!impact.is_animating());

        let mut release = StretchController::new();
        release.scroll_end(0.0);
        assert!(!release.is_animating(), "nothing moved and nothing to undo");

        let mut stretched = StretchController::new();
        stretched.pull(0.3);
        stretched.scroll_end(0.0);
        assert!(
            stretched.is_animating(),
            "but a held stretch must come home"
        );
    }

    #[test]
    fn the_time_correction_is_applied_to_both_the_spring_and_the_velocity() {
        // Upstream notes that scaling time is equivalent to scaling the natural
        // frequency and the initial velocity, and then does the latter --
        // squared into the stiffness, once into the velocity.
        let spring = StretchController::spring();
        let raw = StretchController::STIFFNESS;
        let factor = StretchController::TIME_CORRECTION_FACTOR;
        assert!((spring.stiffness - raw * factor * factor).abs() < 1e-3);
        assert!(spring.stiffness < raw, "which is what slows it down");
    }

    #[test]
    fn a_released_stretch_comes_all_the_way_home() {
        let mut controller = StretchController::new();
        controller.pull(0.8);
        assert!(controller.overscroll() > 0.0);
        controller.scroll_end(0.0);

        let mut ticks = 0;
        while controller.tick(1.0 / 60.0) {
            ticks += 1;
            assert!(ticks < 1000, "the spring should settle");
        }
        assert_eq!(controller.overscroll(), 0.0);
        assert_eq!(controller.interrupted_overscroll(), 0.0);
    }

    // -- The stretching widget --------------------------------------------------

    #[test]
    fn a_reversed_list_stretches_the_same_way_under_the_same_gesture() {
        let forward = StretchingOverscrollIndicator::new(AxisDirection::Down);
        let reversed = StretchingOverscrollIndicator::new(AxisDirection::Up);
        assert_eq!(forward.stretch_strength(0.2), -0.2);
        assert_eq!(reversed.stretch_strength(0.2), 0.2);
        assert_eq!(forward.axis(), Axis::Vertical);
    }

    #[test]
    fn a_viewport_that_fills_the_screen_is_not_clipped() {
        // There is nowhere for the overflow to be seen, and a clip is a layer
        // that costs something.
        let indicator = StretchingOverscrollIndicator::new(AxisDirection::Down);
        assert!(indicator.clips(0.2, 400.0, 800.0));
        assert!(!indicator.clips(0.2, 800.0, 800.0));
        assert!(
            !indicator.clips(0.0, 400.0, 800.0),
            "and nor is a still one"
        );
    }
}
