//! Scrolling in two dimensions at once -- a port of upstream's
//! `widgets/two_dimensional_viewport.dart` and the scroll view over it.
//!
//! A one-dimensional viewport can name a child with a number. A
//! two-dimensional one cannot, so every child here is named by a
//! [`ChildVicinity`] -- an `(x, y)` pair. The word is *vicinity* rather than
//! position on purpose: children may be laid out anywhere, and the pair only
//! says who a child's neighbours are. A table with a merged cell has one child
//! covering several vicinities, and the layout is free to skip the ones it
//! swallowed.
//!
//! Two decisions carry the file.
//!
//! **Paint order comes from the main axis**, and reads backwards until you see
//! why: a vertical main axis gives **row-major** order. Upstream's own comment
//! says "this seems backwards" and then gives the reason -- vertical is
//! Flutter's default scroll axis, row-major is the default for matrices, so
//! the two defaults have to be made to agree even though the mapping between
//! them inverts.
//!
//! **Visibility is computed after layout, not during it.** A child says where
//! it wants to be; the viewport works out how much of that landed on screen.
//! A child entirely outside gets a zero paint extent and is skipped when
//! painting, which is what keeps a table of ten thousand cells to the cost of
//! the forty on screen.
//!
//! ## What is not here
//!
//! The `RenderBox` plumbing -- `performLayout`, hit testing, the element that
//! owns the child manager -- belongs to this crate's own render tree and is
//! not duplicated. What is ported is the vicinity and its ordering, the parent
//! data and its visibility rule, the paint-extent clipping, the build/reuse
//! decision, the keep-alive bucket, and the scroll view's configuration
//! checks.

use crate::render::{Axis, AxisDirection, Offset, Size, axis_direction_to_axis};
use crate::scrollable_helpers::ScrollableDetails;
use std::collections::HashMap;

/// Upstream `ChildVicinity`: which child this is, relative to its neighbours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChildVicinity {
    pub x_index: i32,
    pub y_index: i32,
}

impl ChildVicinity {
    /// Upstream's `ChildVicinity.invalid`, `(-1, -1)`: a child that is between
    /// positions rather than at one.
    pub const INVALID: ChildVicinity = ChildVicinity {
        x_index: -1,
        y_index: -1,
    };

    /// Upstream asserts both indices are `>= -1`, which allows exactly one
    /// negative value and only so that [`ChildVicinity::INVALID`] can be
    /// spelled.
    pub fn new(x_index: i32, y_index: i32) -> ChildVicinity {
        debug_assert!(x_index >= -1 && y_index >= -1);
        ChildVicinity { x_index, y_index }
    }

    pub fn is_valid(&self) -> bool {
        *self != ChildVicinity::INVALID
    }

    /// Upstream's `compareTo`: x first, then y. Column-major order.
    pub fn compare_column_major(&self, other: &ChildVicinity) -> std::cmp::Ordering {
        (self.x_index, self.y_index).cmp(&(other.x_index, other.y_index))
    }

    /// Row-major order: y first, then x. Upstream spells this one out in
    /// `_sortByYIndex` rather than reusing `compareTo`, because it is the
    /// opposite key order.
    pub fn compare_row_major(&self, other: &ChildVicinity) -> std::cmp::Ordering {
        (self.y_index, self.x_index).cmp(&(other.y_index, other.x_index))
    }
}

impl Default for ChildVicinity {
    fn default() -> ChildVicinity {
        ChildVicinity::INVALID
    }
}

impl Ord for ChildVicinity {
    fn cmp(&self, other: &ChildVicinity) -> std::cmp::Ordering {
        self.compare_column_major(other)
    }
}

impl PartialOrd for ChildVicinity {
    fn partial_cmp(&self, other: &ChildVicinity) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for ChildVicinity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "(xIndex: {}, yIndex: {})", self.x_index, self.y_index)
    }
}

/// Upstream `TwoDimensionalViewportParentData`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TwoDimensionalViewportParentData {
    /// Where the layout put the child's top-left corner, in the parent's
    /// coordinates. Set by `layoutChildSequence`.
    pub layout_offset: Option<Offset>,
    pub vicinity: ChildVicinity,
    /// Upstream's `paintOffset`, which **is not `layoutOffset`** whenever the
    /// scroll runs `up` or `left`: the layout offset is measured from the
    /// leading edge of the scroll, and the paint offset from the top left of
    /// the viewport. They coincide only for `down` and `right`. Overriding
    /// `paint` and reaching for `layoutOffset` is the mistake this pair
    /// exists to make findable.
    pub paint_offset: Option<Offset>,
    /// How much of the child actually landed on screen. `None` until
    /// `updateChildPaintData` has run.
    paint_extent: Option<Size>,
    pub keep_alive: bool,
}

impl TwoDimensionalViewportParentData {
    pub fn new() -> TwoDimensionalViewportParentData {
        TwoDimensionalViewportParentData::default()
    }

    pub fn paint_extent(&self) -> Option<Size> {
        self.paint_extent
    }

    pub fn set_paint_extent(&mut self, extent: Size) {
        self.paint_extent = Some(extent);
    }

    /// Upstream's `isVisible`, and it carries a redundancy worth naming:
    ///
    /// ```dart
    /// return _paintExtent != Size.zero
    ///     || _paintExtent!.height != 0.0
    ///     || _paintExtent!.width != 0.0;
    /// ```
    ///
    /// The last two clauses cannot fire. If the first is false then the extent
    /// **is** `Size.zero`, so both of its dimensions are zero too. The whole
    /// expression is "the paint extent is not exactly zero", and that is what
    /// is written here -- with the note, because reading the original invites
    /// the guess that a zero-width-but-tall child is being treated specially,
    /// and it is not: such a child is already visible by the first clause.
    ///
    /// Upstream throws if the extent has not been computed. Asking whether a
    /// child is visible before laying it out is a programming error, not a
    /// state to branch on.
    pub fn is_visible(&self) -> bool {
        let extent = self
            .paint_extent
            .expect("the paint extent of the child has not been determined yet");
        extent != Size::ZERO
    }

    /// Upstream's `keptAlive`: **kept alive and not visible**. A child that is
    /// on screen is not being kept alive; it is simply there.
    pub fn kept_alive(&self) -> bool {
        self.keep_alive && !self.is_visible()
    }
}

/// Upstream `TwoDimensionalChildManager`: what the render object asks of the
/// element that owns the children.
///
/// The four calls bracket a layout pass. `reuse_child` exists separately from
/// `build_child` because a child that is still in the right place must not be
/// rebuilt -- rebuilding it would throw away its state on every scroll frame.
pub trait TwoDimensionalChildManager {
    fn start_layout(&mut self);
    fn build_child(&mut self, vicinity: ChildVicinity);
    fn reuse_child(&mut self, vicinity: ChildVicinity);
    fn end_layout(&mut self);
}

/// Upstream `TwoDimensionalViewport`: the widget side.
///
/// Abstract upstream, and a trait here for the same reason: a viewport has to
/// be told how to lay its children out, and there is no default answer -- a
/// table, a spreadsheet and a free canvas share everything except that.
pub trait TwoDimensionalViewport {
    fn main_axis(&self) -> Axis;
    fn vertical_axis_direction(&self) -> AxisDirection;
    fn horizontal_axis_direction(&self) -> AxisDirection;
    fn cache_extent(&self) -> Option<f32>;

    /// Upstream asserts the two directions really are on their own axes,
    /// because the pair is what tells the viewport which way is which and a
    /// swap would silently transpose the whole layout.
    fn axes_are_valid(&self) -> bool {
        axis_direction_to_axis(self.vertical_axis_direction()) == Axis::Vertical
            && axis_direction_to_axis(self.horizontal_axis_direction()) == Axis::Horizontal
    }
}

/// Upstream `RenderTwoDimensionalViewport`, reduced to the bookkeeping.
///
/// Children are keyed by vicinity rather than held in a list, because the
/// layout is free to ask for them in any order and to skip some entirely.
#[derive(Debug, Default)]
pub struct RenderTwoDimensionalViewport {
    pub main_axis: Axis,
    pub viewport_dimension: Size,
    children: HashMap<ChildVicinity, TwoDimensionalViewportParentData>,
    keep_alive_bucket: HashMap<ChildVicinity, TwoDimensionalViewportParentData>,
    /// Upstream's `_activeChildrenForLayoutPass`: what this pass asked for.
    active_this_pass: Vec<ChildVicinity>,
    /// Upstream's `_currentChildVicinities`, the list that becomes paint order.
    current_vicinities: Vec<ChildVicinity>,
    /// Paint order, after `_reifyChildren`.
    paint_order: Vec<ChildVicinity>,
    pub needs_delegate_rebuild: bool,
}

/// What [`RenderTwoDimensionalViewport::build_or_obtain_child_for`] asked the
/// child manager to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildRequest {
    /// The child does not exist yet, or the delegate changed under it.
    Build,
    /// The child is already there and only needs claiming for this pass.
    Reuse,
}

impl RenderTwoDimensionalViewport {
    pub fn new(main_axis: Axis, viewport_dimension: Size) -> RenderTwoDimensionalViewport {
        RenderTwoDimensionalViewport {
            main_axis,
            viewport_dimension,
            ..RenderTwoDimensionalViewport::default()
        }
    }

    pub fn parent_data_of(
        &self,
        vicinity: ChildVicinity,
    ) -> Option<&TwoDimensionalViewportParentData> {
        self.children.get(&vicinity)
    }

    pub fn children(&self) -> &HashMap<ChildVicinity, TwoDimensionalViewportParentData> {
        &self.children
    }

    pub fn keep_alive_bucket(&self) -> &HashMap<ChildVicinity, TwoDimensionalViewportParentData> {
        &self.keep_alive_bucket
    }

    /// The order [`RenderTwoDimensionalViewport::reify_children`] settled on.
    pub fn paint_order(&self) -> &[ChildVicinity] {
        &self.paint_order
    }

    /// Upstream's `firstChild`, null during layout because the order is not
    /// decided until the pass ends.
    pub fn first_child(&self) -> Option<ChildVicinity> {
        self.paint_order.first().copied()
    }

    pub fn last_child(&self) -> Option<ChildVicinity> {
        self.paint_order.last().copied()
    }

    /// Upstream's `childBefore`.
    pub fn child_before(&self, child: ChildVicinity) -> Option<ChildVicinity> {
        let at = self.paint_order.iter().position(|held| *held == child)?;
        if at == 0 {
            return None;
        }
        self.paint_order.get(at - 1).copied()
    }

    /// Upstream's `childAfter`.
    pub fn child_after(&self, child: ChildVicinity) -> Option<ChildVicinity> {
        let at = self.paint_order.iter().position(|held| *held == child)?;
        self.paint_order.get(at + 1).copied()
    }

    /// Upstream's `_startLayout` side of the pass.
    pub fn start_layout(&mut self) {
        self.active_this_pass.clear();
        self.current_vicinities.clear();
        self.paint_order.clear();
    }

    /// Upstream's `buildOrObtainChildFor`.
    ///
    /// The decision is one condition, and the `needsDelegateRebuild` half of
    /// it is the interesting one: a delegate that changed invalidates **every**
    /// child, even ones sitting exactly where they were. Reusing them would
    /// show the reader the old delegate's content in the new layout.
    ///
    /// Note also that a child coming back out of the keep-alive bucket is a
    /// *reuse*, not a build. That is the whole point of the bucket: the child
    /// scrolled away and came back with its state intact.
    pub fn build_or_obtain_child_for(&mut self, vicinity: ChildVicinity) -> ChildRequest {
        debug_assert!(vicinity.is_valid(), "cannot ask for the invalid vicinity");
        let known =
            self.children.contains_key(&vicinity) || self.keep_alive_bucket.contains_key(&vicinity);
        let request = if self.needs_delegate_rebuild || !known {
            ChildRequest::Build
        } else {
            if let Some(data) = self.keep_alive_bucket.remove(&vicinity) {
                self.children.insert(vicinity, data);
            }
            ChildRequest::Reuse
        };
        request
    }

    /// Records that the child manager produced a child for `vicinity`.
    pub fn did_produce_child(&mut self, vicinity: ChildVicinity) {
        let data = self.children.entry(vicinity).or_default();
        data.vicinity = vicinity;
        if !self.active_this_pass.contains(&vicinity) {
            self.active_this_pass.push(vicinity);
        }
        if !self.current_vicinities.contains(&vicinity) {
            self.current_vicinities.push(vicinity);
        }
    }

    pub fn set_layout_offset(&mut self, vicinity: ChildVicinity, offset: Offset) {
        if let Some(data) = self.children.get_mut(&vicinity) {
            data.layout_offset = Some(offset);
        }
    }

    pub fn set_keep_alive(&mut self, vicinity: ChildVicinity, keep_alive: bool) {
        if let Some(data) = self.children.get_mut(&vicinity) {
            data.keep_alive = keep_alive;
        }
    }

    /// Upstream's `computeChildPaintExtent`: how much of a child landed on
    /// screen.
    ///
    /// Every branch that answers `Size::ZERO` is a child that will not be
    /// painted at all, and that is where the saving is -- a table of ten
    /// thousand cells costs the forty that are visible.
    ///
    /// The first line is worth reading twice: a child with **zero width or
    /// zero height** is invisible outright, whatever its offset. Clipping a
    /// zero-area child would otherwise give it a paint extent inside the
    /// viewport and make it look visible.
    pub fn compute_child_paint_extent(&self, layout_offset: Offset, child_size: Size) -> Size {
        if child_size.height == 0.0 || child_size.width == 0.0 {
            return Size::ZERO;
        }
        let Some(width) = Self::visible_span(
            layout_offset.dx,
            child_size.width,
            self.viewport_dimension.width,
        ) else {
            return Size::ZERO;
        };
        let Some(height) = Self::visible_span(
            layout_offset.dy,
            child_size.height,
            self.viewport_dimension.height,
        ) else {
            return Size::ZERO;
        };
        Size { width, height }
    }

    /// One axis of the clip. `None` means the child does not reach the
    /// viewport on this axis at all.
    fn visible_span(offset: f32, extent: f32, viewport: f32) -> Option<f32> {
        if offset < 0.0 {
            // Started before the leading edge. Upstream's comment: a child
            // starting at -50 has a paint extent of width + (-50).
            if offset + extent <= 0.0 {
                return None;
            }
            return Some(offset + extent);
        }
        if offset >= viewport {
            return None;
        }
        if offset + extent > viewport {
            return Some(viewport - offset);
        }
        Some(extent)
    }

    /// Upstream's `updateChildPaintData`, which turns the layout offset into a
    /// paint offset and an extent.
    ///
    /// The paint offset differs from the layout offset only when the scroll
    /// runs backwards: `up` measures the layout from the bottom, so the paint
    /// offset has to be flipped back into viewport coordinates.
    pub fn update_child_paint_data(
        &mut self,
        vicinity: ChildVicinity,
        child_size: Size,
        vertical_direction: AxisDirection,
        horizontal_direction: AxisDirection,
    ) {
        let viewport = self.viewport_dimension;
        let Some(data) = self.children.get(&vicinity) else {
            return;
        };
        let layout_offset = data
            .layout_offset
            .expect("the child was not given a layoutOffset during layoutChildSequence");
        let extent = self.compute_child_paint_extent(layout_offset, child_size);
        let dx = match horizontal_direction {
            AxisDirection::Left => viewport.width - (layout_offset.dx + child_size.width),
            _ => layout_offset.dx,
        };
        let dy = match vertical_direction {
            AxisDirection::Up => viewport.height - (layout_offset.dy + child_size.height),
            _ => layout_offset.dy,
        };
        if let Some(data) = self.children.get_mut(&vicinity) {
            data.set_paint_extent(extent);
            data.paint_offset = Some(Offset { dx, dy });
        }
    }

    /// Upstream's `_cacheKeepAlives`: whatever this pass did **not** ask for,
    /// but which asked to be kept, moves into the bucket.
    ///
    /// The set difference is the whole of it. A child the layout stopped
    /// asking for has scrolled out of range; if it wanted keeping it is kept,
    /// and otherwise the child manager is free to dispose of it.
    pub fn cache_keep_alives(&mut self) {
        let leaving: Vec<ChildVicinity> = self
            .children
            .keys()
            .copied()
            .filter(|vicinity| !self.active_this_pass.contains(vicinity))
            .collect();
        for vicinity in leaving {
            let Some(data) = self.children.get(&vicinity).copied() else {
                continue;
            };
            if data.keep_alive {
                self.children.remove(&vicinity);
                self.keep_alive_bucket.insert(vicinity, data);
            } else {
                self.children.remove(&vicinity);
            }
        }
    }

    /// Upstream's `_reifyChildren`: settle paint order.
    ///
    /// A **vertical** main axis sorts by y then x -- row major. Upstream's own
    /// comment admits this seems backwards and then says why: vertical is
    /// Flutter's default scroll axis and row-major is the default for
    /// matrices, so making the two defaults agree inverts the mapping between
    /// them.
    ///
    /// A vicinity with no child is skipped rather than being an error: a table
    /// with merged cells has one child spanning several, and the ones it
    /// swallowed never get built.
    pub fn reify_children(&mut self) {
        let mut order = self.current_vicinities.clone();
        match self.main_axis {
            Axis::Vertical => order.sort_by(ChildVicinity::compare_row_major),
            Axis::Horizontal => order.sort_by(ChildVicinity::compare_column_major),
        }
        self.paint_order = order
            .into_iter()
            .filter(|vicinity| self.children.contains_key(vicinity))
            .collect();
        self.current_vicinities.clear();
    }

    /// Upstream's `visitChildren`: paint order, **then** the keep-alive
    /// bucket. The bucket is not in paint order because it is not painted.
    pub fn visit_children(&self) -> Vec<ChildVicinity> {
        let mut visited = self.paint_order.clone();
        let mut kept: Vec<ChildVicinity> = self.keep_alive_bucket.keys().copied().collect();
        kept.sort();
        visited.extend(kept);
        visited
    }

    /// Upstream's `visitChildrenForSemantics`, which pointedly **omits** the
    /// keep-alive bucket. A screen reader announcing rows the reader has
    /// scrolled past would be reading out a table nobody is looking at.
    pub fn visit_children_for_semantics(&self) -> Vec<ChildVicinity> {
        self.paint_order.clone()
    }
}

/// Upstream `DiagonalDragBehavior`: how much of a diagonal drag is honoured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiagonalDragBehavior {
    /// No diagonal scrolling at all: the first direction wins and locks the
    /// axis until the finger lifts.
    #[default]
    None,
    /// Weighed once, at the start, and that verdict stands for the gesture.
    WeightedEvent,
    /// Weighed again on every update, so a drag can change its mind mid-way.
    WeightedContinuous,
    /// Free movement in any direction.
    Free,
}

/// Upstream `TwoDimensionalScrollView`: two scrollables at right angles.
#[derive(Clone, Debug, PartialEq)]
pub struct TwoDimensionalScrollView {
    /// Which axis is the "main" one. It decides paint order below, and up here
    /// it decides which axis `primary` refers to.
    pub main_axis: Axis,
    pub vertical_details: ScrollableDetails,
    pub horizontal_details: ScrollableDetails,
    pub diagonal_drag_behavior: DiagonalDragBehavior,
    /// Upstream's `primary`. `None` means "decide from the context".
    pub primary: Option<bool>,
    /// Whether the main axis was given a controller of its own.
    pub main_axis_has_controller: bool,
    pub cache_extent: Option<f32>,
}

impl Default for TwoDimensionalScrollView {
    fn default() -> TwoDimensionalScrollView {
        TwoDimensionalScrollView::new()
    }
}

impl TwoDimensionalScrollView {
    pub fn new() -> TwoDimensionalScrollView {
        TwoDimensionalScrollView {
            main_axis: Axis::Vertical,
            vertical_details: ScrollableDetails::vertical(false),
            horizontal_details: ScrollableDetails::horizontal(false),
            diagonal_drag_behavior: DiagonalDragBehavior::None,
            primary: None,
            main_axis_has_controller: false,
            cache_extent: None,
        }
    }

    pub fn with_main_axis(mut self, main_axis: Axis) -> Self {
        self.main_axis = main_axis;
        self
    }

    pub fn with_primary(mut self, primary: Option<bool>) -> Self {
        self.primary = primary;
        self
    }

    pub fn with_main_axis_controller(mut self, has_controller: bool) -> Self {
        self.main_axis_has_controller = has_controller;
        self
    }

    /// Upstream's two build-time assertions: each set of details must actually
    /// describe its own axis. Swapping them would transpose the whole view
    /// silently.
    pub fn details_are_valid(&self) -> bool {
        axis_direction_to_axis(self.vertical_details.direction) == Axis::Vertical
            && axis_direction_to_axis(self.horizontal_details.direction) == Axis::Horizontal
    }

    /// The details for the main axis.
    pub fn main_axis_details(&self) -> &ScrollableDetails {
        match self.main_axis {
            Axis::Vertical => &self.vertical_details,
            Axis::Horizontal => &self.horizontal_details,
        }
    }

    /// Upstream's `_shouldInheritPrimary` condition: a primary scroll
    /// controller is taken **only when the main axis has no controller of its
    /// own**, and `primary` was not explicitly false.
    ///
    /// The pairing is a real constraint rather than a preference -- upstream
    /// asserts on it. Two controllers driving one axis would each believe they
    /// owned the scroll position.
    pub fn should_inherit_primary(&self, context_offers_primary: bool) -> bool {
        match self.primary {
            Some(false) => false,
            Some(true) => true,
            None => !self.main_axis_has_controller && context_offers_primary,
        }
    }

    /// Upstream's assertion, stated as a question: is this configuration legal?
    /// `primary: true` with a controller on the main axis is not.
    pub fn primary_configuration_is_valid(&self) -> bool {
        !(self.primary == Some(true) && self.main_axis_has_controller)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vicinity(x: i32, y: i32) -> ChildVicinity {
        ChildVicinity::new(x, y)
    }

    // -- Vicinities and paint order ----------------------------------------

    #[test]
    fn a_vertical_main_axis_paints_in_row_major_order() {
        // Upstream's own comment admits this seems backwards, and then gives
        // the reason: vertical is Flutter's default scroll axis and row-major
        // is the default for matrices, so making the two defaults agree
        // inverts the mapping between them.
        let mut viewport = RenderTwoDimensionalViewport::new(
            Axis::Vertical,
            Size {
                width: 100.0,
                height: 100.0,
            },
        );
        for (x, y) in [(1, 1), (0, 1), (1, 0), (0, 0)] {
            viewport.did_produce_child(vicinity(x, y));
        }
        viewport.reify_children();
        assert_eq!(
            viewport.paint_order(),
            &[
                vicinity(0, 0),
                vicinity(1, 0),
                vicinity(0, 1),
                vicinity(1, 1),
            ],
            "across a row before advancing down"
        );
    }

    #[test]
    fn a_horizontal_main_axis_paints_in_column_major_order() {
        let mut viewport = RenderTwoDimensionalViewport::new(
            Axis::Horizontal,
            Size {
                width: 100.0,
                height: 100.0,
            },
        );
        for (x, y) in [(1, 1), (0, 1), (1, 0), (0, 0)] {
            viewport.did_produce_child(vicinity(x, y));
        }
        viewport.reify_children();
        assert_eq!(
            viewport.paint_order(),
            &[
                vicinity(0, 0),
                vicinity(0, 1),
                vicinity(1, 0),
                vicinity(1, 1),
            ],
            "down a column before advancing across"
        );
    }

    #[test]
    fn the_two_orderings_are_genuinely_different_keys() {
        let a = vicinity(0, 5);
        let b = vicinity(1, 0);
        assert_eq!(a.compare_column_major(&b), std::cmp::Ordering::Less);
        assert_eq!(a.compare_row_major(&b), std::cmp::Ordering::Greater);
        assert_eq!(a.cmp(&b), a.compare_column_major(&b), "Ord is column major");
    }

    #[test]
    fn the_invalid_vicinity_is_the_only_negative_one() {
        assert_eq!(ChildVicinity::INVALID, vicinity(-1, -1));
        assert!(!ChildVicinity::INVALID.is_valid());
        assert!(vicinity(0, 0).is_valid());
        assert_eq!(ChildVicinity::default(), ChildVicinity::INVALID);
        assert_eq!(vicinity(2, 3).to_string(), "(xIndex: 2, yIndex: 3)");
    }

    #[test]
    fn a_vicinity_with_no_child_is_skipped_rather_than_being_an_error() {
        // A table with merged cells has one child spanning several vicinities,
        // and the ones it swallowed never get built.
        let mut viewport = RenderTwoDimensionalViewport::new(
            Axis::Vertical,
            Size {
                width: 100.0,
                height: 100.0,
            },
        );
        viewport.did_produce_child(vicinity(0, 0));
        viewport.did_produce_child(vicinity(2, 0));
        viewport.reify_children();
        assert_eq!(viewport.paint_order(), &[vicinity(0, 0), vicinity(2, 0)]);
        assert_eq!(viewport.child_after(vicinity(0, 0)), Some(vicinity(2, 0)));
        assert_eq!(viewport.child_before(vicinity(0, 0)), None);
        assert_eq!(viewport.child_after(vicinity(2, 0)), None);
    }

    #[test]
    fn the_first_and_last_child_come_from_the_settled_order() {
        let mut viewport = RenderTwoDimensionalViewport::new(
            Axis::Vertical,
            Size {
                width: 100.0,
                height: 100.0,
            },
        );
        assert_eq!(
            viewport.first_child(),
            None,
            "nothing decided during layout"
        );

        viewport.did_produce_child(vicinity(3, 0));
        viewport.did_produce_child(vicinity(0, 2));
        viewport.reify_children();
        assert_eq!(viewport.first_child(), Some(vicinity(3, 0)));
        assert_eq!(viewport.last_child(), Some(vicinity(0, 2)));
    }

    // -- What is on screen -------------------------------------------------

    fn viewport_100() -> RenderTwoDimensionalViewport {
        RenderTwoDimensionalViewport::new(
            Axis::Vertical,
            Size {
                width: 100.0,
                height: 100.0,
            },
        )
    }

    #[test]
    fn a_child_wholly_inside_the_viewport_paints_all_of_itself() {
        let viewport = viewport_100();
        let extent = viewport.compute_child_paint_extent(
            Offset { dx: 10.0, dy: 10.0 },
            Size {
                width: 20.0,
                height: 20.0,
            },
        );
        assert_eq!(
            extent,
            Size {
                width: 20.0,
                height: 20.0
            }
        );
    }

    #[test]
    fn a_child_hanging_off_the_leading_edge_paints_what_is_left_of_it() {
        // Upstream's comment: a child starting at -50 has a paint extent of
        // width + (-50).
        let viewport = viewport_100();
        let extent = viewport.compute_child_paint_extent(
            Offset { dx: -50.0, dy: 0.0 },
            Size {
                width: 80.0,
                height: 20.0,
            },
        );
        assert_eq!(
            extent,
            Size {
                width: 30.0,
                height: 20.0
            }
        );
    }

    #[test]
    fn a_child_entirely_off_either_edge_paints_nothing() {
        let viewport = viewport_100();
        let size = Size {
            width: 20.0,
            height: 20.0,
        };
        assert_eq!(
            viewport.compute_child_paint_extent(Offset { dx: -20.0, dy: 0.0 }, size),
            Size::ZERO,
            "exactly abutting the leading edge is still outside"
        );
        assert_eq!(
            viewport.compute_child_paint_extent(Offset { dx: 100.0, dy: 0.0 }, size),
            Size::ZERO,
            "and starting at the trailing edge is too"
        );
        assert_eq!(
            viewport.compute_child_paint_extent(Offset { dx: 0.0, dy: 200.0 }, size),
            Size::ZERO,
            "one axis being off screen is enough"
        );
    }

    #[test]
    fn a_child_overhanging_the_trailing_edge_is_clipped_to_it() {
        let viewport = viewport_100();
        let extent = viewport.compute_child_paint_extent(
            Offset { dx: 90.0, dy: 90.0 },
            Size {
                width: 40.0,
                height: 40.0,
            },
        );
        assert_eq!(
            extent,
            Size {
                width: 10.0,
                height: 10.0
            }
        );
    }

    #[test]
    fn a_child_with_no_area_is_invisible_wherever_it_is() {
        // Clipping it would otherwise give it an extent inside the viewport
        // and make it look visible.
        let viewport = viewport_100();
        assert_eq!(
            viewport.compute_child_paint_extent(
                Offset { dx: 10.0, dy: 10.0 },
                Size {
                    width: 0.0,
                    height: 50.0
                }
            ),
            Size::ZERO
        );
        assert_eq!(
            viewport.compute_child_paint_extent(
                Offset { dx: 10.0, dy: 10.0 },
                Size {
                    width: 50.0,
                    height: 0.0
                }
            ),
            Size::ZERO
        );
    }

    #[test]
    fn visibility_is_exactly_a_non_zero_paint_extent() {
        // Upstream's expression has two clauses after the first that cannot
        // fire: if the extent is not Size.zero then neither dimension test
        // adds anything, and if it is, both are false too.
        let mut data = TwoDimensionalViewportParentData::new();
        data.set_paint_extent(Size::ZERO);
        assert!(!data.is_visible());

        data.set_paint_extent(Size {
            width: 0.0,
            height: 10.0,
        });
        assert!(
            data.is_visible(),
            "a sliver of a child is still a visible child"
        );
    }

    #[test]
    #[should_panic(expected = "paint extent of the child has not been determined")]
    fn asking_whether_a_child_is_visible_before_laying_it_out_is_a_mistake() {
        let _ = TwoDimensionalViewportParentData::new().is_visible();
    }

    #[test]
    fn a_child_is_only_kept_alive_when_it_is_not_simply_there() {
        // Something on screen is not being kept alive.
        let mut data = TwoDimensionalViewportParentData::new();
        data.keep_alive = true;
        data.set_paint_extent(Size {
            width: 10.0,
            height: 10.0,
        });
        assert!(!data.kept_alive(), "visible, so not kept");

        data.set_paint_extent(Size::ZERO);
        assert!(data.kept_alive());

        data.keep_alive = false;
        assert!(!data.kept_alive());
    }

    #[test]
    fn a_backwards_scroll_paints_somewhere_other_than_it_laid_out() {
        // The layout offset is measured from the leading edge of the scroll;
        // the paint offset from the top left of the viewport. They coincide
        // only for down and right.
        let mut viewport = viewport_100();
        let at = vicinity(0, 0);
        viewport.did_produce_child(at);
        viewport.set_layout_offset(at, Offset { dx: 10.0, dy: 10.0 });
        let size = Size {
            width: 20.0,
            height: 20.0,
        };

        viewport.update_child_paint_data(at, size, AxisDirection::Down, AxisDirection::Right);
        let data = viewport.parent_data_of(at).unwrap();
        assert_eq!(data.paint_offset, data.layout_offset, "same going forwards");

        viewport.update_child_paint_data(at, size, AxisDirection::Up, AxisDirection::Left);
        let data = viewport.parent_data_of(at).unwrap();
        assert_eq!(
            data.paint_offset,
            Some(Offset { dx: 70.0, dy: 70.0 }),
            "and flipped back into viewport coordinates going backwards"
        );
        assert_eq!(
            data.layout_offset,
            Some(Offset { dx: 10.0, dy: 10.0 }),
            "while the layout offset never moved"
        );
    }

    // -- Building, reusing, and keeping alive ------------------------------

    #[test]
    fn a_child_that_is_already_there_is_reused_rather_than_rebuilt() {
        // Rebuilding it would throw away its state on every scroll frame.
        let mut viewport = viewport_100();
        let at = vicinity(0, 0);
        assert_eq!(viewport.build_or_obtain_child_for(at), ChildRequest::Build);
        viewport.did_produce_child(at);

        assert_eq!(
            viewport.build_or_obtain_child_for(at),
            ChildRequest::Reuse,
            "second pass"
        );
    }

    #[test]
    fn a_changed_delegate_invalidates_even_children_that_did_not_move() {
        // Reusing them would show the reader the old delegate's content in the
        // new layout.
        let mut viewport = viewport_100();
        let at = vicinity(0, 0);
        viewport.build_or_obtain_child_for(at);
        viewport.did_produce_child(at);

        viewport.needs_delegate_rebuild = true;
        assert_eq!(viewport.build_or_obtain_child_for(at), ChildRequest::Build);
    }

    #[test]
    fn a_child_coming_back_out_of_the_bucket_is_a_reuse_and_not_a_build() {
        // Which is the whole point of the bucket: it scrolled away and came
        // back with its state intact.
        let mut viewport = viewport_100();
        let at = vicinity(0, 0);
        viewport.build_or_obtain_child_for(at);
        viewport.did_produce_child(at);
        viewport.set_keep_alive(at, true);

        // A pass that does not ask for it.
        viewport.start_layout();
        viewport.cache_keep_alives();
        assert!(viewport.keep_alive_bucket().contains_key(&at));
        assert!(!viewport.children().contains_key(&at));

        assert_eq!(viewport.build_or_obtain_child_for(at), ChildRequest::Reuse);
        assert!(viewport.children().contains_key(&at));
        assert!(viewport.keep_alive_bucket().is_empty());
    }

    #[test]
    fn a_child_that_scrolled_away_without_asking_to_be_kept_simply_goes() {
        let mut viewport = viewport_100();
        let at = vicinity(0, 0);
        viewport.build_or_obtain_child_for(at);
        viewport.did_produce_child(at);

        viewport.start_layout();
        viewport.cache_keep_alives();
        assert!(viewport.children().is_empty());
        assert!(viewport.keep_alive_bucket().is_empty());
    }

    #[test]
    fn only_children_this_pass_did_not_ask_for_are_considered_for_the_bucket() {
        let mut viewport = viewport_100();
        let stays = vicinity(0, 0);
        let leaves = vicinity(0, 9);
        for at in [stays, leaves] {
            viewport.build_or_obtain_child_for(at);
            viewport.did_produce_child(at);
            viewport.set_keep_alive(at, true);
        }

        viewport.start_layout();
        viewport.build_or_obtain_child_for(stays);
        viewport.did_produce_child(stays);
        viewport.cache_keep_alives();

        assert!(viewport.children().contains_key(&stays));
        assert_eq!(
            viewport.keep_alive_bucket().keys().collect::<Vec<_>>(),
            vec![&leaves]
        );
    }

    #[test]
    fn a_kept_alive_child_is_visited_but_never_announced() {
        // A screen reader reading out rows the reader has scrolled past would
        // be reading a table nobody is looking at.
        let mut viewport = viewport_100();
        let on_screen = vicinity(0, 0);
        let kept = vicinity(0, 9);
        for at in [on_screen, kept] {
            viewport.build_or_obtain_child_for(at);
            viewport.did_produce_child(at);
            viewport.set_keep_alive(at, true);
        }
        viewport.start_layout();
        viewport.build_or_obtain_child_for(on_screen);
        viewport.did_produce_child(on_screen);
        viewport.cache_keep_alives();
        viewport.reify_children();

        assert_eq!(viewport.visit_children(), vec![on_screen, kept]);
        assert_eq!(viewport.visit_children_for_semantics(), vec![on_screen]);
    }

    // -- The scroll view ---------------------------------------------------

    #[test]
    fn each_set_of_details_has_to_describe_its_own_axis() {
        // Swapping them would transpose the whole view silently.
        let view = TwoDimensionalScrollView::new();
        assert!(view.details_are_valid());

        let mut swapped = TwoDimensionalScrollView::new();
        swapped.vertical_details = ScrollableDetails::horizontal(false);
        assert!(!swapped.details_are_valid());
    }

    #[test]
    fn the_main_axis_is_the_one_primary_refers_to() {
        let vertical = TwoDimensionalScrollView::new();
        assert_eq!(
            axis_direction_to_axis(vertical.main_axis_details().direction),
            Axis::Vertical
        );

        let horizontal = TwoDimensionalScrollView::new().with_main_axis(Axis::Horizontal);
        assert_eq!(
            axis_direction_to_axis(horizontal.main_axis_details().direction),
            Axis::Horizontal
        );
    }

    #[test]
    fn a_primary_controller_is_taken_only_when_the_main_axis_has_none_of_its_own() {
        // Two controllers driving one axis would each believe they owned the
        // scroll position, which is why upstream asserts rather than picking.
        let plain = TwoDimensionalScrollView::new();
        assert!(plain.should_inherit_primary(true));
        assert!(!plain.should_inherit_primary(false), "context said no");

        let owned = TwoDimensionalScrollView::new().with_main_axis_controller(true);
        assert!(!owned.should_inherit_primary(true));

        let refused = TwoDimensionalScrollView::new().with_primary(Some(false));
        assert!(!refused.should_inherit_primary(true));
    }

    #[test]
    fn asking_for_primary_while_holding_a_controller_is_the_illegal_pair() {
        assert!(TwoDimensionalScrollView::new().primary_configuration_is_valid());
        assert!(
            TwoDimensionalScrollView::new()
                .with_main_axis_controller(true)
                .primary_configuration_is_valid(),
            "a controller on its own is fine"
        );
        assert!(
            !TwoDimensionalScrollView::new()
                .with_primary(Some(true))
                .with_main_axis_controller(true)
                .primary_configuration_is_valid()
        );
    }

    #[test]
    fn no_diagonal_scrolling_is_the_default() {
        // A drag that wanders is usually a drag the reader meant to be
        // straight.
        assert_eq!(
            TwoDimensionalScrollView::new().diagonal_drag_behavior,
            DiagonalDragBehavior::None
        );
        assert_eq!(DiagonalDragBehavior::default(), DiagonalDragBehavior::None);
    }
}
