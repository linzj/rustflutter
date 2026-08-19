//! A list whose items all have the same height, and which lands on whole
//! items -- a port of the delegate and fixed-extent half of upstream's
//! `widgets/list_wheel_scroll_view.dart`.
//!
//! Two ideas live here, and they are separable from the wheel's geometry:
//!
//! * **who supplies the children**, which is what the four delegates answer,
//!   including the looping one that makes a wheel endless;
//! * **where a fling is allowed to stop**, which is what
//!   [`FixedExtentScrollPhysics`] answers: never between two items.
//!
//! The wheel's own cylindrical projection -- `RenderListWheelViewport` and the
//! widgets around it -- is not here; the crate's `CupertinoPicker` in
//! `cupertino.rs` carries a working version of that geometry already.
//!
//! ## What this closes
//!
//! `CupertinoPicker`'s documentation has been recording that
//! `FixedExtentScrollPhysics.createBallisticSimulation`'s scenario 5 -- the
//! tuned friction that lands *exactly* on an item --  was not ported, and that
//! a short ease-out drive stood in for it. [`FrictionSimulation::through`] is
//! now in `physics.rs` and all five scenarios are here.

use crate::animation::Curve;
use crate::framework::AnyWidget;
use crate::physics::{
    FrictionSimulation, Simulation, SpringDescription, SpringSimulation, Tolerance,
};
use crate::render::AxisDirection;
use crate::scrolling::Scroll;
use std::rc::Rc;

/// Upstream `ChangeReportingBehavior`: when a wheel says which item is
/// selected.
///
/// Reporting on every update follows the finger; reporting on scroll end says
/// it once. The difference is felt where the callback is expensive or has a
/// side effect a reader would notice -- a haptic tick per item is one thing,
/// a network request per item quite another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChangeReportingBehavior {
    #[default]
    OnScrollEnd,
    OnScrollUpdate,
}

/// Upstream `ListWheelChildDelegate`: where a wheel's children come from.
///
/// Upstream also has `shouldRebuild`, taking a `covariant` parameter -- which
/// is a downcast to the delegate's own type. Each concrete delegate below
/// carries its own `should_rebuild(&self, old: &Self)` for that reason: the
/// comparison is only meaningful between two delegates of the same kind, and
/// saying so in the signature is what `covariant` was reaching for.
pub trait ListWheelChildDelegate {
    /// The child at `index`, or `None` if there is none there.
    ///
    /// The index is signed because a looping wheel scrolled upwards from its
    /// first item asks for negative ones.
    fn build(&self, index: i64) -> Option<AnyWidget>;

    /// How many children there are, if that is knowable. `None` means
    /// endless -- which is what the looping delegate is.
    fn estimated_child_count(&self) -> Option<usize>;

    /// Which real child an index refers to. The identity for everything but
    /// the looping delegate.
    fn true_index_of(&self, index: i64) -> i64 {
        index
    }
}

/// Upstream `ListWheelChildListDelegate`: an explicit list.
///
/// Upstream holds `List<Widget>` and hands the same widget out again. This
/// crate's `AnyWidget` is not clonable, so the list is of builders -- the same
/// shape every other list in this crate takes for the same reason.
pub struct ListWheelChildListDelegate {
    pub children: Vec<Rc<dyn Fn() -> AnyWidget>>,
}

impl ListWheelChildListDelegate {
    pub fn new(children: Vec<Rc<dyn Fn() -> AnyWidget>>) -> ListWheelChildListDelegate {
        ListWheelChildListDelegate { children }
    }

    /// Upstream's `shouldRebuild`, which compares the child lists.
    pub fn should_rebuild(&self, old: &ListWheelChildListDelegate) -> bool {
        self.children.len() != old.children.len()
            || self
                .children
                .iter()
                .zip(old.children.iter())
                .any(|(a, b)| !Rc::ptr_eq(a, b))
    }
}

impl ListWheelChildDelegate for ListWheelChildListDelegate {
    fn build(&self, index: i64) -> Option<AnyWidget> {
        if index < 0 || index as usize >= self.children.len() {
            return None;
        }
        Some(self.children[index as usize]())
    }

    fn estimated_child_count(&self) -> Option<usize> {
        Some(self.children.len())
    }
}

/// Upstream `ListWheelChildLoopingListDelegate`: the same explicit list, over
/// and over, in both directions.
///
/// This is what makes a wheel of hours or minutes feel like a wheel rather
/// than a list with ends. There is no first item and no last one, so a reader
/// spinning past midnight arrives at one o'clock rather than at a stop.
pub struct ListWheelChildLoopingListDelegate {
    pub children: Vec<Rc<dyn Fn() -> AnyWidget>>,
}

impl ListWheelChildLoopingListDelegate {
    pub fn new(children: Vec<Rc<dyn Fn() -> AnyWidget>>) -> ListWheelChildLoopingListDelegate {
        ListWheelChildLoopingListDelegate { children }
    }

    /// Upstream's `shouldRebuild`.
    pub fn should_rebuild(&self, old: &ListWheelChildLoopingListDelegate) -> bool {
        self.children.len() != old.children.len()
            || self
                .children
                .iter()
                .zip(old.children.iter())
                .any(|(a, b)| !Rc::ptr_eq(a, b))
    }
}

impl ListWheelChildDelegate for ListWheelChildLoopingListDelegate {
    fn build(&self, index: i64) -> Option<AnyWidget> {
        if self.children.is_empty() {
            return None;
        }
        let at = self.true_index_of(index) as usize;
        Some(self.children[at]())
    }

    fn estimated_child_count(&self) -> Option<usize> {
        None
    }

    /// Upstream's `index % children.length`, and **the remainder has to be the
    /// non-negative one**.
    ///
    /// Dart's `%` on a negative left operand returns a non-negative result;
    /// Rust's `%` keeps the sign of the dividend, so `-1 % 5` is `-1` here and
    /// `4` there. A wheel scrolled up from its first item asks for exactly
    /// those negative indices, and the difference between the two operators is
    /// the difference between showing the last item and indexing out of
    /// bounds. `rem_euclid` is Dart's `%`.
    fn true_index_of(&self, index: i64) -> i64 {
        if self.children.is_empty() {
            return 0;
        }
        index.rem_euclid(self.children.len() as i64)
    }
}

/// Upstream `ListWheelChildBuilderDelegate`: children built on demand.
///
/// With a `child_count` the range is known and asking outside it gives
/// nothing. Without one the range is not known, and upstream's rule is that
/// **the builder returning `None` ends the run** -- so the count is discovered
/// by walking off the end of it.
pub struct ListWheelChildBuilderDelegate {
    pub builder: Rc<dyn Fn(i64) -> Option<AnyWidget>>,
    pub child_count: Option<usize>,
}

impl ListWheelChildBuilderDelegate {
    pub fn new(builder: Rc<dyn Fn(i64) -> Option<AnyWidget>>) -> ListWheelChildBuilderDelegate {
        ListWheelChildBuilderDelegate {
            builder,
            child_count: None,
        }
    }

    pub fn with_child_count(mut self, child_count: usize) -> Self {
        self.child_count = Some(child_count);
        self
    }

    /// Upstream's `shouldRebuild`.
    pub fn should_rebuild(&self, old: &ListWheelChildBuilderDelegate) -> bool {
        !Rc::ptr_eq(&self.builder, &old.builder) || self.child_count != old.child_count
    }
}

impl ListWheelChildDelegate for ListWheelChildBuilderDelegate {
    fn build(&self, index: i64) -> Option<AnyWidget> {
        match self.child_count {
            None => (self.builder)(index),
            Some(count) => {
                if index < 0 || index as usize >= count {
                    return None;
                }
                (self.builder)(index)
            }
        }
    }

    fn estimated_child_count(&self) -> Option<usize> {
        self.child_count
    }
}

/// Upstream's `_clipOffsetToScrollableRange`.
pub fn clip_offset_to_scrollable_range(
    offset: f32,
    min_scroll_extent: f32,
    max_scroll_extent: f32,
) -> f32 {
    offset.max(min_scroll_extent).min(max_scroll_extent)
}

/// Upstream's `_getItemFromOffset`: which item an offset is nearest.
///
/// The clamp comes **before** the division, so an overscrolled wheel -- one
/// dragged past its last item and still held -- reports the last item rather
/// than one past it. The reader is looking at the last item; that is the one
/// the wheel is on.
pub fn item_from_offset(
    offset: f32,
    item_extent: f32,
    min_scroll_extent: f32,
    max_scroll_extent: f32,
) -> i64 {
    (clip_offset_to_scrollable_range(offset, min_scroll_extent, max_scroll_extent) / item_extent)
        .round() as i64
}

/// Upstream `FixedExtentMetrics`: a scroll position's numbers, plus which item
/// it is on.
///
/// Upstream's own comment is worth keeping: the `FixedExtent` in the name is
/// about the *items* all being the same size, and the `Fixed` in its parent
/// `FixedScrollMetrics` is about the snapshot being immutable. Two different
/// words spelt the same way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedExtentMetrics {
    pub min_scroll_extent: f32,
    pub max_scroll_extent: f32,
    pub pixels: f32,
    pub viewport_dimension: f32,
    pub axis_direction: AxisDirection,
    pub item_index: i64,
    pub device_pixel_ratio: f32,
}

impl FixedExtentMetrics {
    pub fn new(
        min_scroll_extent: f32,
        max_scroll_extent: f32,
        pixels: f32,
        viewport_dimension: f32,
        item_index: i64,
    ) -> FixedExtentMetrics {
        FixedExtentMetrics {
            min_scroll_extent,
            max_scroll_extent,
            pixels,
            viewport_dimension,
            axis_direction: AxisDirection::Down,
            item_index,
            device_pixel_ratio: 1.0,
        }
    }

    pub fn with_axis_direction(mut self, axis_direction: AxisDirection) -> Self {
        self.axis_direction = axis_direction;
        self
    }

    pub fn with_device_pixel_ratio(mut self, device_pixel_ratio: f32) -> Self {
        self.device_pixel_ratio = device_pixel_ratio;
        self
    }

    /// Upstream's `copyWith`.
    pub fn copy_with(&self, pixels: Option<f32>, item_index: Option<i64>) -> FixedExtentMetrics {
        FixedExtentMetrics {
            pixels: pixels.unwrap_or(self.pixels),
            item_index: item_index.unwrap_or(self.item_index),
            ..*self
        }
    }

    /// Upstream's `ScrollPhysics.toleranceFor`.
    ///
    /// **The tolerances are in device pixels, not logical ones.** A denser
    /// screen settles more precisely, because "close enough to stop" ought to
    /// mean "closer than the reader can see", and what the reader can see is a
    /// physical pixel.
    pub fn tolerance(&self) -> Tolerance {
        Tolerance {
            distance: 1.0 / self.device_pixel_ratio,
            time: Tolerance::DEFAULT.time,
            velocity: 1.0 / (0.050 * self.device_pixel_ratio),
        }
    }
}

/// Upstream `FixedExtentScrollController`: a controller that counts in items
/// rather than in pixels.
///
/// Upstream this owns a registry of attached positions; this crate has no
/// `ScrollController`, so the methods take the [`Scroll`] they act on. What
/// they do to it is upstream's arithmetic exactly, which is the part worth
/// having: the offset of item *n* is *n* times the item extent, so an item
/// index and a scroll offset are the same number in different units.
pub struct FixedExtentScrollController {
    /// Upstream's `initialItem`, defaulting to zero.
    pub initial_item: i64,
    pub item_extent: f32,
}

impl FixedExtentScrollController {
    pub fn new(item_extent: f32) -> FixedExtentScrollController {
        FixedExtentScrollController {
            initial_item: 0,
            item_extent,
        }
    }

    pub fn with_initial_item(mut self, initial_item: i64) -> Self {
        self.initial_item = initial_item;
        self
    }

    /// Where a fresh wheel starts, upstream's
    /// `_FixedExtentScrollPosition`'s `initialPixels`.
    pub fn initial_scroll_offset(&self) -> f32 {
        self.offset_for_item(self.initial_item)
    }

    /// Upstream's `itemIndex * itemExtent`.
    pub fn offset_for_item(&self, item_index: i64) -> f32 {
        item_index as f32 * self.item_extent
    }

    /// Upstream's `selectedItem`, which reads its position's `itemIndex`.
    pub fn selected_item(&self, metrics: &FixedExtentMetrics) -> i64 {
        item_from_offset(
            metrics.pixels,
            self.item_extent,
            metrics.min_scroll_extent,
            metrics.max_scroll_extent,
        )
    }

    /// Upstream's `jumpToItem`, whose documentation says plainly that it does
    /// **not** check the index is in range.
    pub fn jump_to_item(&self, scroll: &mut Scroll, item_index: i64) {
        scroll.jump_to(self.offset_for_item(item_index));
    }

    /// Upstream's `animateToItem`.
    pub fn animate_to_item(
        &self,
        scroll: &mut Scroll,
        item_index: i64,
        duration_micros: i64,
        curve: Curve,
    ) {
        scroll.animate_to(self.offset_for_item(item_index), duration_micros, curve);
    }
}

/// Upstream's `ScrollPhysics._kDefaultSpring`.
pub fn default_scroll_spring() -> SpringDescription {
    SpringDescription::with_damping_ratio(0.5, 100.0, 1.1)
}

/// Upstream `ClampingScrollPhysics.createBallisticSimulation`, which is the
/// parent [`FixedExtentScrollPhysics`] defers to.
///
/// Every platform this crate runs on is a clamping one -- `physics.rs` says so
/// in its module docs -- so the parent is this rather than a choice.
fn parent_ballistic(metrics: &FixedExtentMetrics, velocity: f32) -> Option<Box<dyn Simulation>> {
    let tolerance = metrics.tolerance();
    if metrics.pixels > metrics.max_scroll_extent {
        return Some(Box::new(SpringSimulation::with_tolerance(
            default_scroll_spring(),
            metrics.pixels,
            metrics.max_scroll_extent,
            velocity.min(0.0),
            tolerance,
        )));
    }
    if metrics.pixels < metrics.min_scroll_extent {
        return Some(Box::new(SpringSimulation::with_tolerance(
            default_scroll_spring(),
            metrics.pixels,
            metrics.min_scroll_extent,
            velocity.min(0.0),
            tolerance,
        )));
    }
    if velocity.abs() < tolerance.velocity {
        return None;
    }
    if velocity > 0.0 && metrics.pixels >= metrics.max_scroll_extent {
        return None;
    }
    if velocity < 0.0 && metrics.pixels <= metrics.min_scroll_extent {
        return None;
    }
    Some(Box::new(crate::physics::ClampingScrollSimulation::new(
        metrics.pixels,
        velocity,
    )))
}

/// Upstream `FixedExtentScrollPhysics`: a fling that always lands on an item.
///
/// Upstream's own description is the clearest one: it behaves like a slot
/// machine wheel, except that it never overshoots and rolls back within a
/// single item to settle on it.
#[derive(Clone, Copy, Debug, Default)]
pub struct FixedExtentScrollPhysics;

impl FixedExtentScrollPhysics {
    pub fn new() -> FixedExtentScrollPhysics {
        FixedExtentScrollPhysics
    }

    /// Upstream's `createBallisticSimulation`, all five of its scenarios.
    ///
    /// The shape of the decision is: work out where an ordinary fling *would*
    /// have stopped, find the item nearest that point, and then get to that
    /// item's offset by whichever means suits the distance. Three of the five
    /// scenarios are about handing the problem back rather than solving it,
    /// which is the interesting part -- snapping to items is wrong at the ends
    /// of the list, where the boundary matters more than the grid.
    pub fn create_ballistic_simulation(
        &self,
        metrics: &FixedExtentMetrics,
        item_extent: f32,
        velocity: f32,
    ) -> Option<Box<dyn Simulation>> {
        let tolerance = metrics.tolerance();

        // Scenario 1: out of range and not heading back in. The parent puts us
        // back at the boundary, which is a better place to be than on the
        // nearest item.
        if (velocity <= 0.0 && metrics.pixels <= metrics.min_scroll_extent)
            || (velocity >= 0.0 && metrics.pixels >= metrics.max_scroll_extent)
        {
            return parent_ballistic(metrics, velocity);
        }

        // Where an ordinary fling would have come to rest.
        let test = parent_ballistic(metrics, velocity);
        let natural_stop = test.as_ref().map(|test| test.x(f32::INFINITY));

        // Scenario 2: that fling would have run off the end, so again the
        // boundary wins over the grid.
        if let Some(stop) = natural_stop {
            if stop == metrics.min_scroll_extent || stop == metrics.max_scroll_extent {
                return parent_ballistic(metrics, velocity);
            }
        }

        let settling_item = item_from_offset(
            natural_stop.unwrap_or(metrics.pixels),
            item_extent,
            metrics.min_scroll_extent,
            metrics.max_scroll_extent,
        );
        let settling_pixels = settling_item as f32 * item_extent;

        // Scenario 3: standing still where it means to stand. Nothing to do,
        // and returning a simulation anyway would be a frame of work per frame
        // forever.
        if velocity.abs() < tolerance.velocity
            && (settling_pixels - metrics.pixels).abs() < tolerance.distance
        {
            return None;
        }

        // Scenario 4: not enough velocity to break out of the current item, so
        // it rolls back. A spring, because the motion reverses -- friction
        // only ever goes the way it was already going.
        if settling_item == metrics.item_index {
            return Some(Box::new(SpringSimulation::with_tolerance(
                default_scroll_spring(),
                metrics.pixels,
                settling_pixels,
                velocity,
                tolerance,
            )));
        }

        // Scenario 5: an ordinary fling, with its drag tuned so that it stops
        // exactly on the item nearest where it would have stopped anyway.
        Some(Box::new(FrictionSimulation::through(
            metrics.pixels,
            settling_pixels,
            velocity,
            tolerance.velocity * velocity.signum(),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Empty;
    use std::cell::RefCell;

    fn a_child() -> Rc<dyn Fn() -> AnyWidget> {
        Rc::new(|| crate::framework::leaf(|| Empty))
    }

    fn children(count: usize) -> Vec<Rc<dyn Fn() -> AnyWidget>> {
        (0..count).map(|_| a_child()).collect()
    }

    #[test]
    fn scrolling_up_from_the_first_item_of_a_looping_wheel_reaches_the_last() {
        // Dart's `%` returns a non-negative remainder; Rust's keeps the sign
        // of the dividend, so `-1 % 5` is 4 there and -1 here. A wheel dragged
        // upwards from its first item asks for exactly those negative indices,
        // and the difference between the two operators is the difference
        // between showing the last item and indexing out of bounds.
        let wheel = ListWheelChildLoopingListDelegate::new(children(5));
        assert_eq!(wheel.true_index_of(-1), 4);
        assert_eq!(wheel.true_index_of(-5), 0);
        assert_eq!(wheel.true_index_of(-6), 4);
        assert_eq!(wheel.true_index_of(0), 0);
        assert_eq!(wheel.true_index_of(7), 2);
        // And it really builds something there rather than falling off.
        assert!(wheel.build(-1).is_some());
        assert!(wheel.build(1_000_000).is_some());
    }

    #[test]
    fn a_looping_wheel_has_no_count_and_a_plain_list_does() {
        // Which is how the wheel knows it has no ends: an unknown count is
        // upstream's way of saying endless, and the wheel stops drawing a
        // boundary because there is none to draw.
        assert_eq!(
            ListWheelChildLoopingListDelegate::new(children(5)).estimated_child_count(),
            None
        );
        assert_eq!(
            ListWheelChildListDelegate::new(children(5)).estimated_child_count(),
            Some(5)
        );
    }

    #[test]
    fn an_empty_looping_list_has_nothing_to_show() {
        // Rather than dividing by zero, which is what the modulo would do.
        let empty = ListWheelChildLoopingListDelegate::new(Vec::new());
        assert!(empty.build(0).is_none());
        assert!(empty.build(-3).is_none());
    }

    #[test]
    fn a_plain_list_delegate_has_both_ends() {
        let list = ListWheelChildListDelegate::new(children(3));
        assert!(list.build(-1).is_none());
        assert!(list.build(0).is_some());
        assert!(list.build(2).is_some());
        assert!(list.build(3).is_none());
    }

    #[test]
    fn a_builder_without_a_count_is_ended_by_the_builder_saying_no() {
        // Upstream's rule for the unbounded case: the builder must supply a
        // contiguous run, and returning null terminates it. The count is
        // discovered by walking off the end rather than declared.
        let asked = Rc::new(RefCell::new(Vec::new()));
        let seen = asked.clone();
        let delegate = ListWheelChildBuilderDelegate::new(Rc::new(move |index| {
            seen.borrow_mut().push(index);
            if (0..4).contains(&index) {
                Some(crate::framework::leaf(|| Empty))
            } else {
                None
            }
        }));
        assert!(delegate.build(3).is_some());
        assert!(delegate.build(4).is_none());
        assert_eq!(delegate.estimated_child_count(), None);
        assert_eq!(*asked.borrow(), vec![3, 4]);
    }

    #[test]
    fn a_builder_with_a_count_is_never_asked_outside_it() {
        // The bounded case answers from the count without troubling the
        // builder, which matters because a builder is free to be expensive.
        let asked = Rc::new(RefCell::new(Vec::new()));
        let seen = asked.clone();
        let delegate = ListWheelChildBuilderDelegate::new(Rc::new(move |index| {
            seen.borrow_mut().push(index);
            Some(crate::framework::leaf(|| Empty))
        }))
        .with_child_count(2);

        assert!(delegate.build(1).is_some());
        assert!(delegate.build(2).is_none());
        assert!(delegate.build(-1).is_none());
        assert_eq!(delegate.estimated_child_count(), Some(2));
        assert_eq!(*asked.borrow(), vec![1], "only the one in range");
    }

    #[test]
    fn a_delegate_rebuilds_when_its_children_change_and_not_otherwise() {
        let shared = children(3);
        let same = ListWheelChildListDelegate::new(shared.clone());
        assert!(!same.should_rebuild(&ListWheelChildListDelegate::new(shared.clone())));
        assert!(same.should_rebuild(&ListWheelChildListDelegate::new(children(3))));
        assert!(same.should_rebuild(&ListWheelChildListDelegate::new(children(4))));

        let builder: Rc<dyn Fn(i64) -> Option<AnyWidget>> = Rc::new(|_| None);
        let one = ListWheelChildBuilderDelegate::new(builder.clone()).with_child_count(2);
        assert!(!one.should_rebuild(
            &ListWheelChildBuilderDelegate::new(builder.clone()).with_child_count(2)
        ));
        assert!(one.should_rebuild(
            &ListWheelChildBuilderDelegate::new(builder.clone()).with_child_count(3)
        ));
    }

    #[test]
    fn an_overscrolled_wheel_reports_the_last_item_and_not_one_past_it() {
        // The clamp happens before the division. A reader holding the wheel
        // dragged past its end is looking at the last item, and that is the
        // item the wheel is on.
        let extent = 40.0;
        assert_eq!(item_from_offset(0.0, extent, 0.0, 120.0), 0);
        assert_eq!(item_from_offset(59.0, extent, 0.0, 120.0), 1);
        assert_eq!(item_from_offset(61.0, extent, 0.0, 120.0), 2);
        assert_eq!(item_from_offset(400.0, extent, 0.0, 120.0), 3);
        assert_eq!(item_from_offset(-400.0, extent, 0.0, 120.0), 0);
    }

    #[test]
    fn the_settling_tolerances_are_in_device_pixels() {
        // "Close enough to stop" ought to mean "closer than the reader can
        // see", and what the reader can see is a physical pixel -- so a denser
        // screen settles more precisely.
        let coarse = FixedExtentMetrics::new(0.0, 100.0, 0.0, 50.0, 0).with_device_pixel_ratio(1.0);
        let dense = FixedExtentMetrics::new(0.0, 100.0, 0.0, 50.0, 0).with_device_pixel_ratio(3.0);
        assert!(dense.tolerance().distance < coarse.tolerance().distance);
        assert!(dense.tolerance().velocity < coarse.tolerance().velocity);
        assert!((coarse.tolerance().distance - 1.0).abs() < 1e-6);
        assert!((coarse.tolerance().velocity - 20.0).abs() < 1e-4);
    }

    #[test]
    fn an_item_index_and_a_scroll_offset_are_the_same_number_in_different_units() {
        let controller = FixedExtentScrollController::new(40.0).with_initial_item(3);
        assert_eq!(controller.initial_scroll_offset(), 120.0);
        assert_eq!(controller.offset_for_item(0), 0.0);
        assert_eq!(controller.offset_for_item(-2), -80.0);

        let metrics = FixedExtentMetrics::new(0.0, 400.0, 99.0, 100.0, 0);
        assert_eq!(controller.selected_item(&metrics), 2, "99 is nearest 80");
    }

    #[test]
    fn jumping_to_an_item_does_not_check_the_range() {
        // Upstream's documentation says so in as many words, and it is not an
        // oversight: the wheel's extent is settled at layout, so a jump made
        // before anything has been measured has nothing to be checked against.
        let controller = FixedExtentScrollController::new(40.0);
        let mut scroll = Scroll::new();
        controller.jump_to_item(&mut scroll, 10);
        assert_eq!(scroll.offset, 400.0);
        // And layout is what brings it back.
        scroll.set_extent(120.0, 40.0);
        assert_eq!(scroll.offset, 120.0);
    }

    #[test]
    fn a_friction_simulation_can_be_asked_where_to_stop_instead_of_how_hard_to_brake() {
        // FrictionSimulation::through, the piece CupertinoPicker's docs have
        // been recording as missing. It stops when its velocity has decayed to
        // the end velocity it was given, and that is where the target is.
        let simulation = FrictionSimulation::through(0.0, 200.0, 1000.0, 20.0);
        assert!(simulation.x(0.0).abs() < 1e-3);
        assert!((simulation.dx(0.0) - 1000.0).abs() < 1e-2);

        let stop = (0..20_000)
            .map(|step| step as f32 / 1000.0)
            .find(|time| simulation.is_done(*time))
            .expect("it comes to rest");
        assert!(
            (simulation.x(stop) - 200.0).abs() < 1.0,
            "landed at {} rather than 200",
            simulation.x(stop)
        );
    }

    fn wheel_metrics(pixels: f32, item_extent: f32, items: i64) -> FixedExtentMetrics {
        let max = (items - 1) as f32 * item_extent;
        FixedExtentMetrics::new(
            0.0,
            max,
            pixels,
            item_extent,
            item_from_offset(pixels, item_extent, 0.0, max),
        )
    }

    #[test]
    fn a_wheel_standing_where_it_means_to_stand_is_given_nothing_to_do() {
        // Scenario 3. Returning a simulation anyway would be a frame of work
        // every frame, forever, to hold still.
        let physics = FixedExtentScrollPhysics::new();
        let metrics = wheel_metrics(80.0, 40.0, 10);
        assert!(
            physics
                .create_ballistic_simulation(&metrics, 40.0, 0.0)
                .is_none()
        );
    }

    #[test]
    fn a_flick_too_weak_to_leave_its_item_rolls_back_to_it() {
        // Scenario 4, and it is a spring rather than friction because the
        // motion has to reverse -- friction only ever carries on the way it
        // was already going.
        let physics = FixedExtentScrollPhysics::new();
        let metrics = wheel_metrics(85.0, 40.0, 10);
        assert_eq!(metrics.item_index, 2);
        let simulation = physics
            .create_ballistic_simulation(&metrics, 40.0, 30.0)
            .expect("it has somewhere to go");
        assert!((simulation.x(0.0) - 85.0).abs() < 1e-2);
        let settled = simulation.x(5.0);
        assert!(
            (settled - 80.0).abs() < 1.0,
            "settled at {settled} rather than back on item 2"
        );
    }

    #[test]
    fn a_real_fling_lands_exactly_on_an_item() {
        // Scenario 5, which is the whole point of the physics: whatever the
        // reader's flick was worth, the wheel stops on a whole item.
        let physics = FixedExtentScrollPhysics::new();
        let extent = 40.0;
        let metrics = wheel_metrics(0.0, extent, 100);
        let simulation = physics
            .create_ballistic_simulation(&metrics, extent, 900.0)
            .expect("a fling goes somewhere");

        let stop = (0..20_000)
            .map(|step| step as f32 / 1000.0)
            .find(|time| simulation.is_done(*time))
            .expect("it comes to rest");
        let landed = simulation.x(stop);
        let off_grid = (landed / extent - (landed / extent).round()).abs() * extent;
        // An ordinary fling of the same flick would have stopped at 161.83,
        // which is 1.83 short of item 4 -- so this threshold is what separates
        // the tuned drag from the untuned one, not a formality.
        assert!(
            off_grid < 0.5,
            "stopped at {landed}, which is {off_grid} away from an item"
        );
        assert!(landed > 40.0, "and it actually travelled");
    }

    #[test]
    fn at_the_ends_the_boundary_wins_over_the_grid() {
        // Scenarios 1 and 2. Snapping to the nearest item is the wrong answer
        // where the list runs out: the reader has to be put back inside the
        // list, and that is the parent physics' job.
        let physics = FixedExtentScrollPhysics::new();
        let extent = 40.0;

        // Dragged past the end and released outwards: comes back to the end.
        let past_the_end = FixedExtentMetrics::new(0.0, 120.0, 160.0, 40.0, 3);
        let simulation = physics
            .create_ballistic_simulation(&past_the_end, extent, 200.0)
            .expect("it must come back");
        assert!(
            (simulation.x(5.0) - 120.0).abs() < 1.0,
            "came to rest at {} rather than the end",
            simulation.x(5.0)
        );

        // A fling hard enough to run off the end lands on the end, not on the
        // last item's neighbour.
        let near_the_end = wheel_metrics(80.0, extent, 4);
        let simulation = physics
            .create_ballistic_simulation(&near_the_end, extent, 4000.0)
            .expect("a hard fling goes somewhere");
        let stop = (0..20_000)
            .map(|step| step as f32 / 1000.0)
            .find(|time| simulation.is_done(*time))
            .unwrap_or(10.0);
        assert!(
            simulation.x(stop) <= 120.0 + 1.0,
            "ran past the end to {}",
            simulation.x(stop)
        );
    }

    #[test]
    fn metrics_copy_only_what_they_are_given() {
        let metrics = FixedExtentMetrics::new(0.0, 400.0, 40.0, 100.0, 1)
            .with_axis_direction(AxisDirection::Right)
            .with_device_pixel_ratio(2.0);
        let moved = metrics.copy_with(Some(80.0), Some(2));
        assert_eq!(moved.pixels, 80.0);
        assert_eq!(moved.item_index, 2);
        assert_eq!(moved.axis_direction, AxisDirection::Right);
        assert_eq!(moved.device_pixel_ratio, 2.0);
        assert_eq!(metrics.copy_with(None, None), metrics);
    }
}
