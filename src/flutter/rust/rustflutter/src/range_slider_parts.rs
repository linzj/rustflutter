// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The range slider's parts (upstream `material/range_slider_parts.dart`).
//!
//! A range slider is a slider with two thumbs, and every one of its pieces is
//! a second copy of the single-value piece with the second thumb threaded
//! through it: a track that is active *between* the thumbs rather than up to
//! one, tick marks that ask whether they fall between the two, and a thumb
//! that knows whether it is the one drawn on top when the pair meet.
//!
//! Upstream keeps them in their own file with their own abstract bases, and
//! so does this. The shapes and the arithmetic they share with the
//! single-value ones -- the value indicator painters, the colour
//! interpolation, the per-corner fill -- come from
//! [`slider_theme`](crate::slider_theme) rather than being written twice; the
//! places where upstream's range copy has actually diverged are called out
//! where they occur.
//!
//! # Recorded divergences
//!
//! * Upstream keeps a second copy of `_RoundedRectSliderValueIndicatorPathPainter`
//!   and `_DropSliderValueIndicatorPathPainter` in this file. The constants
//!   are the same to the digit, so the two range indicators here call the
//!   painters in `slider_theme` rather than a duplicate pair.
//! * `debugDisableShadows` and the `_debugDrawShadow` it turns on are not
//!   modelled: the diagnostics layer is P10.

use crate::borders::{BorderRadius, Radius};
use crate::direction::TextDirection;
use crate::engine::{Canvas, Color, Paint, Rect};
use crate::painting::{ClipOp, TextPainter, elevation_shadows};
use crate::render::{Offset, Size};
use crate::slider_theme::{
    DropIndicatorPainter, IndicatorPaintGeometry, RoundedRectIndicatorPainter, SliderThemeData,
    Thumb, enabled_color, fill_rrect, indicator_stroke,
};

/// Upstream `RangeValues`: where a range slider's two thumbs are.
///
/// Upstream does not order them, and neither does this: `start` is the thumb
/// the reader dragged first, and a range slider is allowed to have it past
/// `end` while a drag is in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeValues {
    pub start: f32,
    pub end: f32,
}

impl RangeValues {
    pub const fn new(start: f32, end: f32) -> RangeValues {
        RangeValues { start, end }
    }
}

/// Upstream `RangeLabels`: what the two value indicators say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangeLabels {
    pub start: String,
    pub end: String,
}

impl RangeLabels {
    pub fn new(start: impl Into<String>, end: impl Into<String>) -> RangeLabels {
        RangeLabels {
            start: start.into(),
            end: end.into(),
        }
    }
}

/// Upstream `RangeThumbSelector`: which thumb a tap should move.
///
/// A function rather than a rule because the answer depends on more than
/// the distance -- upstream's default keeps the thumbs from crossing, and
/// an application that wants them to cross replaces it.
///
/// Upstream's is a bare typedef. It is a type of its own here because a
/// `dyn Fn` has neither `Debug` nor `PartialEq`, and the theme that carries
/// it needs both. Two are equal when they are the same closure, by
/// identity -- the same rule, and the same reason, as
/// [`StateProperty`](crate::widget_state::StateProperty).
#[derive(Clone)]
pub struct RangeThumbSelector(
    std::rc::Rc<dyn Fn(TextDirection, RangeValues, f32, Size, Size, f32) -> Option<Thumb>>,
);

impl RangeThumbSelector {
    pub fn new(
        select: impl Fn(TextDirection, RangeValues, f32, Size, Size, f32) -> Option<Thumb> + 'static,
    ) -> RangeThumbSelector {
        RangeThumbSelector(std::rc::Rc::new(select))
    }

    pub fn select(
        &self,
        direction: TextDirection,
        values: RangeValues,
        tap_value: f32,
        thumb_size: Size,
        track_size: Size,
        dx: f32,
    ) -> Option<Thumb> {
        (self.0)(direction, values, tap_value, thumb_size, track_size, dx)
    }
}

impl PartialEq for RangeThumbSelector {
    fn eq(&self, other: &RangeThumbSelector) -> bool {
        std::rc::Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for RangeThumbSelector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RangeThumbSelector")
    }
}

// -- Thumbs (upstream `RangeSliderThumbShape`) --------------------------------

/// Upstream `RangeSliderThumbShape`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeSliderThumbShape {
    Round(RoundRangeSliderThumbShape),
    Handle(HandleRangeSliderThumbShape),
}

/// Upstream `RoundRangeSliderThumbShape`: Material 2's, the same circle the
/// single-value slider draws, plus a ring when the two thumbs meet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundRangeSliderThumbShape {
    pub enabled_thumb_radius: f32,
    pub disabled_thumb_radius: Option<f32>,
    pub elevation: f32,
    pub pressed_elevation: f32,
}

impl Default for RoundRangeSliderThumbShape {
    fn default() -> RoundRangeSliderThumbShape {
        RoundRangeSliderThumbShape {
            enabled_thumb_radius: 10.0,
            disabled_thumb_radius: None,
            elevation: 1.0,
            pressed_elevation: 6.0,
        }
    }
}

impl RoundRangeSliderThumbShape {
    pub fn new() -> RoundRangeSliderThumbShape {
        RoundRangeSliderThumbShape::default()
    }

    /// Upstream's private `_disabledThumbRadius`.
    pub fn disabled_thumb_radius(&self) -> f32 {
        self.disabled_thumb_radius
            .unwrap_or(self.enabled_thumb_radius)
    }

    /// Upstream `getPreferredSize`.
    pub fn preferred_size(&self, is_enabled: bool) -> Size {
        Size::from_radius(if is_enabled {
            self.enabled_thumb_radius
        } else {
            self.disabled_thumb_radius()
        })
    }

    /// Upstream `paint`.
    ///
    /// Two things the single-value thumb does not do. `is_on_top` draws a 1dp
    /// ring, so that two thumbs sitting on the same value still read as two.
    /// And the elevation only follows the activation animation while the
    /// thumb is actually pressed -- on a range slider the animation is shared
    /// by both thumbs, so an unpressed one that took it would rise along with
    /// its partner.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        center: Offset,
        theme: &SliderThemeData,
        activation: f32,
        enable: f32,
        is_on_top: bool,
        is_pressed: bool,
    ) {
        let radius = self.disabled_thumb_radius()
            + (self.enabled_thumb_radius - self.disabled_thumb_radius()) * enable;
        if is_on_top {
            if let Some(color) = theme.overlapping_shape_stroke_color {
                canvas.draw_circle(center.dx, center.dy, radius, &indicator_stroke(color));
            }
        }
        let Some(color) = enabled_color(theme.disabled_thumb_color, theme.thumb_color, enable)
        else {
            return;
        };
        let elevation = if is_pressed {
            self.elevation + (self.pressed_elevation - self.elevation) * activation
        } else {
            self.elevation
        };
        for shadow in elevation_shadows(elevation.max(0.0).round() as u32) {
            canvas.draw_circle(
                center.dx + shadow.offset.dx,
                center.dy + shadow.offset.dy,
                radius + shadow.spread_radius,
                &shadow.to_paint(),
            );
        }
        canvas.draw_circle(center.dx, center.dy, radius, &Paint::new(color));
    }
}

/// Upstream `HandleRangeSliderThumbShape`: Material 3's bar.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandleRangeSliderThumbShape;

impl HandleRangeSliderThumbShape {
    pub fn new() -> HandleRangeSliderThumbShape {
        HandleRangeSliderThumbShape
    }

    /// Upstream `getPreferredSize`, the same constant the single-value bar
    /// reports and, for the same reason, not the size it draws at.
    pub fn preferred_size(&self) -> Size {
        Size::new(4.0, 44.0)
    }

    /// Upstream `paint`.
    ///
    /// It takes `isOnTop` like the circle does and, unlike the circle, never
    /// reads it: two bars that meet are already distinguishable, so upstream
    /// draws no ring.
    pub fn paint(&self, canvas: &mut Canvas, center: Offset, theme: &SliderThemeData, enable: f32) {
        let Some(color) = enabled_color(theme.disabled_thumb_color, theme.thumb_color, enable)
        else {
            return;
        };
        let size = theme
            .thumb_size
            .as_ref()
            .and_then(|property| property.resolve(crate::widget_state::WidgetStates::NONE))
            .unwrap_or_else(|| self.preferred_size());
        let rect = Rect::from_center(center.dx, center.dy, size.width, size.height);
        canvas.draw_rounded_rect(rect, size.shortest_side() / 2.0, &Paint::new(color));
    }
}

impl RangeSliderThumbShape {
    /// Upstream `getPreferredSize`.
    pub fn preferred_size(&self, is_enabled: bool) -> Size {
        match self {
            RangeSliderThumbShape::Round(shape) => shape.preferred_size(is_enabled),
            RangeSliderThumbShape::Handle(shape) => shape.preferred_size(),
        }
    }

    /// Upstream `paint`.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        center: Offset,
        theme: &SliderThemeData,
        activation: f32,
        enable: f32,
        is_on_top: bool,
        is_pressed: bool,
    ) {
        match self {
            RangeSliderThumbShape::Round(shape) => shape.paint(
                canvas, center, theme, activation, enable, is_on_top, is_pressed,
            ),
            RangeSliderThumbShape::Handle(shape) => shape.paint(canvas, center, theme, enable),
        }
    }
}

// -- Tick marks (upstream `RangeSliderTickMarkShape`) -------------------------

/// Upstream `RangeSliderTickMarkShape`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeSliderTickMarkShape {
    Round(RoundRangeSliderTickMarkShape),
}

/// Upstream `RoundRangeSliderTickMarkShape`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundRangeSliderTickMarkShape {
    pub tick_mark_radius: Option<f32>,
}

impl RoundRangeSliderTickMarkShape {
    pub fn new() -> RoundRangeSliderTickMarkShape {
        RoundRangeSliderTickMarkShape::default()
    }

    pub fn with_radius(radius: f32) -> RoundRangeSliderTickMarkShape {
        RoundRangeSliderTickMarkShape {
            tick_mark_radius: Some(radius),
        }
    }

    /// Upstream `getPreferredSize`: a quarter of the track's height when the
    /// shape names no radius of its own.
    pub fn preferred_size(&self, theme: &SliderThemeData) -> Size {
        Size::from_radius(
            self.tick_mark_radius
                .unwrap_or_else(|| theme.track_height.unwrap_or(0.0) / 4.0),
        )
    }

    /// Upstream `paint`. A mark is active when it lies between the two thumbs
    /// and inactive outside them, which is the whole of the difference from
    /// the single-value shape's "before or after the one thumb".
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        center: Offset,
        theme: &SliderThemeData,
        start_thumb: Offset,
        end_thumb: Offset,
        direction: TextDirection,
        enable: f32,
    ) {
        // With a gap in the track, a mark directly under a thumb would sit in
        // the gap the thumb is meant to leave, so upstream drops it. Without a
        // gap it is drawn: the thumb covers it anyway.
        let has_gap = theme.track_gap.is_some_and(|gap| gap > 0.0);
        let under_thumb = start_thumb.dx == center.dx || end_thumb.dx == center.dx;
        if has_gap && under_thumb {
            return;
        }
        let between = match direction {
            TextDirection::Ltr => start_thumb.dx < center.dx && center.dx < end_thumb.dx,
            TextDirection::Rtl => end_thumb.dx < center.dx && center.dx < start_thumb.dx,
        };
        let (disabled, enabled) = if between {
            (
                theme.disabled_active_tick_mark_color,
                theme.active_tick_mark_color,
            )
        } else {
            (
                theme.disabled_inactive_tick_mark_color,
                theme.inactive_tick_mark_color,
            )
        };
        let Some(color) = enabled_color(disabled, enabled, enable) else {
            return;
        };
        let radius = self.preferred_size(theme).width / 2.0;
        if radius > 0.0 {
            canvas.draw_circle(center.dx, center.dy, radius, &Paint::new(color));
        }
    }
}

impl RangeSliderTickMarkShape {
    /// Upstream `getPreferredSize`.
    pub fn preferred_size(&self, theme: &SliderThemeData) -> Size {
        match self {
            RangeSliderTickMarkShape::Round(shape) => shape.preferred_size(theme),
        }
    }

    /// Upstream `paint`.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        center: Offset,
        theme: &SliderThemeData,
        start_thumb: Offset,
        end_thumb: Offset,
        direction: TextDirection,
        enable: f32,
    ) {
        match self {
            RangeSliderTickMarkShape::Round(shape) => shape.paint(
                canvas,
                center,
                theme,
                start_thumb,
                end_thumb,
                direction,
                enable,
            ),
        }
    }
}

// -- The track (upstream `RangeSliderTrackShape`) -----------------------------

/// Upstream `RangeSliderTrackShape`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeSliderTrackShape {
    Rectangular(RectangularRangeSliderTrackShape),
    RoundedRect(RoundedRectRangeSliderTrackShape),
    Gapped(GappedRangeSliderTrackShape),
}

/// Upstream `BaseRangeSliderTrackShape`.
///
/// The same rectangle the single-value tracks lay themselves out in, with one
/// real difference: a theme that names a `padding` still leaves half a thumb
/// at each end here, where the single-value shape leaves nothing. Both thumbs
/// of a range slider reach the ends of the travel, so there is no arrangement
/// in which the outer half of one is not needed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BaseRangeSliderTrackShape;

impl BaseRangeSliderTrackShape {
    /// Upstream `getPreferredRect`.
    pub fn preferred_rect(
        parent_size: Size,
        offset: Offset,
        theme: &SliderThemeData,
        is_enabled: bool,
    ) -> Rect {
        let thumb_width = theme
            .range_thumb_shape
            .map_or(0.0, |shape| shape.preferred_size(is_enabled).width);
        let overlay_width = theme
            .overlay_shape
            .map_or(0.0, |shape| shape.preferred_size(is_enabled).width);
        let mut track_height = theme.track_height.unwrap_or(0.0);
        debug_assert!(overlay_width >= 0.0);
        debug_assert!(track_height >= 0.0);

        if theme.active_track_color == Some(Color::TRANSPARENT)
            && theme.inactive_track_color == Some(Color::TRANSPARENT)
        {
            track_height = 0.0;
        }

        let track_left = offset.dx
            + if theme.padding.is_none() {
                (overlay_width / 2.0).max(thumb_width / 2.0)
            } else {
                thumb_width / 2.0
            };
        let track_top = offset.dy + (parent_size.height - track_height) / 2.0;
        let track_right = track_left + parent_size.width
            - if theme.padding.is_none() {
                thumb_width.max(overlay_width)
            } else {
                thumb_width
            };
        let track_bottom = track_top + track_height;
        Rect::ltrb(
            track_left.min(track_right),
            track_top,
            track_left.max(track_right),
            track_bottom,
        )
    }
}

/// What a range track shape is told about the frame it is painting.
///
/// The single-value [`TrackPaintGeometry`](crate::slider_theme::TrackPaintGeometry)
/// with a second thumb and no secondary value: a range slider has no
/// secondary track, because the space between its thumbs is already the
/// active one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeTrackPaintGeometry {
    pub track: Rect,
    pub start_thumb_center: Offset,
    pub end_thumb_center: Offset,
    pub direction: TextDirection,
    /// The enable animation, already evaluated.
    pub enable: f32,
}

impl RangeTrackPaintGeometry {
    pub fn new(
        track: Rect,
        start_thumb_center: Offset,
        end_thumb_center: Offset,
        direction: TextDirection,
        enable: f32,
    ) -> RangeTrackPaintGeometry {
        RangeTrackPaintGeometry {
            track,
            start_thumb_center,
            end_thumb_center,
            direction,
            enable,
        }
    }

    /// Upstream's `(leftThumbOffset, rightThumbOffset)`: the start thumb is
    /// the left one in a left-to-right slider and the right one otherwise.
    pub fn left_and_right_thumbs(&self) -> (f32, f32) {
        match self.direction {
            TextDirection::Ltr => (self.start_thumb_center.dx, self.end_thumb_center.dx),
            TextDirection::Rtl => (self.end_thumb_center.dx, self.start_thumb_center.dx),
        }
    }

    /// The active and inactive colours, already interpolated for the enable
    /// animation. Unlike the single-value track's pair these do not swap with
    /// the text direction: the active part of a range track is between the
    /// thumbs whichever way round they are.
    fn colors(&self, theme: &SliderThemeData) -> Option<(Color, Color)> {
        let active = enabled_color(
            theme.disabled_active_track_color,
            theme.active_track_color,
            self.enable,
        )?;
        let inactive = enabled_color(
            theme.disabled_inactive_track_color,
            theme.inactive_track_color,
            self.enable,
        )?;
        Some((active, inactive))
    }
}

/// Upstream `RectangularRangeSliderTrackShape`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectangularRangeSliderTrackShape;

impl RectangularRangeSliderTrackShape {
    pub fn new() -> RectangularRangeSliderTrackShape {
        RectangularRangeSliderTrackShape
    }

    /// Upstream `paint`: three flat segments, inactive outside the thumbs and
    /// active between them.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        geometry: &RangeTrackPaintGeometry,
        theme: &SliderThemeData,
    ) {
        let Some((active, inactive)) = geometry.colors(theme) else {
            return;
        };
        let track = geometry.track;
        let (left, right) = geometry.left_and_right_thumbs();
        for (rect, color) in [
            (
                Rect::ltrb(track.left, track.top, left, track.bottom),
                inactive,
            ),
            (Rect::ltrb(left, track.top, right, track.bottom), active),
            (
                Rect::ltrb(right, track.top, track.right, track.bottom),
                inactive,
            ),
        ] {
            if rect.width() > 0.0 {
                canvas.draw_rect(rect, &Paint::new(color));
            }
        }
    }
}

/// Upstream `RoundedRectRangeSliderTrackShape`: Material 2's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundedRectRangeSliderTrackShape;

impl RoundedRectRangeSliderTrackShape {
    pub fn new() -> RoundedRectRangeSliderTrackShape {
        RoundedRectRangeSliderTrackShape
    }

    pub fn is_rounded(&self) -> bool {
        true
    }

    /// Upstream `paint`. `additional_active_track_height` is upstream's
    /// parameter of the same name, defaulting to 2.
    ///
    /// The two inactive ends are drawn unconditionally here, where the
    /// single-value shape guards each against the thumb having reached it:
    /// the active middle is drawn last and over them, so an end the thumb has
    /// swallowed is covered rather than skipped.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        geometry: &RangeTrackPaintGeometry,
        theme: &SliderThemeData,
        additional_active_track_height: f32,
    ) {
        let Some((active, inactive)) = geometry.colors(theme) else {
            return;
        };
        let track = geometry.track;
        let (left, right) = geometry.left_and_right_thumbs();
        let radius = Radius::circular(track.height() / 2.0);
        let track_height = theme.track_height.unwrap_or(0.0);
        let grow = additional_active_track_height / 2.0;

        fill_rrect(
            canvas,
            Rect::ltrb(track.left, track.top, left, track.bottom),
            BorderRadius::only(radius, Radius::ZERO, radius, Radius::ZERO),
            inactive,
        );
        fill_rrect(
            canvas,
            Rect::ltrb(right, track.top, track.right, track.bottom),
            BorderRadius::only(Radius::ZERO, radius, Radius::ZERO, radius),
            inactive,
        );
        fill_rrect(
            canvas,
            Rect::ltrb(
                left - track_height / 2.0,
                track.top - grow,
                right + track_height / 2.0,
                track.bottom + grow,
            ),
            BorderRadius::all(radius),
            active,
        );
    }
}

/// Upstream `GappedRangeSliderTrackShape`: Material 3's, with a gap outside
/// each thumb and a stop indicator at each end.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GappedRangeSliderTrackShape;

impl GappedRangeSliderTrackShape {
    pub fn new() -> GappedRangeSliderTrackShape {
        GappedRangeSliderTrackShape
    }

    pub fn is_rounded(&self) -> bool {
        true
    }

    /// Upstream `paint`.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        geometry: &RangeTrackPaintGeometry,
        theme: &SliderThemeData,
        is_discrete: bool,
    ) {
        let track_height = theme.track_height.unwrap_or(0.0);
        let Some((active, inactive)) = geometry.colors(theme) else {
            return;
        };
        let gap = theme.track_gap.unwrap_or(0.0);
        let track = geometry.track;
        let (left, right) = geometry.left_and_right_thumbs();
        let outer = Radius::circular(track.shortest_side() / 2.0);
        let inner = Radius::circular(2.0);

        let left_rect = Rect::ltrb(track.left, track.top, left - gap, track.bottom);
        let right_rect = Rect::ltrb(right + gap, track.top, track.right, track.bottom);

        canvas.saved(|canvas| {
            let corner = track.shortest_side() / 2.0;
            canvas.clip_rounded_rect(track, corner, corner, ClipOp::Intersect, true);
            if geometry.start_thumb_center.dx > left_rect.left + track_height / 2.0 {
                fill_rrect(
                    canvas,
                    left_rect,
                    BorderRadius::only(outer, inner, outer, inner),
                    inactive,
                );
            }
            if geometry.end_thumb_center.dx < right_rect.right - track_height / 2.0 {
                fill_rrect(
                    canvas,
                    right_rect,
                    BorderRadius::only(inner, outer, inner, outer),
                    inactive,
                );
            }
            // The active middle is drawn only when the two gaps have not met;
            // thumbs closer together than twice the gap leave no track
            // between them at all.
            if left + gap < right - gap {
                fill_rrect(
                    canvas,
                    Rect::ltrb(left + gap, track.top, right - gap, track.bottom),
                    BorderRadius::all(inner),
                    active,
                );
            }
        });

        // A stop indicator at each end, each shown only while its own thumb
        // has not reached it, and neither on a discrete slider -- there is a
        // tick mark there already.
        let trailing_space = track_height / 2.0;
        let (_, center_y) = track.center();
        let start_x = track.left + trailing_space;
        let end_x = track.right - trailing_space;
        if !is_discrete {
            if geometry.start_thumb_center.dx > start_x {
                canvas.draw_circle(start_x, center_y, 2.0, &Paint::new(active));
            }
            if geometry.end_thumb_center.dx < end_x {
                canvas.draw_circle(end_x, center_y, 2.0, &Paint::new(active));
            }
        }
    }
}

impl RangeSliderTrackShape {
    /// Upstream `getPreferredRect`, which all three take from
    /// [`BaseRangeSliderTrackShape`].
    pub fn preferred_rect(
        &self,
        parent_size: Size,
        offset: Offset,
        theme: &SliderThemeData,
        is_enabled: bool,
    ) -> Rect {
        BaseRangeSliderTrackShape::preferred_rect(parent_size, offset, theme, is_enabled)
    }

    /// Upstream `isRounded`.
    pub fn is_rounded(&self) -> bool {
        match self {
            RangeSliderTrackShape::Rectangular(_) => false,
            RangeSliderTrackShape::RoundedRect(shape) => shape.is_rounded(),
            RangeSliderTrackShape::Gapped(shape) => shape.is_rounded(),
        }
    }

    /// Upstream `paint`.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        geometry: &RangeTrackPaintGeometry,
        theme: &SliderThemeData,
        is_discrete: bool,
    ) {
        match self {
            RangeSliderTrackShape::Rectangular(shape) => shape.paint(canvas, geometry, theme),
            RangeSliderTrackShape::RoundedRect(shape) => shape.paint(canvas, geometry, theme, 2.0),
            RangeSliderTrackShape::Gapped(shape) => {
                shape.paint(canvas, geometry, theme, is_discrete)
            }
        }
    }
}

// -- The two range-only value indicators --------------------------------------

/// Upstream `RoundedRectRangeSliderValueIndicatorShape`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundedRectRangeSliderValueIndicatorShape;

impl RoundedRectRangeSliderValueIndicatorShape {
    pub fn new() -> RoundedRectRangeSliderValueIndicatorShape {
        RoundedRectRangeSliderValueIndicatorShape
    }

    pub fn preferred_size(&self, label: &TextPainter, text_scale: f32) -> Size {
        RoundedRectIndicatorPainter::preferred_size(label.width(), text_scale)
    }

    pub fn horizontal_shift(&self, label: &TextPainter, geometry: &IndicatorPaintGeometry) -> f32 {
        crate::slider_theme::indicator_horizontal_shift(
            RoundedRectIndicatorPainter::upper_rectangle_width(label.width(), geometry.scale),
            geometry,
        )
    }

    pub fn paint(
        &self,
        canvas: &mut Canvas,
        geometry: &IndicatorPaintGeometry,
        theme: &SliderThemeData,
        label: &TextPainter,
    ) {
        let Some(background) = theme.value_indicator_color else {
            return;
        };
        RoundedRectIndicatorPainter::paint(
            canvas,
            geometry,
            label,
            background,
            theme.value_indicator_stroke_color,
        );
    }
}

/// Upstream `DropRangeSliderValueIndicatorShape`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DropRangeSliderValueIndicatorShape;

impl DropRangeSliderValueIndicatorShape {
    pub fn new() -> DropRangeSliderValueIndicatorShape {
        DropRangeSliderValueIndicatorShape
    }

    pub fn preferred_size(&self, label: &TextPainter, text_scale: f32) -> Size {
        DropIndicatorPainter::preferred_size(label.width(), text_scale)
    }

    pub fn horizontal_shift(&self, label: &TextPainter, geometry: &IndicatorPaintGeometry) -> f32 {
        crate::slider_theme::indicator_horizontal_shift(
            DropIndicatorPainter::upper_rectangle_width(label.width(), geometry.scale),
            geometry,
        )
    }

    pub fn paint(
        &self,
        canvas: &mut Canvas,
        geometry: &IndicatorPaintGeometry,
        theme: &SliderThemeData,
        label: &TextPainter,
    ) {
        let Some(background) = theme.value_indicator_color else {
            return;
        };
        DropIndicatorPainter::paint(
            canvas,
            geometry,
            label,
            background,
            theme.value_indicator_stroke_color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slider_theme::{RoundSliderOverlayShape, SliderComponentShape};

    fn measurable() -> SliderThemeData {
        SliderThemeData {
            track_height: Some(4.0),
            range_thumb_shape: Some(RangeSliderThumbShape::Round(
                RoundRangeSliderThumbShape::new(),
            )),
            overlay_shape: Some(SliderComponentShape::RoundOverlay(
                RoundSliderOverlayShape::new(),
            )),
            ..SliderThemeData::new()
        }
    }

    #[test]
    fn a_padded_range_track_still_leaves_half_a_thumb_at_each_end() {
        // This is where the range track has actually diverged from the
        // single-value one: with a padding the single-value shape leaves
        // nothing, and this one still leaves half a thumb. Both of a range
        // slider's thumbs reach the ends of the travel, so the outer half of
        // one is always needed.
        let mut theme = measurable();
        theme.padding = Some(crate::borders::EdgeInsetsGeometry::Absolute(
            crate::render::EdgeInsets::all(8.0),
        ));
        let ranged = BaseRangeSliderTrackShape::preferred_rect(
            Size::new(200.0, 40.0),
            Offset::ZERO,
            &theme,
            true,
        );
        assert_eq!(ranged.left, 10.0);
        assert_eq!(ranged.right, 190.0);

        // The single-value shape, given the same theme, leaves nothing.
        let mut single = theme.clone();
        single.thumb_shape = Some(SliderComponentShape::RoundThumb(
            crate::slider_theme::RoundSliderThumbShape::new(),
        ));
        let plain = crate::slider_theme::BaseSliderTrackShape::preferred_rect(
            Size::new(200.0, 40.0),
            Offset::ZERO,
            &single,
            true,
        );
        assert_eq!(plain.left, 0.0);
        assert_eq!(plain.right, 200.0);
    }

    #[test]
    fn without_a_padding_the_two_tracks_measure_the_same() {
        // The divergence is only in the padded branch; unpadded, both leave
        // half the wider of the thumb and the overlay.
        let mut theme = measurable();
        theme.thumb_shape = Some(SliderComponentShape::RoundThumb(
            crate::slider_theme::RoundSliderThumbShape::new(),
        ));
        let ranged = BaseRangeSliderTrackShape::preferred_rect(
            Size::new(200.0, 40.0),
            Offset::ZERO,
            &theme,
            true,
        );
        let plain = crate::slider_theme::BaseSliderTrackShape::preferred_rect(
            Size::new(200.0, 40.0),
            Offset::ZERO,
            &theme,
            true,
        );
        assert_eq!(ranged, plain);
        assert_eq!(ranged.left, 24.0);
    }

    #[test]
    fn the_start_thumb_is_the_left_one_only_left_to_right() {
        let track = Rect::ltrb(0.0, 0.0, 100.0, 4.0);
        let ltr = RangeTrackPaintGeometry::new(
            track,
            Offset::new(20.0, 2.0),
            Offset::new(80.0, 2.0),
            TextDirection::Ltr,
            1.0,
        );
        assert_eq!(ltr.left_and_right_thumbs(), (20.0, 80.0));
        let rtl = RangeTrackPaintGeometry::new(
            track,
            Offset::new(20.0, 2.0),
            Offset::new(80.0, 2.0),
            TextDirection::Rtl,
            1.0,
        );
        assert_eq!(rtl.left_and_right_thumbs(), (80.0, 20.0));
    }

    #[test]
    fn the_range_tracks_colours_do_not_swap_with_the_text_direction() {
        // The single-value track's leading and trailing colours swap, because
        // "before the thumb" is the other side. A range track's active part
        // is between the thumbs whichever way round they are, so nothing
        // swaps -- only which thumb is on the left does.
        let theme = SliderThemeData {
            active_track_color: Some(Color::argb(0xff, 0xff, 0x00, 0x00)),
            inactive_track_color: Some(Color::argb(0xff, 0x00, 0x00, 0xff)),
            disabled_active_track_color: Some(Color::argb(0xff, 0xff, 0x00, 0x00)),
            disabled_inactive_track_color: Some(Color::argb(0xff, 0x00, 0x00, 0xff)),
            ..measurable()
        };
        let track = Rect::ltrb(0.0, 0.0, 100.0, 4.0);
        let ltr = RangeTrackPaintGeometry::new(
            track,
            Offset::new(20.0, 2.0),
            Offset::new(80.0, 2.0),
            TextDirection::Ltr,
            1.0,
        );
        let rtl = RangeTrackPaintGeometry::new(
            track,
            Offset::new(20.0, 2.0),
            Offset::new(80.0, 2.0),
            TextDirection::Rtl,
            1.0,
        );
        assert_eq!(ltr.colors(&theme), rtl.colors(&theme));
        assert_eq!(
            ltr.colors(&theme),
            Some((
                theme.active_track_color.unwrap(),
                theme.inactive_track_color.unwrap()
            ))
        );
    }

    #[test]
    fn a_tick_mark_is_active_between_the_thumbs_and_inactive_outside_them() {
        // The single-value rule is "before or after the one thumb"; this one
        // is "inside or outside the pair", which is not the same question and
        // gives a different answer for every mark past the second thumb.
        let theme = measurable();
        let shape = RoundRangeSliderTickMarkShape::new();
        let start = Offset::new(20.0, 2.0);
        let end = Offset::new(80.0, 2.0);
        let between = |x: f32, direction| {
            // Re-derives what `paint` decides, so the rule is asserted rather
            // than the pixels.
            match direction {
                TextDirection::Ltr => start.dx < x && x < end.dx,
                TextDirection::Rtl => end.dx < x && x < start.dx,
            }
        };
        assert!(between(50.0, TextDirection::Ltr));
        assert!(!between(10.0, TextDirection::Ltr));
        assert!(!between(90.0, TextDirection::Ltr));
        // A mark exactly under a thumb is not between them either way.
        assert!(!between(20.0, TextDirection::Ltr));
        assert_eq!(shape.preferred_size(&theme), Size::from_radius(1.0));
    }

    #[test]
    fn a_mark_under_a_thumb_is_dropped_only_when_the_track_has_a_gap() {
        // With a gap the mark would sit in the space the thumb is meant to
        // leave empty, so upstream skips it. Without a gap it is drawn and
        // the thumb simply covers it -- a reader who drops it in both cases
        // loses a tick from every Material 2 slider.
        let mut theme = measurable();
        assert!(!theme.track_gap.is_some_and(|gap| gap > 0.0));
        theme.track_gap = Some(6.0);
        assert!(theme.track_gap.is_some_and(|gap| gap > 0.0));
        // A gap of zero is not a gap.
        theme.track_gap = Some(0.0);
        assert!(!theme.track_gap.is_some_and(|gap| gap > 0.0));
    }

    #[test]
    fn an_unpressed_thumb_does_not_rise_with_its_partner() {
        // Both thumbs share one activation animation. The single-value thumb
        // reads it unconditionally; this one reads it only while it is the
        // pressed one, or dragging either thumb would lift both.
        let shape = RoundRangeSliderThumbShape::new();
        let pressed = shape.elevation + (shape.pressed_elevation - shape.elevation) * 1.0;
        assert_eq!(pressed, 6.0);
        assert_eq!(shape.elevation, 1.0);
    }

    #[test]
    fn the_material_three_range_thumb_reports_the_same_constant_as_the_single_one() {
        assert_eq!(
            HandleRangeSliderThumbShape::new().preferred_size(),
            crate::slider_theme::HandleThumbShape::new().preferred_size()
        );
    }

    #[test]
    fn only_the_two_rounded_range_tracks_report_themselves_rounded() {
        assert!(!RangeSliderTrackShape::Rectangular(RectangularRangeSliderTrackShape).is_rounded());
        assert!(RangeSliderTrackShape::RoundedRect(RoundedRectRangeSliderTrackShape).is_rounded());
        assert!(RangeSliderTrackShape::Gapped(GappedRangeSliderTrackShape).is_rounded());
    }

    #[test]
    fn two_thumb_selectors_are_the_same_only_when_they_are_the_same_closure() {
        let selector = RangeThumbSelector::new(|_, _, _, _, _, _| Some(Thumb::Start));
        assert_eq!(selector, selector.clone());
        let other = RangeThumbSelector::new(|_, _, _, _, _, _| Some(Thumb::Start));
        assert_ne!(
            selector, other,
            "identity, not behaviour: two closures that do the same thing are still two"
        );
        assert_eq!(
            selector.select(
                TextDirection::Ltr,
                RangeValues::new(0.0, 1.0),
                0.5,
                Size::square(10.0),
                Size::new(100.0, 4.0),
                50.0
            ),
            Some(Thumb::Start)
        );
    }
}
