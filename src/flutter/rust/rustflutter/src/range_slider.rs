//! Port of `material/range_slider.dart`.
//!
//! A slider with two thumbs, and the interesting part is what happens when they
//! are in the same place.

use crate::slider_theme::Thumb;

/// Which way round the track is drawn, as far as thumb selection cares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThumbTextDirection {
    #[default]
    Ltr,
    Rtl,
}

/// Why a range slider's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeSliderError {
    MinExceedsMax,
    StartExceedsEnd,
    StartOutOfRange,
    EndOutOfRange,
    NonPositiveDivisions,
}

/// Upstream `RangeSlider`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeSlider {
    pub start: f32,
    pub end: f32,
    pub min: f32,
    pub max: f32,
    pub divisions: Option<u32>,
    pub enabled: bool,
}

impl RangeSlider {
    /// Upstream `_minTouchTargetWidth`.
    pub const MIN_TOUCH_TARGET_WIDTH: f32 = 48.0;

    pub fn new(start: f32, end: f32) -> RangeSlider {
        RangeSlider {
            start,
            end,
            min: 0.0,
            max: 1.0,
            divisions: None,
            enabled: true,
        }
    }

    /// Upstream's five constructor asserts, in their order.
    pub fn validate(&self) -> Result<(), RangeSliderError> {
        if self.min > self.max {
            return Err(RangeSliderError::MinExceedsMax);
        }
        if self.start > self.end {
            return Err(RangeSliderError::StartExceedsEnd);
        }
        if self.start < self.min || self.start > self.max {
            return Err(RangeSliderError::StartOutOfRange);
        }
        if self.end < self.min || self.end > self.max {
            return Err(RangeSliderError::EndOutOfRange);
        }
        if self.divisions == Some(0) {
            return Err(RangeSliderError::NonPositiveDivisions);
        }
        Ok(())
    }

    /// Upstream `_defaultRangeThumbSelector`.
    ///
    /// `tap_value` and the thumb positions are fractions of the track, and `dx`
    /// is how far the drag has moved horizontally so far -- **zero on the
    /// initial touch**, by definition.
    ///
    /// The whole design is in the `None` this can return. Two thumbs can sit on
    /// top of each other -- a range collapsed to a point is an ordinary thing
    /// for a reader to do -- and then a touch lands inside both touch targets at
    /// once. **The position under the finger cannot say which thumb was meant,
    /// so the code does not guess. It selects nothing and waits.**
    ///
    /// What resolves it is the first non-zero displacement: **the direction you
    /// start moving is what says which thumb you were holding.** Move left and
    /// it was the start thumb, right and it was the end one. That is not a
    /// heuristic so much as the only reading that can be acted on, since a thumb
    /// you have hold of can only be dragged away from the one it is sitting on.
    ///
    /// And it is the direction *on screen*, not in the numbers: under RTL the
    /// start thumb is drawn on the right, so the two swap.
    ///
    /// When the targets do not overlap there is nothing to disambiguate and the
    /// nearer thumb wins, tested against the midpoint by doubling `tap_value`
    /// rather than halving the sum.
    pub fn default_range_thumb_selector(
        &self,
        text_direction: ThumbTextDirection,
        tap_value: f32,
        thumb_width: f32,
        track_width: f32,
        dx: f32,
    ) -> Option<Thumb> {
        let touch_radius = thumb_width.max(RangeSlider::MIN_TOUCH_TARGET_WIDTH) / 2.0;
        let in_start = (tap_value - self.start).abs() * track_width < touch_radius;
        let in_end = (tap_value - self.end).abs() * track_width < touch_radius;

        if in_start && in_end {
            let (towards_start, towards_end) = match text_direction {
                ThumbTextDirection::Ltr => (dx < 0.0, dx > 0.0),
                ThumbTextDirection::Rtl => (dx > 0.0, dx < 0.0),
            };
            if towards_start {
                return Some(Thumb::Start);
            }
            if towards_end {
                return Some(Thumb::End);
            }
            // Ambiguous, and honest about it.
            None
        } else if tap_value * 2.0 < self.start + self.end {
            Some(Thumb::Start)
        } else {
            Some(Thumb::End)
        }
    }

    /// Where the range lands when the selected thumb is dragged to `tap_value`.
    ///
    /// Upstream replaces only the selected side and asserts
    /// `newValues.start <= newValues.end` downstream, so a thumb that would
    /// cross its partner is a caller error rather than something repaired here.
    pub fn values_with(&self, thumb: Thumb, tap_value: f32) -> (f32, f32) {
        match thumb {
            Thumb::Start => (tap_value, self.end),
            Thumb::End => (self.start, tap_value),
        }
    }

    /// The touch radius a thumb of this width gets: never less than half the
    /// minimum touch target, whatever the thumb is drawn at.
    pub fn touch_radius(thumb_width: f32) -> f32 {
        thumb_width.max(RangeSlider::MIN_TOUCH_TARGET_WIDTH) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collapsed() -> RangeSlider {
        // Both thumbs at the same place, which is what makes a touch ambiguous.
        RangeSlider::new(0.5, 0.5)
    }

    fn select(slider: &RangeSlider, tap: f32, dx: f32) -> Option<Thumb> {
        slider.default_range_thumb_selector(ThumbTextDirection::Ltr, tap, 10.0, 400.0, dx)
    }

    // -- Refusing to guess ----------------------------------------------------------

    #[test]
    fn a_touch_on_two_thumbs_at_once_selects_neither() {
        // dx is zero on the initial touch, always.
        assert_eq!(select(&collapsed(), 0.5, 0.0), None);
    }

    #[test]
    fn the_direction_of_the_first_movement_is_what_decides() {
        let slider = collapsed();
        assert_eq!(select(&slider, 0.5, -1.0), Some(Thumb::Start));
        assert_eq!(select(&slider, 0.5, 1.0), Some(Thumb::End));
    }

    #[test]
    fn the_smallest_movement_in_either_direction_is_enough() {
        let slider = collapsed();
        assert_eq!(select(&slider, 0.5, -0.001), Some(Thumb::Start));
        assert_eq!(select(&slider, 0.5, 0.001), Some(Thumb::End));
        assert_eq!(select(&slider, 0.5, 0.0), None, "but nothing is not");
    }

    #[test]
    fn under_rtl_the_same_movement_grabs_the_other_thumb() {
        // The start thumb is drawn on the right, so moving right is moving
        // towards it.
        let slider = collapsed();
        let rtl =
            |dx| slider.default_range_thumb_selector(ThumbTextDirection::Rtl, 0.5, 10.0, 400.0, dx);
        assert_eq!(rtl(1.0), Some(Thumb::Start));
        assert_eq!(rtl(-1.0), Some(Thumb::End));

        assert_ne!(rtl(1.0), select(&slider, 0.5, 1.0), "the two disagree");
        assert_eq!(rtl(0.0), None, "and both are equally undecided at rest");
    }

    // -- When there is nothing to disambiguate --------------------------------------

    #[test]
    fn thumbs_far_apart_do_not_need_a_direction_at_all() {
        let slider = RangeSlider::new(0.1, 0.9);
        assert_eq!(select(&slider, 0.1, 0.0), Some(Thumb::Start));
        assert_eq!(select(&slider, 0.9, 0.0), Some(Thumb::End));
    }

    #[test]
    fn the_nearer_thumb_wins_and_the_midpoint_is_the_boundary() {
        let slider = RangeSlider::new(0.2, 0.8);
        assert_eq!(select(&slider, 0.49, 0.0), Some(Thumb::Start));
        assert_eq!(select(&slider, 0.51, 0.0), Some(Thumb::End));
        assert_eq!(
            select(&slider, 0.5, 0.0),
            Some(Thumb::End),
            "and the midpoint itself falls to the end thumb"
        );
    }

    #[test]
    fn a_touch_target_is_never_smaller_than_the_minimum_however_small_the_thumb() {
        assert_eq!(RangeSlider::touch_radius(4.0), 24.0);
        assert_eq!(RangeSlider::touch_radius(48.0), 24.0);
        assert_eq!(
            RangeSlider::touch_radius(80.0),
            40.0,
            "a thumb larger than the minimum brings its own"
        );
    }

    #[test]
    fn a_wide_thumb_makes_the_ambiguous_region_wider() {
        // The overlap that produces None is not fixed; it grows with the thumb.
        let slider = RangeSlider::new(0.45, 0.55);
        let narrow =
            slider.default_range_thumb_selector(ThumbTextDirection::Ltr, 0.5, 10.0, 2000.0, 0.0);
        let wide =
            slider.default_range_thumb_selector(ThumbTextDirection::Ltr, 0.5, 300.0, 2000.0, 0.0);

        assert_eq!(narrow, Some(Thumb::End), "0.1 of 2000px is well clear");
        assert_eq!(wide, None, "but a 300px thumb reaches across it");
    }

    // -- Moving one side ------------------------------------------------------------

    #[test]
    fn dragging_one_thumb_leaves_the_other_where_it_was() {
        let slider = RangeSlider::new(0.2, 0.8);
        assert_eq!(slider.values_with(Thumb::Start, 0.35), (0.35, 0.8));
        assert_eq!(slider.values_with(Thumb::End, 0.35), (0.2, 0.35));
    }

    #[test]
    fn crossing_is_not_repaired_here() {
        // Upstream asserts it downstream rather than clamping, so the port says
        // the same thing: this is the caller's to have prevented.
        let slider = RangeSlider::new(0.2, 0.8);
        let (start, end) = slider.values_with(Thumb::Start, 0.9);
        assert!(start > end);
    }

    // -- What the constructor refuses ------------------------------------------------

    #[test]
    fn the_range_has_to_fit_inside_the_bounds_and_face_the_right_way() {
        assert_eq!(RangeSlider::new(0.2, 0.8).validate(), Ok(()));
        assert_eq!(
            RangeSlider::new(0.8, 0.2).validate(),
            Err(RangeSliderError::StartExceedsEnd)
        );

        let mut outside = RangeSlider::new(0.2, 0.8);
        outside.max = 0.5;
        assert_eq!(outside.validate(), Err(RangeSliderError::EndOutOfRange));

        let mut inverted = RangeSlider::new(0.2, 0.8);
        inverted.min = 2.0;
        assert_eq!(inverted.validate(), Err(RangeSliderError::MinExceedsMax));
    }

    #[test]
    fn a_slider_may_be_continuous_but_not_divided_into_nothing() {
        let mut slider = RangeSlider::new(0.2, 0.8);
        assert_eq!(slider.validate(), Ok(()), "no divisions means continuous");
        slider.divisions = Some(1);
        assert_eq!(slider.validate(), Ok(()));
        slider.divisions = Some(0);
        assert_eq!(
            slider.validate(),
            Err(RangeSliderError::NonPositiveDivisions)
        );
    }

    #[test]
    fn a_range_collapsed_to_a_point_is_perfectly_legal() {
        // Which is why the ambiguity above has to be handled at all.
        assert_eq!(collapsed().validate(), Ok(()));
    }
}
