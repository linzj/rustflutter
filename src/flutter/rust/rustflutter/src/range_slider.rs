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

    /// Where the range lands when the selected thumb is dragged to
    /// `tap_value`, given how close the two thumbs are allowed to get.
    ///
    /// `separation` is a fraction of the track --
    /// [`RangeSlider::separation_fraction`] converts the theme's pixels.
    ///
    /// # What this used to say
    ///
    /// "Upstream replaces only the selected side and asserts
    /// `newValues.start <= newValues.end` downstream, so a thumb that would
    /// cross its partner is a caller error rather than something repaired
    /// here."
    ///
    /// That was wrong about upstream, and the mistake was load-bearing: it
    /// is exactly the `math.min` and `math.max` in `_handleDragUpdate` that
    /// read `SliderThemeData.minThumbSeparation`, which is why that field
    /// was named nowhere in this port outside its own paperwork. A start
    /// thumb dragged past the end one came back with `start > end`, and the
    /// assert this comment appealed to would have been the thing that fired.
    pub fn values_with(&self, thumb: Thumb, tap_value: f32, separation: f32) -> (f32, f32) {
        match thumb {
            Thumb::Start => (tap_value.min(self.end - separation), self.end),
            Thumb::End => (self.start, tap_value.max(self.start + separation)),
        }
    }

    /// How close the two thumbs may get, in pixels, under `theme`.
    ///
    /// Upstream's `sliderTheme.minThumbSeparation ?? defaults.minThumbSeparation`,
    /// where `defaults` is one of a *separate* pair of tables kept for the
    /// range slider -- `_RangeSliderDefaultsM2` and `_RangeSliderDefaultsM3`
    /// -- and the two disagree about exactly this field: eight pixels under
    /// Material 2, and zero under Material 3, where the thumbs may touch.
    pub fn min_thumb_separation(
        theme: &crate::slider_theme::SliderThemeData,
        use_material3: bool,
    ) -> f32 {
        theme
            .min_thumb_separation
            .unwrap_or(if use_material3 { 0.0 } else { 8.0 })
    }

    /// Upstream `_minThumbSeparationValue`: the theme's gap, in pixels,
    /// as a fraction of the track it is measured across.
    ///
    /// Zero on a discrete slider whatever the theme says -- the divisions
    /// already keep the thumbs a division apart, and a separation on top of
    /// them would stop a thumb reaching a position it is allowed to occupy.
    pub fn separation_fraction(&self, separation: f32, track_width: f32) -> f32 {
        if self.divisions.is_some() || track_width <= 0.0 {
            return 0.0;
        }
        separation / track_width
    }

    /// Which thumb a touch means, under `theme`.
    ///
    /// Upstream's `sliderTheme.thumbSelector ?? _defaultRangeThumbSelector`.
    pub fn select_thumb_under(
        &self,
        theme: &crate::slider_theme::SliderThemeData,
        text_direction: ThumbTextDirection,
        tap_value: f32,
        thumb_width: f32,
        track_width: f32,
        dx: f32,
    ) -> Option<Thumb> {
        self.select_thumb(
            theme.thumb_selector.as_ref(),
            text_direction,
            tap_value,
            thumb_width,
            track_width,
            dx,
        )
    }

    /// Which thumb a touch means: `selector` when there is one,
    /// [`RangeSlider::default_range_thumb_selector`] when there is not.
    ///
    /// Upstream's `sliderTheme.thumbSelector ?? _defaultRangeThumbSelector`,
    /// which is a choice nothing in this port had ever made: both sides were
    /// ported and no caller picked between them.
    pub fn select_thumb(
        &self,
        selector: Option<&crate::range_slider_parts::RangeThumbSelector>,
        text_direction: ThumbTextDirection,
        tap_value: f32,
        thumb_width: f32,
        track_width: f32,
        dx: f32,
    ) -> Option<Thumb> {
        match selector {
            None => self.default_range_thumb_selector(
                text_direction,
                tap_value,
                thumb_width,
                track_width,
                dx,
            ),
            Some(selector) => selector.select(
                match text_direction {
                    ThumbTextDirection::Ltr => crate::direction::TextDirection::Ltr,
                    ThumbTextDirection::Rtl => crate::direction::TextDirection::Rtl,
                },
                crate::range_slider_parts::RangeValues::new(self.start, self.end),
                tap_value,
                crate::render::Size::new(thumb_width, thumb_width),
                crate::render::Size::new(track_width, 0.0),
                dx,
            ),
        }
    }

    /// The touch radius a thumb of this width gets: never less than half the
    /// minimum touch target, whatever the thumb is drawn at.
    pub fn touch_radius(thumb_width: f32) -> f32 {
        thumb_width.max(RangeSlider::MIN_TOUCH_TARGET_WIDTH) / 2.0
    }
}

/// What a `RangeSlider` draws with: the theme's answers, with the defaults
/// upstream keeps in `_RangeSliderDefaultsM2` and `_RangeSliderDefaultsM3`
/// filled in.
///
/// A separate type from [`crate::slider_theme::ResolvedSlider`] because
/// upstream keeps a separate pair of tables, and the two pairs disagree: the
/// single-value slider's Material 3 track is a `GappedSliderTrackShape` and
/// the range slider's is a `GappedRangeSliderTrackShape`, a different class
/// with a gap at each end of the active part rather than one.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedRangeSlider {
    pub track_shape: crate::range_slider_parts::RangeSliderTrackShape,
    pub tick_mark_shape: crate::range_slider_parts::RangeSliderTickMarkShape,
    pub thumb_shape: crate::range_slider_parts::RangeSliderThumbShape,
    pub value_indicator_shape: crate::slider_theme::RangeSliderValueIndicatorShape,
    /// How close the thumbs may get, in pixels.
    pub min_thumb_separation: f32,
    /// The theme the *shapes* are painted against.
    ///
    /// Every one of them reads colours straight off a `SliderThemeData` and
    /// returns without drawing when they are unset, so a shape handed the raw
    /// theme draws nothing at all. The single-value slider found that the
    /// hard way two ticks ago; this one is built with the defaults in it.
    pub shape_theme: crate::slider_theme::SliderThemeData,
}

impl ResolvedRangeSlider {
    pub fn of(context: &mut crate::framework::BuildContext) -> ResolvedRangeSlider {
        let data = crate::slider_theme::SliderTheme::of(context);
        let theme = crate::theme::ThemeData::of(context);
        let colors = theme.color_scheme;
        let material3 = theme.use_material3;

        // `_RangeSliderDefaultsM2` against `_RangeSliderDefaultsM3`. Every
        // line differs, which is what makes this a table rather than a
        // field-by-field fallback.
        let track_shape = data.range_track_shape.unwrap_or(if material3 {
            crate::range_slider_parts::RangeSliderTrackShape::Gapped(
                crate::range_slider_parts::GappedRangeSliderTrackShape::new(),
            )
        } else {
            crate::range_slider_parts::RangeSliderTrackShape::RoundedRect(
                crate::range_slider_parts::RoundedRectRangeSliderTrackShape::new(),
            )
        });
        let tick_mark_shape = data.range_tick_mark_shape.unwrap_or(
            crate::range_slider_parts::RangeSliderTickMarkShape::Round(if material3 {
                // Upstream writes this as `4.0 / 2`, a diameter halved rather
                // than a radius: the mark is as wide as the gap it sits in.
                crate::range_slider_parts::RoundRangeSliderTickMarkShape::with_radius(4.0 / 2.0)
            } else {
                crate::range_slider_parts::RoundRangeSliderTickMarkShape::new()
            }),
        );
        let thumb_shape = data.range_thumb_shape.unwrap_or(if material3 {
            crate::range_slider_parts::RangeSliderThumbShape::Handle(
                crate::range_slider_parts::HandleRangeSliderThumbShape::new(),
            )
        } else {
            crate::range_slider_parts::RangeSliderThumbShape::Round(
                crate::range_slider_parts::RoundRangeSliderThumbShape::new(),
            )
        });
        let value_indicator_shape = data.range_value_indicator_shape.unwrap_or(if material3 {
            crate::slider_theme::RangeSliderValueIndicatorShape::RoundedRect(
                crate::range_slider_parts::RoundedRectRangeSliderValueIndicatorShape::new(),
            )
        } else {
            crate::slider_theme::RangeSliderValueIndicatorShape::Rectangular(
                crate::slider_theme::RectangularRangeSliderValueIndicatorShape::new(),
            )
        });

        let track_height = data
            .track_height
            .unwrap_or(if material3 { 16.0 } else { 4.0 });
        ResolvedRangeSlider {
            track_shape,
            tick_mark_shape,
            thumb_shape,
            value_indicator_shape,
            min_thumb_separation: RangeSlider::min_thumb_separation(&data, material3),
            shape_theme: crate::slider_theme::SliderThemeData {
                track_height: Some(track_height),
                active_track_color: Some(data.active_track_color.unwrap_or(colors.primary)),
                inactive_track_color: Some(
                    data.inactive_track_color
                        .unwrap_or_else(|| colors.surface_container_highest()),
                ),
                thumb_color: Some(data.thumb_color.unwrap_or(colors.primary)),
                // The tick marks again, and the same 38% over four inks: a
                // mark is a hint, and which ink it takes says whether it lies
                // between the thumbs and whether the slider is live.
                active_tick_mark_color: Some(
                    data.active_tick_mark_color
                        .unwrap_or_else(|| faded(colors.on_primary)),
                ),
                inactive_tick_mark_color: Some(
                    data.inactive_tick_mark_color
                        .unwrap_or_else(|| faded(colors.on_surface_variant())),
                ),
                disabled_active_tick_mark_color: Some(
                    data.disabled_active_tick_mark_color
                        .unwrap_or_else(|| faded(colors.on_surface)),
                ),
                disabled_inactive_tick_mark_color: Some(
                    data.disabled_inactive_tick_mark_color
                        .unwrap_or_else(|| faded(colors.on_surface)),
                ),
                range_track_shape: Some(track_shape),
                range_tick_mark_shape: Some(tick_mark_shape),
                range_thumb_shape: Some(thumb_shape),
                range_value_indicator_shape: Some(value_indicator_shape),
                ..data.clone()
            },
        }
    }
}

/// The 38% every tick mark colour wears; see the single-value slider's copy.
fn faded(color: crate::engine::Color) -> crate::engine::Color {
    color.with_alpha((color.alpha() as f32 * 0.38).round() as u8)
}

/// Draws a range slider: the track, then the tick marks, then the two thumbs.
///
/// Upstream's `_RenderRangeSlider.paint` in that order, and the order is the
/// content: a mark under a thumb is covered rather than drawn over it.
///
/// Each of the three shapes was ported with its painter and its own tests and
/// none of them had ever been asked to paint, because this port had no range
/// slider widget at all -- only the logic type above.
struct RangeSliderPainter {
    slider: RangeSlider,
    resolved: ResolvedRangeSlider,
    direction: crate::direction::TextDirection,
}

impl RangeSliderPainter {
    /// Where a value sits along the track, as a fraction, in *drawing*
    /// coordinates: under RTL the start thumb is the right-hand one, which is
    /// what `RangeTrackPaintGeometry` means by its two centres not being in
    /// value order.
    fn fraction(&self, value: f32) -> f32 {
        if self.slider.max <= self.slider.min {
            return 0.0;
        }
        let fraction =
            ((value - self.slider.min) / (self.slider.max - self.slider.min)).clamp(0.0, 1.0);
        match self.direction {
            crate::direction::TextDirection::Ltr => fraction,
            crate::direction::TextDirection::Rtl => 1.0 - fraction,
        }
    }
}

impl crate::render::CustomPainter for RangeSliderPainter {
    fn paint(&self, canvas: &mut crate::engine::Canvas, size: crate::render::Size) {
        let theme = &self.resolved.shape_theme;
        let track = self.resolved.track_shape.preferred_rect(
            size,
            crate::render::Offset::ZERO,
            theme,
            self.slider.enabled,
        );
        let middle = track.top + track.height() / 2.0;
        let at = |fraction: f32| {
            crate::render::Offset::new(track.left + fraction * track.width(), middle)
        };
        let start = at(self.fraction(self.slider.start));
        let end = at(self.fraction(self.slider.end));
        // Upstream's enable animation, at rest: one when the slider takes
        // input and zero when it does not.
        let enable = if self.slider.enabled { 1.0 } else { 0.0 };
        let geometry = crate::range_slider_parts::RangeTrackPaintGeometry::new(
            track,
            start,
            end,
            self.direction,
            enable,
        );
        let is_discrete = self.slider.divisions.is_some();
        self.resolved
            .track_shape
            .paint(canvas, &geometry, theme, is_discrete);

        if let Some(divisions) = self.slider.divisions {
            for step in 0..=divisions {
                let fraction = step as f32 / divisions as f32;
                self.resolved.tick_mark_shape.paint(
                    canvas,
                    at(match self.direction {
                        crate::direction::TextDirection::Ltr => fraction,
                        crate::direction::TextDirection::Rtl => 1.0 - fraction,
                    }),
                    theme,
                    start,
                    end,
                    self.direction,
                    enable,
                );
            }
        }

        // The thumbs last, and the *end* thumb on top: upstream paints the
        // start thumb first, so when the two are in the same place the end
        // one is the visible one. `is_on_top` is what tells the shape they
        // are in the same place at all -- it draws a ring then, so a
        // collapsed range does not look like a single thumb.
        let together = (start.dx - end.dx).abs() < f32::EPSILON;
        for (center, is_on_top) in [(start, false), (end, together)] {
            self.resolved.thumb_shape.paint(
                canvas, center, theme,
                // Upstream's activation and press animations, both at rest:
                // this port has no controller for either yet.
                0.0, enable, is_on_top, false,
            );
        }
    }

    fn should_repaint(&self, _old: &dyn crate::render::CustomPainter) -> bool {
        true
    }

    fn kind_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<RangeSliderPainter>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::framework::Component for RangeSlider {
    fn build(&self, context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        let painter = std::rc::Rc::new(RangeSliderPainter {
            slider: *self,
            resolved: ResolvedRangeSlider::of(context),
            direction: crate::direction::direction_of(context),
        }) as std::rc::Rc<dyn crate::render::CustomPainter>;
        // The box is as tall as the minimum touch target, not as tall as the
        // track: the thumbs stand proud of it and the shapes draw from the
        // track's centre line, which `preferred_rect` puts in the middle of
        // whatever box it is given.
        let height = RangeSlider::MIN_TOUCH_TARGET_WIDTH;
        crate::framework::leaf(move || {
            crate::render::RenderCustomPaint::new(crate::widgets::SizedBox::new(200.0, height))
                .with_foreground_painter(std::rc::Rc::clone(&painter))
        })
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
        assert_eq!(slider.values_with(Thumb::Start, 0.35, 0.0), (0.35, 0.8));
        assert_eq!(slider.values_with(Thumb::End, 0.35, 0.0), (0.2, 0.35));
    }

    #[test]
    fn a_thumb_dragged_past_its_partner_stops_at_it() {
        // This test used to be called `crossing_is_not_repaired_here` and
        // asserted `start > end`, on the strength of a comment claiming
        // upstream only asserts the ordering downstream. Upstream repairs it,
        // in `_handleDragUpdate`, with the `math.min` and `math.max` that are
        // the *only* readers of `SliderThemeData.minThumbSeparation` -- which
        // is how that field came to be named nowhere in this port outside its
        // own paperwork.
        let slider = RangeSlider::new(0.2, 0.8);

        let (start, end) = slider.values_with(Thumb::Start, 0.9, 0.0);
        assert_eq!((start, end), (0.8, 0.8), "and not (0.9, 0.8)");
        assert!(start <= end, "which is the ordering the old test broke");

        let (start, end) = slider.values_with(Thumb::End, 0.1, 0.0);
        assert_eq!((start, end), (0.2, 0.2));
    }

    #[test]
    fn a_separation_stops_the_thumbs_short_of_each_other() {
        // The gap the theme asks for, in fractions of the track. Zero above
        // is what the old behaviour would have been if it had been right
        // about crossing; this is the part `minThumbSeparation` adds.
        let slider = RangeSlider::new(0.2, 0.8);
        assert_eq!(slider.values_with(Thumb::Start, 0.9, 0.05), (0.75, 0.8));
        assert_eq!(slider.values_with(Thumb::End, 0.1, 0.05), (0.2, 0.25));

        // A drag that does not reach its partner is untouched by it.
        assert_eq!(slider.values_with(Thumb::Start, 0.3, 0.05), (0.3, 0.8));
    }

    #[test]
    fn the_separation_is_pixels_over_the_track_and_nothing_on_a_discrete_one() {
        // Upstream's `_minThumbSeparationValue`. The theme's field is in
        // pixels and the values are fractions, so the track width is in it --
        // the same eight pixels is a fifth of a forty-pixel track and a
        // fortieth of an eight-hundred-pixel one.
        let smooth = RangeSlider::new(0.2, 0.8);
        assert_eq!(smooth.separation_fraction(8.0, 800.0), 0.01);
        assert_eq!(smooth.separation_fraction(8.0, 40.0), 0.2);

        // And zero on a discrete slider whatever the theme says: the
        // divisions already hold the thumbs apart, and a gap on top of them
        // would stop a thumb reaching a position it is allowed to occupy.
        let mut discrete = smooth;
        discrete.divisions = Some(4);
        assert_eq!(discrete.separation_fraction(8.0, 800.0), 0.0);

        // A track with no width would divide by zero.
        assert_eq!(smooth.separation_fraction(8.0, 0.0), 0.0);
    }

    // -- Which thumb a touch means ---------------------------------------------------

    #[test]
    fn a_theme_may_replace_the_rule_for_choosing_a_thumb() {
        // Upstream's `sliderTheme.thumbSelector ?? _defaultRangeThumbSelector`
        // -- a choice nothing in this port had ever made. Both sides were
        // ported; no caller picked between them, which is why
        // `SliderThemeData::thumb_selector` reached nothing.
        let slider = collapsed();

        // Unset, the default answers, and its answer at rest is to refuse.
        assert_eq!(
            slider.select_thumb(None, ThumbTextDirection::Ltr, 0.5, 10.0, 400.0, 0.0),
            None
        );

        // An application that would rather always move the end thumb says so,
        // and the default is not consulted.
        let always_end = crate::range_slider_parts::RangeThumbSelector::new(
            |_direction, _values, _tap, _thumb, _track, _dx| Some(Thumb::End),
        );
        assert_eq!(
            slider.select_thumb(
                Some(&always_end),
                ThumbTextDirection::Ltr,
                0.5,
                10.0,
                400.0,
                0.0
            ),
            Some(Thumb::End),
            "the theme's, not the default's"
        );

        // And it is asked about the slider it was given, not a fixed one: the
        // values reach it.
        let seen = std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0)));
        let recorder = std::rc::Rc::clone(&seen);
        let watcher = crate::range_slider_parts::RangeThumbSelector::new(
            move |_direction, values, _tap, _thumb, _track, _dx| {
                recorder.set((values.start, values.end));
                None
            },
        );
        let wide = RangeSlider::new(0.25, 0.75);
        wide.select_thumb(
            Some(&watcher),
            ThumbTextDirection::Ltr,
            0.5,
            10.0,
            400.0,
            0.0,
        );
        assert_eq!(seen.get(), (0.25, 0.75));
    }

    #[test]
    fn material_three_lets_the_thumbs_touch_and_material_two_does_not() {
        // Upstream keeps a separate pair of defaults tables for the range
        // slider, and this is the field they disagree about:
        // `_RangeSliderDefaultsM2` says eight pixels, `_RangeSliderDefaultsM3`
        // says zero.
        use crate::slider_theme::SliderThemeData;
        let unset = SliderThemeData::new();
        assert_eq!(RangeSlider::min_thumb_separation(&unset, true), 0.0);
        assert_eq!(RangeSlider::min_thumb_separation(&unset, false), 8.0);

        // A theme that says so beats both tables.
        let asked = SliderThemeData {
            min_thumb_separation: Some(20.0),
            ..SliderThemeData::new()
        };
        assert_eq!(RangeSlider::min_thumb_separation(&asked, true), 20.0);
        assert_eq!(RangeSlider::min_thumb_separation(&asked, false), 20.0);
    }

    #[test]
    fn the_selector_a_theme_carries_is_the_one_that_is_asked() {
        // The other half of `sliderTheme.thumbSelector ?? _default...`, read
        // off a theme rather than handed in.
        use crate::slider_theme::SliderThemeData;
        let slider = collapsed();
        let plain = SliderThemeData::new();
        assert_eq!(
            slider.select_thumb_under(&plain, ThumbTextDirection::Ltr, 0.5, 10.0, 400.0, 0.0),
            None,
            "the default, which refuses to guess at rest"
        );

        let themed = SliderThemeData {
            thumb_selector: Some(crate::range_slider_parts::RangeThumbSelector::new(
                |_direction, _values, _tap, _thumb, _track, _dx| Some(Thumb::Start),
            )),
            ..SliderThemeData::new()
        };
        assert_eq!(
            slider.select_thumb_under(&themed, ThumbTextDirection::Ltr, 0.5, 10.0, 400.0, 0.0),
            Some(Thumb::Start)
        );
    }

    // -- A range slider that draws, tick 248 -----------------------------------------
    //
    // Everything below this line was unreachable until this tick. The three
    // range shapes were ported with their painters and their own tests, and
    // this port had no range slider widget at all: `RangeSlider` was a logic
    // island with no `build` and no constructor anywhere outside its tests.
    // That is why `range_track_shape`, `range_tick_mark_shape` and
    // `range_value_indicator_shape` were the last three entries in
    // `SliderThemeData`'s unread queue.

    use crate::engine_test_stubs::Drawn;
    use crate::framework::{ElementTree, component};

    fn drawn_by(slider: RangeSlider, material3: bool) -> Vec<Drawn> {
        let mut data = crate::theme::ThemeData::light();
        data.use_material3 = material3;
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(data, component(slider)));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    /// What `ResolvedRangeSlider::of` answers under a bare theme with the
    /// Material 3 flag set either way.
    fn resolved_under(material3: bool) -> ResolvedRangeSlider {
        struct Reader {
            seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedRangeSlider>>>,
        }
        impl crate::framework::Component for Reader {
            fn build(
                &self,
                context: &mut crate::framework::BuildContext,
            ) -> crate::framework::AnyWidget {
                *self.seen.borrow_mut() = Some(ResolvedRangeSlider::of(context));
                crate::framework::leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let mut data = crate::theme::ThemeData::light();
        data.use_material3 = material3;
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            data,
            component(Reader {
                seen: std::rc::Rc::clone(&seen),
            }),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    /// Every circle drawn, by centre, radius and colour.
    fn circles(calls: &[Drawn]) -> Vec<(f32, f32, u32)> {
        calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Circle {
                    cx, radius, argb, ..
                } => Some((*cx, *radius, *argb)),
                _ => None,
            })
            .collect()
    }

    /// Where the Material 2 thumbs are.
    ///
    /// Both halves of this filter were learned by getting it wrong. The
    /// colour, because a round thumb draws its three elevation shadows as
    /// circles too, at the same centre and very nearly the same radius, so
    /// counting circles finds eight thumbs on a slider that has two. And the
    /// size, because Material 3's gapped track draws its own stop indicators
    /// at the two ends of the track -- small circles, in the same `primary`
    /// the thumb is filled with -- so a colour test alone finds two thumbs on
    /// a Material 3 slider that draws none.
    fn thumbs(calls: &[Drawn]) -> Vec<f32> {
        let fill = crate::theme::ThemeData::light().color_scheme.primary.0;
        circles(calls)
            .into_iter()
            .filter(|(_, radius, argb)| *argb == fill && *radius >= 5.0)
            .map(|(cx, _, _)| cx)
            .collect()
    }

    /// Every tick mark, by centre and colour. They are the small circles; a
    /// thumb and its shadows are ten times the size. Material 2 only -- a
    /// Material 3 track puts stop indicators in the same size range.
    fn marks(calls: &[Drawn]) -> Vec<(f32, u32)> {
        circles(calls)
            .into_iter()
            .filter(|(_, radius, _)| *radius < 5.0)
            .map(|(cx, _, argb)| (cx, argb))
            .collect()
    }

    #[test]
    fn a_range_slider_draws_a_track_and_two_thumbs() {
        // The first pixels this type has ever put on a canvas.
        let calls = drawn_by(RangeSlider::new(0.25, 0.75), false);
        assert!(!calls.is_empty(), "it draws at all");

        // Material 2: a rounded rect track and two round thumbs, a quarter
        // and three quarters along.
        let thumbs = thumbs(&calls);
        assert_eq!(thumbs.len(), 2, "two thumbs, not one and not none");
        assert!(thumbs[0] < thumbs[1], "the start thumb is the left one");
        assert!(
            (thumbs[1] - thumbs[0]) > 50.0,
            "and they are a half-track apart, not on top of each other"
        );
    }

    #[test]
    fn the_thumbs_move_with_the_values_and_swap_under_rtl() {
        let positions = |slider: RangeSlider| thumbs(&drawn_by(slider, false));
        let narrow = positions(RangeSlider::new(0.4, 0.6));
        let wide = positions(RangeSlider::new(0.1, 0.9));
        assert!(
            wide[0] < narrow[0] && wide[1] > narrow[1],
            "a wider range puts the thumbs further apart"
        );

        // A collapsed range puts them in the same place, which is the case
        // the thumb shape draws a ring for.
        let together = positions(RangeSlider::new(0.5, 0.5));
        assert_eq!(together[0], together[1]);
    }

    #[test]
    fn a_discrete_range_slider_marks_its_divisions() {
        // `range_tick_mark_shape` had no caller. A continuous slider has
        // nothing to mark, which is what makes this about divisions rather
        // than about range sliders.
        let smooth = RangeSlider::new(0.25, 0.75);
        let mut discrete = smooth;
        discrete.divisions = Some(4);

        assert_eq!(
            marks(&drawn_by(discrete, false)).len(),
            5,
            "four divisions, five marks"
        );
        assert_eq!(marks(&drawn_by(smooth, false)).len(), 0);
    }

    #[test]
    fn a_mark_between_the_thumbs_is_a_different_colour_from_one_outside() {
        // The shape decides this and it is handed *both* thumb centres to do
        // it -- which is the difference between a range track and a
        // single-value one, where a mark only has to know which side of one
        // thumb it is on.
        let mut slider = RangeSlider::new(0.25, 0.75);
        slider.divisions = Some(4);
        let inks: Vec<u32> = marks(&drawn_by(slider, false))
            .into_iter()
            .map(|(_, argb)| argb)
            .collect();
        assert_eq!(inks.len(), 5);
        assert_ne!(
            inks[0], inks[2],
            "the mark at the left end is outside the range and the middle one is in it"
        );
        assert_eq!(inks[0], inks[4], "and both ends are outside it");
        assert_eq!(
            inks[1], inks[0],
            "and a mark the thumb sits exactly on counts as outside, which is              what upstream's strict comparison says"
        );
    }

    #[test]
    fn material_two_and_material_three_do_not_draw_the_same_slider() {
        // Upstream keeps a separate defaults pair for the range slider and
        // the two tables disagree about every shape in it: a rounded-rect
        // track and round thumbs against a gapped track and handle thumbs.
        // A resolver that ignored the flag would draw one picture twice.
        let two = drawn_by(RangeSlider::new(0.25, 0.75), false);
        let three = drawn_by(RangeSlider::new(0.25, 0.75), true);
        assert_ne!(two, three);

        // Each shape separately, and on the resolved value rather than on
        // the pixels, because `assert_ne!` on the whole list is satisfied by
        // *any* one of them differing. Making both tables pick the same
        // *track* shape left that assertion green -- the thumb shapes still
        // differed, and that was enough for it. The tracks then draw the same
        // three paths as each other and differ only in the numbers, which is
        // no test at all.
        let m2 = resolved_under(false);
        let m3 = resolved_under(true);
        assert_ne!(m2.track_shape, m3.track_shape);
        assert_ne!(m2.tick_mark_shape, m3.tick_mark_shape);
        assert_ne!(m2.thumb_shape, m3.thumb_shape);
        assert_ne!(m2.value_indicator_shape, m3.value_indicator_shape);

        // The handle thumb is a rounded rectangle, not a circle, so Material
        // 3 draws no thumb circles at all.
        assert_eq!(thumbs(&two).len(), 2);
        assert_eq!(thumbs(&three).len(), 0, "handles are not circles");
    }

    #[test]
    fn a_theme_that_names_a_track_shape_gets_that_one() {
        // `range_track_shape` read off the theme rather than defaulted, which
        // is the half of the field the defaults table cannot show.
        use crate::range_slider_parts::{RangeSliderTrackShape, RectangularRangeSliderTrackShape};
        let asked = crate::slider_theme::SliderThemeData {
            range_track_shape: Some(RangeSliderTrackShape::Rectangular(
                RectangularRangeSliderTrackShape::default(),
            )),
            ..crate::slider_theme::SliderThemeData::new()
        };
        let mut data = crate::theme::ThemeData::light();
        data.slider_theme = asked;
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            data,
            component(RangeSlider::new(0.25, 0.75)),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        let calls = crate::engine_test_stubs::drawn();
        // A rectangular track draws rectangles; the default rounded one draws
        // rounded rectangles.
        assert!(
            calls.iter().any(|call| matches!(call, Drawn::Rect { .. })),
            "the theme's rectangular track"
        );
        assert!(
            !drawn_by(RangeSlider::new(0.25, 0.75), false)
                .iter()
                .any(|call| matches!(call, Drawn::Rect { .. })),
            "which the default does not draw"
        );
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
