//! Paging -- a port of upstream's `widgets/page_view.dart`.
//!
//! A page view is a scroll view that is only ever allowed to come to rest on a
//! page boundary. Everything here follows from that one constraint.
//!
//! The interesting decision is **how a release picks its page**. It is not
//! "the nearest one": a flick that has barely moved the content would round
//! straight back to where it started, and the reader who flicked would be told
//! their gesture meant nothing. Upstream nudges the target half a page in the
//! direction of the fling **before** rounding, so any release above the
//! velocity tolerance advances exactly one page, and only a slow release falls
//! back to the nearest.
//!
//! ## What is not here
//!
//! `_PagePosition`, the scroll position it extends and the viewport that lays
//! the pages out are absent -- see [`crate::scrolling`]. What is ported is the
//! page/pixel conversion, the release rule, and the controller's arithmetic.

use crate::physics::Tolerance;
use crate::scroll_physics::ScrollPhysics;
use crate::scrolling::ScrollMetrics;

/// Upstream's `precisionErrorTolerance`.
pub const PRECISION_ERROR_TOLERANCE: f32 = 1e-10;

/// Upstream `PageMetrics`: a scroll's metrics, read in pages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageMetrics {
    pub metrics: ScrollMetrics,
    /// Upstream's `viewportFraction`, asserted positive. Less than one leaves
    /// the neighbouring pages peeking in at the edges, which is how a carousel
    /// says "there is more this way" without an arrow.
    pub viewport_fraction: f32,
}

impl PageMetrics {
    pub fn new(metrics: ScrollMetrics, viewport_fraction: f32) -> Option<PageMetrics> {
        if viewport_fraction <= 0.0 {
            return None;
        }
        Some(PageMetrics {
            metrics,
            viewport_fraction,
        })
    }

    /// Upstream's `page`.
    ///
    /// The denominator is `max(1.0, viewportDimension * viewportFraction)`,
    /// and the guard is not decoration: a page view that has not been laid out
    /// yet has a viewport of zero, and the answer would otherwise be a
    /// division by it on the first frame of every page view ever built.
    pub fn page(&self) -> f32 {
        let clamped = self.metrics.pixels.clamp(
            self.metrics.min_scroll_extent,
            self.metrics.max_scroll_extent,
        );
        clamped.max(0.0) / (self.metrics.viewport_dimension * self.viewport_fraction).max(1.0)
    }
}

/// The page/pixel conversion a `_PagePosition` performs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeometry {
    pub viewport_dimension: f32,
    pub viewport_fraction: f32,
}

impl PageGeometry {
    pub fn new(viewport_dimension: f32, viewport_fraction: f32) -> PageGeometry {
        PageGeometry {
            viewport_dimension,
            viewport_fraction,
        }
    }

    /// Upstream's `_initialPageOffset`, and the `max(0, …)` is the whole of
    /// it: **it is zero unless the fraction is above one**.
    ///
    /// A fraction below one makes pages narrower than the viewport, and they
    /// simply start at the leading edge with the next one peeking in. A
    /// fraction *above* one makes each page wider than the viewport, so the
    /// first page hangs off both sides and has to be pulled back by half the
    /// excess to stay centred.
    pub fn initial_page_offset(&self) -> f32 {
        (self.viewport_dimension * (self.viewport_fraction - 1.0) / 2.0).max(0.0)
    }

    /// Upstream's `getPixelsFromPage`.
    pub fn pixels_from_page(&self, page: f32) -> f32 {
        page * self.viewport_dimension * self.viewport_fraction + self.initial_page_offset()
    }

    /// Upstream's `getPageFromPixels`.
    ///
    /// The snap-to-integer at the end is not cosmetic. The round trip through
    /// pixels and back leaves a page at 2.9999999999999996, and a caller
    /// comparing that to 3 -- or rounding it for `nextPage` -- would be one
    /// page out. Upstream snaps whenever the error is under
    /// `precisionErrorTolerance`.
    pub fn page_from_pixels(&self, pixels: f32) -> f32 {
        debug_assert!(self.viewport_dimension > 0.0);
        let actual = (pixels - self.initial_page_offset()).max(0.0)
            / (self.viewport_dimension * self.viewport_fraction);
        let rounded = actual.round();
        if (actual - rounded).abs() < PRECISION_ERROR_TOLERANCE {
            return rounded;
        }
        actual
    }
}

/// Upstream `PageController`.
#[derive(Clone, Debug, PartialEq)]
pub struct PageController {
    pub initial_page: usize,
    /// Upstream's `keepPage`. True by default: the initial page is used the
    /// first time and the saved one thereafter, so a page view rebuilt behind
    /// a route comes back where the reader left it.
    pub keep_page: bool,
    pub viewport_fraction: f32,
    /// The geometry, once a page view has been laid out with this controller.
    geometry: Option<PageGeometry>,
    pixels: f32,
    /// Upstream's `_pageToUseOnStartup`: a page asked for before the viewport
    /// has a size, remembered until it does.
    page_to_use_on_startup: Option<f32>,
    attached_views: usize,
}

impl Default for PageController {
    fn default() -> PageController {
        PageController::new(0)
    }
}

impl PageController {
    pub fn new(initial_page: usize) -> PageController {
        PageController {
            initial_page,
            keep_page: true,
            viewport_fraction: 1.0,
            geometry: None,
            pixels: 0.0,
            page_to_use_on_startup: None,
            attached_views: 0,
        }
    }

    /// Upstream asserts `viewportFraction > 0.0`.
    pub fn with_viewport_fraction(mut self, fraction: f32) -> Option<PageController> {
        if fraction <= 0.0 {
            return None;
        }
        self.viewport_fraction = fraction;
        Some(self)
    }

    pub fn with_keep_page(mut self, keep: bool) -> Self {
        self.keep_page = keep;
        self
    }

    pub fn has_clients(&self) -> bool {
        self.attached_views > 0
    }

    pub fn pixels(&self) -> f32 {
        self.pixels
    }

    /// The scroll position reporting where it got to. Upstream's controller
    /// reads this off its attached `ScrollPosition` rather than storing it;
    /// here the position pushes it in, which is the same information arriving
    /// from the same place.
    pub fn set_pixels(&mut self, pixels: f32) {
        self.pixels = pixels;
    }

    pub fn page_to_use_on_startup(&self) -> Option<f32> {
        self.page_to_use_on_startup
    }

    /// A page view attaching, once it knows its viewport.
    pub fn attach(&mut self, viewport_dimension: f32) {
        self.attached_views += 1;
        let geometry = PageGeometry::new(viewport_dimension, self.viewport_fraction);
        let page = self
            .page_to_use_on_startup
            .take()
            .unwrap_or(self.initial_page as f32);
        self.pixels = geometry.pixels_from_page(page);
        self.geometry = Some(geometry);
    }

    pub fn detach(&mut self) {
        self.attached_views = self.attached_views.saturating_sub(1);
        if self.attached_views == 0 {
            self.geometry = None;
        }
    }

    /// Upstream's `page`, which **asserts** rather than returning null in two
    /// cases: no page view is using this controller, and *more than one* is.
    ///
    /// The second is the one worth naming. Two page views sharing a controller
    /// is not a state the controller can average its way out of -- they may be
    /// on different pages -- so upstream refuses the question instead of
    /// answering it arbitrarily. `hasClients` is what a caller checks first.
    pub fn page(&self) -> Result<f32, &'static str> {
        if self.attached_views == 0 {
            return Err("PageController.page cannot be accessed before a PageView is built");
        }
        if self.attached_views > 1 {
            return Err("multiple PageViews are using the same PageController");
        }
        let geometry = self.geometry.ok_or("no viewport yet")?;
        Ok(geometry.page_from_pixels(self.pixels))
    }

    /// Upstream's `jumpToPage`, which **defers rather than failing** when the
    /// viewport has no size yet. A caller jumping in `initState` is doing a
    /// reasonable thing, and the viewport's size is not known until layout.
    pub fn jump_to_page(&mut self, page: i32) {
        let Some(geometry) = self.geometry else {
            self.page_to_use_on_startup = Some(page as f32);
            return;
        };
        self.pixels = geometry.pixels_from_page(page as f32);
    }

    /// Upstream's `animateToPage`, deferring the same way.
    pub fn animate_to_page(&mut self, page: i32) -> Option<f32> {
        let Some(geometry) = self.geometry else {
            self.page_to_use_on_startup = Some(page as f32);
            return None;
        };
        Some(geometry.pixels_from_page(page as f32))
    }

    /// Upstream's `nextPage`, which is `animateToPage(page.round() + 1)`.
    ///
    /// The **round** is what makes it work mid-flight: a controller stopped
    /// between pages at 2.4 goes to 3, not to 3.4. Adding one to the raw
    /// fractional page would leave the view permanently off a boundary.
    pub fn next_page(&mut self) -> Option<f32> {
        let page = self.page().ok()?;
        self.animate_to_page(page.round() as i32 + 1)
    }

    pub fn previous_page(&mut self) -> Option<f32> {
        let page = self.page().ok()?;
        self.animate_to_page(page.round() as i32 - 1)
    }
}

/// Upstream `PageScrollPhysics`: comes to rest only on a page boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PageScrollPhysics {
    pub geometry: Option<PageGeometry>,
}

impl PageScrollPhysics {
    pub fn new(geometry: Option<PageGeometry>) -> PageScrollPhysics {
        PageScrollPhysics { geometry }
    }

    fn page_of(&self, metrics: &ScrollMetrics) -> f32 {
        match self.geometry {
            Some(geometry) => geometry.page_from_pixels(metrics.pixels),
            None => metrics.pixels / metrics.viewport_dimension,
        }
    }

    fn pixels_of(&self, metrics: &ScrollMetrics, page: f32) -> f32 {
        match self.geometry {
            Some(geometry) => geometry.pixels_from_page(page),
            None => page * metrics.viewport_dimension,
        }
    }

    /// Upstream's `_getTargetPixels`, and the half-page nudge is the whole
    /// design.
    ///
    /// Rounding to the nearest page would mean a flick that had barely moved
    /// the content rounds straight back to where it started -- and the reader
    /// who flicked would be told their gesture meant nothing. Adding half a
    /// page in the fling's direction **before** rounding guarantees that any
    /// release above the velocity tolerance lands one page along, however far
    /// the drag itself got. A slow release adds nothing and takes the nearest.
    ///
    /// There is an asymmetry in it, ported as upstream has it: rounding is
    /// **half away from zero**, so from a position sitting exactly on page
    /// *n*, a forward flick reaches `n + 0.5` and rounds up to `n + 1`, while
    /// a backward flick reaches `n - 0.5` and rounds back to `n`. A backward
    /// flick released at an exact boundary therefore goes nowhere. It almost
    /// never bites, because a backward drag has already moved the content off
    /// the boundary by the time it is released -- at `n - 0.001` the same
    /// flick reaches `n - 0.501` and rounds down correctly. But it is real,
    /// and the regression line below pins both halves of it.
    pub fn target_pixels(
        &self,
        metrics: &ScrollMetrics,
        tolerance: Tolerance,
        velocity: f32,
    ) -> f32 {
        let mut page = self.page_of(metrics);
        if velocity < -tolerance.velocity {
            page -= 0.5;
        } else if velocity > tolerance.velocity {
            page += 0.5;
        }
        self.pixels_of(metrics, page.round())
    }

    /// Upstream's `createBallisticSimulation`, as the decision it makes.
    ///
    /// The early return is the careful part: **already out of range and not
    /// heading back in**, and the page target is skipped entirely in favour of
    /// the parent's ballistics -- which will bring it back to a boundary
    /// anyway. Snapping to a page from out there would fight the bounce
    /// instead of letting it settle.
    pub fn should_defer_to_parent(&self, metrics: &ScrollMetrics, velocity: f32) -> bool {
        (velocity <= 0.0 && metrics.pixels <= metrics.min_scroll_extent)
            || (velocity >= 0.0 && metrics.pixels >= metrics.max_scroll_extent)
    }

    /// Whether a release produces any motion at all. Upstream returns null --
    /// no simulation -- when the target is where the scroll already is.
    pub fn settles_where_it_is(
        &self,
        metrics: &ScrollMetrics,
        tolerance: Tolerance,
        velocity: f32,
    ) -> bool {
        self.target_pixels(metrics, tolerance, velocity) == metrics.pixels
    }
}

impl ScrollPhysics for PageScrollPhysics {
    /// Upstream's one flat override. A page view must not be scrolled a
    /// partial page by a screen reader or a focus change: it would come to
    /// rest between pages, which is the one state it exists to prevent.
    fn allow_implicit_scrolling(&self) -> bool {
        false
    }
}

/// Upstream `PageView`: the widget's configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageView {
    /// Upstream's `pageSnapping`, true by default. Turning it off swaps the
    /// page physics for the ambient ones, which is how a page view becomes an
    /// ordinary list that happens to have page-sized children.
    pub page_snapping: bool,
    /// Upstream's `allowImplicitScrolling`, **false** by default, and the
    /// reason is accessibility rather than physics: with it on, a screen
    /// reader can move focus into the page next door, which is only correct if
    /// that page is genuinely part of the same reading order.
    pub allow_implicit_scrolling: bool,
    pub reverse: bool,
}

impl Default for PageView {
    fn default() -> PageView {
        PageView::new()
    }
}

impl PageView {
    pub fn new() -> PageView {
        PageView {
            page_snapping: true,
            allow_implicit_scrolling: false,
            reverse: false,
        }
    }

    pub fn with_page_snapping(mut self, snapping: bool) -> Self {
        self.page_snapping = snapping;
        self
    }

    /// Which physics the view actually uses.
    pub fn uses_page_physics(&self) -> bool {
        self.page_snapping
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(pixels: f32, max: f32, viewport: f32) -> ScrollMetrics {
        ScrollMetrics {
            pixels,
            min_scroll_extent: 0.0,
            max_scroll_extent: max,
            viewport_dimension: viewport,
        }
    }

    fn tolerance() -> Tolerance {
        Tolerance {
            distance: 0.1,
            time: 0.001,
            velocity: 50.0,
        }
    }

    // -- Pages and pixels --------------------------------------------------

    #[test]
    fn a_full_width_page_view_has_no_initial_offset() {
        let geometry = PageGeometry::new(400.0, 1.0);
        assert_eq!(geometry.initial_page_offset(), 0.0);
        assert_eq!(geometry.pixels_from_page(2.0), 800.0);
        assert_eq!(geometry.page_from_pixels(800.0), 2.0);
    }

    #[test]
    fn narrower_pages_leave_the_neighbours_peeking_in_and_still_start_at_the_edge() {
        // A fraction below one is how a carousel says "there is more this way"
        // without an arrow, and the first page still starts where the viewport
        // does.
        let geometry = PageGeometry::new(400.0, 0.8);
        assert_eq!(geometry.initial_page_offset(), 0.0);
        assert_eq!(geometry.pixels_from_page(1.0), 320.0);
    }

    #[test]
    fn pages_wider_than_the_viewport_have_to_be_pulled_back_to_stay_centred() {
        // Each page hangs off both sides, so the first one is offset by half
        // the excess. That is the whole of the max(0, ...) in the formula.
        let geometry = PageGeometry::new(400.0, 1.5);
        assert_eq!(geometry.initial_page_offset(), 100.0);
        assert_eq!(geometry.pixels_from_page(0.0), 100.0);
        assert_eq!(geometry.page_from_pixels(100.0), 0.0);
    }

    #[test]
    fn the_round_trip_snaps_back_to_a_whole_page() {
        // Without the snap the trip leaves 2.9999999999999996, and a caller
        // rounding that for nextPage would be one page out.
        let geometry = PageGeometry::new(411.42857, 1.0);
        for page in 0..8 {
            let pixels = geometry.pixels_from_page(page as f32);
            assert_eq!(
                geometry.page_from_pixels(pixels),
                page as f32,
                "page {page}"
            );
        }
    }

    #[test]
    fn a_position_genuinely_between_pages_is_not_snapped() {
        let geometry = PageGeometry::new(400.0, 1.0);
        assert_eq!(geometry.page_from_pixels(600.0), 1.5);
    }

    #[test]
    fn pixels_before_the_first_page_read_as_page_zero() {
        // The max(0, ...) in the numerator: overscrolled past the start is
        // still the first page.
        let geometry = PageGeometry::new(400.0, 1.0);
        assert_eq!(geometry.page_from_pixels(-50.0), 0.0);
    }

    // -- The metrics -------------------------------------------------------

    #[test]
    fn a_page_view_that_has_not_been_laid_out_does_not_divide_by_zero() {
        // Which is the first frame of every page view ever built.
        let unlaid = PageMetrics::new(metrics(0.0, 0.0, 0.0), 1.0).unwrap();
        assert_eq!(unlaid.page(), 0.0);
    }

    #[test]
    fn the_metrics_page_is_clamped_into_the_scrollable_range() {
        let overscrolled = PageMetrics::new(metrics(1200.0, 800.0, 400.0), 1.0).unwrap();
        assert_eq!(overscrolled.page(), 2.0, "not 3");
    }

    #[test]
    fn a_viewport_fraction_of_zero_or_less_is_refused() {
        assert!(PageMetrics::new(metrics(0.0, 0.0, 400.0), 0.0).is_none());
        assert!(PageMetrics::new(metrics(0.0, 0.0, 400.0), -1.0).is_none());
        assert!(PageController::new(0).with_viewport_fraction(0.0).is_none());
        assert!(PageController::new(0).with_viewport_fraction(0.5).is_some());
    }

    // -- Where a release lands ---------------------------------------------

    #[test]
    fn a_flick_advances_a_page_even_from_a_standstill() {
        // Rounding to the nearest would send a barely-moved flick back where
        // it started, telling the reader their gesture meant nothing.
        let physics = PageScrollPhysics::new(Some(PageGeometry::new(400.0, 1.0)));
        let at_page_one = metrics(400.0, 1600.0, 400.0);

        assert_eq!(
            physics.target_pixels(&at_page_one, tolerance(), 300.0),
            800.0,
            "forwards"
        );
        assert_eq!(
            physics.target_pixels(&at_page_one, tolerance(), -300.0),
            400.0,
            "and backwards from an exact boundary goes nowhere: 0.5 rounds              half away from zero, back up to 1"
        );

        // Which almost never bites, because a backward drag has already moved
        // the content off the boundary by the time it is released.
        let barely_dragged = metrics(399.0, 1600.0, 400.0);
        assert_eq!(
            physics.target_pixels(&barely_dragged, tolerance(), -300.0),
            0.0,
            "one pixel of drag is enough to make the rounding go the other way"
        );
    }

    #[test]
    fn a_slow_release_takes_the_nearest_page() {
        let physics = PageScrollPhysics::new(Some(PageGeometry::new(400.0, 1.0)));

        let just_past = metrics(420.0, 1600.0, 400.0);
        assert_eq!(physics.target_pixels(&just_past, tolerance(), 10.0), 400.0);

        let nearly_there = metrics(780.0, 1600.0, 400.0);
        assert_eq!(
            physics.target_pixels(&nearly_there, tolerance(), 10.0),
            800.0
        );
    }

    #[test]
    fn a_flick_backwards_from_most_of_the_way_across_still_goes_back() {
        // The half-page nudge beats the position, which is the point: the
        // reader changed their mind and the gesture says so.
        let physics = PageScrollPhysics::new(Some(PageGeometry::new(400.0, 1.0)));
        let nearly_page_two = metrics(780.0, 1600.0, 400.0);
        assert_eq!(
            physics.target_pixels(&nearly_page_two, tolerance(), -300.0),
            400.0
        );
    }

    #[test]
    fn a_release_exactly_at_the_tolerance_is_not_a_flick() {
        // Upstream compares strictly, so the boundary velocity rounds to the
        // nearest rather than advancing.
        let physics = PageScrollPhysics::new(Some(PageGeometry::new(400.0, 1.0)));
        let at_page_one = metrics(400.0, 1600.0, 400.0);
        assert_eq!(
            physics.target_pixels(&at_page_one, tolerance(), 50.0),
            400.0,
            "exactly at tolerance"
        );
        assert_eq!(
            physics.target_pixels(&at_page_one, tolerance(), 50.1),
            800.0,
            "and just past it"
        );
    }

    #[test]
    fn a_release_already_on_a_page_with_no_velocity_produces_no_motion() {
        let physics = PageScrollPhysics::new(Some(PageGeometry::new(400.0, 1.0)));
        let at_page_one = metrics(400.0, 1600.0, 400.0);
        assert!(physics.settles_where_it_is(&at_page_one, tolerance(), 0.0));
        assert!(!physics.settles_where_it_is(&at_page_one, tolerance(), 300.0));
    }

    #[test]
    fn a_bounce_past_the_end_is_left_to_the_parent_to_settle() {
        // Snapping to a page from out there would fight the bounce instead of
        // letting it settle.
        let physics = PageScrollPhysics::new(Some(PageGeometry::new(400.0, 1.0)));

        let past_end_going_out = metrics(1700.0, 1600.0, 400.0);
        assert!(physics.should_defer_to_parent(&past_end_going_out, 100.0));
        assert!(
            !physics.should_defer_to_parent(&past_end_going_out, -100.0),
            "but heading back in is the page physics' business again"
        );

        let past_start = metrics(-50.0, 1600.0, 400.0);
        assert!(physics.should_defer_to_parent(&past_start, -100.0));
        assert!(!physics.should_defer_to_parent(&past_start, 100.0));
    }

    #[test]
    fn a_page_view_is_never_scrolled_a_partial_page_by_the_framework() {
        // It would come to rest between pages, which is the one state it
        // exists to prevent.
        assert!(!PageScrollPhysics::default().allow_implicit_scrolling());
    }

    #[test]
    fn without_a_geometry_the_physics_falls_back_to_plain_viewport_pages() {
        // Upstream's branch for a position that is not a _PagePosition.
        let physics = PageScrollPhysics::new(None);
        let at = metrics(400.0, 1600.0, 400.0);
        assert_eq!(physics.target_pixels(&at, tolerance(), 300.0), 800.0);
    }

    // -- The controller ----------------------------------------------------

    #[test]
    fn a_controller_with_no_page_view_refuses_the_question() {
        let controller = PageController::new(0);
        assert!(!controller.has_clients());
        assert!(controller.page().is_err());
    }

    #[test]
    fn two_page_views_sharing_a_controller_is_refused_rather_than_averaged() {
        // They may be on different pages, and there is no answer to give.
        let mut controller = PageController::new(0);
        controller.attach(400.0);
        assert_eq!(controller.page(), Ok(0.0));

        controller.attach(400.0);
        assert!(controller.page().is_err());
    }

    #[test]
    fn a_controller_starts_at_the_page_it_was_told_to() {
        let mut controller = PageController::new(3);
        controller.attach(400.0);
        assert_eq!(controller.page(), Ok(3.0));
        assert_eq!(controller.pixels(), 1200.0);
    }

    #[test]
    fn jumping_before_the_viewport_has_a_size_is_remembered_rather_than_lost() {
        // A caller jumping in initState is doing a reasonable thing, and the
        // size is not known until layout.
        let mut controller = PageController::new(0);
        controller.jump_to_page(5);
        assert_eq!(controller.page_to_use_on_startup(), Some(5.0));

        controller.attach(400.0);
        assert_eq!(controller.page(), Ok(5.0));
        assert_eq!(
            controller.page_to_use_on_startup(),
            None,
            "and it was spent rather than kept"
        );
    }

    #[test]
    fn next_page_rounds_first_so_it_lands_on_a_boundary() {
        // A controller stopped between pages at 2.4 goes to 3, not 3.4.
        let mut controller = PageController::new(0);
        controller.attach(400.0);
        // The reader dragged most of the way to page 3 and stopped.
        controller.set_pixels(960.0);
        assert_eq!(controller.page(), Ok(2.4));

        assert_eq!(controller.next_page(), Some(1200.0), "page 3");
        assert_eq!(controller.previous_page(), Some(400.0), "page 1");
    }

    #[test]
    fn a_detached_controller_forgets_its_geometry() {
        let mut controller = PageController::new(1);
        controller.attach(400.0);
        controller.detach();
        assert!(!controller.has_clients());
        assert!(controller.page().is_err());
    }

    #[test]
    fn turning_off_snapping_is_what_makes_it_an_ordinary_list() {
        assert!(PageView::new().uses_page_physics());
        assert!(
            !PageView::new()
                .with_page_snapping(false)
                .uses_page_physics()
        );
    }

    #[test]
    fn implicit_scrolling_is_off_by_default_for_a_reason_that_is_not_physics() {
        // With it on, a screen reader can move focus into the page next door,
        // which is only right if that page is part of the same reading order.
        assert!(!PageView::new().allow_implicit_scrolling);
    }
}
