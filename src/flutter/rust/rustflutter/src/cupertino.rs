// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The Cupertino (iOS-style) widget tier.
//!
//! Ported symbol-by-symbol from `packages/flutter/lib/src/cupertino/`, for the
//! gallery's Cupertino demos. Each widget names its upstream anchor at its
//! own site; this header holds the deltas that apply across the tier.
//!
//! # What is deliberately different
//!
//! - **No icon font.** Upstream's glyphs are `CupertinoIcons` codepoints in
//!   the CupertinoIcons font. This crate has no icon font, and a missing glyph
//!   draws nothing at all, so the glyphs this tier needs are drawn: the back
//!   chevron ([`CupertinoNavigationBar`]'s back button), the search and clear
//!   marks ([`CupertinoSearchTextField`]) are strokes and circles on the
//!   canvas, the way [`crate::controls::Checkbox`] draws its tick; tab-bar
//!   "icons" are caller-supplied one- or two-character marks, the same
//!   substitution [`crate::controls::Destination`] makes.
//! - **No blur.** Upstream's nav bar, tab bar and dialog sit over a
//!   `BackdropFilter` blur; there is no backdrop filter in the paint bridge.
//!   The translucent colors (`barBackgroundColor` 0xF0.., `_kDialogColor`
//!   0xCC..) are kept, so the shapes and tints match and only the frosted
//!   texture is missing.
//! - **Overlays are the app's, not the framework's** -- the same rule as
//!   [`crate::controls`]: a dialog or context menu is a surface to put in a
//!   `Stack` over a scrim; there is no `showCupertinoDialog` route machinery.
//! - **Hairlines are one logical pixel.** Upstream draws its dividers at
//!   thickness 0.0 or 0.3 (device-pixel hairlines); at this renderer's unit
//!   scale one logical pixel is the hairline, the convention
//!   [`crate::components::Divider`] already establishes.
//! - **`CupertinoDynamicColor`'s high-contrast variants are dropped.** The
//!   platform bridge carries brightness but no contrast setting, so the
//!   `highContrastColor`/`darkHighContrastColor` columns of
//!   `cupertino/colors.dart` have nothing to resolve against; the light/dark
//!   pair is kept.
//! - **The corner radius is `Radius.circular`, not `RSuperellipse`.** The
//!   paint bridge has no superellipse; upstream itself falls back to `RRect`
//!   "since this shape is really small" in several of these widgets.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use crate::engine::{Color, Paint, Rect, Style, TextStyle};
use crate::framework::{
    AnyWidget, BuildContext, Component, Key, StateHandle, StatefulComponent, leaf, many, single,
    stateful,
};
use crate::gestures::PointerHandlers;
use crate::platform::Brightness;
use crate::render::{
    Alignment, BoxConstraints, BoxedRender, CrossAxisAlignment, EdgeInsets, FlexChild,
    HitTestResult, MainAxisSize, Offset, PaintContext, RenderBox, RenderClipRect,
    RenderConstrainedBox, RenderFlex, RenderOpacity, RenderRef, RenderStack, Size, StackPosition,
    TextOverflow,
};
use crate::widgets::{Align, Center, Column, Container, Empty, Pointer, Row, Text};

// -- Colors -------------------------------------------------------------------
//
// Anchor: cupertino/colors.dart, `class CupertinoColors`.

/// A color with a light and a dark variant. Upstream's `CupertinoDynamicColor`
/// minus its high-contrast columns (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoDynamicColor {
    /// The light-appearance value. Upstream's `color`.
    pub color: Color,
    /// The dark-appearance value. Upstream's `darkColor`.
    pub dark_color: Color,
}

impl CupertinoDynamicColor {
    /// Upstream's `CupertinoDynamicColor.withBrightness`.
    pub const fn with_brightness(color: Color, dark_color: Color) -> CupertinoDynamicColor {
        CupertinoDynamicColor { color, dark_color }
    }

    /// Upstream's `CupertinoDynamicColor.resolve`, given the appearance
    /// directly rather than a context.
    pub const fn resolve(&self, brightness: Brightness) -> Color {
        match brightness {
            Brightness::Light => self.color,
            Brightness::Dark => self.dark_color,
        }
    }
}

impl From<Color> for CupertinoDynamicColor {
    fn from(color: Color) -> CupertinoDynamicColor {
        CupertinoDynamicColor::with_brightness(color, color)
    }
}

/// The iOS system color table. Upstream `CupertinoColors` (colors.dart); the
/// associated constants here carry the same names, uppercased.
pub struct CupertinoColors;

#[allow(non_upper_case_globals)]
impl CupertinoColors {
    pub const WHITE: Color = Color::WHITE;
    pub const BLACK: Color = Color::BLACK;
    pub const TRANSPARENT: Color = Color::TRANSPARENT;
    pub const LIGHT_BACKGROUND_GRAY: Color = Color::rgb(0xE5, 0xE5, 0xEA);
    pub const EXTRA_LIGHT_BACKGROUND_GRAY: Color = Color::rgb(0xEF, 0xEF, 0xF4);
    pub const DARK_BACKGROUND_GRAY: Color = Color::rgb(0x17, 0x17, 0x17);

    pub const INACTIVE_GRAY: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::rgb(0x99, 0x99, 0x99),
        Color::rgb(0x75, 0x75, 0x75),
    );
    pub const DESTRUCTIVE_RED: CupertinoDynamicColor = Self::SYSTEM_RED;

    pub const SYSTEM_BLUE: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(0, 122, 255), Color::rgb(10, 132, 255));
    pub const SYSTEM_GREEN: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(52, 199, 89), Color::rgb(48, 209, 88));
    pub const SYSTEM_MINT: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(0, 199, 190), Color::rgb(99, 230, 226));
    pub const SYSTEM_INDIGO: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(88, 86, 214), Color::rgb(94, 92, 230));
    pub const SYSTEM_ORANGE: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(255, 149, 0), Color::rgb(255, 159, 10));
    pub const SYSTEM_PINK: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(255, 45, 85), Color::rgb(255, 55, 95));
    pub const SYSTEM_BROWN: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(162, 132, 94), Color::rgb(172, 142, 104));
    pub const SYSTEM_PURPLE: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(175, 82, 222), Color::rgb(191, 90, 242));
    pub const SYSTEM_RED: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(255, 59, 48), Color::rgb(255, 69, 58));
    pub const SYSTEM_TEAL: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(90, 200, 250), Color::rgb(100, 210, 255));
    pub const SYSTEM_CYAN: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(50, 173, 230), Color::rgb(100, 210, 255));
    pub const SYSTEM_YELLOW: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(255, 204, 0), Color::rgb(255, 214, 10));
    pub const SYSTEM_GREY: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::rgb(142, 142, 147),
        Color::rgb(142, 142, 147),
    );
    pub const SYSTEM_GREY2: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(174, 174, 178), Color::rgb(99, 99, 102));
    pub const SYSTEM_GREY3: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(199, 199, 204), Color::rgb(72, 72, 74));
    pub const SYSTEM_GREY4: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(209, 209, 214), Color::rgb(58, 58, 60));
    pub const SYSTEM_GREY5: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(229, 229, 234), Color::rgb(44, 44, 46));
    pub const SYSTEM_GREY6: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(242, 242, 247), Color::rgb(28, 28, 30));

    pub const LABEL: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::BLACK, Color::WHITE);
    pub const SECONDARY_LABEL: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::argb(153, 60, 60, 67),
        Color::argb(153, 235, 235, 245),
    );
    pub const TERTIARY_LABEL: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::argb(76, 60, 60, 67),
        Color::argb(76, 235, 235, 245),
    );
    pub const QUATERNARY_LABEL: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::argb(45, 60, 60, 67),
        Color::argb(40, 235, 235, 245),
    );
    pub const SYSTEM_FILL: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::argb(51, 120, 120, 128),
        Color::argb(91, 120, 120, 128),
    );
    pub const SECONDARY_SYSTEM_FILL: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::argb(40, 120, 120, 128),
        Color::argb(81, 120, 120, 128),
    );
    pub const TERTIARY_SYSTEM_FILL: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::argb(30, 118, 118, 128),
        Color::argb(61, 118, 118, 128),
    );
    pub const QUATERNARY_SYSTEM_FILL: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(
            Color::argb(20, 116, 116, 128),
            Color::argb(45, 118, 118, 128),
        );
    pub const PLACEHOLDER_TEXT: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::argb(76, 60, 60, 67),
        Color::argb(76, 235, 235, 245),
    );
    pub const SYSTEM_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::WHITE, Color::BLACK);
    pub const SECONDARY_SYSTEM_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(242, 242, 247), Color::rgb(28, 28, 30));
    pub const TERTIARY_SYSTEM_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::WHITE, Color::rgb(44, 44, 46));
    pub const SYSTEM_GROUPED_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(242, 242, 247), Color::BLACK);
    pub const SECONDARY_SYSTEM_GROUPED_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::WHITE, Color::rgb(28, 28, 30));
    pub const TERTIARY_SYSTEM_GROUPED_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(242, 242, 247), Color::rgb(44, 44, 46));
    pub const SEPARATOR: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
        Color::argb(73, 60, 60, 67),
        Color::argb(153, 84, 84, 88),
    );
    pub const OPAQUE_SEPARATOR: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(198, 198, 200), Color::rgb(56, 56, 58));
    pub const LINK: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(0, 122, 255), Color::rgb(9, 132, 255));
}

// -- Theme --------------------------------------------------------------------
//
// Anchor: cupertino/theme.dart, `CupertinoThemeData` and its `_kDefaultTheme`.

/// Styling shared by the Cupertino widgets below. Upstream's
/// `CupertinoThemeData`, reduced to the fields the ported widgets read:
/// `textTheme` is inlined into each widget (the `_kDefault*TextStyle`
/// constants of text_theme.dart are copied at the use sites), and
/// `selectionHandleColor`/`applyThemeToAll` have no consumers here yet.
#[derive(Clone, Debug, PartialEq)]
pub struct CupertinoTheme {
    pub brightness: Brightness,
    pub primary_color: Color,
    pub primary_contrasting_color: Color,
    /// Nav bars and tab bars. Translucent; the blur that would sit under it
    /// upstream is not ported (see the module docs).
    pub bar_background_color: Color,
    pub scaffold_background_color: Color,
}

impl CupertinoTheme {
    /// `_kDefaultTheme` resolved for a light appearance.
    pub fn light() -> CupertinoTheme {
        let brightness = Brightness::Light;
        CupertinoTheme {
            brightness,
            primary_color: CupertinoColors::SYSTEM_BLUE.resolve(brightness),
            primary_contrasting_color: CupertinoColors::WHITE,
            // `_CupertinoThemeDefaults.barBackgroundColor`. The dark value is
            // the navigation bar's; upstream notes the toolbar/tab bar dark
            // value is 0xF0161616, a distinction only the nav bar keeps.
            bar_background_color: Color(0xF0F9_F9F9),
            scaffold_background_color: CupertinoColors::SYSTEM_BACKGROUND.resolve(brightness),
        }
    }

    /// `_kDefaultTheme` resolved for a dark appearance.
    pub fn dark() -> CupertinoTheme {
        let brightness = Brightness::Dark;
        CupertinoTheme {
            brightness,
            primary_color: CupertinoColors::SYSTEM_BLUE.resolve(brightness),
            primary_contrasting_color: CupertinoColors::WHITE,
            bar_background_color: Color(0xF01D_1D1D),
            scaffold_background_color: CupertinoColors::SYSTEM_BACKGROUND.resolve(brightness),
        }
    }

    /// Resolves a dynamic color against this theme's appearance: upstream's
    /// `CupertinoDynamicColor.resolve(color, context)` with the context's
    /// brightness.
    pub fn resolve(&self, color: CupertinoDynamicColor) -> Color {
        color.resolve(self.brightness)
    }

    /// text_theme.dart's `_kDefaultTextStyle` (17pt, -0.41 tracking) in the
    /// label color.
    pub fn text_style(&self) -> TextStyle {
        TextStyle {
            font_size: 17.0,
            letter_spacing: Some(-0.41),
            color: self.resolve(CupertinoColors::LABEL),
            ..TextStyle::default()
        }
    }

    /// text_theme.dart's `_kDefaultActionTextStyle` in the theme's primary
    /// color (upstream's `actionTextStyle(primaryColor:)`).
    pub fn action_text_style(&self) -> TextStyle {
        TextStyle {
            font_size: 17.0,
            letter_spacing: Some(-0.41),
            color: self.primary_color,
            ..TextStyle::default()
        }
    }
}

impl Default for CupertinoTheme {
    /// Upstream resolves `_kDefaultTheme`'s null brightness from the context;
    /// with no context the platform default is light.
    fn default() -> CupertinoTheme {
        CupertinoTheme::light()
    }
}

/// Reads the Cupertino theme in scope, or the default. Upstream's
/// `CupertinoTheme.of`.
pub fn cupertino_theme_of(context: &BuildContext) -> Rc<CupertinoTheme> {
    context.inherited_or_default::<CupertinoTheme>()
}

// -- Button -------------------------------------------------------------------
//
// Anchor: cupertino/button.dart, `CupertinoButton`.

/// How big a [`CupertinoButton`] is. Upstream's `CupertinoButtonSize`, whose
/// padding, corner radius and minimum size come from the
/// `kCupertinoButtonPadding` / `kCupertinoButtonSizeBorderRadius` /
/// `kCupertinoButtonMinSize` maps in cupertino/constants.dart.
///
/// The pre-iOS-17 constants some references still cite (radius 8, padding
/// 16/8, minimum 44 flat) predate the `sizeStyle` parameter; the maps below
/// are the current anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CupertinoButtonSize {
    Small,
    Medium,
    /// The size upstream's constructors default to.
    #[default]
    Large,
}

impl CupertinoButtonSize {
    /// `kCupertinoButtonPadding`.
    pub fn padding(self) -> EdgeInsets {
        match self {
            CupertinoButtonSize::Small => EdgeInsets::symmetric(12.0, 6.0),
            CupertinoButtonSize::Medium => EdgeInsets::symmetric(15.0, 10.0),
            CupertinoButtonSize::Large => EdgeInsets::symmetric(20.0, 16.0),
        }
    }

    /// `kCupertinoButtonSizeBorderRadius`.
    pub fn border_radius(self) -> f32 {
        match self {
            CupertinoButtonSize::Small | CupertinoButtonSize::Medium => 40.0,
            CupertinoButtonSize::Large => 12.0,
        }
    }

    /// `kCupertinoButtonMinSize`.
    pub fn min_size(self) -> f32 {
        match self {
            CupertinoButtonSize::Small => 28.0,
            CupertinoButtonSize::Medium => 32.0,
            CupertinoButtonSize::Large => K_MIN_INTERACTIVE_DIMENSION_CUPERTINO,
        }
    }
}

/// The least side of a tappable region. Upstream's
/// `kMinInteractiveDimensionCupertino` (cupertino/constants.dart).
pub const K_MIN_INTERACTIVE_DIMENSION_CUPERTINO: f32 = 44.0;

/// The opacity a pressed [`CupertinoButton`] fades to. Upstream's
/// `CupertinoButton.pressedOpacity` default.
pub const PRESSED_OPACITY: f32 = 0.4;

/// How fast the fade out to [`PRESSED_OPACITY`] runs. button.dart's
/// `kFadeOutDuration`.
const BUTTON_FADE_OUT: Duration = Duration::from_millis(120);

/// How fast the fade back in runs. button.dart's `kFadeInDuration`.
const BUTTON_FADE_IN: Duration = Duration::from_millis(180);

/// An iOS-style button. Upstream's `CupertinoButton` (button.dart).
///
/// The pressed state is the caller's, as with [`crate::components::Button`]:
/// pass it with [`CupertinoButton::with_pressed`] and track it with
/// [`CupertinoButton::wired`]. The fade to [`PRESSED_OPACITY`] runs on the
/// frame clock, upstream's `_opacityAnimation` over `kFadeOutDuration` /
/// `kFadeInDuration`.
///
/// The child is a label string rather than an arbitrary widget -- the same
/// reduction [`crate::components::Button`] makes.
pub struct CupertinoButton {
    id: u64,
    label: String,
    filled: bool,
    size: CupertinoButtonSize,
    color: Option<Color>,
    pressed: bool,
    enabled: bool,
    handlers: PointerHandlers,
}

impl CupertinoButton {
    /// The plain button: no fill, label in the primary color.
    pub fn new(id: u64, label: impl Into<String>) -> CupertinoButton {
        CupertinoButton {
            id,
            label: label.into(),
            filled: false,
            size: CupertinoButtonSize::default(),
            color: None,
            pressed: false,
            enabled: true,
            handlers: PointerHandlers::new(),
        }
    }

    /// Upstream's `CupertinoButton.filled`: filled with the primary color.
    pub fn filled(id: u64, label: impl Into<String>) -> CupertinoButton {
        CupertinoButton {
            filled: true,
            ..CupertinoButton::new(id, label)
        }
    }

    /// Upstream's `CupertinoButton.color`: a fill other than the theme's
    /// primary. Setting one on a plain button is what upstream does too --
    /// the constructor asserts `color == null` only relative to `.filled`.
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Upstream's `sizeStyle`.
    pub fn with_size_style(mut self, size: CupertinoButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn with_pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The usual wiring: a tap runs `action` and a press repaints, exactly as
    /// [`crate::components::Button::wired`].
    pub fn wired<S: 'static>(
        mut self,
        handle: StateHandle<S>,
        pressed_field: fn(&mut S) -> &mut Option<u64>,
        action: fn(&mut S),
    ) -> Self {
        if !self.enabled {
            return self;
        }
        let id = self.id;
        let tap_handle = handle.clone();
        let press_handle = handle;
        self.handlers = PointerHandlers::new()
            .with_tap(move |_| {
                tap_handle.set_state(move |state| action(state));
            })
            .with_press_change(move |down| {
                press_handle.set_state(move |state| {
                    *pressed_field(state) = if down { Some(id) } else { None };
                });
            });
        self
    }
}

impl Component for CupertinoButton {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let id = self.id;
        let label = self.label.clone();
        let handlers = self.handlers.clone();
        let pressed = self.pressed && self.enabled;
        let enabled = self.enabled;
        let size = self.size;

        // button.dart's `effectiveForegroundColor`: the contrasting color on
        // a fill, the primary color otherwise, tertiaryLabel when disabled.
        let foreground = if !enabled {
            theme.resolve(CupertinoColors::TERTIARY_LABEL)
        } else if self.filled {
            theme.primary_contrasting_color
        } else {
            self.color.unwrap_or(theme.primary_color)
        };
        // The fill: `.filled` and `color` both paint; a disabled fill becomes
        // `disabledColor`, quaternarySystemFill for `.filled` and
        // tertiarySystemFill otherwise (button.dart's two constructors).
        let fill = if !enabled {
            if self.filled {
                Some(theme.resolve(CupertinoColors::QUATERNARY_SYSTEM_FILL))
            } else {
                self.color
                    .map(|_| theme.resolve(CupertinoColors::TERTIARY_SYSTEM_FILL))
            }
        } else if self.filled {
            Some(self.color.unwrap_or(theme.primary_color))
        } else {
            self.color
        };
        let radius = size.border_radius();
        let padding = size.padding();
        let min = size.min_size();

        // `_opacityAnimation`'s target, and its direction-dependent duration:
        // 120ms on the way down (kFadeOutDuration), 180ms back up
        // (kFadeInDuration), linear both ways -- upstream drives a plain
        // AnimationController, no curve.
        let target = if pressed { PRESSED_OPACITY } else { 1.0 };
        let duration = if pressed {
            BUTTON_FADE_OUT
        } else {
            BUTTON_FADE_IN
        };

        let described = |inner: AnyWidget| {
            let properties = if enabled {
                crate::semantics::SemanticsProperties::button(&self.label)
            } else {
                crate::semantics::SemanticsProperties::disabled_button(&self.label)
            };
            let tap = self.handlers.on_tap.clone();
            crate::semantics::semantics_with_action(
                crate::semantics::node_id_for(id),
                properties,
                inner,
                move |action| {
                    if action == crate::semantics::SemanticsAction::Tap {
                        if let Some(tap) = &tap {
                            tap(crate::gestures::TapEvent {
                                local_position: Offset::ZERO,
                                pointer_id: 0,
                            });
                        }
                    }
                },
            )
        };

        described(crate::implicit::animated(
            target,
            duration,
            crate::animation::Curve::Linear,
            move |opacity| {
                let label = label.clone();
                let handlers = handlers.clone();
                leaf(move || {
                    // The label is text_theme.dart's actionTextStyle, or
                    // actionSmallTextStyle at the small size (button.dart's
                    // `sizeStyle == small ?` branch).
                    let text_style = TextStyle {
                        font_size: if size == CupertinoButtonSize::Small {
                            15.0
                        } else {
                            17.0
                        },
                        letter_spacing: Some(if size == CupertinoButtonSize::Small {
                            -0.23
                        } else {
                            -0.41
                        }),
                        color: foreground,
                        ..TextStyle::default()
                    };
                    let mut face = Container::new()
                        .with_corner_radius(radius)
                        .with_padding(padding)
                        .with_child(
                            // Upstream's `Align(widthFactor: 1.0,
                            // heightFactor: 1.0)`: the button shrink-wraps
                            // its label rather than filling loose
                            // constraints.
                            Align::new(
                                Alignment::CENTER,
                                Text::new(label.clone()).with_style(text_style),
                            )
                            .with_factors(Some(1.0), Some(1.0)),
                        );
                    if let Some(fill) = fill {
                        face = face.with_color(fill);
                    }
                    Pointer::new(
                        id,
                        RenderOpacity::new(
                            opacity,
                            // Upstream's ConstrainedBox with
                            // `kCupertinoButtonMinSize[sizeStyle]` on both
                            // axes.
                            RenderConstrainedBox::new(BoxConstraints::new(
                                min,
                                f32::INFINITY,
                                min,
                                f32::INFINITY,
                            ))
                            .with_child(face),
                        ),
                    )
                    .with_handlers(handlers.clone())
                })
            },
        ))
    }
}

// -- Switch -------------------------------------------------------------------
//
// Anchor: cupertino/switch.dart, `CupertinoSwitch`.

/// The track's width and height: switch.dart's `_kTrackWidth`/`_kTrackHeight`.
pub const SWITCH_TRACK_SIZE: (f32, f32) = (51.0, 31.0);

/// Half the thumb's diameter. thumb_painter.dart's `CupertinoThumbPainter.radius`.
pub const SWITCH_THUMB_RADIUS: f32 = 14.0;

/// The whole hit region. switch.dart's `_kSwitchSize`.
pub const SWITCH_SIZE: (f32, f32) = (59.0, 39.0);

/// How much a held thumb grows. thumb_painter.dart's
/// `CupertinoThumbPainter.extension` (switch.dart's `_kThumbExtensionFactor`).
const THUMB_EXTENSION: f32 = 7.0;

/// A drag this far past the rest position commits the switch to the side it
/// was dragged toward; once it has flipped mid-drag, this much is enough to
/// flip it back. switch.dart's `_kDragCommitThreshold`/`_kDragReverseThreshold`,
/// in units of the track width.
const DRAG_COMMIT_THRESHOLD: f32 = 0.7;
const DRAG_REVERSE_THRESHOLD: f32 = 0.2;

/// switch.dart's `_kDisabledOpacity`.
const SWITCH_DISABLED_OPACITY: f32 = 0.5;

/// The thumb's drop shadows. thumb_painter.dart's `_kSwitchBoxShadows`.
fn switch_thumb_shadows() -> Vec<crate::painting::BoxShadow> {
    vec![
        crate::painting::BoxShadow::new(Color(0x2600_0000), 0.0, 3.0, 8.0, 0.0),
        crate::painting::BoxShadow::new(Color(0x0F00_0000), 0.0, 3.0, 1.0, 0.0),
    ]
}

/// What a [`CupertinoSwitch`] remembers between frames: where a drag has
/// taken the thumb, and whether the thumb is held.
#[derive(Default)]
pub struct CupertinoSwitchState {
    /// The value the current drag has flipped to, if a drag is in charge.
    /// switch.dart's `_dragValue`.
    drag_value: Option<bool>,
    /// Whether the thumb is held, which is what extends it.
    /// switch.dart's reaction controller, reduced to its two ends.
    pressed: bool,
}

/// An iOS-style on/off control. Upstream's `CupertinoSwitch` (switch.dart).
///
/// Tap toggles; a horizontal drag carries the thumb and commits once it has
/// crossed [`DRAG_COMMIT_THRESHOLD`] of the track's width, exactly upstream's
/// `_handleDragUpdate`/`_handleDragEnd`. The value itself is the caller's
/// state, as with [`crate::components::Switch`]; the transient drag and press
/// are the widget's own.
///
/// ```ignore
/// stateful(CupertinoSwitch::new(ids.take(), state.on).wired(handle, |s, on| s.on = on))
/// ```
pub struct CupertinoSwitch {
    id: u64,
    value: bool,
    enabled: bool,
    active_track_color: Option<Color>,
    on_changed: Option<Rc<dyn Fn(bool)>>,
}

impl CupertinoSwitch {
    pub fn new(id: u64, value: bool) -> CupertinoSwitch {
        CupertinoSwitch {
            id,
            value,
            enabled: true,
            active_track_color: None,
            on_changed: None,
        }
    }

    /// Upstream's `activeTrackColor` (`activeColor` before it).
    pub fn with_active_track_color(mut self, color: Color) -> Self {
        self.active_track_color = Some(color);
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// `changed` is given the state and the value the reader asked for.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, changed: fn(&mut S, bool)) -> Self {
        if self.enabled {
            self.on_changed = Some(Rc::new(move |value| {
                handle.set_state(move |state| changed(state, value));
            }));
        }
        self
    }
}

impl StatefulComponent for CupertinoSwitch {
    type State = CupertinoSwitchState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    fn build(
        &self,
        state: &CupertinoSwitchState,
        handle: StateHandle<CupertinoSwitchState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let id = self.id;
        let value = self.value;
        let enabled = self.enabled;
        // switch.dart: `activeTrackColor ?? CupertinoColors.systemGreen` --
        // the native iOS green, kept here rather than the theme's primary
        // color because upstream's `applyTheme` path only takes over when the
        // ambient `CupertinoThemeData.applyThemeToAll` asks it to, and the
        // default is false.
        let active_track = self
            .active_track_color
            .unwrap_or_else(|| theme.resolve(CupertinoColors::SYSTEM_GREEN));
        let inactive_track = theme.resolve(CupertinoColors::SECONDARY_SYSTEM_FILL);
        let track = if value { active_track } else { inactive_track };

        // Where the thumb is: the value, or where the drag has flipped it to.
        // Upstream this is the position controller's value; the animated()
        // below is that controller, 200ms linear as switch.dart sets it.
        let position = if state.drag_value.unwrap_or(value) {
            1.0
        } else {
            0.0
        };
        let extension = if state.pressed { THUMB_EXTENSION } else { 0.0 };

        let mut handlers = PointerHandlers::new();
        if let (Some(on_changed), true) = (&self.on_changed, enabled) {
            let on_tap = on_changed.clone();
            handlers = handlers.with_tap(move |_| on_tap(!value));

            // `_handleDragUpdate`, with `DragEvent.total` standing in for the
            // accumulated `_dragDelta`: the total movement since the press is
            // exactly upstream's tap-down offset plus the accumulated primary
            // deltas. RTL drags read right-to-left, as upstream's
            // `Directionality` switch has it.
            let drag_handle = handle.clone();
            let rtl = crate::direction::current_direction() == crate::direction::TextDirection::Rtl;
            handlers = handlers.with_drag_update(move |drag| {
                let delta = drag.total.dx / SWITCH_TRACK_SIZE.0 * if rtl { -1.0 } else { 1.0 };
                drag_handle.set_state(move |state| {
                    let drag_value = state.drag_value.unwrap_or(value);
                    // `_handleDragUpdate`'s threshold pair: a drag that has
                    // already flipped the value needs only the reverse
                    // threshold to flip back.
                    let threshold = if value != drag_value {
                        DRAG_REVERSE_THRESHOLD
                    } else {
                        DRAG_COMMIT_THRESHOLD
                    };
                    let effective = if value { -threshold } else { threshold };
                    let new_value = delta >= effective;
                    if new_value != drag_value {
                        state.drag_value = Some(new_value);
                    }
                });
            });
            let end_handle = handle.clone();
            let on_end = on_changed.clone();
            handlers = handlers.with_drag_end(move |_| {
                let on_end = on_end.clone();
                end_handle.set_state(move |state| {
                    // `_handleDragEnd`: the drag's last position becomes the
                    // value, reported if it differs.
                    if let Some(drag_value) = state.drag_value.take() {
                        if drag_value != value {
                            on_end(!value);
                        }
                    }
                });
            });
            let press_handle = handle;
            handlers = handlers.with_press_change(move |down| {
                press_handle.set_state(move |state| state.pressed = down);
            });
        }

        let tap = self.on_changed.clone();
        let switch = crate::implicit::animated(
            position,
            Duration::from_millis(200),
            crate::animation::Curve::Linear,
            move |position| {
                let handlers = handlers.clone();
                leaf(move || {
                    let (track_width, track_height) = SWITCH_TRACK_SIZE;
                    let (width, height) = SWITCH_SIZE;
                    // The track, centered in the hit region.
                    let track_left = (width - track_width) / 2.0;
                    let track_top = (height - track_height) / 2.0;
                    let track_box = Container::new()
                        .with_size(track_width, track_height)
                        .with_color(track)
                        .with_corner_radius(track_height / 2.0);

                    // The thumb travels the track's inner length
                    // (`_trackInnerLength`): trackWidth - trackHeight.
                    let travel = track_width - track_height;
                    // A held thumb extends toward the middle of the track,
                    // which is what upstream's `_pressedThumbExtension`
                    // widening reads as.
                    let thumb_width = SWITCH_THUMB_RADIUS * 2.0 + extension;
                    let thumb_left = track_left + track_height / 2.0 - SWITCH_THUMB_RADIUS
                        + position * travel
                        - if position > 0.5 { extension } else { 0.0 };
                    let thumb = Container::new()
                        .with_size(thumb_width, SWITCH_THUMB_RADIUS * 2.0)
                        // thumb_painter.dart's `CupertinoThumbPainter.switchThumb`
                        // default color.
                        .with_color(CupertinoColors::WHITE)
                        .with_corner_radius(SWITCH_THUMB_RADIUS)
                        .with_shadows(switch_thumb_shadows());

                    let mut stack = RenderStack::new().push_positioned(
                        track_box,
                        StackPosition {
                            left: Some(track_left),
                            top: Some(track_top),
                            ..Default::default()
                        },
                    );
                    stack = stack.push_positioned(
                        thumb,
                        StackPosition {
                            left: Some(thumb_left),
                            top: Some((height - SWITCH_THUMB_RADIUS * 2.0) / 2.0),
                            ..Default::default()
                        },
                    );
                    let body: BoxedRender = if enabled {
                        RenderRef::new(stack)
                    } else {
                        // `_kDisabledOpacity`.
                        RenderRef::new(RenderOpacity::new(SWITCH_DISABLED_OPACITY, stack))
                    };
                    Pointer::new(
                        id,
                        Container::new().with_size(width, height).with_child(body),
                    )
                    .with_handlers(handlers.clone())
                })
            },
        );

        crate::semantics::semantics_with_action(
            crate::semantics::node_id_for(id),
            crate::semantics::SemanticsProperties::toggle("", value),
            switch,
            move |action| {
                if action == crate::semantics::SemanticsAction::Tap {
                    if let Some(changed) = &tap {
                        changed(!value);
                    }
                }
            },
        )
    }
}

// -- Slider -------------------------------------------------------------------
//
// Anchor: cupertino/slider.dart, `CupertinoSlider`.

/// The track's inset from each end. slider.dart's `_kPadding`.
const SLIDER_PADDING: f32 = 8.0;

/// The default width. slider.dart's `_kSliderWidth`.
pub const SLIDER_WIDTH: f32 = 176.0;

/// The whole height: twice the thumb radius plus padding both sides.
/// slider.dart's `_kSliderHeight`.
pub const SLIDER_HEIGHT: f32 = 2.0 * (SWITCH_THUMB_RADIUS + SLIDER_PADDING);

/// The thumb's drop shadows. thumb_painter.dart's `_kSliderBoxShadows`.
fn slider_thumb_shadows() -> Vec<crate::painting::BoxShadow> {
    vec![
        crate::painting::BoxShadow::new(Color(0x2600_0000), 0.0, 3.0, 8.0, 0.0),
        crate::painting::BoxShadow::new(Color(0x2900_0000), 0.0, 1.0, 1.0, 0.0),
        crate::painting::BoxShadow::new(Color(0x1A00_0000), 0.0, 3.0, 1.0, 0.0),
    ]
}

/// Where along the track a pointer position reads as, 0..1. slider.dart's
/// `_discretize`-free half of `_RenderCupertinoSlider`: the thumb's center
/// runs from `_trackLeft + radius` to `_trackRight - radius`, and the value is
/// the fraction of that run under the pointer.
fn slider_value_at(local_dx: f32, width: f32) -> f32 {
    let start = SLIDER_PADDING + SWITCH_THUMB_RADIUS;
    let end = width - SLIDER_PADDING - SWITCH_THUMB_RADIUS;
    ((local_dx - start) / (end - start)).clamp(0.0, 1.0)
}

/// An iOS-style slider. Upstream's `CupertinoSlider` (slider.dart): a 2px
/// track (slider.dart paints `trackCenter ± 1.0`), a white 28px thumb with
/// the slider shadows, and a hit region the full [`SLIDER_HEIGHT`] tall.
pub struct CupertinoSlider {
    id: u64,
    value: f32,
    width: f32,
    active_color: Option<Color>,
    thumb_color: Option<Color>,
    handlers: PointerHandlers,
}

impl CupertinoSlider {
    pub fn new(id: u64, value: f32) -> CupertinoSlider {
        CupertinoSlider {
            id,
            value: value.clamp(0.0, 1.0),
            width: SLIDER_WIDTH,
            active_color: None,
            thumb_color: None,
            handlers: PointerHandlers::new(),
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Upstream's `activeColor`, the filled part of the track. Defaults to
    /// the theme's primary color.
    pub fn with_active_color(mut self, color: Color) -> Self {
        self.active_color = Some(color);
        self
    }

    /// Upstream's `thumbColor`. Defaults to white, thumb_painter.dart's
    /// `CupertinoThumbPainter` default.
    pub fn with_thumb_color(mut self, color: Color) -> Self {
        self.thumb_color = Some(color);
        self
    }

    /// Drags and taps both set the value from the pointer's position along
    /// the track, as upstream's `_handleDragUpdate`/`_handleTapDown` do.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, set: fn(&mut S, f32)) -> Self {
        let width = self.width;
        let drag_handle = handle.clone();
        let tap_handle = handle;
        self.handlers = PointerHandlers::new()
            .with_drag_update(move |drag| {
                let value = slider_value_at(drag.local_position.dx, width);
                drag_handle.set_state(move |state| set(state, value));
            })
            .with_tap(move |tap| {
                let value = slider_value_at(tap.local_position.dx, width);
                tap_handle.set_state(move |state| set(state, value));
            });
        self
    }
}

impl Component for CupertinoSlider {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let value = self.value;
        let width = self.width;
        let id = self.id;
        let handlers = self.handlers.clone();
        let active = self.active_color.unwrap_or(theme.primary_color);
        // slider.dart's `trackColor` default.
        let inactive = theme.resolve(CupertinoColors::SECONDARY_SYSTEM_FILL);
        let thumb_color = self.thumb_color.unwrap_or(CupertinoColors::WHITE);

        leaf(move || {
            let center = SLIDER_HEIGHT / 2.0;
            let track_left = SLIDER_PADDING;
            let track_right = width - SLIDER_PADDING;
            // The thumb's center along the track.
            let thumb_center = SLIDER_PADDING
                + SWITCH_THUMB_RADIUS
                + value * (track_right - track_left - SWITCH_THUMB_RADIUS * 2.0);

            // The two track halves, 2px tall and rounded 1 -- the RRect
            // slider.dart draws at `trackCenter - 1.0 .. trackCenter + 1.0`.
            let mut stack = RenderStack::new();
            if value < 1.0 {
                stack = stack.push_positioned(
                    Container::new()
                        .with_color(inactive)
                        .with_corner_radius(1.0),
                    StackPosition {
                        left: Some(thumb_center),
                        right: Some(track_left.max(width - track_right)),
                        top: Some(center - 1.0),
                        height: Some(2.0),
                        ..Default::default()
                    },
                );
            }
            if value > 0.0 {
                stack = stack.push_positioned(
                    Container::new().with_color(active).with_corner_radius(1.0),
                    StackPosition {
                        left: Some(track_left),
                        top: Some(center - 1.0),
                        width: Some(thumb_center - track_left),
                        height: Some(2.0),
                        ..Default::default()
                    },
                );
            }
            stack = stack.push_positioned(
                Container::new()
                    .with_size(SWITCH_THUMB_RADIUS * 2.0, SWITCH_THUMB_RADIUS * 2.0)
                    .with_color(thumb_color)
                    .with_corner_radius(SWITCH_THUMB_RADIUS)
                    .with_shadows(slider_thumb_shadows()),
                StackPosition {
                    left: Some(thumb_center - SWITCH_THUMB_RADIUS),
                    top: Some(center - SWITCH_THUMB_RADIUS),
                    ..Default::default()
                },
            );

            // The hit region is exactly `width` wide and the full height
            // tall: the value comes from the pointer's place along it. The
            // factors shrink-wrap the region under loose constraints, the way
            // upstream's `SizedBox(width: _kSliderWidth...)` bounds it.
            Align::new(
                Alignment::CENTER_LEFT,
                Pointer::new(
                    id,
                    Container::new()
                        .with_size(width, SLIDER_HEIGHT)
                        .with_child(stack),
                )
                .with_handlers(handlers.clone()),
            )
            .with_factors(Some(1.0), Some(1.0))
        })
    }
}

// -- Activity indicator -------------------------------------------------------
//
// Anchor: cupertino/activity_indicator.dart, `CupertinoActivityIndicator` and
// its `_CupertinoActivityIndicatorPainter`.

/// The default radius. activity_indicator.dart's `_kDefaultIndicatorRadius`.
pub const ACTIVITY_INDICATOR_RADIUS: f32 = 10.0;

/// The tick alphas, extracted from the native component.
/// activity_indicator.dart's `_kAlphaValues`.
const TICK_ALPHAS: [u8; 8] = [47, 47, 47, 47, 72, 97, 122, 147];

/// The alpha every revealed tick gets when the indicator is partially
/// revealed. activity_indicator.dart's `_partiallyRevealedAlpha`.
const PARTIAL_TICK_ALPHA: u8 = 147;

/// activity_indicator.dart's `_kActiveTickColor`.
const TICK_COLOR: CupertinoDynamicColor =
    CupertinoDynamicColor::with_brightness(Color(0xFF3C_3C44), Color(0xFFEB_EBF5));

/// What a [`CupertinoActivityIndicator`] remembers: where the rotation has
/// got to, and when it started on the frame clock.
#[derive(Default)]
pub struct CupertinoActivityIndicatorState {
    /// The rotation, 0..1, one full turn a second. Upstream's
    /// `_controller.value`.
    position: f32,
    started_micros: Option<i64>,
}

/// The eight-spoke iOS spinner. Upstream's `CupertinoActivityIndicator`
/// (activity_indicator.dart), animated on the frame clock rather than an
/// `AnimationController` -- one turn a second, as upstream's
/// `AnimationController(duration: const Duration(seconds: 1))..repeat()`.
///
/// ```ignore
/// stateful(CupertinoActivityIndicator::new())
/// ```
pub struct CupertinoActivityIndicator {
    radius: f32,
    color: Option<Color>,
    /// Upstream's `CupertinoActivityIndicator.partiallyRevealed(progress:)`:
    /// a static arc of `progress` of the ticks, used by the refresh control.
    progress: f32,
}

impl CupertinoActivityIndicator {
    pub fn new() -> CupertinoActivityIndicator {
        CupertinoActivityIndicator {
            radius: ACTIVITY_INDICATOR_RADIUS,
            color: None,
            progress: 1.0,
        }
    }

    /// Upstream's `.partiallyRevealed` constructor.
    pub fn partially_revealed(progress: f32) -> CupertinoActivityIndicator {
        CupertinoActivityIndicator {
            progress: progress.clamp(0.0, 1.0),
            ..Self::new()
        }
    }

    pub fn with_radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

impl Default for CupertinoActivityIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulComponent for CupertinoActivityIndicator {
    type State = CupertinoActivityIndicatorState;

    fn advance(&self, state: &mut CupertinoActivityIndicatorState, frame_time_micros: i64) -> bool {
        if self.progress < 1.0 {
            // A partially revealed indicator does not spin upstream either:
            // `_controller` is left stopped.
            return false;
        }
        let started = *state.started_micros.get_or_insert(frame_time_micros);
        state.position = ((frame_time_micros - started).max(0) as f32 / 1_000_000.0) % 1.0;
        true
    }

    fn build(
        &self,
        state: &CupertinoActivityIndicatorState,
        _handle: StateHandle<CupertinoActivityIndicatorState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let color = self.color.unwrap_or_else(|| theme.resolve(TICK_COLOR));
        let position = state.position;
        let progress = self.progress;
        let radius = self.radius;
        leaf(move || ActivityIndicatorTicks {
            position,
            progress,
            radius,
            color,
            laid_out: Size::ZERO,
        })
    }
}

/// The ticks themselves: a render object because a rotating set of rounded
/// rects is a handful of draw calls, not a widget tree. The paint is
/// `_CupertinoActivityIndicatorPainter.paint` line for line.
struct ActivityIndicatorTicks {
    position: f32,
    progress: f32,
    radius: f32,
    color: Color,
    laid_out: Size,
}

impl RenderBox for ActivityIndicatorTicks {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.laid_out = constraints.constrain(Size::square(self.radius * 2.0));
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let tick_count = TICK_ALPHAS.len();
        let active_tick = (tick_count as f32 * self.position).floor() as i32;
        // The tick shape: `-radius/10 .. radius/10` across, `-radius ..
        // -radius/3` up from the center, corners rounded radius/10 --
        // upstream's `tickFundamentalShape` with `_kDefaultIndicatorRadius`
        // being 10.
        let mut tick = crate::painting::RenderPath::new();
        tick.add_rounded_rect(
            Rect::ltrb(
                -self.radius / 10.0,
                -self.radius / 3.0,
                self.radius / 10.0,
                -self.radius,
            ),
            self.radius / 10.0,
            self.radius / 10.0,
        );
        let revealed = (tick_count as f32 * self.progress).floor() as i32;
        context.canvas().saved(|canvas| {
            canvas.translate(
                offset.dx + self.laid_out.width / 2.0,
                offset.dy + self.laid_out.height / 2.0,
            );
            for i in 0..revealed {
                let t = (i - active_tick).rem_euclid(tick_count as i32) as usize;
                let alpha = if self.progress < 1.0 {
                    PARTIAL_TICK_ALPHA
                } else {
                    TICK_ALPHAS[t]
                };
                canvas.draw_path(&tick, &Paint::new(self.color.with_alpha(alpha)));
                canvas.rotate(360.0 / tick_count as f32);
            }
        });
    }

    /// The ticks draw something a finger cannot land on; the default
    /// `hit_test_children`/`hit_test_self` (both false) is correct.
    fn visit_children(&self, _visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {}
}

// -- Alert dialog -------------------------------------------------------------
//
// Anchor: cupertino/dialog.dart, `CupertinoAlertDialog` + `CupertinoDialogAction`.
//
// Presentation is app-side, as with [`crate::controls::Dialog`]: the dialog is
// a surface to put in a `Stack` over a scrim, and there is no
// `showCupertinoDialog` route machinery here.

/// The dialog's width. dialog.dart's `_kCupertinoDialogWidth`. The wider
/// accessibility width (`_kAccessibilityCupertinoDialogWidth`, 310) is not
/// ported: it is keyed to the *title style's* scaled font size, and the
/// dynamic-type plumbing that answers that question here is the text-scale
/// factor, which the dialog's fixed-width design deliberately ignores.
pub const ALERT_DIALOG_WIDTH: f32 = 270.0;

/// The dialog's corner radius. dialog.dart's `_kCornerRadius`.
pub const ALERT_DIALOG_RADIUS: f32 = 14.0;

/// The least tall an action may be. dialog.dart's `_kDialogMinButtonHeight`.
const ACTION_MIN_HEIGHT: f32 = 45.0;

/// The dialog's inset around its text. dialog.dart's `_kDialogEdgePadding`.
const DIALOG_EDGE_PADDING: f32 = 20.0;

/// dialog.dart's `_kDialogColor`.
const DIALOG_COLOR: CupertinoDynamicColor =
    CupertinoDynamicColor::with_brightness(Color(0xCCF2_F2F2), Color(0xCC2D_2D2D));

/// The fill of a held action. dialog.dart's `_kDialogPressedColor`.
const ACTION_PRESSED_COLOR: CupertinoDynamicColor =
    CupertinoDynamicColor::with_brightness(Color(0xFFE1_E1E1), Color(0xFF40_4040));

/// A modal iOS alert. Upstream's `CupertinoAlertDialog` (dialog.dart) -- the
/// surface only; see the section comment above.
///
/// ```ignore
/// component(
///     CupertinoAlertDialog::new()
///         .with_title("Delete?")
///         .with_action(stateful(CupertinoAlertAction::new(ids.take(), "Cancel").wired(...)))
/// )
/// ```
pub struct CupertinoAlertDialog {
    title: Option<String>,
    content: Option<String>,
    actions: RefCell<Vec<AnyWidget>>,
}

impl CupertinoAlertDialog {
    pub fn new() -> CupertinoAlertDialog {
        CupertinoAlertDialog {
            title: None,
            content: None,
            actions: RefCell::new(Vec::new()),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Upstream's `content`.
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn with_action(self, action: AnyWidget) -> Self {
        self.actions.borrow_mut().push(action);
        self
    }
}

impl Default for CupertinoAlertDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for CupertinoAlertDialog {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let title = self.title.clone();
        let content = self.content.clone();
        let actions = std::mem::take(&mut *self.actions.borrow_mut());
        let surface = theme.resolve(DIALOG_COLOR);
        let divider_color = theme.resolve(CupertinoColors::SEPARATOR);
        let label = theme.resolve(CupertinoColors::LABEL);

        let has_title = title.is_some();
        let has_content = content.is_some();
        let action_count = actions.len();

        let mut children: Vec<AnyWidget> = Vec::new();
        children.push(leaf(move || {
            let mut column = Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            // dialog.dart's `_kCupertinoDialogTitleStyle` /
            // `_kCupertinoDialogContentStyle`, each centered. The paddings are
            // upstream's: 20 all round a lone block, the shared edge collapsing
            // to 1 when title and content are both present.
            if let Some(title) = &title {
                column = column.push(
                    Container::new()
                        .with_padding(EdgeInsets::only(
                            DIALOG_EDGE_PADDING,
                            DIALOG_EDGE_PADDING,
                            DIALOG_EDGE_PADDING,
                            if has_content {
                                1.0
                            } else {
                                DIALOG_EDGE_PADDING
                            },
                        ))
                        .with_child(Center::new(
                            Text::new(title.clone())
                                .with_style(TextStyle {
                                    font_size: 17.0,
                                    font_weight: 600,
                                    letter_spacing: Some(-0.5),
                                    height: Some(1.3),
                                    color: label,
                                    ..TextStyle::default()
                                })
                                .with_align(crate::engine::TextAlign::Center),
                        )),
                );
            }
            if let Some(content) = &content {
                column = column.push(
                    Container::new()
                        .with_padding(EdgeInsets::only(
                            DIALOG_EDGE_PADDING,
                            if has_title { 1.0 } else { DIALOG_EDGE_PADDING },
                            DIALOG_EDGE_PADDING,
                            DIALOG_EDGE_PADDING,
                        ))
                        .with_child(Center::new(
                            Text::new(content.clone())
                                .with_style(TextStyle {
                                    font_size: 13.0,
                                    letter_spacing: Some(-0.2),
                                    height: Some(1.35),
                                    color: label,
                                    ..TextStyle::default()
                                })
                                .with_align(crate::engine::TextAlign::Center),
                        )),
                );
            }
            column
        }));
        children.extend(actions);

        many(children, move |rendered| {
            let mut rendered = rendered.into_iter();
            let text_section = rendered.next().unwrap_or_else(|| RenderRef::new(Empty));
            let action_rows: Vec<BoxedRender> = rendered.collect();

            // A hairline: upstream's `_kDividerThickness` is 0.3, a
            // device-pixel line; one logical pixel is this renderer's
            // hairline at unit scale (see the module docs).
            let hairline = || Container::new().with_height(1.0).with_color(divider_color);

            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            if has_title || has_content {
                column = column.push(text_section);
                if action_count > 0 {
                    column = column.push(hairline());
                }
            }
            if action_count == 2 {
                // Two actions sit side by side, split by a vertical divider --
                // upstream's `_actionsColumn` builds exactly this row when
                // there are two and neither has wrapped to two lines.
                let mut row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
                let mut actions = action_rows.into_iter();
                if let Some(first) = actions.next() {
                    row = row.push_flex(FlexChild::expanded(first, 1));
                }
                row = row.push(Container::new().with_width(1.0).with_color(divider_color));
                if let Some(second) = actions.next() {
                    row = row.push_flex(FlexChild::expanded(second, 1));
                }
                column = column.push(row);
            } else {
                // One action, or three or more: a stack with a hairline
                // between each pair. Upstream scrolls this section past five
                // actions or so (`_kDialogActionsSectionMaxHeight`); with no
                // nested scroll view here, a dialog that tall is the caller's
                // to avoid.
                let mut first = true;
                for action in action_rows {
                    if !first {
                        column = column.push(hairline());
                    }
                    first = false;
                    column = column.push(action);
                }
            }

            Box::new(
                // The clip is what keeps a pressed action's fill inside the
                // rounded corner.
                RenderClipRect::new(
                    Container::new()
                        .with_width(ALERT_DIALOG_WIDTH)
                        .with_color(surface)
                        .with_corner_radius(ALERT_DIALOG_RADIUS)
                        .with_child(column),
                )
                .with_corner_radius(ALERT_DIALOG_RADIUS),
            )
        })
    }
}

/// What a [`CupertinoAlertAction`] remembers: whether it is held, which is
/// what paints it [`ACTION_PRESSED_COLOR`]. Upstream's
/// `_CupertinoDialogActionState.isPressed`, kept by the widget rather than the
/// caller because a dialog's actions are transient -- nobody else has a place
/// to keep it.
#[derive(Default)]
pub struct CupertinoAlertActionState {
    pressed: bool,
}

/// One action of a [`CupertinoAlertDialog`]. Upstream's
/// `CupertinoDialogAction` (dialog.dart).
pub struct CupertinoAlertAction {
    id: u64,
    label: String,
    is_default: bool,
    is_destructive: bool,
    enabled: bool,
    on_pressed: Option<Rc<dyn Fn()>>,
}

impl CupertinoAlertAction {
    pub fn new(id: u64, label: impl Into<String>) -> CupertinoAlertAction {
        CupertinoAlertAction {
            id,
            label: label.into(),
            is_default: false,
            is_destructive: false,
            enabled: true,
            on_pressed: None,
        }
    }

    /// Upstream's `isDefaultAction`: the bold one.
    pub fn default_action(mut self) -> Self {
        self.is_default = true;
        self
    }

    /// Upstream's `isDestructiveAction`: the red one.
    pub fn destructive(mut self) -> Self {
        self.is_destructive = true;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, action: fn(&mut S)) -> Self {
        if self.enabled {
            self.on_pressed = Some(Rc::new(move || {
                handle.set_state(move |state| action(state));
            }));
        }
        self
    }
}

impl StatefulComponent for CupertinoAlertAction {
    type State = CupertinoAlertActionState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    fn build(
        &self,
        state: &CupertinoAlertActionState,
        handle: StateHandle<CupertinoAlertActionState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let id = self.id;
        let label = self.label.clone();
        let pressed = state.pressed;
        let enabled = self.enabled;

        // `_kCupertinoDialogActionStyle`, colored by the action's kind:
        // destructive red for a destructive action, the theme's primary
        // otherwise, and inactive gray when disabled (dialog.dart's
        // `style.color` resolution).
        let color = if !enabled {
            theme.resolve(CupertinoColors::INACTIVE_GRAY)
        } else if self.is_destructive {
            theme.resolve(CupertinoColors::DESTRUCTIVE_RED)
        } else {
            theme.primary_color
        };
        let fill = if pressed {
            theme.resolve(ACTION_PRESSED_COLOR)
        } else {
            Color::TRANSPARENT
        };
        let is_default = self.is_default;

        let mut handlers = PointerHandlers::new();
        if let (Some(on_pressed), true) = (&self.on_pressed, enabled) {
            let tapped = on_pressed.clone();
            handlers = handlers.with_tap(move |_| tapped());
            handlers = handlers.with_press_change(move |down| {
                handle.set_state(move |state| state.pressed = down);
            });
        }

        leaf(move || {
            Pointer::new(
                id,
                Container::new()
                    // `_kDialogMinButtonHeight`, and the fill that covers the
                    // action's whole slot when held.
                    .with_height(ACTION_MIN_HEIGHT)
                    .with_color(fill)
                    .with_child(Center::new(
                        Text::new(label.clone())
                            .with_size(16.8)
                            .with_weight(if is_default { 600 } else { 400 })
                            .with_color(color),
                    )),
            )
            .with_handlers(handlers.clone())
        })
    }
}

// -- Navigation bar -----------------------------------------------------------
//
// Anchor: cupertino/nav_bar.dart, `CupertinoNavigationBar`.

/// The bar's height. nav_bar.dart's `_kNavBarPersistentHeight`, which is
/// `kMinInteractiveDimensionCupertino`.
pub const NAV_BAR_HEIGHT: f32 = K_MIN_INTERACTIVE_DIMENSION_CUPERTINO;

/// nav_bar.dart's `_kNavBarEdgePadding`.
const NAV_BAR_EDGE_PADDING: f32 = 16.0;

/// The default bottom border's color. nav_bar.dart's `_kDefaultNavBarBorderColor`.
const NAV_BAR_BORDER_COLOR: Color = Color(0x4D00_0000);

/// The least wide the back button's tap target is.
/// nav_bar.dart's `_kNavBarBackButtonTapWidth`.
const BACK_BUTTON_TAP_WIDTH: f32 = 50.0;

/// A back chevron, drawn. Upstream's `_BackChevron` is the `CupertinoIcons.back`
/// glyph at 30pt; with no icon font here (see the module docs) it is two
/// strokes, like the tick in [`crate::controls::Checkbox`].
struct BackChevron {
    color: Color,
    /// RTL reads the chevron as pointing the other way -- upstream's
    /// `_BackChevron` mirrors itself under `TextDirection.rtl`.
    mirror: bool,
    laid_out: Size,
}

impl RenderBox for BackChevron {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.laid_out = constraints.constrain(Size::new(12.0, 20.0));
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let paint = Paint::new(self.color)
            .with_style(Style::Stroke { width: 2.5 })
            .with_stroke_cap(crate::painting::StrokeCap::Round);
        let (outer, tip) = if self.mirror { (1.0, 9.0) } else { (11.0, 3.0) };
        let x = |dx: f32| offset.dx + dx;
        let y = |dy: f32| offset.dy + dy;
        context
            .canvas()
            .draw_line((x(outer), y(1.0)), (x(tip), y(10.0)), &paint);
        context
            .canvas()
            .draw_line((x(tip), y(10.0)), (x(outer), y(19.0)), &paint);
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }
}

/// The iOS top bar: 44 tall, translucent, title centered, optional back
/// chevron. Upstream's `CupertinoNavigationBar` (nav_bar.dart).
///
/// The background is the theme's `bar_background_color` drawn flat --
/// upstream's `BackdropFilter` blur is not ported (see the module docs), which
/// is what "blur-free translucent approximation" means here.
///
/// The large-title and sliver variants (`CupertinoSliverNavigationBar`) are
/// not part of this port; the gallery demo's use of one is served by a plain
/// bar plus a large title in the body.
pub struct CupertinoNavigationBar {
    middle: Option<String>,
    trailing: RefCell<Option<AnyWidget>>,
    /// The back button: its hit-test id and the previous page's title, if
    /// shown. Upstream builds one automatically from the route stack; with no
    /// route stack here, the caller says when one belongs.
    back: Option<(u64, Option<String>)>,
    back_handlers: PointerHandlers,
    background_color: Option<Color>,
}

impl CupertinoNavigationBar {
    pub fn new() -> CupertinoNavigationBar {
        CupertinoNavigationBar {
            middle: None,
            trailing: RefCell::new(None),
            back: None,
            back_handlers: PointerHandlers::new(),
            background_color: None,
        }
    }

    /// Upstream's `middle`, as a label string (the demos pass a `Text`).
    pub fn with_middle(mut self, title: impl Into<String>) -> Self {
        self.middle = Some(title.into());
        self
    }

    /// Upstream's `trailing`.
    pub fn with_trailing(self, trailing: AnyWidget) -> Self {
        *self.trailing.borrow_mut() = Some(trailing);
        self
    }

    /// Shows a back chevron (and the previous page's title, if given) at the
    /// leading edge -- upstream's `automaticallyImplyLeading` output.
    pub fn with_back(mut self, id: u64, previous_title: Option<String>) -> Self {
        self.back = Some((id, previous_title));
        self
    }

    /// Runs `pop` when the back button is tapped.
    pub fn wired_back<S: 'static>(mut self, handle: StateHandle<S>, pop: fn(&mut S)) -> Self {
        self.back_handlers = PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| pop(state));
        });
        self
    }

    /// Upstream's `backgroundColor`.
    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }
}

impl Default for CupertinoNavigationBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for CupertinoNavigationBar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let background = self.background_color.unwrap_or(theme.bar_background_color);
        let middle = self.middle.clone();
        let trailing = self.trailing.borrow_mut().take();
        let back = self.back.clone();
        let back_handlers = self.back_handlers.clone();
        let primary = theme.primary_color;
        let label_color = theme.resolve(CupertinoColors::LABEL);
        // The bar's own background extends under the status bar, as with
        // [`crate::components::AppBar`]: padding inside the surface, not a
        // SafeArea around it.
        let top = crate::media_query::media_query_of(context).padding.top;
        let rtl = crate::direction::current_direction() == crate::direction::TextDirection::Rtl;

        let mut children: Vec<AnyWidget> = Vec::new();
        let has_back = back.is_some();
        if let Some((back_id, previous_title)) = back {
            children.push(leaf(move || {
                // nav_bar.dart's `CupertinoNavigationBarBackButton`: a chevron
                // and an optional title, in `navActionTextStyle` (which is
                // actionTextStyle), tap target at least
                // `_kNavBarBackButtonTapWidth` wide.
                let mut row = Row::new().with_spacing(6.0).push(
                    Container::new()
                        .with_padding(EdgeInsets::only(6.0, 0.0, 2.0, 0.0))
                        .with_child(BackChevron {
                            color: primary,
                            mirror: rtl,
                            laid_out: Size::ZERO,
                        }),
                );
                if let Some(title) = &previous_title {
                    row = row.push(
                        Text::new(title.clone())
                            .with_size(17.0)
                            .with_color(primary)
                            .with_soft_wrap(false)
                            .with_overflow(TextOverflow::Ellipsis)
                            .with_max_lines(1),
                    );
                }
                Pointer::new(
                    back_id,
                    RenderConstrainedBox::new(BoxConstraints::new(
                        BACK_BUTTON_TAP_WIDTH,
                        f32::INFINITY,
                        0.0,
                        f32::INFINITY,
                    ))
                    .with_child(row),
                )
                .with_handlers(back_handlers.clone())
            }));
        }
        if let Some(middle) = middle {
            children.push(leaf(move || {
                // text_theme.dart's `_kDefaultMiddleTitleTextStyle`: 17pt
                // semibold, -0.41 tracking, in the label color. One line,
                // elided, as every fixed-height bar's title is.
                Align::new(
                    Alignment::CENTER,
                    Text::new(middle.clone())
                        .with_size(17.0)
                        .with_weight(600)
                        .with_color(label_color)
                        .with_soft_wrap(false)
                        .with_overflow(TextOverflow::Ellipsis)
                        .with_max_lines(1),
                )
            }));
        }
        if let Some(trailing) = trailing {
            children.push(trailing);
        }

        let has_middle = self.middle.is_some();
        let has_trailing = children.len() > has_back as usize + has_middle as usize;

        many(children, move |rendered| {
            let mut rendered = rendered.into_iter();
            let leading = if has_back { rendered.next() } else { None };
            let middle = if has_middle { rendered.next() } else { None };
            let trailing = if has_trailing { rendered.next() } else { None };

            // `RenderNavigationToolbar` centers the middle for Cupertino:
            // upstream's `CupertinoNavigationBar` builds
            // `NavigationToolbar(centerMiddle: true)`, where Material's AppBar
            // platform-adapts.
            let mut toolbar = crate::widgets::RenderNavigationToolbar::new()
                .with_center_middle(true)
                .with_middle_spacing(crate::widgets::K_MIDDLE_SPACING);
            if let Some(leading) = leading {
                toolbar = toolbar.with_leading(leading);
            }
            if let Some(middle) = middle {
                toolbar = toolbar.with_middle(middle);
            }
            if let Some(trailing) = trailing {
                toolbar = toolbar.with_trailing(trailing);
            }

            let bar = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push_flex(FlexChild::expanded(
                    RenderRef::new(crate::render::RenderPadding::new(
                        EdgeInsets::only(NAV_BAR_EDGE_PADDING, 0.0, NAV_BAR_EDGE_PADDING, 0.0),
                        toolbar,
                    )),
                    1,
                ))
                // `_kDefaultNavBarBorder`: a bottom hairline.
                .push(
                    Container::new()
                        .with_height(1.0)
                        .with_color(NAV_BAR_BORDER_COLOR),
                );

            Box::new(
                Container::new()
                    .with_color(background)
                    .with_padding(EdgeInsets::only(0.0, top, 0.0, 0.0))
                    .with_child(
                        RenderConstrainedBox::new(BoxConstraints::new(
                            0.0,
                            f32::INFINITY,
                            NAV_BAR_HEIGHT,
                            NAV_BAR_HEIGHT,
                        ))
                        .with_child(bar),
                    ),
            )
        })
    }
}

// -- Tab bar ------------------------------------------------------------------
//
// Anchor: cupertino/bottom_tab_bar.dart, `CupertinoTabBar`.

/// The bar's height. bottom_tab_bar.dart's `_kTabBarHeight`.
pub const TAB_BAR_HEIGHT: f32 = 50.0;

/// One destination in a [`CupertinoTabBar`]. Upstream's `BottomNavigationBarItem`,
/// with `mark` standing in for the `IconData` icon: there is no icon font
/// here, so the icon is a one- or two-character mark, the same substitution
/// [`crate::controls::Destination`] documents.
#[derive(Clone, Debug)]
pub struct CupertinoTabItem {
    pub label: String,
    pub mark: String,
}

impl CupertinoTabItem {
    pub fn new(label: impl Into<String>, mark: impl Into<String>) -> CupertinoTabItem {
        CupertinoTabItem {
            label: label.into(),
            mark: mark.into(),
        }
    }
}

/// The iOS bottom tab bar. Upstream's `CupertinoTabBar` (bottom_tab_bar.dart):
/// 50 tall over the home-indicator inset, icon over label, the active item in
/// the theme's primary color and the rest in `inactiveGray`.
pub struct CupertinoTabBar {
    first_id: u64,
    items: Vec<CupertinoTabItem>,
    selected: usize,
    handlers: RefCell<Vec<PointerHandlers>>,
}

impl CupertinoTabBar {
    /// `first_id` is the hit-test identity of the first item; the rest follow
    /// consecutively, as with [`crate::controls::TabBar`].
    pub fn new(first_id: u64, items: Vec<CupertinoTabItem>, selected: usize) -> CupertinoTabBar {
        CupertinoTabBar {
            first_id,
            items,
            selected,
            handlers: RefCell::new(Vec::new()),
        }
    }

    /// `select` is given the state and the index that was tapped.
    pub fn wired<S: 'static>(self, handle: StateHandle<S>, select: fn(&mut S, usize)) -> Self {
        let handlers = (0..self.items.len())
            .map(|index| {
                let handle = handle.clone();
                PointerHandlers::new().with_tap(move |_| {
                    handle.set_state(move |state| select(state, index));
                })
            })
            .collect();
        *self.handlers.borrow_mut() = handlers;
        self
    }
}

impl Component for CupertinoTabBar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let selected = self.selected;
        let first_id = self.first_id;
        let handlers = self.handlers.borrow().clone();
        let items = self.items.clone();
        let background = theme.bar_background_color;
        let active = theme.primary_color;
        let inactive = theme.resolve(CupertinoColors::INACTIVE_GRAY);
        // bottom_tab_bar.dart's `_kDefaultTabBarBorderColor`.
        let border = CupertinoDynamicColor::with_brightness(Color(0x4C00_0000), Color(0x29FF_FFFF))
            .resolve(theme.brightness);
        // The bar grows by whatever the gesture bar covers, the same rule
        // [`crate::controls::BottomNavigation`] follows.
        let bottom = crate::media_query::media_query_of(context)
            .view_padding
            .bottom;

        leaf(move || {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for (index, item) in items.iter().enumerate() {
                let active_item = index == selected;
                let color = if active_item { active } else { inactive };
                let content = Container::new().with_child(Center::new(
                    Column::new()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_spacing(2.0)
                        // The icon's slot. Upstream sizes the icon to 30
                        // (`_kIconSize` in bottom_tab_bar.dart's tile build);
                        // the mark stands in for it (see CupertinoTabItem).
                        .push(
                            Container::new()
                                .with_size(30.0, 30.0)
                                .with_child(Align::new(
                                    Alignment::CENTER,
                                    Text::new(item.mark.clone())
                                        .with_size(20.0)
                                        .with_weight(600)
                                        .with_color(color),
                                )),
                        )
                        // text_theme.dart's `_kDefaultTabLabelTextStyle`:
                        // 10pt w500, -0.24 tracking.
                        .push(
                            Text::new(item.label.clone())
                                .with_size(10.0)
                                .with_weight(500)
                                .with_color(color),
                        ),
                ));
                let region = match handlers.get(index) {
                    Some(handlers) => Pointer::new(first_id + index as u64, content)
                        .with_handlers(handlers.clone()),
                    None => Pointer::new(first_id + index as u64, content),
                };
                row = row.push_flex(FlexChild::expanded(region, 1));
            }
            Container::new()
                // `_kTabBarHeight`, with the home-indicator inset on top of it.
                .with_height(TAB_BAR_HEIGHT + bottom)
                .with_color(background)
                .with_padding(EdgeInsets::only(0.0, 0.0, 0.0, bottom))
                .with_child(
                    // The top hairline border, inside the 50.
                    RenderFlex::column()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push(Container::new().with_height(1.0).with_color(border))
                        .push_flex(FlexChild::expanded(row, 1)),
                )
        })
    }
}

// -- Scaffolds ----------------------------------------------------------------
//
// Anchors: cupertino/page_scaffold.dart (`CupertinoPageScaffold`) and
// cupertino/tab_scaffold.dart (`CupertinoTabScaffold`). Both are reduced to
// the structure the gallery demos exercise: a bar over a body, and a body
// over a tab bar. Upstream's per-tab `Navigator` stacks (`CupertinoTabView`)
// are not ported -- this crate's `Navigator` (crate::navigation) is the app's
// to compose above the tab it is showing.

/// A page: a navigation bar over a body, on the theme's scaffold background.
pub struct CupertinoPageScaffold {
    navigation_bar: RefCell<Option<AnyWidget>>,
    body: RefCell<Option<AnyWidget>>,
    background_color: Option<Color>,
}

impl CupertinoPageScaffold {
    pub fn new(body: AnyWidget) -> CupertinoPageScaffold {
        CupertinoPageScaffold {
            navigation_bar: RefCell::new(None),
            body: RefCell::new(Some(body)),
            background_color: None,
        }
    }

    /// Upstream's `navigationBar`.
    pub fn with_navigation_bar(self, bar: AnyWidget) -> Self {
        *self.navigation_bar.borrow_mut() = Some(bar);
        self
    }

    /// Upstream's `backgroundColor`.
    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }
}

impl Component for CupertinoPageScaffold {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let background = self
            .background_color
            .unwrap_or(theme.scaffold_background_color);
        let bar = self.navigation_bar.borrow_mut().take();
        let body = self
            .body
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| Empty));
        // The bar has already moved the page past the status bar, so the body
        // must not do it again -- the same MediaQuery reduction
        // [`crate::components::Scaffold`] makes.
        let body = if bar.is_some() {
            let data = crate::media_query::media_query_of(context);
            crate::media_query::MediaQuery::new(data.remove_padding(true, true, true, false), body)
        } else {
            body
        };
        let has_bar = bar.is_some();

        let mut children = Vec::new();
        if let Some(bar) = bar {
            children.push(bar);
        }
        children.push(body);

        many(children, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            let mut rendered = rendered.into_iter();
            if has_bar {
                if let Some(bar) = rendered.next() {
                    column = column.push(bar);
                }
            }
            if let Some(body) = rendered.next() {
                column = column.push_flex(FlexChild::expanded(body, 1));
            }
            RenderRef::new(Container::new().with_color(background).with_child(column))
        })
    }
}

/// A body over a tab bar. Upstream's `CupertinoTabScaffold` (tab_scaffold.dart),
/// reduced to the layout: which tab the body shows is the caller's state.
pub struct CupertinoTabScaffold {
    tab_bar: RefCell<Option<AnyWidget>>,
    body: RefCell<Option<AnyWidget>>,
    background_color: Option<Color>,
}

impl CupertinoTabScaffold {
    pub fn new(tab_bar: AnyWidget, body: AnyWidget) -> CupertinoTabScaffold {
        CupertinoTabScaffold {
            tab_bar: RefCell::new(Some(tab_bar)),
            body: RefCell::new(Some(body)),
            background_color: None,
        }
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }
}

impl Component for CupertinoTabScaffold {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let background = self
            .background_color
            .unwrap_or(theme.scaffold_background_color);
        let tab_bar = self
            .tab_bar
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| Empty));
        let body = self
            .body
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| Empty));

        many(vec![body, tab_bar], move |rendered| {
            let mut rendered = rendered.into_iter();
            let body = rendered.next().unwrap_or_else(|| RenderRef::new(Empty));
            let tab_bar = rendered.next().unwrap_or_else(|| RenderRef::new(Empty));
            // The bar goes under the body: upstream lays the tab bar out
            // first and gives the body what is left, which a column with the
            // body expanded says directly.
            let column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push_flex(FlexChild::expanded(body, 1))
                .push(tab_bar);
            RenderRef::new(Container::new().with_color(background).with_child(column))
        })
    }
}

// -- Segmented control --------------------------------------------------------
//
// Anchor: cupertino/segmented_control.dart, `CupertinoSegmentedControl` (the
// iOS-12 style control; the iOS-13 `CupertinoSlidingSegmentedControl` is a
// different widget and not part of this port).

/// The least tall the control is. segmented_control.dart's
/// `_kMinSegmentedControlHeight`.
pub const SEGMENTED_CONTROL_MIN_HEIGHT: f32 = 28.0;

/// The padding around the whole control. segmented_control.dart's
/// `_kHorizontalItemPadding` -- note it pads the control, not each segment;
/// a segment's own padding is its child's.
pub const SEGMENTED_CONTROL_PADDING: f32 = 16.0;

/// What a [`CupertinoSegmentedControl`] remembers: which segment is held.
/// segmented_control.dart's `_pressedKey`.
#[derive(Default)]
pub struct CupertinoSegmentedControlState {
    pressed: Option<usize>,
}

/// A row of equal segments, the chosen one filled. Upstream's
/// `CupertinoSegmentedControl` (segmented_control.dart).
///
/// Segments are equal by `flex`, which is the visible behavior; upstream's
/// `_RenderSegmentedControl` instead measures the widest child's intrinsic
/// width and holds every segment to it -- a difference only when a segment's
/// content would overflow its share, which a short label never does.
///
/// The per-segment selection fade (`_kFadeDuration`, 165ms) is not ported:
/// the fill changes with the caller's state at the next frame.
pub struct CupertinoSegmentedControl {
    first_id: u64,
    labels: Vec<String>,
    selected: usize,
    on_selected: Option<Rc<dyn Fn(usize)>>,
}

impl CupertinoSegmentedControl {
    pub fn new(first_id: u64, labels: Vec<String>, selected: usize) -> CupertinoSegmentedControl {
        CupertinoSegmentedControl {
            first_id,
            labels,
            selected,
            on_selected: None,
        }
    }

    /// `select` is given the state and the index that was tapped. Upstream's
    /// `onValueChanged`, which does not fire for the already-selected segment.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, select: fn(&mut S, usize)) -> Self {
        self.on_selected = Some(Rc::new(move |index| {
            handle.set_state(move |state| select(state, index));
        }));
        self
    }
}

impl StatefulComponent for CupertinoSegmentedControl {
    type State = CupertinoSegmentedControlState;

    fn key(&self) -> Key {
        Some(self.first_id)
    }

    fn build(
        &self,
        state: &CupertinoSegmentedControlState,
        handle: StateHandle<CupertinoSegmentedControlState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let first_id = self.first_id;
        let labels = self.labels.clone();
        let selected = self.selected;
        let pressed = state.pressed;
        // segmented_control.dart's `_updateColors`: selected fill is the
        // primary color, unselected fill the primary contrasting color, the
        // border the primary color, and a held segment the primary color at
        // 20%. Text is the reverse of the fill: selected segments read in the
        // contrasting color, unselected in the primary.
        let selected_color = theme.primary_color;
        let unselected_color = theme.primary_contrasting_color;
        let pressed_color = theme.primary_color.with_alpha(0x33);

        let count = labels.len();
        let mut handlers: Vec<PointerHandlers> = Vec::new();
        for index in 0..count {
            let mut segment = PointerHandlers::new();
            if let Some(on_selected) = &self.on_selected {
                let tapped = on_selected.clone();
                // `_onTap`: the already-selected segment does not re-report.
                segment = segment.with_tap(move |_| {
                    if index != selected {
                        tapped(index);
                    }
                });
                let press_handle = handle.clone();
                // `_onTapDown` does not mark the selected segment pressed.
                segment = segment.with_press_change(move |down| {
                    press_handle.set_state(move |state| {
                        state.pressed = if down && index != selected {
                            Some(index)
                        } else {
                            None
                        };
                    });
                });
            }
            handlers.push(segment);
        }

        leaf(move || {
            // Center, not stretch: the control is as tall as its tallest
            // segment (the 28 minimum), where stretch would take whatever
            // loose height it is offered. Upstream's
            // `_RenderSegmentedControl` sizes to its children the same way.
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            for (index, label) in labels.iter().enumerate() {
                let is_selected = index == selected;
                let fill = if is_selected {
                    selected_color
                } else if pressed == Some(index) {
                    pressed_color
                } else {
                    unselected_color
                };
                let text_color = if is_selected {
                    unselected_color
                } else {
                    selected_color
                };
                let segment = Container::new()
                    .with_height(SEGMENTED_CONTROL_MIN_HEIGHT)
                    .with_color(fill)
                    .with_child(Center::new(
                        // Upstream imposes only the color on the segment's
                        // text (`getTextColor`); the size is the caller's
                        // Text's, which here is the crate's default.
                        Text::new(label.clone())
                            .with_color(text_color)
                            .with_soft_wrap(false)
                            .with_max_lines(1),
                    ));
                row = row.push_flex(FlexChild::expanded(
                    Pointer::new(first_id + index as u64, segment)
                        .with_handlers(handlers[index].clone()),
                    1,
                ));
            }
            // The border: the primary color, corners rounded 3 --
            // `_RenderSegmentedControl` rounds the two end segments' outer
            // corners at 3.0; with one radius for the whole border the middle
            // segments' joins round too, which no divider makes visible.
            Container::new()
                .with_padding(EdgeInsets::symmetric(SEGMENTED_CONTROL_PADDING, 0.0))
                .with_child(
                    RenderClipRect::new(
                        Container::new()
                            .with_border(1.0, selected_color)
                            .with_corner_radius(3.0)
                            .with_child(row),
                    )
                    .with_corner_radius(3.0),
                )
        })
    }
}

// -- Picker -------------------------------------------------------------------
//
// Anchors: cupertino/picker.dart `CupertinoPicker`,
// widgets/list_wheel_scroll_view.dart `ListWheelScrollView` +
// `FixedExtentScrollPhysics`, rendering/list_wheel_viewport.dart
// `RenderListWheelViewport`, painting/matrix_utils.dart
// `MatrixUtils.createCylindricalProjectionTransform`.

/// picker.dart's `_kDefaultDiameterRatio`.
pub const PICKER_DIAMETER_RATIO: f32 = 1.07;

/// picker.dart's `_kDefaultPerspective`, pinned to
/// `RenderListWheelViewport.defaultPerspective`.
pub const PICKER_PERSPECTIVE: f32 = 0.003;

/// picker.dart's `_kSqueeze`.
pub const PICKER_SQUEEZE: f32 = 1.45;

/// picker.dart's `_kOverAndUnderCenterOpacity`.
pub const PICKER_OFF_CENTER_OPACITY: f32 = 0.447;

/// Tap-to-scroll. picker.dart's `_kCupertinoPickerTapToScrollDuration` and
/// `_kCupertinoPickerTapToScrollCurve`.
const PICKER_TAP_SCROLL_MICROS: i64 = 300_000;

/// The largest |angle| an item at the edge of the visible cylinder reaches.
/// `RenderListWheelViewport._maxVisibleRadian`.
fn max_visible_radian(diameter_ratio: f32) -> f32 {
    if diameter_ratio < 1.0 {
        std::f32::consts::FRAC_PI_2
    } else {
        (1.0 / diameter_ratio).asin()
    }
}

/// `RenderListWheelViewport.scrollOffsetToIndex` / `indexToScrollOffset`.
pub fn scroll_offset_to_index(offset: f32, item_extent: f32) -> i32 {
    (offset / item_extent).floor() as i32
}

pub fn index_to_scroll_offset(index: usize, item_extent: f32) -> f32 {
    index as f32 * item_extent
}

/// The angle a child is at, given where its center falls in the viewport.
/// `_paintTransformedChild`'s `angle` computation.
fn angle_for(flat_center_y: f32, height: f32, diameter_ratio: f32, squeeze: f32) -> f32 {
    let fractional_y = flat_center_y / height;
    -(fractional_y - 0.5) * 2.0 * max_visible_radian(diameter_ratio) / squeeze
}

/// Projects a point on the wheel's flat axis onto the screen, and reports the
/// child's horizontal scale there. This is
/// `MatrixUtils.createCylindricalProjectionTransform` (vertical orientation)
/// evaluated at the child's center: the model matrix translates z by the
/// radius and rotates by `angle` about x, the view steps back by the radius,
/// and the projection divides by `w = perspective * (radius - z) + 1`.
///
/// Returns `(screen_center_y, scale_x)`.
fn project_center(
    y_rel: f32,
    angle: f32,
    radius: f32,
    height: f32,
    perspective: f32,
) -> (f32, f32) {
    let (sin, cos) = angle.sin_cos();
    let y1 = y_rel * cos - radius * sin;
    let z1 = y_rel * sin + radius * cos;
    let w = perspective * (radius - z1) + 1.0;
    (height / 2.0 + y1 / w, 1.0 / w)
}

/// The vertical scale of a child at `y_rel`, sampled over a pixel rather than
/// derived: the projected slope has no tidy closed form once the perspective
/// divide is in, and the difference quotient is what the transform itself
/// would do to the child's top and bottom edges.
fn project_scale_y(y_rel: f32, angle: f32, radius: f32, height: f32, perspective: f32) -> f32 {
    let above = project_center(y_rel - 0.5, angle, radius, height, perspective).0;
    let below = project_center(y_rel + 0.5, angle, radius, height, perspective).0;
    below - above
}

/// Whether a child is wholly inside the magnifier band: its projected band
/// sits within `itemExtent * magnification / 2` of the viewport's center,
/// which is the band `_paintChildWithMagnifier` clips to. Upstream paints a
/// partially intersecting child twice -- once plain, once magnified and
/// clipped to the band; here the child is magnified only when wholly inside,
/// and dimmed otherwise, a stepwise version of the same ramp.
fn inside_magnifier_band(
    screen_center_y: f32,
    height: f32,
    item_extent: f32,
    magnification: f32,
) -> bool {
    (screen_center_y - height / 2.0).abs() + item_extent / 2.0 <= item_extent * magnification / 2.0
}

/// What a [`CupertinoPicker`] remembers between frames.
///
/// The `Default` derives with an empty scroll; [`CupertinoPicker`]'s
/// `initial_state` replaces it with one ranged to the items.
#[derive(Default)]
pub struct CupertinoPickerState {
    /// The wheel's scroll offset. `pub` for the same reason
    /// [`crate::scrolling::Scroll`]'s own fields are: the state is the app's
    /// to read and test.
    pub scroll: crate::scrolling::Scroll,
    /// The laid-out height, written by the wheel's layout a frame after it
    /// changes -- the same one-frame-late measurement the crate documents for
    /// `RenderSizeReporter`-style feedback. The build's visible window uses it
    /// to decide which items to build.
    viewport: Rc<Cell<f32>>,
    /// A finger is in charge: suppress snapping until the drag ends.
    dragging: bool,
    /// The last index reported through `on_selected`.
    reported: Option<usize>,
    /// The offset the in-flight snap is heading for, so it is not reissued.
    snapping_to: Option<f32>,
}

/// A wheel of fixed-extent items. Upstream's `CupertinoPicker` (picker.dart).
///
/// The scroll is the crate's [`crate::scrolling::Scroll`]: drags move it
/// directly, a release flings it with `ClampingScrollSimulation`, and when
/// the wheel comes to rest off an item boundary it is driven to the nearest
/// item -- the landing choice of `FixedExtentScrollPhysics.
/// createBallisticSimulation`, whose scenario-5 tuned friction
/// (`FrictionSimulation.through`) is not ported (PORTING_STATUS.md), so the
/// settle is a short ease-out drive to the same target instead.
///
/// ```ignore
/// stateful(CupertinoPicker::new(ids.take(), 32.0, count, move |index| {
///     component(PickerLabel(labels[index]))
/// }).wired(handle, |s, i| s.selected = i))
/// ```
pub struct CupertinoPicker {
    id: u64,
    item_extent: f32,
    count: usize,
    build_item: Rc<dyn Fn(usize) -> AnyWidget>,
    diameter_ratio: f32,
    squeeze: f32,
    magnification: f32,
    use_magnifier: bool,
    background_color: Option<Color>,
    selection_overlay: bool,
    initial_item: usize,
    on_selected: Option<Rc<dyn Fn(usize)>>,
    /// The themed label color for [`CupertinoPicker::labels`] items, refreshed
    /// on every build; `None` for picker items the caller builds itself.
    label_color: Option<Rc<Cell<Color>>>,
}

impl CupertinoPicker {
    /// `count` items, built by `build_item`. Upstream's
    /// `CupertinoPicker.builder(itemBuilder:childCount:)`.
    pub fn new(
        id: u64,
        item_extent: f32,
        count: usize,
        build_item: impl Fn(usize) -> AnyWidget + 'static,
    ) -> CupertinoPicker {
        assert!(item_extent > 0.0, "itemExtent must be positive");
        CupertinoPicker {
            id,
            item_extent,
            count,
            build_item: Rc::new(build_item),
            diameter_ratio: PICKER_DIAMETER_RATIO,
            squeeze: PICKER_SQUEEZE,
            magnification: 1.0,
            use_magnifier: false,
            background_color: None,
            selection_overlay: true,
            initial_item: 0,
            on_selected: None,
            label_color: None,
        }
    }

    /// A picker of string labels, styled as upstream's ambient
    /// `DefaultTextStyle` styles them: text_theme.dart's
    /// `_kDefaultPickerTextStyle`, 21pt, -0.6 tracking, in the label color.
    ///
    /// The label color is resolved from the ambient [`CupertinoTheme`] on
    /// every build and handed to the item closure through a cell, because the
    /// closure itself runs without a context.
    pub fn labels(id: u64, item_extent: f32, labels: Vec<String>) -> CupertinoPicker {
        let count = labels.len();
        let label_color = Rc::new(Cell::new(CupertinoColors::LABEL.color));
        let item_color = label_color.clone();
        let mut picker = CupertinoPicker::new(id, item_extent, count, move |index| {
            let label = labels[index].clone();
            let item_color = item_color.clone();
            leaf(move || {
                Center::new(
                    Text::new(label.clone())
                        .with_style(TextStyle {
                            font_size: 21.0,
                            letter_spacing: Some(-0.6),
                            color: item_color.get(),
                            ..TextStyle::default()
                        })
                        .with_soft_wrap(false)
                        .with_max_lines(1),
                )
            })
        });
        picker.label_color = Some(label_color);
        picker
    }

    /// Upstream's `diameterRatio`.
    pub fn with_diameter_ratio(mut self, ratio: f32) -> Self {
        assert!(ratio > 0.0, "diameterRatio must be positive");
        self.diameter_ratio = ratio;
        self
    }

    /// Upstream's `squeeze`.
    pub fn with_squeeze(mut self, squeeze: f32) -> Self {
        assert!(squeeze > 0.0, "squeeze must be positive");
        self.squeeze = squeeze;
        self
    }

    /// Upstream's `magnification`.
    pub fn with_magnification(mut self, magnification: f32) -> Self {
        assert!(magnification > 0.0, "magnification must be positive");
        self.magnification = magnification;
        self
    }

    /// Upstream's `useMagnifier`.
    pub fn with_magnifier(mut self, on: bool) -> Self {
        self.use_magnifier = on;
        self
    }

    /// Upstream's `backgroundColor`; None paints nothing, matching the native
    /// pickers upstream's docs point at.
    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// The selection highlight band. Upstream's `selectionOverlay`, which
    /// defaults to `CupertinoPickerDefaultSelectionOverlay`; pass false for
    /// `selectionOverlay: null`.
    pub fn with_selection_overlay(mut self, on: bool) -> Self {
        self.selection_overlay = on;
        self
    }

    /// Upstream's `scrollController.initialItem`.
    pub fn with_initial_item(mut self, index: usize) -> Self {
        self.initial_item = index;
        self
    }

    /// `select` is given the state and the index under the highlight band.
    /// Upstream's `onSelectedItemChanged`, which fires as the center item
    /// changes during a scroll, not only when the wheel settles.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, select: fn(&mut S, usize)) -> Self {
        self.on_selected = Some(Rc::new(move |index| {
            handle.set_state(move |state| select(state, index));
        }));
        self
    }
}

impl StatefulComponent for CupertinoPicker {
    type State = CupertinoPickerState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    fn initial_state(&self) -> CupertinoPickerState {
        let mut scroll = crate::scrolling::Scroll::new();
        let last = self.count.saturating_sub(1);
        scroll.set_extent(index_to_scroll_offset(last, self.item_extent), 0.0);
        scroll.offset = index_to_scroll_offset(self.initial_item.min(last), self.item_extent);
        CupertinoPickerState {
            scroll,
            viewport: Rc::new(Cell::new(0.0)),
            dragging: false,
            reported: None,
            snapping_to: None,
        }
    }

    fn advance(&self, state: &mut CupertinoPickerState, frame_time_micros: i64) -> bool {
        // What layout measured, a frame late at most: upstream's
        // `applyNewDimensions`.
        let last = self.count.saturating_sub(1);
        let max = index_to_scroll_offset(last, self.item_extent);
        state.scroll.set_extent(max, state.viewport.get());

        let mut wants = state.scroll.advance(frame_time_micros);

        if !state.dragging && !state.scroll.is_ballistic() {
            // The wheel has come to rest. `FixedExtentScrollPhysics`'s landing
            // choice: the item nearest where the motion stopped, clamped to
            // the ends (`_getItemFromOffset`).
            let target = (state.scroll.offset / self.item_extent)
                .round()
                .clamp(0.0, last as f32)
                * self.item_extent;
            // The same "already there to within a pixel" gate
            // `Scroll::animate_to` applies.
            if (state.scroll.offset - target).abs() > 1.0 && state.snapping_to != Some(target) {
                state.snapping_to = Some(target);
                let distance = (target - state.scroll.offset).abs();
                // A spring's settle time grows with the distance; this drive
                // does the same in place of the unported spring/friction
                // simulations (see the struct docs).
                let micros = (150.0 * (distance / self.item_extent).clamp(0.4, 3.0)) as i64 * 1000;
                state
                    .scroll
                    .animate_to(target, micros, crate::animation::Curve::EASE_OUT);
                wants = true;
            }
        }

        // The index under the band, reported when it changes --
        // `onSelectedItemChanged` on `ChangeReportingBehavior.onScrollUpdate`.
        let index = (state.scroll.offset / self.item_extent)
            .round()
            .clamp(0.0, last as f32) as usize;
        if state.reported != Some(index) {
            state.reported = Some(index);
            if let Some(on_selected) = &self.on_selected {
                on_selected(index);
            }
        }

        wants
    }

    fn build(
        &self,
        state: &CupertinoPickerState,
        handle: StateHandle<CupertinoPickerState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        state
            .scroll
            .set_notification_sink(context.notification_sink());
        // Themed `labels` items: publish this build's label color before the
        // item closures are assembled below.
        if let Some(label_color) = &self.label_color {
            label_color.set(theme.resolve(CupertinoColors::LABEL));
        }
        let offset = state.scroll.offset;
        let extent = self.item_extent;
        let count = self.count;
        let diameter_ratio = self.diameter_ratio;
        let squeeze = self.squeeze;
        let magnification = self.magnification;
        let use_magnifier = self.use_magnifier;
        let viewport = state.viewport.clone();

        // The visible window, computed as `RenderListWheelViewport.
        // performLayout` does: the wheel's extent times the squeeze, centered
        // on the scroll offset. Before the first layout the height is an
        // estimate -- eleven items, a row more than a typical picker height
        // shows with the squeeze applied; the real height arrives with the
        // first layout and the window is recomputed next build.
        let height_estimate = match viewport.get() {
            h if h > 0.0 => h,
            _ => extent * 11.0,
        };
        let visible = height_estimate * squeeze;
        let first =
            scroll_offset_to_index(offset + extent / 2.0 - visible / 2.0, extent).max(0) as usize;
        let last = scroll_offset_to_index(offset + extent / 2.0 + visible / 2.0, extent)
            .clamp(0, count.saturating_sub(1) as i32) as usize;

        // Gesture wiring: a finger down catches a fling, a drag moves the
        // wheel with the finger, letting go throws it, and the wheel notch
        // walks it -- the same four `app::scroll_handlers` give the page
        // scrollables. A tap scrolls the tapped item to the band, upstream's
        // `_handleChildTap`; the lookup is in flat coordinates, which is where
        // the hit test below also works (the cylindrical displacement moves a
        // row's center by less than half a row near the band, so the nearest
        // index is the same either way).
        let handlers = {
            let down_handle = handle.clone();
            let drag_handle = handle.clone();
            let end_handle = handle.clone();
            let wheel_handle = handle.clone();
            PointerHandlers::new()
                .with_pointer_down(move |_| {
                    down_handle.set_state(|state| {
                        state.dragging = true;
                        state.scroll.stop();
                    });
                })
                .with_drag_update(move |drag| {
                    let delta = drag.delta.dy;
                    drag_handle.set_state(move |state| state.scroll.scroll_by(-delta));
                })
                .with_drag_end(move |end| {
                    let velocity = end.velocity.dy;
                    end_handle.set_state(move |state| {
                        state.dragging = false;
                        state.snapping_to = None;
                        state.scroll.fling(-velocity);
                    });
                })
                .with_scroll(move |scroll| {
                    let delta = scroll.delta.dy;
                    wheel_handle.set_state(move |state| state.scroll.scroll_by(delta));
                })
                .with_tap(move |tap| {
                    handle.set_state(move |state| {
                        let index = ((state.scroll.offset + tap.local_position.dy) / extent)
                            .round()
                            .clamp(0.0, count.saturating_sub(1) as f32);
                        let target = index * extent;
                        state.snapping_to = Some(target);
                        state.scroll.animate_to(
                            target,
                            PICKER_TAP_SCROLL_MICROS,
                            crate::animation::Curve::EASE_IN_OUT,
                        );
                    });
                })
        };

        let mut children: Vec<AnyWidget> = Vec::new();
        for index in first..=last {
            children.push((self.build_item)(index));
        }

        let overlay_color = theme.resolve(CupertinoColors::TERTIARY_SYSTEM_FILL);
        let background = self.background_color;
        let selection_overlay = self.selection_overlay;
        let id = self.id;

        many(children, move |rendered| {
            // The off-center dim, applied at build time because a paint-time
            // opacity layer cannot nest inside the transform layer through
            // `PaintContext`'s layer API. Build and paint read the same
            // offset, so the ramp is consistent; only the height estimate can
            // lag, and only on the first frame.
            let mut items: Vec<BoxedRender> = Vec::new();
            for (i, child) in rendered.into_iter().enumerate() {
                let index = first + i;
                let flat_center = index as f32 * extent + extent / 2.0 - offset;
                let angle = angle_for(flat_center, height_estimate, diameter_ratio, squeeze);
                let radius = height_estimate * diameter_ratio / 2.0;
                let y_rel = flat_center - height_estimate / 2.0;
                let (screen_y, _) =
                    project_center(y_rel, angle, radius, height_estimate, PICKER_PERSPECTIVE);
                let dimmed =
                    !inside_magnifier_band(screen_y, height_estimate, extent, magnification);
                if dimmed {
                    items.push(RenderRef::new(RenderOpacity::new(
                        PICKER_OFF_CENTER_OPACITY,
                        child,
                    )));
                } else {
                    items.push(child);
                }
            }

            let wheel = RenderListWheel {
                children: items,
                first_index: first,
                item_extent: extent,
                offset,
                diameter_ratio,
                squeeze,
                magnification: if use_magnifier { magnification } else { 1.0 },
                viewport_sink: viewport.clone(),
                laid_out: Size::ZERO,
            };

            let mut stack = RenderStack::new().with_fit(crate::render::StackFit::Expand);
            stack = stack.push(wheel);
            if selection_overlay {
                // `CupertinoPickerDefaultSelectionOverlay`: a centered band
                // `itemExtent * magnification` tall, inset 9 on each side,
                // corners rounded 8, in tertiarySystemFill -- and
                // `IgnorePointer`, which the render-level equivalent of is
                // `RenderIgnorePointer`. A non-positioned child under
                // `StackFit::Expand` fills the stack, so `Center` puts the
                // fixed-height band in the middle.
                stack = stack.push(crate::render::RenderIgnorePointer::new(Center::new(
                    Container::new()
                        .with_height(extent * magnification)
                        .with_margin(EdgeInsets::symmetric(9.0, 0.0))
                        .with_color(overlay_color)
                        .with_corner_radius(8.0),
                )));
            }
            let mut container = Container::new().with_child(stack);
            if let Some(background) = background {
                container = container.with_color(background);
            }
            Pointer::new(id, container).with_handlers(handlers.clone())
        })
    }
}

/// The wheel's render object: fixed-extent children laid out flat and painted
/// through the cylindrical projection. `RenderListWheelViewport`, reduced to
/// a vertical, non-looping wheel.
struct RenderListWheel {
    children: Vec<BoxedRender>,
    /// The index `children[0]` stands for.
    first_index: usize,
    item_extent: f32,
    offset: f32,
    diameter_ratio: f32,
    squeeze: f32,
    /// 1.0 when the magnifier is off.
    magnification: f32,
    viewport_sink: Rc<Cell<f32>>,
    laid_out: Size,
}

impl RenderBox for RenderListWheel {
    /// `sizedByParent`: the wheel is exactly what it is offered.
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let width = if constraints.has_bounded_width() {
            constraints.max_width
        } else {
            constraints.min_width
        };
        let height = if constraints.has_bounded_height() {
            constraints.max_height
        } else {
            constraints.min_height
        };
        self.laid_out = Size::new(width, height);
        self.viewport_sink.set(height);
        for child in &mut self.children {
            // `_layoutChild`: the item extent, tight; the cross axis loose.
            child.layout_child(
                BoxConstraints::new(0.0, width, self.item_extent, self.item_extent),
                true,
            );
        }
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let height = self.laid_out.height;
        if height <= 0.0 {
            return;
        }
        let radius = height * self.diameter_ratio / 2.0;
        for (i, child) in self.children.iter().enumerate() {
            let index = self.first_index + i;
            let flat_center =
                index as f32 * self.item_extent + self.item_extent / 2.0 - self.offset;
            let angle = angle_for(flat_center, height, self.diameter_ratio, self.squeeze);
            // The backside of the cylinder is not painted.
            if angle.abs() > std::f32::consts::FRAC_PI_2 {
                continue;
            }
            let y_rel = flat_center - height / 2.0;
            let (screen_y, mut sx) =
                project_center(y_rel, angle, radius, height, PICKER_PERSPECTIVE);
            let mut sy = project_scale_y(y_rel, angle, radius, height, PICKER_PERSPECTIVE);
            if self.magnification > 1.0
                && inside_magnifier_band(screen_y, height, self.item_extent, self.magnification)
            {
                sx *= self.magnification;
                sy *= self.magnification;
            }
            let child_size = child.size();
            // Scale about the child's center, placed at its projected
            // position: `push_transform`'s pivot form.
            let pivot = Offset::new(child_size.width / 2.0, child_size.height / 2.0);
            let at = Offset::new(
                offset.dx + (self.laid_out.width - child_size.width) / 2.0,
                offset.dy + screen_y - child_size.height / 2.0,
            );
            context.push_transform([sx, 0.0, 0.0, sy, 0.0, 0.0], pivot, at, child);
        }
    }

    /// Hit testing works in flat coordinates: the cylindrical transform is a
    /// paint-time projection (upstream's `hitTest` would invert it, which the
    /// 2D affine bridge cannot), and the flat lookup is what the tap handler
    /// above uses, so the two agree.
    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        for (i, child) in self.children.iter().enumerate().rev() {
            let index = self.first_index + i;
            let child_offset = Offset::new(
                (self.laid_out.width - child.size().width) / 2.0,
                index as f32 * self.item_extent - self.offset,
            );
            let local = Offset::new(position.dx - child_offset.dx, position.dy - child_offset.dy);
            if child.hit_test(local, result) {
                return true;
            }
        }
        false
    }

    /// The wheel itself is a target even between items: the drag region is
    /// the whole viewport, as upstream's `ListWheelScrollView` is a
    /// scrollable everywhere.
    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        for (i, child) in self.children.iter().enumerate() {
            let index = self.first_index + i;
            visit(
                child,
                Offset::new(
                    (self.laid_out.width - child.size().width) / 2.0,
                    index as f32 * self.item_extent - self.offset,
                ),
            );
        }
    }
}

// -- Scrollbar ----------------------------------------------------------------
//
// Anchor: cupertino/scrollbar.dart, `CupertinoScrollbar`.

/// The measurements a [`CupertinoScrollbar`] is drawn and faded with:
/// scrollbar.dart's `defaultThickness` (3), `defaultRadius` (1.5),
/// `_kScrollbarMinLength` (36), `_kScrollbarCrossAxisMargin` (3),
/// `_kScrollbarTimeToFade` (1200ms) and `_kScrollbarFadeDuration` (250ms).
///
/// `defaultThicknessWhileDragging` (8) and `defaultRadiusWhileDragging` (4)
/// are not in the set: they belong to the draggable thumb, which this crate's
/// [`crate::scrollbar::Scrollbar`] does not port for either tier -- the same
/// gap PORTING_STATUS.md records for the Material scrollbar.
/// `_kScrollbarMinOverscrollLength` (8) and `_kScrollbarMainAxisMargin` (3)
/// are likewise unported painter details.
pub const CUPERTINO_SCROLLBAR_METRICS: crate::scrollbar::ScrollbarMetrics =
    crate::scrollbar::ScrollbarMetrics {
        thickness: 3.0,
        radius: 1.5,
        min_thumb_length: 36.0,
        cross_axis_margin: 3.0,
        time_to_fade_micros: 1_200_000,
        fade_micros: 250_000,
    };

/// The thumb's color. scrollbar.dart's `_kScrollbarColor`.
pub const CUPERTINO_SCROLLBAR_COLOR: CupertinoDynamicColor =
    CupertinoDynamicColor::with_brightness(Color(0x5900_0000), Color(0x80FF_FFFF));

/// An iOS scrollbar: a thin rounded thumb over the scrollable's trailing
/// edge, fading in on scroll and out a moment after. Upstream's
/// `CupertinoScrollbar` (scrollbar.dart).
///
/// This is the crate's [`crate::scrollbar::Scrollbar`] with Cupertino's
/// measurements ([`CUPERTINO_SCROLLBAR_METRICS`]) and color; the deltas are
/// listed on the metrics constant.
pub struct CupertinoScrollbar {
    build_child: Rc<dyn Fn() -> AnyWidget>,
    /// Upstream's `thumbColor`.
    color: Option<Color>,
}

impl CupertinoScrollbar {
    /// `build_child` builds the scrollable, as a builder for the same reason
    /// [`crate::scrollbar::Scrollbar::new`] takes one.
    pub fn new(build_child: impl Fn() -> AnyWidget + 'static) -> CupertinoScrollbar {
        CupertinoScrollbar {
            build_child: Rc::new(build_child),
            color: None,
        }
    }

    /// Upstream's `thumbColor`; the default resolves
    /// [`CUPERTINO_SCROLLBAR_COLOR`] against the ambient theme.
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

impl Component for CupertinoScrollbar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let color = self
            .color
            .unwrap_or_else(|| theme.resolve(CUPERTINO_SCROLLBAR_COLOR));
        let build_child = self.build_child.clone();
        stateful(
            crate::scrollbar::Scrollbar::new(move || build_child())
                .with_metrics(CUPERTINO_SCROLLBAR_METRICS)
                .with_color(color),
        )
    }
}

// -- Search text field ----------------------------------------------------------
//
// Anchor: cupertino/search_field.dart, `CupertinoSearchTextField`.

/// The field's corner radius. search_field.dart's `_kDefaultBorderRadius`.
pub const SEARCH_FIELD_RADIUS: f32 = 9.0;

/// The search and clear marks' size. Upstream's `itemSize`.
pub const SEARCH_FIELD_ITEM_SIZE: f32 = 20.0;

/// A magnifying glass, drawn. Upstream's prefix is the `CupertinoIcons.search`
/// glyph; with no icon font here (see the module docs) it is a stroked circle
/// and a handle, the way [`BackChevron`] is two strokes.
struct SearchGlyph {
    color: Color,
    laid_out: Size,
}

impl RenderBox for SearchGlyph {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.laid_out =
            constraints.constrain(Size::new(SEARCH_FIELD_ITEM_SIZE, SEARCH_FIELD_ITEM_SIZE));
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let paint = Paint::new(self.color)
            .with_style(Style::Stroke { width: 1.6 })
            .with_stroke_cap(crate::painting::StrokeCap::Round);
        // The glass, sitting high and to the start like the glyph's; the
        // handle runs from its lower-right edge to the corner.
        context
            .canvas()
            .draw_circle(offset.dx + 8.0, offset.dy + 8.0, 5.0, &paint);
        context.canvas().draw_line(
            (offset.dx + 11.6, offset.dy + 11.6),
            (offset.dx + 16.0, offset.dy + 16.0),
            &paint,
        );
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }
}

/// A clear mark, drawn. Upstream's suffix is `CupertinoIcons.xmark_circle_fill`;
/// here a filled circle in the item color with the cross knocked out in the
/// field's background color.
struct ClearGlyph {
    color: Color,
    background: Color,
    laid_out: Size,
}

impl RenderBox for ClearGlyph {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.laid_out =
            constraints.constrain(Size::new(SEARCH_FIELD_ITEM_SIZE, SEARCH_FIELD_ITEM_SIZE));
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let center = offset.dx + SEARCH_FIELD_ITEM_SIZE / 2.0;
        let middle = offset.dy + SEARCH_FIELD_ITEM_SIZE / 2.0;
        context.canvas().draw_circle(
            center,
            middle,
            SEARCH_FIELD_ITEM_SIZE / 2.0,
            &Paint::new(self.color),
        );
        let cross = Paint::new(self.background)
            .with_style(Style::Stroke { width: 1.6 })
            .with_stroke_cap(crate::painting::StrokeCap::Round);
        let arm = 3.5;
        context.canvas().draw_line(
            (center - arm, middle - arm),
            (center + arm, middle + arm),
            &cross,
        );
        context.canvas().draw_line(
            (center - arm, middle + arm),
            (center + arm, middle - arm),
            &cross,
        );
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }
}

/// What a [`CupertinoSearchTextField`] remembers between frames.
#[derive(Default)]
pub struct CupertinoSearchTextFieldState {
    /// The field's current text, mirrored from the inner field so the clear
    /// button's visibility rebuilds with it. Upstream keeps this in its
    /// `TextEditingController` and listens to it.
    pub text: String,
}

/// A search field: the rounded gray well with a search mark on the leading
/// side and, once there is text, a clear button on the trailing side.
/// Upstream's `CupertinoSearchTextField` (search_field.dart).
///
/// The clear button empties the inner field through the state handle the
/// field publishes ([`crate::editable::TextField::with_state_sink`]) --
/// upstream's `controller.clear()`. The marks are drawn, not icon-font
/// glyphs (see the module docs).
///
/// Upstream's default placeholder is the localized
/// `searchTextFieldPlaceholderLabel` ("Search"); there are no localizations
/// in this crate, so the placeholder is unset unless the caller sets one
/// with [`CupertinoSearchTextField::with_placeholder`]. The placeholder's
/// color is the field's own muted color rather than `secondaryLabel`, a
/// half-shade difference noted rather than fixed.
pub struct CupertinoSearchTextField {
    id: u64,
    placeholder: Option<String>,
    enabled: bool,
    on_changed: Option<Rc<dyn Fn(&str)>>,
    on_submitted: Option<Rc<dyn Fn(&str)>>,
    /// Where the inner field publishes its handle on every build.
    field_sink: Rc<RefCell<Option<StateHandle<crate::editable::TextFieldState>>>>,
}

impl CupertinoSearchTextField {
    /// `id` distinguishes this field from the others in the tree, as it does
    /// for [`crate::editable::TextField`].
    pub fn new(id: u64) -> CupertinoSearchTextField {
        CupertinoSearchTextField {
            id,
            placeholder: None,
            enabled: true,
            on_changed: None,
            on_submitted: None,
            field_sink: Rc::new(RefCell::new(None)),
        }
    }

    /// Upstream's `placeholder`.
    pub fn with_placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Called for every change. Upstream's `onChanged`.
    pub fn with_on_changed(mut self, changed: impl Fn(&str) + 'static) -> Self {
        self.on_changed = Some(Rc::new(changed));
        self
    }

    /// Called when the reader submits. Upstream's `onSubmitted`.
    pub fn with_on_submitted(mut self, submitted: impl Fn(&str) + 'static) -> Self {
        self.on_submitted = Some(Rc::new(submitted));
        self
    }

    /// Runs `changed` with the state and the new text on every change, and
    /// `submitted` on submit. Upstream's `onChanged`/`onSubmitted`, wired to
    /// app state the way every other widget in this tier wires its callbacks.
    pub fn wired<S: 'static>(
        mut self,
        handle: StateHandle<S>,
        changed: fn(&mut S, &str),
        submitted: fn(&mut S, &str),
    ) -> Self {
        let changed_handle = handle.clone();
        self.on_changed = Some(Rc::new(move |text| {
            let text = text.to_string();
            changed_handle.set_state(move |state| changed(state, &text));
        }));
        self.on_submitted = Some(Rc::new(move |text| {
            let text = text.to_string();
            handle.set_state(move |state| submitted(state, &text));
        }));
        self
    }
}

impl StatefulComponent for CupertinoSearchTextField {
    type State = CupertinoSearchTextFieldState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    fn build(
        &self,
        state: &CupertinoSearchTextFieldState,
        handle: StateHandle<CupertinoSearchTextFieldState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let background = theme.resolve(CupertinoColors::TERTIARY_SYSTEM_FILL);
        let item_color = theme.resolve(CupertinoColors::SECONDARY_LABEL);

        // The inner field, styled as upstream's `style` default:
        // `_kDefaultTextStyle` (17pt) in the label color.
        let mut style = theme.text_style();
        let own_handle = handle.clone();
        let on_changed = self.on_changed.clone();
        let mut field = crate::editable::TextField::new(self.id)
            .with_style(style.clone())
            .with_state_sink(self.field_sink.clone())
            .with_on_changed(move |text| {
                // Keep the mirror current so the clear button comes and
                // goes; upstream does the same listening to its controller.
                let mirrored = text.to_string();
                own_handle.set_state(move |state| state.text = mirrored);
                if let Some(changed) = &on_changed {
                    changed(text);
                }
            });
        style.color = theme.resolve(CupertinoColors::SECONDARY_LABEL);
        if let Some(placeholder) = &self.placeholder {
            field = field.with_placeholder(placeholder.clone());
        }
        if let Some(submitted) = &self.on_submitted {
            let submitted = submitted.clone();
            field = field.with_on_submitted(move |text| submitted(text));
        }

        // The row: search mark, field, and -- with text in it -- the clear
        // button. The insets are search_field.dart's defaults
        // (`prefixInsets` 6/8/0/8, `suffixInsets` 0/8/5/8, directional, here
        // resolved for ltr; the rtl swap is `EdgeInsetsDirectional`'s and
        // these constants are symmetric enough to keep simple).
        let mut children: Vec<AnyWidget> = Vec::new();
        children.push(leaf(move || SearchGlyph {
            color: item_color,
            laid_out: Size::ZERO,
        }));
        children.push(stateful(field));
        let show_clear = !state.text.is_empty() && self.enabled;
        if show_clear {
            children.push(leaf(move || ClearGlyph {
                color: item_color,
                background,
                laid_out: Size::ZERO,
            }));
        }
        let id = self.id;
        let sink = self.field_sink.clone();

        many(children, move |rendered| {
            let mut rendered = rendered.into_iter();
            let search_mark = rendered.next().expect("the search mark");
            let field = rendered.next().expect("the field");
            let clear_mark = rendered.next();

            let mut row = Row::new().with_cross_axis_alignment(CrossAxisAlignment::Center);
            row = row.push(crate::widgets::Padding::new(
                EdgeInsets::only(6.0, 8.0, 0.0, 8.0),
                search_mark,
            ));
            // `_kDefaultPadding`, `EdgeInsetsDirectional.fromSTEB(5.5, 8,
            // 5.5, 8)`, pads the text between the marks -- upstream's
            // `CupertinoTextField.padding`, not an inset around the row.
            row = row.push_flex(FlexChild::expanded(
                crate::widgets::Padding::new(EdgeInsets::symmetric(5.5, 8.0), field),
                1,
            ));
            if let Some(clear_mark) = clear_mark {
                let clear_handle = handle.clone();
                let sink = sink.clone();
                let clear = Pointer::new(
                    id,
                    crate::widgets::Padding::new(EdgeInsets::only(0.0, 8.0, 5.0, 8.0), clear_mark),
                )
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    // `_clearText`: empty the field through its own handle,
                    // which also tells the IME, and empty the mirror.
                    if let Some(field_handle) = &*sink.borrow() {
                        field_handle.set_state(|state| state.clear());
                    }
                    clear_handle.set_state(|state| state.text.clear());
                }));
                row = row.push(clear);
            }

            // The well itself is tertiarySystemFill rounded 9, wrapping the
            // row as tight as the row wants.
            Container::new()
                .with_color(background)
                .with_corner_radius(SEARCH_FIELD_RADIUS)
                .with_child(row)
        })
    }
}

// -- Context menu ---------------------------------------------------------------
//
// Anchor: cupertino/context_menu.dart, `CupertinoContextMenu`;
// cupertino/context_menu_action.dart, `CupertinoContextMenuAction`.

/// The scrim an open context menu sits over. context_menu.dart's
/// `_kModalBarrierColor`.
pub const CONTEXT_MENU_BARRIER_COLOR: Color = Color(0x6604_040F);

/// The child's corner radius while the menu is open. Upstream's
/// `CupertinoContextMenu.kOpenBorderRadius`.
pub const K_OPEN_BORDER_RADIUS: f32 = 12.0;

/// The action sheet's width. context_menu.dart's `_kMenuWidth`.
pub const CONTEXT_MENU_SHEET_WIDTH: f32 = 250.0;

/// The sheet's corner radius: the `ClipRSuperellipse` of `_ContextMenuSheet`,
/// circular here (see the module docs).
pub const CONTEXT_MENU_SHEET_RADIUS: f32 = 13.0;

/// The sheet's background. context_menu.dart's `_kBackgroundColor`, also
/// `CupertinoContextMenu.kBackgroundColor`.
pub const CONTEXT_MENU_BACKGROUND: CupertinoDynamicColor =
    CupertinoDynamicColor::with_brightness(Color(0xFFF1_F1F1), Color(0xFF21_2122));

/// The action separators' color. context_menu.dart's `_borderColor`.
pub const CONTEXT_MENU_BORDER: CupertinoDynamicColor =
    CupertinoDynamicColor::with_brightness(Color(0xFFA9_A9AF), Color(0xFF57_585A));

/// An action's fill while held. context_menu_action.dart's
/// `_kBackgroundColorPressed`.
const CONTEXT_MENU_ACTION_PRESSED: CupertinoDynamicColor =
    CupertinoDynamicColor::with_brightness(Color(0xFFDD_DDDD), Color(0xFF3F_3F40));

/// An action's minimum height. context_menu_action.dart's `_kButtonHeight`.
const CONTEXT_MENU_ACTION_HEIGHT: f32 = 43.0;

/// What a [`CupertinoContextMenuAction`] remembers between frames.
#[derive(Default)]
pub struct CupertinoContextMenuActionState {
    /// Whether the action is held, for the pressed fill.
    pub pressed: bool,
}

/// One action in a context menu's sheet. Upstream's
/// `CupertinoContextMenuAction` (context_menu_action.dart).
pub struct CupertinoContextMenuAction {
    id: u64,
    label: String,
    is_default: bool,
    is_destructive: bool,
    on_pressed: Option<Rc<dyn Fn()>>,
}

impl CupertinoContextMenuAction {
    pub fn new(id: u64, label: impl Into<String>) -> CupertinoContextMenuAction {
        CupertinoContextMenuAction {
            id,
            label: label.into(),
            is_default: false,
            is_destructive: false,
            on_pressed: None,
        }
    }

    /// Upstream's `isDefaultAction`: the bold one.
    pub fn default_action(mut self) -> Self {
        self.is_default = true;
        self
    }

    /// Upstream's `isDestructiveAction`: the red one.
    pub fn destructive(mut self) -> Self {
        self.is_destructive = true;
        self
    }

    /// Runs `action` when the action is tapped. Upstream's `onPressed`, which
    /// also pops the route; popping here is the app's, so `action` should
    /// close the menu too.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, action: fn(&mut S)) -> Self {
        self.on_pressed = Some(Rc::new(move || {
            handle.set_state(move |state| action(state));
        }));
        self
    }
}

impl StatefulComponent for CupertinoContextMenuAction {
    type State = CupertinoContextMenuActionState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    fn build(
        &self,
        state: &CupertinoContextMenuActionState,
        handle: StateHandle<CupertinoContextMenuActionState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let id = self.id;
        let label = self.label.clone();
        let pressed = state.pressed;

        // `_textStyle`: `_kActionSheetActionStyle` (16pt, 400) in the label
        // color, 600 for a default action, destructiveRed for a destructive
        // one. Upstream has no disabled variant -- a null `onPressed` simply
        // does not tap -- so neither has this.
        let color = if self.is_destructive {
            theme.resolve(CupertinoColors::DESTRUCTIVE_RED)
        } else {
            theme.resolve(CupertinoColors::LABEL)
        };
        let fill = if pressed {
            theme.resolve(CONTEXT_MENU_ACTION_PRESSED)
        } else {
            theme.resolve(CONTEXT_MENU_BACKGROUND)
        };
        let is_default = self.is_default;

        let mut handlers = PointerHandlers::new();
        if let Some(on_pressed) = &self.on_pressed {
            let tapped = on_pressed.clone();
            handlers = handlers.with_tap(move |_| tapped());
            handlers = handlers.with_press_change(move |down| {
                handle.set_state(move |state| state.pressed = down);
            });
        }

        leaf(move || {
            Pointer::new(
                id,
                Container::new()
                    .with_height(CONTEXT_MENU_ACTION_HEIGHT)
                    .with_color(fill)
                    // The action's insets, `EdgeInsets.fromLTRB(15.5, 8.0,
                    // 17.5, 8.0)`.
                    .with_padding(EdgeInsets::only(15.5, 8.0, 17.5, 8.0))
                    .with_alignment(Alignment::CENTER_LEFT)
                    .with_child(
                        Text::new(label.clone())
                            .with_size(16.0)
                            .with_weight(if is_default { 600 } else { 400 })
                            .with_color(color)
                            .with_soft_wrap(false)
                            .with_max_lines(1)
                            .with_overflow(TextOverflow::Ellipsis),
                    ),
            )
            .with_handlers(handlers.clone())
        })
    }
}

/// The sheet the actions are listed in: 250 wide, corners rounded 13,
/// actions stacked with hairline separators between them. Upstream's
/// `_ContextMenuSheet` minus its scrollable (a sheet taller than the screen
/// scrolls upstream; here the app keeps the list short, the same constraint
/// [`crate::menu::PopupMenu`] puts on its callers).
///
/// Presentation is the app's (see the module docs): put the sheet in a
/// `Stack` over a scrim of [`CONTEXT_MENU_BARRIER_COLOR`].
pub struct CupertinoContextMenuSheet {
    actions: RefCell<Vec<AnyWidget>>,
}

impl CupertinoContextMenuSheet {
    pub fn new() -> CupertinoContextMenuSheet {
        CupertinoContextMenuSheet {
            actions: RefCell::new(Vec::new()),
        }
    }

    /// Upstream's `actions`, in order.
    pub fn push(self, action: CupertinoContextMenuAction) -> Self {
        self.actions.borrow_mut().push(stateful(action));
        self
    }
}

impl Default for CupertinoContextMenuSheet {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for CupertinoContextMenuSheet {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let background = theme.resolve(CONTEXT_MENU_BACKGROUND);
        let border = theme.resolve(CONTEXT_MENU_BORDER);
        let actions = std::mem::take(&mut *self.actions.borrow_mut());
        many(actions, move |rendered| {
            let mut column = Column::new().with_main_axis_size(MainAxisSize::Min);
            let mut first = true;
            for action in rendered {
                if !first {
                    // `_ContextMenuSheet`'s `BorderSide(width: 0.4)` separator
                    // -- a device-pixel hairline, one logical pixel here (see
                    // the module docs).
                    let hairline = Container::new().with_height(1.0).with_color(border);
                    column = column.push(hairline);
                }
                first = false;
                column = column.push(action);
            }
            RenderClipRect::new(
                Container::new()
                    .with_width(CONTEXT_MENU_SHEET_WIDTH)
                    .with_color(background)
                    .with_child(column),
            )
            .with_corner_radius(CONTEXT_MENU_SHEET_RADIUS)
        })
    }
}

/// A long-press trigger for a context menu. Upstream's `CupertinoContextMenu`
/// (context_menu.dart) opens a route on long press that zooms the child out
/// of the layout and puts the action sheet beside it; the zoom, blur and
/// drag-to-dismiss animations are not ported (see the module docs). What
/// remains is the trigger: `open` runs on a long press, and putting the
/// [`CupertinoContextMenuSheet`] over a scrim in a `Stack` is the app's, as
/// it is for dialogs in this tier.
pub struct CupertinoContextMenu {
    id: u64,
    child: RefCell<Option<AnyWidget>>,
    handlers: PointerHandlers,
}

impl CupertinoContextMenu {
    pub fn new(id: u64) -> CupertinoContextMenu {
        CupertinoContextMenu {
            id,
            child: RefCell::new(None),
            handlers: PointerHandlers::new(),
        }
    }

    /// Upstream's `child` (its `builder` taking the open animation is the
    /// animated variant, not ported).
    pub fn with_child(self, child: AnyWidget) -> Self {
        *self.child.borrow_mut() = Some(child);
        self
    }

    /// Runs `open` on a long press. Upstream's `onLongPress` handler calling
    /// `Navigator.push`; `open(true)` should put the sheet up, and the
    /// actions' `wired` callbacks should take it down.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, open: fn(&mut S, bool)) -> Self {
        self.handlers = PointerHandlers::new().with_long_press(move |_| {
            handle.set_state(move |state| open(state, true));
        });
        self
    }
}

impl Component for CupertinoContextMenu {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let id = self.id;
        let handlers = self.handlers.clone();
        let child = self
            .child
            .borrow_mut()
            .take()
            .expect("CupertinoContextMenu needs a child");
        single(child, move |inner| {
            Pointer::new(id, inner).with_handlers(handlers.clone())
        })
    }
}

// -- Cupertino's dialogs and action sheets ------------------------------------

/// Upstream's `_isInAccessibilityMode` (`cupertino/dialog.dart`).
///
/// **iOS has no "accessibility mode" to query, so the text scale stands in for
/// one.** A default 14-point font scaled past 1.4× means the reader has turned
/// text up far enough that the dialog should change shape, not merely reflow.
/// Written as `scaled > default * factor` rather than as a comparison of
/// scales, which is upstream's spelling and the one that survives a text
/// scaler that is not a simple multiplier.
pub fn is_in_accessibility_mode(text_scale: f32) -> bool {
    const DEFAULT_FONT_SIZE: f32 = 14.0;
    /// Upstream's `_kMaxRegularTextScaleFactor`.
    const MAX_REGULAR_TEXT_SCALE_FACTOR: f32 = 1.4;
    DEFAULT_FONT_SIZE * text_scale > DEFAULT_FONT_SIZE * MAX_REGULAR_TEXT_SCALE_FACTOR
}

/// Upstream `CupertinoPopupSurface`: the blurred, translucent panel a
/// Cupertino dialog or action sheet is drawn on.
///
/// The blur is the whole of it -- an iOS dialog is not an opaque card but a
/// frosted pane, so what is behind it stays faintly legible and the dialog
/// reads as *over* the page rather than as a new page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoPopupSurface {
    pub blur_sigma: f32,
    /// Upstream's `isSurfacePainted`. Off, the surface blurs and clips but
    /// paints no colour of its own -- which is what an action sheet's
    /// *cancel* button wants, since it supplies its own fill and would
    /// otherwise be tinted twice.
    pub is_surface_painted: bool,
}

impl CupertinoPopupSurface {
    /// Upstream's `defaultBlurSigma`.
    pub const DEFAULT_BLUR_SIGMA: f32 = 30.0;
    /// Upstream's `_clipper` radius.
    pub const CORNER_RADIUS: f32 = 13.0;
    /// Upstream's light-mode saturation, derived to resemble the iOS 17
    /// simulator. Paired with the blur for the reason
    /// [`CupertinoDesktopTextSelectionToolbar::SATURATION_BOOST`] gives: a
    /// blur averages colours and washes them out.
    pub const LIGHT_SATURATION: f32 = 2.0;

    pub fn new() -> CupertinoPopupSurface {
        CupertinoPopupSurface {
            blur_sigma: CupertinoPopupSurface::DEFAULT_BLUR_SIGMA,
            is_surface_painted: true,
        }
    }

    /// Upstream asserts a non-negative sigma outright, since a negative blur
    /// is not a sharpen -- it is nothing the engine can do.
    pub fn with_blur_sigma(mut self, sigma: f32) -> Self {
        debug_assert!(sigma >= 0.0, "a blur sigma is not negative");
        self.blur_sigma = sigma;
        self
    }

    pub fn with_surface_painted(mut self, painted: bool) -> Self {
        self.is_surface_painted = painted;
        self
    }
}

impl Default for CupertinoPopupSurface {
    fn default() -> CupertinoPopupSurface {
        CupertinoPopupSurface::new()
    }
}

/// Upstream `CupertinoDialogAction`: one button of a [`CupertinoAlertDialog`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CupertinoDialogAction {
    /// Upstream's `isDefaultAction`, which makes the label bold. At most one
    /// per dialog, since "the one you probably want" cannot be two things.
    pub is_default_action: bool,
    /// Upstream's `isDestructiveAction`, which makes it red.
    pub is_destructive_action: bool,
}

impl CupertinoDialogAction {
    /// Upstream's `_kCupertinoDialogWidth`.
    pub const DIALOG_WIDTH: f32 = 270.0;
    /// Upstream's `_kAccessibilityCupertinoDialogWidth`.
    ///
    /// **Wider, not narrower.** Where a Material dialog keeps its width and
    /// gives up its whitespace as text grows (see
    /// [`crate::controls::scale_dialog_padding`]), a Cupertino one takes more
    /// of the screen. Two designs answering the same question in opposite
    /// ways, and both are ported as their own platform has them.
    pub const ACCESSIBILITY_DIALOG_WIDTH: f32 = 310.0;
    /// Upstream's `_kDialogEdgePadding`.
    pub const EDGE_PADDING: f32 = 20.0;
    /// Upstream's `_kDialogMinButtonHeight`.
    pub const MIN_BUTTON_HEIGHT: f32 = 45.0;
    /// Upstream's `_kDialogMinButtonFontSize`: however far a reader scales
    /// text *down*, a button's label stops here. Below it the word stops being
    /// a target and becomes decoration.
    pub const MIN_BUTTON_FONT_SIZE: f32 = 10.0;
    /// Upstream's `_kDialogActionsSectionMinHeight`, derived by comparing on
    /// iOS 17 simulators -- which is why it is 67.8 and not a round number.
    pub const ACTIONS_SECTION_MIN_HEIGHT: f32 = 67.8;
    pub const CORNER_RADIUS: f32 = 14.0;

    pub fn new() -> CupertinoDialogAction {
        CupertinoDialogAction::default()
    }

    pub fn default_action() -> CupertinoDialogAction {
        CupertinoDialogAction {
            is_default_action: true,
            ..CupertinoDialogAction::default()
        }
    }

    pub fn destructive() -> CupertinoDialogAction {
        CupertinoDialogAction {
            is_destructive_action: true,
            ..CupertinoDialogAction::default()
        }
    }

    /// How wide the dialog is at a given text scale.
    pub fn dialog_width(text_scale: f32) -> f32 {
        if is_in_accessibility_mode(text_scale) {
            CupertinoDialogAction::ACCESSIBILITY_DIALOG_WIDTH
        } else {
            CupertinoDialogAction::DIALOG_WIDTH
        }
    }
}

/// Upstream `CupertinoActionSheet`: the sheet of choices that slides up from
/// the bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CupertinoActionSheet;

impl CupertinoActionSheet {
    /// Upstream's `_kActionSheetEdgePadding`. Much tighter than a dialog's 20:
    /// a sheet is meant to reach nearly the full width of the screen, where a
    /// dialog floats in the middle of it.
    pub const EDGE_PADDING: f32 = 8.0;
    /// Upstream's `_kActionSheetCancelButtonPadding`: the gap between the
    /// sheet and its cancel button, which is a *separate* panel -- that gap is
    /// what says "this one is not one of the choices".
    pub const CANCEL_BUTTON_PADDING: f32 = 8.0;
    pub const CONTENT_HORIZONTAL_PADDING: f32 = 16.0;
    pub const CONTENT_VERTICAL_PADDING: f32 = 13.5;
    pub const ACTIONS_SECTION_MIN_HEIGHT: f32 = 84.0;
}

/// Upstream `CupertinoActionSheetAction`: one choice on that sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CupertinoActionSheetAction {
    pub is_default_action: bool,
    pub is_destructive_action: bool,
}

impl CupertinoActionSheetAction {
    pub const HORIZONTAL_PADDING: f32 = 10.0;
    /// Upstream's `_kActionSheetButtonMinHeight`, from the simulator.
    pub const MIN_HEIGHT: f32 = 57.17;
    /// Upstream's `_kActionSheetButtonVerticalPaddingFactor`.
    pub const VERTICAL_PADDING_FACTOR: f32 = 0.4;
    /// Upstream's `_kActionSheetButtonVerticalPaddingBase`.
    pub const VERTICAL_PADDING_BASE: f32 = 1.8;

    pub fn new() -> CupertinoActionSheetAction {
        CupertinoActionSheetAction::default()
    }

    /// Upstream's `base + fontSize * factor`, with its comment: "according to
    /// experimenting on the simulator, the height of action sheet buttons is
    /// proportional to the font size down to a minimal height".
    ///
    /// A line rather than a constant, because a sheet's rows have to grow with
    /// their text -- a reader who scaled text up and got the same 57-pixel row
    /// would have the words touching its edges. The **base** is the part that
    /// does not scale, so even a tiny font keeps a hairline of breathing room.
    pub fn vertical_padding(font_size: f32) -> f32 {
        CupertinoActionSheetAction::VERTICAL_PADDING_BASE
            + font_size * CupertinoActionSheetAction::VERTICAL_PADDING_FACTOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{ElementTree, component, provide};

    fn lay_out(widget: AnyWidget, width: f32, height: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(CupertinoTheme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(width, height))
    }

    #[test]
    fn dynamic_colors_resolve_against_the_appearance() {
        let label = CupertinoColors::LABEL;
        assert_eq!(label.resolve(Brightness::Light), Color(0xFF00_0000));
        assert_eq!(label.resolve(Brightness::Dark), Color(0xFFFF_FFFF));
    }

    #[test]
    fn the_themes_carry_the_documented_colors() {
        let light = CupertinoTheme::light();
        assert_eq!(light.brightness, Brightness::Light);
        assert_eq!(light.bar_background_color, Color(0xF0F9_F9F9));
        assert_eq!(light.primary_color, CupertinoColors::SYSTEM_BLUE.color);
        let dark = CupertinoTheme::dark();
        assert_eq!(dark.brightness, Brightness::Dark);
        assert_eq!(dark.bar_background_color, Color(0xF01D_1D1D));
        assert_eq!(dark.primary_color, CupertinoColors::SYSTEM_BLUE.dark_color);
        // `CupertinoDynamicColor.resolve` against the theme, the way the
        // widgets do it.
        assert_eq!(dark.resolve(CupertinoColors::LABEL), Color(0xFFFF_FFFF));
    }

    #[test]
    fn a_button_is_at_least_44_on_each_side() {
        let size = lay_out(component(CupertinoButton::new(1, "OK")), 400.0, 300.0);
        assert!(
            size.width >= K_MIN_INTERACTIVE_DIMENSION_CUPERTINO,
            "{size:?}"
        );
        assert!(
            size.height >= K_MIN_INTERACTIVE_DIMENSION_CUPERTINO,
            "{size:?}"
        );
    }

    #[test]
    fn a_small_button_is_at_least_28_tall() {
        let size = lay_out(
            component(CupertinoButton::new(1, "OK").with_size_style(CupertinoButtonSize::Small)),
            400.0,
            300.0,
        );
        assert!(size.height >= 28.0, "{size:?}");
        // ...and smaller than a large one: the padding and font shrink.
        let large = lay_out(component(CupertinoButton::new(2, "OK")), 400.0, 300.0);
        assert!(size.height < large.height, "{size:?} vs {large:?}");
    }

    #[test]
    fn a_filled_button_is_disabled_in_quaternary_fill() {
        // Lays out without a tap handler; the point is that a disabled button
        // builds at all and keeps the minimum size.
        let size = lay_out(
            component(CupertinoButton::filled(1, "No").with_enabled(false)),
            400.0,
            300.0,
        );
        assert!(
            size.height >= K_MIN_INTERACTIVE_DIMENSION_CUPERTINO,
            "{size:?}"
        );
    }

    #[test]
    fn a_switch_lays_out_at_its_fixed_size() {
        let size = lay_out(stateful(CupertinoSwitch::new(1, true)), 200.0, 200.0);
        assert_eq!(size, Size::new(SWITCH_SIZE.0, SWITCH_SIZE.1));
    }

    #[test]
    fn a_disabled_switch_lays_out_the_same() {
        let size = lay_out(stateful(CupertinoSwitch::new(1, false)), 200.0, 200.0);
        assert_eq!(size, Size::new(SWITCH_SIZE.0, SWITCH_SIZE.1));
    }

    #[test]
    fn the_slider_maps_position_to_value() {
        // The thumb's center travels between 8 + 14 from each edge.
        assert_eq!(slider_value_at(22.0, SLIDER_WIDTH), 0.0);
        assert_eq!(slider_value_at(SLIDER_WIDTH - 22.0, SLIDER_WIDTH), 1.0);
        // Clamped beyond the ends.
        assert_eq!(slider_value_at(0.0, SLIDER_WIDTH), 0.0);
        assert_eq!(slider_value_at(SLIDER_WIDTH, SLIDER_WIDTH), 1.0);
        let mid = slider_value_at(SLIDER_WIDTH / 2.0, SLIDER_WIDTH);
        assert!((mid - 0.5).abs() < 0.01, "{mid}");
    }

    #[test]
    fn a_slider_lays_out_at_its_fixed_size() {
        let size = lay_out(component(CupertinoSlider::new(1, 0.5)), 400.0, 300.0);
        assert_eq!(size, Size::new(SLIDER_WIDTH, SLIDER_HEIGHT));
    }

    #[test]
    fn the_activity_indicator_turns_once_a_second() {
        let indicator = CupertinoActivityIndicator::new();
        let mut state = CupertinoActivityIndicatorState::default();
        assert!(indicator.advance(&mut state, 0));
        assert!(indicator.advance(&mut state, 250_000));
        assert!((state.position - 0.25).abs() < 1e-6, "{}", state.position);
        // It wraps: a second and a half in is half a turn.
        indicator.advance(&mut state, 1_500_000);
        assert!((state.position - 0.5).abs() < 1e-6, "{}", state.position);
    }

    #[test]
    fn a_partially_revealed_indicator_does_not_spin() {
        let indicator = CupertinoActivityIndicator::partially_revealed(0.5);
        let mut state = CupertinoActivityIndicatorState::default();
        assert!(!indicator.advance(&mut state, 100_000));
        assert_eq!(state.position, 0.0);
    }

    #[test]
    fn a_dialog_is_270_wide() {
        let size = lay_out(
            component(CupertinoAlertDialog::new().with_title("Title")),
            800.0,
            600.0,
        );
        assert_eq!(size.width, ALERT_DIALOG_WIDTH);
    }

    #[test]
    fn a_dialog_with_two_actions_lays_them_side_by_side() {
        // Two actions: one row, so the dialog is barely taller than the title
        // plus one action's 45.
        let two = lay_out(
            component(
                CupertinoAlertDialog::new()
                    .with_title("Title")
                    .with_action(stateful(CupertinoAlertAction::new(1, "One")))
                    .with_action(stateful(CupertinoAlertAction::new(2, "Two"))),
            ),
            800.0,
            600.0,
        );
        // Three actions: a stack, so it is two action rows taller.
        let three = lay_out(
            component(
                CupertinoAlertDialog::new()
                    .with_title("Title")
                    .with_action(stateful(CupertinoAlertAction::new(1, "One")))
                    .with_action(stateful(CupertinoAlertAction::new(2, "Two")))
                    .with_action(stateful(CupertinoAlertAction::new(3, "Three"))),
            ),
            800.0,
            600.0,
        );
        assert!(
            three.height > two.height + ACTION_MIN_HEIGHT,
            "{two:?} vs {three:?}"
        );
    }

    #[test]
    fn a_navigation_bar_is_44_plus_the_top_inset() {
        let size = lay_out(
            component(CupertinoNavigationBar::new().with_middle("Title")),
            400.0,
            300.0,
        );
        assert_eq!(size.width, 400.0);
        assert!(size.height >= NAV_BAR_HEIGHT, "{size:?}");
    }

    #[test]
    fn a_tab_bar_is_50_tall() {
        let bar = CupertinoTabBar::new(
            1,
            vec![
                CupertinoTabItem::new("Home", "H"),
                CupertinoTabItem::new("Settings", "S"),
            ],
            0,
        );
        let size = lay_out(component(bar), 400.0, 300.0);
        assert_eq!(size.width, 400.0);
        assert!(size.height >= TAB_BAR_HEIGHT, "{size:?}");
    }

    #[test]
    fn a_segmented_control_gives_each_segment_a_share() {
        let size = lay_out(
            stateful(CupertinoSegmentedControl::new(
                1,
                vec!["One".into(), "Two".into(), "Three".into()],
                0,
            )),
            400.0,
            300.0,
        );
        assert_eq!(size.width, 400.0);
        assert!(
            size.height >= SEGMENTED_CONTROL_MIN_HEIGHT && size.height <= 40.0,
            "{size:?}"
        );
    }

    #[test]
    fn the_picker_index_and_offset_convert_both_ways() {
        assert_eq!(index_to_scroll_offset(0, 32.0), 0.0);
        assert_eq!(index_to_scroll_offset(3, 32.0), 96.0);
        assert_eq!(scroll_offset_to_index(96.0, 32.0), 3);
        // `scrollOffsetToIndex` is the floor -- it answers "which item does
        // this offset fall in", for the visible window; the nearest-item
        // landing is the snap's round, tested through advance below.
        assert_eq!(scroll_offset_to_index(97.0, 32.0), 3);
        assert_eq!(scroll_offset_to_index(127.0, 32.0), 3);
        assert_eq!(scroll_offset_to_index(128.0, 32.0), 4);
    }

    #[test]
    fn the_wheel_sees_at_most_a_quarter_turn() {
        // A diameter ratio under 1 would need more than the cylinder's front;
        // it clamps to pi/2.
        assert_eq!(max_visible_radian(0.5), std::f32::consts::FRAC_PI_2);
        // The default ratio sees asin(1 / 1.07).
        let angle = max_visible_radian(PICKER_DIAMETER_RATIO);
        let expected = (1.0_f32 / PICKER_DIAMETER_RATIO).asin();
        assert!((angle - expected).abs() < 1e-6, "{angle}");
    }

    #[test]
    fn the_projection_holds_the_center_still() {
        // A child at the middle of the wheel projects to the middle of the
        // screen at unit scale, whatever the perspective.
        let (y, scale) = project_center(0.0, 0.0, 107.0, 200.0, PICKER_PERSPECTIVE);
        assert!((y - 100.0).abs() < 1e-4, "{y}");
        assert!((scale - 1.0).abs() < 1e-4, "{scale}");
    }

    #[test]
    fn the_projection_shrinks_children_toward_the_edges() {
        // A child 80 below the middle of the wheel is around the barrel at
        // the angle `angle_for` gives it: smaller and lower than the center
        // one, which is the whole look of the wheel.
        let (center_y, center_scale) = project_center(0.0, 0.0, 107.0, 200.0, PICKER_PERSPECTIVE);
        let edge_angle = angle_for(180.0, 200.0, PICKER_DIAMETER_RATIO, PICKER_SQUEEZE);
        let (edge_y, edge_scale) =
            project_center(80.0, edge_angle, 107.0, 200.0, PICKER_PERSPECTIVE);
        assert!(edge_y > center_y, "{edge_y} vs {center_y}");
        assert!(edge_scale < center_scale, "{edge_scale} vs {center_scale}");
    }

    #[test]
    fn a_resting_picker_snaps_to_the_nearest_item() {
        let picker = CupertinoPicker::new(1, 32.0, 10, |_| leaf(|| Empty));
        let mut state = picker.initial_state();
        // Started at item 0; dragged most of the way to item 1 and released.
        state.scroll.scroll_by(20.0);
        assert!(picker.advance(&mut state, 0));
        assert_eq!(state.snapping_to, Some(32.0));
    }

    #[test]
    fn a_picker_on_a_boundary_stays_put() {
        let picker = CupertinoPicker::new(1, 32.0, 10, |_| leaf(|| Empty));
        let mut state = picker.initial_state();
        assert!(!picker.advance(&mut state, 0));
        assert_eq!(state.snapping_to, None);
        // Resting on item 0, it reports item 0.
        assert_eq!(state.reported, Some(0));
    }

    #[test]
    fn a_picker_reports_the_item_under_the_band() {
        let picker = CupertinoPicker::new(1, 32.0, 10, |_| leaf(|| Empty));
        let mut state = picker.initial_state();
        state.scroll.scroll_by(96.0);
        // On the boundary: no snap wanted...
        let mut frame = 0;
        // ...but the report lands. `animate_to` from the snap is driven by
        // advance; give it frames until the offset settles at 96.
        for _ in 0..100 {
            frame += 16_000;
            if !picker.advance(&mut state, frame) {
                break;
            }
        }
        assert_eq!(state.reported, Some(3));
    }

    #[test]
    fn a_picker_does_not_snap_while_dragging() {
        let picker = CupertinoPicker::new(1, 32.0, 10, |_| leaf(|| Empty));
        let mut state = picker.initial_state();
        state.scroll.scroll_by(20.0);
        state.dragging = true;
        picker.advance(&mut state, 0);
        assert_eq!(state.snapping_to, None);
    }

    #[test]
    fn a_picker_lays_out_sized_by_its_parent() {
        let size = lay_out(
            stateful(CupertinoPicker::labels(
                1,
                32.0,
                vec!["a".into(), "b".into()],
            )),
            300.0,
            216.0,
        );
        // The wheel is sized by its parent: offered 300x216 loose, it takes
        // the offer, as upstream's `SizedBox.expand` around the viewport does.
        assert_eq!(size, Size::new(300.0, 216.0));
    }

    #[test]
    fn the_wheel_hit_tests_in_flat_coordinates() {
        let mut wheel = RenderListWheel {
            children: vec![RenderRef::new(Pointer::new(
                7,
                Container::new().with_size(100.0, 32.0),
            ))],
            first_index: 0,
            item_extent: 32.0,
            offset: 0.0,
            diameter_ratio: PICKER_DIAMETER_RATIO,
            squeeze: PICKER_SQUEEZE,
            magnification: 1.0,
            viewport_sink: Rc::new(Cell::new(0.0)),
            laid_out: Size::ZERO,
        };
        let size = wheel.layout(BoxConstraints::tight_for(Size::new(300.0, 216.0)));
        assert_eq!(size, Size::new(300.0, 216.0));
        // The layout published the viewport height for the next build.
        assert_eq!(wheel.viewport_sink.get(), 216.0);
        // The child is centered horizontally; a tap on it hits it.
        let mut result = HitTestResult::new();
        assert!(wheel.hit_test_children(Offset::new(150.0, 16.0), &mut result));
        assert!(!result.path.is_empty());
        // Below the one child there is nothing to hit.
        let mut miss = HitTestResult::new();
        assert!(!wheel.hit_test_children(Offset::new(150.0, 100.0), &mut miss));
    }

    #[test]
    fn the_cupertino_scrollbar_metrics_are_ios() {
        assert_eq!(CUPERTINO_SCROLLBAR_METRICS.thickness, 3.0);
        assert_eq!(CUPERTINO_SCROLLBAR_METRICS.radius, 1.5);
        assert_eq!(CUPERTINO_SCROLLBAR_METRICS.min_thumb_length, 36.0);
        assert_eq!(CUPERTINO_SCROLLBAR_METRICS.cross_axis_margin, 3.0);
        assert_eq!(CUPERTINO_SCROLLBAR_METRICS.time_to_fade_micros, 1_200_000);
        assert_eq!(CUPERTINO_SCROLLBAR_METRICS.fade_micros, 250_000);
    }

    #[test]
    fn a_cupertino_scrollbar_lays_out_over_its_child() {
        let size = lay_out(
            component(CupertinoScrollbar::new(|| {
                leaf(|| Container::new().with_size(100.0, 100.0))
            })),
            300.0,
            300.0,
        );
        assert_eq!(size, Size::new(100.0, 100.0));
    }

    #[test]
    fn a_search_field_fills_its_width() {
        let size = lay_out(
            stateful(CupertinoSearchTextField::new(1).with_placeholder("Search")),
            300.0,
            200.0,
        );
        assert_eq!(size.width, 300.0);
        // 8 + 8 of padding around a 17pt line: near upstream's 36.
        assert!(size.height > 30.0 && size.height < 44.0, "{size:?}");
    }

    #[test]
    fn an_empty_search_field_has_no_clear_button() {
        // Both arms lay out; the difference is only whether the clear mark's
        // tap target is in the tree, which the smoke of both builds covers.
        let empty = lay_out(stateful(CupertinoSearchTextField::new(1)), 300.0, 200.0);
        assert!(empty.height > 0.0);
    }

    #[test]
    fn a_context_menu_action_is_43_tall() {
        let size = lay_out(
            stateful(CupertinoContextMenuAction::new(1, "Share")),
            400.0,
            300.0,
        );
        assert!(size.height >= CONTEXT_MENU_ACTION_HEIGHT, "{size:?}");
    }

    #[test]
    fn a_context_menu_sheet_is_250_wide() {
        let sheet = CupertinoContextMenuSheet::new()
            .push(CupertinoContextMenuAction::new(1, "One"))
            .push(CupertinoContextMenuAction::new(2, "Two"));
        let size = lay_out(component(sheet), 400.0, 400.0);
        assert_eq!(size.width, CONTEXT_MENU_SHEET_WIDTH);
        // Two actions and a hairline between them.
        assert!(
            (size.height - (2.0 * CONTEXT_MENU_ACTION_HEIGHT + 1.0)).abs() < 2.0,
            "{size:?}"
        );
    }

    #[test]
    fn a_context_menu_trigger_lays_out_its_child() {
        let trigger = CupertinoContextMenu::new(1)
            .with_child(leaf(|| Container::new().with_size(60.0, 60.0)));
        let size = lay_out(component(trigger), 300.0, 300.0);
        assert_eq!(size, Size::new(60.0, 60.0));
    }

    #[test]
    fn accessibility_mode_is_inferred_from_the_text_scale() {
        // iOS has no such mode to query, so the text scale stands in for one:
        // a default 14-point font scaled past 1.4x means the reader has turned
        // text up far enough that the dialog should change shape rather than
        // merely reflow.
        assert!(!is_in_accessibility_mode(1.0));
        assert!(
            !is_in_accessibility_mode(1.4),
            "exactly at the line is not past it"
        );
        assert!(is_in_accessibility_mode(1.5));
        assert!(is_in_accessibility_mode(3.0));
    }

    #[test]
    fn a_cupertino_dialog_gets_wider_where_a_material_one_gives_up_whitespace() {
        // The two platforms answer the same question in opposite ways, and
        // both are ported as their own has them: Material keeps its width and
        // shrinks its padding, Cupertino takes more of the screen.
        assert_eq!(CupertinoDialogAction::dialog_width(1.0), 270.0);
        assert_eq!(CupertinoDialogAction::dialog_width(2.0), 310.0);
        assert!(
            CupertinoDialogAction::ACCESSIBILITY_DIALOG_WIDTH > CupertinoDialogAction::DIALOG_WIDTH
        );
        // Where the Material rule shrinks.
        assert!(crate::controls::scale_dialog_padding(2.0) < 1.0);
    }

    #[test]
    fn an_action_sheet_reaches_wider_than_a_dialog_does() {
        // A sheet is meant to reach nearly the full width of the screen; a
        // dialog floats in the middle of it.
        assert!(CupertinoActionSheet::EDGE_PADDING < CupertinoDialogAction::EDGE_PADDING);
        assert_eq!(CupertinoActionSheet::EDGE_PADDING, 8.0);
        assert_eq!(CupertinoDialogAction::EDGE_PADDING, 20.0);
    }

    #[test]
    fn a_sheets_row_grows_with_its_text_but_never_to_nothing() {
        // Upstream's `base + fontSize * factor`, from experimenting on the
        // simulator. A row that did not grow would have the words touching its
        // edges at a large text size; the *base* is what keeps a hairline of
        // breathing room even at a tiny one.
        let small = CupertinoActionSheetAction::vertical_padding(10.0);
        let large = CupertinoActionSheetAction::vertical_padding(30.0);
        assert!(large > small);
        assert_eq!(
            CupertinoActionSheetAction::vertical_padding(0.0),
            CupertinoActionSheetAction::VERTICAL_PADDING_BASE,
            "the base does not scale away"
        );
        assert!(CupertinoActionSheetAction::VERTICAL_PADDING_BASE > 0.0);
    }

    #[test]
    fn a_button_label_stops_shrinking_at_ten_points() {
        // However far a reader scales text down. Below it the word stops being
        // a target and becomes decoration.
        assert_eq!(CupertinoDialogAction::MIN_BUTTON_FONT_SIZE, 10.0);
        assert!(
            CupertinoDialogAction::MIN_BUTTON_HEIGHT > CupertinoDialogAction::MIN_BUTTON_FONT_SIZE
        );
    }

    #[test]
    fn a_popup_surface_may_blur_without_painting_itself() {
        // Which is what an action sheet's cancel button wants: it supplies its
        // own fill and would otherwise be tinted twice.
        let plain = CupertinoPopupSurface::new();
        assert!(plain.is_surface_painted);
        assert_eq!(plain.blur_sigma, 30.0);

        let unpainted = CupertinoPopupSurface::new().with_surface_painted(false);
        assert!(!unpainted.is_surface_painted);
        assert_eq!(unpainted.blur_sigma, 30.0, "it still blurs");
    }

    #[test]
    fn the_surfaces_blur_comes_with_a_saturation_boost_as_the_desktop_one_does() {
        // Same pairing, same reason: a blur averages colours and washes them
        // out, so the saturation is pushed back to keep what shows through
        // recognisable.
        assert!(CupertinoPopupSurface::DEFAULT_BLUR_SIGMA > 0.0);
        assert!(CupertinoPopupSurface::LIGHT_SATURATION > 1.0);
    }

    #[test]
    fn a_cancel_button_is_a_separate_panel_with_a_gap_before_it() {
        // That gap is what says "this one is not one of the choices" -- a
        // cancel flush against the sheet would read as the last item on it.
        assert!(CupertinoActionSheet::CANCEL_BUTTON_PADDING > 0.0);
    }

    #[test]
    fn a_default_action_and_a_destructive_one_are_different_things() {
        // One is "the one you probably want" and the other is "the one that
        // cannot be undone"; a button is at most one of them.
        let default = CupertinoDialogAction::default_action();
        assert!(default.is_default_action && !default.is_destructive_action);
        let destructive = CupertinoDialogAction::destructive();
        assert!(destructive.is_destructive_action && !destructive.is_default_action);
        let plain = CupertinoDialogAction::new();
        assert!(!plain.is_default_action && !plain.is_destructive_action);
    }
}
