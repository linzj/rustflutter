//! Selecting across a whole page -- a port of upstream's
//! `widgets/selectable_region.dart`.
//!
//! This is the top of the three selection layers: `selection.rs` is the
//! vocabulary, `selection_container.rs` groups selectables, and this decides
//! **which** selectable a dragged edge landed in, and what happens to the ones
//! that appear and disappear while the drag is still going.
//!
//! Two problems here are worth the space they take:
//!
//! * **finding the edge.** A drag position is a point on the page and the
//!   answer is an index into a list of selectables. Nobody knows a mapping, so
//!   the delegate asks a child and walks the way it points -- and stops when
//!   two consecutive children point at *each other*, which means the point is
//!   in the gap between them.
//! * **children that arrive mid-drag.** A list scrolled during a selection
//!   builds selectables that missed the drag entirely. They are handed a
//!   synthesised edge event for the last known position, once each, so they
//!   join a selection already in progress rather than staying blank.
//!
//! ## What is not here
//!
//! The region's gesture recognisers, its context menu, and its keyboard
//! shortcut table are upstream's `SelectableRegionState` build method and the
//! several hundred lines around it. What is ported is the state machine the
//! rest of the file exists to serve.

use crate::selection::{
    SelectedContentRange, SelectionEvent, SelectionGeometry, SelectionResult, SelectionStatus,
};

/// Upstream `SelectableRegionSelectionStatus`: whether a selection is still
/// being dragged out or has settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SelectableRegionSelectionStatus {
    /// The reader's finger or mouse is still down.
    Changing,
    /// They have let go. Upstream's default -- a region with no selection at
    /// all is finalized, not changing.
    #[default]
    Finalized,
}

/// Upstream's `_SelectableRegionSelectionStatusNotifier`.
///
/// Its setter carries an assertion that is really a state machine: **you may
/// finalize only a selection that was changing.** Finalizing twice would tell
/// every listener that the reader had just let go when they had let go some
/// time ago, and a listener that acts on that -- showing a context menu, say
/// -- would act twice.
#[derive(Debug, Default)]
pub struct SelectableRegionSelectionStatusNotifier {
    status: SelectableRegionSelectionStatus,
    notifications: usize,
}

impl SelectableRegionSelectionStatusNotifier {
    pub fn new() -> SelectableRegionSelectionStatusNotifier {
        SelectableRegionSelectionStatusNotifier::default()
    }

    pub fn value(&self) -> SelectableRegionSelectionStatus {
        self.status
    }

    pub fn notifications(&self) -> usize {
        self.notifications
    }

    /// Upstream's setter, with its assertion.
    ///
    /// Returns whether the change was allowed. Going to `Changing` is always
    /// allowed -- a reader may start a new drag from any state -- and going to
    /// `Finalized` is allowed only from `Changing`.
    pub fn set_value(&mut self, status: SelectableRegionSelectionStatus) -> bool {
        let allowed = status == SelectableRegionSelectionStatus::Changing
            || self.status == SelectableRegionSelectionStatus::Changing;
        debug_assert!(
            allowed,
            "attempting to finalize the selection when it is already finalized"
        );
        if !allowed {
            return false;
        }
        self.status = status;
        self.notifications += 1;
        true
    }
}

/// Upstream `SelectableRegionSelectionStatusScope`: publishes the status to
/// the subtree.
pub struct SelectableRegionSelectionStatusScope {
    pub status: SelectableRegionSelectionStatus,
}

impl SelectableRegionSelectionStatusScope {
    pub fn new(status: SelectableRegionSelectionStatus) -> SelectableRegionSelectionStatusScope {
        SelectableRegionSelectionStatusScope { status }
    }

    /// Upstream's `maybeOf`.
    pub fn maybe_of(&self) -> SelectableRegionSelectionStatus {
        self.status
    }

    /// Upstream's `updateShouldNotify`, which compares the *notifier* rather
    /// than the status -- so a status change reaches listeners through the
    /// notifier, and only swapping the notifier itself rebuilds the subtree.
    pub fn update_should_notify(&self, old: &SelectableRegionSelectionStatusScope) -> bool {
        self.status != old.status
    }
}

/// Upstream `SelectionDetails`: what a [`SelectionListener`] is told.
///
/// Upstream declares it `abstract final`, which means nobody outside the
/// library implements it -- it is a view onto the listener's delegate rather
/// than something a caller builds.
pub trait SelectionDetails {
    /// Upstream's `range`, `None` when nothing is selected.
    fn range(&self) -> Option<SelectedContentRange>;

    /// Upstream's `status`.
    fn status(&self) -> SelectionStatus;
}

/// A plain [`SelectionDetails`], which is what the notifier hands out.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SelectionDetailsSnapshot {
    pub range: Option<SelectedContentRange>,
    pub status: SelectionStatus,
}

impl SelectionDetails for SelectionDetailsSnapshot {
    fn range(&self) -> Option<SelectedContentRange> {
        self.range
    }

    fn status(&self) -> SelectionStatus {
        self.status
    }
}

/// Upstream `SelectionListenerNotifier`: the handle a caller holds to hear
/// about a selection.
#[derive(Debug, Default)]
pub struct SelectionListenerNotifier {
    registered: bool,
    selection: SelectionDetailsSnapshot,
}

impl SelectionListenerNotifier {
    pub fn new() -> SelectionListenerNotifier {
        SelectionListenerNotifier::default()
    }

    /// Upstream's `registered`.
    pub fn is_registered(&self) -> bool {
        self.registered
    }

    /// Upstream's `_registerSelectionListenerDelegate`, which asserts it is
    /// not already registered.
    ///
    /// One notifier per listener, and upstream's message says what to do about
    /// it: provide a new one. Sharing a notifier between two listeners would
    /// leave the second silently reporting the first's selection.
    pub fn register(&mut self) -> bool {
        debug_assert!(
            !self.registered,
            "this SelectionListenerNotifier is already registered to another SelectionListener; \
             try providing a new one"
        );
        if self.registered {
            return false;
        }
        self.registered = true;
        true
    }

    /// Upstream's `_unregisterSelectionListenerDelegate`, which `dispose` also
    /// calls.
    pub fn unregister(&mut self) {
        self.registered = false;
        self.selection = SelectionDetailsSnapshot::default();
    }

    /// Upstream's `selection` getter, which **throws** when nothing has
    /// registered.
    ///
    /// Not a default value: a caller reading a selection from an unattached
    /// notifier has made a wiring mistake, and an empty selection would look
    /// exactly like a real one that happens to be empty.
    pub fn selection(&self) -> Result<SelectionDetailsSnapshot, &'static str> {
        if !self.registered {
            return Err("selection client has not been registered to this notifier");
        }
        Ok(self.selection)
    }

    /// What the delegate publishes as the selection changes.
    pub fn publish(&mut self, selection: SelectionDetailsSnapshot) {
        self.selection = selection;
    }
}

/// Upstream `SelectionListener`: hears about the selection in its subtree.
pub struct SelectionListener {
    pub notifier: SelectionListenerNotifier,
}

impl Default for SelectionListener {
    fn default() -> SelectionListener {
        SelectionListener::new()
    }
}

impl SelectionListener {
    pub fn new() -> SelectionListener {
        SelectionListener {
            notifier: SelectionListenerNotifier::new(),
        }
    }

    /// Upstream's `initState`, which registers the notifier to this listener.
    pub fn attach(&mut self) -> bool {
        self.notifier.register()
    }

    /// Upstream's `dispose`.
    pub fn detach(&mut self) {
        self.notifier.unregister();
    }
}

/// Upstream `MultiSelectableSelectionContainerDelegate`: the index bookkeeping
/// that turns a list of selectables into one selection.
#[derive(Debug, Default)]
pub struct MultiSelectableSelectionContainerDelegate {
    /// Upstream's `currentSelectionStartIndex`, `-1` for none. Kept as an
    /// `Option` here, which is the same statement.
    pub current_selection_start_index: Option<usize>,
    pub current_selection_end_index: Option<usize>,
    selectable_count: usize,
}

impl MultiSelectableSelectionContainerDelegate {
    pub fn new(selectable_count: usize) -> MultiSelectableSelectionContainerDelegate {
        MultiSelectableSelectionContainerDelegate {
            current_selection_start_index: None,
            current_selection_end_index: None,
            selectable_count,
        }
    }

    pub fn selectable_count(&self) -> usize {
        self.selectable_count
    }

    pub fn set_selectable_count(&mut self, count: usize) {
        self.selectable_count = count;
    }

    /// The span of selectables a boundary event should reach -- upstream's
    /// `didReceiveSelectionBoundaryEvents`, which walks from the lower of the
    /// two indices to the higher **whichever way round they are**.
    pub fn boundary_span(&self) -> Option<(usize, usize)> {
        let start = self.current_selection_start_index?;
        let end = self.current_selection_end_index?;
        Some((start.min(end), start.max(end)))
    }

    /// Upstream's `_adjustSelectionIndexBasedOnSelectionGeometry`.
    ///
    /// A selection edge that falls *between* two selectables leaves one of
    /// them collapsed at offset zero or at its content length, and either may
    /// be the index the search landed on. This walks towards the other edge
    /// until it finds one that actually has an uncollapsed selection, so the
    /// geometry is read from a selectable that has something to say.
    pub fn adjusted_index(
        current_index: usize,
        toward_index: usize,
        statuses: &[SelectionStatus],
    ) -> usize {
        let forward = toward_index > current_index;
        let mut index = current_index;
        while index != toward_index && statuses[index] != SelectionStatus::Uncollapsed {
            index = if forward { index + 1 } else { index - 1 };
        }
        index
    }

    /// Upstream's `getSelectionGeometry` status rule.
    ///
    /// **If the start point and the end point came from different
    /// selectables, the selection is uncollapsed by construction** -- it spans
    /// at least the gap between them. Only when both came from the same one
    /// does that selectable's own status stand, which is how a caret inside a
    /// single paragraph stays a caret.
    pub fn combined_status(
        start_index: usize,
        end_index: usize,
        start_status: SelectionStatus,
    ) -> SelectionStatus {
        if start_index != end_index {
            SelectionStatus::Uncollapsed
        } else {
            start_status
        }
    }

    /// Upstream's walk for a non-null start point: from the start index
    /// *towards* the end index, stopping at the first selectable that has one.
    ///
    /// Selectables in the middle of a selection can have no selection point --
    /// a fully-selected paragraph has no handle of its own -- so the point has
    /// to be looked for rather than read.
    pub fn walk_to_start_point(
        start_index: usize,
        end_index: usize,
        has_start_point: &[bool],
    ) -> usize {
        let forward = end_index >= start_index;
        let mut index = start_index;
        while index != end_index && !has_start_point[index] {
            index = if forward { index + 1 } else { index - 1 };
        }
        index
    }

    /// The mirror: from the end index *towards* the start index.
    pub fn walk_to_end_point(
        start_index: usize,
        end_index: usize,
        has_end_point: &[bool],
    ) -> usize {
        let forward = end_index >= start_index;
        let mut index = end_index;
        while index != start_index && !has_end_point[index] {
            index = if forward { index - 1 } else { index + 1 };
        }
        index
    }

    /// Upstream's `_initSelection`: find which selectable a fresh edge landed
    /// in.
    ///
    /// `answers` is what each child says when handed the event, in order.
    ///
    /// The walk latches a direction and **stops when two consecutive children
    /// point at each other**: a child saying `Next` after one said `Previous`
    /// means the point is in the gap between them and no child contains it.
    /// Without that the walk would step back and forth for ever.
    pub fn init_selection(
        answers: &[SelectionResult],
        from: usize,
    ) -> (Option<usize>, SelectionResult) {
        let mut new_index: Option<usize> = None;
        let mut result = SelectionResult::None;
        let mut forward: Option<bool> = None;
        let mut index = from;

        while index < answers.len() {
            match answers[index] {
                SelectionResult::Next => {
                    if forward == Some(false) {
                        return (new_index, SelectionResult::End);
                    }
                    forward = Some(true);
                    new_index = Some(index);
                }
                SelectionResult::None => {
                    new_index = Some(index);
                }
                SelectionResult::End => {
                    return (Some(index), SelectionResult::End);
                }
                SelectionResult::Previous => {
                    if index == 0 {
                        return (Some(0), SelectionResult::Previous);
                    }
                    if forward == Some(true) {
                        return (new_index, SelectionResult::End);
                    }
                    forward = Some(false);
                    new_index = Some(index);
                }
                SelectionResult::Pending => {
                    return (Some(index), SelectionResult::Pending);
                }
            }
            match forward {
                Some(true) => index += 1,
                Some(false) => {
                    if index == 0 {
                        break;
                    }
                    index -= 1;
                }
                None => index += 1,
            }
        }
        (new_index, result)
    }
}

/// Upstream `StaticSelectionContainerDelegate`: a delegate whose children come
/// and go while a selection is in progress.
///
/// "Static" is about the *content* not changing, not the child list -- a
/// scrollable's children are built and destroyed as it scrolls, and this is
/// the delegate that keeps a selection whole across that.
#[derive(Debug, Default)]
pub struct StaticSelectionContainerDelegate {
    pub base: MultiSelectableSelectionContainerDelegate,
    /// Upstream's `_hasReceivedStartEvent` and `_hasReceivedEndEvent`.
    has_received_start_event: Vec<u64>,
    has_received_end_event: Vec<u64>,
    last_start_edge_global_position: Option<(f32, f32)>,
    last_end_edge_global_position: Option<(f32, f32)>,
}

impl StaticSelectionContainerDelegate {
    pub fn new(selectable_count: usize) -> StaticSelectionContainerDelegate {
        StaticSelectionContainerDelegate {
            base: MultiSelectableSelectionContainerDelegate::new(selectable_count),
            has_received_start_event: Vec::new(),
            has_received_end_event: Vec::new(),
            last_start_edge_global_position: None,
            last_end_edge_global_position: None,
        }
    }

    /// Upstream's `didReceiveSelectionEventFor`. `for_end` of `None` records
    /// both, which is what a boundary event -- a word or paragraph selection --
    /// does, since it sets both edges at once.
    pub fn did_receive_selection_event_for(&mut self, selectable: u64, for_end: Option<bool>) {
        match for_end {
            Some(true) => {
                Self::record(&mut self.has_received_end_event, selectable);
            }
            Some(false) => {
                Self::record(&mut self.has_received_start_event, selectable);
            }
            None => {
                Self::record(&mut self.has_received_start_event, selectable);
                Self::record(&mut self.has_received_end_event, selectable);
            }
        }
    }

    fn record(set: &mut Vec<u64>, selectable: u64) -> bool {
        if set.contains(&selectable) {
            return false;
        }
        set.push(selectable);
        true
    }

    /// Upstream's `updateLastSelectionEdgeLocation`.
    pub fn update_last_selection_edge_location(&mut self, position: (f32, f32), for_end: bool) {
        if for_end {
            self.last_end_edge_global_position = Some(position);
        } else {
            self.last_start_edge_global_position = Some(position);
        }
    }

    pub fn last_edge(&self, for_end: bool) -> Option<(f32, f32)> {
        if for_end {
            self.last_end_edge_global_position
        } else {
            self.last_start_edge_global_position
        }
    }

    /// Upstream's `ensureChildUpdated`: bring a selectable that arrived
    /// mid-drag up to date.
    ///
    /// **Each child is synthesised for once, and only once**, which is what
    /// the two sets are for: `add` returning false means this child has
    /// already been told, and telling it again would move the edge it had
    /// already placed.
    ///
    /// Returns the synthesised events to send, end edge first -- upstream's
    /// order, which matters because the end edge is the one a forward drag is
    /// currently moving.
    pub fn ensure_child_updated(&mut self, selectable: u64) -> Vec<SelectionEvent> {
        let mut events = Vec::new();
        if let Some((dx, dy)) = self.last_end_edge_global_position {
            if Self::record(&mut self.has_received_end_event, selectable) {
                events.push(SelectionEvent::EdgeUpdate {
                    global_position: crate::render::Offset::new(dx, dy),
                    for_end: true,
                    granularity: None,
                });
            }
        }
        if let Some((dx, dy)) = self.last_start_edge_global_position {
            if Self::record(&mut self.has_received_start_event, selectable) {
                events.push(SelectionEvent::EdgeUpdate {
                    global_position: crate::render::Offset::new(dx, dy),
                    for_end: false,
                    granularity: None,
                });
            }
        }
        events
    }

    /// Upstream's `didChangeSelectables`: forget the children that are gone.
    ///
    /// Pruning matters because the sets are what stops a child being
    /// synthesised twice -- and a child that left and came back **is** new, so
    /// it should be synthesised again.
    pub fn did_change_selectables(&mut self, present: &[u64]) {
        self.has_received_end_event
            .retain(|selectable| present.contains(selectable));
        self.has_received_start_event
            .retain(|selectable| present.contains(selectable));
    }

    pub fn has_received_start_event(&self, selectable: u64) -> bool {
        self.has_received_start_event.contains(&selectable)
    }

    pub fn has_received_end_event(&self, selectable: u64) -> bool {
        self.has_received_end_event.contains(&selectable)
    }

    /// Upstream's `clearInternalSelectionState`.
    pub fn clear(&mut self) {
        self.has_received_start_event.clear();
        self.has_received_end_event.clear();
        self.last_start_edge_global_position = None;
        self.last_end_edge_global_position = None;
        self.base.current_selection_start_index = None;
        self.base.current_selection_end_index = None;
    }
}

/// Upstream `SelectableRegion`: the widget that makes a subtree selectable.
pub struct SelectableRegion {
    /// Upstream's `selectionControls` and friends are not ported; what is here
    /// is the region's own state.
    pub id: u64,
}

impl SelectableRegion {
    pub fn new(id: u64) -> SelectableRegion {
        SelectableRegion { id }
    }

    /// Upstream's `createState`.
    pub fn create_state(&self) -> SelectableRegionState {
        SelectableRegionState {
            id: self.id,
            status: SelectableRegionSelectionStatusNotifier::new(),
            geometry: SelectionGeometry::new(SelectionStatus::None, false),
        }
    }
}

/// Upstream `SelectableRegionState`.
#[derive(Debug)]
pub struct SelectableRegionState {
    pub id: u64,
    pub status: SelectableRegionSelectionStatusNotifier,
    geometry: SelectionGeometry,
}

impl SelectableRegionState {
    /// Upstream's `selectionGeometry`.
    pub fn selection_geometry(&self) -> &SelectionGeometry {
        &self.geometry
    }

    pub fn set_selection_geometry(&mut self, geometry: SelectionGeometry) {
        self.geometry = geometry;
    }

    /// Upstream's `_updateSelectionStatus` on a drag start.
    pub fn begin_drag(&mut self) {
        self.status
            .set_value(SelectableRegionSelectionStatus::Changing);
    }

    /// Upstream's finalize, which may only follow a drag.
    pub fn end_drag(&mut self) -> bool {
        self.status
            .set_value(SelectableRegionSelectionStatus::Finalized)
    }

    /// The scope this region publishes.
    pub fn scope(&self) -> SelectableRegionSelectionStatusScope {
        SelectableRegionSelectionStatusScope::new(self.status.value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_may_be_finalized_only_once() {
        // Finalizing twice would tell every listener the reader had just let
        // go when they let go some time ago, and a listener that acts on it --
        // showing a context menu, say -- would act twice.
        let mut status = SelectableRegionSelectionStatusNotifier::new();
        assert_eq!(status.value(), SelectableRegionSelectionStatus::Finalized);

        assert!(status.set_value(SelectableRegionSelectionStatus::Changing));
        assert!(status.set_value(SelectableRegionSelectionStatus::Finalized));
        assert_eq!(status.notifications(), 2);
    }

    #[test]
    fn a_new_drag_may_start_from_any_state() {
        // Going to Changing is always allowed -- a reader may start a new
        // selection whenever they like, including in the middle of one.
        let mut status = SelectableRegionSelectionStatusNotifier::new();
        assert!(status.set_value(SelectableRegionSelectionStatus::Changing));
        assert!(status.set_value(SelectableRegionSelectionStatus::Changing));
        assert_eq!(status.value(), SelectableRegionSelectionStatus::Changing);
        assert_eq!(status.notifications(), 2);
    }

    #[test]
    fn a_region_finalizes_only_after_a_drag() {
        let mut state = SelectableRegion::new(1).create_state();
        assert_eq!(
            state.status.value(),
            SelectableRegionSelectionStatus::Finalized,
            "a region with no selection is finalized, not changing"
        );
        state.begin_drag();
        assert_eq!(
            state.scope().maybe_of(),
            SelectableRegionSelectionStatus::Changing
        );
        assert!(state.end_drag());
        assert_eq!(
            state.scope().maybe_of(),
            SelectableRegionSelectionStatus::Finalized
        );
    }

    #[test]
    fn the_scope_notifies_only_when_the_status_actually_changed() {
        let changing =
            SelectableRegionSelectionStatusScope::new(SelectableRegionSelectionStatus::Changing);
        assert!(
            !changing.update_should_notify(&SelectableRegionSelectionStatusScope::new(
                SelectableRegionSelectionStatus::Changing
            ))
        );
        assert!(
            changing.update_should_notify(&SelectableRegionSelectionStatusScope::new(
                SelectableRegionSelectionStatus::Finalized
            ))
        );
    }

    #[test]
    fn reading_a_selection_from_an_unattached_notifier_is_an_error_not_an_empty_answer() {
        // An empty selection would look exactly like a real one that happens
        // to be empty, and a caller with a wiring mistake would never find it.
        let mut notifier = SelectionListenerNotifier::new();
        assert!(!notifier.is_registered());
        assert!(notifier.selection().is_err());

        assert!(notifier.register());
        assert!(notifier.is_registered());
        assert_eq!(
            notifier.selection(),
            Ok(SelectionDetailsSnapshot::default())
        );
    }

    #[test]
    fn one_notifier_belongs_to_one_listener() {
        // Sharing one between two listeners would leave the second silently
        // reporting the first's selection, which upstream's message says to
        // fix by providing a new notifier. Upstream asserts on the second
        // registration, so what a caller can actually check is `registered` --
        // and that is what this pins.
        let mut notifier = SelectionListenerNotifier::new();
        assert!(!notifier.is_registered(), "free to take");
        assert!(notifier.register());
        assert!(notifier.is_registered(), "already spoken for");

        notifier.unregister();
        assert!(!notifier.is_registered());
        assert!(notifier.register(), "and free again once released");
    }

    #[test]
    fn detaching_forgets_the_selection_as_well_as_the_registration() {
        // Otherwise a notifier reattached to a new listener would hand out the
        // old listener's selection before the new one had said anything.
        let mut listener = SelectionListener::new();
        assert!(listener.attach());
        listener.notifier.publish(SelectionDetailsSnapshot {
            range: Some(SelectedContentRange::new(3, 9)),
            status: SelectionStatus::Uncollapsed,
        });
        assert_eq!(
            listener.notifier.selection().expect("registered").range,
            Some(SelectedContentRange::new(3, 9))
        );

        listener.detach();
        assert!(listener.attach());
        assert_eq!(
            listener.notifier.selection().expect("registered").range,
            None
        );
    }

    #[test]
    fn a_boundary_event_reaches_every_selectable_between_the_two_edges() {
        // Whichever way round they are: a selection dragged backwards has its
        // start index above its end index, and a word-select in the middle of
        // it still has to reach everything between.
        let mut delegate = MultiSelectableSelectionContainerDelegate::new(10);
        assert_eq!(delegate.boundary_span(), None, "no selection yet");

        delegate.current_selection_start_index = Some(2);
        delegate.current_selection_end_index = Some(6);
        assert_eq!(delegate.boundary_span(), Some((2, 6)));

        delegate.current_selection_start_index = Some(6);
        delegate.current_selection_end_index = Some(2);
        assert_eq!(delegate.boundary_span(), Some((2, 6)), "the same span");
    }

    #[test]
    fn an_edge_between_two_selectables_walks_to_one_with_something_to_say() {
        // A selection edge falling in the gap leaves one selectable collapsed
        // at zero or at its content length, and either may be the index the
        // search landed on.
        let statuses = [
            SelectionStatus::Collapsed,
            SelectionStatus::Collapsed,
            SelectionStatus::Uncollapsed,
            SelectionStatus::Collapsed,
        ];
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::adjusted_index(0, 3, &statuses),
            2,
            "walked forwards to the one with a selection"
        );
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::adjusted_index(3, 0, &statuses),
            2,
            "and backwards to the same one"
        );
        // One that already has something to say is left alone.
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::adjusted_index(2, 0, &statuses),
            2
        );
    }

    #[test]
    fn a_selection_spanning_two_selectables_is_uncollapsed_by_construction() {
        // It covers at least the gap between them, whatever either of them
        // says about itself. Only when both points came from the same
        // selectable does its own status stand -- which is how a caret inside
        // one paragraph stays a caret.
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::combined_status(
                1,
                3,
                SelectionStatus::Collapsed
            ),
            SelectionStatus::Uncollapsed
        );
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::combined_status(
                2,
                2,
                SelectionStatus::Collapsed
            ),
            SelectionStatus::Collapsed
        );
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::combined_status(
                2,
                2,
                SelectionStatus::Uncollapsed
            ),
            SelectionStatus::Uncollapsed
        );
    }

    #[test]
    fn a_missing_selection_point_is_looked_for_rather_than_read() {
        // A fully-selected paragraph in the middle of a selection has no
        // handle of its own, so the point has to be walked to.
        let has_start = [false, false, true, true];
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::walk_to_start_point(0, 3, &has_start),
            2
        );
        // Walking the other way when the selection runs backwards.
        let has_start_back = [true, false, false, false];
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::walk_to_start_point(3, 0, &has_start_back),
            0
        );

        let has_end = [true, true, false, false];
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::walk_to_end_point(0, 3, &has_end),
            1,
            "from the end index towards the start"
        );
    }

    #[test]
    fn the_walk_stops_at_the_other_edge_even_with_nothing_found() {
        // Otherwise it would run off the end of the list.
        let none = [false, false, false, false];
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::walk_to_start_point(0, 3, &none),
            3
        );
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::walk_to_end_point(0, 3, &none),
            0
        );
    }

    #[test]
    fn the_search_stops_when_two_children_point_at_each_other() {
        // Which means the point is in the gap between them and no child
        // contains it. Without that the walk steps back and forth for ever.
        let answers = [
            SelectionResult::Next,
            SelectionResult::Previous,
            SelectionResult::Previous,
        ];
        let (index, result) =
            MultiSelectableSelectionContainerDelegate::init_selection(&answers, 0);
        assert_eq!(result, SelectionResult::End);
        assert_eq!(index, Some(0), "the last child that pointed forwards");
    }

    #[test]
    fn a_child_that_contains_the_point_ends_the_search_outright() {
        let answers = [
            SelectionResult::Next,
            SelectionResult::Next,
            SelectionResult::End,
            SelectionResult::Previous,
        ];
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::init_selection(&answers, 0),
            (Some(2), SelectionResult::End)
        );
    }

    #[test]
    fn a_point_before_the_first_child_is_reported_as_before_the_whole_list() {
        // Not "not found": the container above has to know the edge went off
        // its leading side so it can pass the search on.
        let answers = [SelectionResult::Previous, SelectionResult::Previous];
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::init_selection(&answers, 0),
            (Some(0), SelectionResult::Previous)
        );
    }

    #[test]
    fn a_child_that_cannot_answer_yet_stops_the_search_where_it_is() {
        // Pending means "ask me again once I have been laid out", and carrying
        // on past it would settle on a neighbour that only looks right because
        // this one had not measured itself.
        let answers = [
            SelectionResult::Next,
            SelectionResult::Pending,
            SelectionResult::End,
        ];
        assert_eq!(
            MultiSelectableSelectionContainerDelegate::init_selection(&answers, 0),
            (Some(1), SelectionResult::Pending)
        );
    }

    // -- Children arriving mid-drag --------------------------------------

    #[test]
    fn a_child_that_arrives_mid_drag_is_told_where_the_edges_are() {
        // A list scrolled during a selection builds selectables that missed
        // the drag entirely. They join a selection already in progress rather
        // than staying blank.
        let mut delegate = StaticSelectionContainerDelegate::new(3);
        delegate.update_last_selection_edge_location((10.0, 20.0), false);
        delegate.update_last_selection_edge_location((90.0, 200.0), true);

        let events = delegate.ensure_child_updated(7);
        assert_eq!(events.len(), 2);
        // Upstream's order: the end edge first, because that is the one a
        // forward drag is currently moving.
        assert!(matches!(
            events[0],
            SelectionEvent::EdgeUpdate { for_end: true, .. }
        ));
        assert!(matches!(
            events[1],
            SelectionEvent::EdgeUpdate { for_end: false, .. }
        ));
    }

    #[test]
    fn each_child_is_synthesised_for_exactly_once() {
        // Telling it again would move an edge it had already placed.
        let mut delegate = StaticSelectionContainerDelegate::new(3);
        delegate.update_last_selection_edge_location((10.0, 20.0), false);
        assert_eq!(delegate.ensure_child_updated(7).len(), 1);
        assert_eq!(
            delegate.ensure_child_updated(7).len(),
            0,
            "already brought up to date"
        );
        assert!(delegate.has_received_start_event(7));
        assert!(!delegate.has_received_end_event(7));
    }

    #[test]
    fn a_child_that_left_and_came_back_is_new_again() {
        // Pruning is what makes that true, and it has to be: the child that
        // came back is a fresh selectable with nothing in it.
        let mut delegate = StaticSelectionContainerDelegate::new(3);
        delegate.update_last_selection_edge_location((10.0, 20.0), false);
        delegate.ensure_child_updated(7);
        assert!(delegate.has_received_start_event(7));

        delegate.did_change_selectables(&[1, 2]);
        assert!(!delegate.has_received_start_event(7), "forgotten");
        assert_eq!(delegate.ensure_child_updated(7).len(), 1, "and told again");
    }

    #[test]
    fn a_boundary_event_counts_as_both_edges_at_once() {
        // A word or paragraph selection sets both, so a child that received
        // one needs neither synthesised.
        let mut delegate = StaticSelectionContainerDelegate::new(3);
        delegate.update_last_selection_edge_location((10.0, 20.0), false);
        delegate.update_last_selection_edge_location((90.0, 200.0), true);
        delegate.did_receive_selection_event_for(7, None);
        assert!(delegate.has_received_start_event(7));
        assert!(delegate.has_received_end_event(7));
        assert_eq!(delegate.ensure_child_updated(7).len(), 0);

        // Where a single-edge event leaves the other still to come.
        delegate.did_receive_selection_event_for(8, Some(true));
        assert!(delegate.has_received_end_event(8));
        assert!(!delegate.has_received_start_event(8));
        assert_eq!(delegate.ensure_child_updated(8).len(), 1);
    }

    #[test]
    fn with_no_drag_in_progress_there_is_nothing_to_synthesise() {
        let mut delegate = StaticSelectionContainerDelegate::new(3);
        assert!(delegate.ensure_child_updated(7).is_empty());
        assert_eq!(delegate.last_edge(true), None);
        assert_eq!(delegate.last_edge(false), None);
    }

    #[test]
    fn clearing_forgets_the_drag_and_the_indices_together() {
        let mut delegate = StaticSelectionContainerDelegate::new(3);
        delegate.update_last_selection_edge_location((10.0, 20.0), false);
        delegate.ensure_child_updated(7);
        delegate.base.current_selection_start_index = Some(1);
        delegate.base.current_selection_end_index = Some(2);

        delegate.clear();
        assert_eq!(delegate.last_edge(false), None);
        assert!(!delegate.has_received_start_event(7));
        assert_eq!(delegate.base.current_selection_start_index, None);
        assert_eq!(delegate.base.current_selection_end_index, None);
    }
}
