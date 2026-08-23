// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The per-control themes, from upstream's `material/*_theme.dart` files.
//!
//! Upstream gives every control family a pair: a `*ThemeData` of nullable
//! overrides, and a `*Theme` widget that installs one for a subtree. A
//! control reads its own with `of(context)`, which looks for the nearest
//! installed one and falls back to the field on [`ThemeData`] -- and a field
//! still unset falls back to whatever the control's own default is, which is
//! usually a colour off the [`ColorScheme`](crate::color_scheme::ColorScheme).
//!
//! That three-step fallback is the whole mechanism, and it is why a nullable
//! field here is not laziness: an unset field means "whatever the theme says",
//! and only a set one overrides.
//!
//! # One family at a time
//!
//! Upstream's `ThemeData` carries about forty-five of these. They arrive here
//! with the controls they configure -- a component theme with no control
//! reading it is a data class nobody reads. This file holds the ones whose
//! control is already in the crate; each later cluster adds its own.
//!
//! # Recorded divergences
//!
//! * `clipBehavior` is not modelled. It is a `dart:ui` `Clip`, and this
//!   crate's clipping is a property of the render object that does it
//!   (`RenderClipRect` and friends), not a value a theme passes down.
//! * Upstream's `*ThemeData` classes carry a `debugFillProperties`; the
//!   diagnostics tree is not ported (P10).

use crate::animation::Tween;
use crate::borders::{BorderRadiusGeometry, BorderSide, EdgeInsetsGeometry, ShapeBorder};
use crate::color_scheme::ColorScheme;
use crate::controls::TooltipTriggerMode;
use crate::editable_text::TargetPlatform;
use crate::engine::{Color, TextAlign, TextStyle};
use crate::framework::{AnyWidget, BuildContext, provide};
use crate::painting::StrokeCap;
use crate::platform::Brightness;
use crate::render::{AlignmentGeometry, BoxConstraints, EdgeInsets, Offset, Size};
use crate::services::system::SystemMouseCursor;
use crate::theme::{ThemeData, VisualDensity};
use crate::widget_state::{
    MaterialTapTargetSize, StateProperty, WidgetState, WidgetStates, lerp_state_property,
};

/// Interpolates two optional colours, as every `*ThemeData.lerp` upstream does
/// through `Color.lerp`.
///
/// # A null end fades, it does not switch
///
/// `Color.lerp(null, y, t)` is `_scaleAlpha(y, t)` and `Color.lerp(x, null, t)`
/// is `_scaleAlpha(x, 1 - t)`: an absent colour behaves as *transparent* at
/// that end, so the other end fades in or out.
///
/// This used to step -- answer `a` below the halfway point and `b` above it --
/// while its own doc claimed to follow `Color.lerp`. The two differ everywhere
/// except the ends, and the difference is visible: a theme transition where one
/// side sets a colour and the other does not would flick it on halfway through
/// instead of bringing it in.
pub(crate) fn lerp_color(a: Option<Color>, b: Option<Color>, t: f32) -> Option<Color> {
    fn scale_alpha(color: Color, factor: f32) -> Color {
        color.with_alpha((color.alpha() as f32 * factor.clamp(0.0, 1.0)).round() as u8)
    }
    match (a, b) {
        (None, None) => None,
        (Some(a), Some(b)) => Some(crate::animation::ColorTween { begin: a, end: b }.lerp(t)),
        (Some(a), None) => Some(scale_alpha(a, 1.0 - t)),
        (None, Some(b)) => Some(scale_alpha(b, t)),
    }
}

/// The same for a number, following upstream's `lerpDouble`.
///
/// # A null end is **zero**, not a step
///
/// `lerpDouble` is `a ??= 0.0; b ??= 0.0; a * (1 - t) + b * t`. An absent
/// number behaves as nothing-of-that-quantity, so an elevation that one theme
/// sets and the other does not sinks to the surface rather than snapping there
/// halfway. This had the same step-at-the-middle rule
/// [`lerp_color`] did, and for the same reason: the two agree at the ends and
/// nowhere in between.
///
/// `lerpDouble` returns `a` outright when `a == b`, which is why two absent
/// numbers stay absent instead of interpolating between two zeroes and
/// answering `Some(0.0)`.
pub(crate) fn lerp_f32(a: Option<f32>, b: Option<f32>, t: f32) -> Option<f32> {
    if a == b {
        return a;
    }
    let from = a.unwrap_or(0.0);
    let to = b.unwrap_or(0.0);
    Some(from * (1.0 - t) + to * t)
}

/// A colour property, both ends resolved against the same states and then
/// blended -- upstream's
/// `WidgetStateProperty.lerp<Color?>(a, b, t, Color.lerp)`.
fn lerp_state_color(
    a: Option<&StateProperty<Option<Color>>>,
    b: Option<&StateProperty<Option<Color>>>,
    t: f32,
) -> Option<StateProperty<Option<Color>>> {
    lerp_state_property(a, b, t, |first, second, t| {
        lerp_color(first.flatten(), second.flatten(), t)
    })
}

/// Two optional icon themes interpolated.
fn lerp_icon_theme(
    a: &Option<IconThemeData>,
    b: &Option<IconThemeData>,
    t: f32,
) -> Option<IconThemeData> {
    match (a, b) {
        (Some(first), Some(second)) => Some(IconThemeData::lerp(first, second, t)),
        (first, second) => {
            if t < 0.5 {
                first.clone()
            } else {
                second.clone()
            }
        }
    }
}

/// Anything else: taken from whichever end is nearer, which is what
/// upstream's `lerp` does for the fields it cannot interpolate.
pub(crate) fn lerp_nearer<T: Clone>(a: &Option<T>, b: &Option<T>, t: f32) -> Option<T> {
    if t < 0.5 { a.clone() } else { b.clone() }
}

// -- Divider (upstream `divider_theme.dart`) ----------------------------------

/// Upstream `DividerThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DividerThemeData {
    /// The line's colour. Unset means [`ThemeData::divider_color`].
    pub color: Option<Color>,
    /// How much room the divider takes along its cross axis, line included.
    pub space: Option<f32>,
    /// How thick the line is. Zero draws a hairline.
    pub thickness: Option<f32>,
    /// Empty space before the line starts.
    pub indent: Option<f32>,
    /// Empty space after it ends.
    pub end_indent: Option<f32>,
    pub radius: Option<BorderRadiusGeometry>,
}

impl DividerThemeData {
    pub const fn new() -> DividerThemeData {
        DividerThemeData {
            color: None,
            space: None,
            thickness: None,
            indent: None,
            end_indent: None,
            radius: None,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_space(mut self, space: f32) -> Self {
        self.space = Some(space);
        self
    }

    pub fn with_thickness(mut self, thickness: f32) -> Self {
        self.thickness = Some(thickness);
        self
    }

    pub fn with_indents(mut self, indent: f32, end_indent: f32) -> Self {
        self.indent = Some(indent);
        self.end_indent = Some(end_indent);
        self
    }

    pub fn with_radius(mut self, radius: BorderRadiusGeometry) -> Self {
        self.radius = Some(radius);
        self
    }

    /// Upstream `DividerThemeData.lerp`.
    pub fn lerp(a: &DividerThemeData, b: &DividerThemeData, t: f32) -> DividerThemeData {
        DividerThemeData {
            color: lerp_color(a.color, b.color, t),
            space: lerp_f32(a.space, b.space, t),
            thickness: lerp_f32(a.thickness, b.thickness, t),
            indent: lerp_f32(a.indent, b.indent, t),
            end_indent: lerp_f32(a.end_indent, b.end_indent, t),
            radius: lerp_nearer(&a.radius, &b.radius, t),
        }
    }
}

/// Upstream `DividerTheme`.
pub struct DividerTheme;

impl DividerTheme {
    /// Installs one for a subtree.
    pub fn new(data: DividerThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    /// Upstream `DividerTheme.of`: the nearest installed one, or the field on
    /// the ambient [`ThemeData`].
    pub fn of(context: &mut BuildContext) -> DividerThemeData {
        context
            .inherited::<DividerThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).divider_theme)
    }
}

// -- Card (upstream `card_theme.dart`) ----------------------------------------

/// Upstream `CardThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CardThemeData {
    pub color: Option<Color>,
    pub shadow_color: Option<Color>,
    /// The tint a raised surface takes in a dark theme -- upstream's
    /// `surfaceTintColor`, applied when
    /// [`ThemeData::apply_elevation_overlay_color`] is on.
    pub surface_tint_color: Option<Color>,
    pub elevation: Option<f32>,
    pub margin: Option<EdgeInsetsGeometry>,
    pub shape: Option<ShapeBorder>,
}

impl CardThemeData {
    pub const fn new() -> CardThemeData {
        CardThemeData {
            color: None,
            shadow_color: None,
            surface_tint_color: None,
            elevation: None,
            margin: None,
            shape: None,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }

    pub fn with_margin(mut self, margin: EdgeInsetsGeometry) -> Self {
        self.margin = Some(margin);
        self
    }

    pub fn with_shape(mut self, shape: ShapeBorder) -> Self {
        self.shape = Some(shape);
        self
    }

    /// Upstream `CardThemeData.lerp`.
    pub fn lerp(a: &CardThemeData, b: &CardThemeData, t: f32) -> CardThemeData {
        CardThemeData {
            color: lerp_color(a.color, b.color, t),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            margin: lerp_nearer(&a.margin, &b.margin, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
        }
    }
}

/// Upstream `CardTheme`.
pub struct CardTheme;

impl CardTheme {
    pub fn new(data: CardThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> CardThemeData {
        context
            .inherited::<CardThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).card_theme)
    }
}

// -- Badge (upstream `badge_theme.dart`) --------------------------------------

/// Upstream `BadgeThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BadgeThemeData {
    pub background_color: Option<Color>,
    pub text_color: Option<Color>,
    /// The diameter of a badge with no label -- upstream's `smallSize`.
    pub small_size: Option<f32>,
    /// The height of a badge with one -- upstream's `largeSize`.
    pub large_size: Option<f32>,
    pub text_style: Option<TextStyle>,
    pub padding: Option<EdgeInsetsGeometry>,
    pub alignment: Option<AlignmentGeometry>,
    pub offset: Option<Offset>,
}

impl BadgeThemeData {
    pub fn new() -> BadgeThemeData {
        BadgeThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }

    pub fn with_sizes(mut self, small: f32, large: f32) -> Self {
        self.small_size = Some(small);
        self.large_size = Some(large);
        self
    }

    /// Upstream `BadgeThemeData.lerp`.
    pub fn lerp(a: &BadgeThemeData, b: &BadgeThemeData, t: f32) -> BadgeThemeData {
        BadgeThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            text_color: lerp_color(a.text_color, b.text_color, t),
            small_size: lerp_f32(a.small_size, b.small_size, t),
            large_size: lerp_f32(a.large_size, b.large_size, t),
            text_style: lerp_nearer(&a.text_style, &b.text_style, t),
            padding: lerp_nearer(&a.padding, &b.padding, t),
            alignment: lerp_nearer(&a.alignment, &b.alignment, t),
            offset: match (a.offset, b.offset) {
                (Some(a), Some(b)) => Some(Offset::new(
                    a.dx + (b.dx - a.dx) * t,
                    a.dy + (b.dy - a.dy) * t,
                )),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
        }
    }
}

/// Upstream `BadgeTheme`.
pub struct BadgeTheme;

impl BadgeTheme {
    pub fn new(data: BadgeThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> BadgeThemeData {
        context
            .inherited::<BadgeThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).badge_theme)
    }
}

// -- Tooltip (upstream `tooltip_theme.dart`) ----------------------------------

/// Upstream `TooltipThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TooltipThemeData {
    pub height: Option<f32>,
    pub constraints: Option<BoxConstraints>,
    pub padding: Option<EdgeInsetsGeometry>,
    pub margin: Option<EdgeInsetsGeometry>,
    /// How far above or below the target the tip sits.
    pub vertical_offset: Option<f32>,
    /// Whether it prefers to sit below rather than above.
    pub prefer_below: Option<bool>,
    pub exclude_from_semantics: Option<bool>,
    pub decoration: Option<crate::decoration::Decoration>,
    pub text_style: Option<TextStyle>,
    pub text_align: Option<TextAlign>,
    pub wait_duration: Option<std::time::Duration>,
    pub show_duration: Option<std::time::Duration>,
    pub exit_duration: Option<std::time::Duration>,
    pub trigger_mode: Option<TooltipTriggerMode>,
    pub enable_feedback: Option<bool>,
}

impl TooltipThemeData {
    pub fn new() -> TooltipThemeData {
        TooltipThemeData::default()
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    pub fn with_vertical_offset(mut self, offset: f32) -> Self {
        self.vertical_offset = Some(offset);
        self
    }

    pub fn with_prefer_below(mut self, prefer_below: bool) -> Self {
        self.prefer_below = Some(prefer_below);
        self
    }

    pub fn with_trigger_mode(mut self, mode: TooltipTriggerMode) -> Self {
        self.trigger_mode = Some(mode);
        self
    }

    /// Upstream `TooltipThemeData.lerp`, which interpolates the two numbers
    /// and takes everything else from the nearer end.
    pub fn lerp(a: &TooltipThemeData, b: &TooltipThemeData, t: f32) -> TooltipThemeData {
        TooltipThemeData {
            height: lerp_f32(a.height, b.height, t),
            vertical_offset: lerp_f32(a.vertical_offset, b.vertical_offset, t),
            constraints: lerp_nearer(&a.constraints, &b.constraints, t),
            padding: lerp_nearer(&a.padding, &b.padding, t),
            margin: lerp_nearer(&a.margin, &b.margin, t),
            prefer_below: lerp_nearer(&a.prefer_below, &b.prefer_below, t),
            exclude_from_semantics: lerp_nearer(
                &a.exclude_from_semantics,
                &b.exclude_from_semantics,
                t,
            ),
            decoration: lerp_nearer(&a.decoration, &b.decoration, t),
            text_style: lerp_nearer(&a.text_style, &b.text_style, t),
            text_align: lerp_nearer(&a.text_align, &b.text_align, t),
            wait_duration: lerp_nearer(&a.wait_duration, &b.wait_duration, t),
            show_duration: lerp_nearer(&a.show_duration, &b.show_duration, t),
            exit_duration: lerp_nearer(&a.exit_duration, &b.exit_duration, t),
            trigger_mode: lerp_nearer(&a.trigger_mode, &b.trigger_mode, t),
            enable_feedback: lerp_nearer(&a.enable_feedback, &b.enable_feedback, t),
        }
    }
}

/// Upstream `TooltipTheme`.
pub struct TooltipTheme;

impl TooltipTheme {
    pub fn new(data: TooltipThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> TooltipThemeData {
        context
            .inherited::<TooltipThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).tooltip_theme)
    }
}

// -- Progress indicator (upstream `progress_indicator_theme.dart`) ------------

/// Upstream `ProgressIndicatorThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgressIndicatorThemeData {
    /// The bar or arc itself. Unset means the scheme's primary.
    pub color: Option<Color>,
    pub linear_track_color: Option<Color>,
    pub linear_min_height: Option<f32>,
    pub circular_track_color: Option<Color>,
    pub refresh_background_color: Option<Color>,
    pub border_radius: Option<BorderRadiusGeometry>,
    /// The dot a Material 3 indicator leaves at the end of its track.
    pub stop_indicator_color: Option<Color>,
    pub stop_indicator_radius: Option<f32>,
    pub stroke_width: Option<f32>,
    /// Where the stroke sits relative to the arc: -1 inside, 0 centred,
    /// 1 outside.
    pub stroke_align: Option<f32>,
    pub stroke_cap: Option<StrokeCap>,
    pub constraints: Option<BoxConstraints>,
    /// The gap between the track and the indicator, Material 3's.
    pub track_gap: Option<f32>,
    pub circular_track_padding: Option<EdgeInsetsGeometry>,
}

impl ProgressIndicatorThemeData {
    pub const fn new() -> ProgressIndicatorThemeData {
        ProgressIndicatorThemeData {
            color: None,
            linear_track_color: None,
            linear_min_height: None,
            circular_track_color: None,
            refresh_background_color: None,
            border_radius: None,
            stop_indicator_color: None,
            stop_indicator_radius: None,
            stroke_width: None,
            stroke_align: None,
            stroke_cap: None,
            constraints: None,
            track_gap: None,
            circular_track_padding: None,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_linear_track_color(mut self, color: Color) -> Self {
        self.linear_track_color = Some(color);
        self
    }

    pub fn with_linear_min_height(mut self, height: f32) -> Self {
        self.linear_min_height = Some(height);
        self
    }

    /// Upstream `ProgressIndicatorThemeData.lerp`.
    pub fn lerp(
        a: &ProgressIndicatorThemeData,
        b: &ProgressIndicatorThemeData,
        t: f32,
    ) -> ProgressIndicatorThemeData {
        ProgressIndicatorThemeData {
            color: lerp_color(a.color, b.color, t),
            linear_track_color: lerp_color(a.linear_track_color, b.linear_track_color, t),
            linear_min_height: lerp_f32(a.linear_min_height, b.linear_min_height, t),
            circular_track_color: lerp_color(a.circular_track_color, b.circular_track_color, t),
            refresh_background_color: lerp_color(
                a.refresh_background_color,
                b.refresh_background_color,
                t,
            ),
            border_radius: lerp_nearer(&a.border_radius, &b.border_radius, t),
            stop_indicator_color: lerp_color(a.stop_indicator_color, b.stop_indicator_color, t),
            stop_indicator_radius: lerp_f32(a.stop_indicator_radius, b.stop_indicator_radius, t),
            stroke_width: lerp_f32(a.stroke_width, b.stroke_width, t),
            stroke_align: lerp_f32(a.stroke_align, b.stroke_align, t),
            stroke_cap: lerp_nearer(&a.stroke_cap, &b.stroke_cap, t),
            constraints: lerp_nearer(&a.constraints, &b.constraints, t),
            track_gap: lerp_f32(a.track_gap, b.track_gap, t),
            circular_track_padding: lerp_nearer(
                &a.circular_track_padding,
                &b.circular_track_padding,
                t,
            ),
        }
    }
}

/// Upstream `ProgressIndicatorTheme`.
pub struct ProgressIndicatorTheme;

impl ProgressIndicatorTheme {
    pub fn new(data: ProgressIndicatorThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> ProgressIndicatorThemeData {
        context
            .inherited::<ProgressIndicatorThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).progress_indicator_theme)
    }
}

// -- Checkbox (upstream `checkbox_theme.dart`) --------------------------------

/// Upstream `CheckboxThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CheckboxThemeData {
    pub mouse_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
    /// The box's fill, by state -- unset means the scheme's primary when
    /// checked and nothing when not.
    pub fill_color: Option<StateProperty<Option<Color>>>,
    /// The tick.
    pub check_color: Option<StateProperty<Option<Color>>>,
    /// The ink under the pointer.
    pub overlay_color: Option<StateProperty<Option<Color>>>,
    pub splash_radius: Option<f32>,
    pub material_tap_target_size: Option<MaterialTapTargetSize>,
    pub visual_density: Option<VisualDensity>,
    pub shape: Option<ShapeBorder>,
    pub side: Option<BorderSide>,
}

impl CheckboxThemeData {
    pub fn new() -> CheckboxThemeData {
        CheckboxThemeData::default()
    }

    pub fn with_fill_color(mut self, fill: StateProperty<Option<Color>>) -> Self {
        self.fill_color = Some(fill);
        self
    }

    pub fn with_check_color(mut self, check: StateProperty<Option<Color>>) -> Self {
        self.check_color = Some(check);
        self
    }

    pub fn with_side(mut self, side: BorderSide) -> Self {
        self.side = Some(side);
        self
    }

    pub fn with_material_tap_target_size(mut self, size: MaterialTapTargetSize) -> Self {
        self.material_tap_target_size = Some(size);
        self
    }

    /// Upstream `CheckboxThemeData.lerp`.
    pub fn lerp(a: &CheckboxThemeData, b: &CheckboxThemeData, t: f32) -> CheckboxThemeData {
        CheckboxThemeData {
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
            fill_color: lerp_state_color(a.fill_color.as_ref(), b.fill_color.as_ref(), t),
            check_color: lerp_state_color(a.check_color.as_ref(), b.check_color.as_ref(), t),
            overlay_color: lerp_state_color(a.overlay_color.as_ref(), b.overlay_color.as_ref(), t),
            splash_radius: lerp_f32(a.splash_radius, b.splash_radius, t),
            material_tap_target_size: lerp_nearer(
                &a.material_tap_target_size,
                &b.material_tap_target_size,
                t,
            ),
            visual_density: match (a.visual_density, b.visual_density) {
                (Some(first), Some(second)) => Some(VisualDensity::lerp(first, second, t)),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
            shape: lerp_nearer(&a.shape, &b.shape, t),
            side: match (a.side, b.side) {
                (Some(first), Some(second)) => Some(BorderSide::lerp(first, second, t)),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
        }
    }
}

/// Upstream `CheckboxTheme`.
pub struct CheckboxTheme;

impl CheckboxTheme {
    pub fn new(data: CheckboxThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> CheckboxThemeData {
        context
            .inherited::<CheckboxThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).checkbox_theme)
    }
}

// -- Radio (upstream `radio_theme.dart`) --------------------------------------

/// Upstream `RadioThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RadioThemeData {
    pub mouse_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
    pub fill_color: Option<StateProperty<Option<Color>>>,
    pub overlay_color: Option<StateProperty<Option<Color>>>,
    pub splash_radius: Option<f32>,
    pub material_tap_target_size: Option<MaterialTapTargetSize>,
    pub visual_density: Option<VisualDensity>,
    pub background_color: Option<StateProperty<Option<Color>>>,
    pub side: Option<BorderSide>,
    /// The filled dot's radius, by state.
    pub inner_radius: Option<StateProperty<Option<f32>>>,
}

impl RadioThemeData {
    pub fn new() -> RadioThemeData {
        RadioThemeData::default()
    }

    pub fn with_fill_color(mut self, fill: StateProperty<Option<Color>>) -> Self {
        self.fill_color = Some(fill);
        self
    }

    pub fn with_inner_radius(mut self, radius: StateProperty<Option<f32>>) -> Self {
        self.inner_radius = Some(radius);
        self
    }

    /// Upstream `RadioThemeData.lerp`.
    pub fn lerp(a: &RadioThemeData, b: &RadioThemeData, t: f32) -> RadioThemeData {
        RadioThemeData {
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
            fill_color: lerp_state_color(a.fill_color.as_ref(), b.fill_color.as_ref(), t),
            overlay_color: lerp_state_color(a.overlay_color.as_ref(), b.overlay_color.as_ref(), t),
            splash_radius: lerp_f32(a.splash_radius, b.splash_radius, t),
            material_tap_target_size: lerp_nearer(
                &a.material_tap_target_size,
                &b.material_tap_target_size,
                t,
            ),
            visual_density: match (a.visual_density, b.visual_density) {
                (Some(first), Some(second)) => Some(VisualDensity::lerp(first, second, t)),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
            background_color: lerp_state_color(
                a.background_color.as_ref(),
                b.background_color.as_ref(),
                t,
            ),
            side: match (a.side, b.side) {
                (Some(first), Some(second)) => Some(BorderSide::lerp(first, second, t)),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
            inner_radius: lerp_nearer(&a.inner_radius, &b.inner_radius, t),
        }
    }
}

/// Upstream `RadioTheme`.
pub struct RadioTheme;

impl RadioTheme {
    pub fn new(data: RadioThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> RadioThemeData {
        context
            .inherited::<RadioThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).radio_theme)
    }
}

// -- Switch (upstream `switch_theme.dart`) ------------------------------------

/// Upstream `SwitchThemeData`.
///
/// `thumbIcon` is not here: it is a `WidgetStateProperty<Icon?>`, and the
/// framework has no icon system yet (`E5` in the plan).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SwitchThemeData {
    pub thumb_color: Option<StateProperty<Option<Color>>>,
    pub track_color: Option<StateProperty<Option<Color>>>,
    pub track_outline_color: Option<StateProperty<Option<Color>>>,
    pub track_outline_width: Option<StateProperty<Option<f32>>>,
    pub material_tap_target_size: Option<MaterialTapTargetSize>,
    pub mouse_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
    pub overlay_color: Option<StateProperty<Option<Color>>>,
    pub splash_radius: Option<f32>,
    pub padding: Option<EdgeInsetsGeometry>,
}

impl SwitchThemeData {
    pub fn new() -> SwitchThemeData {
        SwitchThemeData::default()
    }

    pub fn with_thumb_color(mut self, thumb: StateProperty<Option<Color>>) -> Self {
        self.thumb_color = Some(thumb);
        self
    }

    pub fn with_track_color(mut self, track: StateProperty<Option<Color>>) -> Self {
        self.track_color = Some(track);
        self
    }

    /// Upstream `SwitchThemeData.lerp`.
    pub fn lerp(a: &SwitchThemeData, b: &SwitchThemeData, t: f32) -> SwitchThemeData {
        SwitchThemeData {
            thumb_color: lerp_state_color(a.thumb_color.as_ref(), b.thumb_color.as_ref(), t),
            track_color: lerp_state_color(a.track_color.as_ref(), b.track_color.as_ref(), t),
            track_outline_color: lerp_state_color(
                a.track_outline_color.as_ref(),
                b.track_outline_color.as_ref(),
                t,
            ),
            track_outline_width: lerp_nearer(&a.track_outline_width, &b.track_outline_width, t),
            material_tap_target_size: lerp_nearer(
                &a.material_tap_target_size,
                &b.material_tap_target_size,
                t,
            ),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
            overlay_color: lerp_state_color(a.overlay_color.as_ref(), b.overlay_color.as_ref(), t),
            splash_radius: lerp_f32(a.splash_radius, b.splash_radius, t),
            padding: lerp_nearer(&a.padding, &b.padding, t),
        }
    }
}

/// Upstream `SwitchTheme`.
pub struct SwitchTheme;

impl SwitchTheme {
    pub fn new(data: SwitchThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> SwitchThemeData {
        context
            .inherited::<SwitchThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).switch_theme)
    }
}

/// What a checkbox draws with, once the three steps have run -- upstream's
/// `Checkbox.build` reading `CheckboxTheme.of` and then its own defaults.
pub struct ResolvedCheckbox {
    pub fill: Color,
    pub check: Color,
    pub side: BorderSide,
    pub tap_target_size: MaterialTapTargetSize,
}

impl ResolvedCheckbox {
    pub fn of(context: &mut BuildContext, states: WidgetStates) -> ResolvedCheckbox {
        let data = CheckboxTheme::of(context);
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let selected = states.contains(WidgetState::Selected);
        let disabled = states.contains(WidgetState::Disabled);
        // Upstream's `_defaultFillColor`: the primary when checked, nothing
        // when not, and the disabled colour over both.
        let default_fill = if disabled {
            if selected {
                scheme.on_surface.with_alpha(0x61)
            } else {
                Color::TRANSPARENT
            }
        } else if selected {
            scheme.primary
        } else {
            Color::TRANSPARENT
        };
        ResolvedCheckbox {
            fill: data
                .fill_color
                .as_ref()
                .and_then(|property| property.resolve(states))
                .unwrap_or(default_fill),
            check: data
                .check_color
                .as_ref()
                .and_then(|property| property.resolve(states))
                .unwrap_or(scheme.on_primary),
            side: data.side.unwrap_or(BorderSide {
                color: if disabled {
                    scheme.on_surface.with_alpha(0x61)
                } else {
                    scheme.on_surface_variant()
                },
                width: 2.0,
                ..BorderSide::NONE
            }),
            tap_target_size: data.material_tap_target_size.unwrap_or_default(),
        }
    }
}

/// What a radio draws with, once the theme and the defaults have both had a
/// say -- upstream's `Radio.build` reading `RadioTheme.of` and then its own.
///
/// # Two radii, and the inner one is a state property
///
/// Upstream's `_kOuterRadius` is 8 and `_kInnerRadius` is 4.5, and the inner
/// one is overridable *per state* while the outer one is not. That asymmetry is
/// the animation: a radio fills in by growing its dot from nothing, so the dot
/// is the part that has a size per state and the ring is the part that stays
/// put. An unselected radio's inner radius is zero, and that is not a special
/// case -- it is the same property answering for a different state.
pub struct ResolvedRadio {
    /// The dot, and the ring when it is filled.
    pub fill: Color,
    pub side: BorderSide,
    /// Upstream's `_kOuterRadius`.
    pub outer_radius: f32,
    /// Upstream's `innerRadius` property, resolved. Zero when unselected.
    pub inner_radius: f32,
    pub background: Option<Color>,
    pub tap_target_size: MaterialTapTargetSize,
}

impl ResolvedRadio {
    /// Upstream's `_kOuterRadius`.
    pub const OUTER_RADIUS: f32 = 8.0;
    /// Upstream's `_kInnerRadius`.
    pub const INNER_RADIUS: f32 = 4.5;

    pub fn of(context: &mut BuildContext, states: WidgetStates) -> ResolvedRadio {
        let data = RadioTheme::of(context);
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let selected = states.contains(WidgetState::Selected);
        let disabled = states.contains(WidgetState::Disabled);
        // Upstream's `_defaultFillColor`: the primary when chosen, the outline
        // when not, and the disabled colour over both.
        let default_fill = if disabled {
            scheme.on_surface.with_alpha(0x61)
        } else if selected {
            scheme.primary
        } else {
            scheme.on_surface_variant()
        };
        let fill = data
            .fill_color
            .as_ref()
            .and_then(|property| property.resolve(states))
            .unwrap_or(default_fill);
        ResolvedRadio {
            fill,
            // The ring is the fill colour, not a colour of its own: upstream
            // paints the outline with the same `_defaultFillColor` it paints
            // the dot with, which is why choosing a radio colours both at once.
            side: data.side.unwrap_or(BorderSide {
                color: fill,
                width: 2.0,
                ..BorderSide::NONE
            }),
            outer_radius: ResolvedRadio::OUTER_RADIUS,
            inner_radius: data
                .inner_radius
                .as_ref()
                .and_then(|property| property.resolve(states))
                .unwrap_or(if selected {
                    ResolvedRadio::INNER_RADIUS
                } else {
                    0.0
                }),
            background: data
                .background_color
                .as_ref()
                .and_then(|property| property.resolve(states)),
            tap_target_size: data.material_tap_target_size.unwrap_or_default(),
        }
    }
}

/// What a badge draws with, once the widget, the theme and the M3 defaults have
/// each had their say -- upstream's `Badge.build` reading `BadgeTheme.of` and
/// then `_BadgeDefaultsM3`.
///
/// # A badge is red because of what it is for
///
/// The default background is `colorScheme.error` and not the primary. A badge
/// is a count of things demanding attention, and the scheme already has a
/// colour that means exactly that. Using the primary would make a badge look
/// like a decoration of the thing it is sitting on.
///
/// # Two sizes, because there are two badges
///
/// `smallSize` is the diameter of a badge with *no* label -- the bare dot that
/// says "something happened" without saying how much -- and `largeSize` is the
/// height of one with a label. They are separate numbers rather than one
/// scaled, because the dot is not a small stadium: it has no text to leave room
/// for and its size is chosen to read as a mark rather than as a shape.
pub struct ResolvedBadge {
    pub background: Color,
    pub text_color: Color,
    pub small_size: f32,
    pub large_size: f32,
    pub padding: EdgeInsets,
    pub alignment: AlignmentGeometry,
    pub offset: Offset,
    pub text_style: Option<TextStyle>,
}

impl ResolvedBadge {
    /// Upstream's `_BadgeDefaultsM3`.
    pub const SMALL_SIZE: f32 = 6.0;
    pub const LARGE_SIZE: f32 = 16.0;

    pub fn of(context: &mut BuildContext) -> ResolvedBadge {
        let data = BadgeTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        ResolvedBadge {
            background: data.background_color.unwrap_or(scheme.error),
            text_color: data.text_color.unwrap_or(scheme.on_error),
            small_size: data.small_size.unwrap_or(ResolvedBadge::SMALL_SIZE),
            large_size: data.large_size.unwrap_or(ResolvedBadge::LARGE_SIZE),
            // Upstream's `EdgeInsets.symmetric(horizontal: 4)`: room at the
            // sides only, because the height is `largeSize` and padding on top
            // of that would fight it.
            padding: data
                .padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::symmetric(4.0, 0.0)),
            alignment: data.alignment.unwrap_or(AlignmentGeometry::Directional(
                crate::render::AlignmentDirectional::TOP_END,
            )),
            // Upstream's default is `(4, -4)` reading left-to-right and
            // `(-4, -4)` the other way -- out past the corner and up. The
            // `(0, 8)` added to it is not design: upstream's own comment says
            // it was put there so that a change to the positioning arithmetic
            // would not move every existing badge. Kept, because a badge that
            // sits eight pixels from where upstream's does is wrong however
            // defensible the reason.
            offset: {
                let asked = data
                    .offset
                    .unwrap_or(match crate::direction::current_direction() {
                        crate::direction::TextDirection::Ltr => Offset::new(4.0, -4.0),
                        crate::direction::TextDirection::Rtl => Offset::new(-4.0, -4.0),
                    });
                Offset::new(asked.dx, asked.dy + 8.0)
            },
            text_style: data.text_style.clone(),
        }
    }
}

/// What a tooltip is drawn and placed with, once the widget, the theme and the
/// defaults have each had their say -- upstream's `Tooltip.build`.
///
/// # Half the defaults depend on the platform
///
/// A tooltip is 24 tall with 8 of horizontal padding on a desktop and 32 with
/// 16 on a phone, and that is not arbitrary. A desktop tooltip is summoned by a
/// mouse resting exactly on something and read from a foot away; a touch one is
/// summoned by a long press, appears under a hand, and is read at arm's length.
/// The bigger one is not more generous, it is the same tooltip at the distance
/// it will actually be read from.
///
/// Upstream reads `Theme.of(context).platform` for this, which is why
/// [`ThemeData::platform`] exists and is overridable: it is the switch, and a
/// developer previewing another platform expects the tooltip to follow.
pub struct ResolvedTooltip {
    /// The minimum height. Upstream's `height`, which is a floor and not a
    /// fixed size -- a long message wraps and grows.
    pub height: f32,
    pub padding: EdgeInsets,
    pub margin: EdgeInsets,
    pub vertical_offset: f32,
    pub prefer_below: bool,
    pub exclude_from_semantics: bool,
    pub decoration: Option<crate::decoration::Decoration>,
    pub text_style: Option<TextStyle>,
    pub text_align: TextAlign,
    pub wait_duration: std::time::Duration,
    pub show_duration: std::time::Duration,
}

impl ResolvedTooltip {
    /// Upstream's `_defaultVerticalOffset`.
    pub const VERTICAL_OFFSET: f32 = 24.0;
    /// Upstream's `_defaultPreferBelow`.
    pub const PREFER_BELOW: bool = true;
    /// Upstream's `_defaultShowDuration`.
    pub const SHOW_DURATION: std::time::Duration = std::time::Duration::from_millis(1500);
    /// Upstream's `_defaultWaitDuration`, which is **zero**: a tooltip summoned
    /// by a long press has already been waited for.
    pub const WAIT_DURATION: std::time::Duration = std::time::Duration::ZERO;

    /// Upstream's `_getDefaultTooltipHeight`.
    pub fn default_height(platform: TargetPlatform) -> f32 {
        if platform.is_mobile() { 32.0 } else { 24.0 }
    }

    /// Upstream's `_getDefaultPadding`. Only the horizontal half changes: the
    /// vertical padding is 4 everywhere, because the height is what gives a
    /// touch tooltip its room and padding on top of that would fight it.
    pub fn default_padding(platform: TargetPlatform) -> EdgeInsets {
        EdgeInsets::symmetric(if platform.is_mobile() { 16.0 } else { 8.0 }, 4.0)
    }

    pub fn of(context: &mut BuildContext) -> ResolvedTooltip {
        let data = TooltipTheme::of(context);
        let platform = ThemeData::of(context).platform;
        let direction = crate::direction::current_direction();
        ResolvedTooltip {
            height: data
                .height
                .unwrap_or_else(|| ResolvedTooltip::default_height(platform)),
            padding: data
                .padding
                .map(|padding| padding.resolve(direction))
                .unwrap_or_else(|| ResolvedTooltip::default_padding(platform)),
            // Upstream's `_defaultMargin` is `EdgeInsets.zero`: a tooltip is
            // placed against its target, and a margin would be a second opinion
            // about where that is.
            margin: data
                .margin
                .map(|margin| margin.resolve(direction))
                .unwrap_or(EdgeInsets::ZERO),
            vertical_offset: data
                .vertical_offset
                .unwrap_or(ResolvedTooltip::VERTICAL_OFFSET),
            prefer_below: data.prefer_below.unwrap_or(ResolvedTooltip::PREFER_BELOW),
            exclude_from_semantics: data.exclude_from_semantics.unwrap_or(false),
            decoration: data.decoration.clone(),
            text_style: data.text_style.clone(),
            text_align: data.text_align.unwrap_or(TextAlign::Start),
            wait_duration: data.wait_duration.unwrap_or(ResolvedTooltip::WAIT_DURATION),
            show_duration: data.show_duration.unwrap_or(ResolvedTooltip::SHOW_DURATION),
        }
    }
}

/// What a progress indicator draws with -- upstream's
/// `_LinearProgressIndicatorState` and `_CircularProgressIndicatorState`
/// reading `ProgressIndicatorTheme.of` and then the M3 defaults.
///
/// # The track and the fill are different colours for a reason
///
/// The fill is the scheme's primary and the track is
/// `secondaryContainer` -- not a dimmed primary. A track dimmed from the fill
/// reads as "this part is done less"; a track in its own colour reads as the
/// space the fill is moving through, which is what it is. Upstream's linear
/// and circular indicators differ here and the difference is deliberate: a
/// circular one's track is transparent by default, because a spinner with a
/// ring behind it looks like a control rather than an activity.
pub struct ResolvedProgressIndicator {
    pub color: Color,
    /// Upstream's `linearTrackColor`.
    pub linear_track_color: Color,
    pub linear_min_height: f32,
    /// Upstream's `circularTrackColor`, which is `None` by default -- a
    /// spinner draws no ring behind itself.
    pub circular_track_color: Option<Color>,
    pub refresh_background_color: Color,
    pub stop_indicator_color: Option<Color>,
    pub stop_indicator_radius: Option<f32>,
}

impl ResolvedProgressIndicator {
    /// Upstream's `LinearProgressIndicator.minHeight` default.
    pub const LINEAR_MIN_HEIGHT: f32 = 4.0;

    pub fn of(context: &mut BuildContext) -> ResolvedProgressIndicator {
        let data = ProgressIndicatorTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        ResolvedProgressIndicator {
            color: data.color.unwrap_or(scheme.primary),
            linear_track_color: data
                .linear_track_color
                .unwrap_or(scheme.secondary_container()),
            linear_min_height: data
                .linear_min_height
                .unwrap_or(ResolvedProgressIndicator::LINEAR_MIN_HEIGHT),
            circular_track_color: data.circular_track_color,
            refresh_background_color: data.refresh_background_color.unwrap_or(scheme.surface),
            stop_indicator_color: data.stop_indicator_color,
            stop_indicator_radius: data.stop_indicator_radius,
        }
    }
}

/// Where a snack bar's `behavior` came from.
///
/// Upstream's assert message names this, and it is the reason the enum exists
/// rather than a bool: told only that *"Width can only be used with floating
/// behavior"*, a developer who never wrote `behavior:` anywhere has nowhere to
/// look. Told that the fixed behaviour came from the inherited theme, they do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnackBarBehaviorSource {
    Widget,
    Theme,
    Default,
}

/// What a snack bar is drawn with -- upstream's `_SnackBarState.build` reading
/// `SnackBarTheme.of` and then `_SnackbarDefaultsM3`.
///
/// # `width` and `margin` only mean anything when it floats
///
/// A fixed snack bar is attached to the bottom edge of the screen and spans it;
/// there is no room in that for a width or a margin, and upstream asserts
/// rather than quietly ignoring them. It also asserts *where the fixed
/// behaviour came from* -- see [`SnackBarBehaviorSource`].
pub struct ResolvedSnackBar {
    pub behavior: SnackBarBehavior,
    pub behavior_source: SnackBarBehaviorSource,
    pub background_color: Color,
    pub elevation: f32,
    pub width: Option<f32>,
    pub inset_padding: EdgeInsets,
    pub show_close_icon: bool,
    pub action_overflow_threshold: f32,
    pub content_text_style: Option<TextStyle>,
    pub action_text_color: Option<Color>,
}

impl ResolvedSnackBar {
    /// Upstream's `_SnackbarDefaultsM3`.
    pub const ELEVATION: f32 = 6.0;
    pub const ACTION_OVERFLOW_THRESHOLD: f32 = 0.25;
    pub const SHOW_CLOSE_ICON: bool = false;

    /// Upstream's horizontal padding, which differs by behaviour: 16 floating,
    /// 24 fixed. A floating bar already has its inset padding holding it off
    /// the edges, so it needs less of its own.
    pub fn horizontal_padding(behavior: SnackBarBehavior) -> f32 {
        match behavior {
            SnackBarBehavior::Floating => 16.0,
            SnackBarBehavior::Fixed => 24.0,
        }
    }

    pub fn of(context: &mut BuildContext, bar: &crate::snack_bar::SnackBar) -> ResolvedSnackBar {
        let data = SnackBarTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        let (behavior, behavior_source) = match (bar.behavior, data.behavior) {
            (Some(behavior), _) => (behavior, SnackBarBehaviorSource::Widget),
            (None, Some(behavior)) => (behavior, SnackBarBehaviorSource::Theme),
            (None, None) => (SnackBarBehavior::Fixed, SnackBarBehaviorSource::Default),
        };
        ResolvedSnackBar {
            behavior,
            behavior_source,
            background_color: data.background_color.unwrap_or(scheme.inverse_surface()),
            elevation: bar
                .elevation
                .or(data.elevation)
                .unwrap_or(ResolvedSnackBar::ELEVATION),
            width: bar.width.or(data.width),
            inset_padding: data
                .inset_padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets {
                    left: 15.0,
                    top: 5.0,
                    right: 15.0,
                    bottom: 10.0,
                }),
            show_close_icon: bar
                .show_close_icon
                .or(data.show_close_icon)
                .unwrap_or(ResolvedSnackBar::SHOW_CLOSE_ICON),
            action_overflow_threshold: bar
                .action_overflow_threshold
                .or(data.action_overflow_threshold)
                .unwrap_or(ResolvedSnackBar::ACTION_OVERFLOW_THRESHOLD),
            content_text_style: data.content_text_style.clone(),
            action_text_color: data.action_text_color,
        }
    }

    /// Upstream's assert inside `build`: `width` and `margin` are floating-only,
    /// and the message says which of the three steps chose the behaviour.
    ///
    /// Returned rather than asserted so it can be checked; the widget asserts on
    /// it.
    pub fn check(&self, margin: Option<f32>) -> Result<(), String> {
        if self.behavior == SnackBarBehavior::Floating {
            return Ok(());
        }
        let blame = match self.behavior_source {
            SnackBarBehaviorSource::Widget => {
                "SnackBarBehavior.fixed was set in the SnackBar constructor."
            }
            SnackBarBehaviorSource::Theme => {
                "SnackBarBehavior.fixed was set by the inherited SnackBarThemeData."
            }
            SnackBarBehaviorSource::Default => "SnackBarBehavior.fixed was set by default.",
        };
        for (value, name) in [
            (margin.is_some(), "Margin"),
            (self.width.is_some(), "Width"),
        ] {
            if value {
                return Err(format!(
                    "{name} can only be used with floating behavior. {blame}"
                ));
            }
        }
        Ok(())
    }

    /// Upstream's `willOverflowAction`: the action moves to its own line when it
    /// would take more than the threshold's share of the bar.
    ///
    /// A fraction and not a width, because the bar's width is the screen's and
    /// the same action is comfortable on a tablet and crowded on a phone.
    pub fn will_overflow_action(&self, action_width: f32, bar_width: f32) -> bool {
        if bar_width <= 0.0 {
            return false;
        }
        action_width / bar_width > self.action_overflow_threshold
    }
}

/// What an icon is drawn with -- upstream's `Icon.build` reading
/// `IconTheme.of` and then its own fallbacks.
///
/// # Two different defaults, and they are not the same number
///
/// `IconThemeData.fallback()` says 24, and that is the size of an icon under a
/// theme -- which in a real app is every icon, because the app installs one at
/// the root. But `Icon.build`'s own last resort is `kDefaultFontSize`, **14**:
/// with no theme anywhere, an icon falls back to the size of *text*, not to the
/// Material icon size. That is not an oversight. An icon with nothing around it
/// to belong to is a glyph in a line of type, and 14 is what a glyph is.
///
/// # An icon does not grow with the text unless it is told to
///
/// `applyTextScaling` is false by default. An icon inside a sentence should
/// follow the reader's text size; an icon that is a button should not, because
/// the button around it is a fixed target and a growing glyph would burst it.
/// Upstream makes the caller say which kind it is rather than guessing.
pub struct ResolvedIcon {
    pub size: f32,
    pub color: Color,
    pub fill: f32,
    pub weight: f32,
    pub grade: f32,
    pub optical_size: f32,
    pub apply_text_scaling: bool,
    pub shadows: Option<Vec<crate::painting::BoxShadow>>,
}

impl ResolvedIcon {
    /// Upstream's `IconThemeData.fallback()`.
    pub const THEME_SIZE: f32 = 24.0;
    pub const THEME_FILL: f32 = 0.0;
    pub const THEME_WEIGHT: f32 = 400.0;
    pub const THEME_GRADE: f32 = 0.0;
    pub const THEME_OPTICAL_SIZE: f32 = 48.0;
    /// Upstream's `kDefaultFontSize`, which is `Icon.build`'s last resort and
    /// **not** the theme's fallback -- see the type's docs.
    pub const DEFAULT_FONT_SIZE: f32 = 14.0;

    pub fn of(context: &mut BuildContext, icon: &crate::crossfade::Icon) -> ResolvedIcon {
        let data = IconTheme::of(context);
        let apply_text_scaling = icon
            .apply_text_scaling
            .or(data.apply_text_scaling)
            .unwrap_or(false);
        let tentative = icon
            .size
            .or(data.size)
            .unwrap_or(ResolvedIcon::DEFAULT_FONT_SIZE);
        ResolvedIcon {
            size: if apply_text_scaling {
                crate::media_query::current_text_scale() * tentative
            } else {
                tentative
            },
            // Upstream's opacity applies to whatever colour came out, its own
            // or the theme's -- which is why it is not a colour of its own.
            color: {
                let base = icon
                    .color
                    .or(data.color)
                    .unwrap_or(Color::argb(0xFF, 0, 0, 0));
                match data.opacity {
                    Some(opacity) => base
                        .with_alpha((base.alpha() as f32 * opacity.clamp(0.0, 1.0)).round() as u8),
                    None => base,
                }
            },
            fill: icon.fill.or(data.fill).unwrap_or(ResolvedIcon::THEME_FILL),
            weight: icon
                .weight
                .or(data.weight)
                .unwrap_or(ResolvedIcon::THEME_WEIGHT),
            grade: icon
                .grade
                .or(data.grade)
                .unwrap_or(ResolvedIcon::THEME_GRADE),
            optical_size: icon
                .optical_size
                .or(data.optical_size)
                .unwrap_or(ResolvedIcon::THEME_OPTICAL_SIZE),
            apply_text_scaling,
            shadows: icon.shadows.clone().or_else(|| data.shadows.clone()),
        }
    }
}

/// What a text field draws its cursor, selection and handles with --
/// upstream's `_TextFieldState.build` reading `DefaultSelectionStyle.of` and
/// then the platform's own colours.
///
/// # A field that failed validation has a red cursor whatever anyone asked for
///
/// Upstream's line is `cursorColor = _hasError ? errorColor : (widget.cursorColor
/// ?? selectionStyle.cursorColor ?? default)` -- the error is *outside* the
/// chain, not the first step of it. A caller who set a cursor colour does not
/// get to keep it while the field is refusing what was typed: the state matters
/// more than the styling, and a field that looks the same wrong as right is
/// worse than an ugly one.
///
/// The selection colour has no such rule. It is the same colour either way,
/// because a selection is the reader's own doing and recolouring it would be
/// blaming them for the error.
///
/// # The selection is the cursor's colour at forty per cent
///
/// Not a colour of its own: `primary.withOpacity(0.40)`. A selection has to be
/// visible *through* -- the text under it must stay readable -- so it is the
/// same hue announced quietly rather than a second colour competing with it.
pub struct ResolvedTextSelection {
    pub cursor: Color,
    pub selection: Color,
    pub handle: Color,
}

impl ResolvedTextSelection {
    /// Upstream's `withOpacity(0.40)` on the selection.
    pub const SELECTION_OPACITY: f32 = 0.40;

    pub fn of(
        context: &mut BuildContext,
        widget_cursor: Option<Color>,
        has_error: bool,
    ) -> ResolvedTextSelection {
        let data = TextSelectionTheme::of(context);
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let default_cursor = scheme.primary;
        ResolvedTextSelection {
            cursor: if has_error {
                scheme.error
            } else {
                widget_cursor
                    .or(data.cursor_color)
                    .unwrap_or(default_cursor)
            },
            selection: data.selection_color.unwrap_or_else(|| {
                default_cursor
                    .with_alpha((255.0 * ResolvedTextSelection::SELECTION_OPACITY).round() as u8)
            }),
            // Upstream's handle colour falls back to the *primary* and not to
            // the selection colour: a handle is a thing to grab and has to be
            // solid, where the selection behind it is deliberately faint.
            handle: data.selection_handle_color.unwrap_or(default_cursor),
        }
    }
}

/// What a bottom sheet is drawn with -- upstream's `_BottomSheetState.build`
/// and `ModalBottomSheetRoute.buildPage` reading `BottomSheetTheme.of`.
///
/// # A modal sheet's chain has four steps and not three
///
/// `widget ?? theme.modalX ?? theme.X ?? defaults.modalX`. The modal-specific
/// theme field comes first, then the *shared* one, then the modal default.
/// That is what lets a theme say "sheets here look like this" once and have it
/// apply to both kinds, while a theme that sets `modalBackgroundColor` is
/// saying "and modal ones differently". A three-step chain could express one or
/// the other and not both.
///
/// A persistent sheet never looks at the modal fields at all: it is part of the
/// page, not something over it.
///
/// # The theme's drag handle is *and*-ed with whether you can drag
///
/// `showDragHandle ?? (enableDrag && (theme.showDragHandle ?? false))`. A theme
/// asking for handles does not put one on a sheet that cannot be dragged --
/// that would be a control promising something it does not do. Only the sheet's
/// own `showDragHandle` can override that, because a caller saying it outright
/// has taken responsibility for it.
pub struct ResolvedBottomSheet {
    pub background: Color,
    pub elevation: f32,
    pub shadow_color: Option<Color>,
    pub modal_barrier_color: Option<Color>,
    pub show_drag_handle: bool,
    pub drag_handle_color: Option<Color>,
    pub shape: Option<ShapeBorder>,
}

impl ResolvedBottomSheet {
    /// Upstream's `_BottomSheetDefaultsM3.elevation`, which is 1: a persistent
    /// sheet is part of the page and barely lifted off it.
    pub const ELEVATION: f32 = 1.0;
    /// And `modalElevation`, also 1 in Material 3 -- the scrim is what
    /// separates a modal sheet from the page, so it does not also need height.
    pub const MODAL_ELEVATION: f32 = 1.0;

    pub fn of(
        context: &mut BuildContext,
        is_modal: bool,
        show_drag_handle: Option<bool>,
        enable_drag: bool,
    ) -> ResolvedBottomSheet {
        let data = BottomSheetTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        ResolvedBottomSheet {
            background: if is_modal {
                data.modal_background_color.or(data.background_color)
            } else {
                data.background_color
            }
            .unwrap_or(scheme.surface_container_low()),
            elevation: if is_modal {
                data.modal_elevation
                    .or(data.elevation)
                    .unwrap_or(ResolvedBottomSheet::MODAL_ELEVATION)
            } else {
                data.elevation.unwrap_or(ResolvedBottomSheet::ELEVATION)
            },
            shadow_color: data.shadow_color,
            modal_barrier_color: is_modal.then_some(data.modal_barrier_color).flatten(),
            show_drag_handle: show_drag_handle
                .unwrap_or(enable_drag && data.show_drag_handle.unwrap_or(false)),
            drag_handle_color: data.drag_handle_color,
            shape: data.shape.clone(),
        }
    }
}

/// What a dialog is drawn and placed with -- upstream's `Dialog.build` reading
/// `DialogTheme.of` and then the M3 defaults.
///
/// # The keyboard's insets are *added* to the dialog's margin, not maxed with it
///
/// `effectivePadding = MediaQuery.viewInsetsOf(context) + (insetPadding ??
/// theme ?? default)`. When the on-screen keyboard comes up, the dialog is
/// pushed above it **and keeps its own margin on top of that**. Taking the
/// larger of the two would leave the dialog resting on the keyboard: correct by
/// the arithmetic, and wrong to look at, because the margin is not there to
/// clear the edge of the screen -- it is there so the dialog does not touch
/// whatever is beneath it.
pub struct ResolvedDialog {
    pub background: Color,
    pub elevation: f32,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub shape: Option<ShapeBorder>,
    pub alignment: crate::render::Alignment,
    pub inset_padding: EdgeInsets,
    pub constraints: BoxConstraints,
    pub barrier_color: Option<Color>,
    pub title_text_style: Option<TextStyle>,
    pub content_text_style: Option<TextStyle>,
    pub actions_padding: EdgeInsets,
    pub icon_color: Option<Color>,
}

impl ResolvedDialog {
    /// Upstream's `_defaultInsetPadding`.
    pub const INSET_PADDING: EdgeInsets = EdgeInsets {
        left: 40.0,
        top: 24.0,
        right: 40.0,
        bottom: 24.0,
    };
    /// Upstream's `BoxConstraints(minWidth: 280.0)` -- a floor and not a size.
    /// A dialog narrower than this reads as a tooltip that got lost.
    pub const MIN_WIDTH: f32 = 280.0;
    /// Upstream's `_DialogDefaultsM3.elevation`.
    pub const ELEVATION: f32 = 6.0;

    pub fn of(context: &mut BuildContext) -> ResolvedDialog {
        let data = DialogTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        ResolvedDialog {
            background: data
                .background_color
                .unwrap_or(scheme.surface_container_high()),
            elevation: data.elevation.unwrap_or(ResolvedDialog::ELEVATION),
            shadow_color: data.shadow_color,
            surface_tint_color: data.surface_tint_color,
            shape: data.shape.clone(),
            alignment: data
                .alignment
                .map(|alignment| alignment.resolve(crate::direction::current_direction()))
                .unwrap_or(crate::render::Alignment::CENTER),
            inset_padding: data.inset_padding.unwrap_or(ResolvedDialog::INSET_PADDING),
            constraints: data.constraints.unwrap_or(BoxConstraints {
                min_width: ResolvedDialog::MIN_WIDTH,
                max_width: f32::INFINITY,
                min_height: 0.0,
                max_height: f32::INFINITY,
            }),
            barrier_color: data.barrier_color,
            title_text_style: data.title_text_style.clone(),
            content_text_style: data.content_text_style.clone(),
            actions_padding: data
                .actions_padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::ZERO),
            icon_color: data.icon_color,
        }
    }

    /// Upstream's `effectivePadding`: what is covering the view, **plus** the
    /// dialog's own margin. See the type's docs for why it is a sum.
    pub fn effective_padding(&self, view_insets: EdgeInsets) -> EdgeInsets {
        EdgeInsets {
            left: view_insets.left + self.inset_padding.left,
            top: view_insets.top + self.inset_padding.top,
            right: view_insets.right + self.inset_padding.right,
            bottom: view_insets.bottom + self.inset_padding.bottom,
        }
    }
}

/// What an expansion tile is drawn with at each end of its animation --
/// upstream's `_ExpansionTileState._updateHeaderColor`, `_updateIconColor` and
/// `_updateBackgroundColor` reading `ExpansionTileTheme.of`.
///
/// # The pairs are tween endpoints, not an either/or
///
/// `collapsedTextColor` and `textColor` are `begin` and `end` of a
/// `ColorTween` the expansion animation drives. The tile does not *switch*
/// colours when it opens, it crosses from one to the other over the same
/// curve the height follows -- which is why they are resolved together and
/// held together rather than being picked by a boolean.
///
/// Reading the state and choosing one would look right at both ends and wrong
/// for the whole of the animation in between, which is the part anyone
/// actually watches.
///
/// # The backgrounds have no defaults and the foregrounds do
///
/// Upstream's `_updateBackgroundColor` has no `defaults.` third step at all:
/// an unstyled expansion tile is transparent at both ends, because it sits in a
/// list and a list has its own background. The text and icon colours do have
/// defaults, because a colour is not optional the way a background is.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedExpansionTile {
    pub collapsed_background: Option<Color>,
    pub expanded_background: Option<Color>,
    pub collapsed_text_color: Color,
    pub expanded_text_color: Color,
    pub collapsed_icon_color: Color,
    pub expanded_icon_color: Color,
    pub collapsed_shape: Option<ShapeBorder>,
    pub expanded_shape: Option<ShapeBorder>,
    pub tile_padding: EdgeInsets,
    /// Only meaningful while open, which is the only time there are children.
    pub expanded_alignment: crate::render::Alignment,
    pub children_padding: EdgeInsets,
}

impl ResolvedExpansionTile {
    pub fn of(context: &mut BuildContext) -> ResolvedExpansionTile {
        let data = ExpansionTileTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        let direction = crate::direction::current_direction();
        ResolvedExpansionTile {
            // No third step: upstream has none either. Transparent at both ends
            // unless somebody said otherwise.
            collapsed_background: data.collapsed_background_color,
            expanded_background: data.background_color,
            collapsed_text_color: data.collapsed_text_color.unwrap_or(scheme.on_surface),
            // Upstream's `_ExpansionTileDefaultsM3.textColor` is the primary:
            // an open tile's title is the thing the reader just chose, and it
            // says so.
            expanded_text_color: data.text_color.unwrap_or(scheme.primary),
            collapsed_icon_color: data
                .collapsed_icon_color
                .unwrap_or(scheme.on_surface_variant()),
            expanded_icon_color: data.icon_color.unwrap_or(scheme.primary),
            collapsed_shape: data.collapsed_shape.clone(),
            expanded_shape: data.shape.clone(),
            tile_padding: data
                .tile_padding
                .map(|padding| padding.resolve(direction))
                .unwrap_or(EdgeInsets::symmetric(16.0, 0.0)),
            expanded_alignment: data
                .expanded_alignment
                .map(|alignment| alignment.resolve(direction))
                .unwrap_or(crate::render::Alignment::CENTER),
            children_padding: data
                .children_padding
                .map(|padding| padding.resolve(direction))
                .unwrap_or(EdgeInsets::ZERO),
        }
    }

    /// The colours partway through the animation. `t` is the expansion, zero
    /// closed and one open.
    ///
    /// Returns `(background, text, icon)`. The background is an `Option`
    /// throughout: a tween between two absent colours is absent, and one
    /// between an absent colour and a real one fades from transparent --
    /// upstream's `ColorTween` treats null as transparent-of-the-other-end
    /// rather than as opaque black, which is what keeps a tile that colours
    /// only when open from flashing.
    pub fn lerp(&self, t: f32) -> (Option<Color>, Color, Color) {
        let t = t.clamp(0.0, 1.0);
        let background = lerp_color(self.collapsed_background, self.expanded_background, t);
        (
            background,
            crate::animation::ColorTween {
                begin: self.collapsed_text_color,
                end: self.expanded_text_color,
            }
            .lerp(t),
            crate::animation::ColorTween {
                begin: self.collapsed_icon_color,
                end: self.expanded_icon_color,
            }
            .lerp(t),
        )
    }
}

/// What one button of a `ToggleButtons` row is drawn with -- upstream's
/// `_getBorderSide` and the `currentColor` chain in `toggle_buttons.dart`.
///
/// # Three states and three fields, defaulting to the same colour
///
/// Selected, enabled-and-unselected, and disabled each have their own theme
/// field for the label and their own for the border. All three border defaults
/// are `onSurface` at twelve per cent -- **the same colour** -- which is not a
/// redundancy: the fields exist so a theme *can* tell the three apart, and by
/// default a row of toggle buttons is one outlined block whose divisions do not
/// move as the selection does. A default that differed would make the row
/// flicker as the reader clicked along it.
///
/// The labels do differ by default, and that is the whole signal: the primary
/// when selected, `onSurface` at 87% when not, and 38% when disabled.
///
/// # `render_border: false` short-circuits before the width
///
/// Upstream returns `BorderSide.none` first thing, so a row with no border
/// never resolves a width at all -- a caller who set `borderWidth` and turned
/// the border off gets no border, not a zero-width one, and the two differ in
/// what they cost.
pub struct ResolvedToggleButton {
    pub label_color: Color,
    pub border: BorderSide,
    pub fill: Option<Color>,
    pub text_style: Option<TextStyle>,
    pub constraints: Option<BoxConstraints>,
    pub border_radius: Option<crate::borders::BorderRadius>,
}

impl ResolvedToggleButton {
    /// Upstream's `_defaultBorderWidth`.
    pub const BORDER_WIDTH: f32 = 1.0;
    /// Upstream's shared border default: `onSurface` at twelve per cent.
    pub const BORDER_ALPHA: u8 = 0x1F;
    /// The unselected label: `onSurface` at eighty-seven per cent.
    pub const LABEL_ALPHA: u8 = 0xDE;
    /// The disabled label: thirty-eight per cent.
    pub const DISABLED_ALPHA: u8 = 0x61;

    pub fn of(
        context: &mut BuildContext,
        enabled: bool,
        selected: bool,
        render_border: bool,
    ) -> ResolvedToggleButton {
        let data = ToggleButtonsTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        let shared_border = scheme
            .on_surface
            .with_alpha(ResolvedToggleButton::BORDER_ALPHA);

        let label_color = if enabled && selected {
            data.selected_color.unwrap_or(scheme.primary)
        } else if enabled {
            data.color.unwrap_or_else(|| {
                scheme
                    .on_surface
                    .with_alpha(ResolvedToggleButton::LABEL_ALPHA)
            })
        } else {
            data.disabled_color.unwrap_or_else(|| {
                scheme
                    .on_surface
                    .with_alpha(ResolvedToggleButton::DISABLED_ALPHA)
            })
        };

        let border = if !render_border {
            // Before the width, as upstream's `_getBorderSide` does.
            BorderSide::NONE
        } else {
            let color = if enabled && selected {
                data.selected_border_color.unwrap_or(shared_border)
            } else if enabled {
                data.border_color.unwrap_or(shared_border)
            } else {
                data.disabled_border_color.unwrap_or(shared_border)
            };
            BorderSide {
                color,
                width: data
                    .border_width
                    .unwrap_or(ResolvedToggleButton::BORDER_WIDTH),
                ..BorderSide::NONE
            }
        };

        ResolvedToggleButton {
            label_color,
            border,
            // Only a selected button is filled, and only then is the theme's
            // fill colour consulted: an unselected one has nothing to fill.
            fill: (enabled && selected).then_some(data.fill_color).flatten(),
            text_style: data.text_style.clone(),
            constraints: data.constraints,
            border_radius: data.border_radius,
        }
    }
}

/// What a navigation rail is drawn with -- upstream's `_NavigationRailState.build`
/// reading `NavigationRailTheme.of` and then `_NavigationRailDefaultsM3`.
///
/// # An extended rail may not also have a label type
///
/// Upstream's constructor asserts
/// `!extended || (labelType == null || labelType == none)`. An extended rail
/// puts every label beside its icon by definition, so asking for "selected
/// only" or "all" on top of that is a contradiction rather than a preference --
/// there is no arrangement that satisfies both. See
/// [`ResolvedNavigationRail::check`].
///
/// # The group alignment is -1, which is the top and not the middle
///
/// It is a fraction from -1 to 1 down the rail's free space, and the default
/// puts the destinations against the top. A rail is a list you read downwards
/// from the first item; centring it would leave the first destination in a
/// different place on every screen height.
pub struct ResolvedNavigationRail {
    pub background_color: Option<Color>,
    pub elevation: f32,
    pub selected_label_style: Option<TextStyle>,
    pub unselected_label_style: Option<TextStyle>,
    pub selected_icon_theme: Option<IconThemeData>,
    pub unselected_icon_theme: Option<IconThemeData>,
    pub group_alignment: f32,
    pub label_type: NavigationRailLabelType,
    pub use_indicator: bool,
    pub indicator_color: Option<Color>,
    pub indicator_shape: Option<ShapeBorder>,
    pub min_width: f32,
    pub min_extended_width: f32,
}

impl ResolvedNavigationRail {
    /// Upstream's `_NavigationRailDefaultsM3`.
    pub const GROUP_ALIGNMENT: f32 = -1.0;
    pub const MIN_WIDTH: f32 = 80.0;
    pub const MIN_EXTENDED_WIDTH: f32 = 256.0;
    pub const ELEVATION: f32 = 0.0;

    pub fn of(context: &mut BuildContext) -> ResolvedNavigationRail {
        let data = NavigationRailTheme::of(context);
        ResolvedNavigationRail {
            background_color: data.background_color,
            elevation: data.elevation.unwrap_or(ResolvedNavigationRail::ELEVATION),
            selected_label_style: data.selected_label_text_style.clone(),
            unselected_label_style: data.unselected_label_text_style.clone(),
            selected_icon_theme: data.selected_icon_theme.clone(),
            unselected_icon_theme: data.unselected_icon_theme.clone(),
            group_alignment: data
                .group_alignment
                .unwrap_or(ResolvedNavigationRail::GROUP_ALIGNMENT),
            // Upstream's M3 default is `none`: an indicator already says which
            // destination is current, so the labels are for when there is no
            // indicator to read.
            label_type: data.label_type.unwrap_or(NavigationRailLabelType::None),
            use_indicator: data.use_indicator.unwrap_or(true),
            indicator_color: data.indicator_color,
            indicator_shape: data.indicator_shape.clone(),
            min_width: data.min_width.unwrap_or(ResolvedNavigationRail::MIN_WIDTH),
            min_extended_width: data
                .min_extended_width
                .unwrap_or(ResolvedNavigationRail::MIN_EXTENDED_WIDTH),
        }
    }

    /// Upstream's constructor assert, against the resolved label type.
    ///
    /// Returned rather than asserted so it can be checked. It has to be run
    /// against the *resolved* type and not the widget's own: a rail that was
    /// extended and left the label type alone is still wrong when the theme
    /// asks for labels, and that is the case a caller cannot see for
    /// themselves.
    pub fn check(&self, extended: bool) -> Result<(), &'static str> {
        if extended && self.label_type != NavigationRailLabelType::None {
            return Err(
                "an extended NavigationRail already shows every label, so a \
                 labelType other than none has no arrangement that satisfies both",
            );
        }
        Ok(())
    }

    /// The rail's width in either form. Upstream picks between the two by
    /// whether it is extended, and they are separate numbers rather than one
    /// scaled: 80 is an icon with room around it, 256 is a column of text.
    pub fn width(&self, extended: bool) -> f32 {
        if extended {
            self.min_extended_width
        } else {
            self.min_width
        }
    }
}

/// What a data table is drawn with -- upstream's `DataTable.build` reading
/// `DataTableTheme.of` and then its own constants.
///
/// # Both row heights default to the same number, so a default row is fixed
///
/// `dataRowMinHeight` and `dataRowMaxHeight` both fall back to
/// `kMinInteractiveDimension`, which is 48. A table nobody configured therefore
/// has rows of exactly 48 -- the two fields exist to make a row *flexible*, and
/// until one of them is moved there is no flexibility to have.
///
/// The consequence is worth stating because it is easy to get wrong from the
/// outside: raising only the minimum leaves it above the maximum. Upstream
/// asserts `dataRowMinHeight <= dataRowMaxHeight`, and the assert is on what
/// the caller wrote rather than on what resolved, so a caller who raises the
/// minimum and lets the maximum default has written a contradiction --
/// [`ResolvedDataTable::check`] reports it.
///
/// # A heading row is taller than a data row and is not a data row
///
/// 56 against 48. The heading is read once and the rows are read many times;
/// the extra eight points are what stop the header reading as the first entry.
pub struct ResolvedDataTable {
    pub decoration: Option<crate::decoration::Decoration>,
    pub data_row_min_height: f32,
    pub data_row_max_height: f32,
    pub heading_row_height: f32,
    pub horizontal_margin: f32,
    pub column_spacing: f32,
    pub divider_thickness: f32,
    pub checkbox_horizontal_margin: Option<f32>,
    pub data_text_style: Option<TextStyle>,
    pub heading_text_style: Option<TextStyle>,
    pub heading_row_alignment: crate::render::MainAxisAlignment,
}

impl ResolvedDataTable {
    /// Upstream's `kMinInteractiveDimension`, which is both row-height default.
    pub const ROW_HEIGHT: f32 = 48.0;
    /// Upstream's `_headingRowHeight`.
    pub const HEADING_ROW_HEIGHT: f32 = 56.0;
    /// Upstream's `_horizontalMargin`.
    pub const HORIZONTAL_MARGIN: f32 = 24.0;
    /// Upstream's `_columnSpacing`.
    pub const COLUMN_SPACING: f32 = 56.0;
    /// Upstream's `_dividerThickness`.
    pub const DIVIDER_THICKNESS: f32 = 1.0;

    pub fn of(context: &mut BuildContext) -> ResolvedDataTable {
        let data = DataTableTheme::of(context);
        ResolvedDataTable {
            decoration: data.decoration.clone(),
            data_row_min_height: data
                .data_row_min_height
                .unwrap_or(ResolvedDataTable::ROW_HEIGHT),
            data_row_max_height: data
                .data_row_max_height
                .unwrap_or(ResolvedDataTable::ROW_HEIGHT),
            heading_row_height: data
                .heading_row_height
                .unwrap_or(ResolvedDataTable::HEADING_ROW_HEIGHT),
            horizontal_margin: data
                .horizontal_margin
                .unwrap_or(ResolvedDataTable::HORIZONTAL_MARGIN),
            column_spacing: data
                .column_spacing
                .unwrap_or(ResolvedDataTable::COLUMN_SPACING),
            divider_thickness: data
                .divider_thickness
                .unwrap_or(ResolvedDataTable::DIVIDER_THICKNESS),
            // No default: upstream leaves it null and the checkbox falls back
            // to the horizontal margin, which is a different rule from having
            // a margin of its own.
            checkbox_horizontal_margin: data.checkbox_horizontal_margin,
            data_text_style: data.data_text_style.clone(),
            heading_text_style: data.heading_text_style.clone(),
            heading_row_alignment: data
                .heading_row_alignment
                .unwrap_or(crate::render::MainAxisAlignment::Start),
        }
    }

    /// Upstream's `assert(dataRowMinHeight <= dataRowMaxHeight)`.
    ///
    /// Run against the resolution rather than the caller's own two values,
    /// because the case that bites is raising the minimum and letting the
    /// maximum default -- both fall back to the same 48, so a caller who moved
    /// one of them has written a contradiction without setting two fields.
    pub fn check(&self) -> Result<(), &'static str> {
        if self.data_row_min_height > self.data_row_max_height {
            return Err(
                "dataRowMinHeight is above dataRowMaxHeight -- both default to \
                 the same height, so raising one alone leaves them crossed",
            );
        }
        Ok(())
    }

    /// Where a checkbox column's margin comes from. Upstream falls back to the
    /// table's horizontal margin rather than to a constant: the checkbox is in
    /// the same gutter as everything else unless it is given one of its own.
    pub fn checkbox_margin(&self) -> f32 {
        self.checkbox_horizontal_margin
            .unwrap_or(self.horizontal_margin)
    }
}

/// What a bottom navigation bar is drawn with -- upstream's
/// `_BottomNavigationBarState.build` reading `BottomNavigationBarTheme.of`.
///
/// # One default is a constant and the other is computed from a resolved value
///
/// `showSelectedLabels` falls back to `true` flat. `showUnselectedLabels` falls
/// back to `_defaultShowUnselected`, which is **false when shifting and true
/// when fixed** -- so its default depends on the *resolved* type, which in turn
/// depends on the item count.
///
/// The asymmetry is the design: the selected label is what tells the reader
/// where they are and is never hidden, while the unselected ones are hidden
/// exactly when there is no room for them, which is what shifting means. Giving
/// both the same default would either crowd a four-item bar or leave a
/// three-item one unlabelled.
///
/// # Selected and unselected item colours are not a pair with one default
///
/// Upstream leaves both null here and lets the widget fall back to the theme's
/// primary and to `textTheme.caption.color`; nothing is invented in the
/// resolution, because a colour made up here is one the widget could not tell
/// from an answer.
pub struct ResolvedBottomNavigationBar {
    pub bar_type: BottomNavigationBarType,
    pub background_color: Option<Color>,
    pub elevation: f32,
    pub selected_item_color: Option<Color>,
    pub unselected_item_color: Option<Color>,
    pub selected_label_style: Option<TextStyle>,
    pub unselected_label_style: Option<TextStyle>,
    pub selected_icon_theme: Option<IconThemeData>,
    pub unselected_icon_theme: Option<IconThemeData>,
    pub show_selected_labels: bool,
    pub show_unselected_labels: bool,
    pub enable_feedback: bool,
    pub landscape_layout: BottomNavigationBarLandscapeLayout,
}

impl ResolvedBottomNavigationBar {
    /// Upstream's default elevation.
    pub const ELEVATION: f32 = 8.0;

    /// Upstream's `_defaultShowUnselected`.
    pub fn default_show_unselected(bar_type: BottomNavigationBarType) -> bool {
        match bar_type {
            BottomNavigationBarType::Shifting => false,
            BottomNavigationBarType::Fixed => true,
        }
    }

    pub fn of(
        context: &mut BuildContext,
        bar: &crate::bottom_bars::BottomNavigationBar,
    ) -> ResolvedBottomNavigationBar {
        let data = BottomNavigationBarTheme::of(context);
        // The type first, because one of the label defaults is computed from
        // it -- and from the *resolved* one, so a theme that asks for shifting
        // changes what the unselected labels do without touching them.
        let bar_type = bar.effective_type(data.bar_type);
        ResolvedBottomNavigationBar {
            bar_type,
            background_color: data.background_color,
            elevation: data
                .elevation
                .unwrap_or(ResolvedBottomNavigationBar::ELEVATION),
            selected_item_color: data.selected_item_color,
            unselected_item_color: data.unselected_item_color,
            selected_label_style: data.selected_label_style.clone(),
            unselected_label_style: data.unselected_label_style.clone(),
            selected_icon_theme: data.selected_icon_theme.clone(),
            unselected_icon_theme: data.unselected_icon_theme.clone(),
            show_selected_labels: bar
                .show_selected_labels
                .or(data.show_selected_labels)
                .unwrap_or(true),
            show_unselected_labels: bar
                .show_unselected_labels
                .or(data.show_unselected_labels)
                .unwrap_or_else(|| ResolvedBottomNavigationBar::default_show_unselected(bar_type)),
            enable_feedback: data.enable_feedback.unwrap_or(true),
            landscape_layout: data
                .landscape_layout
                .unwrap_or(BottomNavigationBarLandscapeLayout::Spread),
        }
    }
}

/// What a Material 3 navigation bar is drawn with -- upstream's
/// `_NavigationBarState.build` reading `NavigationBarTheme.of`.
///
/// # The animation duration is the one field the theme has no say in
///
/// Every other field is `widget ?? theme ?? default`. The duration is
/// `animationDuration ?? const Duration(milliseconds: 500)` -- **two steps, and
/// the theme is skipped**, because `NavigationBarThemeData` has no such field
/// to consult. It is not an oversight in the chain; there is nothing to chain
/// to.
///
/// This port's `NavigationBar` used to say "`None` uses the theme's, and the
/// theme's default is 500ms", which described a step that does not exist on
/// either side.
///
/// # The height is the indicator's height and not a bar height
///
/// `_kIndicatorHeight` is 32, and it is the same constant the destination
/// indicator uses. A bar whose height was chosen independently would leave the
/// indicator floating in it or clipped by it.
pub struct ResolvedNavigationBar {
    pub height: f32,
    pub background_color: Option<Color>,
    pub elevation: f32,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub indicator_color: Option<Color>,
    pub indicator_shape: Option<ShapeBorder>,
    pub label_behavior: NavigationDestinationLabelBehavior,
    pub label_padding: EdgeInsets,
    /// Milliseconds. Never from the theme -- see the type's docs.
    pub animation_duration_ms: u32,
}

impl ResolvedNavigationBar {
    /// Upstream's `_kIndicatorHeight`, which is also the bar's default height.
    pub const HEIGHT: f32 = 32.0;
    /// Upstream's default elevation for the M3 bar.
    pub const ELEVATION: f32 = 3.0;
    /// Upstream's `const Duration(milliseconds: 500)`, reached without
    /// consulting the theme.
    pub const ANIMATION_MS: u32 = 500;

    pub fn of(
        context: &mut BuildContext,
        bar: &crate::bottom_bars::NavigationBar,
    ) -> ResolvedNavigationBar {
        let data = NavigationBarTheme::of(context);
        ResolvedNavigationBar {
            height: data.height.unwrap_or(ResolvedNavigationBar::HEIGHT),
            background_color: data.background_color,
            elevation: data.elevation.unwrap_or(ResolvedNavigationBar::ELEVATION),
            shadow_color: data.shadow_color,
            surface_tint_color: data.surface_tint_color,
            indicator_color: data.indicator_color,
            indicator_shape: data.indicator_shape.clone(),
            // Upstream's M3 default: every destination keeps its label. The
            // M3 bar does not shift, so there is no count at which the labels
            // stop fitting -- which is why this default is a constant where
            // `BottomNavigationBar`'s was computed.
            label_behavior: data
                .label_behavior
                .unwrap_or(NavigationDestinationLabelBehavior::AlwaysShow),
            label_padding: data
                .label_padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::ZERO),
            animation_duration_ms: bar
                .animation_duration_ms
                .unwrap_or(ResolvedNavigationBar::ANIMATION_MS),
        }
    }
}

/// What a navigation drawer is drawn with -- upstream's `NavigationDrawer.build`
/// and `_NavigationDrawerDestinationInfo`'s readers of `NavigationDrawerTheme.of`.
///
/// # The drawer's own surface has a two-step chain that ends in *another*
/// widget's theme
///
/// `NavigationDrawer.build` writes `backgroundColor ?? theme.backgroundColor`
/// and hands the result -- **null included** -- to a plain [`crate::drawer`]
/// `Drawer`. Same for the shadow, the surface tint and the elevation. The third
/// step is not skipped; it happens somewhere else, in `DrawerThemeData` and
/// `_DrawerDefaultsM3`.
///
/// So a `DrawerTheme` wrapped around a `NavigationDrawer` moves its background,
/// and a `NavigationDrawerTheme` wrapped around a plain `Drawer` does not. That
/// asymmetry is the whole content of the finding, and it is invisible from the
/// values: `_NavigationDrawerDefaultsM3` **also declares** those four fields,
/// with exactly the numbers `_DrawerDefaultsM3` declares -- elevation 1,
/// `surfaceContainerLow`, transparent, transparent. Nothing in
/// `navigation_drawer.dart` ever reads them (`defaults.` appears eleven times
/// there and not once for these four). They are dead copies emitted by
/// `gen_defaults`, and they agree with the live ones, which is exactly why
/// resolving from the wrong one would never show up.
///
/// This type therefore leaves them `Option` and does not invent the third step
/// -- see [`ResolvedNavigationDrawer::surface`], which performs it by asking
/// the drawer's own theme, the way upstream does.
///
/// # The destination fields do have three steps, and start at the *drawer*
///
/// The indicator's colour and shape come from `info.indicatorColor ??
/// theme.indicatorColor ?? defaults.indicatorColor`, and `info` carries what
/// the **`NavigationDrawer`** was given -- a `NavigationDrawerDestination` has
/// no indicator field of its own to offer. The destination decides its icon and
/// its label; where it sits in the highlight is the drawer's business.
///
/// # A disabled destination is not "disabled and selected"
///
/// Upstream resolves against `enabled ? selectedState : disabledState` where
/// `disabledState` is `{disabled}` **alone**. The selection is dropped, not
/// added to. The consequence is that a disabled destination's selected and
/// unselected icons resolve to the same thing, so the crossfade between them
/// has nothing left to show -- which is the point: a destination you cannot
/// reach should not advertise that it is the one you are on.
pub struct ResolvedNavigationDrawer {
    /// Two steps only -- see the type's docs. `None` here means "ask the
    /// `Drawer`", not "no background".
    pub background_color: Option<Color>,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub elevation: Option<f32>,
    pub tile_height: f32,
    pub indicator_color: Color,
    pub indicator_shape: ShapeBorder,
    pub indicator_size: Size,
    /// The theme's, if it has one. `None` falls to the M3 default computed in
    /// [`ResolvedNavigationDrawer::label_style`].
    pub label_text_style: Option<StateProperty<Option<TextStyle>>>,
    pub icon_theme: Option<StateProperty<Option<IconThemeData>>>,
    /// The drawer widget's, with no theme step at all: `NavigationDrawerThemeData`
    /// has no `tilePadding` field, so `EdgeInsets.symmetric(horizontal: 12)` is
    /// the widget's own default and the only source.
    pub tile_padding: EdgeInsets,
    label_large: Option<TextStyle>,
    on_surface_variant: Color,
    on_secondary_container: Color,
}

impl ResolvedNavigationDrawer {
    /// Upstream `_NavigationDrawerDefaultsM3.tileHeight`.
    pub const TILE_HEIGHT: f32 = 56.0;
    /// Upstream `_NavigationDrawerDefaultsM3.indicatorSize`. The width is a
    /// flat 336 rather than the drawer's width less its padding: the indicator
    /// is a fixed pill the destinations sit inside, so it is the same length
    /// whatever the drawer is.
    pub const INDICATOR_SIZE: Size = Size::new(336.0, 56.0);
    /// Upstream's `_colors.onSurfaceVariant.withOpacity(0.38)` for a disabled
    /// destination, as an alpha.
    pub const DISABLED_OPACITY: f32 = 0.38;

    /// Upstream's three state sets: `{selected}`, `{}`, `{disabled}`.
    ///
    /// Disabled wins outright and takes the selection with it -- see the type's
    /// docs.
    pub fn states(enabled: bool, selected: bool) -> WidgetStates {
        if !enabled {
            return WidgetStates::NONE.with(WidgetState::Disabled);
        }
        if selected {
            WidgetStates::NONE.with(WidgetState::Selected)
        } else {
            WidgetStates::NONE
        }
    }

    pub fn of(
        context: &mut BuildContext,
        drawer: &crate::navigation_destinations::NavigationDrawer,
    ) -> ResolvedNavigationDrawer {
        let data = NavigationDrawerTheme::of(context);
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        ResolvedNavigationDrawer {
            // Two steps. The third is `Drawer`'s -- do not add one here.
            background_color: drawer.background_color.or(data.background_color),
            shadow_color: data.shadow_color,
            surface_tint_color: data.surface_tint_color,
            elevation: data.elevation,
            tile_height: data
                .tile_height
                .unwrap_or(ResolvedNavigationDrawer::TILE_HEIGHT),
            indicator_color: drawer
                .indicator_color
                .or(data.indicator_color)
                .unwrap_or(scheme.secondary_container()),
            indicator_shape: data.indicator_shape.clone().unwrap_or(ShapeBorder::Stadium(
                crate::borders::StadiumBorder::default(),
            )),
            indicator_size: data
                .indicator_size
                .unwrap_or(ResolvedNavigationDrawer::INDICATOR_SIZE),
            label_text_style: data.label_text_style.clone(),
            icon_theme: data.icon_theme.clone(),
            tile_padding: drawer.tile_padding,
            label_large: theme.text_theme.label_large.clone(),
            on_surface_variant: scheme.on_surface_variant(),
            on_secondary_container: scheme.on_secondary_container(),
        }
    }

    /// Upstream's per-state colour for a destination's label and icon:
    /// disabled is the variant at 38 per cent, selected is the colour that
    /// reads against the indicator, and everything else is the variant.
    ///
    /// The selected colour is `onSecondaryContainer` and not a brighter version
    /// of the unselected one, because the selected destination is the one
    /// sitting on the indicator -- it is a different background, not more
    /// emphasis on the same one.
    pub fn foreground(&self, states: WidgetStates) -> Color {
        if states.contains(WidgetState::Disabled) {
            return crate::elevation_overlay::with_opacity(
                self.on_surface_variant,
                ResolvedNavigationDrawer::DISABLED_OPACITY,
            );
        }
        if states.contains(WidgetState::Selected) {
            self.on_secondary_container
        } else {
            self.on_surface_variant
        }
    }

    /// The theme's label style for these states, or the M3 default:
    /// `labelLarge` recoloured by [`ResolvedNavigationDrawer::foreground`].
    pub fn label_style(&self, states: WidgetStates) -> Option<TextStyle> {
        if let Some(property) = &self.label_text_style {
            return property.resolve(states);
        }
        self.label_large.clone().map(|style| TextStyle {
            color: self.foreground(states),
            ..style
        })
    }

    /// The theme's icon theme for these states, or the M3 default: size 24 in
    /// the same foreground.
    pub fn icon_theme(&self, states: WidgetStates) -> IconThemeData {
        if let Some(property) = &self.icon_theme {
            if let Some(data) = property.resolve(states) {
                return data;
            }
        }
        IconThemeData {
            size: Some(24.0),
            color: Some(self.foreground(states)),
            ..IconThemeData::default()
        }
    }

    /// The third step for the surface fields: hand what this resolved -- nulls
    /// and all -- to the drawer's own theme, which is where upstream's null
    /// goes.
    pub fn surface(&self, context: &mut BuildContext) -> ResolvedDrawer {
        let mut drawer = ResolvedDrawer::of(context);
        if let Some(color) = self.background_color {
            drawer.background = color;
        }
        drawer
    }
}

// -- App bar (upstream `app_bar_theme.dart`) ----------------------------------

/// Upstream `AppBarThemeData`.
///
/// `systemOverlayStyle` is not here: it is a `SystemUiOverlayStyle`, the
/// services-side status-bar description this port has not reached.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppBarThemeData {
    pub background_color: Option<Color>,
    /// What the title and the icons are drawn in.
    pub foreground_color: Option<Color>,
    pub elevation: Option<f32>,
    /// The elevation once something has been scrolled under it -- Material 3
    /// raises the bar rather than shadowing it.
    pub scrolled_under_elevation: Option<f32>,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub shape: Option<ShapeBorder>,
    /// How the leading icon is drawn.
    pub icon_theme: Option<IconThemeData>,
    /// How the trailing action icons are drawn, which upstream keeps apart
    /// from the leading one.
    pub actions_icon_theme: Option<IconThemeData>,
    pub center_title: Option<bool>,
    pub title_spacing: Option<f32>,
    pub leading_width: Option<f32>,
    pub toolbar_height: Option<f32>,
    pub toolbar_text_style: Option<TextStyle>,
    pub title_text_style: Option<TextStyle>,
    pub actions_padding: Option<EdgeInsetsGeometry>,
}

impl AppBarThemeData {
    pub fn new() -> AppBarThemeData {
        AppBarThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_foreground_color(mut self, color: Color) -> Self {
        self.foreground_color = Some(color);
        self
    }

    pub fn with_toolbar_height(mut self, height: f32) -> Self {
        self.toolbar_height = Some(height);
        self
    }

    pub fn with_center_title(mut self, center: bool) -> Self {
        self.center_title = Some(center);
        self
    }

    pub fn with_elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }

    /// Upstream `AppBarThemeData.lerp`.
    pub fn lerp(a: &AppBarThemeData, b: &AppBarThemeData, t: f32) -> AppBarThemeData {
        AppBarThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            foreground_color: lerp_color(a.foreground_color, b.foreground_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            scrolled_under_elevation: lerp_f32(
                a.scrolled_under_elevation,
                b.scrolled_under_elevation,
                t,
            ),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            icon_theme: lerp_icon_theme(&a.icon_theme, &b.icon_theme, t),
            actions_icon_theme: lerp_icon_theme(&a.actions_icon_theme, &b.actions_icon_theme, t),
            center_title: lerp_nearer(&a.center_title, &b.center_title, t),
            title_spacing: lerp_f32(a.title_spacing, b.title_spacing, t),
            leading_width: lerp_f32(a.leading_width, b.leading_width, t),
            toolbar_height: lerp_f32(a.toolbar_height, b.toolbar_height, t),
            toolbar_text_style: lerp_nearer(&a.toolbar_text_style, &b.toolbar_text_style, t),
            title_text_style: lerp_nearer(&a.title_text_style, &b.title_text_style, t),
            actions_padding: lerp_nearer(&a.actions_padding, &b.actions_padding, t),
        }
    }
}

/// Upstream `AppBarTheme`.
pub struct AppBarTheme;

impl AppBarTheme {
    pub fn new(data: AppBarThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> AppBarThemeData {
        context
            .inherited::<AppBarThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).app_bar_theme)
    }
}

/// What an app bar draws with, once the three steps have run.
///
/// Upstream's `AppBar.build`: `backgroundColor` is the theme's, then the
/// scheme's surface; `foregroundColor` is the theme's, then `onSurface`; the
/// height is `kToolbarHeight` where nobody said.
pub struct ResolvedAppBar {
    pub background: Color,
    pub foreground: Color,
    pub toolbar_height: f32,
    pub center_title: bool,
    pub title_spacing: f32,
}

impl ResolvedAppBar {
    /// Upstream's `kToolbarHeight`.
    pub const TOOLBAR_HEIGHT: f32 = 56.0;
    /// Upstream's `NavigationToolbar.kMiddleSpacing`.
    pub const TITLE_SPACING: f32 = 16.0;

    pub fn of(context: &mut BuildContext) -> ResolvedAppBar {
        let data = AppBarTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        ResolvedAppBar {
            background: data.background_color.unwrap_or(scheme.surface),
            foreground: data.foreground_color.unwrap_or(scheme.on_surface),
            toolbar_height: data
                .toolbar_height
                .unwrap_or(ResolvedAppBar::TOOLBAR_HEIGHT),
            center_title: data.center_title.unwrap_or(false),
            title_spacing: data.title_spacing.unwrap_or(ResolvedAppBar::TITLE_SPACING),
        }
    }
}

// -- Bottom sheet (upstream `bottom_sheet_theme.dart`) ------------------------

/// Upstream `BottomSheetThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BottomSheetThemeData {
    pub background_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub elevation: Option<f32>,
    /// The sheet's own colour when it is modal, which may differ.
    pub modal_background_color: Option<Color>,
    /// What the rest of the screen is dimmed with behind a modal sheet.
    pub modal_barrier_color: Option<Color>,
    pub shadow_color: Option<Color>,
    pub modal_elevation: Option<f32>,
    pub shape: Option<ShapeBorder>,
    pub show_drag_handle: Option<bool>,
    pub drag_handle_color: Option<Color>,
    pub drag_handle_size: Option<Size>,
    pub constraints: Option<BoxConstraints>,
}

impl BottomSheetThemeData {
    pub fn new() -> BottomSheetThemeData {
        BottomSheetThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_show_drag_handle(mut self, show: bool) -> Self {
        self.show_drag_handle = Some(show);
        self
    }

    /// Upstream `BottomSheetThemeData.lerp`.
    pub fn lerp(
        a: &BottomSheetThemeData,
        b: &BottomSheetThemeData,
        t: f32,
    ) -> BottomSheetThemeData {
        BottomSheetThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            modal_background_color: lerp_color(
                a.modal_background_color,
                b.modal_background_color,
                t,
            ),
            modal_barrier_color: lerp_color(a.modal_barrier_color, b.modal_barrier_color, t),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            modal_elevation: lerp_f32(a.modal_elevation, b.modal_elevation, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            show_drag_handle: lerp_nearer(&a.show_drag_handle, &b.show_drag_handle, t),
            drag_handle_color: lerp_color(a.drag_handle_color, b.drag_handle_color, t),
            drag_handle_size: match (a.drag_handle_size, b.drag_handle_size) {
                (Some(first), Some(second)) => Some(Size::new(
                    first.width + (second.width - first.width) * t,
                    first.height + (second.height - first.height) * t,
                )),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
            constraints: lerp_nearer(&a.constraints, &b.constraints, t),
        }
    }
}

/// Upstream `BottomSheetTheme`.
pub struct BottomSheetTheme;

impl BottomSheetTheme {
    pub fn new(data: BottomSheetThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> BottomSheetThemeData {
        context
            .inherited::<BottomSheetThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).bottom_sheet_theme)
    }
}

// -- Snack bar (upstream `snack_bar_theme.dart`) ------------------------------

/// Upstream `SnackBarBehavior`: whether the bar is part of the scaffold's
/// layout or floats over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnackBarBehavior {
    /// The scaffold makes room for it, and a floating action button moves up.
    Fixed,
    /// It floats above the content, inset from the edges.
    Floating,
}

/// Upstream `SnackBarThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnackBarThemeData {
    pub background_color: Option<Color>,
    pub action_text_color: Option<Color>,
    pub disabled_action_text_color: Option<Color>,
    pub content_text_style: Option<TextStyle>,
    pub elevation: Option<f32>,
    pub shape: Option<ShapeBorder>,
    pub behavior: Option<SnackBarBehavior>,
    /// A floating bar's width, when it should not stretch.
    pub width: Option<f32>,
    pub inset_padding: Option<EdgeInsetsGeometry>,
    pub show_close_icon: Option<bool>,
    pub close_icon_color: Option<Color>,
    /// How much of the bar's width the action may take before the two go on
    /// separate lines.
    pub action_overflow_threshold: Option<f32>,
    pub action_background_color: Option<Color>,
    pub disabled_action_background_color: Option<Color>,
}

impl SnackBarThemeData {
    pub fn new() -> SnackBarThemeData {
        SnackBarThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_behavior(mut self, behavior: SnackBarBehavior) -> Self {
        self.behavior = Some(behavior);
        self
    }

    /// Upstream `SnackBarThemeData.lerp`.
    pub fn lerp(a: &SnackBarThemeData, b: &SnackBarThemeData, t: f32) -> SnackBarThemeData {
        SnackBarThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            action_text_color: lerp_color(a.action_text_color, b.action_text_color, t),
            disabled_action_text_color: lerp_color(
                a.disabled_action_text_color,
                b.disabled_action_text_color,
                t,
            ),
            content_text_style: lerp_nearer(&a.content_text_style, &b.content_text_style, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            behavior: lerp_nearer(&a.behavior, &b.behavior, t),
            width: lerp_f32(a.width, b.width, t),
            inset_padding: lerp_nearer(&a.inset_padding, &b.inset_padding, t),
            show_close_icon: lerp_nearer(&a.show_close_icon, &b.show_close_icon, t),
            close_icon_color: lerp_color(a.close_icon_color, b.close_icon_color, t),
            action_overflow_threshold: lerp_f32(
                a.action_overflow_threshold,
                b.action_overflow_threshold,
                t,
            ),
            action_background_color: lerp_color(
                a.action_background_color,
                b.action_background_color,
                t,
            ),
            disabled_action_background_color: lerp_color(
                a.disabled_action_background_color,
                b.disabled_action_background_color,
                t,
            ),
        }
    }
}

/// Upstream `SnackBarTheme`.
pub struct SnackBarTheme;

impl SnackBarTheme {
    pub fn new(data: SnackBarThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> SnackBarThemeData {
        context
            .inherited::<SnackBarThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).snack_bar_theme)
    }
}

// -- List tile (upstream `list_tile_theme.dart`, and the enums it needs) ------

/// Upstream `ListTileStyle`: which of the two shapes a tile is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ListTileStyle {
    /// A tile in a list.
    #[default]
    List,
    /// A tile in a navigation drawer, which is denser and reads as a
    /// destination rather than a row.
    Drawer,
}

/// Upstream `ListTileControlAffinity`, declared with the thing it describes in
/// [`crate::list_tiles`] and re-exported here.
///
/// It was declared twice -- same name, same variants, same upstream
/// original -- and the two copies could not disagree loudly, because
/// nothing made them meet. A type two modules have to agree on belongs
/// to neither of them.
pub use crate::list_tiles::ListTileControlAffinity;

/// Upstream `ListTileTitleAlignment`: where the leading and trailing widgets
/// sit against the title.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ListTileTitleAlignment {
    /// Top for a three-line tile, centred otherwise -- upstream's default.
    #[default]
    ThreeLine,
    /// Centred on the title's own first line.
    TitleHeight,
    Top,
    Center,
    Bottom,
}

/// Upstream `ListTileThemeData`.
///
/// `mouseCursor` is a `WidgetStateProperty<MouseCursor?>` here as upstream;
/// `visualDensity` is [`VisualDensity`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListTileThemeData {
    /// Whether the tile is packed tighter -- upstream's `dense`.
    pub dense: Option<bool>,
    pub shape: Option<ShapeBorder>,
    pub style: Option<ListTileStyle>,
    /// What a selected tile's text and icons are drawn in.
    pub selected_color: Option<Color>,
    pub icon_color: Option<Color>,
    pub text_color: Option<Color>,
    pub title_text_style: Option<TextStyle>,
    pub subtitle_text_style: Option<TextStyle>,
    pub leading_and_trailing_text_style: Option<TextStyle>,
    pub content_padding: Option<EdgeInsetsGeometry>,
    pub tile_color: Option<Color>,
    pub selected_tile_color: Option<Color>,
    /// The gap between the leading widget and the title.
    pub horizontal_title_gap: Option<f32>,
    pub min_vertical_padding: Option<f32>,
    pub min_leading_width: Option<f32>,
    pub min_tile_height: Option<f32>,
    pub enable_feedback: Option<bool>,
    pub mouse_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
    pub visual_density: Option<VisualDensity>,
    pub title_alignment: Option<ListTileTitleAlignment>,
    pub control_affinity: Option<ListTileControlAffinity>,
    pub is_three_line: Option<bool>,
}

impl ListTileThemeData {
    pub fn new() -> ListTileThemeData {
        ListTileThemeData::default()
    }

    pub fn with_dense(mut self, dense: bool) -> Self {
        self.dense = Some(dense);
        self
    }

    pub fn with_content_padding(mut self, padding: EdgeInsetsGeometry) -> Self {
        self.content_padding = Some(padding);
        self
    }

    pub fn with_tile_color(mut self, color: Color) -> Self {
        self.tile_color = Some(color);
        self
    }

    pub fn with_selected_tile_color(mut self, color: Color) -> Self {
        self.selected_tile_color = Some(color);
        self
    }

    pub fn with_text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }

    pub fn with_min_tile_height(mut self, height: f32) -> Self {
        self.min_tile_height = Some(height);
        self
    }

    pub fn with_horizontal_title_gap(mut self, gap: f32) -> Self {
        self.horizontal_title_gap = Some(gap);
        self
    }

    /// Upstream `ListTileThemeData.lerp`.
    pub fn lerp(a: &ListTileThemeData, b: &ListTileThemeData, t: f32) -> ListTileThemeData {
        ListTileThemeData {
            dense: lerp_nearer(&a.dense, &b.dense, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            style: lerp_nearer(&a.style, &b.style, t),
            selected_color: lerp_color(a.selected_color, b.selected_color, t),
            icon_color: lerp_color(a.icon_color, b.icon_color, t),
            text_color: lerp_color(a.text_color, b.text_color, t),
            title_text_style: lerp_nearer(&a.title_text_style, &b.title_text_style, t),
            subtitle_text_style: lerp_nearer(&a.subtitle_text_style, &b.subtitle_text_style, t),
            leading_and_trailing_text_style: lerp_nearer(
                &a.leading_and_trailing_text_style,
                &b.leading_and_trailing_text_style,
                t,
            ),
            content_padding: lerp_nearer(&a.content_padding, &b.content_padding, t),
            tile_color: lerp_color(a.tile_color, b.tile_color, t),
            selected_tile_color: lerp_color(a.selected_tile_color, b.selected_tile_color, t),
            horizontal_title_gap: lerp_f32(a.horizontal_title_gap, b.horizontal_title_gap, t),
            min_vertical_padding: lerp_f32(a.min_vertical_padding, b.min_vertical_padding, t),
            min_leading_width: lerp_f32(a.min_leading_width, b.min_leading_width, t),
            min_tile_height: lerp_f32(a.min_tile_height, b.min_tile_height, t),
            enable_feedback: lerp_nearer(&a.enable_feedback, &b.enable_feedback, t),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
            visual_density: match (a.visual_density, b.visual_density) {
                (Some(first), Some(second)) => Some(VisualDensity::lerp(first, second, t)),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
            title_alignment: lerp_nearer(&a.title_alignment, &b.title_alignment, t),
            control_affinity: lerp_nearer(&a.control_affinity, &b.control_affinity, t),
            is_three_line: lerp_nearer(&a.is_three_line, &b.is_three_line, t),
        }
    }
}

/// Upstream `ListTileTheme`.
pub struct ListTileTheme;

impl ListTileTheme {
    pub fn new(data: ListTileThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> ListTileThemeData {
        context
            .inherited::<ListTileThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).list_tile_theme)
    }
}

/// What a list tile lays itself out with, once the three steps have run.
///
/// Upstream's `ListTile.build`: the padding, the gap and the minimum height
/// come off `ListTileTheme.of(context)` and then off upstream's own
/// constants, which differ for a dense tile.
pub struct ResolvedListTile {
    pub content_padding: EdgeInsets,
    pub horizontal_title_gap: f32,
    pub min_vertical_padding: f32,
    pub min_leading_width: f32,
    pub min_tile_height: f32,
    pub tile_color: Option<Color>,
    pub text_color: Color,
    pub dense: bool,
}

impl ResolvedListTile {
    /// Upstream's `_kMinTileHeight`-ish constants, which the Material 3
    /// defaults spell as 56 for a one-line tile and 48 when dense.
    pub const MIN_TILE_HEIGHT: f32 = 56.0;
    pub const DENSE_MIN_TILE_HEIGHT: f32 = 48.0;
    /// Upstream's default `horizontalTitleGap`.
    pub const HORIZONTAL_TITLE_GAP: f32 = 16.0;
    /// Upstream's default `minVerticalPadding`.
    pub const MIN_VERTICAL_PADDING: f32 = 4.0;
    /// Upstream's default `minLeadingWidth`.
    pub const MIN_LEADING_WIDTH: f32 = 40.0;

    /// Upstream's resolution, with the widget's own `dense` folded in.
    ///
    /// `dense_override` is the tile's own value, and it has to arrive *here*
    /// rather than being applied to the result, because `minTileHeight` is
    /// `data.minTileHeight ?? (dense ? 48 : 56)`: a theme that set the height
    /// explicitly wins outright and `dense` changes nothing, while a theme that
    /// did not gets one of two constants chosen by `dense`. Adjusting the
    /// height afterwards cannot tell those two cases apart.
    pub fn of(
        context: &mut BuildContext,
        selected: bool,
        dense_override: Option<bool>,
    ) -> ResolvedListTile {
        let data = ListTileTheme::of(context);
        let theme = ThemeData::of(context);
        let dense = dense_override.or(data.dense).unwrap_or(false);
        let text_color = if selected {
            data.selected_color.unwrap_or(theme.color_scheme.primary)
        } else {
            data.text_color.unwrap_or(theme.color_scheme.on_surface)
        };
        ResolvedListTile {
            content_padding: data
                .content_padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::symmetric(16.0, 0.0)),
            horizontal_title_gap: data
                .horizontal_title_gap
                .unwrap_or(ResolvedListTile::HORIZONTAL_TITLE_GAP),
            min_vertical_padding: data
                .min_vertical_padding
                .unwrap_or(ResolvedListTile::MIN_VERTICAL_PADDING),
            min_leading_width: data
                .min_leading_width
                .unwrap_or(ResolvedListTile::MIN_LEADING_WIDTH),
            min_tile_height: data.min_tile_height.unwrap_or(if dense {
                ResolvedListTile::DENSE_MIN_TILE_HEIGHT
            } else {
                ResolvedListTile::MIN_TILE_HEIGHT
            }),
            tile_color: if selected {
                data.selected_tile_color
            } else {
                data.tile_color
            },
            text_color,
            dense,
        }
    }
}

// -- Dialog (upstream `dialog_theme.dart`) ------------------------------------

/// Upstream `DialogThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DialogThemeData {
    pub background_color: Option<Color>,
    pub elevation: Option<f32>,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub shape: Option<ShapeBorder>,
    /// Where the dialog sits in the screen -- upstream's `alignment`, which
    /// is centre where nobody said.
    pub alignment: Option<AlignmentGeometry>,
    pub title_text_style: Option<TextStyle>,
    pub content_text_style: Option<TextStyle>,
    pub actions_padding: Option<EdgeInsetsGeometry>,
    pub icon_color: Option<Color>,
    /// What the screen behind the dialog is dimmed with.
    pub barrier_color: Option<Color>,
    /// How far the dialog stays from the edges of the screen.
    pub inset_padding: Option<EdgeInsets>,
    pub constraints: Option<BoxConstraints>,
}

impl DialogThemeData {
    pub fn new() -> DialogThemeData {
        DialogThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_barrier_color(mut self, color: Color) -> Self {
        self.barrier_color = Some(color);
        self
    }

    pub fn with_inset_padding(mut self, padding: EdgeInsets) -> Self {
        self.inset_padding = Some(padding);
        self
    }

    pub fn with_alignment(mut self, alignment: AlignmentGeometry) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Upstream `DialogThemeData.lerp`.
    pub fn lerp(a: &DialogThemeData, b: &DialogThemeData, t: f32) -> DialogThemeData {
        DialogThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            alignment: lerp_nearer(&a.alignment, &b.alignment, t),
            title_text_style: lerp_nearer(&a.title_text_style, &b.title_text_style, t),
            content_text_style: lerp_nearer(&a.content_text_style, &b.content_text_style, t),
            actions_padding: lerp_nearer(&a.actions_padding, &b.actions_padding, t),
            icon_color: lerp_color(a.icon_color, b.icon_color, t),
            barrier_color: lerp_color(a.barrier_color, b.barrier_color, t),
            inset_padding: lerp_nearer(&a.inset_padding, &b.inset_padding, t),
            constraints: lerp_nearer(&a.constraints, &b.constraints, t),
        }
    }
}

/// Upstream `DialogTheme`.
pub struct DialogTheme;

impl DialogTheme {
    pub fn new(data: DialogThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> DialogThemeData {
        context
            .inherited::<DialogThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).dialog_theme)
    }
}

// -- Chip (upstream `chip_theme.dart`) ----------------------------------------

/// Upstream `ChipThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChipThemeData {
    /// The whole fill, by state -- Material 3's way in, which supersedes the
    /// four separate colours below when it is set.
    pub color: Option<StateProperty<Option<Color>>>,
    pub background_color: Option<Color>,
    pub delete_icon_color: Option<Color>,
    pub disabled_color: Option<Color>,
    pub selected_color: Option<Color>,
    /// The fill of a chip that is selected *and* is the secondary one in its
    /// group -- upstream's `secondarySelectedColor`.
    pub secondary_selected_color: Option<Color>,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub selected_shadow_color: Option<Color>,
    pub show_checkmark: Option<bool>,
    pub checkmark_color: Option<Color>,
    pub label_padding: Option<EdgeInsetsGeometry>,
    pub padding: Option<EdgeInsetsGeometry>,
    pub side: Option<BorderSide>,
    pub shape: Option<ShapeBorder>,
    pub label_style: Option<TextStyle>,
    pub secondary_label_style: Option<TextStyle>,
    pub brightness: Option<Brightness>,
    pub icon_theme: Option<IconThemeData>,
    pub elevation: Option<f32>,
    pub press_elevation: Option<f32>,
    pub avatar_box_constraints: Option<BoxConstraints>,
    pub delete_icon_box_constraints: Option<BoxConstraints>,
}

impl ChipThemeData {
    pub fn new() -> ChipThemeData {
        ChipThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_selected_color(mut self, color: Color) -> Self {
        self.selected_color = Some(color);
        self
    }

    pub fn with_disabled_color(mut self, color: Color) -> Self {
        self.disabled_color = Some(color);
        self
    }

    pub fn with_color(mut self, color: StateProperty<Option<Color>>) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_side(mut self, side: BorderSide) -> Self {
        self.side = Some(side);
        self
    }

    pub fn with_padding(mut self, padding: EdgeInsetsGeometry) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Upstream `ChipThemeData.lerp`.
    pub fn lerp(a: &ChipThemeData, b: &ChipThemeData, t: f32) -> ChipThemeData {
        ChipThemeData {
            color: lerp_state_color(a.color.as_ref(), b.color.as_ref(), t),
            background_color: lerp_color(a.background_color, b.background_color, t),
            delete_icon_color: lerp_color(a.delete_icon_color, b.delete_icon_color, t),
            disabled_color: lerp_color(a.disabled_color, b.disabled_color, t),
            selected_color: lerp_color(a.selected_color, b.selected_color, t),
            secondary_selected_color: lerp_color(
                a.secondary_selected_color,
                b.secondary_selected_color,
                t,
            ),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            selected_shadow_color: lerp_color(a.selected_shadow_color, b.selected_shadow_color, t),
            show_checkmark: lerp_nearer(&a.show_checkmark, &b.show_checkmark, t),
            checkmark_color: lerp_color(a.checkmark_color, b.checkmark_color, t),
            label_padding: lerp_nearer(&a.label_padding, &b.label_padding, t),
            padding: lerp_nearer(&a.padding, &b.padding, t),
            side: match (a.side, b.side) {
                (Some(first), Some(second)) => Some(BorderSide::lerp(first, second, t)),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
            shape: lerp_nearer(&a.shape, &b.shape, t),
            label_style: lerp_nearer(&a.label_style, &b.label_style, t),
            secondary_label_style: lerp_nearer(
                &a.secondary_label_style,
                &b.secondary_label_style,
                t,
            ),
            brightness: lerp_nearer(&a.brightness, &b.brightness, t),
            icon_theme: lerp_icon_theme(&a.icon_theme, &b.icon_theme, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            press_elevation: lerp_f32(a.press_elevation, b.press_elevation, t),
            avatar_box_constraints: lerp_nearer(
                &a.avatar_box_constraints,
                &b.avatar_box_constraints,
                t,
            ),
            delete_icon_box_constraints: lerp_nearer(
                &a.delete_icon_box_constraints,
                &b.delete_icon_box_constraints,
                t,
            ),
        }
    }
}

/// Upstream `ChipTheme`.
pub struct ChipTheme;

impl ChipTheme {
    pub fn new(data: ChipThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> ChipThemeData {
        context
            .inherited::<ChipThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).chip_theme)
    }
}

/// What a chip draws with, once the three steps have run.
///
/// Upstream's `RawChip.build` resolves the fill through `color` first --
/// Material 3's state property, which supersedes the four separate colours --
/// then `selectedColor` or `disabledColor` by the flags, then
/// `backgroundColor`, and finally the control's own default. That last step
/// is the `default_fill` argument: the control knows what it looks like when
/// nobody has themed it, and here that is one of the three chip styles the
/// crate draws.
pub struct ResolvedChip {
    pub fill: Color,
    pub side: Option<BorderSide>,
    pub padding: EdgeInsets,
}

impl ResolvedChip {
    /// Upstream's default chip padding.
    pub const PADDING: f32 = 4.0;

    pub fn of(
        context: &mut BuildContext,
        states: WidgetStates,
        default_fill: Color,
    ) -> ResolvedChip {
        let data = ChipTheme::of(context);
        let selected = states.contains(WidgetState::Selected);
        let disabled = states.contains(WidgetState::Disabled);
        let fill = data
            .color
            .as_ref()
            .and_then(|property| property.resolve(states))
            .or(if disabled {
                data.disabled_color
            } else if selected {
                data.selected_color
            } else {
                None
            })
            .or(data.background_color)
            .unwrap_or(default_fill);
        ResolvedChip {
            fill,
            side: data.side,
            padding: data
                .padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::all(ResolvedChip::PADDING)),
        }
    }
}

// -- Tab bar (upstream `tab_bar_theme.dart`, and the enums it needs) ----------

/// Upstream `TabBarIndicatorSize`: whether the indicator is as wide as the
/// tab or as wide as the label in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabBarIndicatorSize {
    Tab,
    Label,
}

/// Upstream `TabAlignment`: where the tabs sit in a bar wider than they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAlignment {
    Start,
    /// Start, with the leading offset Material's spec asks for.
    StartOffset,
    /// Stretched to fill the bar.
    Fill,
    Center,
}

/// Upstream `TabIndicatorAnimation`: how the indicator travels between tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabIndicatorAnimation {
    Linear,
    /// Material 3's: the indicator stretches as it goes and settles.
    Elastic,
}

/// Upstream `TabBarThemeData`.
///
/// `splashFactory` is not here: it is an `InteractiveInkFeatureFactory`, and
/// this crate's ink is a property of the control that draws it rather than a
/// factory a theme passes down (`ink.rs`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabBarThemeData {
    /// The whole indicator as a decoration, which supersedes
    /// [`TabBarThemeData::indicator_color`].
    pub indicator: Option<crate::decoration::Decoration>,
    pub indicator_color: Option<Color>,
    pub indicator_size: Option<TabBarIndicatorSize>,
    pub divider_color: Option<Color>,
    pub divider_height: Option<f32>,
    pub label_color: Option<Color>,
    pub label_padding: Option<EdgeInsetsGeometry>,
    pub label_style: Option<TextStyle>,
    pub unselected_label_color: Option<Color>,
    pub unselected_label_style: Option<TextStyle>,
    pub overlay_color: Option<StateProperty<Option<Color>>>,
    pub mouse_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
    pub tab_alignment: Option<TabAlignment>,
    pub text_scaler: Option<crate::painting::TextScaler>,
    pub indicator_animation: Option<TabIndicatorAnimation>,
}

impl TabBarThemeData {
    pub fn new() -> TabBarThemeData {
        TabBarThemeData::default()
    }

    pub fn with_indicator_color(mut self, color: Color) -> Self {
        self.indicator_color = Some(color);
        self
    }

    pub fn with_indicator_size(mut self, size: TabBarIndicatorSize) -> Self {
        self.indicator_size = Some(size);
        self
    }

    pub fn with_label_color(mut self, color: Color) -> Self {
        self.label_color = Some(color);
        self
    }

    pub fn with_unselected_label_color(mut self, color: Color) -> Self {
        self.unselected_label_color = Some(color);
        self
    }

    pub fn with_tab_alignment(mut self, alignment: TabAlignment) -> Self {
        self.tab_alignment = Some(alignment);
        self
    }

    /// Upstream `TabBarThemeData.lerp`.
    pub fn lerp(a: &TabBarThemeData, b: &TabBarThemeData, t: f32) -> TabBarThemeData {
        TabBarThemeData {
            indicator: lerp_nearer(&a.indicator, &b.indicator, t),
            indicator_color: lerp_color(a.indicator_color, b.indicator_color, t),
            indicator_size: lerp_nearer(&a.indicator_size, &b.indicator_size, t),
            divider_color: lerp_color(a.divider_color, b.divider_color, t),
            divider_height: lerp_f32(a.divider_height, b.divider_height, t),
            label_color: lerp_color(a.label_color, b.label_color, t),
            label_padding: lerp_nearer(&a.label_padding, &b.label_padding, t),
            label_style: lerp_nearer(&a.label_style, &b.label_style, t),
            unselected_label_color: lerp_color(
                a.unselected_label_color,
                b.unselected_label_color,
                t,
            ),
            unselected_label_style: lerp_nearer(
                &a.unselected_label_style,
                &b.unselected_label_style,
                t,
            ),
            overlay_color: lerp_state_color(a.overlay_color.as_ref(), b.overlay_color.as_ref(), t),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
            tab_alignment: lerp_nearer(&a.tab_alignment, &b.tab_alignment, t),
            text_scaler: lerp_nearer(&a.text_scaler, &b.text_scaler, t),
            indicator_animation: lerp_nearer(&a.indicator_animation, &b.indicator_animation, t),
        }
    }
}

/// Upstream `TabBarTheme`.
pub struct TabBarTheme;

impl TabBarTheme {
    pub fn new(data: TabBarThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> TabBarThemeData {
        context
            .inherited::<TabBarThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).tab_bar_theme)
    }
}

/// What a tab bar draws with, once the three steps have run.
pub struct ResolvedTabBar {
    pub indicator_color: Color,
    pub label_color: Color,
    pub unselected_label_color: Color,
    pub divider_color: Color,
    pub divider_height: f32,
    pub indicator_size: TabBarIndicatorSize,
    pub label_padding: EdgeInsets,
    pub label_style: Option<TextStyle>,
    pub unselected_label_style: Option<TextStyle>,
}

impl ResolvedTabBar {
    /// Upstream's Material 3 default indicator thickness.
    pub const INDICATOR_WEIGHT: f32 = 3.0;
    /// Upstream's default divider height.
    pub const DIVIDER_HEIGHT: f32 = 1.0;
    /// Upstream's pre-M3 unselected-label alpha: seventy per cent.
    pub const UNSELECTED_ALPHA: u8 = 0xB2;

    pub fn of(context: &mut BuildContext) -> ResolvedTabBar {
        let data = TabBarTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        ResolvedTabBar {
            // The indicator has a colour of its own and does not follow the
            // label: upstream's `_TabsPrimaryDefaultsM3.indicatorColor` is the
            // primary in its own right, and a theme that recolours the labels
            // leaves the underline where it was.
            indicator_color: data.indicator_color.unwrap_or(scheme.primary),
            // Five steps, not three. A colour set *inside the text style*
            // counts, and counts after both explicit `labelColor`s. Upstream's
            // comment says why the style is consulted so late: moving it up
            // would be a breaking change with no migration, so the less
            // specific place keeps the higher precedence.
            label_color: data
                .label_color
                .or_else(|| data.label_style.as_ref().map(|style| style.color))
                .unwrap_or(scheme.primary),
            unselected_label_color: data
                .unselected_label_color
                .or_else(|| {
                    data.unselected_label_style
                        .as_ref()
                        .map(|style| style.color)
                })
                .unwrap_or(scheme.on_surface_variant()),
            divider_color: data.divider_color.unwrap_or(scheme.outline_variant()),
            divider_height: data
                .divider_height
                .unwrap_or(ResolvedTabBar::DIVIDER_HEIGHT),
            // Upstream's Material 3 default is `TabBarIndicatorSize.tab`.
            indicator_size: data.indicator_size.unwrap_or(TabBarIndicatorSize::Tab),
            label_padding: data
                .label_padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::symmetric(16.0, 0.0)),
            label_style: data.label_style.clone(),
            unselected_label_style: data.unselected_label_style.clone(),
        }
    }

    /// Upstream's pre-Material-3 fallback for an unselected label: the selected
    /// colour at seventy per cent.
    ///
    /// Not the default here -- Material 3 gives the unselected label a scheme
    /// colour of its own -- but this is what the field *means*: the two labels
    /// are one colour said at two volumes, not two colours. A caller matching
    /// an older design needs it.
    pub fn unselected_from(selected: Color) -> Color {
        selected.with_alpha(ResolvedTabBar::UNSELECTED_ALPHA)
    }
}

// -- Data table (upstream `data_table_theme.dart`) ----------------------------

/// Upstream `DataTableThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DataTableThemeData {
    pub decoration: Option<crate::decoration::Decoration>,
    pub data_row_color: Option<StateProperty<Option<Color>>>,
    pub data_row_min_height: Option<f32>,
    pub data_row_max_height: Option<f32>,
    pub data_text_style: Option<TextStyle>,
    pub heading_row_color: Option<StateProperty<Option<Color>>>,
    pub heading_row_height: Option<f32>,
    pub heading_text_style: Option<TextStyle>,
    /// The space before the first column and after the last.
    pub horizontal_margin: Option<f32>,
    pub column_spacing: Option<f32>,
    pub divider_thickness: Option<f32>,
    pub checkbox_horizontal_margin: Option<f32>,
    pub heading_cell_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
    pub data_row_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
    pub heading_row_alignment: Option<crate::render::MainAxisAlignment>,
}

impl DataTableThemeData {
    pub fn new() -> DataTableThemeData {
        DataTableThemeData::default()
    }

    pub fn with_column_spacing(mut self, spacing: f32) -> Self {
        self.column_spacing = Some(spacing);
        self
    }

    pub fn with_horizontal_margin(mut self, margin: f32) -> Self {
        self.horizontal_margin = Some(margin);
        self
    }

    pub fn with_heading_row_height(mut self, height: f32) -> Self {
        self.heading_row_height = Some(height);
        self
    }

    pub fn with_data_row_heights(mut self, min: f32, max: f32) -> Self {
        self.data_row_min_height = Some(min);
        self.data_row_max_height = Some(max);
        self
    }

    /// Upstream `DataTableThemeData.lerp`.
    pub fn lerp(a: &DataTableThemeData, b: &DataTableThemeData, t: f32) -> DataTableThemeData {
        DataTableThemeData {
            decoration: lerp_nearer(&a.decoration, &b.decoration, t),
            data_row_color: lerp_state_color(
                a.data_row_color.as_ref(),
                b.data_row_color.as_ref(),
                t,
            ),
            data_row_min_height: lerp_f32(a.data_row_min_height, b.data_row_min_height, t),
            data_row_max_height: lerp_f32(a.data_row_max_height, b.data_row_max_height, t),
            data_text_style: lerp_nearer(&a.data_text_style, &b.data_text_style, t),
            heading_row_color: lerp_state_color(
                a.heading_row_color.as_ref(),
                b.heading_row_color.as_ref(),
                t,
            ),
            heading_row_height: lerp_f32(a.heading_row_height, b.heading_row_height, t),
            heading_text_style: lerp_nearer(&a.heading_text_style, &b.heading_text_style, t),
            horizontal_margin: lerp_f32(a.horizontal_margin, b.horizontal_margin, t),
            column_spacing: lerp_f32(a.column_spacing, b.column_spacing, t),
            divider_thickness: lerp_f32(a.divider_thickness, b.divider_thickness, t),
            checkbox_horizontal_margin: lerp_f32(
                a.checkbox_horizontal_margin,
                b.checkbox_horizontal_margin,
                t,
            ),
            heading_cell_cursor: lerp_nearer(&a.heading_cell_cursor, &b.heading_cell_cursor, t),
            data_row_cursor: lerp_nearer(&a.data_row_cursor, &b.data_row_cursor, t),
            heading_row_alignment: lerp_nearer(
                &a.heading_row_alignment,
                &b.heading_row_alignment,
                t,
            ),
        }
    }
}

/// Upstream `DataTableTheme`.
pub struct DataTableTheme;

impl DataTableTheme {
    pub fn new(data: DataTableThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> DataTableThemeData {
        context
            .inherited::<DataTableThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).data_table_theme)
    }
}

// -- Navigation rail (upstream `navigation_rail_theme.dart`) ------------------

/// Upstream `NavigationRailLabelType`: which of a rail's labels are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationRailLabelType {
    /// Icons only.
    None,
    /// The selected destination's label, and no others.
    Selected,
    /// Every label.
    All,
}

/// Upstream `NavigationRailThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavigationRailThemeData {
    pub background_color: Option<Color>,
    pub elevation: Option<f32>,
    pub unselected_label_text_style: Option<TextStyle>,
    pub selected_label_text_style: Option<TextStyle>,
    pub unselected_icon_theme: Option<IconThemeData>,
    pub selected_icon_theme: Option<IconThemeData>,
    /// Where the destinations sit along the rail: -1 top, 0 centre, 1 bottom.
    pub group_alignment: Option<f32>,
    pub label_type: Option<NavigationRailLabelType>,
    /// Whether the selected destination gets Material 3's pill behind it.
    pub use_indicator: Option<bool>,
    pub indicator_color: Option<Color>,
    pub indicator_shape: Option<ShapeBorder>,
    pub min_width: Option<f32>,
    /// The width once the rail is extended to show its labels beside the
    /// icons.
    pub min_extended_width: Option<f32>,
}

impl NavigationRailThemeData {
    pub fn new() -> NavigationRailThemeData {
        NavigationRailThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_label_type(mut self, label_type: NavigationRailLabelType) -> Self {
        self.label_type = Some(label_type);
        self
    }

    pub fn with_indicator_color(mut self, color: Color) -> Self {
        self.indicator_color = Some(color);
        self
    }

    pub fn with_min_width(mut self, width: f32) -> Self {
        self.min_width = Some(width);
        self
    }

    /// Upstream `NavigationRailThemeData.lerp`.
    pub fn lerp(
        a: &NavigationRailThemeData,
        b: &NavigationRailThemeData,
        t: f32,
    ) -> NavigationRailThemeData {
        NavigationRailThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            unselected_label_text_style: lerp_nearer(
                &a.unselected_label_text_style,
                &b.unselected_label_text_style,
                t,
            ),
            selected_label_text_style: lerp_nearer(
                &a.selected_label_text_style,
                &b.selected_label_text_style,
                t,
            ),
            unselected_icon_theme: lerp_icon_theme(
                &a.unselected_icon_theme,
                &b.unselected_icon_theme,
                t,
            ),
            selected_icon_theme: lerp_icon_theme(&a.selected_icon_theme, &b.selected_icon_theme, t),
            group_alignment: lerp_f32(a.group_alignment, b.group_alignment, t),
            label_type: lerp_nearer(&a.label_type, &b.label_type, t),
            use_indicator: lerp_nearer(&a.use_indicator, &b.use_indicator, t),
            indicator_color: lerp_color(a.indicator_color, b.indicator_color, t),
            indicator_shape: lerp_nearer(&a.indicator_shape, &b.indicator_shape, t),
            min_width: lerp_f32(a.min_width, b.min_width, t),
            min_extended_width: lerp_f32(a.min_extended_width, b.min_extended_width, t),
        }
    }
}

/// Upstream `NavigationRailTheme`.
pub struct NavigationRailTheme;

impl NavigationRailTheme {
    pub fn new(data: NavigationRailThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> NavigationRailThemeData {
        context
            .inherited::<NavigationRailThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).navigation_rail_theme)
    }
}

// -- Bottom navigation bar (upstream `bottom_navigation_bar_theme.dart`) ------

/// Upstream `BottomNavigationBarType` and `BottomNavigationBarLandscapeLayout`.
///
/// Defined with the widget in [`crate::bottom_bars`] and re-exported here
/// rather than declared twice. They were declared twice, and the second copy
/// only announced itself when the theme's resolution tried to hand one to
/// `BottomNavigationBar::effective_type` -- two types with the same name, the
/// same variants and the same upstream original, which the compiler is right
/// to refuse. A type two modules have to agree on belongs to neither of them,
/// which is the same rule `ButtonVariant::default_colors` was moved for.
pub use crate::bottom_bars::{BottomNavigationBarLandscapeLayout, BottomNavigationBarType};

/// Upstream `BottomNavigationBarThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BottomNavigationBarThemeData {
    pub background_color: Option<Color>,
    pub elevation: Option<f32>,
    pub selected_icon_theme: Option<IconThemeData>,
    pub unselected_icon_theme: Option<IconThemeData>,
    pub selected_item_color: Option<Color>,
    pub unselected_item_color: Option<Color>,
    pub selected_label_style: Option<TextStyle>,
    pub unselected_label_style: Option<TextStyle>,
    pub show_selected_labels: Option<bool>,
    pub show_unselected_labels: Option<bool>,
    pub bar_type: Option<BottomNavigationBarType>,
    pub enable_feedback: Option<bool>,
    pub landscape_layout: Option<BottomNavigationBarLandscapeLayout>,
    pub mouse_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
}

impl BottomNavigationBarThemeData {
    pub fn new() -> BottomNavigationBarThemeData {
        BottomNavigationBarThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_item_colors(mut self, selected: Color, unselected: Color) -> Self {
        self.selected_item_color = Some(selected);
        self.unselected_item_color = Some(unselected);
        self
    }

    pub fn with_show_labels(mut self, selected: bool, unselected: bool) -> Self {
        self.show_selected_labels = Some(selected);
        self.show_unselected_labels = Some(unselected);
        self
    }

    /// Upstream `BottomNavigationBarThemeData.lerp`.
    pub fn lerp(
        a: &BottomNavigationBarThemeData,
        b: &BottomNavigationBarThemeData,
        t: f32,
    ) -> BottomNavigationBarThemeData {
        BottomNavigationBarThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            selected_icon_theme: lerp_icon_theme(&a.selected_icon_theme, &b.selected_icon_theme, t),
            unselected_icon_theme: lerp_icon_theme(
                &a.unselected_icon_theme,
                &b.unselected_icon_theme,
                t,
            ),
            selected_item_color: lerp_color(a.selected_item_color, b.selected_item_color, t),
            unselected_item_color: lerp_color(a.unselected_item_color, b.unselected_item_color, t),
            selected_label_style: lerp_nearer(&a.selected_label_style, &b.selected_label_style, t),
            unselected_label_style: lerp_nearer(
                &a.unselected_label_style,
                &b.unselected_label_style,
                t,
            ),
            show_selected_labels: lerp_nearer(&a.show_selected_labels, &b.show_selected_labels, t),
            show_unselected_labels: lerp_nearer(
                &a.show_unselected_labels,
                &b.show_unselected_labels,
                t,
            ),
            bar_type: lerp_nearer(&a.bar_type, &b.bar_type, t),
            enable_feedback: lerp_nearer(&a.enable_feedback, &b.enable_feedback, t),
            landscape_layout: lerp_nearer(&a.landscape_layout, &b.landscape_layout, t),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
        }
    }
}

/// Upstream `BottomNavigationBarTheme`.
pub struct BottomNavigationBarTheme;

impl BottomNavigationBarTheme {
    pub fn new(data: BottomNavigationBarThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> BottomNavigationBarThemeData {
        context
            .inherited::<BottomNavigationBarThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).bottom_navigation_bar_theme)
    }
}

// -- Drawer (upstream `drawer_theme.dart`) ------------------------------------

/// Upstream `DrawerThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawerThemeData {
    pub background_color: Option<Color>,
    /// What the rest of the screen is dimmed with while the drawer is open.
    pub scrim_color: Option<Color>,
    pub elevation: Option<f32>,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    /// The shape of a drawer that opens from the reading edge.
    pub shape: Option<ShapeBorder>,
    /// The shape of one that opens from the other edge -- upstream's
    /// `endShape`, which is a separate field because a drawer's rounded
    /// corners are on the inner side and that side swaps.
    pub end_shape: Option<ShapeBorder>,
    pub width: Option<f32>,
}

impl DrawerThemeData {
    pub fn new() -> DrawerThemeData {
        DrawerThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_scrim_color(mut self, color: Color) -> Self {
        self.scrim_color = Some(color);
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Upstream `DrawerThemeData.lerp`.
    pub fn lerp(a: &DrawerThemeData, b: &DrawerThemeData, t: f32) -> DrawerThemeData {
        DrawerThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            scrim_color: lerp_color(a.scrim_color, b.scrim_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            end_shape: lerp_nearer(&a.end_shape, &b.end_shape, t),
            width: lerp_f32(a.width, b.width, t),
        }
    }
}

/// Upstream `DrawerTheme`.
pub struct DrawerTheme;

impl DrawerTheme {
    pub fn new(data: DrawerThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> DrawerThemeData {
        context
            .inherited::<DrawerThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).drawer_theme)
    }
}

/// What a drawer draws with, once the three steps have run.
pub struct ResolvedDrawer {
    pub background: Color,
    pub scrim: Color,
    pub width: f32,
}

impl ResolvedDrawer {
    /// Upstream's `_kWidth`.
    pub const WIDTH: f32 = 304.0;
    /// Upstream's `_kScrimColor` -- black at 54 per cent.
    pub const SCRIM: Color = Color(0x8a000000);

    pub fn of(context: &mut BuildContext) -> ResolvedDrawer {
        let data = DrawerTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        ResolvedDrawer {
            background: data
                .background_color
                .unwrap_or(scheme.surface_container_low()),
            scrim: data.scrim_color.unwrap_or(ResolvedDrawer::SCRIM),
            width: data.width.unwrap_or(ResolvedDrawer::WIDTH),
        }
    }
}

// -- Button style (upstream `button_style.dart`) ------------------------------

/// Upstream `IconAlignment`: which side of a button's label its icon sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconAlignment {
    Start,
    End,
}

/// Upstream `ButtonStyle`: everything a `ButtonStyleButton` can be told,
/// state by state.
///
/// Nearly every field is a [`StateProperty`], because a button looks
/// different hovered, focused, pressed and disabled, and a theme that could
/// only name one colour could not say so. The four button widgets upstream
/// differ only in the style they default to, which is what
/// [`ButtonVariant`](crate::components::ButtonVariant) is here.
///
/// `splashFactory`, `backgroundBuilder` and `foregroundBuilder` are not here:
/// the first is an `InteractiveInkFeatureFactory` (ink belongs to the control
/// that draws it in this crate), and the other two are builders that wrap the
/// button's child in an arbitrary widget, which needs the widget-in-a-theme
/// shape this port has no place for yet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ButtonStyle {
    pub text_style: Option<StateProperty<Option<TextStyle>>>,
    pub background_color: Option<StateProperty<Option<Color>>>,
    pub foreground_color: Option<StateProperty<Option<Color>>>,
    pub overlay_color: Option<StateProperty<Option<Color>>>,
    pub shadow_color: Option<StateProperty<Option<Color>>>,
    pub surface_tint_color: Option<StateProperty<Option<Color>>>,
    pub elevation: Option<StateProperty<Option<f32>>>,
    pub padding: Option<StateProperty<Option<EdgeInsetsGeometry>>>,
    pub minimum_size: Option<StateProperty<Option<Size>>>,
    pub fixed_size: Option<StateProperty<Option<Size>>>,
    pub maximum_size: Option<StateProperty<Option<Size>>>,
    pub icon_color: Option<StateProperty<Option<Color>>>,
    pub icon_size: Option<StateProperty<Option<f32>>>,
    pub icon_alignment: Option<IconAlignment>,
    pub side: Option<StateProperty<Option<BorderSide>>>,
    pub shape: Option<StateProperty<Option<ShapeBorder>>>,
    pub mouse_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
    pub visual_density: Option<VisualDensity>,
    pub tap_target_size: Option<MaterialTapTargetSize>,
    /// How long the button takes to move between two states' appearances.
    pub animation_duration: Option<std::time::Duration>,
    pub enable_feedback: Option<bool>,
    pub alignment: Option<AlignmentGeometry>,
}

impl ButtonStyle {
    pub fn new() -> ButtonStyle {
        ButtonStyle::default()
    }

    pub fn with_background_color(mut self, color: StateProperty<Option<Color>>) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_foreground_color(mut self, color: StateProperty<Option<Color>>) -> Self {
        self.foreground_color = Some(color);
        self
    }

    pub fn with_side(mut self, side: StateProperty<Option<BorderSide>>) -> Self {
        self.side = Some(side);
        self
    }

    pub fn with_padding(mut self, padding: StateProperty<Option<EdgeInsetsGeometry>>) -> Self {
        self.padding = Some(padding);
        self
    }

    pub fn with_minimum_size(mut self, size: StateProperty<Option<Size>>) -> Self {
        self.minimum_size = Some(size);
        self
    }

    pub fn with_elevation(mut self, elevation: StateProperty<Option<f32>>) -> Self {
        self.elevation = Some(elevation);
        self
    }

    pub fn with_tap_target_size(mut self, size: MaterialTapTargetSize) -> Self {
        self.tap_target_size = Some(size);
        self
    }

    /// Upstream `ButtonStyle.merge`: this style's fields where it has them,
    /// the other's where it does not.
    ///
    /// This is how the four button widgets combine what the caller passed
    /// with the theme's style and then with their own defaults -- three
    /// merges, in that order.
    pub fn merge(&self, other: &ButtonStyle) -> ButtonStyle {
        fn pick<T: Clone>(mine: &Option<T>, theirs: &Option<T>) -> Option<T> {
            mine.clone().or_else(|| theirs.clone())
        }
        ButtonStyle {
            text_style: pick(&self.text_style, &other.text_style),
            background_color: pick(&self.background_color, &other.background_color),
            foreground_color: pick(&self.foreground_color, &other.foreground_color),
            overlay_color: pick(&self.overlay_color, &other.overlay_color),
            shadow_color: pick(&self.shadow_color, &other.shadow_color),
            surface_tint_color: pick(&self.surface_tint_color, &other.surface_tint_color),
            elevation: pick(&self.elevation, &other.elevation),
            padding: pick(&self.padding, &other.padding),
            minimum_size: pick(&self.minimum_size, &other.minimum_size),
            fixed_size: pick(&self.fixed_size, &other.fixed_size),
            maximum_size: pick(&self.maximum_size, &other.maximum_size),
            icon_color: pick(&self.icon_color, &other.icon_color),
            icon_size: pick(&self.icon_size, &other.icon_size),
            icon_alignment: pick(&self.icon_alignment, &other.icon_alignment),
            side: pick(&self.side, &other.side),
            shape: pick(&self.shape, &other.shape),
            mouse_cursor: pick(&self.mouse_cursor, &other.mouse_cursor),
            visual_density: pick(&self.visual_density, &other.visual_density),
            tap_target_size: pick(&self.tap_target_size, &other.tap_target_size),
            animation_duration: pick(&self.animation_duration, &other.animation_duration),
            enable_feedback: pick(&self.enable_feedback, &other.enable_feedback),
            alignment: pick(&self.alignment, &other.alignment),
        }
    }

    /// Upstream `ButtonStyle.lerp`.
    pub fn lerp(a: &ButtonStyle, b: &ButtonStyle, t: f32) -> ButtonStyle {
        ButtonStyle {
            text_style: lerp_nearer(&a.text_style, &b.text_style, t),
            background_color: lerp_state_color(
                a.background_color.as_ref(),
                b.background_color.as_ref(),
                t,
            ),
            foreground_color: lerp_state_color(
                a.foreground_color.as_ref(),
                b.foreground_color.as_ref(),
                t,
            ),
            overlay_color: lerp_state_color(a.overlay_color.as_ref(), b.overlay_color.as_ref(), t),
            shadow_color: lerp_state_color(a.shadow_color.as_ref(), b.shadow_color.as_ref(), t),
            surface_tint_color: lerp_state_color(
                a.surface_tint_color.as_ref(),
                b.surface_tint_color.as_ref(),
                t,
            ),
            elevation: lerp_nearer(&a.elevation, &b.elevation, t),
            padding: lerp_nearer(&a.padding, &b.padding, t),
            minimum_size: lerp_nearer(&a.minimum_size, &b.minimum_size, t),
            fixed_size: lerp_nearer(&a.fixed_size, &b.fixed_size, t),
            maximum_size: lerp_nearer(&a.maximum_size, &b.maximum_size, t),
            icon_color: lerp_state_color(a.icon_color.as_ref(), b.icon_color.as_ref(), t),
            icon_size: lerp_nearer(&a.icon_size, &b.icon_size, t),
            icon_alignment: lerp_nearer(&a.icon_alignment, &b.icon_alignment, t),
            side: lerp_nearer(&a.side, &b.side, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
            visual_density: match (a.visual_density, b.visual_density) {
                (Some(first), Some(second)) => Some(VisualDensity::lerp(first, second, t)),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
            tap_target_size: lerp_nearer(&a.tap_target_size, &b.tap_target_size, t),
            animation_duration: lerp_nearer(&a.animation_duration, &b.animation_duration, t),
            enable_feedback: lerp_nearer(&a.enable_feedback, &b.enable_feedback, t),
            alignment: lerp_nearer(&a.alignment, &b.alignment, t),
        }
    }
}

/// Two optional styles interpolated -- what every button theme's `lerp` is.
fn lerp_button_style(
    a: &Option<ButtonStyle>,
    b: &Option<ButtonStyle>,
    t: f32,
) -> Option<ButtonStyle> {
    match (a, b) {
        (Some(first), Some(second)) => Some(ButtonStyle::lerp(first, second, t)),
        (first, second) => {
            if t < 0.5 {
                first.clone()
            } else {
                second.clone()
            }
        }
    }
}

/// Upstream `ElevatedButtonThemeData` / `ElevatedButtonTheme`.
///
/// Upstream declares this as a class with one field -- the style its buttons
/// take -- and a widget to install it. So does this.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElevatedButtonThemeData {
    pub style: Option<ButtonStyle>,
}

impl ElevatedButtonThemeData {
    pub fn new() -> ElevatedButtonThemeData {
        ElevatedButtonThemeData::default()
    }

    pub fn with_style(mut self, style: ButtonStyle) -> ElevatedButtonThemeData {
        self.style = Some(style);
        self
    }

    /// Upstream's `lerp` for this theme, which is its style's.
    pub fn lerp(
        a: &ElevatedButtonThemeData,
        b: &ElevatedButtonThemeData,
        t: f32,
    ) -> ElevatedButtonThemeData {
        ElevatedButtonThemeData {
            style: lerp_button_style(&a.style, &b.style, t),
        }
    }
}

/// Upstream `ElevatedButtonTheme`.
pub struct ElevatedButtonTheme;

impl ElevatedButtonTheme {
    pub fn new(data: ElevatedButtonThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> ElevatedButtonThemeData {
        context
            .inherited::<ElevatedButtonThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).elevated_button_theme.clone())
    }
}

/// Upstream `FilledButtonThemeData` / `FilledButtonTheme`.
///
/// Upstream declares this as a class with one field -- the style its buttons
/// take -- and a widget to install it. So does this.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilledButtonThemeData {
    pub style: Option<ButtonStyle>,
}

impl FilledButtonThemeData {
    pub fn new() -> FilledButtonThemeData {
        FilledButtonThemeData::default()
    }

    pub fn with_style(mut self, style: ButtonStyle) -> FilledButtonThemeData {
        self.style = Some(style);
        self
    }

    /// Upstream's `lerp` for this theme, which is its style's.
    pub fn lerp(
        a: &FilledButtonThemeData,
        b: &FilledButtonThemeData,
        t: f32,
    ) -> FilledButtonThemeData {
        FilledButtonThemeData {
            style: lerp_button_style(&a.style, &b.style, t),
        }
    }
}

/// Upstream `FilledButtonTheme`.
pub struct FilledButtonTheme;

impl FilledButtonTheme {
    pub fn new(data: FilledButtonThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> FilledButtonThemeData {
        context
            .inherited::<FilledButtonThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).filled_button_theme.clone())
    }
}

/// Upstream `TextButtonThemeData` / `TextButtonTheme`.
///
/// Upstream declares this as a class with one field -- the style its buttons
/// take -- and a widget to install it. So does this.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextButtonThemeData {
    pub style: Option<ButtonStyle>,
}

impl TextButtonThemeData {
    pub fn new() -> TextButtonThemeData {
        TextButtonThemeData::default()
    }

    pub fn with_style(mut self, style: ButtonStyle) -> TextButtonThemeData {
        self.style = Some(style);
        self
    }

    /// Upstream's `lerp` for this theme, which is its style's.
    pub fn lerp(a: &TextButtonThemeData, b: &TextButtonThemeData, t: f32) -> TextButtonThemeData {
        TextButtonThemeData {
            style: lerp_button_style(&a.style, &b.style, t),
        }
    }
}

/// Upstream `TextButtonTheme`.
pub struct TextButtonTheme;

impl TextButtonTheme {
    pub fn new(data: TextButtonThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> TextButtonThemeData {
        context
            .inherited::<TextButtonThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).text_button_theme.clone())
    }
}

/// Upstream `OutlinedButtonThemeData` / `OutlinedButtonTheme`.
///
/// Upstream declares this as a class with one field -- the style its buttons
/// take -- and a widget to install it. So does this.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutlinedButtonThemeData {
    pub style: Option<ButtonStyle>,
}

impl OutlinedButtonThemeData {
    pub fn new() -> OutlinedButtonThemeData {
        OutlinedButtonThemeData::default()
    }

    pub fn with_style(mut self, style: ButtonStyle) -> OutlinedButtonThemeData {
        self.style = Some(style);
        self
    }

    /// Upstream's `lerp` for this theme, which is its style's.
    pub fn lerp(
        a: &OutlinedButtonThemeData,
        b: &OutlinedButtonThemeData,
        t: f32,
    ) -> OutlinedButtonThemeData {
        OutlinedButtonThemeData {
            style: lerp_button_style(&a.style, &b.style, t),
        }
    }
}

/// Upstream `OutlinedButtonTheme`.
pub struct OutlinedButtonTheme;

impl OutlinedButtonTheme {
    pub fn new(data: OutlinedButtonThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> OutlinedButtonThemeData {
        context
            .inherited::<OutlinedButtonThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).outlined_button_theme.clone())
    }
}

/// Upstream `IconButtonThemeData` / `IconButtonTheme`.
///
/// Upstream declares this as a class with one field -- the style its buttons
/// take -- and a widget to install it. So does this.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IconButtonThemeData {
    pub style: Option<ButtonStyle>,
}

impl IconButtonThemeData {
    pub fn new() -> IconButtonThemeData {
        IconButtonThemeData::default()
    }

    pub fn with_style(mut self, style: ButtonStyle) -> IconButtonThemeData {
        self.style = Some(style);
        self
    }

    /// Upstream's `lerp` for this theme, which is its style's.
    pub fn lerp(a: &IconButtonThemeData, b: &IconButtonThemeData, t: f32) -> IconButtonThemeData {
        IconButtonThemeData {
            style: lerp_button_style(&a.style, &b.style, t),
        }
    }
}

/// Upstream `IconButtonTheme`.
pub struct IconButtonTheme;

impl IconButtonTheme {
    pub fn new(data: IconButtonThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> IconButtonThemeData {
        context
            .inherited::<IconButtonThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).icon_button_theme.clone())
    }
}

/// What a button draws with, once the three steps and the merges have run.
///
/// Upstream's `ButtonStyleButton.build` merges three styles -- the one the
/// caller passed, the one the theme installed, and the widget's own defaults
/// -- and then resolves each field against the button's states.
pub struct ResolvedButton {
    pub background: Option<Color>,
    pub foreground: Color,
    pub side: Option<BorderSide>,
    pub padding: Option<EdgeInsets>,
    pub minimum_size: Option<Size>,
}

impl ResolvedButton {
    /// Resolves for a button of `variant`, whose own defaults are the last
    /// word.
    pub fn of(
        context: &mut BuildContext,
        variant: crate::components::ButtonVariant,
        states: WidgetStates,
        defaults: ResolvedButton,
    ) -> ResolvedButton {
        use crate::components::ButtonVariant;
        // Each variant reads the theme upstream's matching widget reads.
        let style = match variant {
            ButtonVariant::Filled | ButtonVariant::Danger => FilledButtonTheme::of(context).style,
            ButtonVariant::Elevated => ElevatedButtonTheme::of(context).style,
            ButtonVariant::Outlined => OutlinedButtonTheme::of(context).style,
            ButtonVariant::Text => TextButtonTheme::of(context).style,
        };
        let Some(style) = style else {
            return defaults;
        };
        ResolvedButton {
            background: style
                .background_color
                .as_ref()
                .and_then(|property| property.resolve(states))
                .or(defaults.background),
            foreground: style
                .foreground_color
                .as_ref()
                .and_then(|property| property.resolve(states))
                .unwrap_or(defaults.foreground),
            side: style
                .side
                .as_ref()
                .and_then(|property| property.resolve(states))
                .or(defaults.side),
            padding: style
                .padding
                .as_ref()
                .and_then(|property| property.resolve(states))
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .or(defaults.padding),
            minimum_size: style
                .minimum_size
                .as_ref()
                .and_then(|property| property.resolve(states))
                .or(defaults.minimum_size),
        }
    }
}

// -- Material banner (upstream `banner_theme.dart`) ---------------------------

/// Upstream `MaterialBannerThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialBannerThemeData {
    pub background_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub shadow_color: Option<Color>,
    /// The rule the banner draws under itself.
    pub divider_color: Option<Color>,
    pub content_text_style: Option<TextStyle>,
    pub elevation: Option<f32>,
    pub padding: Option<EdgeInsetsGeometry>,
    /// The space around the leading widget, when there is one.
    pub leading_padding: Option<EdgeInsetsGeometry>,
}

impl MaterialBannerThemeData {
    pub fn new() -> MaterialBannerThemeData {
        MaterialBannerThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_divider_color(mut self, color: Color) -> Self {
        self.divider_color = Some(color);
        self
    }

    pub fn with_padding(mut self, padding: EdgeInsetsGeometry) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Upstream `MaterialBannerThemeData.lerp`.
    pub fn lerp(
        a: &MaterialBannerThemeData,
        b: &MaterialBannerThemeData,
        t: f32,
    ) -> MaterialBannerThemeData {
        MaterialBannerThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            divider_color: lerp_color(a.divider_color, b.divider_color, t),
            content_text_style: lerp_nearer(&a.content_text_style, &b.content_text_style, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            padding: lerp_nearer(&a.padding, &b.padding, t),
            leading_padding: lerp_nearer(&a.leading_padding, &b.leading_padding, t),
        }
    }
}

/// Upstream `MaterialBannerTheme`.
pub struct MaterialBannerTheme;

impl MaterialBannerTheme {
    pub fn new(data: MaterialBannerThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> MaterialBannerThemeData {
        context
            .inherited::<MaterialBannerThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).banner_theme.clone())
    }
}

// -- Expansion tile (upstream `expansion_tile_theme.dart`) --------------------

/// Upstream `ExpansionTileThemeData`.
///
/// Nearly every field comes in a pair -- one for the expanded tile and one
/// for the collapsed one -- because the two are different enough that
/// interpolating between them is not what a theme wants to say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExpansionTileThemeData {
    pub background_color: Option<Color>,
    pub collapsed_background_color: Option<Color>,
    pub tile_padding: Option<EdgeInsetsGeometry>,
    /// Where the expanded children sit inside the tile.
    pub expanded_alignment: Option<AlignmentGeometry>,
    pub children_padding: Option<EdgeInsetsGeometry>,
    pub icon_color: Option<Color>,
    pub collapsed_icon_color: Option<Color>,
    pub text_color: Option<Color>,
    pub collapsed_text_color: Option<Color>,
    pub shape: Option<ShapeBorder>,
    pub collapsed_shape: Option<ShapeBorder>,
    /// How the tile opens and closes.
    pub expansion_animation_style: Option<crate::animation::AnimationStyle>,
}

impl ExpansionTileThemeData {
    pub fn new() -> ExpansionTileThemeData {
        ExpansionTileThemeData::default()
    }

    pub fn with_background_colors(mut self, expanded: Color, collapsed: Color) -> Self {
        self.background_color = Some(expanded);
        self.collapsed_background_color = Some(collapsed);
        self
    }

    pub fn with_text_colors(mut self, expanded: Color, collapsed: Color) -> Self {
        self.text_color = Some(expanded);
        self.collapsed_text_color = Some(collapsed);
        self
    }

    pub fn with_tile_padding(mut self, padding: EdgeInsetsGeometry) -> Self {
        self.tile_padding = Some(padding);
        self
    }

    /// Upstream `ExpansionTileThemeData.lerp`.
    pub fn lerp(
        a: &ExpansionTileThemeData,
        b: &ExpansionTileThemeData,
        t: f32,
    ) -> ExpansionTileThemeData {
        ExpansionTileThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            collapsed_background_color: lerp_color(
                a.collapsed_background_color,
                b.collapsed_background_color,
                t,
            ),
            tile_padding: lerp_nearer(&a.tile_padding, &b.tile_padding, t),
            expanded_alignment: lerp_nearer(&a.expanded_alignment, &b.expanded_alignment, t),
            children_padding: lerp_nearer(&a.children_padding, &b.children_padding, t),
            icon_color: lerp_color(a.icon_color, b.icon_color, t),
            collapsed_icon_color: lerp_color(a.collapsed_icon_color, b.collapsed_icon_color, t),
            text_color: lerp_color(a.text_color, b.text_color, t),
            collapsed_text_color: lerp_color(a.collapsed_text_color, b.collapsed_text_color, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            collapsed_shape: lerp_nearer(&a.collapsed_shape, &b.collapsed_shape, t),
            expansion_animation_style: lerp_nearer(
                &a.expansion_animation_style,
                &b.expansion_animation_style,
                t,
            ),
        }
    }
}

/// Upstream `ExpansionTileTheme`.
pub struct ExpansionTileTheme;

impl ExpansionTileTheme {
    pub fn new(data: ExpansionTileThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> ExpansionTileThemeData {
        context
            .inherited::<ExpansionTileThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).expansion_tile_theme.clone())
    }
}

// -- The Material 2 button theme (upstream `button_theme.dart`) ---------------

/// Upstream `ButtonTextTheme`: which colour a Material 2 button's label
/// takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonTextTheme {
    /// Black or white, by the theme's brightness.
    #[default]
    Normal,
    /// The scheme's secondary.
    Accent,
    /// The scheme's primary, with the fill following it.
    Primary,
}

/// Upstream `ButtonBarLayoutBehavior`: whether a bar of buttons is padded out
/// to the minimum touch height or takes only what it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonBarLayoutBehavior {
    /// Constrained to `kMinInteractiveDimension`.
    #[default]
    Padded,
    Constrained,
}

/// Upstream `ButtonThemeData`: the Material 2 button theme.
///
/// Not to be confused with [`ButtonStyle`] and the five `*ButtonThemeData`
/// classes, which are the Material 3 way. Upstream keeps this one for the
/// widgets that predate them, and so does this -- it is a different set of
/// questions (a minimum width and a height, rather than a property per
/// state).
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonThemeData {
    pub min_width: f32,
    pub height: f32,
    pub text_theme: ButtonTextTheme,
    pub layout_behavior: ButtonBarLayoutBehavior,
    pub padding: Option<EdgeInsetsGeometry>,
    pub shape: Option<ShapeBorder>,
    /// Whether a dropdown's menu lines up with the button that opened it.
    pub aligned_dropdown: bool,
    pub button_color: Option<Color>,
    pub disabled_color: Option<Color>,
    pub focus_color: Option<Color>,
    pub hover_color: Option<Color>,
    pub highlight_color: Option<Color>,
    pub splash_color: Option<Color>,
    pub color_scheme: Option<ColorScheme>,
    pub material_tap_target_size: Option<MaterialTapTargetSize>,
}

impl Default for ButtonThemeData {
    fn default() -> ButtonThemeData {
        ButtonThemeData::new()
    }
}

impl ButtonThemeData {
    /// Upstream's defaults: `minWidth: 88`, `height: 36`.
    pub fn new() -> ButtonThemeData {
        ButtonThemeData {
            min_width: 88.0,
            height: 36.0,
            text_theme: ButtonTextTheme::Normal,
            layout_behavior: ButtonBarLayoutBehavior::Padded,
            padding: None,
            shape: None,
            aligned_dropdown: false,
            button_color: None,
            disabled_color: None,
            focus_color: None,
            hover_color: None,
            highlight_color: None,
            splash_color: None,
            color_scheme: None,
            material_tap_target_size: None,
        }
    }

    pub fn with_min_width(mut self, min_width: f32) -> Self {
        self.min_width = min_width;
        self
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn with_text_theme(mut self, text_theme: ButtonTextTheme) -> Self {
        self.text_theme = text_theme;
        self
    }

    /// Upstream `ButtonThemeData.padding`, whose fallback depends on the text
    /// theme: a primary button is padded wider than a plain one.
    pub fn padding(&self) -> EdgeInsets {
        if let Some(padding) = self.padding {
            return padding.resolve(crate::direction::current_direction());
        }
        match self.text_theme {
            ButtonTextTheme::Normal | ButtonTextTheme::Accent => EdgeInsets::symmetric(16.0, 0.0),
            ButtonTextTheme::Primary => EdgeInsets::symmetric(24.0, 0.0),
        }
    }
}

/// Upstream `ButtonTheme`.
pub struct ButtonTheme;

impl ButtonTheme {
    pub fn new(data: ButtonThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> ButtonThemeData {
        context
            .inherited::<ButtonThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).button_theme.clone())
    }
}

// -- Scrollbar (upstream `scrollbar_theme.dart`) ------------------------------

/// Upstream `ScrollbarThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScrollbarThemeData {
    /// Whether the thumb is drawn at all -- by state, because a scrollbar
    /// that appears while dragging and fades otherwise is the usual thing.
    pub thumb_visibility: Option<StateProperty<Option<bool>>>,
    pub thickness: Option<StateProperty<Option<f32>>>,
    pub track_visibility: Option<StateProperty<Option<bool>>>,
    /// Whether the thumb can be dragged, rather than only shown.
    pub interactive: Option<bool>,
    pub radius: Option<crate::borders::Radius>,
    pub thumb_color: Option<StateProperty<Option<Color>>>,
    pub track_color: Option<StateProperty<Option<Color>>>,
    pub track_border_color: Option<StateProperty<Option<Color>>>,
    /// The gap between the bar and the edge of the viewport.
    pub cross_axis_margin: Option<f32>,
    /// The gap at each end of the track.
    pub main_axis_margin: Option<f32>,
    /// How short the thumb may get on a very long list.
    pub min_thumb_length: Option<f32>,
}

impl ScrollbarThemeData {
    pub fn new() -> ScrollbarThemeData {
        ScrollbarThemeData::default()
    }

    pub fn with_thickness(mut self, thickness: StateProperty<Option<f32>>) -> Self {
        self.thickness = Some(thickness);
        self
    }

    pub fn with_thumb_color(mut self, color: StateProperty<Option<Color>>) -> Self {
        self.thumb_color = Some(color);
        self
    }

    pub fn with_radius(mut self, radius: crate::borders::Radius) -> Self {
        self.radius = Some(radius);
        self
    }

    pub fn with_margins(mut self, cross_axis: f32, main_axis: f32) -> Self {
        self.cross_axis_margin = Some(cross_axis);
        self.main_axis_margin = Some(main_axis);
        self
    }

    pub fn with_min_thumb_length(mut self, length: f32) -> Self {
        self.min_thumb_length = Some(length);
        self
    }

    /// Upstream `ScrollbarThemeData.lerp`.
    pub fn lerp(a: &ScrollbarThemeData, b: &ScrollbarThemeData, t: f32) -> ScrollbarThemeData {
        ScrollbarThemeData {
            thumb_visibility: lerp_nearer(&a.thumb_visibility, &b.thumb_visibility, t),
            thickness: lerp_nearer(&a.thickness, &b.thickness, t),
            track_visibility: lerp_nearer(&a.track_visibility, &b.track_visibility, t),
            interactive: lerp_nearer(&a.interactive, &b.interactive, t),
            radius: lerp_nearer(&a.radius, &b.radius, t),
            thumb_color: lerp_state_color(a.thumb_color.as_ref(), b.thumb_color.as_ref(), t),
            track_color: lerp_state_color(a.track_color.as_ref(), b.track_color.as_ref(), t),
            track_border_color: lerp_state_color(
                a.track_border_color.as_ref(),
                b.track_border_color.as_ref(),
                t,
            ),
            cross_axis_margin: lerp_f32(a.cross_axis_margin, b.cross_axis_margin, t),
            main_axis_margin: lerp_f32(a.main_axis_margin, b.main_axis_margin, t),
            min_thumb_length: lerp_f32(a.min_thumb_length, b.min_thumb_length, t),
        }
    }
}

/// Upstream `ScrollbarTheme`.
pub struct ScrollbarTheme;

impl ScrollbarTheme {
    pub fn new(data: ScrollbarThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> ScrollbarThemeData {
        context
            .inherited::<ScrollbarThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).scrollbar_theme.clone())
    }
}

/// What a scrollbar draws with, once the three steps have run.
pub struct ResolvedScrollbar {
    pub thickness: f32,
    pub thumb_color: Color,
    pub radius: crate::borders::Radius,
    pub cross_axis_margin: f32,
    pub main_axis_margin: f32,
    pub min_thumb_length: f32,
    pub interactive: bool,
}

impl ResolvedScrollbar {
    /// Upstream's `_kScrollbarThickness`.
    pub const THICKNESS: f32 = 8.0;
    /// Upstream's `_kScrollbarMinLength`.
    pub const MIN_THUMB_LENGTH: f32 = 48.0;

    pub fn of(context: &mut BuildContext, states: WidgetStates) -> ResolvedScrollbar {
        let data = ScrollbarTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        ResolvedScrollbar {
            thickness: data
                .thickness
                .as_ref()
                .and_then(|property| property.resolve(states))
                .unwrap_or(ResolvedScrollbar::THICKNESS),
            thumb_color: data
                .thumb_color
                .as_ref()
                .and_then(|property| property.resolve(states))
                // Upstream's Material 3 default: the outline at three
                // quarters when dragged, and rather fainter otherwise.
                .unwrap_or(if states.contains(WidgetState::Dragged) {
                    scheme.outline()
                } else {
                    scheme.outline().with_alpha(0x4d)
                }),
            radius: data.radius.unwrap_or(crate::borders::Radius::circular(
                ResolvedScrollbar::THICKNESS / 2.0,
            )),
            cross_axis_margin: data.cross_axis_margin.unwrap_or(0.0),
            main_axis_margin: data.main_axis_margin.unwrap_or(0.0),
            min_thumb_length: data
                .min_thumb_length
                .unwrap_or(ResolvedScrollbar::MIN_THUMB_LENGTH),
            interactive: data.interactive.unwrap_or(true),
        }
    }
}

// -- Menus (upstream `menu_style.dart`, `menu_theme.dart` and friends) --------

/// Upstream `MenuStyle`: what a menu panel is told, state by state.
///
/// The same shape as [`ButtonStyle`] and for the same reason -- a menu is a
/// surface that can be hovered and focused -- with the fields a panel has
/// rather than the ones a label has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuStyle {
    pub background_color: Option<StateProperty<Option<Color>>>,
    pub shadow_color: Option<StateProperty<Option<Color>>>,
    pub surface_tint_color: Option<StateProperty<Option<Color>>>,
    pub elevation: Option<StateProperty<Option<f32>>>,
    pub padding: Option<StateProperty<Option<EdgeInsetsGeometry>>>,
    pub minimum_size: Option<StateProperty<Option<Size>>>,
    pub fixed_size: Option<StateProperty<Option<Size>>>,
    pub maximum_size: Option<StateProperty<Option<Size>>>,
    pub side: Option<StateProperty<Option<BorderSide>>>,
    pub shape: Option<StateProperty<Option<ShapeBorder>>>,
    pub mouse_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
    pub visual_density: Option<VisualDensity>,
    pub alignment: Option<AlignmentGeometry>,
}

impl MenuStyle {
    pub fn new() -> MenuStyle {
        MenuStyle::default()
    }

    pub fn with_background_color(mut self, color: StateProperty<Option<Color>>) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_padding(mut self, padding: StateProperty<Option<EdgeInsetsGeometry>>) -> Self {
        self.padding = Some(padding);
        self
    }

    pub fn with_alignment(mut self, alignment: AlignmentGeometry) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Upstream `MenuStyle.lerp`.
    pub fn lerp(a: &MenuStyle, b: &MenuStyle, t: f32) -> MenuStyle {
        MenuStyle {
            background_color: lerp_state_color(
                a.background_color.as_ref(),
                b.background_color.as_ref(),
                t,
            ),
            shadow_color: lerp_state_color(a.shadow_color.as_ref(), b.shadow_color.as_ref(), t),
            surface_tint_color: lerp_state_color(
                a.surface_tint_color.as_ref(),
                b.surface_tint_color.as_ref(),
                t,
            ),
            elevation: lerp_nearer(&a.elevation, &b.elevation, t),
            padding: lerp_nearer(&a.padding, &b.padding, t),
            minimum_size: lerp_nearer(&a.minimum_size, &b.minimum_size, t),
            fixed_size: lerp_nearer(&a.fixed_size, &b.fixed_size, t),
            maximum_size: lerp_nearer(&a.maximum_size, &b.maximum_size, t),
            side: lerp_nearer(&a.side, &b.side, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
            visual_density: match (a.visual_density, b.visual_density) {
                (Some(first), Some(second)) => Some(VisualDensity::lerp(first, second, t)),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
            alignment: lerp_nearer(&a.alignment, &b.alignment, t),
        }
    }
}

/// Two optional menu styles interpolated.
fn lerp_menu_style(a: &Option<MenuStyle>, b: &Option<MenuStyle>, t: f32) -> Option<MenuStyle> {
    match (a, b) {
        (Some(first), Some(second)) => Some(MenuStyle::lerp(first, second, t)),
        (first, second) => {
            if t < 0.5 {
                first.clone()
            } else {
                second.clone()
            }
        }
    }
}

/// Upstream `MenuThemeData`.
///
/// `submenuIcon` is not here: it is a `WidgetStateProperty<Widget?>`, a
/// widget carried in a theme, which is a shape this port has no place for
/// yet (the same reason `ButtonStyle`'s two builders are absent).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuThemeData {
    pub style: Option<MenuStyle>,
}

impl MenuThemeData {
    pub fn new() -> MenuThemeData {
        MenuThemeData::default()
    }

    pub fn with_style(mut self, style: MenuStyle) -> MenuThemeData {
        self.style = Some(style);
        self
    }

    /// Upstream `MenuThemeData.lerp`.
    pub fn lerp(a: &MenuThemeData, b: &MenuThemeData, t: f32) -> MenuThemeData {
        MenuThemeData {
            style: lerp_menu_style(&a.style, &b.style, t),
        }
    }
}

/// Upstream `MenuTheme`.
pub struct MenuTheme;

impl MenuTheme {
    pub fn new(data: MenuThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> MenuThemeData {
        context
            .inherited::<MenuThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).menu_theme.clone())
    }
}

/// Upstream `MenuBarThemeData`, which is a `MenuThemeData` under another
/// name -- upstream declares it as a subclass with no fields of its own, so
/// that a menu bar and the menus inside it can be themed apart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuBarThemeData {
    pub style: Option<MenuStyle>,
}

impl MenuBarThemeData {
    pub fn new() -> MenuBarThemeData {
        MenuBarThemeData::default()
    }

    pub fn with_style(mut self, style: MenuStyle) -> MenuBarThemeData {
        self.style = Some(style);
        self
    }

    /// Upstream `MenuBarThemeData.lerp`.
    pub fn lerp(a: &MenuBarThemeData, b: &MenuBarThemeData, t: f32) -> MenuBarThemeData {
        MenuBarThemeData {
            style: lerp_menu_style(&a.style, &b.style, t),
        }
    }
}

/// Upstream `MenuBarTheme`.
pub struct MenuBarTheme;

impl MenuBarTheme {
    pub fn new(data: MenuBarThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> MenuBarThemeData {
        context
            .inherited::<MenuBarThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).menu_bar_theme.clone())
    }
}

/// Upstream `MenuButtonThemeData`: the style of the entries in a menu.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuButtonThemeData {
    pub style: Option<ButtonStyle>,
}

impl MenuButtonThemeData {
    pub fn new() -> MenuButtonThemeData {
        MenuButtonThemeData::default()
    }

    pub fn with_style(mut self, style: ButtonStyle) -> MenuButtonThemeData {
        self.style = Some(style);
        self
    }

    /// Upstream `MenuButtonThemeData.lerp`.
    pub fn lerp(a: &MenuButtonThemeData, b: &MenuButtonThemeData, t: f32) -> MenuButtonThemeData {
        MenuButtonThemeData {
            style: lerp_button_style(&a.style, &b.style, t),
        }
    }
}

/// Upstream `MenuButtonTheme`.
pub struct MenuButtonTheme;

impl MenuButtonTheme {
    pub fn new(data: MenuButtonThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> MenuButtonThemeData {
        context
            .inherited::<MenuButtonThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).menu_button_theme.clone())
    }
}

/// Upstream `SegmentedButtonThemeData`.
///
/// `selectedIcon` is not here, for the reason `MenuThemeData.submenuIcon` is
/// not: it is a widget carried in a theme.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SegmentedButtonThemeData {
    pub style: Option<ButtonStyle>,
}

impl SegmentedButtonThemeData {
    pub fn new() -> SegmentedButtonThemeData {
        SegmentedButtonThemeData::default()
    }

    pub fn with_style(mut self, style: ButtonStyle) -> SegmentedButtonThemeData {
        self.style = Some(style);
        self
    }

    /// Upstream `SegmentedButtonThemeData.lerp`.
    pub fn lerp(
        a: &SegmentedButtonThemeData,
        b: &SegmentedButtonThemeData,
        t: f32,
    ) -> SegmentedButtonThemeData {
        SegmentedButtonThemeData {
            style: lerp_button_style(&a.style, &b.style, t),
        }
    }
}

/// Upstream `SegmentedButtonTheme`.
pub struct SegmentedButtonTheme;

impl SegmentedButtonTheme {
    pub fn new(data: SegmentedButtonThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> SegmentedButtonThemeData {
        context
            .inherited::<SegmentedButtonThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).segmented_button_theme.clone())
    }
}

// -- Floating action button (upstream `floating_action_button_theme.dart`) ----

/// Upstream `FloatingActionButtonThemeData`.
///
/// The five elevations are five fields rather than one state property:
/// upstream predates `WidgetStateProperty` here and has not moved this class
/// over, and a port that "modernised" it would be answering a question
/// upstream has not answered.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FloatingActionButtonThemeData {
    pub foreground_color: Option<Color>,
    pub background_color: Option<Color>,
    pub focus_color: Option<Color>,
    pub hover_color: Option<Color>,
    pub splash_color: Option<Color>,
    pub elevation: Option<f32>,
    pub focus_elevation: Option<f32>,
    pub hover_elevation: Option<f32>,
    pub disabled_elevation: Option<f32>,
    /// The elevation while it is held down.
    pub highlight_elevation: Option<f32>,
    pub shape: Option<ShapeBorder>,
    pub enable_feedback: Option<bool>,
    pub icon_size: Option<f32>,
    /// The box a plain button is laid out in.
    pub size_constraints: Option<BoxConstraints>,
    pub small_size_constraints: Option<BoxConstraints>,
    pub large_size_constraints: Option<BoxConstraints>,
    /// The box an extended button -- one with a label beside its icon -- is
    /// laid out in.
    pub extended_size_constraints: Option<BoxConstraints>,
    pub extended_icon_label_spacing: Option<f32>,
    pub extended_padding: Option<EdgeInsetsGeometry>,
    pub extended_text_style: Option<TextStyle>,
    pub mouse_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
}

impl FloatingActionButtonThemeData {
    pub fn new() -> FloatingActionButtonThemeData {
        FloatingActionButtonThemeData::default()
    }

    pub fn with_colors(mut self, background: Color, foreground: Color) -> Self {
        self.background_color = Some(background);
        self.foreground_color = Some(foreground);
        self
    }

    pub fn with_elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }

    pub fn with_shape(mut self, shape: ShapeBorder) -> Self {
        self.shape = Some(shape);
        self
    }

    pub fn with_size_constraints(mut self, constraints: BoxConstraints) -> Self {
        self.size_constraints = Some(constraints);
        self
    }

    /// Upstream `FloatingActionButtonThemeData.lerp`.
    pub fn lerp(
        a: &FloatingActionButtonThemeData,
        b: &FloatingActionButtonThemeData,
        t: f32,
    ) -> FloatingActionButtonThemeData {
        FloatingActionButtonThemeData {
            foreground_color: lerp_color(a.foreground_color, b.foreground_color, t),
            background_color: lerp_color(a.background_color, b.background_color, t),
            focus_color: lerp_color(a.focus_color, b.focus_color, t),
            hover_color: lerp_color(a.hover_color, b.hover_color, t),
            splash_color: lerp_color(a.splash_color, b.splash_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            focus_elevation: lerp_f32(a.focus_elevation, b.focus_elevation, t),
            hover_elevation: lerp_f32(a.hover_elevation, b.hover_elevation, t),
            disabled_elevation: lerp_f32(a.disabled_elevation, b.disabled_elevation, t),
            highlight_elevation: lerp_f32(a.highlight_elevation, b.highlight_elevation, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            enable_feedback: lerp_nearer(&a.enable_feedback, &b.enable_feedback, t),
            icon_size: lerp_f32(a.icon_size, b.icon_size, t),
            size_constraints: lerp_nearer(&a.size_constraints, &b.size_constraints, t),
            small_size_constraints: lerp_nearer(
                &a.small_size_constraints,
                &b.small_size_constraints,
                t,
            ),
            large_size_constraints: lerp_nearer(
                &a.large_size_constraints,
                &b.large_size_constraints,
                t,
            ),
            extended_size_constraints: lerp_nearer(
                &a.extended_size_constraints,
                &b.extended_size_constraints,
                t,
            ),
            extended_icon_label_spacing: lerp_f32(
                a.extended_icon_label_spacing,
                b.extended_icon_label_spacing,
                t,
            ),
            extended_padding: lerp_nearer(&a.extended_padding, &b.extended_padding, t),
            extended_text_style: lerp_nearer(&a.extended_text_style, &b.extended_text_style, t),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
        }
    }
}

/// Upstream `FloatingActionButtonTheme`.
pub struct FloatingActionButtonTheme;

impl FloatingActionButtonTheme {
    pub fn new(data: FloatingActionButtonThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> FloatingActionButtonThemeData {
        context
            .inherited::<FloatingActionButtonThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).floating_action_button_theme.clone())
    }
}

/// What a floating action button draws with, once the three steps have run.
pub struct ResolvedFloatingActionButton {
    pub background: Color,
    pub foreground: Color,
    /// The elevation for the states it is in -- upstream picks one of the
    /// five fields rather than blending them.
    pub elevation: f32,
    pub size: BoxConstraints,
}

impl ResolvedFloatingActionButton {
    /// Upstream's `_defaultElevation`.
    pub const ELEVATION: f32 = 6.0;
    /// Upstream's `_defaultHighlightElevation`.
    pub const HIGHLIGHT_ELEVATION: f32 = 12.0;
    /// Upstream's `_kSizeConstraints`.
    pub const SIZE: f32 = 56.0;

    pub fn of(context: &mut BuildContext, states: WidgetStates) -> ResolvedFloatingActionButton {
        let data = FloatingActionButtonTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        // Upstream picks by state, in this order: disabled, then held, then
        // hovered, then focused, then the resting elevation.
        let elevation = if states.contains(WidgetState::Disabled) {
            data.disabled_elevation.or(data.elevation)
        } else if states.contains(WidgetState::Pressed) {
            data.highlight_elevation
                .or(Some(ResolvedFloatingActionButton::HIGHLIGHT_ELEVATION))
        } else if states.contains(WidgetState::Hovered) {
            data.hover_elevation.or(data.elevation)
        } else if states.contains(WidgetState::Focused) {
            data.focus_elevation.or(data.elevation)
        } else {
            data.elevation
        };
        ResolvedFloatingActionButton {
            background: data.background_color.unwrap_or(scheme.primary_container()),
            foreground: data
                .foreground_color
                .unwrap_or(scheme.on_primary_container()),
            elevation: elevation.unwrap_or(ResolvedFloatingActionButton::ELEVATION),
            size: data.size_constraints.unwrap_or(BoxConstraints {
                min_width: ResolvedFloatingActionButton::SIZE,
                max_width: ResolvedFloatingActionButton::SIZE,
                min_height: ResolvedFloatingActionButton::SIZE,
                max_height: ResolvedFloatingActionButton::SIZE,
            }),
        }
    }
}

// -- Toggle buttons (upstream `toggle_buttons_theme.dart`) --------------------

/// Upstream `ToggleButtonsThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToggleButtonsThemeData {
    pub text_style: Option<TextStyle>,
    pub constraints: Option<BoxConstraints>,
    /// The label's colour when the button is neither selected nor disabled.
    pub color: Option<Color>,
    pub selected_color: Option<Color>,
    pub disabled_color: Option<Color>,
    /// What a selected button is filled with.
    pub fill_color: Option<Color>,
    pub focus_color: Option<Color>,
    pub highlight_color: Option<Color>,
    pub splash_color: Option<Color>,
    pub hover_color: Option<Color>,
    pub border_color: Option<Color>,
    pub selected_border_color: Option<Color>,
    pub disabled_border_color: Option<Color>,
    pub border_width: Option<f32>,
    pub border_radius: Option<crate::borders::BorderRadius>,
}

impl ToggleButtonsThemeData {
    pub fn new() -> ToggleButtonsThemeData {
        ToggleButtonsThemeData::default()
    }

    pub fn with_colors(mut self, plain: Color, selected: Color) -> Self {
        self.color = Some(plain);
        self.selected_color = Some(selected);
        self
    }

    pub fn with_fill_color(mut self, color: Color) -> Self {
        self.fill_color = Some(color);
        self
    }

    pub fn with_border(mut self, color: Color, width: f32) -> Self {
        self.border_color = Some(color);
        self.border_width = Some(width);
        self
    }

    pub fn with_border_radius(mut self, radius: crate::borders::BorderRadius) -> Self {
        self.border_radius = Some(radius);
        self
    }

    /// Upstream `ToggleButtonsThemeData.lerp`.
    pub fn lerp(
        a: &ToggleButtonsThemeData,
        b: &ToggleButtonsThemeData,
        t: f32,
    ) -> ToggleButtonsThemeData {
        ToggleButtonsThemeData {
            text_style: lerp_nearer(&a.text_style, &b.text_style, t),
            constraints: lerp_nearer(&a.constraints, &b.constraints, t),
            color: lerp_color(a.color, b.color, t),
            selected_color: lerp_color(a.selected_color, b.selected_color, t),
            disabled_color: lerp_color(a.disabled_color, b.disabled_color, t),
            fill_color: lerp_color(a.fill_color, b.fill_color, t),
            focus_color: lerp_color(a.focus_color, b.focus_color, t),
            highlight_color: lerp_color(a.highlight_color, b.highlight_color, t),
            splash_color: lerp_color(a.splash_color, b.splash_color, t),
            hover_color: lerp_color(a.hover_color, b.hover_color, t),
            border_color: lerp_color(a.border_color, b.border_color, t),
            selected_border_color: lerp_color(a.selected_border_color, b.selected_border_color, t),
            disabled_border_color: lerp_color(a.disabled_border_color, b.disabled_border_color, t),
            border_width: lerp_f32(a.border_width, b.border_width, t),
            border_radius: lerp_nearer(&a.border_radius, &b.border_radius, t),
        }
    }
}

/// Upstream `ToggleButtonsTheme`.
pub struct ToggleButtonsTheme;

impl ToggleButtonsTheme {
    pub fn new(data: ToggleButtonsThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> ToggleButtonsThemeData {
        context
            .inherited::<ToggleButtonsThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).toggle_buttons_theme.clone())
    }
}

// -- Search (upstream `search_bar_theme.dart`, `search_view_theme.dart`) ------

/// Upstream `TextCapitalization` (`services/text_input.dart`): what the
/// keyboard capitalises for a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextCapitalization {
    /// The first letter of every word.
    Words,
    /// The first letter of every sentence.
    Sentences,
    /// Every letter.
    Characters,
    /// Nothing.
    #[default]
    None,
}

/// Upstream `SearchBarThemeData`: the resting bar.
///
/// Every field here is a state property, which is the difference between
/// this and [`SearchViewThemeData`] below -- the bar is a control a pointer
/// touches, and the view it opens into is a surface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchBarThemeData {
    pub elevation: Option<StateProperty<Option<f32>>>,
    pub background_color: Option<StateProperty<Option<Color>>>,
    pub shadow_color: Option<StateProperty<Option<Color>>>,
    pub surface_tint_color: Option<StateProperty<Option<Color>>>,
    pub overlay_color: Option<StateProperty<Option<Color>>>,
    pub side: Option<StateProperty<Option<BorderSide>>>,
    pub shape: Option<StateProperty<Option<ShapeBorder>>>,
    pub padding: Option<StateProperty<Option<EdgeInsetsGeometry>>>,
    pub text_style: Option<StateProperty<Option<TextStyle>>>,
    pub hint_style: Option<StateProperty<Option<TextStyle>>>,
    pub constraints: Option<BoxConstraints>,
    pub text_capitalization: Option<TextCapitalization>,
}

impl SearchBarThemeData {
    pub fn new() -> SearchBarThemeData {
        SearchBarThemeData::default()
    }

    pub fn with_background_color(mut self, color: StateProperty<Option<Color>>) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_elevation(mut self, elevation: StateProperty<Option<f32>>) -> Self {
        self.elevation = Some(elevation);
        self
    }

    pub fn with_constraints(mut self, constraints: BoxConstraints) -> Self {
        self.constraints = Some(constraints);
        self
    }

    /// Upstream `SearchBarThemeData.lerp`.
    pub fn lerp(a: &SearchBarThemeData, b: &SearchBarThemeData, t: f32) -> SearchBarThemeData {
        SearchBarThemeData {
            elevation: lerp_nearer(&a.elevation, &b.elevation, t),
            background_color: lerp_state_color(
                a.background_color.as_ref(),
                b.background_color.as_ref(),
                t,
            ),
            shadow_color: lerp_state_color(a.shadow_color.as_ref(), b.shadow_color.as_ref(), t),
            surface_tint_color: lerp_state_color(
                a.surface_tint_color.as_ref(),
                b.surface_tint_color.as_ref(),
                t,
            ),
            overlay_color: lerp_state_color(a.overlay_color.as_ref(), b.overlay_color.as_ref(), t),
            side: lerp_nearer(&a.side, &b.side, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            padding: lerp_nearer(&a.padding, &b.padding, t),
            text_style: lerp_nearer(&a.text_style, &b.text_style, t),
            hint_style: lerp_nearer(&a.hint_style, &b.hint_style, t),
            constraints: lerp_nearer(&a.constraints, &b.constraints, t),
            text_capitalization: lerp_nearer(&a.text_capitalization, &b.text_capitalization, t),
        }
    }
}

/// Upstream `SearchBarTheme`.
pub struct SearchBarTheme;

impl SearchBarTheme {
    pub fn new(data: SearchBarThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> SearchBarThemeData {
        context
            .inherited::<SearchBarThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).search_bar_theme.clone())
    }
}

/// Upstream `SearchViewThemeData`: the panel a search bar opens into.
///
/// Plain fields rather than state properties, because a view is not a thing
/// with states -- it is open or it is not there.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchViewThemeData {
    pub background_color: Option<Color>,
    pub elevation: Option<f32>,
    pub surface_tint_color: Option<Color>,
    pub side: Option<BorderSide>,
    pub shape: Option<ShapeBorder>,
    pub header_height: Option<f32>,
    pub header_text_style: Option<TextStyle>,
    pub header_hint_style: Option<TextStyle>,
    pub constraints: Option<BoxConstraints>,
    pub padding: Option<EdgeInsetsGeometry>,
    /// The padding of the bar inside the view's header, which is a different
    /// bar from the one that opened it.
    pub bar_padding: Option<EdgeInsetsGeometry>,
    /// Whether the view is as tall as its results rather than as tall as it
    /// is allowed to be.
    pub shrink_wrap: Option<bool>,
    pub divider_color: Option<Color>,
}

impl SearchViewThemeData {
    pub fn new() -> SearchViewThemeData {
        SearchViewThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_header_height(mut self, height: f32) -> Self {
        self.header_height = Some(height);
        self
    }

    pub fn with_shrink_wrap(mut self, shrink_wrap: bool) -> Self {
        self.shrink_wrap = Some(shrink_wrap);
        self
    }

    /// Upstream `SearchViewThemeData.lerp`.
    pub fn lerp(a: &SearchViewThemeData, b: &SearchViewThemeData, t: f32) -> SearchViewThemeData {
        SearchViewThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            side: match (a.side, b.side) {
                (Some(first), Some(second)) => Some(BorderSide::lerp(first, second, t)),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
            shape: lerp_nearer(&a.shape, &b.shape, t),
            header_height: lerp_f32(a.header_height, b.header_height, t),
            header_text_style: lerp_nearer(&a.header_text_style, &b.header_text_style, t),
            header_hint_style: lerp_nearer(&a.header_hint_style, &b.header_hint_style, t),
            constraints: lerp_nearer(&a.constraints, &b.constraints, t),
            padding: lerp_nearer(&a.padding, &b.padding, t),
            bar_padding: lerp_nearer(&a.bar_padding, &b.bar_padding, t),
            shrink_wrap: lerp_nearer(&a.shrink_wrap, &b.shrink_wrap, t),
            divider_color: lerp_color(a.divider_color, b.divider_color, t),
        }
    }
}

/// Upstream `SearchViewTheme`.
pub struct SearchViewTheme;

impl SearchViewTheme {
    pub fn new(data: SearchViewThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> SearchViewThemeData {
        context
            .inherited::<SearchViewThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).search_view_theme.clone())
    }
}

// -- Time picker (upstream `time_picker_theme.dart`) --------------------------

/// Upstream `TimePickerThemeData`.
///
/// The names read oddly out of context and are upstream's: a time picker has
/// a *dial* with a hand on it, an *hour-minute* pair of fields above it, and
/// a *day period* toggle for AM and PM, and each of those three is themed
/// separately because they are three different things that happen to sit in
/// one dialog.
///
/// `inputDecorationTheme` is not here: it is an `InputDecorationThemeData`,
/// which arrives with the text field cluster.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimePickerThemeData {
    pub background_color: Option<Color>,
    pub cancel_button_style: Option<ButtonStyle>,
    pub confirm_button_style: Option<ButtonStyle>,
    pub day_period_border_side: Option<BorderSide>,
    /// The AM/PM toggle's fill. Upstream keeps it private behind a getter
    /// because a null there means "the scheme's", not "transparent".
    pub day_period_color: Option<Color>,
    pub day_period_shape: Option<ShapeBorder>,
    pub day_period_text_color: Option<Color>,
    pub day_period_text_style: Option<TextStyle>,
    pub dial_background_color: Option<Color>,
    pub dial_hand_color: Option<Color>,
    pub dial_text_color: Option<Color>,
    pub dial_text_style: Option<TextStyle>,
    pub elevation: Option<f32>,
    pub entry_mode_icon_color: Option<Color>,
    pub help_text_style: Option<TextStyle>,
    pub hour_minute_color: Option<Color>,
    pub hour_minute_shape: Option<ShapeBorder>,
    pub hour_minute_text_color: Option<Color>,
    pub hour_minute_text_style: Option<TextStyle>,
    pub padding: Option<EdgeInsetsGeometry>,
    pub shape: Option<ShapeBorder>,
    /// The colon between the hour and the minute, by state.
    pub time_selector_separator_color: Option<StateProperty<Option<Color>>>,
    pub time_selector_separator_text_style: Option<StateProperty<Option<TextStyle>>>,
}

impl TimePickerThemeData {
    pub fn new() -> TimePickerThemeData {
        TimePickerThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_dial_colors(mut self, background: Color, hand: Color) -> Self {
        self.dial_background_color = Some(background);
        self.dial_hand_color = Some(hand);
        self
    }

    pub fn with_hour_minute_color(mut self, color: Color) -> Self {
        self.hour_minute_color = Some(color);
        self
    }

    pub fn with_elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }

    /// Upstream `TimePickerThemeData.lerp`.
    pub fn lerp(a: &TimePickerThemeData, b: &TimePickerThemeData, t: f32) -> TimePickerThemeData {
        TimePickerThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            cancel_button_style: lerp_button_style(
                &a.cancel_button_style,
                &b.cancel_button_style,
                t,
            ),
            confirm_button_style: lerp_button_style(
                &a.confirm_button_style,
                &b.confirm_button_style,
                t,
            ),
            day_period_border_side: match (a.day_period_border_side, b.day_period_border_side) {
                (Some(first), Some(second)) => Some(BorderSide::lerp(first, second, t)),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
            day_period_color: lerp_color(a.day_period_color, b.day_period_color, t),
            day_period_shape: lerp_nearer(&a.day_period_shape, &b.day_period_shape, t),
            day_period_text_color: lerp_color(a.day_period_text_color, b.day_period_text_color, t),
            day_period_text_style: lerp_nearer(
                &a.day_period_text_style,
                &b.day_period_text_style,
                t,
            ),
            dial_background_color: lerp_color(a.dial_background_color, b.dial_background_color, t),
            dial_hand_color: lerp_color(a.dial_hand_color, b.dial_hand_color, t),
            dial_text_color: lerp_color(a.dial_text_color, b.dial_text_color, t),
            dial_text_style: lerp_nearer(&a.dial_text_style, &b.dial_text_style, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            entry_mode_icon_color: lerp_color(a.entry_mode_icon_color, b.entry_mode_icon_color, t),
            help_text_style: lerp_nearer(&a.help_text_style, &b.help_text_style, t),
            hour_minute_color: lerp_color(a.hour_minute_color, b.hour_minute_color, t),
            hour_minute_shape: lerp_nearer(&a.hour_minute_shape, &b.hour_minute_shape, t),
            hour_minute_text_color: lerp_color(
                a.hour_minute_text_color,
                b.hour_minute_text_color,
                t,
            ),
            hour_minute_text_style: lerp_nearer(
                &a.hour_minute_text_style,
                &b.hour_minute_text_style,
                t,
            ),
            padding: lerp_nearer(&a.padding, &b.padding, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            time_selector_separator_color: lerp_state_color(
                a.time_selector_separator_color.as_ref(),
                b.time_selector_separator_color.as_ref(),
                t,
            ),
            time_selector_separator_text_style: lerp_nearer(
                &a.time_selector_separator_text_style,
                &b.time_selector_separator_text_style,
                t,
            ),
        }
    }
}

/// Upstream `TimePickerTheme`.
pub struct TimePickerTheme;

impl TimePickerTheme {
    pub fn new(data: TimePickerThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> TimePickerThemeData {
        context
            .inherited::<TimePickerThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).time_picker_theme.clone())
    }
}

// -- Date picker (upstream `date_picker_theme.dart`) --------------------------

/// Upstream `DatePickerThemeData`.
///
/// The longest of the component themes, and it is long for a reason: a date
/// picker is four surfaces, not one. There is the dialog, the grid of days
/// inside it, the grid of years behind that, and the *range* picker, which
/// upstream themes with its own copy of every dialog field (`rangePicker*`)
/// because a range picker is a full-screen page rather than a dialog and does
/// not want the dialog's paint.
///
/// `inputDecorationTheme` and `locale` are not here: the first arrives with
/// the text field cluster, and the second with localisation (`E4`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DatePickerThemeData {
    pub background_color: Option<Color>,
    pub elevation: Option<f32>,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub shape: Option<ShapeBorder>,

    pub header_background_color: Option<Color>,
    pub header_foreground_color: Option<Color>,
    pub header_headline_style: Option<TextStyle>,
    pub header_help_style: Option<TextStyle>,

    /// The row of weekday initials above the grid.
    pub weekday_style: Option<TextStyle>,
    pub day_style: Option<TextStyle>,
    pub day_foreground_color: Option<StateProperty<Option<Color>>>,
    pub day_background_color: Option<StateProperty<Option<Color>>>,
    pub day_overlay_color: Option<StateProperty<Option<Color>>>,
    pub day_shape: Option<StateProperty<Option<ShapeBorder>>>,

    pub today_foreground_color: Option<StateProperty<Option<Color>>>,
    pub today_background_color: Option<StateProperty<Option<Color>>>,
    pub today_border: Option<BorderSide>,

    pub year_style: Option<TextStyle>,
    pub year_foreground_color: Option<StateProperty<Option<Color>>>,
    pub year_background_color: Option<StateProperty<Option<Color>>>,
    pub year_overlay_color: Option<StateProperty<Option<Color>>>,
    pub year_shape: Option<StateProperty<Option<ShapeBorder>>>,

    pub range_picker_background_color: Option<Color>,
    pub range_picker_elevation: Option<f32>,
    pub range_picker_shadow_color: Option<Color>,
    pub range_picker_surface_tint_color: Option<Color>,
    pub range_picker_shape: Option<ShapeBorder>,
    pub range_picker_header_background_color: Option<Color>,
    pub range_picker_header_foreground_color: Option<Color>,
    pub range_picker_header_headline_style: Option<TextStyle>,
    pub range_picker_header_help_style: Option<TextStyle>,
    /// What the days between the two ends of a range are washed with.
    pub range_selection_background_color: Option<Color>,
    pub range_selection_overlay_color: Option<StateProperty<Option<Color>>>,

    pub divider_color: Option<Color>,
    pub cancel_button_style: Option<ButtonStyle>,
    pub confirm_button_style: Option<ButtonStyle>,
    /// The button that swaps the calendar for the text field.
    pub toggle_button_text_style: Option<TextStyle>,
    pub sub_header_foreground_color: Option<Color>,
}

impl DatePickerThemeData {
    pub fn new() -> DatePickerThemeData {
        DatePickerThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_header_colors(mut self, background: Color, foreground: Color) -> Self {
        self.header_background_color = Some(background);
        self.header_foreground_color = Some(foreground);
        self
    }

    pub fn with_day_foreground_color(mut self, color: StateProperty<Option<Color>>) -> Self {
        self.day_foreground_color = Some(color);
        self
    }

    pub fn with_range_selection_background_color(mut self, color: Color) -> Self {
        self.range_selection_background_color = Some(color);
        self
    }

    pub fn with_today_border(mut self, border: BorderSide) -> Self {
        self.today_border = Some(border);
        self
    }

    /// Upstream `DatePickerThemeData.lerp`.
    pub fn lerp(a: &DatePickerThemeData, b: &DatePickerThemeData, t: f32) -> DatePickerThemeData {
        let side = |first: Option<BorderSide>, second: Option<BorderSide>| match (first, second) {
            (Some(first), Some(second)) => Some(BorderSide::lerp(first, second, t)),
            (first, second) => {
                if t < 0.5 {
                    first
                } else {
                    second
                }
            }
        };
        DatePickerThemeData {
            background_color: lerp_color(a.background_color, b.background_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            header_background_color: lerp_color(
                a.header_background_color,
                b.header_background_color,
                t,
            ),
            header_foreground_color: lerp_color(
                a.header_foreground_color,
                b.header_foreground_color,
                t,
            ),
            header_headline_style: lerp_nearer(
                &a.header_headline_style,
                &b.header_headline_style,
                t,
            ),
            header_help_style: lerp_nearer(&a.header_help_style, &b.header_help_style, t),
            weekday_style: lerp_nearer(&a.weekday_style, &b.weekday_style, t),
            day_style: lerp_nearer(&a.day_style, &b.day_style, t),
            day_foreground_color: lerp_state_color(
                a.day_foreground_color.as_ref(),
                b.day_foreground_color.as_ref(),
                t,
            ),
            day_background_color: lerp_state_color(
                a.day_background_color.as_ref(),
                b.day_background_color.as_ref(),
                t,
            ),
            day_overlay_color: lerp_state_color(
                a.day_overlay_color.as_ref(),
                b.day_overlay_color.as_ref(),
                t,
            ),
            day_shape: lerp_nearer(&a.day_shape, &b.day_shape, t),
            today_foreground_color: lerp_state_color(
                a.today_foreground_color.as_ref(),
                b.today_foreground_color.as_ref(),
                t,
            ),
            today_background_color: lerp_state_color(
                a.today_background_color.as_ref(),
                b.today_background_color.as_ref(),
                t,
            ),
            today_border: side(a.today_border, b.today_border),
            year_style: lerp_nearer(&a.year_style, &b.year_style, t),
            year_foreground_color: lerp_state_color(
                a.year_foreground_color.as_ref(),
                b.year_foreground_color.as_ref(),
                t,
            ),
            year_background_color: lerp_state_color(
                a.year_background_color.as_ref(),
                b.year_background_color.as_ref(),
                t,
            ),
            year_overlay_color: lerp_state_color(
                a.year_overlay_color.as_ref(),
                b.year_overlay_color.as_ref(),
                t,
            ),
            year_shape: lerp_nearer(&a.year_shape, &b.year_shape, t),
            range_picker_background_color: lerp_color(
                a.range_picker_background_color,
                b.range_picker_background_color,
                t,
            ),
            range_picker_elevation: lerp_f32(a.range_picker_elevation, b.range_picker_elevation, t),
            range_picker_shadow_color: lerp_color(
                a.range_picker_shadow_color,
                b.range_picker_shadow_color,
                t,
            ),
            range_picker_surface_tint_color: lerp_color(
                a.range_picker_surface_tint_color,
                b.range_picker_surface_tint_color,
                t,
            ),
            range_picker_shape: lerp_nearer(&a.range_picker_shape, &b.range_picker_shape, t),
            range_picker_header_background_color: lerp_color(
                a.range_picker_header_background_color,
                b.range_picker_header_background_color,
                t,
            ),
            range_picker_header_foreground_color: lerp_color(
                a.range_picker_header_foreground_color,
                b.range_picker_header_foreground_color,
                t,
            ),
            range_picker_header_headline_style: lerp_nearer(
                &a.range_picker_header_headline_style,
                &b.range_picker_header_headline_style,
                t,
            ),
            range_picker_header_help_style: lerp_nearer(
                &a.range_picker_header_help_style,
                &b.range_picker_header_help_style,
                t,
            ),
            range_selection_background_color: lerp_color(
                a.range_selection_background_color,
                b.range_selection_background_color,
                t,
            ),
            range_selection_overlay_color: lerp_state_color(
                a.range_selection_overlay_color.as_ref(),
                b.range_selection_overlay_color.as_ref(),
                t,
            ),
            divider_color: lerp_color(a.divider_color, b.divider_color, t),
            cancel_button_style: lerp_button_style(
                &a.cancel_button_style,
                &b.cancel_button_style,
                t,
            ),
            confirm_button_style: lerp_button_style(
                &a.confirm_button_style,
                &b.confirm_button_style,
                t,
            ),
            toggle_button_text_style: lerp_nearer(
                &a.toggle_button_text_style,
                &b.toggle_button_text_style,
                t,
            ),
            sub_header_foreground_color: lerp_color(
                a.sub_header_foreground_color,
                b.sub_header_foreground_color,
                t,
            ),
        }
    }
}

/// Upstream `DatePickerTheme`.
pub struct DatePickerTheme;

impl DatePickerTheme {
    pub fn new(data: DatePickerThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> DatePickerThemeData {
        context
            .inherited::<DatePickerThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).date_picker_theme.clone())
    }
}

// -- Input decoration (upstream `input_decorator.dart`) -----------------------

/// Upstream `FloatingLabelBehavior`, declared with the thing it describes in
/// [`crate::input_decorator`] and re-exported here.
///
/// It was declared twice -- same name, same variants, same upstream
/// original -- and the two copies could not disagree loudly, because
/// nothing made them meet. A type two modules have to agree on belongs
/// to neither of them.
pub use crate::input_decorator::FloatingLabelBehavior;

/// Upstream `FloatingLabelAlignment`: where along the top edge the floated
/// label sits.
///
/// Upstream is a value class over a private `_x` in -1..1 with two named
/// constants; there are only the two, and a third would need the private
/// constructor, so this is those two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FloatingLabelAlignment {
    /// The reading edge -- upstream's `-1.0`.
    #[default]
    Start,
    /// The middle -- upstream's `0.0`.
    Center,
}

impl FloatingLabelAlignment {
    /// The position upstream stores, in -1..1.
    pub fn x(self) -> f32 {
        match self {
            FloatingLabelAlignment::Start => -1.0,
            FloatingLabelAlignment::Center => 0.0,
        }
    }
}

/// Upstream `InputDecorationThemeData`.
///
/// The longest single class in the material wave after the date picker, and
/// the reason is the same: a decorated field is a stack of parts -- a label
/// that floats, a hint under it, a helper and an error below, a prefix and a
/// suffix beside, a counter at the end, and five borders for the five states
/// it can be in -- and each part is themed on its own.
///
/// The five borders are five fields rather than one state property because
/// that is upstream's shape: an `InputBorder` is a `ShapeBorder` and cannot
/// be resolved from a state set, so the state is which field you read.
#[derive(Clone, Debug, PartialEq)]
pub struct InputDecorationThemeData {
    pub label_style: Option<TextStyle>,
    pub floating_label_style: Option<TextStyle>,
    pub helper_style: Option<TextStyle>,
    pub helper_max_lines: Option<i32>,
    pub hint_style: Option<TextStyle>,
    pub hint_fade_duration: Option<std::time::Duration>,
    pub hint_max_lines: Option<i32>,
    pub error_style: Option<TextStyle>,
    pub error_max_lines: Option<i32>,
    pub floating_label_behavior: FloatingLabelBehavior,
    pub floating_label_alignment: FloatingLabelAlignment,
    /// Whether the field is packed tighter.
    pub is_dense: bool,
    pub content_padding: Option<EdgeInsetsGeometry>,
    /// Whether the field has no vertical padding at all, so it can sit in a
    /// row of its own height.
    pub is_collapsed: bool,
    pub icon_color: Option<Color>,
    pub prefix_style: Option<TextStyle>,
    pub prefix_icon_color: Option<Color>,
    pub prefix_icon_constraints: Option<BoxConstraints>,
    pub suffix_style: Option<TextStyle>,
    pub suffix_icon_color: Option<Color>,
    pub suffix_icon_constraints: Option<BoxConstraints>,
    pub counter_style: Option<TextStyle>,
    /// Whether the field is filled behind its text.
    pub filled: bool,
    pub fill_color: Option<Color>,
    /// The outline a Material 3 field draws when it is not focused.
    pub outline_border: Option<BorderSide>,
    /// The rule under a filled field.
    pub active_indicator_border: Option<BorderSide>,
    pub focus_color: Option<Color>,
    pub hover_color: Option<Color>,
    pub error_border: Option<ShapeBorder>,
    pub focused_border: Option<ShapeBorder>,
    pub focused_error_border: Option<ShapeBorder>,
    pub disabled_border: Option<ShapeBorder>,
    pub enabled_border: Option<ShapeBorder>,
    pub border: Option<ShapeBorder>,
    /// Whether the label lines up with the hint rather than with the top of
    /// the field, which is what a multi-line field wants.
    pub align_label_with_hint: bool,
    pub constraints: Option<BoxConstraints>,
    pub visual_density: Option<VisualDensity>,
}

impl Default for InputDecorationThemeData {
    fn default() -> InputDecorationThemeData {
        InputDecorationThemeData::new()
    }
}

impl InputDecorationThemeData {
    /// Upstream's defaults: auto-floating label at the start, not dense, not
    /// collapsed, not filled, label not aligned with the hint.
    pub fn new() -> InputDecorationThemeData {
        InputDecorationThemeData {
            label_style: None,
            floating_label_style: None,
            helper_style: None,
            helper_max_lines: None,
            hint_style: None,
            hint_fade_duration: None,
            hint_max_lines: None,
            error_style: None,
            error_max_lines: None,
            floating_label_behavior: FloatingLabelBehavior::Auto,
            floating_label_alignment: FloatingLabelAlignment::Start,
            is_dense: false,
            content_padding: None,
            is_collapsed: false,
            icon_color: None,
            prefix_style: None,
            prefix_icon_color: None,
            prefix_icon_constraints: None,
            suffix_style: None,
            suffix_icon_color: None,
            suffix_icon_constraints: None,
            counter_style: None,
            filled: false,
            fill_color: None,
            outline_border: None,
            active_indicator_border: None,
            focus_color: None,
            hover_color: None,
            error_border: None,
            focused_border: None,
            focused_error_border: None,
            disabled_border: None,
            enabled_border: None,
            border: None,
            align_label_with_hint: false,
            constraints: None,
            visual_density: None,
        }
    }

    pub fn with_filled(mut self, filled: bool, fill_color: Color) -> Self {
        self.filled = filled;
        self.fill_color = Some(fill_color);
        self
    }

    pub fn with_border(mut self, border: ShapeBorder) -> Self {
        self.border = Some(border);
        self
    }

    pub fn with_focused_border(mut self, border: ShapeBorder) -> Self {
        self.focused_border = Some(border);
        self
    }

    pub fn with_error_border(mut self, border: ShapeBorder) -> Self {
        self.error_border = Some(border);
        self
    }

    pub fn with_floating_label_behavior(mut self, behavior: FloatingLabelBehavior) -> Self {
        self.floating_label_behavior = behavior;
        self
    }

    pub fn with_content_padding(mut self, padding: EdgeInsetsGeometry) -> Self {
        self.content_padding = Some(padding);
        self
    }

    pub fn with_dense(mut self, is_dense: bool) -> Self {
        self.is_dense = is_dense;
        self
    }

    /// Upstream `InputDecoration.border`'s resolution: which of the five
    /// borders a field in these states draws.
    ///
    /// Upstream's `_getFallbackBorder` reads them in exactly this order, and
    /// the order is the whole of it -- a disabled field with an error shows
    /// the disabled border, and a focused one with an error shows the
    /// focused-error border rather than either of its parents.
    pub fn resolve_border(&self, states: WidgetStates) -> Option<ShapeBorder> {
        let has_error = states.contains(WidgetState::Error);
        let picked = if states.contains(WidgetState::Disabled) {
            self.disabled_border.clone()
        } else if has_error && states.contains(WidgetState::Focused) {
            self.focused_error_border.clone()
        } else if has_error {
            self.error_border.clone()
        } else if states.contains(WidgetState::Focused) {
            self.focused_border.clone()
        } else {
            self.enabled_border.clone()
        };
        picked.or_else(|| self.border.clone())
    }
}

/// Upstream `InputDecorationTheme`.
pub struct InputDecorationTheme;

impl InputDecorationTheme {
    pub fn new(data: InputDecorationThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> InputDecorationThemeData {
        context
            .inherited::<InputDecorationThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).input_decoration_theme.clone())
    }
}

// -- Icon theme (upstream `widgets/icon_theme_data.dart`) ---------------------

/// Upstream `IconThemeData`: how the icons under it are drawn.
///
/// A data class only -- the framework has no `Icon` widget yet (`E5` in the
/// plan) -- and it is here because four of the component themes above carry
/// one and had to leave the field out until it existed. Upstream's `Shadow`
/// is this crate's [`BoxShadow`](crate::painting::BoxShadow) with its spread
/// left at zero; upstream's `Shadow` has no spread, and `BoxShadow` is the
/// same three fields plus one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IconThemeData {
    pub size: Option<f32>,
    /// The Material Symbols axes: how filled, how heavy, how much optical
    /// correction. Upstream passes these straight to the font's variable
    /// axes.
    pub fill: Option<f32>,
    pub weight: Option<f32>,
    pub grade: Option<f32>,
    pub optical_size: Option<f32>,
    pub color: Option<Color>,
    /// Upstream keeps this private behind a getter that clamps to 0..1; the
    /// clamp is in [`IconThemeData::opacity`].
    opacity: Option<f32>,
    pub shadows: Option<Vec<crate::painting::BoxShadow>>,
    /// Whether the size follows the reader's text scale.
    pub apply_text_scaling: Option<bool>,
}

impl IconThemeData {
    pub fn new() -> IconThemeData {
        IconThemeData::default()
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    /// Upstream's `opacity` setter, which stores whatever it is given; the
    /// clamp happens on the way out.
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = Some(opacity);
        self
    }

    /// Upstream's `opacity` getter: `_opacity?.clamp(0.0, 1.0)`.
    pub fn opacity(&self) -> Option<f32> {
        self.opacity.map(|opacity| opacity.clamp(0.0, 1.0))
    }

    /// Upstream `IconThemeData.lerp`.
    pub fn lerp(a: &IconThemeData, b: &IconThemeData, t: f32) -> IconThemeData {
        IconThemeData {
            size: lerp_f32(a.size, b.size, t),
            fill: lerp_f32(a.fill, b.fill, t),
            weight: lerp_f32(a.weight, b.weight, t),
            grade: lerp_f32(a.grade, b.grade, t),
            optical_size: lerp_f32(a.optical_size, b.optical_size, t),
            color: lerp_color(a.color, b.color, t),
            opacity: lerp_f32(a.opacity(), b.opacity(), t),
            shadows: lerp_nearer(&a.shadows, &b.shadows, t),
            apply_text_scaling: lerp_nearer(&a.apply_text_scaling, &b.apply_text_scaling, t),
        }
    }

    /// Upstream `merge`: this one's fields where it has them, the other's
    /// where it does not.
    ///
    /// Every field is `self.x.or(other.x)` and the direction matters on every
    /// one of them -- a nearer `IconTheme` overrides a further one, which is
    /// the whole reason themes nest. A test that sets a field on one side only
    /// cannot see the direction; `tools/order_sweep.py` found eight of these
    /// here, one per field.
    pub fn merge(&self, other: &IconThemeData) -> IconThemeData {
        IconThemeData {
            size: self.size.or(other.size),
            fill: self.fill.or(other.fill),
            weight: self.weight.or(other.weight),
            grade: self.grade.or(other.grade),
            optical_size: self.optical_size.or(other.optical_size),
            color: self.color.or(other.color),
            opacity: self.opacity.or(other.opacity),
            shadows: self.shadows.clone().or_else(|| other.shadows.clone()),
            apply_text_scaling: self.apply_text_scaling.or(other.apply_text_scaling),
        }
    }
}

/// Upstream `IconTheme`.
pub struct IconTheme;

impl IconTheme {
    pub fn new(data: IconThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> IconThemeData {
        context
            .inherited::<IconThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).icon_theme.clone())
    }
}

// -- Text selection (upstream `text_selection_theme.dart`) --------------------

/// Upstream `TextSelectionThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextSelectionThemeData {
    pub cursor_color: Option<Color>,
    /// What selected text is highlighted with.
    pub selection_color: Option<Color>,
    /// The draggable dots at each end of a selection on a touch screen.
    pub selection_handle_color: Option<Color>,
}

impl TextSelectionThemeData {
    pub fn new() -> TextSelectionThemeData {
        TextSelectionThemeData::default()
    }

    pub fn with_cursor_color(mut self, color: Color) -> Self {
        self.cursor_color = Some(color);
        self
    }

    pub fn with_selection_color(mut self, color: Color) -> Self {
        self.selection_color = Some(color);
        self
    }

    pub fn with_selection_handle_color(mut self, color: Color) -> Self {
        self.selection_handle_color = Some(color);
        self
    }

    /// Upstream `TextSelectionThemeData.lerp`.
    pub fn lerp(
        a: &TextSelectionThemeData,
        b: &TextSelectionThemeData,
        t: f32,
    ) -> TextSelectionThemeData {
        TextSelectionThemeData {
            cursor_color: lerp_color(a.cursor_color, b.cursor_color, t),
            selection_color: lerp_color(a.selection_color, b.selection_color, t),
            selection_handle_color: lerp_color(
                a.selection_handle_color,
                b.selection_handle_color,
                t,
            ),
        }
    }
}

/// Upstream `TextSelectionTheme`.
pub struct TextSelectionTheme;

impl TextSelectionTheme {
    pub fn new(data: TextSelectionThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> TextSelectionThemeData {
        context
            .inherited::<TextSelectionThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).text_selection_theme.clone())
    }
}

// -- Popup menu (upstream `popup_menu_theme.dart`) ----------------------------

/// Upstream `PopupMenuPosition`, declared with the thing it describes in
/// [`crate::menu`] and re-exported here.
///
/// It was declared twice -- same name, same variants, same upstream
/// original -- and the two copies could not disagree loudly, because
/// nothing made them meet. A type two modules have to agree on belongs
/// to neither of them.
pub use crate::menu::PopupMenuPosition;

/// Upstream `PopupMenuThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PopupMenuThemeData {
    pub color: Option<Color>,
    pub shape: Option<ShapeBorder>,
    pub menu_padding: Option<EdgeInsetsGeometry>,
    pub elevation: Option<f32>,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    /// The style of an entry, for the entries that take a plain one.
    pub text_style: Option<TextStyle>,
    /// The style of an entry by state, which supersedes
    /// [`PopupMenuThemeData::text_style`] where both are set.
    pub label_text_style: Option<StateProperty<Option<TextStyle>>>,
    pub enable_feedback: Option<bool>,
    pub mouse_cursor: Option<StateProperty<Option<SystemMouseCursor>>>,
    pub position: Option<PopupMenuPosition>,
    pub icon_color: Option<Color>,
    pub icon_size: Option<f32>,
}

impl PopupMenuThemeData {
    pub fn new() -> PopupMenuThemeData {
        PopupMenuThemeData::default()
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_position(mut self, position: PopupMenuPosition) -> Self {
        self.position = Some(position);
        self
    }

    pub fn with_elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }

    /// Upstream `PopupMenuThemeData.lerp`.
    pub fn lerp(a: &PopupMenuThemeData, b: &PopupMenuThemeData, t: f32) -> PopupMenuThemeData {
        PopupMenuThemeData {
            color: lerp_color(a.color, b.color, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            menu_padding: lerp_nearer(&a.menu_padding, &b.menu_padding, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            text_style: lerp_nearer(&a.text_style, &b.text_style, t),
            label_text_style: lerp_nearer(&a.label_text_style, &b.label_text_style, t),
            enable_feedback: lerp_nearer(&a.enable_feedback, &b.enable_feedback, t),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
            position: lerp_nearer(&a.position, &b.position, t),
            icon_color: lerp_color(a.icon_color, b.icon_color, t),
            icon_size: lerp_f32(a.icon_size, b.icon_size, t),
        }
    }
}

/// Upstream `PopupMenuTheme`.
pub struct PopupMenuTheme;

impl PopupMenuTheme {
    pub fn new(data: PopupMenuThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> PopupMenuThemeData {
        context
            .inherited::<PopupMenuThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).popup_menu_theme.clone())
    }
}

// -- Dropdown menu (upstream `dropdown_menu_theme.dart`) ----------------------

/// Upstream `DropdownMenuThemeData`.
///
/// Three of a kind: the field's text, the field's decoration, and the menu
/// that drops out of it -- which is a [`MenuStyle`], the same one a
/// [`MenuTheme`] carries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DropdownMenuThemeData {
    pub text_style: Option<TextStyle>,
    pub input_decoration_theme: Option<InputDecorationThemeData>,
    pub menu_style: Option<MenuStyle>,
    pub disabled_color: Option<Color>,
}

impl DropdownMenuThemeData {
    pub fn new() -> DropdownMenuThemeData {
        DropdownMenuThemeData::default()
    }

    pub fn with_menu_style(mut self, menu_style: MenuStyle) -> Self {
        self.menu_style = Some(menu_style);
        self
    }

    pub fn with_input_decoration_theme(mut self, theme: InputDecorationThemeData) -> Self {
        self.input_decoration_theme = Some(theme);
        self
    }

    /// Upstream `DropdownMenuThemeData.lerp`.
    pub fn lerp(
        a: &DropdownMenuThemeData,
        b: &DropdownMenuThemeData,
        t: f32,
    ) -> DropdownMenuThemeData {
        DropdownMenuThemeData {
            text_style: lerp_nearer(&a.text_style, &b.text_style, t),
            // Upstream takes the decoration from the nearer end too: it has
            // no `lerp` of its own, for the reason given on that class.
            input_decoration_theme: lerp_nearer(
                &a.input_decoration_theme,
                &b.input_decoration_theme,
                t,
            ),
            menu_style: lerp_menu_style(&a.menu_style, &b.menu_style, t),
            disabled_color: lerp_color(a.disabled_color, b.disabled_color, t),
        }
    }
}

/// Upstream `DropdownMenuTheme`.
pub struct DropdownMenuTheme;

impl DropdownMenuTheme {
    pub fn new(data: DropdownMenuThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> DropdownMenuThemeData {
        context
            .inherited::<DropdownMenuThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).dropdown_menu_theme.clone())
    }
}

// -- Bottom app bar (upstream `bottom_app_bar_theme.dart`) --------------------

/// Upstream `BottomAppBarThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BottomAppBarThemeData {
    pub color: Option<Color>,
    pub elevation: Option<f32>,
    /// The notch a floating action button sits in -- the one place a theme
    /// carries a [`NotchedShape`](crate::borders::NotchedShape).
    pub shape: Option<crate::borders::NotchedShape>,
    pub height: Option<f32>,
    pub surface_tint_color: Option<Color>,
    pub shadow_color: Option<Color>,
    pub padding: Option<EdgeInsetsGeometry>,
}

impl BottomAppBarThemeData {
    pub fn new() -> BottomAppBarThemeData {
        BottomAppBarThemeData::default()
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    pub fn with_shape(mut self, shape: crate::borders::NotchedShape) -> Self {
        self.shape = Some(shape);
        self
    }

    /// Upstream `BottomAppBarThemeData.lerp`.
    pub fn lerp(
        a: &BottomAppBarThemeData,
        b: &BottomAppBarThemeData,
        t: f32,
    ) -> BottomAppBarThemeData {
        BottomAppBarThemeData {
            color: lerp_color(a.color, b.color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            height: lerp_f32(a.height, b.height, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            padding: lerp_nearer(&a.padding, &b.padding, t),
        }
    }
}

/// Upstream `BottomAppBarTheme`.
pub struct BottomAppBarTheme;

impl BottomAppBarTheme {
    pub fn new(data: BottomAppBarThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> BottomAppBarThemeData {
        context
            .inherited::<BottomAppBarThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).bottom_app_bar_theme.clone())
    }
}

// -- Navigation bar and drawer (upstream `navigation_bar_theme.dart`,
//    `navigation_drawer_theme.dart`) -----------------------------------------

/// Upstream `NavigationDestinationLabelBehavior`: which of a navigation
/// bar's labels are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NavigationDestinationLabelBehavior {
    #[default]
    AlwaysShow,
    AlwaysHide,
    /// Only under the destination the reader is on.
    OnlyShowSelected,
}

/// Upstream `NavigationBarThemeData`: Material 3's bottom bar, which is a
/// different widget from the Material 2 [`BottomNavigationBarThemeData`] and
/// keeps its own theme.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavigationBarThemeData {
    pub height: Option<f32>,
    pub background_color: Option<Color>,
    pub elevation: Option<f32>,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    /// The pill behind the selected destination's icon.
    pub indicator_color: Option<Color>,
    pub indicator_shape: Option<ShapeBorder>,
    pub label_text_style: Option<StateProperty<Option<TextStyle>>>,
    /// The icon theme by state -- one property rather than the Material 2
    /// bar's two fields, which is the newer widget's shape.
    pub icon_theme: Option<StateProperty<Option<IconThemeData>>>,
    pub label_behavior: Option<NavigationDestinationLabelBehavior>,
    pub overlay_color: Option<StateProperty<Option<Color>>>,
    pub label_padding: Option<EdgeInsetsGeometry>,
}

impl NavigationBarThemeData {
    pub fn new() -> NavigationBarThemeData {
        NavigationBarThemeData::default()
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    pub fn with_indicator_color(mut self, color: Color) -> Self {
        self.indicator_color = Some(color);
        self
    }

    pub fn with_label_behavior(mut self, behavior: NavigationDestinationLabelBehavior) -> Self {
        self.label_behavior = Some(behavior);
        self
    }

    /// Upstream `NavigationBarThemeData.lerp`.
    pub fn lerp(
        a: &NavigationBarThemeData,
        b: &NavigationBarThemeData,
        t: f32,
    ) -> NavigationBarThemeData {
        NavigationBarThemeData {
            height: lerp_f32(a.height, b.height, t),
            background_color: lerp_color(a.background_color, b.background_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            indicator_color: lerp_color(a.indicator_color, b.indicator_color, t),
            indicator_shape: lerp_nearer(&a.indicator_shape, &b.indicator_shape, t),
            label_text_style: lerp_nearer(&a.label_text_style, &b.label_text_style, t),
            icon_theme: lerp_nearer(&a.icon_theme, &b.icon_theme, t),
            label_behavior: lerp_nearer(&a.label_behavior, &b.label_behavior, t),
            overlay_color: lerp_state_color(a.overlay_color.as_ref(), b.overlay_color.as_ref(), t),
            label_padding: lerp_nearer(&a.label_padding, &b.label_padding, t),
        }
    }
}

/// Upstream `NavigationBarTheme`.
pub struct NavigationBarTheme;

impl NavigationBarTheme {
    pub fn new(data: NavigationBarThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> NavigationBarThemeData {
        context
            .inherited::<NavigationBarThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).navigation_bar_theme.clone())
    }
}

/// Upstream `NavigationDrawerThemeData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavigationDrawerThemeData {
    /// How tall one destination is.
    pub tile_height: Option<f32>,
    pub background_color: Option<Color>,
    pub elevation: Option<f32>,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub indicator_color: Option<Color>,
    pub indicator_shape: Option<ShapeBorder>,
    pub indicator_size: Option<Size>,
    pub label_text_style: Option<StateProperty<Option<TextStyle>>>,
    pub icon_theme: Option<StateProperty<Option<IconThemeData>>>,
}

impl NavigationDrawerThemeData {
    pub fn new() -> NavigationDrawerThemeData {
        NavigationDrawerThemeData::default()
    }

    pub fn with_tile_height(mut self, height: f32) -> Self {
        self.tile_height = Some(height);
        self
    }

    pub fn with_indicator_color(mut self, color: Color) -> Self {
        self.indicator_color = Some(color);
        self
    }

    pub fn with_indicator_size(mut self, size: Size) -> Self {
        self.indicator_size = Some(size);
        self
    }

    /// Upstream `NavigationDrawerThemeData.lerp`.
    pub fn lerp(
        a: &NavigationDrawerThemeData,
        b: &NavigationDrawerThemeData,
        t: f32,
    ) -> NavigationDrawerThemeData {
        NavigationDrawerThemeData {
            tile_height: lerp_f32(a.tile_height, b.tile_height, t),
            background_color: lerp_color(a.background_color, b.background_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            indicator_color: lerp_color(a.indicator_color, b.indicator_color, t),
            indicator_shape: lerp_nearer(&a.indicator_shape, &b.indicator_shape, t),
            indicator_size: match (a.indicator_size, b.indicator_size) {
                (Some(first), Some(second)) => Some(Size::new(
                    first.width + (second.width - first.width) * t,
                    first.height + (second.height - first.height) * t,
                )),
                (first, second) => {
                    if t < 0.5 {
                        first
                    } else {
                        second
                    }
                }
            },
            label_text_style: lerp_nearer(&a.label_text_style, &b.label_text_style, t),
            icon_theme: lerp_nearer(&a.icon_theme, &b.icon_theme, t),
        }
    }
}

/// Upstream `NavigationDrawerTheme`.
pub struct NavigationDrawerTheme;

impl NavigationDrawerTheme {
    pub fn new(data: NavigationDrawerThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> NavigationDrawerThemeData {
        context
            .inherited::<NavigationDrawerThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).navigation_drawer_theme.clone())
    }
}

// -- Carousel (upstream `carousel_theme.dart`) --------------------------------

/// Upstream `CarouselViewThemeData`.
///
/// `itemClipBehavior` is not modelled, for the reason `clipBehavior` is not
/// modelled anywhere else here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarouselViewThemeData {
    pub padding: Option<EdgeInsets>,
    pub background_color: Option<Color>,
    pub elevation: Option<f32>,
    pub shape: Option<ShapeBorder>,
    pub overlay_color: Option<StateProperty<Option<Color>>>,
}

impl CarouselViewThemeData {
    pub fn new() -> CarouselViewThemeData {
        CarouselViewThemeData::default()
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Upstream `CarouselViewThemeData.lerp`.
    pub fn lerp(
        a: &CarouselViewThemeData,
        b: &CarouselViewThemeData,
        t: f32,
    ) -> CarouselViewThemeData {
        CarouselViewThemeData {
            padding: lerp_nearer(&a.padding, &b.padding, t),
            background_color: lerp_color(a.background_color, b.background_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shape: lerp_nearer(&a.shape, &b.shape, t),
            overlay_color: lerp_state_color(a.overlay_color.as_ref(), b.overlay_color.as_ref(), t),
        }
    }
}

/// Upstream `CarouselViewTheme`.
pub struct CarouselViewTheme;

impl CarouselViewTheme {
    pub fn new(data: CarouselViewThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> CarouselViewThemeData {
        context
            .inherited::<CarouselViewThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).carousel_view_theme.clone())
    }
}

// -- Typography (upstream `material/text_theme.dart`, `typography.dart`) ------

/// Upstream `TextTheme`: the fifteen named styles a Material theme carries.
///
/// Material 3's names are a grid: three sizes each of display, headline,
/// title, body and label. A control asks for the role rather than for a size
/// -- a button's label is `labelLarge` wherever it is -- which is what lets
/// one typography swap resize an application coherently.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextTheme {
    pub display_large: Option<TextStyle>,
    pub display_medium: Option<TextStyle>,
    pub display_small: Option<TextStyle>,
    pub headline_large: Option<TextStyle>,
    pub headline_medium: Option<TextStyle>,
    pub headline_small: Option<TextStyle>,
    pub title_large: Option<TextStyle>,
    pub title_medium: Option<TextStyle>,
    pub title_small: Option<TextStyle>,
    pub body_large: Option<TextStyle>,
    pub body_medium: Option<TextStyle>,
    pub body_small: Option<TextStyle>,
    pub label_large: Option<TextStyle>,
    pub label_medium: Option<TextStyle>,
    pub label_small: Option<TextStyle>,
}

impl TextTheme {
    pub fn new() -> TextTheme {
        TextTheme::default()
    }

    /// Upstream `TextTheme.merge`: this theme's styles where it has them,
    /// the other's where it does not.
    pub fn merge(&self, other: &TextTheme) -> TextTheme {
        TextTheme {
            display_large: self
                .display_large
                .clone()
                .or_else(|| other.display_large.clone()),
            display_medium: self
                .display_medium
                .clone()
                .or_else(|| other.display_medium.clone()),
            display_small: self
                .display_small
                .clone()
                .or_else(|| other.display_small.clone()),
            headline_large: self
                .headline_large
                .clone()
                .or_else(|| other.headline_large.clone()),
            headline_medium: self
                .headline_medium
                .clone()
                .or_else(|| other.headline_medium.clone()),
            headline_small: self
                .headline_small
                .clone()
                .or_else(|| other.headline_small.clone()),
            title_large: self
                .title_large
                .clone()
                .or_else(|| other.title_large.clone()),
            title_medium: self
                .title_medium
                .clone()
                .or_else(|| other.title_medium.clone()),
            title_small: self
                .title_small
                .clone()
                .or_else(|| other.title_small.clone()),
            body_large: self.body_large.clone().or_else(|| other.body_large.clone()),
            body_medium: self
                .body_medium
                .clone()
                .or_else(|| other.body_medium.clone()),
            body_small: self.body_small.clone().or_else(|| other.body_small.clone()),
            label_large: self
                .label_large
                .clone()
                .or_else(|| other.label_large.clone()),
            label_medium: self
                .label_medium
                .clone()
                .or_else(|| other.label_medium.clone()),
            label_small: self
                .label_small
                .clone()
                .or_else(|| other.label_small.clone()),
        }
    }

    /// Upstream `TextTheme.apply`, narrowed to the colour: every style that
    /// is set takes it.
    pub fn apply_color(&self, color: Color) -> TextTheme {
        let recolour = |style: &Option<TextStyle>| {
            style.as_ref().map(|style| TextStyle {
                color,
                ..style.clone()
            })
        };
        TextTheme {
            display_large: recolour(&self.display_large),
            display_medium: recolour(&self.display_medium),
            display_small: recolour(&self.display_small),
            headline_large: recolour(&self.headline_large),
            headline_medium: recolour(&self.headline_medium),
            headline_small: recolour(&self.headline_small),
            title_large: recolour(&self.title_large),
            title_medium: recolour(&self.title_medium),
            title_small: recolour(&self.title_small),
            body_large: recolour(&self.body_large),
            body_medium: recolour(&self.body_medium),
            body_small: recolour(&self.body_small),
            label_large: recolour(&self.label_large),
            label_medium: recolour(&self.label_medium),
            label_small: recolour(&self.label_small),
        }
    }

    /// Upstream `TextTheme.lerp`.
    pub fn lerp(a: &TextTheme, b: &TextTheme, t: f32) -> TextTheme {
        TextTheme {
            display_large: lerp_nearer(&a.display_large, &b.display_large, t),
            display_medium: lerp_nearer(&a.display_medium, &b.display_medium, t),
            display_small: lerp_nearer(&a.display_small, &b.display_small, t),
            headline_large: lerp_nearer(&a.headline_large, &b.headline_large, t),
            headline_medium: lerp_nearer(&a.headline_medium, &b.headline_medium, t),
            headline_small: lerp_nearer(&a.headline_small, &b.headline_small, t),
            title_large: lerp_nearer(&a.title_large, &b.title_large, t),
            title_medium: lerp_nearer(&a.title_medium, &b.title_medium, t),
            title_small: lerp_nearer(&a.title_small, &b.title_small, t),
            body_large: lerp_nearer(&a.body_large, &b.body_large, t),
            body_medium: lerp_nearer(&a.body_medium, &b.body_medium, t),
            body_small: lerp_nearer(&a.body_small, &b.body_small, t),
            label_large: lerp_nearer(&a.label_large, &b.label_large, t),
            label_medium: lerp_nearer(&a.label_medium, &b.label_medium, t),
            label_small: lerp_nearer(&a.label_small, &b.label_small, t),
        }
    }
}

/// Upstream `Typography`: the geometries a `TextTheme` is built from.
///
/// Three of them, because a script's metrics are not the Latin alphabet's:
/// `english_like` for scripts with the Latin alphabet's proportions, `dense`
/// for Chinese, Japanese and Korean, and `tall` for scripts that need more
/// room above and below. Upstream picks by locale; this exposes the three
/// and leaves the picking to the localisation wave (`E4`).
///
/// The tables are the Material 3 ones (`_M3Typography`), generated by
/// parsing upstream rather than copied: forty-five rows of four numbers is
/// not something to transcribe by hand.
///
/// # In Material 3 the three tables carry the same numbers
///
/// This surprises, and it is upstream's doing rather than this port's.
/// Material 2's geometries really did differ in size and line height;
/// Material 3's differ in **one field only** -- `dense` sets
/// `textBaseline: ideographic` where the other two set `alphabetic`, and
/// `english_like` and `tall` are identical throughout. The regression line
/// below asserts that, so that a reader who expects three different tables
/// finds out why there are not.
///
/// **Recorded divergence:** this crate's [`TextStyle`](crate::engine::TextStyle)
/// carries no baseline -- the engine's text ABI takes none -- so the one
/// field that distinguishes `dense` is dropped, and all three functions
/// currently answer the same numbers. They stay three functions because the
/// distinction is upstream's and returns the moment the baseline can be
/// carried; collapsing them into one would bake the limitation into the
/// API.
pub struct Typography;

impl Typography {
    /// Upstream `Typography.englishLike2021`.
    pub fn english_like() -> TextTheme {
        TextTheme {
            display_large: Some(TextStyle {
                font_size: 57.0,
                font_weight: 400,
                letter_spacing: Some(-0.25),
                height: Some(1.12),
                ..TextStyle::default()
            }),
            display_medium: Some(TextStyle {
                font_size: 45.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.16),
                ..TextStyle::default()
            }),
            display_small: Some(TextStyle {
                font_size: 36.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.22),
                ..TextStyle::default()
            }),
            headline_large: Some(TextStyle {
                font_size: 32.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.25),
                ..TextStyle::default()
            }),
            headline_medium: Some(TextStyle {
                font_size: 28.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.29),
                ..TextStyle::default()
            }),
            headline_small: Some(TextStyle {
                font_size: 24.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.33),
                ..TextStyle::default()
            }),
            title_large: Some(TextStyle {
                font_size: 22.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.27),
                ..TextStyle::default()
            }),
            title_medium: Some(TextStyle {
                font_size: 16.0,
                font_weight: 500,
                letter_spacing: Some(0.15),
                height: Some(1.50),
                ..TextStyle::default()
            }),
            title_small: Some(TextStyle {
                font_size: 14.0,
                font_weight: 500,
                letter_spacing: Some(0.10),
                height: Some(1.43),
                ..TextStyle::default()
            }),
            body_large: Some(TextStyle {
                font_size: 16.0,
                font_weight: 400,
                letter_spacing: Some(0.50),
                height: Some(1.50),
                ..TextStyle::default()
            }),
            body_medium: Some(TextStyle {
                font_size: 14.0,
                font_weight: 400,
                letter_spacing: Some(0.25),
                height: Some(1.43),
                ..TextStyle::default()
            }),
            body_small: Some(TextStyle {
                font_size: 12.0,
                font_weight: 400,
                letter_spacing: Some(0.40),
                height: Some(1.33),
                ..TextStyle::default()
            }),
            label_large: Some(TextStyle {
                font_size: 14.0,
                font_weight: 500,
                letter_spacing: Some(0.10),
                height: Some(1.43),
                ..TextStyle::default()
            }),
            label_medium: Some(TextStyle {
                font_size: 12.0,
                font_weight: 500,
                letter_spacing: Some(0.50),
                height: Some(1.33),
                ..TextStyle::default()
            }),
            label_small: Some(TextStyle {
                font_size: 11.0,
                font_weight: 500,
                letter_spacing: Some(0.50),
                height: Some(1.45),
                ..TextStyle::default()
            }),
        }
    }

    /// Upstream `Typography.dense2021`.
    pub fn dense() -> TextTheme {
        TextTheme {
            display_large: Some(TextStyle {
                font_size: 57.0,
                font_weight: 400,
                letter_spacing: Some(-0.25),
                height: Some(1.12),
                ..TextStyle::default()
            }),
            display_medium: Some(TextStyle {
                font_size: 45.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.16),
                ..TextStyle::default()
            }),
            display_small: Some(TextStyle {
                font_size: 36.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.22),
                ..TextStyle::default()
            }),
            headline_large: Some(TextStyle {
                font_size: 32.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.25),
                ..TextStyle::default()
            }),
            headline_medium: Some(TextStyle {
                font_size: 28.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.29),
                ..TextStyle::default()
            }),
            headline_small: Some(TextStyle {
                font_size: 24.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.33),
                ..TextStyle::default()
            }),
            title_large: Some(TextStyle {
                font_size: 22.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.27),
                ..TextStyle::default()
            }),
            title_medium: Some(TextStyle {
                font_size: 16.0,
                font_weight: 500,
                letter_spacing: Some(0.15),
                height: Some(1.50),
                ..TextStyle::default()
            }),
            title_small: Some(TextStyle {
                font_size: 14.0,
                font_weight: 500,
                letter_spacing: Some(0.10),
                height: Some(1.43),
                ..TextStyle::default()
            }),
            body_large: Some(TextStyle {
                font_size: 16.0,
                font_weight: 400,
                letter_spacing: Some(0.50),
                height: Some(1.50),
                ..TextStyle::default()
            }),
            body_medium: Some(TextStyle {
                font_size: 14.0,
                font_weight: 400,
                letter_spacing: Some(0.25),
                height: Some(1.43),
                ..TextStyle::default()
            }),
            body_small: Some(TextStyle {
                font_size: 12.0,
                font_weight: 400,
                letter_spacing: Some(0.40),
                height: Some(1.33),
                ..TextStyle::default()
            }),
            label_large: Some(TextStyle {
                font_size: 14.0,
                font_weight: 500,
                letter_spacing: Some(0.10),
                height: Some(1.43),
                ..TextStyle::default()
            }),
            label_medium: Some(TextStyle {
                font_size: 12.0,
                font_weight: 500,
                letter_spacing: Some(0.50),
                height: Some(1.33),
                ..TextStyle::default()
            }),
            label_small: Some(TextStyle {
                font_size: 11.0,
                font_weight: 500,
                letter_spacing: Some(0.50),
                height: Some(1.45),
                ..TextStyle::default()
            }),
        }
    }

    /// Upstream `Typography.tall2021`.
    pub fn tall() -> TextTheme {
        TextTheme {
            display_large: Some(TextStyle {
                font_size: 57.0,
                font_weight: 400,
                letter_spacing: Some(-0.25),
                height: Some(1.12),
                ..TextStyle::default()
            }),
            display_medium: Some(TextStyle {
                font_size: 45.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.16),
                ..TextStyle::default()
            }),
            display_small: Some(TextStyle {
                font_size: 36.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.22),
                ..TextStyle::default()
            }),
            headline_large: Some(TextStyle {
                font_size: 32.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.25),
                ..TextStyle::default()
            }),
            headline_medium: Some(TextStyle {
                font_size: 28.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.29),
                ..TextStyle::default()
            }),
            headline_small: Some(TextStyle {
                font_size: 24.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.33),
                ..TextStyle::default()
            }),
            title_large: Some(TextStyle {
                font_size: 22.0,
                font_weight: 400,
                letter_spacing: Some(0.00),
                height: Some(1.27),
                ..TextStyle::default()
            }),
            title_medium: Some(TextStyle {
                font_size: 16.0,
                font_weight: 500,
                letter_spacing: Some(0.15),
                height: Some(1.50),
                ..TextStyle::default()
            }),
            title_small: Some(TextStyle {
                font_size: 14.0,
                font_weight: 500,
                letter_spacing: Some(0.10),
                height: Some(1.43),
                ..TextStyle::default()
            }),
            body_large: Some(TextStyle {
                font_size: 16.0,
                font_weight: 400,
                letter_spacing: Some(0.50),
                height: Some(1.50),
                ..TextStyle::default()
            }),
            body_medium: Some(TextStyle {
                font_size: 14.0,
                font_weight: 400,
                letter_spacing: Some(0.25),
                height: Some(1.43),
                ..TextStyle::default()
            }),
            body_small: Some(TextStyle {
                font_size: 12.0,
                font_weight: 400,
                letter_spacing: Some(0.40),
                height: Some(1.33),
                ..TextStyle::default()
            }),
            label_large: Some(TextStyle {
                font_size: 14.0,
                font_weight: 500,
                letter_spacing: Some(0.10),
                height: Some(1.43),
                ..TextStyle::default()
            }),
            label_medium: Some(TextStyle {
                font_size: 12.0,
                font_weight: 500,
                letter_spacing: Some(0.50),
                height: Some(1.33),
                ..TextStyle::default()
            }),
            label_small: Some(TextStyle {
                font_size: 11.0,
                font_weight: 500,
                letter_spacing: Some(0.50),
                height: Some(1.45),
                ..TextStyle::default()
            }),
        }
    }
}

// -- Button bar (upstream `button_bar_theme.dart`) ----------------------------

/// Upstream `ButtonBarThemeData`: how a row of dialog buttons is arranged.
///
/// Material 2's, like [`ButtonThemeData`] which it echoes -- upstream keeps
/// both for the widgets that predate the Material 3 button family.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ButtonBarThemeData {
    pub alignment: Option<crate::render::MainAxisAlignment>,
    pub main_axis_size: Option<crate::render::MainAxisSize>,
    pub button_text_theme: Option<ButtonTextTheme>,
    pub button_min_width: Option<f32>,
    pub button_height: Option<f32>,
    pub button_padding: Option<EdgeInsetsGeometry>,
    pub button_aligned_dropdown: Option<bool>,
    pub layout_behavior: Option<ButtonBarLayoutBehavior>,
    /// Which way the buttons stack when they will not fit on one line.
    pub overflow_direction: Option<crate::render::VerticalDirection>,
}

impl ButtonBarThemeData {
    pub fn new() -> ButtonBarThemeData {
        ButtonBarThemeData::default()
    }

    pub fn with_alignment(mut self, alignment: crate::render::MainAxisAlignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    pub fn with_button_metrics(mut self, min_width: f32, height: f32) -> Self {
        self.button_min_width = Some(min_width);
        self.button_height = Some(height);
        self
    }

    pub fn with_overflow_direction(mut self, direction: crate::render::VerticalDirection) -> Self {
        self.overflow_direction = Some(direction);
        self
    }

    /// Upstream `ButtonBarThemeData.lerp`.
    pub fn lerp(a: &ButtonBarThemeData, b: &ButtonBarThemeData, t: f32) -> ButtonBarThemeData {
        ButtonBarThemeData {
            alignment: lerp_nearer(&a.alignment, &b.alignment, t),
            main_axis_size: lerp_nearer(&a.main_axis_size, &b.main_axis_size, t),
            button_text_theme: lerp_nearer(&a.button_text_theme, &b.button_text_theme, t),
            button_min_width: lerp_f32(a.button_min_width, b.button_min_width, t),
            button_height: lerp_f32(a.button_height, b.button_height, t),
            button_padding: lerp_nearer(&a.button_padding, &b.button_padding, t),
            button_aligned_dropdown: lerp_nearer(
                &a.button_aligned_dropdown,
                &b.button_aligned_dropdown,
                t,
            ),
            layout_behavior: lerp_nearer(&a.layout_behavior, &b.layout_behavior, t),
            overflow_direction: lerp_nearer(&a.overflow_direction, &b.overflow_direction, t),
        }
    }
}

/// Upstream `ButtonBarTheme`.
pub struct ButtonBarTheme;

impl ButtonBarTheme {
    pub fn new(data: ButtonBarThemeData, child: AnyWidget) -> AnyWidget {
        provide(data, child)
    }

    pub fn of(context: &mut BuildContext) -> ButtonBarThemeData {
        context
            .inherited::<ButtonBarThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).button_bar_theme.clone())
    }
}

// -- Theme extensions (upstream `theme_data.dart`) ----------------------------

/// Upstream `ThemeExtension<T>`: an application's own theme data, carried on
/// [`ThemeData`] beside the ones the framework declares.
///
/// Upstream keys the extensions by their runtime type and finds one with
/// `Theme.of(context).extension<MyExtension>()`; here the key is a
/// [`TypeId`](std::any::TypeId) and the lookup is
/// [`ThemeData::extension`](crate::theme::ThemeData::extension).
///
/// The trait is object-safe on purpose -- a theme holds a list of them and
/// cannot know their types -- which is why `lerp` takes and returns a boxed
/// one rather than `Self`.
pub trait ThemeExtension: std::any::Any {
    /// Upstream's `lerp`, with the other end as a trait object because the
    /// list this is stored in is untyped. An implementation downcasts and
    /// falls back to `self` where the other end is a different extension,
    /// which is what upstream's `covariant` parameter means at runtime.
    fn lerp(&self, other: &dyn ThemeExtension, t: f32) -> std::rc::Rc<dyn ThemeExtension>;

    /// For the downcast in `lerp` and in
    /// [`ThemeData::extension`](crate::theme::ThemeData::extension).
    fn as_any(&self) -> &dyn std::any::Any;
}

/// The extensions a [`ThemeData`] carries, keyed by type.
///
/// Two are equal when they hold the same objects, by identity: an extension
/// is a trait object with no `PartialEq` to call, and a theme rebuilt with a
/// freshly constructed extension counts as changed -- the same rule, and the
/// same reason, as [`StateProperty`].
#[derive(Clone, Default)]
pub struct ThemeExtensions {
    entries: Vec<(std::any::TypeId, std::rc::Rc<dyn ThemeExtension>)>,
}

impl ThemeExtensions {
    pub fn new() -> ThemeExtensions {
        ThemeExtensions::default()
    }

    /// Adds one, replacing whatever was stored for its type -- upstream's
    /// map keyed by runtime type.
    pub fn insert<T: ThemeExtension + 'static>(&mut self, extension: T) {
        let key = std::any::TypeId::of::<T>();
        let value: std::rc::Rc<dyn ThemeExtension> = std::rc::Rc::new(extension);
        match self.entries.iter_mut().find(|(at, _)| *at == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key, value)),
        }
    }

    /// Upstream `ThemeData.extension<T>()`.
    pub fn get<T: ThemeExtension + 'static>(&self) -> Option<&T> {
        self.entries
            .iter()
            .find(|(at, _)| *at == std::any::TypeId::of::<T>())
            .and_then(|(_, extension)| extension.as_any().downcast_ref::<T>())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Upstream's `_lerpThemeExtensions`: each extension interpolated with
    /// the one of its own type on the other side, and an extension with no
    /// counterpart taken from whichever end has it.
    pub fn lerp(a: &ThemeExtensions, b: &ThemeExtensions, t: f32) -> ThemeExtensions {
        let mut result = ThemeExtensions::new();
        for (key, extension) in &a.entries {
            match b.entries.iter().find(|(at, _)| at == key) {
                Some((_, other)) => result
                    .entries
                    .push((*key, extension.lerp(other.as_ref(), t))),
                // Upstream keeps `a`'s where `b` has none: an extension that
                // the new theme does not mention is not thereby removed
                // half-way through an animation.
                None => result.entries.push((*key, std::rc::Rc::clone(extension))),
            }
        }
        for (key, extension) in &b.entries {
            if !a.entries.iter().any(|(at, _)| at == key) {
                result.entries.push((*key, std::rc::Rc::clone(extension)));
            }
        }
        result
    }
}

impl PartialEq for ThemeExtensions {
    fn eq(&self, other: &ThemeExtensions) -> bool {
        self.entries.len() == other.entries.len()
            && self.entries.iter().zip(other.entries.iter()).all(
                |((mine, first), (theirs, second))| {
                    mine == theirs && std::rc::Rc::ptr_eq(first, second)
                },
            )
    }
}

impl std::fmt::Debug for ThemeExtensions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ThemeExtensions")
            .field("len", &self.entries.len())
            .finish()
    }
}

/// The three-step fallback a [`crate::components::MaterialBanner`] reads:
/// the widget's own field, then [`MaterialBannerThemeData`], then upstream's
/// `_BannerDefaultsM3`.
///
/// The widget's own fields are not here -- a resolver reads the ambient, and
/// the widget applies its own on top -- with one exception noted below.
pub struct ResolvedMaterialBanner {
    pub background_color: Color,
    pub surface_tint_color: Option<Color>,
    pub shadow_color: Option<Color>,
    pub divider_color: Color,
    pub elevation: f32,
    /// Left as the ambient's raw answer because the default depends on
    /// something only the widget knows -- whether its actions fit on the
    /// content's row. See [`ResolvedMaterialBanner::content_padding`].
    pub padding: Option<EdgeInsetsGeometry>,
    pub leading_padding: EdgeInsetsGeometry,
}

impl ResolvedMaterialBanner {
    /// Upstream's `minActionBarHeight` default: a bar of actions is at least
    /// this tall whatever is in it, so a banner with one short button does
    /// not read as a thinner banner.
    pub const MIN_ACTION_BAR_HEIGHT: f32 = 52.0;
    /// Upstream's `_kMaxContentTextScaleFactor`, which the banner clamps its
    /// content and its actions to "to keep the visual hierarchy the same even
    /// with larger font sizes".
    pub const MAX_CONTENT_TEXT_SCALE_FACTOR: f32 = 1.5;

    pub fn of(context: &mut BuildContext) -> ResolvedMaterialBanner {
        let data = MaterialBannerTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        ResolvedMaterialBanner {
            // `_BannerDefaultsM3.backgroundColor`.
            background_color: data
                .background_color
                .unwrap_or_else(|| scheme.surface_container_low()),
            // `_BannerDefaultsM3.surfaceTintColor` is transparent, which is
            // M3 saying "no tint" -- the tint is M2's way of showing
            // elevation and M3 does not use it here.
            surface_tint_color: data.surface_tint_color,
            // No default: upstream's `defaults` has no `shadowColor`, so an
            // unset one means the `Material`'s own.
            shadow_color: data.shadow_color,
            // `_BannerDefaultsM3.dividerColor`.
            divider_color: data
                .divider_color
                .unwrap_or_else(|| scheme.outline_variant()),
            // Upstream's expression is `widget.elevation ?? bannerTheme
            // .elevation ?? 0.0` -- it never reaches `defaults`, whose M3
            // value is 1.0. So a banner with no theme sits flat on the page,
            // not one step off it. Ported as written; see the regression line.
            elevation: data.elevation.unwrap_or(0.0),
            padding: data.padding,
            leading_padding: data
                .leading_padding
                .unwrap_or(EdgeInsetsGeometry::Directional(
                    crate::render::EdgeInsetsDirectional::only(0.0, 0.0, 16.0, 0.0),
                )),
        }
    }

    /// Upstream's padding default, which is two different insets depending on
    /// whether the actions share the content's row.
    ///
    /// The reason they differ: on one row the actions sit beside the text and
    /// their own 52-tall bar supplies the height, so the banner needs almost
    /// no top inset of its own. Stacked, nothing else is holding the text off
    /// the top edge, so the banner does it: 24 above and 4 below.
    pub fn content_padding(&self, is_single_row: bool) -> EdgeInsetsGeometry {
        self.padding.unwrap_or(if is_single_row {
            EdgeInsetsGeometry::Directional(crate::render::EdgeInsetsDirectional::only(
                16.0, 2.0, 0.0, 0.0,
            ))
        } else {
            EdgeInsetsGeometry::Directional(crate::render::EdgeInsetsDirectional::only(
                16.0, 24.0, 16.0, 4.0,
            ))
        })
    }

    /// Upstream's `widget.margin ?? EdgeInsets.only(bottom: elevation > 0 ?
    /// 10.0 : 0.0)`: a raised banner leaves room under itself for its own
    /// shadow, and a flat one has no shadow to leave room for.
    pub fn default_margin(&self) -> EdgeInsets {
        EdgeInsets::only(0.0, 0.0, 0.0, if self.elevation > 0.0 { 10.0 } else { 0.0 })
    }
}

/// What a divider draws with, once the theme has had its say -- the three-step
/// fallback written out once, since every control does the same thing.
///
/// Upstream `Divider`'s build: `themeData.color ?? theme.dividerColor`,
/// `themeData.space ?? 16`, `themeData.thickness ?? 0`.
pub struct ResolvedDivider {
    pub color: Color,
    pub space: f32,
    pub thickness: f32,
    pub indent: f32,
    pub end_indent: f32,
}

impl ResolvedDivider {
    pub fn of(context: &mut BuildContext) -> ResolvedDivider {
        let data = DividerTheme::of(context);
        let theme = ThemeData::of(context);
        ResolvedDivider {
            color: data.color.unwrap_or(theme.divider_color),
            space: data.space.unwrap_or(16.0),
            thickness: data.thickness.unwrap_or(0.0),
            indent: data.indent.unwrap_or(0.0),
            end_indent: data.end_indent.unwrap_or(0.0),
        }
    }

    /// The height of the line itself: upstream draws a hairline for a
    /// thickness of zero, which is what `math.max(thickness, 0.0)` on a
    /// device pixel comes to.
    pub fn line_thickness(&self) -> f32 {
        self.thickness.max(1.0)
    }
}

/// The scheme a control falls back to when neither its own theme nor
/// [`ThemeData`] said -- the last step of the three.
pub fn scheme_of(context: &mut BuildContext) -> ColorScheme {
    ThemeData::of(context).color_scheme
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{Component, ElementTree, component, leaf};
    use crate::theme::MaterialTheme;
    use crate::widgets::SizedBox;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Builds `read` inside `tree` and hands back what it saw.
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

    #[test]
    fn an_installed_component_theme_wins_over_the_theme_datas_field() {
        // Nothing installed: the field on ThemeData, which starts empty.
        let bare = read_in(|child| child, DividerTheme::of);
        assert_eq!(bare, DividerThemeData::new());

        // Installed: that one.
        let installed = read_in(
            |child| DividerTheme::new(DividerThemeData::new().with_thickness(3.0), child),
            DividerTheme::of,
        );
        assert_eq!(installed.thickness, Some(3.0));
    }

    #[test]
    fn a_theme_datas_field_reaches_a_control_that_asks_for_it() {
        let data = ThemeData::light()
            .with_divider_theme(DividerThemeData::new().with_color(Color::argb(255, 1, 2, 3)));
        let seen = read_in(
            move |child| MaterialTheme::new(data, child),
            DividerTheme::of,
        );
        assert_eq!(seen.color, Some(Color::argb(255, 1, 2, 3)));
    }

    #[test]
    fn the_three_steps_resolve_in_upstreams_order() {
        // Step three: nothing set anywhere, so the divider takes
        // ThemeData::divider_color and upstream's own defaults.
        let plain = read_in(|child| child, ResolvedDivider::of);
        assert_eq!(plain.color, ThemeData::fallback().divider_color);
        assert_eq!(plain.space, 16.0, "upstream's default space");
        assert_eq!(plain.thickness, 0.0);
        assert_eq!(plain.line_thickness(), 1.0, "zero draws a hairline");

        // Step two: the ThemeData field.
        let data = ThemeData::light()
            .with_divider_theme(DividerThemeData::new().with_space(24.0).with_thickness(2.0));
        let installed = data.clone();
        let from_theme = read_in(
            move |child| MaterialTheme::new(installed, child),
            ResolvedDivider::of,
        );
        assert_eq!(from_theme.space, 24.0);
        assert_eq!(from_theme.line_thickness(), 2.0);

        // Step one: an installed DividerTheme, which beats both.
        let nearest = read_in(
            move |child| {
                MaterialTheme::new(
                    data,
                    DividerTheme::new(DividerThemeData::new().with_space(40.0), child),
                )
            },
            ResolvedDivider::of,
        );
        assert_eq!(nearest.space, 40.0);
        assert_eq!(
            nearest.thickness, 0.0,
            "the nearest theme replaces the whole data, as upstream's `of` does"
        );
    }

    #[test]
    fn lerping_a_component_theme_interpolates_what_it_can() {
        let a = DividerThemeData::new()
            .with_color(Color::argb(255, 0, 0, 0))
            .with_space(10.0);
        let b = DividerThemeData::new()
            .with_color(Color::argb(255, 255, 255, 255))
            .with_space(20.0);
        let half = DividerThemeData::lerp(&a, &b, 0.5);
        assert_eq!(half.color, Some(Color::argb(255, 128, 128, 128)));
        assert_eq!(half.space, Some(15.0));

        // One end unset: the number **grows from zero**, it does not change
        // over at the halfway point.
        //
        // This test used to assert the changeover, and explained it as what
        // `Color.lerp(null, colour, t)` comes to. That is not what `Color.lerp`
        // does either -- it scales the alpha -- and a number is not a colour
        // anyway: `lerpDouble` reads an absent end as `0.0` and interpolates.
        // A divider that one theme spaces and the other does not closes up
        // smoothly instead of snapping shut halfway through.
        let one_ended = DividerThemeData::lerp(&DividerThemeData::new(), &b, 0.4);
        assert_eq!(one_ended.space, Some(8.0), "two fifths of the way to 20");
        assert_eq!(
            DividerThemeData::lerp(&DividerThemeData::new(), &b, 0.6).space,
            Some(12.0)
        );
        assert_eq!(
            DividerThemeData::lerp(&DividerThemeData::new(), &b, 1.0).space,
            Some(20.0),
            "and arrives at the end it was going to"
        );

        // A colour, by contrast, fades: it is the alpha that scales.
        let faded = DividerThemeData::lerp(&DividerThemeData::new(), &b, 0.4);
        assert_eq!(faded.color.expect("fading in").alpha(), 102);
    }

    #[test]
    fn two_absent_numbers_stay_absent_rather_than_becoming_zero() {
        // `lerpDouble` returns `a` outright when `a == b`, which is what stops
        // two nulls interpolating between two zeroes and answering `Some(0.0)`
        // -- a theme with no elevation anywhere would otherwise acquire one of
        // exactly nothing, which is a different thing from having none.
        assert_eq!(lerp_f32(None, None, 0.5), None);

        // The same line also keeps two *equal* numbers exactly equal, which is
        // not free: `a * (1 - t) + a * t` is not `a` in floating point, and a
        // theme lerping against itself would drift in the last bits. Upstream
        // has the check for that and for NaN, which f32 comparison cannot see.
        assert_eq!(lerp_f32(Some(123.456), Some(123.456), 0.001), Some(123.456));
        assert_eq!(lerp_f32(Some(0.1), Some(0.1), 0.002), Some(0.1));
    }

    #[test]
    fn a_number_leaving_grows_down_to_nothing() {
        // The other direction: set at the start and absent at the end.
        assert_eq!(lerp_f32(Some(10.0), None, 0.25), Some(7.5));
        assert_eq!(lerp_f32(Some(10.0), None, 1.0), Some(0.0));
    }

    #[test]
    fn a_field_that_cannot_be_interpolated_still_changes_over_at_the_middle() {
        // `lerp_nearer` keeps the old rule and should: a shape or an enum has
        // no midpoint, so the only honest answer is one end or the other.
        assert_eq!(lerp_nearer(&Some("a"), &Some("b"), 0.4), Some("a"));
        assert_eq!(lerp_nearer(&Some("a"), &Some("b"), 0.6), Some("b"));
    }

    #[test]
    fn every_component_theme_starts_empty_and_says_nothing() {
        let theme = ThemeData::light();
        assert_eq!(theme.divider_theme, DividerThemeData::new());
        assert_eq!(theme.card_theme, CardThemeData::new());
        assert_eq!(
            theme.progress_indicator_theme,
            ProgressIndicatorThemeData::new()
        );
        assert_eq!(theme.badge_theme, BadgeThemeData::new());
        assert_eq!(theme.tooltip_theme, TooltipThemeData::new());
    }

    #[test]
    fn a_state_property_on_a_theme_resolves_against_the_control_s_states() {
        use crate::widget_state::{StateProperty, WidgetState, WidgetStates};

        let checked = WidgetStates::NONE.with(WidgetState::Selected);
        let disabled = WidgetStates::NONE.with(WidgetState::Disabled);

        // Upstream's own default shape for a fill: the primary when checked,
        // nothing when not.
        let plain = read_in(
            |child| child,
            move |context| ResolvedCheckbox::of(context, checked).fill,
        );
        assert_eq!(plain, ThemeData::fallback().color_scheme.primary);

        let unchecked = read_in(
            |child| child,
            move |context| ResolvedCheckbox::of(context, WidgetStates::NONE).fill,
        );
        assert_eq!(unchecked, Color::TRANSPARENT);

        // A disabled, checked box is the disabled colour rather than the
        // primary -- the states reach the default, not only the override.
        let off = read_in(
            |child| child,
            move |context| ResolvedCheckbox::of(context, checked.with(WidgetState::Disabled)).fill,
        );
        assert_ne!(off, ThemeData::fallback().color_scheme.primary);
        assert_eq!(off.alpha(), 0x61);

        // And an installed theme's property wins, resolved against the same
        // states.
        let themed = read_in(
            move |child| {
                CheckboxTheme::new(
                    CheckboxThemeData::new().with_fill_color(StateProperty::resolve_with(
                        |states: WidgetStates| {
                            if states.contains(WidgetState::Disabled) {
                                Some(Color::argb(255, 1, 1, 1))
                            } else {
                                Some(Color::argb(255, 2, 2, 2))
                            }
                        },
                    )),
                    child,
                )
            },
            move |context| ResolvedCheckbox::of(context, disabled).fill,
        );
        assert_eq!(themed, Color::argb(255, 1, 1, 1));
    }

    #[test]
    fn two_theme_datas_carrying_the_same_property_object_are_equal() {
        use crate::widget_state::StateProperty;

        // A theme is compared to decide whether its dependants rebuild, so a
        // property field has to have an equality. It is identity: the same
        // object is the same property.
        let property = StateProperty::all(Some(Color::argb(255, 5, 5, 5)));
        let first = CheckboxThemeData::new().with_fill_color(property.clone());
        let second = CheckboxThemeData::new().with_fill_color(property);
        assert_eq!(first, second);

        let rebuilt = CheckboxThemeData::new()
            .with_fill_color(StateProperty::all(Some(Color::argb(255, 5, 5, 5))));
        assert_ne!(
            first, rebuilt,
            "a freshly built resolver counts as changed, which is the safe way              round: a resolver may close over anything"
        );
    }

    #[test]
    fn a_tap_target_size_pads_a_control_out_to_the_minimum() {
        use crate::render::Size;
        use crate::widget_state::MaterialTapTargetSize;

        let drawn = Size::new(18.0, 18.0);
        assert_eq!(
            MaterialTapTargetSize::Padded.minimum_size(drawn),
            Size::new(48.0, 48.0)
        );
        assert_eq!(MaterialTapTargetSize::ShrinkWrap.minimum_size(drawn), drawn);
        assert_eq!(
            MaterialTapTargetSize::default(),
            MaterialTapTargetSize::Padded,
            "upstream's default is the accessible one"
        );
    }

    #[test]
    fn an_app_bar_resolves_its_surface_and_its_height() {
        // Nothing said: the scheme's surface, `onSurface` on top of it, and
        // upstream's `kToolbarHeight`.
        let plain = read_in(|child| child, ResolvedAppBar::of);
        let scheme = ThemeData::fallback().color_scheme;
        assert_eq!(plain.background, scheme.surface);
        assert_eq!(plain.foreground, scheme.on_surface);
        assert_eq!(plain.toolbar_height, 56.0);
        assert_eq!(plain.title_spacing, 16.0);
        assert!(!plain.center_title);

        let themed = read_in(
            |child| {
                AppBarTheme::new(
                    AppBarThemeData::new()
                        .with_background_color(Color::argb(255, 4, 4, 4))
                        .with_toolbar_height(72.0)
                        .with_center_title(true),
                    child,
                )
            },
            ResolvedAppBar::of,
        );
        assert_eq!(themed.background, Color::argb(255, 4, 4, 4));
        assert_eq!(themed.toolbar_height, 72.0);
        assert!(themed.center_title);
        assert_eq!(
            themed.foreground, scheme.on_surface,
            "a field the theme did not set still falls through to the scheme"
        );
    }

    #[test]
    fn an_app_bar_takes_a_themed_height_over_the_one_a_subtitle_would_ask_for() {
        use crate::components::AppBar;
        use crate::framework::ElementTree;
        use crate::render::{BoxConstraints, RenderBox};

        fn height_of(widget: AnyWidget) -> f32 {
            let mut tree = ElementTree::new();
            tree.rebuild(widget);
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::loose(400.0, 400.0)).height
        }

        // A bar with a subtitle is the taller of the crate's two heights...
        let with_subtitle = height_of(component(AppBar::new("Title").with_subtitle("Subtitle")));
        let plain = height_of(component(AppBar::new("Title")));
        assert!(with_subtitle > plain);

        // ...until a theme names one, which wins over both. The bar draws a
        // rule under itself, so the measured height is the toolbar plus that
        // -- the toolbar's own contribution is the difference between two
        // themed heights.
        let themed = |height: f32| {
            height_of(AppBarTheme::new(
                AppBarThemeData::new().with_toolbar_height(height),
                component(AppBar::new("Title").with_subtitle("Subtitle")),
            ))
        };
        assert_eq!(themed(90.0) - themed(60.0), 30.0);
        assert_eq!(
            themed(60.0) - plain,
            60.0 - 56.0,
            "and it replaces the height a subtitle would have asked for"
        );
    }

    #[test]
    fn a_list_tile_resolves_its_padding_gap_and_minimum_height() {
        let plain = read_in(
            |child| child,
            |context| ResolvedListTile::of(context, false, None),
        );
        assert_eq!(plain.min_tile_height, 56.0);
        assert_eq!(plain.horizontal_title_gap, 16.0);
        assert_eq!(plain.min_leading_width, 40.0);
        assert_eq!(
            plain.tile_color, None,
            "no fill unless a theme asked for one"
        );
        assert_eq!(
            plain.text_color,
            ThemeData::fallback().color_scheme.on_surface
        );

        // Dense drops the minimum height, as upstream's defaults do.
        let dense = read_in(
            |child| ListTileTheme::new(ListTileThemeData::new().with_dense(true), child),
            |context| ResolvedListTile::of(context, false, None),
        );
        assert_eq!(dense.min_tile_height, 48.0);

        // Selected reads a different pair of colours -- upstream keeps two
        // and picks by the flag rather than blending.
        let selected = read_in(
            |child| {
                ListTileTheme::new(
                    ListTileThemeData::new()
                        .with_tile_color(Color::argb(255, 1, 1, 1))
                        .with_selected_tile_color(Color::argb(255, 2, 2, 2)),
                    child,
                )
            },
            |context| ResolvedListTile::of(context, true, None),
        );
        assert_eq!(selected.tile_color, Some(Color::argb(255, 2, 2, 2)));
        assert_eq!(
            selected.text_color,
            ThemeData::fallback().color_scheme.primary,
            "a selected tile's text is the selected colour, which defaults to the primary"
        );
    }

    #[test]
    fn a_list_tile_is_at_least_as_tall_as_its_theme_says() {
        use crate::components::ListTile;
        use crate::framework::ElementTree;
        use crate::render::{BoxConstraints, RenderBox};

        fn height_of(widget: AnyWidget) -> f32 {
            let mut tree = ElementTree::new();
            tree.rebuild(widget);
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::loose(400.0, 400.0)).height
        }

        let plain = height_of(component(ListTile::new("Title")));
        assert!(plain >= 56.0, "upstream's minimum, {plain}");

        let taller = height_of(ListTileTheme::new(
            ListTileThemeData::new().with_min_tile_height(96.0),
            component(ListTile::new("Title")),
        ));
        assert_eq!(taller, 96.0);
    }

    #[test]
    fn a_dialog_theme_carries_the_barrier_and_the_inset() {
        let plain = read_in(|child| child, DialogTheme::of);
        assert_eq!(plain, DialogThemeData::new());

        let themed = read_in(
            |child| {
                DialogTheme::new(
                    DialogThemeData::new()
                        .with_barrier_color(Color::argb(128, 0, 0, 0))
                        .with_inset_padding(EdgeInsets::all(40.0)),
                    child,
                )
            },
            DialogTheme::of,
        );
        assert_eq!(themed.barrier_color, Some(Color::argb(128, 0, 0, 0)));
        assert_eq!(themed.inset_padding, Some(EdgeInsets::all(40.0)));
    }

    #[test]
    fn a_chip_resolves_its_fill_in_upstreams_order() {
        use crate::widget_state::{StateProperty, WidgetState, WidgetStates};

        let selected = WidgetStates::NONE.with(WidgetState::Selected);
        let default_fill = Color::argb(255, 9, 9, 9);

        // Nothing themed: the control's own default, which is the last step.
        let plain = read_in(
            |child| child,
            move |context| ResolvedChip::of(context, WidgetStates::NONE, default_fill).fill,
        );
        assert_eq!(plain, default_fill);

        // `backgroundColor` beats the control's default.
        let background = read_in(
            |child| {
                ChipTheme::new(
                    ChipThemeData::new().with_background_color(Color::argb(255, 1, 1, 1)),
                    child,
                )
            },
            move |context| ResolvedChip::of(context, WidgetStates::NONE, default_fill).fill,
        );
        assert_eq!(background, Color::argb(255, 1, 1, 1));

        // `selectedColor` beats `backgroundColor` when the chip is selected.
        let picked = read_in(
            |child| {
                ChipTheme::new(
                    ChipThemeData::new()
                        .with_background_color(Color::argb(255, 1, 1, 1))
                        .with_selected_color(Color::argb(255, 2, 2, 2)),
                    child,
                )
            },
            move |context| ResolvedChip::of(context, selected, default_fill).fill,
        );
        assert_eq!(picked, Color::argb(255, 2, 2, 2));

        // And `color` -- Material 3's state property -- beats all of them.
        let m3 = read_in(
            |child| {
                ChipTheme::new(
                    ChipThemeData::new()
                        .with_background_color(Color::argb(255, 1, 1, 1))
                        .with_selected_color(Color::argb(255, 2, 2, 2))
                        .with_color(StateProperty::all(Some(Color::argb(255, 3, 3, 3)))),
                    child,
                )
            },
            move |context| ResolvedChip::of(context, selected, default_fill).fill,
        );
        assert_eq!(m3, Color::argb(255, 3, 3, 3));
    }

    #[test]
    fn a_tab_bar_falls_through_to_the_scheme_role_by_role() {
        let scheme = ThemeData::fallback().color_scheme;
        let plain = read_in(|child| child, ResolvedTabBar::of);
        assert_eq!(plain.indicator_color, scheme.primary);
        assert_eq!(plain.label_color, scheme.primary);
        assert_eq!(plain.unselected_label_color, scheme.on_surface_variant());
        assert_eq!(plain.divider_color, scheme.outline_variant());
        assert_eq!(plain.divider_height, 1.0);
        assert_eq!(plain.indicator_size, TabBarIndicatorSize::Tab);

        let themed = read_in(
            |child| {
                TabBarTheme::new(
                    TabBarThemeData::new()
                        .with_indicator_color(Color::argb(255, 7, 7, 7))
                        .with_indicator_size(TabBarIndicatorSize::Label),
                    child,
                )
            },
            ResolvedTabBar::of,
        );
        assert_eq!(themed.indicator_color, Color::argb(255, 7, 7, 7));
        assert_eq!(themed.indicator_size, TabBarIndicatorSize::Label);
        assert_eq!(
            themed.label_color, scheme.primary,
            "an unset role still falls through"
        );
    }

    #[test]
    fn a_data_table_theme_carries_its_metrics() {
        let themed = read_in(
            |child| {
                DataTableTheme::new(
                    DataTableThemeData::new()
                        .with_column_spacing(48.0)
                        .with_horizontal_margin(12.0)
                        .with_data_row_heights(40.0, 60.0),
                    child,
                )
            },
            DataTableTheme::of,
        );
        assert_eq!(themed.column_spacing, Some(48.0));
        assert_eq!(themed.horizontal_margin, Some(12.0));
        assert_eq!(themed.data_row_min_height, Some(40.0));
        assert_eq!(themed.data_row_max_height, Some(60.0));
        assert_eq!(themed.heading_row_height, None);
    }

    #[test]
    fn a_drawer_takes_its_width_and_its_surface_from_its_theme() {
        let plain = read_in(|child| child, ResolvedDrawer::of);
        assert_eq!(plain.width, 304.0, "upstream's `_kWidth`");
        assert_eq!(plain.scrim, Color(0x8a000000), "black at 54 per cent");
        assert_eq!(
            plain.background,
            ThemeData::fallback().color_scheme.surface_container_low(),
            "`_DrawerDefaultsM3.backgroundColor`"
        );

        let themed = read_in(
            |child| {
                DrawerTheme::new(
                    DrawerThemeData::new()
                        .with_width(360.0)
                        .with_background_color(Color::argb(255, 3, 3, 3)),
                    child,
                )
            },
            ResolvedDrawer::of,
        );
        assert_eq!(themed.width, 360.0);
        assert_eq!(themed.background, Color::argb(255, 3, 3, 3));
    }

    #[test]
    fn a_drawer_widget_is_as_wide_as_its_theme_says() {
        use crate::drawer::Drawer;
        use crate::framework::ElementTree;
        use crate::render::{BoxConstraints, RenderBox};

        fn width_of(widget: AnyWidget) -> f32 {
            let mut tree = ElementTree::new();
            tree.rebuild(widget);
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::loose(1000.0, 600.0)).width
        }

        assert_eq!(
            width_of(component(Drawer::new(leaf(|| SizedBox::new(1.0, 1.0))))),
            304.0
        );

        assert_eq!(
            width_of(DrawerTheme::new(
                DrawerThemeData::new().with_width(360.0),
                component(Drawer::new(leaf(|| SizedBox::new(1.0, 1.0)))),
            )),
            360.0
        );

        // A width given to the widget outright still beats the theme, which
        // is upstream's order: the widget's own field is checked first.
        assert_eq!(
            width_of(DrawerTheme::new(
                DrawerThemeData::new().with_width(360.0),
                component(Drawer::new(leaf(|| SizedBox::new(1.0, 1.0))).with_width(200.0)),
            )),
            200.0
        );
    }

    #[test]
    fn the_navigation_themes_start_empty_and_carry_what_they_are_given() {
        let rail = read_in(
            |child| {
                NavigationRailTheme::new(
                    NavigationRailThemeData::new()
                        .with_label_type(NavigationRailLabelType::All)
                        .with_min_width(96.0),
                    child,
                )
            },
            NavigationRailTheme::of,
        );
        assert_eq!(rail.label_type, Some(NavigationRailLabelType::All));
        assert_eq!(rail.min_width, Some(96.0));
        assert_eq!(rail.elevation, None);

        let bar = read_in(
            |child| {
                BottomNavigationBarTheme::new(
                    BottomNavigationBarThemeData::new()
                        .with_item_colors(Color::argb(255, 1, 1, 1), Color::argb(255, 2, 2, 2))
                        .with_show_labels(true, false),
                    child,
                )
            },
            BottomNavigationBarTheme::of,
        );
        assert_eq!(bar.selected_item_color, Some(Color::argb(255, 1, 1, 1)));
        assert_eq!(bar.show_unselected_labels, Some(false));
    }

    #[test]
    fn a_button_style_merge_takes_this_ones_fields_first() {
        use crate::widget_state::StateProperty;

        let mine = ButtonStyle::new()
            .with_background_color(StateProperty::all(Some(Color::argb(255, 1, 1, 1))));
        let theirs = ButtonStyle::new()
            .with_background_color(StateProperty::all(Some(Color::argb(255, 2, 2, 2))))
            .with_foreground_color(StateProperty::all(Some(Color::argb(255, 3, 3, 3))));

        let merged = mine.merge(&theirs);
        assert_eq!(
            merged
                .background_color
                .as_ref()
                .expect("set on both")
                .resolve(WidgetStates::NONE),
            Some(Color::argb(255, 1, 1, 1)),
            "the receiver wins where both have a field"
        );
        assert_eq!(
            merged
                .foreground_color
                .as_ref()
                .expect("set on the other")
                .resolve(WidgetStates::NONE),
            Some(Color::argb(255, 3, 3, 3)),
            "and the other fills what the receiver left unset"
        );
        assert_eq!(merged.side, None, "neither had one");
    }

    #[test]
    fn a_button_reads_the_theme_its_variant_names() {
        use crate::components::ButtonVariant;
        use crate::widget_state::{StateProperty, WidgetState};

        let defaults = || ResolvedButton {
            background: Some(Color::argb(255, 9, 9, 9)),
            foreground: Color::argb(255, 8, 8, 8),
            side: None,
            padding: None,
            minimum_size: None,
        };

        // No theme: the control's own defaults, untouched.
        let plain = read_in(
            |child| child,
            move |context| {
                ResolvedButton::of(
                    context,
                    ButtonVariant::Filled,
                    WidgetStates::NONE,
                    defaults(),
                )
                .background
            },
        );
        assert_eq!(plain, Some(Color::argb(255, 9, 9, 9)));

        // A filled button reads the filled button theme...
        let filled = read_in(
            |child| {
                FilledButtonTheme::new(
                    FilledButtonThemeData::new().with_style(
                        ButtonStyle::new().with_background_color(StateProperty::all(Some(
                            Color::argb(255, 1, 1, 1),
                        ))),
                    ),
                    child,
                )
            },
            move |context| {
                ResolvedButton::of(
                    context,
                    ButtonVariant::Filled,
                    WidgetStates::NONE,
                    defaults(),
                )
                .background
            },
        );
        assert_eq!(filled, Some(Color::argb(255, 1, 1, 1)));

        // ...and an outlined one does not: it reads its own.
        let outlined = read_in(
            |child| {
                FilledButtonTheme::new(
                    FilledButtonThemeData::new().with_style(
                        ButtonStyle::new().with_background_color(StateProperty::all(Some(
                            Color::argb(255, 1, 1, 1),
                        ))),
                    ),
                    child,
                )
            },
            move |context| {
                ResolvedButton::of(
                    context,
                    ButtonVariant::Outlined,
                    WidgetStates::NONE,
                    defaults(),
                )
                .background
            },
        );
        assert_eq!(
            outlined,
            Some(Color::argb(255, 9, 9, 9)),
            "the filled button's theme says nothing about an outlined one"
        );

        // And the states reach the property.
        let disabled = read_in(
            |child| {
                TextButtonTheme::new(
                    TextButtonThemeData::new().with_style(
                        ButtonStyle::new().with_foreground_color(StateProperty::resolve_with(
                            |states: WidgetStates| {
                                if states.contains(WidgetState::Disabled) {
                                    Some(Color::argb(255, 5, 5, 5))
                                } else {
                                    Some(Color::argb(255, 6, 6, 6))
                                }
                            },
                        )),
                    ),
                    child,
                )
            },
            move |context| {
                ResolvedButton::of(
                    context,
                    ButtonVariant::Text,
                    WidgetStates::NONE.with(WidgetState::Disabled),
                    defaults(),
                )
                .foreground
            },
        );
        assert_eq!(disabled, Color::argb(255, 5, 5, 5));
    }

    #[test]
    fn a_button_widget_takes_a_themed_height() {
        use crate::components::{Button, ButtonVariant};
        use crate::framework::ElementTree;
        use crate::render::{BoxConstraints, RenderBox, Size};
        use crate::widget_state::StateProperty;

        fn height_of(widget: AnyWidget) -> f32 {
            let mut tree = ElementTree::new();
            tree.rebuild(widget);
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::loose(400.0, 400.0)).height
        }

        let plain = height_of(component(Button::new(1, "Go")));
        assert_eq!(plain, 40.0, "upstream's `Size(64, 40)` for a filled button");

        let taller = height_of(FilledButtonTheme::new(
            FilledButtonThemeData::new().with_style(
                ButtonStyle::new()
                    .with_minimum_size(StateProperty::all(Some(Size::new(64.0, 56.0)))),
            ),
            component(Button::new(1, "Go").with_style(ButtonVariant::Filled)),
        ));
        assert_eq!(taller, 56.0);
    }

    #[test]
    fn a_banner_takes_its_fill_and_its_rule_from_its_theme() {
        use crate::controls::Banner;
        use crate::framework::ElementTree;
        use crate::render::{BoxConstraints, EdgeInsets, RenderBox};

        fn height_of(widget: AnyWidget) -> f32 {
            let mut tree = ElementTree::new();
            tree.rebuild(widget);
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::loose(400.0, 400.0)).height
        }

        let plain = height_of(component(Banner::new("Something happened")));
        // A themed padding changes the banner's height by twice the change,
        // which is the observable half of the wiring.
        let padded = height_of(MaterialBannerTheme::new(
            MaterialBannerThemeData::new().with_padding(EdgeInsetsGeometry::Absolute(
                EdgeInsets::symmetric(0.0, 40.0),
            )),
            component(Banner::new("Something happened")),
        ));
        assert!(padded > plain, "{padded} should exceed {plain}");
        assert_eq!(padded - plain, 80.0 - 2.0 * 12.0);
    }

    #[test]
    fn the_material_two_button_theme_pads_by_its_text_theme() {
        // Upstream's `ButtonThemeData.padding` falls back differently for a
        // primary button than for a plain one -- twenty-four against sixteen.
        assert_eq!(
            ButtonThemeData::new().padding(),
            EdgeInsets::symmetric(16.0, 0.0)
        );
        assert_eq!(
            ButtonThemeData::new()
                .with_text_theme(ButtonTextTheme::Primary)
                .padding(),
            EdgeInsets::symmetric(24.0, 0.0)
        );
        // And a padding given outright wins over both.
        let mut given = ButtonThemeData::new().with_text_theme(ButtonTextTheme::Primary);
        given.padding = Some(EdgeInsetsGeometry::Absolute(EdgeInsets::all(4.0)));
        assert_eq!(given.padding(), EdgeInsets::all(4.0));

        // Upstream's defaults for the two metrics.
        assert_eq!(ButtonThemeData::new().min_width, 88.0);
        assert_eq!(ButtonThemeData::new().height, 36.0);
    }

    #[test]
    fn an_expansion_tile_theme_keeps_its_two_states_apart() {
        let themed = read_in(
            |child| {
                ExpansionTileTheme::new(
                    ExpansionTileThemeData::new()
                        .with_background_colors(
                            Color::argb(255, 1, 1, 1),
                            Color::argb(255, 2, 2, 2),
                        )
                        .with_text_colors(Color::argb(255, 3, 3, 3), Color::argb(255, 4, 4, 4)),
                    child,
                )
            },
            ExpansionTileTheme::of,
        );
        // Expanded and collapsed are separate fields, not two ends of an
        // interpolation: a tile that is open is a different tile.
        assert_eq!(themed.background_color, Some(Color::argb(255, 1, 1, 1)));
        assert_eq!(
            themed.collapsed_background_color,
            Some(Color::argb(255, 2, 2, 2))
        );
        assert_eq!(themed.text_color, Some(Color::argb(255, 3, 3, 3)));
        assert_eq!(themed.collapsed_text_color, Some(Color::argb(255, 4, 4, 4)));
    }

    #[test]
    fn a_scrollbar_resolves_its_thickness_and_its_thumb() {
        use crate::widget_state::{StateProperty, WidgetState, WidgetStates};

        let plain = read_in(
            |child| child,
            |context| ResolvedScrollbar::of(context, WidgetStates::NONE),
        );
        assert_eq!(plain.thickness, 8.0);
        assert_eq!(plain.min_thumb_length, 48.0);
        assert!(plain.interactive, "upstream's default is a draggable thumb");

        // The idle thumb is fainter than the dragged one -- upstream's M3
        // default, and the states reach it.
        let dragged = read_in(
            |child| child,
            |context| ResolvedScrollbar::of(context, WidgetStates::NONE.with(WidgetState::Dragged)),
        );
        assert_eq!(dragged.thumb_color.alpha(), 255);
        assert_eq!(plain.thumb_color.alpha(), 0x4d);

        let themed = read_in(
            |child| {
                ScrollbarTheme::new(
                    ScrollbarThemeData::new()
                        .with_thickness(StateProperty::all(Some(14.0)))
                        .with_min_thumb_length(20.0),
                    child,
                )
            },
            |context| ResolvedScrollbar::of(context, WidgetStates::NONE),
        );
        assert_eq!(themed.thickness, 14.0);
        assert_eq!(themed.min_thumb_length, 20.0);
    }

    #[test]
    fn a_menu_bar_theme_is_a_menu_theme_under_another_name() {
        // Upstream declares `MenuBarThemeData` as a `MenuThemeData` subclass
        // with no fields of its own, so that the bar and the menus hanging
        // off it can be themed apart. Two types, one shape.
        let style = MenuStyle::new().with_alignment(AlignmentGeometry::CENTER);
        let bar = MenuBarThemeData::new().with_style(style.clone());
        let menu = MenuThemeData::new().with_style(style);
        assert_eq!(bar.style, menu.style);

        let installed = read_in(
            |child| {
                MenuTheme::new(
                    MenuThemeData::new()
                        .with_style(MenuStyle::new().with_alignment(AlignmentGeometry::CENTER)),
                    child,
                )
            },
            MenuTheme::of,
        );
        assert!(installed.style.is_some());
        // And installing one does not install the other.
        let bar_of = read_in(
            |child| MenuTheme::new(MenuThemeData::new().with_style(MenuStyle::new()), child),
            MenuBarTheme::of,
        );
        assert_eq!(bar_of, MenuBarThemeData::new());
    }

    #[test]
    fn the_style_only_themes_carry_a_button_style() {
        use crate::widget_state::StateProperty;

        let style = ButtonStyle::new()
            .with_background_color(StateProperty::all(Some(Color::argb(255, 6, 6, 6))));
        let segmented = read_in(
            |child| {
                SegmentedButtonTheme::new(
                    SegmentedButtonThemeData::new().with_style(style.clone()),
                    child,
                )
            },
            SegmentedButtonTheme::of,
        );
        assert_eq!(segmented.style, Some(style.clone()));

        let menu_button = read_in(
            |child| {
                MenuButtonTheme::new(MenuButtonThemeData::new().with_style(style.clone()), child)
            },
            MenuButtonTheme::of,
        );
        assert_eq!(menu_button.style, Some(style));
    }

    #[test]
    fn a_floating_action_button_picks_one_elevation_rather_than_blending() {
        use crate::widget_state::{WidgetState, WidgetStates};

        let scheme = ThemeData::fallback().color_scheme;
        let resting = read_in(
            |child| child,
            |context| ResolvedFloatingActionButton::of(context, WidgetStates::NONE),
        );
        assert_eq!(resting.elevation, 6.0, "upstream's `_defaultElevation`");
        assert_eq!(resting.background, scheme.primary_container());
        assert_eq!(resting.foreground, scheme.on_primary_container());
        assert_eq!(resting.size.max_width, 56.0);

        // Held down: the highlight elevation, not the resting one.
        let held = read_in(
            |child| child,
            |context| {
                ResolvedFloatingActionButton::of(
                    context,
                    WidgetStates::NONE.with(WidgetState::Pressed),
                )
            },
        );
        assert_eq!(held.elevation, 12.0);

        // A theme names the resting elevation; hover falls back to it,
        // because upstream's `hoverElevation` is a field of its own and an
        // unset one means "the resting one" rather than "some blend".
        let themed = read_in(
            |child| {
                FloatingActionButtonTheme::new(
                    FloatingActionButtonThemeData::new().with_elevation(2.0),
                    child,
                )
            },
            |context| {
                ResolvedFloatingActionButton::of(
                    context,
                    WidgetStates::NONE.with(WidgetState::Hovered),
                )
            },
        );
        assert_eq!(themed.elevation, 2.0);
    }

    #[test]
    fn a_toggle_buttons_theme_keeps_its_three_label_colours_apart() {
        let themed = read_in(
            |child| {
                ToggleButtonsTheme::new(
                    ToggleButtonsThemeData::new()
                        .with_colors(Color::argb(255, 1, 1, 1), Color::argb(255, 2, 2, 2))
                        .with_border(Color::argb(255, 3, 3, 3), 2.0),
                    child,
                )
            },
            ToggleButtonsTheme::of,
        );
        assert_eq!(themed.color, Some(Color::argb(255, 1, 1, 1)));
        assert_eq!(themed.selected_color, Some(Color::argb(255, 2, 2, 2)));
        assert_eq!(themed.disabled_color, None, "a third field, and unset");
        assert_eq!(themed.border_width, Some(2.0));
    }

    #[test]
    fn the_search_bar_is_stateful_and_the_view_it_opens_is_not() {
        use crate::widget_state::{StateProperty, WidgetState, WidgetStates};

        // The bar's fields are state properties -- it is a control a pointer
        // touches.
        let bar = read_in(
            |child| {
                SearchBarTheme::new(
                    SearchBarThemeData::new().with_background_color(StateProperty::resolve_with(
                        |states: WidgetStates| {
                            if states.contains(WidgetState::Hovered) {
                                Some(Color::argb(255, 1, 1, 1))
                            } else {
                                Some(Color::argb(255, 2, 2, 2))
                            }
                        },
                    )),
                    child,
                )
            },
            SearchBarTheme::of,
        );
        let property = bar.background_color.expect("set");
        assert_eq!(
            property.resolve(WidgetStates::NONE.with(WidgetState::Hovered)),
            Some(Color::argb(255, 1, 1, 1))
        );
        assert_eq!(
            property.resolve(WidgetStates::NONE),
            Some(Color::argb(255, 2, 2, 2))
        );

        // The view's are plain values -- it is open or it is not there, and
        // there is no hovered state for a panel.
        let view = read_in(
            |child| {
                SearchViewTheme::new(
                    SearchViewThemeData::new()
                        .with_background_color(Color::argb(255, 3, 3, 3))
                        .with_header_height(72.0)
                        .with_shrink_wrap(true),
                    child,
                )
            },
            SearchViewTheme::of,
        );
        assert_eq!(view.background_color, Some(Color::argb(255, 3, 3, 3)));
        assert_eq!(view.header_height, Some(72.0));
        assert_eq!(view.shrink_wrap, Some(true));
        assert_eq!(view.divider_color, None);
    }

    #[test]
    fn a_time_picker_themes_its_three_parts_apart() {
        let themed = read_in(
            |child| {
                TimePickerTheme::new(
                    TimePickerThemeData::new()
                        .with_background_color(Color::argb(255, 1, 1, 1))
                        .with_dial_colors(Color::argb(255, 2, 2, 2), Color::argb(255, 3, 3, 3))
                        .with_hour_minute_color(Color::argb(255, 4, 4, 4)),
                    child,
                )
            },
            TimePickerTheme::of,
        );
        // The dialog, the dial and the hour-minute fields are three separate
        // things that happen to share a box; upstream themes each of them on
        // its own and so does this.
        assert_eq!(themed.background_color, Some(Color::argb(255, 1, 1, 1)));
        assert_eq!(
            themed.dial_background_color,
            Some(Color::argb(255, 2, 2, 2))
        );
        assert_eq!(themed.dial_hand_color, Some(Color::argb(255, 3, 3, 3)));
        assert_eq!(themed.hour_minute_color, Some(Color::argb(255, 4, 4, 4)));
        // And the AM/PM toggle is a fourth, still unset here.
        assert_eq!(themed.day_period_color, None);
    }

    #[test]
    fn a_date_picker_themes_the_range_picker_separately_from_the_dialog() {
        let mut data = DatePickerThemeData::new()
            .with_background_color(Color::argb(255, 1, 1, 1))
            .with_header_colors(Color::argb(255, 2, 2, 2), Color::argb(255, 3, 3, 3));
        data.range_picker_background_color = Some(Color::argb(255, 4, 4, 4));

        let themed = read_in(
            move |child| DatePickerTheme::new(data.clone(), child),
            DatePickerTheme::of,
        );
        // Upstream keeps a second copy of every dialog field for the range
        // picker, because a range picker is a full-screen page rather than a
        // dialog and does not want the dialog's paint. Setting one leaves the
        // other alone.
        assert_eq!(themed.background_color, Some(Color::argb(255, 1, 1, 1)));
        assert_eq!(
            themed.range_picker_background_color,
            Some(Color::argb(255, 4, 4, 4))
        );
        assert_eq!(
            themed.range_picker_header_background_color, None,
            "the dialog's header colour does not reach the range picker's"
        );
    }

    #[test]
    fn an_input_decoration_picks_one_of_its_five_borders_in_upstreams_order() {
        use crate::borders::{BorderSide, OutlineInputBorder, ShapeBorder, UnderlineInputBorder};
        use crate::widget_state::{WidgetState, WidgetStates};

        let side = |width: f32| BorderSide {
            color: Color::BLACK,
            width,
            ..BorderSide::NONE
        };
        let data = InputDecorationThemeData::new()
            .with_border(ShapeBorder::Underline(UnderlineInputBorder::new(side(1.0))))
            .with_focused_border(ShapeBorder::Outline(OutlineInputBorder::new(side(2.0))))
            .with_error_border(ShapeBorder::Outline(OutlineInputBorder::new(side(3.0))));

        let width_of =
            |border: Option<ShapeBorder>| border.and_then(|b| b.outlined_side()).map(|s| s.width);

        // Nothing set for the enabled state, so it falls through to `border`.
        assert_eq!(width_of(data.resolve_border(WidgetStates::NONE)), Some(1.0));
        assert_eq!(
            width_of(data.resolve_border(WidgetStates::NONE.with(WidgetState::Focused))),
            Some(2.0)
        );
        assert_eq!(
            width_of(data.resolve_border(WidgetStates::NONE.with(WidgetState::Error))),
            Some(3.0)
        );

        // Focused *and* in error asks for the focused-error border, which is
        // unset here -- so it falls to `border` rather than to either parent.
        // That is upstream's order, and the case a reader guesses wrong.
        assert_eq!(
            width_of(
                data.resolve_border(
                    WidgetStates::NONE
                        .with(WidgetState::Error)
                        .with(WidgetState::Focused)
                )
            ),
            Some(1.0)
        );

        // Disabled wins over everything, error included.
        assert_eq!(
            width_of(
                data.resolve_border(
                    WidgetStates::NONE
                        .with(WidgetState::Disabled)
                        .with(WidgetState::Error)
                )
            ),
            Some(1.0)
        );
    }

    #[test]
    fn the_floating_label_defaults_are_upstreams() {
        let data = InputDecorationThemeData::new();
        assert_eq!(data.floating_label_behavior, FloatingLabelBehavior::Auto);
        assert_eq!(data.floating_label_alignment, FloatingLabelAlignment::Start);
        assert_eq!(FloatingLabelAlignment::Start.x(), -1.0);
        assert_eq!(FloatingLabelAlignment::Center.x(), 0.0);
        assert!(!data.filled);
        assert!(!data.is_dense);
        assert!(!data.is_collapsed);
        assert!(!data.align_label_with_hint);
    }

    #[test]
    fn an_icon_theme_clamps_the_opacity_on_the_way_out_not_in() {
        // Upstream stores whatever it was given and clamps in the getter, so
        // a merge or a lerp sees the stored value and a painter sees 0..1.
        let over = IconThemeData::new().with_opacity(1.7);
        assert_eq!(over.opacity(), Some(1.0));
        let under = IconThemeData::new().with_opacity(-0.2);
        assert_eq!(under.opacity(), Some(0.0));
        assert_eq!(IconThemeData::new().opacity(), None);
    }

    #[test]
    fn an_icon_theme_merges_the_receivers_fields_first() {
        let mine = IconThemeData::new().with_size(24.0);
        let theirs = IconThemeData::new()
            .with_size(48.0)
            .with_color(Color::argb(255, 1, 1, 1));
        let merged = mine.merge(&theirs);
        assert_eq!(merged.size, Some(24.0));
        assert_eq!(merged.color, Some(Color::argb(255, 1, 1, 1)));
    }

    #[test]
    fn the_component_themes_that_were_waiting_on_an_icon_theme_have_one_now() {
        // Four component themes had these fields left out because the type
        // did not exist. They carry them now, and the leading and trailing
        // ones stay apart -- upstream keeps two because an app bar's back
        // arrow and its actions are often different weights.
        let bar = read_in(
            |child| {
                AppBarTheme::new(
                    AppBarThemeData {
                        icon_theme: Some(IconThemeData::new().with_size(20.0)),
                        ..AppBarThemeData::new()
                    },
                    child,
                )
            },
            AppBarTheme::of,
        );
        assert_eq!(bar.icon_theme.and_then(|theme| theme.size), Some(20.0));
        assert_eq!(bar.actions_icon_theme, None);

        assert_eq!(ChipThemeData::new().icon_theme, None);
        assert_eq!(NavigationRailThemeData::new().selected_icon_theme, None);
        assert_eq!(
            BottomNavigationBarThemeData::new().unselected_icon_theme,
            None
        );
    }

    #[test]
    fn a_text_selection_theme_carries_its_three_colours() {
        let themed = read_in(
            |child| {
                TextSelectionTheme::new(
                    TextSelectionThemeData::new()
                        .with_cursor_color(Color::argb(255, 1, 1, 1))
                        .with_selection_color(Color::argb(255, 2, 2, 2)),
                    child,
                )
            },
            TextSelectionTheme::of,
        );
        assert_eq!(themed.cursor_color, Some(Color::argb(255, 1, 1, 1)));
        assert_eq!(themed.selection_color, Some(Color::argb(255, 2, 2, 2)));
        assert_eq!(themed.selection_handle_color, None);
    }

    #[test]
    fn a_dropdown_menu_theme_carries_all_three_of_its_parts() {
        use crate::widget_state::StateProperty;

        let themed = read_in(
            |child| {
                DropdownMenuTheme::new(
                    DropdownMenuThemeData::new()
                        .with_menu_style(MenuStyle::new().with_background_color(
                            StateProperty::all(Some(Color::argb(255, 1, 1, 1))),
                        ))
                        .with_input_decoration_theme(
                            InputDecorationThemeData::new().with_dense(true),
                        ),
                    child,
                )
            },
            DropdownMenuTheme::of,
        );
        // A dropdown is a field and a menu, and upstream themes each with the
        // type that already exists for it rather than inventing a third.
        assert!(themed.menu_style.is_some());
        assert!(themed.input_decoration_theme.expect("set").is_dense);
        assert_eq!(themed.disabled_color, None);
    }

    #[test]
    fn a_popup_menu_position_defaults_to_covering_its_button() {
        assert_eq!(PopupMenuPosition::default(), PopupMenuPosition::Over);

        let themed = read_in(
            |child| {
                PopupMenuTheme::new(
                    PopupMenuThemeData::new()
                        .with_position(PopupMenuPosition::Under)
                        .with_elevation(8.0),
                    child,
                )
            },
            PopupMenuTheme::of,
        );
        assert_eq!(themed.position, Some(PopupMenuPosition::Under));
        assert_eq!(themed.elevation, Some(8.0));
    }

    #[test]
    fn a_bottom_app_bar_theme_is_the_one_that_carries_a_notch() {
        use crate::borders::NotchedShape;

        let themed = read_in(
            |child| {
                BottomAppBarTheme::new(
                    BottomAppBarThemeData::new()
                        .with_height(80.0)
                        .with_shape(NotchedShape::Circular { inverted: false }),
                    child,
                )
            },
            BottomAppBarTheme::of,
        );
        assert_eq!(themed.height, Some(80.0));
        assert!(matches!(themed.shape, Some(NotchedShape::Circular { .. })));
    }

    #[test]
    fn the_two_bottom_bars_have_two_themes_and_two_shapes() {
        use crate::widget_state::{StateProperty, WidgetState, WidgetStates};

        // Material 2's bar keeps two icon-theme fields, one per state;
        // Material 3's keeps one property resolved against the states. They
        // are different widgets and upstream themes them apart, so setting
        // one says nothing about the other.
        let m3 = read_in(
            |child| {
                NavigationBarTheme::new(
                    NavigationBarThemeData {
                        icon_theme: Some(StateProperty::resolve_with(|states: WidgetStates| {
                            Some(IconThemeData::new().with_size(
                                if states.contains(WidgetState::Selected) {
                                    28.0
                                } else {
                                    24.0
                                },
                            ))
                        })),
                        ..NavigationBarThemeData::new().with_height(72.0)
                    },
                    child,
                )
            },
            NavigationBarTheme::of,
        );
        assert_eq!(m3.height, Some(72.0));
        let property = m3.icon_theme.expect("set");
        assert_eq!(
            property
                .resolve(WidgetStates::NONE.with(WidgetState::Selected))
                .and_then(|theme| theme.size),
            Some(28.0)
        );
        assert_eq!(
            property.resolve(WidgetStates::NONE).and_then(|t| t.size),
            Some(24.0)
        );

        // And the Material 2 bar's theme is untouched by it.
        let m2 = read_in(
            |child| NavigationBarTheme::new(NavigationBarThemeData::new().with_height(72.0), child),
            BottomNavigationBarTheme::of,
        );
        assert_eq!(m2, BottomNavigationBarThemeData::new());
    }

    #[test]
    fn a_navigation_drawers_indicator_has_a_size_of_its_own() {
        use crate::render::Size;

        let themed = read_in(
            |child| {
                NavigationDrawerTheme::new(
                    NavigationDrawerThemeData::new()
                        .with_tile_height(56.0)
                        .with_indicator_size(Size::new(336.0, 56.0)),
                    child,
                )
            },
            NavigationDrawerTheme::of,
        );
        assert_eq!(themed.tile_height, Some(56.0));
        assert_eq!(themed.indicator_size, Some(Size::new(336.0, 56.0)));

        // Halfway between two sizes is the size halfway between them, which
        // is the one field here that interpolates rather than switching over.
        let wider = NavigationDrawerThemeData::new().with_indicator_size(Size::new(436.0, 56.0));
        let half = NavigationDrawerThemeData::lerp(&themed, &wider, 0.5);
        assert_eq!(half.indicator_size, Some(Size::new(386.0, 56.0)));
    }

    #[test]
    fn a_carousel_theme_carries_its_padding_and_its_fill() {
        use crate::render::EdgeInsets;

        let themed = read_in(
            |child| {
                CarouselViewTheme::new(
                    CarouselViewThemeData::new()
                        .with_padding(EdgeInsets::all(8.0))
                        .with_background_color(Color::argb(255, 5, 5, 5)),
                    child,
                )
            },
            CarouselViewTheme::of,
        );
        assert_eq!(themed.padding, Some(EdgeInsets::all(8.0)));
        assert_eq!(themed.background_color, Some(Color::argb(255, 5, 5, 5)));
    }

    #[test]
    fn the_material_three_type_scale_is_upstreams() {
        let english = Typography::english_like();
        // Spot-checks against `_M3Typography.englishLike`, one per family,
        // because the generator is only as trustworthy as a value read back
        // from the other side.
        let display = english.display_large.expect("set");
        assert_eq!(display.font_size, 57.0);
        assert_eq!(display.font_weight, 400);
        assert_eq!(display.letter_spacing, Some(-0.25));

        let title = english.title_medium.expect("set");
        assert_eq!(title.font_size, 16.0);
        assert_eq!(title.font_weight, 500);

        let label = english.label_small.expect("set");
        assert_eq!(label.font_size, 11.0);
        assert_eq!(label.height, Some(1.45));

        // Every one of the fifteen is filled in, in all three geometries.
        for theme in [
            Typography::english_like(),
            Typography::dense(),
            Typography::tall(),
        ] {
            assert!(theme.display_large.is_some());
            assert!(theme.body_medium.is_some());
            assert!(theme.label_small.is_some());
        }
    }

    #[test]
    fn the_material_three_geometries_carry_the_same_numbers() {
        // This is upstream's doing and worth pinning. Material 2's three
        // geometries differed in size and line height; Material 3's differ in
        // `textBaseline` alone -- `dense` is ideographic, the other two are
        // alphabetic -- and `englishLike` and `tall` are identical
        // throughout. This port's `TextStyle` carries no baseline, so all
        // three answer the same table; the line is here so that a reader who
        // expects three different tables learns why there is one.
        let english = Typography::english_like();
        let dense = Typography::dense();
        let tall = Typography::tall();
        assert_eq!(english.body_medium, dense.body_medium);
        assert_eq!(english.body_medium, tall.body_medium);
        assert_eq!(english.display_large, tall.display_large);
    }

    #[test]
    fn a_text_theme_merges_and_recolours() {
        use crate::engine::TextStyle;

        let mine = TextTheme {
            body_medium: Some(TextStyle {
                font_size: 99.0,
                ..TextStyle::default()
            }),
            ..TextTheme::new()
        };
        let merged = mine.merge(&Typography::english_like());
        assert_eq!(merged.body_medium.expect("set").font_size, 99.0);
        assert_eq!(
            merged.display_large.expect("from the other").font_size,
            57.0
        );

        // `apply_color` reaches every style that is set and leaves the unset
        // ones unset -- a theme with three styles does not gain twelve.
        let recoloured = mine.apply_color(Color::argb(255, 1, 2, 3));
        assert_eq!(
            recoloured.body_medium.expect("set").color,
            Color::argb(255, 1, 2, 3)
        );
        assert!(recoloured.display_large.is_none());
    }

    #[test]
    fn a_theme_data_builds_its_two_text_themes_from_the_scheme() {
        let light = ThemeData::light();
        assert_eq!(
            light.text_theme.body_medium.expect("built").color,
            light.color_scheme.on_surface
        );
        // The primary text theme is what reads on `primaryColor`, which in a
        // light theme is the primary itself.
        assert_eq!(
            light.primary_text_theme.body_medium.expect("built").color,
            light.color_scheme.on_primary
        );
        // And in a dark theme the bars take the surface, so the text on them
        // is `onSurface` rather than `onPrimary`.
        let dark = ThemeData::dark();
        assert_eq!(
            dark.primary_text_theme.body_medium.expect("built").color,
            dark.color_scheme.on_surface
        );
    }

    #[test]
    fn a_theme_extension_is_found_by_its_type_and_lerped_with_its_own_kind() {
        use std::rc::Rc;

        #[derive(Clone, Debug, PartialEq)]
        struct Brand {
            accent: Color,
        }

        impl ThemeExtension for Brand {
            fn lerp(&self, other: &dyn ThemeExtension, t: f32) -> Rc<dyn ThemeExtension> {
                match other.as_any().downcast_ref::<Brand>() {
                    Some(other) => Rc::new(Brand {
                        accent: crate::animation::ColorTween {
                            begin: self.accent,
                            end: other.accent,
                        }
                        .lerp(t),
                    }),
                    // A different extension: keep this one, which is what
                    // upstream's covariant parameter comes to at runtime.
                    None => Rc::new(self.clone()),
                }
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        #[derive(Clone, Debug)]
        struct Other;

        impl ThemeExtension for Other {
            fn lerp(&self, _other: &dyn ThemeExtension, _t: f32) -> Rc<dyn ThemeExtension> {
                Rc::new(Other)
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let theme = ThemeData::light().with_extension(Brand {
            accent: Color::argb(255, 0, 0, 0),
        });
        assert_eq!(
            theme.extension::<Brand>().expect("stored").accent,
            Color::argb(255, 0, 0, 0)
        );
        assert!(theme.extension::<Other>().is_none(), "keyed by type");

        // Lerped with the extension of its own type on the other side.
        let darker = ThemeData::light().with_extension(Brand {
            accent: Color::argb(255, 255, 255, 255),
        });
        let half = ThemeData::lerp(&theme, &darker, 0.5);
        assert_eq!(
            half.extension::<Brand>().expect("kept").accent,
            Color::argb(255, 128, 128, 128)
        );

        // An extension the other end does not carry survives the walk: a
        // theme that does not mention it has not removed it.
        let bare = ThemeData::light();
        let still_there = ThemeData::lerp(&theme, &bare, 0.5);
        assert!(still_there.extension::<Brand>().is_some());
    }

    #[test]
    fn a_button_bar_theme_carries_the_material_two_metrics() {
        use crate::render::{MainAxisAlignment, VerticalDirection};

        let themed = read_in(
            |child| {
                ButtonBarTheme::new(
                    ButtonBarThemeData::new()
                        .with_alignment(MainAxisAlignment::End)
                        .with_button_metrics(64.0, 36.0)
                        .with_overflow_direction(VerticalDirection::Up),
                    child,
                )
            },
            ButtonBarTheme::of,
        );
        assert_eq!(themed.alignment, Some(MainAxisAlignment::End));
        assert_eq!(themed.button_min_width, Some(64.0));
        assert_eq!(themed.overflow_direction, Some(VerticalDirection::Up));
    }
}

#[cfg(test)]
mod merge_direction_tests {
    use super::*;

    /// Two data values with *every* field set, so that each `or` has something
    /// on both sides and its direction is visible.
    fn near() -> IconThemeData {
        let mut data = IconThemeData::new();
        data.size = Some(1.0);
        data.fill = Some(0.1);
        data.weight = Some(100.0);
        data.grade = Some(10.0);
        data.optical_size = Some(11.0);
        data.color = Some(Color::argb(0xFF, 1, 1, 1));
        data.apply_text_scaling = Some(true);
        data.with_opacity(0.25)
    }

    fn far() -> IconThemeData {
        let mut data = IconThemeData::new();
        data.size = Some(2.0);
        data.fill = Some(0.2);
        data.weight = Some(200.0);
        data.grade = Some(20.0);
        data.optical_size = Some(22.0);
        data.color = Some(Color::argb(0xFF, 2, 2, 2));
        data.apply_text_scaling = Some(false);
        data.with_opacity(0.75)
    }

    #[test]
    fn a_nearer_icon_theme_wins_every_field_and_not_just_the_first() {
        // Which is the whole reason themes nest. Written with both sides fully
        // set, because a field set on one side only cannot show a direction --
        // that is what let eight of these go untested.
        let merged = near().merge(&far());
        assert_eq!(merged.size, Some(1.0));
        assert_eq!(merged.fill, Some(0.1));
        assert_eq!(merged.weight, Some(100.0));
        assert_eq!(merged.grade, Some(10.0));
        assert_eq!(merged.optical_size, Some(11.0));
        assert_eq!(merged.color, Some(Color::argb(0xFF, 1, 1, 1)));
        assert_eq!(merged.apply_text_scaling, Some(true));
        assert_eq!(merged.opacity(), Some(0.25));
    }

    #[test]
    fn merging_the_other_way_round_gives_the_other_answer_everywhere() {
        // The pair of the test above: if either were passing by accident the
        // two would agree.
        let merged = far().merge(&near());
        assert_eq!(merged.size, Some(2.0));
        assert_eq!(merged.fill, Some(0.2));
        assert_eq!(merged.weight, Some(200.0));
        assert_eq!(merged.grade, Some(20.0));
        assert_eq!(merged.optical_size, Some(22.0));
        assert_eq!(merged.color, Some(Color::argb(0xFF, 2, 2, 2)));
        assert_eq!(merged.apply_text_scaling, Some(false));
        assert_eq!(merged.opacity(), Some(0.75));
    }

    #[test]
    fn a_field_only_the_far_theme_has_still_comes_through() {
        let mut sparse = IconThemeData::new();
        sparse.size = Some(1.0);
        let merged = sparse.merge(&far());
        assert_eq!(merged.size, Some(1.0), "its own");
        assert_eq!(merged.weight, Some(200.0), "and the other's for the rest");
    }
}
