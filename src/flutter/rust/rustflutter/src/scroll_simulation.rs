//! A port of `widgets/scroll_simulation.dart`'s `BouncingScrollSimulation`.
//!
//! iOS's fling, and it is **two simulations glued at a computed instant**:
//! friction while the content is in range, a spring once it is not. The join
//! is what makes it feel like one motion -- the spring is handed the exact
//! velocity the friction had at the moment it crossed the edge, so nothing
//! jumps.

use crate::physics::{FrictionSimulation, Simulation, SpringDescription, SpringSimulation};

/// Upstream `BouncingScrollSimulation`.
pub struct BouncingScrollSimulation {
    pub leading_extent: f32,
    pub trailing_extent: f32,
    friction: Option<FrictionSimulation>,
    spring: Option<SpringSimulation>,
    /// The instant the spring takes over.
    ///
    /// Two special values do the work of two branches: **negative infinity**
    /// means the spring is in charge from the start (the content was already
    /// out of range), and **positive infinity** means it never runs at all (the
    /// fling stops before the edge).
    spring_time: f32,
}

impl BouncingScrollSimulation {
    /// Upstream's comment gives the provenance: `UIScrollView.decelerationRate`
    /// `.normal` is 0.998 **per millisecond**, and `0.998^1000 ≈ 0.135` is the
    /// same rate per second. Flutter's friction constant is Apple's number
    /// converted to Flutter's unit of time, not a value anybody tuned.
    pub const FRICTION_DRAG: f32 = 0.135;

    /// The most velocity a fling may hand to the bounce.
    ///
    /// A fling fast enough to blow through the end does **not** transfer all of
    /// its energy: a proportional bounce at twenty thousand pixels a second
    /// would throw the content most of the way off the screen and back. Capping
    /// it means a very fast fling and a merely fast one bounce the same amount,
    /// which is what the platform does and what a reader expects.
    pub const MAX_SPRING_TRANSFER_VELOCITY: f32 = 5000.0;

    pub fn new(
        position: f32,
        velocity: f32,
        leading_extent: f32,
        trailing_extent: f32,
        spring: SpringDescription,
    ) -> BouncingScrollSimulation {
        debug_assert!(leading_extent <= trailing_extent);

        if position < leading_extent {
            // Already past the start: nothing to decelerate, only to return.
            return BouncingScrollSimulation {
                leading_extent,
                trailing_extent,
                friction: None,
                spring: Some(SpringSimulation::new(
                    spring,
                    position,
                    leading_extent,
                    velocity,
                )),
                spring_time: f32::NEG_INFINITY,
            };
        }
        if position > trailing_extent {
            return BouncingScrollSimulation {
                leading_extent,
                trailing_extent,
                friction: None,
                spring: Some(SpringSimulation::new(
                    spring,
                    position,
                    trailing_extent,
                    velocity,
                )),
                spring_time: f32::NEG_INFINITY,
            };
        }

        let friction =
            FrictionSimulation::new(BouncingScrollSimulation::FRICTION_DRAG, position, velocity);
        let final_x = friction.final_x();

        let (spring_simulation, spring_time) = if velocity > 0.0 && final_x > trailing_extent {
            let time = friction.time_at_x(trailing_extent);
            let handover = friction
                .dx(time)
                .min(BouncingScrollSimulation::MAX_SPRING_TRANSFER_VELOCITY);
            (
                Some(SpringSimulation::new(
                    spring,
                    trailing_extent,
                    trailing_extent,
                    handover,
                )),
                time,
            )
        } else if velocity < 0.0 && final_x < leading_extent {
            let time = friction.time_at_x(leading_extent);
            let handover = friction
                .dx(time)
                .min(BouncingScrollSimulation::MAX_SPRING_TRANSFER_VELOCITY);
            (
                Some(SpringSimulation::new(
                    spring,
                    leading_extent,
                    leading_extent,
                    handover,
                )),
                time,
            )
        } else {
            // The fling stops on its own before either edge, so there is no
            // bounce to schedule.
            (None, f32::INFINITY)
        };

        BouncingScrollSimulation {
            leading_extent,
            trailing_extent,
            friction: Some(friction),
            spring: spring_simulation,
            spring_time,
        }
    }

    pub fn spring_time(&self) -> f32 {
        self.spring_time
    }

    pub fn will_bounce(&self) -> bool {
        self.spring.is_some()
    }

    /// Whether the bounce is already under way at time zero.
    pub fn starts_out_of_range(&self) -> bool {
        self.spring_time == f32::NEG_INFINITY
    }

    /// Upstream `_simulation`, which also sets the time offset. The spring's
    /// own clock starts at the handover, so its time has to be rebased -- except
    /// in the already-out-of-range case, where the offset stays zero because
    /// negative infinity is not a real instant.
    fn active(&self, time: f32) -> (&dyn Simulation, f32) {
        if time > self.spring_time {
            let offset = if self.spring_time.is_finite() {
                self.spring_time
            } else {
                0.0
            };
            (
                self.spring.as_ref().expect("a spring past the spring time") as &dyn Simulation,
                offset,
            )
        } else {
            (
                self.friction
                    .as_ref()
                    .expect("friction before the spring time") as &dyn Simulation,
                0.0,
            )
        }
    }
}

impl Simulation for BouncingScrollSimulation {
    fn x(&self, time: f32) -> f32 {
        let (simulation, offset) = self.active(time);
        simulation.x(time - offset)
    }

    fn dx(&self, time: f32) -> f32 {
        let (simulation, offset) = self.active(time);
        simulation.dx(time - offset)
    }

    fn is_done(&self, time: f32) -> bool {
        let (simulation, offset) = self.active(time);
        simulation.is_done(time - offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spring() -> SpringDescription {
        SpringDescription::with_damping_ratio(1.0, 100.0, 1.1)
    }

    fn fling(position: f32, velocity: f32) -> BouncingScrollSimulation {
        BouncingScrollSimulation::new(position, velocity, 0.0, 1000.0, spring())
    }

    #[test]
    fn the_friction_constant_is_apples_number_in_flutters_unit_of_time() {
        // UIScrollView.decelerationRate .normal is 0.998 per millisecond, and
        // 0.998^1000 is about 0.135 per second. Nobody tuned this.
        let per_millisecond: f64 = 0.998;
        let per_second = per_millisecond.powi(1000) as f32;
        assert!(
            (per_second - BouncingScrollSimulation::FRICTION_DRAG).abs() < 0.001,
            "{per_second}"
        );
    }

    #[test]
    fn a_fling_that_stops_short_of_the_edge_schedules_no_bounce() {
        let gentle = fling(500.0, 100.0);
        assert!(!gentle.will_bounce());
        assert_eq!(
            gentle.spring_time(),
            f32::INFINITY,
            "so the spring never runs"
        );
    }

    #[test]
    fn a_fling_that_would_carry_past_the_end_hands_over_at_the_edge() {
        let hard = fling(900.0, 4000.0);
        assert!(hard.will_bounce());
        assert!(hard.spring_time().is_finite());
        assert!(hard.spring_time() > 0.0);

        // Just before the handover the content is still short of the edge;
        // just after, the spring is in charge.
        let before = hard.x(hard.spring_time() * 0.5);
        assert!(before < 1000.0, "{before}");
    }

    #[test]
    fn the_join_has_no_seam_in_it() {
        // The spring is handed the position and velocity the friction had at
        // the exact instant it crossed, so nothing jumps.
        let hard = fling(900.0, 3000.0);
        let t = hard.spring_time();
        let just_before = hard.x(t - 0.0001);
        let just_after = hard.x(t + 0.0001);
        assert!(
            (just_before - just_after).abs() < 1.0,
            "{just_before} then {just_after}"
        );
        assert!((just_after - 1000.0).abs() < 1.0, "and at the edge");
    }

    #[test]
    fn content_already_out_of_range_springs_from_the_very_first_instant() {
        // Negative infinity is doing the work of a branch here.
        let overscrolled = fling(1100.0, 0.0);
        assert!(overscrolled.starts_out_of_range());
        assert_eq!(overscrolled.spring_time(), f32::NEG_INFINITY);
        assert_eq!(overscrolled.x(0.0), 1100.0);
        assert!(overscrolled.x(0.1) < 1100.0, "already coming home");

        let underscrolled = fling(-50.0, 0.0);
        assert!(underscrolled.starts_out_of_range());
        assert!(underscrolled.x(0.1) > -50.0);
    }

    #[test]
    fn a_very_fast_fling_and_a_merely_fast_one_bounce_the_same_amount() {
        // The transfer is capped, because a proportional bounce at twenty
        // thousand pixels a second would throw the content off the screen.
        assert_eq!(
            BouncingScrollSimulation::MAX_SPRING_TRANSFER_VELOCITY,
            5000.0
        );

        let fast = fling(999.0, 20000.0);
        let faster = fling(999.0, 60000.0);
        let peak = |sim: &BouncingScrollSimulation| {
            let mut best: f32 = 0.0;
            for step in 0..200 {
                best = best.max(sim.x(step as f32 * 0.005));
            }
            best
        };
        assert!(
            (peak(&fast) - peak(&faster)).abs() < 1.0,
            "{} vs {}",
            peak(&fast),
            peak(&faster)
        );
    }

    #[test]
    fn a_fling_the_other_way_bounces_off_the_start() {
        let backwards = fling(50.0, -4000.0);
        assert!(backwards.will_bounce());
        assert!(backwards.spring_time().is_finite());
    }

    #[test]
    fn the_bounce_settles_at_the_edge_it_came_from() {
        let hard = fling(900.0, 3000.0);
        let mut time = hard.spring_time();
        while !hard.is_done(time) && time < 60.0 {
            time += 1.0 / 60.0;
        }
        assert!(time < 60.0, "the spring should settle");
        assert!((hard.x(time) - 1000.0).abs() < 1.0);
    }

    #[test]
    fn a_friction_run_says_when_it_passes_a_point_and_when_it_never_will() {
        let friction = crate::physics::FrictionSimulation::new(0.135, 0.0, 1000.0);
        assert_eq!(friction.time_at_x(0.0), 0.0);

        let halfway = friction.final_x() / 2.0;
        let t = friction.time_at_x(halfway);
        assert!(t.is_finite() && t > 0.0);
        assert!((friction.x(t) - halfway).abs() < 0.1);

        assert_eq!(
            friction.time_at_x(-10.0),
            f32::INFINITY,
            "behind it, so never"
        );
        assert_eq!(
            friction.time_at_x(friction.final_x() + 10.0),
            f32::INFINITY,
            "past where it stops, so never"
        );
        assert_eq!(
            crate::physics::FrictionSimulation::new(0.135, 0.0, 0.0).time_at_x(10.0),
            f32::INFINITY,
            "and something that is not moving never gets anywhere"
        );
    }
}
