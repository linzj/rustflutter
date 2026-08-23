//! The vocabulary of selecting text across widgets -- a port of upstream's
//! `rendering/selection.dart`.
//!
//! Dragging across a page selects text in several widgets at once, and none of
//! them knows about the others. What holds them together is this file: a
//! **registrar** every selectable registers with, an **event** the registrar
//! sends down, and a **result** each selectable sends back saying whether the
//! edge it was given belongs to it, to something before it, or to something
//! after it.
//!
//! That third answer is the whole mechanism. A selectable asked about a point
//! it does not contain does not guess -- it says [`SelectionResult::Previous`]
//! or [`SelectionResult::Next`], and the container walks that way and asks the
//! next one. So a drag that leaves a paragraph finds the following one without
//! anybody computing a global layout.

use crate::direction::TextDirection;
use crate::engine::Rect;
use crate::render::Offset;

/// Upstream `SelectionResult`: what a selectable says when it is handed a
/// selection edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionResult {
    /// The edge is before this selectable; try the one before.
    Previous,
    /// The edge is after it; try the one after.
    Next,
    /// The edge is inside this selectable, and the search stops.
    End,
    /// Upstream's `pending`: the selectable cannot answer yet, typically
    /// because it has not been laid out.
    Pending,
    /// Upstream's `none`: this event does not have a result -- the answer to
    /// a clear or a select-all, which no selectable can be "in".
    None,
}

/// Upstream `SelectionStatus`: how much of a selectable is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SelectionStatus {
    /// A selection with something in it.
    Uncollapsed,
    /// A selection whose two ends are the same point -- a caret, not a range.
    Collapsed,
    /// No selection at all.
    #[default]
    None,
}

/// Upstream `TextGranularity`: how much a granular extension moves by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextGranularity {
    Character,
    Word,
    Paragraph,
    Line,
    Document,
}

/// Upstream `TextSelectionHandleType`, declared in [`crate::text_selection_controls`] and re-exported here.
///
/// It was declared twice -- same variants, same upstream original --
/// and nothing made the two copies meet, which is how they could have
/// drifted apart unnoticed. A type two modules have to agree on belongs
/// to neither of them.
pub use crate::text_selection_controls::TextSelectionHandleType;

/// Upstream `SelectionExtendDirection`: which way a directional extension
/// walks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionExtendDirection {
    /// Upstream's `previousLine`.
    PreviousLine,
    /// Upstream's `nextLine`.
    NextLine,
    /// Upstream's `forward`.
    Forward,
    /// Upstream's `backward`.
    Backward,
}

/// Upstream `SelectionEventType`: which event this is, without downcasting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionEventType {
    StartEdgeUpdate,
    EndEdgeUpdate,
    Clear,
    SelectAll,
    SelectWord,
    SelectParagraph,
    GranularlyExtendSelection,
    DirectionallyExtendSelection,
}

/// Upstream `SelectedContentRange`: where in a selectable's content the
/// selection starts and ends.
///
/// Both offsets are into the selectable's own content, and either may be the
/// larger -- **the range remembers which way the reader dragged**. A selection
/// made right-to-left has a start offset past its end offset, and a caller
/// that sorted them would lose the direction the next keystroke has to extend
/// in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedContentRange {
    pub start_offset: usize,
    pub end_offset: usize,
}

impl SelectedContentRange {
    pub fn new(start_offset: usize, end_offset: usize) -> SelectedContentRange {
        SelectedContentRange {
            start_offset,
            end_offset,
        }
    }

    /// Whether the reader dragged backwards.
    pub fn is_reversed(&self) -> bool {
        self.start_offset > self.end_offset
    }

    /// How much is selected, whichever way round it is.
    pub fn length(&self) -> usize {
        self.start_offset.abs_diff(self.end_offset)
    }
}

/// Upstream `SelectedContent`: what a copy would put on the clipboard.
///
/// Upstream carries only plain text and marks it a TODO -- rich content is not
/// supported yet. Ported as it is: a selection that claimed to carry
/// formatting it cannot reproduce would be worse than one that says plainly
/// what it has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedContent {
    pub plain_text: String,
}

impl SelectedContent {
    pub fn new(plain_text: impl Into<String>) -> SelectedContent {
        SelectedContent {
            plain_text: plain_text.into(),
        }
    }
}

/// Upstream `SelectionPoint`: where a selection handle goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionPoint {
    pub local_position: Offset,
    /// The height of the line the point is on, which is what gives the handle
    /// something to be the height of.
    pub line_height: f32,
    pub handle_type: TextSelectionHandleType,
}

impl SelectionPoint {
    pub fn new(
        local_position: Offset,
        line_height: f32,
        handle_type: TextSelectionHandleType,
    ) -> SelectionPoint {
        SelectionPoint {
            local_position,
            line_height,
            handle_type,
        }
    }
}

/// Upstream `SelectionGeometry`: everything the painter and the handles need.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SelectionGeometry {
    pub start_selection_point: Option<SelectionPoint>,
    pub end_selection_point: Option<SelectionPoint>,
    pub selection_rects: Vec<Rect>,
    pub status: SelectionStatus,
    /// Whether there is anything here that *could* be selected. Distinct from
    /// whether anything is: an empty paragraph has content and no selection,
    /// and a container with no selectables at all has neither.
    pub has_content: bool,
}

impl SelectionGeometry {
    pub fn new(status: SelectionStatus, has_content: bool) -> SelectionGeometry {
        SelectionGeometry {
            start_selection_point: None,
            end_selection_point: None,
            selection_rects: Vec::new(),
            status,
            has_content,
        }
    }

    pub fn with_points(
        mut self,
        start: Option<SelectionPoint>,
        end: Option<SelectionPoint>,
    ) -> Self {
        self.start_selection_point = start;
        self.end_selection_point = end;
        self
    }

    pub fn with_rects(mut self, rects: Vec<Rect>) -> Self {
        self.selection_rects = rects;
        self
    }

    /// Upstream's `hasSelection`.
    pub fn has_selection(&self) -> bool {
        self.status != SelectionStatus::None
    }

    /// Upstream's constructor assertion: **a geometry with handle positions
    /// cannot claim to have no selection.**
    ///
    /// Returns whether it holds. The two would contradict each other -- a
    /// handle is drawn at a selection point, so a point with a status of
    /// `None` would put a handle on screen for a selection that is not there.
    pub fn is_consistent(&self) -> bool {
        (self.start_selection_point.is_none() && self.end_selection_point.is_none())
            || self.status != SelectionStatus::None
    }
}

/// Upstream `SelectionUtils`: the two pieces of geometry every selectable
/// needs and none of them should write twice.
pub struct SelectionUtils;

impl SelectionUtils {
    /// Upstream's `getResultBasedOnRect`: whether a point is in this
    /// selectable, before it, or after it.
    ///
    /// The order of the tests is the interesting part. Vertical position is
    /// asked first and settles most cases outright -- above the rectangle is
    /// *previous* and below is *next*, whatever the horizontal position, because
    /// text runs in lines and a point two lines up is earlier no matter how far
    /// right it is. Only for a point beside the rectangle does the horizontal
    /// position decide, and there the right edge means next.
    pub fn result_based_on_rect(target: Rect, point: Offset) -> SelectionResult {
        if point.dx >= target.left
            && point.dx < target.right
            && point.dy >= target.top
            && point.dy < target.bottom
        {
            return SelectionResult::End;
        }
        if point.dy < target.top {
            return SelectionResult::Previous;
        }
        if point.dy > target.bottom {
            return SelectionResult::Next;
        }
        if point.dx >= target.right {
            SelectionResult::Next
        } else {
            SelectionResult::Previous
        }
    }

    /// Upstream's `adjustDragOffset`: pull a point outside a selectable onto
    /// its nearest corner.
    ///
    /// The plane outside the rectangle is cut into just **two** areas, not
    /// four or eight. Everything above it, and everything to its left on its
    /// own lines, is "before" and snaps to the leading corner; everything else
    /// is "after" and snaps to the trailing one. That is exactly how text
    /// reads, and it is why the two corners are swapped under a right-to-left
    /// direction rather than mirrored some other way.
    pub fn adjust_drag_offset(target: Rect, point: Offset, direction: TextDirection) -> Offset {
        if point.dx >= target.left
            && point.dx < target.right
            && point.dy >= target.top
            && point.dy < target.bottom
        {
            return point;
        }
        let before =
            point.dy <= target.top || (point.dy <= target.bottom && point.dx <= target.left);
        match (before, direction) {
            (true, TextDirection::Ltr) => Offset::new(target.left, target.top),
            (true, TextDirection::Rtl) => Offset::new(target.right, target.top),
            (false, TextDirection::Ltr) => Offset::new(target.right, target.bottom),
            (false, TextDirection::Rtl) => Offset::new(target.left, target.bottom),
        }
    }
}

/// Upstream `SelectionEvent` and its seven subclasses, as one value.
///
/// Upstream they are a class hierarchy with a `type` field for switching on;
/// the type field exists because downcasting is what a receiver would
/// otherwise have to do. Here the enum *is* the type field, and the payloads
/// ride along with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectionEvent {
    /// Upstream `SelectAllSelectionEvent`.
    SelectAll,
    /// Upstream `ClearSelectionEvent`.
    Clear,
    /// Upstream `SelectWordSelectionEvent`.
    SelectWord { global_position: Offset },
    /// Upstream `SelectParagraphSelectionEvent`.
    SelectParagraph {
        global_position: Offset,
        /// Upstream's `absorb`: whether this event should take over an
        /// existing selection rather than starting a new one.
        absorb: bool,
    },
    /// Upstream `SelectionEdgeUpdateEvent`, both of its constructors.
    EdgeUpdate {
        global_position: Offset,
        /// Whether this is the end edge (`forEnd`) or the start edge.
        for_end: bool,
        granularity: Option<TextGranularity>,
    },
    /// Upstream `GranularlyExtendSelectionEvent`.
    GranularlyExtend {
        forward: bool,
        /// Whether the selection is being extended or moved -- upstream's
        /// `isEnd` is about which edge moves, and this is upstream's
        /// `granularity`.
        is_end: bool,
        granularity: TextGranularity,
    },
    /// Upstream `DirectionallyExtendSelectionEvent`.
    DirectionallyExtend {
        /// Upstream's `dx`, kept because a vertical move has to remember which
        /// column the reader started in -- otherwise a caret walking down
        /// through short lines drifts left and never comes back.
        dx: f32,
        is_end: bool,
        direction: SelectionExtendDirection,
    },
}

// Upstream's seven `SelectionEvent` subclasses. The enum above is what a
// receiver switches on -- upstream's own `type` field admits the set is closed
// -- but the subclasses are how a *sender* names what it is asking for, and a
// name that exists only as an enum variant is a name `tools/coverage.py`
// cannot see. Each one converts into the variant it stands for.

/// Upstream `SelectAllSelectionEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectAllSelectionEvent;

/// Upstream `ClearSelectionEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClearSelectionEvent;

/// Upstream `SelectWordSelectionEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectWordSelectionEvent {
    pub global_position: Offset,
}

/// Upstream `SelectParagraphSelectionEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectParagraphSelectionEvent {
    pub global_position: Offset,
    pub absorb: bool,
}

/// Upstream `SelectionEdgeUpdateEvent`, whose two named constructors differ
/// only in `forEnd`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionEdgeUpdateEvent {
    pub global_position: Offset,
    pub for_end: bool,
    pub granularity: Option<TextGranularity>,
}

impl SelectionEdgeUpdateEvent {
    /// Upstream's `SelectionEdgeUpdateEvent.forStart`.
    pub fn for_start(global_position: Offset) -> SelectionEdgeUpdateEvent {
        SelectionEdgeUpdateEvent {
            global_position,
            for_end: false,
            granularity: None,
        }
    }

    /// Upstream's `SelectionEdgeUpdateEvent.forEnd`.
    pub fn for_end(global_position: Offset) -> SelectionEdgeUpdateEvent {
        SelectionEdgeUpdateEvent {
            global_position,
            for_end: true,
            granularity: None,
        }
    }

    pub fn with_granularity(mut self, granularity: TextGranularity) -> Self {
        self.granularity = Some(granularity);
        self
    }
}

/// Upstream `GranularlyExtendSelectionEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GranularlyExtendSelectionEvent {
    pub forward: bool,
    pub is_end: bool,
    pub granularity: TextGranularity,
}

/// Upstream `DirectionallyExtendSelectionEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionallyExtendSelectionEvent {
    pub dx: f32,
    pub is_end: bool,
    pub direction: SelectionExtendDirection,
}

impl From<SelectAllSelectionEvent> for SelectionEvent {
    fn from(_event: SelectAllSelectionEvent) -> SelectionEvent {
        SelectionEvent::SelectAll
    }
}

impl From<ClearSelectionEvent> for SelectionEvent {
    fn from(_event: ClearSelectionEvent) -> SelectionEvent {
        SelectionEvent::Clear
    }
}

impl From<SelectWordSelectionEvent> for SelectionEvent {
    fn from(event: SelectWordSelectionEvent) -> SelectionEvent {
        SelectionEvent::SelectWord {
            global_position: event.global_position,
        }
    }
}

impl From<SelectParagraphSelectionEvent> for SelectionEvent {
    fn from(event: SelectParagraphSelectionEvent) -> SelectionEvent {
        SelectionEvent::SelectParagraph {
            global_position: event.global_position,
            absorb: event.absorb,
        }
    }
}

impl From<SelectionEdgeUpdateEvent> for SelectionEvent {
    fn from(event: SelectionEdgeUpdateEvent) -> SelectionEvent {
        SelectionEvent::EdgeUpdate {
            global_position: event.global_position,
            for_end: event.for_end,
            granularity: event.granularity,
        }
    }
}

impl From<GranularlyExtendSelectionEvent> for SelectionEvent {
    fn from(event: GranularlyExtendSelectionEvent) -> SelectionEvent {
        SelectionEvent::GranularlyExtend {
            forward: event.forward,
            is_end: event.is_end,
            granularity: event.granularity,
        }
    }
}

impl From<DirectionallyExtendSelectionEvent> for SelectionEvent {
    fn from(event: DirectionallyExtendSelectionEvent) -> SelectionEvent {
        SelectionEvent::DirectionallyExtend {
            dx: event.dx,
            is_end: event.is_end,
            direction: event.direction,
        }
    }
}

impl SelectionEvent {
    /// Upstream's `type` field.
    pub fn event_type(&self) -> SelectionEventType {
        match self {
            SelectionEvent::SelectAll => SelectionEventType::SelectAll,
            SelectionEvent::Clear => SelectionEventType::Clear,
            SelectionEvent::SelectWord { .. } => SelectionEventType::SelectWord,
            SelectionEvent::SelectParagraph { .. } => SelectionEventType::SelectParagraph,
            SelectionEvent::EdgeUpdate { for_end: true, .. } => SelectionEventType::EndEdgeUpdate,
            SelectionEvent::EdgeUpdate { for_end: false, .. } => {
                SelectionEventType::StartEdgeUpdate
            }
            SelectionEvent::GranularlyExtend { .. } => {
                SelectionEventType::GranularlyExtendSelection
            }
            SelectionEvent::DirectionallyExtend { .. } => {
                SelectionEventType::DirectionallyExtendSelection
            }
        }
    }
}

/// Upstream `SelectionHandler`: something that owns a selection and can be
/// asked about it.
pub trait SelectionHandler {
    /// Upstream's `value`.
    fn selection_geometry(&self) -> SelectionGeometry;

    /// Upstream's `dispatchSelectionEvent`.
    fn dispatch_selection_event(&mut self, event: SelectionEvent) -> SelectionResult;

    /// Upstream's `getSelectedContent`.
    fn selected_content(&self) -> Option<SelectedContent>;

    /// Upstream's `getSelection`, which is `null` when nothing is selected.
    fn selection_range(&self) -> Option<SelectedContentRange> {
        None
    }

    /// Upstream's `contentLength`.
    fn content_length(&self) -> usize {
        0
    }
}

/// Upstream `Selectable`: a [`SelectionHandler`] that is also a thing on the
/// screen.
pub trait Selectable: SelectionHandler {
    /// Upstream's `boundingBoxes`.
    ///
    /// A **list**, not one rectangle: a paragraph that wraps occupies several
    /// lines and no single box describes it without also covering the empty
    /// space beside the last line. A selection edge dropped in that space
    /// belongs to whatever is actually there.
    fn bounding_boxes(&self) -> Vec<Rect>;
}

/// Upstream `SelectionRegistrar`: what a selectable registers with.
pub trait SelectionRegistrar {
    /// Upstream's `add`.
    fn add(&mut self, id: u64);

    /// Upstream's `remove`.
    fn remove(&mut self, id: u64);
}

/// Upstream `SelectionRegistrant`: the mixin that keeps a selectable's
/// registration in step with its registrar.
///
/// Its whole job is one rule: **changing registrar means leaving the old one
/// before joining the new**, and a selectable with no registrar is registered
/// nowhere. Getting that wrong leaves a dead selectable in a registrar's list,
/// where it goes on being asked about selection edges it can no longer answer.
#[derive(Debug, Default)]
pub struct SelectionRegistrant {
    id: u64,
    registered_with: Option<u64>,
}

impl SelectionRegistrant {
    pub fn new(id: u64) -> SelectionRegistrant {
        SelectionRegistrant {
            id,
            registered_with: None,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// Which registrar this is currently in, if any.
    pub fn registered_with(&self) -> Option<u64> {
        self.registered_with
    }

    /// Upstream's `registrar` setter.
    ///
    /// Returns the registrar to leave and the one to join, in that order --
    /// the order matters, and doing it the other way round would briefly have
    /// the same selectable in two registrars.
    pub fn set_registrar(&mut self, registrar: Option<u64>) -> (Option<u64>, Option<u64>) {
        if self.registered_with == registrar {
            return (None, None);
        }
        let leaving = self.registered_with.take();
        self.registered_with = registrar;
        (leaving, registrar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paragraph occupying the middle band of a page.
    fn paragraph() -> Rect {
        Rect::ltrb(100.0, 200.0, 300.0, 260.0)
    }

    #[test]
    fn a_selectable_that_does_not_contain_the_point_says_which_way_to_look() {
        // The third answer is the whole mechanism: a drag leaving a paragraph
        // finds the next one without anybody computing a global layout.
        let target = paragraph();
        assert_eq!(
            SelectionUtils::result_based_on_rect(target, Offset::new(150.0, 230.0)),
            SelectionResult::End
        );
        assert_eq!(
            SelectionUtils::result_based_on_rect(target, Offset::new(150.0, 100.0)),
            SelectionResult::Previous
        );
        assert_eq!(
            SelectionUtils::result_based_on_rect(target, Offset::new(150.0, 400.0)),
            SelectionResult::Next
        );
    }

    #[test]
    fn vertical_position_is_asked_first_and_settles_most_cases_outright() {
        // Text runs in lines, so a point two lines up is earlier no matter how
        // far right it is. Asking horizontally first would send a drag that
        // went up and to the right forwards.
        let target = paragraph();
        assert_eq!(
            SelectionUtils::result_based_on_rect(target, Offset::new(9999.0, 100.0)),
            SelectionResult::Previous,
            "far to the right but above"
        );
        assert_eq!(
            SelectionUtils::result_based_on_rect(target, Offset::new(-9999.0, 400.0)),
            SelectionResult::Next,
            "far to the left but below"
        );
    }

    #[test]
    fn beside_the_paragraph_it_is_the_horizontal_position_that_decides() {
        let target = paragraph();
        assert_eq!(
            SelectionUtils::result_based_on_rect(target, Offset::new(50.0, 230.0)),
            SelectionResult::Previous,
            "to its left, on its lines"
        );
        assert_eq!(
            SelectionUtils::result_based_on_rect(target, Offset::new(350.0, 230.0)),
            SelectionResult::Next
        );
    }

    #[test]
    fn a_point_outside_snaps_to_the_leading_or_trailing_corner_and_nothing_else() {
        // The plane outside is cut into two areas, not four: everything above,
        // and everything to the left on its own lines, is "before"; everything
        // else is "after". That is exactly how text reads.
        let target = paragraph();
        let inside = Offset::new(150.0, 230.0);
        assert_eq!(
            SelectionUtils::adjust_drag_offset(target, inside, TextDirection::Ltr),
            inside,
            "a point inside is left alone"
        );

        for above in [Offset::new(150.0, 10.0), Offset::new(9999.0, 10.0)] {
            assert_eq!(
                SelectionUtils::adjust_drag_offset(target, above, TextDirection::Ltr),
                Offset::new(100.0, 200.0),
                "the top left"
            );
        }
        assert_eq!(
            SelectionUtils::adjust_drag_offset(
                target,
                Offset::new(50.0, 230.0),
                TextDirection::Ltr
            ),
            Offset::new(100.0, 200.0),
            "left of it, on its lines, is still before"
        );
        assert_eq!(
            SelectionUtils::adjust_drag_offset(
                target,
                Offset::new(350.0, 230.0),
                TextDirection::Ltr
            ),
            Offset::new(300.0, 260.0),
            "the bottom right"
        );
    }

    #[test]
    fn a_right_to_left_paragraph_swaps_the_two_corners() {
        // Not mirrored some other way: "before" is still the top of the
        // paragraph, and in a right-to-left paragraph the top *starts* on the
        // right.
        let target = paragraph();
        let above = Offset::new(150.0, 10.0);
        assert_eq!(
            SelectionUtils::adjust_drag_offset(target, above, TextDirection::Rtl),
            Offset::new(300.0, 200.0),
            "the top right"
        );
        let below = Offset::new(150.0, 400.0);
        assert_eq!(
            SelectionUtils::adjust_drag_offset(target, below, TextDirection::Rtl),
            Offset::new(100.0, 260.0),
            "the bottom left"
        );
    }

    #[test]
    fn a_range_remembers_which_way_the_reader_dragged() {
        // A caller that sorted the offsets would lose the direction the next
        // keystroke has to extend in.
        let forwards = SelectedContentRange::new(4, 11);
        assert!(!forwards.is_reversed());
        let backwards = SelectedContentRange::new(11, 4);
        assert!(backwards.is_reversed());
        assert_eq!(
            forwards.length(),
            backwards.length(),
            "same amount either way"
        );
        assert_ne!(forwards, backwards, "and they are not the same range");

        let caret = SelectedContentRange::new(7, 7);
        assert_eq!(caret.length(), 0);
        assert!(!caret.is_reversed());
    }

    #[test]
    fn a_geometry_with_handles_cannot_claim_to_have_no_selection() {
        // A handle is drawn at a selection point, so the two together would
        // put a handle on screen for a selection that is not there.
        let nothing = SelectionGeometry::new(SelectionStatus::None, true);
        assert!(nothing.is_consistent());
        assert!(!nothing.has_selection());

        let point =
            SelectionPoint::new(Offset::new(10.0, 20.0), 16.0, TextSelectionHandleType::Left);
        let contradictory =
            SelectionGeometry::new(SelectionStatus::None, true).with_points(Some(point), None);
        assert!(!contradictory.is_consistent());

        let sound = SelectionGeometry::new(SelectionStatus::Collapsed, true)
            .with_points(Some(point), Some(point));
        assert!(sound.is_consistent());
        assert!(sound.has_selection());
    }

    #[test]
    fn having_content_and_having_a_selection_are_different_questions() {
        // An empty paragraph has content and no selection; a container with no
        // selectables at all has neither.
        let empty_paragraph = SelectionGeometry::new(SelectionStatus::None, true);
        assert!(empty_paragraph.has_content);
        assert!(!empty_paragraph.has_selection());

        let nothing_at_all = SelectionGeometry::new(SelectionStatus::None, false);
        assert!(!nothing_at_all.has_content);
        assert!(!nothing_at_all.has_selection());

        // And the default is the emptiest of the three.
        let default = SelectionGeometry::default();
        assert_eq!(default.status, SelectionStatus::None);
        assert!(!default.has_content);
        assert!(default.selection_rects.is_empty());
    }

    #[test]
    fn every_event_names_its_own_type_and_the_two_edges_are_different_types() {
        // Which is what a receiver switches on instead of downcasting.
        assert_eq!(
            SelectionEvent::SelectAll.event_type(),
            SelectionEventType::SelectAll
        );
        assert_eq!(
            SelectionEvent::Clear.event_type(),
            SelectionEventType::Clear
        );
        assert_eq!(
            SelectionEvent::SelectWord {
                global_position: Offset::ZERO
            }
            .event_type(),
            SelectionEventType::SelectWord
        );
        assert_eq!(
            SelectionEvent::SelectParagraph {
                global_position: Offset::ZERO,
                absorb: true
            }
            .event_type(),
            SelectionEventType::SelectParagraph
        );

        let start = SelectionEvent::EdgeUpdate {
            global_position: Offset::ZERO,
            for_end: false,
            granularity: None,
        };
        let end = SelectionEvent::EdgeUpdate {
            global_position: Offset::ZERO,
            for_end: true,
            granularity: None,
        };
        assert_eq!(start.event_type(), SelectionEventType::StartEdgeUpdate);
        assert_eq!(end.event_type(), SelectionEventType::EndEdgeUpdate);
        assert_ne!(start.event_type(), end.event_type());
    }

    #[test]
    fn a_vertical_extension_carries_the_column_it_started_in() {
        // Upstream's dx. Without it a caret walking down through short lines
        // drifts left and never comes back -- every reader has met an editor
        // that does this.
        let down = SelectionEvent::DirectionallyExtend {
            dx: 240.0,
            is_end: true,
            direction: SelectionExtendDirection::NextLine,
        };
        assert_eq!(
            down.event_type(),
            SelectionEventType::DirectionallyExtendSelection
        );
        match down {
            SelectionEvent::DirectionallyExtend { dx, .. } => assert_eq!(dx, 240.0),
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_granular_extension_says_how_far_and_which_way_and_which_edge() {
        let event = SelectionEvent::GranularlyExtend {
            forward: true,
            is_end: true,
            granularity: TextGranularity::Word,
        };
        assert_eq!(
            event.event_type(),
            SelectionEventType::GranularlyExtendSelection
        );
        // All five granularities are distinct answers.
        let all = [
            TextGranularity::Character,
            TextGranularity::Word,
            TextGranularity::Line,
            TextGranularity::Paragraph,
            TextGranularity::Document,
        ];
        for (index, granularity) in all.iter().enumerate() {
            for other in &all[index + 1..] {
                assert_ne!(granularity, other);
            }
        }
    }

    #[test]
    fn changing_registrar_leaves_the_old_one_before_joining_the_new() {
        // Getting this wrong leaves a dead selectable in a registrar's list,
        // where it goes on being asked about edges it can no longer answer.
        let mut registrant = SelectionRegistrant::new(7);
        assert_eq!(registrant.registered_with(), None);

        assert_eq!(registrant.set_registrar(Some(1)), (None, Some(1)));
        assert_eq!(registrant.registered_with(), Some(1));

        assert_eq!(
            registrant.set_registrar(Some(2)),
            (Some(1), Some(2)),
            "leave one, then join two"
        );
        assert_eq!(registrant.registered_with(), Some(2));

        assert_eq!(registrant.set_registrar(None), (Some(2), None));
        assert_eq!(registrant.registered_with(), None);
    }

    #[test]
    fn setting_the_same_registrar_again_does_nothing_at_all() {
        // Not "leave and rejoin", which would drop the selectable out of the
        // registrar's ordering and put it back at the end.
        let mut registrant = SelectionRegistrant::new(7);
        registrant.set_registrar(Some(1));
        assert_eq!(registrant.set_registrar(Some(1)), (None, None));
        assert_eq!(registrant.registered_with(), Some(1));
        assert_eq!(registrant.id(), 7);
    }

    #[test]
    fn the_selected_content_is_plain_text_and_says_so() {
        // Upstream marks rich content a TODO. A selection claiming to carry
        // formatting it cannot reproduce would be worse than one that says
        // plainly what it has.
        let content = SelectedContent::new("the quick brown fox");
        assert_eq!(content.plain_text, "the quick brown fox");
        assert_eq!(content, SelectedContent::new("the quick brown fox"));
    }

    #[test]
    fn a_selection_point_carries_the_line_height_its_handle_is_drawn_against() {
        let point = SelectionPoint::new(
            Offset::new(30.0, 40.0),
            18.0,
            TextSelectionHandleType::Collapsed,
        );
        assert_eq!(point.line_height, 18.0);
        assert_eq!(point.handle_type, TextSelectionHandleType::Collapsed);
        // The collapsed handle is its own kind, not one of the two ends.
        assert_ne!(
            TextSelectionHandleType::Collapsed,
            TextSelectionHandleType::Left
        );
        assert_ne!(
            TextSelectionHandleType::Collapsed,
            TextSelectionHandleType::Right
        );
    }

    #[test]
    fn each_named_event_converts_to_the_variant_it_stands_for() {
        // The subclasses are how a sender names what it is asking for; the
        // enum is what a receiver switches on. The two have to agree.
        assert_eq!(
            SelectionEvent::from(SelectAllSelectionEvent).event_type(),
            SelectionEventType::SelectAll
        );
        assert_eq!(
            SelectionEvent::from(ClearSelectionEvent).event_type(),
            SelectionEventType::Clear
        );
        assert_eq!(
            SelectionEvent::from(SelectWordSelectionEvent {
                global_position: Offset::new(3.0, 4.0)
            }),
            SelectionEvent::SelectWord {
                global_position: Offset::new(3.0, 4.0)
            }
        );
        assert_eq!(
            SelectionEvent::from(SelectParagraphSelectionEvent {
                global_position: Offset::ZERO,
                absorb: true
            }),
            SelectionEvent::SelectParagraph {
                global_position: Offset::ZERO,
                absorb: true
            }
        );
        assert_eq!(
            SelectionEvent::from(GranularlyExtendSelectionEvent {
                forward: true,
                is_end: false,
                granularity: TextGranularity::Line
            })
            .event_type(),
            SelectionEventType::GranularlyExtendSelection
        );
        assert_eq!(
            SelectionEvent::from(DirectionallyExtendSelectionEvent {
                dx: 12.0,
                is_end: true,
                direction: SelectionExtendDirection::Backward
            })
            .event_type(),
            SelectionEventType::DirectionallyExtendSelection
        );
    }

    #[test]
    fn the_two_edge_constructors_differ_only_in_which_edge_they_move() {
        let start = SelectionEdgeUpdateEvent::for_start(Offset::new(5.0, 6.0));
        let end = SelectionEdgeUpdateEvent::for_end(Offset::new(5.0, 6.0));
        assert!(!start.for_end);
        assert!(end.for_end);
        assert_eq!(start.global_position, end.global_position);
        assert_eq!(
            SelectionEvent::from(start).event_type(),
            SelectionEventType::StartEdgeUpdate
        );
        assert_eq!(
            SelectionEvent::from(end).event_type(),
            SelectionEventType::EndEdgeUpdate
        );

        // Granularity is optional and rides along.
        let by_word =
            SelectionEdgeUpdateEvent::for_end(Offset::ZERO).with_granularity(TextGranularity::Word);
        assert_eq!(by_word.granularity, Some(TextGranularity::Word));
        assert_eq!(
            SelectionEvent::from(by_word),
            SelectionEvent::EdgeUpdate {
                global_position: Offset::ZERO,
                for_end: true,
                granularity: Some(TextGranularity::Word)
            }
        );
    }

    #[test]
    fn a_pending_answer_is_not_the_same_as_no_answer() {
        // Pending means "ask me again once I have been laid out"; None is the
        // answer to a clear or a select-all, which no selectable can be in.
        assert_ne!(SelectionResult::Pending, SelectionResult::None);
        assert_ne!(SelectionResult::Pending, SelectionResult::End);
    }
}
