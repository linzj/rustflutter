//! A port of `widgets/sliver_fill.dart`: `SliverFillViewport` and
//! `SliverFillRemaining`.
//!
//! Two slivers that size themselves from the viewport rather than from their
//! content. One gives every child a share of it; the other takes whatever the
//! slivers above it left over.

use crate::render::SliverConstraints;

/// Upstream `SliverFillViewport`.
///
/// Each child is `viewport_fraction` of the viewport along the main axis --
/// which is how a page view is built, and how a "peeking" carousel that shows
/// the edges of its neighbours is built too.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliverFillViewport {
    /// Must be greater than zero. Below one, more than one child is visible at
    /// a time; above one, each child is larger than the viewport.
    pub viewport_fraction: f32,
    /// Whether to pad both ends so the first and last child come to rest in the
    /// centre. Defaults to true.
    pub pad_ends: bool,
    pub allow_implicit_scrolling: bool,
}

impl SliverFillViewport {
    pub fn new() -> SliverFillViewport {
        SliverFillViewport {
            viewport_fraction: 1.0,
            pad_ends: true,
            allow_implicit_scrolling: true,
        }
    }

    pub fn with_fraction(mut self, fraction: f32) -> Self {
        debug_assert!(fraction > 0.0, "a child of no width is not a child");
        self.viewport_fraction = fraction;
        self
    }

    pub fn without_padded_ends(mut self) -> Self {
        self.pad_ends = false;
        self
    }

    pub fn is_valid(&self) -> bool {
        self.viewport_fraction > 0.0
    }

    /// Upstream's `_SliverFractionalPadding` fraction:
    /// `padEnds ? clampDouble(1 - viewportFraction, 0, 1) / 2 : 0`.
    ///
    /// With a fraction of 0.8 the padding is 0.1 of the viewport at each end,
    /// which is exactly what puts the first child in the middle when the list
    /// is scrolled to the start.
    ///
    /// The clamp is doing the work upstream documents in prose: **`pad_ends`
    /// has no effect when the fraction is greater than one.** There is nothing
    /// to centre when every child is already wider than the viewport, and
    /// `1 - fraction` going negative is how that falls out rather than being
    /// checked for.
    pub fn end_padding_fraction(&self) -> f32 {
        if !self.pad_ends {
            return 0.0;
        }
        (1.0 - self.viewport_fraction).clamp(0.0, 1.0) / 2.0
    }

    /// Each child's extent along the main axis.
    pub fn child_extent(&self, viewport_main_axis_extent: f32) -> f32 {
        viewport_main_axis_extent * self.viewport_fraction
    }

    /// Upstream's advice on turning the padding off: a `SliverFillViewport`
    /// that is not the only thing on its axis should not centre itself, or the
    /// padding lands in the middle of somebody else's list.
    pub fn should_pad_when_alone_on_the_axis(alone: bool) -> bool {
        alone
    }
}

impl Default for SliverFillViewport {
    fn default() -> Self {
        SliverFillViewport::new()
    }
}

/// Which of upstream's three render objects a [`SliverFillRemaining`] builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRemainingKind {
    /// The default: the child scrolls, so it is allowed to be taller than what
    /// is left and to extend past the viewport.
    WithScrollable,
    /// The child fills what is left and stops there.
    WithoutScrollable,
    /// The child also stretches into the space an overscroll opens up.
    WithoutScrollableAndFillingOverscroll,
}

/// Upstream `SliverFillRemaining`.
///
/// Two booleans, but **three** render objects: `fill_overscroll` is only
/// consulted when `has_scroll_body` is false, and upstream says so in the
/// field's own documentation. A child that scrolls has no fixed size to
/// stretch, so there is nothing for the fourth combination to mean.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliverFillRemaining {
    /// Defaults to **true**, which is the more surprising of the two: the child
    /// extends beyond the viewport and scrolls, as a nested scroll view's body
    /// does. Setting it false is what makes this a "fill the rest of the page"
    /// sliver.
    pub has_scroll_body: bool,
    /// Whether the child stretches into the space iOS's physics open up past
    /// the end. Only relevant when the body does not scroll.
    pub fill_overscroll: bool,
}

impl SliverFillRemaining {
    pub fn new() -> SliverFillRemaining {
        SliverFillRemaining {
            has_scroll_body: true,
            fill_overscroll: false,
        }
    }

    pub fn without_scroll_body(mut self) -> Self {
        self.has_scroll_body = false;
        self
    }

    pub fn filling_overscroll(mut self) -> Self {
        self.fill_overscroll = true;
        self
    }

    /// Upstream's `build`, which is a two-step choice rather than a four-way
    /// switch.
    pub fn kind(&self) -> FillRemainingKind {
        if self.has_scroll_body {
            return FillRemainingKind::WithScrollable;
        }
        if self.fill_overscroll {
            FillRemainingKind::WithoutScrollableAndFillingOverscroll
        } else {
            FillRemainingKind::WithoutScrollable
        }
    }

    /// The extent the child is given.
    ///
    /// The non-scrolling case has a deference written into upstream's
    /// documentation: if the preceding scroll extent or the child's own extent
    /// exceeds the viewport, **the sliver defers to the child's size rather
    /// than overriding it**. Filling what is left is a courtesy, not a
    /// guarantee, and squashing a child that does not fit would be worse than
    /// letting it overflow.
    pub fn child_extent(&self, constraints: &SliverConstraints, child_natural_extent: f32) -> f32 {
        if self.has_scroll_body {
            return child_natural_extent;
        }
        let left_over =
            (constraints.viewport_main_axis_extent - constraints.preceding_scroll_extent).max(0.0);
        let base = left_over.max(child_natural_extent);
        if self.fill_overscroll {
            // An overscroll shows the viewport past its own end; the child
            // stretches to cover it rather than leaving the background bare.
            base.max(constraints.remaining_paint_extent)
        } else {
            base
        }
    }
}

impl Default for SliverFillRemaining {
    fn default() -> Self {
        SliverFillRemaining::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::render::{AxisDirection, GrowthDirection};
    use crate::scrolling::ScrollDirection;

    fn constraints(preceding: f32, remaining_paint: f32) -> SliverConstraints {
        SliverConstraints {
            axis_direction: AxisDirection::Down,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: preceding,
            overlap: 0.0,
            remaining_paint_extent: remaining_paint,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::Right,
            viewport_main_axis_extent: 800.0,
            cache_origin: 0.0,
            remaining_cache_extent: 800.0,
        }
    }

    // -- SliverFillViewport ----------------------------------------------------

    #[test]
    fn a_full_width_page_view_needs_no_padding_at_the_ends() {
        let pages = SliverFillViewport::new();
        assert_eq!(pages.viewport_fraction, 1.0);
        assert_eq!(pages.end_padding_fraction(), 0.0);
        assert_eq!(pages.child_extent(800.0), 800.0);
    }

    #[test]
    fn a_peeking_carousel_is_padded_so_the_first_card_rests_in_the_middle() {
        // With a fraction of 0.8 the padding is 0.1 of the viewport at each
        // end, which is exactly half of what the card leaves over.
        let carousel = SliverFillViewport::new().with_fraction(0.8);
        assert!((carousel.end_padding_fraction() - 0.1).abs() < 1e-6);
        assert!((carousel.child_extent(800.0) - 640.0).abs() < 1e-4);
    }

    #[test]
    fn padding_the_ends_has_no_effect_on_children_wider_than_the_viewport() {
        // Which upstream states in prose and the clamp does in code: there is
        // nothing to centre when every child already overflows.
        let oversized = SliverFillViewport::new().with_fraction(1.5);
        assert_eq!(oversized.end_padding_fraction(), 0.0);
        assert!(oversized.pad_ends, "even though it was asked for");
        assert_eq!(oversized.child_extent(800.0), 1200.0);
    }

    #[test]
    fn turning_the_padding_off_is_for_a_sliver_that_is_not_alone_on_its_axis() {
        // Or the padding lands in the middle of somebody else's list.
        let shared = SliverFillViewport::new()
            .with_fraction(0.8)
            .without_padded_ends();
        assert_eq!(shared.end_padding_fraction(), 0.0);
        assert!(SliverFillViewport::should_pad_when_alone_on_the_axis(true));
        assert!(!SliverFillViewport::should_pad_when_alone_on_the_axis(
            false
        ));
    }

    #[test]
    fn a_child_of_no_width_is_not_a_child() {
        assert!(SliverFillViewport::new().is_valid());
        assert!(
            !SliverFillViewport {
                viewport_fraction: 0.0,
                ..SliverFillViewport::new()
            }
            .is_valid()
        );
    }

    // -- SliverFillRemaining -----------------------------------------------------

    #[test]
    fn two_booleans_give_three_render_objects_and_not_four() {
        // fillOverscroll is only consulted when the body does not scroll: a
        // child that scrolls has no fixed size to stretch.
        assert_eq!(
            SliverFillRemaining::new().kind(),
            FillRemainingKind::WithScrollable
        );
        assert_eq!(
            SliverFillRemaining::new().filling_overscroll().kind(),
            FillRemainingKind::WithScrollable,
            "asking for it changes nothing while the body scrolls"
        );
        assert_eq!(
            SliverFillRemaining::new().without_scroll_body().kind(),
            FillRemainingKind::WithoutScrollable
        );
        assert_eq!(
            SliverFillRemaining::new()
                .without_scroll_body()
                .filling_overscroll()
                .kind(),
            FillRemainingKind::WithoutScrollableAndFillingOverscroll
        );
    }

    #[test]
    fn the_surprising_default_is_that_the_child_scrolls_past_the_viewport() {
        // Which is what a nested scroll view's body needs; setting it false is
        // what makes this a "fill the rest of the page" sliver.
        let default = SliverFillRemaining::new();
        assert!(default.has_scroll_body);
        assert_eq!(
            default.child_extent(&constraints(0.0, 800.0), 3000.0),
            3000.0
        );
    }

    #[test]
    fn a_non_scrolling_child_is_grown_to_whatever_is_left() {
        let filling = SliverFillRemaining::new().without_scroll_body();
        assert_eq!(
            filling.child_extent(&constraints(200.0, 600.0), 100.0),
            600.0
        );
        assert_eq!(
            filling.child_extent(&constraints(0.0, 800.0), 100.0),
            800.0,
            "and to the whole viewport when nothing came before it"
        );
    }

    #[test]
    fn filling_what_is_left_is_a_courtesy_and_not_a_guarantee() {
        // Upstream's documentation says the sliver defers to the child's size
        // when the child or what came before it exceeds the viewport. Squashing
        // a child that does not fit would be worse than letting it overflow.
        let filling = SliverFillRemaining::new().without_scroll_body();
        assert_eq!(
            filling.child_extent(&constraints(200.0, 600.0), 900.0),
            900.0,
            "a child taller than what is left keeps its own size"
        );
        assert_eq!(
            filling.child_extent(&constraints(1000.0, 0.0), 300.0),
            300.0,
            "and so does one whose predecessors already filled the viewport"
        );
    }

    #[test]
    fn filling_the_overscroll_covers_the_space_the_bounce_opened_up() {
        // Rather than leaving the background bare behind a pulled-down page.
        let plain = SliverFillRemaining::new().without_scroll_body();
        let stretching = SliverFillRemaining::new()
            .without_scroll_body()
            .filling_overscroll();
        // An overscroll shows more paint extent than the viewport has room for.
        let overscrolled = constraints(0.0, 950.0);

        assert_eq!(plain.child_extent(&overscrolled, 100.0), 800.0);
        assert_eq!(stretching.child_extent(&overscrolled, 100.0), 950.0);
    }

    #[test]
    fn nothing_to_stretch_into_leaves_the_child_where_it_was() {
        let stretching = SliverFillRemaining::new()
            .without_scroll_body()
            .filling_overscroll();
        assert_eq!(
            stretching.child_extent(&constraints(0.0, 800.0), 100.0),
            800.0
        );
    }
}
