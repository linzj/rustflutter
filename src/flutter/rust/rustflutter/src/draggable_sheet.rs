//! A sheet a reader drags up from the bottom -- a port of upstream's
//! `widgets/draggable_scrollable_sheet.dart`.
//!
//! The sheet's size is a **fraction of the screen**, not a number of pixels,
//! which is the decision everything else follows from: the same sheet is half
//! the screen on a phone and half the screen on a tablet, and a caller
//! specifying "open at 40%" does not have to know either height.
//!
//! Two pieces carry the judgement:
//!
//! * [`SnappingSimulation`], which decides **which** snap size a released
//!   drag flies to. It is not "the nearest one" -- a flick chooses the one in
//!   its own direction even when the other is closer, because a reader who
//!   flicked upwards meant up.
//! * [`DraggableSheetExtent`], which owns the conversion between the fraction
//!   and the pixels, and the two flags that decide whether a rebuild keeps the
//!   reader's position or returns to the caller's.

use crate::physics::{Simulation, Tolerance};

/// Upstream `DraggableScrollableNotification`: the sheet moved.
///
/// Carries the initial extent as well as the current one, which is what lets
/// a listener tell "the reader dragged it back to where it started" from
/// "nothing has happened yet".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DraggableScrollableNotification {
    pub min_extent: f32,
    pub max_extent: f32,
    pub extent: f32,
    pub initial_extent: f32,
    /// Upstream's `shouldCloseOnMinExtent`: whether reaching the bottom means
    /// the sheet should be dismissed, or merely that it is small.
    pub should_close_on_min_extent: bool,
}

impl DraggableScrollableNotification {
    /// Whether the sheet is as small as it goes -- which, with
    /// `should_close_on_min_extent`, is what a route listens for to pop
    /// itself.
    pub fn is_at_min(&self) -> bool {
        self.extent <= self.min_extent
    }
}

/// Upstream's `_DraggableSheetExtent`: the sheet's size, in both units.
///
/// Upstream keeps `availablePixels` as `double.infinity` until the first
/// layout, and this port keeps that: a conversion asked for before anything
/// has been measured gives an answer of zero pixels' worth of size, which is
/// the correct "I do not know yet" rather than a guess.
#[derive(Clone, Debug, PartialEq)]
pub struct DraggableSheetExtent {
    pub min_size: f32,
    pub max_size: f32,
    pub initial_size: f32,
    pub snap: bool,
    pub snap_sizes: Vec<f32>,
    pub snap_animation_duration_micros: Option<i64>,
    pub should_close_on_min_extent: bool,
    current_size: f32,
    /// How many pixels the sheet's maximum fraction is worth, filled in at
    /// layout.
    pub available_pixels: f32,
    /// Upstream's `hasDragged`: whether a *reader* has moved this sheet.
    /// Snapping is disabled until they have, so a programmatic `animateTo`
    /// does not get yanked to a snap size on arrival.
    pub has_dragged: bool,
    /// Upstream's `hasChanged`: whether the size has moved from its initial
    /// value at all, by any means. This is the one a rebuild consults.
    pub has_changed: bool,
}

impl DraggableSheetExtent {
    pub fn new(min_size: f32, max_size: f32, initial_size: f32) -> DraggableSheetExtent {
        debug_assert!(min_size >= 0.0);
        debug_assert!(max_size <= 1.0);
        debug_assert!(min_size <= initial_size);
        debug_assert!(initial_size <= max_size);
        DraggableSheetExtent {
            min_size,
            max_size,
            initial_size,
            snap: false,
            snap_sizes: Vec::new(),
            snap_animation_duration_micros: None,
            should_close_on_min_extent: true,
            current_size: initial_size,
            available_pixels: f32::INFINITY,
            has_dragged: false,
            has_changed: false,
        }
    }

    pub fn with_snap(mut self, snap: bool, snap_sizes: Vec<f32>) -> Self {
        self.snap = snap;
        self.snap_sizes = snap_sizes;
        self
    }

    pub fn current_size(&self) -> f32 {
        self.current_size
    }

    /// Upstream's `isAtMin`/`isAtMax`.
    pub fn is_at_min(&self) -> bool {
        self.min_size >= self.current_size
    }

    pub fn is_at_max(&self) -> bool {
        self.max_size <= self.current_size
    }

    /// Upstream's `pixelsToSize`, which divides by the pixels the *maximum*
    /// fraction is worth rather than by the viewport: the sheet's coordinate
    /// system is "fraction of the screen", so a drag of a hundred pixels means
    /// more on a short screen.
    pub fn pixels_to_size(&self, pixels: f32) -> f32 {
        if !self.available_pixels.is_finite() || self.available_pixels == 0.0 {
            return 0.0;
        }
        pixels / self.available_pixels * self.max_size
    }

    /// Upstream's `sizeToPixels`.
    pub fn size_to_pixels(&self, size: f32) -> f32 {
        if !self.available_pixels.is_finite() {
            return 0.0;
        }
        size / self.max_size * self.available_pixels
    }

    pub fn current_pixels(&self) -> f32 {
        self.size_to_pixels(self.current_size)
    }

    /// Upstream's `pixelSnapSizes`.
    pub fn pixel_snap_sizes(&self) -> Vec<f32> {
        self.snap_sizes
            .iter()
            .map(|size| self.size_to_pixels(*size))
            .collect()
    }

    /// Upstream's `addPixelDelta`: a drag moved the sheet.
    ///
    /// **Zero available pixels means nothing happens at all**, rather than a
    /// division by zero -- upstream returns early, so a drag arriving before
    /// the first layout is dropped rather than sending the sheet to infinity.
    pub fn add_pixel_delta(&mut self, delta: f32) -> Option<DraggableScrollableNotification> {
        self.has_dragged = true;
        self.has_changed = true;
        if self.available_pixels == 0.0 {
            return None;
        }
        let next = self.current_size + self.pixels_to_size(delta);
        self.update_size(next)
    }

    /// Upstream's `updateSize`, which clamps and then says nothing if the
    /// clamp landed where it already was.
    pub fn update_size(&mut self, new_size: f32) -> Option<DraggableScrollableNotification> {
        let clamped = new_size.clamp(self.min_size, self.max_size);
        if self.current_size == clamped {
            return None;
        }
        self.current_size = clamped;
        Some(DraggableScrollableNotification {
            min_extent: self.min_size,
            max_extent: self.max_size,
            extent: self.current_size,
            initial_extent: self.initial_size,
            should_close_on_min_extent: self.should_close_on_min_extent,
        })
    }

    /// Upstream's `copyWith`, which a rebuild uses.
    ///
    /// **The reader's position survives a rebuild and the caller's does not.**
    /// If the sheet has changed at all, the new extent keeps the current size
    /// clamped into the new bounds; if it has not, it takes the new initial
    /// size. A caller changing `initialChildSize` therefore moves a sheet
    /// nobody has touched and leaves one they have.
    pub fn copy_with(
        &self,
        min_size: f32,
        max_size: f32,
        initial_size: f32,
        snap: bool,
        snap_sizes: Vec<f32>,
    ) -> DraggableSheetExtent {
        let current_size = if self.has_changed {
            self.current_size.clamp(min_size, max_size)
        } else {
            initial_size
        };
        DraggableSheetExtent {
            min_size,
            max_size,
            initial_size,
            snap,
            snap_sizes,
            snap_animation_duration_micros: self.snap_animation_duration_micros,
            should_close_on_min_extent: self.should_close_on_min_extent,
            current_size,
            available_pixels: f32::INFINITY,
            has_dragged: self.has_dragged,
            has_changed: self.has_changed,
        }
    }
}

/// Upstream's `_impliedSnapSizes`: the caller's snap sizes, with the ends
/// guaranteed.
///
/// A sheet always snaps to its own minimum and maximum whether or not the
/// caller listed them, because otherwise a reader could drag to the top and
/// then be thrown back to the highest size that *was* listed.
pub fn implied_snap_sizes(min_size: f32, max_size: f32, snap_sizes: &[f32]) -> Vec<f32> {
    debug_assert!(
        snap_sizes
            .iter()
            .all(|size| *size >= min_size && *size <= max_size)
    );
    debug_assert!(snap_sizes.windows(2).all(|pair| pair[1] > pair[0]));
    if snap_sizes.is_empty() {
        return vec![min_size, max_size];
    }
    let mut sizes = Vec::with_capacity(snap_sizes.len() + 2);
    if snap_sizes[0] != min_size {
        sizes.push(min_size);
    }
    sizes.extend_from_slice(snap_sizes);
    if snap_sizes[snap_sizes.len() - 1] != max_size {
        sizes.push(max_size);
    }
    sizes
}

/// Upstream's `_SnappingSimulation`: a released sheet flying to a snap size.
///
/// It is a straight line at a constant speed, not a spring, and it stops dead
/// on arrival. What makes it interesting is the choice of destination.
#[derive(Clone, Copy, Debug)]
pub struct SnappingSimulation {
    pub position: f32,
    pub velocity: f32,
    snap_position: f32,
    tolerance: Tolerance,
}

impl SnappingSimulation {
    /// Upstream's `minimumSpeed`, and its comment: a minimum so the snap does
    /// not play too slowly. A sheet released with almost no velocity still has
    /// to *go* somewhere, visibly.
    pub const MINIMUM_SPEED: f32 = 1600.0;

    pub fn new(
        position: f32,
        initial_velocity: f32,
        pixel_snap_sizes: &[f32],
        snap_animation_duration_micros: Option<i64>,
        tolerance: Tolerance,
    ) -> SnappingSimulation {
        let snap_position =
            Self::snap_size_for(position, initial_velocity, pixel_snap_sizes, tolerance);
        let velocity = match snap_animation_duration_micros {
            Some(micros) if micros > 0 => (snap_position - position) * 1_000_000.0 / micros as f32,
            // Upstream's comment: check the direction of the *target* rather
            // than the sign of the velocity, because a very slow flick may
            // snap the opposite way to the way it was going.
            _ if snap_position < position => initial_velocity.min(-Self::MINIMUM_SPEED),
            _ => initial_velocity.max(Self::MINIMUM_SPEED),
        };
        SnappingSimulation {
            position,
            velocity,
            snap_position,
            tolerance,
        }
    }

    /// Upstream's `Simulation.tolerance`, which every simulation carries even
    /// when its own `isDone` does not consult it -- the driver reads it to
    /// decide when to stop ticking.
    pub fn tolerance(&self) -> Tolerance {
        self.tolerance
    }

    /// Where this simulation is heading.
    pub fn snap_position(&self) -> f32 {
        self.snap_position
    }

    /// Upstream's `_getSnapSize`: which snap size a release goes to.
    ///
    /// **Not simply the nearest one.** With any real velocity the sheet goes
    /// to the size in the velocity's *direction*, even when the other is
    /// closer -- a reader who flicked upwards meant up, and being dropped back
    /// down because they started nearer the bottom would read as the sheet
    /// fighting them. Only a release with no velocity to speak of takes the
    /// nearest.
    ///
    /// "No velocity to speak of" is `tolerance.velocity`, and *which*
    /// tolerance matters: upstream builds this simulation with
    /// `physics.toleranceFor(position)`, which is around twenty pixels a
    /// second on a one-times screen, not the `Tolerance` default of a
    /// thousandth. A finger is never as still as a thousandth of a pixel a
    /// second, so with the default this branch would essentially never be
    /// taken.
    pub fn snap_size_for(
        position: f32,
        initial_velocity: f32,
        pixel_snap_sizes: &[f32],
        tolerance: Tolerance,
    ) -> f32 {
        let Some(index_of_next) = pixel_snap_sizes.iter().position(|size| *size >= position) else {
            // Past the last snap size: upstream indexes with -1 from
            // `indexWhere`, which in Dart means "not found" and reaches the
            // final `return pixelSnapSizes[indexOfNextSize]` -- a lookup at
            // -1. Reaching it would need a position above the maximum, which
            // the extent's clamp prevents. The last size is what that
            // position means.
            return *pixel_snap_sizes.last().unwrap_or(&position);
        };
        if index_of_next == 0 {
            return pixel_snap_sizes[0];
        }
        let next = pixel_snap_sizes[index_of_next];
        if next == position {
            return next;
        }
        let previous = pixel_snap_sizes[index_of_next - 1];
        if initial_velocity.abs() <= tolerance.velocity {
            return if position - previous < next - position {
                previous
            } else {
                next
            };
        }
        if initial_velocity < 0.0 {
            previous
        } else {
            next
        }
    }
}

impl Simulation for SnappingSimulation {
    fn x(&self, time: f32) -> f32 {
        let next = self.position + self.velocity * time;
        if (self.velocity >= 0.0 && next > self.snap_position)
            || (self.velocity < 0.0 && next < self.snap_position)
        {
            return self.snap_position;
        }
        next
    }

    fn dx(&self, time: f32) -> f32 {
        if self.is_done(time) {
            0.0
        } else {
            self.velocity
        }
    }

    /// Upstream compares for **equality** with the snap size rather than using
    /// a tolerance, which works because `x` returns the snap size exactly once
    /// it has been passed.
    fn is_done(&self, time: f32) -> bool {
        self.x(time) == self.snap_position
    }
}

/// Upstream `DraggableScrollableController`: drives a sheet from outside it.
///
/// Upstream this attaches to the sheet's scroll controller; here it holds the
/// extent directly. What it is *for* is the same: a caller that wants to open,
/// close or move the sheet without the reader touching it.
#[derive(Debug, Default)]
pub struct DraggableScrollableController {
    extent: Option<DraggableSheetExtent>,
}

impl DraggableScrollableController {
    pub fn new() -> DraggableScrollableController {
        DraggableScrollableController { extent: None }
    }

    /// Upstream's `_attach`.
    pub fn attach(&mut self, extent: DraggableSheetExtent) {
        self.extent = Some(extent);
    }

    /// Upstream's `_detach`.
    pub fn detach(&mut self) {
        self.extent = None;
    }

    /// Upstream's `isAttached`. Every other method asserts on it, because a
    /// controller used before its sheet exists has nothing to move.
    pub fn is_attached(&self) -> bool {
        self.extent.is_some()
    }

    pub fn extent(&self) -> Option<&DraggableSheetExtent> {
        self.extent.as_ref()
    }

    /// Upstream's `size`, the sheet's current fraction.
    pub fn size(&self) -> Option<f32> {
        self.extent.as_ref().map(|extent| extent.current_size())
    }

    /// Upstream's `jumpTo`.
    ///
    /// **Clears `hasDragged` and sets `hasChanged`**, which is the pair that
    /// says "this sheet has been moved, but not by the reader". Snapping stays
    /// off until they touch it, so a sheet placed at 43% by a caller stays
    /// there rather than being pulled to a snap size.
    pub fn jump_to(&mut self, size: f32) -> Option<DraggableScrollableNotification> {
        debug_assert!(self.is_attached(), "the controller has no sheet");
        let extent = self.extent.as_mut()?;
        extent.has_dragged = false;
        extent.has_changed = true;
        extent.update_size(size)
    }

    /// Upstream's `animateTo`, which sets the same two flags before it starts
    /// and clamps its target into the sheet's bounds.
    pub fn animate_to_target(&mut self, size: f32) -> Option<f32> {
        debug_assert!(self.is_attached(), "the controller has no sheet");
        let extent = self.extent.as_mut()?;
        extent.has_dragged = false;
        extent.has_changed = true;
        Some(size.clamp(extent.min_size, extent.max_size))
    }

    /// Upstream's `reset`, reached through the actuator.
    pub fn reset(&mut self) -> Option<DraggableScrollableNotification> {
        let extent = self.extent.as_mut()?;
        extent.has_dragged = false;
        extent.has_changed = false;
        let initial = extent.initial_size;
        extent.update_size(initial)
    }
}

/// Upstream `DraggableScrollableActuator`: a handle for resetting every sheet
/// below it.
///
/// Upstream it is an inherited notifier that sheets listen to, so that
/// `DraggableScrollableActuator.reset(context)` reaches them without anyone
/// holding a controller. The static returns whether it found one, which is how
/// a caller can tell "no sheet was reset" from "a sheet was reset to where it
/// already was".
#[derive(Debug, Default)]
pub struct DraggableScrollableActuator {
    should_reset: bool,
}

impl DraggableScrollableActuator {
    pub fn new() -> DraggableScrollableActuator {
        DraggableScrollableActuator {
            should_reset: false,
        }
    }

    /// Upstream's static `reset`.
    pub fn reset(&mut self) -> bool {
        self.should_reset = true;
        true
    }

    /// Upstream's `_InheritedResetNotifier.shouldReset`, which **consumes**
    /// the flag: a reset happens once, no matter how many times the sheet is
    /// rebuilt afterwards.
    pub fn should_reset(&mut self) -> bool {
        let should = self.should_reset;
        self.should_reset = false;
        should
    }
}

/// Upstream `DraggableScrollableSheet`: the widget itself.
pub struct DraggableScrollableSheet {
    pub initial_child_size: f32,
    pub min_child_size: f32,
    pub max_child_size: f32,
    pub expand: bool,
    pub snap: bool,
    pub snap_sizes: Vec<f32>,
    pub snap_animation_duration_micros: Option<i64>,
    pub should_close_on_min_extent: bool,
}

impl Default for DraggableScrollableSheet {
    fn default() -> DraggableScrollableSheet {
        DraggableScrollableSheet::new()
    }
}

impl DraggableScrollableSheet {
    pub fn new() -> DraggableScrollableSheet {
        DraggableScrollableSheet {
            initial_child_size: 0.5,
            min_child_size: 0.25,
            max_child_size: 1.0,
            expand: true,
            snap: false,
            snap_sizes: Vec::new(),
            snap_animation_duration_micros: None,
            should_close_on_min_extent: true,
        }
    }

    pub fn with_sizes(mut self, initial: f32, min: f32, max: f32) -> Self {
        self.initial_child_size = initial;
        self.min_child_size = min;
        self.max_child_size = max;
        self
    }

    pub fn with_snap(mut self, snap: bool, snap_sizes: Vec<f32>) -> Self {
        self.snap = snap;
        self.snap_sizes = snap_sizes;
        self
    }

    pub fn with_snap_animation_duration(mut self, micros: i64) -> Self {
        self.snap_animation_duration_micros = Some(micros);
        self
    }

    /// Upstream's `initState`, which builds the extent from the widget.
    pub fn create_extent(&self) -> DraggableSheetExtent {
        let mut extent = DraggableSheetExtent::new(
            self.min_child_size,
            self.max_child_size,
            self.initial_child_size,
        )
        .with_snap(
            self.snap,
            implied_snap_sizes(self.min_child_size, self.max_child_size, &self.snap_sizes),
        );
        extent.snap_animation_duration_micros = self.snap_animation_duration_micros;
        extent.should_close_on_min_extent = self.should_close_on_min_extent;
        extent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sheet on a 800-pixel screen, sized as a fraction of it.
    fn laid_out(sheet: &DraggableScrollableSheet, screen: f32) -> DraggableSheetExtent {
        let mut extent = sheet.create_extent();
        extent.available_pixels = sheet.max_child_size * screen;
        extent
    }

    #[test]
    fn the_sheet_is_a_fraction_of_the_screen_and_a_drag_means_more_on_a_short_one() {
        // Which is the decision everything else follows from: the same sheet
        // is half the screen on a phone and half on a tablet, and a caller
        // saying "open at 40%" does not have to know either height.
        let sheet = DraggableScrollableSheet::new().with_sizes(0.5, 0.25, 1.0);
        let mut tall = laid_out(&sheet, 800.0);
        let mut short = laid_out(&sheet, 400.0);

        tall.add_pixel_delta(80.0);
        short.add_pixel_delta(80.0);
        assert!((tall.current_size() - 0.6).abs() < 1e-5);
        assert!((short.current_size() - 0.7).abs() < 1e-5);
    }

    #[test]
    fn a_drag_before_the_first_layout_is_dropped_rather_than_dividing_by_zero() {
        let sheet = DraggableScrollableSheet::new();
        let mut unmeasured = sheet.create_extent();
        assert!(!unmeasured.available_pixels.is_finite());
        assert_eq!(unmeasured.pixels_to_size(80.0), 0.0);
        assert_eq!(unmeasured.size_to_pixels(0.5), 0.0);

        let mut zero = laid_out(&sheet, 0.0);
        assert_eq!(zero.add_pixel_delta(80.0), None);
        assert_eq!(zero.current_size(), 0.5, "nothing moved");
        assert!(zero.has_dragged, "but the reader did touch it");
    }

    #[test]
    fn a_move_that_lands_where_it_already_was_says_nothing() {
        let sheet = DraggableScrollableSheet::new().with_sizes(0.5, 0.25, 1.0);
        let mut extent = laid_out(&sheet, 800.0);
        assert!(extent.update_size(0.6).is_some());
        assert_eq!(extent.update_size(0.6), None, "same size, no notification");
        // And the clamp is applied before that comparison.
        extent.update_size(1.0);
        assert_eq!(extent.update_size(5.0), None, "already at the top");
    }

    #[test]
    fn a_notification_carries_where_it_started_as_well_as_where_it_is() {
        // Which is how a listener tells "dragged back to the start" from
        // "nothing has happened".
        let sheet = DraggableScrollableSheet::new().with_sizes(0.5, 0.25, 1.0);
        let mut extent = laid_out(&sheet, 800.0);
        let moved = extent.update_size(0.25).expect("it moved");
        assert_eq!(moved.initial_extent, 0.5);
        assert_eq!(moved.extent, 0.25);
        assert!(moved.is_at_min());
        assert!(moved.should_close_on_min_extent, "upstream's default");
    }

    #[test]
    fn the_ends_are_always_snap_sizes_whether_or_not_the_caller_listed_them() {
        // Otherwise a reader could drag to the very top and be thrown back to
        // the highest size that *was* listed.
        assert_eq!(implied_snap_sizes(0.25, 1.0, &[]), vec![0.25, 1.0]);
        assert_eq!(implied_snap_sizes(0.25, 1.0, &[0.5]), vec![0.25, 0.5, 1.0]);
        // And they are not duplicated when the caller did list them.
        assert_eq!(
            implied_snap_sizes(0.25, 1.0, &[0.25, 0.5, 1.0]),
            vec![0.25, 0.5, 1.0]
        );
        assert_eq!(
            implied_snap_sizes(0.25, 1.0, &[0.25, 0.5]),
            vec![0.25, 0.5, 1.0]
        );
    }

    fn tolerance() -> Tolerance {
        Tolerance::DEFAULT
    }

    #[test]
    fn a_flick_goes_the_way_it_was_flicked_even_when_the_other_size_is_nearer() {
        // The heart of the snapping rule, and the reason it is not simply
        // "the nearest": a reader who flicked upwards meant up, and being
        // dropped back down because they started nearer the bottom would read
        // as the sheet fighting them.
        let sizes = [0.0, 100.0, 400.0];
        // Sitting at 120, which is much nearer 100 than 400.
        assert_eq!(
            SnappingSimulation::snap_size_for(120.0, 900.0, &sizes, tolerance()),
            400.0,
            "flicked up"
        );
        assert_eq!(
            SnappingSimulation::snap_size_for(120.0, -900.0, &sizes, tolerance()),
            100.0,
            "flicked down"
        );
        // Only a release with no velocity to speak of takes the nearest.
        assert_eq!(
            SnappingSimulation::snap_size_for(120.0, 0.0, &sizes, tolerance()),
            100.0
        );
        assert_eq!(
            SnappingSimulation::snap_size_for(380.0, 0.0, &sizes, tolerance()),
            400.0
        );
    }

    #[test]
    fn a_sheet_already_on_a_snap_size_stays_there() {
        let sizes = [0.0, 100.0, 400.0];
        assert_eq!(
            SnappingSimulation::snap_size_for(100.0, 900.0, &sizes, tolerance()),
            100.0,
            "already snapped, however hard it was flicked"
        );
        // And below the first size there is only one answer.
        assert_eq!(
            SnappingSimulation::snap_size_for(-10.0, 900.0, &sizes, tolerance()),
            0.0
        );
    }

    #[test]
    fn a_slow_release_still_moves_visibly() {
        // Upstream's minimumSpeed, and its comment: a minimum so the snapping
        // animation does not play too slowly. A sheet let go with almost no
        // velocity still has to go somewhere, visibly.
        let sizes = [0.0, 100.0, 400.0];
        // Below the tolerance, so this counts as no velocity at all and the
        // nearest size wins.
        let crawling = SnappingSimulation::new(120.0, 0.0005, &sizes, None, tolerance());
        assert_eq!(crawling.snap_position(), 100.0, "nearest, at that speed");
        assert_eq!(
            crawling.velocity,
            -SnappingSimulation::MINIMUM_SPEED,
            "and it goes there at the minimum speed"
        );

        // A fast flick keeps its own speed.
        let flicked = SnappingSimulation::new(120.0, 5000.0, &sizes, None, tolerance());
        assert_eq!(flicked.velocity, 5000.0);
    }

    #[test]
    fn the_velocity_is_chosen_from_the_target_not_from_the_flick() {
        // Upstream's comment says so outright: a very slow flick may snap the
        // opposite way to the way it was going, so the sign has to come from
        // where the sheet is heading.
        let sizes = [0.0, 100.0, 400.0];
        // A feeble upward flick from 110: too slow to count as a flick, so it
        // snaps *down* to 100 -- against the direction it was moving.
        let contrary = SnappingSimulation::new(110.0, 0.0005, &sizes, None, tolerance());
        assert_eq!(contrary.snap_position(), 100.0);
        assert!(
            contrary.velocity < 0.0,
            "downwards, though the flick was upwards: {}",
            contrary.velocity
        );
    }

    #[test]
    fn a_duration_replaces_the_speed_entirely() {
        // With snapAnimationDuration the sheet covers the distance in that
        // time, whatever the flick was worth -- the minimum speed does not
        // apply, because the caller has said how long it should take.
        let sizes = [0.0, 100.0, 400.0];
        let timed = SnappingSimulation::new(120.0, 900.0, &sizes, Some(200_000), tolerance());
        assert_eq!(timed.snap_position(), 400.0);
        let expected = (400.0 - 120.0) * 1_000_000.0 / 200_000.0;
        assert!(
            (timed.velocity - expected).abs() < 1e-3,
            "{}",
            timed.velocity
        );
        // And it arrives exactly when it was told to.
        assert!((timed.x(0.2) - 400.0).abs() < 1e-2);
        assert!(timed.is_done(0.2));
    }

    #[test]
    fn the_snap_stops_dead_on_arrival_rather_than_overshooting() {
        let sizes = [0.0, 100.0, 400.0];
        // From 110 rather than 100: sitting exactly on a snap size means
        // staying there, which the test above pins.
        let flying = SnappingSimulation::new(110.0, 2000.0, &sizes, None, tolerance());
        assert_eq!(flying.snap_position(), 400.0);
        assert!(!flying.is_done(0.05));
        assert_eq!(flying.x(10.0), 400.0, "and never past it");
        assert!(flying.is_done(10.0));
        assert_eq!(flying.dx(10.0), 0.0);
        assert_eq!(flying.dx(0.05), 2000.0);
    }

    #[test]
    fn a_rebuild_keeps_the_readers_position_and_not_the_callers() {
        // hasChanged is the flag that decides. A caller changing
        // initialChildSize moves a sheet nobody has touched, and leaves one
        // they have.
        let sheet = DraggableScrollableSheet::new().with_sizes(0.5, 0.25, 1.0);
        let untouched = laid_out(&sheet, 800.0);
        let rebuilt = untouched.copy_with(0.25, 1.0, 0.8, false, vec![0.25, 1.0]);
        assert_eq!(rebuilt.current_size(), 0.8, "the new initial size");

        let mut touched = laid_out(&sheet, 800.0);
        touched.add_pixel_delta(160.0);
        let held = touched.current_size();
        let rebuilt = touched.copy_with(0.25, 1.0, 0.8, false, vec![0.25, 1.0]);
        assert_eq!(rebuilt.current_size(), held, "the reader's position stands");
        assert!(rebuilt.has_dragged, "and it remembers who moved it");
    }

    #[test]
    fn a_rebuild_with_tighter_bounds_pulls_the_reader_inside_them() {
        let sheet = DraggableScrollableSheet::new().with_sizes(0.5, 0.25, 1.0);
        let mut extent = laid_out(&sheet, 800.0);
        extent.add_pixel_delta(320.0);
        assert!(extent.current_size() > 0.8);
        let rebuilt = extent.copy_with(0.25, 0.6, 0.5, false, vec![0.25, 0.6]);
        assert_eq!(rebuilt.current_size(), 0.6);
    }

    #[test]
    fn a_controller_move_is_a_change_but_not_a_drag() {
        // The pair of flags that says "moved, but not by the reader". Snapping
        // stays off until they touch it, so a sheet placed at 43% by a caller
        // stays there instead of being pulled to a snap size.
        let sheet = DraggableScrollableSheet::new().with_sizes(0.5, 0.25, 1.0);
        let mut controller = DraggableScrollableController::new();
        assert!(!controller.is_attached());
        assert_eq!(controller.size(), None);

        controller.attach(laid_out(&sheet, 800.0));
        assert!(controller.is_attached());
        assert!(controller.jump_to(0.43).is_some());
        assert_eq!(controller.size(), Some(0.43));

        let extent = controller.extent().expect("attached");
        assert!(extent.has_changed);
        assert!(!extent.has_dragged, "the reader has not touched it");
    }

    #[test]
    fn animating_to_somewhere_out_of_range_lands_at_the_edge() {
        let sheet = DraggableScrollableSheet::new().with_sizes(0.5, 0.25, 0.9);
        let mut controller = DraggableScrollableController::new();
        controller.attach(laid_out(&sheet, 800.0));
        assert_eq!(controller.animate_to_target(1.0), Some(0.9));
        assert_eq!(controller.animate_to_target(0.0), Some(0.25));
    }

    #[test]
    fn a_reset_returns_the_sheet_and_forgets_that_anything_happened() {
        let sheet = DraggableScrollableSheet::new().with_sizes(0.5, 0.25, 1.0);
        let mut controller = DraggableScrollableController::new();
        let mut extent = laid_out(&sheet, 800.0);
        extent.add_pixel_delta(240.0);
        controller.attach(extent);

        assert!(controller.reset().is_some());
        assert_eq!(controller.size(), Some(0.5));
        let extent = controller.extent().expect("attached");
        assert!(!extent.has_changed, "as though it had never moved");
        assert!(!extent.has_dragged);
    }

    #[test]
    fn the_actuator_resets_once_however_many_times_the_sheet_rebuilds() {
        // Upstream's shouldReset consumes the flag, so a rebuild after the
        // reset does not reset again.
        let mut actuator = DraggableScrollableActuator::new();
        assert!(!actuator.should_reset(), "nothing asked for yet");
        assert!(actuator.reset());
        assert!(actuator.should_reset());
        assert!(!actuator.should_reset(), "and only once");
    }

    #[test]
    fn the_sheet_hands_its_extent_the_snap_sizes_it_implied() {
        let sheet = DraggableScrollableSheet::new()
            .with_sizes(0.5, 0.25, 1.0)
            .with_snap(true, vec![0.5])
            .with_snap_animation_duration(200_000);
        let extent = sheet.create_extent();
        assert!(extent.snap);
        assert_eq!(extent.snap_sizes, vec![0.25, 0.5, 1.0]);
        assert_eq!(extent.snap_animation_duration_micros, Some(200_000));
        assert_eq!(extent.current_size(), 0.5);
        assert!(extent.is_at_min() == false && extent.is_at_max() == false);

        let laid = laid_out(&sheet, 800.0);
        assert_eq!(laid.pixel_snap_sizes(), vec![200.0, 400.0, 800.0]);
        assert_eq!(laid.current_pixels(), 400.0);
    }
}
