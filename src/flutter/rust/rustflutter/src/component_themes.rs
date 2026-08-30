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
use crate::framework::{AnyWidget, BuildContext, provide_theme};
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

/// Upstream `EdgeInsets.lerp`, which has the same two null arms as
/// [`EdgeInsetsGeometry::lerp`]: a missing end scales the present one rather
/// than holding it still.
fn lerp_edge_insets(a: Option<EdgeInsets>, b: Option<EdgeInsets>, t: f32) -> Option<EdgeInsets> {
    let scale = |insets: EdgeInsets, factor: f32| EdgeInsets {
        left: insets.left * factor,
        top: insets.top * factor,
        right: insets.right * factor,
        bottom: insets.bottom * factor,
    };
    match (a, b) {
        (None, None) => None,
        (None, Some(b)) => Some(scale(b, t)),
        (Some(a), None) => Some(scale(a, 1.0 - t)),
        (Some(a), Some(b)) => Some(<EdgeInsets as crate::implicit::Lerp>::lerp(a, b, t)),
    }
}

/// `WidgetStateProperty.lerp<EdgeInsetsGeometry?>(a, b, t,
/// EdgeInsetsGeometry.lerp)`.
fn lerp_state_insets(
    a: Option<&StateProperty<Option<EdgeInsetsGeometry>>>,
    b: Option<&StateProperty<Option<EdgeInsetsGeometry>>>,
    t: f32,
) -> Option<StateProperty<Option<EdgeInsetsGeometry>>> {
    lerp_state_property(a, b, t, |first, second, t| {
        EdgeInsetsGeometry::lerp(first.flatten(), second.flatten(), t)
    })
}

/// `WidgetStateProperty.lerp<Size?>(a, b, t, Size.lerp)`.
///
/// `Size.lerp`'s missing end scales the present size rather than holding it,
/// so a minimum that only one theme names grows in rather than springing to
/// full size.
fn lerp_state_size(
    a: Option<&StateProperty<Option<Size>>>,
    b: Option<&StateProperty<Option<Size>>>,
    t: f32,
) -> Option<StateProperty<Option<Size>>> {
    fn blend(a: Option<Size>, b: Option<Size>, t: f32) -> Option<Size> {
        match (a, b) {
            (None, None) => None,
            (None, Some(b)) => Some(Size::new(b.width * t, b.height * t)),
            (Some(a), None) => Some(Size::new(a.width * (1.0 - t), a.height * (1.0 - t))),
            (Some(a), Some(b)) => Some(Size::new(
                a.width + (b.width - a.width) * t,
                a.height + (b.height - a.height) * t,
            )),
        }
    }
    lerp_state_property(a, b, t, |first, second, t| {
        blend(first.flatten(), second.flatten(), t)
    })
}

/// Upstream `WidgetStateBorderSide.lerp`, whose `_LerpSides` gives a missing
/// side the other's colour at zero alpha and zero width -- so a border that
/// appears fades in rather than snapping to full width.
fn lerp_state_side(
    a: Option<&StateProperty<Option<BorderSide>>>,
    b: Option<&StateProperty<Option<BorderSide>>>,
    t: f32,
) -> Option<StateProperty<Option<BorderSide>>> {
    fn vanishing(side: &BorderSide) -> BorderSide {
        BorderSide {
            color: side.color.with_alpha(0),
            width: 0.0,
            ..*side
        }
    }
    lerp_state_property(a, b, t, |first, second, t| {
        match (first.flatten(), second.flatten()) {
            (None, None) => None,
            (None, Some(b)) => Some(BorderSide::lerp(vanishing(&b), b, t)),
            (Some(a), None) => Some(BorderSide::lerp(a, vanishing(&a), t)),
            (Some(a), Some(b)) => Some(BorderSide::lerp(a, b, t)),
        }
    })
}

/// `WidgetStateProperty.lerp<double?>(a, b, t, lerpDouble)`.
fn lerp_state_f32(
    a: Option<&StateProperty<Option<f32>>>,
    b: Option<&StateProperty<Option<f32>>>,
    t: f32,
) -> Option<StateProperty<Option<f32>>> {
    lerp_state_property(a, b, t, |first, second, t| {
        lerp_f32(first.flatten(), second.flatten(), t)
    })
}

/// `WidgetStateProperty.lerp<OutlinedBorder?>(a, b, t, OutlinedBorder.lerp)`.
fn lerp_state_shape(
    a: Option<&StateProperty<Option<ShapeBorder>>>,
    b: Option<&StateProperty<Option<ShapeBorder>>>,
    t: f32,
) -> Option<StateProperty<Option<ShapeBorder>>> {
    lerp_state_property(a, b, t, |first, second, t| {
        ShapeBorder::lerp(first.flatten(), second.flatten(), t)
    })
}

/// `WidgetStateProperty.lerp<IconThemeData?>(a, b, t, IconThemeData.lerp)`.
fn lerp_state_icon_theme(
    a: Option<&StateProperty<Option<IconThemeData>>>,
    b: Option<&StateProperty<Option<IconThemeData>>>,
    t: f32,
) -> Option<StateProperty<Option<IconThemeData>>> {
    lerp_state_property(a, b, t, |first, second, t| {
        lerp_icon_theme(&first.flatten(), &second.flatten(), t)
    })
}

/// `WidgetStateProperty.lerp<TextStyle?>(a, b, t, TextStyle.lerp)`.
///
/// The same shape as [`lerp_state_color`]: both ends resolve against the same
/// states and the two answers blend. [`lerp_text_style`]'s note on the single
/// null applies here too, one state at a time.
fn lerp_state_text_style(
    a: Option<&StateProperty<Option<TextStyle>>>,
    b: Option<&StateProperty<Option<TextStyle>>>,
    t: f32,
) -> Option<StateProperty<Option<TextStyle>>> {
    lerp_state_property(a, b, t, |first, second, t| {
        lerp_text_style(&first.flatten(), &second.flatten(), t)
    })
}

/// Upstream `TextStyle.lerp` behind this file's optional-wrapper idiom.
///
/// # What upstream does with a single null that this cannot
///
/// Upstream's null arms do not step. `TextStyle.lerp(null, b, t)` builds a
/// style whose colour is `Color.lerp(null, b.color, t)` -- a fade in from
/// transparent -- whose weight comes from `FontWeight.lerp(null, b.fontWeight,
/// t)`, and whose every other field is `t < 0.5 ? null : b.x`. The fields that
/// step are the ones with no midpoint; the two that fade are the two that
/// have one.
///
/// This port cannot express that arm, because its [`TextStyle`] has no null
/// fields: `color`, `font_size`, `font_weight` and `align` are values, not
/// options, so "a style whose size is still null" has nowhere to live. A
/// style that only one end names therefore steps whole at the midpoint,
/// colour included, where upstream would fade its colour in. Both ends
/// present -- which is every field of a `TextTheme` built from a Material
/// baseline, and so the case a real theme transition takes -- goes through
/// [`TextStyle::lerp`] and is exact.
fn lerp_text_style(a: &Option<TextStyle>, b: &Option<TextStyle>, t: f32) -> Option<TextStyle> {
    match (a, b) {
        (Some(first), Some(second)) => Some(TextStyle::lerp(first, second, t)),
        (first, second) => {
            if t < 0.5 {
                first.clone()
            } else {
                second.clone()
            }
        }
    }
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
            radius: BorderRadiusGeometry::lerp_optional(a.radius, b.radius, t),
        }
    }
}

/// Upstream `DividerTheme`.
pub struct DividerTheme;

impl DividerTheme {
    /// Installs one for a subtree.
    pub fn new(data: DividerThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            margin: EdgeInsetsGeometry::lerp(a.margin, b.margin, t),
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
        }
    }
}

/// Upstream `CardTheme`.
pub struct CardTheme;

impl CardTheme {
    pub fn new(data: CardThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            text_style: lerp_text_style(&a.text_style, &b.text_style, t),
            padding: EdgeInsetsGeometry::lerp(a.padding, b.padding, t),
            alignment: AlignmentGeometry::lerp(a.alignment, b.alignment, t),
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
        provide_theme(data, child)
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
            constraints: BoxConstraints::lerp(a.constraints, b.constraints, t),
            padding: EdgeInsetsGeometry::lerp(a.padding, b.padding, t),
            margin: EdgeInsetsGeometry::lerp(a.margin, b.margin, t),
            // # Five fields upstream's own `lerp` drops
            //
            // `TooltipThemeData.lerp` assigns ten fields and leaves
            // `waitDuration`, `showDuration`, `exitDuration`, `triggerMode`
            // and `enableFeedback` unset, so a tooltip theme half-way through
            // a transition loses them and each tooltip falls back to its own
            // default. This port carries them at the nearer end instead.
            //
            // That is a deliberate difference, not an oversight here: the
            // five have no midpoint, dropping them is visible behaviour, and
            // it reads like an upstream oversight rather than a decision. It
            // is pinned by a test so that it stays a choice.
            prefer_below: lerp_nearer(&a.prefer_below, &b.prefer_below, t),
            exclude_from_semantics: lerp_nearer(
                &a.exclude_from_semantics,
                &b.exclude_from_semantics,
                t,
            ),
            decoration: crate::decoration::Decoration::lerp(
                a.decoration.clone(),
                b.decoration.clone(),
                t,
            ),
            text_style: lerp_text_style(&a.text_style, &b.text_style, t),
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
        provide_theme(data, child)
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
            border_radius: BorderRadiusGeometry::lerp_optional(a.border_radius, b.border_radius, t),
            stop_indicator_color: lerp_color(a.stop_indicator_color, b.stop_indicator_color, t),
            stop_indicator_radius: lerp_f32(a.stop_indicator_radius, b.stop_indicator_radius, t),
            stroke_width: lerp_f32(a.stroke_width, b.stroke_width, t),
            stroke_align: lerp_f32(a.stroke_align, b.stroke_align, t),
            stroke_cap: lerp_nearer(&a.stroke_cap, &b.stroke_cap, t),
            constraints: BoxConstraints::lerp(a.constraints, b.constraints, t),
            track_gap: lerp_f32(a.track_gap, b.track_gap, t),
            circular_track_padding: EdgeInsetsGeometry::lerp(
                a.circular_track_padding,
                b.circular_track_padding,
                t,
            ),
        }
    }
}

/// Upstream `ProgressIndicatorTheme`.
pub struct ProgressIndicatorTheme;

impl ProgressIndicatorTheme {
    pub fn new(data: ProgressIndicatorThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
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
        provide_theme(data, child)
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
            inner_radius: lerp_state_f32(a.inner_radius.as_ref(), b.inner_radius.as_ref(), t),
        }
    }
}

/// Upstream `RadioTheme`.
pub struct RadioTheme;

impl RadioTheme {
    pub fn new(data: RadioThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            track_outline_width: lerp_state_f32(
                a.track_outline_width.as_ref(),
                b.track_outline_width.as_ref(),
                t,
            ),
            material_tap_target_size: lerp_nearer(
                &a.material_tap_target_size,
                &b.material_tap_target_size,
                t,
            ),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
            overlay_color: lerp_state_color(a.overlay_color.as_ref(), b.overlay_color.as_ref(), t),
            splash_radius: lerp_f32(a.splash_radius, b.splash_radius, t),
            padding: EdgeInsetsGeometry::lerp(a.padding, b.padding, t),
        }
    }
}

/// Upstream `SwitchTheme`.
pub struct SwitchTheme;

impl SwitchTheme {
    pub fn new(data: SwitchThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
    /// How long the tooltip stays after the pointer leaves. Upstream's own
    /// default is 100ms, which is a tenth of `show_duration`'s -- a pointer
    /// that slid off is not the same event as a reader who has finished
    /// reading, and it is not given the same grace.
    pub exit_duration: std::time::Duration,
    /// What a touch does. Upstream's `Tooltip.triggerMode`, whose fallback is
    /// this whole three-step chain: the widget's, then the theme's, then
    /// `longPress`.
    ///
    /// Hover is not in it. A mouse resting on a tooltip shows it whatever this
    /// says, because hovering is not a gesture anyone has to be taught --
    /// upstream says exactly that in `TooltipTriggerMode`'s doc.
    pub trigger_mode: crate::raw_tooltip::TooltipTriggerMode,
}

impl ResolvedTooltip {
    /// Upstream's `_defaultVerticalOffset`.
    pub const VERTICAL_OFFSET: f32 = 24.0;
    /// Upstream's `_defaultPreferBelow`.
    pub const PREFER_BELOW: bool = true;
    /// Upstream's `_defaultShowDuration`.
    pub const SHOW_DURATION: std::time::Duration = std::time::Duration::from_millis(1500);
    /// Upstream's `_defaultExitDuration`.
    pub const EXIT_DURATION: std::time::Duration = std::time::Duration::from_millis(100);
    /// Upstream's `_defaultWaitDuration`, which is **zero**: a tooltip summoned
    /// by a long press has already been waited for.
    pub const WAIT_DURATION: std::time::Duration = std::time::Duration::ZERO;
    /// Upstream's `_defaultTriggerMode`.
    pub const TRIGGER_MODE: crate::raw_tooltip::TooltipTriggerMode =
        crate::raw_tooltip::TooltipTriggerMode::LongPress;

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
            exit_duration: data.exit_duration.unwrap_or(ResolvedTooltip::EXIT_DURATION),
            trigger_mode: data.trigger_mode.unwrap_or(ResolvedTooltip::TRIGGER_MODE),
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
    /// How the ends of the drawn arc are cut. Upstream's own default is not
    /// one value: `StrokeCap.round` for a spinner and for a linear bar's
    /// track, `StrokeCap.butt` for the gapped Material 3 linear bar -- so
    /// `None` here means "each painter's own", not "square".
    pub stroke_cap: Option<crate::painting::StrokeCap>,
    /// The room a circular indicator leaves around its track.
    pub circular_track_padding: Option<EdgeInsetsGeometry>,
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
            stroke_cap: data.stroke_cap,
            circular_track_padding: data.circular_track_padding,
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
    /// The action's label colour, enabled and disabled.
    ///
    /// Upstream resolves each as
    /// `widget.textColor ?? snackBarTheme.actionTextColor ?? defaults`, and
    /// `_SnackbarDefaultsM3` answers `colorScheme.inversePrimary` for both.
    /// This carried the theme's enabled colour only, and neither the action's
    /// own override nor the default -- so an action that named a colour was
    /// ignored, and one that named none had none.
    pub action_text_color: Color,
    pub disabled_action_text_color: Color,
    /// The action's background, enabled and disabled. Upstream's default for
    /// both is `Colors.transparent`: the action is a text button unless a
    /// theme or the action itself asks otherwise.
    pub action_background_color: Color,
    pub disabled_action_background_color: Color,
    /// Upstream's
    /// `widget.closeIconColor ?? snackBarTheme.closeIconColor ?? defaults`,
    /// whose M3 default is `colorScheme.onInverseSurface` -- the same ink the
    /// content is written in.
    pub close_icon_color: Color,
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
        let action = bar.action.as_ref();
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
            // Each of the five is the action's own value, then the theme's,
            // then upstream's default. The action's overrides were reaching
            // nothing at all before, and three of the theme's five were too.
            action_text_color: action
                .and_then(|action| action.text_color)
                .or(data.action_text_color)
                .unwrap_or(scheme.inverse_primary()),
            disabled_action_text_color: action
                .and_then(|action| action.disabled_text_color)
                .or(data.disabled_action_text_color)
                .unwrap_or(scheme.inverse_primary()),
            action_background_color: action
                .and_then(|action| action.background_color)
                .or(data.action_background_color)
                .unwrap_or(Color::TRANSPARENT),
            disabled_action_background_color: action
                .and_then(|action| action.disabled_background_color)
                .or(data.disabled_action_background_color)
                .unwrap_or(Color::TRANSPARENT),
            close_icon_color: bar
                .close_icon_color
                .or(data.close_icon_color)
                .unwrap_or(scheme.on_inverse_surface()),
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
    /// Upstream's `_BottomSheetDefaultsM3.dragHandleSize` is 32 by 4 -- wide
    /// and thin, so it reads as a grip rather than a button.
    pub drag_handle_size: crate::render::Size,
}

impl ResolvedBottomSheet {
    /// Upstream's `_BottomSheetDefaultsM3.elevation`, which is 1: a persistent
    /// sheet is part of the page and barely lifted off it.
    pub const ELEVATION: f32 = 1.0;
    /// Upstream's `_BottomSheetDefaultsM3.dragHandleSize`.
    pub const DRAG_HANDLE_SIZE: crate::render::Size = crate::render::Size {
        width: 32.0,
        height: 4.0,
    };
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
            drag_handle_size: data
                .drag_handle_size
                .unwrap_or(ResolvedBottomSheet::DRAG_HANDLE_SIZE),
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
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let material3 = theme.use_material3;
        let text_theme = theme.text_theme.clone();
        ResolvedDialog {
            background: data
                .background_color
                .unwrap_or(scheme.surface_container_high()),
            elevation: data.elevation.unwrap_or(ResolvedDialog::ELEVATION),
            // Both transparent under Material 3, and that is an answer
            // rather than a gap: an M3 dialog is elevation 6 with **no
            // shadow and no tint**, so its height off the page is said
            // entirely by `surfaceContainerHigh`. Left unset, anything
            // downstream reads "nobody said" and draws the shadow upstream
            // turned off on purpose.
            shadow_color: data.shadow_color.or(Some(if material3 {
                Color::TRANSPARENT
            } else {
                theme.shadow_color
            })),
            surface_tint_color: data.surface_tint_color.or(if material3 {
                Some(Color::TRANSPARENT)
            } else {
                // `_DialogDefaultsM2` has no `surfaceTintColor` at all: a
                // tint is a Material 3 idea, and there is nothing to invent.
                None
            }),
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
            // The one field on this resolver upstream really does leave
            // unanswered: a barrier belongs to `showDialog`, not to the
            // dialog, and its default lives there.
            barrier_color: data.barrier_color,
            title_text_style: data.title_text_style.clone().or_else(|| {
                if material3 {
                    text_theme.headline_small.clone()
                } else {
                    text_theme.title_large.clone()
                }
            }),
            content_text_style: data.content_text_style.clone().or_else(|| {
                if material3 {
                    text_theme.body_medium.clone()
                } else {
                    text_theme.title_medium.clone()
                }
            }),
            actions_padding: data
                .actions_padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::ZERO),
            // Material 3 gives the icon `secondary` outright; Material 2
            // takes whatever the surrounding icon theme is using, so a
            // dialog's icon matches the icons around it rather than standing
            // apart from them. Where that theme has no colour there is
            // nothing to fall back to.
            icon_color: data.icon_color.or(if material3 {
                Some(scheme.secondary)
            } else {
                IconTheme::of(context).color
            }),
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
    /// How long the tile takes to open and how it eases.
    ///
    /// Upstream reads the three parts separately --
    /// `expansionAnimationStyle?.duration ?? 200ms`,
    /// `?.curve ?? Curves.easeIn`, and `?.reverseCurve` with **no fallback at
    /// all** -- so a style that names only a duration keeps the default
    /// curve. Carried whole here for the same reason: the parts are asked
    /// for one at a time.
    pub expansion_animation_style: Option<crate::animation::AnimationStyle>,
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
            expansion_animation_style: data.expansion_animation_style.clone(),
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
    /// The two row fills and the two cursors, each already resolved against
    /// the states in hand.
    ///
    /// `None` is an answer for all four: upstream has no default row colour
    /// -- a table draws on whatever it is placed on -- and no default cursor
    /// beyond the pointer's own.
    pub data_row_color: Option<Color>,
    pub heading_row_color: Option<Color>,
    pub data_row_cursor: Option<SystemMouseCursor>,
    pub heading_cell_cursor: Option<SystemMouseCursor>,
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
        ResolvedDataTable::of_in(context, WidgetStates::NONE)
    }

    /// [`ResolvedDataTable::of`] for a row or cell that knows its states,
    /// which the four state properties need: a hovered row and a selected
    /// one are different fills of the same field.
    pub fn of_in(context: &mut BuildContext, states: WidgetStates) -> ResolvedDataTable {
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
            data_row_color: data
                .data_row_color
                .as_ref()
                .and_then(|property| property.resolve(states)),
            heading_row_color: data
                .heading_row_color
                .as_ref()
                .and_then(|property| property.resolve(states)),
            data_row_cursor: data
                .data_row_cursor
                .as_ref()
                .and_then(|property| property.resolve(states)),
            heading_cell_cursor: data
                .heading_cell_cursor
                .as_ref()
                .and_then(|property| property.resolve(states)),
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
    /// Upstream's two `ColorTween` ends, resolved.
    ///
    /// # The last step depends on the bar's type
    ///
    /// A **fixed** bar falls back to `ThemeData::unselected_widget_color`
    /// for the unselected end and to `themeColor` for the selected one --
    /// `colorScheme.primary` under a light theme, `secondary` under a dark
    /// one. A **shifting** bar falls back to `colorScheme.surface` for
    /// *both*, because its items sit on a coloured background of their own
    /// and the contrast comes from the background rather than the ink.
    ///
    /// These were `Option<Color>` copies of the theme's fields with no
    /// last step at all, so none of those three fallbacks reached
    /// anything.
    pub selected_item_color: Color,
    pub unselected_item_color: Color,
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

    /// Upstream's `themeColor`, the last step of a fixed bar's *selected*
    /// end: the primary role under a light theme and the secondary under a
    /// dark one. The swap is the point -- a dark theme's primary is a pale
    /// tint meant for large areas, and a small selected icon needs the
    /// accent.
    pub fn theme_color(theme: &ThemeData) -> Color {
        match theme.brightness() {
            crate::platform::Brightness::Light => theme.color_scheme.primary,
            crate::platform::Brightness::Dark => theme.color_scheme.secondary,
        }
    }

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
        let theme = ThemeData::of(context);
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
            selected_item_color: bar
                .selected_item_color
                .or(data.selected_item_color)
                .or(match bar_type {
                    BottomNavigationBarType::Fixed => bar.fixed_color,
                    BottomNavigationBarType::Shifting => None,
                })
                .unwrap_or(match bar_type {
                    BottomNavigationBarType::Fixed => {
                        ResolvedBottomNavigationBar::theme_color(&theme)
                    }
                    BottomNavigationBarType::Shifting => theme.color_scheme.surface,
                }),
            unselected_item_color: bar
                .unselected_item_color
                .or(data.unselected_item_color)
                .unwrap_or(match bar_type {
                    BottomNavigationBarType::Fixed => theme.unselected_widget_color,
                    BottomNavigationBarType::Shifting => theme.color_scheme.surface,
                }),
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

/// What a bottom app bar is drawn with -- upstream's `_BottomAppBarState.build`
/// reading `BottomAppBarTheme.of` and then one of two defaults classes.
///
/// # The elevation is an input to the colour, not just to the shadow
///
/// Upstream resolves `color` through three steps and then never paints it:
/// `effectiveColor` is `applySurfaceTint(color, surfaceTintColor, elevation)`
/// under Material 3 and `applyOverlay(context, color, elevation)` under
/// Material 2. Both take the elevation. So raising a bar's elevation changes
/// what colour it is, and a caller who set the colour and the elevation
/// separately has set the colour twice. See
/// [`ResolvedBottomAppBar::effective_color`].
///
/// # By default that transform does nothing -- in both branches, for opposite
/// reasons
///
/// Material 3 takes the branch that uses the surface tint and defaults the tint
/// to **transparent**, which `applySurfaceTint` short-circuits on. Material 2
/// defaults the tint to a real colour, `colorScheme.surfaceTint`, and takes the
/// branch that **ignores it**. Neither default tints anything; the field only
/// ever acts when a caller sets it *and* is on Material 3.
///
/// It is worth writing down because the resolution looks alive from either
/// side: M2 recomputes a scheme colour on every build and throws it away, and
/// M3 keeps a field it does consult and hands it a value that means "don't".
///
/// # Two things Material 2 leaves null that Material 3 pins
///
/// `height` and `shape`. An M2 bar has no height of its own and is as tall as
/// its child; an M3 bar is 80 whatever is in it. An M2 bar has no notch unless
/// asked; an M3 bar defaults to `AutomaticNotchedShape(RoundedRectangleBorder())`,
/// so it **cuts a hole for a floating action button without being told to** --
/// but only when there is one to cut for; see
/// [`ResolvedBottomAppBar::cuts_a_notch`].
///
/// # The padding's default is not in the defaults class
///
/// Every other field's third step is `defaults.field`. The padding's is written
/// inline at the use site as `isMaterial3 ? EdgeInsets.symmetric(...) :
/// EdgeInsets.zero`, and neither `_BottomAppBarDefaultsM2` nor
/// `_BottomAppBarDefaultsM3` declares a padding. The chain is the same length;
/// only the place the last step is written differs.
///
/// # Which defaults class runs is a theme-wide switch
///
/// [`ThemeData::use_material3`], as upstream. It used to be a `material3` field
/// on the widget here -- a field upstream's `BottomAppBar` does not have --
/// on the grounds that exactly one widget branched on it and a theme field
/// nobody else read would advertise a switch that switched nothing. The popup
/// menu is the second, so it moved to the theme, where upstream keeps it.
pub struct ResolvedBottomAppBar {
    pub color: Color,
    pub elevation: f32,
    /// `None` under Material 2: the bar is as tall as its child.
    pub height: Option<f32>,
    /// `None` means no notch is possible; `Some` means one is possible, not
    /// that one is cut -- see [`ResolvedBottomAppBar::cuts_a_notch`].
    pub shape: Option<crate::borders::NotchedShape>,
    pub surface_tint_color: Color,
    pub shadow_color: Color,
    pub padding: EdgeInsets,
    material3: bool,
    /// Upstream's `ThemeData.applyElevationOverlayColor`, which is the second
    /// of the three conditions `applyOverlay` checks.
    ///
    /// It was passed as a literal `true` here, so a theme that turned the
    /// overlay off was ignored -- and turning it off is the only way a
    /// Material 2 application keeps a dark surface flat.
    apply_elevation_overlay_color: bool,
}

impl ResolvedBottomAppBar {
    /// Upstream `_BottomAppBarDefaultsM3.elevation`.
    pub const M3_ELEVATION: f32 = 3.0;
    /// Upstream `_BottomAppBarDefaultsM2.elevation`.
    pub const M2_ELEVATION: f32 = 8.0;
    /// Upstream `_BottomAppBarDefaultsM3.height`. Material 2 has none.
    pub const M3_HEIGHT: f32 = 80.0;
    /// Upstream's Material 2 light-mode colour: plain white, from before the
    /// bar took its colour from a scheme.
    pub const M2_LIGHT: Color = Color(0xFFFFFFFF);
    /// Upstream's `Colors.grey[800]` for Material 2 in the dark.
    pub const M2_DARK: Color = Color(0xFF424242);

    pub fn of(
        context: &mut BuildContext,
        bar: &crate::bottom_bars::BottomAppBar,
    ) -> ResolvedBottomAppBar {
        let data = BottomAppBarTheme::of(context);
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let material3 = theme.use_material3;
        ResolvedBottomAppBar {
            color: data.color.unwrap_or_else(|| {
                if material3 {
                    scheme.surface_container()
                } else if theme.brightness() == crate::platform::Brightness::Dark {
                    ResolvedBottomAppBar::M2_DARK
                } else {
                    ResolvedBottomAppBar::M2_LIGHT
                }
            }),
            elevation: data.elevation.unwrap_or(if material3 {
                ResolvedBottomAppBar::M3_ELEVATION
            } else {
                ResolvedBottomAppBar::M2_ELEVATION
            }),
            height: data.height.or(if material3 {
                Some(ResolvedBottomAppBar::M3_HEIGHT)
            } else {
                None
            }),
            shape: bar.shape.clone().or(data.shape.clone()).or_else(|| {
                // Unconditional under Material 3: the bar carries a shape
                // whether or not anyone asked for one.
                material3.then(|| crate::borders::NotchedShape::Automatic {
                    host: crate::borders::ShapeBorder::Rounded(
                        crate::borders::RoundedRectangleBorder::default(),
                    ),
                    guest: None,
                })
            }),
            surface_tint_color: data.surface_tint_color.unwrap_or(if material3 {
                Color::TRANSPARENT
            } else {
                scheme.surface_tint()
            }),
            shadow_color: data.shadow_color.unwrap_or(if material3 {
                Color::TRANSPARENT
            } else {
                Color(0xFF000000)
            }),
            // The one default written at the use site rather than in a
            // defaults class -- see the type's docs.
            padding: data
                .padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(if material3 {
                    EdgeInsets::symmetric(
                        crate::bottom_bars::BottomAppBar::M3_PADDING.0,
                        crate::bottom_bars::BottomAppBar::M3_PADDING.1,
                    )
                } else {
                    EdgeInsets::ZERO
                }),
            material3,
            apply_elevation_overlay_color: theme.apply_elevation_overlay_color,
        }
    }

    /// What actually gets painted: the colour with the elevation folded in.
    ///
    /// Two different transforms, and they are not two spellings of one idea.
    /// The Material 3 one blends a tint over the colour and applies to any
    /// colour. The Material 2 one only fires in the dark, and only when the
    /// colour is already the surface -- a bar someone coloured by hand keeps
    /// its colour there, and the same bar under Material 3 does not.
    pub fn effective_color(&self, is_dark: bool, surface: Color, on_surface: Color) -> Color {
        if self.material3 {
            crate::elevation_overlay::ElevationOverlay::apply_surface_tint(
                self.color,
                Some(self.surface_tint_color),
                self.elevation,
            )
        } else {
            crate::elevation_overlay::ElevationOverlay::apply_overlay(
                self.color,
                self.elevation,
                self.apply_elevation_overlay_color,
                is_dark,
                surface,
                on_surface,
            )
        }
    }

    /// Whether a hole is cut, which needs a shape **and** something to cut for.
    ///
    /// Upstream's `notchedShape != null && hasFab`. A resolved shape is not a
    /// notch: with no floating action button in the `Scaffold` the clipper is a
    /// plain rounded rectangle, so an M3 bar carries its default notch around
    /// and never uses it until a button arrives.
    pub fn cuts_a_notch(&self, has_floating_action_button: bool) -> bool {
        self.shape.is_some() && has_floating_action_button
    }
}

/// What a popup menu and its items are drawn with -- upstream's
/// `_PopupMenuItemState.build` and `_PopupMenuRoute` reading
/// `PopupMenuTheme.of` and then one of two defaults classes.
///
/// # `text_style` and `label_text_style` are not a fallback pair
///
/// The theme holds both, and it is tempting to read them as one superseding the
/// other. They do not compete: `useMaterial3` chooses **which chain runs at
/// all**.
///
/// * Material 3: `widget.labelTextStyle ?? theme.labelTextStyle ?? defaults.labelTextStyle`,
///   all three state-resolved. `_PopupMenuDefaultsM3` fills `labelTextStyle`
///   and leaves `textStyle` null.
/// * Material 2: `widget.textStyle ?? theme.textStyle ?? defaults.textStyle`,
///   all three flat. `_PopupMenuDefaultsM2` fills `textStyle` and leaves
///   `labelTextStyle` null.
///
/// So a theme that sets only `textStyle` does **nothing** under Material 3, and
/// one that sets only `labelTextStyle` does nothing under Material 2 -- not
/// "is overridden", but is never read. This port's `PopupMenuThemeData`
/// documented `label_text_style` as superseding `text_style` "where both are
/// set", which describes a contest that never happens.
///
/// # Disabled is handled in two different places, and the difference shows
///
/// Material 3 has no separate step: the disabled colour comes out of the state
/// resolution itself, so a caller's own resolver has the last word. Material 2
/// has no state property to resolve, so upstream applies
/// `style.copyWith(color: theme.disabledColor)` **after** the chain has run --
/// over whatever won, a caller's own `textStyle` included.
///
/// That is the observable difference: on Material 2 a caller cannot colour a
/// disabled item, because the overwrite happens downstream of them; on Material
/// 3 they can, because it happens inside the step they supplied.
///
/// # Two paddings that sound alike, and only one is themeable
///
/// `menuPadding` is the menu's own, `EdgeInsets.symmetric(vertical: 8)`, and it
/// is a theme field. The item's padding is `widget.padding ?? (m3 ? 12 : 16)`
/// horizontal, read from a **static** on the defaults class -- there is no
/// theme field for it at all.
///
/// They are perpendicular, which is why they compose instead of fighting: the
/// menu pads top and bottom, the item pads left and right, and neither has an
/// opinion about the other's axis.
pub struct ResolvedPopupMenu {
    pub color: Color,
    pub shape: Option<ShapeBorder>,
    pub elevation: f32,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    /// The menu's own padding: vertical, and themeable.
    pub menu_padding: EdgeInsets,
    /// The item's: horizontal, and not themeable -- see the type's docs.
    pub item_padding: EdgeInsets,
    pub enable_feedback: bool,
    pub position: PopupMenuPosition,
    pub icon_color: Option<Color>,
    pub icon_size: Option<f32>,
    text_style: Option<TextStyle>,
    label_text_style: Option<StateProperty<Option<TextStyle>>>,
    title_medium: Option<TextStyle>,
    label_large: Option<TextStyle>,
    on_surface: Color,
    disabled_color: Color,
    use_material3: bool,
}

impl ResolvedPopupMenu {
    /// Upstream `_PopupMenuDefaultsM3.elevation`.
    pub const M3_ELEVATION: f32 = 3.0;
    /// Upstream `_PopupMenuDefaultsM2.elevation`.
    pub const M2_ELEVATION: f32 = 8.0;
    /// Upstream's `menuPadding` on both defaults classes -- the one number the
    /// two agree on.
    pub const MENU_PADDING: f32 = 8.0;
    /// Upstream `_PopupMenuDefaultsM3.menuItemPadding`, horizontal.
    pub const M3_ITEM_PADDING: f32 = 12.0;
    /// Upstream `_PopupMenuDefaultsM2.menuItemPadding`, horizontal.
    pub const M2_ITEM_PADDING: f32 = 16.0;
    /// Upstream's corner radius for the Material 3 menu.
    pub const M3_RADIUS: f32 = 4.0;
    /// Upstream's `_colors.onSurface.withOpacity(0.38)` for a disabled entry.
    pub const DISABLED_OPACITY: f32 = 0.38;

    pub fn of(context: &mut BuildContext) -> ResolvedPopupMenu {
        let data = PopupMenuTheme::of(context);
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let use_material3 = theme.use_material3;
        ResolvedPopupMenu {
            color: data.color.unwrap_or_else(|| scheme.surface_container()),
            shape: data.shape.clone().or_else(|| {
                use_material3.then(|| {
                    ShapeBorder::Rounded(crate::borders::RoundedRectangleBorder::new(
                        crate::borders::BorderSide::NONE,
                        crate::borders::BorderRadiusGeometry::circular(
                            ResolvedPopupMenu::M3_RADIUS,
                        ),
                    ))
                })
            }),
            elevation: data.elevation.unwrap_or(if use_material3 {
                ResolvedPopupMenu::M3_ELEVATION
            } else {
                ResolvedPopupMenu::M2_ELEVATION
            }),
            shadow_color: data.shadow_color.or(if use_material3 {
                Some(scheme.shadow())
            } else {
                None
            }),
            surface_tint_color: data.surface_tint_color.or(if use_material3 {
                Some(Color::TRANSPARENT)
            } else {
                None
            }),
            menu_padding: data
                .menu_padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::symmetric(0.0, ResolvedPopupMenu::MENU_PADDING)),
            item_padding: EdgeInsets::symmetric(
                if use_material3 {
                    ResolvedPopupMenu::M3_ITEM_PADDING
                } else {
                    ResolvedPopupMenu::M2_ITEM_PADDING
                },
                0.0,
            ),
            enable_feedback: data.enable_feedback.unwrap_or(true),
            position: data.position.unwrap_or(PopupMenuPosition::Over),
            icon_color: data.icon_color,
            icon_size: data.icon_size,
            text_style: data.text_style.clone(),
            label_text_style: data.label_text_style.clone(),
            title_medium: theme.text_theme.title_medium.clone(),
            label_large: theme.text_theme.label_large.clone(),
            on_surface: scheme.on_surface,
            disabled_color: theme.disabled_color,
            use_material3,
        }
    }

    /// An entry's style, by the whole of upstream's rule.
    ///
    /// One branch or the other, never a blend -- and Material 2's disabled
    /// colour is applied after the chain rather than inside it. See the type's
    /// docs for why that is visible from outside.
    pub fn entry_style(&self, enabled: bool) -> Option<TextStyle> {
        if self.use_material3 {
            let states = if enabled {
                WidgetStates::NONE
            } else {
                WidgetStates::NONE.with(WidgetState::Disabled)
            };
            if let Some(property) = &self.label_text_style {
                return property.resolve(states);
            }
            return self.label_large.clone().map(|style| TextStyle {
                color: if enabled {
                    self.on_surface
                } else {
                    crate::elevation_overlay::with_opacity(
                        self.on_surface,
                        ResolvedPopupMenu::DISABLED_OPACITY,
                    )
                },
                ..style
            });
        }

        let style = self
            .text_style
            .clone()
            .or_else(|| self.title_medium.clone());
        if enabled {
            return style;
        }
        // Material 2's overwrite, downstream of everything above it.
        style.map(|style| TextStyle {
            color: self.disabled_color,
            ..style
        })
    }
}

/// Which way a menu panel runs, which is also which theme it reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuPanelAxis {
    /// A row of menus along the top of a window: `MenuBarTheme`.
    Horizontal,
    /// A column of items hanging off an anchor: `MenuTheme`.
    Vertical,
}

/// What a menu panel is drawn with -- upstream's `_MenuPanelState.build`.
///
/// # The axis picks the theme, not the widget
///
/// Upstream writes
/// `switch (widget.orientation) { horizontal => MenuBarTheme.of(context),
/// vertical => MenuTheme.of(context) }`. It is the **same `_MenuPanel`** in
/// both cases. Being a `MenuBar` is not what makes `MenuBarTheme` apply; being
/// horizontal is, and a `MenuBar` is horizontal.
///
/// So the two themes are not "the bar's" and "the menu's" in the sense of
/// belonging to two widgets. They are one widget's two orientations, themed
/// separately because a row and a column want different things -- which is
/// exactly what the two defaults classes turn out to say.
///
/// # The two defaults differ in two fields, and both differences are the axis
///
/// `_MenuBarDefaultsM3` and `_MenuDefaultsM3` agree on elevation 3, the
/// four-radius border, `surfaceContainer`, the scheme's shadow, a transparent
/// tint and the theme's visual density. They disagree on exactly two things:
///
/// * **alignment** -- the bar's is `bottomStart`, the menu's is `topEnd`. A
///   bar's submenu drops below it; a menu's flies out beside it. There is
///   nowhere else for either to go.
/// * **padding** -- the bar's is 4 **horizontal**, the menu's is 8
///   **vertical**. A row is padded at its ends and a column at its ends, and
///   the ends are on different axes.
///
/// Both differences fall out of the one fact. Nothing else about a row of
/// menus differs from a column of them.
///
/// # Every state property is resolved against no states at all
///
/// `_MenuPanelState.build`'s `resolve` calls `getProperty(style)?.resolve(<WidgetState>{})`
/// -- the empty set, unconditionally. A panel is a surface, not a control: it
/// is not hovered or pressed or focused, its items are. So a `MenuStyle` whose
/// background answers differently when hovered is asked as though nothing were
/// happening, and only ever gives its no-state answer.
///
/// # `elevation ?? 0` cannot fire
///
/// The line reads `resolve(...elevation) ?? 0`, a fourth step after the three.
/// It is unreachable: the chain's last step is the defaults class, both of them
/// supply `WidgetStatePropertyAll(3.0)`, and a widget or theme style whose
/// elevation resolves to null falls through to that same default rather than
/// out of the chain. The zero is what would happen if a defaults class ever
/// stopped supplying one.
pub struct ResolvedMenuPanel {
    pub axis: MenuPanelAxis,
    pub background_color: Option<Color>,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub elevation: f32,
    pub padding: EdgeInsets,
    pub minimum_size: Option<Size>,
    pub fixed_size: Option<Size>,
    pub maximum_size: Option<Size>,
    pub side: Option<BorderSide>,
    pub shape: Option<ShapeBorder>,
    pub visual_density: VisualDensity,
    pub alignment: AlignmentGeometry,
}

impl ResolvedMenuPanel {
    /// Upstream's `elevation` on both defaults classes.
    pub const ELEVATION: f32 = 3.0;
    /// Upstream's `_defaultMenuBorder` radius, on both.
    pub const RADIUS: f32 = 4.0;
    /// Upstream's `_kTopLevelMenuHorizontalMinPadding`.
    pub const BAR_PADDING: f32 = 4.0;
    /// Upstream's `_kMenuVerticalMinPadding`.
    pub const MENU_PADDING: f32 = 8.0;
    /// The `?? 0` after the chain, which cannot be reached -- see the type's
    /// docs. Named so the claim has something to point at.
    pub const UNREACHABLE_ELEVATION: f32 = 0.0;

    /// The defaults for an axis: everything the two share, plus the two things
    /// they do not.
    pub fn defaults(axis: MenuPanelAxis, scheme: &ColorScheme) -> ResolvedMenuPanel {
        ResolvedMenuPanel {
            axis,
            background_color: Some(scheme.surface_container()),
            shadow_color: Some(scheme.shadow()),
            surface_tint_color: Some(Color::TRANSPARENT),
            elevation: ResolvedMenuPanel::ELEVATION,
            padding: match axis {
                MenuPanelAxis::Horizontal => {
                    EdgeInsets::symmetric(ResolvedMenuPanel::BAR_PADDING, 0.0)
                }
                MenuPanelAxis::Vertical => {
                    EdgeInsets::symmetric(0.0, ResolvedMenuPanel::MENU_PADDING)
                }
            },
            minimum_size: None,
            fixed_size: None,
            maximum_size: None,
            side: None,
            shape: Some(ShapeBorder::Rounded(
                crate::borders::RoundedRectangleBorder::new(
                    crate::borders::BorderSide::NONE,
                    crate::borders::BorderRadiusGeometry::circular(ResolvedMenuPanel::RADIUS),
                ),
            )),
            visual_density: VisualDensity::STANDARD,
            alignment: match axis {
                MenuPanelAxis::Horizontal => AlignmentGeometry::Directional(
                    crate::render::AlignmentDirectional::BOTTOM_START,
                ),
                MenuPanelAxis::Vertical => {
                    AlignmentGeometry::Directional(crate::render::AlignmentDirectional::TOP_END)
                }
            },
        }
    }

    pub fn of(
        context: &mut BuildContext,
        axis: MenuPanelAxis,
        widget_style: Option<&MenuStyle>,
    ) -> ResolvedMenuPanel {
        let theme = ThemeData::of(context);
        // The axis chooses the theme. Reading both and picking afterwards
        // would give the same answer by accident; upstream does not consult
        // the one it is not using.
        let theme_style = match axis {
            MenuPanelAxis::Horizontal => MenuBarTheme::of(context).style,
            MenuPanelAxis::Vertical => MenuTheme::of(context).style,
        };
        let theme_style = theme_style.as_ref();

        let mut resolved = ResolvedMenuPanel::defaults(axis, &theme.color_scheme);
        resolved.visual_density = theme.visual_density;

        // Upstream's `resolve`: the empty state set, every time.
        let states = WidgetStates::NONE;

        macro_rules! pick {
            ($field:ident) => {
                widget_style
                    .and_then(|style| style.$field.as_ref())
                    .and_then(|property| property.resolve(states))
                    .or_else(|| {
                        theme_style
                            .and_then(|style| style.$field.as_ref())
                            .and_then(|property| property.resolve(states))
                    })
            };
        }

        if let Some(color) = pick!(background_color) {
            resolved.background_color = Some(color);
        }
        if let Some(color) = pick!(shadow_color) {
            resolved.shadow_color = Some(color);
        }
        if let Some(color) = pick!(surface_tint_color) {
            resolved.surface_tint_color = Some(color);
        }
        if let Some(elevation) = pick!(elevation) {
            resolved.elevation = elevation;
        }
        if let Some(padding) = pick!(padding) {
            resolved.padding = padding.resolve(crate::direction::current_direction());
        }
        if let Some(shape) = pick!(shape) {
            resolved.shape = Some(shape);
        }
        resolved.minimum_size = pick!(minimum_size);
        resolved.fixed_size = pick!(fixed_size);
        resolved.maximum_size = pick!(maximum_size);
        resolved.side = pick!(side);

        // Not state properties: a plain `Option` each, two steps and then the
        // default already in place.
        if let Some(density) = widget_style
            .and_then(|style| style.visual_density)
            .or_else(|| theme_style.and_then(|style| style.visual_density))
        {
            resolved.visual_density = density;
        }
        if let Some(alignment) = widget_style
            .and_then(|style| style.alignment)
            .or_else(|| theme_style.and_then(|style| style.alignment))
        {
            resolved.alignment = alignment;
        }
        resolved
    }
}

/// What one line of a menu is drawn with -- upstream's `_MenuButtonDefaultsM3`
/// under `MenuButtonTheme.of`.
///
/// # Two widgets, one theme -- the mirror image of the panel
///
/// `MenuItemButton` and `SubmenuButton` both return `_MenuButtonDefaultsM3`
/// from `defaultStyleOf` and both read `MenuButtonTheme` in `themeStyleOf`.
/// Where [`ResolvedMenuPanel`] is one widget reading two themes chosen by its
/// axis, this is two widgets reading one theme. In neither case does the
/// theme's name name a widget, which is the thing both are easy to get wrong.
///
/// # The label and the icon do not react at all; the overlay is the whole of
/// the feedback
///
/// `foregroundColor` has four arms -- pressed, hovered, focused and the
/// fall-through -- and **all four return `onSurface`**. `iconColor` has the
/// same four, all returning `onSurfaceVariant`. Only `disabled` differs, and it
/// differs by fading.
///
/// `overlayColor` is where the interaction lives: `onSurface` at 0.1 pressed,
/// 0.08 hovered, 0.1 focused, transparent otherwise. So a menu line tells a
/// reader it is under the pointer **by what is painted behind it**, never by
/// recolouring its text. Text that moved would make a menu flicker as the
/// pointer crossed it.
///
/// This is worth stating because it is a case where swapping the order of the
/// pressed, hovered and focused arms of `foregroundColor` is unobservable --
/// not because nothing checks it, but because there is nothing to check. The
/// values are equal. `tools/order_sweep.py` looks for the opposite case, an
/// order that matters and that nothing pins; this is an order that does not
/// matter, and no test can make it.
///
/// Even in `overlayColor` the three are not all distinct: pressed and focused
/// are both 0.1 and only hovered is lighter. A pointer resting on a line is a
/// weaker statement than one pressing it or a keyboard having chosen it.
///
/// # The label is stronger than the icon
///
/// `onSurface` against `onSurfaceVariant`. Both are readable; the label is what
/// is read.
pub struct ResolvedMenuButton {
    pub background: Color,
    pub foreground: Color,
    pub icon_color: Color,
    pub icon_size: f32,
    pub overlay: Color,
    pub elevation: f32,
    pub minimum_size: Size,
    pub maximum_size: Size,
    pub alignment: AlignmentGeometry,
    pub enable_feedback: bool,
}

impl ResolvedMenuButton {
    /// Upstream's `minimumSize`.
    pub const MINIMUM_SIZE: Size = Size::new(64.0, 48.0);
    /// Upstream's `iconSize`.
    pub const ICON_SIZE: f32 = 24.0;
    /// Upstream's disabled fade, on both the label and the icon.
    pub const DISABLED_OPACITY: f32 = 0.38;
    /// Upstream's overlay opacity when pressed, and when focused.
    pub const PRESSED_OVERLAY: f32 = 0.1;
    /// Upstream's overlay opacity when hovered, which is the lighter one.
    pub const HOVERED_OVERLAY: f32 = 0.08;

    /// Upstream's `foregroundColor` resolver: one colour for every state that
    /// is not disabled.
    pub fn foreground_for(states: WidgetStates, scheme: &ColorScheme) -> Color {
        if states.contains(WidgetState::Disabled) {
            return crate::elevation_overlay::with_opacity(
                scheme.on_surface,
                ResolvedMenuButton::DISABLED_OPACITY,
            );
        }
        scheme.on_surface
    }

    /// Upstream's `iconColor` resolver, which is the same shape in a weaker
    /// colour.
    pub fn icon_color_for(states: WidgetStates, scheme: &ColorScheme) -> Color {
        if states.contains(WidgetState::Disabled) {
            return crate::elevation_overlay::with_opacity(
                scheme.on_surface_variant(),
                ResolvedMenuButton::DISABLED_OPACITY,
            );
        }
        scheme.on_surface_variant()
    }

    /// Upstream's `overlayColor` resolver, where the interaction actually
    /// shows.
    pub fn overlay_for(states: WidgetStates, scheme: &ColorScheme) -> Color {
        let opacity = if states.contains(WidgetState::Pressed) {
            ResolvedMenuButton::PRESSED_OVERLAY
        } else if states.contains(WidgetState::Hovered) {
            ResolvedMenuButton::HOVERED_OVERLAY
        } else if states.contains(WidgetState::Focused) {
            ResolvedMenuButton::PRESSED_OVERLAY
        } else {
            return Color::TRANSPARENT;
        };
        crate::elevation_overlay::with_opacity(scheme.on_surface, opacity)
    }

    pub fn of(context: &mut BuildContext, states: WidgetStates) -> ResolvedMenuButton {
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let style = MenuButtonTheme::of(context).style;
        let style = style.as_ref();

        macro_rules! pick {
            ($field:ident) => {
                style
                    .and_then(|style| style.$field.as_ref())
                    .and_then(|property| property.resolve(states))
            };
        }

        ResolvedMenuButton {
            // Transparent, not the surface: a menu line sits on the panel's
            // background and painting its own would draw the panel twice.
            background: pick!(background_color).unwrap_or(Color::TRANSPARENT),
            foreground: pick!(foreground_color)
                .unwrap_or_else(|| ResolvedMenuButton::foreground_for(states, &scheme)),
            icon_color: pick!(icon_color)
                .unwrap_or_else(|| ResolvedMenuButton::icon_color_for(states, &scheme)),
            icon_size: pick!(icon_size).unwrap_or(ResolvedMenuButton::ICON_SIZE),
            overlay: pick!(overlay_color)
                .unwrap_or_else(|| ResolvedMenuButton::overlay_for(states, &scheme)),
            elevation: pick!(elevation).unwrap_or(0.0),
            minimum_size: pick!(minimum_size).unwrap_or(ResolvedMenuButton::MINIMUM_SIZE),
            maximum_size: pick!(maximum_size).unwrap_or(Size::new(f32::INFINITY, f32::INFINITY)),
            alignment: style.and_then(|style| style.alignment).unwrap_or(
                AlignmentGeometry::Directional(crate::render::AlignmentDirectional::CENTER_START),
            ),
            enable_feedback: style
                .and_then(|style| style.enable_feedback)
                .unwrap_or(true),
        }
    }
}

/// What a search bar is drawn with -- upstream's `_SearchBarDefaultsM3` under
/// `SearchBarTheme.of`.
///
/// # A bar is a control and a view is a surface, said in the theme's types
///
/// Every field of `SearchBarThemeData` is a `WidgetStateProperty`; every field
/// of `SearchViewThemeData` is a plain nullable. The bar can be pressed and
/// hovered; the view is the thing the results sit on and cannot be.
///
/// That is the same fact [`ResolvedMenuPanel`] carries, and upstream spells it
/// two different ways. There the panel's fields *are* state properties and the
/// build resolves every one of them against the empty set. Here there is
/// nothing to resolve, because the theme was never given state properties to
/// hold. Both say a surface has no states; only one of them can be got wrong by
/// passing the wrong set.
///
/// # A search bar does not react to being focused, and a menu line does
///
/// `overlayColor` is `onSurface` at 0.1 pressed, 0.08 hovered, and
/// **`Colors.transparent` focused** -- identical to the fall-through, and
/// written out anyway. `_MenuButtonDefaultsM3` gives focused the same weight as
/// pressed.
///
/// The difference is what else is on screen to say so. A focused search bar has
/// a caret blinking in it and a keyboard aimed at it; a focused menu line has
/// nothing but the highlight. So one needs the overlay to show the keyboard's
/// position and the other would be saying it twice.
pub struct ResolvedSearchBar {
    pub background_color: Color,
    pub elevation: f32,
    pub shadow_color: Color,
    pub surface_tint_color: Color,
    pub overlay: Color,
    pub side: Option<BorderSide>,
    pub shape: ShapeBorder,
    pub padding: EdgeInsets,
    pub text_style: Option<TextStyle>,
    pub hint_style: Option<TextStyle>,
    pub constraints: BoxConstraints,
    pub text_capitalization: TextCapitalization,
}

impl ResolvedSearchBar {
    /// Upstream's `elevation`, which the view shares.
    pub const ELEVATION: f32 = 6.0;
    /// Upstream's horizontal `padding`, which the view's `barPadding` also is.
    pub const PADDING: f32 = 8.0;
    pub const MIN_WIDTH: f32 = 360.0;
    /// Upstream's `maxWidth`. **The view has none** -- see
    /// [`ResolvedSearchView`].
    pub const MAX_WIDTH: f32 = 800.0;
    pub const MIN_HEIGHT: f32 = 56.0;
    pub const PRESSED_OVERLAY: f32 = 0.1;
    pub const HOVERED_OVERLAY: f32 = 0.08;

    /// Upstream's `overlayColor` resolver. Focused is transparent, which is
    /// the fall-through -- see the type's docs for why that is deliberate.
    pub fn overlay_for(states: WidgetStates, scheme: &ColorScheme) -> Color {
        let opacity = if states.contains(WidgetState::Pressed) {
            ResolvedSearchBar::PRESSED_OVERLAY
        } else if states.contains(WidgetState::Hovered) {
            ResolvedSearchBar::HOVERED_OVERLAY
        } else {
            return Color::TRANSPARENT;
        };
        crate::elevation_overlay::with_opacity(scheme.on_surface, opacity)
    }

    pub fn of(context: &mut BuildContext, states: WidgetStates) -> ResolvedSearchBar {
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let data = SearchBarTheme::of(context);

        macro_rules! pick {
            ($field:ident) => {
                data.$field
                    .as_ref()
                    .and_then(|property| property.resolve(states))
            };
        }

        ResolvedSearchBar {
            background_color: pick!(background_color)
                .unwrap_or_else(|| scheme.surface_container_high()),
            elevation: pick!(elevation).unwrap_or(ResolvedSearchBar::ELEVATION),
            shadow_color: pick!(shadow_color).unwrap_or_else(|| scheme.shadow()),
            surface_tint_color: pick!(surface_tint_color).unwrap_or(Color::TRANSPARENT),
            overlay: pick!(overlay_color)
                .unwrap_or_else(|| ResolvedSearchBar::overlay_for(states, &scheme)),
            // No default side: a bar is told apart from its background by
            // being a different colour, not by being outlined.
            side: pick!(side),
            shape: pick!(shape).unwrap_or(ShapeBorder::Stadium(
                crate::borders::StadiumBorder::default(),
            )),
            padding: pick!(padding)
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::symmetric(ResolvedSearchBar::PADDING, 0.0)),
            text_style: pick!(text_style).or_else(|| {
                theme.text_theme.body_large.clone().map(|style| TextStyle {
                    color: scheme.on_surface,
                    ..style
                })
            }),
            hint_style: pick!(hint_style).or_else(|| {
                theme.text_theme.body_large.clone().map(|style| TextStyle {
                    color: scheme.on_surface_variant(),
                    ..style
                })
            }),
            constraints: data.constraints.unwrap_or(BoxConstraints {
                min_width: ResolvedSearchBar::MIN_WIDTH,
                max_width: ResolvedSearchBar::MAX_WIDTH,
                min_height: ResolvedSearchBar::MIN_HEIGHT,
                max_height: f32::INFINITY,
            }),
            text_capitalization: data.text_capitalization.unwrap_or(TextCapitalization::None),
        }
    }
}

/// What the view a search bar opens is drawn with -- upstream's
/// `_SearchViewDefaultsM3` under `SearchViewTheme.of`.
///
/// # The view's bar padding is the bar's padding
///
/// Both are `EdgeInsets.symmetric(horizontal: 8)`. The view's header *is* a
/// search bar, and a header that padded its field differently from the bar
/// that opened it would read as a second, unrelated field appearing where the
/// first one was.
///
/// The two also share `surfaceContainerHigh`, elevation 6, a transparent tint,
/// and `bodyLarge` on `onSurface` for the text with `onSurfaceVariant` for the
/// hint. The view is the bar, grown.
///
/// # Except that the bar is capped and the view is not
///
/// The bar is `minWidth 360, maxWidth 800, minHeight 56`; the view is
/// `minWidth 360, minHeight 240` and **no maximum width at all**. A line of
/// text stops being readable past a certain width, so the bar is capped. A
/// region holding results does not, so it is not. And 56 against 240 is the
/// same statement twice: one is a line, the other is a place.
///
/// # Full screen takes the corners off
///
/// The shape is 28-radius when docked and a plain rectangle when full screen.
/// A full-screen view has no corners to round -- rounding them would draw a
/// card floating on a background that is not there.
///
/// This is the one default that depends on something that is neither the theme
/// nor the widget: `isFullScreen` is a constructor argument of the defaults
/// class itself, the way `context` is.
pub struct ResolvedSearchView {
    pub background_color: Color,
    pub elevation: f32,
    pub surface_tint_color: Color,
    pub side: Option<BorderSide>,
    pub shape: ShapeBorder,
    pub header_height: Option<f32>,
    pub header_text_style: Option<TextStyle>,
    pub header_hint_style: Option<TextStyle>,
    pub constraints: BoxConstraints,
    pub padding: Option<EdgeInsets>,
    pub bar_padding: EdgeInsets,
    pub shrink_wrap: bool,
    pub divider_color: Color,
}

impl ResolvedSearchView {
    /// Upstream's `elevation`, shared with the bar.
    pub const ELEVATION: f32 = 6.0;
    /// Upstream's docked corner radius.
    pub const RADIUS: f32 = 28.0;
    /// Upstream's `fullScreenBarHeight`, which is not the docked one.
    pub const FULL_SCREEN_BAR_HEIGHT: f32 = 72.0;
    pub const MIN_WIDTH: f32 = 360.0;
    /// Upstream's `minHeight`, against the bar's 56.
    pub const MIN_HEIGHT: f32 = 240.0;

    pub fn of(context: &mut BuildContext, is_full_screen: bool) -> ResolvedSearchView {
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let data = SearchViewTheme::of(context);
        let body_large = |color: Color| {
            theme
                .text_theme
                .body_large
                .clone()
                .map(|style| TextStyle { color, ..style })
        };
        ResolvedSearchView {
            background_color: data
                .background_color
                .unwrap_or_else(|| scheme.surface_container_high()),
            elevation: data.elevation.unwrap_or(ResolvedSearchView::ELEVATION),
            surface_tint_color: data.surface_tint_color.unwrap_or(Color::TRANSPARENT),
            side: data.side,
            shape: data.shape.clone().unwrap_or_else(|| {
                ShapeBorder::Rounded(crate::borders::RoundedRectangleBorder::new(
                    crate::borders::BorderSide::NONE,
                    crate::borders::BorderRadiusGeometry::circular(if is_full_screen {
                        0.0
                    } else {
                        ResolvedSearchView::RADIUS
                    }),
                ))
            }),
            // No default: the header is as tall as what is in it unless a
            // theme says otherwise.
            header_height: data.header_height,
            header_text_style: data
                .header_text_style
                .clone()
                .or_else(|| body_large(scheme.on_surface)),
            header_hint_style: data
                .header_hint_style
                .clone()
                .or_else(|| body_large(scheme.on_surface_variant())),
            constraints: data.constraints.unwrap_or(BoxConstraints {
                min_width: ResolvedSearchView::MIN_WIDTH,
                // No maximum: see the type's docs.
                max_width: f32::INFINITY,
                min_height: ResolvedSearchView::MIN_HEIGHT,
                max_height: f32::INFINITY,
            }),
            padding: data
                .padding
                .map(|padding| padding.resolve(crate::direction::current_direction())),
            bar_padding: data
                .bar_padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::symmetric(ResolvedSearchBar::PADDING, 0.0)),
            shrink_wrap: data.shrink_wrap.unwrap_or(false),
            divider_color: data.divider_color.unwrap_or_else(|| scheme.outline()),
        }
    }
}

/// Where a dropdown's insets sit, which is the whole of what
/// `ButtonThemeData.alignedDropdown` decides.
///
/// # One flag is why this Material 2 theme is still alive
///
/// `ButtonTheme.of` is read in exactly three places upstream: once by
/// `ButtonBar`, and twice by `DropdownButton` -- both times for
/// `alignedDropdown` and nothing else. Every other field of `ButtonThemeData`
/// reaches a widget only through the `copyWith` in `ButtonBar.build`. A whole
/// theme kept for one boolean, and the boolean is read by a widget that is not
/// a button.
///
/// # The start inset changes hands; the end inset does not
///
/// Aligned, the button is padded `start 16, end 4` and the menu's margin is
/// zero. Unaligned, the button's padding is zero and the menu's margin is
/// `start 16, end 24`.
///
/// The 16 is the same on both sides of the switch: it moves from the menu to
/// the button and back, so exactly one of the two carries it. The end value
/// does **not** transfer -- 4 against 24 -- because the two ends are doing
/// different jobs. The aligned button's 4 is room beside the arrow; the
/// unaligned menu's 24 is clearance from what it is not lined up with.
///
/// # And the button half of it has a second condition
///
/// The menu margin is chosen on `alignedDropdown` alone. The button padding is
/// chosen on `alignedDropdown && _inputDecoration == null` -- a dropdown inside
/// an `InputDecorator` takes the decoration's padding and ignores this, while
/// still moving its *menu*. The flag half-applies, and which half depends on
/// something the flag has never heard of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropdownAlignment {
    pub button_padding: crate::render::EdgeInsetsDirectional,
    pub menu_margin: crate::render::EdgeInsetsDirectional,
}

impl DropdownAlignment {
    /// The inset that changes hands between the button and the menu.
    pub const SHARED_START: f32 = 16.0;
    /// The aligned button's end padding: room beside the arrow.
    pub const ALIGNED_END: f32 = 4.0;
    /// The unaligned menu's end margin: clearance, which is a different job
    /// and a different number.
    pub const UNALIGNED_END: f32 = 24.0;

    /// Upstream's two pairs of constants.
    ///
    /// `in_input_decorator` is the second condition on the button half only --
    /// see the type's docs.
    pub fn of(aligned: bool, in_input_decorator: bool) -> DropdownAlignment {
        DropdownAlignment {
            button_padding: if aligned && !in_input_decorator {
                crate::render::EdgeInsetsDirectional {
                    start: DropdownAlignment::SHARED_START,
                    top: 0.0,
                    end: DropdownAlignment::ALIGNED_END,
                    bottom: 0.0,
                }
            } else {
                crate::render::EdgeInsetsDirectional::ZERO
            },
            menu_margin: if aligned {
                crate::render::EdgeInsetsDirectional::ZERO
            } else {
                crate::render::EdgeInsetsDirectional {
                    start: DropdownAlignment::SHARED_START,
                    top: 0.0,
                    end: DropdownAlignment::UNALIGNED_END,
                    bottom: 0.0,
                }
            },
        }
    }

    /// What a `DropdownButton` gets from the ambient `ButtonTheme`.
    pub fn from_theme(context: &mut BuildContext, in_input_decorator: bool) -> DropdownAlignment {
        DropdownAlignment::of(
            ButtonTheme::of(context).aligned_dropdown,
            in_input_decorator,
        )
    }
}

/// How `IconButtonTheme` reaches an icon button -- and it reaches four widgets
/// two different ways.
///
/// # Two verbs, and they give opposite precedence
///
/// `IconButton.themeStyleOf` writes
/// `IconButtonTheme.of(context).style?.merge(iconThemeStyle) ?? iconThemeStyle`.
/// `ListTile.build` and `AppBar.build` write
/// `IconButtonTheme.of(context).style?.copyWith(foregroundColor: ...)`.
///
/// `merge` keeps this style's fields and takes the other's only where this one
/// is null, so **the theme wins and the ambient `IconTheme` fills its gaps**.
/// Upstream's own doc says it: *"if any of the properties exist in both
/// [IconButtonTheme] and [IconTheme], [IconTheme] will be overridden."*
///
/// `copyWith` replaces the named field outright, so **the reader wins**.
///
/// Same theme, opposite answer, and which one a caller gets depends on what
/// they put the button inside. That is not an inconsistency: a bare
/// `IconButton` has no opinion about what is behind it and should defer, while
/// a `ListTile` or an `AppBar` painted its own background and has to impose a
/// colour that reads against it. A theme that could recolour an app bar's
/// actions into invisibility would be a theme that could break the app bar.
///
/// # A merge is per-field, which a `??` ladder is not
///
/// The distinction does work here. A theme that sets only the foreground still
/// lets the `IconTheme`'s size through, because the merge asks each field
/// separately. Written as `themeStyle ?? iconThemeStyle` the first non-null
/// style would have taken everything, and setting one field would have silently
/// dropped the other.
///
/// # The ambient icon theme is filtered before it enters
///
/// `iconThemeStyle` is built from `isDefaultColor ? null : iconTheme.color` and
/// the same for the size, so the `IconTheme` contributes **only what was
/// deliberately set**. It is a third source that has already had its defaults
/// removed, which is why merging it under the theme is safe: it cannot
/// re-assert a fallback the theme was trying to replace.
///
/// # Where this port cannot follow upstream
///
/// `isDefaultColor` is `identical(iconTheme.color, kDefaultIconDarkColor)` --
/// **object identity, not equality**. In Dart a `const` colour with the same
/// value is canonicalised and so *is* identical, while a non-const one holding
/// the same value is not, and would count as deliberately set. The behaviour
/// therefore turns on whether the caller's colour was const.
///
/// A Rust `Color` is `Copy` and has no identity to compare, so this port
/// compares by value. That matches upstream for the const case, which is the
/// documented and ordinary one, and differs for a non-const colour that happens
/// to equal the default -- where upstream treats it as set and this treats it
/// as absent. Recorded rather than papered over: it is a real difference and
/// the alternative would be inventing an identity Rust does not have.
pub struct ResolvedIconButton {
    /// What the theme and the ambient icon theme make between them, before the
    /// button's own defaults are merged under it.
    pub style: ButtonStyle,
}

impl ResolvedIconButton {
    /// Upstream's `kDefaultIconDarkColor`, the light-mode default.
    pub const DEFAULT_DARK: Color = Color(0xDD000000);
    /// Upstream's `kDefaultIconLightColor`.
    pub const DEFAULT_LIGHT: Color = Color(0xFFFFFFFF);
    /// Upstream's `IconThemeData.fallback().size`.
    pub const DEFAULT_SIZE: f32 = 24.0;

    /// Upstream's `iconThemeStyle`: the ambient icon theme with its defaults
    /// removed, so only what somebody chose survives.
    pub fn from_icon_theme(icon_theme: &IconThemeData, is_dark: bool) -> ButtonStyle {
        let default_color = if is_dark {
            ResolvedIconButton::DEFAULT_LIGHT
        } else {
            ResolvedIconButton::DEFAULT_DARK
        };
        let mut style = ButtonStyle::new();
        if let Some(color) = icon_theme.color {
            if color != default_color {
                style.foreground_color = Some(StateProperty::all(Some(color)));
            }
        }
        if let Some(size) = icon_theme.size {
            if size != ResolvedIconButton::DEFAULT_SIZE {
                style.icon_size = Some(StateProperty::all(Some(size)));
            }
        }
        style
    }

    /// Upstream's `IconButton.themeStyleOf`: the theme merged over the filtered
    /// icon theme, and the icon theme alone where there is no theme.
    pub fn of(context: &mut BuildContext, icon_theme: &IconThemeData) -> ResolvedIconButton {
        let is_dark = ThemeData::of(context).brightness() == crate::platform::Brightness::Dark;
        let from_icons = ResolvedIconButton::from_icon_theme(icon_theme, is_dark);
        let style = match IconButtonTheme::of(context).style {
            Some(theme_style) => theme_style.merge(&from_icons),
            None => from_icons,
        };
        ResolvedIconButton { style }
    }

    /// What a `ListTile` or an `AppBar` does instead: take the theme's style if
    /// there is one and **overwrite** the foreground, or start from nothing and
    /// set it.
    ///
    /// The `copyWith` half of the two verbs -- see the type's docs.
    pub fn forced_foreground(context: &mut BuildContext, foreground: Color) -> ResolvedIconButton {
        let mut style = IconButtonTheme::of(context).style.unwrap_or_default();
        style.foreground_color = Some(StateProperty::all(Some(foreground)));
        ResolvedIconButton { style }
    }

    /// The foreground this resolution lands on, for a given state.
    pub fn foreground(&self, states: WidgetStates) -> Option<Color> {
        self.style
            .foreground_color
            .as_ref()
            .and_then(|property| property.resolve(states))
    }

    /// The icon size this resolution lands on.
    pub fn icon_size(&self, states: WidgetStates) -> Option<f32> {
        self.style
            .icon_size
            .as_ref()
            .and_then(|property| property.resolve(states))
    }
}

/// Which of `InputDecoration`'s five named borders a field is drawn with.
///
/// Six states -- enabled or not, focused or not, in error or not -- and five
/// borders to cover them, because one of the five covers two of the cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputBorderSlot {
    Error,
    Disabled,
    FocusedError,
    Focused,
    Enabled,
}

/// Where the *side* of a resolved input border comes from, once its shape is
/// settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputBorderSide {
    /// The caller's own, kept as given -- see [`ResolvedInputBorder`] for the
    /// two ways that happens.
    AsGiven,
    /// A filled field's rule, from `activeIndicatorBorder`.
    ActiveIndicator,
    /// An unfilled field's outline, from `outlineBorder`.
    Outline,
    /// Material 2, which computes a width rather than reading a side.
    MaterialTwo,
}

/// Which of a decorated field's words is being asked about.
///
/// Five slots and not four: the label and the *floating* label are separate
/// fields upstream, and separate here, even though Material 3's two tables
/// give them character for character the same answer. Material 2's do not --
/// a floating label turns primary when the field is focused and an inline one
/// does not -- and folding them would lose that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputTextSlot {
    Hint,
    Label,
    FloatingLabel,
    Helper,
    Error,
}

/// What the words in a decorated field are drawn in.
///
/// Upstream's `_InputDecoratorDefaultsM2` and `_InputDecoratorDefaultsM3`,
/// which between them are the material library's only readers of
/// `ThemeData.hintColor`.
///
/// # Two things Material 2 does that Material 3 does not
///
/// A disabled field's helper and error lines go **transparent** under
/// Material 2 rather than faint. That is not a way of saying "very faint": it
/// is how the line is hidden *without changing the layout*, so a field does
/// not change height when it is disabled. Material 3 fades them to 38%
/// instead and lets them be read.
///
/// And Material 2's hint and inline label are `hintColor` with **no text role
/// at all** -- a bare `TextStyle` carrying only a colour, which upstream
/// merges over whatever the field's own style is. Material 3 gives them
/// `bodyLarge` and `bodySmall` outright.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedInputTextStyles {
    pub hint: TextStyle,
    pub label: TextStyle,
    pub floating_label: TextStyle,
    pub helper: TextStyle,
    pub error: TextStyle,
}

impl ResolvedInputTextStyles {
    /// Upstream's disabled and hint opacity in the Material 3 tables.
    pub const DISABLED_OPACITY: f32 = 0.38;

    pub fn of(context: &mut BuildContext, states: WidgetStates) -> ResolvedInputTextStyles {
        let theme = ThemeData::of(context);
        let data = InputDecorationTheme::of(context);
        ResolvedInputTextStyles {
            hint: ResolvedInputTextStyles::style_for(
                &theme,
                InputTextSlot::Hint,
                states,
                data.hint_style.clone(),
            ),
            label: ResolvedInputTextStyles::style_for(
                &theme,
                InputTextSlot::Label,
                states,
                data.label_style.clone(),
            ),
            floating_label: ResolvedInputTextStyles::style_for(
                &theme,
                InputTextSlot::FloatingLabel,
                states,
                data.floating_label_style.clone(),
            ),
            helper: ResolvedInputTextStyles::style_for(
                &theme,
                InputTextSlot::Helper,
                states,
                data.helper_style.clone(),
            ),
            error: ResolvedInputTextStyles::style_for(
                &theme,
                InputTextSlot::Error,
                states,
                data.error_style.clone(),
            ),
        }
    }

    /// One slot, under one state set. `asked` is what the theme said, which
    /// wins outright when it said anything.
    pub fn style_for(
        theme: &ThemeData,
        slot: InputTextSlot,
        states: WidgetStates,
        asked: Option<TextStyle>,
    ) -> TextStyle {
        if let Some(style) = asked {
            return style;
        }
        let role = match slot {
            // Material 2 gives the hint and the label a colour and no role.
            // Material 3 gives the label `bodyLarge` and leaves the hint's
            // role alone too -- its `hintStyle` is a bare `TextStyle(color:)`
            // in both tables.
            InputTextSlot::Hint => None,
            InputTextSlot::Label | InputTextSlot::FloatingLabel => {
                if theme.use_material3 {
                    theme.text_theme.body_large.clone()
                } else {
                    None
                }
            }
            InputTextSlot::Helper | InputTextSlot::Error => theme.text_theme.body_small.clone(),
        };
        TextStyle {
            color: ResolvedInputTextStyles::color_for(theme, slot, states),
            ..role.unwrap_or_default()
        }
    }

    /// The ink alone, which is the whole of what the two tables disagree
    /// about.
    pub fn color_for(theme: &ThemeData, slot: InputTextSlot, states: WidgetStates) -> Color {
        let scheme = theme.color_scheme;
        let disabled = states.contains(WidgetState::Disabled);
        let has_error = states.contains(WidgetState::Error);
        let focused = states.contains(WidgetState::Focused);
        let hovered = states.contains(WidgetState::Hovered);

        if !theme.use_material3 {
            return match slot {
                // A disabled field's helper and error go transparent, not
                // faint: the line keeps its room and stops being readable.
                InputTextSlot::Helper if disabled => Color::TRANSPARENT,
                InputTextSlot::Error if disabled => Color::TRANSPARENT,
                InputTextSlot::Error => scheme.error,
                _ if disabled => theme.disabled_color,
                // Only the *floating* label answers to error and focus under
                // Material 2. The inline one is `hintColor` whatever the
                // field is doing, which is what makes these two slots and not
                // one.
                InputTextSlot::FloatingLabel if has_error => scheme.error,
                InputTextSlot::FloatingLabel if focused => scheme.primary,
                _ => theme.hint_color,
            };
        }

        let faint = scheme
            .on_surface
            .with_alpha((255.0 * ResolvedInputTextStyles::DISABLED_OPACITY).round() as u8);
        match slot {
            // Material 3's error line has no disabled branch at all: a
            // complaint stays red on a field that cannot be edited, because
            // the reader still has to know why it is refused.
            InputTextSlot::Error => scheme.error,
            _ if disabled => faint,
            InputTextSlot::Label | InputTextSlot::FloatingLabel if has_error => {
                // Two of the three arms are `error`; only the hovered one is
                // not, and it is `onErrorContainer` -- a hover is a promise
                // that the field can be fixed, so it softens rather than
                // shouts.
                if hovered && !focused {
                    scheme.on_error_container()
                } else {
                    scheme.error
                }
            }
            InputTextSlot::Label | InputTextSlot::FloatingLabel if focused => scheme.primary,
            _ => scheme.on_surface_variant(),
        }
    }
}

/// How an input decoration's border is arrived at -- upstream's
/// `_InputDecoratorState._getDefaultBorder` and the five-way pick above it.
///
/// # A border's shape and its side come from different places
///
/// `_getDefaultBorder` takes the border for its **shape** -- underline, outline,
/// whatever the caller asked for -- and then calls `copyWith(borderSide: ...)`
/// with a side from somewhere else entirely. The caller says what outline the
/// field has; the theme says what colour and width it is drawn in.
///
/// # Error beats disabled
///
/// The five-way pick reads
/// `!enabled ? (error ? errorBorder : disabledBorder) : focused ? (error ?
/// focusedErrorBorder : focusedBorder) : (error ? errorBorder : enabledBorder)`.
/// `errorBorder` appears in two of the three arms, and the disabled arm is one
/// of them: **a field you cannot edit still tells you it is wrong.** Being
/// unable to fix something is not a reason to stop being told about it.
///
/// # Two ways to keep the side you were given
///
/// * The border is a `WidgetStateProperty<InputBorder>`. A caller answering per
///   state is already doing what the code below would do, so it steps aside
///   rather than resolving twice.
/// * The border's side is `BorderSide.none`. Replacing it would put a line back
///   on a border that asked for none -- and, as
///   [`crate::input_decorator::ShapedInputBorder`] records, a border with no
///   side still has a shape, so asking for none is a decision and not an
///   absence.
///
/// # Filled and unfilled read different fields, and only one reads the theme
///
/// Filled fields take the side from
/// `InputDecorationTheme.of(context).activeIndicatorBorder ??
/// defaults.activeIndicatorBorder`. Unfilled fields take it from
/// `defaults.outlineBorder` **alone**.
///
/// `defaults` is `_InputDecoratorDefaultsM3(context)`, not the ambient theme,
/// and `applyDefaults` -- which folds the theme into the decoration higher up
/// -- carries neither field, because neither has an `InputDecoration`
/// counterpart. So `InputDecorationThemeData.outlineBorder` is a public,
/// documented field whose value the decorator never reads: it appears once more
/// in the file, inside a `??` chain that only asks whether *any* field is
/// non-null.
///
/// The filled branch reaching for `InputDecorationTheme.of(context)` a second
/// time, when the theme is already folded into `decoration`, is the tell --
/// somebody needed a theme field `applyDefaults` does not carry and went and
/// got it. They did that for the active indicator and not for the outline.
///
/// # The two default ladders are the same ladder, two values apart
///
/// `_InputDecoratorDefaultsM3.activeIndicatorBorder` and `.outlineBorder` have
/// the same six arms in the same order -- disabled, error+focused, error+hovered,
/// error, focused, hovered, resting -- and differ in exactly two of them:
///
/// * **disabled**: `onSurface` at 0.38 for the indicator, at **0.12** for the
///   outline. Three times fainter for the shape that encloses more. A single
///   rule under the text has to stay legible as a line; a box drawn all the way
///   round a dead field at that strength would read as a live one.
/// * **resting**: `onSurfaceVariant` for the indicator, `outline` for the
///   outline. The indicator is nearly content and takes a text role; the
///   outline is a container edge and takes the role named for it.
///
/// Everything else -- both error widths, the focused 2.0, the hover -- is
/// shared. And within the error arm focused still wins the width: **the colour
/// says what is wrong and the width says where you are.**
pub struct ResolvedInputBorder {
    pub slot: InputBorderSlot,
    pub side: InputBorderSide,
}

impl ResolvedInputBorder {
    /// Upstream's focused width, in both ladders and in Material 2.
    pub const FOCUSED_WIDTH: f32 = 2.0;
    /// Upstream's resting width.
    pub const RESTING_WIDTH: f32 = 1.0;
    /// The width for a field with no line to draw -- collapsed, borderless or
    /// disabled, which Material 2 folds into one case.
    pub const NO_WIDTH: f32 = 0.0;
    /// Upstream's disabled opacity for a filled field's indicator.
    pub const DISABLED_INDICATOR_OPACITY: f32 = 0.38;
    /// Upstream's disabled opacity for an unfilled field's outline, which is
    /// the fainter of the two.
    pub const DISABLED_OUTLINE_OPACITY: f32 = 0.12;

    /// The five-way pick. See the type's docs: error beats disabled.
    pub fn slot_for(enabled: bool, focused: bool, has_error: bool) -> InputBorderSlot {
        if !enabled {
            return if has_error {
                InputBorderSlot::Error
            } else {
                InputBorderSlot::Disabled
            };
        }
        if focused {
            return if has_error {
                InputBorderSlot::FocusedError
            } else {
                InputBorderSlot::Focused
            };
        }
        if has_error {
            InputBorderSlot::Error
        } else {
            InputBorderSlot::Enabled
        }
    }

    /// Where the side comes from, or that it is kept as given.
    ///
    /// `border_is_state_property` and `side_is_none` are the two early returns.
    pub fn side_for(
        border_is_state_property: bool,
        side_is_none: bool,
        use_material3: bool,
        filled: bool,
    ) -> InputBorderSide {
        if border_is_state_property || side_is_none {
            return InputBorderSide::AsGiven;
        }
        if !use_material3 {
            return InputBorderSide::MaterialTwo;
        }
        if filled {
            InputBorderSide::ActiveIndicator
        } else {
            InputBorderSide::Outline
        }
    }

    /// Upstream's Material 2 width: zero when there is nothing to draw, two
    /// when focused, one otherwise.
    ///
    /// The zero case folds three different reasons together -- a collapsed
    /// field, a border explicitly set to none, and a disabled field -- because
    /// the drawn result is the same and only the reason differs.
    pub fn material_two_width(
        is_collapsed: bool,
        border_is_none: bool,
        enabled: bool,
        focused: bool,
    ) -> f32 {
        if is_collapsed || border_is_none || !enabled {
            return ResolvedInputBorder::NO_WIDTH;
        }
        if focused {
            ResolvedInputBorder::FOCUSED_WIDTH
        } else {
            ResolvedInputBorder::RESTING_WIDTH
        }
    }

    /// The colour of a Material 3 side, by state, for whichever of the two
    /// ladders is in play. They share every arm but two.
    pub fn side_color(
        side: InputBorderSide,
        states: WidgetStates,
        scheme: &ColorScheme,
    ) -> Option<Color> {
        let outline = match side {
            InputBorderSide::ActiveIndicator => false,
            InputBorderSide::Outline => true,
            _ => return None,
        };
        if states.contains(WidgetState::Disabled) {
            // The one arm where the two ladders differ by more than a role.
            return Some(crate::elevation_overlay::with_opacity(
                scheme.on_surface,
                if outline {
                    ResolvedInputBorder::DISABLED_OUTLINE_OPACITY
                } else {
                    ResolvedInputBorder::DISABLED_INDICATOR_OPACITY
                },
            ));
        }
        if states.contains(WidgetState::Error) {
            if states.contains(WidgetState::Focused) {
                return Some(scheme.error);
            }
            if states.contains(WidgetState::Hovered) {
                return Some(scheme.on_error_container());
            }
            return Some(scheme.error);
        }
        if states.contains(WidgetState::Focused) {
            return Some(scheme.primary);
        }
        if states.contains(WidgetState::Hovered) {
            return Some(scheme.on_surface);
        }
        Some(if outline {
            scheme.outline()
        } else {
            scheme.on_surface_variant()
        })
    }

    /// The width of a Material 3 side: two when focused, one otherwise, in
    /// both ladders and whether or not there is an error.
    pub fn side_width(states: WidgetStates) -> f32 {
        if states.contains(WidgetState::Focused) {
            ResolvedInputBorder::FOCUSED_WIDTH
        } else {
            ResolvedInputBorder::RESTING_WIDTH
        }
    }

    pub fn of(
        context: &mut BuildContext,
        enabled: bool,
        focused: bool,
        has_error: bool,
        border_is_state_property: bool,
        side_is_none: bool,
    ) -> ResolvedInputBorder {
        let theme = ThemeData::of(context);
        // Read for the same reason upstream reads it: `filled` is one of the
        // fields `applyDefaults` folds into the decoration, so the theme is
        // what decides it when the caller did not.
        let filled = InputDecorationTheme::of(context).filled;
        ResolvedInputBorder {
            slot: ResolvedInputBorder::slot_for(enabled, focused, has_error),
            side: ResolvedInputBorder::side_for(
                border_is_state_property,
                side_is_none,
                theme.use_material3,
                filled,
            ),
        }
    }
}

/// What one segment of a segmented button is drawn with -- upstream's
/// `_SegmentedButtonDefaultsM3.style` under `SegmentedButtonTheme.of`.
///
/// # Only two states matter, and upstream writes eight arms anyway
///
/// `foregroundColor` has a four-arm ladder for selected and another for
/// unselected -- pressed, hovered, focused, fall-through -- and **all four of
/// each return the same colour**: `onSecondaryContainer` selected,
/// `onSurface` not. Only `selected` and `disabled` change the answer.
///
/// So the label does not react to being touched, exactly as
/// [`ResolvedMenuButton`]'s does not, and for the same reason: the feedback
/// lives in the overlay. What is different here is that the ladder is
/// *doubled* -- eight written arms collapsing to two values -- because the
/// generator emits the full cross product whether or not the tokens differ.
///
/// # A disabled segment has no container, selected or not
///
/// `backgroundColor` checks disabled **before** selected and returns null for
/// it, and returns null for unselected too. So a segment is filled only when
/// it is selected *and* enabled: disabling a selected segment takes the pill
/// away entirely rather than fading it.
///
/// That reads backwards until you notice what is left. The tick stays, the
/// outline stays, and the label stays at 38 per cent -- three ways of saying
/// "this one, and you cannot have it". A faded container would have been a
/// fourth saying the same thing, in the one channel that also has to keep
/// working for the segments beside it.
///
/// # The overlay carries the interaction, and it is the same shape twice over
///
/// `onSecondaryContainer` when selected and `onSurface` when not, at 0.1
/// pressed, 0.08 hovered, 0.1 focused. Pressed and focused agree and only
/// hovering is lighter -- the same ordering as a menu line, with the same
/// reason: a pointer resting on something is a weaker statement than pressing
/// it or having chosen it with a keyboard.
///
/// # Two spellings of nothing, in one file
///
/// The defaults class's `overlayColor` falls through to **null**;
/// `resolveStateColor`'s map, a few lines below, falls through to
/// **`Colors.transparent`**. Both mean no overlay and the painted result is the
/// same, so nothing forces them to agree -- which is why they do not.
///
/// # The disabled outline is 0.12, the same number the input border uses
///
/// [`ResolvedInputBorder::DISABLED_OUTLINE_OPACITY`] is also
/// `onSurface` at 0.12. Two unrelated components, the same role -- a line
/// tracing the edge of something dead -- and the same number. Where the
/// disabled *foreground* is 0.38 in both, because that is text.
pub struct ResolvedSegmentedButton {
    pub background: Option<Color>,
    pub foreground: Color,
    pub overlay: Option<Color>,
    pub side: BorderSide,
    pub surface_tint: Color,
    pub elevation: f32,
    pub icon_size: f32,
    pub minimum_height: f32,
}

impl ResolvedSegmentedButton {
    /// Upstream's `iconSize`, which is smaller than a button's usual 24 -- a
    /// segment's tick sits beside a label rather than standing alone.
    pub const ICON_SIZE: f32 = 18.0;
    /// Upstream's `minimumSize`, which is `Size.fromHeight` -- a height and no
    /// width, because a segment is as wide as its label and the row divides
    /// what is there.
    pub const MINIMUM_HEIGHT: f32 = 40.0;
    /// Upstream's disabled opacity for the label.
    pub const DISABLED_FOREGROUND_OPACITY: f32 = 0.38;
    /// Upstream's disabled opacity for the outline -- the same 0.12 the input
    /// border uses for the same job.
    pub const DISABLED_SIDE_OPACITY: f32 = 0.12;
    pub const PRESSED_OVERLAY: f32 = 0.1;
    pub const HOVERED_OVERLAY: f32 = 0.08;

    /// Upstream's `backgroundColor` resolver. Disabled is checked first, so a
    /// disabled segment has no container whether or not it is selected.
    pub fn background_for(states: WidgetStates, scheme: &ColorScheme) -> Option<Color> {
        if states.contains(WidgetState::Disabled) {
            return None;
        }
        if states.contains(WidgetState::Selected) {
            return Some(scheme.secondary_container());
        }
        None
    }

    /// Upstream's `foregroundColor` resolver, with its eight arms collapsed to
    /// the two answers they give.
    pub fn foreground_for(states: WidgetStates, scheme: &ColorScheme) -> Color {
        if states.contains(WidgetState::Disabled) {
            return crate::elevation_overlay::with_opacity(
                scheme.on_surface,
                ResolvedSegmentedButton::DISABLED_FOREGROUND_OPACITY,
            );
        }
        if states.contains(WidgetState::Selected) {
            scheme.on_secondary_container()
        } else {
            scheme.on_surface
        }
    }

    /// Upstream's `overlayColor` resolver: the same three opacities over
    /// whichever colour the selection picked.
    pub fn overlay_for(states: WidgetStates, scheme: &ColorScheme) -> Option<Color> {
        let base = if states.contains(WidgetState::Selected) {
            scheme.on_secondary_container()
        } else {
            scheme.on_surface
        };
        let opacity = if states.contains(WidgetState::Pressed) {
            ResolvedSegmentedButton::PRESSED_OVERLAY
        } else if states.contains(WidgetState::Hovered) {
            ResolvedSegmentedButton::HOVERED_OVERLAY
        } else if states.contains(WidgetState::Focused) {
            ResolvedSegmentedButton::PRESSED_OVERLAY
        } else {
            // Upstream's `null`, which the helper beside it spells
            // `Colors.transparent` -- see the type's docs.
            return None;
        };
        Some(crate::elevation_overlay::with_opacity(base, opacity))
    }

    /// Upstream's `side` resolver, which has only the two arms the others
    /// pretend to have more of.
    pub fn side_for(states: WidgetStates, scheme: &ColorScheme) -> BorderSide {
        let color = if states.contains(WidgetState::Disabled) {
            crate::elevation_overlay::with_opacity(
                scheme.on_surface,
                ResolvedSegmentedButton::DISABLED_SIDE_OPACITY,
            )
        } else {
            scheme.outline()
        };
        BorderSide {
            color,
            width: 1.0,
            ..BorderSide::NONE
        }
    }

    /// Upstream's `resolveStateColor`: one `overlayColor` stands in for both
    /// the selected and the unselected source.
    ///
    /// A caller who names an overlay has said what the interaction looks like
    /// in both states at once, so neither of the other two is consulted --
    /// which is what makes it one knob rather than a third.
    pub fn state_color(
        unselected: Option<Color>,
        selected: Option<Color>,
        overlay: Option<Color>,
        states: WidgetStates,
    ) -> Option<Color> {
        let base = if states.contains(WidgetState::Selected) {
            overlay.or(selected)
        } else {
            overlay.or(unselected)
        }?;
        let opacity = if states.contains(WidgetState::Pressed) {
            ResolvedSegmentedButton::PRESSED_OVERLAY
        } else if states.contains(WidgetState::Hovered) {
            ResolvedSegmentedButton::HOVERED_OVERLAY
        } else if states.contains(WidgetState::Focused) {
            ResolvedSegmentedButton::PRESSED_OVERLAY
        } else {
            return Some(Color::TRANSPARENT);
        };
        Some(crate::elevation_overlay::with_opacity(base, opacity))
    }

    pub fn of(context: &mut BuildContext, states: WidgetStates) -> ResolvedSegmentedButton {
        let scheme = ThemeData::of(context).color_scheme;
        let style = SegmentedButtonTheme::of(context).style;
        let style = style.as_ref();

        macro_rules! pick {
            ($field:ident) => {
                style
                    .and_then(|style| style.$field.as_ref())
                    .and_then(|property| property.resolve(states))
            };
        }

        ResolvedSegmentedButton {
            background: pick!(background_color)
                .or_else(|| ResolvedSegmentedButton::background_for(states, &scheme)),
            foreground: pick!(foreground_color)
                .unwrap_or_else(|| ResolvedSegmentedButton::foreground_for(states, &scheme)),
            overlay: pick!(overlay_color)
                .or_else(|| ResolvedSegmentedButton::overlay_for(states, &scheme)),
            side: pick!(side).unwrap_or_else(|| ResolvedSegmentedButton::side_for(states, &scheme)),
            surface_tint: pick!(surface_tint_color).unwrap_or(Color::TRANSPARENT),
            elevation: pick!(elevation).unwrap_or(0.0),
            icon_size: pick!(icon_size).unwrap_or(ResolvedSegmentedButton::ICON_SIZE),
            minimum_height: pick!(minimum_size)
                .map(|size| size.height)
                .unwrap_or(ResolvedSegmentedButton::MINIMUM_HEIGHT),
        }
    }
}

/// What a dropdown menu is drawn with -- upstream's `_DropdownMenuState.build`
/// reading `DropdownMenuTheme.of` and `_DropdownMenuDefaultsM3`.
///
/// # This theme is mostly other components' themes
///
/// Three of `DropdownMenuThemeData`'s four fields are somebody else's type: a
/// `TextStyle`, a [`MenuStyle`] and an `InputDecorationThemeData`. Every other
/// theme in this file describes how its own widget looks; this one says **what
/// those components are instead, when they appear inside a dropdown menu**.
///
/// # So a `MenuTheme` around a dropdown does not reach its menu
///
/// The menu style arrives as `MenuAnchor(style: effectiveMenuStyle)`, which is
/// the *widget* step of [`ResolvedMenuPanel`]'s chain -- the step above
/// `MenuTheme`. And `effectiveMenuStyle` is never null, because the defaults
/// class supplies one. So a `MenuTheme` can only contribute fields the
/// dropdown's own style left null, and the fields the defaults fill are not
/// among them.
///
/// # Each sub-theme is taken whole, and this is the `??` end of a distinction
///
/// `widget.menuStyle ?? theme.menuStyle ?? defaults.menuStyle`, and the same
/// shape for the text style and the input decoration theme. **The first
/// non-null object wins entirely.**
///
/// [`ResolvedIconButton`] records the other choice, where upstream writes
/// `merge` and combines field by field. Here it is `??`, so a caller who sets
/// `DropdownMenuTheme.menuStyle` to change one thing has silently discarded
/// `_kMinimumWidth`, the maximum size and the visual density along with it --
/// the whole of what the defaults were carrying.
///
/// # The menu is at least as wide as the field that opened it
///
/// Whatever `minimumSize` survived the ladder is then overwritten with
/// `min(anchorWidth, maximumWidth)` -- or `min(widget.width, maximumWidth)`
/// when a width was given. A dropdown narrower than its own field would look
/// like a different control opening.
///
/// Of the three fields the defaults' menu style carries, that leaves
/// `minimumSize` replaced on any build where the anchor has been measured,
/// `maximumSize` replaced whenever a `menuHeight` is given, and only
/// `visualDensity` reaching the panel as written.
///
/// # The minimum's clamp reads the maximum that ends up final
///
/// Upstream builds the minimum as a resolver closing over the **variable**
/// `effectiveMenuStyle`, and then reassigns that variable when `menuHeight` is
/// set. Dart closures capture variables rather than values, so by the time the
/// resolver runs it reads the reassigned maximum.
///
/// The consequence is that giving a `menuHeight` -- which replaces the maximum
/// with `Size(infinity, height)` -- also removes the width cap the minimum was
/// clamping against, so the minimum becomes the anchor's width unclamped. A
/// height silently widens the menu. Written in the order the reads happen
/// rather than the order the writes do, which is what
/// [`ResolvedDropdownMenu::minimum_width`] does.
///
/// # A dropdown's field is outlined and a bare text field's is not
///
/// `defaults.inputDecorationTheme` is `InputDecorationThemeData(border:
/// OutlineInputBorder())`, where `_getDefaultBorder` falls back to
/// `const UnderlineInputBorder()` for everything else. A dropdown is a field
/// you press as much as one you type in, and a box reads as pressable where a
/// rule does not.
pub struct ResolvedDropdownMenu {
    pub text_style: Option<TextStyle>,
    pub disabled_color: Color,
    pub menu_style: MenuStyle,
    pub input_border_is_outline: bool,
}

impl ResolvedDropdownMenu {
    /// Upstream's `_kMinimumWidth`, the floor the defaults' menu style carries
    /// -- and that a caller replacing that style loses.
    pub const MINIMUM_WIDTH: f32 = 112.0;
    /// Upstream's `disabledColor`, the same 0.38 as everywhere else.
    pub const DISABLED_OPACITY: f32 = 0.38;

    /// Upstream's `defaults.menuStyle`: a width floor, no ceiling, and the
    /// standard density.
    pub fn default_menu_style() -> MenuStyle {
        let mut style = MenuStyle::new();
        style.minimum_size = Some(StateProperty::all(Some(Size::new(
            ResolvedDropdownMenu::MINIMUM_WIDTH,
            0.0,
        ))));
        style.maximum_size = Some(StateProperty::all(Some(Size::new(
            f32::INFINITY,
            f32::INFINITY,
        ))));
        style.visual_density = Some(VisualDensity::STANDARD);
        style
    }

    /// Upstream's `effectiveTextStyle`: the base, and a disabled menu's is that
    /// base recoloured -- or a style carrying nothing but the colour, if there
    /// was no base to recolour.
    pub fn text_style_for(
        base: Option<TextStyle>,
        enabled: bool,
        disabled: Color,
    ) -> Option<TextStyle> {
        if enabled {
            return base;
        }
        Some(match base {
            Some(style) => TextStyle {
                color: disabled,
                ..style
            },
            None => TextStyle {
                color: disabled,
                ..TextStyle::default()
            },
        })
    }

    /// The menu's minimum width, written in the order the reads happen.
    ///
    /// The caller's width beats the anchor's, and either is clamped by the
    /// **final** maximum -- which is `None` once a `menuHeight` has replaced
    /// it, so a height removes the clamp. See the type's docs.
    pub fn minimum_width(
        given_width: Option<f32>,
        anchor_width: Option<f32>,
        menu_height: Option<f32>,
        style_maximum_width: Option<f32>,
    ) -> Option<f32> {
        let wanted = given_width.or(anchor_width)?;
        let maximum = if menu_height.is_some() {
            // `Size(infinity, height)` -- the width ceiling went with it.
            None
        } else {
            style_maximum_width
        };
        Some(match maximum {
            Some(cap) => wanted.min(cap),
            None => wanted,
        })
    }

    pub fn of(context: &mut BuildContext, enabled: bool) -> ResolvedDropdownMenu {
        let theme = ThemeData::of(context);
        let data = DropdownMenuTheme::of(context);
        let disabled_color = data.disabled_color.unwrap_or_else(|| {
            crate::elevation_overlay::with_opacity(
                theme.color_scheme.on_surface,
                ResolvedDropdownMenu::DISABLED_OPACITY,
            )
        });
        let base = data
            .text_style
            .clone()
            .or(theme.text_theme.body_large.clone());
        ResolvedDropdownMenu {
            text_style: ResolvedDropdownMenu::text_style_for(base, enabled, disabled_color),
            disabled_color,
            // Whole or not at all -- see the type's docs.
            menu_style: data
                .menu_style
                .clone()
                .unwrap_or_else(ResolvedDropdownMenu::default_menu_style),
            input_border_is_outline: data.input_decoration_theme.is_none(),
        }
    }
}

/// What the hour and minute boxes at the top of a time picker are drawn
/// with, under one state set.
///
/// Resolved per box rather than per picker: there are two of them and only
/// one is selected at a time, which is the whole reason these fields are
/// state properties.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedHourMinute {
    pub background: Color,
    pub foreground: Color,
    pub style: TextStyle,
    /// The box's outline. Upstream's field is a whole `ShapeBorder`, so a
    /// theme may make the box a stadium or a circle and not only round its
    /// corners differently -- which is why this is the shape and not the
    /// radius the two tables happen to differ by.
    pub shape: ShapeBorder,
    pub shape_radius: f32,
    /// The colon between the two boxes. `None` under Material 2, which has no
    /// such field: a Material 2 picker draws its colon in the hour/minute
    /// style, like the digits beside it.
    pub separator_color: Option<Color>,
    pub separator_style: Option<TextStyle>,
}

impl ResolvedHourMinute {
    /// Upstream's Material 2 shape radius.
    pub const M2_RADIUS: f32 = 4.0;
    /// Upstream's Material 3 shape radius, which is also
    /// [`ResolvedTimePicker::HOUR_MINUTE_RADIUS`].
    pub const M3_RADIUS: f32 = 8.0;
    /// Material 2's tint on an unselected box, and on a selected one under a
    /// light theme.
    pub const M2_OPACITY: f32 = 0.12;
    /// Material 2's tint on a selected box under a **dark** theme. Twice the
    /// light one, because the same twelve percent of `primary` over a dark
    /// surface would not be seen.
    pub const M2_SELECTED_DARK_OPACITY: f32 = 0.24;

    pub fn of(
        context: &mut BuildContext,
        entry_mode: crate::pickers::TimePickerEntryMode,
        states: WidgetStates,
    ) -> ResolvedHourMinute {
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let material3 = theme.use_material3;
        let data = TimePickerTheme::of(context);
        let text_theme = theme.text_theme.clone();

        let foreground = data
            .hour_minute_text_color
            .unwrap_or_else(|| ResolvedHourMinute::foreground_for(&scheme, material3, states));
        ResolvedHourMinute {
            background: data
                .hour_minute_color
                .unwrap_or_else(|| ResolvedHourMinute::background_for(&scheme, material3, states)),
            foreground,
            style: data.hour_minute_text_style.clone().unwrap_or_else(|| {
                // The one style in the framework that branches on the entry
                // mode. On the dial the number is the whole screen's subject;
                // in text-entry mode it is inside a field that has to leave
                // room for a border and a label, so it comes down a rung.
                let role = if material3 {
                    match entry_mode {
                        crate::pickers::TimePickerEntryMode::Dial
                        | crate::pickers::TimePickerEntryMode::DialOnly => {
                            text_theme.display_large.clone()
                        }
                        _ => text_theme.display_medium.clone(),
                    }
                } else {
                    text_theme.display_medium.clone()
                };
                TextStyle {
                    color: foreground,
                    ..role.unwrap_or_default()
                }
            }),
            shape: data.hour_minute_shape.clone().unwrap_or_else(|| {
                ShapeBorder::Rounded(crate::borders::RoundedRectangleBorder::new(
                    crate::borders::BorderSide::NONE,
                    crate::borders::BorderRadiusGeometry::circular(if material3 {
                        ResolvedHourMinute::M3_RADIUS
                    } else {
                        ResolvedHourMinute::M2_RADIUS
                    }),
                ))
            }),
            shape_radius: if material3 {
                ResolvedHourMinute::M3_RADIUS
            } else {
                ResolvedHourMinute::M2_RADIUS
            },
            separator_color: data
                .time_selector_separator_color
                .as_ref()
                .and_then(|property| property.resolve(states))
                .or(if material3 {
                    Some(scheme.on_surface)
                } else {
                    None
                }),
            separator_style: data
                .time_selector_separator_text_style
                .as_ref()
                .and_then(|property| property.resolve(states))
                .or_else(|| {
                    if material3 {
                        text_theme.display_large.clone()
                    } else {
                        None
                    }
                }),
        }
    }

    /// The box's fill.
    ///
    /// Material 3 **blends** rather than picks: the state overlay goes over
    /// the container colour instead of replacing it. And its pressed overlay
    /// is the ink at full opacity, so pressing the hour box turns it from
    /// `primaryContainer` into `onPrimaryContainer` outright -- the strongest
    /// state change in any of these tables.
    pub fn background_for(scheme: &ColorScheme, material3: bool, states: WidgetStates) -> Color {
        let selected = states.contains(WidgetState::Selected);
        if !material3 {
            // One of the last places a colour is picked by brightness rather
            // than by a scheme role.
            let dark = scheme.brightness == crate::platform::Brightness::Dark;
            return if selected {
                crate::elevation_overlay::with_opacity(
                    scheme.primary,
                    if dark {
                        ResolvedHourMinute::M2_SELECTED_DARK_OPACITY
                    } else {
                        ResolvedHourMinute::M2_OPACITY
                    },
                )
            } else {
                crate::elevation_overlay::with_opacity(
                    scheme.on_surface,
                    ResolvedHourMinute::M2_OPACITY,
                )
            };
        }
        let (base, ink) = if selected {
            (scheme.primary_container(), scheme.on_primary_container())
        } else {
            (scheme.surface_container_highest(), scheme.on_surface)
        };
        let overlay = if states.contains(WidgetState::Pressed) {
            ink
        } else if states.contains(WidgetState::Hovered) {
            crate::elevation_overlay::with_opacity(ink, ResolvedTimePicker::HOVERED_OVERLAY)
        } else if states.contains(WidgetState::Focused) {
            crate::elevation_overlay::with_opacity(ink, ResolvedTimePicker::PRESSED_OVERLAY)
        } else {
            base
        };
        crate::elevation_overlay::alpha_blend(overlay, base)
    }

    /// The digits' ink. Upstream's Material 3 ladder writes the same answer
    /// in all four arms of each branch, so what it comes to is: selected
    /// digits are `onPrimaryContainer` and the rest are `onSurface`.
    pub fn foreground_for(scheme: &ColorScheme, material3: bool, states: WidgetStates) -> Color {
        if states.contains(WidgetState::Selected) {
            if material3 {
                scheme.on_primary_container()
            } else {
                scheme.primary
            }
        } else {
            scheme.on_surface
        }
    }
}

/// What the AM/PM toggle's two halves are drawn with, under one state set.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedDayPeriod {
    pub background: Color,
    pub foreground: Color,
    pub style: TextStyle,
    /// The outline, already put on the shape. Upstream keeps the shape and
    /// the side as two fields and combines them at the call site, so a theme
    /// may name the rounding and the outline separately.
    pub shape: ShapeBorder,
    pub side: BorderSide,
}

impl ResolvedDayPeriod {
    /// Material 2's fade on the unselected half's words.
    pub const M2_UNSELECTED_OPACITY: f32 = 0.60;
    /// Material 2's fade on the outline, blended onto the surface rather than
    /// left translucent.
    pub const M2_BORDER_OPACITY: f32 = 0.38;

    pub fn of(context: &mut BuildContext, states: WidgetStates) -> ResolvedDayPeriod {
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let material3 = theme.use_material3;
        let data = TimePickerTheme::of(context);
        let selected = states.contains(WidgetState::Selected);
        let dark = scheme.brightness == crate::platform::Brightness::Dark;

        let foreground = data.day_period_text_color.unwrap_or(if selected {
            if material3 {
                scheme.on_tertiary_container()
            } else {
                scheme.primary
            }
        } else if material3 {
            scheme.on_surface_variant()
        } else {
            crate::elevation_overlay::with_opacity(
                scheme.on_surface,
                ResolvedDayPeriod::M2_UNSELECTED_OPACITY,
            )
        });
        let side = data.day_period_border_side.unwrap_or(BorderSide {
            color: if material3 {
                scheme.outline()
            } else {
                // Blended onto the surface rather than left translucent: the
                // toggle sits on the dialog, and a see-through outline would
                // pick up whatever the elevation overlay put behind it.
                crate::elevation_overlay::alpha_blend(
                    crate::elevation_overlay::with_opacity(
                        scheme.on_surface,
                        ResolvedDayPeriod::M2_BORDER_OPACITY,
                    ),
                    scheme.surface,
                )
            },
            width: 1.0,
            ..BorderSide::NONE
        });
        ResolvedDayPeriod {
            background: data.day_period_color.unwrap_or(if !selected {
                // Transparent in both tables, and upstream says why in a
                // comment it repeats in each: the unselected half should
                // match the dialog behind it, and transparency does that
                // "without being redundant and allows the optional elevation
                // overlay for dark mode to be visible". A colour copied from
                // the dialog would be a second place to change it, and would
                // sit *over* the elevation overlay instead of under it.
                Color::TRANSPARENT
            } else if material3 {
                scheme.tertiary_container()
            } else {
                crate::elevation_overlay::with_opacity(
                    scheme.primary,
                    if dark {
                        ResolvedHourMinute::M2_SELECTED_DARK_OPACITY
                    } else {
                        ResolvedHourMinute::M2_OPACITY
                    },
                )
            }),
            foreground,
            style: data.day_period_text_style.clone().unwrap_or_else(|| {
                // The same construction in both tables: `titleMedium` with
                // the resolved colour on it.
                TextStyle {
                    color: foreground,
                    ..theme.text_theme.title_medium.clone().unwrap_or_default()
                }
            }),
            // Upstream's `(theme.dayPeriodShape ?? defaults).copyWith(side:
            // resolvedSide)`: whichever shape wins takes whichever side wins,
            // so a theme may name the rounding and the outline separately.
            // Only a rounded rectangle can carry a side here, which is what
            // both tables give and what upstream's `OutlinedBorder` type
            // requires of any replacement.
            shape: match data.day_period_shape.clone() {
                Some(ShapeBorder::Rounded(rounded)) => ShapeBorder::Rounded(
                    crate::borders::RoundedRectangleBorder::new(side, rounded.border_radius),
                ),
                Some(other) => other,
                None => ShapeBorder::Rounded(crate::borders::RoundedRectangleBorder::new(
                    side,
                    crate::borders::BorderRadiusGeometry::circular(if material3 {
                        ResolvedHourMinute::M3_RADIUS
                    } else {
                        ResolvedHourMinute::M2_RADIUS
                    }),
                )),
            },
            side,
        }
    }
}

/// What the clock face is drawn with, under one state set.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedDial {
    pub background: Color,
    /// The hand. `primary` in both tables.
    pub hand: Color,
    pub text_color: Color,
    /// `bodyLarge` in both tables, with the resolved colour on it.
    pub text_style: TextStyle,
}

impl ResolvedDial {
    /// Material 2's dial face tint, in the dark and in the light.
    pub const M2_DARK_OPACITY: f32 = 0.12;
    pub const M2_LIGHT_OPACITY: f32 = 0.08;

    pub fn of(context: &mut BuildContext, states: WidgetStates) -> ResolvedDial {
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let material3 = theme.use_material3;
        let data = TimePickerTheme::of(context);
        let dark = scheme.brightness == crate::platform::Brightness::Dark;

        let text_color =
            data.dial_text_color
                .unwrap_or(if states.contains(WidgetState::Selected) {
                    // Both are the ink for "on the hand", which is `primary`.
                    // Material 2 had no `onPrimary` habit yet and reached for
                    // `surface` instead.
                    if material3 {
                        scheme.on_primary
                    } else {
                        scheme.surface
                    }
                } else {
                    scheme.on_surface
                });
        ResolvedDial {
            background: data.dial_background_color.unwrap_or(if material3 {
                scheme.surface_container_highest()
            } else {
                crate::elevation_overlay::with_opacity(
                    scheme.on_surface,
                    if dark {
                        ResolvedDial::M2_DARK_OPACITY
                    } else {
                        ResolvedDial::M2_LIGHT_OPACITY
                    },
                )
            }),
            hand: data.dial_hand_color.unwrap_or(scheme.primary),
            text_color,
            text_style: data
                .dial_text_style
                .clone()
                .map(|style| TextStyle {
                    color: text_color,
                    ..style
                })
                .unwrap_or_else(|| TextStyle {
                    color: text_color,
                    ..theme.text_theme.body_large.clone().unwrap_or_default()
                }),
        }
    }
}

/// What a time picker is drawn with -- upstream's `_TimePickerDefaultsM3`
/// under `TimePickerTheme.of`.
///
/// # The entry mode is a third input to the defaults, and it moves one field
///
/// `_TimePickerDefaultsM3(context, {entryMode})` takes the mode the way the
/// search view's defaults take `isFullScreen` -- an argument that is neither
/// the theme nor the widget. And as there, it changes **exactly one** default:
/// `hourMinuteTextStyle` is `displayLarge` on a dial and `displayMedium` in a
/// field.
///
/// One size smaller when it is editable. A dial's hour is a target you tap; an
/// input's is text you type into, with a caret in it, and `displayLarge` would
/// give it a caret the height of a thumb.
///
/// # The input boxes are the dial boxes minus eight, in height only
///
/// `hourMinuteInputSize` is `Size(width, height - 8)`, with upstream's note
/// that the spec says eight and there is no token for it yet. The width does
/// not move: a field still has to hold two digits, and only the room around
/// them comes down.
///
/// # A 24-hour clock's boxes are wider at the same height
///
/// 114 against 96. There is no AM/PM selector beside them in that mode, and
/// the width it was taking goes to the numbers rather than to the margins.
///
/// # An unselected day period is transparent rather than the dialog's colour
///
/// Upstream says why in a comment, and it is the kind of thing that reads as a
/// redundancy until you see the reason: *"Making it transparent enables that
/// without being redundant and allows the optional elevation overlay for dark
/// mode to be visible."*
///
/// Painting `surfaceContainerHigh` over `surfaceContainerHigh` is not the same
/// as painting nothing, because the dialog's elevation overlay sits between
/// them in the dark. **Transparent and same-colour differ exactly where
/// something is layered underneath.**
///
/// # And the hour/minute text ladder is eight arms with two answers again
///
/// Selected returns `onPrimaryContainer` from all four of its arms and
/// unselected returns `onSurface` from all four, as
/// [`ResolvedSegmentedButton`]'s does. The interaction is in
/// `hourMinuteColor`, which blends an overlay over `surfaceContainerHighest`
/// at the usual 0.1 pressed, 0.08 hovered, 0.1 focused.
pub struct ResolvedTimePicker {
    pub background_color: Color,
    pub elevation: f32,
    pub day_period_border: BorderSide,
    pub entry_mode_icon_color: Color,
    pub hour_minute_size: Size,
    pub hour_minute_text_is_large: bool,
    pub hour_minute_shape_radius: f32,
    /// The line above the picker -- "Select time". Material 3 recolours it to
    /// `onSurfaceVariant`; Material 2's is `labelSmall` flat, so it keeps
    /// whatever ink the scale carries.
    pub help_text_style: Option<TextStyle>,
}

impl ResolvedTimePicker {
    /// Upstream's `hourMinuteSize`.
    pub const HOUR_MINUTE_SIZE: Size = Size::new(96.0, 80.0);
    /// Upstream's `hourMinuteSize24Hour` width, which is the only thing that
    /// differs from [`ResolvedTimePicker::HOUR_MINUTE_SIZE`].
    pub const HOUR_MINUTE_WIDTH_24_HOUR: f32 = 114.0;
    /// Upstream's "eight pixels smaller than the regular size in the spec, but
    /// there's no token for it yet".
    pub const INPUT_HEIGHT_REDUCTION: f32 = 8.0;
    /// Upstream's `hourMinuteShape` radius.
    pub const HOUR_MINUTE_RADIUS: f32 = 8.0;
    /// Upstream's `elevation`.
    pub const ELEVATION: f32 = 6.0;
    pub const PRESSED_OVERLAY: f32 = 0.1;
    pub const HOVERED_OVERLAY: f32 = 0.08;

    /// The hour and minute box, by the two things that change it.
    ///
    /// Twenty-four hours widens it; input shortens it. The two are independent
    /// and compose, because one moves the width and the other the height.
    pub fn hour_minute_size(twenty_four_hour: bool, is_input: bool) -> Size {
        let width = if twenty_four_hour {
            ResolvedTimePicker::HOUR_MINUTE_WIDTH_24_HOUR
        } else {
            ResolvedTimePicker::HOUR_MINUTE_SIZE.width
        };
        let height = if is_input {
            ResolvedTimePicker::HOUR_MINUTE_SIZE.height - ResolvedTimePicker::INPUT_HEIGHT_REDUCTION
        } else {
            ResolvedTimePicker::HOUR_MINUTE_SIZE.height
        };
        Size::new(width, height)
    }

    /// Whether the hour/minute text is `displayLarge` rather than
    /// `displayMedium` -- the one default the entry mode moves.
    pub fn hour_minute_text_is_large(mode: crate::pickers::TimePickerEntryMode) -> bool {
        use crate::pickers::TimePickerEntryMode;
        matches!(
            mode,
            TimePickerEntryMode::Dial | TimePickerEntryMode::DialOnly
        )
    }

    /// Upstream's `dayPeriodColor`: the scheme's container when selected, and
    /// **transparent** otherwise -- not the dialog's colour. See the type's
    /// docs.
    pub fn day_period_color(states: WidgetStates, scheme: &ColorScheme) -> Color {
        if states.contains(WidgetState::Selected) {
            scheme.tertiary_container()
        } else {
            Color::TRANSPARENT
        }
    }

    /// Upstream's `_hourMinuteTextColor`, whose eight arms give two answers.
    pub fn hour_minute_text_color(states: WidgetStates, scheme: &ColorScheme) -> Color {
        if states.contains(WidgetState::Selected) {
            scheme.on_primary_container()
        } else {
            scheme.on_surface
        }
    }

    /// Upstream's `hourMinuteColor`: an overlay blended over
    /// `surfaceContainerHighest`, which is where the interaction shows.
    pub fn hour_minute_color(states: WidgetStates, scheme: &ColorScheme) -> Color {
        let opacity = if states.contains(WidgetState::Pressed) {
            ResolvedTimePicker::PRESSED_OVERLAY
        } else if states.contains(WidgetState::Hovered) {
            ResolvedTimePicker::HOVERED_OVERLAY
        } else if states.contains(WidgetState::Focused) {
            ResolvedTimePicker::PRESSED_OVERLAY
        } else {
            return scheme.surface_container_highest();
        };
        crate::elevation_overlay::alpha_blend(
            crate::elevation_overlay::with_opacity(scheme.on_surface, opacity),
            scheme.surface_container_highest(),
        )
    }

    pub fn of(
        context: &mut BuildContext,
        mode: crate::pickers::TimePickerEntryMode,
        twenty_four_hour: bool,
    ) -> ResolvedTimePicker {
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let material3 = theme.use_material3;
        let data = TimePickerTheme::of(context);
        let is_input = !ResolvedTimePicker::hour_minute_text_is_large(mode);
        ResolvedTimePicker {
            background_color: data
                .background_color
                .unwrap_or_else(|| scheme.surface_container_high()),
            elevation: data.elevation.unwrap_or(ResolvedTimePicker::ELEVATION),
            day_period_border: data.day_period_border_side.unwrap_or(BorderSide {
                color: scheme.outline(),
                width: 1.0,
                ..BorderSide::NONE
            }),
            // This was a flat `onSurface`, which is Material 3's answer.
            // Material 2 fades it to sixty percent in the light and leaves it
            // at full strength in the dark -- another of the brightness
            // branches that table is full of.
            entry_mode_icon_color: data.entry_mode_icon_color.unwrap_or_else(|| {
                if material3 || scheme.brightness == crate::platform::Brightness::Dark {
                    scheme.on_surface
                } else {
                    crate::elevation_overlay::with_opacity(
                        scheme.on_surface,
                        ResolvedDayPeriod::M2_UNSELECTED_OPACITY,
                    )
                }
            }),
            /// The line above the picker -- "Select time".
            help_text_style: data.help_text_style.clone().or_else(|| {
                if material3 {
                    theme
                        .text_theme
                        .label_medium
                        .clone()
                        .map(|style| TextStyle {
                            color: scheme.on_surface_variant(),
                            ..style
                        })
                } else {
                    // Flat: no colour merged, so it keeps the scale's own.
                    theme.text_theme.label_small.clone()
                }
            }),
            hour_minute_size: ResolvedTimePicker::hour_minute_size(twenty_four_hour, is_input),
            hour_minute_text_is_large: !is_input,
            hour_minute_shape_radius: ResolvedTimePicker::HOUR_MINUTE_RADIUS,
        }
    }
}

/// Which of a date picker's cells is being asked about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateCellSlot {
    /// An ordinary date in the calendar grid.
    Day,
    /// The date that is today, which is drawn differently even when it is
    /// not selected -- that is what the today border is for.
    Today,
    /// A year in the list the header's arrow opens.
    Year,
}

/// What one cell of a date picker is drawn with, under one state set.
///
/// A resolver per cell rather than per picker, because that is the shape of
/// the question: a calendar asks it once for every date on the screen, with
/// a different state set each time.
///
/// `background` and `overlay` are `Option` because upstream's properties
/// answer null for the ordinary case, and null is not a colour: an
/// unselected date has **no** background, which is different from having a
/// transparent one -- the surface behind it shows through whatever that is,
/// including a range selection's tint.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedDateCell {
    pub foreground: Color,
    pub background: Option<Color>,
    pub overlay: Option<Color>,
    /// The outline the fill is clipped to.
    ///
    /// Set in the *constructors* of both defaults classes rather than
    /// overridden as getters, which is why grepping upstream for
    /// `get dayShape` finds nothing. Both pass the same pair: a date is a
    /// circle and a year is a pill.
    pub shape: ShapeBorder,
}

impl ResolvedDateCell {
    /// Upstream's `withOpacity(0.38)` for a disabled cell.
    pub const DISABLED_OPACITY: f32 = 0.38;
    /// Material 3's pressed and focused overlay.
    pub const M3_STRONG_OVERLAY: f32 = 0.1;
    /// The hovered overlay, which both tables share.
    pub const HOVERED_OVERLAY: f32 = 0.08;
    /// Material 2's focused overlay, and its pressed one when the day is not
    /// selected.
    pub const M2_OVERLAY: f32 = 0.12;
    /// Material 2's overlay for a **pressed, selected** day: three times the
    /// next largest number in either table. A selected date already carries
    /// the primary colour, so an ordinary ripple over it would not be seen.
    pub const M2_PRESSED_SELECTED_OVERLAY: f32 = 0.38;

    pub fn of(
        context: &mut BuildContext,
        slot: DateCellSlot,
        states: WidgetStates,
    ) -> ResolvedDateCell {
        let theme = ThemeData::of(context);
        let data = DatePickerTheme::of(context);
        let scheme = theme.color_scheme;
        let material3 = theme.use_material3;

        let asked = |property: &Option<StateProperty<Option<Color>>>| {
            property
                .as_ref()
                .and_then(|property| property.resolve(states))
        };
        let (foreground, background, overlay, shape) = match slot {
            DateCellSlot::Day => (
                &data.day_foreground_color,
                &data.day_background_color,
                &data.day_overlay_color,
                &data.day_shape,
            ),
            DateCellSlot::Today => (
                &data.today_foreground_color,
                &data.today_background_color,
                // Upstream gives today no overlay of its own: it is a date
                // like the others as far as touching it goes.
                &data.day_overlay_color,
                // And no shape of its own either -- today is drawn by
                // putting a side on the day shape.
                &data.day_shape,
            ),
            DateCellSlot::Year => (
                &data.year_foreground_color,
                &data.year_background_color,
                &data.year_overlay_color,
                &data.year_shape,
            ),
        };

        ResolvedDateCell {
            foreground: asked(foreground).unwrap_or_else(|| {
                ResolvedDateCell::foreground_for(&scheme, material3, slot, states)
            }),
            background: asked(background)
                .or_else(|| ResolvedDateCell::background_for(&scheme, slot, states)),
            overlay: asked(overlay)
                .or_else(|| ResolvedDateCell::overlay_for(&scheme, material3, slot, states)),
            shape: shape
                .as_ref()
                .and_then(|property| property.resolve(states))
                .unwrap_or_else(|| ResolvedDateCell::shape_for(slot)),
        }
    }

    /// A date is a circle and a year is a pill, in both tables. Today takes
    /// the date's -- upstream draws today by putting a *side* on the day
    /// shape rather than by giving it a shape of its own, which is why a
    /// custom `dayShape` carries today's ring with it.
    pub fn shape_for(slot: DateCellSlot) -> ShapeBorder {
        match slot {
            DateCellSlot::Day | DateCellSlot::Today => {
                ShapeBorder::Circle(crate::borders::CircleBorder::default())
            }
            DateCellSlot::Year => ShapeBorder::Stadium(crate::borders::StadiumBorder::default()),
        }
    }

    /// The ink, before any theme has been consulted.
    pub fn foreground_for(
        scheme: &ColorScheme,
        material3: bool,
        slot: DateCellSlot,
        states: WidgetStates,
    ) -> Color {
        // A Material 2 year list is not themed at all -- the three year
        // properties are absent from `_DatePickerDefaultsM2`, not null in it
        // -- so it falls to whatever draws it.
        let faded = |color: Color| {
            crate::elevation_overlay::with_opacity(color, ResolvedDateCell::DISABLED_OPACITY)
        };
        if states.contains(WidgetState::Selected) {
            return scheme.on_primary;
        }
        match slot {
            DateCellSlot::Day => {
                if states.contains(WidgetState::Disabled) {
                    faded(scheme.on_surface)
                } else {
                    scheme.on_surface
                }
            }
            // The one real disagreement between the tables: a disabled today
            // fades towards the ordinary ink under Material 2 and stays a
            // faded primary under Material 3. So an out-of-range today still
            // says it is today under Material 3, and stops saying so under
            // Material 2.
            DateCellSlot::Today => {
                if states.contains(WidgetState::Disabled) {
                    faded(if material3 {
                        scheme.primary
                    } else {
                        scheme.on_surface
                    })
                } else {
                    scheme.primary
                }
            }
            DateCellSlot::Year => {
                if states.contains(WidgetState::Disabled) {
                    faded(scheme.on_surface_variant())
                } else {
                    scheme.on_surface_variant()
                }
            }
        }
    }

    /// The fill. `None` for anything but a selected cell, in every table --
    /// and today's is upstream's `dayBackgroundColor`, the *defaults*
    /// object's rather than the theme's, so a theme that recolours ordinary
    /// dates does not thereby recolour today.
    pub fn background_for(
        scheme: &ColorScheme,
        _slot: DateCellSlot,
        states: WidgetStates,
    ) -> Option<Color> {
        if states.contains(WidgetState::Selected) {
            Some(scheme.primary)
        } else {
            None
        }
    }

    /// The ripple. Two branches, selected and not, and the numbers in them
    /// are where the two tables differ most.
    pub fn overlay_for(
        scheme: &ColorScheme,
        material3: bool,
        _slot: DateCellSlot,
        states: WidgetStates,
    ) -> Option<Color> {
        let selected = states.contains(WidgetState::Selected);
        let ink = if selected {
            scheme.on_primary
        } else {
            scheme.on_surface_variant()
        };
        let opacity = if states.contains(WidgetState::Pressed) {
            if material3 {
                ResolvedDateCell::M3_STRONG_OVERLAY
            } else if selected {
                ResolvedDateCell::M2_PRESSED_SELECTED_OVERLAY
            } else {
                ResolvedDateCell::M2_OVERLAY
            }
        } else if states.contains(WidgetState::Hovered) {
            ResolvedDateCell::HOVERED_OVERLAY
        } else if states.contains(WidgetState::Focused) {
            if material3 {
                ResolvedDateCell::M3_STRONG_OVERLAY
            } else {
                ResolvedDateCell::M2_OVERLAY
            }
        } else {
            // Not touched, so nothing over it. `None` and not transparent:
            // there is no layer, rather than an invisible one.
            return None;
        };
        Some(crate::elevation_overlay::with_opacity(ink, opacity))
    }
}

/// What a date picker is drawn with -- upstream's `_DatePickerDefaultsM3`
/// under `DatePickerTheme.of`.
///
/// # The one component where selected beats disabled
///
/// `dayForegroundColor` checks `selected` **first** and `disabled` second.
/// Every other ladder in this file goes the other way:
/// [`ResolvedSegmentedButton`], [`ResolvedMenuButton`],
/// [`ResolvedInputBorder`] and [`ResolvedNavigationDrawer`] all let disabled
/// win, and this one does not.
///
/// So a selected day that is also disabled is drawn **as selected** -- filled,
/// in `onPrimary` -- rather than faded. That is the right way round here
/// because a picker's selection is the answer it currently holds. A day is
/// disabled when it falls outside the selectable range, which is exactly the
/// case where the caller most needs to see what the picker has: fading it
/// would hide the value while leaving the picker holding it.
///
/// A disabled segment or menu line is merely unavailable, and nothing is lost
/// by dimming it. A disabled *selection* is a state someone has to resolve.
///
/// # A day is a circle and a year is a stadium
///
/// `dayShape` is `CircleBorder` and `yearShape` is `StadiumBorder`. One or two
/// digits fit in a circle; four do not, and a pill is a circle that gave up on
/// being one. The shape follows how wide the content is rather than a
/// preference.
///
/// # Today has no fill of its own
///
/// `todayBackgroundColor` **is** `dayBackgroundColor` -- upstream returns the
/// other getter, not a copy of it. Today is marked by its border and its text
/// colour, so there is no fill to conflict with the selected one, and
/// selecting today can simply reuse the day's.
///
/// The two foreground ladders say the same thing from the other side: today is
/// `primary` where a day is `onSurface`, and `primary` at 0.38 where a day is
/// `onSurface` at 0.38 -- **but both are `onPrimary` when selected**, because
/// at that point both are sitting on the same primary-coloured circle. The
/// ladders converge exactly at the arm where the background changed.
///
/// # The range picker is the same rule the search view states as a branch
///
/// The dialog is elevation 6 with a 28-radius shape; `rangePickerElevation` is
/// **0** and `rangePickerShape` a plain `RoundedRectangleBorder()`. A range
/// picker is full screen, and a full-screen surface has no corners to round
/// and nothing to float above.
///
/// [`ResolvedSearchView`] reaches the same conclusion by branching on
/// `isFullScreen` inside one default. Here it is two separate fields instead.
/// One rule, two encodings -- and the second is why the theme has twenty-odd
/// `rangePicker`-prefixed fields.
///
/// # One default reads another
///
/// `toggleButtonTextStyle` is `titleSmall.apply(color: subHeaderForegroundColor)`,
/// and `subHeaderForegroundColor` is another getter on the same class. Moving
/// the sub-header's colour moves the toggle button's text with it, which is
/// what you want from two things that sit in the same strip -- and is a
/// dependency between defaults rather than between a default and the theme.
pub struct ResolvedDatePicker {
    pub background_color: Color,
    pub elevation: f32,
    pub shape_radius: f32,
    pub header_background_color: Color,
    pub header_foreground_color: Color,
    pub sub_header_foreground_color: Color,
    pub today_border: BorderSide,
    pub range_picker_elevation: f32,
    pub range_picker_shape_radius: f32,
    /// The date at the top of the dialog, large.
    pub header_headline_style: Option<TextStyle>,
    /// The line above it -- "Select date" -- small.
    pub header_help_style: Option<TextStyle>,
    /// The row of letters above the calendar.
    pub weekday_style: Option<TextStyle>,
    /// The dates themselves.
    pub day_style: Option<TextStyle>,
    /// The years, in the list the header's arrow opens.
    pub year_style: Option<TextStyle>,
    /// The button that swaps the calendar for the text field.
    pub toggle_button_text_style: Option<TextStyle>,
    pub range_picker_header_headline_style: Option<TextStyle>,
    pub range_picker_header_help_style: Option<TextStyle>,
    /// The card a Material 2 range picker is drawn on. `None` under Material
    /// 3, which is not an oversight: that table does not override it, because
    /// a Material 3 range picker fills the screen and takes the dialog's own
    /// background. Material 2's is a card and needs one of its own.
    pub range_picker_background_color: Option<Color>,
    pub range_picker_shadow_color: Color,
    pub range_picker_surface_tint_color: Color,
    /// The range picker's outline. A plain rectangle in both tables -- no
    /// rounding at all, where the ordinary dialog gets 28 -- because a range
    /// picker is the whole screen and a screen has no corners to round.
    pub range_picker_shape: ShapeBorder,
    pub range_picker_header_background_color: Color,
    pub range_picker_header_foreground_color: Color,
    /// The strip drawn behind the dates between the two ends.
    pub range_selection_background_color: Color,
    /// Kept as the property rather than resolved here: see
    /// [`ResolvedDatePicker::range_selection_overlay`], which is asked once
    /// per cell like the rest of a calendar's state colours.
    pub range_selection_overlay_color: Option<StateProperty<Option<Color>>>,
}

impl ResolvedDatePicker {
    /// Upstream's dialog `elevation`.
    pub const ELEVATION: f32 = 6.0;
    /// Upstream's dialog corner radius.
    pub const RADIUS: f32 = 28.0;
    /// Upstream's `rangePickerElevation`, which is not the dialog's.
    pub const RANGE_ELEVATION: f32 = 0.0;
    /// Upstream's `rangePickerShape` radius: a plain rectangle.
    pub const RANGE_RADIUS: f32 = 0.0;
    /// Material 2's tint on the strip behind a selected range.
    pub const RANGE_SELECTION_OPACITY: f32 = 0.12;
    /// The fade Material 2 puts on the row of weekday letters. A third of
    /// the way between the 0.38 a disabled thing wears and full strength:
    /// quieter than the dates, still readable as words.
    pub const WEEKDAY_OPACITY: f32 = 0.60;
    /// Upstream's `subHeaderForegroundColor` opacity -- a third number in the
    /// family, beside 0.38 for disabled and 0.12 for a dead outline.
    pub const SUB_HEADER_OPACITY: f32 = 0.60;
    pub const DISABLED_OPACITY: f32 = 0.38;
    pub const PRESSED_OVERLAY: f32 = 0.1;
    pub const HOVERED_OVERLAY: f32 = 0.08;

    /// Upstream's `dayForegroundColor`. **Selected is checked before
    /// disabled** -- see the type's docs for why this one goes the other way.
    pub fn day_foreground(states: WidgetStates, scheme: &ColorScheme) -> Color {
        if states.contains(WidgetState::Selected) {
            return scheme.on_primary;
        }
        if states.contains(WidgetState::Disabled) {
            return crate::elevation_overlay::with_opacity(
                scheme.on_surface,
                ResolvedDatePicker::DISABLED_OPACITY,
            );
        }
        scheme.on_surface
    }

    /// Upstream's `todayForegroundColor`: the same ladder in the primary, and
    /// the same `onPrimary` at the arm where the circle appears underneath.
    pub fn today_foreground(states: WidgetStates, scheme: &ColorScheme) -> Color {
        if states.contains(WidgetState::Selected) {
            return scheme.on_primary;
        }
        if states.contains(WidgetState::Disabled) {
            return crate::elevation_overlay::with_opacity(
                scheme.primary,
                ResolvedDatePicker::DISABLED_OPACITY,
            );
        }
        scheme.primary
    }

    /// Upstream's `dayBackgroundColor`, which `todayBackgroundColor` returns
    /// rather than copies: only the chosen day is filled, and disabled does
    /// not take that away.
    pub fn day_background(states: WidgetStates, scheme: &ColorScheme) -> Option<Color> {
        states
            .contains(WidgetState::Selected)
            .then_some(scheme.primary)
    }

    /// Upstream's `dayOverlayColor`: `onPrimary` over a filled day and
    /// `onSurfaceVariant` over an empty one, at the usual three opacities.
    pub fn day_overlay(states: WidgetStates, scheme: &ColorScheme) -> Option<Color> {
        let base = if states.contains(WidgetState::Selected) {
            scheme.on_primary
        } else {
            scheme.on_surface_variant()
        };
        let opacity = if states.contains(WidgetState::Pressed) {
            ResolvedDatePicker::PRESSED_OVERLAY
        } else if states.contains(WidgetState::Hovered) {
            ResolvedDatePicker::HOVERED_OVERLAY
        } else if states.contains(WidgetState::Focused) {
            ResolvedDatePicker::PRESSED_OVERLAY
        } else {
            return None;
        };
        Some(crate::elevation_overlay::with_opacity(base, opacity))
    }

    /// The ripple on a date inside a selected range.
    ///
    /// Material 3 has **no selected branch at all**, and that is the whole
    /// content of the field: inside a range every cell *is* selected, so
    /// branching on it says nothing. The strip ripples one way, in
    /// `onPrimaryContainer` over the `secondaryContainer` it is filled with.
    /// Material 2 keeps the two branches its ordinary day overlay has,
    /// including the heavy 0.38 for a pressed selected cell.
    pub fn range_selection_overlay(
        &self,
        scheme: &ColorScheme,
        material3: bool,
        states: WidgetStates,
    ) -> Option<Color> {
        if let Some(asked) = self
            .range_selection_overlay_color
            .as_ref()
            .and_then(|property| property.resolve(states))
        {
            return Some(asked);
        }
        if material3 {
            let opacity =
                if states.contains(WidgetState::Pressed) || states.contains(WidgetState::Focused) {
                    ResolvedDateCell::M3_STRONG_OVERLAY
                } else if states.contains(WidgetState::Hovered) {
                    ResolvedDateCell::HOVERED_OVERLAY
                } else {
                    return None;
                };
            return Some(crate::elevation_overlay::with_opacity(
                scheme.on_primary_container(),
                opacity,
            ));
        }
        ResolvedDateCell::overlay_for(scheme, false, DateCellSlot::Day, states)
    }

    pub fn of(context: &mut BuildContext) -> ResolvedDatePicker {
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
        let material3 = theme.use_material3;
        let dark = scheme.brightness == crate::platform::Brightness::Dark;
        let text_theme = theme.text_theme.clone();
        let data = DatePickerTheme::of(context);
        let sub_header = data.sub_header_foreground_color.unwrap_or_else(|| {
            crate::elevation_overlay::with_opacity(
                scheme.on_surface,
                ResolvedDatePicker::SUB_HEADER_OPACITY,
            )
        });
        ResolvedDatePicker {
            background_color: data
                .background_color
                .unwrap_or_else(|| scheme.surface_container_high()),
            elevation: data.elevation.unwrap_or(ResolvedDatePicker::ELEVATION),
            shape_radius: ResolvedDatePicker::RADIUS,
            // Transparent, and for the reason the time picker's day period is:
            // the header sits on the dialog and painting it again would hide
            // what is between them.
            header_background_color: data.header_background_color.unwrap_or(Color::TRANSPARENT),
            header_foreground_color: data
                .header_foreground_color
                .unwrap_or_else(|| scheme.on_surface_variant()),
            sub_header_foreground_color: sub_header,
            today_border: data.today_border.unwrap_or(BorderSide {
                color: scheme.primary,
                width: 1.0,
                ..BorderSide::NONE
            }),
            range_picker_elevation: data
                .range_picker_elevation
                .unwrap_or(ResolvedDatePicker::RANGE_ELEVATION),
            range_picker_shape_radius: ResolvedDatePicker::RANGE_RADIUS,
            header_headline_style: data.header_headline_style.clone().or_else(|| {
                if material3 {
                    text_theme.headline_large.clone()
                } else {
                    text_theme.headline_small.clone()
                }
            }),
            header_help_style: data.header_help_style.clone().or_else(|| {
                if material3 {
                    text_theme.label_large.clone()
                } else {
                    text_theme.label_small.clone()
                }
            }),
            // A role with a colour put over it, and not the role's own.
            // Material 2's sixty percent is the letter row being quieter than
            // the dates beneath it; Material 3 drops the fade and grows the
            // type instead, which says the same thing the other way round.
            weekday_style: data.weekday_style.clone().or_else(|| {
                if material3 {
                    text_theme.body_large.clone().map(|style| TextStyle {
                        color: scheme.on_surface,
                        ..style
                    })
                } else {
                    text_theme.body_small.clone().map(|style| TextStyle {
                        color: crate::elevation_overlay::with_opacity(
                            scheme.on_surface,
                            ResolvedDatePicker::WEEKDAY_OPACITY,
                        ),
                        ..style
                    })
                }
            }),
            day_style: data.day_style.clone().or_else(|| {
                if material3 {
                    text_theme.body_large.clone()
                } else {
                    text_theme.body_small.clone()
                }
            }),
            // The one style both tables agree on outright: a year is a year
            // at either size.
            year_style: data
                .year_style
                .clone()
                .or_else(|| text_theme.body_large.clone()),
            // And the one they agree on by *construction* rather than by
            // value: `titleSmall` in the sub-header's colour, whichever
            // colour that turned out to be. The button and the words beside
            // it are one control.
            toggle_button_text_style: data.toggle_button_text_style.clone().or_else(|| {
                text_theme.title_small.clone().map(|style| TextStyle {
                    color: sub_header,
                    ..style
                })
            }),
            range_picker_header_headline_style: data
                .range_picker_header_headline_style
                .clone()
                .or_else(|| {
                    if material3 {
                        text_theme.title_large.clone()
                    } else {
                        text_theme.headline_small.clone()
                    }
                }),
            range_picker_header_help_style: data.range_picker_header_help_style.clone().or_else(
                || {
                    if material3 {
                        text_theme.title_small.clone()
                    } else {
                        text_theme.label_small.clone()
                    }
                },
            ),
            range_picker_background_color: data.range_picker_background_color.or(if material3 {
                None
            } else {
                Some(scheme.surface)
            }),
            // Both transparent in both tables, and for the reason the
            // dialog's are: the elevation is said by the colour underneath.
            range_picker_shadow_color: data.range_picker_shadow_color.unwrap_or(Color::TRANSPARENT),
            range_picker_surface_tint_color: data
                .range_picker_surface_tint_color
                .unwrap_or(Color::TRANSPARENT),
            range_picker_shape: data
                .range_picker_shape
                .clone()
                .unwrap_or(ShapeBorder::Rounded(
                    crate::borders::RoundedRectangleBorder::default(),
                )),
            // The last place in the date picker where Material 2 picks a
            // colour by *brightness* rather than by a scheme role. A dark
            // theme's header is `surface` and a light one's is `primary`,
            // with the foreground following. Material 3 makes the header
            // transparent and lets the dialog behind it show, the same move
            // it makes for the ordinary header.
            range_picker_header_background_color: data
                .range_picker_header_background_color
                .unwrap_or(if material3 {
                    Color::TRANSPARENT
                } else if dark {
                    scheme.surface
                } else {
                    scheme.primary
                }),
            range_picker_header_foreground_color: data
                .range_picker_header_foreground_color
                .unwrap_or(if material3 {
                    scheme.on_surface_variant()
                } else if dark {
                    scheme.on_surface
                } else {
                    scheme.on_primary
                }),
            // A tinted primary under Material 2, a container role of its own
            // under Material 3 -- the strip stops being a faded version of
            // the selection and becomes a surface in its own right.
            range_selection_overlay_color: data.range_selection_overlay_color.clone(),
            range_selection_background_color: data.range_selection_background_color.unwrap_or(
                if material3 {
                    scheme.secondary_container()
                } else {
                    crate::elevation_overlay::with_opacity(
                        scheme.primary,
                        ResolvedDatePicker::RANGE_SELECTION_OPACITY,
                    )
                },
            ),
        }
    }
}

/// What one item of a carousel is drawn with -- upstream's
/// `_CarouselViewState._buildCarouselItem` reading `CarouselViewTheme.of`.
///
/// # The only component here with no defaults class
///
/// Every other theme in this file ends its chain in a generated
/// `_XDefaultsM3`. The carousel has none: all six defaults are `??` constants
/// written by hand in the build method.
///
/// That absence is why none of this session's recurring defaults-class
/// findings apply to it. There are no dead defaults to inherit, as the bottom
/// app bar has four of; no default reading another, as
/// [`ResolvedDatePicker`]'s toggle button reads its sub-header; and no third
/// constructor input, as [`ResolvedSearchView`] and [`ResolvedTimePicker`]
/// take. There is nothing for `gen_defaults` to regenerate, so there is
/// nothing it can leave behind.
///
/// # And they are resolved per item rather than per build
///
/// The chain runs inside `_buildCarouselItem(index)`, so every visible item
/// resolves the theme again. Nothing here depends on the index, so the answers
/// agree -- but they are six ladders per item rather than six per frame.
///
/// # A carousel item is flat, where everything else in this file is raised
///
/// The elevation default is **0**. A dialog, a menu, a search bar and a time
/// picker are all 3 or 6; an item in a carousel is already separated from its
/// neighbours by the padding, and lifting it would be saying the same thing
/// twice.
///
/// # The padding is on all four sides, and every other one here is on one axis
///
/// `EdgeInsets.all(4)`, against the menu's 8 vertical, the menu bar's 4
/// horizontal, the popup item's 12 horizontal and the search bar's 8
/// horizontal. A carousel scrolls one way but its items have to clear the
/// container's edge in both.
///
/// # The overlay is resolved whether or not anything will use it
///
/// `effectiveOverlayColor` is computed before the branch that decides between
/// an `InkWell` and a bare `GestureDetector`, and the `GestureDetector` has
/// nowhere to put it. So `enableSplash: false` gives an item that is
/// **tappable but silent** -- the tap still arrives, and nothing is painted to
/// say so.
///
/// Its fall-through returns `null` rather than transparent -- the same two
/// spellings of nothing that [`ResolvedSegmentedButton`] carries both of, and
/// this is the null one.
pub struct ResolvedCarouselView {
    pub padding: EdgeInsets,
    pub background_color: Color,
    pub elevation: f32,
    pub shape_radius: f32,
}

impl ResolvedCarouselView {
    /// Upstream's inline `EdgeInsets.all(4.0)`.
    pub const PADDING: f32 = 4.0;
    /// Upstream's inline elevation, which is flat.
    pub const ELEVATION: f32 = 0.0;
    /// Upstream's inline corner radius -- the same 28 the date picker's dialog
    /// and the docked search view use.
    pub const RADIUS: f32 = 28.0;
    pub const PRESSED_OVERLAY: f32 = 0.1;
    pub const HOVERED_OVERLAY: f32 = 0.08;

    /// Upstream's inline `overlayColor` resolver, whose fall-through is
    /// `null` and not transparent.
    pub fn overlay_for(states: WidgetStates, scheme: &ColorScheme) -> Option<Color> {
        let opacity = if states.contains(WidgetState::Pressed) {
            ResolvedCarouselView::PRESSED_OVERLAY
        } else if states.contains(WidgetState::Hovered) {
            ResolvedCarouselView::HOVERED_OVERLAY
        } else if states.contains(WidgetState::Focused) {
            ResolvedCarouselView::PRESSED_OVERLAY
        } else {
            return None;
        };
        Some(crate::elevation_overlay::with_opacity(
            scheme.on_surface,
            opacity,
        ))
    }

    /// What the item actually paints for these states, which is nothing at all
    /// without a splash -- see the type's docs.
    pub fn painted_overlay(
        enable_splash: bool,
        states: WidgetStates,
        scheme: &ColorScheme,
    ) -> Option<Color> {
        enable_splash
            .then(|| ResolvedCarouselView::overlay_for(states, scheme))
            .flatten()
    }

    pub fn of(context: &mut BuildContext) -> ResolvedCarouselView {
        let scheme = ThemeData::of(context).color_scheme;
        let data = CarouselViewTheme::of(context);
        ResolvedCarouselView {
            padding: data
                .padding
                .unwrap_or(EdgeInsets::all(ResolvedCarouselView::PADDING)),
            background_color: data.background_color.unwrap_or(scheme.surface),
            elevation: data.elevation.unwrap_or(ResolvedCarouselView::ELEVATION),
            shape_radius: ResolvedCarouselView::RADIUS,
        }
    }
}

/// Upstream `ThemeData.estimateBrightnessForColor`: whether text on this
/// colour should be light or dark.
///
/// # The threshold is not WCAG's, and upstream says so
///
/// The spec's is 0.0525; upstream uses **0.15**, with the comment that
/// "Material Design appears to bias more towards using light text than WCAG20
/// recommends" and that 0.15 "seemed close to what the Material Design spec
/// shows for its color palette". A number arrived at by looking at a picture,
/// written down as such.
///
/// The effect of the higher threshold is that more colours count as *light*,
/// so more of them get dark text.
///
/// # And it is squared before comparing
///
/// `(luminance + 0.05)^2 > 0.15` rather than `luminance > something`. The
/// `+ 0.05` and the square are the shape of WCAG's contrast ratio with one
/// side pinned to white, so the test is really "is white text worse than dark
/// text here", asked in the units the ratio is defined in.
pub fn estimate_brightness_for_color(color: Color) -> crate::platform::Brightness {
    let luminance = color.compute_luminance();
    if (luminance + 0.05) * (luminance + 0.05) > BRIGHTNESS_THRESHOLD {
        crate::platform::Brightness::Light
    } else {
        crate::platform::Brightness::Dark
    }
}

/// Upstream's `kThreshold` in `estimateBrightnessForColor` -- see there.
pub const BRIGHTNESS_THRESHOLD: f32 = 0.15;

/// What a `MaterialButton`'s label is coloured with -- upstream's
/// `ButtonThemeData.getTextColor`, and the getters it leans on.
///
/// # `ButtonTheme` is not kept alive by one boolean
///
/// A previous tick recorded that `ButtonTheme.of` had three call sites
/// upstream -- `ButtonBar` once and `DropdownButton` twice, both for
/// `alignedDropdown` -- and concluded that a whole theme survived for one
/// flag. **That was wrong.** There is a fourth, `material_button.dart:394`,
/// and it is the one the theme is actually for: `MaterialButton.build` reads
/// `getFillColor`, `getTextColor`, `getFocusColor`, `getHoverColor`, the four
/// elevation getters, `getPadding` and `getConstraints` off it.
///
/// The grep behind that claim was piped through `head -12` and
/// `material_button.dart` sorts after `dropdown_menu.dart`. The
/// `alignedDropdown` findings stand on their own; the sentence about the theme
/// living for a single bool does not.
///
/// # Disabled is checked first and changes less than it looks like
///
/// `getTextColor` opens with `if (!button.enabled) return getDisabledTextColor(button)`,
/// which reads as disabled beating the caller's own `textColor`. It does not:
/// `getDisabledTextColor` is `textColor ?? disabledTextColor ?? onSurface@0.38`,
/// so a `textColor` wins either way. **All the disabled branch decides is what
/// happens when there is none** -- and that is where `disabledTextColor` gets
/// its only chance to be read.
///
/// # A primary label is chosen against its own fill, not against the page
///
/// `normal` and `accent` answer from the ambient brightness and the scheme.
/// `primary` estimates the brightness **of the fill colour** and picks against
/// that, falling back to the ambient brightness only when there is no fill. A
/// button drawn in a dark colour on a light page gets white text, which asking
/// the page would have got wrong.
///
/// # And the two darks are different darks
///
/// `normal` returns `black87` and `primary` returns `black`. Text on the page
/// is body text and takes the Material 2 body black; text on a coloured fill
/// needs the whole of it. The eight-percent difference is the difference
/// between reading a label and reading a paragraph.
pub struct MaterialButtonColors;

impl MaterialButtonColors {
    /// Upstream's `Colors.black87`, the body-text black.
    pub const BLACK87: Color = Color(0xDD000000);
    /// Upstream's disabled label opacity.
    pub const DISABLED_OPACITY: f32 = 0.38;
    /// Upstream's disabled fill opacity for a primary button.
    pub const DISABLED_FILL_OPACITY: f32 = 0.12;

    /// Upstream's `getDisabledTextColor`, which still asks for `textColor`
    /// first -- see the type's docs.
    pub fn disabled_text_color(
        text_color: Option<Color>,
        disabled_text_color: Option<Color>,
        scheme: &ColorScheme,
    ) -> Color {
        text_color.or(disabled_text_color).unwrap_or_else(|| {
            crate::elevation_overlay::with_opacity(
                scheme.on_surface,
                MaterialButtonColors::DISABLED_OPACITY,
            )
        })
    }

    /// Upstream's `getBrightness`:
    ///
    /// ```dart
    /// Brightness getBrightness(MaterialButton button) {
    ///   return button.colorBrightness ?? colorScheme!.brightness;
    /// }
    /// ```
    ///
    /// The one member of `MaterialButton` nothing in this crate answered.
    /// [`MaterialButtonColors::text_color`] already took a brightness, so the
    /// machinery was complete and the override that feeds it was absent --
    /// and what that cost is specific: a button with a dark custom fill on a
    /// light page had no way to say so, and [`ButtonTextTheme::Normal`] reads
    /// the *page's* brightness, so the label came out black on a dark button.
    ///
    /// It reaches exactly one of the three text themes. `Normal` asks the
    /// brightness directly; `Primary` asks the **fill** first and only falls
    /// back to the brightness when there is no fill, so a coloured primary
    /// button was already right; `Accent` never asks. One override, one
    /// theme that depends on it, and two that were answered anyway -- which
    /// is why nothing noticed.
    pub fn brightness(
        color_brightness: Option<crate::platform::Brightness>,
        scheme: &ColorScheme,
    ) -> crate::platform::Brightness {
        color_brightness.unwrap_or(scheme.brightness)
    }

    /// Upstream's `getTextColor`.
    pub fn text_color(
        enabled: bool,
        text_color: Option<Color>,
        disabled_text_color: Option<Color>,
        text_theme: ButtonTextTheme,
        fill: Option<Color>,
        brightness: crate::platform::Brightness,
        scheme: &ColorScheme,
    ) -> Color {
        use crate::platform::Brightness;
        if !enabled {
            return MaterialButtonColors::disabled_text_color(
                text_color,
                disabled_text_color,
                scheme,
            );
        }
        if let Some(color) = text_color {
            return color;
        }
        match text_theme {
            ButtonTextTheme::Normal => match brightness {
                Brightness::Dark => Color::WHITE,
                Brightness::Light => MaterialButtonColors::BLACK87,
            },
            ButtonTextTheme::Accent => scheme.secondary,
            ButtonTextTheme::Primary => {
                // The fill decides, and the page only when there is no fill.
                let fill_is_dark = match fill {
                    Some(fill) => estimate_brightness_for_color(fill) == Brightness::Dark,
                    None => brightness == Brightness::Dark,
                };
                if fill_is_dark {
                    Color::WHITE
                } else {
                    // Not `BLACK87`: a label on a fill needs the whole black.
                    Color::BLACK
                }
            }
        }
    }

    /// Upstream's `getFillColor`, whose second clause is a **runtime-type
    /// test**: `if (button.runtimeType == MaterialButton) return null`.
    ///
    /// A plain `MaterialButton` gets no fill from the theme at all -- only the
    /// subclasses that used to sit on top of it did. It is written as an
    /// exact-type comparison rather than a virtual call, so being a
    /// `MaterialButton` and being *exactly* a `MaterialButton` are different
    /// answers. There is no Rust for that, so it arrives as a flag the caller
    /// sets, named for what it means rather than for how upstream spells it.
    pub fn fill_color(
        enabled: bool,
        color: Option<Color>,
        disabled_color: Option<Color>,
        is_exactly_material_button: bool,
        theme_button_color: Option<Color>,
        text_theme: ButtonTextTheme,
        scheme: &ColorScheme,
    ) -> Option<Color> {
        let own = if enabled { color } else { disabled_color };
        if own.is_some() {
            return own;
        }
        if is_exactly_material_button {
            return None;
        }
        if enabled {
            if let Some(color) = theme_button_color {
                return Some(color);
            }
        }
        Some(match text_theme {
            ButtonTextTheme::Normal | ButtonTextTheme::Accent => {
                if enabled {
                    scheme.primary
                } else {
                    crate::elevation_overlay::with_opacity(
                        scheme.on_surface,
                        MaterialButtonColors::DISABLED_FILL_OPACITY,
                    )
                }
            }
            ButtonTextTheme::Primary => {
                if enabled {
                    theme_button_color.unwrap_or(scheme.primary)
                } else {
                    crate::elevation_overlay::with_opacity(
                        scheme.on_surface,
                        MaterialButtonColors::DISABLED_FILL_OPACITY,
                    )
                }
            }
        })
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
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            icon_theme: lerp_icon_theme(&a.icon_theme, &b.icon_theme, t),
            actions_icon_theme: lerp_icon_theme(&a.actions_icon_theme, &b.actions_icon_theme, t),
            center_title: lerp_nearer(&a.center_title, &b.center_title, t),
            title_spacing: lerp_f32(a.title_spacing, b.title_spacing, t),
            leading_width: lerp_f32(a.leading_width, b.leading_width, t),
            toolbar_height: lerp_f32(a.toolbar_height, b.toolbar_height, t),
            toolbar_text_style: lerp_text_style(&a.toolbar_text_style, &b.toolbar_text_style, t),
            title_text_style: lerp_text_style(&a.title_text_style, &b.title_text_style, t),
            actions_padding: EdgeInsetsGeometry::lerp(a.actions_padding, b.actions_padding, t),
        }
    }
}

/// Upstream `AppBarTheme`.
pub struct AppBarTheme;

impl AppBarTheme {
    pub fn new(data: AppBarThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
    /// The elevation to use **while something is scrolled underneath**, which
    /// is a different number from the resting one: Material 3 lifts a bar off
    /// the content it is covering rather than keeping it flat. Upstream's
    /// default is 3, and its last step falls back to the resting elevation
    /// rather than to nothing.
    pub scrolled_under_elevation: f32,
    /// The icons on the trailing side. Upstream's chain reaches the bar's
    /// *leading* icon theme and then the theme's before the defaults, so a
    /// theme that set only `iconTheme` colours the actions too.
    pub actions_icon_theme: IconThemeData,
    /// How much room the leading slot reserves. Upstream's `_kLeadingWidth`
    /// is `kToolbarHeight`, with the comment "So the leading button is
    /// square" -- the number is the height for a reason.
    pub leading_width: f32,
    /// The style for text in the toolbar that is not the title.
    pub toolbar_text_style: Option<TextStyle>,
    /// The style for the title.
    ///
    /// Upstream's role for it is `titleLarge`, in both defaults tables, and
    /// nothing in this port read either the theme's field or the role: the
    /// bar drew its title with a hand-rolled style carrying a hard-coded
    /// weight of 700, where `titleLarge` is 400.
    ///
    /// Both styles are the *defaults* put through
    /// `copyWith(color: foregroundColor)`. The role brings the size, the
    /// weight and the family; the bar brings the ink, because a foreground
    /// colour is a property of the bar and not of the type scale. A style the
    /// theme named outright keeps its own colour -- it is past the defaults
    /// by then.
    pub title_text_style: Option<TextStyle>,
}

impl ResolvedAppBar {
    /// Upstream's `kToolbarHeight`.
    pub const TOOLBAR_HEIGHT: f32 = 56.0;
    /// Upstream's `NavigationToolbar.kMiddleSpacing`.
    pub const TITLE_SPACING: f32 = 16.0;
    /// Upstream's `_AppBarDefaultsM3.scrolledUnderElevation`.
    pub const SCROLLED_UNDER_ELEVATION: f32 = 3.0;
    /// Upstream's default action icon size.
    pub const ACTIONS_ICON_SIZE: f32 = 24.0;

    pub fn of(context: &mut BuildContext) -> ResolvedAppBar {
        ResolvedAppBar::of_with_center_title(context, None, 0)
    }

    /// [`ResolvedAppBar::of`] with the bar's own `centerTitle` and how many
    /// actions it has.
    ///
    /// Upstream's `_getEffectiveCenterTitle` is three levels deep and the
    /// bottom one is a **platform rule**:
    ///
    /// ```dart
    /// return centerTitle ?? appbarTheme.centerTitle ?? platformCenter();
    /// ```
    ///
    /// This resolver had the middle level and then `unwrap_or(false)`, so on
    /// iOS and macOS the title was never centred -- which is that platform's
    /// whole convention for a navigation bar.
    pub fn of_with_center_title(
        context: &mut BuildContext,
        center_title: Option<bool>,
        action_count: usize,
    ) -> ResolvedAppBar {
        let data = AppBarTheme::of(context);
        let scheme = ThemeData::of(context).color_scheme;
        let platform = ThemeData::of(context).platform;
        // Upstream's `foregroundColor`, computed once: both text styles are
        // their default role put through `copyWith(color: foregroundColor)`.
        let foreground = data.foreground_color.unwrap_or(scheme.on_surface);
        ResolvedAppBar {
            background: data.background_color.unwrap_or(scheme.surface),
            foreground,
            toolbar_height: data
                .toolbar_height
                .unwrap_or(ResolvedAppBar::TOOLBAR_HEIGHT),
            center_title: center_title
                .or(data.center_title)
                .unwrap_or_else(|| ResolvedAppBar::platform_center(platform, action_count)),
            title_spacing: data.title_spacing.unwrap_or(ResolvedAppBar::TITLE_SPACING),
            scrolled_under_elevation: data
                .scrolled_under_elevation
                .unwrap_or(ResolvedAppBar::SCROLLED_UNDER_ELEVATION),
            actions_icon_theme: data
                .actions_icon_theme
                .clone()
                .or_else(|| data.icon_theme.clone())
                .unwrap_or_else(|| {
                    IconThemeData::new()
                        .with_color(scheme.on_surface_variant())
                        .with_size(ResolvedAppBar::ACTIONS_ICON_SIZE)
                }),
            leading_width: data.leading_width.unwrap_or(ResolvedAppBar::TOOLBAR_HEIGHT),
            toolbar_text_style: data.toolbar_text_style.clone().or_else(|| {
                ThemeData::of(context)
                    .text_theme
                    .body_medium
                    .clone()
                    .map(|style| TextStyle {
                        color: foreground,
                        ..style
                    })
            }),
            title_text_style: data.title_text_style.clone().or_else(|| {
                ThemeData::of(context)
                    .text_theme
                    .title_large
                    .clone()
                    .map(|style| TextStyle {
                        color: foreground,
                        ..style
                    })
            }),
        }
    }

    /// Upstream's `platformCenter()`, which is the last word when neither the
    /// bar nor the theme has one.
    ///
    /// The Apple platforms centre a title **only while there are fewer than
    /// two actions**, and that clause is the interesting part: a centred title
    /// with buttons on both sides has to be short enough to fit between them,
    /// so a bar that has grown a second action gives up on centring rather
    /// than truncating the title. Everywhere else the title starts at the
    /// leading edge and the question does not arise.
    pub fn platform_center(
        platform: crate::editable_text::TargetPlatform,
        action_count: usize,
    ) -> bool {
        use crate::editable_text::TargetPlatform;
        match platform {
            TargetPlatform::IOS | TargetPlatform::MacOS => action_count < 2,
            TargetPlatform::Android
            | TargetPlatform::Fuchsia
            | TargetPlatform::Linux
            | TargetPlatform::Windows => false,
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
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
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
            constraints: BoxConstraints::lerp(a.constraints, b.constraints, t),
        }
    }
}

/// Upstream `BottomSheetTheme`.
pub struct BottomSheetTheme;

impl BottomSheetTheme {
    pub fn new(data: BottomSheetThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            content_text_style: lerp_text_style(&a.content_text_style, &b.content_text_style, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            behavior: lerp_nearer(&a.behavior, &b.behavior, t),
            width: lerp_f32(a.width, b.width, t),
            inset_padding: EdgeInsetsGeometry::lerp(a.inset_padding, b.inset_padding, t),
            // Upstream's `SnackBarThemeData.lerp` does not assign
            // `showCloseIcon`, so a blended theme loses it. Carried here at
            // the nearer end, for the reason given on
            // [`TooltipThemeData::lerp`].
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
        provide_theme(data, child)
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

    /// Upstream's `ListTileThemeData.visualDensity`, the middle step of
    /// `visualDensity ?? tileTheme.visualDensity ?? theme.visualDensity`.
    pub fn with_visual_density(mut self, density: VisualDensity) -> Self {
        self.visual_density = Some(density);
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
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            style: lerp_nearer(&a.style, &b.style, t),
            selected_color: lerp_color(a.selected_color, b.selected_color, t),
            icon_color: lerp_color(a.icon_color, b.icon_color, t),
            text_color: lerp_color(a.text_color, b.text_color, t),
            title_text_style: lerp_text_style(&a.title_text_style, &b.title_text_style, t),
            subtitle_text_style: lerp_text_style(&a.subtitle_text_style, &b.subtitle_text_style, t),
            leading_and_trailing_text_style: lerp_text_style(
                &a.leading_and_trailing_text_style,
                &b.leading_and_trailing_text_style,
                t,
            ),
            content_padding: EdgeInsetsGeometry::lerp(a.content_padding, b.content_padding, t),
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
        provide_theme(data, child)
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
    /// Upstream's `visualDensity ?? tileTheme.visualDensity ?? theme.visualDensity`.
    ///
    /// Carried here rather than read at the call site because it is the third
    /// step of the same chain everything else on this struct came down, and a
    /// caller that reached past it to `ThemeData` would skip the tile theme.
    pub visual_density: VisualDensity,
    /// The three text styles, each `tile ?? tileTheme ?? defaults`.
    ///
    /// Upstream's M3 defaults are three *different* roles -- `bodyLarge` for
    /// the title, `bodyMedium` for the subtitle, `labelSmall` for whatever
    /// sits at the ends -- so a tile whose three styles were one style would
    /// look wrong in a way no single number shows.
    pub title_text_style: Option<TextStyle>,
    pub subtitle_text_style: Option<TextStyle>,
    pub leading_and_trailing_text_style: Option<TextStyle>,
    /// Upstream's
    /// `titleAlignment ?? tileTheme.titleAlignment ?? (useMaterial3 ?
    /// threeLine : titleHeight)`.
    ///
    /// The tile used to hard-code the `ThreeLine` rule, so the theme's field
    /// reached nothing and four of the five variants were unreachable.
    pub title_alignment: ListTileTitleAlignment,
}

impl ResolvedListTile {
    /// The one-line tile height.
    ///
    /// Upstream has **no constant for this**. `ListTile.minTileHeight` is a
    /// nullable parameter, and the numbers live in its doc comment: "default
    /// tile heights are 56.0, 72.0, and 88.0 for one, two, three lines" and,
    /// dense, "48.0, 64.0, and 76.0". So this is one of six, and the other
    /// five are not carried here.
    pub const MIN_TILE_HEIGHT: f32 = 56.0;
    pub const DENSE_MIN_TILE_HEIGHT: f32 = 48.0;

    /// Upstream's `_RenderListTile._effectiveHorizontalTitleGap`:
    /// `_horizontalTitleGap + visualDensity.horizontal * 2.0`.
    ///
    /// # Two pixels per unit here, four everywhere else
    ///
    /// This is the **other** half of the visual density, and it does not go
    /// through `baseSizeAdjustment`. That method multiplies by 4 -- Material's
    /// "four pixel increments" -- and upstream deliberately does not use it
    /// here: the gap moves by `horizontal * 2.0`, half as far.
    ///
    /// So a port reaching for `base_size_adjustment().0`, which is the
    /// obvious thing to reach for having just used `.1` for the height, moves
    /// this gap **twice as far as upstream does**. The two halves of one
    /// `VisualDensity` are not used symmetrically.
    ///
    /// A compact density can drive the gap negative, and upstream does not
    /// clamp it -- the title simply sits nearer the leading widget than the
    /// nominal gap, which is what asking for a compact layout means.
    pub fn effective_horizontal_title_gap(&self) -> f32 {
        self.horizontal_title_gap + self.visual_density.horizontal * 2.0
    }

    /// Upstream's `_RenderListTile._defaultTileHeight`: the height a tile
    /// falls back to when nothing set `minTileHeight`.
    ///
    /// # Six numbers, not two, and the choice is not only about `dense`
    ///
    /// ```dart
    /// baseDensity.dy + switch ((isThreeLine, subtitle != null)) {
    ///   (true, _)      => isDense ? 76.0 : 88.0,  // 3 lines
    ///   (false, true)  => isDense ? 64.0 : 72.0,  // 2 lines
    ///   (false, false) => isDense ? 48.0 : 56.0,  // 1 line
    /// }
    /// ```
    ///
    /// This crate carried the last row only, and
    /// [`ResolvedListTile::of`] described it as though `dense ? 48 : 56` were
    /// the whole rule. A tile with a subtitle therefore asked for **56 where
    /// upstream asks 72**, and a three-line tile for 56 where upstream asks
    /// 88 -- a row too short by half a line, which is the sort of thing that
    /// shows up as clipped descenders rather than as anything obviously
    /// broken.
    ///
    /// **Three lines wins over a subtitle**: the first arm ignores whether
    /// there is one, because a tile declaring itself three-line has already
    /// said how tall it is however its slots are filled.
    ///
    /// The visual density's `dy` is **added**, not multiplied, and it is
    /// signed -- a compact density is negative and makes every row shorter by
    /// the same amount rather than by a proportion.
    pub fn default_tile_height(
        is_three_line: bool,
        has_subtitle: bool,
        dense: bool,
        density_dy: f32,
    ) -> f32 {
        let rows = match (is_three_line, has_subtitle) {
            (true, _) => {
                if dense {
                    76.0
                } else {
                    88.0
                }
            }
            (false, true) => {
                if dense {
                    64.0
                } else {
                    72.0
                }
            }
            (false, false) => {
                if dense {
                    ResolvedListTile::DENSE_MIN_TILE_HEIGHT
                } else {
                    ResolvedListTile::MIN_TILE_HEIGHT
                }
            }
        };
        density_dy + rows
    }
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
    /// `data.minTileHeight ?? default_tile_height(..)`: a theme that set the
    /// height explicitly wins outright and `dense` changes nothing, while a
    /// theme that did not gets one of **six** heights -- see
    /// [`ResolvedListTile::default_tile_height`], which is chosen by the
    /// tile's line count as well as by `dense`. Adjusting the height
    /// afterwards cannot tell those two cases apart.
    ///
    /// What this answers is the **one-line** fallback, because the line count
    /// is the tile's own and is not known here; `ListTile::build` asks
    /// `default_tile_height` with the facts it has.
    pub fn of(
        context: &mut BuildContext,
        selected: bool,
        dense_override: Option<bool>,
    ) -> ResolvedListTile {
        ResolvedListTile::of_with_selected_color(context, selected, dense_override, None)
    }

    /// [`ResolvedListTile::of`] with the widget's own `selectedColor`.
    ///
    /// Upstream's `ListTile` takes one and its three control tiles all pass
    /// theirs: `SwitchListTile.build` hands over `selectedColor:
    /// effectiveActiveColor`, so **a selected switch row's title is the
    /// switch's own colour** rather than the theme's selected colour. A page
    /// of settings rows with a green switch would otherwise have its selected
    /// row's title in the theme's primary, which is a second accent colour on
    /// the same line as the first.
    pub fn of_with_selected_color(
        context: &mut BuildContext,
        selected: bool,
        dense_override: Option<bool>,
        selected_color: Option<Color>,
    ) -> ResolvedListTile {
        ResolvedListTile::of_with_density(context, selected, dense_override, selected_color, None)
    }

    /// The same, with the widget's own visual density as the first step of
    /// upstream's `visualDensity ?? tileTheme.visualDensity ??
    /// theme.visualDensity`.
    pub fn of_with_density(
        context: &mut BuildContext,
        selected: bool,
        dense_override: Option<bool>,
        selected_color: Option<Color>,
        density_override: Option<VisualDensity>,
    ) -> ResolvedListTile {
        let data = ListTileTheme::of(context);
        let theme = ThemeData::of(context);
        let dense = dense_override.or(data.dense).unwrap_or(false);
        let text_color = if selected {
            // The widget's own is above the theme's, which is above the
            // scheme's -- upstream's order, and the widget's is the one a
            // control tile fills in.
            selected_color
                .or(data.selected_color)
                .unwrap_or(theme.color_scheme.primary)
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
            visual_density: density_override
                .or(data.visual_density)
                .unwrap_or(ThemeData::of(context).visual_density),
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
            title_text_style: data
                .title_text_style
                .clone()
                .or_else(|| theme.text_theme.body_large.clone()),
            subtitle_text_style: data
                .subtitle_text_style
                .clone()
                .or_else(|| theme.text_theme.body_medium.clone()),
            leading_and_trailing_text_style: data
                .leading_and_trailing_text_style
                .clone()
                .or_else(|| theme.text_theme.label_small.clone()),
            title_alignment: data.title_alignment.unwrap_or(if theme.use_material3 {
                ListTileTitleAlignment::ThreeLine
            } else {
                ListTileTitleAlignment::TitleHeight
            }),
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
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            alignment: AlignmentGeometry::lerp(a.alignment, b.alignment, t),
            title_text_style: lerp_text_style(&a.title_text_style, &b.title_text_style, t),
            content_text_style: lerp_text_style(&a.content_text_style, &b.content_text_style, t),
            actions_padding: EdgeInsetsGeometry::lerp(a.actions_padding, b.actions_padding, t),
            icon_color: lerp_color(a.icon_color, b.icon_color, t),
            barrier_color: lerp_color(a.barrier_color, b.barrier_color, t),
            inset_padding: lerp_edge_insets(a.inset_padding, b.inset_padding, t),
            constraints: BoxConstraints::lerp(a.constraints, b.constraints, t),
        }
    }
}

/// Upstream `DialogTheme`.
pub struct DialogTheme;

impl DialogTheme {
    pub fn new(data: DialogThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            label_padding: EdgeInsetsGeometry::lerp(a.label_padding, b.label_padding, t),
            padding: EdgeInsetsGeometry::lerp(a.padding, b.padding, t),
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
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            label_style: lerp_text_style(&a.label_style, &b.label_style, t),
            secondary_label_style: lerp_text_style(
                &a.secondary_label_style,
                &b.secondary_label_style,
                t,
            ),
            brightness: lerp_nearer(&a.brightness, &b.brightness, t),
            icon_theme: lerp_icon_theme(&a.icon_theme, &b.icon_theme, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            press_elevation: lerp_f32(a.press_elevation, b.press_elevation, t),
            avatar_box_constraints: BoxConstraints::lerp(
                a.avatar_box_constraints,
                b.avatar_box_constraints,
                t,
            ),
            delete_icon_box_constraints: BoxConstraints::lerp(
                a.delete_icon_box_constraints,
                b.delete_icon_box_constraints,
                t,
            ),
        }
    }
}

/// Upstream `ChipTheme`.
pub struct ChipTheme;

impl ChipTheme {
    pub fn new(data: ChipThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
    /// The tick on a selected chip. Upstream's M3 default is **null**, which
    /// means "whatever the label is written in" rather than a colour of its
    /// own -- so `None` here is an answer and not a gap.
    pub checkmark_color: Option<Color>,
    /// Upstream's chain runs through the icon theme before reaching the
    /// defaults, so a theme that set only an icon colour still colours the
    /// delete cross.
    pub delete_icon_color: Option<Color>,
    pub selected_shadow_color: Option<Color>,
    /// Upstream has no default for these two: the layout falls back to a
    /// square of the content's own size, which is a layout rule rather than a
    /// theme value. `None` is what a widget checks for.
    pub avatar_box_constraints: Option<BoxConstraints>,
    pub delete_icon_box_constraints: Option<BoxConstraints>,
    /// The label's style.
    ///
    /// A **selected choice chip** takes `secondaryLabelStyle` instead, and
    /// its fill takes `secondarySelectedColor` -- see
    /// [`ResolvedChip::of_choice`]. Those two fields exist for exactly one
    /// widget, which is why nothing reached them until that widget had a
    /// resolution step of its own.
    pub label_style: Option<TextStyle>,
}

impl ResolvedChip {
    /// Upstream's default chip padding.
    pub const PADDING: f32 = 4.0;

    pub fn of(
        context: &mut BuildContext,
        states: WidgetStates,
        default_fill: Color,
    ) -> ResolvedChip {
        ResolvedChip::resolve(context, states, default_fill, false)
    }

    /// [`ResolvedChip::of`] for a `ChoiceChip`, which upstream builds as
    ///
    /// ```dart
    /// labelStyle: labelStyle ?? (selected ? chipTheme.secondaryLabelStyle : null),
    /// selectedColor: selectedColor ?? chipTheme.secondarySelectedColor,
    /// ```
    ///
    /// The `secondary` pair is the theme's answer for "the one chip in
    /// this row that is chosen", and it is separate from `selectedColor`
    /// because a filter chip's selection is a toggle while a choice
    /// chip's is a pick -- the two want to look different.
    pub fn of_choice(
        context: &mut BuildContext,
        states: WidgetStates,
        default_fill: Color,
    ) -> ResolvedChip {
        ResolvedChip::resolve(context, states, default_fill, true)
    }

    fn resolve(
        context: &mut BuildContext,
        states: WidgetStates,
        default_fill: Color,
        choice: bool,
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
                // A chosen choice chip asks the theme's `secondary` slot
                // first; every other chip has no such slot to ask.
                if choice {
                    data.secondary_selected_color.or(data.selected_color)
                } else {
                    data.selected_color
                }
            } else {
                None
            })
            .or(data.background_color)
            .unwrap_or(default_fill);
        ResolvedChip {
            fill,
            side: data.side,
            checkmark_color: data.checkmark_color,
            delete_icon_color: data
                .delete_icon_color
                .or_else(|| data.icon_theme.as_ref().and_then(|icons| icons.color))
                .or(Some(if disabled {
                    ThemeData::of(context).color_scheme.on_surface
                } else {
                    ThemeData::of(context).color_scheme.on_surface_variant()
                })),
            selected_shadow_color: data.selected_shadow_color,
            avatar_box_constraints: data.avatar_box_constraints,
            delete_icon_box_constraints: data.delete_icon_box_constraints,
            label_style: if choice && selected {
                data.secondary_label_style
                    .clone()
                    .or_else(|| data.label_style.clone())
            } else {
                data.label_style.clone()
            },
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
            indicator: crate::decoration::Decoration::lerp(
                a.indicator.clone(),
                b.indicator.clone(),
                t,
            ),
            indicator_color: lerp_color(a.indicator_color, b.indicator_color, t),
            indicator_size: lerp_nearer(&a.indicator_size, &b.indicator_size, t),
            divider_color: lerp_color(a.divider_color, b.divider_color, t),
            divider_height: lerp_f32(a.divider_height, b.divider_height, t),
            label_color: lerp_color(a.label_color, b.label_color, t),
            label_padding: EdgeInsetsGeometry::lerp(a.label_padding, b.label_padding, t),
            label_style: lerp_text_style(&a.label_style, &b.label_style, t),
            unselected_label_color: lerp_color(
                a.unselected_label_color,
                b.unselected_label_color,
                t,
            ),
            unselected_label_style: lerp_text_style(
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
        provide_theme(data, child)
    }

    pub fn of(context: &mut BuildContext) -> TabBarThemeData {
        context
            .inherited::<TabBarThemeData>()
            .map(|data| (*data).clone())
            .unwrap_or_else(|| ThemeData::of(context).tab_bar_theme)
    }
}

/// What a tab bar draws with, once the three steps have run.
/// The role a tab's label takes when nobody named a style.
///
/// One function because upstream's tables give the selected and the
/// unselected label the *same* answer, in all three of them, and a
/// correction to one is a correction to both.
fn tab_label_role(theme: &ThemeData) -> Option<TextStyle> {
    if theme.use_material3 {
        theme.text_theme.title_small.clone()
    } else {
        theme.primary_text_theme.body_large.clone()
    }
}

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
    /// A whole decoration to draw instead of the underline. `None` means
    /// "draw the underline from [`ResolvedTabBar::indicator_color`] and the
    /// weight" -- upstream has no default for it either.
    pub indicator: Option<crate::decoration::Decoration>,
    /// Where the tabs sit in a bar wider than they are.
    ///
    /// Upstream's default depends on **whether the bar scrolls**, and on the
    /// Material version: a scrolling bar starts at the leading edge, with an
    /// offset under Material 3 that Material 2 does not have, and a bar that
    /// does not scroll fills.
    pub tab_alignment: TabAlignment,
    /// How the indicator travels between tabs. Upstream's default depends on
    /// [`ResolvedTabBar::indicator_size`]: an indicator as wide as its label
    /// stretches and settles (`Elastic`) while one as wide as the whole tab
    /// slides (`Linear`) -- the elastic feel reads as the underline reaching
    /// for the next word, which only makes sense when it is word-shaped.
    pub indicator_animation: TabIndicatorAnimation,
    /// `None` leaves the ambient text scale alone, which is what upstream
    /// passing it into `MediaQuery.copyWith` amounts to.
    pub text_scaler: Option<crate::painting::TextScaler>,
}

impl ResolvedTabBar {
    /// Upstream's Material 3 default indicator thickness.
    pub const INDICATOR_WEIGHT: f32 = 3.0;
    /// Upstream's default divider height.
    pub const DIVIDER_HEIGHT: f32 = 1.0;
    /// Upstream's pre-M3 unselected-label alpha: seventy per cent.
    pub const UNSELECTED_ALPHA: u8 = 0xB2;

    pub fn of(context: &mut BuildContext) -> ResolvedTabBar {
        ResolvedTabBar::of_bar(context, false)
    }

    /// [`ResolvedTabBar::of`] for a bar that knows whether it scrolls, which
    /// two of upstream's defaults depend on.
    pub fn of_bar(context: &mut BuildContext, scrollable: bool) -> ResolvedTabBar {
        let data = TabBarTheme::of(context);
        let material = ThemeData::of(context);
        let scheme = material.color_scheme;
        // Upstream's Material 3 default is `TabBarIndicatorSize.tab`, and
        // it is resolved first because the animation's default reads it.
        let indicator_size = data.indicator_size.unwrap_or(TabBarIndicatorSize::Tab);
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
            indicator_size,
            label_padding: data
                .label_padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::symmetric(16.0, 0.0)),
            // Upstream's three tables agree that both styles are the same
            // role as each other -- a selected tab is told apart by its
            // colour and its underline, not by being a different size. What
            // they disagree about is which role: `titleSmall` under Material
            // 3, and `primaryTextTheme.bodyLarge` under Material 2.
            //
            // `primaryTextTheme` is the scale for text drawn *on* a
            // primary-coloured surface, which is what a Material 2 tab bar
            // is: it sits in the app bar. Material 3's does not, so it reads
            // the ordinary scale.
            label_style: data
                .label_style
                .clone()
                .or_else(|| tab_label_role(&material)),
            unselected_label_style: data
                .unselected_label_style
                .clone()
                .or_else(|| tab_label_role(&material)),
            indicator: data.indicator.clone(),
            tab_alignment: data.tab_alignment.unwrap_or(if scrollable {
                if material.use_material3 {
                    TabAlignment::StartOffset
                } else {
                    TabAlignment::Start
                }
            } else {
                TabAlignment::Fill
            }),
            indicator_animation: data.indicator_animation.unwrap_or(match indicator_size {
                TabBarIndicatorSize::Label => TabIndicatorAnimation::Elastic,
                TabBarIndicatorSize::Tab => TabIndicatorAnimation::Linear,
            }),
            text_scaler: data.text_scaler,
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
            decoration: crate::decoration::Decoration::lerp(
                a.decoration.clone(),
                b.decoration.clone(),
                t,
            ),
            data_row_color: lerp_state_color(
                a.data_row_color.as_ref(),
                b.data_row_color.as_ref(),
                t,
            ),
            data_row_min_height: lerp_f32(a.data_row_min_height, b.data_row_min_height, t),
            data_row_max_height: lerp_f32(a.data_row_max_height, b.data_row_max_height, t),
            data_text_style: lerp_text_style(&a.data_text_style, &b.data_text_style, t),
            heading_row_color: lerp_state_color(
                a.heading_row_color.as_ref(),
                b.heading_row_color.as_ref(),
                t,
            ),
            heading_row_height: lerp_f32(a.heading_row_height, b.heading_row_height, t),
            heading_text_style: lerp_text_style(&a.heading_text_style, &b.heading_text_style, t),
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
        provide_theme(data, child)
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
            unselected_label_text_style: lerp_text_style(
                &a.unselected_label_text_style,
                &b.unselected_label_text_style,
                t,
            ),
            selected_label_text_style: lerp_text_style(
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
            indicator_shape: ShapeBorder::lerp(
                a.indicator_shape.clone(),
                b.indicator_shape.clone(),
                t,
            ),
            min_width: lerp_f32(a.min_width, b.min_width, t),
            min_extended_width: lerp_f32(a.min_extended_width, b.min_extended_width, t),
        }
    }
}

/// Upstream `NavigationRailTheme`.
pub struct NavigationRailTheme;

impl NavigationRailTheme {
    pub fn new(data: NavigationRailThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            selected_label_style: lerp_text_style(
                &a.selected_label_style,
                &b.selected_label_style,
                t,
            ),
            unselected_label_style: lerp_text_style(
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
        provide_theme(data, child)
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
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            end_shape: ShapeBorder::lerp(a.end_shape.clone(), b.end_shape.clone(), t),
            width: lerp_f32(a.width, b.width, t),
        }
    }
}

/// Upstream `DrawerTheme`.
pub struct DrawerTheme;

impl DrawerTheme {
    pub fn new(data: DrawerThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
    /// The two shapes, one per edge the drawer can open from.
    ///
    /// Not one shape mirrored: upstream's `_DrawerDefaultsM3` rounds the
    /// corners on the side **facing the page**, which is the trailing side
    /// for a start drawer and the leading side for an end one, so the two
    /// defaults are two shapes and `Drawer.build` picks by `isDrawerStart`.
    /// Neither reached anything here -- `ResolvedDrawer` had no shape at all.
    pub shape: ShapeBorder,
    pub end_shape: ShapeBorder,
    pub background: Color,
    pub scrim: Color,
    pub width: f32,
}

impl ResolvedDrawer {
    /// Upstream's `_kWidth`.
    pub const WIDTH: f32 = 304.0;
    /// The scrim behind an open drawer: black at 54 per cent.
    ///
    /// Upstream names no constant for it either -- `Drawer.scrimColor` is
    /// nullable and its doc says the fallback "defaults to `Colors.black54`",
    /// so the number comes from the palette rather than from drawer.dart.
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
            shape: data
                .shape
                .clone()
                .unwrap_or_else(|| ResolvedDrawer::default_shape(false)),
            end_shape: data
                .end_shape
                .clone()
                .unwrap_or_else(|| ResolvedDrawer::default_shape(true)),
        }
    }

    /// Upstream's `_DrawerDefaultsM3.shape` and `.endShape`: sixteen logical
    /// pixels of rounding on the edge that faces the page.
    ///
    /// The radius is directional -- `BorderRadiusDirectional.horizontal(end:)`
    /// for a start drawer and `(start:)` for an end one -- so under RTL the
    /// rounded side swaps with the drawer, which is the point of naming it
    /// that way rather than left and right.
    pub fn default_shape(end_drawer: bool) -> ShapeBorder {
        let corner = crate::borders::Radius::circular(16.0);
        let radius = if end_drawer {
            crate::borders::BorderRadiusDirectional {
                top_start: corner,
                bottom_start: corner,
                top_end: crate::borders::Radius::ZERO,
                bottom_end: crate::borders::Radius::ZERO,
            }
        } else {
            crate::borders::BorderRadiusDirectional {
                top_start: crate::borders::Radius::ZERO,
                bottom_start: crate::borders::Radius::ZERO,
                top_end: corner,
                bottom_end: corner,
            }
        };
        ShapeBorder::Rounded(crate::borders::RoundedRectangleBorder::new(
            BorderSide::NONE,
            crate::borders::BorderRadiusGeometry::Directional(radius),
        ))
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
            text_style: lerp_state_text_style(a.text_style.as_ref(), b.text_style.as_ref(), t),
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
            elevation: lerp_state_f32(a.elevation.as_ref(), b.elevation.as_ref(), t),
            padding: lerp_state_insets(a.padding.as_ref(), b.padding.as_ref(), t),
            minimum_size: lerp_state_size(a.minimum_size.as_ref(), b.minimum_size.as_ref(), t),
            fixed_size: lerp_state_size(a.fixed_size.as_ref(), b.fixed_size.as_ref(), t),
            maximum_size: lerp_state_size(a.maximum_size.as_ref(), b.maximum_size.as_ref(), t),
            icon_color: lerp_state_color(a.icon_color.as_ref(), b.icon_color.as_ref(), t),
            icon_size: lerp_state_f32(a.icon_size.as_ref(), b.icon_size.as_ref(), t),
            icon_alignment: lerp_nearer(&a.icon_alignment, &b.icon_alignment, t),
            side: lerp_state_side(a.side.as_ref(), b.side.as_ref(), t),
            shape: lerp_state_shape(a.shape.as_ref(), b.shape.as_ref(), t),
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
            alignment: AlignmentGeometry::lerp(a.alignment, b.alignment, t),
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
        provide_theme(data, child)
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
        provide_theme(data, child)
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
        provide_theme(data, child)
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
        provide_theme(data, child)
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
        provide_theme(data, child)
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
    /// Which side of the label the icon sits on, and how long the button
    /// takes to move between its states.
    ///
    /// Upstream resolves both through the same `effectiveValue` walk as
    /// everything above -- widget style, theme style, defaults -- and hands
    /// them to the button it builds. Neither reached anything here.
    ///
    /// The last step for both is the *button's* own default rather than the
    /// theme's, which is why they arrive through `defaults` like the rest:
    /// `IconAlignment.start` and `kThemeChangeDuration`.
    pub icon_alignment: IconAlignment,
    pub animation_duration: std::time::Duration,
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
            icon_alignment: style.icon_alignment.unwrap_or(defaults.icon_alignment),
            animation_duration: style
                .animation_duration
                .unwrap_or(defaults.animation_duration),
        }
    }

    /// Upstream's `kThemeChangeDuration`, which is what a button with no
    /// style of its own animates over.
    pub const ANIMATION_DURATION: std::time::Duration = std::time::Duration::from_millis(200);
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
            content_text_style: lerp_text_style(&a.content_text_style, &b.content_text_style, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            padding: EdgeInsetsGeometry::lerp(a.padding, b.padding, t),
            leading_padding: EdgeInsetsGeometry::lerp(a.leading_padding, b.leading_padding, t),
        }
    }
}

/// Upstream `MaterialBannerTheme`.
pub struct MaterialBannerTheme;

impl MaterialBannerTheme {
    pub fn new(data: MaterialBannerThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            tile_padding: EdgeInsetsGeometry::lerp(a.tile_padding, b.tile_padding, t),
            expanded_alignment: AlignmentGeometry::lerp(
                a.expanded_alignment,
                b.expanded_alignment,
                t,
            ),
            children_padding: EdgeInsetsGeometry::lerp(a.children_padding, b.children_padding, t),
            icon_color: lerp_color(a.icon_color, b.icon_color, t),
            collapsed_icon_color: lerp_color(a.collapsed_icon_color, b.collapsed_icon_color, t),
            text_color: lerp_color(a.text_color, b.text_color, t),
            collapsed_text_color: lerp_color(a.collapsed_text_color, b.collapsed_text_color, t),
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            collapsed_shape: ShapeBorder::lerp(
                a.collapsed_shape.clone(),
                b.collapsed_shape.clone(),
                t,
            ),
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
        provide_theme(data, child)
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
        provide_theme(data, child)
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
            // Upstream wraps these two in `WidgetStateProperty.lerp<bool?>`
            // with `_lerpBool`, which is `t < 0.5 ? a : b` -- a step, per
            // resolved state. Stepping every state on the same `t` is the
            // same answer as stepping the whole property, so `lerp_nearer`
            // is not a shortcut here; it is the same function.
            thumb_visibility: lerp_nearer(&a.thumb_visibility, &b.thumb_visibility, t),
            thickness: lerp_state_f32(a.thickness.as_ref(), b.thickness.as_ref(), t),
            track_visibility: lerp_nearer(&a.track_visibility, &b.track_visibility, t),
            interactive: lerp_nearer(&a.interactive, &b.interactive, t),
            radius: crate::borders::Radius::lerp_optional(a.radius, b.radius, t),
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
        provide_theme(data, child)
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
    /// The line down the edge of the track.
    ///
    /// Upstream's default is **brightness-dependent**: `onSurface` at a tenth
    /// opacity under a light theme and a quarter under a dark one, because a
    /// faint line on a dark ground disappears at the opacity that reads as
    /// faint on a light one.
    pub track_border_color: Color,
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
            track_border_color: data
                .track_border_color
                .as_ref()
                .and_then(|property| property.resolve(states))
                .unwrap_or_else(|| {
                    let ink = scheme.on_surface;
                    match ThemeData::of(context).brightness() {
                        crate::platform::Brightness::Light => {
                            ink.with_alpha((ink.alpha() as f32 * 0.1).round() as u8)
                        }
                        crate::platform::Brightness::Dark => {
                            ink.with_alpha((ink.alpha() as f32 * 0.25).round() as u8)
                        }
                    }
                }),
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
            elevation: lerp_state_f32(a.elevation.as_ref(), b.elevation.as_ref(), t),
            padding: lerp_state_insets(a.padding.as_ref(), b.padding.as_ref(), t),
            minimum_size: lerp_state_size(a.minimum_size.as_ref(), b.minimum_size.as_ref(), t),
            fixed_size: lerp_state_size(a.fixed_size.as_ref(), b.fixed_size.as_ref(), t),
            maximum_size: lerp_state_size(a.maximum_size.as_ref(), b.maximum_size.as_ref(), t),
            side: lerp_state_side(a.side.as_ref(), b.side.as_ref(), t),
            shape: lerp_state_shape(a.shape.as_ref(), b.shape.as_ref(), t),
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
            alignment: AlignmentGeometry::lerp(a.alignment, b.alignment, t),
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
        provide_theme(data, child)
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
        provide_theme(data, child)
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
        provide_theme(data, child)
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
        provide_theme(data, child)
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
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            enable_feedback: lerp_nearer(&a.enable_feedback, &b.enable_feedback, t),
            icon_size: lerp_f32(a.icon_size, b.icon_size, t),
            size_constraints: BoxConstraints::lerp(a.size_constraints, b.size_constraints, t),
            small_size_constraints: BoxConstraints::lerp(
                a.small_size_constraints,
                b.small_size_constraints,
                t,
            ),
            large_size_constraints: BoxConstraints::lerp(
                a.large_size_constraints,
                b.large_size_constraints,
                t,
            ),
            extended_size_constraints: BoxConstraints::lerp(
                a.extended_size_constraints,
                b.extended_size_constraints,
                t,
            ),
            extended_icon_label_spacing: lerp_f32(
                a.extended_icon_label_spacing,
                b.extended_icon_label_spacing,
                t,
            ),
            extended_padding: EdgeInsetsGeometry::lerp(a.extended_padding, b.extended_padding, t),
            extended_text_style: lerp_text_style(&a.extended_text_style, &b.extended_text_style, t),
            mouse_cursor: lerp_nearer(&a.mouse_cursor, &b.mouse_cursor, t),
        }
    }
}

/// Upstream `FloatingActionButtonTheme`.
pub struct FloatingActionButtonTheme;

impl FloatingActionButtonTheme {
    pub fn new(data: FloatingActionButtonThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
    /// The constraints for **this kind** of button. Upstream switches on
    /// `_FloatingActionButtonType` and reads one of four theme fields; this
    /// read `size_constraints` whatever the button was, so three of the four
    /// reached nothing.
    pub size: BoxConstraints,
    /// The gap between the icon and the label, for the extended form only.
    /// Upstream's chain ends at a literal `8.0` rather than a default-table
    /// entry, which is why the number is here and not beside the others.
    pub extended_icon_label_spacing: f32,
    /// The extended form's padding. Upstream's M3 default is
    /// **asymmetric and depends on whether there is an icon**:
    /// `EdgeInsetsDirectional.only(start: hasChild ? 16 : 20, end: 20)` -- a
    /// label with no icon beside it needs the same room on both sides, and
    /// one with an icon does not.
    pub extended_padding: EdgeInsetsGeometry,
    /// The extended form's label style, `textTheme.labelLarge` by default.
    pub extended_text_style: Option<TextStyle>,
}

impl ResolvedFloatingActionButton {
    /// Upstream's `_defaultElevation`.
    pub const ELEVATION: f32 = 6.0;
    /// Upstream's `_defaultHighlightElevation`.
    pub const HIGHLIGHT_ELEVATION: f32 = 12.0;
    /// The regular button's side.
    ///
    /// Upstream resolves a `BoxConstraints sizeConstraints` from the theme and
    /// the M2/M3 defaults rather than declaring a constant; 56 is what its
    /// class doc states ("width of 56.0 logical pixels").
    pub const SIZE: f32 = 56.0;

    /// Upstream's four size tables, from `_FABDefaultsM3`.
    pub const SMALL_SIZE: f32 = 40.0;
    pub const LARGE_SIZE: f32 = 96.0;
    /// The extended form fixes only the height: its width is its content's.
    pub const EXTENDED_HEIGHT: f32 = 56.0;
    pub const EXTENDED_ICON_LABEL_SPACING: f32 = 8.0;

    pub fn of(context: &mut BuildContext, states: WidgetStates) -> ResolvedFloatingActionButton {
        ResolvedFloatingActionButton::of_kind(
            context,
            states,
            crate::buttons::FloatingActionButtonKind::Regular,
            false,
        )
    }

    /// [`ResolvedFloatingActionButton::of`] for a button that knows which of
    /// upstream's four it is, and -- for the extended form -- whether it has
    /// an icon, which changes its padding.
    pub fn of_kind(
        context: &mut BuildContext,
        states: WidgetStates,
        kind: crate::buttons::FloatingActionButtonKind,
        has_icon: bool,
    ) -> ResolvedFloatingActionButton {
        use crate::buttons::FloatingActionButtonKind;
        let data = FloatingActionButtonTheme::of(context);
        let theme = ThemeData::of(context);
        let scheme = theme.color_scheme;
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
            size: match kind {
                FloatingActionButtonKind::Regular => data.size_constraints,
                FloatingActionButtonKind::Small => data.small_size_constraints,
                FloatingActionButtonKind::Large => data.large_size_constraints,
                FloatingActionButtonKind::Extended => data.extended_size_constraints,
            }
            .unwrap_or(match kind {
                FloatingActionButtonKind::Regular => {
                    BoxConstraints::tight_for(crate::render::Size::new(
                        ResolvedFloatingActionButton::SIZE,
                        ResolvedFloatingActionButton::SIZE,
                    ))
                }
                FloatingActionButtonKind::Small => {
                    BoxConstraints::tight_for(crate::render::Size::new(
                        ResolvedFloatingActionButton::SMALL_SIZE,
                        ResolvedFloatingActionButton::SMALL_SIZE,
                    ))
                }
                FloatingActionButtonKind::Large => {
                    BoxConstraints::tight_for(crate::render::Size::new(
                        ResolvedFloatingActionButton::LARGE_SIZE,
                        ResolvedFloatingActionButton::LARGE_SIZE,
                    ))
                }
                // Height only: the width is whatever the label needs.
                FloatingActionButtonKind::Extended => BoxConstraints {
                    min_width: 0.0,
                    max_width: f32::INFINITY,
                    min_height: ResolvedFloatingActionButton::EXTENDED_HEIGHT,
                    max_height: ResolvedFloatingActionButton::EXTENDED_HEIGHT,
                },
            }),
            extended_icon_label_spacing: data
                .extended_icon_label_spacing
                .unwrap_or(ResolvedFloatingActionButton::EXTENDED_ICON_LABEL_SPACING),
            extended_padding: data.extended_padding.unwrap_or({
                let start = if has_icon { 16.0 } else { 20.0 };
                EdgeInsetsGeometry::Directional(crate::render::EdgeInsetsDirectional {
                    start,
                    top: 0.0,
                    end: 20.0,
                    bottom: 0.0,
                })
            }),
            extended_text_style: data
                .extended_text_style
                .clone()
                .or_else(|| theme.text_theme.label_large.clone()),
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
            text_style: lerp_text_style(&a.text_style, &b.text_style, t),
            constraints: BoxConstraints::lerp(a.constraints, b.constraints, t),
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
            border_radius: crate::borders::BorderRadius::lerp_optional(
                a.border_radius,
                b.border_radius,
                t,
            ),
        }
    }
}

/// Upstream `ToggleButtonsTheme`.
pub struct ToggleButtonsTheme;

impl ToggleButtonsTheme {
    pub fn new(data: ToggleButtonsThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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

impl TextCapitalization {
    /// Every value, so a test can walk the table rather than sample it.
    pub const ALL: [TextCapitalization; 4] = [
        TextCapitalization::Words,
        TextCapitalization::Sentences,
        TextCapitalization::Characters,
        TextCapitalization::None,
    ];

    /// Upstream's `toString()` on the enum, which is what the text input
    /// channel carries.
    ///
    /// A table **nothing on this side reads**: it goes out on
    /// `flutter/textinput` and the embedder looks it up. A row that took its
    /// neighbour's string would change what keyboard the reader gets and
    /// nothing here would notice, which is what `variant_sweep` found for two
    /// of these four.
    pub fn as_name(self) -> &'static str {
        match self {
            TextCapitalization::Words => "TextCapitalization.words",
            TextCapitalization::Sentences => "TextCapitalization.sentences",
            TextCapitalization::Characters => "TextCapitalization.characters",
            TextCapitalization::None => "TextCapitalization.none",
        }
    }
}

#[cfg(test)]
mod text_capitalization_tests {
    use super::TextCapitalization;

    #[test]
    fn every_capitalization_names_itself_the_way_dart_would() {
        // Upstream sends `textCapitalization.toString()`, and a Dart enum's is
        // `EnumName.valueName`. These are protocol, not ours to choose: an
        // embedder is already written against each one.
        assert_eq!(
            TextCapitalization::ALL.map(TextCapitalization::as_name),
            [
                "TextCapitalization.words",
                "TextCapitalization.sentences",
                "TextCapitalization.characters",
                "TextCapitalization.none",
            ]
        );
    }

    #[test]
    fn and_no_two_of_them_share_a_name() {
        // What makes a neighbour-swap detectable at all. Two rows with one
        // string is a keyboard setting the reader cannot reach.
        for (index, one) in TextCapitalization::ALL.iter().enumerate() {
            for other in TextCapitalization::ALL.iter().skip(index + 1) {
                assert_ne!(one.as_name(), other.as_name(), "{one:?} and {other:?}");
            }
        }
    }
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
            elevation: lerp_state_f32(a.elevation.as_ref(), b.elevation.as_ref(), t),
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
            side: lerp_state_side(a.side.as_ref(), b.side.as_ref(), t),
            shape: lerp_state_shape(a.shape.as_ref(), b.shape.as_ref(), t),
            padding: lerp_state_insets(a.padding.as_ref(), b.padding.as_ref(), t),
            text_style: lerp_state_text_style(a.text_style.as_ref(), b.text_style.as_ref(), t),
            hint_style: lerp_state_text_style(a.hint_style.as_ref(), b.hint_style.as_ref(), t),
            constraints: BoxConstraints::lerp(a.constraints, b.constraints, t),
            text_capitalization: lerp_nearer(&a.text_capitalization, &b.text_capitalization, t),
        }
    }
}

/// Upstream `SearchBarTheme`.
pub struct SearchBarTheme;

impl SearchBarTheme {
    pub fn new(data: SearchBarThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            header_height: lerp_f32(a.header_height, b.header_height, t),
            header_text_style: lerp_text_style(&a.header_text_style, &b.header_text_style, t),
            header_hint_style: lerp_text_style(&a.header_hint_style, &b.header_hint_style, t),
            constraints: BoxConstraints::lerp(a.constraints, b.constraints, t),
            padding: EdgeInsetsGeometry::lerp(a.padding, b.padding, t),
            bar_padding: EdgeInsetsGeometry::lerp(a.bar_padding, b.bar_padding, t),
            shrink_wrap: lerp_nearer(&a.shrink_wrap, &b.shrink_wrap, t),
            divider_color: lerp_color(a.divider_color, b.divider_color, t),
        }
    }
}

/// Upstream `SearchViewTheme`.
pub struct SearchViewTheme;

impl SearchViewTheme {
    pub fn new(data: SearchViewThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            day_period_shape: ShapeBorder::lerp(
                a.day_period_shape.clone(),
                b.day_period_shape.clone(),
                t,
            ),
            day_period_text_color: lerp_color(a.day_period_text_color, b.day_period_text_color, t),
            day_period_text_style: lerp_text_style(
                &a.day_period_text_style,
                &b.day_period_text_style,
                t,
            ),
            dial_background_color: lerp_color(a.dial_background_color, b.dial_background_color, t),
            dial_hand_color: lerp_color(a.dial_hand_color, b.dial_hand_color, t),
            dial_text_color: lerp_color(a.dial_text_color, b.dial_text_color, t),
            dial_text_style: lerp_text_style(&a.dial_text_style, &b.dial_text_style, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            entry_mode_icon_color: lerp_color(a.entry_mode_icon_color, b.entry_mode_icon_color, t),
            help_text_style: lerp_text_style(&a.help_text_style, &b.help_text_style, t),
            hour_minute_color: lerp_color(a.hour_minute_color, b.hour_minute_color, t),
            hour_minute_shape: ShapeBorder::lerp(
                a.hour_minute_shape.clone(),
                b.hour_minute_shape.clone(),
                t,
            ),
            hour_minute_text_color: lerp_color(
                a.hour_minute_text_color,
                b.hour_minute_text_color,
                t,
            ),
            hour_minute_text_style: lerp_text_style(
                &a.hour_minute_text_style,
                &b.hour_minute_text_style,
                t,
            ),
            padding: EdgeInsetsGeometry::lerp(a.padding, b.padding, t),
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            time_selector_separator_color: lerp_state_color(
                a.time_selector_separator_color.as_ref(),
                b.time_selector_separator_color.as_ref(),
                t,
            ),
            time_selector_separator_text_style: lerp_state_text_style(
                a.time_selector_separator_text_style.as_ref(),
                b.time_selector_separator_text_style.as_ref(),
                t,
            ),
        }
    }
}

/// Upstream `TimePickerTheme`.
pub struct TimePickerTheme;

impl TimePickerTheme {
    pub fn new(data: TimePickerThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
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
            header_headline_style: lerp_text_style(
                &a.header_headline_style,
                &b.header_headline_style,
                t,
            ),
            header_help_style: lerp_text_style(&a.header_help_style, &b.header_help_style, t),
            weekday_style: lerp_text_style(&a.weekday_style, &b.weekday_style, t),
            day_style: lerp_text_style(&a.day_style, &b.day_style, t),
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
            day_shape: lerp_state_shape(a.day_shape.as_ref(), b.day_shape.as_ref(), t),
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
            year_style: lerp_text_style(&a.year_style, &b.year_style, t),
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
            year_shape: lerp_state_shape(a.year_shape.as_ref(), b.year_shape.as_ref(), t),
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
            range_picker_shape: ShapeBorder::lerp(
                a.range_picker_shape.clone(),
                b.range_picker_shape.clone(),
                t,
            ),
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
            range_picker_header_headline_style: lerp_text_style(
                &a.range_picker_header_headline_style,
                &b.range_picker_header_headline_style,
                t,
            ),
            range_picker_header_help_style: lerp_text_style(
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
            toggle_button_text_style: lerp_text_style(
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
        provide_theme(data, child)
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
        provide_theme(data, child)
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
            // Upstream is `Shadow.lerpList`, which scales the excess items on
            // whichever side has more rather than stepping the whole list: a
            // second shadow that only one end has fades in.
            shadows: match (&a.shadows, &b.shadows) {
                (None, None) => None,
                (first, second) => Some(crate::painting::BoxShadow::lerp_list(
                    first.as_deref().unwrap_or(&[]),
                    second.as_deref().unwrap_or(&[]),
                    t,
                )),
            },
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
        provide_theme(data, child)
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
        provide_theme(data, child)
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
    /// The style of an entry by state.
    ///
    /// It does **not** supersede [`PopupMenuThemeData::text_style`], which is
    /// what this used to say. The two never meet: `useMaterial3` picks which
    /// of them is read and the other is not consulted at all. See
    /// [`ResolvedPopupMenu`].
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
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            menu_padding: EdgeInsetsGeometry::lerp(a.menu_padding, b.menu_padding, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shadow_color: lerp_color(a.shadow_color, b.shadow_color, t),
            surface_tint_color: lerp_color(a.surface_tint_color, b.surface_tint_color, t),
            text_style: lerp_text_style(&a.text_style, &b.text_style, t),
            label_text_style: lerp_state_text_style(
                a.label_text_style.as_ref(),
                b.label_text_style.as_ref(),
                t,
            ),
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
        provide_theme(data, child)
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
            text_style: lerp_text_style(&a.text_style, &b.text_style, t),
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
        provide_theme(data, child)
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
            padding: EdgeInsetsGeometry::lerp(a.padding, b.padding, t),
        }
    }
}

/// Upstream `BottomAppBarTheme`.
pub struct BottomAppBarTheme;

impl BottomAppBarTheme {
    pub fn new(data: BottomAppBarThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            indicator_shape: ShapeBorder::lerp(
                a.indicator_shape.clone(),
                b.indicator_shape.clone(),
                t,
            ),
            label_text_style: lerp_state_text_style(
                a.label_text_style.as_ref(),
                b.label_text_style.as_ref(),
                t,
            ),
            icon_theme: lerp_state_icon_theme(a.icon_theme.as_ref(), b.icon_theme.as_ref(), t),
            label_behavior: lerp_nearer(&a.label_behavior, &b.label_behavior, t),
            overlay_color: lerp_state_color(a.overlay_color.as_ref(), b.overlay_color.as_ref(), t),
            label_padding: EdgeInsetsGeometry::lerp(a.label_padding, b.label_padding, t),
        }
    }
}

/// Upstream `NavigationBarTheme`.
pub struct NavigationBarTheme;

impl NavigationBarTheme {
    pub fn new(data: NavigationBarThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            indicator_shape: ShapeBorder::lerp(
                a.indicator_shape.clone(),
                b.indicator_shape.clone(),
                t,
            ),
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
            label_text_style: lerp_state_text_style(
                a.label_text_style.as_ref(),
                b.label_text_style.as_ref(),
                t,
            ),
            icon_theme: lerp_state_icon_theme(a.icon_theme.as_ref(), b.icon_theme.as_ref(), t),
        }
    }
}

/// Upstream `NavigationDrawerTheme`.
pub struct NavigationDrawerTheme;

impl NavigationDrawerTheme {
    pub fn new(data: NavigationDrawerThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            padding: lerp_edge_insets(a.padding, b.padding, t),
            background_color: lerp_color(a.background_color, b.background_color, t),
            elevation: lerp_f32(a.elevation, b.elevation, t),
            shape: ShapeBorder::lerp(a.shape.clone(), b.shape.clone(), t),
            overlay_color: lerp_state_color(a.overlay_color.as_ref(), b.overlay_color.as_ref(), t),
        }
    }
}

/// Upstream `CarouselViewTheme`.
pub struct CarouselViewTheme;

impl CarouselViewTheme {
    pub fn new(data: CarouselViewThemeData, child: AnyWidget) -> AnyWidget {
        provide_theme(data, child)
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
            display_large: lerp_text_style(&a.display_large, &b.display_large, t),
            display_medium: lerp_text_style(&a.display_medium, &b.display_medium, t),
            display_small: lerp_text_style(&a.display_small, &b.display_small, t),
            headline_large: lerp_text_style(&a.headline_large, &b.headline_large, t),
            headline_medium: lerp_text_style(&a.headline_medium, &b.headline_medium, t),
            headline_small: lerp_text_style(&a.headline_small, &b.headline_small, t),
            title_large: lerp_text_style(&a.title_large, &b.title_large, t),
            title_medium: lerp_text_style(&a.title_medium, &b.title_medium, t),
            title_small: lerp_text_style(&a.title_small, &b.title_small, t),
            body_large: lerp_text_style(&a.body_large, &b.body_large, t),
            body_medium: lerp_text_style(&a.body_medium, &b.body_medium, t),
            body_small: lerp_text_style(&a.body_small, &b.body_small, t),
            label_large: lerp_text_style(&a.label_large, &b.label_large, t),
            label_medium: lerp_text_style(&a.label_medium, &b.label_medium, t),
            label_small: lerp_text_style(&a.label_small, &b.label_small, t),
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
/// Upstream `ScriptCategory`: which of a typography's three geometries a
/// locale reads with.
///
/// # It is a line-height requirement, not a list of languages
///
/// Upstream describes `dense` and `tall` with the same phrase -- both "require
/// extra line height to accommodate larger glyphs" -- and separates them only
/// by which languages they cover. What the category names is what the writing
/// needs, and the language list is how you find it.
///
/// # Which is why Vietnamese is `tall` and not `englishLike`
///
/// Upstream calls it out: Vietnamese "uses a localized form of the Latin
/// writing system", so an alphabet-based rule would file it with English --
/// but "its accented glyphs can be much taller than those found in Western
/// European languages", and the glyph heights are what the geometry is about.
/// **The category follows the ink, not the alphabet.**
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ScriptCategory {
    /// Latin, Greek and Cyrillic.
    #[default]
    EnglishLike,
    /// Chinese, Japanese, Korean.
    Dense,
    /// South and Southeast Asian and Middle-Eastern languages, and Vietnamese.
    Tall,
}

pub struct Typography;

impl Typography {
    /// Upstream's `geometryThemeFor`, which in Material 3 returns the same
    /// theme whichever way it goes.
    ///
    /// # The three geometries are one geometry now
    ///
    /// `englishLike2021`, `dense2021` and `tall2021` are the same fifteen
    /// styles with the same sizes and the same heights. So this switch has
    /// three arms and one answer, and a Material 3 application renders Thai
    /// and English through identical metrics.
    ///
    /// It was not always so. In `Typography.material2014` the three genuinely
    /// differ, and `ScriptCategory`'s documentation still describes that
    /// difference -- "font sizes for tall and dense scripts, for text styles
    /// that are smaller than the title style, are one unit larger".
    ///
    /// Two things about that sentence are worth pinning, because it is the
    /// documentation of a behaviour the default typography no longer has:
    ///
    /// * The **title styles move too**, so "smaller than the title style"
    ///   understates it -- `titleLarge` goes 20 to 21 along with everything
    ///   below it.
    /// * **`labelMedium` does not move**, alone among the eight styles at
    ///   title level and below, while `labelLarge` and `labelSmall` on either
    ///   side of it both do. It reads as an oversight in the 2014 table
    ///   rather than a decision, and it is pinned here as what upstream says
    ///   rather than what the rule would predict.
    ///
    /// # And this switch is worth having where `TextWidthBasis`'s was not
    ///
    /// A previous tick declined to add `TextWidthBasis` because this port's
    /// two measurements would compute the same number, making its arms
    /// indistinguishable. This one is also indistinguishable today, and the
    /// difference is where the sameness comes from: there it was **a
    /// limitation of the engine bridge**, and here it is **upstream's own
    /// design**, which `material2014` shows can be un-made. A switch that is
    /// currently trivial because the values happen to agree is still a switch;
    /// one that is trivial because we cannot see the values is a pretence.
    pub fn geometry_for(category: ScriptCategory) -> TextTheme {
        Typography::select(
            category,
            Typography::english_like(),
            Typography::dense(),
            Typography::tall(),
        )
    }

    /// The routing on its own, taking the three rather than reading them.
    ///
    /// Written this way because it cannot be shown to work otherwise: with the
    /// three constants identical, a mutation collapsing every arm onto
    /// `english_like` passes every test that can be written against
    /// [`Typography::geometry_for`]. The same remedy as
    /// [`CupertinoColors::elevating_is_one_step_down`] -- **a function over
    /// constants that happen to agree cannot be shown to tell them apart, so
    /// the values come in as arguments.**
    pub fn select(
        category: ScriptCategory,
        english_like: TextTheme,
        dense: TextTheme,
        tall: TextTheme,
    ) -> TextTheme {
        match category {
            ScriptCategory::EnglishLike => english_like,
            ScriptCategory::Dense => dense,
            ScriptCategory::Tall => tall,
        }
    }

    /// Whether the three geometries currently differ at all.
    ///
    /// False under Material 3, and the reason to have the question is that it
    /// was true under Material 2 and could be again.
    pub fn geometries_differ() -> bool {
        Typography::any_geometry_differs(
            &Typography::english_like(),
            &Typography::dense(),
            &Typography::tall(),
        )
    }

    /// The comparison on its own, for the reason [`Typography::select`] takes
    /// its themes: over three constants that agree, a version checking only
    /// one of the two pairs is indistinguishable from one checking both.
    pub fn any_geometry_differs(
        english_like: &TextTheme,
        dense: &TextTheme,
        tall: &TextTheme,
    ) -> bool {
        english_like != dense || english_like != tall
    }

    /// Upstream's colour half: `black` for a light theme, `white` for a dark
    /// one, merged with the geometry the locale chose.
    ///
    /// # A text theme is two themes, chosen by two different things
    ///
    /// Upstream: the theme "is created by merging a color text theme -- black
    /// for Brightness.light themes and white for Brightness.dark themes --
    /// and a geometry text theme, one of englishLike, dense, or tall,
    /// depending on the locale."
    ///
    /// **The brightness picks the ink and the language picks the metrics**,
    /// and neither knows about the other. A Japanese application in the dark
    /// takes one from each.
    pub fn for_theme(
        brightness: crate::platform::Brightness,
        category: ScriptCategory,
    ) -> TextTheme {
        let geometry = Typography::geometry_for(category);
        geometry.apply_color(match brightness {
            crate::platform::Brightness::Light => Color::BLACK,
            crate::platform::Brightness::Dark => Color::WHITE,
        })
    }

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
/// Upstream `ButtonBarThemeData`, which upstream marks
/// `@Deprecated("Use OverflowBar instead")` -- as it does `ButtonBar`, the only
/// widget that ever read it.
///
/// # Nothing reads it here, and nothing should
///
/// `tools/unwired.py` lists this as a theme with no reader. That is true and it
/// is not a gap: this port maps `ButtonBar` to
/// [`crate::overflow_bar::OverflowBar`], which is what upstream's deprecation
/// notice says to use, and `OverflowBar` has never consulted a theme. A reader
/// invented for this type would be a resolver for a widget that does not exist.
///
/// # What it used to decide, since the type is here to say so
///
/// `ButtonBar.build` took `ButtonTheme.of(context)` and called `copyWith` on it
/// with six fields, each `own argument ?? barTheme.field ?? constant`. The
/// parent theme was a **base to copy onto rather than a step in a chain**: the
/// six -- text theme, minimum width, height, padding, aligned dropdown, layout
/// behaviour -- were overwritten whatever it said, and what survived was
/// everything `ButtonBarThemeData` has no field for. A bar re-measured its
/// buttons and did not recolour them.
///
/// The spacing came out of one line, `paddingUnit = padding.horizontal / 4`.
/// Upstream's comment explains the four as "half of the average of the left and
/// right padding" -- `horizontal` is left plus right, so one division averages
/// and the other halves. The halving is what made the arithmetic close: each
/// child was wrapped in `symmetric(horizontal: unit)`, so between two
/// neighbours there were two halves and the gap was the whole 8; at the ends
/// the bar added its own `unit` to the `unit` the end child already carried,
/// making that 8 too; and the vertical was `2 * unit`, 8 again. One number in
/// four places, and the four in the divisor is what put it there.
///
/// The minimum width is the one number that is not `ButtonThemeData`'s own:
/// 64 against 88, because a button in a row is already read as part of a group
/// and does not have to hold its own width to be found.
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
            button_padding: EdgeInsetsGeometry::lerp(a.button_padding, b.button_padding, t),
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
        provide_theme(data, child)
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
    /// Upstream's `radius ?? dividerTheme.radius ?? defaults.radius`, and
    /// neither `_DividerDefaultsM2` nor `_DividerDefaultsM3` sets one -- so
    /// this is the theme's or nothing. It was missing here, which meant no
    /// divider could round its corners whatever the theme said.
    pub radius: Option<BorderRadiusGeometry>,
}

impl ResolvedDivider {
    pub fn of(context: &mut BuildContext) -> ResolvedDivider {
        let data = DividerTheme::of(context);
        let theme = ThemeData::of(context);
        ResolvedDivider {
            radius: data.radius,
            color: data.color.unwrap_or(theme.divider_color),
            space: data.space.unwrap_or(16.0),
            thickness: data.thickness.unwrap_or(0.0),
            indent: data.indent.unwrap_or(0.0),
            end_indent: data.end_indent.unwrap_or(0.0),
        }
    }

    /// The width of the line itself, in logical pixels, on a screen of
    /// `device_pixel_ratio`.
    ///
    /// # A thickness of zero is a hairline, and a hairline is one *device*
    /// pixel
    ///
    /// Upstream's default thickness is **0**, and it keeps it: neither
    /// `Divider.createBorderSide` nor either `build` clamps, so a
    /// `BorderSide` of width 0 reaches the painter, where it means the
    /// thinnest line the screen can draw -- one device pixel, which is
    /// `1 / devicePixelRatio` logical ones.
    ///
    /// **This used to answer `thickness.max(1.0)`**, under a doc claiming
    /// that was "`math.max(thickness, 0.0)` on a device pixel". There is no
    /// such expression upstream -- the doc cited something that does not
    /// exist, and the code did not even match the doc it cited. On a 3x
    /// screen the difference is a rule three times too heavy: a hairline
    /// there is 0.333 logical pixels, not 1.
    ///
    /// A **positive** thickness is taken as it is; only zero means "as thin
    /// as this screen can manage". A ratio of zero or less answers a single
    /// logical pixel rather than dividing by it.
    pub fn line_thickness_for(&self, device_pixel_ratio: f32) -> f32 {
        if self.thickness > 0.0 {
            return self.thickness;
        }
        if device_pixel_ratio > 0.0 {
            1.0 / device_pixel_ratio
        } else {
            1.0
        }
    }

    /// [`ResolvedDivider::line_thickness_for`] on a screen with square
    /// pixels, for the callers that have no view to ask.
    pub fn line_thickness(&self) -> f32 {
        self.line_thickness_for(1.0)
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
    use crate::framework::{Component, ElementTree, component, leaf, provide};
    use crate::theme::MaterialTheme;
    use crate::widgets::SizedBox;
    use std::cell::RefCell;
    use std::rc::Rc;

    // -- The one member nothing answered, tick 271 ---------------------------

    #[test]
    fn a_button_may_say_it_is_dark_when_the_page_is_not() {
        // Upstream's `getBrightness`, the one member of `MaterialButton` with
        // no reader anywhere in this crate. `text_color` already took a
        // brightness, so what was missing is the override that feeds it.
        use crate::platform::Brightness;
        let light = ThemeData::light().color_scheme;
        assert_eq!(
            MaterialButtonColors::brightness(None, &light),
            Brightness::Light,
            "with nothing said, the page decides"
        );
        assert_eq!(
            MaterialButtonColors::brightness(Some(Brightness::Dark), &light),
            Brightness::Dark,
            "and the button may disagree with it"
        );
        // Both directions, so this is an override and not a one-way flag.
        let dark = ThemeData::dark().color_scheme;
        assert_eq!(
            MaterialButtonColors::brightness(Some(Brightness::Light), &dark),
            Brightness::Light
        );
    }

    #[test]
    fn the_override_reaches_the_normal_text_theme_and_not_the_other_two() {
        // What it costs to be missing, said as the difference it makes. A
        // dark button on a light page gets a white label -- and did not,
        // because `Normal` reads the page's brightness.
        use crate::platform::Brightness;
        let scheme = ThemeData::light().color_scheme;
        let label = |brightness, theme| {
            MaterialButtonColors::text_color(true, None, None, theme, None, brightness, &scheme)
        };

        assert_eq!(
            label(
                MaterialButtonColors::brightness(Some(Brightness::Dark), &scheme),
                ButtonTextTheme::Normal
            ),
            Color::WHITE
        );
        assert_eq!(
            label(
                MaterialButtonColors::brightness(None, &scheme),
                ButtonTextTheme::Normal
            ),
            MaterialButtonColors::BLACK87,
            "which is what a dark button used to get"
        );

        // `Accent` never asks, so the override cannot move it.
        assert_eq!(
            label(Brightness::Dark, ButtonTextTheme::Accent),
            label(Brightness::Light, ButtonTextTheme::Accent)
        );

        // `Primary` asks the *fill* first, so a coloured button was already
        // right and only a fill-less one consults the brightness.
        let filled = |brightness| {
            MaterialButtonColors::text_color(
                true,
                None,
                None,
                ButtonTextTheme::Primary,
                Some(Color::BLACK),
                brightness,
                &scheme,
            )
        };
        assert_eq!(filled(Brightness::Light), filled(Brightness::Dark));
        assert_eq!(filled(Brightness::Light), Color::WHITE, "a dark fill");
        assert_ne!(
            label(Brightness::Dark, ButtonTextTheme::Primary),
            label(Brightness::Light, ButtonTextTheme::Primary),
            "and with no fill it is the brightness after all"
        );
    }

    // -- The AM/PM toggle and the dial, tick 257 -----------------------------
    //
    // Nine fields, none with a resolver.

    fn day_period(
        data: TimePickerThemeData,
        theme: ThemeData,
        states: WidgetStates,
    ) -> ResolvedDayPeriod {
        struct Reader {
            seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedDayPeriod>>>,
            states: WidgetStates,
        }
        impl crate::framework::Component for Reader {
            fn build(&self, context: &mut BuildContext) -> crate::framework::AnyWidget {
                *self.seen.borrow_mut() = Some(ResolvedDayPeriod::of(context, self.states));
                crate::framework::leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            TimePickerTheme::new(
                data,
                crate::framework::component(Reader {
                    seen: std::rc::Rc::clone(&seen),
                    states,
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn dial(data: TimePickerThemeData, theme: ThemeData, states: WidgetStates) -> ResolvedDial {
        struct Reader {
            seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedDial>>>,
            states: WidgetStates,
        }
        impl crate::framework::Component for Reader {
            fn build(&self, context: &mut BuildContext) -> crate::framework::AnyWidget {
                *self.seen.borrow_mut() = Some(ResolvedDial::of(context, self.states));
                crate::framework::leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            TimePickerTheme::new(
                data,
                crate::framework::component(Reader {
                    seen: std::rc::Rc::clone(&seen),
                    states,
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn time_picker_under(data: TimePickerThemeData, theme: ThemeData) -> ResolvedTimePicker {
        struct Reader {
            seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedTimePicker>>>,
        }
        impl crate::framework::Component for Reader {
            fn build(&self, context: &mut BuildContext) -> crate::framework::AnyWidget {
                *self.seen.borrow_mut() = Some(ResolvedTimePicker::of(
                    context,
                    crate::pickers::TimePickerEntryMode::Dial,
                    false,
                ));
                crate::framework::leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            TimePickerTheme::new(
                data,
                crate::framework::component(Reader {
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn material_two_light() -> ThemeData {
        let mut theme = ThemeData::light();
        theme.use_material3 = false;
        theme
    }

    #[test]
    fn a_theme_that_names_the_toggles_and_the_dials_colours_wins() {
        // "The theme wins" and "the table answers" are two different rules,
        // and the second tick running I had only tested the second. Every
        // field on both resolvers is asked for here.
        const A: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
        const B: Color = Color::argb(0xFF, 0x44, 0x55, 0x66);
        const C: Color = Color::argb(0xFF, 0x77, 0x88, 0x99);
        let named = TimePickerThemeData {
            day_period_color: Some(A),
            day_period_text_color: Some(B),
            day_period_text_style: Some(TextStyle {
                font_size: 41.0,
                ..TextStyle::default()
            }),
            dial_background_color: Some(A),
            dial_hand_color: Some(B),
            dial_text_color: Some(C),
            dial_text_style: Some(TextStyle {
                font_size: 42.0,
                ..TextStyle::default()
            }),
            help_text_style: Some(TextStyle {
                font_size: 43.0,
                ..TextStyle::default()
            }),
            ..TimePickerThemeData::new()
        };
        // Unselected, where the tables answer transparent -- so a named
        // colour is the only way this can come back opaque.
        let toggle = day_period(named.clone(), ThemeData::light(), WidgetStates::NONE);
        assert_eq!(toggle.background, A);
        assert_eq!(toggle.foreground, B);
        assert_eq!(toggle.style.font_size, 41.0);

        let face = dial(named.clone(), ThemeData::light(), WidgetStates::NONE);
        assert_eq!(face.background, A);
        assert_eq!(face.hand, B);
        assert_eq!(face.text_color, C);
        assert_eq!(face.text_style.font_size, 42.0);
        assert_eq!(
            face.text_style.color, C,
            "and a named style still takes the resolved ink"
        );

        assert_eq!(
            time_picker_under(named, ThemeData::light())
                .help_text_style
                .map(|style| style.font_size),
            Some(43.0)
        );
    }

    #[test]
    fn the_unselected_half_of_the_toggle_is_transparent_in_both_tables() {
        // Upstream repeats the reason in a comment in each table: the
        // unselected half should match the dialog behind it, and transparency
        // does that "without being redundant and allows the optional
        // elevation overlay for dark mode to be visible". A colour copied
        // from the dialog would be a second place to change it, and would sit
        // *over* the elevation overlay instead of under it.
        for theme in [ThemeData::light(), material_two_light(), ThemeData::dark()] {
            assert_eq!(
                day_period(
                    TimePickerThemeData::new(),
                    theme.clone(),
                    WidgetStates::NONE
                )
                .background,
                Color::TRANSPARENT
            );
        }

        // The selected half is not, in either.
        let selected = WidgetStates::NONE.with(WidgetState::Selected);
        assert_eq!(
            day_period(TimePickerThemeData::new(), ThemeData::light(), selected).background,
            ThemeData::light().color_scheme.tertiary_container()
        );
        assert_ne!(
            day_period(TimePickerThemeData::new(), material_two_light(), selected).background,
            Color::TRANSPARENT
        );
    }

    #[test]
    fn the_toggle_takes_its_outline_and_its_rounding_from_two_separate_fields() {
        // Upstream's `(theme.dayPeriodShape ?? defaults).copyWith(side:
        // resolvedSide)`. A theme may name one without the other, and
        // whichever shape wins takes whichever side wins.
        const MINE: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
        let named_side = day_period(
            TimePickerThemeData {
                day_period_border_side: Some(BorderSide {
                    color: MINE,
                    width: 5.0,
                    ..BorderSide::NONE
                }),
                ..TimePickerThemeData::new()
            },
            ThemeData::light(),
            WidgetStates::NONE,
        );
        assert_eq!(named_side.side.color, MINE);
        // And the side reached the shape, not only the field beside it.
        match &named_side.shape {
            ShapeBorder::Rounded(rounded) => {
                assert_eq!(rounded.side.color, MINE);
                assert_eq!(rounded.side.width, 5.0);
            }
            other => panic!("expected a rounded rectangle, got {other:?}"),
        }
    }

    #[test]
    fn the_toggles_outline_is_blended_onto_the_surface_under_material_two() {
        // Not left translucent: the toggle sits on the dialog, and a
        // see-through outline would pick up whatever the elevation overlay
        // put behind it. Material 3 names `outline` outright and needs no
        // blend.
        let two = day_period(
            TimePickerThemeData::new(),
            material_two_light(),
            WidgetStates::NONE,
        );
        assert_eq!(two.side.color.alpha(), 0xFF, "opaque, not 38% of something");
        assert_ne!(two.side.color, material_two_light().color_scheme.on_surface);

        let three = day_period(
            TimePickerThemeData::new(),
            ThemeData::light(),
            WidgetStates::NONE,
        );
        assert_eq!(three.side.color, ThemeData::light().color_scheme.outline());
        assert_ne!(three.side.color, two.side.color);
    }

    #[test]
    fn the_toggles_words_are_title_medium_carrying_the_resolved_ink() {
        // The same construction in both tables, and the two colours differ.
        let theme = ThemeData::light();
        let selected = WidgetStates::NONE.with(WidgetState::Selected);
        let chosen = day_period(TimePickerThemeData::new(), theme.clone(), selected);
        let quiet = day_period(
            TimePickerThemeData::new(),
            theme.clone(),
            WidgetStates::NONE,
        );

        assert_eq!(
            chosen.style.font_size,
            theme.text_theme.title_medium.as_ref().unwrap().font_size
        );
        assert_eq!(chosen.style.color, chosen.foreground);
        assert_eq!(quiet.style.color, quiet.foreground);
        assert_eq!(
            chosen.foreground,
            theme.color_scheme.on_tertiary_container()
        );
        assert_eq!(quiet.foreground, theme.color_scheme.on_surface_variant());

        // Material 2 fades the unselected half's words instead of giving them
        // a role of their own.
        let old = day_period(
            TimePickerThemeData::new(),
            material_two_light(),
            WidgetStates::NONE,
        );
        assert!(old.foreground.alpha() < 0xFF, "sixty percent");
        assert_eq!(
            day_period(TimePickerThemeData::new(), material_two_light(), selected).foreground,
            material_two_light().color_scheme.primary
        );
    }

    #[test]
    fn the_hand_and_the_dials_type_scale_are_the_same_in_both_tables() {
        // Worth asserting rather than leaving to look like an oversight: most
        // of this table branches, and these two do not.
        let theme = ThemeData::light();
        let three = dial(
            TimePickerThemeData::new(),
            theme.clone(),
            WidgetStates::NONE,
        );
        let two = dial(
            TimePickerThemeData::new(),
            material_two_light(),
            WidgetStates::NONE,
        );
        assert_eq!(three.hand, theme.color_scheme.primary);
        assert_eq!(two.hand, three.hand);
        assert_eq!(
            three.text_style.font_size,
            theme.text_theme.body_large.as_ref().unwrap().font_size
        );
        assert_eq!(two.text_style.font_size, three.text_style.font_size);
    }

    #[test]
    fn a_chosen_hour_on_the_dial_is_written_in_the_ink_for_the_hand() {
        // The hand is `primary`, so the number on it has to be the ink for
        // primary. Material 3 says `onPrimary`; Material 2 says `surface`,
        // which is the same idea from before it had the habit.
        let theme = ThemeData::light();
        let selected = WidgetStates::NONE.with(WidgetState::Selected);
        assert_eq!(
            dial(TimePickerThemeData::new(), theme.clone(), selected).text_color,
            theme.color_scheme.on_primary
        );
        assert_eq!(
            dial(TimePickerThemeData::new(), material_two_light(), selected).text_color,
            material_two_light().color_scheme.surface
        );

        // Unselected agrees: an hour off the hand is ordinary ink.
        for theme in [ThemeData::light(), material_two_light()] {
            let scheme = theme.color_scheme;
            assert_eq!(
                dial(TimePickerThemeData::new(), theme, WidgetStates::NONE).text_color,
                scheme.on_surface
            );
        }
    }

    #[test]
    fn the_dial_face_is_a_surface_role_under_three_and_a_tint_under_two() {
        let theme = ThemeData::light();
        assert_eq!(
            dial(
                TimePickerThemeData::new(),
                theme.clone(),
                WidgetStates::NONE
            )
            .background,
            theme.color_scheme.surface_container_highest()
        );

        // And Material 2's tint is heavier in the dark, like the rest of that
        // table.
        let mut dark = ThemeData::dark();
        dark.use_material3 = false;
        let by_day = dial(
            TimePickerThemeData::new(),
            material_two_light(),
            WidgetStates::NONE,
        );
        let by_night = dial(TimePickerThemeData::new(), dark, WidgetStates::NONE);
        assert_eq!(by_day.background.alpha(), 20, "8% of 255");
        assert_eq!(by_night.background.alpha(), 31, "12%");
    }

    #[test]
    fn the_help_line_is_recoloured_under_three_and_left_alone_under_two() {
        // Material 3 puts `onSurfaceVariant` on `labelMedium`; Material 2's
        // `labelSmall` is flat, so it keeps whatever ink the scale carries.
        let theme = ThemeData::light();
        let three = time_picker_under(TimePickerThemeData::new(), theme.clone());
        assert_eq!(
            three.help_text_style.as_ref().unwrap().color,
            theme.color_scheme.on_surface_variant()
        );
        assert_eq!(
            three.help_text_style.as_ref().map(|s| s.font_size),
            theme.text_theme.label_medium.as_ref().map(|s| s.font_size)
        );

        let two = time_picker_under(TimePickerThemeData::new(), material_two_light());
        assert_eq!(
            two.help_text_style.as_ref().map(|s| s.font_size),
            material_two_light()
                .text_theme
                .label_small
                .as_ref()
                .map(|s| s.font_size)
        );
        assert_eq!(
            two.help_text_style.as_ref().unwrap().color,
            material_two_light()
                .text_theme
                .label_small
                .as_ref()
                .unwrap()
                .color,
            "flat: the scale's own ink"
        );
    }

    #[test]
    fn a_light_material_two_picker_fades_its_entry_mode_icon() {
        // This was resolved as a flat `onSurface`, which is Material 3's
        // answer. Material 2 fades it to sixty percent in the light and
        // leaves it at full strength in the dark.
        let mut dark = ThemeData::dark();
        dark.use_material3 = false;
        let by_day = time_picker_under(TimePickerThemeData::new(), material_two_light());
        let by_night = time_picker_under(TimePickerThemeData::new(), dark.clone());
        assert!(by_day.entry_mode_icon_color.alpha() < 0xFF);
        assert_eq!(by_night.entry_mode_icon_color, dark.color_scheme.on_surface);

        // Material 3 does not fade it at either brightness.
        for theme in [ThemeData::light(), ThemeData::dark()] {
            let scheme = theme.color_scheme;
            assert_eq!(
                time_picker_under(TimePickerThemeData::new(), theme).entry_mode_icon_color,
                scheme.on_surface
            );
        }
    }

    // -- The big number at the top of a time picker, tick 256 ----------------
    //
    // Six fields, none with a resolver.

    fn hour_minute(
        data: TimePickerThemeData,
        theme: ThemeData,
        entry_mode: crate::pickers::TimePickerEntryMode,
        states: WidgetStates,
    ) -> ResolvedHourMinute {
        struct Reader {
            seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedHourMinute>>>,
            entry_mode: crate::pickers::TimePickerEntryMode,
            states: WidgetStates,
        }
        impl crate::framework::Component for Reader {
            fn build(&self, context: &mut BuildContext) -> crate::framework::AnyWidget {
                *self.seen.borrow_mut() = Some(ResolvedHourMinute::of(
                    context,
                    self.entry_mode,
                    self.states,
                ));
                crate::framework::leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            TimePickerTheme::new(
                data,
                crate::framework::component(Reader {
                    seen: std::rc::Rc::clone(&seen),
                    entry_mode,
                    states,
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn dialled(states: WidgetStates) -> ResolvedHourMinute {
        hour_minute(
            TimePickerThemeData::new(),
            ThemeData::light(),
            crate::pickers::TimePickerEntryMode::Dial,
            states,
        )
    }

    #[test]
    fn pressing_an_hour_box_turns_it_into_its_own_ink() {
        // Material 3's `hourMinuteColor` *blends* rather than picks: the
        // state overlay goes over the container colour instead of replacing
        // it. And the pressed overlay is the ink at full opacity, so the box
        // goes from `primaryContainer` to `onPrimaryContainer` outright --
        // the strongest state change in any of these tables.
        let scheme = ThemeData::light().color_scheme;
        let selected = WidgetStates::NONE.with(WidgetState::Selected);
        assert_eq!(dialled(selected).background, scheme.primary_container());
        assert_eq!(
            dialled(selected.with(WidgetState::Pressed)).background,
            scheme.on_primary_container()
        );

        // Hovered and focused are ordinary blends over the same base, so they
        // land between the two.
        let hovered = dialled(selected.with(WidgetState::Hovered)).background;
        assert_ne!(hovered, scheme.primary_container());
        assert_ne!(hovered, scheme.on_primary_container());
        let focused = dialled(selected.with(WidgetState::Focused)).background;
        assert_ne!(hovered, focused, "0.08 against 0.1");

        // And what "blend" buys is that the answer is **opaque** in every
        // state: the box has a solid fill. Handing the overlay back unblended
        // would leave a translucent one, and the dialog behind it would show
        // through the hour you are choosing.
        for states in [
            WidgetStates::NONE,
            selected,
            selected.with(WidgetState::Pressed),
            selected.with(WidgetState::Hovered),
            selected.with(WidgetState::Focused),
            WidgetStates::NONE.with(WidgetState::Hovered),
        ] {
            assert_eq!(
                dialled(states).background.alpha(),
                0xFF,
                "{states:?} left the box see-through"
            );
        }
    }

    #[test]
    fn a_time_picker_writes_the_hour_smaller_once_you_are_typing_it() {
        // The one style in the framework that branches on the entry mode. On
        // the dial the number is the screen's whole subject; in a text field
        // it has to leave room for a border and a label.
        use crate::pickers::TimePickerEntryMode;
        let theme = ThemeData::light();
        let big = hour_minute(
            TimePickerThemeData::new(),
            theme.clone(),
            TimePickerEntryMode::Dial,
            WidgetStates::NONE,
        );
        let typed = hour_minute(
            TimePickerThemeData::new(),
            theme.clone(),
            TimePickerEntryMode::Input,
            WidgetStates::NONE,
        );
        assert_eq!(
            big.style.font_size,
            theme.text_theme.display_large.as_ref().unwrap().font_size
        );
        assert_eq!(
            typed.style.font_size,
            theme.text_theme.display_medium.as_ref().unwrap().font_size
        );
        assert!(big.style.font_size > typed.style.font_size);

        // `DialOnly` is a dial, and `InputOnly` is a field: the "only"
        // variants are the same two modes without the button to swap.
        assert_eq!(
            hour_minute(
                TimePickerThemeData::new(),
                theme.clone(),
                TimePickerEntryMode::DialOnly,
                WidgetStates::NONE
            )
            .style
            .font_size,
            big.style.font_size
        );

        // Material 2 uses `displayMedium` for both, so the branch is a
        // Material 3 distinction.
        let mut two = theme.clone();
        two.use_material3 = false;
        assert_eq!(
            hour_minute(
                TimePickerThemeData::new(),
                two.clone(),
                TimePickerEntryMode::Dial,
                WidgetStates::NONE
            )
            .style
            .font_size,
            hour_minute(
                TimePickerThemeData::new(),
                two,
                TimePickerEntryMode::Input,
                WidgetStates::NONE
            )
            .style
            .font_size
        );
    }

    #[test]
    fn the_colon_is_a_material_three_idea() {
        // Neither separator field exists in `_TimePickerDefaultsM2`. A
        // Material 2 picker draws its colon in the hour/minute style, like
        // the digits beside it; Material 3 gives the colon a colour and a
        // style of its own so it can be quieter than the numbers it sits
        // between.
        let scheme = ThemeData::light().color_scheme;
        let three = dialled(WidgetStates::NONE);
        assert_eq!(three.separator_color, Some(scheme.on_surface));
        assert_eq!(
            three.separator_style.as_ref().map(|s| s.font_size),
            ThemeData::light()
                .text_theme
                .display_large
                .as_ref()
                .map(|s| s.font_size)
        );

        let mut two = ThemeData::light();
        two.use_material3 = false;
        let old = hour_minute(
            TimePickerThemeData::new(),
            two,
            crate::pickers::TimePickerEntryMode::Dial,
            WidgetStates::NONE,
        );
        assert_eq!(old.separator_color, None);
        assert_eq!(old.separator_style, None);
    }

    #[test]
    fn a_dark_material_two_theme_tints_a_selected_box_twice_as_hard() {
        // The same twelve percent of `primary` over a dark surface would not
        // be seen. One of the last places a colour comes from the brightness
        // rather than from a scheme role.
        let mut light = ThemeData::light();
        light.use_material3 = false;
        let mut dark = ThemeData::dark();
        dark.use_material3 = false;
        let selected = WidgetStates::NONE.with(WidgetState::Selected);
        let by_day = hour_minute(
            TimePickerThemeData::new(),
            light,
            crate::pickers::TimePickerEntryMode::Dial,
            selected,
        );
        let by_night = hour_minute(
            TimePickerThemeData::new(),
            dark,
            crate::pickers::TimePickerEntryMode::Dial,
            selected,
        );
        assert_eq!(by_day.background.alpha(), 31, "12% of 255");
        assert_eq!(by_night.background.alpha(), 61, "24%");
    }

    #[test]
    fn the_shape_is_a_rung_rounder_under_material_three() {
        // Upstream's field is a whole `ShapeBorder`, not a radius, so a theme
        // may make the box a stadium and not only round its corners
        // differently. Resolving a radius alone would have quietly refused
        // that.
        let stadium = ShapeBorder::Stadium(crate::borders::StadiumBorder::default());
        assert!(matches!(
            hour_minute(
                TimePickerThemeData {
                    hour_minute_shape: Some(stadium.clone()),
                    ..TimePickerThemeData::new()
                },
                ThemeData::light(),
                crate::pickers::TimePickerEntryMode::Dial,
                WidgetStates::NONE
            )
            .shape,
            ShapeBorder::Stadium(_)
        ));
        assert!(matches!(
            dialled(WidgetStates::NONE).shape,
            ShapeBorder::Rounded(_)
        ));

        assert_eq!(dialled(WidgetStates::NONE).shape_radius, 8.0);
        let mut two = ThemeData::light();
        two.use_material3 = false;
        assert_eq!(
            hour_minute(
                TimePickerThemeData::new(),
                two,
                crate::pickers::TimePickerEntryMode::Dial,
                WidgetStates::NONE
            )
            .shape_radius,
            4.0
        );
    }

    #[test]
    fn the_digits_ink_turns_over_with_the_selection_and_the_style_carries_it() {
        // Upstream's Material 3 ladder writes the same answer in all four
        // arms of each branch, so what it comes to is two colours. The style
        // is that colour put on the role -- the digits are drawn from the
        // style, not from the colour beside it.
        let scheme = ThemeData::light().color_scheme;
        let selected = dialled(WidgetStates::NONE.with(WidgetState::Selected));
        let quiet = dialled(WidgetStates::NONE);
        assert_eq!(selected.foreground, scheme.on_primary_container());
        assert_eq!(quiet.foreground, scheme.on_surface);
        assert_eq!(selected.style.color, selected.foreground);
        assert_eq!(quiet.style.color, quiet.foreground);

        // And the four state arms really do agree, which is worth saying
        // because upstream writes them out separately.
        for state in [
            WidgetState::Pressed,
            WidgetState::Hovered,
            WidgetState::Focused,
        ] {
            assert_eq!(
                dialled(WidgetStates::of(&[WidgetState::Selected, state])).foreground,
                selected.foreground,
                "{state:?}"
            );
        }
    }

    #[test]
    fn a_theme_that_names_the_boxs_colours_is_taken_at_its_word() {
        const FILL: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
        const INK: Color = Color::argb(0xFF, 0x44, 0x55, 0x66);
        let named = hour_minute(
            TimePickerThemeData {
                hour_minute_color: Some(FILL),
                hour_minute_text_color: Some(INK),
                ..TimePickerThemeData::new()
            },
            ThemeData::light(),
            crate::pickers::TimePickerEntryMode::Dial,
            WidgetStates::NONE.with(WidgetState::Pressed),
        );
        assert_eq!(named.background, FILL, "even pressed");
        assert_eq!(named.foreground, INK);
        assert_eq!(named.style.color, INK, "and the style follows the colour");
    }

    // -- What a calendar's cells are drawn with, tick 254 --------------------
    //
    // Three slots and eight properties, none of which had a resolver: a date
    // in this port had no selected colour, no disabled colour and no ripple.

    fn cell(
        data: DatePickerThemeData,
        theme: ThemeData,
        slot: DateCellSlot,
        states: WidgetStates,
    ) -> ResolvedDateCell {
        struct Reader {
            seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedDateCell>>>,
            slot: DateCellSlot,
            states: WidgetStates,
        }
        impl crate::framework::Component for Reader {
            fn build(&self, context: &mut BuildContext) -> crate::framework::AnyWidget {
                *self.seen.borrow_mut() =
                    Some(ResolvedDateCell::of(context, self.slot, self.states));
                crate::framework::leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            DatePickerTheme::new(
                data,
                crate::framework::component(Reader {
                    seen: std::rc::Rc::clone(&seen),
                    slot,
                    states,
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn plain_cell(slot: DateCellSlot, states: WidgetStates) -> ResolvedDateCell {
        cell(DatePickerThemeData::new(), ThemeData::light(), slot, states)
    }

    // -- The cell shapes and the range picker, tick 255 ----------------------
    //
    // The last ten fields on `DatePickerThemeData`. The three shapes are set
    // in the *constructors* of both defaults classes rather than overridden
    // as getters, which is why grepping upstream for `get dayShape` finds
    // nothing at all.

    #[test]
    fn a_date_is_a_circle_and_a_year_is_a_pill() {
        // Both tables pass the same pair. Today has no shape of its own:
        // upstream draws it by putting a *side* on the day shape, which is
        // why a custom `dayShape` carries today's ring with it.
        assert!(matches!(
            plain_cell(DateCellSlot::Day, WidgetStates::NONE).shape,
            ShapeBorder::Circle(_)
        ));
        assert_eq!(
            plain_cell(DateCellSlot::Today, WidgetStates::NONE).shape,
            plain_cell(DateCellSlot::Day, WidgetStates::NONE).shape
        );
        assert!(matches!(
            plain_cell(DateCellSlot::Year, WidgetStates::NONE).shape,
            ShapeBorder::Stadium(_)
        ));

        // A named day shape reaches today as well, and not the year.
        let named = DatePickerThemeData {
            day_shape: Some(StateProperty::all(Some(ShapeBorder::Stadium(
                crate::borders::StadiumBorder::default(),
            )))),
            ..DatePickerThemeData::new()
        };
        for slot in [DateCellSlot::Day, DateCellSlot::Today] {
            assert!(
                matches!(
                    cell(named.clone(), ThemeData::light(), slot, WidgetStates::NONE).shape,
                    ShapeBorder::Stadium(_)
                ),
                "{slot:?}"
            );
        }
        assert!(matches!(
            cell(
                named,
                ThemeData::light(),
                DateCellSlot::Year,
                WidgetStates::NONE
            )
            .shape,
            ShapeBorder::Stadium(_)
        ));
    }

    #[test]
    fn a_range_picker_has_no_rounding_where_the_dialog_has_twenty_eight() {
        // A range picker is the whole screen, and a screen has no corners to
        // round.
        let resolved = date_picker_under(DatePickerThemeData::new(), ThemeData::light());
        assert_eq!(resolved.shape_radius, 28.0);
        assert_eq!(resolved.range_picker_shape_radius, 0.0);
        assert!(matches!(
            resolved.range_picker_shape,
            ShapeBorder::Rounded(_)
        ));
    }

    #[test]
    fn a_material_two_range_pickers_header_is_chosen_by_brightness() {
        // The last place in the date picker where a colour comes from the
        // brightness rather than from a scheme role. A dark theme's header is
        // `surface` and a light one's is `primary`, and the foreground
        // follows so the words stay legible on whichever it landed on.
        let light = ThemeData::light();
        let mut two_light = light.clone();
        two_light.use_material3 = false;
        let mut two_dark = ThemeData::dark();
        two_dark.use_material3 = false;

        let day = date_picker_under(DatePickerThemeData::new(), two_light.clone());
        assert_eq!(
            day.range_picker_header_background_color,
            two_light.color_scheme.primary
        );
        assert_eq!(
            day.range_picker_header_foreground_color,
            two_light.color_scheme.on_primary
        );

        let night = date_picker_under(DatePickerThemeData::new(), two_dark.clone());
        assert_eq!(
            night.range_picker_header_background_color,
            two_dark.color_scheme.surface
        );
        assert_eq!(
            night.range_picker_header_foreground_color,
            two_dark.color_scheme.on_surface
        );
        assert_ne!(
            night.range_picker_header_background_color, two_dark.color_scheme.primary,
            "which is what a role-based rule would have given it"
        );

        // Material 3 makes the header transparent whatever the brightness --
        // the same move it makes for the ordinary header -- and lets the
        // dialog behind it show.
        for theme in [ThemeData::light(), ThemeData::dark()] {
            let resolved = date_picker_under(DatePickerThemeData::new(), theme);
            assert_eq!(
                resolved.range_picker_header_background_color,
                Color::TRANSPARENT
            );
        }
    }

    #[test]
    fn a_material_three_range_picker_has_no_background_of_its_own() {
        // Not overridden in that table, and that is the answer: a Material 3
        // range picker fills the screen and takes the dialog's own
        // background. Material 2's is a card and needs one.
        assert_eq!(
            date_picker_under(DatePickerThemeData::new(), ThemeData::light())
                .range_picker_background_color,
            None
        );

        let mut two = ThemeData::light();
        two.use_material3 = false;
        assert_eq!(
            date_picker_under(DatePickerThemeData::new(), two.clone())
                .range_picker_background_color,
            Some(two.color_scheme.surface)
        );
    }

    #[test]
    fn the_range_strip_stops_being_a_faded_selection_and_becomes_a_surface() {
        // Material 2 tints `primary` to twelve percent; Material 3 gives the
        // strip a container role of its own, at full strength.
        let three = ThemeData::light();
        let mut two = three.clone();
        two.use_material3 = false;

        let new = date_picker_under(DatePickerThemeData::new(), three.clone());
        assert_eq!(
            new.range_selection_background_color,
            three.color_scheme.secondary_container()
        );
        assert_eq!(
            new.range_selection_background_color.alpha(),
            0xFF,
            "a surface, not a tint"
        );

        let old = date_picker_under(DatePickerThemeData::new(), two.clone());
        assert_ne!(
            old.range_selection_background_color,
            new.range_selection_background_color
        );
        assert!(
            old.range_selection_background_color.alpha() < 0x40,
            "a twelve percent tint"
        );
    }

    #[test]
    fn a_range_ripples_all_one_way_under_material_three() {
        // Inside a range every cell *is* selected, so a branch on selection
        // says nothing -- and Material 3's `rangeSelectionOverlayColor` has
        // no selected branch at all. That is the whole content of the field.
        // Material 2 kept the two branches its ordinary day overlay has,
        // including the heavy 0.38 for a pressed selected cell.
        let three = ThemeData::light();
        let mut two = three.clone();
        two.use_material3 = false;
        let resolved = date_picker_under(DatePickerThemeData::new(), three.clone());
        let scheme = three.color_scheme;

        let pressed = WidgetStates::NONE.with(WidgetState::Pressed);
        let pressed_selected = WidgetStates::of(&[WidgetState::Pressed, WidgetState::Selected]);
        assert_eq!(
            resolved.range_selection_overlay(&scheme, true, pressed),
            resolved.range_selection_overlay(&scheme, true, pressed_selected),
            "selected changes nothing"
        );
        // And it is `onPrimaryContainer`, the ink for the container the strip
        // is filled with -- not `onPrimary`, which is what a date outside the
        // strip would use.
        assert_eq!(
            resolved
                .range_selection_overlay(&scheme, true, pressed)
                .unwrap(),
            crate::elevation_overlay::with_opacity(
                scheme.on_primary_container(),
                ResolvedDateCell::M3_STRONG_OVERLAY
            )
        );

        // Material 2 does branch, and heavily.
        assert_ne!(
            resolved.range_selection_overlay(&scheme, false, pressed),
            resolved.range_selection_overlay(&scheme, false, pressed_selected)
        );

        // Untouched is no layer at all, in both.
        assert_eq!(
            resolved.range_selection_overlay(&scheme, true, WidgetStates::NONE),
            None
        );
        assert_eq!(
            resolved.range_selection_overlay(&scheme, false, WidgetStates::NONE),
            None
        );

        // And a theme that names one is taken at its word -- including for
        // the untouched state, where both tables answer nothing.
        const MINE: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
        let named = date_picker_under(
            DatePickerThemeData {
                range_selection_overlay_color: Some(StateProperty::all(Some(MINE))),
                ..DatePickerThemeData::new()
            },
            three,
        );
        assert_eq!(
            named.range_selection_overlay(&scheme, true, WidgetStates::NONE),
            Some(MINE)
        );
        assert_eq!(
            named.range_selection_overlay(&scheme, true, pressed),
            Some(MINE)
        );
    }

    #[test]
    fn a_range_pickers_shadow_and_tint_are_transparent_in_both_tables() {
        // The same pair of transparents the dialog carries, and for the same
        // reason: how far off the page it sits is said by the colour
        // underneath, not by a shadow.
        let mut two = ThemeData::light();
        two.use_material3 = false;
        for theme in [ThemeData::light(), two] {
            let resolved = date_picker_under(DatePickerThemeData::new(), theme);
            assert_eq!(resolved.range_picker_shadow_color, Color::TRANSPARENT);
            assert_eq!(resolved.range_picker_surface_tint_color, Color::TRANSPARENT);
        }
    }

    #[test]
    fn an_unselected_date_has_no_background_rather_than_a_transparent_one() {
        // Not the same thing. A date with no fill lets whatever is behind it
        // show through -- including a range selection's tint, which is drawn
        // under the cells and would be covered by a transparent rectangle
        // painted over it.
        assert_eq!(
            plain_cell(DateCellSlot::Day, WidgetStates::NONE).background,
            None
        );
        assert_ne!(
            plain_cell(DateCellSlot::Day, WidgetStates::NONE).background,
            Some(Color::TRANSPARENT)
        );

        let chosen = plain_cell(
            DateCellSlot::Day,
            WidgetStates::NONE.with(WidgetState::Selected),
        );
        assert_eq!(
            chosen.background,
            Some(ThemeData::light().color_scheme.primary)
        );
        assert_eq!(
            chosen.foreground,
            ThemeData::light().color_scheme.on_primary,
            "and the ink turns over with it"
        );
    }

    #[test]
    fn an_out_of_range_today_still_says_it_is_today_under_material_three() {
        // The one real disagreement between the two tables. A disabled today
        // fades towards the ordinary ink under Material 2 and stays a faded
        // `primary` under Material 3 -- so the ring around today still reads
        // as today's when the date cannot be chosen, and stopped reading as
        // anything under Material 2.
        let scheme = ThemeData::light().color_scheme;
        let mut two = ThemeData::light();
        two.use_material3 = false;
        let disabled = WidgetStates::NONE.with(WidgetState::Disabled);

        let new = cell(
            DatePickerThemeData::new(),
            ThemeData::light(),
            DateCellSlot::Today,
            disabled,
        );
        let old = cell(
            DatePickerThemeData::new(),
            two,
            DateCellSlot::Today,
            disabled,
        );
        assert_ne!(new.foreground, old.foreground);

        // Both are faded, so the difference is which colour is faded and not
        // whether one is.
        assert_ne!(new.foreground, scheme.primary, "faded, not full strength");
        assert_ne!(
            new.foreground,
            plain_cell(DateCellSlot::Day, disabled).foreground,
            "and not the ordinary date's, which is what Material 2 makes it"
        );
        assert_eq!(
            old.foreground,
            plain_cell(DateCellSlot::Day, disabled).foreground
        );
    }

    #[test]
    fn a_today_that_is_not_disabled_is_primary_in_both_tables() {
        // The rest of today's ladder agrees, which is what makes the disabled
        // arm above a disagreement rather than a different rule.
        let scheme = ThemeData::light().color_scheme;
        let mut two = ThemeData::light();
        two.use_material3 = false;
        assert_eq!(
            plain_cell(DateCellSlot::Today, WidgetStates::NONE).foreground,
            scheme.primary
        );
        assert_eq!(
            cell(
                DatePickerThemeData::new(),
                two,
                DateCellSlot::Today,
                WidgetStates::NONE
            )
            .foreground,
            scheme.primary
        );
        // And a selected today is `onPrimary` like any selected date: it is
        // sitting on the primary fill, so it has to be.
        assert_eq!(
            plain_cell(
                DateCellSlot::Today,
                WidgetStates::NONE.with(WidgetState::Selected)
            )
            .foreground,
            scheme.on_primary
        );
    }

    #[test]
    fn a_pressed_selected_day_gets_a_much_heavier_ripple_under_material_two() {
        // 0.38, three times the next largest number in either table. A
        // selected date is already carrying the primary colour, so an
        // ordinary ripple over it would not be seen. Material 3 solves the
        // same problem by drawing the ripple in `onPrimary` and leaving the
        // number where it is.
        let mut two = ThemeData::light();
        two.use_material3 = false;
        let pressed_selected = WidgetStates::of(&[WidgetState::Selected, WidgetState::Pressed]);

        let old = cell(
            DatePickerThemeData::new(),
            two.clone(),
            DateCellSlot::Day,
            pressed_selected,
        );
        let new = plain_cell(DateCellSlot::Day, pressed_selected);
        assert_ne!(old.overlay, new.overlay);
        // "Heavier than Material 3" is too weak a claim to be worth making:
        // every Material 2 overlay is heavier than every Material 3 one, so
        // an arm that had lost its 0.38 and fallen back to 0.12 would still
        // satisfy it. The distinctive thing is that Material 2's *selected*
        // branch presses harder than its own unselected one.
        let pressed_plain = cell(
            DatePickerThemeData::new(),
            two.clone(),
            DateCellSlot::Day,
            WidgetStates::NONE.with(WidgetState::Pressed),
        );
        assert!(
            old.overlay.unwrap().alpha() > pressed_plain.overlay.unwrap().alpha() * 2,
            "0.38 against 0.12, and not merely a little more"
        );

        // Hovered is the one arm the two tables agree on.
        let hovered = WidgetStates::NONE.with(WidgetState::Hovered);
        assert_eq!(
            cell(DatePickerThemeData::new(), two, DateCellSlot::Day, hovered).overlay,
            plain_cell(DateCellSlot::Day, hovered).overlay
        );
    }

    #[test]
    fn an_untouched_cell_has_no_overlay_layer_at_all() {
        // `None` rather than transparent, for the same reason as the
        // background: there is no layer, not an invisible one.
        for slot in [DateCellSlot::Day, DateCellSlot::Today, DateCellSlot::Year] {
            assert_eq!(
                plain_cell(slot, WidgetStates::NONE).overlay,
                None,
                "{slot:?}"
            );
            assert_eq!(
                plain_cell(slot, WidgetStates::NONE.with(WidgetState::Selected)).overlay,
                None,
                "{slot:?} selected but untouched"
            );
        }
    }

    #[test]
    fn a_year_is_onsurfacevariant_where_a_date_is_onsurface() {
        // The year list is quieter than the calendar: it is a way of getting
        // somewhere, not the thing being read.
        let scheme = ThemeData::light().color_scheme;
        assert_eq!(
            plain_cell(DateCellSlot::Year, WidgetStates::NONE).foreground,
            scheme.on_surface_variant()
        );
        assert_eq!(
            plain_cell(DateCellSlot::Day, WidgetStates::NONE).foreground,
            scheme.on_surface
        );
        assert_ne!(scheme.on_surface_variant(), scheme.on_surface);
    }

    #[test]
    fn a_theme_that_names_a_cells_colour_is_taken_at_its_word() {
        const MINE: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
        let named = cell(
            DatePickerThemeData {
                day_background_color: Some(StateProperty::all(Some(MINE))),
                ..DatePickerThemeData::new()
            },
            ThemeData::light(),
            DateCellSlot::Day,
            WidgetStates::NONE,
        );
        assert_eq!(
            named.background,
            Some(MINE),
            "even unselected, where the table answers nothing"
        );

        // And it does not thereby recolour today. Upstream's
        // `todayBackgroundColor` is the *defaults* object's
        // `dayBackgroundColor`, not the theme's -- so the alias is inside the
        // table and does not reach across from a theme's own answer.
        let today = cell(
            DatePickerThemeData {
                day_background_color: Some(StateProperty::all(Some(MINE))),
                ..DatePickerThemeData::new()
            },
            ThemeData::light(),
            DateCellSlot::Today,
            WidgetStates::NONE,
        );
        assert_eq!(today.background, None);
    }

    // -- What a calendar reads like, tick 253 --------------------------------
    //
    // `ResolvedDatePicker` answered for nine fields and left the eight text
    // styles to fall through to nothing. Upstream's two tables disagree about
    // six of the eight, and the disagreement is not decorative: a Material 3
    // calendar is a whole rung of the type scale larger than a Material 2
    // one. `TextTheme::headline_large` had no reader in this port at all.

    fn date_picker_under(data: DatePickerThemeData, theme: ThemeData) -> ResolvedDatePicker {
        struct Reader {
            seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedDatePicker>>>,
        }
        impl crate::framework::Component for Reader {
            fn build(&self, context: &mut BuildContext) -> crate::framework::AnyWidget {
                *self.seen.borrow_mut() = Some(ResolvedDatePicker::of(context));
                crate::framework::leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            DatePickerTheme::new(
                data,
                crate::framework::component(Reader {
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn material_two() -> ThemeData {
        let mut theme = ThemeData::light();
        theme.use_material3 = false;
        theme
    }

    #[test]
    fn a_material_three_calendar_is_a_rung_larger_than_a_material_two_one() {
        // Six of the eight differ, and these three carry the size: the date
        // at the top, the row of letters, and the dates themselves.
        let three = ThemeData::light();
        let two = material_two();
        let new = date_picker_under(DatePickerThemeData::new(), three.clone());
        let old = date_picker_under(DatePickerThemeData::new(), two.clone());

        assert_eq!(
            new.header_headline_style.as_ref().map(|s| s.font_size),
            three
                .text_theme
                .headline_large
                .as_ref()
                .map(|s| s.font_size)
        );
        assert_eq!(
            old.header_headline_style.as_ref().map(|s| s.font_size),
            two.text_theme.headline_small.as_ref().map(|s| s.font_size)
        );
        assert!(
            new.header_headline_style.as_ref().unwrap().font_size
                > old.header_headline_style.as_ref().unwrap().font_size,
            "headlineLarge over headlineSmall"
        );

        assert!(
            new.day_style.as_ref().unwrap().font_size > old.day_style.as_ref().unwrap().font_size,
            "bodyLarge over bodySmall"
        );
        assert!(
            new.weekday_style.as_ref().unwrap().font_size
                > old.weekday_style.as_ref().unwrap().font_size
        );
    }

    #[test]
    fn material_two_quietens_the_weekday_letters_and_material_three_does_not() {
        // Two ways of saying the same thing: the letter row above a calendar
        // is not the calendar. Material 2 fades it to sixty percent; Material
        // 3 leaves it at full strength and grows the dates under it instead.
        let three = ThemeData::light();
        let two = material_two();
        let new = date_picker_under(DatePickerThemeData::new(), three.clone());
        let old = date_picker_under(DatePickerThemeData::new(), two.clone());

        assert_eq!(
            new.weekday_style.as_ref().unwrap().color,
            three.color_scheme.on_surface
        );
        let faded = old.weekday_style.as_ref().unwrap().color;
        assert_ne!(faded, two.color_scheme.on_surface);
        assert_ne!(
            faded,
            two.text_theme.body_small.as_ref().unwrap().color,
            "and it is not the role's own colour either"
        );
    }

    #[test]
    fn the_toggle_button_wears_the_sub_headers_colour_in_both_tables() {
        // The one style the two tables agree on by construction rather than
        // by value: `titleSmall` in whatever the sub-header's colour turned
        // out to be. The button and the words beside it are one control, so a
        // theme that recolours the sub-header takes the button with it.
        const MINE: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
        for theme in [ThemeData::light(), material_two()] {
            let resolved = date_picker_under(
                DatePickerThemeData {
                    sub_header_foreground_color: Some(MINE),
                    ..DatePickerThemeData::new()
                },
                theme.clone(),
            );
            assert_eq!(resolved.sub_header_foreground_color, MINE);
            assert_eq!(
                resolved.toggle_button_text_style.as_ref().unwrap().color,
                MINE
            );
            assert_eq!(
                resolved
                    .toggle_button_text_style
                    .as_ref()
                    .map(|s| s.font_size),
                theme.text_theme.title_small.as_ref().map(|s| s.font_size)
            );
        }
    }

    #[test]
    fn a_year_is_a_year_at_either_size() {
        // The one style both tables answer with the same role. Worth
        // asserting rather than leaving to look like an oversight: seven of
        // the eight branch, and this one does not.
        let three = date_picker_under(DatePickerThemeData::new(), ThemeData::light());
        let two = date_picker_under(DatePickerThemeData::new(), material_two());
        assert_eq!(three.year_style, two.year_style);
        assert_eq!(three.year_style, ThemeData::light().text_theme.body_large);
    }

    #[test]
    fn a_range_pickers_header_is_smaller_than_a_single_dates() {
        // Material 3 gives the range picker `titleLarge` where the ordinary
        // header gets `headlineLarge`: two dates and a dash need the room
        // that one date did not.
        let resolved = date_picker_under(DatePickerThemeData::new(), ThemeData::light());
        assert!(
            resolved
                .range_picker_header_headline_style
                .as_ref()
                .unwrap()
                .font_size
                < resolved.header_headline_style.as_ref().unwrap().font_size
        );

        // Material 2 gives both the same role, so this is a Material 3
        // distinction and not a range picker one.
        let old = date_picker_under(DatePickerThemeData::new(), material_two());
        assert_eq!(
            old.range_picker_header_headline_style,
            old.header_headline_style
        );
    }

    #[test]
    fn a_named_style_beats_both_tables() {
        let mine = TextStyle {
            font_size: 41.0,
            ..TextStyle::default()
        };
        let resolved = date_picker_under(
            DatePickerThemeData {
                day_style: Some(mine.clone()),
                ..DatePickerThemeData::new()
            },
            ThemeData::light(),
        );
        assert_eq!(resolved.day_style, Some(mine));
        assert_ne!(
            resolved.weekday_style.as_ref().map(|s| s.font_size),
            Some(41.0),
            "and only that one"
        );
    }

    // -- The words in a decorated field, tick 249 ----------------------------
    //
    // `InputDecoration` modelled the whole structure of a field and
    // `ResolvedInputBorder` answered for its lines. Nothing answered for the
    // words: all five style fields on `InputDecorationThemeData` were plain
    // options, and an unset one fell through to nothing at all. Upstream's
    // two defaults tables are the material library's only readers of
    // `ThemeData.hintColor`, which is why `hint_color` -- and
    // `TextTheme::body_small` with it -- was named nowhere in this port
    // outside its own paperwork.

    fn input_ink(material3: bool, slot: InputTextSlot, states: WidgetStates) -> Color {
        let mut theme = ThemeData::light();
        theme.use_material3 = material3;
        ResolvedInputTextStyles::color_for(&theme, slot, states)
    }

    #[test]
    fn a_disabled_fields_helper_line_goes_transparent_under_material_two() {
        // Not "very faint". Transparent is how the line is hidden *without
        // changing the layout*, so a field does not change height when it is
        // disabled. Material 3 fades it to 38% instead and lets it be read.
        let states = WidgetStates::NONE.with(WidgetState::Disabled);
        assert_eq!(
            input_ink(false, InputTextSlot::Helper, states),
            Color::TRANSPARENT
        );
        assert_eq!(
            input_ink(false, InputTextSlot::Error, states),
            Color::TRANSPARENT
        );

        let faint = input_ink(true, InputTextSlot::Helper, states);
        assert_ne!(faint, Color::TRANSPARENT);
        assert_eq!(faint.alpha(), 97, "38% of 255, rounded");
    }

    #[test]
    fn a_material_three_error_stays_red_on_a_field_that_cannot_be_edited() {
        // The one slot in either table with no disabled branch. A complaint
        // stays legible on a disabled field because the reader still has to
        // know why the field is refused -- and it is the only reason a
        // disabled field is ever red.
        let error = ThemeData::light().color_scheme.error;
        assert_eq!(
            input_ink(
                true,
                InputTextSlot::Error,
                WidgetStates::NONE.with(WidgetState::Disabled)
            ),
            error
        );
        assert_eq!(
            input_ink(true, InputTextSlot::Error, WidgetStates::NONE),
            error
        );
    }

    #[test]
    fn only_the_floating_label_answers_to_focus_under_material_two() {
        // Which is what makes the label and the floating label two slots and
        // not one. Material 3's two tables give them character for character
        // the same answer; Material 2's do not.
        let theme = ThemeData::light();
        let focused = WidgetStates::NONE.with(WidgetState::Focused);
        assert_eq!(
            input_ink(false, InputTextSlot::FloatingLabel, focused),
            theme.color_scheme.primary
        );
        assert_eq!(
            input_ink(false, InputTextSlot::Label, focused),
            theme.hint_color,
            "an inline label is hintColor whatever the field is doing"
        );

        // Under Material 3 the two agree, in every one of the states they
        // branch on -- which is a claim worth making, because it is the
        // reason folding them would look harmless.
        for states in [
            WidgetStates::NONE,
            WidgetStates::NONE.with(WidgetState::Focused),
            WidgetStates::NONE.with(WidgetState::Disabled),
            WidgetStates::NONE.with(WidgetState::Error),
            WidgetStates::NONE.with(WidgetState::Hovered),
        ] {
            assert_eq!(
                input_ink(true, InputTextSlot::Label, states),
                input_ink(true, InputTextSlot::FloatingLabel, states),
                "{states:?}"
            );
        }
    }

    #[test]
    fn a_hovered_error_label_softens_instead_of_shouting() {
        // Upstream's error branch has three arms and two of them are the same
        // colour. Only the hovered one differs, and it is `onErrorContainer`:
        // a hover is a promise the field can be fixed.
        let scheme = ThemeData::light().color_scheme;
        let hovered = WidgetStates::of(&[WidgetState::Error, WidgetState::Hovered]);
        assert_eq!(
            input_ink(true, InputTextSlot::Label, hovered),
            scheme.on_error_container()
        );

        // Focused beats hovered, which is upstream's order: the focused arm
        // is tested first.
        let both = hovered.with(WidgetState::Focused);
        assert_eq!(input_ink(true, InputTextSlot::Label, both), scheme.error);

        // And an error that is neither is plain error red.
        assert_eq!(
            input_ink(
                true,
                InputTextSlot::Label,
                WidgetStates::NONE.with(WidgetState::Error)
            ),
            scheme.error
        );
    }

    #[test]
    fn the_hint_is_the_themes_hint_colour_and_the_helper_carries_body_small() {
        // The two queue entries this tick is about, said directly.
        let theme = ThemeData::light();
        assert_eq!(
            input_ink(false, InputTextSlot::Hint, WidgetStates::NONE),
            theme.hint_color
        );
        assert_ne!(
            theme.hint_color, theme.color_scheme.on_surface,
            "and hintColor is not just the ordinary ink"
        );

        // `bodySmall` is the helper and error role in both tables. The hint
        // has no role in either -- upstream's `hintStyle` is a bare
        // `TextStyle(color:)` that the field's own style is merged under.
        let helper = ResolvedInputTextStyles::style_for(
            &theme,
            InputTextSlot::Helper,
            WidgetStates::NONE,
            None,
        );
        let body_small = theme.text_theme.body_small.clone().expect("a role");
        assert_eq!(helper.font_size, body_small.font_size);
        assert_ne!(
            helper.font_size,
            theme
                .text_theme
                .body_large
                .clone()
                .expect("a role")
                .font_size,
            "the helper line is smaller than the field's own text"
        );

        let hint = ResolvedInputTextStyles::style_for(
            &theme,
            InputTextSlot::Hint,
            WidgetStates::NONE,
            None,
        );
        assert_eq!(hint.font_size, TextStyle::default().font_size);
    }

    #[test]
    fn a_theme_that_names_a_style_is_taken_at_its_word() {
        // The other half: a table is only consulted for what the theme left
        // unset.
        let theme = ThemeData::light();
        let mine = TextStyle {
            color: Color::argb(0xFF, 0x11, 0x22, 0x33),
            font_size: 41.0,
            ..TextStyle::default()
        };
        let asked = ResolvedInputTextStyles::style_for(
            &theme,
            InputTextSlot::Hint,
            WidgetStates::NONE.with(WidgetState::Disabled),
            Some(mine.clone()),
        );
        assert_eq!(
            asked, mine,
            "even disabled, and even in the slot the table has an opinion about"
        );
    }

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
    fn the_title_gap_moves_half_as_far_as_the_height_does() {
        // Both halves come off one `VisualDensity`, and they are *not* used
        // symmetrically: the height goes through `baseSizeAdjustment`, which
        // is four pixels a unit, while the gap is `horizontal * 2.0`.
        // Reaching for `base_size_adjustment().0` here -- the obvious thing,
        // having just used `.1` for the height -- moves the gap twice as far
        // as upstream.
        let density = VisualDensity {
            horizontal: -2.0,
            vertical: -2.0,
        };
        let tile = ResolvedListTile {
            horizontal_title_gap: 16.0,
            visual_density: density,
            ..read_in(
                |child| child,
                |context| ResolvedListTile::of(context, false, None),
            )
        };

        assert_eq!(tile.effective_horizontal_title_gap(), 16.0 - 4.0);
        assert_eq!(
            density.base_size_adjustment().0,
            -8.0,
            "which is what the wrong answer would have been"
        );
        assert_ne!(
            tile.effective_horizontal_title_gap(),
            16.0 + density.base_size_adjustment().0
        );
    }

    #[test]
    fn a_standard_density_leaves_the_title_gap_alone() {
        let tile = ResolvedListTile {
            horizontal_title_gap: 16.0,
            visual_density: VisualDensity::STANDARD,
            ..read_in(
                |child| child,
                |context| ResolvedListTile::of(context, false, None),
            )
        };
        assert_eq!(tile.effective_horizontal_title_gap(), 16.0);
    }

    #[test]
    fn a_compact_density_may_take_the_title_gap_below_zero() {
        // Upstream does not clamp it. A gap of 2 at the most compact density
        // is negative, and that is what asking for compact means.
        let tile = ResolvedListTile {
            horizontal_title_gap: 2.0,
            visual_density: VisualDensity {
                horizontal: -4.0,
                vertical: 0.0,
            },
            ..read_in(
                |child| child,
                |context| ResolvedListTile::of(context, false, None),
            )
        };
        assert_eq!(tile.effective_horizontal_title_gap(), 2.0 - 8.0);
        assert!(tile.effective_horizontal_title_gap() < 0.0);
    }

    #[test]
    fn the_visual_density_comes_down_the_same_three_steps_as_everything_else() {
        // `visualDensity ?? tileTheme.visualDensity ?? theme.visualDensity`.
        // Tick 341 left this as a hardcoded 0.0 in `ListTile::build`, which
        // is the step-three answer for a standard theme and wrong for any
        // application that set a density anywhere.
        let compact = VisualDensity {
            horizontal: -1.0,
            vertical: -2.0,
        };

        // Step three: nobody said, so the theme's own.
        let dense_theme = ThemeData::light().with_visual_density(compact);
        let installed = dense_theme.clone();
        let from_theme = read_in(
            move |child| MaterialTheme::new(installed, child),
            |context| ResolvedListTile::of(context, false, None),
        );
        assert_eq!(from_theme.visual_density, compact);

        // Step two: the tile theme, over the ThemeData.
        let roomy = VisualDensity {
            horizontal: 1.0,
            vertical: 2.0,
        };
        let both = ThemeData::light()
            .with_visual_density(compact)
            .with_list_tile_theme(ListTileThemeData::new().with_visual_density(roomy));
        let installed = both.clone();
        let from_tile_theme = read_in(
            move |child| MaterialTheme::new(installed, child),
            |context| ResolvedListTile::of(context, false, None),
        );
        assert_eq!(
            from_tile_theme.visual_density, roomy,
            "the tile theme is nearer than the ThemeData"
        );
    }

    #[test]
    fn density_shifts_a_row_and_dense_picks_a_different_one() {
        // The two are easy to conflate and they compose: `dense` chooses the
        // 48 row instead of the 56 one, and the density then moves whichever
        // was chosen by a signed number of pixels.
        type Tile = ResolvedListTile;
        let shift = VisualDensity {
            horizontal: 0.0,
            vertical: -2.0,
        }
        .base_size_adjustment()
        .1;
        assert!(shift < 0.0, "a compact density is negative: {shift}");

        assert_eq!(
            Tile::default_tile_height(false, false, false, shift),
            56.0 + shift
        );
        assert_eq!(
            Tile::default_tile_height(false, false, true, shift),
            48.0 + shift,
            "dense picked the other row, and the shift still applies"
        );
        assert_eq!(
            Tile::default_tile_height(false, true, true, shift),
            64.0 + shift
        );
    }

    #[test]
    fn a_tile_with_a_subtitle_asks_for_a_taller_row() {
        // Six numbers, not two. This crate carried the one-line row only, so
        // every tile asked for 56 -- sixteen short for a two-line one and
        // thirty-two for three lines.
        type Tile = ResolvedListTile;
        assert_eq!(Tile::default_tile_height(false, false, false, 0.0), 56.0);
        assert_eq!(Tile::default_tile_height(false, true, false, 0.0), 72.0);
        assert_eq!(Tile::default_tile_height(true, true, false, 0.0), 88.0);

        assert_eq!(Tile::default_tile_height(false, false, true, 0.0), 48.0);
        assert_eq!(Tile::default_tile_height(false, true, true, 0.0), 64.0);
        assert_eq!(Tile::default_tile_height(true, true, true, 0.0), 76.0);

        // Dense is shorter in every row, and by a different amount in each --
        // it is a table, not one subtraction.
        for (three, subtitle) in [(false, false), (false, true), (true, true)] {
            assert!(
                Tile::default_tile_height(three, subtitle, true, 0.0)
                    < Tile::default_tile_height(three, subtitle, false, 0.0)
            );
        }
        // The dense saving is 8 for one and two lines but 12 for three, so
        // it is a table rather than one subtraction applied everywhere.
        assert_eq!(
            56.0 - Tile::default_tile_height(false, false, true, 0.0),
            8.0
        );
        assert_eq!(
            72.0 - Tile::default_tile_height(false, true, true, 0.0),
            8.0
        );
        assert_eq!(
            88.0 - Tile::default_tile_height(true, true, true, 0.0),
            12.0
        );
    }

    #[test]
    fn a_three_line_tile_does_not_care_whether_it_has_a_subtitle() {
        // Upstream's first arm is `(true, _)`: a tile that declared itself
        // three-line has already said how tall it is.
        type Tile = ResolvedListTile;
        assert_eq!(
            Tile::default_tile_height(true, false, false, 0.0),
            Tile::default_tile_height(true, true, false, 0.0)
        );
        assert_eq!(Tile::default_tile_height(true, false, false, 0.0), 88.0);
    }

    #[test]
    fn the_visual_density_is_added_to_every_row_alike() {
        // `baseDensity.dy + switch (..)`: added, not multiplied, and signed,
        // so a compact density takes the same amount off each row rather than
        // a proportion of it.
        type Tile = ResolvedListTile;
        for (three, subtitle, dense) in [
            (false, false, false),
            (false, true, false),
            (true, true, true),
        ] {
            let plain = Tile::default_tile_height(three, subtitle, dense, 0.0);
            assert_eq!(
                Tile::default_tile_height(three, subtitle, dense, -4.0),
                plain - 4.0
            );
            assert_eq!(
                Tile::default_tile_height(three, subtitle, dense, 6.0),
                plain + 6.0
            );
        }
    }

    #[test]
    fn a_hairline_is_one_device_pixel_and_not_one_logical_one() {
        // Upstream's default thickness is 0 and it keeps it: nothing clamps,
        // so `BorderSide(width: 0)` reaches the painter meaning "the thinnest
        // this screen can draw". This used to answer a flat 1.0 -- on a 3x
        // screen a rule three times too heavy.
        let plain = read_in(|child| child, ResolvedDivider::of);
        assert_eq!(plain.thickness, 0.0, "upstream's default");

        assert_eq!(plain.line_thickness_for(1.0), 1.0);
        assert!((plain.line_thickness_for(3.0) - 1.0 / 3.0).abs() < 1e-6);
        assert_eq!(plain.line_thickness_for(2.0), 0.5);
        assert!(
            plain.line_thickness_for(3.0) < plain.line_thickness_for(1.0),
            "a denser screen draws a finer hairline, not the same one"
        );
    }

    #[test]
    fn a_thickness_somebody_asked_for_is_not_a_hairline() {
        // Only zero means "as thin as possible"; a positive thickness is a
        // measurement and the screen does not get to reinterpret it.
        let data =
            ThemeData::light().with_divider_theme(DividerThemeData::new().with_thickness(2.0));
        let installed = data.clone();
        let asked = read_in(
            move |child| MaterialTheme::new(installed, child),
            ResolvedDivider::of,
        );
        assert_eq!(asked.line_thickness_for(1.0), 2.0);
        assert_eq!(asked.line_thickness_for(3.0), 2.0, "the same on any screen");

        // Including a thickness thinner than a logical pixel, which upstream
        // allows and which a `max(1.0)` would quietly round up to a rule
        // twice what was asked for.
        let fine =
            ThemeData::light().with_divider_theme(DividerThemeData::new().with_thickness(0.5));
        let installed = fine.clone();
        let hairline_ish = read_in(
            move |child| MaterialTheme::new(installed, child),
            ResolvedDivider::of,
        );
        assert_eq!(hairline_ish.line_thickness_for(1.0), 0.5);
        assert_eq!(hairline_ish.line_thickness_for(3.0), 0.5);
    }

    #[test]
    fn the_border_side_a_divider_hands_out_asks_the_screen_too() {
        // `create_border_side` is the other way a divider's width reaches a
        // painter -- a drawer's edge takes one. It has to read the same
        // screen the component build does, or the two disagree on the same
        // page.
        let side = read_in(
            |child| {
                crate::media_query::MediaQuery::new(
                    crate::media_query::MediaQueryData {
                        device_pixel_ratio: 3.0,
                        ..crate::media_query::MediaQueryData::default()
                    },
                    child,
                )
            },
            crate::components::Divider::create_border_side,
        );
        assert!(
            (side.width - 1.0 / 3.0).abs() < 1e-6,
            "a hairline on a 3x screen, not a whole logical pixel: {}",
            side.width
        );
    }

    #[test]
    fn a_screen_that_reports_no_ratio_still_gets_a_line() {
        // Dividing by it would answer infinity, and a rule of infinite width
        // is worse than one logical pixel.
        let plain = read_in(|child| child, ResolvedDivider::of);
        assert_eq!(plain.line_thickness_for(0.0), 1.0);
        assert_eq!(plain.line_thickness_for(-2.0), 1.0);
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
            icon_alignment: IconAlignment::Start,
            animation_duration: ResolvedButton::ANIMATION_DURATION,
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

    // -- Which end each component theme's lerp runs from ---------------------
    //
    // Every one of these blends its two ends on a single line, and a lerp is
    // symmetric at the midpoint -- so a test at `t = 0.5` cannot tell
    // `lerp(a, b, t)` from `lerp(b, a, t)`. Each of these runs a quarter of
    // the way, and each names a distinct function, so a swap anywhere in the
    // set turns exactly one of them red.

    /// Two densities whose axes move opposite ways, so a line that reads the
    /// wrong axis lands on the other's answer.
    fn sparse() -> VisualDensity {
        VisualDensity {
            horizontal: -4.0,
            vertical: 4.0,
        }
    }

    fn dense() -> VisualDensity {
        VisualDensity {
            horizontal: 4.0,
            vertical: -4.0,
        }
    }

    fn quartered() -> Option<VisualDensity> {
        Some(VisualDensity {
            horizontal: -2.0,
            vertical: 2.0,
        })
    }

    fn quartered_back() -> Option<VisualDensity> {
        Some(VisualDensity {
            horizontal: 2.0,
            vertical: -2.0,
        })
    }

    /// Two sides differing only in width, so a swap shows as the wrong width.
    fn side(width: f32) -> BorderSide {
        BorderSide {
            color: Color::argb(255, 255, 0, 0),
            width,
            ..BorderSide::NONE
        }
    }

    #[test]
    fn the_optional_wrappers_hand_the_first_end_first() {
        // Three functions of the same shape: both ends present goes to the
        // type's own `lerp`, and only there does the order matter.
        let small = IconThemeData {
            size: Some(4.0),
            ..IconThemeData::default()
        };
        let large = IconThemeData {
            size: Some(20.0),
            ..IconThemeData::default()
        };
        assert_eq!(
            lerp_icon_theme(&Some(small.clone()), &Some(large.clone()), 0.25)
                .and_then(|theme| theme.size),
            Some(8.0)
        );
        assert_eq!(
            lerp_icon_theme(&Some(large), &Some(small), 0.25).and_then(|theme| theme.size),
            Some(20.0 - 4.0)
        );

        let a = ButtonStyle {
            visual_density: Some(sparse()),
            ..ButtonStyle::default()
        };
        let b = ButtonStyle {
            visual_density: Some(dense()),
            ..ButtonStyle::default()
        };
        assert_eq!(
            lerp_button_style(&Some(a.clone()), &Some(b.clone()), 0.25)
                .and_then(|style| style.visual_density),
            quartered()
        );
        assert_eq!(
            lerp_button_style(&Some(b), &Some(a), 0.25).and_then(|style| style.visual_density),
            quartered_back()
        );

        let a = MenuStyle {
            visual_density: Some(sparse()),
            ..MenuStyle::default()
        };
        let b = MenuStyle {
            visual_density: Some(dense()),
            ..MenuStyle::default()
        };
        assert_eq!(
            lerp_menu_style(&Some(a.clone()), &Some(b.clone()), 0.25)
                .and_then(|style| style.visual_density),
            quartered()
        );
        assert_eq!(
            lerp_menu_style(&Some(b), &Some(a), 0.25).and_then(|style| style.visual_density),
            quartered_back()
        );
    }

    #[test]
    fn every_density_arm_blends_from_the_first_end() {
        // Five separate copies of the same three-line arm, one per theme.
        let a = CheckboxThemeData {
            visual_density: Some(sparse()),
            ..CheckboxThemeData::default()
        };
        let b = CheckboxThemeData {
            visual_density: Some(dense()),
            ..CheckboxThemeData::default()
        };
        assert_eq!(
            CheckboxThemeData::lerp(&a, &b, 0.25).visual_density,
            quartered()
        );
        assert_eq!(
            CheckboxThemeData::lerp(&b, &a, 0.25).visual_density,
            quartered_back()
        );

        let a = RadioThemeData {
            visual_density: Some(sparse()),
            ..RadioThemeData::default()
        };
        let b = RadioThemeData {
            visual_density: Some(dense()),
            ..RadioThemeData::default()
        };
        assert_eq!(
            RadioThemeData::lerp(&a, &b, 0.25).visual_density,
            quartered()
        );
        assert_eq!(
            RadioThemeData::lerp(&b, &a, 0.25).visual_density,
            quartered_back()
        );

        let a = ListTileThemeData {
            visual_density: Some(sparse()),
            ..ListTileThemeData::default()
        };
        let b = ListTileThemeData {
            visual_density: Some(dense()),
            ..ListTileThemeData::default()
        };
        assert_eq!(
            ListTileThemeData::lerp(&a, &b, 0.25).visual_density,
            quartered()
        );
        assert_eq!(
            ListTileThemeData::lerp(&b, &a, 0.25).visual_density,
            quartered_back()
        );

        let a = ButtonStyle {
            visual_density: Some(sparse()),
            ..ButtonStyle::default()
        };
        let b = ButtonStyle {
            visual_density: Some(dense()),
            ..ButtonStyle::default()
        };
        assert_eq!(ButtonStyle::lerp(&a, &b, 0.25).visual_density, quartered());
        assert_eq!(
            ButtonStyle::lerp(&b, &a, 0.25).visual_density,
            quartered_back()
        );

        let a = MenuStyle {
            visual_density: Some(sparse()),
            ..MenuStyle::default()
        };
        let b = MenuStyle {
            visual_density: Some(dense()),
            ..MenuStyle::default()
        };
        assert_eq!(MenuStyle::lerp(&a, &b, 0.25).visual_density, quartered());
        assert_eq!(
            MenuStyle::lerp(&b, &a, 0.25).visual_density,
            quartered_back()
        );
    }

    #[test]
    fn every_side_arm_blends_from_the_first_end() {
        let a = CheckboxThemeData {
            side: Some(side(4.0)),
            ..CheckboxThemeData::default()
        };
        let b = CheckboxThemeData {
            side: Some(side(20.0)),
            ..CheckboxThemeData::default()
        };
        assert_eq!(
            CheckboxThemeData::lerp(&a, &b, 0.25).side.map(|s| s.width),
            Some(8.0)
        );
        assert_eq!(
            CheckboxThemeData::lerp(&b, &a, 0.25).side.map(|s| s.width),
            Some(16.0)
        );

        let a = RadioThemeData {
            side: Some(side(4.0)),
            ..RadioThemeData::default()
        };
        let b = RadioThemeData {
            side: Some(side(20.0)),
            ..RadioThemeData::default()
        };
        assert_eq!(
            RadioThemeData::lerp(&a, &b, 0.25).side.map(|s| s.width),
            Some(8.0)
        );
        assert_eq!(
            RadioThemeData::lerp(&b, &a, 0.25).side.map(|s| s.width),
            Some(16.0)
        );

        let a = ChipThemeData {
            side: Some(side(4.0)),
            ..ChipThemeData::default()
        };
        let b = ChipThemeData {
            side: Some(side(20.0)),
            ..ChipThemeData::default()
        };
        assert_eq!(
            ChipThemeData::lerp(&a, &b, 0.25).side.map(|s| s.width),
            Some(8.0)
        );
        assert_eq!(
            ChipThemeData::lerp(&b, &a, 0.25).side.map(|s| s.width),
            Some(16.0)
        );
    }

    // -- The blends that replaced a step at tick 221 ------------------------
    //
    // `tools/unlerped_fields.py` and a line-by-line read against upstream
    // found this file stepping fields upstream interpolates: every text
    // style, every padding, and the `WidgetStateProperty.lerp<double?>`
    // family. A stepping typography means every piece of text in an
    // application changes size in one frame partway through a theme
    // transition instead of growing into its new size.

    /// A typography whose fifteen styles are fifteen *different* sizes, so
    /// that a line naming the wrong one answers with another style's size.
    fn sized_theme(base: f32) -> TextTheme {
        let mut n = 0.0;
        let mut next = || {
            n += 1.0;
            Some(TextStyle {
                font_size: base + n,
                ..TextStyle::default()
            })
        };
        TextTheme {
            display_large: next(),
            display_medium: next(),
            display_small: next(),
            headline_large: next(),
            headline_medium: next(),
            headline_small: next(),
            title_large: next(),
            title_medium: next(),
            title_small: next(),
            body_large: next(),
            body_medium: next(),
            body_small: next(),
            label_large: next(),
            label_medium: next(),
            label_small: next(),
        }
    }

    #[test]
    fn a_typography_grows_into_its_new_sizes_rather_than_jumping() {
        // Upstream is `TextStyle.lerp` on all fifteen. This port had
        // `lerp_nearer` on all fifteen, which answers `a` for the whole first
        // half and then jumps.
        assert_eq!(
            TextTheme::lerp(&sized_theme(0.0), &sized_theme(80.0), 0.25),
            sized_theme(20.0)
        );
        assert_eq!(
            TextTheme::lerp(&sized_theme(80.0), &sized_theme(0.0), 0.25),
            sized_theme(60.0)
        );
    }

    #[test]
    fn a_style_only_one_end_names_still_steps() {
        // Upstream's single-null arm fades the colour in and steps the rest.
        // This port's `TextStyle` has no null fields -- `color` and
        // `font_size` are values -- so there is nowhere to put "a style whose
        // size is still null", and the whole style steps. Stated here rather
        // than left to be discovered.
        let named = TextTheme {
            body_large: Some(TextStyle {
                font_size: 20.0,
                ..TextStyle::default()
            }),
            ..TextTheme::default()
        };
        assert_eq!(
            TextTheme::lerp(&TextTheme::default(), &named, 0.499).body_large,
            None
        );
        assert_eq!(
            TextTheme::lerp(&TextTheme::default(), &named, 0.5).body_large,
            named.body_large
        );
    }

    #[test]
    fn a_tooltips_padding_and_margin_slide_rather_than_jumping() {
        // Upstream: `EdgeInsetsGeometry.lerp` for both. Two different pairs,
        // so a line reading the other field lands on the wrong number.
        let insets = |edge: f32| {
            Some(EdgeInsetsGeometry::Absolute(EdgeInsets {
                left: edge,
                top: edge,
                right: edge,
                bottom: edge,
            }))
        };
        let a = TooltipThemeData {
            padding: insets(4.0),
            margin: insets(12.0),
            ..TooltipThemeData::default()
        };
        let b = TooltipThemeData {
            padding: insets(20.0),
            margin: insets(28.0),
            ..TooltipThemeData::default()
        };
        let quarter = TooltipThemeData::lerp(&a, &b, 0.25);
        assert_eq!(
            quarter
                .padding
                .map(|p| p.resolve(crate::direction::TextDirection::Ltr).left),
            Some(8.0)
        );
        assert_eq!(
            quarter
                .margin
                .map(|m| m.resolve(crate::direction::TextDirection::Ltr).left),
            Some(16.0)
        );
    }

    #[test]
    fn a_button_styles_numbers_blend_state_by_state() {
        // Upstream: `WidgetStateProperty.lerp<double?>(a, b, t, lerpDouble)`.
        // Two different pairs again, so `elevation` and `icon_size` cannot
        // stand in for one another.
        let a = ButtonStyle {
            elevation: Some(StateProperty::all(Some(4.0))),
            icon_size: Some(StateProperty::all(Some(12.0))),
            ..ButtonStyle::default()
        };
        let b = ButtonStyle {
            elevation: Some(StateProperty::all(Some(20.0))),
            icon_size: Some(StateProperty::all(Some(28.0))),
            ..ButtonStyle::default()
        };
        let quarter = ButtonStyle::lerp(&a, &b, 0.25);
        assert_eq!(
            quarter
                .elevation
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            Some(8.0)
        );
        assert_eq!(
            quarter
                .icon_size
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            Some(16.0)
        );
    }

    #[test]
    fn a_navigation_bars_label_and_icon_blend_state_by_state() {
        // Upstream: `WidgetStateProperty.lerp<TextStyle?>` and
        // `<IconThemeData?>`. Both were stepping here.
        let a = NavigationBarThemeData {
            label_text_style: Some(StateProperty::all(Some(TextStyle {
                font_size: 4.0,
                ..TextStyle::default()
            }))),
            icon_theme: Some(StateProperty::all(Some(IconThemeData {
                size: Some(12.0),
                ..IconThemeData::default()
            }))),
            ..NavigationBarThemeData::default()
        };
        let b = NavigationBarThemeData {
            label_text_style: Some(StateProperty::all(Some(TextStyle {
                font_size: 20.0,
                ..TextStyle::default()
            }))),
            icon_theme: Some(StateProperty::all(Some(IconThemeData {
                size: Some(28.0),
                ..IconThemeData::default()
            }))),
            ..NavigationBarThemeData::default()
        };
        let quarter = NavigationBarThemeData::lerp(&a, &b, 0.25);
        assert_eq!(
            quarter
                .label_text_style
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE)
                .map(|style| style.font_size),
            Some(8.0)
        );
        assert_eq!(
            quarter
                .icon_theme
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE)
                .and_then(|theme| theme.size),
            Some(16.0)
        );
    }

    #[test]
    fn a_date_pickers_day_and_year_shapes_morph_state_by_state() {
        // Upstream: `WidgetStateProperty.lerp<OutlinedBorder?>`. The two
        // shapes carry different widths, so a line reading the other field
        // answers with a width that is not its own.
        let circle = |width: f32| {
            Some(StateProperty::all(Some(ShapeBorder::Circle(
                crate::borders::CircleBorder::new(
                    BorderSide {
                        color: Color::argb(255, 255, 0, 0),
                        width,
                        ..BorderSide::NONE
                    },
                    0.0,
                ),
            ))))
        };
        let a = DatePickerThemeData {
            day_shape: circle(4.0),
            year_shape: circle(12.0),
            ..DatePickerThemeData::default()
        };
        let b = DatePickerThemeData {
            day_shape: circle(20.0),
            year_shape: circle(28.0),
            ..DatePickerThemeData::default()
        };
        let quarter = DatePickerThemeData::lerp(&a, &b, 0.25);
        let width_of = |shape: Option<StateProperty<Option<ShapeBorder>>>| match shape
            .expect("two ends is enough")
            .resolve(WidgetStates::NONE)
        {
            Some(ShapeBorder::Circle(circle)) => circle.side.width,
            other => panic!("{other:?}"),
        };
        assert_eq!(width_of(quarter.day_shape), 8.0);
        assert_eq!(width_of(quarter.year_shape), 16.0);
    }

    #[test]
    fn the_remaining_state_numbers_blend_too() {
        // Three more `WidgetStateProperty.lerp<double?>` sites, each in a
        // different theme, each with its own pair of numbers.
        let radio = RadioThemeData {
            inner_radius: Some(StateProperty::all(Some(4.0))),
            ..RadioThemeData::default()
        };
        let bigger = RadioThemeData {
            inner_radius: Some(StateProperty::all(Some(20.0))),
            ..RadioThemeData::default()
        };
        assert_eq!(
            RadioThemeData::lerp(&radio, &bigger, 0.25)
                .inner_radius
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            Some(8.0)
        );

        let switch = SwitchThemeData {
            track_outline_width: Some(StateProperty::all(Some(8.0))),
            ..SwitchThemeData::default()
        };
        let thicker = SwitchThemeData {
            track_outline_width: Some(StateProperty::all(Some(24.0))),
            ..SwitchThemeData::default()
        };
        assert_eq!(
            SwitchThemeData::lerp(&switch, &thicker, 0.25)
                .track_outline_width
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            Some(12.0)
        );

        let bar = ScrollbarThemeData {
            thickness: Some(StateProperty::all(Some(12.0))),
            ..ScrollbarThemeData::default()
        };
        let fatter = ScrollbarThemeData {
            thickness: Some(StateProperty::all(Some(28.0))),
            ..ScrollbarThemeData::default()
        };
        assert_eq!(
            ScrollbarThemeData::lerp(&bar, &fatter, 0.25)
                .thickness
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            Some(16.0)
        );
    }

    #[test]
    fn a_dialogs_inset_padding_slides_and_a_missing_one_grows_in() {
        // Upstream: `EdgeInsets.lerp`. This is the one plain-`EdgeInsets`
        // padding in the file; `SnackBarThemeData`'s field of the same name
        // is a geometry, which is why the two cannot share a line.
        let a = DialogThemeData {
            inset_padding: Some(EdgeInsets {
                left: 4.0,
                top: 8.0,
                right: 12.0,
                bottom: 16.0,
            }),
            ..DialogThemeData::default()
        };
        let b = DialogThemeData {
            inset_padding: Some(EdgeInsets {
                left: 20.0,
                top: 24.0,
                right: 28.0,
                bottom: 32.0,
            }),
            ..DialogThemeData::default()
        };
        assert_eq!(
            DialogThemeData::lerp(&a, &b, 0.25).inset_padding,
            Some(EdgeInsets {
                left: 8.0,
                top: 12.0,
                right: 16.0,
                bottom: 20.0,
            })
        );
        // A missing end scales rather than holding the other still.
        assert_eq!(
            DialogThemeData::lerp(&DialogThemeData::default(), &b, 0.25).inset_padding,
            Some(EdgeInsets {
                left: 5.0,
                top: 6.0,
                right: 7.0,
                bottom: 8.0,
            })
        );
    }

    // -- The geometry families, tick 222 ------------------------------------
    //
    // Thirty-seven more fields that were stepping where upstream blends:
    // constraints, sizes, radii, alignments, sides and decorations. Each test
    // below gives its fields distinct values, so a line naming a neighbour's
    // field answers with a number that is not its own.

    #[test]
    fn a_tooltips_constraints_and_decoration_blend() {
        // Upstream: `BoxConstraints.lerp` and `Decoration.lerp`.
        let a = TooltipThemeData {
            constraints: Some(BoxConstraints::new(4.0, 8.0, 12.0, 16.0)),
            decoration: Some(crate::decoration::Decoration::Box(
                crate::decoration::BoxDecoration::new()
                    .with_fill(crate::render::Fill::Solid(Color::argb(255, 0, 0, 0))),
            )),
            ..TooltipThemeData::default()
        };
        let b = TooltipThemeData {
            constraints: Some(BoxConstraints::new(20.0, 24.0, 28.0, 32.0)),
            decoration: Some(crate::decoration::Decoration::Box(
                crate::decoration::BoxDecoration::new()
                    .with_fill(crate::render::Fill::Solid(Color::argb(255, 0, 0, 80))),
            )),
            ..TooltipThemeData::default()
        };
        let quarter = TooltipThemeData::lerp(&a, &b, 0.25);
        assert_eq!(
            quarter.constraints,
            Some(BoxConstraints::new(8.0, 12.0, 16.0, 20.0))
        );
        match quarter.decoration {
            Some(crate::decoration::Decoration::Box(box_decoration)) => {
                match &box_decoration.fill {
                    Some(crate::render::Fill::Solid(color)) => assert_eq!(color.blue(), 20),
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_button_styles_three_sizes_and_its_side_and_alignment_blend() {
        // Upstream: `WidgetStateProperty.lerp<Size?>` for the three sizes,
        // `WidgetStateBorderSide.lerp` for the side, `AlignmentGeometry.lerp`
        // for the alignment. All five were stepping.
        let size = |w: f32, h: f32| Some(StateProperty::all(Some(Size::new(w, h))));
        let a = ButtonStyle {
            minimum_size: size(4.0, 8.0),
            fixed_size: size(12.0, 16.0),
            maximum_size: size(40.0, 44.0),
            side: Some(StateProperty::all(Some(BorderSide {
                color: Color::argb(255, 255, 0, 0),
                width: 4.0,
                ..BorderSide::NONE
            }))),
            alignment: Some(AlignmentGeometry::Absolute(crate::render::Alignment {
                x: -1.0,
                y: 1.0,
            })),
            ..ButtonStyle::default()
        };
        let b = ButtonStyle {
            minimum_size: size(20.0, 24.0),
            fixed_size: size(28.0, 32.0),
            maximum_size: size(56.0, 60.0),
            side: Some(StateProperty::all(Some(BorderSide {
                color: Color::argb(255, 255, 0, 0),
                width: 20.0,
                ..BorderSide::NONE
            }))),
            alignment: Some(AlignmentGeometry::Absolute(crate::render::Alignment {
                x: 1.0,
                y: -1.0,
            })),
            ..ButtonStyle::default()
        };
        let quarter = ButtonStyle::lerp(&a, &b, 0.25);
        let resolved = |property: Option<StateProperty<Option<Size>>>| {
            property
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE)
        };
        assert_eq!(resolved(quarter.minimum_size), Some(Size::new(8.0, 12.0)));
        assert_eq!(resolved(quarter.fixed_size), Some(Size::new(16.0, 20.0)));
        assert_eq!(resolved(quarter.maximum_size), Some(Size::new(44.0, 48.0)));
        assert_eq!(
            quarter
                .side
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE)
                .map(|side| side.width),
            Some(8.0)
        );
        // The two axes move opposite ways, so a line reading the wrong one
        // lands on the other axis's answer.
        assert_eq!(
            quarter.alignment,
            Some(AlignmentGeometry::Absolute(crate::render::Alignment {
                x: -0.5,
                y: 0.5
            }))
        );
    }

    #[test]
    fn a_side_that_only_one_end_names_fades_in() {
        // Upstream's `_LerpSides` gives the missing end the other's colour at
        // zero alpha and zero width, so a border appearing fades in rather
        // than snapping to full width.
        let a = ButtonStyle {
            side: Some(StateProperty::all(None)),
            ..ButtonStyle::default()
        };
        let b = ButtonStyle {
            side: Some(StateProperty::all(Some(BorderSide {
                color: Color::argb(255, 255, 0, 0),
                width: 8.0,
                ..BorderSide::NONE
            }))),
            ..ButtonStyle::default()
        };
        let side = ButtonStyle::lerp(&a, &b, 0.25)
            .side
            .expect("two ends is enough")
            .resolve(WidgetStates::NONE)
            .expect("the present end");
        assert_eq!(side.width, 2.0);
        assert_eq!(side.color.alpha(), 64);
    }

    #[test]
    fn the_radii_blend_in_the_themes_that_carry_them() {
        // Three different themes, three different types: a geometry, a plain
        // radius, and a concrete border radius.
        let divider = DividerThemeData {
            radius: Some(BorderRadiusGeometry::Absolute(
                crate::borders::BorderRadius::circular(4.0),
            )),
            ..DividerThemeData::default()
        };
        let rounder = DividerThemeData {
            radius: Some(BorderRadiusGeometry::Absolute(
                crate::borders::BorderRadius::circular(20.0),
            )),
            ..DividerThemeData::default()
        };
        assert_eq!(
            DividerThemeData::lerp(&divider, &rounder, 0.25)
                .radius
                .map(|r| r.resolve(crate::direction::TextDirection::Ltr).top_left),
            Some(crate::borders::Radius::circular(8.0))
        );

        let bar = ScrollbarThemeData {
            radius: Some(crate::borders::Radius::circular(4.0)),
            ..ScrollbarThemeData::default()
        };
        let rounded = ScrollbarThemeData {
            radius: Some(crate::borders::Radius::circular(20.0)),
            ..ScrollbarThemeData::default()
        };
        assert_eq!(
            ScrollbarThemeData::lerp(&bar, &rounded, 0.25).radius,
            Some(crate::borders::Radius::circular(8.0))
        );

        let toggles = ToggleButtonsThemeData {
            border_radius: Some(crate::borders::BorderRadius::circular(4.0)),
            ..ToggleButtonsThemeData::default()
        };
        let softer = ToggleButtonsThemeData {
            border_radius: Some(crate::borders::BorderRadius::circular(20.0)),
            ..ToggleButtonsThemeData::default()
        };
        assert_eq!(
            ToggleButtonsThemeData::lerp(&toggles, &softer, 0.25).border_radius,
            Some(crate::borders::BorderRadius::circular(8.0))
        );
    }

    #[test]
    fn a_floating_action_buttons_three_constraint_sets_blend_separately() {
        // Upstream: `BoxConstraints.lerp` for each. Three different pairs, so
        // a line naming another set answers with the wrong numbers.
        let a = FloatingActionButtonThemeData {
            small_size_constraints: Some(BoxConstraints::new(4.0, 4.0, 4.0, 4.0)),
            large_size_constraints: Some(BoxConstraints::new(12.0, 12.0, 12.0, 12.0)),
            extended_size_constraints: Some(BoxConstraints::new(40.0, 40.0, 40.0, 40.0)),
            ..FloatingActionButtonThemeData::default()
        };
        let b = FloatingActionButtonThemeData {
            small_size_constraints: Some(BoxConstraints::new(20.0, 20.0, 20.0, 20.0)),
            large_size_constraints: Some(BoxConstraints::new(28.0, 28.0, 28.0, 28.0)),
            extended_size_constraints: Some(BoxConstraints::new(56.0, 56.0, 56.0, 56.0)),
            ..FloatingActionButtonThemeData::default()
        };
        let quarter = FloatingActionButtonThemeData::lerp(&a, &b, 0.25);
        assert_eq!(
            quarter.small_size_constraints.map(|c| c.min_width),
            Some(8.0)
        );
        assert_eq!(
            quarter.large_size_constraints.map(|c| c.min_width),
            Some(16.0)
        );
        assert_eq!(
            quarter.extended_size_constraints.map(|c| c.min_width),
            Some(44.0)
        );
    }

    #[test]
    fn a_tab_bars_indicator_and_a_progress_tracks_padding_blend() {
        // Upstream: `Decoration.lerp` and `EdgeInsetsGeometry.lerp`.
        let indicator = |blue: u8| {
            Some(crate::decoration::Decoration::Box(
                crate::decoration::BoxDecoration::new()
                    .with_fill(crate::render::Fill::Solid(Color::argb(255, 0, 0, blue))),
            ))
        };
        let a = TabBarThemeData {
            indicator: indicator(0),
            ..TabBarThemeData::default()
        };
        let b = TabBarThemeData {
            indicator: indicator(80),
            ..TabBarThemeData::default()
        };
        match TabBarThemeData::lerp(&a, &b, 0.25).indicator {
            Some(crate::decoration::Decoration::Box(box_decoration)) => {
                match &box_decoration.fill {
                    Some(crate::render::Fill::Solid(color)) => assert_eq!(color.blue(), 20),
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        }

        let track = |edge: f32| {
            Some(EdgeInsetsGeometry::Absolute(EdgeInsets {
                left: edge,
                top: edge,
                right: edge,
                bottom: edge,
            }))
        };
        let a = ProgressIndicatorThemeData {
            circular_track_padding: track(4.0),
            border_radius: Some(BorderRadiusGeometry::Absolute(
                crate::borders::BorderRadius::circular(12.0),
            )),
            ..ProgressIndicatorThemeData::default()
        };
        let b = ProgressIndicatorThemeData {
            circular_track_padding: track(20.0),
            border_radius: Some(BorderRadiusGeometry::Absolute(
                crate::borders::BorderRadius::circular(28.0),
            )),
            ..ProgressIndicatorThemeData::default()
        };
        let quarter = ProgressIndicatorThemeData::lerp(&a, &b, 0.25);
        assert_eq!(
            quarter
                .circular_track_padding
                .map(|p| p.resolve(crate::direction::TextDirection::Ltr).left),
            Some(8.0)
        );
        assert_eq!(
            quarter
                .border_radius
                .map(|r| r.resolve(crate::direction::TextDirection::Ltr).top_left),
            Some(crate::borders::Radius::circular(16.0))
        );
    }

    #[test]
    fn a_chips_two_box_constraints_blend_separately() {
        // Upstream: `BoxConstraints.lerp` for both. Two different pairs.
        let a = ChipThemeData {
            avatar_box_constraints: Some(BoxConstraints::new(4.0, 4.0, 4.0, 4.0)),
            delete_icon_box_constraints: Some(BoxConstraints::new(12.0, 12.0, 12.0, 12.0)),
            ..ChipThemeData::default()
        };
        let b = ChipThemeData {
            avatar_box_constraints: Some(BoxConstraints::new(20.0, 20.0, 20.0, 20.0)),
            delete_icon_box_constraints: Some(BoxConstraints::new(28.0, 28.0, 28.0, 28.0)),
            ..ChipThemeData::default()
        };
        let quarter = ChipThemeData::lerp(&a, &b, 0.25);
        assert_eq!(
            quarter.avatar_box_constraints.map(|c| c.min_width),
            Some(8.0)
        );
        assert_eq!(
            quarter.delete_icon_box_constraints.map(|c| c.min_width),
            Some(16.0)
        );
    }

    #[test]
    fn the_remaining_geometry_fields_blend_and_name_their_own_field() {
        // Ten more sites the mutation pass found unwatched, one per line
        // here, each with a number no other line in the test uses -- so a
        // line naming a neighbour answers with a value that is not its own.
        // Every one is `EdgeInsetsGeometry.lerp`, `AlignmentGeometry.lerp` or
        // `BoxConstraints.lerp` upstream.
        let edge = |n: f32| {
            Some(EdgeInsetsGeometry::Absolute(EdgeInsets {
                left: n,
                top: n,
                right: n,
                bottom: n,
            }))
        };
        let left_of = |insets: Option<EdgeInsetsGeometry>| {
            insets.map(|i| i.resolve(crate::direction::TextDirection::Ltr).left)
        };

        // Two themes carry `actions_padding`, and they must not share a line.
        let bar = AppBarThemeData {
            actions_padding: edge(4.0),
            ..AppBarThemeData::default()
        };
        let wider = AppBarThemeData {
            actions_padding: edge(20.0),
            ..AppBarThemeData::default()
        };
        assert_eq!(
            left_of(AppBarThemeData::lerp(&bar, &wider, 0.25).actions_padding),
            Some(8.0)
        );

        let dialog = DialogThemeData {
            actions_padding: edge(8.0),
            inset_padding: None,
            ..DialogThemeData::default()
        };
        let roomier = DialogThemeData {
            actions_padding: edge(24.0),
            inset_padding: None,
            ..DialogThemeData::default()
        };
        assert_eq!(
            left_of(DialogThemeData::lerp(&dialog, &roomier, 0.25).actions_padding),
            Some(12.0)
        );

        let snack = SnackBarThemeData {
            inset_padding: edge(12.0),
            ..SnackBarThemeData::default()
        };
        let further = SnackBarThemeData {
            inset_padding: edge(28.0),
            ..SnackBarThemeData::default()
        };
        assert_eq!(
            left_of(SnackBarThemeData::lerp(&snack, &further, 0.25).inset_padding),
            Some(16.0)
        );

        let tile = ListTileThemeData {
            content_padding: edge(16.0),
            ..ListTileThemeData::default()
        };
        let padded = ListTileThemeData {
            content_padding: edge(32.0),
            ..ListTileThemeData::default()
        };
        assert_eq!(
            left_of(ListTileThemeData::lerp(&tile, &padded, 0.25).content_padding),
            Some(20.0)
        );

        // Three themes carry `label_padding`.
        let chip = ChipThemeData {
            label_padding: edge(20.0),
            ..ChipThemeData::default()
        };
        let looser = ChipThemeData {
            label_padding: edge(36.0),
            ..ChipThemeData::default()
        };
        assert_eq!(
            left_of(ChipThemeData::lerp(&chip, &looser, 0.25).label_padding),
            Some(24.0)
        );

        let tabs = TabBarThemeData {
            label_padding: edge(24.0),
            ..TabBarThemeData::default()
        };
        let spread = TabBarThemeData {
            label_padding: edge(40.0),
            ..TabBarThemeData::default()
        };
        assert_eq!(
            left_of(TabBarThemeData::lerp(&tabs, &spread, 0.25).label_padding),
            Some(28.0)
        );

        let nav = NavigationBarThemeData {
            label_padding: edge(28.0),
            ..NavigationBarThemeData::default()
        };
        let airier = NavigationBarThemeData {
            label_padding: edge(44.0),
            ..NavigationBarThemeData::default()
        };
        assert_eq!(
            left_of(NavigationBarThemeData::lerp(&nav, &airier, 0.25).label_padding),
            Some(32.0)
        );

        let banner = MaterialBannerThemeData {
            leading_padding: edge(32.0),
            ..MaterialBannerThemeData::default()
        };
        let indented = MaterialBannerThemeData {
            leading_padding: edge(48.0),
            ..MaterialBannerThemeData::default()
        };
        assert_eq!(
            left_of(MaterialBannerThemeData::lerp(&banner, &indented, 0.25).leading_padding),
            Some(36.0)
        );

        // Three fields of three different kinds in one theme, so a line
        // reading its neighbour's field cannot even typecheck the same way.
        let expansion = ExpansionTileThemeData {
            tile_padding: edge(36.0),
            children_padding: edge(44.0),
            expanded_alignment: Some(AlignmentGeometry::Absolute(crate::render::Alignment {
                x: -1.0,
                y: 1.0,
            })),
            ..ExpansionTileThemeData::default()
        };
        let opened = ExpansionTileThemeData {
            tile_padding: edge(52.0),
            children_padding: edge(60.0),
            expanded_alignment: Some(AlignmentGeometry::Absolute(crate::render::Alignment {
                x: 1.0,
                y: -1.0,
            })),
            ..ExpansionTileThemeData::default()
        };
        let quarter = ExpansionTileThemeData::lerp(&expansion, &opened, 0.25);
        assert_eq!(left_of(quarter.tile_padding), Some(40.0));
        assert_eq!(left_of(quarter.children_padding), Some(48.0));
        // The two axes move opposite ways, so a line reading the wrong one
        // lands on the other axis's answer.
        assert_eq!(
            quarter.expanded_alignment,
            Some(AlignmentGeometry::Absolute(crate::render::Alignment {
                x: -0.5,
                y: 0.5
            }))
        );

        let fab = FloatingActionButtonThemeData {
            size_constraints: Some(BoxConstraints::new(60.0, 60.0, 60.0, 60.0)),
            extended_padding: edge(64.0),
            ..FloatingActionButtonThemeData::default()
        };
        let grown = FloatingActionButtonThemeData {
            size_constraints: Some(BoxConstraints::new(76.0, 76.0, 76.0, 76.0)),
            extended_padding: edge(80.0),
            ..FloatingActionButtonThemeData::default()
        };
        let quarter = FloatingActionButtonThemeData::lerp(&fab, &grown, 0.25);
        assert_eq!(quarter.size_constraints.map(|c| c.min_width), Some(64.0));
        assert_eq!(left_of(quarter.extended_padding), Some(68.0));
    }

    #[test]
    fn a_size_that_only_one_end_names_grows_in() {
        // `Size.lerp(null, b, t)` is `b * t`, the same shape as its sibling
        // geometries: a minimum only the destination theme names grows out of
        // nothing rather than springing to full size at the first frame.
        let a = ButtonStyle {
            minimum_size: Some(StateProperty::all(None)),
            ..ButtonStyle::default()
        };
        let b = ButtonStyle {
            minimum_size: Some(StateProperty::all(Some(Size::new(20.0, 4.0)))),
            ..ButtonStyle::default()
        };
        assert_eq!(
            ButtonStyle::lerp(&a, &b, 0.25)
                .minimum_size
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            Some(Size::new(5.0, 1.0))
        );
        assert_eq!(
            ButtonStyle::lerp(&b, &a, 0.25)
                .minimum_size
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            Some(Size::new(15.0, 3.0))
        );
    }

    // -- Shapes morph, tick 223 ---------------------------------------------
    //
    // Twenty-seven fields across twenty-three themes went through
    // `lerp_nearer`, so every card, dialog, chip, drawer, menu and picker
    // swapped its outline in one frame at the midpoint of a theme transition
    // instead of morphing. Tick 218 spent a whole tick making
    // `ShapeBorder::lerp` right; nothing in the themes was calling it.

    /// A rounded rectangle whose side width is the only thing that moves, so
    /// a blended shape reads back as a number.
    fn outline(width: f32) -> Option<ShapeBorder> {
        Some(ShapeBorder::Rounded(
            crate::borders::RoundedRectangleBorder::new(
                BorderSide {
                    color: Color::argb(255, 255, 0, 0),
                    width,
                    ..BorderSide::NONE
                },
                crate::borders::BorderRadiusGeometry::Absolute(
                    crate::borders::BorderRadius::circular(8.0),
                ),
            ),
        ))
    }

    fn outline_width(shape: Option<ShapeBorder>) -> f32 {
        match shape {
            Some(ShapeBorder::Rounded(rounded)) => rounded.side.width,
            other => panic!("{other:?}"),
        }
    }

    fn state_outline_width(shape: Option<StateProperty<Option<ShapeBorder>>>) -> f32 {
        outline_width(
            shape
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
        )
    }

    #[test]
    fn a_shape_morphs_rather_than_being_swapped_at_the_midpoint() {
        // One theme in full, to say what "morphs" means: a quarter of the way
        // is a quarter of the way, not the first end.
        let a = CardThemeData {
            shape: outline(4.0),
            ..CardThemeData::default()
        };
        let b = CardThemeData {
            shape: outline(20.0),
            ..CardThemeData::default()
        };
        assert_eq!(outline_width(CardThemeData::lerp(&a, &b, 0.25).shape), 8.0);
        assert_eq!(outline_width(CardThemeData::lerp(&b, &a, 0.25).shape), 16.0);
    }

    #[test]
    fn the_themes_with_two_or_more_shapes_keep_them_apart() {
        // Where one theme carries several outlines, each gets a width no
        // other line in the same theme uses -- so a line naming its
        // neighbour's field answers with a width that is not its own.
        let drawer = DrawerThemeData {
            shape: outline(4.0),
            end_shape: outline(12.0),
            ..DrawerThemeData::default()
        };
        let opened = DrawerThemeData {
            shape: outline(20.0),
            end_shape: outline(28.0),
            ..DrawerThemeData::default()
        };
        let quarter = DrawerThemeData::lerp(&drawer, &opened, 0.25);
        assert_eq!(outline_width(quarter.shape), 8.0);
        assert_eq!(outline_width(quarter.end_shape), 16.0);

        let tile = ExpansionTileThemeData {
            shape: outline(4.0),
            collapsed_shape: outline(12.0),
            ..ExpansionTileThemeData::default()
        };
        let expanded = ExpansionTileThemeData {
            shape: outline(20.0),
            collapsed_shape: outline(28.0),
            ..ExpansionTileThemeData::default()
        };
        let quarter = ExpansionTileThemeData::lerp(&tile, &expanded, 0.25);
        assert_eq!(outline_width(quarter.shape), 8.0);
        assert_eq!(outline_width(quarter.collapsed_shape), 16.0);

        let clock = TimePickerThemeData {
            shape: outline(4.0),
            day_period_shape: outline(12.0),
            hour_minute_shape: outline(40.0),
            ..TimePickerThemeData::default()
        };
        let later = TimePickerThemeData {
            shape: outline(20.0),
            day_period_shape: outline(28.0),
            hour_minute_shape: outline(56.0),
            ..TimePickerThemeData::default()
        };
        let quarter = TimePickerThemeData::lerp(&clock, &later, 0.25);
        assert_eq!(outline_width(quarter.shape), 8.0);
        assert_eq!(outline_width(quarter.day_period_shape), 16.0);
        assert_eq!(outline_width(quarter.hour_minute_shape), 44.0);

        let calendar = DatePickerThemeData {
            shape: outline(4.0),
            range_picker_shape: outline(12.0),
            ..DatePickerThemeData::default()
        };
        let next_month = DatePickerThemeData {
            shape: outline(20.0),
            range_picker_shape: outline(28.0),
            ..DatePickerThemeData::default()
        };
        let quarter = DatePickerThemeData::lerp(&calendar, &next_month, 0.25);
        assert_eq!(outline_width(quarter.shape), 8.0);
        assert_eq!(outline_width(quarter.range_picker_shape), 16.0);
    }

    #[test]
    fn every_other_theme_that_carries_a_shape_morphs_it_too() {
        // One assertion per remaining site. They are separate `lerp`
        // methods, so a line in one cannot stand in for a line in another --
        // what this catches is a site left behind on `lerp_nearer`.
        macro_rules! morphs {
            ($theme:ident, $field:ident) => {{
                let a = $theme {
                    $field: outline(4.0),
                    ..$theme::default()
                };
                let b = $theme {
                    $field: outline(20.0),
                    ..$theme::default()
                };
                assert_eq!(
                    outline_width($theme::lerp(&a, &b, 0.25).$field),
                    8.0,
                    concat!(stringify!($theme), "::", stringify!($field))
                );
            }};
        }
        morphs!(CheckboxThemeData, shape);
        morphs!(AppBarThemeData, shape);
        morphs!(BottomSheetThemeData, shape);
        morphs!(SnackBarThemeData, shape);
        morphs!(ListTileThemeData, shape);
        morphs!(DialogThemeData, shape);
        morphs!(ChipThemeData, shape);
        morphs!(NavigationRailThemeData, indicator_shape);
        morphs!(FloatingActionButtonThemeData, shape);
        morphs!(SearchViewThemeData, shape);
        morphs!(PopupMenuThemeData, shape);
        morphs!(NavigationBarThemeData, indicator_shape);
        morphs!(NavigationDrawerThemeData, indicator_shape);
        morphs!(CarouselViewThemeData, shape);
    }

    #[test]
    fn the_three_state_property_shapes_morph_state_by_state() {
        // Upstream: `WidgetStateProperty.lerp<OutlinedBorder?>`.
        macro_rules! morphs {
            ($theme:ident) => {{
                let a = $theme {
                    shape: Some(StateProperty::all(outline(4.0))),
                    ..$theme::default()
                };
                let b = $theme {
                    shape: Some(StateProperty::all(outline(20.0))),
                    ..$theme::default()
                };
                assert_eq!(
                    state_outline_width($theme::lerp(&a, &b, 0.25).shape),
                    8.0,
                    stringify!($theme)
                );
            }};
        }
        morphs!(ButtonStyle);
        morphs!(MenuStyle);
        morphs!(SearchBarThemeData);
    }

    #[test]
    fn a_notched_shape_really_does_step() {
        // `BottomAppBarThemeData.shape` is a `NotchedShape`, and upstream
        // steps it: `t < 0.5 ? a?.shape : b?.shape`. It sits among two dozen
        // outlines that all morph, so the reason it does not is worth having
        // written down where the next sweep will read it. A notch is a
        // computed cut-out, not a border with a width; there is nothing
        // between a circular notch and a flat edge to be half-way at.
        let a = BottomAppBarThemeData {
            shape: Some(crate::borders::NotchedShape::Circular { inverted: false }),
            ..BottomAppBarThemeData::default()
        };
        let b = BottomAppBarThemeData {
            shape: Some(crate::borders::NotchedShape::Circular { inverted: true }),
            ..BottomAppBarThemeData::default()
        };
        assert_eq!(
            BottomAppBarThemeData::lerp(&a, &b, 0.499).shape,
            Some(crate::borders::NotchedShape::Circular { inverted: false })
        );
        assert_eq!(
            BottomAppBarThemeData::lerp(&a, &b, 0.5).shape,
            Some(crate::borders::NotchedShape::Circular { inverted: true })
        );
    }

    // -- Direction, for the half of this file no screen had read -------------
    //
    // `tools/swap_lerps.py` stopped at the first `#[cfg(test)]`, and this file
    // has 96,000 characters of code after it. With the boundary corrected the
    // screen found sixty swappable sites here rather than eleven, and
    // twenty-three of them could have their two ends exchanged with the whole
    // suite still green. These are those. A lerp is symmetric at its midpoint,
    // so each runs a quarter of the way, and each pair of ends differs enough
    // that the reversed answer is a different number.

    #[test]
    fn every_remaining_inset_runs_from_the_first_end() {
        macro_rules! runs {
            ($theme:ident, $field:ident) => {{
                let a = $theme {
                    $field: Some(EdgeInsetsGeometry::Absolute(EdgeInsets {
                        left: 4.0,
                        top: 4.0,
                        right: 4.0,
                        bottom: 4.0,
                    })),
                    ..$theme::default()
                };
                let b = $theme {
                    $field: Some(EdgeInsetsGeometry::Absolute(EdgeInsets {
                        left: 20.0,
                        top: 20.0,
                        right: 20.0,
                        bottom: 20.0,
                    })),
                    ..$theme::default()
                };
                let read = |theme: $theme| {
                    theme
                        .$field
                        .map(|i| i.resolve(crate::direction::TextDirection::Ltr).left)
                };
                assert_eq!(
                    read($theme::lerp(&a, &b, 0.25)),
                    Some(8.0),
                    concat!(stringify!($theme), "::", stringify!($field))
                );
                assert_eq!(
                    read($theme::lerp(&b, &a, 0.25)),
                    Some(16.0),
                    concat!(stringify!($theme), "::", stringify!($field), " reversed")
                );
            }};
        }
        runs!(CardThemeData, margin);
        runs!(BadgeThemeData, padding);
        runs!(SwitchThemeData, padding);
        runs!(ChipThemeData, padding);
        runs!(MaterialBannerThemeData, padding);
        runs!(SearchViewThemeData, padding);
        runs!(SearchViewThemeData, bar_padding);
        runs!(TimePickerThemeData, padding);
        runs!(PopupMenuThemeData, menu_padding);
        runs!(BottomAppBarThemeData, padding);
        runs!(ButtonBarThemeData, button_padding);
    }

    #[test]
    fn every_remaining_constraint_runs_from_the_first_end() {
        macro_rules! runs {
            ($theme:ident, $field:ident) => {{
                let a = $theme {
                    $field: Some(BoxConstraints::new(4.0, 4.0, 4.0, 4.0)),
                    ..$theme::default()
                };
                let b = $theme {
                    $field: Some(BoxConstraints::new(20.0, 20.0, 20.0, 20.0)),
                    ..$theme::default()
                };
                assert_eq!(
                    $theme::lerp(&a, &b, 0.25).$field.map(|c| c.min_width),
                    Some(8.0),
                    concat!(stringify!($theme), "::", stringify!($field))
                );
                assert_eq!(
                    $theme::lerp(&b, &a, 0.25).$field.map(|c| c.min_width),
                    Some(16.0),
                    concat!(stringify!($theme), "::", stringify!($field), " reversed")
                );
            }};
        }
        runs!(ProgressIndicatorThemeData, constraints);
        runs!(BottomSheetThemeData, constraints);
        runs!(DialogThemeData, constraints);
        runs!(ToggleButtonsThemeData, constraints);
        runs!(SearchBarThemeData, constraints);
        runs!(SearchViewThemeData, constraints);
    }

    #[test]
    fn every_remaining_alignment_runs_from_the_first_end() {
        macro_rules! runs {
            ($theme:ident, $field:ident) => {{
                // The two axes move opposite ways, so a line reading the
                // wrong one lands on the other axis's answer as well.
                let a = $theme {
                    $field: Some(AlignmentGeometry::Absolute(crate::render::Alignment {
                        x: -1.0,
                        y: 1.0,
                    })),
                    ..$theme::default()
                };
                let b = $theme {
                    $field: Some(AlignmentGeometry::Absolute(crate::render::Alignment {
                        x: 1.0,
                        y: -1.0,
                    })),
                    ..$theme::default()
                };
                assert_eq!(
                    $theme::lerp(&a, &b, 0.25).$field,
                    Some(AlignmentGeometry::Absolute(crate::render::Alignment {
                        x: -0.5,
                        y: 0.5
                    })),
                    concat!(stringify!($theme), "::", stringify!($field))
                );
                assert_eq!(
                    $theme::lerp(&b, &a, 0.25).$field,
                    Some(AlignmentGeometry::Absolute(crate::render::Alignment {
                        x: 0.5,
                        y: -0.5
                    })),
                    concat!(stringify!($theme), "::", stringify!($field), " reversed")
                );
            }};
        }
        runs!(BadgeThemeData, alignment);
        runs!(DialogThemeData, alignment);
        runs!(MenuStyle, alignment);
    }

    #[test]
    fn the_remaining_border_side_arms_run_from_the_first_end() {
        // Three more copies of the `(Some, Some) => BorderSide::lerp` arm,
        // in three themes that had none.
        let side = |width: f32| {
            Some(BorderSide {
                color: Color::argb(255, 255, 0, 0),
                width,
                ..BorderSide::NONE
            })
        };

        let a = SearchViewThemeData {
            side: side(4.0),
            ..SearchViewThemeData::default()
        };
        let b = SearchViewThemeData {
            side: side(20.0),
            ..SearchViewThemeData::default()
        };
        assert_eq!(
            SearchViewThemeData::lerp(&a, &b, 0.25)
                .side
                .map(|s| s.width),
            Some(8.0)
        );
        assert_eq!(
            SearchViewThemeData::lerp(&b, &a, 0.25)
                .side
                .map(|s| s.width),
            Some(16.0)
        );

        let a = TimePickerThemeData {
            day_period_border_side: side(4.0),
            ..TimePickerThemeData::default()
        };
        let b = TimePickerThemeData {
            day_period_border_side: side(20.0),
            ..TimePickerThemeData::default()
        };
        assert_eq!(
            TimePickerThemeData::lerp(&a, &b, 0.25)
                .day_period_border_side
                .map(|s| s.width),
            Some(8.0)
        );
        assert_eq!(
            TimePickerThemeData::lerp(&b, &a, 0.25)
                .day_period_border_side
                .map(|s| s.width),
            Some(16.0)
        );

        let a = DatePickerThemeData {
            today_border: side(4.0),
            ..DatePickerThemeData::default()
        };
        let b = DatePickerThemeData {
            today_border: side(20.0),
            ..DatePickerThemeData::default()
        };
        assert_eq!(
            DatePickerThemeData::lerp(&a, &b, 0.25)
                .today_border
                .map(|s| s.width),
            Some(8.0)
        );
        assert_eq!(
            DatePickerThemeData::lerp(&b, &a, 0.25)
                .today_border
                .map(|s| s.width),
            Some(16.0)
        );
    }

    // -- Every plainly-blended field, and every line naming its own ---------
    //
    // `tools/unlerped_fields.py` froze each of this file's 371 blended
    // fields in turn and 284 of them left the suite green. Two hundred and
    // eighty-four tests would be absurd; one assertion per theme is not.
    //
    // Each theme below is built twice with every one of its plainly-blended
    // fields set to a number no other field in that theme uses. The blended
    // result can only equal the theme built from the expected numbers if
    // every line reads the field it is assigned to -- which is the defect
    // this shape actually has, a copy-pasted line still naming the field
    // above it. Reading it back a quarter of the way rather than half also
    // catches a line whose two ends are the wrong way round.
    //
    // Generated from the source: the field names are the thing under test,
    // so typing them by hand would be typing the bug into the test.

    fn numbered_divider_theme_data(base: u8) -> DividerThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        DividerThemeData {
            color: Some(Color::argb(255, 0, 0, next())),
            space: Some(f32::from(next())),
            thickness: Some(f32::from(next())),
            indent: Some(f32::from(next())),
            end_indent: Some(f32::from(next())),
            ..DividerThemeData::default()
        }
    }

    #[test]
    fn divider_theme_data_blends_every_field_it_names() {
        assert_eq!(
            DividerThemeData::lerp(
                &numbered_divider_theme_data(0),
                &numbered_divider_theme_data(80),
                0.25
            ),
            numbered_divider_theme_data(20)
        );
        assert_eq!(
            DividerThemeData::lerp(
                &numbered_divider_theme_data(80),
                &numbered_divider_theme_data(0),
                0.25
            ),
            numbered_divider_theme_data(60)
        );
    }

    fn numbered_card_theme_data(base: u8) -> CardThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        CardThemeData {
            color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            ..CardThemeData::default()
        }
    }

    #[test]
    fn card_theme_data_blends_every_field_it_names() {
        assert_eq!(
            CardThemeData::lerp(
                &numbered_card_theme_data(0),
                &numbered_card_theme_data(80),
                0.25
            ),
            numbered_card_theme_data(20)
        );
        assert_eq!(
            CardThemeData::lerp(
                &numbered_card_theme_data(80),
                &numbered_card_theme_data(0),
                0.25
            ),
            numbered_card_theme_data(60)
        );
    }

    fn numbered_badge_theme_data(base: u8) -> BadgeThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        BadgeThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            text_color: Some(Color::argb(255, 0, 0, next())),
            small_size: Some(f32::from(next())),
            large_size: Some(f32::from(next())),
            ..BadgeThemeData::default()
        }
    }

    #[test]
    fn badge_theme_data_blends_every_field_it_names() {
        assert_eq!(
            BadgeThemeData::lerp(
                &numbered_badge_theme_data(0),
                &numbered_badge_theme_data(80),
                0.25
            ),
            numbered_badge_theme_data(20)
        );
        assert_eq!(
            BadgeThemeData::lerp(
                &numbered_badge_theme_data(80),
                &numbered_badge_theme_data(0),
                0.25
            ),
            numbered_badge_theme_data(60)
        );
    }

    fn numbered_tooltip_theme_data(base: u8) -> TooltipThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        TooltipThemeData {
            height: Some(f32::from(next())),
            vertical_offset: Some(f32::from(next())),
            ..TooltipThemeData::default()
        }
    }

    #[test]
    fn tooltip_theme_data_blends_every_field_it_names() {
        assert_eq!(
            TooltipThemeData::lerp(
                &numbered_tooltip_theme_data(0),
                &numbered_tooltip_theme_data(80),
                0.25
            ),
            numbered_tooltip_theme_data(20)
        );
        assert_eq!(
            TooltipThemeData::lerp(
                &numbered_tooltip_theme_data(80),
                &numbered_tooltip_theme_data(0),
                0.25
            ),
            numbered_tooltip_theme_data(60)
        );
    }

    fn numbered_progress_indicator_theme_data(base: u8) -> ProgressIndicatorThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        ProgressIndicatorThemeData {
            color: Some(Color::argb(255, 0, 0, next())),
            linear_track_color: Some(Color::argb(255, 0, 0, next())),
            circular_track_color: Some(Color::argb(255, 0, 0, next())),
            stop_indicator_color: Some(Color::argb(255, 0, 0, next())),
            linear_min_height: Some(f32::from(next())),
            stop_indicator_radius: Some(f32::from(next())),
            stroke_width: Some(f32::from(next())),
            stroke_align: Some(f32::from(next())),
            track_gap: Some(f32::from(next())),
            ..ProgressIndicatorThemeData::default()
        }
    }

    #[test]
    fn progress_indicator_theme_data_blends_every_field_it_names() {
        assert_eq!(
            ProgressIndicatorThemeData::lerp(
                &numbered_progress_indicator_theme_data(0),
                &numbered_progress_indicator_theme_data(80),
                0.25
            ),
            numbered_progress_indicator_theme_data(20)
        );
        assert_eq!(
            ProgressIndicatorThemeData::lerp(
                &numbered_progress_indicator_theme_data(80),
                &numbered_progress_indicator_theme_data(0),
                0.25
            ),
            numbered_progress_indicator_theme_data(60)
        );
    }

    fn numbered_app_bar_theme_data(base: u8) -> AppBarThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        AppBarThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            foreground_color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            title_spacing: Some(f32::from(next())),
            leading_width: Some(f32::from(next())),
            toolbar_height: Some(f32::from(next())),
            ..AppBarThemeData::default()
        }
    }

    #[test]
    fn app_bar_theme_data_blends_every_field_it_names() {
        assert_eq!(
            AppBarThemeData::lerp(
                &numbered_app_bar_theme_data(0),
                &numbered_app_bar_theme_data(80),
                0.25
            ),
            numbered_app_bar_theme_data(20)
        );
        assert_eq!(
            AppBarThemeData::lerp(
                &numbered_app_bar_theme_data(80),
                &numbered_app_bar_theme_data(0),
                0.25
            ),
            numbered_app_bar_theme_data(60)
        );
    }

    fn numbered_bottom_sheet_theme_data(base: u8) -> BottomSheetThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        BottomSheetThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            modal_barrier_color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            drag_handle_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            modal_elevation: Some(f32::from(next())),
            ..BottomSheetThemeData::default()
        }
    }

    #[test]
    fn bottom_sheet_theme_data_blends_every_field_it_names() {
        assert_eq!(
            BottomSheetThemeData::lerp(
                &numbered_bottom_sheet_theme_data(0),
                &numbered_bottom_sheet_theme_data(80),
                0.25
            ),
            numbered_bottom_sheet_theme_data(20)
        );
        assert_eq!(
            BottomSheetThemeData::lerp(
                &numbered_bottom_sheet_theme_data(80),
                &numbered_bottom_sheet_theme_data(0),
                0.25
            ),
            numbered_bottom_sheet_theme_data(60)
        );
    }

    fn numbered_snack_bar_theme_data(base: u8) -> SnackBarThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        SnackBarThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            action_text_color: Some(Color::argb(255, 0, 0, next())),
            close_icon_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            width: Some(f32::from(next())),
            ..SnackBarThemeData::default()
        }
    }

    #[test]
    fn snack_bar_theme_data_blends_every_field_it_names() {
        assert_eq!(
            SnackBarThemeData::lerp(
                &numbered_snack_bar_theme_data(0),
                &numbered_snack_bar_theme_data(80),
                0.25
            ),
            numbered_snack_bar_theme_data(20)
        );
        assert_eq!(
            SnackBarThemeData::lerp(
                &numbered_snack_bar_theme_data(80),
                &numbered_snack_bar_theme_data(0),
                0.25
            ),
            numbered_snack_bar_theme_data(60)
        );
    }

    fn numbered_list_tile_theme_data(base: u8) -> ListTileThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        ListTileThemeData {
            selected_color: Some(Color::argb(255, 0, 0, next())),
            icon_color: Some(Color::argb(255, 0, 0, next())),
            text_color: Some(Color::argb(255, 0, 0, next())),
            tile_color: Some(Color::argb(255, 0, 0, next())),
            selected_tile_color: Some(Color::argb(255, 0, 0, next())),
            horizontal_title_gap: Some(f32::from(next())),
            min_vertical_padding: Some(f32::from(next())),
            min_leading_width: Some(f32::from(next())),
            min_tile_height: Some(f32::from(next())),
            ..ListTileThemeData::default()
        }
    }

    #[test]
    fn list_tile_theme_data_blends_every_field_it_names() {
        assert_eq!(
            ListTileThemeData::lerp(
                &numbered_list_tile_theme_data(0),
                &numbered_list_tile_theme_data(80),
                0.25
            ),
            numbered_list_tile_theme_data(20)
        );
        assert_eq!(
            ListTileThemeData::lerp(
                &numbered_list_tile_theme_data(80),
                &numbered_list_tile_theme_data(0),
                0.25
            ),
            numbered_list_tile_theme_data(60)
        );
    }

    fn numbered_dialog_theme_data(base: u8) -> DialogThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        DialogThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            icon_color: Some(Color::argb(255, 0, 0, next())),
            barrier_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            ..DialogThemeData::default()
        }
    }

    #[test]
    fn dialog_theme_data_blends_every_field_it_names() {
        assert_eq!(
            DialogThemeData::lerp(
                &numbered_dialog_theme_data(0),
                &numbered_dialog_theme_data(80),
                0.25
            ),
            numbered_dialog_theme_data(20)
        );
        assert_eq!(
            DialogThemeData::lerp(
                &numbered_dialog_theme_data(80),
                &numbered_dialog_theme_data(0),
                0.25
            ),
            numbered_dialog_theme_data(60)
        );
    }

    fn numbered_chip_theme_data(base: u8) -> ChipThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        ChipThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            delete_icon_color: Some(Color::argb(255, 0, 0, next())),
            disabled_color: Some(Color::argb(255, 0, 0, next())),
            selected_color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            selected_shadow_color: Some(Color::argb(255, 0, 0, next())),
            checkmark_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            press_elevation: Some(f32::from(next())),
            ..ChipThemeData::default()
        }
    }

    #[test]
    fn chip_theme_data_blends_every_field_it_names() {
        assert_eq!(
            ChipThemeData::lerp(
                &numbered_chip_theme_data(0),
                &numbered_chip_theme_data(80),
                0.25
            ),
            numbered_chip_theme_data(20)
        );
        assert_eq!(
            ChipThemeData::lerp(
                &numbered_chip_theme_data(80),
                &numbered_chip_theme_data(0),
                0.25
            ),
            numbered_chip_theme_data(60)
        );
    }

    fn numbered_tab_bar_theme_data(base: u8) -> TabBarThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        TabBarThemeData {
            indicator_color: Some(Color::argb(255, 0, 0, next())),
            divider_color: Some(Color::argb(255, 0, 0, next())),
            label_color: Some(Color::argb(255, 0, 0, next())),
            divider_height: Some(f32::from(next())),
            ..TabBarThemeData::default()
        }
    }

    #[test]
    fn tab_bar_theme_data_blends_every_field_it_names() {
        assert_eq!(
            TabBarThemeData::lerp(
                &numbered_tab_bar_theme_data(0),
                &numbered_tab_bar_theme_data(80),
                0.25
            ),
            numbered_tab_bar_theme_data(20)
        );
        assert_eq!(
            TabBarThemeData::lerp(
                &numbered_tab_bar_theme_data(80),
                &numbered_tab_bar_theme_data(0),
                0.25
            ),
            numbered_tab_bar_theme_data(60)
        );
    }

    fn numbered_data_table_theme_data(base: u8) -> DataTableThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        DataTableThemeData {
            data_row_min_height: Some(f32::from(next())),
            data_row_max_height: Some(f32::from(next())),
            heading_row_height: Some(f32::from(next())),
            horizontal_margin: Some(f32::from(next())),
            column_spacing: Some(f32::from(next())),
            divider_thickness: Some(f32::from(next())),
            ..DataTableThemeData::default()
        }
    }

    #[test]
    fn data_table_theme_data_blends_every_field_it_names() {
        assert_eq!(
            DataTableThemeData::lerp(
                &numbered_data_table_theme_data(0),
                &numbered_data_table_theme_data(80),
                0.25
            ),
            numbered_data_table_theme_data(20)
        );
        assert_eq!(
            DataTableThemeData::lerp(
                &numbered_data_table_theme_data(80),
                &numbered_data_table_theme_data(0),
                0.25
            ),
            numbered_data_table_theme_data(60)
        );
    }

    fn numbered_navigation_rail_theme_data(base: u8) -> NavigationRailThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        NavigationRailThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            indicator_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            group_alignment: Some(f32::from(next())),
            min_width: Some(f32::from(next())),
            min_extended_width: Some(f32::from(next())),
            ..NavigationRailThemeData::default()
        }
    }

    #[test]
    fn navigation_rail_theme_data_blends_every_field_it_names() {
        assert_eq!(
            NavigationRailThemeData::lerp(
                &numbered_navigation_rail_theme_data(0),
                &numbered_navigation_rail_theme_data(80),
                0.25
            ),
            numbered_navigation_rail_theme_data(20)
        );
        assert_eq!(
            NavigationRailThemeData::lerp(
                &numbered_navigation_rail_theme_data(80),
                &numbered_navigation_rail_theme_data(0),
                0.25
            ),
            numbered_navigation_rail_theme_data(60)
        );
    }

    fn numbered_bottom_navigation_bar_theme_data(base: u8) -> BottomNavigationBarThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        BottomNavigationBarThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            selected_item_color: Some(Color::argb(255, 0, 0, next())),
            unselected_item_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            ..BottomNavigationBarThemeData::default()
        }
    }

    #[test]
    fn bottom_navigation_bar_theme_data_blends_every_field_it_names() {
        assert_eq!(
            BottomNavigationBarThemeData::lerp(
                &numbered_bottom_navigation_bar_theme_data(0),
                &numbered_bottom_navigation_bar_theme_data(80),
                0.25
            ),
            numbered_bottom_navigation_bar_theme_data(20)
        );
        assert_eq!(
            BottomNavigationBarThemeData::lerp(
                &numbered_bottom_navigation_bar_theme_data(80),
                &numbered_bottom_navigation_bar_theme_data(0),
                0.25
            ),
            numbered_bottom_navigation_bar_theme_data(60)
        );
    }

    fn numbered_drawer_theme_data(base: u8) -> DrawerThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        DrawerThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            scrim_color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            width: Some(f32::from(next())),
            ..DrawerThemeData::default()
        }
    }

    #[test]
    fn drawer_theme_data_blends_every_field_it_names() {
        assert_eq!(
            DrawerThemeData::lerp(
                &numbered_drawer_theme_data(0),
                &numbered_drawer_theme_data(80),
                0.25
            ),
            numbered_drawer_theme_data(20)
        );
        assert_eq!(
            DrawerThemeData::lerp(
                &numbered_drawer_theme_data(80),
                &numbered_drawer_theme_data(0),
                0.25
            ),
            numbered_drawer_theme_data(60)
        );
    }

    fn numbered_material_banner_theme_data(base: u8) -> MaterialBannerThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        MaterialBannerThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            divider_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            ..MaterialBannerThemeData::default()
        }
    }

    #[test]
    fn material_banner_theme_data_blends_every_field_it_names() {
        assert_eq!(
            MaterialBannerThemeData::lerp(
                &numbered_material_banner_theme_data(0),
                &numbered_material_banner_theme_data(80),
                0.25
            ),
            numbered_material_banner_theme_data(20)
        );
        assert_eq!(
            MaterialBannerThemeData::lerp(
                &numbered_material_banner_theme_data(80),
                &numbered_material_banner_theme_data(0),
                0.25
            ),
            numbered_material_banner_theme_data(60)
        );
    }

    fn numbered_expansion_tile_theme_data(base: u8) -> ExpansionTileThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        ExpansionTileThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            icon_color: Some(Color::argb(255, 0, 0, next())),
            collapsed_icon_color: Some(Color::argb(255, 0, 0, next())),
            text_color: Some(Color::argb(255, 0, 0, next())),
            collapsed_text_color: Some(Color::argb(255, 0, 0, next())),
            ..ExpansionTileThemeData::default()
        }
    }

    #[test]
    fn expansion_tile_theme_data_blends_every_field_it_names() {
        assert_eq!(
            ExpansionTileThemeData::lerp(
                &numbered_expansion_tile_theme_data(0),
                &numbered_expansion_tile_theme_data(80),
                0.25
            ),
            numbered_expansion_tile_theme_data(20)
        );
        assert_eq!(
            ExpansionTileThemeData::lerp(
                &numbered_expansion_tile_theme_data(80),
                &numbered_expansion_tile_theme_data(0),
                0.25
            ),
            numbered_expansion_tile_theme_data(60)
        );
    }

    fn numbered_scrollbar_theme_data(base: u8) -> ScrollbarThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        ScrollbarThemeData {
            cross_axis_margin: Some(f32::from(next())),
            main_axis_margin: Some(f32::from(next())),
            min_thumb_length: Some(f32::from(next())),
            ..ScrollbarThemeData::default()
        }
    }

    #[test]
    fn scrollbar_theme_data_blends_every_field_it_names() {
        assert_eq!(
            ScrollbarThemeData::lerp(
                &numbered_scrollbar_theme_data(0),
                &numbered_scrollbar_theme_data(80),
                0.25
            ),
            numbered_scrollbar_theme_data(20)
        );
        assert_eq!(
            ScrollbarThemeData::lerp(
                &numbered_scrollbar_theme_data(80),
                &numbered_scrollbar_theme_data(0),
                0.25
            ),
            numbered_scrollbar_theme_data(60)
        );
    }

    fn numbered_floating_action_button_theme_data(base: u8) -> FloatingActionButtonThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        FloatingActionButtonThemeData {
            foreground_color: Some(Color::argb(255, 0, 0, next())),
            background_color: Some(Color::argb(255, 0, 0, next())),
            focus_color: Some(Color::argb(255, 0, 0, next())),
            hover_color: Some(Color::argb(255, 0, 0, next())),
            splash_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            focus_elevation: Some(f32::from(next())),
            hover_elevation: Some(f32::from(next())),
            disabled_elevation: Some(f32::from(next())),
            highlight_elevation: Some(f32::from(next())),
            icon_size: Some(f32::from(next())),
            ..FloatingActionButtonThemeData::default()
        }
    }

    #[test]
    fn floating_action_button_theme_data_blends_every_field_it_names() {
        assert_eq!(
            FloatingActionButtonThemeData::lerp(
                &numbered_floating_action_button_theme_data(0),
                &numbered_floating_action_button_theme_data(80),
                0.25
            ),
            numbered_floating_action_button_theme_data(20)
        );
        assert_eq!(
            FloatingActionButtonThemeData::lerp(
                &numbered_floating_action_button_theme_data(80),
                &numbered_floating_action_button_theme_data(0),
                0.25
            ),
            numbered_floating_action_button_theme_data(60)
        );
    }

    fn numbered_toggle_buttons_theme_data(base: u8) -> ToggleButtonsThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        ToggleButtonsThemeData {
            color: Some(Color::argb(255, 0, 0, next())),
            selected_color: Some(Color::argb(255, 0, 0, next())),
            disabled_color: Some(Color::argb(255, 0, 0, next())),
            fill_color: Some(Color::argb(255, 0, 0, next())),
            focus_color: Some(Color::argb(255, 0, 0, next())),
            highlight_color: Some(Color::argb(255, 0, 0, next())),
            splash_color: Some(Color::argb(255, 0, 0, next())),
            hover_color: Some(Color::argb(255, 0, 0, next())),
            border_color: Some(Color::argb(255, 0, 0, next())),
            selected_border_color: Some(Color::argb(255, 0, 0, next())),
            disabled_border_color: Some(Color::argb(255, 0, 0, next())),
            border_width: Some(f32::from(next())),
            ..ToggleButtonsThemeData::default()
        }
    }

    #[test]
    fn toggle_buttons_theme_data_blends_every_field_it_names() {
        assert_eq!(
            ToggleButtonsThemeData::lerp(
                &numbered_toggle_buttons_theme_data(0),
                &numbered_toggle_buttons_theme_data(80),
                0.25
            ),
            numbered_toggle_buttons_theme_data(20)
        );
        assert_eq!(
            ToggleButtonsThemeData::lerp(
                &numbered_toggle_buttons_theme_data(80),
                &numbered_toggle_buttons_theme_data(0),
                0.25
            ),
            numbered_toggle_buttons_theme_data(60)
        );
    }

    fn numbered_search_view_theme_data(base: u8) -> SearchViewThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        SearchViewThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            divider_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            header_height: Some(f32::from(next())),
            ..SearchViewThemeData::default()
        }
    }

    #[test]
    fn search_view_theme_data_blends_every_field_it_names() {
        assert_eq!(
            SearchViewThemeData::lerp(
                &numbered_search_view_theme_data(0),
                &numbered_search_view_theme_data(80),
                0.25
            ),
            numbered_search_view_theme_data(20)
        );
        assert_eq!(
            SearchViewThemeData::lerp(
                &numbered_search_view_theme_data(80),
                &numbered_search_view_theme_data(0),
                0.25
            ),
            numbered_search_view_theme_data(60)
        );
    }

    fn numbered_time_picker_theme_data(base: u8) -> TimePickerThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        TimePickerThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            day_period_color: Some(Color::argb(255, 0, 0, next())),
            day_period_text_color: Some(Color::argb(255, 0, 0, next())),
            dial_background_color: Some(Color::argb(255, 0, 0, next())),
            dial_hand_color: Some(Color::argb(255, 0, 0, next())),
            dial_text_color: Some(Color::argb(255, 0, 0, next())),
            entry_mode_icon_color: Some(Color::argb(255, 0, 0, next())),
            hour_minute_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            ..TimePickerThemeData::default()
        }
    }

    #[test]
    fn time_picker_theme_data_blends_every_field_it_names() {
        assert_eq!(
            TimePickerThemeData::lerp(
                &numbered_time_picker_theme_data(0),
                &numbered_time_picker_theme_data(80),
                0.25
            ),
            numbered_time_picker_theme_data(20)
        );
        assert_eq!(
            TimePickerThemeData::lerp(
                &numbered_time_picker_theme_data(80),
                &numbered_time_picker_theme_data(0),
                0.25
            ),
            numbered_time_picker_theme_data(60)
        );
    }

    fn numbered_date_picker_theme_data(base: u8) -> DatePickerThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        DatePickerThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            divider_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            range_picker_elevation: Some(f32::from(next())),
            ..DatePickerThemeData::default()
        }
    }

    #[test]
    fn date_picker_theme_data_blends_every_field_it_names() {
        assert_eq!(
            DatePickerThemeData::lerp(
                &numbered_date_picker_theme_data(0),
                &numbered_date_picker_theme_data(80),
                0.25
            ),
            numbered_date_picker_theme_data(20)
        );
        assert_eq!(
            DatePickerThemeData::lerp(
                &numbered_date_picker_theme_data(80),
                &numbered_date_picker_theme_data(0),
                0.25
            ),
            numbered_date_picker_theme_data(60)
        );
    }

    fn numbered_text_selection_theme_data(base: u8) -> TextSelectionThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        TextSelectionThemeData {
            cursor_color: Some(Color::argb(255, 0, 0, next())),
            selection_color: Some(Color::argb(255, 0, 0, next())),
            ..TextSelectionThemeData::default()
        }
    }

    #[test]
    fn text_selection_theme_data_blends_every_field_it_names() {
        assert_eq!(
            TextSelectionThemeData::lerp(
                &numbered_text_selection_theme_data(0),
                &numbered_text_selection_theme_data(80),
                0.25
            ),
            numbered_text_selection_theme_data(20)
        );
        assert_eq!(
            TextSelectionThemeData::lerp(
                &numbered_text_selection_theme_data(80),
                &numbered_text_selection_theme_data(0),
                0.25
            ),
            numbered_text_selection_theme_data(60)
        );
    }

    fn numbered_popup_menu_theme_data(base: u8) -> PopupMenuThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        PopupMenuThemeData {
            color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            icon_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            icon_size: Some(f32::from(next())),
            ..PopupMenuThemeData::default()
        }
    }

    #[test]
    fn popup_menu_theme_data_blends_every_field_it_names() {
        assert_eq!(
            PopupMenuThemeData::lerp(
                &numbered_popup_menu_theme_data(0),
                &numbered_popup_menu_theme_data(80),
                0.25
            ),
            numbered_popup_menu_theme_data(20)
        );
        assert_eq!(
            PopupMenuThemeData::lerp(
                &numbered_popup_menu_theme_data(80),
                &numbered_popup_menu_theme_data(0),
                0.25
            ),
            numbered_popup_menu_theme_data(60)
        );
    }

    fn numbered_bottom_app_bar_theme_data(base: u8) -> BottomAppBarThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        BottomAppBarThemeData {
            color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            height: Some(f32::from(next())),
            ..BottomAppBarThemeData::default()
        }
    }

    #[test]
    fn bottom_app_bar_theme_data_blends_every_field_it_names() {
        assert_eq!(
            BottomAppBarThemeData::lerp(
                &numbered_bottom_app_bar_theme_data(0),
                &numbered_bottom_app_bar_theme_data(80),
                0.25
            ),
            numbered_bottom_app_bar_theme_data(20)
        );
        assert_eq!(
            BottomAppBarThemeData::lerp(
                &numbered_bottom_app_bar_theme_data(80),
                &numbered_bottom_app_bar_theme_data(0),
                0.25
            ),
            numbered_bottom_app_bar_theme_data(60)
        );
    }

    fn numbered_navigation_bar_theme_data(base: u8) -> NavigationBarThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        NavigationBarThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            indicator_color: Some(Color::argb(255, 0, 0, next())),
            height: Some(f32::from(next())),
            elevation: Some(f32::from(next())),
            ..NavigationBarThemeData::default()
        }
    }

    #[test]
    fn navigation_bar_theme_data_blends_every_field_it_names() {
        assert_eq!(
            NavigationBarThemeData::lerp(
                &numbered_navigation_bar_theme_data(0),
                &numbered_navigation_bar_theme_data(80),
                0.25
            ),
            numbered_navigation_bar_theme_data(20)
        );
        assert_eq!(
            NavigationBarThemeData::lerp(
                &numbered_navigation_bar_theme_data(80),
                &numbered_navigation_bar_theme_data(0),
                0.25
            ),
            numbered_navigation_bar_theme_data(60)
        );
    }

    fn numbered_navigation_drawer_theme_data(base: u8) -> NavigationDrawerThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        NavigationDrawerThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            shadow_color: Some(Color::argb(255, 0, 0, next())),
            surface_tint_color: Some(Color::argb(255, 0, 0, next())),
            indicator_color: Some(Color::argb(255, 0, 0, next())),
            tile_height: Some(f32::from(next())),
            elevation: Some(f32::from(next())),
            ..NavigationDrawerThemeData::default()
        }
    }

    #[test]
    fn navigation_drawer_theme_data_blends_every_field_it_names() {
        assert_eq!(
            NavigationDrawerThemeData::lerp(
                &numbered_navigation_drawer_theme_data(0),
                &numbered_navigation_drawer_theme_data(80),
                0.25
            ),
            numbered_navigation_drawer_theme_data(20)
        );
        assert_eq!(
            NavigationDrawerThemeData::lerp(
                &numbered_navigation_drawer_theme_data(80),
                &numbered_navigation_drawer_theme_data(0),
                0.25
            ),
            numbered_navigation_drawer_theme_data(60)
        );
    }

    fn numbered_carousel_view_theme_data(base: u8) -> CarouselViewThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        CarouselViewThemeData {
            background_color: Some(Color::argb(255, 0, 0, next())),
            elevation: Some(f32::from(next())),
            ..CarouselViewThemeData::default()
        }
    }

    #[test]
    fn carousel_view_theme_data_blends_every_field_it_names() {
        assert_eq!(
            CarouselViewThemeData::lerp(
                &numbered_carousel_view_theme_data(0),
                &numbered_carousel_view_theme_data(80),
                0.25
            ),
            numbered_carousel_view_theme_data(20)
        );
        assert_eq!(
            CarouselViewThemeData::lerp(
                &numbered_carousel_view_theme_data(80),
                &numbered_carousel_view_theme_data(0),
                0.25
            ),
            numbered_carousel_view_theme_data(60)
        );
    }

    fn numbered_button_bar_theme_data(base: u8) -> ButtonBarThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        ButtonBarThemeData {
            button_min_width: Some(f32::from(next())),
            button_height: Some(f32::from(next())),
            ..ButtonBarThemeData::default()
        }
    }

    #[test]
    fn button_bar_theme_data_blends_every_field_it_names() {
        assert_eq!(
            ButtonBarThemeData::lerp(
                &numbered_button_bar_theme_data(0),
                &numbered_button_bar_theme_data(80),
                0.25
            ),
            numbered_button_bar_theme_data(20)
        );
        assert_eq!(
            ButtonBarThemeData::lerp(
                &numbered_button_bar_theme_data(80),
                &numbered_button_bar_theme_data(0),
                0.25
            ),
            numbered_button_bar_theme_data(60)
        );
    }

    // -- The same, for the themes that keep it all in state properties ------
    //
    // A whole-struct comparison cannot work here: `StateProperty` compares by
    // pointer, so two separately-built properties are never equal even when
    // they resolve to the same value. These resolve each field instead, at
    // the same states, and compare the numbers.

    fn state_numbered_checkbox_theme_data(base: u8) -> CheckboxThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        CheckboxThemeData {
            fill_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            check_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            overlay_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            ..CheckboxThemeData::default()
        }
    }

    #[test]
    fn checkbox_theme_data_blends_every_state_field_it_names() {
        let quarter = CheckboxThemeData::lerp(
            &state_numbered_checkbox_theme_data(0),
            &state_numbered_checkbox_theme_data(80),
            0.25,
        );
        assert_eq!(
            quarter
                .fill_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_checkbox_theme_data(20)
                .fill_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "fill_color"
        );
        assert_eq!(
            quarter
                .check_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_checkbox_theme_data(20)
                .check_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "check_color"
        );
        assert_eq!(
            quarter
                .overlay_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_checkbox_theme_data(20)
                .overlay_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "overlay_color"
        );
    }

    fn state_numbered_radio_theme_data(base: u8) -> RadioThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        RadioThemeData {
            fill_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            overlay_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            background_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            inner_radius: Some(StateProperty::all(Some(f32::from(next())))),
            ..RadioThemeData::default()
        }
    }

    #[test]
    fn radio_theme_data_blends_every_state_field_it_names() {
        let quarter = RadioThemeData::lerp(
            &state_numbered_radio_theme_data(0),
            &state_numbered_radio_theme_data(80),
            0.25,
        );
        assert_eq!(
            quarter
                .fill_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_radio_theme_data(20)
                .fill_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "fill_color"
        );
        assert_eq!(
            quarter
                .overlay_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_radio_theme_data(20)
                .overlay_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "overlay_color"
        );
        assert_eq!(
            quarter
                .background_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_radio_theme_data(20)
                .background_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "background_color"
        );
        assert_eq!(
            quarter
                .inner_radius
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_radio_theme_data(20)
                .inner_radius
                .unwrap()
                .resolve(WidgetStates::NONE),
            "inner_radius"
        );
    }

    fn state_numbered_switch_theme_data(base: u8) -> SwitchThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        SwitchThemeData {
            thumb_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            track_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            track_outline_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            overlay_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            track_outline_width: Some(StateProperty::all(Some(f32::from(next())))),
            ..SwitchThemeData::default()
        }
    }

    #[test]
    fn switch_theme_data_blends_every_state_field_it_names() {
        let quarter = SwitchThemeData::lerp(
            &state_numbered_switch_theme_data(0),
            &state_numbered_switch_theme_data(80),
            0.25,
        );
        assert_eq!(
            quarter
                .thumb_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_switch_theme_data(20)
                .thumb_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "thumb_color"
        );
        assert_eq!(
            quarter
                .track_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_switch_theme_data(20)
                .track_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "track_color"
        );
        assert_eq!(
            quarter
                .track_outline_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_switch_theme_data(20)
                .track_outline_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "track_outline_color"
        );
        assert_eq!(
            quarter
                .overlay_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_switch_theme_data(20)
                .overlay_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "overlay_color"
        );
        assert_eq!(
            quarter
                .track_outline_width
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_switch_theme_data(20)
                .track_outline_width
                .unwrap()
                .resolve(WidgetStates::NONE),
            "track_outline_width"
        );
    }

    fn state_numbered_data_table_theme_data(base: u8) -> DataTableThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        DataTableThemeData {
            data_row_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            heading_row_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            ..DataTableThemeData::default()
        }
    }

    #[test]
    fn data_table_theme_data_blends_every_state_field_it_names() {
        let quarter = DataTableThemeData::lerp(
            &state_numbered_data_table_theme_data(0),
            &state_numbered_data_table_theme_data(80),
            0.25,
        );
        assert_eq!(
            quarter
                .data_row_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_data_table_theme_data(20)
                .data_row_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "data_row_color"
        );
        assert_eq!(
            quarter
                .heading_row_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_data_table_theme_data(20)
                .heading_row_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "heading_row_color"
        );
    }

    fn state_numbered_button_style(base: u8) -> ButtonStyle {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        ButtonStyle {
            background_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            foreground_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            overlay_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            shadow_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            surface_tint_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            icon_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            elevation: Some(StateProperty::all(Some(f32::from(next())))),
            icon_size: Some(StateProperty::all(Some(f32::from(next())))),
            ..ButtonStyle::default()
        }
    }

    #[test]
    fn button_style_blends_every_state_field_it_names() {
        let quarter = ButtonStyle::lerp(
            &state_numbered_button_style(0),
            &state_numbered_button_style(80),
            0.25,
        );
        assert_eq!(
            quarter
                .background_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_button_style(20)
                .background_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "background_color"
        );
        assert_eq!(
            quarter
                .foreground_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_button_style(20)
                .foreground_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "foreground_color"
        );
        assert_eq!(
            quarter
                .overlay_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_button_style(20)
                .overlay_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "overlay_color"
        );
        assert_eq!(
            quarter
                .shadow_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_button_style(20)
                .shadow_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "shadow_color"
        );
        assert_eq!(
            quarter
                .surface_tint_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_button_style(20)
                .surface_tint_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "surface_tint_color"
        );
        assert_eq!(
            quarter
                .icon_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_button_style(20)
                .icon_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "icon_color"
        );
        assert_eq!(
            quarter
                .elevation
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_button_style(20)
                .elevation
                .unwrap()
                .resolve(WidgetStates::NONE),
            "elevation"
        );
        assert_eq!(
            quarter
                .icon_size
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_button_style(20)
                .icon_size
                .unwrap()
                .resolve(WidgetStates::NONE),
            "icon_size"
        );
    }

    fn state_numbered_scrollbar_theme_data(base: u8) -> ScrollbarThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        ScrollbarThemeData {
            thumb_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            track_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            track_border_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            thickness: Some(StateProperty::all(Some(f32::from(next())))),
            ..ScrollbarThemeData::default()
        }
    }

    #[test]
    fn scrollbar_theme_data_blends_every_state_field_it_names() {
        let quarter = ScrollbarThemeData::lerp(
            &state_numbered_scrollbar_theme_data(0),
            &state_numbered_scrollbar_theme_data(80),
            0.25,
        );
        assert_eq!(
            quarter
                .thumb_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_scrollbar_theme_data(20)
                .thumb_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "thumb_color"
        );
        assert_eq!(
            quarter
                .track_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_scrollbar_theme_data(20)
                .track_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "track_color"
        );
        assert_eq!(
            quarter
                .track_border_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_scrollbar_theme_data(20)
                .track_border_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "track_border_color"
        );
        assert_eq!(
            quarter
                .thickness
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_scrollbar_theme_data(20)
                .thickness
                .unwrap()
                .resolve(WidgetStates::NONE),
            "thickness"
        );
    }

    fn state_numbered_menu_style(base: u8) -> MenuStyle {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        MenuStyle {
            background_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            shadow_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            surface_tint_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            elevation: Some(StateProperty::all(Some(f32::from(next())))),
            ..MenuStyle::default()
        }
    }

    #[test]
    fn menu_style_blends_every_state_field_it_names() {
        let quarter = MenuStyle::lerp(
            &state_numbered_menu_style(0),
            &state_numbered_menu_style(80),
            0.25,
        );
        assert_eq!(
            quarter
                .background_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_menu_style(20)
                .background_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "background_color"
        );
        assert_eq!(
            quarter
                .shadow_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_menu_style(20)
                .shadow_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "shadow_color"
        );
        assert_eq!(
            quarter
                .surface_tint_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_menu_style(20)
                .surface_tint_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "surface_tint_color"
        );
        assert_eq!(
            quarter
                .elevation
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_menu_style(20)
                .elevation
                .unwrap()
                .resolve(WidgetStates::NONE),
            "elevation"
        );
    }

    fn state_numbered_search_bar_theme_data(base: u8) -> SearchBarThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        SearchBarThemeData {
            background_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            shadow_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            surface_tint_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            overlay_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            elevation: Some(StateProperty::all(Some(f32::from(next())))),
            ..SearchBarThemeData::default()
        }
    }

    #[test]
    fn search_bar_theme_data_blends_every_state_field_it_names() {
        let quarter = SearchBarThemeData::lerp(
            &state_numbered_search_bar_theme_data(0),
            &state_numbered_search_bar_theme_data(80),
            0.25,
        );
        assert_eq!(
            quarter
                .background_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_search_bar_theme_data(20)
                .background_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "background_color"
        );
        assert_eq!(
            quarter
                .shadow_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_search_bar_theme_data(20)
                .shadow_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "shadow_color"
        );
        assert_eq!(
            quarter
                .surface_tint_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_search_bar_theme_data(20)
                .surface_tint_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "surface_tint_color"
        );
        assert_eq!(
            quarter
                .overlay_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_search_bar_theme_data(20)
                .overlay_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "overlay_color"
        );
        assert_eq!(
            quarter
                .elevation
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_search_bar_theme_data(20)
                .elevation
                .unwrap()
                .resolve(WidgetStates::NONE),
            "elevation"
        );
    }

    fn state_numbered_date_picker_theme_data(base: u8) -> DatePickerThemeData {
        let mut n = 0;
        let mut next = || {
            n += 1;
            base + n
        };
        DatePickerThemeData {
            day_foreground_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            day_background_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            day_overlay_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            today_foreground_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            today_background_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            year_foreground_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            year_background_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            year_overlay_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, next())))),
            range_selection_overlay_color: Some(StateProperty::all(Some(Color::argb(
                255,
                0,
                0,
                next(),
            )))),
            ..DatePickerThemeData::default()
        }
    }

    #[test]
    fn date_picker_theme_data_blends_every_state_field_it_names() {
        let quarter = DatePickerThemeData::lerp(
            &state_numbered_date_picker_theme_data(0),
            &state_numbered_date_picker_theme_data(80),
            0.25,
        );
        assert_eq!(
            quarter
                .day_foreground_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_date_picker_theme_data(20)
                .day_foreground_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "day_foreground_color"
        );
        assert_eq!(
            quarter
                .day_background_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_date_picker_theme_data(20)
                .day_background_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "day_background_color"
        );
        assert_eq!(
            quarter
                .day_overlay_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_date_picker_theme_data(20)
                .day_overlay_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "day_overlay_color"
        );
        assert_eq!(
            quarter
                .today_foreground_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_date_picker_theme_data(20)
                .today_foreground_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "today_foreground_color"
        );
        assert_eq!(
            quarter
                .today_background_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_date_picker_theme_data(20)
                .today_background_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "today_background_color"
        );
        assert_eq!(
            quarter
                .year_foreground_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_date_picker_theme_data(20)
                .year_foreground_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "year_foreground_color"
        );
        assert_eq!(
            quarter
                .year_background_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_date_picker_theme_data(20)
                .year_background_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "year_background_color"
        );
        assert_eq!(
            quarter
                .year_overlay_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_date_picker_theme_data(20)
                .year_overlay_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "year_overlay_color"
        );
        assert_eq!(
            quarter
                .range_selection_overlay_color
                .expect("two ends is enough")
                .resolve(WidgetStates::NONE),
            state_numbered_date_picker_theme_data(20)
                .range_selection_overlay_color
                .unwrap()
                .resolve(WidgetStates::NONE),
            "range_selection_overlay_color"
        );
    }

    // -- Three differences from upstream, tick 227 --------------------------

    #[test]
    fn an_icon_themes_shadows_blend_rather_than_stepping() {
        // Upstream is `Shadow.lerpList`, which scales the excess items on
        // whichever side has more rather than swapping the whole list at the
        // midpoint. This port had `lerp_nearer`.
        let shadow = |blur: f32| crate::painting::BoxShadow {
            color: Color::argb(255, 0, 0, 0),
            offset: crate::render::Offset::new(0.0, 0.0),
            blur_radius: blur,
            spread_radius: 0.0,
        };
        let a = IconThemeData::new().with_size(4.0);
        let a = IconThemeData {
            shadows: Some(vec![shadow(4.0)]),
            ..a
        };
        let b = IconThemeData {
            shadows: Some(vec![shadow(20.0)]),
            ..IconThemeData::new().with_size(4.0)
        };
        let quarter = IconThemeData::lerp(&a, &b, 0.25);
        assert_eq!(
            quarter.shadows.as_ref().map(|s| s[0].blur_radius),
            Some(8.0)
        );
        let back = IconThemeData::lerp(&b, &a, 0.25);
        assert_eq!(back.shadows.as_ref().map(|s| s[0].blur_radius), Some(16.0));

        // And a second shadow only one end has fades in rather than
        // appearing at the midpoint: that is what `lerpList` is for.
        let two = IconThemeData {
            shadows: Some(vec![shadow(4.0), shadow(40.0)]),
            ..IconThemeData::new().with_size(4.0)
        };
        let grown = IconThemeData::lerp(&a, &two, 0.25);
        assert_eq!(
            grown.shadows.as_ref().map(|s| s.len()),
            Some(2),
            "the second shadow is present from the first frame"
        );
    }

    #[test]
    fn the_five_fields_upstreams_tooltip_lerp_drops_are_carried_here() {
        // `TooltipThemeData.lerp` assigns ten fields and leaves
        // `waitDuration`, `showDuration`, `exitDuration`, `triggerMode` and
        // `enableFeedback` unset, so upstream's blended theme loses them and
        // each tooltip falls back to its own default. This port carries them
        // at the nearer end. The difference is deliberate; this pins it so it
        // stays a choice rather than becoming an accident.
        let a = TooltipThemeData {
            wait_duration: Some(std::time::Duration::from_millis(100)),
            show_duration: Some(std::time::Duration::from_millis(200)),
            exit_duration: Some(std::time::Duration::from_millis(300)),
            enable_feedback: Some(true),
            ..TooltipThemeData::default()
        };
        let b = TooltipThemeData {
            wait_duration: Some(std::time::Duration::from_millis(400)),
            show_duration: Some(std::time::Duration::from_millis(500)),
            exit_duration: Some(std::time::Duration::from_millis(600)),
            enable_feedback: Some(false),
            ..TooltipThemeData::default()
        };
        // Upstream would answer `None` for every one of these.
        let early = TooltipThemeData::lerp(&a, &b, 0.25);
        assert_eq!(
            early.wait_duration,
            Some(std::time::Duration::from_millis(100))
        );
        assert_eq!(
            early.show_duration,
            Some(std::time::Duration::from_millis(200))
        );
        assert_eq!(
            early.exit_duration,
            Some(std::time::Duration::from_millis(300))
        );
        assert_eq!(early.enable_feedback, Some(true));

        let late = TooltipThemeData::lerp(&a, &b, 0.75);
        assert_eq!(
            late.wait_duration,
            Some(std::time::Duration::from_millis(400))
        );
        assert_eq!(late.enable_feedback, Some(false));
    }

    #[test]
    fn the_close_icon_upstreams_snack_bar_lerp_drops_is_carried_here() {
        // The same difference, one field, in `SnackBarThemeData.lerp`.
        let a = SnackBarThemeData {
            show_close_icon: Some(true),
            ..SnackBarThemeData::default()
        };
        let b = SnackBarThemeData {
            show_close_icon: Some(false),
            ..SnackBarThemeData::default()
        };
        assert_eq!(
            SnackBarThemeData::lerp(&a, &b, 0.25).show_close_icon,
            Some(true)
        );
        assert_eq!(
            SnackBarThemeData::lerp(&a, &b, 0.75).show_close_icon,
            Some(false)
        );
    }

    #[test]
    fn the_resolved_divider_carries_the_themes_radius() {
        // The painter test in `components.rs` can only see that a rounding
        // happened, because a rounded fill reaches the engine as a path and
        // the stub records a path by its bounding box. This is the other
        // half: the number that arrives is the theme's, and an unset field
        // stays unset rather than acquiring a default of its own -- upstream's
        // two default tables set no radius.
        let themed = |radius: Option<f32>| {
            read_in(
                move |child| {
                    MaterialTheme::new(
                        ThemeData::light(),
                        DividerTheme::new(
                            DividerThemeData {
                                radius: radius.map(|r| {
                                    crate::borders::BorderRadiusGeometry::Absolute(
                                        crate::borders::BorderRadius::circular(r),
                                    )
                                }),
                                ..DividerThemeData::default()
                            },
                            child,
                        ),
                    )
                },
                |context| ResolvedDivider::of(context).radius,
            )
        };
        assert_eq!(
            themed(Some(6.0)),
            Some(crate::borders::BorderRadiusGeometry::Absolute(
                crate::borders::BorderRadius::circular(6.0)
            ))
        );
        assert_eq!(themed(None), None);
    }

    // -- Seven chip theme fields that reached nothing, tick 234 -------------
    //
    // `ResolvedChip` carried a fill, a side and a padding. `checkmarkColor`,
    // `deleteIconColor`, `selectedShadowColor`, `avatarBoxConstraints` and
    // `deleteIconBoxConstraints` reached nothing at all, and `ChoiceChip`'s
    // `secondarySelectedColor` and `secondaryLabelStyle` had no resolution
    // step to reach.
    //
    // Every value below is a number no other line in the test uses.

    fn chip_under<T: 'static>(
        data: ChipThemeData,
        read: impl Fn(&mut BuildContext) -> T + 'static,
    ) -> T {
        read_in(move |child| ChipTheme::new(data.clone(), child), read)
    }

    #[test]
    fn the_five_plain_chip_fields_arrive_from_the_theme() {
        use crate::widget_state::WidgetStates;
        let fill = Color::argb(255, 9, 9, 9);
        let data = ChipThemeData {
            checkmark_color: Some(Color::argb(255, 0, 0, 10)),
            delete_icon_color: Some(Color::argb(255, 0, 0, 20)),
            selected_shadow_color: Some(Color::argb(255, 0, 0, 30)),
            avatar_box_constraints: Some(BoxConstraints::tight_for(crate::render::Size::new(
                40.0, 40.0,
            ))),
            delete_icon_box_constraints: Some(BoxConstraints::tight_for(crate::render::Size::new(
                50.0, 50.0,
            ))),
            ..ChipThemeData::new()
        };
        let resolved = chip_under(data, move |context| {
            ResolvedChip::of(context, WidgetStates::NONE, fill)
        });
        assert_eq!(resolved.checkmark_color, Some(Color::argb(255, 0, 0, 10)));
        assert_eq!(resolved.delete_icon_color, Some(Color::argb(255, 0, 0, 20)));
        assert_eq!(
            resolved.selected_shadow_color,
            Some(Color::argb(255, 0, 0, 30))
        );
        assert_eq!(
            resolved.avatar_box_constraints.map(|c| c.max_width),
            Some(40.0)
        );
        assert_eq!(
            resolved.delete_icon_box_constraints.map(|c| c.max_width),
            Some(50.0)
        );
    }

    #[test]
    fn a_checkmark_with_no_colour_stays_without_one() {
        // Upstream's M3 default for `checkmarkColor` is **null**, which means
        // "whatever the label is written in" rather than a colour of its own.
        // `None` here is an answer, not a gap.
        use crate::widget_state::WidgetStates;
        let fill = Color::argb(255, 9, 9, 9);
        let resolved = chip_under(ChipThemeData::new(), move |context| {
            ResolvedChip::of(context, WidgetStates::NONE, fill)
        });
        assert_eq!(resolved.checkmark_color, None);
        // And neither box constraint acquires one: upstream has no default
        // for those either -- the layout falls back to a square of the
        // content's size, which is a layout rule and not a theme value.
        assert_eq!(resolved.avatar_box_constraints, None);
        assert_eq!(resolved.delete_icon_box_constraints, None);
    }

    #[test]
    fn the_delete_icon_falls_through_the_icon_theme_before_the_default() {
        // Upstream's chain reaches `chipTheme.iconTheme?.color` before the
        // defaults, so a theme that set only an icon colour still colours the
        // delete cross.
        use crate::widget_state::WidgetStates;
        let fill = Color::argb(255, 9, 9, 9);
        let through_icons = chip_under(
            ChipThemeData {
                icon_theme: Some(IconThemeData::new().with_color(Color::argb(255, 0, 0, 60))),
                ..ChipThemeData::new()
            },
            move |context| ResolvedChip::of(context, WidgetStates::NONE, fill).delete_icon_color,
        );
        assert_eq!(through_icons, Some(Color::argb(255, 0, 0, 60)));

        // Its own field still wins over that.
        let named = chip_under(
            ChipThemeData {
                delete_icon_color: Some(Color::argb(255, 0, 0, 70)),
                icon_theme: Some(IconThemeData::new().with_color(Color::argb(255, 0, 0, 60))),
                ..ChipThemeData::new()
            },
            move |context| ResolvedChip::of(context, WidgetStates::NONE, fill).delete_icon_color,
        );
        assert_eq!(named, Some(Color::argb(255, 0, 0, 70)));
    }

    #[test]
    fn a_chosen_choice_chip_asks_the_secondary_slot_first() {
        // Upstream: `selectedColor ?? chipTheme.secondarySelectedColor`, and a
        // filter chip has no such slot to ask. The two differ because a
        // filter chip's selection is a toggle and a choice chip's is a pick.
        use crate::widget_state::{WidgetState, WidgetStates};
        let selected = WidgetStates::NONE.with(WidgetState::Selected);
        let fill = Color::argb(255, 9, 9, 9);
        let data = ChipThemeData {
            selected_color: Some(Color::argb(255, 0, 0, 80)),
            secondary_selected_color: Some(Color::argb(255, 0, 0, 90)),
            ..ChipThemeData::new()
        };
        assert_eq!(
            chip_under(data.clone(), move |context| ResolvedChip::of_choice(
                context, selected, fill
            )
            .fill),
            Color::argb(255, 0, 0, 90),
            "the chosen one takes the secondary colour"
        );
        assert_eq!(
            chip_under(data, move |context| ResolvedChip::of(
                context, selected, fill
            )
            .fill),
            Color::argb(255, 0, 0, 80),
            "and every other chip takes the ordinary one"
        );
    }

    #[test]
    fn and_the_secondary_label_style_with_it() {
        use crate::widget_state::{WidgetState, WidgetStates};
        let selected = WidgetStates::NONE.with(WidgetState::Selected);
        let none = WidgetStates::NONE;
        let fill = Color::argb(255, 9, 9, 9);
        let data = ChipThemeData {
            label_style: Some(TextStyle {
                font_size: 11.0,
                ..TextStyle::default()
            }),
            secondary_label_style: Some(TextStyle {
                font_size: 22.0,
                ..TextStyle::default()
            }),
            ..ChipThemeData::new()
        };
        let size = |resolved: ResolvedChip| resolved.label_style.map(|style| style.font_size);
        assert_eq!(
            size(chip_under(data.clone(), move |context| {
                ResolvedChip::of_choice(context, selected, fill)
            })),
            Some(22.0),
            "chosen"
        );
        assert_eq!(
            size(chip_under(data.clone(), move |context| {
                ResolvedChip::of_choice(context, none, fill)
            })),
            Some(11.0),
            "not chosen -- the ordinary style"
        );
        assert_eq!(
            size(chip_under(data, move |context| {
                ResolvedChip::of(context, selected, fill)
            })),
            Some(11.0),
            "and a selected filter chip never asks for the secondary one"
        );
    }

    // -- Four app bar theme fields that reached nothing, tick 235 -----------
    //
    // `ResolvedAppBar` carried a background, a foreground, a height, the
    // centring rule and the title spacing. `scrolledUnderElevation`,
    // `actionsIconTheme`, `leadingWidth` and `toolbarTextStyle` reached
    // nothing.
    //
    // Every value below is a number no other line in the test uses.

    fn app_bar_under<T: 'static>(
        data: AppBarThemeData,
        read: impl Fn(&mut BuildContext) -> T + 'static,
    ) -> T {
        read_in(move |child| AppBarTheme::new(data.clone(), child), read)
    }

    #[test]
    fn the_four_app_bar_fields_arrive_from_the_theme() {
        let data = AppBarThemeData {
            scrolled_under_elevation: Some(11.0),
            leading_width: Some(22.0),
            toolbar_text_style: Some(TextStyle {
                font_size: 33.0,
                ..TextStyle::default()
            }),
            actions_icon_theme: Some(IconThemeData::new().with_size(44.0)),
            ..AppBarThemeData::new()
        };
        let resolved = app_bar_under(data, ResolvedAppBar::of);
        assert_eq!(resolved.scrolled_under_elevation, 11.0);
        assert_eq!(resolved.leading_width, 22.0);
        assert_eq!(
            resolved.toolbar_text_style.map(|style| style.font_size),
            Some(33.0)
        );
        assert_eq!(resolved.actions_icon_theme.size, Some(44.0));
    }

    #[test]
    fn the_actions_fall_through_the_bars_own_icon_theme_first() {
        // Upstream's chain reaches `appBarTheme.iconTheme` before the
        // defaults, so a theme that set only `iconTheme` colours the actions
        // too -- the trailing icons follow the leading one unless told
        // otherwise.
        let through = app_bar_under(
            AppBarThemeData {
                icon_theme: Some(IconThemeData::new().with_size(55.0)),
                ..AppBarThemeData::new()
            },
            ResolvedAppBar::of,
        );
        assert_eq!(through.actions_icon_theme.size, Some(55.0));

        // And the actions' own field still wins over that.
        let named = app_bar_under(
            AppBarThemeData {
                icon_theme: Some(IconThemeData::new().with_size(55.0)),
                actions_icon_theme: Some(IconThemeData::new().with_size(66.0)),
                ..AppBarThemeData::new()
            },
            ResolvedAppBar::of,
        );
        assert_eq!(named.actions_icon_theme.size, Some(66.0));
    }

    // -- The title bar's title, tick 250 -------------------------------------
    //
    // `ResolvedAppBar` resolved the toolbar's text style and had no field for
    // the title's, so `AppBar` drew its title with a hand-rolled style
    // carrying a hard-coded weight of 700. `titleLarge` is 400, and it is the
    // role upstream's two defaults tables both name. Neither
    // `AppBarThemeData::title_text_style` nor `TextTheme::title_large` had a
    // reader here.
    //
    // Changing what the bar draws its title with broke nothing, which is the
    // other half of the finding: nothing was watching.

    #[test]
    fn a_bars_title_is_the_type_scales_title_large_and_not_a_bold_one() {
        // The role, character for character, except for the ink.
        let theme = ThemeData::light();
        let role = theme.text_theme.title_large.clone().expect("a role");
        let bar = app_bar_under(AppBarThemeData::new(), ResolvedAppBar::of);
        let title = bar.title_text_style.clone().expect("a title style");

        assert_eq!(title.font_size, role.font_size);
        assert_eq!(title.font_weight, role.font_weight);
        assert_ne!(
            title.font_weight, 700,
            "which is what the bar used to hard-code, and titleLarge is 400"
        );

        // And it is a different role from the toolbar's, which is bodyMedium:
        // a bar's title is not the same size as the words beside it.
        let toolbar = bar.toolbar_text_style.clone().expect("a toolbar style");
        assert_ne!(title.font_size, toolbar.font_size);
    }

    #[test]
    fn both_styles_take_the_bars_foreground_colour_and_not_the_roles() {
        // Upstream's `defaults.titleTextStyle?.copyWith(color: foregroundColor)`
        // -- the role brings the size, the weight and the family, and the bar
        // brings the ink, because a foreground colour belongs to the bar and
        // not to the type scale. `toolbar_text_style` was resolved here
        // without that merge, so a bar with a foreground colour drew its
        // non-title text in the wrong one.
        const MINE: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
        let theme = ThemeData::light();
        let bar = app_bar_under(
            AppBarThemeData {
                foreground_color: Some(MINE),
                ..AppBarThemeData::new()
            },
            ResolvedAppBar::of,
        );
        assert_eq!(bar.title_text_style.clone().expect("a style").color, MINE);
        assert_eq!(bar.toolbar_text_style.clone().expect("a style").color, MINE);
        assert_ne!(
            theme.text_theme.title_large.clone().expect("a role").color,
            MINE,
            "which the role does not carry, so this says the merge happened"
        );
    }

    #[test]
    fn a_style_the_theme_names_outright_keeps_its_own_colour() {
        // The merge is on the *defaults*, not on whatever the theme said. A
        // caller who named a style has already decided its ink; putting the
        // bar's foreground over it would make the field unusable for the one
        // thing it is for.
        const MINE: Color = Color::argb(0xFF, 0x44, 0x55, 0x66);
        const FOREGROUND: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
        let asked = TextStyle {
            color: MINE,
            font_size: 41.0,
            ..TextStyle::default()
        };
        let bar = app_bar_under(
            AppBarThemeData {
                foreground_color: Some(FOREGROUND),
                title_text_style: Some(asked.clone()),
                ..AppBarThemeData::new()
            },
            ResolvedAppBar::of,
        );
        assert_eq!(bar.title_text_style, Some(asked));
    }

    #[test]
    fn the_leading_slot_is_square_by_default() {
        // Upstream's `_kLeadingWidth` is `kToolbarHeight`, with the comment
        // "So the leading button is square". The number is the height for a
        // reason, so this checks the *relationship* and not a literal 56.
        let plain = read_in(|child| child, ResolvedAppBar::of);
        assert_eq!(plain.leading_width, plain.toolbar_height);
        assert_eq!(plain.leading_width, ResolvedAppBar::TOOLBAR_HEIGHT);
    }

    #[test]
    fn scrolling_under_lifts_the_bar_by_a_different_number() {
        // Material 3 lifts a bar off the content it is covering rather than
        // keeping it flat, so the two elevations are two numbers and a
        // resolver that answered one for both would be wrong in a way nothing
        // else here would notice.
        let plain = read_in(|child| child, ResolvedAppBar::of);
        assert_eq!(
            plain.scrolled_under_elevation,
            ResolvedAppBar::SCROLLED_UNDER_ELEVATION
        );
        assert_ne!(
            ResolvedAppBar::SCROLLED_UNDER_ELEVATION,
            0.0,
            "the resting elevation is zero under Material 3, so an equal pair \
             would mean the bar never lifts"
        );
    }

    #[test]
    fn the_toolbar_style_ends_at_the_typographys_body_medium() {
        // Upstream's last step is `textTheme.bodyMedium`. Without this the
        // fallback could be dropped entirely and the test above -- which only
        // asks what the theme's own value resolves to -- would stay green.
        let plain = read_in(|child| child, ResolvedAppBar::of);
        assert_eq!(
            plain.toolbar_text_style,
            crate::theme::ThemeData::light().text_theme.body_medium,
            "and it is not nothing"
        );
        assert!(plain.toolbar_text_style.is_some());
    }

    // -- Seven small wires, four resolvers, tick 236 ------------------------
    //
    // Every value below is a number no other line in the test uses.

    #[test]
    fn a_tiles_three_text_styles_are_three_different_roles() {
        // Upstream's M3 defaults are `bodyLarge`, `bodyMedium` and
        // `labelSmall` -- a tile whose three styles were one style would look
        // wrong in a way no single number shows, so this checks that the
        // three defaults differ as well as that each field arrives.
        let themed = read_in(
            |child| {
                ListTileTheme::new(
                    ListTileThemeData {
                        title_text_style: Some(TextStyle {
                            font_size: 11.0,
                            ..TextStyle::default()
                        }),
                        subtitle_text_style: Some(TextStyle {
                            font_size: 22.0,
                            ..TextStyle::default()
                        }),
                        leading_and_trailing_text_style: Some(TextStyle {
                            font_size: 33.0,
                            ..TextStyle::default()
                        }),
                        ..ListTileThemeData::default()
                    },
                    child,
                )
            },
            |context| ResolvedListTile::of(context, false, None),
        );
        assert_eq!(
            themed.title_text_style.map(|style| style.font_size),
            Some(11.0)
        );
        assert_eq!(
            themed.subtitle_text_style.map(|style| style.font_size),
            Some(22.0)
        );
        assert_eq!(
            themed
                .leading_and_trailing_text_style
                .map(|style| style.font_size),
            Some(33.0)
        );

        let plain = read_in(
            |child| child,
            |context| ResolvedListTile::of(context, false, None),
        );
        let typography = crate::theme::ThemeData::light().text_theme;
        assert_eq!(plain.title_text_style, typography.body_large);
        assert_eq!(plain.subtitle_text_style, typography.body_medium);
        assert_eq!(
            plain.leading_and_trailing_text_style,
            typography.label_small
        );
        assert_ne!(
            plain.title_text_style, plain.leading_and_trailing_text_style,
            "the three roles are three styles, so the assertions above say \
             something"
        );
    }

    #[test]
    fn a_progress_indicator_carries_its_cap_and_track_padding() {
        // `None` for the cap is an answer: upstream's own default is not one
        // value -- round for a spinner and for a linear track, butt for the
        // gapped Material 3 bar -- so it means "each painter's own".
        let plain = read_in(|child| child, ResolvedProgressIndicator::of);
        assert_eq!(plain.stroke_cap, None);
        assert_eq!(plain.circular_track_padding, None);

        let themed = read_in(
            |child| {
                ProgressIndicatorTheme::new(
                    ProgressIndicatorThemeData {
                        stroke_cap: Some(crate::painting::StrokeCap::Square),
                        circular_track_padding: Some(EdgeInsetsGeometry::Absolute(
                            crate::render::EdgeInsets::all(44.0),
                        )),
                        ..ProgressIndicatorThemeData::default()
                    },
                    child,
                )
            },
            ResolvedProgressIndicator::of,
        );
        assert_eq!(themed.stroke_cap, Some(crate::painting::StrokeCap::Square));
        assert_eq!(
            themed
                .circular_track_padding
                .map(|p| p.resolve(crate::direction::TextDirection::Ltr).left),
            Some(44.0)
        );
    }

    #[test]
    fn a_scrollbars_track_border_is_fainter_on_a_light_ground() {
        // Upstream's default is brightness-dependent: a tenth of the ink's
        // opacity under a light theme and a quarter under a dark one, because
        // a line that reads as faint on white disappears on black.
        use crate::widget_state::WidgetStates;
        let under = |theme: crate::theme::ThemeData| {
            read_in(
                move |child| {
                    crate::theme::MaterialTheme::new(
                        theme.clone(),
                        ScrollbarTheme::new(ScrollbarThemeData::default(), child),
                    )
                },
                |context| ResolvedScrollbar::of(context, WidgetStates::NONE).track_border_color,
            )
        };
        let light = under(crate::theme::ThemeData::light());
        let dark = under(crate::theme::ThemeData::dark());
        assert!(
            dark.alpha() > light.alpha(),
            "the dark theme's line is the stronger one: {} vs {}",
            dark.alpha(),
            light.alpha()
        );

        // And the theme's own field wins over both.
        let named = read_in(
            |child| {
                ScrollbarTheme::new(
                    ScrollbarThemeData {
                        track_border_color: Some(crate::widget_state::StateProperty::all(Some(
                            Color::argb(255, 0, 0, 55),
                        ))),
                        ..ScrollbarThemeData::default()
                    },
                    child,
                )
            },
            |context| ResolvedScrollbar::of(context, WidgetStates::NONE).track_border_color,
        );
        assert_eq!(named, Color::argb(255, 0, 0, 55));
    }

    #[test]
    fn a_tooltip_leaves_faster_than_it_stays() {
        // Upstream's `_defaultExitDuration` is 100ms against `showDuration`'s
        // 1500: a pointer that slid off is not the same event as a reader who
        // has finished reading, and it is not given the same grace.
        let plain = read_in(|child| child, ResolvedTooltip::of);
        assert_eq!(plain.exit_duration, ResolvedTooltip::EXIT_DURATION);
        assert!(
            plain.exit_duration < plain.show_duration,
            "{:?} against {:?}",
            plain.exit_duration,
            plain.show_duration
        );

        let themed = read_in(
            |child| {
                TooltipTheme::new(
                    TooltipThemeData {
                        exit_duration: Some(std::time::Duration::from_millis(66)),
                        ..TooltipThemeData::default()
                    },
                    child,
                )
            },
            ResolvedTooltip::of,
        );
        assert_eq!(themed.exit_duration, std::time::Duration::from_millis(66));
    }

    // -- Four tab bar theme fields that reached nothing, tick 238 -----------
    //
    // Two of the four have defaults that depend on something the *bar* knows
    // and the theme does not, so the resolver has to be told: a scrolling bar
    // aligns differently from one that fills, and the indicator's animation
    // follows the indicator's size.

    fn tabs_under<T: 'static>(
        data: TabBarThemeData,
        material: crate::theme::ThemeData,
        scrollable: bool,
        read: impl Fn(ResolvedTabBar) -> T + 'static,
    ) -> T {
        read_in(
            move |child| {
                crate::theme::MaterialTheme::new(
                    material.clone(),
                    TabBarTheme::new(data.clone(), child),
                )
            },
            move |context| read(ResolvedTabBar::of_bar(context, scrollable)),
        )
    }

    #[test]
    fn where_the_tabs_sit_depends_on_scrolling_and_on_the_material_version() {
        let alignment = |scrollable, material3| {
            tabs_under(
                TabBarThemeData::default(),
                crate::theme::ThemeData {
                    use_material3: material3,
                    ..crate::theme::ThemeData::light()
                },
                scrollable,
                |resolved| resolved.tab_alignment,
            )
        };
        // A bar that does not scroll fills, whichever version it is.
        assert_eq!(alignment(false, true), TabAlignment::Fill);
        assert_eq!(alignment(false, false), TabAlignment::Fill);
        // A scrolling one starts, with the offset Material 3 asks for and
        // Material 2 does not -- three answers from one field.
        assert_eq!(alignment(true, true), TabAlignment::StartOffset);
        assert_eq!(alignment(true, false), TabAlignment::Start);

        // And the theme's own value beats all four.
        assert_eq!(
            tabs_under(
                TabBarThemeData {
                    tab_alignment: Some(TabAlignment::Center),
                    ..TabBarThemeData::default()
                },
                crate::theme::ThemeData::light(),
                true,
                |resolved| resolved.tab_alignment,
            ),
            TabAlignment::Center
        );
    }

    #[test]
    fn the_indicators_animation_follows_the_indicators_size() {
        // An indicator as wide as its label stretches and settles; one as
        // wide as the whole tab slides. The elastic feel reads as the
        // underline reaching for the next word, which only makes sense when
        // it is word-shaped.
        let animation = |size| {
            tabs_under(
                TabBarThemeData {
                    indicator_size: Some(size),
                    ..TabBarThemeData::default()
                },
                crate::theme::ThemeData::light(),
                false,
                |resolved| resolved.indicator_animation,
            )
        };
        assert_eq!(
            animation(TabBarIndicatorSize::Label),
            TabIndicatorAnimation::Elastic
        );
        assert_eq!(
            animation(TabBarIndicatorSize::Tab),
            TabIndicatorAnimation::Linear
        );

        // And the theme's own value beats the size-derived one.
        assert_eq!(
            tabs_under(
                TabBarThemeData {
                    indicator_size: Some(TabBarIndicatorSize::Label),
                    indicator_animation: Some(TabIndicatorAnimation::Linear),
                    ..TabBarThemeData::default()
                },
                crate::theme::ThemeData::light(),
                false,
                |resolved| resolved.indicator_animation,
            ),
            TabIndicatorAnimation::Linear
        );
    }

    #[test]
    fn the_indicator_and_the_text_scaler_have_no_defaults_to_invent() {
        // `None` is the answer for both: a null indicator means "draw the
        // underline from the colour and the weight", and a null scaler means
        // "leave the ambient one alone" -- which is what upstream passing it
        // into `MediaQuery.copyWith` amounts to.
        let plain = tabs_under(
            TabBarThemeData::default(),
            crate::theme::ThemeData::light(),
            false,
            |resolved| (resolved.indicator.is_some(), resolved.text_scaler),
        );
        assert_eq!(plain, (false, None));

        let themed = tabs_under(
            TabBarThemeData {
                indicator: Some(crate::decoration::Decoration::Box(
                    crate::decoration::BoxDecoration::new()
                        .with_fill(crate::render::Fill::Solid(Color::argb(255, 0, 0, 99))),
                )),
                text_scaler: Some(crate::painting::TextScaler::linear(2.0)),
                ..TabBarThemeData::default()
            },
            crate::theme::ThemeData::light(),
            false,
            |resolved| (resolved.indicator.clone(), resolved.text_scaler),
        );
        match themed.0 {
            Some(crate::decoration::Decoration::Box(box_decoration)) => {
                match &box_decoration.fill {
                    Some(crate::render::Fill::Solid(color)) => {
                        assert_eq!(*color, Color::argb(255, 0, 0, 99))
                    }
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(themed.1, Some(crate::painting::TextScaler::linear(2.0)));
    }

    // -- Seven small wires, tick 239 ----------------------------------------
    //
    // Every value below is a number no other line in the test uses.

    #[test]
    fn a_data_tables_four_state_properties_resolve_against_the_states() {
        use crate::widget_state::{StateProperty, WidgetState, WidgetStates};
        let hovered = WidgetStates::NONE.with(WidgetState::Hovered);
        let data = DataTableThemeData {
            data_row_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, 11)))),
            heading_row_color: Some(StateProperty::all(Some(Color::argb(255, 0, 0, 22)))),
            data_row_cursor: Some(StateProperty::all(Some(SystemMouseCursor::Text))),
            heading_cell_cursor: Some(StateProperty::all(Some(SystemMouseCursor::Click))),
            ..DataTableThemeData::default()
        };
        let resolved = read_in(
            move |child| DataTableTheme::new(data.clone(), child),
            move |context| ResolvedDataTable::of_in(context, hovered),
        );
        assert_eq!(resolved.data_row_color, Some(Color::argb(255, 0, 0, 11)));
        assert_eq!(resolved.heading_row_color, Some(Color::argb(255, 0, 0, 22)));
        assert_eq!(resolved.data_row_cursor, Some(SystemMouseCursor::Text));
        assert_eq!(resolved.heading_cell_cursor, Some(SystemMouseCursor::Click));

        // Four `None`s with nothing set: upstream has no default row colour
        // -- a table draws on whatever it is placed on -- and no cursor
        // beyond the pointer's own.
        let plain = read_in(|child| child, ResolvedDataTable::of);
        assert_eq!(plain.data_row_color, None);
        assert_eq!(plain.heading_row_color, None);
        assert_eq!(plain.data_row_cursor, None);
        assert_eq!(plain.heading_cell_cursor, None);
    }

    #[test]
    fn a_drag_handle_is_wide_and_thin_so_it_reads_as_a_grip() {
        // Upstream's `_BottomSheetDefaultsM3.dragHandleSize` is 32 by 4.
        let plain = read_in(
            |child| child,
            |context| ResolvedBottomSheet::of(context, false, None, true),
        );
        assert_eq!(
            plain.drag_handle_size,
            ResolvedBottomSheet::DRAG_HANDLE_SIZE
        );
        assert!(
            plain.drag_handle_size.width > plain.drag_handle_size.height * 4.0,
            "much wider than it is tall: {:?}",
            plain.drag_handle_size
        );

        let themed = read_in(
            |child| {
                BottomSheetTheme::new(
                    BottomSheetThemeData {
                        drag_handle_size: Some(crate::render::Size::new(55.0, 6.0)),
                        ..BottomSheetThemeData::default()
                    },
                    child,
                )
            },
            |context| ResolvedBottomSheet::of(context, false, None, true),
        );
        assert_eq!(themed.drag_handle_size, crate::render::Size::new(55.0, 6.0));
    }

    #[test]
    fn an_expansion_tile_carries_its_animation_style_whole() {
        // Upstream asks for the three parts one at a time, each with its own
        // fallback -- and `reverseCurve` has none at all -- so a style that
        // names only a duration keeps the default curve. Carrying the style
        // whole is what lets a reader do that.
        let plain = read_in(|child| child, ResolvedExpansionTile::of);
        assert_eq!(plain.expansion_animation_style, None);

        let themed = read_in(
            |child| {
                ExpansionTileTheme::new(
                    ExpansionTileThemeData {
                        expansion_animation_style: Some(crate::animation::AnimationStyle {
                            duration: Some(std::time::Duration::from_millis(77)),
                            ..crate::animation::AnimationStyle::default()
                        }),
                        ..ExpansionTileThemeData::default()
                    },
                    child,
                )
            },
            ResolvedExpansionTile::of,
        );
        let style = themed
            .expansion_animation_style
            .expect("the theme named one");
        assert_eq!(style.duration, Some(std::time::Duration::from_millis(77)));
        assert_eq!(
            style.curve, None,
            "and the parts it did not name stay unnamed, so each keeps its \
             own fallback"
        );
    }

    #[test]
    fn a_drawer_has_a_shape_for_each_edge_it_can_open_from() {
        // `ResolvedDrawer` had no shape at all, so neither `shape` nor
        // `endShape` reached anything. They are two fields and not one
        // mirrored: upstream rounds the corners on the side **facing the
        // page**, which is the trailing side for a start drawer and the
        // leading side for an end one, and `Drawer.build` picks by
        // `isDrawerStart`.
        let outline = |width: f32| {
            Some(ShapeBorder::Rounded(
                crate::borders::RoundedRectangleBorder::new(
                    BorderSide {
                        color: Color::argb(255, 255, 0, 0),
                        width,
                        ..BorderSide::NONE
                    },
                    crate::borders::BorderRadiusGeometry::Zero,
                ),
            ))
        };
        let width_of = |shape: ShapeBorder| match shape {
            ShapeBorder::Rounded(rounded) => rounded.side.width,
            other => panic!("{other:?}"),
        };
        let resolved = read_in(
            move |child| {
                DrawerTheme::new(
                    DrawerThemeData {
                        shape: outline(33.0),
                        end_shape: outline(44.0),
                        ..DrawerThemeData::default()
                    },
                    child,
                )
            },
            ResolvedDrawer::of,
        );
        assert_eq!(width_of(resolved.shape), 33.0);
        assert_eq!(width_of(resolved.end_shape), 44.0);

        // With neither set, the two defaults are two shapes -- mirror images,
        // so they are not equal, which is what makes them a pair.
        let plain = read_in(|child| child, ResolvedDrawer::of);
        assert_eq!(plain.shape, ResolvedDrawer::default_shape(false));
        assert_eq!(plain.end_shape, ResolvedDrawer::default_shape(true));
        assert_ne!(plain.shape, plain.end_shape);
    }

    #[test]
    fn a_buttons_icon_side_and_animation_come_off_the_style_too() {
        // `tools/unread_theme_fields.py` found `ButtonStyle::icon_alignment`
        // and `animation_duration` reaching nothing. Upstream resolves both
        // through the same `effectiveValue` walk as the colours and hands
        // them to the button it builds.
        use crate::components::ButtonVariant;

        let defaults = || ResolvedButton {
            background: None,
            foreground: Color::argb(255, 8, 8, 8),
            side: None,
            padding: None,
            minimum_size: None,
            icon_alignment: IconAlignment::Start,
            animation_duration: ResolvedButton::ANIMATION_DURATION,
        };

        let themed = read_in(
            |child| {
                FilledButtonTheme::new(
                    FilledButtonThemeData {
                        style: Some(ButtonStyle {
                            icon_alignment: Some(IconAlignment::End),
                            animation_duration: Some(std::time::Duration::from_millis(77)),
                            ..ButtonStyle::default()
                        }),
                    },
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
            },
        );
        assert_eq!(themed.icon_alignment, IconAlignment::End);
        assert_eq!(
            themed.animation_duration,
            std::time::Duration::from_millis(77)
        );
    }

    #[test]
    fn and_fall_back_to_the_buttons_own_defaults() {
        // The last step for both belongs to the *button*, not the theme:
        // `IconAlignment.start` and `kThemeChangeDuration`. A style that
        // names neither leaves the button's own answers standing.
        use crate::components::ButtonVariant;

        let defaults = || ResolvedButton {
            background: None,
            foreground: Color::argb(255, 8, 8, 8),
            side: None,
            padding: None,
            minimum_size: None,
            icon_alignment: IconAlignment::End,
            animation_duration: std::time::Duration::from_millis(88),
        };
        let plain = read_in(
            |child| {
                FilledButtonTheme::new(
                    FilledButtonThemeData {
                        style: Some(ButtonStyle::default()),
                    },
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
            },
        );
        assert_eq!(plain.icon_alignment, IconAlignment::End);
        assert_eq!(
            plain.animation_duration,
            std::time::Duration::from_millis(88)
        );

        // And the constant is upstream's `kThemeChangeDuration`.
        assert_eq!(
            ResolvedButton::ANIMATION_DURATION,
            std::time::Duration::from_millis(200)
        );
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

#[cfg(test)]
mod script_category_tests {
    use super::*;
    use crate::platform::Brightness;

    #[test]
    fn material_threes_three_geometries_are_one_geometry() {
        // The same fifteen styles with the same sizes and heights, so a
        // Material 3 application renders Thai and English through identical
        // metrics.
        assert_eq!(Typography::english_like(), Typography::dense());
        assert_eq!(Typography::english_like(), Typography::tall());
        assert!(!Typography::geometries_differ());
    }

    #[test]
    fn so_the_switch_has_three_arms_and_one_answer() {
        let english = Typography::geometry_for(ScriptCategory::EnglishLike);
        for category in [
            ScriptCategory::EnglishLike,
            ScriptCategory::Dense,
            ScriptCategory::Tall,
        ] {
            assert_eq!(Typography::geometry_for(category), english, "{category:?}");
        }
    }

    /// Three themes that are visibly different, so the routing has something
    /// to get wrong.
    fn distinct() -> (TextTheme, TextTheme, TextTheme) {
        let sized = |size: f32| TextTheme {
            body_medium: Some(TextStyle {
                font_size: size,
                ..TextStyle::default()
            }),
            ..TextTheme::default()
        };
        (sized(1.0), sized(2.0), sized(3.0))
    }

    #[test]
    fn the_routing_sends_each_category_to_its_own_theme() {
        // Checked against three distinct themes, because against the real
        // three -- which are identical -- collapsing every arm onto
        // `english_like` passes.
        let (english, dense, tall) = distinct();
        for (category, expected) in [
            (ScriptCategory::EnglishLike, 1.0),
            (ScriptCategory::Dense, 2.0),
            (ScriptCategory::Tall, 3.0),
        ] {
            assert_eq!(
                Typography::select(category, english.clone(), dense.clone(), tall.clone())
                    .body_medium
                    .map(|style| style.font_size),
                Some(expected),
                "{category:?}"
            );
        }
    }

    #[test]
    fn and_the_difference_check_looks_at_both_pairs() {
        // A version comparing only english against dense would miss a tall
        // that had drifted, which is exactly what a future typography could
        // do to one of the three.
        let (english, dense, tall) = distinct();
        assert!(Typography::any_geometry_differs(&english, &dense, &tall));
        assert!(
            Typography::any_geometry_differs(&english, &english, &tall),
            "dense matching is not enough"
        );
        assert!(
            Typography::any_geometry_differs(&english, &dense, &english),
            "nor is tall matching"
        );
        assert!(!Typography::any_geometry_differs(
            &english, &english, &english
        ));
    }

    #[test]
    fn and_english_like_is_the_one_a_locale_falls_back_to() {
        assert_eq!(ScriptCategory::default(), ScriptCategory::EnglishLike);
    }

    // -- The two axes -----------------------------------------------------------

    #[test]
    fn the_brightness_picks_the_ink_and_the_language_picks_the_metrics() {
        // Neither knows about the other: a Japanese application in the dark
        // takes one from each.
        let light = Typography::for_theme(Brightness::Light, ScriptCategory::Dense);
        let dark = Typography::for_theme(Brightness::Dark, ScriptCategory::Dense);
        assert_eq!(
            light.body_medium.as_ref().map(|style| style.color),
            Some(Color::BLACK)
        );
        assert_eq!(
            dark.body_medium.as_ref().map(|style| style.color),
            Some(Color::WHITE)
        );
    }

    #[test]
    fn and_the_two_choices_do_not_interfere() {
        // The same brightness gives the same ink whatever the script, and the
        // same script gives the same metrics whatever the brightness.
        for category in [ScriptCategory::EnglishLike, ScriptCategory::Tall] {
            assert_eq!(
                Typography::for_theme(Brightness::Light, category)
                    .body_medium
                    .as_ref()
                    .map(|style| style.color),
                Some(Color::BLACK)
            );
            assert_eq!(
                Typography::for_theme(Brightness::Dark, category)
                    .body_medium
                    .as_ref()
                    .map(|style| style.font_size),
                Typography::for_theme(Brightness::Light, category)
                    .body_medium
                    .as_ref()
                    .map(|style| style.font_size)
            );
        }
    }

    #[test]
    fn the_colour_reaches_every_style_that_is_set() {
        let dark = Typography::for_theme(Brightness::Dark, ScriptCategory::EnglishLike);
        for style in [
            &dark.display_large,
            &dark.title_medium,
            &dark.body_small,
            &dark.label_small,
        ] {
            assert_eq!(style.as_ref().map(|style| style.color), Some(Color::WHITE));
        }
    }

    #[test]
    fn a_geometry_carries_no_colour_of_its_own_yet() {
        // Which is why the merge is a merge: the geometry is metrics and the
        // colour arrives separately.
        let plain = Typography::english_like();
        let inked = Typography::for_theme(Brightness::Dark, ScriptCategory::EnglishLike);
        assert_ne!(
            plain.body_medium.as_ref().map(|style| style.color),
            inked.body_medium.as_ref().map(|style| style.color)
        );
        assert_eq!(
            plain.body_medium.as_ref().map(|style| style.font_size),
            inked.body_medium.as_ref().map(|style| style.font_size),
            "and the metrics survive the inking"
        );
    }
}

// -- Whose colour a selected row is drawn in ----------------------------------

#[cfg(test)]
mod selected_tile_colour_tests {
    //! Upstream's three control tiles all hand their `ListTile` a
    //! `selectedColor` taken from their own control -- `SwitchListTile.build`
    //! passes `effectiveActiveColor` -- so a selected row is drawn in the
    //! colour of the thing that made it selected.
    //!
    //! This port resolved the selected colour from the list tile theme alone,
    //! which put the theme's primary on a row whose switch is some other
    //! colour: two accents on one line.

    use super::{ListTileTheme, ListTileThemeData, ResolvedListTile};
    use crate::components::Theme;
    use crate::engine::Color;
    use crate::framework::{
        AnyWidget, BuildContext, Component, ElementTree, component, leaf, provide,
    };
    use std::cell::Cell;
    use std::rc::Rc;

    const MINE: Color = Color(0xff00cc44);
    const THEMES: Color = Color(0xffcc0044);

    /// The text colour a tile resolves, and the two scheme colours it falls
    /// back to -- read from the same context, because the scheme the
    /// resolution consults is `ThemeData`'s and not the one the test provides.
    fn resolved(
        selected: bool,
        own: Option<Color>,
        theme_colour: Option<Color>,
    ) -> (Color, Color, Color) {
        struct Reader {
            selected: bool,
            own: Option<Color>,
            seen: Rc<Cell<(Color, Color, Color)>>,
        }
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let tile = ResolvedListTile::of_with_selected_color(
                    context,
                    self.selected,
                    None,
                    self.own,
                );
                let scheme = super::ThemeData::of(context).color_scheme;
                self.seen
                    .set((tile.text_color, scheme.primary, scheme.on_surface));
                leaf(|| crate::widgets::Empty)
            }
        }
        let seen = Rc::new(Cell::new((Color(0), Color(0), Color(0))));
        let mut data = ListTileThemeData::new();
        data.selected_color = theme_colour;
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Theme::dark(),
            ListTileTheme::new(
                data,
                component(Reader {
                    selected,
                    own: own,
                    seen: Rc::clone(&seen),
                }),
            ),
        ));
        seen.get()
    }

    #[test]
    fn a_colour_the_widget_gave_beats_the_themes() {
        // The order upstream's `ListTile` uses, and the one a control tile
        // relies on: it fills the widget's slot with its control's colour and
        // expects that to win.
        assert_eq!(resolved(true, Some(MINE), Some(THEMES)).0, MINE);
    }

    #[test]
    fn and_the_themes_beats_the_schemes() {
        assert_eq!(resolved(true, None, Some(THEMES)).0, THEMES);
        let (text, primary, _) = resolved(true, None, None);
        assert_eq!(text, primary, "with nobody else asked, the scheme's");
    }

    #[test]
    fn but_none_of_them_touches_a_row_that_is_not_selected() {
        // The colour is *for* being selected. A tile that took it anyway would
        // paint every row of a list in the accent colour.
        let (text, primary, on_surface) = resolved(false, Some(MINE), Some(THEMES));
        assert_eq!(text, on_surface);
        assert_ne!(text, MINE);
        assert_ne!(text, primary, "and not the accent either");
    }
}

// -- Whether an app bar centres its title -------------------------------------

#[cfg(test)]
mod app_bar_centring_tests {
    //! Upstream's `_getEffectiveCenterTitle` is
    //! `centerTitle ?? appbarTheme.centerTitle ?? platformCenter()`, and this
    //! resolver had the middle level and then `unwrap_or(false)`. So on iOS
    //! and macOS the title was never centred, which is that platform's whole
    //! convention for a navigation bar.

    use super::ResolvedAppBar;
    use crate::editable_text::TargetPlatform;

    #[test]
    fn the_apple_platforms_centre_a_title_and_the_others_do_not() {
        for platform in [TargetPlatform::IOS, TargetPlatform::MacOS] {
            assert!(ResolvedAppBar::platform_center(platform, 0), "{platform:?}");
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Linux,
            TargetPlatform::Windows,
        ] {
            assert!(
                !ResolvedAppBar::platform_center(platform, 0),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn but_a_second_action_makes_an_apple_bar_give_up_on_centring() {
        // The clause that is easy to miss: `actions == null || actions.length < 2`.
        // A centred title with buttons on both sides has to fit between them,
        // so a bar that has grown a second action stops centring rather than
        // truncating the title.
        assert!(ResolvedAppBar::platform_center(TargetPlatform::IOS, 1));
        assert!(!ResolvedAppBar::platform_center(TargetPlatform::IOS, 2));
        assert!(!ResolvedAppBar::platform_center(TargetPlatform::MacOS, 3));
    }

    #[test]
    fn and_the_count_changes_nothing_anywhere_else() {
        // The clause belongs to the Apple branch. Android reading it would
        // make a bar's alignment depend on how many buttons it happens to
        // carry, which is not a rule anybody stated.
        for actions in [0, 1, 2, 5] {
            assert!(!ResolvedAppBar::platform_center(
                TargetPlatform::Android,
                actions
            ));
        }
    }
}
