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

/// Interpolates two optional colours, as every `*ThemeData.lerp` upstream
/// does through `Color.lerp`: a null end is a null answer before the halfway
/// point and the other end's colour after it.
fn lerp_color(a: Option<Color>, b: Option<Color>, t: f32) -> Option<Color> {
    match (a, b) {
        (None, None) => None,
        (Some(a), Some(b)) => Some(crate::animation::ColorTween { begin: a, end: b }.lerp(t)),
        _ => {
            if t < 0.5 {
                a
            } else {
                b
            }
        }
    }
}

/// The same for a number.
fn lerp_f32(a: Option<f32>, b: Option<f32>, t: f32) -> Option<f32> {
    match (a, b) {
        (None, None) => None,
        (Some(a), Some(b)) => Some(a + (b - a) * t),
        _ => {
            if t < 0.5 {
                a
            } else {
                b
            }
        }
    }
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

/// Anything else: taken from whichever end is nearer, which is what
/// upstream's `lerp` does for the fields it cannot interpolate.
fn lerp_nearer<T: Clone>(a: &Option<T>, b: &Option<T>, t: f32) -> Option<T> {
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

// -- App bar (upstream `app_bar_theme.dart`) ----------------------------------

/// Upstream `AppBarThemeData`.
///
/// `iconTheme`, `actionsIconTheme` and `systemOverlayStyle` are not here:
/// the first two are an `IconThemeData` and the framework has no icon system
/// yet (`E5`), and the third is a `SystemUiOverlayStyle`, which is the
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

/// Upstream `ListTileControlAffinity`: which end a tile's control sits at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ListTileControlAffinity {
    Leading,
    Trailing,
    /// Whatever the platform does -- upstream resolves this per target.
    #[default]
    Platform,
}

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

    pub fn of(context: &mut BuildContext, selected: bool) -> ResolvedListTile {
        let data = ListTileTheme::of(context);
        let theme = ThemeData::of(context);
        let dense = data.dense.unwrap_or(false);
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
///
/// `iconTheme` is not here: it is an `IconThemeData`, and the framework has
/// no icon system yet (`E5`).
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
}

impl ResolvedTabBar {
    /// Upstream's Material 3 default indicator thickness.
    pub const INDICATOR_WEIGHT: f32 = 3.0;
    /// Upstream's default divider height.
    pub const DIVIDER_HEIGHT: f32 = 1.0;

    pub fn of(context: &mut BuildContext) -> ResolvedTabBar {
        let data = TabBarTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        ResolvedTabBar {
            indicator_color: data.indicator_color.unwrap_or(scheme.primary),
            label_color: data.label_color.unwrap_or(scheme.primary),
            unselected_label_color: data
                .unselected_label_color
                .unwrap_or(scheme.on_surface_variant()),
            divider_color: data.divider_color.unwrap_or(scheme.outline_variant()),
            divider_height: data
                .divider_height
                .unwrap_or(ResolvedTabBar::DIVIDER_HEIGHT),
            // Upstream's Material 3 default is `TabBarIndicatorSize.tab`.
            indicator_size: data.indicator_size.unwrap_or(TabBarIndicatorSize::Tab),
        }
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
///
/// The two `IconThemeData` fields are not here: the framework has no icon
/// system yet (`E5`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavigationRailThemeData {
    pub background_color: Option<Color>,
    pub elevation: Option<f32>,
    pub unselected_label_text_style: Option<TextStyle>,
    pub selected_label_text_style: Option<TextStyle>,
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

/// Upstream `BottomNavigationBarType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomNavigationBarType {
    /// Every destination the same width, all labels shown.
    Fixed,
    /// The selected destination grows and the others shrink.
    Shifting,
}

/// Upstream `BottomNavigationBarLandscapeLayout`: how the destinations are
/// arranged when the bar is wider than it is tall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomNavigationBarLandscapeLayout {
    /// Spread across the whole bar.
    Spread,
    /// Grouped in the middle.
    Centered,
    /// Icon and label side by side rather than stacked.
    Linear,
}

/// Upstream `BottomNavigationBarThemeData`.
///
/// The two `IconThemeData` fields are not here (`E5`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BottomNavigationBarThemeData {
    pub background_color: Option<Color>,
    pub elevation: Option<f32>,
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

        // One end unset: the answer changes over at the halfway point, which
        // is what `Color.lerp(null, colour, t)` comes to for a field a theme
        // either has or does not.
        let one_ended = DividerThemeData::lerp(&DividerThemeData::new(), &b, 0.4);
        assert_eq!(one_ended.space, None);
        assert_eq!(
            DividerThemeData::lerp(&DividerThemeData::new(), &b, 0.6).space,
            Some(20.0)
        );
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
            |context| ResolvedListTile::of(context, false),
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
            |context| ResolvedListTile::of(context, false),
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
            |context| ResolvedListTile::of(context, true),
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
}
