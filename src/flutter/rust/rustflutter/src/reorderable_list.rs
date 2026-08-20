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
use crate::render::{Axis, Offset};

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

/// Upstream `SliverReorderableList`: the sliver a reorderable list is made of.
pub struct SliverReorderableList {
    pub item_count: usize,
    pub axis: Axis,
    pub reverse: bool,
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
}

impl ReorderableList {
    pub fn new(item_count: usize) -> ReorderableList {
        ReorderableList {
            sliver: SliverReorderableList::new(item_count),
        }
    }

    pub fn with_axis(mut self, axis: Axis) -> Self {
        self.sliver = self.sliver.with_axis(axis);
        self
    }

    pub fn with_reverse(mut self, reverse: bool) -> Self {
        self.sliver = self.sliver.with_reverse(reverse);
        self
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
}
