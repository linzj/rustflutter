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
use crate::render::{AlignmentGeometry, BoxConstraints, Offset, Size};
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
}
