// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The slider's parts and the theme that carries them (upstream
//! `material/slider_parts.dart`, `material/slider_theme.dart`,
//! `material/slider_value_indicator_shape.dart`).
//!
//! A slider is drawn by five interchangeable pieces -- a track, tick marks, a
//! thumb, a press overlay and a value indicator -- and the theme holds one of
//! each along with every colour they are painted in. Upstream makes each piece
//! an abstract class with concrete subclasses; here each concrete one is its
//! own struct and the abstract base is the enum that names them, which is how
//! [`ShapeBorder`](crate::borders::ShapeBorder) is put together for the same
//! reason: the set is closed, and a `match` is what a painter wants.
//!
//! # Where the animations went
//!
//! Upstream's shapes take `Animation<double>` for the enable and activation
//! animations and read them through a `ColorTween`. A shape does not listen to
//! them -- it evaluates them once, while painting -- so the parameter here is
//! the `f32` it would have evaluated to. The slider itself, when it arrives,
//! is what will drive those numbers.
//!
//! # Recorded divergences
//!
//! * Three of the four `range*Shape` fields wait for
//!   `range_slider_parts.dart`; the types they would be typed against are
//!   not written yet. `thumbSelector`, a `RangeThumbSelector`, waits with
//!   them. The fourth, `rangeValueIndicatorShape`, has its type here --
//!   upstream keeps the two range indicators beside the single-value ones --
//!   and the field lands with the other three so that they arrive together.
//! * Upstream's thumb draws its elevation with `Canvas.drawShadow`, which the
//!   engine binding does not expose. It is drawn here as the circles of
//!   [`elevation_shadows`](crate::painting::elevation_shadows), which is the
//!   same table `drawShadow` consults.

use crate::animation::{ColorTween, Tween};
use crate::borders::{BorderRadius, EdgeInsetsGeometry, Radius};
use crate::component_themes::{lerp_color, lerp_f32, lerp_nearer};
use crate::direction::TextDirection;
use crate::engine::{Canvas, Color, Paint, Rect, Style, TextStyle};
use crate::framework::{AnyWidget, BuildContext, provide};
use crate::painting::{ClipOp, RenderPath, TextPainter, elevation_shadows};
use crate::render::{Offset, Size};
use crate::services::system::SystemMouseCursor;
use crate::theme::ThemeData;
use crate::widget_state::{StateProperty, WidgetStates, lerp_state_property};

/// Upstream `lerpDouble` on two numbers that are both present.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Upstream `ShowValueIndicator`: when the bubble over the thumb is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShowValueIndicator {
    /// Only when the slider has divisions.
    #[default]
    OnlyForDiscrete,
    /// Only when it does not.
    OnlyForContinuous,
    /// Upstream deprecated this after v3.28.0-1.0.pre in favour of
    /// [`ShowValueIndicator::OnDrag`], which is the same behaviour under a
    /// name that says what it does. It is kept because upstream keeps it.
    Always,
    /// While a thumb is being dragged, whether or not there are divisions.
    OnDrag,
    AlwaysVisible,
    Never,
}

/// Upstream `Thumb`: which end of a range slider is meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thumb {
    Start,
    End,
}

/// Upstream `SliderInteraction`: what a gesture on the slider is allowed to
/// do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SliderInteraction {
    /// A tap jumps to the value under it, and a drag from anywhere slides.
    #[default]
    TapAndSlide,
    TapOnly,
    SlideOnly,
    /// A drag slides only when it started on the thumb.
    SlideThumb,
}

// -- The pieces drawn at a point (upstream `SliderComponentShape`) ------------

/// Upstream `SliderComponentShape`: a piece drawn at one point on the track --
/// the thumb, the press overlay, or the value indicator over it.
///
/// The three are one type upstream because the theme's three fields are
/// interchangeable: a shape written as a thumb can be installed as an
/// overlay. That is preserved here rather than split into three enums.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SliderComponentShape {
    /// Upstream `SliderComponentShape.noThumb` and `noOverlay`, which are the
    /// same `_EmptySliderComponentShape`: no size, and nothing drawn.
    Empty,
    RoundThumb(RoundSliderThumbShape),
    Handle(HandleThumbShape),
    RoundOverlay(RoundSliderOverlayShape),
    RectangularIndicator(RectangularSliderValueIndicatorShape),
    RoundedRectIndicator(RoundedRectSliderValueIndicatorShape),
    DropIndicator(DropSliderValueIndicatorShape),
    PaddleIndicator(PaddleSliderValueIndicatorShape),
}

/// Upstream `RoundSliderThumbShape`: Material 2's circular thumb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundSliderThumbShape {
    pub enabled_thumb_radius: f32,
    /// Upstream's is nullable and falls back to
    /// [`RoundSliderThumbShape::enabled_thumb_radius`]; the fallback is in
    /// [`RoundSliderThumbShape::disabled_thumb_radius`].
    pub disabled_thumb_radius: Option<f32>,
    pub elevation: f32,
    pub pressed_elevation: f32,
}

impl Default for RoundSliderThumbShape {
    fn default() -> RoundSliderThumbShape {
        RoundSliderThumbShape {
            enabled_thumb_radius: 10.0,
            disabled_thumb_radius: None,
            elevation: 1.0,
            pressed_elevation: 6.0,
        }
    }
}

impl RoundSliderThumbShape {
    pub fn new() -> RoundSliderThumbShape {
        RoundSliderThumbShape::default()
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

    /// Upstream `paint`: the thumb grows and takes its colour as the slider
    /// is enabled, and rises as it is pressed.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        center: Offset,
        theme: &SliderThemeData,
        activation: f32,
        enable: f32,
    ) {
        let radius = lerp(
            self.disabled_thumb_radius(),
            self.enabled_thumb_radius,
            enable,
        );
        let Some(color) = enabled_color(theme.disabled_thumb_color, theme.thumb_color, enable)
        else {
            return;
        };
        let elevation = lerp(self.elevation, self.pressed_elevation, activation);
        // Upstream's `canvas.drawShadow(path, black, elevation, true)`; the
        // binding has no `drawShadow`, so the same table is drawn by hand.
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

/// Upstream `HandleThumbShape`: Material 3's thumb, a tall rounded bar rather
/// than a circle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandleThumbShape;

impl HandleThumbShape {
    pub fn new() -> HandleThumbShape {
        HandleThumbShape
    }

    /// Upstream `getPreferredSize`, which is a constant. The size it actually
    /// draws at comes from [`SliderThemeData::thumb_size`], because Material
    /// 3's thumb is narrower while it is being dragged.
    pub fn preferred_size(&self) -> Size {
        Size::new(4.0, 44.0)
    }

    /// Upstream `paint`.
    pub fn paint(&self, canvas: &mut Canvas, center: Offset, theme: &SliderThemeData, enable: f32) {
        let Some(color) = enabled_color(theme.disabled_thumb_color, theme.thumb_color, enable)
        else {
            return;
        };
        // Upstream resolves against the empty state set here, with a comment
        // that the states have already been applied where the slider built
        // the theme it handed down.
        let size = theme
            .thumb_size
            .as_ref()
            .and_then(|property| property.resolve(WidgetStates::NONE))
            .unwrap_or_else(|| self.preferred_size());
        let rect = Rect::from_center(center.dx, center.dy, size.width, size.height);
        canvas.draw_rounded_rect(rect, size.shortest_side() / 2.0, &Paint::new(color));
    }
}

/// Upstream `RoundSliderOverlayShape`: the circle that grows under the thumb
/// while it is pressed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundSliderOverlayShape {
    pub overlay_radius: f32,
}

impl Default for RoundSliderOverlayShape {
    fn default() -> RoundSliderOverlayShape {
        RoundSliderOverlayShape {
            overlay_radius: 24.0,
        }
    }
}

impl RoundSliderOverlayShape {
    pub fn new() -> RoundSliderOverlayShape {
        RoundSliderOverlayShape::default()
    }

    /// Upstream `getPreferredSize`, which does not depend on the state: the
    /// overlay reserves its full size whether or not it is drawn, so that the
    /// track does not move when it appears.
    pub fn preferred_size(&self) -> Size {
        Size::from_radius(self.overlay_radius)
    }

    /// Upstream `paint`: nothing at rest, the full circle while pressed.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        center: Offset,
        theme: &SliderThemeData,
        activation: f32,
    ) {
        let Some(color) = theme.overlay_color else {
            return;
        };
        canvas.draw_circle(
            center.dx,
            center.dy,
            lerp(0.0, self.overlay_radius, activation),
            &Paint::new(color),
        );
    }
}

impl SliderComponentShape {
    /// Upstream `getPreferredSize`.
    pub fn preferred_size(&self, is_enabled: bool) -> Size {
        match self {
            SliderComponentShape::Empty => Size::ZERO,
            SliderComponentShape::RoundThumb(shape) => shape.preferred_size(is_enabled),
            SliderComponentShape::Handle(shape) => shape.preferred_size(),
            SliderComponentShape::RoundOverlay(shape) => shape.preferred_size(),
            // The four indicators size themselves from the label they
            // are about to draw, which this signature has no room for;
            // upstream's own two-argument overload cannot answer for
            // them either, and returns nothing.
            SliderComponentShape::RectangularIndicator(_)
            | SliderComponentShape::RoundedRectIndicator(_)
            | SliderComponentShape::DropIndicator(_)
            | SliderComponentShape::PaddleIndicator(_) => Size::ZERO,
        }
    }

    /// Upstream's `getPreferredSize` with its two optional named
    /// parameters filled in, which is the form the value indicators
    /// override: their size is the label's plus their own padding.
    pub fn preferred_size_for_label(
        &self,
        is_enabled: bool,
        label: &TextPainter,
        text_scale: f32,
    ) -> Size {
        match self {
            SliderComponentShape::RectangularIndicator(shape) => {
                shape.preferred_size(label, text_scale)
            }
            SliderComponentShape::RoundedRectIndicator(shape) => {
                shape.preferred_size(label, text_scale)
            }
            SliderComponentShape::DropIndicator(shape) => shape.preferred_size(label, text_scale),
            SliderComponentShape::PaddleIndicator(shape) => shape.preferred_size(label, text_scale),
            other => other.preferred_size(is_enabled),
        }
    }

    /// Upstream `paint`.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        center: Offset,
        theme: &SliderThemeData,
        activation: f32,
        enable: f32,
    ) {
        match self {
            SliderComponentShape::Empty => {}
            SliderComponentShape::RoundThumb(shape) => {
                shape.paint(canvas, center, theme, activation, enable)
            }
            SliderComponentShape::Handle(shape) => shape.paint(canvas, center, theme, enable),
            SliderComponentShape::RoundOverlay(shape) => {
                shape.paint(canvas, center, theme, activation)
            }
            // A value indicator needs a label and the box it may
            // spill into; painting one through this signature would
            // have nothing to draw, so it is a no-op and
            // `paint_indicator` is the way in.
            SliderComponentShape::RectangularIndicator(_)
            | SliderComponentShape::RoundedRectIndicator(_)
            | SliderComponentShape::DropIndicator(_)
            | SliderComponentShape::PaddleIndicator(_) => {}
        }
    }

    /// Upstream `paint` for the shapes installed as a value indicator,
    /// which are the ones that read the label and the overflow box.
    pub fn paint_indicator(
        &self,
        canvas: &mut Canvas,
        geometry: &IndicatorPaintGeometry,
        theme: &SliderThemeData,
        label: &TextPainter,
    ) {
        match self {
            SliderComponentShape::RectangularIndicator(shape) => {
                shape.paint(canvas, geometry, theme, label)
            }
            SliderComponentShape::RoundedRectIndicator(shape) => {
                shape.paint(canvas, geometry, theme, label)
            }
            SliderComponentShape::DropIndicator(shape) => {
                shape.paint(canvas, geometry, theme, label)
            }
            SliderComponentShape::PaddleIndicator(shape) => {
                shape.paint(canvas, geometry, theme, label)
            }
            // A thumb or an overlay installed as the value indicator
            // draws where it would anyway; upstream's shapes are
            // interchangeable, and this is what that costs.
            other => other.paint(canvas, geometry.center, theme, geometry.scale, 1.0),
        }
    }
}

// -- Tick marks (upstream `SliderTickMarkShape`) ------------------------------

/// Upstream `SliderTickMarkShape`: the marks along a slider with divisions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SliderTickMarkShape {
    /// Upstream `SliderTickMarkShape.noTickMark`, which is
    /// `_EmptySliderTickMarkShape`.
    Empty,
    Round(RoundSliderTickMarkShape),
}

/// Upstream `RoundSliderTickMarkShape`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundSliderTickMarkShape {
    /// Unset means a quarter of the track's height, which is why this is
    /// nullable rather than a number with a default.
    pub tick_mark_radius: Option<f32>,
}

impl RoundSliderTickMarkShape {
    pub fn new() -> RoundSliderTickMarkShape {
        RoundSliderTickMarkShape::default()
    }

    pub fn with_radius(radius: f32) -> RoundSliderTickMarkShape {
        RoundSliderTickMarkShape {
            tick_mark_radius: Some(radius),
        }
    }

    /// Upstream `getPreferredSize`.
    pub fn preferred_size(&self, theme: &SliderThemeData) -> Size {
        Size::from_radius(
            self.tick_mark_radius
                .unwrap_or_else(|| theme.track_height.unwrap_or(0.0) / 4.0),
        )
    }

    /// Upstream `paint`: a mark past the thumb is inactive, one before it is
    /// active, and which side "past" is depends on the text direction.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        center: Offset,
        theme: &SliderThemeData,
        thumb_center: Offset,
        direction: TextDirection,
        enable: f32,
    ) {
        let offset = center.dx - thumb_center.dx;
        let inactive = match direction {
            TextDirection::Ltr => offset > 0.0,
            TextDirection::Rtl => offset < 0.0,
        };
        let (disabled, enabled) = if inactive {
            (
                theme.disabled_inactive_tick_mark_color,
                theme.inactive_tick_mark_color,
            )
        } else {
            (
                theme.disabled_active_tick_mark_color,
                theme.active_tick_mark_color,
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

impl SliderTickMarkShape {
    /// Upstream `getPreferredSize`.
    pub fn preferred_size(&self, theme: &SliderThemeData) -> Size {
        match self {
            SliderTickMarkShape::Empty => Size::ZERO,
            SliderTickMarkShape::Round(shape) => shape.preferred_size(theme),
        }
    }

    /// Upstream `paint`.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        center: Offset,
        theme: &SliderThemeData,
        thumb_center: Offset,
        direction: TextDirection,
        enable: f32,
    ) {
        match self {
            SliderTickMarkShape::Empty => {}
            SliderTickMarkShape::Round(shape) => {
                shape.paint(canvas, center, theme, thumb_center, direction, enable)
            }
        }
    }
}

// -- The track (upstream `SliderTrackShape`) ----------------------------------

/// Upstream `SliderTrackShape`: the bar the thumb slides along.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SliderTrackShape {
    Rectangular(RectangularSliderTrackShape),
    RoundedRect(RoundedRectSliderTrackShape),
    Gapped(GappedSliderTrackShape),
}

/// Upstream `BaseSliderTrackShape`: the rectangle every track shape lays
/// itself out in.
///
/// A mixin upstream, and a free function here for the same reason -- it is
/// one calculation that three shapes share and that none of them overrides.
/// The width it leaves at each end is half the wider of the thumb and the
/// overlay, so that neither is clipped at the ends of the travel; a theme
/// with a `padding` takes that over and the shape leaves nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BaseSliderTrackShape;

impl BaseSliderTrackShape {
    /// Upstream `getPreferredRect`.
    pub fn preferred_rect(
        parent_size: Size,
        offset: Offset,
        theme: &SliderThemeData,
        is_enabled: bool,
    ) -> Rect {
        let thumb_width = theme
            .thumb_shape
            .map_or(0.0, |shape| shape.preferred_size(is_enabled).width);
        let overlay_width = theme
            .overlay_shape
            .map_or(0.0, |shape| shape.preferred_size(is_enabled).width);
        let mut track_height = theme.track_height.unwrap_or(0.0);
        debug_assert!(overlay_width >= 0.0);
        debug_assert!(track_height >= 0.0);

        // Upstream: if both track colours are transparent, only the height is
        // overridden, so that the slider keeps its overall width.
        if theme.active_track_color == Some(Color::TRANSPARENT)
            && theme.inactive_track_color == Some(Color::TRANSPARENT)
        {
            track_height = 0.0;
        }

        let end_space = if theme.padding.is_none() {
            (overlay_width / 2.0).max(thumb_width / 2.0)
        } else {
            0.0
        };
        let track_left = offset.dx + end_space;
        let track_top = offset.dy + (parent_size.height - track_height) / 2.0;
        let track_right = track_left + parent_size.width
            - if theme.padding.is_none() {
                thumb_width.max(overlay_width)
            } else {
                0.0
            };
        let track_bottom = track_top + track_height;
        // Upstream: a parent narrower than the slider puts the right edge
        // left of the left one, so the two are swapped rather than asserted.
        Rect::ltrb(
            track_left.min(track_right),
            track_top,
            track_left.max(track_right),
            track_bottom,
        )
    }
}

/// Upstream `RectangularSliderTrackShape`: square ends, and the two segments
/// meet at the thumb.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectangularSliderTrackShape;

impl RectangularSliderTrackShape {
    pub fn new() -> RectangularSliderTrackShape {
        RectangularSliderTrackShape
    }

    /// Upstream `paint`.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        geometry: &TrackPaintGeometry,
        theme: &SliderThemeData,
    ) {
        // Upstream: a track no taller than nothing draws nothing, and the
        // whole method is then a no-op rather than a stack of empty rects.
        if theme.track_height.unwrap_or(0.0) <= 0.0 {
            return;
        }
        let Some((leading, trailing)) = track_paints(theme, geometry) else {
            return;
        };
        let track = geometry.track;
        let thumb = geometry.thumb_center.dx;

        // Upstream's `if (!segment.isEmpty)` at each of the three. The
        // height is already guarded above by the track-height check, but the
        // rectangle comes from geometry a caller supplied and can be
        // degenerate for reasons the theme knows nothing about.
        let left = Rect::ltrb(track.left, track.top, thumb, track.bottom);
        if !left.is_empty() {
            canvas.draw_rect(left, &Paint::new(leading));
        }
        let right = Rect::ltrb(thumb, track.top, track.right, track.bottom);
        if !right.is_empty() {
            canvas.draw_rect(right, &Paint::new(trailing));
        }

        if let Some((secondary, color)) = geometry.secondary_segment(theme, false) {
            if !secondary.is_empty() {
                canvas.draw_rect(secondary, &Paint::new(color));
            }
        }
    }
}

/// Upstream `RoundedRectSliderTrackShape`: Material 2's track, with rounded
/// ends and an active half that is a touch taller.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundedRectSliderTrackShape;

impl RoundedRectSliderTrackShape {
    pub fn new() -> RoundedRectSliderTrackShape {
        RoundedRectSliderTrackShape
    }

    /// Upstream's `isRounded`, which the slider consults to decide whether
    /// the value indicator needs to clear a rounded end.
    pub fn is_rounded(&self) -> bool {
        true
    }

    /// Upstream `paint`. `additional_active_track_height` is upstream's
    /// parameter of the same name, defaulting to 2.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        geometry: &TrackPaintGeometry,
        theme: &SliderThemeData,
        additional_active_track_height: f32,
    ) {
        let track_height = theme.track_height.unwrap_or(0.0);
        if track_height <= 0.0 {
            return;
        }
        let Some((leading, trailing)) = track_paints(theme, geometry) else {
            return;
        };
        let track = geometry.track;
        let thumb = geometry.thumb_center.dx;
        let track_radius = Radius::circular(track.height() / 2.0);
        let active_radius =
            Radius::circular((track.height() + additional_active_track_height) / 2.0);
        let ltr = geometry.direction == TextDirection::Ltr;
        let grow = additional_active_track_height / 2.0;

        // The inactive segment runs from the thumb to the far end. Upstream
        // stops half a track-height short so that a thumb at the very end
        // does not leave a sliver of inactive track showing past it.
        if thumb < track.right - track_height / 2.0 {
            let rect = Rect::ltrb(
                thumb - track_height / 2.0,
                if ltr { track.top } else { track.top - grow },
                track.right,
                if ltr {
                    track.bottom
                } else {
                    track.bottom + grow
                },
            );
            let radius = if ltr { track_radius } else { active_radius };
            fill_rrect(canvas, rect, BorderRadius::all(radius), trailing);
        }
        if thumb > track.left + track_height / 2.0 {
            let rect = Rect::ltrb(
                track.left,
                if ltr { track.top - grow } else { track.top },
                thumb + track_height / 2.0,
                if ltr {
                    track.bottom + grow
                } else {
                    track.bottom
                },
            );
            let radius = if ltr { active_radius } else { track_radius };
            fill_rrect(canvas, rect, BorderRadius::all(radius), leading);
        }

        if let Some((rect, color)) = geometry.secondary_segment(theme, false) {
            // Only the outer end of the secondary segment is rounded: the
            // inner one runs into the active track.
            let radius = if ltr {
                BorderRadius::only(Radius::ZERO, track_radius, Radius::ZERO, track_radius)
            } else {
                BorderRadius::only(track_radius, Radius::ZERO, track_radius, Radius::ZERO)
            };
            fill_rrect(canvas, rect, radius, color);
        }
    }
}

/// Upstream `GappedSliderTrackShape`: Material 3's track, which leaves a gap
/// on each side of the thumb and marks the far end with a stop indicator.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GappedSliderTrackShape;

impl GappedSliderTrackShape {
    pub fn new() -> GappedSliderTrackShape {
        GappedSliderTrackShape
    }

    pub fn is_rounded(&self) -> bool {
        true
    }

    /// Upstream `paint`.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        geometry: &TrackPaintGeometry,
        theme: &SliderThemeData,
        is_discrete: bool,
    ) {
        let track_height = theme.track_height.unwrap_or(0.0);
        if track_height <= 0.0 {
            return;
        }
        let gap = theme.track_gap.unwrap_or(0.0);
        debug_assert!(gap >= 0.0, "a negative track gap is not a gap");
        let Some((leading, trailing)) = track_paints(theme, geometry) else {
            return;
        };
        let track = geometry.track;
        let thumb = geometry.thumb_center.dx;
        let outer = Radius::circular(track.shortest_side() / 2.0);
        // Upstream's `trackInsideCornerRadius`, a constant 2: the corners
        // that face the gap are only slightly rounded, not stadium ends.
        let inner = Radius::circular(2.0);
        let ltr = geometry.direction == TextDirection::Ltr;

        let left = Rect::ltrb(
            track.left,
            track.top,
            track.left.max(thumb - gap),
            track.bottom,
        );
        let right = Rect::ltrb(thumb + gap, track.top, track.right, track.bottom);

        canvas.saved(|canvas| {
            // Upstream clips to the whole track, so a segment whose inside
            // corner radius would poke past the rounded end is trimmed.
            let corner = track.shortest_side() / 2.0;
            canvas.clip_rounded_rect(track, corner, corner, ClipOp::Intersect, true);
            if thumb > left.left + track_height / 2.0 {
                fill_rrect(
                    canvas,
                    left,
                    BorderRadius::only(outer, inner, outer, inner),
                    leading,
                );
            }
            if thumb < right.right - track_height / 2.0 {
                fill_rrect(
                    canvas,
                    right,
                    BorderRadius::only(inner, outer, inner, outer),
                    trailing,
                );
            }
            if let Some((rect, color)) = geometry.secondary_segment(theme, true) {
                // Both directions round the same pair: the segment gets
                // an inside corner where it meets the active track and a
                // full one at the far end, whichever way round those are.
                fill_rrect(
                    canvas,
                    rect,
                    BorderRadius::only(inner, outer, inner, outer),
                    color,
                );
            }
        });

        // The stop indicator: a dot at the far end, shown while the thumb has
        // not reached it, and only on a continuous slider -- a discrete one
        // already has a tick mark there.
        let trailing_space = track_height / 2.0;
        let (_, center_y) = track.center();
        let indicator_x = if ltr {
            track.right - trailing_space
        } else {
            track.left + trailing_space
        };
        let show = if ltr {
            thumb < indicator_x
        } else {
            thumb > indicator_x
        };
        if show && !is_discrete {
            if let Some(active) = enabled_color(
                theme.disabled_active_track_color,
                theme.active_track_color,
                geometry.enable,
            ) {
                canvas.draw_circle(indicator_x, center_y, 2.0, &Paint::new(active));
            }
        }
    }
}

impl SliderTrackShape {
    /// Upstream `getPreferredRect`, which every one of the three takes from
    /// [`BaseSliderTrackShape`].
    pub fn preferred_rect(
        &self,
        parent_size: Size,
        offset: Offset,
        theme: &SliderThemeData,
        is_enabled: bool,
    ) -> Rect {
        BaseSliderTrackShape::preferred_rect(parent_size, offset, theme, is_enabled)
    }

    /// Upstream `isRounded`, which is false on the base and overridden to
    /// true by the two rounded shapes.
    pub fn is_rounded(&self) -> bool {
        match self {
            SliderTrackShape::Rectangular(_) => false,
            SliderTrackShape::RoundedRect(shape) => shape.is_rounded(),
            SliderTrackShape::Gapped(shape) => shape.is_rounded(),
        }
    }

    /// Upstream `paint`.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        geometry: &TrackPaintGeometry,
        theme: &SliderThemeData,
        is_discrete: bool,
    ) {
        match self {
            SliderTrackShape::Rectangular(shape) => shape.paint(canvas, geometry, theme),
            SliderTrackShape::RoundedRect(shape) => shape.paint(canvas, geometry, theme, 2.0),
            SliderTrackShape::Gapped(shape) => shape.paint(canvas, geometry, theme, is_discrete),
        }
    }
}

/// What a track shape is told about the frame it is painting.
///
/// Upstream passes these one by one as named parameters -- the parent box,
/// the offset, the thumb's centre, the secondary offset, the text direction
/// and the two animations. They travel together and are gathered here, which
/// is the only structural change to the three `paint` methods.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackPaintGeometry {
    /// The rectangle from [`SliderTrackShape::preferred_rect`].
    pub track: Rect,
    pub thumb_center: Offset,
    /// Upstream's `secondaryOffset`: how far a buffered or secondary value
    /// has reached, which a media slider draws behind the thumb.
    pub secondary_offset: Option<Offset>,
    pub direction: TextDirection,
    /// The enable animation, already evaluated.
    pub enable: f32,
}

impl TrackPaintGeometry {
    pub fn new(track: Rect, thumb_center: Offset, direction: TextDirection, enable: f32) -> Self {
        TrackPaintGeometry {
            track,
            thumb_center,
            secondary_offset: None,
            direction,
            enable,
        }
    }

    pub fn with_secondary_offset(mut self, offset: Offset) -> Self {
        self.secondary_offset = Some(offset);
        self
    }

    /// The secondary segment and its colour, or nothing if there is no
    /// secondary value or it has not passed the thumb.
    ///
    /// `gapped` gives the Material 3 shape's rule, which measures from the
    /// far side of the gap rather than from the thumb's centre.
    fn secondary_segment(&self, theme: &SliderThemeData, gapped: bool) -> Option<(Rect, Color)> {
        let secondary = self.secondary_offset?;
        let gap = if gapped {
            theme.track_gap.unwrap_or(0.0)
        } else {
            0.0
        };
        let ltr = self.direction == TextDirection::Ltr;
        let thumb = self.thumb_center.dx;
        let shows = if ltr {
            secondary.dx > thumb + gap
        } else {
            secondary.dx < thumb - gap
        };
        if !shows {
            return None;
        }
        let color = enabled_color(
            theme.disabled_secondary_active_track_color,
            theme.secondary_active_track_color,
            self.enable,
        )?;
        let rect = if ltr {
            Rect::ltrb(thumb + gap, self.track.top, secondary.dx, self.track.bottom)
        } else {
            Rect::ltrb(secondary.dx - gap, self.track.top, thumb, self.track.bottom)
        };
        Some((rect, color))
    }
}

/// The leading and trailing colours of a track, which are the active and
/// inactive ones in a left-to-right slider and swapped in a right-to-left
/// one -- upstream's `(leftTrackPaint, rightTrackPaint)`.
fn track_paints(theme: &SliderThemeData, geometry: &TrackPaintGeometry) -> Option<(Color, Color)> {
    let active = enabled_color(
        theme.disabled_active_track_color,
        theme.active_track_color,
        geometry.enable,
    )?;
    let inactive = enabled_color(
        theme.disabled_inactive_track_color,
        theme.inactive_track_color,
        geometry.enable,
    )?;
    Some(match geometry.direction {
        TextDirection::Ltr => (active, inactive),
        TextDirection::Rtl => (inactive, active),
    })
}

/// Upstream's `ColorTween(begin: disabled, end: enabled).evaluate(animation)`,
/// with the null ends that upstream asserts away handled rather than asserted.
pub(crate) fn enabled_color(
    disabled: Option<Color>,
    enabled: Option<Color>,
    t: f32,
) -> Option<Color> {
    match (disabled, enabled) {
        (Some(disabled), Some(enabled)) => Some(ColorTween::new(disabled, enabled).lerp(t)),
        (first, second) => second.or(first),
    }
}

/// Fills a rounded rectangle whose corners are not all the same, which the
/// binding has no single call for -- the same hand-walked path
/// [`OutlineInputBorder`](crate::borders::OutlineInputBorder) draws.
pub(crate) fn fill_rrect(canvas: &mut Canvas, rect: Rect, radius: BorderRadius, color: Color) {
    if rect.width() <= 0.0 {
        return;
    }
    canvas.draw_path(&radius.to_rrect(rect).to_path(), &Paint::new(color));
}

// -- The theme (upstream `slider_theme.dart`) ---------------------------------

/// Upstream `SliderThemeData`: every colour and every shape a slider draws
/// itself with.
///
/// The largest of the component themes, because a slider is the control with
/// the most separately-coloured parts: each of track, tick mark and thumb has
/// an active, an inactive and a disabled colour, and the shapes that draw them
/// are swappable too.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SliderThemeData {
    pub track_height: Option<f32>,
    pub active_track_color: Option<Color>,
    pub inactive_track_color: Option<Color>,
    /// The part between the thumb and a secondary value -- what a media
    /// player draws its buffered range with.
    pub secondary_active_track_color: Option<Color>,
    pub disabled_active_track_color: Option<Color>,
    pub disabled_secondary_active_track_color: Option<Color>,
    pub disabled_inactive_track_color: Option<Color>,
    pub active_tick_mark_color: Option<Color>,
    pub inactive_tick_mark_color: Option<Color>,
    pub disabled_active_tick_mark_color: Option<Color>,
    pub disabled_inactive_tick_mark_color: Option<Color>,
    pub thumb_color: Option<Color>,
    /// The ring drawn round a range slider's thumbs while they overlap, so
    /// that two thumbs on the same value are still two.
    pub overlapping_shape_stroke_color: Option<Color>,
    pub disabled_thumb_color: Option<Color>,
    pub overlay_color: Option<Color>,
    pub value_indicator_color: Option<Color>,
    pub value_indicator_stroke_color: Option<Color>,
    pub overlay_shape: Option<SliderComponentShape>,
    pub tick_mark_shape: Option<SliderTickMarkShape>,
    pub thumb_shape: Option<SliderComponentShape>,
    pub track_shape: Option<SliderTrackShape>,
    pub value_indicator_shape: Option<SliderComponentShape>,
    pub range_tick_mark_shape: Option<crate::range_slider_parts::RangeSliderTickMarkShape>,
    pub range_thumb_shape: Option<crate::range_slider_parts::RangeSliderThumbShape>,
    pub range_track_shape: Option<crate::range_slider_parts::RangeSliderTrackShape>,
    pub range_value_indicator_shape: Option<RangeSliderValueIndicatorShape>,
    pub show_value_indicator: Option<ShowValueIndicator>,
    pub value_indicator_text_style: Option<TextStyle>,
    /// How close a range slider's two thumbs may come.
    pub min_thumb_separation: Option<f32>,
    /// Which thumb a tap moves. Upstream's default keeps the two from
    /// crossing; an application that wants them to cross replaces it,
    /// which is why this is a function and not a rule.
    pub thumb_selector: Option<crate::range_slider_parts::RangeThumbSelector>,
    pub mouse_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
    pub allowed_interaction: Option<SliderInteraction>,
    pub padding: Option<EdgeInsetsGeometry>,
    /// Material 3's thumb narrows while it is dragged, which is why the size
    /// is a state property and not a number.
    pub thumb_size: Option<StateProperty<Option<Size>>>,
    pub track_gap: Option<f32>,
    /// Upstream's opt-out from the 2024 Material 3 look, which it will remove
    /// once the new one is the only one.
    pub year_2023: Option<bool>,
}

impl SliderThemeData {
    pub fn new() -> SliderThemeData {
        SliderThemeData::default()
    }

    /// Upstream `SliderThemeData.fromPrimaryColors`: the Material 2 defaults,
    /// which are the three primary colours at fixed opacities.
    ///
    /// The alphas are upstream's constants, kept as the hex they are written
    /// in there so that the table can be read against it.
    pub fn from_primary_colors(
        primary_color: Color,
        primary_color_dark: Color,
        primary_color_light: Color,
        value_indicator_text_style: TextStyle,
    ) -> SliderThemeData {
        SliderThemeData {
            track_height: Some(2.0),
            active_track_color: Some(primary_color.with_alpha(0xff)),
            inactive_track_color: Some(primary_color.with_alpha(0x3d)),
            secondary_active_track_color: Some(primary_color.with_alpha(0x8a)),
            disabled_active_track_color: Some(primary_color_dark.with_alpha(0x52)),
            disabled_inactive_track_color: Some(primary_color_dark.with_alpha(0x1f)),
            disabled_secondary_active_track_color: Some(primary_color_dark.with_alpha(0x1f)),
            active_tick_mark_color: Some(primary_color_light.with_alpha(0x8a)),
            inactive_tick_mark_color: Some(primary_color.with_alpha(0x8a)),
            disabled_active_tick_mark_color: Some(primary_color_light.with_alpha(0x1f)),
            disabled_inactive_tick_mark_color: Some(primary_color_dark.with_alpha(0x1f)),
            thumb_color: Some(primary_color.with_alpha(0xff)),
            overlapping_shape_stroke_color: Some(Color::WHITE),
            disabled_thumb_color: Some(primary_color_dark.with_alpha(0x52)),
            overlay_color: Some(primary_color.with_alpha(0x1f)),
            value_indicator_color: Some(primary_color.with_alpha(0xff)),
            value_indicator_stroke_color: Some(primary_color.with_alpha(0xff)),
            overlay_shape: Some(SliderComponentShape::RoundOverlay(
                RoundSliderOverlayShape::new(),
            )),
            tick_mark_shape: Some(SliderTickMarkShape::Round(RoundSliderTickMarkShape::new())),
            thumb_shape: Some(SliderComponentShape::RoundThumb(
                RoundSliderThumbShape::new(),
            )),
            track_shape: Some(SliderTrackShape::RoundedRect(
                RoundedRectSliderTrackShape::new(),
            )),
            value_indicator_shape: Some(SliderComponentShape::PaddleIndicator(
                PaddleSliderValueIndicatorShape::new(),
            )),
            range_tick_mark_shape: Some(
                crate::range_slider_parts::RangeSliderTickMarkShape::Round(
                    crate::range_slider_parts::RoundRangeSliderTickMarkShape::new(),
                ),
            ),
            range_thumb_shape: Some(crate::range_slider_parts::RangeSliderThumbShape::Round(
                crate::range_slider_parts::RoundRangeSliderThumbShape::new(),
            )),
            range_track_shape: Some(
                crate::range_slider_parts::RangeSliderTrackShape::RoundedRect(
                    crate::range_slider_parts::RoundedRectRangeSliderTrackShape::new(),
                ),
            ),
            range_value_indicator_shape: Some(RangeSliderValueIndicatorShape::Paddle(
                PaddleRangeSliderValueIndicatorShape::new(),
            )),
            value_indicator_text_style: Some(value_indicator_text_style),
            show_value_indicator: Some(ShowValueIndicator::OnlyForDiscrete),
            ..SliderThemeData::default()
        }
    }

    pub fn with_track_height(mut self, height: f32) -> Self {
        self.track_height = Some(height);
        self
    }

    pub fn with_track_shape(mut self, shape: SliderTrackShape) -> Self {
        self.track_shape = Some(shape);
        self
    }

    pub fn with_thumb_shape(mut self, shape: SliderComponentShape) -> Self {
        self.thumb_shape = Some(shape);
        self
    }

    pub fn with_track_colors(mut self, active: Color, inactive: Color) -> Self {
        self.active_track_color = Some(active);
        self.inactive_track_color = Some(inactive);
        self
    }

    pub fn with_track_gap(mut self, gap: f32) -> Self {
        self.track_gap = Some(gap);
        self
    }

    /// Upstream `SliderThemeData.lerp`.
    ///
    /// Every colour and number interpolates; every shape and every enum takes
    /// the nearer end, because a shape half-way between a circle and a bar is
    /// not a shape.
    ///
    /// Three fields are neither, and this port had all three stepping when
    /// upstream blends them: `padding` goes through
    /// `EdgeInsetsGeometry.lerp`, `thumb_size` through
    /// `WidgetStateProperty.lerp<Size?>(..., Size.lerp)`, and
    /// `value_indicator_text_style` through `TextStyle.lerp`. A stepping
    /// padding jumps the whole track sideways at the midpoint of a theme
    /// transition; a stepping thumb size jumps the thumb.
    pub fn lerp(a: &SliderThemeData, b: &SliderThemeData, t: f32) -> SliderThemeData {
        SliderThemeData {
            track_height: lerp_f32(a.track_height, b.track_height, t),
            active_track_color: lerp_color(a.active_track_color, b.active_track_color, t),
            inactive_track_color: lerp_color(a.inactive_track_color, b.inactive_track_color, t),
            secondary_active_track_color: lerp_color(
                a.secondary_active_track_color,
                b.secondary_active_track_color,
                t,
            ),
            disabled_active_track_color: lerp_color(
                a.disabled_active_track_color,
                b.disabled_active_track_color,
                t,
            ),
            disabled_inactive_track_color: lerp_color(
                a.disabled_inactive_track_color,
                b.disabled_inactive_track_color,
                t,
            ),
            disabled_secondary_active_track_color: lerp_color(
                a.disabled_secondary_active_track_color,
                b.disabled_secondary_active_track_color,
                t,
            ),
            active_tick_mark_color: lerp_color(
                a.active_tick_mark_color,
                b.active_tick_mark_color,
                t,
            ),
            inactive_tick_mark_color: lerp_color(
                a.inactive_tick_mark_color,
                b.inactive_tick_mark_color,
                t,
            ),
            disabled_active_tick_mark_color: lerp_color(
                a.disabled_active_tick_mark_color,
                b.disabled_active_tick_mark_color,
                t,
            ),
            disabled_inactive_tick_mark_color: lerp_color(
                a.disabled_inactive_tick_mark_color,
                b.disabled_inactive_tick_mark_color,
                t,
            ),
            thumb_color: lerp_color(a.thumb_color, b.thumb_color, t),
            overlapping_shape_stroke_color: lerp_color(
                a.overlapping_shape_stroke_color,
                b.overlapping_shape_stroke_color,
                t,
            ),
            disabled_thumb_color: lerp_color(a.disabled_thumb_color, b.disabled_thumb_color, t),
            overlay_color: lerp_color(a.overlay_color, b.overlay_color, t),
            value_indicator_color: lerp_color(a.value_indicator_color, b.value_indicator_color, t),
            value_indicator_stroke_color: lerp_color(
                a.value_indicator_stroke_color,
                b.value_indicator_stroke_color,
                t,
            ),
            overlay_shape: lerp_nearer(&a.overlay_shape, &b.overlay_shape, t),
            tick_mark_shape: lerp_nearer(&a.tick_mark_shape, &b.tick_mark_shape, t),
            thumb_shape: lerp_nearer(&a.thumb_shape, &b.thumb_shape, t),
            track_shape: lerp_nearer(&a.track_shape, &b.track_shape, t),
            value_indicator_shape: lerp_nearer(
                &a.value_indicator_shape,
                &b.value_indicator_shape,
                t,
            ),
            show_value_indicator: lerp_nearer(&a.show_value_indicator, &b.show_value_indicator, t),
            range_tick_mark_shape: lerp_nearer(
                &a.range_tick_mark_shape,
                &b.range_tick_mark_shape,
                t,
            ),
            range_thumb_shape: lerp_nearer(&a.range_thumb_shape, &b.range_thumb_shape, t),
            range_track_shape: lerp_nearer(&a.range_track_shape, &b.range_track_shape, t),
            range_value_indicator_shape: lerp_nearer(
                &a.range_value_indicator_shape,
                &b.range_value_indicator_shape,
                t,
            ),

            value_indicator_text_style: match (
                &a.value_indicator_text_style,
                &b.value_indicator_text_style,
            ) {
                (Some(first), Some(second)) => Some(TextStyle::lerp(first, second, t)),
                (first, second) => {
                    if t < 0.5 {
                        first.clone()
                    } else {
                        second.clone()
                    }
                }
            },
            min_thumb_separation: lerp_f32(a.min_thumb_separation, b.min_thumb_separation, t),
            thumb_selector: lerp_nearer(&a.thumb_selector, &b.thumb_selector, t),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
            allowed_interaction: lerp_nearer(&a.allowed_interaction, &b.allowed_interaction, t),
            padding: EdgeInsetsGeometry::lerp(a.padding, b.padding, t),
            thumb_size: lerp_state_property(
                a.thumb_size.as_ref(),
                b.thumb_size.as_ref(),
                t,
                |first, second, t| lerp_size(first.flatten(), second.flatten(), t),
            ),
            track_gap: lerp_f32(a.track_gap, b.track_gap, t),
            year_2023: lerp_nearer(&a.year_2023, &b.year_2023, t),
        }
    }
}

/// Upstream `Size.lerp`, whose two null arms are not "hold the other end":
/// a missing end scales the present size by `t` (or `1 - t`), so a thumb that
/// appears grows out of nothing rather than springing to full size.
fn lerp_size(a: Option<Size>, b: Option<Size>, t: f32) -> Option<Size> {
    match (a, b) {
        (None, None) => None,
        (None, Some(b)) => Some(Size::new(b.width * t, b.height * t)),
        (Some(a), None) => Some(Size::new(a.width * (1.0 - t), a.height * (1.0 - t))),
        (Some(a), Some(b)) => Some(<Size as crate::implicit::Lerp>::lerp(a, b, t)),
    }
}

/// Upstream `SliderTheme`.
pub struct SliderTheme;

impl SliderTheme {
    pub fn new(data: SliderThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> SliderThemeData {
        context
            .inherited::<SliderThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).slider_theme.clone())
    }
}

// -- Value indicators (upstream `slider_value_indicator_shape.dart` and the
//    two in `slider_parts.dart`) ----------------------------------------------

/// What every value indicator is told about the frame it is painting.
///
/// Upstream passes these as named parameters on each painter's `paint`. They
/// travel together and are gathered here, which is the only structural change
/// to the four painters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndicatorPaintGeometry {
    /// Where the thumb is, in the slider's own coordinates.
    pub center: Offset,
    /// The same point in the coordinates `size_with_overflow` is measured in
    /// -- upstream's `parentBox.localToGlobal(center)`. The two differ only
    /// when the slider is not at the origin of the box the bubble may spill
    /// into, which is why upstream asks for both.
    pub global_center: Offset,
    /// The box the bubble is allowed to spill into: upstream's
    /// `sizeWithOverflow`, which is the whole slider including the room the
    /// indicator is given above it.
    pub size_with_overflow: Size,
    /// The activation animation, already evaluated. Zero means the indicator
    /// is not showing, and every painter returns without drawing.
    pub scale: f32,
    pub text_scale_factor: f32,
}

impl IndicatorPaintGeometry {
    pub fn new(center: Offset, size_with_overflow: Size, scale: f32) -> IndicatorPaintGeometry {
        IndicatorPaintGeometry {
            center,
            global_center: center,
            size_with_overflow,
            scale,
            text_scale_factor: 1.0,
        }
    }

    pub fn with_global_center(mut self, center: Offset) -> Self {
        self.global_center = center;
        self
    }

    pub fn with_text_scale_factor(mut self, factor: f32) -> Self {
        self.text_scale_factor = factor;
        self
    }
}

/// Upstream's shared `getHorizontalShift`, which three of the four painters
/// have a copy of.
///
/// The bubble is pushed towards the middle so that it does not hang off the
/// end of the slider. A negative answer moves it left, a positive one right.
/// When the bubble is wider than the whole slider it cannot be kept inside,
/// and upstream then pins whichever edge is overflowing further instead.
pub(crate) fn indicator_horizontal_shift(
    rectangle_width: f32,
    geometry: &IndicatorPaintGeometry,
) -> f32 {
    const EDGE_PADDING: f32 = 8.0;
    let global_x = geometry.global_center.dx;
    let overflow_left = (rectangle_width / 2.0 - global_x + EDGE_PADDING).max(0.0);
    let overflow_right = (rectangle_width / 2.0
        - (geometry.size_with_overflow.width - global_x - EDGE_PADDING))
        .max(0.0);

    if rectangle_width < geometry.size_with_overflow.width {
        overflow_left - overflow_right
    } else if overflow_left - overflow_right > 0.0 {
        overflow_left - EDGE_PADDING * geometry.text_scale_factor
    } else {
        -overflow_right + EDGE_PADDING * geometry.text_scale_factor
    }
}

/// The one-pixel outline upstream draws when the theme names a stroke colour.
pub(crate) fn indicator_stroke(color: Color) -> Paint {
    Paint::new(color).with_style(Style::Stroke { width: 1.0 })
}

/// Upstream's `_RectangularSliderValueIndicatorPathPainter`.
pub(crate) struct RectangularIndicatorPainter;

impl RectangularIndicatorPainter {
    pub(crate) const TRIANGLE_HEIGHT: f32 = 8.0;
    pub(crate) const LABEL_PADDING: f32 = 16.0;
    pub(crate) const PREFERRED_HEIGHT: f32 = 32.0;
    pub(crate) const MIN_LABEL_WIDTH: f32 = 16.0;
    pub(crate) const BOTTOM_TIP_Y_OFFSET: f32 = 14.0;
    pub(crate) const UPPER_RECT_RADIUS: f32 = 4.0;

    pub(crate) fn upper_rectangle_width(label_width: f32, scale: f32, text_scale: f32) -> f32 {
        let unscaled =
            (Self::MIN_LABEL_WIDTH * text_scale).max(label_width) + Self::LABEL_PADDING * 2.0;
        unscaled * scale
    }

    pub(crate) fn preferred_size(label_width: f32, label_height: f32, text_scale: f32) -> Size {
        Size::new(
            Self::upper_rectangle_width(label_width, 1.0, text_scale),
            label_height + Self::LABEL_PADDING,
        )
    }

    pub(crate) fn paint(
        canvas: &mut Canvas,
        geometry: &IndicatorPaintGeometry,
        label: &TextPainter,
        background: Color,
        stroke: Option<Color>,
    ) {
        if geometry.scale == 0.0 {
            return;
        }
        let width =
            Self::upper_rectangle_width(label.width(), geometry.scale, geometry.text_scale_factor);
        let shift = indicator_horizontal_shift(width, geometry);
        let rect_height = label.height() + Self::LABEL_PADDING;
        let upper = Rect::xywh(
            -width / 2.0 + shift,
            -Self::TRIANGLE_HEIGHT - rect_height,
            width,
            rect_height,
        );

        let mut path = RenderPath::new();
        // Upstream starts the triangle at the origin implicitly: a fresh
        // `Path` is at (0, 0), and the first `lineTo` draws from there.
        path.move_to(0.0, 0.0);
        path.line_to(-Self::TRIANGLE_HEIGHT, -Self::TRIANGLE_HEIGHT);
        path.line_to(Self::TRIANGLE_HEIGHT, -Self::TRIANGLE_HEIGHT);
        path.close();
        BorderRadius::circular(Self::UPPER_RECT_RADIUS)
            .to_rrect(upper)
            .append_to(&mut path);

        canvas.saved(|canvas| {
            // The bubble is drawn relative to the thumb's centre, a little
            // above it -- the tip of the triangle sits on the track.
            canvas.translate(
                geometry.center.dx,
                geometry.center.dy - Self::BOTTOM_TIP_Y_OFFSET,
            );
            canvas.scale(geometry.scale, geometry.scale);
            if let Some(color) = stroke {
                canvas.draw_path(&path, &indicator_stroke(color));
            }
            canvas.draw_path(&path, &Paint::new(background));
            canvas.translate(0.0, -Self::PREFERRED_HEIGHT / 4.0 - upper.height());
            let label_offset = (
                shift - label.width() / 2.0,
                upper.height() / 2.0 - label.height() / 2.0,
            );
            label.paint(canvas, label_offset);
        });
    }
}

/// Upstream's `_RoundedRectSliderValueIndicatorPathPainter`.
pub(crate) struct RoundedRectIndicatorPainter;

impl RoundedRectIndicatorPainter {
    pub(crate) const LABEL_PADDING: f32 = 10.0;
    pub(crate) const PREFERRED_HEIGHT: f32 = 32.0;
    pub(crate) const MIN_LABEL_WIDTH: f32 = 16.0;
    pub(crate) const RECT_Y_OFFSET: f32 = 10.0;
    pub(crate) const BOTTOM_TIP_Y_OFFSET: f32 = 16.0;

    pub(crate) fn upper_rectangle_width(label_width: f32, scale: f32) -> f32 {
        (Self::MIN_LABEL_WIDTH.max(label_width) + Self::LABEL_PADDING * 2.0) * scale
    }

    pub(crate) fn preferred_size(label_width: f32, text_scale: f32) -> Size {
        Size::new(
            Self::MIN_LABEL_WIDTH.max(label_width) + Self::LABEL_PADDING * 2.0 * text_scale,
            Self::PREFERRED_HEIGHT * text_scale,
        )
    }

    pub(crate) fn paint(
        canvas: &mut Canvas,
        geometry: &IndicatorPaintGeometry,
        label: &TextPainter,
        background: Color,
        stroke: Option<Color>,
    ) {
        if geometry.scale == 0.0 {
            return;
        }
        let width = Self::upper_rectangle_width(label.width(), geometry.scale);
        let shift = indicator_horizontal_shift(width, geometry);
        let upper = Rect::xywh(
            -width / 2.0 + shift,
            -Self::RECT_Y_OFFSET - Self::PREFERRED_HEIGHT,
            width,
            Self::PREFERRED_HEIGHT,
        );

        canvas.saved(|canvas| {
            canvas.translate(
                geometry.center.dx,
                geometry.center.dy - Self::BOTTOM_TIP_Y_OFFSET,
            );
            canvas.scale(geometry.scale, geometry.scale);
            let radius = upper.height() / 2.0;
            if let Some(color) = stroke {
                canvas.draw_rounded_rect(upper, radius, &indicator_stroke(color));
            }
            canvas.draw_rounded_rect(upper, radius, &Paint::new(background));
            canvas.translate(0.0, -Self::PREFERRED_HEIGHT / 4.0 - upper.height());
            // Upstream divides the height by 2.3 rather than 2 here, and by
            // 1.75 in the drop shape: the label sits a little above centre in
            // both, and the two numbers are theirs.
            let label_offset = (
                shift - label.width() / 2.0,
                upper.height() / 2.3 - label.height() / 2.0,
            );
            label.paint(canvas, label_offset);
        });
    }
}

/// Upstream's `_DropSliderValueIndicatorPathPainter`.
pub(crate) struct DropIndicatorPainter;

impl DropIndicatorPainter {
    pub(crate) const TRIANGLE_HEIGHT: f32 = 10.0;
    pub(crate) const LABEL_PADDING: f32 = 8.0;
    pub(crate) const PREFERRED_HEIGHT: f32 = 32.0;
    pub(crate) const MIN_LABEL_WIDTH: f32 = 20.0;
    pub(crate) const MIN_RECT_HEIGHT: f32 = 28.0;
    pub(crate) const RECT_Y_OFFSET: f32 = 6.0;
    pub(crate) const BOTTOM_TIP_Y_OFFSET: f32 = 16.0;
    pub(crate) const UPPER_RECT_RADIUS: f32 = 4.0;

    pub(crate) fn upper_rectangle_width(label_width: f32, scale: f32) -> f32 {
        (Self::MIN_LABEL_WIDTH.max(label_width) + Self::LABEL_PADDING) * scale
    }

    pub(crate) fn preferred_size(label_width: f32, text_scale: f32) -> Size {
        Size::new(
            Self::MIN_LABEL_WIDTH.max(label_width) + Self::LABEL_PADDING * 2.0 * text_scale,
            Self::PREFERRED_HEIGHT * text_scale,
        )
    }

    /// Upstream's `_adjustBorderRadius`, which lerps from the 4px corner to a
    /// fully round one at `1.0 - rectness` where `rectness` is the constant
    /// zero -- so it is always the round end. It is kept as upstream wrote it
    /// because the constant is where upstream left the knob.
    pub(crate) fn adjusted_border_radius(rect: Rect) -> BorderRadius {
        const RECTNESS: f32 = 0.0;
        BorderRadius::lerp(
            BorderRadius::circular(Self::UPPER_RECT_RADIUS),
            BorderRadius::circular(rect.shortest_side() / 2.0),
            1.0 - RECTNESS,
        )
    }

    pub(crate) fn paint(
        canvas: &mut Canvas,
        geometry: &IndicatorPaintGeometry,
        label: &TextPainter,
        background: Color,
        stroke: Option<Color>,
    ) {
        if geometry.scale == 0.0 {
            return;
        }
        let width = Self::upper_rectangle_width(label.width(), geometry.scale);
        let shift = indicator_horizontal_shift(width, geometry);
        let upper = Rect::xywh(
            -width / 2.0 + shift,
            -Self::RECT_Y_OFFSET - Self::MIN_RECT_HEIGHT,
            width,
            Self::MIN_RECT_HEIGHT,
        );

        canvas.saved(|canvas| {
            canvas.translate(
                geometry.center.dx,
                geometry.center.dy - Self::BOTTOM_TIP_Y_OFFSET,
            );
            canvas.scale(geometry.scale, geometry.scale);

            let mut path = RenderPath::new();
            path.move_to(0.0, 0.0);
            path.line_to(-Self::TRIANGLE_HEIGHT, -Self::TRIANGLE_HEIGHT);
            path.line_to(Self::TRIANGLE_HEIGHT, -Self::TRIANGLE_HEIGHT);
            path.close();
            Self::adjusted_border_radius(upper)
                .to_rrect(upper)
                .append_to(&mut path);

            if let Some(color) = stroke {
                canvas.draw_path(&path, &indicator_stroke(color));
            }
            canvas.draw_path(&path, &Paint::new(background));
            canvas.translate(0.0, -Self::PREFERRED_HEIGHT / 4.0 - upper.height());
            let label_offset = (
                shift - label.width() / 2.0,
                upper.height() / 1.75 - label.height() / 2.0,
            );
            label.paint(canvas, label_offset);
        });
    }
}

/// Upstream's `_PaddleSliderValueIndicatorPathPainter`: the Material 2
/// bubble, which is two circles joined by a waisted neck.
///
/// The shape changes with the label: the top lobe spreads sideways as the
/// text grows, and the arcs on the neck move down to keep meeting it
/// smoothly. That is why it is arcs rather than a rounded rectangle, and why
/// the arithmetic below is trigonometry rather than a table.
pub(crate) struct PaddleIndicatorPainter;

impl PaddleIndicatorPainter {
    pub(crate) const TOP_LOBE_RADIUS: f32 = 16.0;
    pub(crate) const MIN_LABEL_WIDTH: f32 = 16.0;
    pub(crate) const BOTTOM_LOBE_RADIUS: f32 = 10.0;
    pub(crate) const LABEL_PADDING: f32 = 8.0;
    pub(crate) const DISTANCE_BETWEEN_TOP_BOTTOM_CENTERS: f32 = 40.0;
    pub(crate) const MIDDLE_NECK_WIDTH: f32 = 3.0;
    pub(crate) const BOTTOM_NECK_RADIUS: f32 = 4.5;
    pub(crate) const TOP_NECK_RADIUS: f32 = 13.0;
    /// The base of the triangle between the top lobe's centre and the two top
    /// neck arcs' centres.
    pub(crate) const NECK_TRIANGLE_BASE: f32 =
        Self::TOP_NECK_RADIUS + Self::MIDDLE_NECK_WIDTH / 2.0;
    pub(crate) const RIGHT_BOTTOM_NECK_CENTER_X: f32 =
        Self::MIDDLE_NECK_WIDTH / 2.0 + Self::BOTTOM_NECK_RADIUS;
    pub(crate) const NECK_TRIANGLE_HYPOTENUSE: f32 = Self::TOP_LOBE_RADIUS + Self::TOP_NECK_RADIUS;
    pub(crate) const PREFERRED_HEIGHT: f32 = Self::DISTANCE_BETWEEN_TOP_BOTTOM_CENTERS
        + Self::TOP_LOBE_RADIUS
        + Self::BOTTOM_LOBE_RADIUS;
    pub(crate) const TOP_LOBE_CENTER_Y: f32 = -Self::DISTANCE_BETWEEN_TOP_BOTTOM_CENTERS;

    pub(crate) fn preferred_size(label_width: f32, text_scale: f32) -> Size {
        Size::new(
            (Self::MIN_LABEL_WIDTH * text_scale).max(label_width)
                + Self::LABEL_PADDING * 2.0 * text_scale,
            Self::PREFERRED_HEIGHT * text_scale,
        )
    }

    pub(crate) fn arc(path: &mut RenderPath, cx: f32, cy: f32, radius: f32, start: f32, end: f32) {
        path.arc_to(Rect::from_circle(cx, cy, radius), start, end - start, false);
    }

    /// Upstream's `_getIdealOffset`: how far sideways the bubble would like
    /// to move to stay on the slider, bounded by how far the paddle can
    /// stretch without tearing.
    pub(crate) fn ideal_offset(
        half_width_needed: f32,
        scale: f32,
        center: Offset,
        width_with_overflow: f32,
    ) -> f32 {
        const EDGE_MARGIN: f32 = 8.0;
        let left = -Self::TOP_LOBE_RADIUS - half_width_needed;
        let right = Self::TOP_LOBE_RADIUS + half_width_needed;
        // Scaling around the origin is a multiplication, which is why
        // upstream does not build a transform here.
        let top_left_x = left * scale + center.dx;
        let bottom_right_x = right * scale + center.dx;
        let mut shift = 0.0;
        if top_left_x < EDGE_MARGIN {
            shift = EDGE_MARGIN - top_left_x;
        }
        if bottom_right_x > width_with_overflow - EDGE_MARGIN {
            shift = width_with_overflow - EDGE_MARGIN - bottom_right_x;
        }
        shift = if scale == 0.0 { 0.0 } else { shift / scale };
        if shift < 0.0 {
            shift.max(-half_width_needed)
        } else {
            shift.min(half_width_needed)
        }
    }

    pub(crate) fn horizontal_shift(label: &TextPainter, geometry: &IndicatorPaintGeometry) -> f32 {
        let text_scale = geometry.text_scale_factor;
        let inverse_text_scale = if text_scale != 0.0 {
            1.0 / text_scale
        } else {
            0.0
        };
        let half_width_needed = (inverse_text_scale * label.width() / 2.0
            - (Self::TOP_LOBE_RADIUS - Self::LABEL_PADDING))
            .max(0.0);
        let shift = Self::ideal_offset(
            half_width_needed,
            text_scale * geometry.scale,
            geometry.global_center,
            geometry.size_with_overflow.width,
        );
        shift * text_scale
    }

    pub(crate) fn paint(
        canvas: &mut Canvas,
        geometry: &IndicatorPaintGeometry,
        label: &TextPainter,
        background: Color,
        stroke: Option<Color>,
    ) {
        if geometry.scale == 0.0 {
            // Upstream's note: zero scale draws nothing, and stopping here
            // also keeps the divisions below from sending NaNs to the engine.
            return;
        }
        let text_scale = geometry.text_scale_factor;
        let overall_scale = geometry.scale * text_scale;
        let inverse_text_scale = if text_scale != 0.0 {
            1.0 / text_scale
        } else {
            0.0
        };
        let label_half_width = label.width() / 2.0;

        canvas.saved(|canvas| {
            canvas.translate(geometry.center.dx, geometry.center.dy);
            canvas.scale(overall_scale, overall_scale);

            // The bottom lobe keeps its size on screen as the paddle scales,
            // which is why its radius is divided by the scale here.
            let bottom_lobe_radius = Self::BOTTOM_LOBE_RADIUS / overall_scale;
            let hypotenuse = Self::BOTTOM_NECK_RADIUS + bottom_lobe_radius;
            let right_bottom_neck_center_y = -(hypotenuse * hypotenuse
                - Self::RIGHT_BOTTOM_NECK_CENTER_X * Self::RIGHT_BOTTOM_NECK_CENTER_X)
                .max(0.0)
                .sqrt();
            let pi = std::f32::consts::PI;
            let right_bottom_neck_angle_end =
                pi + (right_bottom_neck_center_y / Self::RIGHT_BOTTOM_NECK_CENTER_X).atan();

            let mut path = RenderPath::new();
            path.move_to(Self::MIDDLE_NECK_WIDTH / 2.0, right_bottom_neck_center_y);
            Self::arc(
                &mut path,
                Self::RIGHT_BOTTOM_NECK_CENTER_X,
                right_bottom_neck_center_y,
                Self::BOTTOM_NECK_RADIUS,
                pi,
                right_bottom_neck_angle_end,
            );
            Self::arc(
                &mut path,
                0.0,
                0.0,
                bottom_lobe_radius,
                right_bottom_neck_angle_end - pi,
                2.0 * pi - right_bottom_neck_angle_end,
            );
            Self::arc(
                &mut path,
                -Self::RIGHT_BOTTOM_NECK_CENTER_X,
                right_bottom_neck_center_y,
                Self::BOTTOM_NECK_RADIUS,
                pi - right_bottom_neck_angle_end,
                0.0,
            );

            let half_width_needed = (inverse_text_scale * label_half_width
                - (Self::TOP_LOBE_RADIUS - Self::LABEL_PADDING))
                .max(0.0);
            let shift = Self::ideal_offset(
                half_width_needed,
                overall_scale,
                geometry.global_center,
                geometry.size_with_overflow.width,
            );
            let left_width_needed = half_width_needed - shift;
            let right_width_needed = half_width_needed + shift;

            // How far each side of the neck has spread, as a fraction of the
            // room it has; at one the arc has flattened out entirely.
            let left_amount = (left_width_needed / Self::NECK_TRIANGLE_BASE).clamp(0.0, 1.0);
            let right_amount = (right_width_needed / Self::NECK_TRIANGLE_BASE).clamp(0.0, 1.0);
            let thirty_degrees = pi / 6.0;
            let ninety_degrees = pi / 2.0;
            let two_seventy_degrees = 3.0 * pi / 2.0;
            let left_theta = (1.0 - left_amount) * thirty_degrees;
            let right_theta = (1.0 - right_amount) * thirty_degrees;
            let left_top_neck_center_y =
                Self::TOP_LOBE_CENTER_Y + left_theta.cos() * Self::NECK_TRIANGLE_HYPOTENUSE;
            let right_top_neck_center_y =
                Self::TOP_LOBE_CENTER_Y + right_theta.cos() * Self::NECK_TRIANGLE_HYPOTENUSE;
            let left_neck_arc_angle = ninety_degrees - left_theta;
            let right_neck_arc_angle = pi + ninety_degrees - right_theta;

            // The neck is pulled down to meet the bottom lobe when the text
            // is small, and lets go as the text scale grows -- the cube is
            // upstream's, and it is what makes the release feel abrupt rather
            // than linear.
            let neck_stretch_baseline = (right_bottom_neck_center_y
                - left_top_neck_center_y.max(right_top_neck_center_y))
            .max(0.0);
            let t = inverse_text_scale.powf(3.0);
            let stretch = (neck_stretch_baseline * t).clamp(0.0, 10.0 * neck_stretch_baseline);
            let neck_stretch_y = neck_stretch_baseline - stretch;

            Self::arc(
                &mut path,
                -Self::NECK_TRIANGLE_BASE,
                left_top_neck_center_y + neck_stretch_y,
                Self::TOP_NECK_RADIUS,
                0.0,
                -left_neck_arc_angle,
            );
            Self::arc(
                &mut path,
                -left_width_needed,
                Self::TOP_LOBE_CENTER_Y + neck_stretch_y,
                Self::TOP_LOBE_RADIUS,
                ninety_degrees + left_theta,
                two_seventy_degrees,
            );
            Self::arc(
                &mut path,
                right_width_needed,
                Self::TOP_LOBE_CENTER_Y + neck_stretch_y,
                Self::TOP_LOBE_RADIUS,
                two_seventy_degrees,
                two_seventy_degrees + pi - right_theta,
            );
            Self::arc(
                &mut path,
                Self::NECK_TRIANGLE_BASE,
                right_top_neck_center_y + neck_stretch_y,
                Self::TOP_NECK_RADIUS,
                right_neck_arc_angle,
                pi,
            );

            if let Some(color) = stroke {
                canvas.draw_path(&path, &indicator_stroke(color));
            }
            canvas.draw_path(&path, &Paint::new(background));

            canvas.saved(|canvas| {
                canvas.translate(
                    shift,
                    -Self::DISTANCE_BETWEEN_TOP_BOTTOM_CENTERS + neck_stretch_y,
                );
                // The label is drawn at its own scale inside a paddle that is
                // drawn at the text scale, so it is unscaled again here.
                canvas.scale(inverse_text_scale, inverse_text_scale);
                label.paint(canvas, (-label_half_width, -label.height() / 2.0));
            });
        });
    }
}

/// Upstream `RectangularSliderValueIndicatorShape`: a rounded rectangle over
/// a small triangle pointing at the thumb.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectangularSliderValueIndicatorShape;

impl RectangularSliderValueIndicatorShape {
    pub fn new() -> RectangularSliderValueIndicatorShape {
        RectangularSliderValueIndicatorShape
    }

    pub fn preferred_size(&self, label: &TextPainter, text_scale: f32) -> Size {
        RectangularIndicatorPainter::preferred_size(label.width(), label.height(), text_scale)
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
        RectangularIndicatorPainter::paint(
            canvas,
            geometry,
            label,
            background,
            theme.value_indicator_stroke_color,
        );
    }
}

/// Upstream `RoundedRectSliderValueIndicatorShape`: Material 3's, a stadium
/// with no tail at all.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundedRectSliderValueIndicatorShape;

impl RoundedRectSliderValueIndicatorShape {
    pub fn new() -> RoundedRectSliderValueIndicatorShape {
        RoundedRectSliderValueIndicatorShape
    }

    pub fn preferred_size(&self, label: &TextPainter, text_scale: f32) -> Size {
        RoundedRectIndicatorPainter::preferred_size(label.width(), text_scale)
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

/// Upstream `DropSliderValueIndicatorShape`: a rounded rectangle whose tail
/// is a wider triangle, so the whole reads as a drop.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DropSliderValueIndicatorShape;

impl DropSliderValueIndicatorShape {
    pub fn new() -> DropSliderValueIndicatorShape {
        DropSliderValueIndicatorShape
    }

    pub fn preferred_size(&self, label: &TextPainter, text_scale: f32) -> Size {
        DropIndicatorPainter::preferred_size(label.width(), text_scale)
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

/// Upstream `PaddleSliderValueIndicatorShape`: Material 2's, two circles
/// joined by a waisted neck that stretches as the label grows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaddleSliderValueIndicatorShape;

impl PaddleSliderValueIndicatorShape {
    pub fn new() -> PaddleSliderValueIndicatorShape {
        PaddleSliderValueIndicatorShape
    }

    pub fn preferred_size(&self, label: &TextPainter, text_scale: f32) -> Size {
        PaddleIndicatorPainter::preferred_size(label.width(), text_scale)
    }

    /// Upstream's `getHorizontalShift`, which the slider asks for separately
    /// so that it knows where the bubble will land before it paints.
    pub fn horizontal_shift(&self, label: &TextPainter, geometry: &IndicatorPaintGeometry) -> f32 {
        PaddleIndicatorPainter::horizontal_shift(label, geometry)
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
        PaddleIndicatorPainter::paint(
            canvas,
            geometry,
            label,
            background,
            theme.value_indicator_stroke_color,
        );
    }
}

/// Upstream `RangeSliderValueIndicatorShape`: the same bubble, told which of
/// a range slider's two thumbs it is over.
///
/// The abstract class is declared in upstream's `range_slider_parts.dart`
/// with the rest of the range family; its two concrete shapes live with the
/// single-value ones, so they are here and the enum comes with them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeSliderValueIndicatorShape {
    Rectangular(RectangularRangeSliderValueIndicatorShape),
    Paddle(PaddleRangeSliderValueIndicatorShape),
    RoundedRect(crate::range_slider_parts::RoundedRectRangeSliderValueIndicatorShape),
    Drop(crate::range_slider_parts::DropRangeSliderValueIndicatorShape),
}

/// Upstream `RectangularRangeSliderValueIndicatorShape`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectangularRangeSliderValueIndicatorShape;

impl RectangularRangeSliderValueIndicatorShape {
    pub fn new() -> RectangularRangeSliderValueIndicatorShape {
        RectangularRangeSliderValueIndicatorShape
    }

    pub fn preferred_size(&self, label: &TextPainter, text_scale: f32) -> Size {
        RectangularIndicatorPainter::preferred_size(label.width(), label.height(), text_scale)
    }

    /// Upstream's `getHorizontalShift`: a range slider asks for it so that it
    /// can keep the two bubbles from overlapping.
    pub fn horizontal_shift(&self, label: &TextPainter, geometry: &IndicatorPaintGeometry) -> f32 {
        indicator_horizontal_shift(
            RectangularIndicatorPainter::upper_rectangle_width(
                label.width(),
                geometry.scale,
                geometry.text_scale_factor,
            ),
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
        RectangularIndicatorPainter::paint(
            canvas,
            geometry,
            label,
            background,
            theme.value_indicator_stroke_color,
        );
    }
}

/// Upstream `PaddleRangeSliderValueIndicatorShape`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaddleRangeSliderValueIndicatorShape;

impl PaddleRangeSliderValueIndicatorShape {
    pub fn new() -> PaddleRangeSliderValueIndicatorShape {
        PaddleRangeSliderValueIndicatorShape
    }

    pub fn preferred_size(&self, label: &TextPainter, text_scale: f32) -> Size {
        PaddleIndicatorPainter::preferred_size(label.width(), text_scale)
    }

    pub fn horizontal_shift(&self, label: &TextPainter, geometry: &IndicatorPaintGeometry) -> f32 {
        PaddleIndicatorPainter::horizontal_shift(label, geometry)
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
        PaddleIndicatorPainter::paint(
            canvas,
            geometry,
            label,
            background,
            theme.value_indicator_stroke_color,
        );
    }
}

impl RangeSliderValueIndicatorShape {
    /// Upstream `getPreferredSize`.
    pub fn preferred_size(&self, label: &TextPainter, text_scale: f32) -> Size {
        match self {
            RangeSliderValueIndicatorShape::Rectangular(shape) => {
                shape.preferred_size(label, text_scale)
            }
            RangeSliderValueIndicatorShape::Paddle(shape) => {
                shape.preferred_size(label, text_scale)
            }
            RangeSliderValueIndicatorShape::RoundedRect(shape) => {
                shape.preferred_size(label, text_scale)
            }
            RangeSliderValueIndicatorShape::Drop(shape) => shape.preferred_size(label, text_scale),
        }
    }

    /// Upstream `getHorizontalShift`.
    pub fn horizontal_shift(&self, label: &TextPainter, geometry: &IndicatorPaintGeometry) -> f32 {
        match self {
            RangeSliderValueIndicatorShape::Rectangular(shape) => {
                shape.horizontal_shift(label, geometry)
            }
            RangeSliderValueIndicatorShape::Paddle(shape) => {
                shape.horizontal_shift(label, geometry)
            }
            RangeSliderValueIndicatorShape::RoundedRect(shape) => {
                shape.horizontal_shift(label, geometry)
            }
            RangeSliderValueIndicatorShape::Drop(shape) => shape.horizontal_shift(label, geometry),
        }
    }

    /// Upstream `paint`.
    ///
    /// Upstream also takes the [`Thumb`] this bubble belongs to. Neither of
    /// the two shapes reads it -- the bubble is the same over either end --
    /// and it is here for the same reason it is there: a shape written
    /// outside the framework may want it.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        geometry: &IndicatorPaintGeometry,
        theme: &SliderThemeData,
        label: &TextPainter,
        _thumb: Thumb,
    ) {
        match self {
            RangeSliderValueIndicatorShape::Rectangular(shape) => {
                shape.paint(canvas, geometry, theme, label)
            }
            RangeSliderValueIndicatorShape::Paddle(shape) => {
                shape.paint(canvas, geometry, theme, label)
            }
            RangeSliderValueIndicatorShape::RoundedRect(shape) => {
                shape.paint(canvas, geometry, theme, label)
            }
            RangeSliderValueIndicatorShape::Drop(shape) => {
                shape.paint(canvas, geometry, theme, label)
            }
        }
    }
}

/// The last step of the three-step fallback for a slider: what
/// [`SliderTheme::of`] left unset, filled in from the colour scheme the way
/// upstream's `_SliderDefaultsM3` does.
///
/// Upstream keeps two default tables and picks between them on
/// [`SliderThemeData::year_2023`] -- the 2023 look is a thin round-thumbed
/// track, the 2024 one a tall gapped track with a bar for a thumb. Both are
/// here because the flag is upstream's and a caller that sets it means it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedSlider {
    pub track_height: f32,
    pub active_track_color: Color,
    pub inactive_track_color: Color,
    pub thumb_color: Color,
    pub track_shape: SliderTrackShape,
    pub thumb_shape: SliderComponentShape,
    /// The size the thumb is actually drawn at, which for Material 3's bar
    /// comes from [`SliderThemeData::thumb_size`] and for the circle is twice
    /// its radius.
    pub thumb_size: Size,
}

impl ResolvedSlider {
    pub fn of(context: &mut BuildContext) -> ResolvedSlider {
        let data = SliderTheme::of(context);
        let colors = ThemeData::of(context).color_scheme;
        // Upstream's `_SliderDefaultsM3Year2023` against `_SliderDefaultsM3`:
        // the flag chooses the whole table, not one field at a time.
        let year_2023 = data.year_2023.unwrap_or(false);
        let thumb_shape = data.thumb_shape.unwrap_or(if year_2023 {
            SliderComponentShape::RoundThumb(RoundSliderThumbShape::new())
        } else {
            SliderComponentShape::Handle(HandleThumbShape::new())
        });
        let thumb_size = match thumb_shape {
            SliderComponentShape::Handle(shape) => data
                .thumb_size
                .as_ref()
                .and_then(|property| property.resolve(WidgetStates::NONE))
                .unwrap_or_else(|| shape.preferred_size()),
            other => other.preferred_size(true),
        };
        ResolvedSlider {
            track_height: data
                .track_height
                .unwrap_or(if year_2023 { 4.0 } else { 16.0 }),
            active_track_color: data.active_track_color.unwrap_or(colors.primary),
            inactive_track_color: data
                .inactive_track_color
                .unwrap_or_else(|| colors.surface_container_highest()),
            thumb_color: data.thumb_color.unwrap_or(colors.primary),
            track_shape: data.track_shape.unwrap_or(if year_2023 {
                SliderTrackShape::RoundedRect(RoundedRectSliderTrackShape::new())
            } else {
                SliderTrackShape::Gapped(GappedSliderTrackShape::new())
            }),
            thumb_shape,
            thumb_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{Component, ElementTree, component, leaf};
    use crate::render::EdgeInsets;
    use crate::theme::MaterialTheme;
    use crate::widgets::SizedBox;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Builds `read` inside the tree `wrap` makes and hands back what it saw.
    fn read_in<T: 'static, F, R>(wrap: F, read: R) -> T
    where
        F: FnOnce(AnyWidget) -> AnyWidget,
        R: Fn(&mut BuildContext) -> T + 'static,
    {
        struct Reader<T, R> {
            seen: Rc<RefCell<Option<T>>>,
            read: R,
        }

        impl<T: 'static, R: Fn(&mut BuildContext) -> T + 'static> Component for Reader<T, R> {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.seen.borrow_mut() = Some((self.read)(context));
                leaf(|| SizedBox::new(1.0, 1.0))
            }
        }

        let seen = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(wrap(component(Reader {
            seen: Rc::clone(&seen),
            read,
        })));
        seen.borrow_mut().take().expect("built")
    }

    /// A theme with a thumb and a track height, which is what
    /// `getPreferredRect` reads.
    fn measurable() -> SliderThemeData {
        SliderThemeData::new()
            .with_track_height(4.0)
            .with_thumb_shape(SliderComponentShape::RoundThumb(
                RoundSliderThumbShape::new(),
            ))
    }

    #[test]
    fn the_track_leaves_room_for_the_wider_of_the_thumb_and_the_overlay() {
        // A thumb of radius 10 is 20 wide; an overlay of radius 24 is 48. The
        // end space is half the wider of the two, and the track loses the
        // whole of it -- not half of it, and not the thumb's.
        let mut theme = measurable();
        theme.overlay_shape = Some(SliderComponentShape::RoundOverlay(
            RoundSliderOverlayShape::new(),
        ));
        let rect = BaseSliderTrackShape::preferred_rect(
            Size::new(200.0, 40.0),
            Offset::ZERO,
            &theme,
            true,
        );
        assert_eq!(rect.left, 24.0);
        assert_eq!(rect.right, 200.0 - 24.0);
        // Vertically centred, at the theme's height.
        assert_eq!(rect.top, 18.0);
        assert_eq!(rect.height(), 4.0);
    }

    #[test]
    fn a_theme_with_padding_takes_the_end_space_over_from_the_shape() {
        // The shape leaves nothing at the ends when the theme has a padding.
        // That is the whole of upstream's null check on it: the two are
        // alternatives, not additions.
        let mut theme = measurable();
        theme.overlay_shape = Some(SliderComponentShape::RoundOverlay(
            RoundSliderOverlayShape::new(),
        ));
        theme.padding = Some(EdgeInsetsGeometry::Absolute(EdgeInsets::all(8.0)));
        let rect = BaseSliderTrackShape::preferred_rect(
            Size::new(200.0, 40.0),
            Offset::ZERO,
            &theme,
            true,
        );
        assert_eq!(rect.left, 0.0);
        assert_eq!(rect.right, 200.0);
    }

    #[test]
    fn two_transparent_track_colours_collapse_the_height_but_not_the_width() {
        // Upstream's comment says why: the slider keeps its overall width so
        // that an invisible track does not move everything beside it.
        let mut theme = measurable();
        theme.active_track_color = Some(Color::TRANSPARENT);
        theme.inactive_track_color = Some(Color::TRANSPARENT);
        let rect = BaseSliderTrackShape::preferred_rect(
            Size::new(200.0, 40.0),
            Offset::ZERO,
            &theme,
            true,
        );
        assert_eq!(rect.height(), 0.0);
        assert_eq!(rect.width(), 180.0);
        // One of the two transparent is not enough.
        theme.active_track_color = Some(Color::WHITE);
        let rect = BaseSliderTrackShape::preferred_rect(
            Size::new(200.0, 40.0),
            Offset::ZERO,
            &theme,
            true,
        );
        assert_eq!(rect.height(), 4.0);
    }

    #[test]
    fn a_parent_narrower_than_the_slider_gets_a_swapped_rect_not_an_inverted_one() {
        // 20 of thumb taken out of a 10-wide parent puts the right edge left
        // of the left one; upstream swaps them rather than asserting.
        let theme = measurable();
        let rect =
            BaseSliderTrackShape::preferred_rect(Size::new(10.0, 40.0), Offset::ZERO, &theme, true);
        assert!(rect.left <= rect.right, "{rect:?} should not be inverted");
        assert_eq!(rect.width(), 10.0);
    }

    #[test]
    fn a_disabled_thumb_with_no_radius_of_its_own_keeps_the_enabled_one() {
        let shape = RoundSliderThumbShape::new();
        assert_eq!(shape.preferred_size(true), shape.preferred_size(false));
        let smaller = RoundSliderThumbShape {
            disabled_thumb_radius: Some(4.0),
            ..RoundSliderThumbShape::new()
        };
        assert_eq!(smaller.preferred_size(false), Size::from_radius(4.0));
        assert_eq!(smaller.preferred_size(true), Size::from_radius(10.0));
    }

    #[test]
    fn a_tick_mark_with_no_radius_is_a_quarter_of_the_track() {
        let theme = SliderThemeData::new().with_track_height(16.0);
        let shape = RoundSliderTickMarkShape::new();
        assert_eq!(shape.preferred_size(&theme), Size::from_radius(4.0));
        assert_eq!(
            RoundSliderTickMarkShape::with_radius(1.0).preferred_size(&theme),
            Size::from_radius(1.0)
        );
    }

    #[test]
    fn the_overlay_reserves_its_size_whether_or_not_it_is_drawn() {
        // The preferred size is the same enabled and disabled, and the same
        // pressed and at rest -- only the painted radius follows the
        // activation. A slider whose track moved when the overlay appeared
        // would jitter under the finger.
        let shape = RoundSliderOverlayShape::new();
        assert_eq!(shape.preferred_size(), Size::from_radius(24.0));
        assert_eq!(
            SliderComponentShape::RoundOverlay(shape).preferred_size(true),
            SliderComponentShape::RoundOverlay(shape).preferred_size(false)
        );
    }

    #[test]
    fn the_material_three_thumb_draws_at_the_themes_size_not_its_preferred_one() {
        // The preferred size is a constant 4x44, but the drawn size comes
        // from the theme's thumb size, which is a state property because the
        // thumb narrows while it is dragged. The two are not the same number
        // and are not meant to be.
        let shape = HandleThumbShape::new();
        assert_eq!(shape.preferred_size(), Size::new(4.0, 44.0));
        let theme = SliderThemeData {
            thumb_size: Some(StateProperty::all(Some(Size::new(2.0, 44.0)))),
            ..SliderThemeData::new()
        };
        assert_ne!(
            theme
                .thumb_size
                .as_ref()
                .unwrap()
                .resolve(WidgetStates::NONE),
            Some(shape.preferred_size())
        );
    }

    #[test]
    fn only_the_two_rounded_tracks_report_themselves_rounded() {
        assert!(
            !SliderTrackShape::Rectangular(RectangularSliderTrackShape::new()).is_rounded(),
            "the base shape is not rounded, and the rectangular track does not override it"
        );
        assert!(SliderTrackShape::RoundedRect(RoundedRectSliderTrackShape::new()).is_rounded());
        assert!(SliderTrackShape::Gapped(GappedSliderTrackShape::new()).is_rounded());
    }

    #[test]
    fn lerping_a_theme_interpolates_the_colours_and_snaps_the_shapes() {
        let a = SliderThemeData::new()
            .with_track_height(2.0)
            .with_track_shape(SliderTrackShape::Rectangular(
                RectangularSliderTrackShape::new(),
            ));
        let b = SliderThemeData::new()
            .with_track_height(10.0)
            .with_track_shape(SliderTrackShape::Gapped(GappedSliderTrackShape::new()));
        let middle = SliderThemeData::lerp(&a, &b, 0.5);
        assert_eq!(middle.track_height, Some(6.0));
        // A shape half-way between two shapes is not a shape, so it snaps --
        // and at exactly the halfway point it is already the second one.
        assert_eq!(middle.track_shape, b.track_shape);
        assert_eq!(
            SliderThemeData::lerp(&a, &b, 0.49).track_shape,
            a.track_shape
        );
    }

    #[test]
    fn the_material_two_defaults_are_the_primary_colours_at_upstreams_alphas() {
        let theme = SliderThemeData::from_primary_colors(
            Color::argb(0xff, 0x21, 0x96, 0xf3),
            Color::argb(0xff, 0x19, 0x76, 0xd2),
            Color::argb(0xff, 0xbb, 0xde, 0xfb),
            TextStyle::default(),
        );
        // The active track is the primary colour outright; the inactive one
        // is the same colour at 24%. Both come off the primary colour -- the
        // inactive track is not a paler colour, it is the same one faded.
        assert_eq!(theme.active_track_color.unwrap().alpha(), 0xff);
        assert_eq!(theme.inactive_track_color.unwrap().alpha(), 0x3d);
        assert_eq!(
            theme.active_track_color.unwrap().red(),
            theme.inactive_track_color.unwrap().red()
        );
        // The overlapping-thumb stroke is the one colour that is not derived
        // from the three: upstream hard-codes white.
        assert_eq!(theme.overlapping_shape_stroke_color, Some(Color::WHITE));
        assert_eq!(
            theme.show_value_indicator,
            Some(ShowValueIndicator::OnlyForDiscrete)
        );
    }

    #[test]
    fn a_slider_theme_falls_back_through_the_theme_data_to_nothing() {
        // Nobody said: the empty theme.
        let bare = read_in(
            |child| MaterialTheme::new(ThemeData::light(), child),
            |context| SliderTheme::of(context).track_height,
        );
        assert_eq!(bare, None);

        // The theme data said: its field.
        let from_theme_data = read_in(
            |child| {
                let theme = ThemeData {
                    slider_theme: SliderThemeData::new().with_track_height(9.0),
                    ..ThemeData::light()
                };
                MaterialTheme::new(theme, child)
            },
            |context| SliderTheme::of(context).track_height,
        );
        assert_eq!(from_theme_data, Some(9.0));

        // A nearer installed one said: it wins over both.
        let from_nearer = read_in(
            |child| {
                let theme = ThemeData {
                    slider_theme: SliderThemeData::new().with_track_height(9.0),
                    ..ThemeData::light()
                };
                MaterialTheme::new(
                    theme,
                    SliderTheme::new(SliderThemeData::new().with_track_height(1.0), child),
                )
            },
            |context| SliderTheme::of(context).track_height,
        );
        assert_eq!(from_nearer, Some(1.0));
    }

    #[test]
    fn a_secondary_value_behind_the_thumb_draws_nothing() {
        // Upstream only draws the secondary segment where it has passed the
        // thumb; behind the thumb it is already under the active track.
        let theme = SliderThemeData {
            secondary_active_track_color: Some(Color::WHITE),
            disabled_secondary_active_track_color: Some(Color::WHITE),
            ..measurable()
        };
        let track = Rect::ltrb(0.0, 0.0, 100.0, 4.0);
        let ahead = TrackPaintGeometry::new(track, Offset::new(50.0, 2.0), TextDirection::Ltr, 1.0)
            .with_secondary_offset(Offset::new(80.0, 2.0));
        assert!(ahead.secondary_segment(&theme, false).is_some());
        let behind =
            TrackPaintGeometry::new(track, Offset::new(50.0, 2.0), TextDirection::Ltr, 1.0)
                .with_secondary_offset(Offset::new(20.0, 2.0));
        assert!(behind.secondary_segment(&theme, false).is_none());
        // Right to left, the same value is the other way round.
        let rtl = TrackPaintGeometry::new(track, Offset::new(50.0, 2.0), TextDirection::Rtl, 1.0)
            .with_secondary_offset(Offset::new(20.0, 2.0));
        assert!(rtl.secondary_segment(&theme, false).is_some());
    }

    #[test]
    fn the_gapped_track_measures_the_secondary_value_from_the_far_side_of_the_gap() {
        // A secondary value inside the gap draws nothing: the gap is meant to
        // be empty, and a sliver of secondary track in it would fill it in.
        let theme = SliderThemeData {
            secondary_active_track_color: Some(Color::WHITE),
            disabled_secondary_active_track_color: Some(Color::WHITE),
            track_gap: Some(6.0),
            ..measurable()
        };
        let track = Rect::ltrb(0.0, 0.0, 100.0, 4.0);
        let geometry =
            TrackPaintGeometry::new(track, Offset::new(50.0, 2.0), TextDirection::Ltr, 1.0)
                .with_secondary_offset(Offset::new(53.0, 2.0));
        assert!(geometry.secondary_segment(&theme, true).is_none());
        // Without the gap rule, the same offset is past the thumb and draws.
        assert!(geometry.secondary_segment(&theme, false).is_some());
    }

    #[test]
    fn the_year_flag_picks_the_whole_default_table_not_one_field() {
        // Upstream keeps two default classes and chooses between them; a
        // reader who expects the flag to change only the track height gets
        // the shapes wrong, and with them the thumb.
        let modern = read_in(
            |child| MaterialTheme::new(ThemeData::light(), child),
            ResolvedSlider::of,
        );
        assert_eq!(modern.track_height, 16.0);
        assert_eq!(
            modern.thumb_shape,
            SliderComponentShape::Handle(HandleThumbShape::new())
        );
        assert_eq!(
            modern.track_shape,
            SliderTrackShape::Gapped(GappedSliderTrackShape::new())
        );

        let older = read_in(
            |child| {
                let theme = ThemeData {
                    slider_theme: SliderThemeData {
                        year_2023: Some(true),
                        ..SliderThemeData::new()
                    },
                    ..ThemeData::light()
                };
                MaterialTheme::new(theme, child)
            },
            ResolvedSlider::of,
        );
        assert_eq!(older.track_height, 4.0);
        assert_eq!(
            older.thumb_shape,
            SliderComponentShape::RoundThumb(RoundSliderThumbShape::new())
        );
        assert_eq!(
            older.track_shape,
            SliderTrackShape::RoundedRect(RoundedRectSliderTrackShape::new())
        );
    }

    #[test]
    fn an_installed_slider_theme_reaches_the_resolved_defaults() {
        // The point of the wiring: a theme installed over the control moves
        // the numbers the control draws with, rather than the control keeping
        // its own constants.
        let resolved = read_in(
            |child| {
                MaterialTheme::new(
                    ThemeData::light(),
                    SliderTheme::new(
                        SliderThemeData::new()
                            .with_track_height(3.0)
                            .with_track_colors(Color::WHITE, Color::TRANSPARENT),
                        child,
                    ),
                )
            },
            ResolvedSlider::of,
        );
        assert_eq!(resolved.track_height, 3.0);
        assert_eq!(resolved.active_track_color, Color::WHITE);
        assert_eq!(resolved.inactive_track_color, Color::TRANSPARENT);
        // What the theme did not say still comes off the scheme.
        let scheme_primary = read_in(
            |child| MaterialTheme::new(ThemeData::light(), child),
            |context| ThemeData::of(context).color_scheme.primary,
        );
        assert_eq!(resolved.thumb_color, scheme_primary);
    }

    /// A label of a known width, so the indicator arithmetic has something to
    /// measure.
    fn label(text: &str) -> TextPainter {
        let mut painter = TextPainter::new().text(text, TextStyle::default());
        painter.layout(f32::INFINITY);
        painter
    }

    #[test]
    fn the_bubble_is_pushed_in_from_whichever_edge_it_would_hang_off() {
        let width = 100.0;
        // Room on both sides: no shift at all.
        let middle =
            IndicatorPaintGeometry::new(Offset::new(200.0, 0.0), Size::new(400.0, 60.0), 1.0);
        assert_eq!(indicator_horizontal_shift(width, &middle), 0.0);

        // Hard against the left edge: shifted right by what hangs off, plus
        // the 8px margin upstream keeps.
        let left = IndicatorPaintGeometry::new(Offset::new(10.0, 0.0), Size::new(400.0, 60.0), 1.0);
        assert_eq!(indicator_horizontal_shift(width, &left), 50.0 - 10.0 + 8.0);

        // Hard against the right edge: shifted left by the same amount.
        let right =
            IndicatorPaintGeometry::new(Offset::new(390.0, 0.0), Size::new(400.0, 60.0), 1.0);
        assert_eq!(
            indicator_horizontal_shift(width, &right),
            -(50.0 - 10.0 + 8.0)
        );
    }

    #[test]
    fn a_bubble_wider_than_the_slider_is_pinned_rather_than_centred() {
        // There is no shift that keeps a bubble wider than the box inside it,
        // so upstream stops trying to centre it and pins the edge that is
        // overflowing further. A reader who expects the first branch to apply
        // everywhere gets a bubble that hangs off both ends.
        let narrow = Size::new(60.0, 60.0);
        let near_left = IndicatorPaintGeometry::new(Offset::new(10.0, 0.0), narrow, 1.0);
        let shift = indicator_horizontal_shift(100.0, &near_left);
        // Pinned to the left margin: the overflow on the left, less the 8px.
        assert_eq!(shift, (50.0 - 10.0 + 8.0) - 8.0);
    }

    #[test]
    fn an_indicator_never_measures_narrower_than_its_minimum_label() {
        // An empty label still gets a bubble: the minimum width is what keeps
        // a one-character value from drawing as a sliver. Past that minimum
        // the bubble follows the text.
        let narrow = RectangularIndicatorPainter::preferred_size(0.0, 12.0, 1.0);
        let wide = RectangularIndicatorPainter::preferred_size(90.0, 12.0, 1.0);
        assert_eq!(narrow.width, 16.0 + 32.0);
        assert_eq!(wide.width, 90.0 + 32.0);
        // The height is the label's plus the padding, and does not have a
        // floor -- only the width does.
        assert_eq!(narrow.height, 12.0 + 16.0);
    }

    #[test]
    fn the_drop_indicators_corner_is_always_the_round_one() {
        // Upstream lerps from a 4px corner to a fully round one at
        // `1.0 - rectness`, and leaves `rectness` at zero -- so the 4px end
        // is never reached. Reading the lerp and expecting 4px is the wrong
        // guess, and the constant is where upstream left the knob.
        let rect = Rect::xywh(0.0, 0.0, 100.0, 28.0);
        let radius = DropIndicatorPainter::adjusted_border_radius(rect);
        assert_eq!(radius, BorderRadius::circular(14.0));
    }

    #[test]
    fn the_paddle_is_the_same_height_whatever_the_label_says() {
        // The top lobe spreads sideways as the text grows; it does not grow
        // taller, which is why the height is a constant and the width is not.
        let short = PaddleIndicatorPainter::preferred_size(8.0, 1.0);
        let long = PaddleIndicatorPainter::preferred_size(120.0, 1.0);
        assert_eq!(short.height, 66.0);
        assert_eq!(long.height, 66.0);
        assert!(long.width > short.width);
        // 40 between the two centres, plus one radius at each end.
        assert_eq!(short.height, 40.0 + 16.0 + 10.0);
    }

    #[test]
    fn the_paddle_shifts_no_further_than_the_label_needs() {
        // The paddle stretches by moving its top lobe, and the lobe can only
        // move as far as the text has spread it. A slider at the very edge of
        // its box asks for more than that and gets the clamp.
        let needed = 30.0;
        let far_left =
            PaddleIndicatorPainter::ideal_offset(needed, 1.0, Offset::new(0.0, 0.0), 400.0);
        assert_eq!(far_left, needed);
        let far_right =
            PaddleIndicatorPainter::ideal_offset(needed, 1.0, Offset::new(400.0, 0.0), 400.0);
        assert_eq!(far_right, -needed);
        // In the middle it does not move.
        assert_eq!(
            PaddleIndicatorPainter::ideal_offset(needed, 1.0, Offset::new(200.0, 0.0), 400.0),
            0.0
        );
    }

    #[test]
    fn a_paddle_with_no_room_to_spare_does_not_divide_by_zero() {
        // Upstream returns before the arithmetic when the scale is zero, and
        // says why: the divisions below would send NaNs to the engine.
        assert_eq!(
            PaddleIndicatorPainter::ideal_offset(10.0, 0.0, Offset::new(0.0, 0.0), 400.0),
            0.0
        );
    }

    #[test]
    fn a_range_indicator_draws_the_same_bubble_over_either_thumb() {
        // Neither of the two shapes reads the thumb. Upstream passes it
        // anyway, and so does this -- but a reader expecting the start and
        // end bubbles to differ is reading something that is not there.
        let text = label("50");
        let shape = RangeSliderValueIndicatorShape::Paddle(PaddleRangeSliderValueIndicatorShape);
        let geometry =
            IndicatorPaintGeometry::new(Offset::new(50.0, 0.0), Size::new(200.0, 80.0), 1.0);
        assert_eq!(
            shape.horizontal_shift(&text, &geometry),
            RangeSliderValueIndicatorShape::Paddle(PaddleRangeSliderValueIndicatorShape)
                .horizontal_shift(&text, &geometry)
        );
        assert_eq!(shape.preferred_size(&text, 1.0).height, 66.0);
    }

    #[test]
    fn an_indicator_shape_has_no_size_until_it_is_given_a_label() {
        // The two-argument form cannot answer for an indicator, and upstream's
        // cannot either -- its overload takes the label as an optional extra.
        let shape = SliderComponentShape::PaddleIndicator(PaddleSliderValueIndicatorShape::new());
        assert_eq!(shape.preferred_size(true), Size::ZERO);
        assert_eq!(
            shape
                .preferred_size_for_label(true, &label("50"), 1.0)
                .height,
            66.0
        );
        // A thumb answers the same either way: the extra parameters are for
        // the shapes that need them.
        let thumb = SliderComponentShape::RoundThumb(RoundSliderThumbShape::new());
        assert_eq!(
            thumb.preferred_size(true),
            thumb.preferred_size_for_label(true, &label("50"), 1.0)
        );
    }
    #[test]
    fn the_track_colours_swap_with_the_text_direction() {
        let theme = SliderThemeData {
            active_track_color: Some(Color::argb(0xff, 0xff, 0x00, 0x00)),
            inactive_track_color: Some(Color::argb(0xff, 0x00, 0x00, 0xff)),
            disabled_active_track_color: Some(Color::argb(0xff, 0xff, 0x00, 0x00)),
            disabled_inactive_track_color: Some(Color::argb(0xff, 0x00, 0x00, 0xff)),
            ..measurable()
        };
        let track = Rect::ltrb(0.0, 0.0, 100.0, 4.0);
        let ltr = TrackPaintGeometry::new(track, Offset::new(50.0, 2.0), TextDirection::Ltr, 1.0);
        let rtl = TrackPaintGeometry::new(track, Offset::new(50.0, 2.0), TextDirection::Rtl, 1.0);
        let (leading, trailing) = track_paints(&theme, &ltr).expect("both colours set");
        assert_eq!(leading, theme.active_track_color.unwrap());
        assert_eq!(trailing, theme.inactive_track_color.unwrap());
        // The leading half is the active one in both directions -- what
        // changes is which side of the thumb the leading half is on.
        let (leading, trailing) = track_paints(&theme, &rtl).expect("both colours set");
        assert_eq!(leading, theme.inactive_track_color.unwrap());
        assert_eq!(trailing, theme.active_track_color.unwrap());
    }

    // -- What the field-by-field walk in `lerp` actually blends --------------
    //
    // `tools/unlerped_fields.py` froze each of this method's 22 lines in turn
    // -- replacing the whole blend with its first end -- and 17 of them left
    // the suite green. Nothing anywhere read those fields through a lerp, so
    // a line that named the field above it would have been invisible too.
    // That is the defect this shape actually has.

    /// A theme whose sixteen colours are sixteen *different* numbers, so that
    /// a line naming the wrong field answers with another field's value.
    fn numbered(base: u8) -> SliderThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            Some(Color::argb(255, 0, 0, base + n))
        };
        SliderThemeData {
            active_track_color: next(),
            inactive_track_color: next(),
            secondary_active_track_color: next(),
            disabled_active_track_color: next(),
            disabled_secondary_active_track_color: next(),
            disabled_inactive_track_color: next(),
            active_tick_mark_color: next(),
            inactive_tick_mark_color: next(),
            disabled_active_tick_mark_color: next(),
            disabled_inactive_tick_mark_color: next(),
            thumb_color: next(),
            overlapping_shape_stroke_color: next(),
            disabled_thumb_color: next(),
            overlay_color: next(),
            value_indicator_color: next(),
            value_indicator_stroke_color: next(),
            ..SliderThemeData::default()
        }
    }

    #[test]
    fn every_colour_blends_and_every_line_names_its_own_field() {
        // Every other field is absent at both ends, so the whole struct
        // compares equal in one assertion -- and it can only compare equal if
        // all sixteen lines read the field they are assigned to.
        assert_eq!(
            SliderThemeData::lerp(&numbered(0), &numbered(80), 0.25),
            numbered(20)
        );
        assert_eq!(
            SliderThemeData::lerp(&numbered(80), &numbered(0), 0.25),
            numbered(60)
        );
    }

    #[test]
    fn the_three_numbers_blend_and_do_not_share_a_line() {
        // Three different pairs, so a line reading a neighbour's field lands
        // on a number that is not its own.
        let a = SliderThemeData {
            track_height: Some(4.0),
            min_thumb_separation: Some(8.0),
            track_gap: Some(12.0),
            ..SliderThemeData::default()
        };
        let b = SliderThemeData {
            track_height: Some(20.0),
            min_thumb_separation: Some(24.0),
            track_gap: Some(28.0),
            ..SliderThemeData::default()
        };
        let quarter = SliderThemeData::lerp(&a, &b, 0.25);
        assert_eq!(
            (
                quarter.track_height,
                quarter.min_thumb_separation,
                quarter.track_gap
            ),
            (Some(8.0), Some(12.0), Some(16.0))
        );
        let back = SliderThemeData::lerp(&b, &a, 0.25);
        assert_eq!(
            (back.track_height, back.min_thumb_separation, back.track_gap),
            (Some(16.0), Some(20.0), Some(24.0))
        );
    }

    // -- Three fields this port had stepping that upstream blends ------------

    #[test]
    fn the_padding_slides_rather_than_jumping_at_the_midpoint() {
        // Upstream is `EdgeInsetsGeometry.lerp(a.padding, b.padding, t)`.
        // This port had `lerp_nearer`, which answers `a` for the whole first
        // half and then jumps -- the track would shift sideways in one frame
        // partway through a theme transition.
        let a = SliderThemeData {
            padding: Some(EdgeInsetsGeometry::Absolute(EdgeInsets {
                left: 4.0,
                top: 8.0,
                right: 12.0,
                bottom: 16.0,
            })),
            ..SliderThemeData::default()
        };
        let b = SliderThemeData {
            padding: Some(EdgeInsetsGeometry::Absolute(EdgeInsets {
                left: 20.0,
                top: 24.0,
                right: 28.0,
                bottom: 32.0,
            })),
            ..SliderThemeData::default()
        };
        assert_eq!(
            SliderThemeData::lerp(&a, &b, 0.25).padding,
            Some(EdgeInsetsGeometry::Absolute(EdgeInsets {
                left: 8.0,
                top: 12.0,
                right: 16.0,
                bottom: 20.0,
            }))
        );
    }

    #[test]
    fn the_thumb_size_grows_rather_than_jumping() {
        // Upstream is `WidgetStateProperty.lerp<Size?>(..., Size.lerp)`.
        let a = SliderThemeData {
            thumb_size: Some(StateProperty::all(Some(Size::new(4.0, 20.0)))),
            ..SliderThemeData::default()
        };
        let b = SliderThemeData {
            thumb_size: Some(StateProperty::all(Some(Size::new(20.0, 4.0)))),
            ..SliderThemeData::default()
        };
        // The two dimensions move opposite ways, so a line reading the wrong
        // one lands on the other's answer.
        assert_eq!(
            SliderThemeData::lerp(&a, &b, 0.25)
                .thumb_size
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            Some(Size::new(8.0, 16.0))
        );

        // And a thumb size that only one end has grows out of nothing rather
        // than springing to full size: upstream's `Size.lerp(null, b, t)` is
        // `b * t`.
        let only_b = SliderThemeData {
            thumb_size: Some(StateProperty::all(None)),
            ..SliderThemeData::default()
        };
        assert_eq!(
            SliderThemeData::lerp(&only_b, &b, 0.25)
                .thumb_size
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            Some(Size::new(5.0, 1.0))
        );
    }

    #[test]
    fn the_value_indicator_text_blends_rather_than_jumping() {
        // Upstream is `TextStyle.lerp`, which moves the size, the weight and
        // the colour.
        let a = SliderThemeData {
            value_indicator_text_style: Some(TextStyle {
                font_size: 4.0,
                font_weight: 400,
                ..TextStyle::default()
            }),
            ..SliderThemeData::default()
        };
        let b = SliderThemeData {
            value_indicator_text_style: Some(TextStyle {
                font_size: 20.0,
                font_weight: 800,
                ..TextStyle::default()
            }),
            ..SliderThemeData::default()
        };
        let quarter = SliderThemeData::lerp(&a, &b, 0.25)
            .value_indicator_text_style
            .expect("two ends is enough");
        assert_eq!((quarter.font_size, quarter.font_weight), (8.0, 500));
    }

    #[test]
    fn every_stepping_field_steps_at_the_midpoint_and_names_its_own_field() {
        // These are the fields with no midpoint -- a shape half-way between a
        // circle and a bar is not a shape -- so each takes the nearer end.
        // The tests above all sample a quarter of the way, where the nearer
        // end *is* `a`, and freezing one of these lines to `a` is invisible
        // there. Past the midpoint it is not.
        //
        // Every field gets a different variant at each end, so a line reading
        // its neighbour's field answers with a shape that is not its own.
        let a = SliderThemeData {
            overlay_shape: Some(SliderComponentShape::Empty),
            tick_mark_shape: Some(SliderTickMarkShape::Empty),
            thumb_shape: Some(SliderComponentShape::Handle(HandleThumbShape::default())),
            show_value_indicator: Some(ShowValueIndicator::Never),
            range_thumb_shape: Some(crate::range_slider_parts::RangeSliderThumbShape::Handle(
                crate::range_slider_parts::HandleRangeSliderThumbShape::default(),
            )),
            range_track_shape: Some(crate::range_slider_parts::RangeSliderTrackShape::Rectangular(
                crate::range_slider_parts::RectangularRangeSliderTrackShape::default(),
            )),
            allowed_interaction: Some(SliderInteraction::TapOnly),
            year_2023: Some(true),
            ..SliderThemeData::default()
        };
        let b = SliderThemeData {
            overlay_shape: Some(SliderComponentShape::RoundOverlay(
                RoundSliderOverlayShape::default(),
            )),
            tick_mark_shape: Some(SliderTickMarkShape::Round(
                RoundSliderTickMarkShape::default(),
            )),
            thumb_shape: Some(SliderComponentShape::RoundThumb(
                RoundSliderThumbShape::default(),
            )),
            show_value_indicator: Some(ShowValueIndicator::AlwaysVisible),
            range_thumb_shape: Some(crate::range_slider_parts::RangeSliderThumbShape::Round(
                crate::range_slider_parts::RoundRangeSliderThumbShape::default(),
            )),
            range_track_shape: Some(crate::range_slider_parts::RangeSliderTrackShape::Gapped(
                crate::range_slider_parts::GappedRangeSliderTrackShape::default(),
            )),
            allowed_interaction: Some(SliderInteraction::SlideThumb),
            year_2023: Some(false),
            ..SliderThemeData::default()
        };

        // Just short of the midpoint every one of them is still `a`'s...
        assert_eq!(SliderThemeData::lerp(&a, &b, 0.499), a);
        // ...and just past it every one of them is `b`'s, in the same frame.
        assert_eq!(SliderThemeData::lerp(&a, &b, 0.5), b);
    }
}

#[cfg(test)]
mod rectangular_track_paint_tests {
    use super::{RectangularSliderTrackShape, SliderThemeData, TrackPaintGeometry};
    use crate::direction::TextDirection;
    use crate::engine::Color;
    use crate::engine::{LayerTree, Rect};
    use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
    use crate::render::{Offset, PaintContext, Size};

    const ACTIVE: Color = Color(0xff112233);
    const INACTIVE: Color = Color(0xff445566);
    const SECONDARY: Color = Color(0xff778899);

    fn theme(height: f32) -> SliderThemeData {
        let mut theme = SliderThemeData::new()
            .with_track_height(height)
            .with_track_colors(ACTIVE, INACTIVE);
        theme.secondary_active_track_color = Some(SECONDARY);
        theme
    }

    /// A track a hundred wide and four tall, with the thumb wherever asked.
    fn geometry(thumb_x: f32, direction: TextDirection) -> TrackPaintGeometry {
        TrackPaintGeometry::new(
            Rect::ltrb(0.0, 8.0, 100.0, 12.0),
            Offset::new(thumb_x, 10.0),
            direction,
            1.0,
        )
    }

    fn painted(theme: &SliderThemeData, geometry: &TrackPaintGeometry) -> Vec<Drawn> {
        let mut layers = LayerTree::new(200, 200);
        reset_drawn();
        {
            let mut context = PaintContext::new(&mut layers, Size::new(200.0, 200.0));
            RectangularSliderTrackShape::new().paint(context.canvas(), geometry, theme);
        }
        drawn()
    }

    fn rect(left: f32, right: f32, colour: Color) -> Drawn {
        Drawn::Rect {
            left,
            top: 8.0,
            right,
            bottom: 12.0,
            argb: colour.0,
            stroke: None,
        }
    }

    #[test]
    fn the_track_is_split_at_the_thumbs_centre() {
        // Not at the value, and not at either edge of the thumb: the two
        // segments meet exactly where the thumb's centre is.
        assert_eq!(
            painted(&theme(4.0), &geometry(30.0, TextDirection::Ltr)),
            vec![rect(0.0, 30.0, ACTIVE), rect(30.0, 100.0, INACTIVE)]
        );
    }

    #[test]
    fn reading_right_to_left_swaps_which_side_is_active() {
        // The leading segment is always the one left of the thumb; which of
        // the two colours it takes is the reading direction's business. This
        // could not be asserted until a draw call carried its own colour --
        // with one global "last colour" there is nothing to compare.
        assert_eq!(
            painted(&theme(4.0), &geometry(30.0, TextDirection::Rtl)),
            vec![rect(0.0, 30.0, INACTIVE), rect(30.0, 100.0, ACTIVE)],
            "the same two rectangles, the colours the other way round"
        );
    }

    #[test]
    fn a_thumb_at_either_end_leaves_one_segment_rather_than_an_empty_one() {
        // A zero-width rectangle is not a thinner rectangle, it is a draw call
        // that paints nothing, and upstream skips it.
        assert_eq!(
            painted(&theme(4.0), &geometry(0.0, TextDirection::Ltr)),
            vec![rect(0.0, 100.0, INACTIVE)],
            "hard left: everything is inactive"
        );
        assert_eq!(
            painted(&theme(4.0), &geometry(100.0, TextDirection::Ltr)),
            vec![rect(0.0, 100.0, ACTIVE)],
            "hard right: everything is active"
        );
    }

    #[test]
    fn a_track_of_no_height_draws_nothing_at_all() {
        // Rather than a stack of empty rectangles.
        assert_eq!(
            painted(&theme(0.0), &geometry(30.0, TextDirection::Ltr)),
            vec![]
        );
        assert_eq!(
            painted(&SliderThemeData::new(), &geometry(30.0, TextDirection::Ltr)),
            vec![],
            "and a theme that never named a height is the same case"
        );
    }

    #[test]
    fn a_secondary_value_is_a_third_rectangle_beyond_the_thumb() {
        // What a media slider draws for buffered-but-unplayed.
        let calls = painted(
            &theme(4.0),
            &geometry(30.0, TextDirection::Ltr).with_secondary_offset(Offset::new(70.0, 10.0)),
        );
        assert_eq!(
            calls,
            vec![
                rect(0.0, 30.0, ACTIVE),
                rect(30.0, 100.0, INACTIVE),
                rect(30.0, 70.0, SECONDARY),
            ],
            "from the thumb to the buffer, over the inactive segment"
        );
    }

    #[test]
    fn a_secondary_value_with_no_colour_for_it_is_not_drawn() {
        // The colour is what says the theme wants one at all; upstream's
        // enabled_color returns nothing and the segment goes with it.
        let mut bare = theme(4.0);
        bare.secondary_active_track_color = None;
        let calls = painted(
            &bare,
            &geometry(30.0, TextDirection::Ltr).with_secondary_offset(Offset::new(70.0, 10.0)),
        );
        assert_eq!(calls.len(), 2, "the two ordinary segments only: {calls:?}");
    }

    #[test]
    fn a_secondary_value_behind_the_thumb_is_not_drawn() {
        // It has already been passed, so there is nothing buffered-but-unread
        // to show.
        let calls = painted(
            &theme(4.0),
            &geometry(70.0, TextDirection::Ltr).with_secondary_offset(Offset::new(30.0, 10.0)),
        );
        assert_eq!(calls.len(), 2, "the two ordinary segments only: {calls:?}");
    }
}

// -- Where a tick mark is, and which side of the thumb that is ----------------

#[cfg(test)]
mod tick_mark_paint_tests {
    //! `variant_sweep` found two arms here that nothing was looking at: the
    //! RTL branch of "which side of the thumb is past it", and the whole of
    //! `Round`'s preferred size, which could answer `Size::ZERO` -- the empty
    //! shape's answer -- with the suite green. A zero size draws no tick marks
    //! at all, because `paint` guards on `radius > 0.0`.

    use super::{
        RoundSliderTickMarkShape, SliderThemeData, SliderTickMarkShape,
    };
    use crate::direction::TextDirection;
    use crate::engine::{Color, LayerTree};
    use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
    use crate::render::{Offset, PaintContext, Size};

    const ACTIVE: Color = Color(0xff00aa00);
    const INACTIVE: Color = Color(0xff888888);
    const TRACK_HEIGHT: f32 = 8.0;

    fn theme() -> SliderThemeData {
        let mut theme = SliderThemeData::new().with_track_height(TRACK_HEIGHT);
        theme.active_tick_mark_color = Some(ACTIVE);
        theme.inactive_tick_mark_color = Some(INACTIVE);
        theme
    }

    /// The colour a mark at `center_x` is given with the thumb at `thumb_x`.
    fn mark_colour(center_x: f32, thumb_x: f32, direction: TextDirection) -> Option<u32> {
        let mut layers = LayerTree::new(200, 200);
        reset_drawn();
        {
            let mut context = PaintContext::new(&mut layers, Size::new(200.0, 200.0));
            RoundSliderTickMarkShape::new().paint(
                context.canvas(),
                Offset::new(center_x, 10.0),
                &theme(),
                Offset::new(thumb_x, 10.0),
                direction,
                1.0,
            );
        }
        drawn().iter().find_map(|call| match call {
            Drawn::Circle { argb, .. } => Some(*argb),
            _ => None,
        })
    }

    #[test]
    fn a_mark_the_thumb_has_passed_is_active_and_one_ahead_of_it_is_not() {
        // Left to right: the thumb sweeps rightwards, so everything left of it
        // has been chosen and everything right of it has not.
        assert_eq!(mark_colour(20.0, 50.0, TextDirection::Ltr), Some(ACTIVE.0));
        assert_eq!(
            mark_colour(80.0, 50.0, TextDirection::Ltr),
            Some(INACTIVE.0)
        );
    }

    #[test]
    fn and_right_to_left_it_is_the_other_way_round() {
        // The arm nothing was looking at. Written as the mirror of the test
        // above rather than as two more numbers: in an RTL locale the thumb
        // sweeps leftwards, so a mark to its *right* is the one it has passed.
        // Get this wrong and every tick on every slider in every RTL language
        // is coloured on the wrong side, which no LTR test can see.
        for (mark, thumb) in [(20.0, 50.0), (80.0, 50.0), (10.0, 90.0)] {
            let ltr = mark_colour(mark, thumb, TextDirection::Ltr);
            let rtl = mark_colour(mark, thumb, TextDirection::Rtl);
            assert!(ltr.is_some() && rtl.is_some());
            assert_ne!(
                ltr, rtl,
                "mark at {mark} with the thumb at {thumb}: the two directions \
                 have to disagree, or one of them is not being read"
            );
        }
    }

    #[test]
    fn a_mark_under_the_thumb_is_active_in_both_directions() {
        // The boundary, and the one place the two agree: the comparison is
        // strict on both sides, so an offset of exactly zero falls to the
        // active branch either way. That is what stops the mark the thumb is
        // sitting on from flickering as it crosses.
        assert_eq!(mark_colour(50.0, 50.0, TextDirection::Ltr), Some(ACTIVE.0));
        assert_eq!(mark_colour(50.0, 50.0, TextDirection::Rtl), Some(ACTIVE.0));
    }

    #[test]
    fn a_round_tick_mark_has_a_size_and_the_empty_one_has_none() {
        // The other surviving arm. `Round` answering `Size::ZERO` -- the empty
        // shape's answer -- makes `paint` draw nothing, because it guards on a
        // positive radius. A slider that quietly stopped showing its
        // divisions would look like a slider that had none.
        let theme = theme();
        let round = SliderTickMarkShape::Round(RoundSliderTickMarkShape::new());
        let size = round.preferred_size(&theme);
        assert!(size.width > 0.0, "{size:?}");
        assert_eq!(
            SliderTickMarkShape::Empty.preferred_size(&theme),
            Size::ZERO
        );
        assert_ne!(size, Size::ZERO);
    }

    #[test]
    fn an_unset_radius_is_a_quarter_of_the_track_height() {
        // Upstream's default, and the reason the field is nullable rather than
        // a number: a slider with a taller track gets bigger divisions without
        // anybody restating them.
        let theme = theme();
        let default = RoundSliderTickMarkShape::new().preferred_size(&theme);
        assert_eq!(default, Size::from_radius(TRACK_HEIGHT / 4.0));

        let asked = RoundSliderTickMarkShape::with_radius(5.0).preferred_size(&theme);
        assert_eq!(asked, Size::from_radius(5.0));
        assert_ne!(asked, default, "and a radius given is a radius used");
    }

    #[test]
    fn the_empty_shape_draws_nothing_whatever_it_is_asked() {
        let mut layers = LayerTree::new(200, 200);
        reset_drawn();
        {
            let mut context = PaintContext::new(&mut layers, Size::new(200.0, 200.0));
            SliderTickMarkShape::Empty.paint(
                context.canvas(),
                Offset::new(20.0, 10.0),
                &theme(),
                Offset::new(50.0, 10.0),
                TextDirection::Ltr,
                1.0,
            );
        }
        assert!(drawn().is_empty());
    }
}

