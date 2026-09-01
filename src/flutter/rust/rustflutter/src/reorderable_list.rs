//! Dragging a row to a new place in a list -- a port of upstream's
//! `widgets/reorderable_list.dart`.
//!
//! Almost all of the difficulty here is in one question, asked on every frame
//! of a drag: **given where the dragged row is now, which index should it land
//! at?** Upstream answers it in `_dragUpdateItems`, and the answer is not "the
//! nearest row" -- it is a set of rules about where the *edges* of the moving
//! row fall relative to the halves of each stationary one. Those rules are
//! ported here as [`insert_index_for`], which is a pure function of geometry
//! and can be reasoned about on its own.
//!
//! The second-hardest part is that the answer means two different things.
//! While the drag is happening, the insert index is computed **with the
//! dragged row still in the list**; once the drag is dropped, the row is
//! removed first and then inserted, which shifts every later index down by
//! one. [`reordered_index`] is that adjustment, and it is the classic way to
//! get a reorderable list wrong.
//!
//! ## What is not here
//!
//! The drag proxy is an `OverlayEntry` upstream. [`crate::overlay`] has the
//! entry bookkeeping but nothing that hosts the widget yet;
//! the auto-scroll at the edges is an `EdgeDraggingAutoScroller`, which is not
//! ported either. Both are about *showing* the drag rather than about deciding
//! what it means, and the deciding is what this module carries.

use crate::engine::Rect;
use crate::multidrag::{
    DelayedMultiDragGestureRecognizer, ImmediateMultiDragGestureRecognizer,
    MultiDragGestureRecognizer,
};
use crate::render::{Axis, EdgeInsets, Offset};
use crate::scroll_view::ScrollView;
use crate::scrolling::{CacheExtentStyle, ScrollCacheExtent};

/// Where a stationary row sits while another is being dragged past it.
///
/// Upstream's `_ReorderableItemState.updateForGap`. The gap opens **between**
/// the row being dragged and the position it would currently land at, and only
/// the rows strictly between those two move at all.
///
/// The two branches are the two directions of travel: dragging upwards
/// (`gap_index < drag_index`) pushes the rows in between *down*, and dragging
/// downwards pushes them *up*. A reversed list swaps both, because in a
/// reversed list "down the screen" is "earlier in the list".
pub fn gap_offset(
    index: usize,
    drag_index: usize,
    gap_index: usize,
    gap_extent: f32,
    reverse: bool,
) -> f32 {
    if gap_index < drag_index && index < drag_index && index >= gap_index {
        if reverse { -gap_extent } else { gap_extent }
    } else if gap_index > drag_index && index > drag_index && index < gap_index {
        if reverse { gap_extent } else { -gap_extent }
    } else {
        0.0
    }
}

/// One stationary row, as [`insert_index_for`] needs to see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReorderableItemGeometry {
    pub index: usize,
    /// Upstream's `targetGeometry()`: where the row will be once its gap
    /// animation has finished, not where it is mid-animation. Asking about the
    /// destination rather than the current position is what keeps the answer
    /// from oscillating while the rows are still sliding.
    pub geometry: Rect,
}

fn start_and_extent(rect: Rect, axis: Axis) -> (f32, f32) {
    match axis {
        Axis::Vertical => (rect.top, rect.height()),
        Axis::Horizontal => (rect.left, rect.width()),
    }
}

/// Upstream's `_dragUpdateItems`: where the dragged row would land.
///
/// `proxy_start` is the leading edge of the dragged row in the scroll
/// direction, and `gap_extent` its length along that axis.
///
/// The rules read the **edges of the moving row against the halves of each
/// stationary one**, which is what makes the swap feel like it happens when
/// the rows visibly overlap rather than when their centres cross:
///
/// * the moving row's start landing in the first half of a stationary row
///   means it goes *before* that row, and the search stops;
/// * its end landing in the second half means it goes *after*, and the search
///   stops;
/// * a row entirely before or after the moving row only ever moves the answer
///   in its own direction, and never past an answer already found.
///
/// The last of those is the reason for the `new_index <` and `new_index >`
/// guards, which look redundant and are not: the rows are walked in map order,
/// so a later iteration must not undo an earlier one's conclusion.
pub fn insert_index_for(
    items: &[ReorderableItemGeometry],
    drag_index: usize,
    current_insert_index: usize,
    proxy_start: f32,
    gap_extent: f32,
    axis: Axis,
    reverse: bool,
) -> usize {
    let proxy_end = proxy_start + gap_extent;
    let mut new_index = current_insert_index;

    for item in items {
        // In a reversed list the dragged row itself is skipped outright; going
        // forwards it is handled, but only by the one clause below.
        if reverse && item.index == drag_index {
            continue;
        }
        let (item_start, item_extent) = start_and_extent(item.geometry, axis);
        let item_end = item_start + item_extent;
        let item_middle = item_start + item_extent / 2.0;

        if reverse {
            if item_end >= proxy_end && proxy_end >= item_middle {
                new_index = item.index;
                break;
            } else if item_middle >= proxy_start && proxy_start >= item_start {
                new_index = item.index + 1;
                break;
            } else if item_start > proxy_end && new_index < item.index + 1 {
                new_index = item.index + 1;
            } else if proxy_start > item_end && new_index > item.index {
                new_index = item.index;
            }
        } else if item.index == drag_index {
            // Upstream's comment: if the end of the proxy is not in the ending
            // half of the row, do not process it, because it is the row being
            // dragged. The row still has to be able to claim its *own* index
            // back, which is what this clause is for.
            if item_middle <= proxy_end && proxy_end <= item_end {
                new_index = drag_index;
            }
        } else if item_start <= proxy_start && proxy_start <= item_middle {
            new_index = item.index;
            break;
        } else if item_middle <= proxy_end && proxy_end <= item_end {
            new_index = item.index + 1;
            break;
        } else if item_end < proxy_start && new_index < item.index + 1 {
            new_index = item.index + 1;
        } else if proxy_end < item_start && new_index > item.index {
            new_index = item.index;
        }
    }
    new_index
}

/// Upstream's index adjustment inside `_handleReorderItem`.
///
/// **The insert index was computed with the dragged row still in the list.**
/// Actually reordering removes it first, which shortens everything after it by
/// one, so an index past the old position has to come down by one to mean the
/// same place. Getting this wrong moves rows one further than the reader
/// dropped them, and only in one direction, which is why it survives casual
/// testing.
pub fn reordered_index(old_index: usize, new_index: usize) -> usize {
    if new_index > old_index {
        new_index - 1
    } else {
        new_index
    }
}

/// What a drop should tell each of upstream's two reorder callbacks.
///
/// Upstream has both `onReorder` and `onReorderItem` and they do **not**
/// receive the same numbers, which is easy to miss: `onReorder` gets the raw
/// pair and is called whenever they differ, while `onReorderItem` gets the
/// adjusted pair and is skipped when the adjustment makes them equal -- that
/// case being a row dropped back where it started, by the long way round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReorderReport {
    /// Call `onReorder(old_index, new_index)` with the raw pair.
    Raw { old_index: usize, new_index: usize },
    /// Call `onReorderItem(old_index, new_index)` with the adjusted pair.
    Adjusted { old_index: usize, new_index: usize },
    /// Nothing moved.
    Nothing,
}

/// Upstream's `_handleReorderItem`.
pub fn reorder_report(old_index: usize, new_index: usize, has_on_reorder: bool) -> ReorderReport {
    if has_on_reorder && old_index != new_index {
        return ReorderReport::Raw {
            old_index,
            new_index,
        };
    }
    let adjusted = reordered_index(old_index, new_index);
    if old_index != adjusted {
        ReorderReport::Adjusted {
            old_index,
            new_index: adjusted,
        }
    } else {
        ReorderReport::Nothing
    }
}

/// Which of the four cases a drop falls into, upstream's `_dragEnd`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropCase {
    /// The row is going back where it came from. Upstream's comment explains
    /// why this is `insert_index == index + 1` and not `== index`: coming back
    /// from *below*, the insert index is computed with the dragged row still
    /// present, so returning to the original position reads as one past it.
    BackFromBelow,
    /// No movement at all.
    Unmoved,
    /// Dropping before the row currently at the insert position.
    BeforeItem,
    /// Dropping after the row before the insert position.
    AfterPrevious,
}

/// Upstream's `_dragEnd`, reduced to which case applies.
pub fn drop_case(index: usize, insert_index: usize, item_count: usize, reverse: bool) -> DropCase {
    if insert_index == index + 1 {
        DropCase::BackFromBelow
    } else if insert_index == index {
        DropCase::Unmoved
    } else if reverse {
        if insert_index >= item_count {
            DropCase::AfterPrevious
        } else {
            DropCase::BeforeItem
        }
    } else if insert_index == 0 {
        DropCase::BeforeItem
    } else {
        DropCase::AfterPrevious
    }
}

// -- The listeners ------------------------------------------------------------

/// Upstream `ReorderableDragStartListener`: a wrapper that starts a reorder as
/// soon as a finger lands on it.
///
/// Meant for a *drag handle* rather than a whole row -- a listener that claims
/// the gesture immediately cannot share a row with a scroll, because the two
/// are the same gesture at the moment it starts.
pub struct ReorderableDragStartListener {
    pub index: usize,
    /// Upstream's `enabled`. A row that cannot be moved still builds
    /// identically; only the listener goes away, so nothing shifts when a list
    /// is locked.
    pub enabled: bool,
}

impl ReorderableDragStartListener {
    pub fn new(index: usize) -> ReorderableDragStartListener {
        ReorderableDragStartListener {
            index,
            enabled: true,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Upstream's `createRecognizer`, which is the single point of difference
    /// between this and [`ReorderableDelayedDragStartListener`].
    pub fn create_recognizer(&self) -> MultiDragGestureRecognizer {
        ImmediateMultiDragGestureRecognizer::new().base
    }
}

/// Upstream `ReorderableDelayedDragStartListener`: the same thing, after a
/// hold.
///
/// Meant for a whole row. Inside a scroll view an immediate drag and a scroll
/// are the same gesture and direction cannot tell them apart -- the row moves
/// the way the list scrolls -- so the hold is what separates them.
pub struct ReorderableDelayedDragStartListener {
    pub base: ReorderableDragStartListener,
}

impl ReorderableDelayedDragStartListener {
    pub fn new(index: usize) -> ReorderableDelayedDragStartListener {
        ReorderableDelayedDragStartListener {
            base: ReorderableDragStartListener::new(index),
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.base.enabled = enabled;
        self
    }

    /// Upstream's `createRecognizer` override.
    pub fn create_recognizer(&self) -> MultiDragGestureRecognizer {
        DelayedMultiDragGestureRecognizer::new().base
    }
}

impl std::ops::Deref for ReorderableDelayedDragStartListener {
    type Target = ReorderableDragStartListener;

    fn deref(&self) -> &ReorderableDragStartListener {
        &self.base
    }
}

// -- The lists ----------------------------------------------------------------

/// Why a call to [`SliverReorderableListState::start_item_drag_reorder`] did
/// what it did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragStartOutcome {
    /// The drag was set up on this index.
    Started,
    /// A drag was already in progress, so it was cancelled first and this one
    /// started.
    CancelledPrevious,
    /// Upstream throws here: a drag cannot start on a row that is not on
    /// screen, because there is no geometry to reorder against.
    ItemNotVisible,
}

/// Upstream `SliverReorderableListState`: the bookkeeping a reorder needs.
///
/// The overlay proxy and the auto-scroller are not ported (see the module
/// docs); what is here is which row is being dragged, where it would land, and
/// the rules that move those two.
#[derive(Debug, Default)]
pub struct SliverReorderableListState {
    /// Upstream's `_items`: the rows currently on screen, by index.
    items: Vec<ReorderableItemGeometry>,
    drag_index: Option<usize>,
    insert_index: Option<usize>,
    /// Upstream's `_recognizerPointer`, which is why a second finger does not
    /// steal a drag that a first one started.
    recognizer_pointer: Option<i64>,
    pub item_count: usize,
    pub reverse: bool,
    pub axis: Axis,
}

impl SliverReorderableListState {
    pub fn new(item_count: usize) -> SliverReorderableListState {
        SliverReorderableListState {
            items: Vec::new(),
            drag_index: None,
            insert_index: None,
            recognizer_pointer: None,
            item_count,
            reverse: false,
            axis: Axis::Vertical,
        }
    }

    pub fn with_axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    pub fn with_reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Upstream's `_registerItem`.
    pub fn register_item(&mut self, item: ReorderableItemGeometry) {
        match self.items.iter().position(|held| held.index == item.index) {
            Some(at) => self.items[at] = item,
            None => self.items.push(item),
        }
    }

    /// Upstream's `_unregisterItem`, which only removes the row if the one
    /// registered at that index is still the same one -- a row that has
    /// already been replaced must not take its successor with it.
    pub fn unregister_item(&mut self, index: usize, geometry: Rect) {
        if let Some(at) = self
            .items
            .iter()
            .position(|held| held.index == index && held.geometry == geometry)
        {
            self.items.remove(at);
        }
    }

    pub fn items(&self) -> &[ReorderableItemGeometry] {
        &self.items
    }

    pub fn drag_index(&self) -> Option<usize> {
        self.drag_index
    }

    pub fn insert_index(&self) -> Option<usize> {
        self.insert_index
    }

    pub fn is_dragging(&self) -> bool {
        self.drag_index.is_some()
    }

    /// Upstream's `startItemDragReorder`.
    ///
    /// The recogniser bookkeeping is the interesting half: a drag already in
    /// progress is cancelled, and a recogniser left over from a *different*
    /// pointer is thrown away. Keeping it would mean a second finger inheriting
    /// the first one's half-finished gesture.
    pub fn start_item_drag_reorder(&mut self, index: usize, pointer: i64) -> DragStartOutcome {
        debug_assert!(index < self.item_count);
        let mut outcome = DragStartOutcome::Started;
        if self.drag_index.is_some() {
            self.cancel_reorder();
            outcome = DragStartOutcome::CancelledPrevious;
        } else if self.recognizer_pointer.is_some_and(|held| held != pointer) {
            self.recognizer_pointer = None;
        }
        if !self.items.iter().any(|item| item.index == index) {
            return DragStartOutcome::ItemNotVisible;
        }
        self.drag_index = Some(index);
        self.insert_index = Some(index);
        self.recognizer_pointer = Some(pointer);
        outcome
    }

    /// Upstream's `_dragUpdateItems`, applied to this state.
    pub fn drag_update(&mut self, proxy_start: f32, gap_extent: f32) -> Option<usize> {
        let drag_index = self.drag_index?;
        let insert_index = self.insert_index?;
        let next = insert_index_for(
            &self.items,
            drag_index,
            insert_index,
            proxy_start,
            gap_extent,
            self.axis,
            self.reverse,
        );
        self.insert_index = Some(next);
        Some(next)
    }

    /// Upstream's `_dropCompleted`, minus the animation: what the drop should
    /// report, and the state reset that follows it.
    pub fn drop_completed(&mut self, has_on_reorder: bool) -> ReorderReport {
        let report = match (self.drag_index, self.insert_index) {
            (Some(old_index), Some(new_index)) => {
                reorder_report(old_index, new_index, has_on_reorder)
            }
            _ => ReorderReport::Nothing,
        };
        self.cancel_reorder();
        report
    }

    /// Upstream's `cancelReorder`, whose documentation is worth keeping: it
    /// should be called **before** any major change to the list, so that a
    /// drag in progress is not left confused by rows moving underneath it.
    pub fn cancel_reorder(&mut self) {
        self.drag_index = None;
        self.insert_index = None;
        self.recognizer_pointer = None;
    }

    /// Upstream's `didUpdateWidget`: a changed item count cancels any reorder,
    /// for exactly that reason.
    pub fn set_item_count(&mut self, item_count: usize) {
        if item_count != self.item_count {
            self.cancel_reorder();
        }
        self.item_count = item_count;
    }

    /// The offset a stationary row should sit at, given the drag in progress.
    pub fn gap_offset_for(&self, index: usize, gap_extent: f32) -> Offset {
        let (Some(drag_index), Some(gap_index)) = (self.drag_index, self.insert_index) else {
            return Offset::ZERO;
        };
        let along = gap_offset(index, drag_index, gap_index, gap_extent, self.reverse);
        match self.axis {
            Axis::Vertical => Offset::new(0.0, along),
            Axis::Horizontal => Offset::new(along, 0.0),
        }
    }
}

// -- The configuration all three constructors assert about --------------------

/// Why a reorderable list refused the way it was built.
///
/// One enum for all three constructors, because upstream asks the same two
/// questions in all three. `AChildWithoutAKey` is the exception: only
/// [`ReorderableListView`] can be given children directly, so only it can be
/// given one without a key, and [`ReorderableConfig::validate`] never returns
/// that variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReorderableError {
    MoreThanOneExtentSource,
    AChildWithoutAKey,
    BothCallbacks,
    NeitherCallback,
    /// One of the scroll view's own asserts, carried with its message. A
    /// reorderable list is a scroll view around a sliver, so everything
    /// upstream's `ScrollView` refuses it refuses too -- and refuses *later*,
    /// because the scroll view is built rather than constructed here. See
    /// [`crate::scroll_view::ScrollView::validate`].
    ScrollView(&'static str),
}

/// The part of a reorderable list's configuration that upstream asserts about,
/// *identically*, in `ReorderableList`, `SliverReorderableList` and
/// `ReorderableListView`.
///
/// The three constructors carry the same two asserts word for word. Writing
/// them out three times here would be three things that can drift, so they are
/// asked once and the three lists hold one of these. Only the Material list
/// wedges a third assert -- the child-key one -- between them, which is why
/// this type offers the two halves separately as well as together: putting the
/// key check in the middle is what upstream does, and the order decides which
/// complaint a list with two mistakes reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReorderableConfig {
    /// Upstream's `onReorderItem`, the callback that receives the *adjusted*
    /// pair. See [`reorder_report`].
    pub has_on_reorder_item: bool,
    /// Upstream's deprecated `onReorder`, which receives the raw pair.
    pub has_on_reorder: bool,
    pub has_item_extent: bool,
    pub has_prototype_item: bool,
    pub has_item_extent_builder: bool,
}

impl ReorderableConfig {
    /// A list with the modern callback and no fixed extent -- what the two
    /// widget-layer constructors look like with nothing optional supplied.
    pub fn new() -> ReorderableConfig {
        ReorderableConfig {
            has_on_reorder_item: true,
            has_on_reorder: false,
            has_item_extent: false,
            has_prototype_item: false,
            has_item_extent_builder: false,
        }
    }

    /// Upstream's extent assert, which is written as three pairwise tests
    /// rather than a count:
    ///
    /// ```dart
    /// (itemExtent == null && prototypeItem == null) ||
    ///     (itemExtent == null && itemExtentBuilder == null) ||
    ///     (prototypeItem == null && itemExtentBuilder == null),
    /// ```
    ///
    /// Each clause names the two that are *absent*, so the whole reads "some
    /// two of the three were left out". Equivalent to "at most one given", and
    /// it takes a minute to see why.
    pub fn check_extent_sources(&self) -> Result<(), ReorderableError> {
        let given = [
            self.has_item_extent,
            self.has_prototype_item,
            self.has_item_extent_builder,
        ]
        .iter()
        .filter(|given| **given)
        .count();
        if given > 1 {
            Err(ReorderableError::MoreThanOneExtentSource)
        } else {
            Ok(())
        }
    }

    /// Upstream's callback assert: **exactly one** of the two.
    ///
    /// Neither is refused rather than tolerated, which is worth noticing --
    /// a reorderable list that reports nothing when a row is dropped looks like
    /// a list that refuses to reorder, and the reader has no way to tell those
    /// apart. Both is refused because the two callbacks are handed different
    /// numbers for the same drop, and a list that fired both would be reporting
    /// one move twice, in two conventions.
    pub fn check_callbacks(&self) -> Result<(), ReorderableError> {
        match (self.has_on_reorder_item, self.has_on_reorder) {
            (true, true) => Err(ReorderableError::BothCallbacks),
            (false, false) => Err(ReorderableError::NeitherCallback),
            _ => Ok(()),
        }
    }

    /// Both asserts, in the order the two widget-layer constructors write them.
    pub fn validate(&self) -> Result<(), ReorderableError> {
        self.check_extent_sources()?;
        self.check_callbacks()
    }
}

impl Default for ReorderableConfig {
    fn default() -> ReorderableConfig {
        ReorderableConfig::new()
    }
}

/// Upstream `SliverReorderableList`: the sliver a reorderable list is made of.
pub struct SliverReorderableList {
    pub item_count: usize,
    pub axis: Axis,
    pub reverse: bool,
    pub config: ReorderableConfig,
    /// Upstream's `autoScrollerVelocityScalar`, whose default upstream calls
    /// `_kDefaultAutoScrollVelocityScalar` -- kept as a number even though the
    /// auto-scroller itself is not ported, so a caller configuring one is not
    /// silently ignored later.
    pub auto_scroller_velocity_scalar: f32,
}

impl SliverReorderableList {
    /// Upstream's `_kDefaultAutoScrollVelocityScalar`.
    pub const DEFAULT_AUTO_SCROLL_VELOCITY_SCALAR: f32 = 50.0;

    pub fn new(item_count: usize) -> SliverReorderableList {
        SliverReorderableList {
            item_count,
            axis: Axis::Vertical,
            reverse: false,
            auto_scroller_velocity_scalar: Self::DEFAULT_AUTO_SCROLL_VELOCITY_SCALAR,
            config: ReorderableConfig::new(),
        }
    }

    pub fn with_axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    pub fn with_reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    pub fn with_config(mut self, config: ReorderableConfig) -> Self {
        self.config = config;
        self
    }

    /// Upstream's constructor asserts.
    pub fn validate(&self) -> Result<(), ReorderableError> {
        self.config.validate()
    }

    /// Upstream's `createState`.
    pub fn create_state(&self) -> SliverReorderableListState {
        SliverReorderableListState::new(self.item_count)
            .with_axis(self.axis)
            .with_reverse(self.reverse)
    }
}

/// Upstream `ReorderableList`: the scroll view around a
/// [`SliverReorderableList`].
pub struct ReorderableList {
    pub sliver: SliverReorderableList,
    /// The scroll view upstream's `ReorderableListState.build` puts around the
    /// sliver, and everything it is configured with: upstream hands it
    /// scrollDirection, reverse, controller, primary, physics, shrinkWrap,
    /// anchor, dragStartBehavior, keyboardDismissBehavior, restorationId and
    /// clipBehavior straight from this widget. Those are not this list's rules
    /// -- they are [`ScrollView`]'s, already ported with its four asserts --
    /// so they live in one here rather than being declared a second time.
    pub scroll: ScrollView,
    /// Upstream's deprecated `cacheExtent`, a bare double. The sliver has no
    /// such field: the cache extent belongs to the scroll view around it.
    pub cache_extent: Option<f32>,
    /// Upstream's `scrollCacheExtent`, which replaces it.
    pub scroll_cache_extent: Option<ScrollCacheExtent>,
}

impl ReorderableList {
    pub fn new(item_count: usize) -> ReorderableList {
        ReorderableList {
            sliver: SliverReorderableList::new(item_count),
            scroll: ScrollView::new(),
            cache_extent: None,
            scroll_cache_extent: None,
        }
    }

    pub fn with_cache_extent(mut self, cache_extent: f32) -> Self {
        self.cache_extent = Some(cache_extent);
        self
    }

    pub fn with_scroll_cache_extent(mut self, extent: ScrollCacheExtent) -> Self {
        self.scroll_cache_extent = Some(extent);
        self
    }

    /// Upstream's `ReorderableListState._effectiveScrollCacheExtent`, which is
    /// [`ScrollCacheExtent::effective`] with the unit fixed at pixels: a
    /// `ReorderableList` never had a `cacheExtentStyle` to read one from, so
    /// upstream writes `ScrollCacheExtent.pixels(cacheExtent!)` outright.
    ///
    /// The getter lives on the *state* upstream and is read in `build`, which
    /// is not a detail: the list is what the reader configured, and the state
    /// is what hands the answer to the `CustomScrollView` it builds. Here the
    /// configuration is on the list, so the question is asked of the list.
    pub fn effective_scroll_cache_extent(&self) -> Option<ScrollCacheExtent> {
        ScrollCacheExtent::effective(
            self.scroll_cache_extent,
            self.cache_extent,
            CacheExtentStyle::Pixel,
        )
    }

    /// Sets the axis on **both** halves, because upstream has one axis in two
    /// places: the scroll view is given `scrollDirection`, and the sliver
    /// reads the same thing back off the constraints it is laid out under. The
    /// sliver's gap arithmetic needs it as a value here, so the two are kept in
    /// step through this one setter rather than being settable apart.
    pub fn with_axis(mut self, axis: Axis) -> Self {
        self.sliver = self.sliver.with_axis(axis);
        self.scroll.scroll_direction = axis;
        self
    }

    /// The same again for `reverse`, and for the same reason.
    pub fn with_reverse(mut self, reverse: bool) -> Self {
        self.sliver = self.sliver.with_reverse(reverse);
        self.scroll.reverse = reverse;
        self
    }

    pub fn with_config(mut self, config: ReorderableConfig) -> Self {
        self.sliver = self.sliver.with_config(config);
        self
    }

    /// Takes a whole scroll view, and carries its axis and direction down to
    /// the sliver -- see [`Self::with_axis`] for why those two travel together.
    pub fn with_scroll(mut self, scroll: ScrollView) -> Self {
        self.scroll = scroll;
        self.with_axis(scroll.scroll_direction)
            .with_reverse(scroll.reverse)
    }

    /// Upstream's constructor asserts, which are the sliver's asserts asked a
    /// second time: `ReorderableListState.build` hands every one of these
    /// straight down to the `SliverReorderableList` it builds, so a
    /// configuration this refuses is one the sliver would refuse anyway. It is
    /// asked here because the reader who wrote the mistake wrote it *here*, and
    /// upstream's third assert -- `itemCount >= 0` -- is not asked at all,
    /// because a `usize` has already answered it.
    pub fn validate(&self) -> Result<(), ReorderableError> {
        self.sliver.validate()?;
        self.scroll.validate().map_err(ReorderableError::ScrollView)
    }

    /// Upstream's `createState`.
    pub fn create_state(&self) -> ReorderableListState {
        ReorderableListState {
            sliver: self.sliver.create_state(),
        }
    }
}

/// Upstream `ReorderableListState`, which forwards its three public methods to
/// the sliver's state -- upstream's own implementation is exactly that.
#[derive(Debug, Default)]
pub struct ReorderableListState {
    pub sliver: SliverReorderableListState,
}

impl ReorderableListState {
    /// Upstream's `startItemDragReorder`.
    pub fn start_item_drag_reorder(&mut self, index: usize, pointer: i64) -> DragStartOutcome {
        self.sliver.start_item_drag_reorder(index, pointer)
    }

    /// Upstream's `cancelReorder`.
    pub fn cancel_reorder(&mut self) {
        self.sliver.cancel_reorder();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ten rows of forty pixels, stacked vertically from zero.
    fn rows(count: usize) -> Vec<ReorderableItemGeometry> {
        (0..count)
            .map(|index| ReorderableItemGeometry {
                index,
                geometry: Rect::xywh(0.0, index as f32 * 40.0, 200.0, 40.0),
            })
            .collect()
    }

    fn insert_at(proxy_start: f32, drag_index: usize, from: usize) -> usize {
        insert_index_for(
            &rows(10),
            drag_index,
            from,
            proxy_start,
            40.0,
            Axis::Vertical,
            false,
        )
    }

    #[test]
    fn a_row_swaps_when_the_rows_visibly_overlap_not_when_their_centres_cross() {
        // The rules read the edges of the moving row against the *halves* of
        // each stationary one. Dragging row 0 downwards, the answer changes as
        // soon as the moving row's trailing edge passes the middle of row 1 --
        // which is when a reader can see the two overlapping, and well before
        // the two centres cross.
        assert_eq!(insert_at(0.0, 0, 0), 0, "still home");
        assert_eq!(insert_at(19.0, 0, 0), 0, "not yet past row 1's middle");
        // And it goes to 2 rather than 1, because with the dragged row still
        // counted, "before row 1" is where row 0 already is. Index 1 is
        // unreachable from index 0, which is the sort of thing that looks like
        // an off-by-one until the adjustment below is taken into account: a
        // drop at 2 reorders to 1, one place down, which is what the reader
        // did.
        assert_eq!(insert_at(21.0, 0, 0), 2, "the trailing edge crossed it");
        assert_eq!(reordered_index(0, 2), 1);
    }

    #[test]
    fn the_dragged_row_can_claim_its_own_index_back() {
        // Upstream gives the dragged row its own clause, and this is what it
        // is for: a reader who drags a row away and brings it back must be
        // able to put it down where it started. Nothing else in the walk can
        // produce the dragged row's own index, because every other clause is
        // about a row that is not this one.
        let moved = insert_at(200.0, 3, 3);
        assert_eq!(moved, 5, "dragged well down the list");
        assert_eq!(insert_at(110.0, 3, moved), 3, "and it came home");
        assert_eq!(reorder_report(3, 3, false), ReorderReport::Nothing);
    }

    #[test]
    fn a_row_far_from_the_moving_one_never_undoes_an_answer_already_found() {
        // The `new_index <` and `new_index >` guards look redundant and are
        // not: the rows are walked in map order, so a later iteration must not
        // overwrite an earlier one's conclusion.
        let items = rows(10);
        // The proxy sits squarely over row 5.
        let answer = insert_index_for(&items, 0, 0, 205.0, 40.0, Axis::Vertical, false);
        // The same computation with the rows presented in the other order must
        // agree -- if the guards were missing, the last row walked would win.
        let mut backwards = items.clone();
        backwards.reverse();
        assert_eq!(
            insert_index_for(&backwards, 0, 0, 205.0, 40.0, Axis::Vertical, false),
            answer,
            "the walk order must not change the answer"
        );
    }

    #[test]
    fn a_reversed_list_reads_the_same_geometry_the_other_way_round() {
        // In a reversed list, further down the screen is earlier in the list,
        // so the same drag downwards lands at a smaller index rather than a
        // larger one.
        let items = rows(10);
        let forwards = insert_index_for(&items, 0, 0, 205.0, 40.0, Axis::Vertical, false);
        let reversed = insert_index_for(&items, 0, 0, 205.0, 40.0, Axis::Vertical, true);
        assert_ne!(forwards, reversed);
    }

    #[test]
    fn a_horizontal_list_measures_along_the_other_axis() {
        // A staircase, so that the two axes have genuinely different answers
        // to give: read across, the rows are packed forty apart; read down,
        // they are a hundred apart and the same proxy has reached none of them.
        let staircase: Vec<ReorderableItemGeometry> = (0..10)
            .map(|index| ReorderableItemGeometry {
                index,
                geometry: Rect::xywh(index as f32 * 40.0, index as f32 * 100.0, 40.0, 40.0),
            })
            .collect();
        assert_eq!(
            insert_index_for(&staircase, 0, 0, 21.0, 40.0, Axis::Horizontal, false),
            2,
            "past the middle of the next row across"
        );
        assert_eq!(
            insert_index_for(&staircase, 0, 0, 21.0, 40.0, Axis::Vertical, false),
            0,
            "nowhere near the next row down"
        );
    }

    #[test]
    fn the_index_a_drop_means_is_not_the_index_the_drag_reported() {
        // The insert index is computed with the dragged row still in the list.
        // Reordering removes it first, which shortens everything after it by
        // one. Getting this wrong moves rows one further than the reader
        // dropped them, and only in one direction, which is why it survives
        // casual testing.
        assert_eq!(reordered_index(2, 5), 4, "dropped later: one comes off");
        assert_eq!(reordered_index(5, 2), 2, "dropped earlier: unchanged");
        assert_eq!(reordered_index(3, 3), 3);
        assert_eq!(reordered_index(3, 4), 3, "one past is where it already was");
    }

    #[test]
    fn the_two_reorder_callbacks_are_not_given_the_same_numbers() {
        // Upstream has both, and they differ: onReorder gets the raw pair
        // whenever they differ at all, while onReorderItem gets the adjusted
        // pair and is skipped when the adjustment makes them equal -- that
        // case being a row dropped back where it started by the long way round.
        assert_eq!(
            reorder_report(2, 5, true),
            ReorderReport::Raw {
                old_index: 2,
                new_index: 5
            }
        );
        assert_eq!(
            reorder_report(2, 5, false),
            ReorderReport::Adjusted {
                old_index: 2,
                new_index: 4
            }
        );
        assert_eq!(
            reorder_report(3, 4, false),
            ReorderReport::Nothing,
            "adjusted back to where it started"
        );
        assert_eq!(
            reorder_report(3, 4, true),
            ReorderReport::Raw {
                old_index: 3,
                new_index: 4
            },
            "but the raw callback still hears about it"
        );
        assert_eq!(reorder_report(3, 3, true), ReorderReport::Nothing);
    }

    #[test]
    fn the_gap_opens_only_between_the_moving_row_and_where_it_would_land() {
        // Rows outside that span do not move at all, which is what makes a
        // long list cheap to drag through.
        // Dragging row 6 up to position 2: rows 2..5 slide down.
        assert_eq!(gap_offset(1, 6, 2, 40.0, false), 0.0);
        assert_eq!(gap_offset(2, 6, 2, 40.0, false), 40.0);
        assert_eq!(gap_offset(5, 6, 2, 40.0, false), 40.0);
        assert_eq!(gap_offset(6, 6, 2, 40.0, false), 0.0, "the row itself");
        assert_eq!(gap_offset(7, 6, 2, 40.0, false), 0.0);

        // Dragging row 2 down to position 6: rows 3..5 slide up.
        assert_eq!(gap_offset(2, 2, 6, 40.0, false), 0.0);
        assert_eq!(gap_offset(3, 2, 6, 40.0, false), -40.0);
        assert_eq!(gap_offset(5, 2, 6, 40.0, false), -40.0);
        assert_eq!(gap_offset(6, 2, 6, 40.0, false), 0.0, "the gap's own slot");
    }

    #[test]
    fn a_reversed_list_opens_its_gap_the_other_way() {
        assert_eq!(gap_offset(3, 6, 2, 40.0, true), -40.0);
        assert_eq!(gap_offset(4, 2, 6, 40.0, true), 40.0);
    }

    #[test]
    fn returning_from_below_reads_as_one_past_the_original_position() {
        // Upstream's comment: the insert index is calculated with the dragged
        // row still present, so coming back from below the original position
        // reads as index + 1 rather than index. Without this branch the drop
        // animation would aim a whole row away.
        assert_eq!(drop_case(3, 4, 10, false), DropCase::BackFromBelow);
        assert_eq!(drop_case(3, 3, 10, false), DropCase::Unmoved);
        assert_eq!(drop_case(3, 0, 10, false), DropCase::BeforeItem);
        assert_eq!(drop_case(3, 7, 10, false), DropCase::AfterPrevious);
        // Reversed, the two tail cases swap.
        assert_eq!(drop_case(3, 10, 10, true), DropCase::AfterPrevious);
        assert_eq!(drop_case(3, 7, 10, true), DropCase::BeforeItem);
    }

    // -- The state -------------------------------------------------------

    fn dragging_state() -> SliverReorderableListState {
        let mut state = SliverReorderableListState::new(10);
        for item in rows(10) {
            state.register_item(item);
        }
        state
    }

    #[test]
    fn a_drag_cannot_start_on_a_row_that_is_not_on_screen() {
        // Upstream throws here, and its TODO asks whether it could scroll to
        // the row instead. Either way there is no geometry to reorder against.
        let mut state = SliverReorderableListState::new(10);
        assert_eq!(
            state.start_item_drag_reorder(3, 1),
            DragStartOutcome::ItemNotVisible
        );
        assert!(!state.is_dragging());

        let mut state = dragging_state();
        assert_eq!(
            state.start_item_drag_reorder(3, 1),
            DragStartOutcome::Started
        );
        assert_eq!(state.drag_index(), Some(3));
        assert_eq!(state.insert_index(), Some(3));
    }

    #[test]
    fn a_second_finger_does_not_inherit_the_first_ones_half_finished_drag() {
        let mut state = dragging_state();
        state.start_item_drag_reorder(3, 1);
        assert_eq!(
            state.start_item_drag_reorder(5, 2),
            DragStartOutcome::CancelledPrevious
        );
        assert_eq!(state.drag_index(), Some(5));
    }

    #[test]
    fn changing_the_item_count_cancels_a_drag_in_progress() {
        // Upstream's documentation says cancelReorder should be called before
        // any major change to the list, so a drag is not left confused by rows
        // moving underneath it -- and a changed item count is that.
        let mut state = dragging_state();
        state.start_item_drag_reorder(3, 1);
        state.set_item_count(10);
        assert!(state.is_dragging(), "the same count is not a change");
        state.set_item_count(9);
        assert!(!state.is_dragging());
    }

    #[test]
    fn a_whole_drag_reports_the_index_the_reader_meant() {
        let mut state = dragging_state();
        state.start_item_drag_reorder(1, 1);
        // Drag row 1 down to sit over row 5.
        state.drag_update(205.0, 40.0);
        let landed = state.insert_index().expect("a drag is in progress");
        assert!(landed > 1, "it moved: {landed}");
        assert_eq!(
            state.drop_completed(false),
            ReorderReport::Adjusted {
                old_index: 1,
                new_index: landed - 1
            }
        );
        assert!(!state.is_dragging(), "and the drag is over");
    }

    #[test]
    fn cancelling_reports_nothing_and_forgets_everything() {
        let mut state = dragging_state();
        state.start_item_drag_reorder(1, 1);
        state.drag_update(205.0, 40.0);
        state.cancel_reorder();
        assert!(!state.is_dragging());
        assert_eq!(state.insert_index(), None);
        assert_eq!(state.drop_completed(false), ReorderReport::Nothing);
    }

    #[test]
    fn a_row_replaced_at_the_same_index_does_not_take_its_successor_with_it() {
        // Upstream's `_unregisterItem` only removes the row if the one held at
        // that index is still the same one.
        let mut state = dragging_state();
        let replacement = ReorderableItemGeometry {
            index: 3,
            geometry: Rect::xywh(0.0, 999.0, 200.0, 40.0),
        };
        state.register_item(replacement);
        // The old geometry unregisters, and finds nothing to remove.
        state.unregister_item(3, Rect::xywh(0.0, 120.0, 200.0, 40.0));
        assert!(state.items().iter().any(|item| item.index == 3));
        state.unregister_item(3, replacement.geometry);
        assert!(!state.items().iter().any(|item| item.index == 3));
    }

    #[test]
    fn the_stationary_rows_move_only_while_a_drag_is_in_progress() {
        let mut state = dragging_state();
        assert_eq!(state.gap_offset_for(4, 40.0), Offset::ZERO);
        state.start_item_drag_reorder(6, 1);
        state.drag_update(60.0, 40.0);
        let gap = state.insert_index().unwrap();
        assert!(gap < 6, "dragged upwards to {gap}");
        assert_eq!(state.gap_offset_for(gap, 40.0), Offset::new(0.0, 40.0));
        assert_eq!(state.gap_offset_for(6, 40.0), Offset::ZERO);
    }

    #[test]
    fn the_two_listeners_differ_only_in_which_gesture_starts_the_drag() {
        // The immediate one is for a drag handle; the delayed one is for a
        // whole row, because inside a scroll view an immediate drag and a
        // scroll are the same gesture and only time separates them.
        assert_eq!(
            ReorderableDragStartListener::new(3)
                .create_recognizer()
                .policy(),
            crate::multidrag::MultiDragPolicy::Immediate
        );
        assert_eq!(
            ReorderableDelayedDragStartListener::new(3)
                .create_recognizer()
                .policy(),
            crate::multidrag::MultiDragPolicy::Delayed {
                delay_micros: crate::multidrag::DEFAULT_MULTI_DRAG_DELAY_MICROS
            }
        );
        assert_eq!(ReorderableDelayedDragStartListener::new(3).index, 3);
        assert!(ReorderableDragStartListener::new(3).enabled);
        assert!(
            !ReorderableDragStartListener::new(3)
                .with_enabled(false)
                .enabled
        );
        assert!(
            !ReorderableDelayedDragStartListener::new(3)
                .with_enabled(false)
                .enabled
        );
    }

    #[test]
    fn the_list_forwards_to_its_sliver_because_upstream_does() {
        let list = ReorderableList::new(10)
            .with_axis(Axis::Horizontal)
            .with_reverse(true);
        let mut state = list.create_state();
        assert_eq!(state.sliver.axis, Axis::Horizontal);
        assert!(state.sliver.reverse);
        assert_eq!(
            state.start_item_drag_reorder(0, 1),
            DragStartOutcome::ItemNotVisible,
            "nothing is registered yet"
        );
        state.cancel_reorder();
        assert!(!state.sliver.is_dragging());
    }

    // -- The two asserts, at the layer upstream writes them ------------------------

    #[test]
    fn the_sliver_refuses_more_than_one_source_of_extent() {
        let mut config = ReorderableConfig::new();
        assert_eq!(
            SliverReorderableList::new(3).with_config(config).validate(),
            Ok(()),
            "none is fine"
        );

        config.has_prototype_item = true;
        assert_eq!(
            SliverReorderableList::new(3).with_config(config).validate(),
            Ok(()),
            "and one is fine"
        );

        config.has_item_extent_builder = true;
        assert_eq!(
            SliverReorderableList::new(3).with_config(config).validate(),
            Err(ReorderableError::MoreThanOneExtentSource)
        );
    }

    #[test]
    fn the_sliver_refuses_both_callbacks_and_refuses_neither() {
        let mut config = ReorderableConfig::new();
        config.has_on_reorder = true;
        assert_eq!(
            SliverReorderableList::new(3).with_config(config).validate(),
            Err(ReorderableError::BothCallbacks)
        );

        config.has_on_reorder_item = false;
        assert_eq!(
            SliverReorderableList::new(3).with_config(config).validate(),
            Ok(()),
            "the deprecated one alone still works"
        );

        config.has_on_reorder = false;
        assert_eq!(
            SliverReorderableList::new(3).with_config(config).validate(),
            Err(ReorderableError::NeitherCallback),
            "a list that reports nothing is a list that refuses to reorder"
        );
    }

    #[test]
    fn the_extent_complaint_comes_before_the_callback_one() {
        // Both asserts fail; upstream writes the extent one first, so that is
        // the one a reader is told about.
        let config = ReorderableConfig {
            has_on_reorder_item: false,
            has_on_reorder: false,
            has_item_extent: true,
            has_prototype_item: true,
            has_item_extent_builder: false,
        };
        assert_eq!(
            config.validate(),
            Err(ReorderableError::MoreThanOneExtentSource)
        );
        assert_eq!(
            config.check_callbacks(),
            Err(ReorderableError::NeitherCallback)
        );
    }

    #[test]
    fn the_axis_reaches_both_the_scroll_view_and_the_sliver() {
        // Upstream has one axis in two places: the scroll view is told
        // `scrollDirection` and the sliver reads the same thing off its
        // constraints. Here the sliver holds it as a value, so the two have to
        // be set together or the gaps open along the wrong axis.
        let list = ReorderableList::new(4).with_axis(Axis::Horizontal);
        assert_eq!(list.scroll.scroll_direction, Axis::Horizontal);
        assert_eq!(list.sliver.axis, Axis::Horizontal);

        let reversed = ReorderableList::new(4).with_reverse(true);
        assert!(reversed.scroll.reverse && reversed.sliver.reverse);

        // And a whole scroll view carries its own axis down with it.
        let whole = ReorderableList::new(4).with_scroll(ScrollView::new().horizontal());
        assert_eq!(whole.sliver.axis, Axis::Horizontal);
        assert_eq!(whole.create_state().sliver.axis, Axis::Horizontal);
    }

    #[test]
    fn the_scroll_view_refuses_what_a_scroll_view_refuses() {
        let list = ReorderableList::new(4).with_scroll(
            ScrollView::new()
                .shrink_wrapped()
                .with_primary(true)
                .with_controller(),
        );
        assert!(matches!(
            list.validate(),
            Err(ReorderableError::ScrollView(_))
        ));

        // And the list's own asserts still come first.
        let mut config = ReorderableConfig::new();
        config.has_item_extent = true;
        config.has_prototype_item = true;
        let both_wrong = ReorderableList::new(4)
            .with_config(config)
            .with_scroll(ScrollView::new().with_primary(true).with_controller());
        assert_eq!(
            both_wrong.validate(),
            Err(ReorderableError::MoreThanOneExtentSource)
        );
    }

    // -- Two ways of asking for a cache extent -------------------------------------

    #[test]
    fn saying_nothing_about_the_cache_extent_stays_nothing() {
        // Not the default: the widget layer passes the silence on, and the
        // render viewport is where a missing extent becomes a number.
        let list = ReorderableList::new(3);
        assert_eq!(list.effective_scroll_cache_extent(), None);
        assert_eq!(
            ScrollCacheExtent::defaulted(None, CacheExtentStyle::Pixel),
            Some(ScrollCacheExtent::pixels(
                crate::scrolling::DEFAULT_CACHE_EXTENT
            )),
            "which is a different question, asked further down"
        );
    }

    #[test]
    fn the_deprecated_double_is_read_as_pixels_here() {
        // A `ReorderableList` never had a `cacheExtentStyle`, so upstream
        // writes `ScrollCacheExtent.pixels(cacheExtent!)` outright.
        let list = ReorderableList::new(3).with_cache_extent(120.0);
        assert_eq!(
            list.effective_scroll_cache_extent(),
            Some(ScrollCacheExtent::pixels(120.0))
        );
        assert_eq!(
            list.effective_scroll_cache_extent()
                .map(|extent| extent.in_pixels(800.0)),
            Some(120.0),
            "and a viewport of any size does not change it"
        );
    }

    #[test]
    fn the_new_field_wins_over_the_deprecated_one() {
        let list = ReorderableList::new(3)
            .with_cache_extent(120.0)
            .with_scroll_cache_extent(ScrollCacheExtent::viewport(0.5));
        assert_eq!(
            list.effective_scroll_cache_extent(),
            Some(ScrollCacheExtent::viewport(0.5))
        );
        assert_eq!(
            list.effective_scroll_cache_extent()
                .map(|extent| extent.in_pixels(800.0)),
            Some(400.0),
            "so the unit comes from the field that won, not from this widget"
        );
    }

    #[test]
    fn the_scroll_view_asks_the_same_question_as_the_sliver_it_builds() {
        let mut config = ReorderableConfig::new();
        config.has_item_extent = true;
        config.has_item_extent_builder = true;
        let list = ReorderableList::new(3).with_config(config);
        assert_eq!(list.validate(), list.sliver.validate());
        assert_eq!(
            list.validate(),
            Err(ReorderableError::MoreThanOneExtentSource)
        );
    }
}

// -- material/reorderable_list.dart ------------------------------------------------
//
// The Material list on top of the widgets-layer machinery above. It reuses
// `reorder_report` rather than restating it.

/// What a `ReorderableListView` wraps each of its items in when it builds the
/// drag handles itself.
///
/// **The two platforms are given different things to drag.** Upstream's
/// `_itemBuilder` switches on `Theme.of(context).platform`: a desktop row gets
/// a handle drawn over it and dragged the moment it is pressed, and a phone
/// row gets no handle at all -- the whole row is the target, after a long
/// press.
///
/// Both halves follow from the same fact. A desktop reader has a pointer and
/// no other use for a press on a row, so a small target they can see is
/// better; a phone reader's press is already spoken for by the scroll, so the
/// gesture has to be one the scroll does not claim, and a long press is that
/// gesture. The two listeners this crate already has are exactly this pair --
/// see [`ReorderableDragStartListener`] and
/// [`ReorderableDelayedDragStartListener`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultDragHandles {
    /// Upstream's linux/windows/macOS arm.
    Visible,
    /// Upstream's iOS/android/fuchsia arm.
    WholeRowOnLongPress,
    /// `buildDefaultDragHandles: false`: the item is wrapped in a
    /// `KeyedSubtree` and nothing else. The application is expected to put its
    /// own listener somewhere inside the row.
    None,
}

/// Upstream's `_itemBuilder` switch.
pub fn default_drag_handles(
    builds_default_drag_handles: bool,
    platform: crate::editable_text::TargetPlatform,
) -> DefaultDragHandles {
    use crate::editable_text::TargetPlatform;
    if !builds_default_drag_handles {
        return DefaultDragHandles::None;
    }
    match platform {
        TargetPlatform::Linux | TargetPlatform::Windows | TargetPlatform::MacOS => {
            DefaultDragHandles::Visible
        }
        TargetPlatform::IOS | TargetPlatform::Android | TargetPlatform::Fuchsia => {
            DefaultDragHandles::WholeRowOnLongPress
        }
    }
}

/// Where the built-in handle sits over the row.
///
/// Directional insets, `None` where upstream leaves the edge unconstrained.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragHandleSpot {
    pub start: Option<f32>,
    pub end: Option<f32>,
    pub top: Option<f32>,
    pub bottom: Option<f32>,
    pub alignment: crate::render::AlignmentDirectional,
}

/// Upstream's `Positioned.directional` around the handle, and the `Align`
/// inside it.
///
/// **The handle is given the whole cross axis and pinned to the trailing edge
/// of the main one.** In a vertical list that is a full-height strip down the
/// end side, with the handle centred in it; in a horizontal list, a
/// full-width strip along the bottom. The strip is what makes the handle easy
/// to hit -- the reader can be anywhere along the row's height and still land
/// on it -- while the alignment is what keeps the handle itself in one place.
///
/// The inset is [`DRAG_HANDLE_INSET`] on the pinned edge only. The other three
/// are zero or unconstrained, which is not an oversight: an inset on the
/// leading edge would shrink the strip from the side the row's own content is
/// on.
pub fn drag_handle_spot(axis: Axis) -> DragHandleSpot {
    match axis {
        Axis::Vertical => DragHandleSpot {
            start: None,
            end: Some(DRAG_HANDLE_INSET),
            top: Some(0.0),
            bottom: Some(0.0),
            alignment: crate::render::AlignmentDirectional::CENTER_END,
        },
        Axis::Horizontal => DragHandleSpot {
            start: Some(0.0),
            end: Some(0.0),
            top: None,
            bottom: Some(DRAG_HANDLE_INSET),
            alignment: crate::render::AlignmentDirectional::BOTTOM_CENTER,
        },
    }
}

/// How far the handle is held off the edge it is pinned to.
pub const DRAG_HANDLE_INSET: f32 = 8.0;

/// The cursor over the built-in handle.
///
/// ```dart
/// widget.mouseCursor ??
///     const WidgetStateMouseCursor.fromMap(<WidgetStatesConstraint, MouseCursor>{
///       WidgetState.dragged: SystemMouseCursors.grabbing,
///       WidgetState.any: SystemMouseCursors.grab,
///     }),
/// <WidgetState>{if (_dragging.value) WidgetState.dragged},
/// ```
///
/// An open hand over the handle and a closed one while it is held: the two
/// halves of the same gesture, and the only place in this list where the
/// pointer is told what it may do before it does it. A cursor given by the
/// application is resolved against the same state rather than replacing the
/// idea, so an application that supplies a state-dependent cursor still gets
/// its dragged arm during a drag.
pub fn drag_handle_cursor(
    given: Option<&crate::widget_state::StateProperty<crate::services::system::SystemMouseCursor>>,
    dragging: bool,
) -> crate::services::system::SystemMouseCursor {
    use crate::services::system::SystemMouseCursor;
    let states = if dragging {
        crate::widget_state::WidgetStates::NONE.with(crate::widget_state::WidgetState::Dragged)
    } else {
        crate::widget_state::WidgetStates::NONE
    };
    match given {
        Some(property) => property.resolve(states),
        None if dragging => SystemMouseCursor::Grabbing,
        None => SystemMouseCursor::Grab,
    }
}

/// How a reorderable list's padding is divided between its header, its list
/// and its footer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReorderablePadding {
    pub header: EdgeInsets,
    pub list: EdgeInsets,
    pub footer: EdgeInsets,
}

/// Upstream `ReorderableListView`.
#[derive(Clone, Debug, PartialEq)]
pub struct ReorderableListView {
    pub item_count: usize,
    /// The two asserts this shares with the widget-layer lists.
    pub config: ReorderableConfig,
    /// The scroll view this builds around its sliver. Fourteen of upstream's
    /// thirty-one members are its parameters, passed straight through, and
    /// none of them is a rule of this class: see [`ReorderableList::scroll`].
    pub scroll: ScrollView,
    /// Upstream's `padding`, which is **not** simply handed to the scroll
    /// view: see [`ReorderableListView::split_padding`].
    pub padding: Option<EdgeInsets>,
    /// Upstream's `mouseCursor`, over the built-in drag handle only -- its own
    /// doc says so: "The cursor for a mouse pointer when it enters or is
    /// hovering over the drag handle." See [`drag_handle_cursor`].
    pub mouse_cursor:
        Option<crate::widget_state::StateProperty<crate::services::system::SystemMouseCursor>>,
    /// Upstream's `header`: a row before the list that cannot be reordered.
    pub has_header: bool,
    /// Upstream's `footer`, the same after it.
    pub has_footer: bool,
    /// Only the list constructor can answer this up front. See
    /// [`ReorderableListView::validate`].
    pub every_child_has_a_key: bool,
    pub builds_default_drag_handles: bool,
}

impl ReorderableListView {
    pub fn new(item_count: usize) -> ReorderableListView {
        ReorderableListView {
            item_count,
            config: ReorderableConfig::new(),
            scroll: ScrollView::new(),
            padding: None,
            mouse_cursor: None,
            has_header: false,
            has_footer: false,
            every_child_has_a_key: true,
            builds_default_drag_handles: true,
        }
    }

    /// Upstream's constructor asserts.
    ///
    /// The key rule is enforced **twice, by two different mechanisms**: the list
    /// constructor asserts `children.every((w) => w.key != null)` up front,
    /// while `_itemBuilder` throws a `FlutterError` per item as it is built.
    /// That is not belt and braces -- a builder's items do not exist until
    /// somebody scrolls far enough to need them, so the only moment to check one
    /// is the moment it appears. The two constructors get the same rule and the
    /// only enforcement each of them can have.
    ///
    /// The other two are [`ReorderableConfig`]'s, and the key assert goes
    /// *between* them because that is where upstream writes it -- a list built
    /// with two fixed extents **and** a keyless child complains about the
    /// extents, and one with a keyless child and no callback at all complains
    /// about the child.
    pub fn validate(&self) -> Result<(), ReorderableError> {
        self.config.check_extent_sources()?;
        if !self.every_child_has_a_key {
            return Err(ReorderableError::AChildWithoutAKey);
        }
        self.config.check_callbacks()?;
        // Last, because upstream's are not the same kind of check: these three
        // are this constructor's asserts and fire as the widget is written,
        // while the scroll view's fire when `build` constructs one. A list
        // that gets both wrong hears about its own mistake first.
        self.scroll.validate().map_err(ReorderableError::ScrollView)
    }

    /// Upstream's padding split, from `build`:
    ///
    /// ```dart
    /// // If there is a header or footer we can't just apply the padding to the
    /// // list, so we break it up into padding for the header, footer and
    /// // padding for the list.
    /// ```
    ///
    /// **The padding may only be spent once.** A list with a header is three
    /// slivers, and putting the whole padding on each of them would put a gap
    /// between the header and the first row that the reader never asked for,
    /// and leave the header inset from the top *and* the list inset from the
    /// header. So the side a header sits against keeps its padding, and the
    /// same side of the list gives it up: the header takes the padding with
    /// the edge it shares with the list zeroed, and the list takes the padding
    /// with the edge it shares with the header zeroed.
    ///
    /// **A reversed list flips which side that is.** Upstream swaps `start`
    /// and `end` before anything else, because in a reversed list the header
    /// is at the bottom of the screen -- so it is the *bottom* padding it
    /// keeps and the *top* it gives up.
    ///
    /// With neither a header nor a footer nothing is split: the whole padding
    /// goes on the list, which is upstream's `when (start ?? end) == null`
    /// guard -- one test for both, because that expression is null only when
    /// both of them are.
    ///
    /// The cross axis is untouched throughout. Only the ends of the scroll are
    /// shared between the three slivers; the sides belong to all of them.
    pub fn split_padding(&self) -> ReorderablePadding {
        let padding = self.padding.unwrap_or(EdgeInsets::ZERO);
        let (mut start, mut end) = (
            self.has_header.then_some(0.0),
            self.has_footer.then_some(0.0),
        );
        if self.scroll.reverse {
            (start, end) = (end, start);
        }
        if start.is_none() && end.is_none() {
            return ReorderablePadding {
                header: EdgeInsets::ZERO,
                list: padding,
                footer: EdgeInsets::ZERO,
            };
        }
        let (start_padding, end_padding, list) = match self.scroll.scroll_direction {
            Axis::Vertical => (
                EdgeInsets::only(padding.left, 0.0, padding.right, padding.bottom),
                EdgeInsets::only(padding.left, padding.top, padding.right, 0.0),
                EdgeInsets::only(
                    padding.left,
                    start.unwrap_or(padding.top),
                    padding.right,
                    end.unwrap_or(padding.bottom),
                ),
            ),
            Axis::Horizontal => (
                EdgeInsets::only(0.0, padding.top, padding.right, padding.bottom),
                EdgeInsets::only(padding.left, padding.top, 0.0, padding.bottom),
                EdgeInsets::only(
                    start.unwrap_or(padding.left),
                    padding.top,
                    end.unwrap_or(padding.right),
                    padding.bottom,
                ),
            ),
        };
        let (header, footer) = if self.scroll.reverse {
            (start_padding, end_padding)
        } else {
            (end_padding, start_padding)
        };
        ReorderablePadding {
            header,
            list,
            footer,
        }
    }

    /// What each row is wrapped in on this platform.
    pub fn drag_handles(
        &self,
        platform: crate::editable_text::TargetPlatform,
    ) -> DefaultDragHandles {
        default_drag_handles(self.builds_default_drag_handles, platform)
    }

    /// The cursor over the built-in handle, given whether a drag is under way.
    pub fn handle_cursor(&self, dragging: bool) -> crate::services::system::SystemMouseCursor {
        drag_handle_cursor(self.mouse_cursor.as_ref(), dragging)
    }

    /// What a drop reports, deferring to [`reorder_report`] for the arithmetic.
    pub fn report_drop(&self, old_index: usize, new_index: usize) -> ReorderReport {
        reorder_report(old_index, new_index, self.config.has_on_reorder)
    }

    /// Upstream wraps each item's key in a private global key keyed on the state
    /// as well, so the same key may appear in two lists at once.
    pub fn wraps_child_keys() -> bool {
        true
    }
}

// -- material/tooltip_visibility.dart ------------------------------------------------

/// Upstream `TooltipVisibility`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooltipVisibility {
    pub visible: bool,
}

impl TooltipVisibility {
    /// Upstream `TooltipVisibility.of`, which **defaults to `true` when there is
    /// no ancestor at all**.
    ///
    /// An inherited widget whose absence is a yes. Which is the only workable
    /// default -- tooltips exist without anybody opting in, and this widget is
    /// there to turn them *off* for a subtree.
    pub fn of(enclosing: Option<TooltipVisibility>) -> bool {
        enclosing.map_or(true, |scope| scope.visible)
    }
}

/// Upstream `TooltipState`, whose defaults live on the state class rather than
/// the widget so the theme can be consulted first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TooltipState {
    pub visible: bool,
    pub has_overlay_entry: bool,
}

impl TooltipState {
    pub const DEFAULT_VERTICAL_OFFSET: f32 = 24.0;
    pub const DEFAULT_PREFER_BELOW: bool = true;
    pub const DEFAULT_SHOW_DURATION_MS: u64 = 1500;
    pub const DEFAULT_EXIT_DURATION_MS: u64 = 100;
    /// Upstream's `_defaultWaitDuration` is `Duration.zero`: **a tooltip that
    /// has been asked for does not make you wait again.** The wait is what a
    /// hovering pointer serves before the tooltip is asked for at all.
    pub const DEFAULT_WAIT_DURATION_MS: u64 = 0;

    pub fn new() -> TooltipState {
        TooltipState {
            visible: true,
            has_overlay_entry: false,
        }
    }

    /// Upstream `ensureTooltipVisible`, which forwards to the `RawTooltipState`
    /// behind a global key **and answers `false` if there is not one yet**.
    ///
    /// The return value is not "is it visible now" but "was there something to
    /// ask": a tooltip whose raw state has not been built cannot be shown, and
    /// saying so is more useful than throwing.
    pub fn ensure_tooltip_visible(&self, raw_state_built: bool) -> bool {
        raw_state_built
    }

    /// Upstream reads `TooltipVisibility.of(context)` in
    /// `didChangeDependencies`, so an ancestor turning tooltips off reaches this
    /// one without it being rebuilt by its parent.
    pub fn did_change_dependencies(&mut self, enclosing: Option<TooltipVisibility>) {
        self.visible = TooltipVisibility::of(enclosing);
    }
}

impl Default for TooltipState {
    fn default() -> Self {
        TooltipState::new()
    }
}

#[cfg(test)]
mod material_reorderable_tests {
    use super::*;

    // -- The same rule, two enforcements ------------------------------------------

    #[test]
    fn a_child_without_a_key_is_refused_where_it_can_be_seen_up_front() {
        let mut list = ReorderableListView::new(3);
        assert_eq!(list.validate(), Ok(()));
        list.every_child_has_a_key = false;
        assert_eq!(list.validate(), Err(ReorderableError::AChildWithoutAKey));
    }

    // -- What the reader is given to drag -----------------------------------------

    #[test]
    fn a_desktop_row_gets_a_handle_and_a_phone_row_is_one() {
        use crate::editable_text::TargetPlatform;
        for platform in [
            TargetPlatform::Linux,
            TargetPlatform::Windows,
            TargetPlatform::MacOS,
        ] {
            assert_eq!(
                default_drag_handles(true, platform),
                DefaultDragHandles::Visible,
                "{platform:?}"
            );
        }
        for platform in [
            TargetPlatform::IOS,
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
        ] {
            assert_eq!(
                default_drag_handles(true, platform),
                DefaultDragHandles::WholeRowOnLongPress,
                "{platform:?}: the scroll has already claimed a plain press"
            );
        }
    }

    #[test]
    fn a_list_that_builds_no_handles_gets_none_on_any_platform() {
        use crate::editable_text::TargetPlatform;
        for platform in [TargetPlatform::MacOS, TargetPlatform::Android] {
            assert_eq!(
                default_drag_handles(false, platform),
                DefaultDragHandles::None,
                "{platform:?}"
            );
        }
        let mut list = ReorderableListView::new(3);
        assert_eq!(
            list.drag_handles(TargetPlatform::MacOS),
            DefaultDragHandles::Visible,
            "and a list builds them by default"
        );
        list.builds_default_drag_handles = false;
        assert_eq!(
            list.drag_handles(TargetPlatform::MacOS),
            DefaultDragHandles::None
        );
    }

    #[test]
    fn the_handle_takes_the_whole_cross_axis_and_hugs_the_trailing_edge() {
        let down = drag_handle_spot(Axis::Vertical);
        assert_eq!(
            (down.top, down.bottom),
            (Some(0.0), Some(0.0)),
            "a full-height strip, so the reader can be anywhere down the row"
        );
        assert_eq!(down.end, Some(DRAG_HANDLE_INSET));
        assert_eq!(down.start, None, "and nothing pinning its leading edge");
        assert_eq!(
            down.alignment,
            crate::render::AlignmentDirectional::CENTER_END
        );

        let across = drag_handle_spot(Axis::Horizontal);
        assert_eq!((across.start, across.end), (Some(0.0), Some(0.0)));
        assert_eq!(across.bottom, Some(DRAG_HANDLE_INSET));
        assert_eq!(across.top, None);
        assert_eq!(
            across.alignment,
            crate::render::AlignmentDirectional::BOTTOM_CENTER
        );
    }

    #[test]
    fn an_open_hand_over_the_handle_and_a_closed_one_holding_it() {
        use crate::services::system::SystemMouseCursor;
        let list = ReorderableListView::new(3);
        assert_eq!(list.handle_cursor(false), SystemMouseCursor::Grab);
        assert_eq!(list.handle_cursor(true), SystemMouseCursor::Grabbing);
    }

    #[test]
    fn a_cursor_of_ones_own_is_still_resolved_against_the_drag() {
        // Upstream resolves whatever it is given with `{if (dragging)
        // WidgetState.dragged}`, so a state-dependent cursor keeps its dragged
        // arm rather than being flattened to one value.
        use crate::services::system::SystemMouseCursor;
        use crate::widget_state::{StateProperty, WidgetState};

        let mut list = ReorderableListView::new(3);
        list.mouse_cursor = Some(StateProperty::resolve_with(|states| {
            if states.contains(WidgetState::Dragged) {
                SystemMouseCursor::Move
            } else {
                SystemMouseCursor::Click
            }
        }));
        assert_eq!(list.handle_cursor(false), SystemMouseCursor::Click);
        assert_eq!(list.handle_cursor(true), SystemMouseCursor::Move);
    }

    // -- The padding may only be spent once ---------------------------------------

    /// Ten either side, twenty top and bottom.
    fn inset() -> EdgeInsets {
        EdgeInsets::only(10.0, 20.0, 10.0, 20.0)
    }

    #[test]
    fn with_no_header_or_footer_the_padding_is_not_split_at_all() {
        let mut list = ReorderableListView::new(3);
        list.padding = Some(inset());
        let split = list.split_padding();
        assert_eq!(split.list, inset());
        assert_eq!(split.header, EdgeInsets::ZERO);
        assert_eq!(split.footer, EdgeInsets::ZERO);
    }

    #[test]
    fn a_header_keeps_the_padding_at_its_own_end_and_the_list_gives_it_up() {
        // Otherwise the reader gets the gap twice: once above the header and
        // once between the header and the first row.
        let mut list = ReorderableListView::new(3);
        list.padding = Some(inset());
        list.has_header = true;
        let split = list.split_padding();

        assert_eq!(
            split.header,
            EdgeInsets::only(10.0, 20.0, 10.0, 0.0),
            "the header keeps the top and gives up the edge it shares"
        );
        assert_eq!(
            split.list,
            EdgeInsets::only(10.0, 0.0, 10.0, 20.0),
            "and the list gives up the top, keeping the bottom it still owns"
        );
        assert_eq!(
            (split.list.left, split.list.right),
            (10.0, 10.0),
            "the cross axis belongs to all three and is never split"
        );
    }

    #[test]
    fn a_reversed_list_puts_the_header_at_the_other_end() {
        // In a reversed list the header is at the bottom of the screen, so it
        // is the bottom padding it keeps.
        let mut list = ReorderableListView::new(3);
        list.padding = Some(inset());
        list.has_header = true;
        list.scroll.reverse = true;
        let split = list.split_padding();

        assert_eq!(split.header, EdgeInsets::only(10.0, 0.0, 10.0, 20.0));
        assert_eq!(split.list, EdgeInsets::only(10.0, 20.0, 10.0, 0.0));
    }

    #[test]
    fn with_both_the_list_gives_up_both_ends() {
        let mut list = ReorderableListView::new(3);
        list.padding = Some(inset());
        list.has_header = true;
        list.has_footer = true;
        let split = list.split_padding();

        assert_eq!(split.header, EdgeInsets::only(10.0, 20.0, 10.0, 0.0));
        assert_eq!(split.footer, EdgeInsets::only(10.0, 0.0, 10.0, 20.0));
        assert_eq!(split.list, EdgeInsets::only(10.0, 0.0, 10.0, 0.0));
    }

    #[test]
    fn a_horizontal_list_splits_the_other_pair_of_edges() {
        let mut list = ReorderableListView::new(3);
        list.padding = Some(EdgeInsets::only(10.0, 20.0, 30.0, 40.0));
        list.has_header = true;
        list.scroll.scroll_direction = Axis::Horizontal;
        let split = list.split_padding();

        assert_eq!(
            split.header,
            EdgeInsets::only(10.0, 20.0, 0.0, 40.0),
            "the header keeps the left and gives up the right"
        );
        assert_eq!(
            split.list,
            EdgeInsets::only(0.0, 20.0, 30.0, 40.0),
            "and the top and bottom, which are the cross axis now, are untouched"
        );
    }

    #[test]
    fn a_reorderable_list_view_is_a_scroll_view_and_is_refused_like_one() {
        // Fourteen of upstream's thirty-one members are the scroll view's, and
        // its asserts come with them: a list that asks to be primary and hands
        // over a controller is refused here as it would be there.
        let mut list = ReorderableListView::new(3);
        assert_eq!(list.validate(), Ok(()));

        list.scroll = ScrollView::new().with_controller().with_primary(true);
        assert_eq!(
            list.validate(),
            Err(ReorderableError::ScrollView(
                "You cannot both set primary to true and pass an explicit controller."
            ))
        );

        list.scroll = ScrollView::new();
        list.scroll.anchor = 1.5;
        assert!(matches!(
            list.validate(),
            Err(ReorderableError::ScrollView(_))
        ));
    }

    #[test]
    fn the_lists_own_complaints_come_before_the_scroll_views() {
        // Two different moments upstream: this constructor's asserts fire as
        // the widget is written, the scroll view's when `build` makes one.
        let mut list = ReorderableListView::new(3);
        list.scroll = ScrollView::new().with_controller().with_primary(true);
        list.config.has_item_extent = true;
        list.config.has_prototype_item = true;
        assert_eq!(
            list.validate(),
            Err(ReorderableError::MoreThanOneExtentSource),
            "a list that gets both wrong hears about its own mistake first"
        );
    }

    #[test]
    fn the_key_complaint_sits_between_the_two_shared_ones() {
        // Upstream wedges the key assert between the extent one and the
        // callback one, and the order decides what a list with two mistakes is
        // told about.
        let mut list = ReorderableListView::new(3);
        list.every_child_has_a_key = false;

        list.config.has_item_extent = true;
        list.config.has_prototype_item = true;
        assert_eq!(
            list.validate(),
            Err(ReorderableError::MoreThanOneExtentSource),
            "the extents are asked about first"
        );

        list.config.has_prototype_item = false;
        list.config.has_on_reorder_item = false;
        assert_eq!(
            list.validate(),
            Err(ReorderableError::AChildWithoutAKey),
            "but the key comes before the callbacks"
        );
    }

    #[test]
    fn at_most_one_source_of_extent() {
        let mut list = ReorderableListView::new(3);
        assert_eq!(list.validate(), Ok(()), "none is fine");

        list.config.has_item_extent = true;
        assert_eq!(list.validate(), Ok(()), "and one is fine");

        list.config.has_prototype_item = true;
        assert_eq!(
            list.validate(),
            Err(ReorderableError::MoreThanOneExtentSource)
        );

        // Every pair is refused, not just the first.
        list.config.has_item_extent = false;
        list.config.has_item_extent_builder = true;
        assert_eq!(
            list.validate(),
            Err(ReorderableError::MoreThanOneExtentSource)
        );
    }

    #[test]
    fn exactly_one_of_the_two_reorder_callbacks() {
        let mut list = ReorderableListView::new(3);
        assert_eq!(list.validate(), Ok(()));

        list.config.has_on_reorder = true;
        assert_eq!(list.validate(), Err(ReorderableError::BothCallbacks));

        list.config.has_on_reorder_item = false;
        assert_eq!(
            list.validate(),
            Ok(()),
            "the deprecated one alone still works"
        );

        list.config.has_on_reorder = false;
        assert_eq!(list.validate(), Err(ReorderableError::NeitherCallback));
    }

    #[test]
    fn the_view_hands_its_drops_to_the_same_arithmetic_as_the_sliver() {
        let modern = ReorderableListView::new(5);
        assert_eq!(
            modern.report_drop(2, 3),
            ReorderReport::Nothing,
            "one slot down is where it already was"
        );
        assert_eq!(
            modern.report_drop(2, 4),
            ReorderReport::Adjusted {
                old_index: 2,
                new_index: 3
            }
        );

        let mut legacy = ReorderableListView::new(5);
        legacy.config.has_on_reorder = true;
        legacy.config.has_on_reorder_item = false;
        assert_eq!(
            legacy.report_drop(2, 3),
            ReorderReport::Raw {
                old_index: 2,
                new_index: 3
            },
            "which the deprecated callback does report, unadjusted"
        );
    }

    // -- An inherited widget whose absence is a yes ---------------------------------

    #[test]
    fn tooltips_are_visible_when_nobody_has_said_anything() {
        assert!(TooltipVisibility::of(None));
        assert!(TooltipVisibility::of(Some(TooltipVisibility {
            visible: true
        })));
        assert!(!TooltipVisibility::of(Some(TooltipVisibility {
            visible: false
        })));
    }

    #[test]
    fn an_ancestor_turning_them_off_reaches_a_state_that_was_not_rebuilt() {
        let mut state = TooltipState::new();
        assert!(state.visible);
        state.did_change_dependencies(Some(TooltipVisibility { visible: false }));
        assert!(!state.visible);
        state.did_change_dependencies(None);
        assert!(state.visible, "and removing the scope restores the default");
    }

    #[test]
    fn asking_an_unbuilt_tooltip_to_show_answers_no_rather_than_throwing() {
        let state = TooltipState::new();
        assert!(!state.ensure_tooltip_visible(false));
        assert!(state.ensure_tooltip_visible(true));
    }

    #[test]
    fn a_tooltip_asked_for_directly_does_not_wait() {
        assert_eq!(TooltipState::DEFAULT_WAIT_DURATION_MS, 0);
        assert_eq!(TooltipState::DEFAULT_SHOW_DURATION_MS, 1500);
        assert!(
            TooltipState::DEFAULT_EXIT_DURATION_MS < TooltipState::DEFAULT_SHOW_DURATION_MS,
            "a tooltip leaves faster than it stays"
        );
    }
}
