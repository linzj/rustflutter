//! Ports of `widgets/sliver_persistent_header.dart`,
//! `widgets/pinned_header_sliver.dart`, `widgets/sliver_resizing_header.dart`
//! and `widgets/sliver_floating_header.dart`.
//!
//! Four answers to "a header that stays", and the thing that separates them is
//! one distinction the sliver protocol makes and nothing else does:
//! **`layout_extent` is how much room a sliver takes from what follows;
//! `paint_extent` is how much of it you can see.** For ordinary content those
//! are the same number. Every header here is a different pair.
//!
//! A pinned header keeps its paint extent while its layout extent falls to
//! zero -- it stops taking room and keeps being seen. A floating header pushes
//! its layout extent back up as it returns, or does not, depending on whether
//! it should shove the content down or slide over it.

use crate::render::{Axis, SliverConstraints, SliverGeometry};
use crate::scrolling::ScrollDirection;

/// Upstream `SliverPersistentHeaderDelegate`.
///
/// The oldest of the four and the only one that makes the caller **predict the
/// sizes**: `minExtent` and `maxExtent` are asked for before anything is laid
/// out, and upstream's documentation insists they must not change over the
/// delegate's lifetime -- they have to come from the constructor arguments, and
/// a delegate that would answer differently must say so through
/// `shouldRebuild`. The three newer headers exist largely because that is a
/// hard promise to keep about a widget you have not measured.
pub trait SliverPersistentHeaderDelegate {
    /// `shrink_offset` runs from zero to `max_extent - min_extent`, and is
    /// always in that range.
    ///
    /// `overlaps_content` says whether anything will be drawn beneath this
    /// header -- typically the cue for a shadow. Upstream is careful to say it
    /// is *usually* true exactly when the shrink offset is at its greatest and
    /// that this is **not guaranteed**: a nested scroll view can overlap a
    /// header that has not shrunk at all.
    fn build(&self, shrink_offset: f32, overlaps_content: bool) -> u64;

    fn min_extent(&self) -> f32;

    fn max_extent(&self) -> f32;

    /// Must be non-null for a floating header with a snap or show-on-screen
    /// configuration, because both of those animate.
    fn has_vsync(&self) -> bool {
        false
    }

    fn has_snap_configuration(&self) -> bool {
        false
    }

    fn has_stretch_configuration(&self) -> bool {
        false
    }

    fn has_show_on_screen_configuration(&self) -> bool {
        false
    }

    /// Upstream's contract, quoted because it is more demanding than the name
    /// suggests: this must return true if the two delegates would give
    /// different extents, a different snap configuration, **or a meaningfully
    /// different widget tree from `build` for the same arguments**.
    fn should_rebuild(&self, old: &Self) -> bool
    where
        Self: Sized;

    /// Whether the delegate's own promise holds.
    fn extents_are_ordered(&self) -> bool {
        self.min_extent() <= self.max_extent()
    }
}

/// Which of the four render objects a [`SliverPersistentHeader`] builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistentHeaderVariant {
    /// Neither pinned nor floating: it scrolls away and comes back only when
    /// the reader reaches the part of the list it lives in.
    Scrolling,
    /// Sticks at its minimum extent once it has shrunk.
    Pinned,
    /// Grows back the moment the reader reverses, from anywhere in the list.
    Floating,
    /// Both.
    FloatingPinned,
}

/// Upstream `SliverPersistentHeader`.
///
/// The two flags are independent questions, and that is why there are four
/// render objects rather than a spectrum. **`pinned` asks what happens when
/// the reader scrolls past it; `floating` asks what happens when they turn
/// round.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliverPersistentHeader {
    pub pinned: bool,
    pub floating: bool,
}

impl SliverPersistentHeader {
    pub fn new() -> SliverPersistentHeader {
        SliverPersistentHeader {
            pinned: false,
            floating: false,
        }
    }

    pub fn with_flags(pinned: bool, floating: bool) -> SliverPersistentHeader {
        SliverPersistentHeader { pinned, floating }
    }

    pub fn variant(&self) -> PersistentHeaderVariant {
        match (self.floating, self.pinned) {
            (true, true) => PersistentHeaderVariant::FloatingPinned,
            (false, true) => PersistentHeaderVariant::Pinned,
            (true, false) => PersistentHeaderVariant::Floating,
            (false, false) => PersistentHeaderVariant::Scrolling,
        }
    }

    /// Upstream: "The delegate's snapConfiguration is ignored unless floating
    /// is true." Snapping is a thing that happens on the way back, and a header
    /// that does not come back on its own has no way back to snap along.
    pub fn honours_snap_configuration(&self) -> bool {
        self.floating
    }
}

impl Default for SliverPersistentHeader {
    fn default() -> Self {
        SliverPersistentHeader::new()
    }
}

fn cache_offset(constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
    // Upstream's `calculateCacheOffset`, reduced to the part these headers use.
    let min = constraints.scroll_offset + constraints.cache_origin;
    let max = constraints.scroll_offset + constraints.remaining_cache_extent;
    (to.clamp(min, max) - from.clamp(min, max)).max(0.0)
}

/// Upstream `PinnedHeaderSliver`.
///
/// The narrow case of [`SliverPersistentHeader`], and preferable for it: there
/// is no delegate and **no need to predict the header's size**, because it only
/// has one. It measures its child and reports that.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PinnedHeaderSliver;

impl PinnedHeaderSliver {
    pub fn new() -> PinnedHeaderSliver {
        PinnedHeaderSliver
    }

    /// The whole of upstream's `performLayout`.
    ///
    /// The pin is in two lines. `layout_extent` falls away as the header
    /// scrolls -- it stops taking room from what follows -- while `paint_extent`
    /// stays at the child's full size. And `max_scroll_obstruction_extent` is
    /// the child's whole extent, which is how the viewport is told that this
    /// much of it will never be scrollable again.
    pub fn layout(&self, constraints: &SliverConstraints, child_extent: f32) -> SliverGeometry {
        let layout_extent = (child_extent - constraints.scroll_offset)
            .clamp(0.0, constraints.remaining_paint_extent);
        let paint_extent =
            child_extent.min(constraints.remaining_paint_extent - constraints.overlap);
        SliverGeometry {
            scroll_extent: child_extent,
            // Painting at the overlap is what stacks it below whatever pinned
            // thing came before it, rather than under it.
            paint_origin: constraints.overlap,
            paint_extent,
            layout_extent,
            max_paint_extent: child_extent,
            max_scroll_obstruction_extent: child_extent,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            // Upstream: "Conservatively say we do have overflow to avoid
            // complexity." Being wrong here costs a clip layer; being wrong the
            // other way costs a header drawn outside its viewport.
            has_visual_overflow: true,
            scroll_offset_correction: None,
            cache_extent: cache_offset(constraints, 0.0, child_extent),
            cross_axis_extent: None,
        }
    }

    /// Upstream `describeSemanticsConfiguration`: once the header is covering
    /// content, its children are tagged to be excluded from scrolling. A screen
    /// reader that tried to scroll to something pinned would scroll for ever.
    pub fn excludes_children_from_scrolling(geometry: &SliverGeometry, child_extent: f32) -> bool {
        geometry.layout_extent < child_extent
    }
}

/// Upstream `SliverResizingHeader`.
///
/// The middle case: a header that shrinks between two sizes. What it does
/// differently from a delegate is that **the two sizes are widgets rather than
/// numbers** -- hand it a one-line version and a three-line version and it
/// measures them. If no maximum prototype is given it takes a dry layout of the
/// child itself, which is measuring without committing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SliverResizingHeader {
    /// The measured extent of the minimum prototype, if there is one. Absent
    /// means zero: a header that may shrink away entirely.
    pub min_extent_prototype: Option<f32>,
    /// The measured extent of the maximum prototype. Absent means "however big
    /// the child wants to be".
    pub max_extent_prototype: Option<f32>,
}

impl SliverResizingHeader {
    pub fn new() -> SliverResizingHeader {
        SliverResizingHeader::default()
    }

    pub fn with_prototypes(min: Option<f32>, max: Option<f32>) -> SliverResizingHeader {
        SliverResizingHeader {
            min_extent_prototype: min,
            max_extent_prototype: max,
        }
    }

    pub fn min_extent(&self) -> f32 {
        self.min_extent_prototype.unwrap_or(0.0)
    }

    pub fn max_extent(&self, child_dry_extent: f32) -> f32 {
        self.max_extent_prototype.unwrap_or(child_dry_extent)
    }

    /// The constraints the child is laid out with. The maximum shrinks as the
    /// reader scrolls and is floored at the minimum, so **the child resizes
    /// itself** rather than being clipped -- which is what lets a title move and
    /// a subtitle disappear instead of being cut in half.
    pub fn child_extent_limits(
        &self,
        constraints: &SliverConstraints,
        max_extent: f32,
    ) -> (f32, f32) {
        let shrink_offset = constraints.scroll_offset.min(max_extent);
        let min = self.min_extent();
        (min, min.max(max_extent - shrink_offset))
    }

    /// Upstream's `performLayout`.
    pub fn layout(
        &self,
        constraints: &SliverConstraints,
        child_dry_extent: f32,
        child_extent: f32,
    ) -> SliverGeometry {
        let max_extent = self.max_extent(child_dry_extent);
        let layout_extent = child_extent.min(max_extent - constraints.scroll_offset);
        let paint_extent = child_extent.min(constraints.remaining_paint_extent);
        SliverGeometry {
            // The full height, not the child's current one: the list scrolls
            // past all of it even though only the shrunken part stays.
            scroll_extent: max_extent,
            paint_origin: constraints.overlap,
            paint_extent,
            layout_extent: layout_extent.clamp(0.0, constraints.remaining_paint_extent),
            max_paint_extent: child_extent,
            // Only the shrunken remainder is permanently in the way.
            max_scroll_obstruction_extent: self.min_extent(),
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: true,
            scroll_offset_correction: None,
            cache_extent: cache_offset(constraints, 0.0, child_extent),
            cross_axis_extent: None,
        }
    }
}

/// Upstream `FloatingHeaderSnapMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloatingHeaderSnapMode {
    /// The header slides in over the content, which stays where it was.
    #[default]
    Overlay,
    /// The header pushes the content down as it returns.
    Scroll,
}

/// Upstream `SliverFloatingHeader`.
///
/// The header that comes back when the reader turns round, from anywhere in the
/// list. Its trick is that it does **not** lay out against
/// `constraints.scrollOffset`: it keeps an `effective_scroll_offset` of its own
/// that moves by the same delta the reader scrolled, so the header returns at
/// exactly the speed of the gesture rather than only once the top of the list
/// comes back into view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliverFloatingHeader {
    pub snap_mode: FloatingHeaderSnapMode,
    effective_scroll_offset: f32,
    last_scroll_offset: Option<f32>,
}

impl SliverFloatingHeader {
    pub fn new() -> SliverFloatingHeader {
        SliverFloatingHeader {
            snap_mode: FloatingHeaderSnapMode::Overlay,
            effective_scroll_offset: 0.0,
            last_scroll_offset: None,
        }
    }

    pub fn with_snap_mode(mut self, snap_mode: FloatingHeaderSnapMode) -> Self {
        self.snap_mode = snap_mode;
        self
    }

    pub fn effective_scroll_offset(&self) -> f32 {
        self.effective_scroll_offset
    }

    /// Upstream's `performLayout`, with `floatingHeaderNeedsToBeUpdated`
    /// spelled out as `is_floating`: on the first layout, and whenever the
    /// header is not floating, the effective offset simply follows the real
    /// one.
    pub fn layout(
        &mut self,
        constraints: &SliverConstraints,
        child_extent: f32,
        is_floating: bool,
    ) -> SliverGeometry {
        match (is_floating, self.last_scroll_offset) {
            (true, Some(last)) => {
                let mut delta = last - constraints.scroll_offset; // > 0 while growing
                if constraints.user_scroll_direction == ScrollDirection::Forward {
                    if self.effective_scroll_offset > child_extent {
                        // Coming back from far down the list: park the header
                        // just above the viewport's edge so it can slide in
                        // from there rather than from wherever it vanished.
                        self.effective_scroll_offset = child_extent;
                    }
                } else {
                    // Growing while not scrolling back is a contradiction.
                    // Upstream calls it noise and drops it.
                    delta = delta.min(0.0);
                }
                self.effective_scroll_offset =
                    (self.effective_scroll_offset - delta).clamp(0.0, constraints.scroll_offset);
            }
            _ => self.effective_scroll_offset = constraints.scroll_offset,
        }

        let paint_extent = child_extent - self.effective_scroll_offset;
        let layout_extent = match self.snap_mode {
            // Taking no more room than the scroll position allows: the header
            // arrives over the top of what is already there.
            FloatingHeaderSnapMode::Overlay => child_extent - constraints.scroll_offset,
            // Taking as much room as it shows: the content is pushed down.
            FloatingHeaderSnapMode::Scroll => paint_extent,
        };
        self.last_scroll_offset = Some(constraints.scroll_offset);

        let paint_extent = paint_extent.clamp(0.0, constraints.remaining_paint_extent);
        SliverGeometry {
            scroll_extent: child_extent,
            // Only a negative overlap is honoured: a floating header does not
            // stack under a pinned one, it comes out in front.
            paint_origin: constraints.overlap.min(0.0),
            paint_extent,
            layout_extent: layout_extent.clamp(0.0, constraints.remaining_paint_extent),
            max_paint_extent: child_extent,
            max_scroll_obstruction_extent: 0.0,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: true,
            scroll_offset_correction: None,
            cache_extent: cache_offset(constraints, 0.0, child_extent),
            cross_axis_extent: None,
        }
    }
}

impl Default for SliverFloatingHeader {
    fn default() -> Self {
        SliverFloatingHeader::new()
    }
}

/// The extent of a box along a sliver's main axis.
pub fn box_extent(axis: Axis, size: (f32, f32)) -> f32 {
    match axis {
        Axis::Vertical => size.1,
        Axis::Horizontal => size.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::render::{AxisDirection, GrowthDirection};

    fn constraints(scroll_offset: f32) -> SliverConstraints {
        SliverConstraints {
            axis_direction: AxisDirection::Down,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::Right,
            viewport_main_axis_extent: 800.0,
            cache_origin: 0.0,
            remaining_cache_extent: 800.0,
        }
    }

    /// A test delegate that says what it was built with.
    struct Header {
        min: f32,
        max: f32,
        child: u64,
    }

    impl SliverPersistentHeaderDelegate for Header {
        fn build(&self, _shrink_offset: f32, _overlaps_content: bool) -> u64 {
            self.child
        }
        fn min_extent(&self) -> f32 {
            self.min
        }
        fn max_extent(&self) -> f32 {
            self.max
        }
        fn should_rebuild(&self, old: &Self) -> bool {
            self.min != old.min || self.max != old.max || self.child != old.child
        }
    }

    // -- The two independent questions -----------------------------------------

    #[test]
    fn pinned_and_floating_are_two_questions_and_so_there_are_four_answers() {
        // pinned asks what happens when the reader scrolls past it; floating
        // asks what happens when they turn round.
        assert_eq!(
            SliverPersistentHeader::with_flags(false, false).variant(),
            PersistentHeaderVariant::Scrolling
        );
        assert_eq!(
            SliverPersistentHeader::with_flags(true, false).variant(),
            PersistentHeaderVariant::Pinned
        );
        assert_eq!(
            SliverPersistentHeader::with_flags(false, true).variant(),
            PersistentHeaderVariant::Floating
        );
        assert_eq!(
            SliverPersistentHeader::with_flags(true, true).variant(),
            PersistentHeaderVariant::FloatingPinned
        );
        assert_eq!(
            SliverPersistentHeader::new().variant(),
            PersistentHeaderVariant::Scrolling,
            "and doing neither is the default"
        );
    }

    #[test]
    fn snapping_is_ignored_unless_the_header_floats() {
        // Snapping happens on the way back, and a header that does not come
        // back on its own has no way back to snap along.
        assert!(SliverPersistentHeader::with_flags(true, true).honours_snap_configuration());
        assert!(SliverPersistentHeader::with_flags(false, true).honours_snap_configuration());
        assert!(!SliverPersistentHeader::with_flags(true, false).honours_snap_configuration());
    }

    #[test]
    fn a_delegate_must_promise_its_extents_are_ordered() {
        let ok = Header {
            min: 56.0,
            max: 200.0,
            child: 1,
        };
        assert!(ok.extents_are_ordered());
        assert!(
            !Header {
                min: 200.0,
                max: 56.0,
                child: 1,
            }
            .extents_are_ordered()
        );
    }

    #[test]
    fn a_delegate_that_would_build_differently_must_say_so() {
        // Upstream's contract is broader than the name: different extents,
        // different snap configuration, or a meaningfully different tree.
        let base = Header {
            min: 56.0,
            max: 200.0,
            child: 1,
        };
        assert!(!base.should_rebuild(&Header {
            min: 56.0,
            max: 200.0,
            child: 1
        }));
        assert!(base.should_rebuild(&Header {
            min: 56.0,
            max: 200.0,
            child: 2
        }));
        assert!(base.should_rebuild(&Header {
            min: 40.0,
            max: 200.0,
            child: 1
        }));
    }

    #[test]
    fn a_delegate_animates_nothing_unless_it_says_otherwise() {
        let header = Header {
            min: 56.0,
            max: 200.0,
            child: 1,
        };
        assert!(!header.has_vsync());
        assert!(!header.has_snap_configuration());
        assert!(!header.has_stretch_configuration());
        assert!(!header.has_show_on_screen_configuration());
    }

    // -- The pin ------------------------------------------------------------------

    #[test]
    fn a_pinned_header_stops_taking_room_and_keeps_being_seen() {
        // Which is the whole distinction the sliver protocol makes and nothing
        // else does.
        let header = PinnedHeaderSliver::new();

        let at_rest = header.layout(&constraints(0.0), 56.0);
        assert_eq!(at_rest.layout_extent, 56.0);
        assert_eq!(at_rest.paint_extent, 56.0);

        let half_gone = header.layout(&constraints(28.0), 56.0);
        assert_eq!(half_gone.layout_extent, 28.0, "taking half the room");
        assert_eq!(half_gone.paint_extent, 56.0, "and still entirely visible");

        let scrolled_far = header.layout(&constraints(1000.0), 56.0);
        assert_eq!(scrolled_far.layout_extent, 0.0, "taking none");
        assert_eq!(scrolled_far.paint_extent, 56.0, "and still entirely there");
        assert!(scrolled_far.visible);
    }

    #[test]
    fn a_pinned_header_tells_the_viewport_how_much_it_will_never_give_back() {
        let geometry = PinnedHeaderSliver::new().layout(&constraints(0.0), 56.0);
        assert_eq!(geometry.max_scroll_obstruction_extent, 56.0);
        assert_eq!(geometry.scroll_extent, 56.0);
    }

    #[test]
    fn a_pinned_header_paints_at_the_overlap_so_it_stacks_below_the_one_before() {
        let mut stacked = constraints(0.0);
        stacked.overlap = 56.0;
        let geometry = PinnedHeaderSliver::new().layout(&stacked, 48.0);
        assert_eq!(geometry.paint_origin, 56.0);
        assert_eq!(
            geometry.paint_extent, 48.0,
            "and the overlap is taken out of what it may paint into"
        );
    }

    #[test]
    fn a_pinned_header_covering_content_is_taken_out_of_the_scroll_for_a_screen_reader() {
        // Something that tried to scroll to a pinned header would scroll for
        // ever.
        let header = PinnedHeaderSliver::new();
        let at_rest = header.layout(&constraints(0.0), 56.0);
        assert!(!PinnedHeaderSliver::excludes_children_from_scrolling(
            &at_rest, 56.0
        ));

        let covering = header.layout(&constraints(20.0), 56.0);
        assert!(PinnedHeaderSliver::excludes_children_from_scrolling(
            &covering, 56.0
        ));
    }

    #[test]
    fn a_header_says_it_overflows_whether_or_not_it_does() {
        // Upstream is conservative on purpose. Being wrong this way costs a
        // clip layer; being wrong the other way draws a header outside its
        // viewport.
        assert!(
            PinnedHeaderSliver::new()
                .layout(&constraints(0.0), 56.0)
                .has_visual_overflow
        );
        assert!(
            SliverResizingHeader::new()
                .layout(&constraints(0.0), 200.0, 200.0)
                .has_visual_overflow
        );
    }

    // -- The resize ------------------------------------------------------------------

    #[test]
    fn a_resizing_header_shrinks_its_child_rather_than_clipping_it() {
        // Which is what lets a title move and a subtitle vanish instead of
        // being cut in half.
        let header = SliverResizingHeader::with_prototypes(Some(56.0), Some(200.0));
        assert_eq!(
            header.child_extent_limits(&constraints(0.0), 200.0),
            (56.0, 200.0)
        );
        assert_eq!(
            header.child_extent_limits(&constraints(80.0), 200.0),
            (56.0, 120.0)
        );
        assert_eq!(
            header.child_extent_limits(&constraints(500.0), 200.0),
            (56.0, 56.0),
            "and never below the minimum"
        );
    }

    #[test]
    fn a_resizing_header_scrolls_past_all_of_itself_but_only_keeps_the_remainder() {
        // scrollExtent is the full height and maxScrollObstructionExtent is the
        // shrunken one -- the two numbers say different things and both matter.
        let header = SliverResizingHeader::with_prototypes(Some(56.0), Some(200.0));
        let geometry = header.layout(&constraints(0.0), 200.0, 200.0);
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.max_scroll_obstruction_extent, 56.0);
    }

    #[test]
    fn an_absent_maximum_prototype_means_however_big_the_child_wants_to_be() {
        // Measured by a dry layout, which is measuring without committing.
        let measured = SliverResizingHeader::with_prototypes(Some(56.0), None);
        assert_eq!(measured.max_extent(180.0), 180.0);

        let fixed = SliverResizingHeader::with_prototypes(Some(56.0), Some(200.0));
        assert_eq!(
            fixed.max_extent(180.0),
            200.0,
            "and a prototype overrules the child"
        );
    }

    #[test]
    fn an_absent_minimum_prototype_means_it_may_shrink_away_entirely() {
        let header = SliverResizingHeader::with_prototypes(None, Some(200.0));
        assert_eq!(header.min_extent(), 0.0);
        assert_eq!(
            header.child_extent_limits(&constraints(500.0), 200.0),
            (0.0, 0.0)
        );
        assert_eq!(
            header
                .layout(&constraints(500.0), 200.0, 0.0)
                .max_scroll_obstruction_extent,
            0.0
        );
    }

    // -- The float ---------------------------------------------------------------------

    #[test]
    fn a_floating_header_returns_at_the_speed_of_the_gesture_not_of_the_list() {
        // It does not lay out against the real scroll offset. Scrolling back
        // fifty pixels brings back fifty pixels of header, however far down the
        // list the reader is.
        let mut header = SliverFloatingHeader::new();
        let mut far_down = constraints(5000.0);
        header.layout(&far_down, 120.0, true);
        assert_eq!(header.effective_scroll_offset(), 5000.0, "entirely hidden");

        // The reader turns round.
        far_down.user_scroll_direction = ScrollDirection::Forward;
        far_down.scroll_offset = 4950.0;
        let geometry = header.layout(&far_down, 120.0, true);
        assert_eq!(
            header.effective_scroll_offset(),
            70.0,
            "parked just above the edge, then moved by the fifty scrolled"
        );
        assert_eq!(geometry.paint_extent, 50.0, "fifty pixels of it are back");
    }

    #[test]
    fn a_floating_header_that_is_not_floating_just_follows_the_scroll() {
        let mut header = SliverFloatingHeader::new();
        header.layout(&constraints(40.0), 120.0, false);
        assert_eq!(header.effective_scroll_offset(), 40.0);

        let geometry = header.layout(&constraints(90.0), 120.0, false);
        assert_eq!(header.effective_scroll_offset(), 90.0);
        assert_eq!(geometry.paint_extent, 30.0);
    }

    #[test]
    fn a_header_growing_while_the_reader_is_not_scrolling_back_is_treated_as_noise() {
        // Upstream calls the combination a contradiction and drops the delta.
        let mut header = SliverFloatingHeader::new();
        let mut c = constraints(200.0);
        header.layout(&c, 120.0, true);
        assert_eq!(header.effective_scroll_offset(), 200.0);

        // A positive delta -- the header would grow -- while the direction says
        // reverse.
        c.user_scroll_direction = ScrollDirection::Reverse;
        c.scroll_offset = 150.0;
        header.layout(&c, 120.0, true);
        assert_eq!(
            header.effective_scroll_offset(),
            150.0,
            "clamped to the scroll offset rather than grown by the delta"
        );
    }

    #[test]
    fn the_header_can_never_be_more_revealed_than_the_scroll_position_allows() {
        let mut header = SliverFloatingHeader::new();
        let mut c = constraints(30.0);
        header.layout(&c, 120.0, true);

        c.user_scroll_direction = ScrollDirection::Forward;
        c.scroll_offset = 10.0;
        header.layout(&c, 120.0, true);
        assert!(header.effective_scroll_offset() <= 10.0);
    }

    #[test]
    fn overlay_slides_over_the_content_and_scroll_pushes_it_down() {
        // The same returning header, two answers to "does anything move to make
        // room?"
        let mut overlay = SliverFloatingHeader::new();
        let mut scrolling =
            SliverFloatingHeader::new().with_snap_mode(FloatingHeaderSnapMode::Scroll);
        let mut c = constraints(300.0);
        overlay.layout(&c, 120.0, true);
        scrolling.layout(&c, 120.0, true);

        c.user_scroll_direction = ScrollDirection::Forward;
        c.scroll_offset = 260.0;
        let over = overlay.layout(&c, 120.0, true);
        let push = scrolling.layout(&c, 120.0, true);

        assert_eq!(over.paint_extent, push.paint_extent, "both show the same");
        assert_eq!(over.layout_extent, 0.0, "but one takes no room");
        assert_eq!(push.layout_extent, 40.0, "and the other pushes the content");
    }

    #[test]
    fn a_floating_header_comes_out_in_front_rather_than_stacking_under() {
        let mut header = SliverFloatingHeader::new();
        let mut c = constraints(0.0);
        c.overlap = 56.0;
        let geometry = header.layout(&c, 120.0, true);
        assert_eq!(geometry.paint_origin, 0.0, "a positive overlap is ignored");

        c.overlap = -20.0;
        let mut other = SliverFloatingHeader::new();
        assert_eq!(other.layout(&c, 120.0, true).paint_origin, -20.0);
    }

    #[test]
    fn a_floating_header_never_obstructs_the_scroll_permanently() {
        // It is not pinned: it goes away again.
        let mut header = SliverFloatingHeader::new();
        assert_eq!(
            header
                .layout(&constraints(0.0), 120.0, true)
                .max_scroll_obstruction_extent,
            0.0
        );
    }

    #[test]
    fn the_main_axis_extent_is_the_one_the_scroll_runs_along() {
        assert_eq!(box_extent(Axis::Vertical, (400.0, 56.0)), 56.0);
        assert_eq!(box_extent(Axis::Horizontal, (400.0, 56.0)), 400.0);
    }
}
