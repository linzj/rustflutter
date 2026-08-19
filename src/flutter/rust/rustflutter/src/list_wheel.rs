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
use crate::render::{
    AxisDirection, BoxConstraints, BoxedRender, HitTestResult, Offset, PaintContext, RenderBox,
    Size,
};
use crate::scrolling::Scroll;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
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

/// Upstream's `RenderListWheelViewport.defaultDiameterRatio`.
pub const DEFAULT_DIAMETER_RATIO: f32 = 2.0;

/// Upstream's `RenderListWheelViewport.defaultPerspective`. An arbitrary but
/// aesthetically reasonable value, in upstream's own words.
pub const DEFAULT_PERSPECTIVE: f32 = 0.003;

/// Upstream's `RenderListWheelViewport.diameterRatioZeroMessage`, kept because
/// it says *why* zero is refused rather than merely that it is: a cylinder of
/// zero diameter has nothing to draw on.
pub const DIAMETER_RATIO_ZERO_MESSAGE: &str = "You can't set a diameterRatio of 0 or of a negative      number. It would imply a cylinder of 0 in diameter in which case nothing will be drawn.";

/// Upstream's `RenderListWheelViewport.perspectiveTooHighMessage`.
pub const PERSPECTIVE_TOO_HIGH_MESSAGE: &str = "A perspective too high will be clipped in the      z-axis and therefore not renderable. Value must be between 0 and 0.01.";

/// Upstream's
/// `RenderListWheelViewport.clipBehaviorAndRenderChildrenOutsideViewportConflict`.
pub const CLIP_AND_RENDER_OUTSIDE_CONFLICT: &str = "Cannot renderChildrenOutsideViewport and clip since children rendered outside will be      clipped anyway.";

// -- The three widgets --------------------------------------------------------

/// Upstream `ListWheelElement`: the thing that decides which children exist and
/// keeps the live ones contiguous.
///
/// Upstream this is an `Element` that also implements `ListWheelChildManager`,
/// and the manager half is the part with rules in it. This crate has no
/// elements, so what is ported is the manager: **a widget is built once per
/// index and remembered, a rebuild forgets everything, and asking whether a
/// child exists is answered by building it** -- which is how the end of an
/// unbounded builder's run gets discovered during layout rather than declared
/// in advance.
pub struct ListWheelElement {
    delegate: Rc<dyn ListWheelChildDelegate>,
    /// Upstream's `_childWidgets`, split in two because `AnyWidget` cannot be
    /// cloned: what a child *is* can only be handed out once, but **whether**
    /// there is one at an index is the answer the render object asks for over
    /// and over during layout, and that is what must not cost a rebuild each
    /// time.
    exists: RefCell<HashMap<i64, bool>>,
    built: RefCell<HashMap<i64, AnyWidget>>,
    /// Upstream's `_childElements`, a sorted map. Sorted because the invariant
    /// below is about order.
    active: RefCell<BTreeSet<i64>>,
    /// How many times the delegate was actually asked. Upstream's cache exists
    /// to keep this down; counting it is how the regression lines check that
    /// it does.
    builds: Cell<usize>,
}

impl ListWheelElement {
    pub fn new(delegate: Rc<dyn ListWheelChildDelegate>) -> ListWheelElement {
        ListWheelElement {
            delegate,
            exists: RefCell::new(HashMap::new()),
            built: RefCell::new(HashMap::new()),
            active: RefCell::new(BTreeSet::new()),
            builds: Cell::new(0),
        }
    }

    /// Upstream's `childCount`, straight from the delegate: `None` for a wheel
    /// with no ends.
    pub fn child_count(&self) -> Option<usize> {
        self.delegate.estimated_child_count()
    }

    /// How many times the delegate has been asked to build since the last
    /// [`Self::perform_rebuild`].
    pub fn delegate_builds(&self) -> usize {
        self.builds.get()
    }

    fn ensure(&self, index: i64) {
        if self.exists.borrow().contains_key(&index) {
            return;
        }
        self.builds.set(self.builds.get() + 1);
        let child = self.delegate.build(index);
        self.exists.borrow_mut().insert(index, child.is_some());
        if let Some(child) = child {
            self.built.borrow_mut().insert(index, child);
        }
    }

    /// Upstream's `childExistsAt`, which is `retrieveWidget(index) != null`.
    ///
    /// The render object asks this while walking outwards from the centre, and
    /// the first `false` is where the wheel stops. For a bounded delegate the
    /// answer is arithmetic; for an unbounded builder it is the builder's own
    /// verdict, which is why the answer has to come from building.
    pub fn child_exists_at(&self, index: i64) -> bool {
        self.ensure(index);
        self.exists.borrow()[&index]
    }

    /// Upstream's `createChild`: bring index into the live set and hand back
    /// its widget.
    ///
    /// `after` is the index this one follows, or `None` to insert first --
    /// upstream asserts that the predecessor is already live, because the live
    /// set is a contiguous run and inserting into a hole would break it.
    pub fn create_child(&self, index: i64, after: Option<i64>) -> Option<AnyWidget> {
        debug_assert!(
            after.is_none_or(|after| self.active.borrow().contains(&after)),
            "a child may only be created after one that is already live"
        );
        self.ensure(index);
        match self.built.borrow_mut().remove(&index) {
            Some(child) => {
                self.active.borrow_mut().insert(index);
                Some(child)
            }
            None => {
                if self.exists.borrow()[&index] {
                    // Handed out already and asked for again: build it afresh
                    // rather than hand out nothing. Upstream returns the same
                    // cached widget instance here; this crate's widgets are
                    // not shareable, so an equal one is the closest thing.
                    self.builds.set(self.builds.get() + 1);
                    let child = self.delegate.build(index);
                    if child.is_some() {
                        self.active.borrow_mut().insert(index);
                    }
                    child
                } else {
                    self.active.borrow_mut().remove(&index);
                    None
                }
            }
        }
    }

    /// Upstream's `removeChild`.
    pub fn remove_child(&self, index: i64) {
        self.active.borrow_mut().remove(&index);
    }

    /// The live indices, in order.
    pub fn active_indices(&self) -> Vec<i64> {
        self.active.borrow().iter().copied().collect()
    }

    /// Upstream's `performRebuild`.
    ///
    /// The cache is cleared -- that is the whole point of a rebuild -- and then
    /// the live run is walked from its first index to its last, dropping any
    /// index whose child has stopped existing. Walking the *span* rather than
    /// the set is upstream's own loop, and it is what makes a shrinking builder
    /// let go of its tail.
    pub fn perform_rebuild(&self) {
        self.exists.borrow_mut().clear();
        self.built.borrow_mut().clear();
        self.builds.set(0);
        let live = self.active_indices();
        let (Some(&first), Some(&last)) = (live.first(), live.last()) else {
            return;
        };
        for index in first..=last {
            if self.child_exists_at(index) {
                self.active.borrow_mut().insert(index);
            } else {
                self.active.borrow_mut().remove(&index);
            }
        }
    }

    /// Upstream's `moveRenderObjectChild`, which is an assertion that this
    /// never happens, with its message kept.
    ///
    /// The live set is a contiguous increasing run and everything above relies
    /// on it, so there is no such thing as moving one child within it.
    pub fn move_child(&self, _old_index: i64, _new_index: i64) {
        debug_assert!(
            false,
            "Currently we maintain the list in contiguous increasing order, so \
             moving children around is not allowed."
        );
    }
}

/// Upstream `ListWheelViewport`: the wheel's parameters, and the render object
/// they configure.
///
/// Upstream this is a `RenderObjectWidget` whose element is
/// [`ListWheelElement`]. Here it carries the same parameters, validates them
/// the same way, and builds the same render object.
pub struct ListWheelViewport {
    pub item_extent: f32,
    pub diameter_ratio: f32,
    pub perspective: f32,
    pub magnification: f32,
    pub use_magnifier: bool,
    pub over_and_under_center_opacity: f32,
    pub squeeze: f32,
    pub render_children_outside_viewport: bool,
    pub clip: bool,
}

impl ListWheelViewport {
    pub fn new(item_extent: f32) -> ListWheelViewport {
        ListWheelViewport {
            item_extent,
            diameter_ratio: DEFAULT_DIAMETER_RATIO,
            perspective: DEFAULT_PERSPECTIVE,
            magnification: 1.0,
            use_magnifier: false,
            over_and_under_center_opacity: 1.0,
            squeeze: 1.0,
            render_children_outside_viewport: false,
            clip: true,
        }
    }

    pub fn with_diameter_ratio(mut self, diameter_ratio: f32) -> Self {
        self.diameter_ratio = diameter_ratio;
        self
    }

    pub fn with_perspective(mut self, perspective: f32) -> Self {
        self.perspective = perspective;
        self
    }

    pub fn with_squeeze(mut self, squeeze: f32) -> Self {
        self.squeeze = squeeze;
        self
    }

    pub fn with_magnifier(mut self, magnification: f32) -> Self {
        self.use_magnifier = true;
        self.magnification = magnification;
        self
    }

    pub fn with_over_and_under_center_opacity(mut self, opacity: f32) -> Self {
        self.over_and_under_center_opacity = opacity;
        self
    }

    pub fn with_children_outside_viewport(mut self, render_outside: bool) -> Self {
        self.render_children_outside_viewport = render_outside;
        self
    }

    pub fn with_clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }

    /// Upstream's constructor assertions, gathered.
    ///
    /// Returns the message upstream would have asserted with, or `None` if the
    /// parameters are usable. They are kept as *messages* rather than as bare
    /// conditions because each one says why: a cylinder of zero diameter has
    /// nothing to draw on, a perspective over a hundredth is clipped away in
    /// z, and clipping a wheel that was asked to draw outside itself throws
    /// away exactly what it was asked for.
    pub fn validate(&self) -> Option<&'static str> {
        if self.diameter_ratio <= 0.0 {
            return Some(DIAMETER_RATIO_ZERO_MESSAGE);
        }
        if self.perspective <= 0.0 || self.perspective > 0.01 {
            return Some(PERSPECTIVE_TOO_HIGH_MESSAGE);
        }
        if self.magnification <= 0.0 {
            return Some("magnification must be positive");
        }
        if !(0.0..=1.0).contains(&self.over_and_under_center_opacity) {
            return Some("overAndUnderCenterOpacity must be between 0 and 1");
        }
        if self.item_extent <= 0.0 {
            return Some("itemExtent must be positive");
        }
        if self.squeeze <= 0.0 {
            return Some("squeeze must be positive");
        }
        if self.render_children_outside_viewport && self.clip {
            return Some(CLIP_AND_RENDER_OUTSIDE_CONFLICT);
        }
        None
    }

    /// How tall a slice of the wheel is live at a given viewport height:
    /// upstream's `performLayout`, which multiplies by the squeeze because a
    /// squeezed wheel packs more items into the same height.
    pub fn visible_extent(&self, viewport_height: f32) -> f32 {
        viewport_height * self.squeeze
    }

    /// The window of indices the wheel would lay out at this offset, before
    /// the delegate is consulted about which of them exist.
    pub fn visible_window(&self, offset: f32, viewport_height: f32) -> (i64, i64) {
        let visible = self.visible_extent(viewport_height);
        let half = self.item_extent / 2.0;
        (
            scroll_offset_to_index(offset + half - visible / 2.0, self.item_extent) as i64,
            scroll_offset_to_index(offset + half + visible / 2.0, self.item_extent) as i64,
        )
    }

    /// The render object this widget configures.
    pub fn render(
        &self,
        children: Vec<BoxedRender>,
        first_index: usize,
        offset: f32,
        viewport_sink: Rc<Cell<f32>>,
    ) -> RenderListWheel {
        RenderListWheel {
            children,
            first_index,
            item_extent: self.item_extent,
            offset,
            diameter_ratio: self.diameter_ratio,
            squeeze: self.squeeze,
            magnification: if self.use_magnifier {
                self.magnification
            } else {
                1.0
            },
            perspective: self.perspective,
            viewport_sink,
            laid_out: Size::ZERO,
        }
    }
}

/// Upstream `ListWheelScrollView`: the wheel, the scrolling and the reporting.
///
/// The reporting is the part with judgement in it, and it is ported here whole
/// -- see [`Self::report_selected`]. The scrolling itself is this crate's
/// [`Scroll`] driven by [`FixedExtentScrollPhysics`], and the drawing is
/// [`ListWheelViewport`].
pub struct ListWheelScrollView {
    pub viewport: ListWheelViewport,
    pub delegate: Rc<dyn ListWheelChildDelegate>,
    pub controller: FixedExtentScrollController,
    pub change_reporting_behavior: ChangeReportingBehavior,
    on_selected_item_changed: Option<Rc<dyn Fn(i64)>>,
}

impl ListWheelScrollView {
    pub fn new(
        viewport: ListWheelViewport,
        delegate: Rc<dyn ListWheelChildDelegate>,
    ) -> ListWheelScrollView {
        let item_extent = viewport.item_extent;
        ListWheelScrollView {
            viewport,
            delegate,
            controller: FixedExtentScrollController::new(item_extent),
            // Upstream's default, which is not the enum's first member: a
            // wheel that only spoke at the end of a scroll would leave a
            // caller redrawing a label one gesture late.
            change_reporting_behavior: ChangeReportingBehavior::OnScrollUpdate,
            on_selected_item_changed: None,
        }
    }

    pub fn with_controller(mut self, controller: FixedExtentScrollController) -> Self {
        self.controller = controller;
        self
    }

    pub fn with_change_reporting_behavior(mut self, behavior: ChangeReportingBehavior) -> Self {
        self.change_reporting_behavior = behavior;
        self
    }

    pub fn on_selected_item_changed(mut self, on_changed: impl Fn(i64) + 'static) -> Self {
        self.on_selected_item_changed = Some(Rc::new(on_changed));
        self
    }

    /// Where a fresh wheel starts, and what it will claim was already selected
    /// -- upstream's `initState`, which seeds `_lastReportedItemIndex` from the
    /// controller rather than from zero.
    ///
    /// Without that seeding a wheel opened on item seven would announce item
    /// seven the moment it was built, and a caller listening for a *change*
    /// would act on something the reader had not done.
    pub fn initial_reported_item(&self) -> i64 {
        self.controller.initial_item
    }

    /// Upstream's `_handleScrollNotification` and `_reportSelectedItemChanged`,
    /// together.
    ///
    /// `last_reported` is updated in place, and the true index is returned when
    /// there is something to report. Three rules, all upstream's:
    ///
    /// * the notification's kind has to match the configured behaviour -- an
    ///   update for `OnScrollUpdate`, an end for `OnScrollEnd`;
    /// * nothing is said unless the index actually changed;
    /// * and what is reported is the **true** index, through the delegate. A
    ///   looping wheel's scroll position counts upwards forever, so without
    ///   this a reader spinning past the end would be told they had selected
    ///   item 137 of a list of twelve.
    pub fn report_selected(
        &self,
        metrics: &FixedExtentMetrics,
        at_scroll_end: bool,
        last_reported: &mut i64,
    ) -> Option<i64> {
        let wanted = match self.change_reporting_behavior {
            ChangeReportingBehavior::OnScrollEnd => at_scroll_end,
            ChangeReportingBehavior::OnScrollUpdate => !at_scroll_end,
        };
        if !wanted || self.on_selected_item_changed.is_none() {
            return None;
        }
        if metrics.item_index == *last_reported {
            return None;
        }
        *last_reported = metrics.item_index;
        let true_index = self.delegate.true_index_of(metrics.item_index);
        if let Some(on_changed) = &self.on_selected_item_changed {
            on_changed(true_index);
        }
        Some(true_index)
    }

    /// The ballistic the wheel is given when a fling ends, from the physics
    /// ported above rather than from an ease-out standing in for it.
    pub fn ballistic(
        &self,
        metrics: &FixedExtentMetrics,
        velocity: f32,
    ) -> Option<Box<dyn Simulation>> {
        FixedExtentScrollPhysics::new().create_ballistic_simulation(
            metrics,
            self.viewport.item_extent,
            velocity,
        )
    }

    /// The metrics for a given scroll offset, which is what the reporting and
    /// the ballistic both read.
    ///
    /// **A wheel with no count has no floor either.** Upstream's
    /// `RenderListWheelViewport._minEstimatedScrollExtent` returns negative
    /// infinity when the child manager reports no count, and the maximum
    /// returns positive infinity -- so a looping wheel is unbounded in *both*
    /// directions. Giving it a floor of zero would stop it dead the first time
    /// a reader spun it upwards past its first item, which is precisely the
    /// thing a looping wheel exists to allow.
    pub fn metrics(
        &self,
        offset: f32,
        viewport_height: f32,
        item_count: Option<usize>,
    ) -> FixedExtentMetrics {
        let (min, max) = match item_count {
            Some(count) => (
                0.0,
                (count.saturating_sub(1)) as f32 * self.viewport.item_extent,
            ),
            None => (f32::NEG_INFINITY, f32::INFINITY),
        };
        FixedExtentMetrics::new(
            min,
            max,
            offset,
            viewport_height,
            item_from_offset(offset, self.viewport.item_extent, min, max),
        )
    }
}

// -- The wheel's geometry -----------------------------------------------------
//
// Moved here from `cupertino.rs`, where it was private to `CupertinoPicker`.
// Upstream it lives one layer below the widget that uses it, in
// `rendering/list_wheel_viewport.dart` and `painting/matrix_utils.dart`, and
// two widgets need it now rather than one -- upstream's `CupertinoPicker` is
// itself built on `ListWheelScrollView`.

/// The largest |angle| an item at the edge of the visible cylinder reaches.
/// `RenderListWheelViewport._maxVisibleRadian`.
pub(crate) fn max_visible_radian(diameter_ratio: f32) -> f32 {
    if diameter_ratio < 1.0 {
        std::f32::consts::FRAC_PI_2
    } else {
        (1.0 / diameter_ratio).asin()
    }
}

/// `RenderListWheelViewport.scrollOffsetToIndex` / `indexToScrollOffset`.
pub fn scroll_offset_to_index(offset: f32, item_extent: f32) -> i32 {
    (offset / item_extent).floor() as i32
}

pub fn index_to_scroll_offset(index: usize, item_extent: f32) -> f32 {
    index as f32 * item_extent
}

/// The angle a child is at, given where its center falls in the viewport.
/// `_paintTransformedChild`'s `angle` computation.
pub(crate) fn angle_for(flat_center_y: f32, height: f32, diameter_ratio: f32, squeeze: f32) -> f32 {
    let fractional_y = flat_center_y / height;
    -(fractional_y - 0.5) * 2.0 * max_visible_radian(diameter_ratio) / squeeze
}

/// Projects a point on the wheel's flat axis onto the screen, and reports the
/// child's horizontal scale there. This is
/// `MatrixUtils.createCylindricalProjectionTransform` (vertical orientation)
/// evaluated at the child's center: the model matrix translates z by the
/// radius and rotates by `angle` about x, the view steps back by the radius,
/// and the projection divides by `w = perspective * (radius - z) + 1`.
///
/// Returns `(screen_center_y, scale_x)`.
pub(crate) fn project_center(
    y_rel: f32,
    angle: f32,
    radius: f32,
    height: f32,
    perspective: f32,
) -> (f32, f32) {
    let (sin, cos) = angle.sin_cos();
    let y1 = y_rel * cos - radius * sin;
    let z1 = y_rel * sin + radius * cos;
    let w = perspective * (radius - z1) + 1.0;
    (height / 2.0 + y1 / w, 1.0 / w)
}

/// The vertical scale of a child at `y_rel`, sampled over a pixel rather than
/// derived: the projected slope has no tidy closed form once the perspective
/// divide is in, and the difference quotient is what the transform itself
/// would do to the child's top and bottom edges.
pub(crate) fn project_scale_y(
    y_rel: f32,
    angle: f32,
    radius: f32,
    height: f32,
    perspective: f32,
) -> f32 {
    let above = project_center(y_rel - 0.5, angle, radius, height, perspective).0;
    let below = project_center(y_rel + 0.5, angle, radius, height, perspective).0;
    below - above
}

/// Whether a child is wholly inside the magnifier band: its projected band
/// sits within `itemExtent * magnification / 2` of the viewport's center,
/// which is the band `_paintChildWithMagnifier` clips to. Upstream paints a
/// partially intersecting child twice -- once plain, once magnified and
/// clipped to the band; here the child is magnified only when wholly inside,
/// and dimmed otherwise, a stepwise version of the same ramp.
pub(crate) fn inside_magnifier_band(
    screen_center_y: f32,
    height: f32,
    item_extent: f32,
    magnification: f32,
) -> bool {
    (screen_center_y - height / 2.0).abs() + item_extent / 2.0 <= item_extent * magnification / 2.0
}

/// The wheel's render object: fixed-extent children laid out flat and painted
/// through the cylindrical projection. `RenderListWheelViewport`, reduced to
/// a vertical, non-looping wheel.
pub struct RenderListWheel {
    pub children: Vec<BoxedRender>,
    /// The index `children[0]` stands for.
    pub first_index: usize,
    pub item_extent: f32,
    pub offset: f32,
    pub diameter_ratio: f32,
    pub squeeze: f32,
    /// 1.0 when the magnifier is off.
    pub magnification: f32,
    /// Upstream's `perspective`. Was the picker's constant while this lived in
    /// `cupertino.rs`; upstream's render object has always had it as a
    /// parameter, and now that two widgets share the object it has to be one.
    pub perspective: f32,
    pub viewport_sink: Rc<Cell<f32>>,
    pub laid_out: Size,
}

impl RenderBox for RenderListWheel {
    /// `sizedByParent`: the wheel is exactly what it is offered.
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let width = if constraints.has_bounded_width() {
            constraints.max_width
        } else {
            constraints.min_width
        };
        let height = if constraints.has_bounded_height() {
            constraints.max_height
        } else {
            constraints.min_height
        };
        self.laid_out = Size::new(width, height);
        self.viewport_sink.set(height);
        for child in &mut self.children {
            // `_layoutChild`: the item extent, tight; the cross axis loose.
            child.layout_child(
                BoxConstraints::new(0.0, width, self.item_extent, self.item_extent),
                true,
            );
        }
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let height = self.laid_out.height;
        if height <= 0.0 {
            return;
        }
        let radius = height * self.diameter_ratio / 2.0;
        for (i, child) in self.children.iter().enumerate() {
            let index = self.first_index + i;
            let flat_center =
                index as f32 * self.item_extent + self.item_extent / 2.0 - self.offset;
            let angle = angle_for(flat_center, height, self.diameter_ratio, self.squeeze);
            // The backside of the cylinder is not painted.
            if angle.abs() > std::f32::consts::FRAC_PI_2 {
                continue;
            }
            let y_rel = flat_center - height / 2.0;
            let (screen_y, mut sx) = project_center(y_rel, angle, radius, height, self.perspective);
            let mut sy = project_scale_y(y_rel, angle, radius, height, self.perspective);
            if self.magnification > 1.0
                && inside_magnifier_band(screen_y, height, self.item_extent, self.magnification)
            {
                sx *= self.magnification;
                sy *= self.magnification;
            }
            let child_size = child.size();
            // Scale about the child's center, placed at its projected
            // position: `push_transform`'s pivot form.
            let pivot = Offset::new(child_size.width / 2.0, child_size.height / 2.0);
            let at = Offset::new(
                offset.dx + (self.laid_out.width - child_size.width) / 2.0,
                offset.dy + screen_y - child_size.height / 2.0,
            );
            context.push_transform([sx, 0.0, 0.0, sy, 0.0, 0.0], pivot, at, child);
        }
    }

    /// Hit testing works in flat coordinates: the cylindrical transform is a
    /// paint-time projection (upstream's `hitTest` would invert it, which the
    /// 2D affine bridge cannot), and the flat lookup is what the tap handler
    /// above uses, so the two agree.
    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        for (i, child) in self.children.iter().enumerate().rev() {
            let index = self.first_index + i;
            let child_offset = Offset::new(
                (self.laid_out.width - child.size().width) / 2.0,
                index as f32 * self.item_extent - self.offset,
            );
            let local = Offset::new(position.dx - child_offset.dx, position.dy - child_offset.dy);
            if child.hit_test(local, result) {
                return true;
            }
        }
        false
    }

    /// The wheel itself is a target even between items: the drag region is
    /// the whole viewport, as upstream's `ListWheelScrollView` is a
    /// scrollable everywhere.
    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        for (i, child) in self.children.iter().enumerate() {
            let index = self.first_index + i;
            visit(
                child,
                Offset::new(
                    (self.laid_out.width - child.size().width) / 2.0,
                    index as f32 * self.item_extent - self.offset,
                ),
            );
        }
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

    fn counting_builder(
        count: Option<usize>,
    ) -> (Rc<RefCell<Vec<i64>>>, Rc<dyn ListWheelChildDelegate>) {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let seen = asked.clone();
        let mut delegate = ListWheelChildBuilderDelegate::new(Rc::new(move |index| {
            seen.borrow_mut().push(index);
            Some(crate::framework::leaf(|| Empty))
        }));
        if let Some(count) = count {
            delegate = delegate.with_child_count(count);
        }
        (asked, Rc::new(delegate))
    }

    #[test]
    fn asking_twice_whether_a_child_exists_builds_it_once() {
        // What upstream's widget cache is for. The render object asks this
        // while walking outwards from the centre, repeatedly, and a delegate
        // is free to be expensive.
        let (asked, delegate) = counting_builder(Some(10));
        let element = ListWheelElement::new(delegate);
        assert!(element.child_exists_at(3));
        assert!(element.child_exists_at(3));
        assert!(element.child_exists_at(3));
        assert_eq!(*asked.borrow(), vec![3]);
        assert_eq!(element.delegate_builds(), 1);
    }

    #[test]
    fn the_end_of_an_unbounded_run_is_found_by_asking_rather_than_declared() {
        // A builder delegate with no count says where it ends by returning
        // nothing, and that is the answer the wheel stops at.
        let delegate: Rc<dyn ListWheelChildDelegate> =
            Rc::new(ListWheelChildBuilderDelegate::new(Rc::new(|index| {
                (0..4)
                    .contains(&index)
                    .then(|| crate::framework::leaf(|| Empty))
            })));
        let element = ListWheelElement::new(delegate);
        assert_eq!(element.child_count(), None, "the count is not known");
        assert!(element.child_exists_at(3));
        assert!(!element.child_exists_at(4));
    }

    #[test]
    fn a_looping_wheel_reports_no_count_at_all() {
        let element =
            ListWheelElement::new(Rc::new(ListWheelChildLoopingListDelegate::new(children(5))));
        assert_eq!(element.child_count(), None);
        assert!(element.child_exists_at(-40));
        assert!(element.child_exists_at(40));
    }

    #[test]
    fn a_rebuild_forgets_the_cache_and_lets_go_of_a_vanished_tail() {
        // Upstream's performRebuild walks the live *span* rather than the live
        // set, so an index whose child has stopped existing is dropped. A
        // builder whose count shrinks is exactly that case.
        let count = Rc::new(Cell::new(6usize));
        let live = count.clone();
        let delegate: Rc<dyn ListWheelChildDelegate> =
            Rc::new(ListWheelChildBuilderDelegate::new(Rc::new(move |index| {
                (index >= 0 && (index as usize) < live.get())
                    .then(|| crate::framework::leaf(|| Empty))
            })));
        let element = ListWheelElement::new(delegate);
        for index in 0..6 {
            assert!(
                element
                    .create_child(index, (index > 0).then_some(index - 1))
                    .is_some()
            );
        }
        assert_eq!(element.active_indices(), vec![0, 1, 2, 3, 4, 5]);

        count.set(3);
        // Without the rebuild the stale answers stand -- which is what the
        // cache is for and why clearing it is the first thing a rebuild does.
        assert!(element.child_exists_at(5));
        element.perform_rebuild();
        assert!(!element.child_exists_at(5));
        assert_eq!(element.active_indices(), vec![0, 1, 2]);
        assert_eq!(element.delegate_builds(), 6, "the whole span was rewalked");
    }

    #[test]
    fn a_child_that_stops_existing_is_not_made_live() {
        let delegate: Rc<dyn ListWheelChildDelegate> =
            Rc::new(ListWheelChildListDelegate::new(children(2)));
        let element = ListWheelElement::new(delegate);
        assert!(element.create_child(0, None).is_some());
        assert!(element.create_child(1, Some(0)).is_some());
        assert!(element.create_child(2, Some(1)).is_none());
        assert_eq!(element.active_indices(), vec![0, 1]);
        element.remove_child(1);
        assert_eq!(element.active_indices(), vec![0]);
    }

    #[test]
    fn the_viewport_refuses_the_parameters_upstream_refuses_and_says_why() {
        let good = ListWheelViewport::new(40.0);
        assert_eq!(good.validate(), None);

        assert_eq!(
            ListWheelViewport::new(40.0)
                .with_diameter_ratio(0.0)
                .validate(),
            Some(DIAMETER_RATIO_ZERO_MESSAGE)
        );
        assert_eq!(
            ListWheelViewport::new(40.0)
                .with_perspective(0.02)
                .validate(),
            Some(PERSPECTIVE_TOO_HIGH_MESSAGE)
        );
        assert_eq!(
            ListWheelViewport::new(0.0).validate(),
            Some("itemExtent must be positive")
        );
        assert_eq!(
            ListWheelViewport::new(40.0).with_squeeze(0.0).validate(),
            Some("squeeze must be positive")
        );
        // Drawing outside the viewport and clipping to it are each reasonable
        // and together are a contradiction: the clip throws away exactly what
        // the other asked for.
        assert_eq!(
            ListWheelViewport::new(40.0)
                .with_children_outside_viewport(true)
                .validate(),
            Some(CLIP_AND_RENDER_OUTSIDE_CONFLICT)
        );
        assert_eq!(
            ListWheelViewport::new(40.0)
                .with_children_outside_viewport(true)
                .with_clip(false)
                .validate(),
            None
        );
    }

    #[test]
    fn a_squeezed_wheel_keeps_more_items_alive_in_the_same_height() {
        // Which is what the squeeze means: the same viewport shows more of the
        // cylinder, so more indices have to be built.
        let plain = ListWheelViewport::new(40.0);
        let squeezed = ListWheelViewport::new(40.0).with_squeeze(1.45);
        assert!(squeezed.visible_extent(200.0) > plain.visible_extent(200.0));

        let (plain_first, plain_last) = plain.visible_window(0.0, 200.0);
        let (tight_first, tight_last) = squeezed.visible_window(0.0, 200.0);
        assert!(tight_first <= plain_first && tight_last >= plain_last);
    }

    #[test]
    fn a_fresh_wheel_does_not_announce_the_item_it_opened_on() {
        // Upstream seeds _lastReportedItemIndex from the controller. Without
        // it a wheel opened on item seven would report item seven at once, and
        // a caller listening for a change would act on something the reader
        // had not done.
        let delegate: Rc<dyn ListWheelChildDelegate> =
            Rc::new(ListWheelChildListDelegate::new(children(20)));
        let heard = Rc::new(RefCell::new(Vec::new()));
        let told = heard.clone();
        let wheel = ListWheelScrollView::new(ListWheelViewport::new(40.0), delegate)
            .with_controller(FixedExtentScrollController::new(40.0).with_initial_item(7))
            .on_selected_item_changed(move |index| told.borrow_mut().push(index));

        let mut last = wheel.initial_reported_item();
        assert_eq!(last, 7);
        let metrics = wheel.metrics(280.0, 200.0, Some(20));
        assert_eq!(metrics.item_index, 7);
        assert_eq!(wheel.report_selected(&metrics, false, &mut last), None);
        assert!(heard.borrow().is_empty());

        // Move one item and it does speak.
        let metrics = wheel.metrics(320.0, 200.0, Some(20));
        assert_eq!(wheel.report_selected(&metrics, false, &mut last), Some(8));
        assert_eq!(*heard.borrow(), vec![8]);
    }

    #[test]
    fn a_looping_wheel_reports_the_item_and_not_how_far_it_has_spun() {
        // The scroll position of a looping wheel counts upwards forever. What
        // the reader picked is the item, so the report goes through the
        // delegate's trueIndexOf -- otherwise a reader spinning past the end
        // is told they selected item 137 of a list of twelve.
        let delegate: Rc<dyn ListWheelChildDelegate> =
            Rc::new(ListWheelChildLoopingListDelegate::new(children(12)));
        let heard = Rc::new(RefCell::new(Vec::new()));
        let told = heard.clone();
        let wheel = ListWheelScrollView::new(ListWheelViewport::new(40.0), delegate)
            .on_selected_item_changed(move |index| told.borrow_mut().push(index));

        let mut last = 0;
        let metrics = wheel.metrics(137.0 * 40.0, 200.0, None);
        assert_eq!(metrics.item_index, 137);
        assert_eq!(wheel.report_selected(&metrics, false, &mut last), Some(5));
        assert_eq!(*heard.borrow(), vec![5], "137 mod 12");

        // And backwards past the start, where Dart's remainder matters again.
        let metrics = wheel.metrics(-1.0 * 40.0, 200.0, None);
        assert_eq!(wheel.report_selected(&metrics, false, &mut last), Some(11));
    }

    #[test]
    fn the_reporting_behavior_decides_which_moment_speaks() {
        let delegate: Rc<dyn ListWheelChildDelegate> =
            Rc::new(ListWheelChildListDelegate::new(children(20)));
        let build = |behavior| {
            ListWheelScrollView::new(ListWheelViewport::new(40.0), delegate.clone())
                .with_change_reporting_behavior(behavior)
                .on_selected_item_changed(|_| {})
        };

        let on_update = build(ChangeReportingBehavior::OnScrollUpdate);
        let metrics = on_update.metrics(200.0, 200.0, Some(20));
        let mut last = 0;
        assert_eq!(on_update.report_selected(&metrics, true, &mut last), None);
        assert_eq!(
            on_update.report_selected(&metrics, false, &mut last),
            Some(5)
        );

        let on_end = build(ChangeReportingBehavior::OnScrollEnd);
        let mut last = 0;
        assert_eq!(on_end.report_selected(&metrics, false, &mut last), None);
        assert_eq!(on_end.report_selected(&metrics, true, &mut last), Some(5));

        // Upstream's default is the talkative one.
        assert_eq!(
            ListWheelScrollView::new(ListWheelViewport::new(40.0), delegate)
                .change_reporting_behavior,
            ChangeReportingBehavior::OnScrollUpdate
        );
    }

    #[test]
    fn nothing_is_said_twice_for_the_same_item() {
        let delegate: Rc<dyn ListWheelChildDelegate> =
            Rc::new(ListWheelChildListDelegate::new(children(20)));
        let heard = Rc::new(RefCell::new(Vec::new()));
        let told = heard.clone();
        let wheel = ListWheelScrollView::new(ListWheelViewport::new(40.0), delegate)
            .on_selected_item_changed(move |index| told.borrow_mut().push(index));

        let mut last = 0;
        // Three offsets, all nearest item 5.
        for offset in [195.0, 200.0, 205.0] {
            let metrics = wheel.metrics(offset, 200.0, Some(20));
            wheel.report_selected(&metrics, false, &mut last);
        }
        assert_eq!(*heard.borrow(), vec![5]);
    }

    #[test]
    fn the_wheel_flings_on_the_physics_that_were_ported_for_it() {
        // Not the ease-out CupertinoPicker still uses: this goes through
        // FixedExtentScrollPhysics, so the landing is scenario 5's tuned
        // friction.
        let delegate: Rc<dyn ListWheelChildDelegate> =
            Rc::new(ListWheelChildListDelegate::new(children(100)));
        let wheel = ListWheelScrollView::new(ListWheelViewport::new(40.0), delegate);
        let metrics = wheel.metrics(0.0, 200.0, Some(100));
        let simulation = wheel
            .ballistic(&metrics, 900.0)
            .expect("a fling goes somewhere");
        let stop = (0..20_000)
            .map(|step| step as f32 / 1000.0)
            .find(|time| simulation.is_done(*time))
            .expect("it stops");
        let landed = simulation.x(stop);
        assert!((landed / 40.0 - (landed / 40.0).round()).abs() * 40.0 < 0.5);
    }
}
