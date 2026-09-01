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
use crate::engine::Rect;
use crate::framework::AnyWidget;
use crate::painting::ClipBehavior;
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
/// Upstream `ListWheelChildManager`: what the render object asks when it needs
/// to know how far the wheel goes and to get another child.
///
/// This is the seam between the element and the render object, and the reason
/// it is a seam is that the render object cannot know where the list ends. It
/// works outwards from the centre and asks, index by index, and the manager
/// answers -- from arithmetic for a bounded delegate, and from actually
/// building for a builder that only finds out by trying.
///
/// `child_count` of `None` is not "no children"; it is **no known limit**, and
/// upstream says so explicitly: the range is then whatever contiguous run
/// [`child_exists_at`](ListWheelChildManager::child_exists_at) keeps saying yes
/// to. A looping wheel answers `None` and yes to everything, which is how it
/// has no ends.
pub trait ListWheelChildManager {
    /// Upstream's `childCount`. `None` means no explicit limit -- see the
    /// trait's docs, because this is the member most easily read backwards.
    fn child_count(&self) -> Option<usize>;

    /// Upstream's `childExistsAt`. About whether the *delegate* can supply one,
    /// not about whether it is currently attached.
    fn child_exists_at(&self, index: i64) -> bool;

    /// Upstream's `createChild(index, after:)`. `after` is the index this one
    /// follows, and upstream asserts it is already live, because the live set
    /// is one contiguous run and inserting into a hole would break it.
    ///
    /// Upstream returns nothing and mutates the render object's child list;
    /// here the child is handed back, which is the same event said in the
    /// direction this crate's trees are built.
    fn create_child(&self, index: i64, after: Option<i64>) -> Option<AnyWidget>;

    /// Upstream's `removeChild`, which takes the `RenderBox`. Here the index
    /// identifies it, because that is what the live set is keyed by.
    fn remove_child(&self, index: i64);
}

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

/// [`ListWheelElement`] is upstream's `ListWheelChildManager` implementer --
/// upstream's element `implements ListWheelChildManager` on the same four
/// members.
impl ListWheelChildManager for ListWheelElement {
    fn child_count(&self) -> Option<usize> {
        ListWheelElement::child_count(self)
    }

    fn child_exists_at(&self, index: i64) -> bool {
        ListWheelElement::child_exists_at(self, index)
    }

    fn create_child(&self, index: i64, after: Option<i64>) -> Option<AnyWidget> {
        ListWheelElement::create_child(self, index, after)
    }

    fn remove_child(&self, index: i64) {
        ListWheelElement::remove_child(self, index)
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
    /// Upstream's `offAxisFraction` -- see
    /// [`RenderListWheelViewport::off_axis_fraction`].
    pub off_axis_fraction: f32,
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
            off_axis_fraction: 0.0,
            over_and_under_center_opacity: 1.0,
            squeeze: 1.0,
            render_children_outside_viewport: false,
            clip: true,
        }
    }

    /// Upstream's `offAxisFraction`, whose default is 0.0 -- the middle. See
    /// [`RenderListWheelViewport::off_axis_fraction`].
    pub fn with_off_axis_fraction(mut self, fraction: f32) -> Self {
        self.off_axis_fraction = fraction;
        self
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
    ) -> RenderListWheelViewport {
        RenderListWheelViewport {
            children,
            first_index,
            item_extent: self.item_extent,
            offset,
            diameter_ratio: self.diameter_ratio,
            squeeze: self.squeeze,
            use_magnifier: self.use_magnifier,
            magnification: self.magnification,
            off_axis_fraction: self.off_axis_fraction,
            over_and_under_center_opacity: self.over_and_under_center_opacity,
            render_children_outside_viewport: self.render_children_outside_viewport,
            clip_behavior: if self.clip {
                ClipBehavior::HardEdge
            } else {
                ClipBehavior::None
            },
            perspective: self.perspective,
            viewport_sink,
            laid_out: Size::ZERO,
            child_data: RefCell::new(Vec::new()),
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

/// Upstream `ListWheelParentData`: what the wheel keeps about each child it is
/// holding.
///
/// Two fields, and they answer different questions. `index` is the child's
/// place in the list, which the render object cannot derive because the live
/// run does not start at zero -- upstream's comment says it is the manager's
/// job to maintain. `transform` is what the child was painted through, saved
/// *during paint* because `apply_paint_transform` is asked afterwards and the
/// projection is not something either side can recompute from geometry alone.
///
/// `transform` being `None` means laid out but not painted. Upstream notes
/// that normally this does not happen, because the wheel paints everything it
/// lays out -- but a child on the backside of the cylinder is skipped, so here
/// it does happen and is not an error.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ListWheelParentData {
    /// Upstream's `index`.
    pub index: Option<i64>,
    /// Upstream's `transform`, as this crate's 2D affine.
    pub transform: Option<[f32; 6]>,
}

/// The wheel's render object: fixed-extent children laid out flat and painted
/// through the cylindrical projection. Upstream `RenderListWheelViewport`,
/// vertical and non-looping.
pub struct RenderListWheelViewport {
    pub children: Vec<BoxedRender>,
    /// The index `children[0]` stands for.
    pub first_index: usize,
    pub item_extent: f32,
    pub offset: f32,
    pub diameter_ratio: f32,
    pub squeeze: f32,
    /// Upstream's `useMagnifier`. Separate from `magnification` because they
    /// mean different things: a wheel can have a magnification set and the
    /// magnifier off, and upstream checks this flag, not the ratio.
    pub use_magnifier: bool,
    /// Upstream's `magnification`, which upstream asserts is positive.
    pub magnification: f32,
    /// Upstream's `offAxisFraction`: where the cylinder's axis sits across the
    /// wheel.
    ///
    /// **Zero is the middle**, which reads backwards until you see upstream's
    /// arithmetic: `_centerOriginTransform` translates the origin by
    /// `width / 2 * (-offAxisFraction * 2 + 1)`, so 0.0 lands at `width / 2`,
    /// 0.5 at the left edge and -0.5 at the right.
    ///
    /// It moves the *origin the projection turns about*, not the children --
    /// which is why a wheel with the axis off to one side looks like it is
    /// being seen from the side rather than looking like a shifted wheel. The
    /// date picker's columns use it so that all of them appear to turn on one
    /// shared axis.
    pub off_axis_fraction: f32,
    /// Upstream's `overAndUnderCenterOpacity`: how visible the children away
    /// from the magnifier are.
    ///
    /// It is a **flat opacity, not a ramp**: every child outside the centre
    /// band gets exactly this value, and upstream paints all of them into *one*
    /// shared layer before painting the centre ones at full opacity. That one
    /// layer is the whole reason the paint path splits in two -- an opacity
    /// layer per child would cost one layer per row and would show the seams
    /// where rows overlap.
    ///
    /// 1.0 is upstream's default and means no dimming; upstream tests `>= 1`
    /// and takes the single-pass path, so the default costs nothing.
    pub over_and_under_center_opacity: f32,
    /// Upstream's `renderChildrenOutsideViewport`, which **doubles the range of
    /// children laid out**. They are not painted -- the backside of the
    /// cylinder is skipped either way -- so what it buys is children that are
    /// already built when they turn into view. Upstream asserts it is not set
    /// together with clipping, because the two ask for opposite things.
    pub render_children_outside_viewport: bool,
    /// Upstream's `clipBehavior`, which defaults to `hardEdge`.
    pub clip_behavior: ClipBehavior,
    /// Upstream's `perspective`. Was the picker's constant while this lived in
    /// `cupertino.rs`; upstream's render object has always had it as a
    /// parameter, and now that two widgets share the object it has to be one.
    pub perspective: f32,
    pub viewport_sink: Rc<Cell<f32>>,
    pub laid_out: Size,
    /// Upstream's per-child `ListWheelParentData`, indexed alongside
    /// `children`. A `RefCell` because upstream writes the transform from
    /// `paint`, which does not have the render object mutably.
    pub child_data: RefCell<Vec<ListWheelParentData>>,
}

/// Upstream's constructor defaults, so a caller states only what it means to
/// change. `item_extent` has no upstream default -- it is required there -- and
/// zero here is a base to be overwritten, not a usable wheel.
impl Default for RenderListWheelViewport {
    fn default() -> RenderListWheelViewport {
        RenderListWheelViewport {
            children: Vec::new(),
            first_index: 0,
            item_extent: 0.0,
            offset: 0.0,
            diameter_ratio: DEFAULT_DIAMETER_RATIO,
            squeeze: 1.0,
            use_magnifier: false,
            magnification: 1.0,
            off_axis_fraction: 0.0,
            over_and_under_center_opacity: 1.0,
            render_children_outside_viewport: false,
            clip_behavior: ClipBehavior::HardEdge,
            perspective: DEFAULT_PERSPECTIVE,
            viewport_sink: Rc::new(Cell::new(0.0)),
            laid_out: Size::ZERO,
            child_data: RefCell::new(Vec::new()),
        }
    }
}

impl RenderListWheelViewport {
    /// Upstream's `indexOf`, which reads the child's parent data.
    pub fn index_of(&self, child: usize) -> Option<i64> {
        self.child_data.borrow().get(child).and_then(|d| d.index)
    }

    /// Upstream's `applyPaintTransform`, which multiplies in whatever the last
    /// paint recorded. Nothing recorded means the child was not painted, and
    /// upstream leaves the transform alone.
    pub fn apply_paint_transform(&self, child: usize) -> Option<[f32; 6]> {
        self.child_data
            .borrow()
            .get(child)
            .and_then(|d| d.transform)
    }

    /// Upstream's `_topScrollMarginExtent`: **the wheel is anchored at its
    /// middle, not at its top.**
    ///
    /// `indexToScrollOffset` is `index * itemExtent` for the layout, the
    /// extents and the visible range alike, and the scroll offset that selects
    /// an item is the offset that puts that item *under the selection band* --
    /// which is the middle of the viewport, not its top edge. This is the
    /// half-viewport that turns one into the other.
    ///
    /// Leaving it out is self-consistent and wrong: the wheel still scrolls,
    /// and every row it shows is half a viewport away from the row it reports.
    /// A 216-high date picker read six rows below its own answer.
    fn top_scroll_margin_extent(&self) -> f32 {
        -self.laid_out.height / 2.0 + self.item_extent / 2.0
    }

    /// Upstream's `_getUntransformedPaintingCoordinateY`: a child's layout
    /// coordinate (`index * itemExtent`) as a y in the viewport, before the
    /// cylinder bends it.
    fn untransformed_painting_y(&self, layout_y: f32) -> f32 {
        layout_y - self.top_scroll_margin_extent() - self.offset
    }

    /// Upstream's `_shouldClipAtCurrentOffset`, which clips only when a child
    /// actually reaches past an edge -- so a wheel whose content fits pays for
    /// no clip layer at all.
    pub fn should_clip_at_current_offset(&self) -> bool {
        if self.render_children_outside_viewport {
            return false;
        }
        let highest = self.untransformed_painting_y(0.0);
        // `_maxEstimatedScrollExtent`: the last child's layout coordinate.
        let max_extent =
            (self.first_index + self.children.len()).saturating_sub(1) as f32 * self.item_extent;
        highest < 0.0 || self.laid_out.height < highest + max_extent + self.item_extent
    }

    /// Where the projection's origin sits across the wheel. Upstream's
    /// `_centerOriginTransform` translates by
    /// `centerX * (-offAxisFraction * 2 + 1)`, which is the middle at 0.5 and
    /// slides to an edge as the fraction goes to 0 or 1.
    /// Upstream's `_paintVisibleChildren`, which is the two-pass split and
    /// nothing else.
    ///
    /// One shared opacity layer around every off-centre child, then the centre
    /// ones at full opacity -- see
    /// [`over_and_under_center_opacity`](Self::over_and_under_center_opacity)
    /// for why it is one layer and not one per child. With no dimming asked for
    /// there is one pass and no layer at all.
    fn paint_children(&self, context: &mut PaintContext, offset: Offset) {
        if self.over_and_under_center_opacity >= 1.0 {
            self.paint_all_children(context, offset, None);
            return;
        }
        let alpha = self.off_center_alpha();
        context.in_layer(
            |tree| tree.push_opacity(alpha, 0.0, 0.0),
            |context| self.paint_all_children(context, offset, Some(false)),
        );
        self.paint_all_children(context, offset, Some(true));
    }

    /// Upstream's `_paintAllChildren`.
    ///
    /// `center` selects which children this pass is for, and it is upstream's
    /// own three-way parameter: `None` for all of them, `Some(true)` for the
    /// ones in the magnifier band, `Some(false)` for the ones outside it.
    /// Upstream splits a *partially* intersecting child across both passes by
    /// clipping it twice; this port magnifies a child only when it is wholly
    /// inside the band -- see [`inside_magnifier_band`] -- so each child falls
    /// on exactly one side, which is the same split made stepwise.
    fn paint_all_children(&self, context: &mut PaintContext, offset: Offset, center: Option<bool>) {
        let height = self.laid_out.height;
        if height <= 0.0 {
            return;
        }
        self.child_data
            .borrow_mut()
            .resize(self.children.len(), ListWheelParentData::default());
        let radius = height * self.diameter_ratio / 2.0;
        for (i, child) in self.children.iter().enumerate() {
            let index = self.first_index + i;
            self.child_data.borrow_mut()[i].index = Some(index as i64);
            let flat_center = self.untransformed_painting_y(index as f32 * self.item_extent)
                + self.item_extent / 2.0;
            let angle = angle_for(flat_center, height, self.diameter_ratio, self.squeeze);
            // The backside of the cylinder is not painted -- which is also why
            // `renderChildrenOutsideViewport` costs nothing at paint time: the
            // extra children it lays out are exactly the ones skipped here.
            if angle.abs() > std::f32::consts::FRAC_PI_2 || angle.is_nan() {
                continue;
            }
            let y_rel = flat_center - height / 2.0;
            let (screen_y, mut sx) = project_center(y_rel, angle, radius, height, self.perspective);
            let mut sy = project_scale_y(y_rel, angle, radius, height, self.perspective);
            let in_center = self.use_magnifier
                && inside_magnifier_band(screen_y, height, self.item_extent, self.magnification);
            if center.is_some_and(|center| center != in_center) {
                continue;
            }
            if in_center {
                sx *= self.magnification;
                sy *= self.magnification;
            }
            let child_size = child.size();
            // Scale about the projection's origin, placed at the child's
            // projected position: `push_transform`'s pivot form. The pivot is
            // in the child's coordinates, so the origin -- which
            // `offAxisFraction` moves across the *viewport* -- is carried back
            // through where the child sits.
            let across = (self.laid_out.width - child_size.width) / 2.0;
            let pivot = Offset::new(self.center_origin_x() - across, child_size.height / 2.0);
            let at = Offset::new(
                offset.dx + across,
                offset.dy + screen_y - child_size.height / 2.0,
            );
            let matrix = [sx, 0.0, 0.0, sy, 0.0, 0.0];
            // Upstream saves the transform it painted through so that
            // `applyPaintTransform` can answer afterwards -- see
            // [`ListWheelParentData::transform`]. This is `push_transform`'s
            // composition written out: scale about the pivot, then place.
            self.child_data.borrow_mut()[i].transform = Some([
                sx,
                0.0,
                0.0,
                sy,
                at.dx + pivot.dx - sx * pivot.dx,
                at.dy + pivot.dy - sy * pivot.dy,
            ]);
            context.push_transform(matrix, pivot, at, child);
        }
    }

    /// The alpha the off-centre children share. Upstream's
    /// `(overAndUnderCenterOpacity * 255).round()`.
    fn off_center_alpha(&self) -> u8 {
        (self.over_and_under_center_opacity.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    /// Where the projection's origin sits across the wheel. Upstream's
    /// `_centerOriginTransform` translates by
    /// `width / 2 * (-offAxisFraction * 2 + 1)` -- the middle at 0.0, the left
    /// edge at 0.5, the right edge at -0.5.
    fn center_origin_x(&self) -> f32 {
        self.laid_out.width / 2.0 * (-self.off_axis_fraction * 2.0 + 1.0)
    }
}

impl RenderBox for RenderListWheelViewport {
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

    /// Upstream's `paint`, which is the clip decision and nothing else.
    ///
    /// The clip is only pushed when a child actually reaches past an edge --
    /// upstream's `_shouldClipAtCurrentOffset` -- so a wheel whose content fits
    /// pays for no clip layer at all. `renderChildrenOutsideViewport` turns it
    /// off outright, because the two ask for opposite things and upstream
    /// asserts they are not both set.
    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        if self.children.is_empty() {
            return;
        }
        if self.clip_behavior == ClipBehavior::None || !self.should_clip_at_current_offset() {
            self.paint_children(context, offset);
            return;
        }
        let bounds = Rect::xywh(
            offset.dx,
            offset.dy,
            self.laid_out.width,
            self.laid_out.height,
        );
        let behavior = self.clip_behavior;
        context.in_layer(
            |tree| tree.push_clip_rect(bounds, behavior),
            |context| self.paint_children(context, offset),
        );
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
                self.untransformed_painting_y(index as f32 * self.item_extent),
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
                    self.untransformed_painting_y(index as f32 * self.item_extent),
                ),
            );
        }
    }

    /// Upstream `RenderListWheelViewport.computeDryLayout`, which is
    /// `constraints.biggest` -- and `layout` above says the same thing the
    /// long way, taking each side's maximum when it is bounded and its minimum
    /// when it is not. That is what [`BoxConstraints::biggest`] is.
    ///
    /// See PORTING_STATUS.md, tick 470.
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
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
    fn a_wheel_measured_dry_takes_the_room_it_would_take() {
        // Upstream `RenderListWheelViewport.computeDryLayout`, which is
        // `constraints.biggest` -- and `layout` says the same thing the long
        // way. The trait's default is `Size::ZERO`, so a wheel measured by a
        // parent that had not committed came out with no size, and a wheel
        // with no size shows nothing.
        let wheel = ListWheelViewport {
            item_extent: 40.0,
            ..ListWheelViewport::new(40.0)
        };
        let mut render = wheel.render(Vec::new(), 0, 0.0, Rc::new(Cell::new(0.0)));
        let room = crate::render::BoxConstraints::loose(200.0, 300.0);
        let dry = crate::render::RenderBox::compute_dry_layout(&render, room);
        assert_eq!(dry, crate::render::Size::new(200.0, 300.0));
        assert_eq!(
            crate::render::RenderBox::layout(&mut render, room),
            dry,
            "and the dry answer is the wet one"
        );

        // Unbounded on one side, it takes the minimum there instead -- which
        // is what `biggest` means when a maximum is infinite.
        let endless = crate::render::BoxConstraints {
            min_width: 30.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: 120.0,
        };
        assert_eq!(
            crate::render::RenderBox::compute_dry_layout(&render, endless),
            crate::render::Size::new(30.0, 120.0)
        );
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
        // And a wheel that has been flicked is given something to do, which
        // is what says the `None` above is a decision rather than the only
        // answer this function has. The first draft of this line used the same
        // wheel a little off its mark with no velocity, and tripped a
        // `debug_assert` in `FrictionSimulation`: rolling back somewhere is a
        // motion, and a motion with no velocity is not one this physics can
        // build.
        let flicked = wheel_metrics(85.0, 40.0, 10);
        assert!(
            physics
                .create_ballistic_simulation(&flicked, 40.0, 30.0)
                .is_some(),
            "a flicked wheel has somewhere to go"
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

#[cfg(test)]
mod viewport_tests {
    use super::*;
    use crate::render::RenderRef;
    use crate::widgets::Container;

    fn wheel(count: usize) -> RenderListWheelViewport {
        RenderListWheelViewport {
            children: (0..count)
                .map(|_| RenderRef::new(Container::new().with_size(100.0, 40.0)))
                .collect(),
            item_extent: 40.0,
            ..Default::default()
        }
    }

    fn laid_out(mut wheel: RenderListWheelViewport) -> RenderListWheelViewport {
        wheel.layout(BoxConstraints::tight_for(Size::new(200.0, 200.0)));
        wheel
    }

    fn children(count: usize) -> Vec<Rc<dyn Fn() -> AnyWidget>> {
        (0..count)
            .map(|_| -> Rc<dyn Fn() -> AnyWidget> {
                Rc::new(|| crate::framework::leaf(|| crate::widgets::Empty))
            })
            .collect()
    }

    fn paint_once(wheel: &RenderListWheelViewport) {
        let mut layers = crate::engine::LayerTree::new(200, 200);
        let mut context = PaintContext::new(&mut layers, Size::new(200.0, 200.0));
        wheel.paint(&mut context, Offset::ZERO);
    }

    /// Which children a single pass painted, read off the parent data it wrote.
    fn painted_by(wheel: &RenderListWheelViewport, center: Option<bool>) -> Vec<bool> {
        wheel.child_data.borrow_mut().clear();
        let mut layers = crate::engine::LayerTree::new(200, 200);
        let mut context = PaintContext::new(&mut layers, Size::new(200.0, 200.0));
        wheel.paint_all_children(&mut context, Offset::ZERO, center);
        wheel
            .child_data
            .borrow()
            .iter()
            .map(|d| d.transform.is_some())
            .collect()
    }

    // -- ListWheelChildManager --------------------------------------------------------

    #[test]
    fn no_known_limit_is_not_no_children() {
        // The member most easily read backwards: `None` means the wheel has no
        // ends, which is how the looping delegate works, not that it is empty.
        let looping =
            ListWheelElement::new(Rc::new(ListWheelChildLoopingListDelegate::new(children(3))));
        assert_eq!(ListWheelChildManager::child_count(&looping), None);
        assert!(looping.child_exists_at(1_000_000));
        assert!(looping.child_exists_at(-1_000_000));

        let bounded = ListWheelElement::new(Rc::new(ListWheelChildListDelegate::new(children(3))));
        assert_eq!(ListWheelChildManager::child_count(&bounded), Some(3));
        assert!(!bounded.child_exists_at(3));
    }

    #[test]
    fn the_element_is_the_manager() {
        // Upstream's element `implements ListWheelChildManager`; this is the
        // same claim, made where the compiler can check it.
        fn ask(manager: &dyn ListWheelChildManager) -> Option<usize> {
            manager.child_count()
        }
        let element = ListWheelElement::new(Rc::new(ListWheelChildListDelegate::new(children(4))));
        assert_eq!(ask(&element), Some(4));

        let made = ListWheelChildManager::create_child(&element, 0, None);
        assert!(made.is_some());
        assert_eq!(element.active_indices(), vec![0]);
        ListWheelChildManager::remove_child(&element, 0);
        assert!(element.active_indices().is_empty());
    }

    // -- ListWheelParentData ------------------------------------------------------------

    #[test]
    fn each_child_knows_its_index_and_the_first_one_need_not_be_zero() {
        // The render object cannot derive it: the live run starts wherever the
        // wheel is scrolled to.
        let mut w = wheel(3);
        w.first_index = 17;
        let w = laid_out(w);
        paint_once(&w);
        assert_eq!(w.index_of(0), Some(17));
        assert_eq!(w.index_of(2), Some(19));
        assert_eq!(w.index_of(3), None, "there is no fourth child");
    }

    #[test]
    fn a_child_on_the_backside_is_laid_out_and_not_painted() {
        // So its transform stays empty, which upstream says normally does not
        // happen -- here it does, and it is not an error.
        let w = laid_out(wheel(40));
        paint_once(&w);
        let data = w.child_data.borrow();
        assert!(
            data.iter().any(|d| d.transform.is_some()),
            "something faces the front"
        );
        assert!(
            data.iter().any(|d| d.transform.is_none()),
            "and something is round the back"
        );
        assert!(
            data.iter().all(|d| d.index.is_some()),
            "but every one of them still knows its index"
        );
    }

    #[test]
    fn the_recorded_transform_is_what_the_child_was_painted_through() {
        // The child at the centre is not turned at all -- angle 0, and the
        // perspective divide is 1 there -- so it must be painted through an
        // identity scale at its natural place. Anything else means the recorded
        // transform and the paint have drifted apart.
        //
        // The offset is what says which child that is: the wheel is anchored at
        // its middle, so `offset == index * itemExtent` centres `index`.
        let mut w = wheel(5);
        w.offset = 2.0 * 40.0;
        let w = laid_out(w);
        paint_once(&w);
        let [a, b, c, d, e, f] = w.apply_paint_transform(2).expect("index 2 is centred");
        assert_eq!((b, c), (0.0, 0.0), "no rotation: the projection is a scale");
        assert!((a - 1.0).abs() < 1e-3, "unscaled across, got {a}");
        assert!((d - 1.0).abs() < 1e-3, "and down, got {d}");
        assert!((e - 50.0).abs() < 1e-3, "centred across the wheel, got {e}");
        assert!((f - 80.0).abs() < 1e-3, "and centred down it, got {f}");
    }

    /// The offset that selects an item is the offset that puts it under the
    /// selection band.
    ///
    /// Upstream's `_topScrollMarginExtent` is the whole of this, and leaving it
    /// out is self-consistent and wrong: the wheel scrolls, the extents are
    /// right, the visible range is right, and every row it *shows* is half a
    /// viewport away from the row it *reports*. A 216-high Cupertino date
    /// picker sitting on August showed November.
    #[test]
    fn the_offset_that_selects_an_item_is_the_offset_that_centres_it() {
        for index in 0..5 {
            let mut w = wheel(5);
            w.offset = index as f32 * 40.0;
            let w = laid_out(w);
            paint_once(&w);
            let placed = w
                .apply_paint_transform(index)
                .unwrap_or_else(|| panic!("index {index} is painted"));
            // The child's top, from the recorded translation: half the
            // viewport less half an item.
            assert!(
                (placed[5] - 80.0).abs() < 1e-3,
                "index {index} should sit at the middle, got {}",
                placed[5]
            );
            // And it is the one under the band, so it is not turned at all.
            assert!((placed[0] - 1.0).abs() < 1e-3, "{placed:?}");
        }
    }

    #[test]
    fn a_child_away_from_the_centre_is_turned_and_shrunk() {
        // The other half: the projection has to actually do something, or the
        // test above would pass on a wheel that never turns.
        let mut w = wheel(5);
        w.offset = 2.0 * 40.0;
        let w = laid_out(w);
        paint_once(&w);
        let far = w.apply_paint_transform(0).expect("painted");
        assert!(far[0] < 1.0, "narrower, got {}", far[0]);
        assert!(far[3] < 1.0, "and shorter, got {}", far[3]);
    }

    // -- offAxisFraction ------------------------------------------------------------------

    #[test]
    fn zero_puts_the_axis_in_the_middle() {
        // Reads backwards, and is upstream's arithmetic: 0.0 -> width / 2.
        let w = laid_out(wheel(3));
        assert_eq!(w.center_origin_x(), 100.0, "the middle of a 200-wide wheel");

        let mut left = wheel(3);
        left.off_axis_fraction = 0.5;
        assert_eq!(laid_out(left).center_origin_x(), 0.0, "the left edge");

        let mut right = wheel(3);
        right.off_axis_fraction = -0.5;
        assert_eq!(laid_out(right).center_origin_x(), 200.0, "the right edge");
    }

    #[test]
    fn moving_the_axis_leaves_the_centre_child_where_it_was() {
        // It moves the origin the projection turns about, not the children --
        // and the child at the centre is not turned at all, so it must not move.
        let mut centered = wheel(9);
        centered.offset = 40.0 * 4.0;
        let centered = laid_out(centered);
        paint_once(&centered);
        let straight = centered.apply_paint_transform(4).expect("painted");

        let mut off = wheel(9);
        off.offset = 40.0 * 4.0;
        off.off_axis_fraction = 0.5;
        let off = laid_out(off);
        paint_once(&off);
        let tilted = off.apply_paint_transform(4).expect("painted");

        for (a, b) in straight.iter().zip(tilted.iter()) {
            assert!((a - b).abs() < 1e-3, "{straight:?} vs {tilted:?}");
        }
    }

    #[test]
    fn a_turned_child_does_move_when_the_axis_does() {
        // The other half: if nothing moved, the fraction would be doing nothing.
        let mut straight = wheel(9);
        straight.offset = 40.0 * 4.0 - 80.0;
        let straight = laid_out(straight);
        paint_once(&straight);
        let a = straight.apply_paint_transform(1).expect("painted");

        let mut off = wheel(9);
        off.offset = 40.0 * 4.0 - 80.0;
        off.off_axis_fraction = 0.5;
        let off = laid_out(off);
        paint_once(&off);
        let b = off.apply_paint_transform(1).expect("painted");

        assert!((a[4] - b[4]).abs() > 1.0, "{a:?} vs {b:?}");
    }

    // -- overAndUnderCenterOpacity ----------------------------------------------------------

    #[test]
    fn full_opacity_is_one_pass_and_no_layer() {
        // Upstream tests `>= 1` and takes the single-pass path, so the default
        // costs nothing.
        let w = laid_out(wheel(5));
        assert_eq!(w.over_and_under_center_opacity, 1.0);
        assert_eq!(w.off_center_alpha(), 255);
    }

    #[test]
    fn a_dimmed_wheel_fades_with_a_layer_rather_than_a_canvas_group() {
        // The alpha tests above are arithmetic; this is what reaches the
        // engine, and the answer is: nothing on the canvas. The dimming is
        // `push_opacity` -- a compositor layer -- so no `save_layer` is
        // opened.
        //
        // Which is the point. A canvas group costs a buffer the size of the
        // wheel on every frame and is this thread's to fill; a layer is the
        // compositor's to schedule, and can be cached across frames where the
        // wheel has not moved. The two are one call apart in `paint_children`.
        let mut w = wheel(5);
        w.over_and_under_center_opacity = 0.5;
        let w = laid_out(w);
        crate::engine_test_stubs::reset_drawn();
        paint_once(&w);
        assert!(
            !crate::engine_test_stubs::drawn()
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::SaveLayer { .. })),
            "the fade is a layer, not a canvas group"
        );
    }

    #[test]
    fn and_an_undimmed_one_opens_none() {
        // The single-pass path. A group at full opacity would cost a buffer
        // the size of the wheel on every frame and change nothing.
        let w = laid_out(wheel(5));
        crate::engine_test_stubs::reset_drawn();
        paint_once(&w);
        assert!(
            !crate::engine_test_stubs::drawn()
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::SaveLayer { .. })),
            "no group at full opacity"
        );
    }

    #[test]
    fn the_dimming_is_flat_and_not_a_ramp() {
        // Every off-centre child gets the same value -- one shared layer. A
        // per-child ramp would need one layer per row and would show the seams
        // where rows overlap.
        let mut w = wheel(5);
        w.over_and_under_center_opacity = 0.5;
        assert_eq!(w.off_center_alpha(), 128);
        w.over_and_under_center_opacity = 0.0;
        assert_eq!(w.off_center_alpha(), 0);
    }

    #[test]
    fn the_two_passes_between_them_paint_each_child_once() {
        // The centre pass and the off-centre pass partition the children; a
        // child painted by both would come out heavier than its neighbours.
        let mut w = wheel(9);
        w.use_magnifier = true;
        w.magnification = 1.5;
        w.over_and_under_center_opacity = 0.5;
        w.offset = 40.0 * 4.0 - 80.0;
        let w = laid_out(w);

        let in_center = painted_by(&w, Some(true));
        let outside = painted_by(&w, Some(false));

        assert!(in_center.iter().any(|&x| x), "something is in the band");
        assert!(outside.iter().any(|&x| x), "and something is not");
        for (i, (a, b)) in in_center.iter().zip(outside.iter()).enumerate() {
            assert!(!(*a && *b), "child {i} was painted by both passes");
        }

        let both = painted_by(&w, None);
        for (i, (a, b)) in in_center.iter().zip(outside.iter()).enumerate() {
            assert_eq!(both[i], *a || *b, "child {i}");
        }
    }

    // -- useMagnifier -----------------------------------------------------------------------

    #[test]
    fn a_magnification_with_the_magnifier_off_does_nothing() {
        // Upstream branches on `useMagnifier`, not on the ratio. They are two
        // settings, and a wheel can carry a magnification it is not using --
        // which is what a picker does while the magnifier is being turned off.
        let mut w = wheel(5);
        w.magnification = 2.0;
        w.use_magnifier = false;
        w.offset = 2.0 * 40.0;
        let w = laid_out(w);
        paint_once(&w);
        let plain = w.apply_paint_transform(2).expect("index 2 is centred");
        assert!(
            (plain[0] - 1.0).abs() < 1e-3,
            "unmagnified, got {}",
            plain[0]
        );

        let mut on = wheel(5);
        on.magnification = 2.0;
        on.use_magnifier = true;
        on.offset = 2.0 * 40.0;
        let on = laid_out(on);
        paint_once(&on);
        let magnified = on.apply_paint_transform(2).expect("index 2 is centred");
        assert!((magnified[0] - 2.0).abs() < 1e-3, "got {}", magnified[0]);
    }

    // -- renderChildrenOutsideViewport and clipping --------------------------------------------

    #[test]
    fn a_wheel_whose_content_fits_asks_for_no_clip() {
        // Five 40-high items exactly fill a 200-high wheel -- but only at the
        // offset that centres the middle one, because the wheel is anchored at
        // its middle. At any other offset the run hangs over an edge and the
        // clip is asked for, which the test below is the other half of.
        let mut w = wheel(5);
        w.offset = 2.0 * 40.0;
        let w = laid_out(w);
        assert!(!w.should_clip_at_current_offset(), "5 * 40 == 200, exactly");
    }

    #[test]
    fn a_child_reaching_past_an_edge_asks_for_one() {
        let w = laid_out(wheel(6));
        assert!(w.should_clip_at_current_offset(), "6 * 40 > 200");

        let mut scrolled = wheel(5);
        scrolled.offset = 2.0 * 40.0 + 10.0;
        assert!(
            laid_out(scrolled).should_clip_at_current_offset(),
            "and content that fits but is scrolled reaches past the top"
        );
    }

    #[test]
    fn rendering_outside_the_viewport_turns_clipping_off_outright() {
        // The two ask for opposite things, and upstream asserts they are not
        // both set.
        let mut w = wheel(20);
        w.render_children_outside_viewport = true;
        assert!(!laid_out(w).should_clip_at_current_offset());
    }
}
