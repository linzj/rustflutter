//! Two scrollables that behave as one -- a port of upstream's
//! `widgets/nested_scroll_view.dart`.
//!
//! A page with an app bar that collapses as a tabbed list scrolls is really
//! two scroll views: an outer one holding the header slivers and an inner one
//! per tab. `NestedScrollView` makes a finger's drag drive whichever of the two
//! should move, so a reader never feels the seam.
//!
//! The part that is fully portable, and the part this module is mostly about,
//! is the **overlap handshake**: how a pinned header in the outer view tells
//! the inner view how much of the top of the screen it is covering.
//!
//! * [`RenderSliverOverlapAbsorber`] wraps a pinned header and **takes its
//!   obstruction out of the outer view's own scroll extent**, recording it on a
//!   [`SliverOverlapAbsorberHandle`].
//! * [`RenderSliverOverlapInjector`] sits at the top of the inner view and
//!   **puts that much empty space back**, so the inner list's first item starts
//!   below the header rather than under it.
//!
//! The two never meet: they only share the handle. That is what lets the header
//! live in one scrollable and the space it needs appear in another.
//!
//! ## What is not here
//!
//! Upstream's `_NestedScrollCoordinator` -- the several hundred lines that
//! decide which of the two positions a drag moves, and how a fling crosses
//! between them -- is built on `ScrollActivity`, `ScrollActivityDelegate` and
//! `ScrollHoldController`, none of which this crate has. [`NestedScrollView`]
//! and [`NestedScrollViewState`] here carry the handle and the two controllers
//! and say so.

use crate::render::{SliverConstraints, SliverGeometry};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Upstream `SliverOverlapAbsorberHandle`: the one thing an absorber and an
/// injector share.
///
/// Both extents are `None` until an absorber has laid out. Upstream asserts on
/// that in the injector, with a message that is really a description of the
/// contract: the absorber must be an *earlier* descendant of a common
/// ancestor viewport, so that it is always laid out first in a given frame.
#[derive(Default)]
pub struct SliverOverlapAbsorberHandle {
    layout_extent: Cell<Option<f32>>,
    scroll_extent: Cell<Option<f32>>,
    /// Upstream's `_writers`: how many render objects have taken ownership.
    ///
    /// It exists to catch one mistake -- the same handle handed to two
    /// absorbers -- which would otherwise show up as a header that flickers
    /// between two sizes with no obvious cause.
    writers: Cell<usize>,
    listeners: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl std::fmt::Debug for SliverOverlapAbsorberHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.describe())
    }
}

impl SliverOverlapAbsorberHandle {
    pub fn new() -> SliverOverlapAbsorberHandle {
        SliverOverlapAbsorberHandle::default()
    }

    pub fn layout_extent(&self) -> Option<f32> {
        self.layout_extent.get()
    }

    pub fn scroll_extent(&self) -> Option<f32> {
        self.scroll_extent.get()
    }

    pub fn writers(&self) -> usize {
        self.writers.get()
    }

    /// Upstream's `attach` on the absorber, which increments the count.
    pub fn add_writer(&self) {
        self.writers.set(self.writers.get() + 1);
    }

    /// Upstream's `detach`.
    pub fn remove_writer(&self) {
        self.writers.set(self.writers.get().saturating_sub(1));
    }

    /// Upstream's `_setExtents`, which asserts there is exactly one writer.
    pub fn set_extents(&self, layout_extent: Option<f32>, scroll_extent: Option<f32>) {
        debug_assert_eq!(
            self.writers.get(),
            1,
            "multiple RenderSliverOverlapAbsorbers have been given the same handle"
        );
        self.layout_extent.set(layout_extent);
        self.scroll_extent.set(scroll_extent);
    }

    pub fn add_listener(&self, listener: Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(listener);
    }

    /// Upstream's `_markNeedsLayout`, which is a notification and nothing
    /// else: the handle does not lay anything out, it tells whoever is
    /// listening that they must.
    pub fn mark_needs_layout(&self) {
        let listeners = self.listeners.borrow().clone();
        for listener in listeners {
            listener();
        }
    }

    /// Upstream's `toString`, whose three cases are worth keeping because two
    /// of them are diagnoses: no owner at all, or more than one.
    pub fn describe(&self) -> String {
        let extra = match self.writers.get() {
            0 => ", orphan".to_string(),
            1 => String::new(),
            writers => format!(", {writers} WRITERS ASSIGNED"),
        };
        match self.layout_extent.get() {
            Some(extent) => format!("SliverOverlapAbsorberHandle({extent}{extra})"),
            None => format!("SliverOverlapAbsorberHandle(null{extra})"),
        }
    }
}

/// Upstream `SliverOverlapAbsorber`: the widget around a pinned header.
pub struct SliverOverlapAbsorber {
    pub handle: Rc<SliverOverlapAbsorberHandle>,
}

impl SliverOverlapAbsorber {
    pub fn new(handle: Rc<SliverOverlapAbsorberHandle>) -> SliverOverlapAbsorber {
        SliverOverlapAbsorber { handle }
    }

    /// Upstream's `createRenderObject`.
    pub fn create_render_object(&self) -> RenderSliverOverlapAbsorber {
        RenderSliverOverlapAbsorber::new(self.handle.clone())
    }
}

/// Upstream `RenderSliverOverlapAbsorber`: takes a pinned header's obstruction
/// out of its own scroll view.
pub struct RenderSliverOverlapAbsorber {
    pub handle: Rc<SliverOverlapAbsorberHandle>,
}

impl RenderSliverOverlapAbsorber {
    pub fn new(handle: Rc<SliverOverlapAbsorberHandle>) -> RenderSliverOverlapAbsorber {
        handle.add_writer();
        RenderSliverOverlapAbsorber { handle }
    }

    /// Upstream's `performLayout`, given the child's geometry.
    ///
    /// Two subtractions, and they are the point. The obstruction comes off the
    /// **scroll** extent, so the outer view no longer thinks it has that much
    /// content to scroll through; and off the **layout** extent, so the sliver
    /// after this one starts where the header stops covering rather than where
    /// it stops painting. What the header paints is untouched -- it still
    /// covers the top of the screen, it is just no longer *charged* for it
    /// here, because the inner view is about to be charged instead.
    ///
    /// `None` for the child is upstream's `child == null`: an absorber with
    /// nothing in it absorbs nothing.
    pub fn layout(&self, child_geometry: Option<SliverGeometry>) -> SliverGeometry {
        let Some(child) = child_geometry else {
            self.handle.set_extents(None, None);
            return SliverGeometry::ZERO;
        };
        let obstruction = child.max_scroll_obstruction_extent;
        self.handle
            .set_extents(Some(obstruction), Some(obstruction));
        SliverGeometry {
            scroll_extent: child.scroll_extent - obstruction,
            layout_extent: (child.paint_extent - obstruction).max(0.0),
            ..child
        }
    }
}

/// Upstream `SliverOverlapInjector`: the widget at the top of the inner view.
pub struct SliverOverlapInjector {
    pub handle: Rc<SliverOverlapAbsorberHandle>,
}

impl SliverOverlapInjector {
    pub fn new(handle: Rc<SliverOverlapAbsorberHandle>) -> SliverOverlapInjector {
        SliverOverlapInjector { handle }
    }

    /// Upstream's `createRenderObject`.
    pub fn create_render_object(&self) -> RenderSliverOverlapInjector {
        RenderSliverOverlapInjector::new(self.handle.clone())
    }
}

/// Upstream `RenderSliverOverlapInjector`: puts the absorbed space back.
pub struct RenderSliverOverlapInjector {
    pub handle: Rc<SliverOverlapAbsorberHandle>,
    current_layout_extent: Cell<Option<f32>>,
    current_max_extent: Cell<Option<f32>>,
}

impl RenderSliverOverlapInjector {
    pub fn new(handle: Rc<SliverOverlapAbsorberHandle>) -> RenderSliverOverlapInjector {
        RenderSliverOverlapInjector {
            handle,
            current_layout_extent: Cell::new(None),
            current_max_extent: Cell::new(None),
        }
    }

    /// Whether the handle has moved since the last layout -- upstream's check
    /// in `attach` and in the `handle` setter, which decides whether a
    /// relayout is needed at all.
    ///
    /// **Upstream compares `_currentMaxExtent` against the handle's
    /// `scrollExtent`, while `performLayout` sets it from the handle's
    /// `layoutExtent`.** The two fields are always the same number in practice
    /// -- the absorber writes its obstruction into both -- so the mismatch
    /// never shows, but the comparison and the assignment are reading
    /// different things. Ported as written, and named here rather than
    /// quietly corrected.
    pub fn needs_layout(&self) -> bool {
        self.handle.layout_extent() != self.current_layout_extent.get()
            || self.handle.scroll_extent() != self.current_max_extent.get()
    }

    /// Upstream's `performLayout`.
    ///
    /// The two clamps differ by the scroll offset, and that difference is the
    /// injected space scrolling away: the **paint** extent is as much of the
    /// gap as fits on screen, while the **layout** extent is as much of it as
    /// has not yet been scrolled past. So the inner list's first item begins
    /// below the header and then slides up under it, which is what a collapsing
    /// header looks like.
    pub fn layout(&self, constraints: &SliverConstraints) -> SliverGeometry {
        self.current_layout_extent.set(self.handle.layout_extent());
        // Upstream assigns `layoutExtent` here, not `scrollExtent`; see
        // `needs_layout`.
        self.current_max_extent.set(self.handle.layout_extent());
        let Some(extent) = self.current_layout_extent.get() else {
            debug_assert!(
                false,
                "SliverOverlapInjector has found no absorbed extent to inject. The \
                 SliverOverlapAbsorber must be an earlier descendant of a common ancestor \
                 Viewport, so that it is always laid out first in a given frame."
            );
            return SliverGeometry::ZERO;
        };
        let clamped_paint = extent.min(constraints.remaining_paint_extent);
        let clamped_layout =
            (extent - constraints.scroll_offset).min(constraints.remaining_paint_extent);
        SliverGeometry {
            scroll_extent: extent,
            paint_extent: clamped_paint.max(0.0),
            layout_extent: clamped_layout.max(0.0),
            max_paint_extent: self.current_max_extent.get().unwrap_or(extent),
            ..SliverGeometry::ZERO
        }
    }
}

/// Upstream `NestedScrollViewViewport`: a viewport that also carries the
/// handle.
pub struct NestedScrollViewViewport {
    pub handle: Rc<SliverOverlapAbsorberHandle>,
}

impl NestedScrollViewViewport {
    pub fn new(handle: Rc<SliverOverlapAbsorberHandle>) -> NestedScrollViewViewport {
        NestedScrollViewViewport { handle }
    }

    pub fn create_render_object(&self) -> RenderNestedScrollViewViewport {
        RenderNestedScrollViewViewport::new(self.handle.clone())
    }
}

/// Upstream `RenderNestedScrollViewViewport`: an ordinary viewport that tells
/// the handle whenever it is dirtied.
///
/// This is the piece that closes the loop. The absorber writes the handle
/// during layout, but nothing would make the *injector* -- in a different
/// scroll view entirely -- lay out again if the header merely changed size.
/// Marking the handle from the viewport's own `markNeedsLayout` is what
/// forwards that news across the seam.
pub struct RenderNestedScrollViewViewport {
    handle: Rc<SliverOverlapAbsorberHandle>,
}

impl RenderNestedScrollViewViewport {
    pub fn new(handle: Rc<SliverOverlapAbsorberHandle>) -> RenderNestedScrollViewViewport {
        RenderNestedScrollViewViewport { handle }
    }

    pub fn handle(&self) -> &Rc<SliverOverlapAbsorberHandle> {
        &self.handle
    }

    /// Upstream's `handle` setter, which tells the *new* handle rather than
    /// the old one: whoever is listening to the new handle is the one whose
    /// layout has just become wrong.
    pub fn set_handle(&mut self, handle: Rc<SliverOverlapAbsorberHandle>) {
        if Rc::ptr_eq(&self.handle, &handle) {
            return;
        }
        self.handle = handle;
        self.handle.mark_needs_layout();
    }

    /// Upstream's `markNeedsLayout`.
    pub fn mark_needs_layout(&self) {
        self.handle.mark_needs_layout();
    }
}

/// Upstream `NestedScrollView`: an outer scroll view of headers with an inner
/// one inside it.
pub struct NestedScrollView {
    /// Upstream's `floatHeaderSlivers`, which decides whether the headers may
    /// come back into view before the inner list has scrolled back to its top.
    pub float_header_slivers: bool,
    /// Upstream's `reverse`.
    pub reverse: bool,
}

impl Default for NestedScrollView {
    fn default() -> NestedScrollView {
        NestedScrollView::new()
    }
}

impl NestedScrollView {
    pub fn new() -> NestedScrollView {
        NestedScrollView {
            float_header_slivers: false,
            reverse: false,
        }
    }

    pub fn with_float_header_slivers(mut self, float: bool) -> Self {
        self.float_header_slivers = float;
        self
    }

    pub fn with_reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Upstream's `createState`.
    pub fn create_state(&self) -> NestedScrollViewState {
        NestedScrollViewState::new()
    }

    /// Upstream's static `sliverOverlapAbsorberHandleFor`, which is how a
    /// header sliver finds the handle without being handed one.
    pub fn sliver_overlap_absorber_handle_for(
        state: &NestedScrollViewState,
    ) -> Rc<SliverOverlapAbsorberHandle> {
        state.absorber_handle()
    }
}

/// Upstream `NestedScrollViewState`.
///
/// Upstream this owns the coordinator and exposes the two controllers it
/// makes. The coordinator is not ported (see the module docs); the handle,
/// which is what everything else in this file talks to, is.
pub struct NestedScrollViewState {
    absorber_handle: Rc<SliverOverlapAbsorberHandle>,
    has_scrolled_body: bool,
}

impl Default for NestedScrollViewState {
    fn default() -> NestedScrollViewState {
        NestedScrollViewState::new()
    }
}

impl NestedScrollViewState {
    pub fn new() -> NestedScrollViewState {
        NestedScrollViewState {
            absorber_handle: Rc::new(SliverOverlapAbsorberHandle::new()),
            has_scrolled_body: false,
        }
    }

    /// Upstream's `_absorberHandle`, reached through
    /// `NestedScrollView.sliverOverlapAbsorberHandleFor`.
    pub fn absorber_handle(&self) -> Rc<SliverOverlapAbsorberHandle> {
        self.absorber_handle.clone()
    }

    /// Upstream's `_handleHasScrolledBodyChanged`: whether the *inner* view has
    /// been scrolled at all.
    ///
    /// A floating header uses it to decide whether it may reappear -- the
    /// header should not float back down while the body is still at its top,
    /// because then there would be nothing for it to float over.
    pub fn has_scrolled_body(&self) -> bool {
        self.has_scrolled_body
    }

    pub fn set_has_scrolled_body(&mut self, has_scrolled_body: bool) {
        self.has_scrolled_body = has_scrolled_body;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pinned header sixty pixels tall, fully on screen.
    fn pinned_header() -> SliverGeometry {
        SliverGeometry {
            scroll_extent: 200.0,
            paint_extent: 60.0,
            layout_extent: 60.0,
            max_paint_extent: 200.0,
            max_scroll_obstruction_extent: 60.0,
            hit_test_extent: 60.0,
            visible: true,
            ..SliverGeometry::ZERO
        }
    }

    fn constraints(scroll_offset: f32, remaining: f32) -> SliverConstraints {
        SliverConstraints {
            scroll_offset,
            remaining_paint_extent: remaining,
            ..SliverConstraints::default()
        }
    }

    #[test]
    fn the_absorber_takes_the_headers_obstruction_out_of_its_own_scroll_view() {
        // Two subtractions, and they are the point: the outer view stops
        // thinking it has that much content to scroll through, and the sliver
        // after the header starts where the header stops *covering* rather
        // than where it stops painting.
        let handle = Rc::new(SliverOverlapAbsorberHandle::new());
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());
        let geometry = absorber.layout(Some(pinned_header()));

        assert_eq!(
            geometry.scroll_extent, 140.0,
            "200 less the 60 it obstructs"
        );
        assert_eq!(geometry.layout_extent, 0.0, "60 painted less 60 obstructed");
        assert_eq!(
            geometry.paint_extent, 60.0,
            "and what it paints is untouched -- it still covers the top"
        );
        assert_eq!(handle.layout_extent(), Some(60.0));
        assert_eq!(handle.scroll_extent(), Some(60.0));
    }

    #[test]
    fn an_absorber_with_nothing_in_it_absorbs_nothing() {
        let handle = Rc::new(SliverOverlapAbsorberHandle::new());
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());
        assert_eq!(absorber.layout(None), SliverGeometry::ZERO);
        assert_eq!(handle.layout_extent(), None);
    }

    #[test]
    fn a_header_that_obstructs_nothing_passes_straight_through() {
        // An ordinary scrolling sliver wrapped in an absorber is unchanged,
        // which is what makes the absorber safe to wrap a whole header list in.
        let handle = Rc::new(SliverOverlapAbsorberHandle::new());
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());
        let plain = SliverGeometry {
            scroll_extent: 200.0,
            paint_extent: 80.0,
            layout_extent: 80.0,
            max_paint_extent: 200.0,
            ..SliverGeometry::ZERO
        };
        let geometry = absorber.layout(Some(plain));
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.layout_extent, 80.0);
        assert_eq!(handle.layout_extent(), Some(0.0));
    }

    #[test]
    fn the_injector_puts_the_absorbed_space_back_at_the_top_of_the_other_view() {
        let handle = Rc::new(SliverOverlapAbsorberHandle::new());
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());
        absorber.layout(Some(pinned_header()));

        let injector = RenderSliverOverlapInjector::new(handle.clone());
        let geometry = injector.layout(&constraints(0.0, 600.0));
        assert_eq!(geometry.scroll_extent, 60.0, "exactly what was absorbed");
        assert_eq!(geometry.paint_extent, 60.0);
        assert_eq!(geometry.layout_extent, 60.0);
        assert_eq!(geometry.max_paint_extent, 60.0);
    }

    #[test]
    fn the_injected_space_scrolls_away_while_the_gap_keeps_its_size() {
        // The two clamps differ by the scroll offset, and that difference is
        // the whole collapsing-header effect: the paint extent stays put while
        // the layout extent shrinks, so the inner list slides up *under* the
        // header rather than pushing it.
        let handle = Rc::new(SliverOverlapAbsorberHandle::new());
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());
        absorber.layout(Some(pinned_header()));
        let injector = RenderSliverOverlapInjector::new(handle.clone());

        let scrolled = injector.layout(&constraints(25.0, 600.0));
        assert_eq!(scrolled.paint_extent, 60.0, "the gap is still 60 tall");
        assert_eq!(scrolled.layout_extent, 35.0, "25 of it has gone past");

        let past = injector.layout(&constraints(200.0, 600.0));
        assert_eq!(past.layout_extent, 0.0, "never negative");
        assert_eq!(past.paint_extent, 60.0);
    }

    #[test]
    fn a_viewport_too_short_for_the_gap_clamps_both_extents() {
        let handle = Rc::new(SliverOverlapAbsorberHandle::new());
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());
        absorber.layout(Some(pinned_header()));
        let injector = RenderSliverOverlapInjector::new(handle.clone());

        let cramped = injector.layout(&constraints(0.0, 20.0));
        assert_eq!(cramped.paint_extent, 20.0);
        assert_eq!(cramped.layout_extent, 20.0);
        assert_eq!(
            cramped.scroll_extent, 60.0,
            "but the scroll extent is still the whole gap"
        );
    }

    #[test]
    fn the_injector_asks_the_handle_whether_anything_moved() {
        let handle = Rc::new(SliverOverlapAbsorberHandle::new());
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());
        let injector = RenderSliverOverlapInjector::new(handle.clone());

        absorber.layout(Some(pinned_header()));
        assert!(injector.needs_layout(), "the handle has news");
        injector.layout(&constraints(0.0, 600.0));
        assert!(!injector.needs_layout(), "and now it does not");

        // A header that changed height is news again.
        let taller = SliverGeometry {
            max_scroll_obstruction_extent: 90.0,
            ..pinned_header()
        };
        absorber.layout(Some(taller));
        assert!(injector.needs_layout());
    }

    #[test]
    fn the_injectors_two_records_are_read_from_different_fields() {
        // Upstream compares `_currentMaxExtent` against the handle's
        // `scrollExtent` but assigns it from the handle's `layoutExtent`. The
        // absorber writes the same number into both, so it never shows -- but
        // a handle whose two extents differ leaves the injector permanently
        // dirty, laying out again every frame and never settling. Ported as
        // written, and this line is what makes the consequence visible rather
        // than leaving it as a remark.
        let handle = Rc::new(SliverOverlapAbsorberHandle::new());
        handle.add_writer();
        handle.set_extents(Some(60.0), Some(90.0));

        let injector = RenderSliverOverlapInjector::new(handle.clone());
        injector.layout(&constraints(0.0, 600.0));
        assert!(
            injector.needs_layout(),
            "dirty immediately after laying out"
        );

        // With the two agreeing -- which is all the absorber ever writes -- it
        // settles as it should.
        handle.set_extents(Some(60.0), Some(60.0));
        injector.layout(&constraints(0.0, 600.0));
        assert!(!injector.needs_layout());
    }

    #[test]
    fn the_handle_notices_when_it_has_been_given_to_two_absorbers() {
        // The mistake it exists to catch: the same handle handed to two
        // absorbers would otherwise show up as a header flickering between two
        // sizes with no obvious cause.
        let handle = Rc::new(SliverOverlapAbsorberHandle::new());
        assert_eq!(handle.writers(), 0);
        assert!(
            handle.describe().contains("orphan"),
            "{}",
            handle.describe()
        );

        let _first = RenderSliverOverlapAbsorber::new(handle.clone());
        assert_eq!(handle.writers(), 1);
        assert!(!handle.describe().contains("WRITERS ASSIGNED"));

        let _second = RenderSliverOverlapAbsorber::new(handle.clone());
        assert_eq!(handle.writers(), 2);
        assert!(
            handle.describe().contains("2 WRITERS ASSIGNED"),
            "{}",
            handle.describe()
        );

        handle.remove_writer();
        assert_eq!(handle.writers(), 1);
    }

    #[test]
    fn marking_the_handle_is_what_carries_the_news_across_the_seam() {
        // The absorber writes the handle during the outer view's layout, but
        // nothing would make the injector -- in a different scroll view -- lay
        // out again on its own. The viewport marking the handle is the link.
        let handle = Rc::new(SliverOverlapAbsorberHandle::new());
        let heard = Rc::new(Cell::new(0usize));
        let counter = heard.clone();
        handle.add_listener(Rc::new(move || counter.set(counter.get() + 1)));

        let viewport = RenderNestedScrollViewViewport::new(handle.clone());
        viewport.mark_needs_layout();
        assert_eq!(heard.get(), 1);
        viewport.mark_needs_layout();
        assert_eq!(heard.get(), 2);
    }

    #[test]
    fn swapping_the_handle_tells_the_new_one_and_not_the_old() {
        // Whoever listens to the new handle is the one whose layout has just
        // become wrong.
        let old = Rc::new(SliverOverlapAbsorberHandle::new());
        let new = Rc::new(SliverOverlapAbsorberHandle::new());
        let old_heard = Rc::new(Cell::new(0usize));
        let new_heard = Rc::new(Cell::new(0usize));
        let a = old_heard.clone();
        let b = new_heard.clone();
        old.add_listener(Rc::new(move || a.set(a.get() + 1)));
        new.add_listener(Rc::new(move || b.set(b.get() + 1)));

        let mut viewport = RenderNestedScrollViewViewport::new(old.clone());
        viewport.set_handle(new.clone());
        assert_eq!(old_heard.get(), 0);
        assert_eq!(new_heard.get(), 1);

        // The same handle again is not a change.
        viewport.set_handle(new.clone());
        assert_eq!(new_heard.get(), 1);
    }

    #[test]
    fn the_widgets_hand_their_render_objects_the_same_handle() {
        let state = NestedScrollView::new().create_state();
        let handle = NestedScrollView::sliver_overlap_absorber_handle_for(&state);

        let absorber = SliverOverlapAbsorber::new(handle.clone()).create_render_object();
        let injector = SliverOverlapInjector::new(handle.clone()).create_render_object();
        assert!(Rc::ptr_eq(&absorber.handle, &injector.handle));
        assert!(Rc::ptr_eq(&absorber.handle, &state.absorber_handle()));

        let viewport = NestedScrollViewViewport::new(handle.clone()).create_render_object();
        assert!(Rc::ptr_eq(viewport.handle(), &handle));

        // A whole round trip through the pair, over the handle they share.
        absorber.layout(Some(pinned_header()));
        assert_eq!(
            injector.layout(&constraints(0.0, 600.0)).scroll_extent,
            60.0
        );
    }

    #[test]
    fn a_floating_header_waits_for_the_body_to_have_been_scrolled() {
        // The header should not float back down while the body is still at its
        // top, because then there would be nothing for it to float over.
        let mut state = NestedScrollView::new()
            .with_float_header_slivers(true)
            .create_state();
        assert!(!state.has_scrolled_body());
        state.set_has_scrolled_body(true);
        assert!(state.has_scrolled_body());

        assert!(
            NestedScrollView::new()
                .with_float_header_slivers(true)
                .float_header_slivers
        );
        assert!(
            !NestedScrollView::new().float_header_slivers,
            "upstream's default"
        );
        assert!(NestedScrollView::new().with_reverse(true).reverse);
    }
}
