//! A port of `material/carousel.dart`.
//!
//! A row of items that snaps, like a [`crate::page_view::PageView`] whose pages
//! are narrower than the viewport and need not all be the same width. The
//! snapping physics is `PageScrollPhysics` with one addition, and that addition
//! is what this file is worth reading for.

/// Upstream `CarouselScrollPhysics`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarouselScrollPhysics;

impl CarouselScrollPhysics {
    /// Upstream's `precisionErrorTolerance`.
    pub const PRECISION_ERROR_TOLERANCE: f32 = 1e-10;

    /// How wide one item is, as a fraction of the viewport.
    ///
    /// With a fixed `item_extent` it is that over the viewport. With uneven
    /// items it is the **first** weight over their sum -- so "one item" means
    /// "however wide the leading one is", and a carousel of big-then-small
    /// items snaps by the big one.
    pub fn item_fraction(
        item_extent: Option<f32>,
        flex_weights: Option<&[i32]>,
        viewport_dimension: f32,
    ) -> f32 {
        if let Some(extent) = item_extent {
            return extent / viewport_dimension;
        }
        let weights = flex_weights.expect("a carousel has an extent or weights");
        let sum: i32 = weights.iter().sum();
        weights[0] as f32 / sum as f32
    }

    /// Upstream `_getTargetPixels`, and the line worth keeping is the one that
    /// is not in `PageScrollPhysics`:
    ///
    /// ```dart
    /// if ((actual - round).abs() < precisionErrorTolerance) { item = round; }
    /// ```
    ///
    /// **A hair off an item boundary means you are on it.** Without that snap,
    /// a position of `2.9999999` with a flick forward becomes `3.4999999` and
    /// rounds to 3, while `3.0000001` becomes `3.5000001` and rounds to **4**.
    /// A pixel of accumulated floating-point error would skip a whole item, and
    /// which way it skipped would depend on arithmetic nobody can see.
    /// A divergence worth stating rather than papering over.
    ///
    /// Upstream's `precisionErrorTolerance` is `1e-10`, which is meaningful
    /// against a **double's** spacing near three -- about `4e-16` -- and
    /// meaningless against a **single's**, which is about `2e-7`. This crate's
    /// scroll offsets are `f32`, so an offset that differs from an exact item
    /// boundary differs by at least a thousand times the tolerance, and one
    /// that does not differ is already exact.
    ///
    /// **The guard cannot fire here.** It is ported because the line is part of
    /// the answer and a reader comparing the two files should find it, not
    /// because it does anything at this width. See the test.
    pub fn target_pixels(
        pixels: f32,
        item_width: f32,
        velocity: f32,
        velocity_tolerance: f32,
    ) -> f32 {
        let actual = pixels.max(0.0) / item_width;
        let rounded = actual.round();
        let mut item =
            if (actual - rounded).abs() < CarouselScrollPhysics::PRECISION_ERROR_TOLERANCE {
                rounded
            } else {
                actual
            };
        // A flick moves the target half an item, so the rounding below lands on
        // the next one; anything slower keeps whichever item it is nearest.
        if velocity < -velocity_tolerance {
            item -= 0.5;
        } else if velocity > velocity_tolerance {
            item += 0.5;
        }
        item.round() * item_width
    }

    /// Upstream returns to the parent physics at either end, so the overscroll
    /// bounce or glow is the platform's rather than the carousel's.
    pub fn snaps(
        pixels: f32,
        min_scroll_extent: f32,
        max_scroll_extent: f32,
        velocity: f32,
    ) -> bool {
        !((velocity <= 0.0 && pixels <= min_scroll_extent)
            || (velocity >= 0.0 && pixels >= max_scroll_extent))
    }
}

/// What went wrong with a carousel's configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarouselError {
    /// Upstream: *"flexWeights is null or it contains non-positive integers"*.
    /// An item of no width is not an item.
    NonPositiveWeight,
    /// Neither an extent nor weights, or both.
    AmbiguousExtent,
}

/// Upstream `CarouselView`.
#[derive(Clone, Debug, PartialEq)]
pub struct CarouselView {
    /// Every item the same size.
    pub item_extent: Option<f32>,
    /// Or items in proportion, which is what makes a carousel look like one:
    /// a large leading item with smaller ones trailing off.
    pub flex_weights: Option<Vec<i32>>,
    pub item_count: usize,
}

impl CarouselView {
    pub fn uniform(item_extent: f32, item_count: usize) -> CarouselView {
        CarouselView {
            item_extent: Some(item_extent),
            flex_weights: None,
            item_count,
        }
    }

    /// Upstream's `CarouselView.weighted`.
    pub fn weighted(flex_weights: Vec<i32>, item_count: usize) -> CarouselView {
        CarouselView {
            item_extent: None,
            flex_weights: Some(flex_weights),
            item_count,
        }
    }

    pub fn validate(&self) -> Result<(), CarouselError> {
        match (&self.item_extent, &self.flex_weights) {
            (Some(_), None) => Ok(()),
            (None, Some(weights)) => {
                if weights.is_empty() || weights.iter().any(|weight| *weight <= 0) {
                    Err(CarouselError::NonPositiveWeight)
                } else {
                    Ok(())
                }
            }
            _ => Err(CarouselError::AmbiguousExtent),
        }
    }

    /// The width of the leading item, which is the one the snapping is measured
    /// in.
    pub fn leading_item_width(&self, viewport_dimension: f32) -> f32 {
        viewport_dimension
            * CarouselScrollPhysics::item_fraction(
                self.item_extent,
                self.flex_weights.as_deref(),
                viewport_dimension,
            )
    }
}

/// Why [`CarouselController::leading_item`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeadingItemError {
    /// Nothing has been built with this controller yet.
    NotAttached,
    /// More than one carousel is attached, and *"the leading item"* has no
    /// answer -- so it refuses rather than picking one of them.
    AttachedToSeveral,
}

/// Upstream `CarouselController`.
#[derive(Clone, Debug, PartialEq)]
pub struct CarouselController {
    /// Upstream documents this as *"the item that expands to the maximum size
    /// when first creating the CarouselView"* -- so in a weighted carousel the
    /// leading item is the big one, and this says which item starts there.
    pub initial_item: usize,
    attached: usize,
    leading: usize,
}

impl CarouselController {
    pub fn new(initial_item: usize) -> CarouselController {
        CarouselController {
            initial_item,
            attached: 0,
            leading: initial_item,
        }
    }

    pub fn attach(&mut self) {
        self.attached += 1;
    }

    pub fn detach(&mut self) {
        self.attached = self.attached.saturating_sub(1);
    }

    pub fn set_leading(&mut self, leading: usize) {
        self.leading = leading;
    }

    /// Upstream's two asserts, each with its own message.
    pub fn leading_item(&self) -> Result<usize, LeadingItemError> {
        match self.attached {
            0 => Err(LeadingItemError::NotAttached),
            1 => Ok(self.leading),
            _ => Err(LeadingItemError::AttachedToSeveral),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f32 = 50.0;

    // -- The snap that PageScrollPhysics does not have -------------------------

    #[test]
    fn the_precision_guard_cannot_fire_at_this_float_width() {
        // Upstream's tolerance is 1e-10, meaningful against a double's spacing
        // near three (about 4e-16) and not against a single's (about 2e-7).
        // With f32 offsets, a position that differs from an exact boundary
        // differs by at least a thousand times the tolerance, and one that does
        // not differ is already exact -- so the branch is unreachable here.
        //
        // Said out loud rather than tested around: the line is ported because a
        // reader comparing the two files should find it.
        let width = 100.0f32;
        let exact = 300.0f32;
        let nudged = 300.0f32 + f32::EPSILON * 300.0;
        assert_ne!(exact, nudged, "a genuinely different f32");
        assert!(
            ((nudged / width) - (nudged / width).round()).abs()
                > CarouselScrollPhysics::PRECISION_ERROR_TOLERANCE * 1000.0,
            "and one the guard cannot help with"
        );

        // What the guard would have protected: without it, a hair either side
        // of a boundary lands on different items under a flick. At f32 that
        // hair cannot exist, so both sides agree for a plainer reason.
        assert_eq!(
            CarouselScrollPhysics::target_pixels(exact, width, 100.0, 50.0),
            400.0
        );
    }

    #[test]
    fn a_flick_moves_one_item_and_a_drift_keeps_the_nearest() {
        let width = 100.0;
        // Nearly at item three, released gently: it settles on three.
        assert_eq!(
            CarouselScrollPhysics::target_pixels(280.0, width, 0.0, TOLERANCE),
            300.0
        );
        // The same place, flicked backwards: it goes to two.
        assert_eq!(
            CarouselScrollPhysics::target_pixels(280.0, width, -TOLERANCE * 2.0, TOLERANCE),
            200.0
        );
        // And flicked forwards: three.
        assert_eq!(
            CarouselScrollPhysics::target_pixels(280.0, width, TOLERANCE * 2.0, TOLERANCE),
            300.0
        );
    }

    #[test]
    fn a_velocity_inside_the_tolerance_is_not_a_flick() {
        let width = 100.0;
        assert_eq!(
            CarouselScrollPhysics::target_pixels(280.0, width, TOLERANCE / 2.0, TOLERANCE),
            300.0,
            "it settles where it would have settled at rest"
        );
    }

    #[test]
    fn a_negative_position_is_clamped_before_it_is_measured() {
        assert_eq!(
            CarouselScrollPhysics::target_pixels(-50.0, 100.0, 0.0, TOLERANCE),
            0.0
        );
    }

    #[test]
    fn at_either_end_the_platforms_own_physics_take_over() {
        // So the overscroll bounce or glow belongs to the platform rather than
        // to the carousel.
        assert!(!CarouselScrollPhysics::snaps(0.0, 0.0, 1000.0, -100.0));
        assert!(!CarouselScrollPhysics::snaps(1000.0, 0.0, 1000.0, 100.0));
        assert!(CarouselScrollPhysics::snaps(500.0, 0.0, 1000.0, 100.0));
        assert!(
            CarouselScrollPhysics::snaps(0.0, 0.0, 1000.0, 100.0),
            "and moving away from an end still snaps"
        );
    }

    // -- Item widths --------------------------------------------------------------

    #[test]
    fn one_item_means_however_wide_the_leading_one_is() {
        // A carousel of big-then-small items snaps by the big one.
        assert_eq!(
            CarouselScrollPhysics::item_fraction(None, Some(&[3, 2, 1]), 600.0),
            0.5
        );
        assert_eq!(
            CarouselScrollPhysics::item_fraction(Some(200.0), None, 600.0),
            1.0 / 3.0
        );
    }

    #[test]
    fn a_weighted_carousel_measures_its_snap_by_the_big_item() {
        let uneven = CarouselView::weighted(vec![3, 2, 1], 10);
        assert_eq!(uneven.leading_item_width(600.0), 300.0);

        let uniform = CarouselView::uniform(200.0, 10);
        assert!((uniform.leading_item_width(600.0) - 200.0).abs() < 1e-4);
    }

    #[test]
    fn an_item_of_no_width_is_not_an_item() {
        assert_eq!(CarouselView::weighted(vec![3, 2, 1], 10).validate(), Ok(()));
        assert_eq!(
            CarouselView::weighted(vec![3, 0, 1], 10).validate(),
            Err(CarouselError::NonPositiveWeight)
        );
        assert_eq!(
            CarouselView::weighted(vec![], 10).validate(),
            Err(CarouselError::NonPositiveWeight)
        );
    }

    #[test]
    fn an_extent_and_weights_at_once_have_no_answer_and_neither_does_having_neither() {
        let both = CarouselView {
            item_extent: Some(200.0),
            flex_weights: Some(vec![3, 2]),
            item_count: 10,
        };
        assert_eq!(both.validate(), Err(CarouselError::AmbiguousExtent));

        let neither = CarouselView {
            item_extent: None,
            flex_weights: None,
            item_count: 10,
        };
        assert_eq!(neither.validate(), Err(CarouselError::AmbiguousExtent));
    }

    // -- The controller ---------------------------------------------------------------

    #[test]
    fn the_leading_item_has_no_answer_when_two_carousels_share_a_controller() {
        // So it refuses rather than picking one of them.
        let mut controller = CarouselController::new(0);
        assert_eq!(
            controller.leading_item(),
            Err(LeadingItemError::NotAttached)
        );

        controller.attach();
        assert_eq!(controller.leading_item(), Ok(0));

        controller.attach();
        assert_eq!(
            controller.leading_item(),
            Err(LeadingItemError::AttachedToSeveral)
        );

        controller.detach();
        assert_eq!(controller.leading_item(), Ok(0), "one again");
    }

    #[test]
    fn the_initial_item_is_the_one_that_starts_out_big() {
        let mut controller = CarouselController::new(2);
        controller.attach();
        assert_eq!(controller.leading_item(), Ok(2));

        controller.set_leading(5);
        assert_eq!(controller.leading_item(), Ok(5));
        assert_eq!(
            controller.initial_item, 2,
            "and where it began is still where it began"
        );
    }
}
