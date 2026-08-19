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
//! * The four `range*Shape` fields wait for `range_slider_parts.dart`; the
//!   types they would be typed against are not written yet. `thumbSelector`,
//!   a `RangeThumbSelector`, waits with them.
//! * Upstream's thumb draws its elevation with `Canvas.drawShadow`, which the
//!   engine binding does not expose. It is drawn here as the circles of
//!   [`elevation_shadows`](crate::painting::elevation_shadows), which is the
//!   same table `drawShadow` consults.

use crate::animation::{ColorTween, Tween};
use crate::borders::{BorderRadius, EdgeInsetsGeometry, Radius};
use crate::component_themes::{lerp_color, lerp_f32, lerp_nearer};
use crate::direction::TextDirection;
use crate::engine::{Canvas, Color, Paint, Rect, TextStyle};
use crate::framework::{AnyWidget, BuildContext, provide};
use crate::painting::{ClipOp, elevation_shadows};
use crate::render::{Offset, Size};
use crate::services::system::SystemMouseCursor;
use crate::theme::ThemeData;
use crate::widget_state::{StateProperty, WidgetStates};

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

        let left = Rect::ltrb(track.left, track.top, thumb, track.bottom);
        if left.width() > 0.0 {
            canvas.draw_rect(left, &Paint::new(leading));
        }
        let right = Rect::ltrb(thumb, track.top, track.right, track.bottom);
        if right.width() > 0.0 {
            canvas.draw_rect(right, &Paint::new(trailing));
        }

        if let Some((secondary, color)) = geometry.secondary_segment(theme, false) {
            if secondary.width() > 0.0 {
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
fn enabled_color(disabled: Option<Color>, enabled: Option<Color>, t: f32) -> Option<Color> {
    match (disabled, enabled) {
        (Some(disabled), Some(enabled)) => Some(ColorTween::new(disabled, enabled).lerp(t)),
        (first, second) => second.or(first),
    }
}

/// Fills a rounded rectangle whose corners are not all the same, which the
/// binding has no single call for -- the same hand-walked path
/// [`OutlineInputBorder`](crate::borders::OutlineInputBorder) draws.
fn fill_rrect(canvas: &mut Canvas, rect: Rect, radius: BorderRadius, color: Color) {
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
    pub show_value_indicator: Option<ShowValueIndicator>,
    pub value_indicator_text_style: Option<TextStyle>,
    /// How close a range slider's two thumbs may come.
    pub min_thumb_separation: Option<f32>,
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
            // Upstream also sets `valueIndicatorShape` and the four range
            // shapes here; both wait for the value indicators and the range
            // parts, as the module comment records.
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
            value_indicator_text_style: lerp_nearer(
                &a.value_indicator_text_style,
                &b.value_indicator_text_style,
                t,
            ),
            min_thumb_separation: lerp_f32(a.min_thumb_separation, b.min_thumb_separation, t),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
            allowed_interaction: lerp_nearer(&a.allowed_interaction, &b.allowed_interaction, t),
            padding: lerp_nearer(&a.padding, &b.padding, t),
            thumb_size: lerp_nearer(&a.thumb_size, &b.thumb_size, t),
            track_gap: lerp_f32(a.track_gap, b.track_gap, t),
            year_2023: lerp_nearer(&a.year_2023, &b.year_2023, t),
        }
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
}
