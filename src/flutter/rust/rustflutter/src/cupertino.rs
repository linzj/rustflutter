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
//! - **The bars do not blur, though the bridge now can.** Upstream's nav
//!   bar, tab bar and dialog sit over a `BackdropFilter` blur. The paint
//!   bridge has had one since `RenderBackdropFilter`, and
//!   [`CupertinoPopupSurface`] uses it -- so what is left here is a choice not
//!   yet made for the two bars, not a capability the port lacks. This note
//!   said the opposite until tick 286, when `stale_notes.py` caught it.
//!   The translucent colors (`barBackgroundColor` 0xF0.., `_kDialogColor`
//!   0xCC..) are kept, so the shapes and tints match and only the frosted
//!   texture is missing.
//! - **A dialog is a surface the app puts up, not a route** -- the same rule
//!   as [`crate::controls`]: [`CupertinoAlertDialog`] is something to put in a
//!   `Stack` over a scrim, and there is no `showCupertinoDialog` route
//!   machinery.
//!
//!   This used to cover the context menu as well, and no longer does:
//!   [`CupertinoContextMenu`] opens itself over the application through
//!   [`crate::theatre`]'s overlay, because *where* it opens is the whole
//!   point of it -- upstream pushes on the **root** navigator so the menu
//!   covers the application rather than the page that owns the child, and an
//!   app-composed `Stack` covers only whatever built it. The gallery's demo
//!   showed the difference plainly: its scrim reached the edges of the demo
//!   card and stopped.
//! - **Hairlines are one logical pixel.** Upstream draws its dividers at
//!   thickness 0.0 or 0.3 (device-pixel hairlines); at this renderer's unit
//!   scale one logical pixel is the hairline, the convention
//!   [`crate::components::Divider`] already establishes.
//! - **`CupertinoDynamicColor`'s high-contrast variants are dropped.** The
//!   platform bridge carries brightness but no contrast setting, so the
//!   `highContrastColor`/`darkHighContrastColor` columns of
//!   `cupertino/colors.dart` have nothing to resolve against.
//!
//!   This note used to cover the **elevated** columns as well, under the same
//!   reason, and the reason does not reach them: elevation does not come from
//!   the platform at all. `CupertinoUserInterfaceLevel` is a widget in the
//!   tree -- a sheet raises the level for what it lays over -- and this crate
//!   has had it all along, documented as existing to pick between a dynamic
//!   colour's base and elevated values while there was no elevated value to
//!   pick. Four of the eight columns are carried now; only high contrast is
//!   still missing, and it is missing for a reason that is actually about it.
//! - **The corner radius is `Radius.circular`, not `RSuperellipse`.** The
//!   paint bridge has no superellipse; upstream itself falls back to `RRect`
//!   "since this shape is really small" in several of these widgets.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use crate::animation::{Controller, Curve, Direction};
use crate::engine::{Color, Paint, Rect, Style, TextStyle};
use crate::framework::{
    AnyWidget, BuildContext, Component, Key, StateHandle, StatefulComponent, component, leaf, many,
    single, stateful,
};
use crate::gestures::PointerHandlers;
use crate::list_wheel::{
    RenderListWheelViewport, angle_for, index_to_scroll_offset, inside_magnifier_band,
    project_center, scroll_offset_to_index,
};
use crate::platform::Brightness;
use crate::render::{
    Alignment, BoxConstraints, BoxedRender, CrossAxisAlignment, EdgeInsets, FlexChild,
    MainAxisSize, Offset, PaintContext, RenderBox, RenderClipRect, RenderConstrainedBox,
    RenderFlex, RenderOpacity, RenderRef, RenderStack, RenderTransform, Size, StackPosition,
    TextOverflow,
};
use crate::theatre::{Anchor, PortalController};
use crate::widgets::{Align, Center, Column, Container, Empty, Pointer, Row, Text};

// -- Colors -------------------------------------------------------------------
//
// Anchor: cupertino/colors.dart, `class CupertinoColors`.

/// The base interface level, spelled out at the call sites below that have no
/// ambient level to consult.
///
/// Upstream's `resolveFrom` reaches the level through the tree; these sites
/// resolve from a brightness they were handed and have no context, so they
/// pass the base explicitly. Naming it keeps the gap visible: it is a value
/// somebody chose, not a default that filled itself in.
const BASE: CupertinoUserInterfaceLevelData = CupertinoUserInterfaceLevelData::Base;

/// Upstream `CupertinoDynamicColor`: four of its eight columns.
///
/// Upstream resolves a colour against three independent things -- platform
/// brightness, interface elevation and high contrast -- which is 2x2x2 = eight
/// values. This carries the four that are not high contrast; see the module
/// docs for why that one column is missing and this one is not.
///
/// # Elevation never moves the light value
///
/// Across all eighteen of upstream's system colours that declare an elevated
/// variant, `color == elevatedColor` **every time**. Only the dark side ever
/// moves, and then only for the six background roles and `separator`.
///
/// It follows from what raising something means: in the dark you lift a
/// surface by lightening it, and in the light there is nowhere lighter than
/// white to go. See [`CupertinoDynamicColor::elevation_only_moves_the_dark`].
///
/// # And only the surface moves, not what is on it
///
/// `label`, the fills and `link` do not move under elevation; the backgrounds
/// do. Content does not change when the surface under it rises -- the surface
/// does. `separator` moves and `opaqueSeparator` does not, which is the same
/// rule seen from the other end: a translucent separator shows what is behind
/// it and has to follow it, and an opaque one has nothing to follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoDynamicColor {
    /// The light-appearance value. Upstream's `color`.
    pub color: Color,
    /// The dark-appearance value. Upstream's `darkColor`.
    pub dark_color: Color,
    /// Upstream's `elevatedColor`: the light value on a raised surface, which
    /// in upstream's whole table is always the light value.
    pub elevated_color: Color,
    /// Upstream's `darkElevatedColor`, the one of the four that actually
    /// carries elevation.
    pub dark_elevated_color: Color,
}

impl CupertinoDynamicColor {
    /// Upstream's `CupertinoDynamicColor.withBrightness`: a colour that does
    /// not vary with elevation, so both levels are the base value.
    pub const fn with_brightness(color: Color, dark_color: Color) -> CupertinoDynamicColor {
        CupertinoDynamicColor {
            color,
            dark_color,
            elevated_color: color,
            dark_elevated_color: dark_color,
        }
    }

    /// A colour that also varies with elevation. The light elevated value is
    /// taken rather than assumed, even though upstream's own table never
    /// moves it -- a rule that holds everywhere is still a rule and not a law.
    pub const fn with_elevation(
        color: Color,
        dark_color: Color,
        dark_elevated_color: Color,
    ) -> CupertinoDynamicColor {
        CupertinoDynamicColor {
            color,
            dark_color,
            elevated_color: color,
            dark_elevated_color,
        }
    }

    /// Upstream's resolution table, given the traits directly rather than a
    /// context.
    pub const fn resolve(
        &self,
        brightness: Brightness,
        level: CupertinoUserInterfaceLevelData,
    ) -> Color {
        match (brightness, level) {
            (Brightness::Light, CupertinoUserInterfaceLevelData::Base) => self.color,
            (Brightness::Light, CupertinoUserInterfaceLevelData::Elevated) => self.elevated_color,
            (Brightness::Dark, CupertinoUserInterfaceLevelData::Base) => self.dark_color,
            (Brightness::Dark, CupertinoUserInterfaceLevelData::Elevated) => {
                self.dark_elevated_color
            }
        }
    }

    /// Upstream's `_isPlatformBrightnessDependent`.
    ///
    /// A colour whose light and dark halves agree does not consult the
    /// brightness -- and upstream's point is not the saved comparison. Not
    /// consulting it means **not depending on it**, so a widget drawn in such
    /// a colour is not rebuilt when the appearance changes. The flag is a
    /// dependency decision wearing an optimisation's clothes.
    pub const fn is_platform_brightness_dependent(&self) -> bool {
        !(self.color.0 == self.dark_color.0 && self.elevated_color.0 == self.dark_elevated_color.0)
    }

    /// Upstream's `_isInterfaceElevationDependent`, the same idea on the other
    /// axis.
    pub const fn is_interface_elevation_dependent(&self) -> bool {
        !(self.color.0 == self.elevated_color.0 && self.dark_color.0 == self.dark_elevated_color.0)
    }

    /// Whether elevation, where this colour has any, moves only its dark half
    /// -- the rule that holds across upstream's entire table.
    pub const fn elevation_only_moves_the_dark(&self) -> bool {
        self.color.0 == self.elevated_color.0
    }

    /// Upstream's `resolveFrom`: the brightness from the Cupertino theme and
    /// the level from whatever laid this subtree over something, each
    /// consulted only if this colour varies along it.
    ///
    /// Both fall back the way upstream's do -- `Brightness.light` and
    /// `CupertinoUserInterfaceLevelData.base` -- which is why this uses
    /// [`CupertinoUserInterfaceLevel::maybe_of`] and not `of`: resolving a
    /// colour outside any level is ordinary, where *asking* for the level
    /// outside one is a mistake.
    pub fn resolve_from(&self, context: &BuildContext) -> Color {
        let brightness = if self.is_platform_brightness_dependent() {
            context
                .inherited::<CupertinoTheme>()
                .map(|theme| theme.brightness)
                .unwrap_or(Brightness::Light)
        } else {
            Brightness::Light
        };
        let level = if self.is_interface_elevation_dependent() {
            CupertinoUserInterfaceLevel::maybe_of(context)
                .unwrap_or(CupertinoUserInterfaceLevelData::Base)
        } else {
            CupertinoUserInterfaceLevelData::Base
        };
        self.resolve(brightness, level)
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
    /// The seven colours below are the only ones in upstream's table whose
    /// value moves under elevation, and the six backgrounds all move the same
    /// way: **a raised surface takes the value of the next role down the
    /// ladder.**
    ///
    /// `systemBackground` elevated is `secondarySystemBackground`'s dark;
    /// secondary elevated is tertiary's; tertiary elevated is one step further
    /// again, past the end of the three that have names. The grouped trio
    /// repeats it with the same greys.
    ///
    /// So iOS's two ways of saying "this is layered over that" -- the numbered
    /// role and the elevation trait -- arrive at the same colour. See
    /// [`CupertinoColors::elevating_is_one_step_down`].
    pub const SYSTEM_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_elevation(Color::WHITE, Color::BLACK, Color::rgb(28, 28, 30));
    pub const SECONDARY_SYSTEM_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_elevation(
            Color::rgb(242, 242, 247),
            Color::rgb(28, 28, 30),
            Color::rgb(44, 44, 46),
        );
    pub const TERTIARY_SYSTEM_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_elevation(
            Color::WHITE,
            Color::rgb(44, 44, 46),
            Color::rgb(58, 58, 60),
        );
    pub const SYSTEM_GROUPED_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_elevation(
            Color::rgb(242, 242, 247),
            Color::BLACK,
            Color::rgb(28, 28, 30),
        );
    pub const SECONDARY_SYSTEM_GROUPED_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_elevation(
            Color::WHITE,
            Color::rgb(28, 28, 30),
            Color::rgb(44, 44, 46),
        );
    pub const TERTIARY_SYSTEM_GROUPED_BACKGROUND: CupertinoDynamicColor =
        CupertinoDynamicColor::with_elevation(
            Color::rgb(242, 242, 247),
            Color::rgb(44, 44, 46),
            Color::rgb(58, 58, 60),
        );
    /// The one non-background that moves, and it moves much further: 84,84,88
    /// to 210,210,210 at the same alpha. A translucent separator shows the
    /// surface through it, so when that surface lightens the separator has to
    /// outrun it to stay a line. [`CupertinoColors::OPAQUE_SEPARATOR`] hides
    /// what is behind it and therefore has nothing to follow -- which is the
    /// same rule read from the other end.
    pub const SEPARATOR: CupertinoDynamicColor = CupertinoDynamicColor::with_elevation(
        Color::argb(73, 60, 60, 67),
        Color::argb(153, 84, 84, 88),
        Color::argb(153, 210, 210, 210),
    );
    pub const OPAQUE_SEPARATOR: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(198, 198, 200), Color::rgb(56, 56, 58));
    pub const LINK: CupertinoDynamicColor =
        CupertinoDynamicColor::with_brightness(Color::rgb(0, 122, 255), Color::rgb(9, 132, 255));

    /// The six backgrounds, in the order the ladder runs, as
    /// `(role, the role below it)`.
    ///
    /// Elevating either trio by one level lands on the dark value of the next
    /// entry -- see [`CupertinoColors::SYSTEM_BACKGROUND`].
    pub const BACKGROUND_LADDER: [(CupertinoDynamicColor, CupertinoDynamicColor); 4] = [
        (Self::SYSTEM_BACKGROUND, Self::SECONDARY_SYSTEM_BACKGROUND),
        (
            Self::SECONDARY_SYSTEM_BACKGROUND,
            Self::TERTIARY_SYSTEM_BACKGROUND,
        ),
        (
            Self::SYSTEM_GROUPED_BACKGROUND,
            Self::SECONDARY_SYSTEM_GROUPED_BACKGROUND,
        ),
        (
            Self::SECONDARY_SYSTEM_GROUPED_BACKGROUND,
            Self::TERTIARY_SYSTEM_GROUPED_BACKGROUND,
        ),
    ];

    /// Whether raising each role by one level gives the role below it.
    ///
    /// Takes the ladder rather than reading
    /// [`CupertinoColors::BACKGROUND_LADDER`] itself, because a predicate over
    /// a constant cannot be shown to work: a mutation making it check only its
    /// first pair survived every test, since `all` over a prefix of a correct
    /// list is still true and nothing could hand it an incorrect one. Passing
    /// the ladder in is what makes the claim falsifiable.
    pub fn elevating_is_one_step_down(
        ladder: &[(CupertinoDynamicColor, CupertinoDynamicColor)],
    ) -> bool {
        ladder
            .iter()
            .all(|(role, below)| role.dark_elevated_color == below.dark_color)
    }
}

// -- Theme --------------------------------------------------------------------
//
// Anchor: cupertino/theme.dart, `CupertinoThemeData` and its `_kDefaultTheme`.

/// Styling shared by the Cupertino widgets below. Upstream's
/// `CupertinoThemeData`, reduced to the fields the ported widgets read:
/// `textTheme` is inlined into each widget (the `_kDefault*TextStyle`
/// constants of text_theme.dart are copied at the use sites), and
/// `selectionHandleColor` has no consumer here yet.
#[derive(Clone, Debug, PartialEq)]
pub struct CupertinoTheme {
    pub brightness: Brightness,
    pub primary_color: Color,
    /// Upstream's `CupertinoThemeData.applyThemeToAll`.
    ///
    /// **False by default**, and that default is the interesting part: an iOS
    /// switch is green because iOS switches are green, not because the
    /// application is. A theme's primary colour is for the things the
    /// application chose -- buttons, links -- and taking over the system
    /// controls as well is opt-in.
    ///
    /// [`CupertinoSwitch::with_apply_theme`] is the per-widget override of it.
    pub apply_theme_to_all: bool,
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
            // Upstream's default. A system control keeps its system colour
            // unless the application says otherwise.
            apply_theme_to_all: false,
            primary_color: CupertinoColors::SYSTEM_BLUE.resolve(brightness, BASE),
            primary_contrasting_color: CupertinoColors::WHITE,
            // `_CupertinoThemeDefaults.barBackgroundColor`. The dark value is
            // the navigation bar's; upstream notes the toolbar/tab bar dark
            // value is 0xF0161616, a distinction only the nav bar keeps.
            bar_background_color: Color(0xF0F9_F9F9),
            scaffold_background_color: CupertinoColors::SYSTEM_BACKGROUND.resolve(brightness, BASE),
        }
    }

    /// `_kDefaultTheme` resolved for a dark appearance.
    pub fn dark() -> CupertinoTheme {
        let brightness = Brightness::Dark;
        CupertinoTheme {
            brightness,
            // Upstream's default. A system control keeps its system colour
            // unless the application says otherwise.
            apply_theme_to_all: false,
            primary_color: CupertinoColors::SYSTEM_BLUE.resolve(brightness, BASE),
            primary_contrasting_color: CupertinoColors::WHITE,
            bar_background_color: Color(0xF01D_1D1D),
            scaffold_background_color: CupertinoColors::SYSTEM_BACKGROUND.resolve(brightness, BASE),
        }
    }

    /// Resolves a dynamic color against this theme's appearance: upstream's
    /// `CupertinoDynamicColor.resolve(color, context)` with the context's
    /// brightness.
    pub fn resolve(&self, color: CupertinoDynamicColor) -> Color {
        color.resolve(self.brightness, BASE)
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

/// The accessibility marks an iOS switch draws beside its thumb, when the
/// reader has asked for them.
///
/// # They are not letters, they are the power symbols
///
/// Upstream draws the "on" mark as a rectangle `_kOnLabelWidth = 1` by
/// `_kOnLabelHeight = 10`, and the "off" mark with `drawCircle` at
/// `_kOffLabelRadius = 5`. A one-by-ten bar and a circle: **the I and the O**
/// of the international power marks, drawn as primitives rather than set as
/// text.
///
/// Which is why they need no font and no localization -- a bar and a ring mean
/// the same thing in every script, and a switch that spelled "on" would have
/// to be translated and would stop fitting.
///
/// # And they appear only when the setting is on
///
/// `MediaQuery.onOffSwitchLabelsOf(context)` gates the whole pair: upstream
/// builds `(onColor, offColor)` or **null**, so with the setting off there is
/// nothing to draw rather than something drawn transparently.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchOnOffLabels {
    pub on_color: Color,
    pub off_color: Color,
}

impl SwitchOnOffLabels {
    /// switch.dart's `_kOnLabelWidth` and `_kOnLabelHeight`: the bar.
    pub const ON_SIZE: (f32, f32) = (1.0, 10.0);
    /// switch.dart's `_kOffLabelRadius`: the ring.
    pub const OFF_RADIUS: f32 = 5.0;
    /// switch.dart's `_kOnLabelPaddingHorizontal`.
    pub const ON_PADDING: f32 = 11.0;
    /// switch.dart's `_kOffLabelPaddingHorizontal`, which is **not** the on
    /// one: a circle of radius five and a bar one wide do not sit at the same
    /// inset if they are to look equally far in.
    pub const OFF_PADDING: f32 = 12.0;

    /// Upstream's `_kOffLabelColor` in its ordinary contrast.
    ///
    /// The high-contrast value is white, and this port cannot express the
    /// difference: `CupertinoDynamicColor`'s high-contrast columns are the one
    /// pair still missing, for the reason the module docs give -- the platform
    /// bridge carries no contrast setting. So the mark is drawn in the
    /// ordinary grey even for a reader who asked for high contrast, which is
    /// the same gap seen from a new place.
    pub const OFF_COLOR: Color = Color::argb(255, 179, 179, 179);

    /// Upstream's pair, or `None` when the reader has not asked for the marks.
    pub fn resolve(
        on_off_switch_labels: bool,
        on_color: Option<Color>,
        off_color: Option<Color>,
    ) -> Option<SwitchOnOffLabels> {
        if !on_off_switch_labels {
            return None;
        }
        Some(SwitchOnOffLabels {
            on_color: on_color.unwrap_or(Color::WHITE),
            off_color: off_color.unwrap_or(SwitchOnOffLabels::OFF_COLOR),
        })
    }
}

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
    /// Upstream's `applyTheme`, which is nullable on purpose: `None` means
    /// "whatever the theme says", not "no".
    apply_theme: Option<bool>,
    on_changed: Option<Rc<dyn Fn(bool)>>,
}

impl CupertinoSwitch {
    pub fn new(id: u64, value: bool) -> CupertinoSwitch {
        CupertinoSwitch {
            id,
            value,
            enabled: true,
            active_track_color: None,
            apply_theme: None,
            on_changed: None,
        }
    }

    /// Upstream's `activeTrackColor` (`activeColor` before it).
    pub fn with_active_track_color(mut self, color: Color) -> Self {
        self.active_track_color = Some(color);
        self
    }

    /// Upstream's `applyTheme`: whether this switch takes the theme's primary
    /// colour when it is on.
    ///
    /// Not a plain `bool`, because upstream's is nullable and the null is a
    /// third answer rather than a missing one:
    ///
    /// ```dart
    /// widget.activeTrackColor
    ///   ?? ((widget.applyTheme ?? theme.applyThemeToAll) ? theme.primaryColor : null)
    ///   ?? CupertinoColors.systemGreen
    /// ```
    ///
    /// Three levels, and this port had the first and the third. A switch that
    /// said `applyTheme: true` was ignored, and so was a theme that said
    /// `applyThemeToAll: true` -- every switch in this crate was iOS green
    /// whatever it or its theme asked for.
    ///
    /// The nullability is what lets one switch disagree with its theme in
    /// **either** direction: `Some(true)` takes the primary colour under a
    /// theme that says no, and `Some(false)` keeps the green under a theme
    /// that says yes.
    pub fn with_apply_theme(mut self, apply: bool) -> Self {
        self.apply_theme = Some(apply);
        self
    }

    /// The track colour for an *on* switch, which is upstream's three-level
    /// chain above. Pulled out so it can be asked without building a switch.
    pub fn active_track_color(
        one_off: Option<Color>,
        apply_theme: Option<bool>,
        theme_applies_to_all: bool,
        theme_primary: Color,
        system_green: Color,
    ) -> Color {
        if let Some(colour) = one_off {
            return colour;
        }
        if apply_theme.unwrap_or(theme_applies_to_all) {
            return theme_primary;
        }
        system_green
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
        let active_track = CupertinoSwitch::active_track_color(
            self.active_track_color,
            self.apply_theme,
            theme.apply_theme_to_all,
            theme.primary_color,
            theme.resolve(CupertinoColors::SYSTEM_GREEN),
        );
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
        let actions = self.actions.borrow().clone();
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
/// upstream's `BackdropFilter` blur is not applied here, which is what
/// "blur-free translucent approximation" means. The bridge can blur --
/// [`CupertinoPopupSurface`] does -- so this bar is a place the capability has
/// not been wired to, and not a gap in the port.
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
        let trailing = self.trailing.borrow().clone();
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
                // A title longer than twelve UTF-16 units is *replaced* by
                // the generic word, not ellipsized -- see
                // `CupertinoNavigationBarBackButton::label_for`. The ellipsis
                // below is upstream's too, and only a short title in a narrow
                // bar ever reaches it.
                if let Some(text) =
                    CupertinoNavigationBarBackButton::label_for(previous_title.as_deref()).text()
                {
                    row = row.push(
                        Text::new(text.to_string())
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
    /// Upstream's `iconSize`, the tile's icon slot.
    pub const ICON_SIZE: f32 = 30.0;

    /// Upstream's `preferredSize`, which is `Size.fromHeight(height)` -- **the
    /// bar's own height and not the box it draws.**
    ///
    /// The box is `height + bottomPadding`, so a bar over a home indicator
    /// occupies more than it reports. That is not an inconsistency: the
    /// padding is added to the box and immediately handed back to the content
    /// as bottom padding, so the items still live in their 50 and the extra is
    /// empty space over the indicator.
    ///
    /// What reads `preferredSize` wants to know how much room the *tabs* need;
    /// what lays the bar out gives it the inset as well. Reporting the sum
    /// would make a scaffold reserve the inset twice.
    pub fn preferred_height() -> f32 {
        TAB_BAR_HEIGHT
    }

    /// What the bar actually occupies, given the view's bottom inset.
    pub fn box_height(bottom_inset: f32) -> f32 {
        TAB_BAR_HEIGHT + bottom_inset
    }

    /// What a screen reader is told about the tab at `index`, counting from
    /// **zero** as this crate's loops do.
    ///
    /// ```dart
    /// hint: localizations.tabSemanticsLabel(tabIndex: index + 1, tabCount: items.length),
    /// ```
    ///
    /// The `+ 1` is the whole method. `tabSemanticsLabel` asserts its index is
    /// at least one because it is a position a person is told out loud --
    /// "Tab 1 of 3" -- and the loop that calls it counts from zero. Passing
    /// the loop variable straight through gives "Tab 0 of 3" and then never
    /// says "Tab 3". Doing the conversion here, once, is what keeps every
    /// caller from having to remember it.
    ///
    /// It goes to `Semantics.hint`, not `label`: the tab's own icon and title
    /// are the label, and this is the extra sentence about where it sits.
    /// `selected` is a third thing again, and upstream sets it separately.
    pub fn tab_semantics_hint(index: usize, count: usize) -> Option<String> {
        crate::cupertino_app::DefaultCupertinoLocalizations::tab_semantics_label(
            index as u32 + 1,
            count as u32,
        )
    }

    /// Upstream's `opaque`: whether anything shows through.
    ///
    /// It is decided by the **resolved** background colour's alpha being
    /// exactly `0xFF`, not by a flag anybody set. And what it decides is the
    /// blur: upstream puts one behind a bar that is not opaque, and none
    /// behind one that is -- because a blur under something you cannot see
    /// through is work nobody will look at.
    ///
    /// Resolved first, because a `CupertinoDynamicColor` can be opaque in one
    /// appearance and not the other, and the question is about the colour that
    /// will be painted.
    pub fn is_opaque(resolved_background: Color) -> bool {
        resolved_background.alpha() == 0xFF
    }

    /// Whether centring an item's column and bottom-aligning it come to the
    /// same thing here.
    ///
    /// Upstream's row is `CrossAxisAlignment.end`, with the comment "Align
    /// bottom since we want the labels to be aligned" -- a tile with a shorter
    /// icon would otherwise sit its label higher than its neighbour's.
    ///
    /// This port centres instead, and gets the same picture, because the icon
    /// slot is a fixed [`CupertinoTabBar::ICON_SIZE`] square for every item:
    /// with every column the same height, centred and bottom-aligned are the
    /// same position. **The equivalence rests on the fixed slot, not on the
    /// alignment being unimportant** -- an item allowed to size its own icon
    /// would break it, and the row would need upstream's `end`.
    pub fn alignment_is_equivalent(icon_heights: &[f32]) -> bool {
        icon_heights
            .iter()
            .all(|height| *height == CupertinoTabBar::ICON_SIZE)
    }

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
            .resolve(theme.brightness, BASE);
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
        let bar = self.navigation_bar.borrow().clone();
        let body = self.body.borrow().clone().unwrap_or_else(|| leaf(|| Empty));
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
            .borrow()
            .clone()
            .unwrap_or_else(|| leaf(|| Empty));
        let body = self.body.borrow().clone().unwrap_or_else(|| leaf(|| Empty));

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

/// Upstream `FixedExtentScrollController`, reduced to the two things a
/// picker's owner uses it for: saying which item the wheel starts on, and
/// driving the wheel afterwards.
///
/// It exists because upstream's date picker is not one wheel but several that
/// have to move each other -- scrolling the hour column past noon animates the
/// AM/PM column, and landing on February 30th animates the day column back to
/// the 28th. A column's scroll position lives in its own state here, so the
/// only way for the owner to reach it is a shared handle, which is what this
/// is. The pattern is [`crate::theatre::PortalController`]'s: the widget
/// carries one, the state attaches to it on the first build, and calls made
/// before then are dropped rather than queued -- a wheel that is not on screen
/// has nothing to animate.
#[derive(Clone, Default)]
pub struct FixedExtentScrollController {
    inner: Rc<RefCell<FixedExtentScrollInner>>,
}

#[derive(Default)]
struct FixedExtentScrollInner {
    initial_item: usize,
    item_extent: f32,
    /// How many items the wheel has, and whether it loops -- what turns a
    /// caller's index into the wheel index nearest where the wheel is now.
    count: usize,
    looping: bool,
    attached: Option<StateHandle<CupertinoPickerState>>,
}

impl FixedExtentScrollController {
    /// Upstream's `FixedExtentScrollController(initialItem:)`.
    pub fn new(initial_item: usize) -> FixedExtentScrollController {
        FixedExtentScrollController {
            inner: Rc::new(RefCell::new(FixedExtentScrollInner {
                initial_item,
                item_extent: 0.0,
                count: 0,
                looping: false,
                attached: None,
            })),
        }
    }

    pub fn initial_item(&self) -> usize {
        self.inner.borrow().initial_item
    }

    /// Upstream's `hasClients`.
    pub fn has_clients(&self) -> bool {
        self.inner.borrow().attached.is_some()
    }

    /// Upstream's `animateToItem`. Does nothing while the wheel is not
    /// mounted, which is upstream's `hasClients` guard at its call sites.
    pub fn animate_to_item(&self, index: usize, micros: i64, curve: crate::animation::Curve) {
        let (handle, extent, count, looping) = {
            let inner = self.inner.borrow();
            (
                inner.attached.clone(),
                inner.item_extent,
                inner.count,
                inner.looping,
            )
        };
        let Some(handle) = handle else {
            return;
        };
        handle.set_state(move |state| {
            // On a looping wheel the caller's index names a *value*, not a
            // place -- the same value is at every turn -- so the wheel goes to
            // whichever turn is nearest, which is also the shortest way round.
            let target = if looping && count > 0 {
                let here = (state.scroll.offset / extent).round() as i64;
                let turn = here.div_euclid(count as i64);
                let candidates =
                    [turn - 1, turn, turn + 1].map(|turn| turn * count as i64 + index as i64);
                let nearest = candidates
                    .into_iter()
                    .filter(|candidate| *candidate >= 0)
                    .min_by_key(|candidate| (candidate - here).abs())
                    .unwrap_or(index as i64);
                nearest as f32 * extent
            } else {
                index_to_scroll_offset(index, extent)
            };
            state.snapping_to = Some(target);
            state.scroll.animate_to(target, micros, curve);
        });
    }

    /// Called from the wheel's own build, which is the first moment there is a
    /// state to drive.
    fn attach(
        &self,
        handle: StateHandle<CupertinoPickerState>,
        item_extent: f32,
        count: usize,
        looping: bool,
    ) {
        let inner = &mut *self.inner.borrow_mut();
        inner.attached = Some(handle);
        inner.item_extent = item_extent;
        inner.count = count;
        inner.looping = looping;
    }
}

/// A wheel of fixed-extent items. Upstream's `CupertinoPicker` (picker.dart).
///
/// The scroll is the crate's [`crate::scrolling::Scroll`]: drags move it
/// directly, a release flings it with `ClampingScrollSimulation`, and when
/// the wheel comes to rest off an item boundary it is driven to the nearest
/// item -- the landing choice of `FixedExtentScrollPhysics.
/// createBallisticSimulation`, whose five scenarios now live in
/// [`crate::list_wheel::FixedExtentScrollPhysics`] along with the tuned
/// friction (`FrictionSimulation.through`) that scenario 5 needs. This picker
/// has not yet been rewired onto them: it drives the settle with a short
/// ease-out to the same target, so it lands on the same item by a different
/// path. Rewiring it is the remaining half of that work.
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
    /// Upstream's `offAxisFraction`, 0.0 by default -- see
    /// [`crate::list_wheel::RenderListWheelViewport::off_axis_fraction`]. The
    /// date picker sets it per column so all of them appear to turn on one
    /// shared axis.
    off_axis_fraction: f32,
    background_color: Option<Color>,
    /// Upstream's `selectionOverlay`, which defaults to a
    /// `CupertinoPickerDefaultSelectionOverlay` capped on both edges and is
    /// `null` for a picker that draws no band. A date picker passes a
    /// differently capped one per column so the three read as one band.
    selection_overlay: Option<CupertinoPickerDefaultSelectionOverlay>,
    initial_item: usize,
    /// Upstream's `scrollController`. When one is given it, not
    /// `initial_item`, says where the wheel starts.
    scroll_controller: Option<FixedExtentScrollController>,
    /// Upstream's `looping`: December is followed by January.
    looping: bool,
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
            off_axis_fraction: 0.0,
            background_color: None,
            selection_overlay: Some(CupertinoPickerDefaultSelectionOverlay::new()),
            initial_item: 0,
            scroll_controller: None,
            looping: false,
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
    /// defaults to `CupertinoPickerDefaultSelectionOverlay`; pass `None` for
    /// `selectionOverlay: null`, and a capped one for a column of a date
    /// picker (see [`CupertinoPickerDefaultSelectionOverlay::for_column`]).
    pub fn with_selection_overlay(
        mut self,
        overlay: Option<CupertinoPickerDefaultSelectionOverlay>,
    ) -> Self {
        self.selection_overlay = overlay;
        self
    }

    /// Upstream's `offAxisFraction`: where the cylinder's axis sits across
    /// this column, so that a row of columns appears to turn on one shared
    /// axis.
    pub fn with_off_axis_fraction(mut self, fraction: f32) -> Self {
        self.off_axis_fraction = fraction;
        self
    }

    /// Upstream's `onSelectedItemChanged`, for a caller whose callback is not
    /// a state change on one handle -- a date picker's columns, which write
    /// through their own state.
    pub fn with_on_selected(mut self, on_selected: impl Fn(usize) + 'static) -> Self {
        self.on_selected = Some(Rc::new(on_selected));
        self
    }

    /// Upstream's `scrollController.initialItem`.
    pub fn with_initial_item(mut self, index: usize) -> Self {
        self.initial_item = index;
        self
    }

    /// Upstream's `scrollController`, for an owner that has to move this wheel
    /// later -- a date picker's hour column moving its AM/PM column.
    pub fn with_scroll_controller(mut self, controller: FixedExtentScrollController) -> Self {
        self.scroll_controller = Some(controller);
        self
    }

    /// Upstream's `looping`, which a date picker sets on its day, month, hour
    /// and minute columns so that they have no visible ends.
    ///
    /// **Upstream's loop is infinite and this one is not.** Its
    /// `ListWheelChildLoopingListDelegate` answers for every index there is,
    /// negative ones included, and reports no child count at all; this crate's
    /// wheel is a finite list with a scroll extent, so looping here is
    /// [`CupertinoPicker::LOOPING_ITEMS`] items' worth of turns with the wheel
    /// starting in the middle of them. A reader who drags for long enough
    /// reaches an end; at thirty-two logical pixels an item that is some
    /// hundreds of screens of dragging in one direction.
    pub fn with_looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    /// Roughly how many items a looping wheel lays out in total. See
    /// [`CupertinoPicker::with_looping`].
    pub const LOOPING_ITEMS: usize = 20_000;

    /// How many items the wheel scrolls through, which is `count` unless it
    /// loops.
    fn wheel_count(&self) -> usize {
        if self.looping && self.count > 0 {
            self.turns() * self.count
        } else {
            self.count
        }
    }

    fn turns(&self) -> usize {
        (CupertinoPicker::LOOPING_ITEMS / self.count.max(1)).max(1)
    }

    /// The wheel index the caller's `initial_item` lands on: the same item,
    /// half the turns in, so there is as much wheel above it as below.
    fn wheel_initial(&self, item: usize) -> usize {
        if self.looping && self.count > 0 {
            self.turns() / 2 * self.count + item % self.count
        } else {
            item
        }
    }

    /// The caller's index for a wheel index.
    fn logical_index(&self, index: usize) -> usize {
        if self.looping && self.count > 0 {
            index % self.count
        } else {
            index
        }
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
        let last = self.wheel_count().saturating_sub(1);
        scroll.set_extent(index_to_scroll_offset(last, self.item_extent), 0.0);
        // A `scrollController` outranks `initial_item`, as upstream's does:
        // the two are the same argument, one held by the caller.
        let initial = match &self.scroll_controller {
            Some(controller) => controller.initial_item(),
            None => self.initial_item,
        };
        scroll.offset =
            index_to_scroll_offset(self.wheel_initial(initial).min(last), self.item_extent);
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
        let last = self.wheel_count().saturating_sub(1);
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
        let index = self.logical_index(index);
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
        // The owner's handle on this wheel, if it kept one.
        if let Some(controller) = &self.scroll_controller {
            controller.attach(handle.clone(), self.item_extent, self.count, self.looping);
        }
        // Themed `labels` items: publish this build's label color before the
        // item closures are assembled below.
        if let Some(label_color) = &self.label_color {
            label_color.set(theme.resolve(CupertinoColors::LABEL));
        }
        let offset = state.scroll.offset;
        let extent = self.item_extent;
        let count = self.wheel_count();
        let logical_count = self.count;
        let looping = self.looping;
        let diameter_ratio = self.diameter_ratio;
        let squeeze = self.squeeze;
        let magnification = self.magnification;
        let use_magnifier = self.use_magnifier;
        let off_axis_fraction = self.off_axis_fraction;
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
                        // Back through `_getUntransformedPaintingCoordinateY`:
                        // the wheel is anchored at its middle, so the layout
                        // coordinate under the finger is the tap's y plus the
                        // offset, less the half viewport the anchor is worth.
                        let layout_y = state.scroll.offset + tap.local_position.dy
                            - height_estimate / 2.0
                            + extent / 2.0;
                        let index = (layout_y / extent)
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
            let item = if looping && logical_count > 0 {
                index % logical_count
            } else {
                index
            };
            children.push((self.build_item)(item));
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
                // `_getUntransformedPaintingCoordinateY` plus half an item:
                // the wheel is anchored at its middle (see
                // `RenderListWheelViewport::top_scroll_margin_extent`), and
                // this has to agree with what the render object will do or the
                // dim lands on different rows than the magnifier.
                let flat_center =
                    index as f32 * extent + height_estimate / 2.0 - extent / 2.0 - offset
                        + extent / 2.0;
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

            let wheel = RenderListWheelViewport {
                children: items,
                first_index: first,
                item_extent: extent,
                offset,
                diameter_ratio,
                squeeze,
                use_magnifier,
                magnification,
                off_axis_fraction,
                over_and_under_center_opacity: 1.0,
                render_children_outside_viewport: false,
                clip_behavior: crate::painting::ClipBehavior::HardEdge,
                perspective: PICKER_PERSPECTIVE,
                viewport_sink: viewport.clone(),
                laid_out: Size::ZERO,
                child_data: std::cell::RefCell::new(Vec::new()),
            };

            let mut stack = RenderStack::new().with_fit(crate::render::StackFit::Expand);
            stack = stack.push(wheel);
            if let Some(overlay) = selection_overlay {
                // `CupertinoPickerDefaultSelectionOverlay`: a centered band
                // `itemExtent * magnification` tall, inset 9 on the edges it
                // caps, corners rounded 8 on those same edges, in
                // tertiarySystemFill -- and `IgnorePointer`, which the
                // render-level equivalent of is `RenderIgnorePointer`. A
                // non-positioned child under `StackFit::Expand` fills the
                // stack, so `Center` puts the fixed-height band in the middle.
                //
                // The caps are what let a date picker's three columns read as
                // one band: see the type's own docs.
                let (start_radius, end_radius) = overlay.radii();
                stack = stack.push(crate::render::RenderIgnorePointer::new(Center::new(
                    Container::new()
                        .with_height(extent * magnification)
                        .with_margin(overlay.margin())
                        .with_color(overlay_color)
                        .with_border_radius(crate::borders::BorderRadius::horizontal(
                            crate::borders::Radius::circular(start_radius),
                            crate::borders::Radius::circular(end_radius),
                        )),
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
/// The placeholder defaults to the localized
/// `searchTextFieldPlaceholderLabel` ("Search"), as upstream's does:
///
/// ```dart
/// final String placeholder =
///     widget.placeholder ?? CupertinoLocalizations.of(context).searchTextFieldPlaceholderLabel;
/// ```
///
/// **This doc used to say the opposite** -- "there are no localizations in
/// this crate, so the placeholder is unset unless the caller sets one" -- and
/// that was true when it was written. The crate grew a
/// [`crate::cupertino_app::DefaultCupertinoLocalizations`] several ticks
/// later and the sentence stayed, so a search field built without a
/// placeholder was an empty grey well where upstream shows a word. The note
/// names no backticked subject, so `stale_notes.py` is dumb to it by its own
/// documented rule; this one was found by hand.
///
/// The placeholder's color is the field's own muted color rather than
/// `secondaryLabel`, a half-shade difference noted rather than fixed.
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

    /// The word the well shows when it is empty: whatever the caller set, or
    /// upstream's localized default.
    ///
    /// `unwrap_or` and not `unwrap_or_default`: an unset placeholder falls
    /// back to "Search", while a placeholder explicitly set to the empty
    /// string is a caller asking for a blank well and gets one.
    ///
    /// **The build below is the only caller, and no test here proves it.**
    /// Replacing that one line with the old `if let Some(..)` -- reinstating
    /// the empty well -- turns nothing red: this decision is tested, its use
    /// is not.
    ///
    /// The reason given here used to be that `RenderBox::visit_children` is a
    /// no-op by default, so a walk from the root reached two leaves and
    /// stopped. **That was wrong**, and tick 299 found out why: the walk does
    /// reach every node, but each one arrives wrapped in a `RenderRef`, so a
    /// downcast answers `None` at every step. [`crate::render::unwrapped`]
    /// exists now and lifts that. What still stands between here and the
    /// assertion is narrower: this widget's decisions land in the *widget*
    /// tree -- a placeholder string, a tap handler -- and the render walk does
    /// not carry them.
    pub fn effective_placeholder(&self) -> String {
        self.placeholder.clone().unwrap_or_else(|| {
            crate::cupertino_app::DefaultCupertinoLocalizations::SEARCH_TEXT_FIELD_PLACEHOLDER_LABEL
                .to_string()
        })
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

    /// Upstream's `_defaultOnSuffixTap`: whether tapping the clear button
    /// should announce the change, given what was in the field.
    ///
    /// ```dart
    /// void _defaultOnSuffixTap() {
    ///   final bool textChanged = _effectiveController.text.isNotEmpty;
    ///   _effectiveController.clear();
    ///   if (widget.onChanged != null && textChanged) {
    ///     widget.onChanged!(_effectiveController.text);
    ///   }
    /// }
    /// ```
    ///
    /// # Three things in six lines, and this port had none of them
    ///
    /// * **`textChanged` is read before the clear.** After it, the field is
    ///   empty either way and the question cannot be asked any more.
    /// * **Clearing an already-empty field announces nothing.** With the
    ///   default `suffixMode` of `editing` the button is not there to tap, so
    ///   this guard looks dead -- but `suffixMode` is a caller's choice, and
    ///   under `always` the button sits on an empty field. An application
    ///   searching as the reader types would otherwise re-run its search on
    ///   every tap of a button that did nothing.
    /// * **`onChanged` is given the text read *after* clearing** -- the empty
    ///   string, not what was there. It is a change notification, so it
    ///   carries the new value.
    ///
    /// The clear itself is unconditional; only the announcement is guarded.
    ///
    /// Answers **what to announce** rather than whether to, so the empty
    /// string travels with the decision: a caller cannot pass the old text by
    /// mistake, which is the shape the Dart makes easy to get wrong -- the
    /// controller is right there, and `onChanged(oldText)` reads fine.
    pub fn suffix_tap(text_before: &str) -> Option<&'static str> {
        if text_before.is_empty() {
            return None;
        }
        Some("")
    }

    /// Whether tapping clear announces anything at all, which is
    /// [`CupertinoSearchTextField::suffix_tap`] read as a question.
    pub fn suffix_tap_announces(text_before: &str) -> bool {
        CupertinoSearchTextField::suffix_tap(text_before).is_some()
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
        let on_changed_for_clear = self.on_changed.clone();
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
        field = field.with_placeholder(self.effective_placeholder());
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
        let show_clear_text = state.text.clone();
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
                let on_changed_for_clear = on_changed_for_clear.clone();
                // Read at build time rather than in the handler: `set_state`
                // may defer, so the state cannot be read back synchronously
                // from inside a tap. This is the same text the button's own
                // presence was decided from a few lines above.
                let text_before = show_clear_text.clone();
                let clear = Pointer::new(
                    id,
                    crate::widgets::Padding::new(EdgeInsets::only(0.0, 8.0, 5.0, 8.0), clear_mark),
                )
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    // Upstream's `_defaultOnSuffixTap`. Whether to announce is
                    // decided from the text *before* the clear, because after
                    // it the field is empty either way -- see
                    // [`CupertinoSearchTextField::suffix_tap`].
                    //
                    // These four lines are not covered: mutations that stop
                    // announcing, or announce the old text, stay green.
                    //
                    // Tick 329 said the reason was that a tap handler in a
                    // build closure cannot be reached from a test. **That was
                    // wrong** -- tick 330 built this field in an
                    // `ElementTree`, laid it out, painted it and sent a real
                    // Down/Up through `GestureRouter::dispatch`, which is how
                    // `editable.rs` has tested its own taps all along.
                    //
                    // What that experiment found instead is worse and is
                    // written up in PORTING_STATUS: with text in the field
                    // this button **is built** (the build reports
                    // `show_clear=true`) and the handler below **still never
                    // runs**, at any point inside the laid-out 300x44 box. So
                    // the clear button does not respond to taps at all, and
                    // the missing coverage is a symptom rather than the
                    // problem. Sharing a pointer-region id with the field was
                    // ruled out; the cause is not yet known.
                    let announcement = CupertinoSearchTextField::suffix_tap(&text_before);

                    // `_clearText`: empty the field through its own handle,
                    // which also tells the IME, and empty the mirror.
                    if let Some(field_handle) = &*sink.borrow() {
                        field_handle.set_state(|state| state.clear());
                    }
                    clear_handle.set_state(|state| state.text.clear());

                    // With the *new* text, which is the empty string: this is
                    // a change notification and it carries the new value.
                    if let (Some(text), Some(on_changed)) = (announcement, &on_changed_for_clear) {
                        on_changed(text);
                    }
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

/// How far the child grows while the press is held. context_menu.dart's
/// `_kOpenScale`.
pub const CONTEXT_MENU_OPEN_SCALE: f32 = 1.15;

/// The floor that growth is clamped to when the grown child would leave the
/// safe area. context_menu.dart's `_kMinScaleFactor`.
pub const CONTEXT_MENU_MIN_SCALE_FACTOR: f32 = 1.02;

/// How long the press has to be held before the menu opens. context_menu.dart's
/// `_previewLongPressTimeout`.
///
/// **Not [`crate::gestures::LONG_PRESS_TIMEOUT_MICROS`]**: upstream runs its own
/// controller off the tap-down rather than using a `LongPressGestureRecognizer`,
/// because the same controller drives the child's growth -- the animation *is*
/// the timer, and it is 300ms longer than an ordinary long press.
pub const CONTEXT_MENU_PREVIEW_TIMEOUT: Duration = Duration::from_millis(800);

/// How long the menu takes to open once the press is done.
/// context_menu.dart's `_kModalPopupTransitionDuration`.
pub const CONTEXT_MENU_TRANSITION: Duration = Duration::from_millis(335);

/// Where in the combined animation the menu starts opening. Upstream's
/// `CupertinoContextMenu.animationOpensAt`, which is the press timeout over the
/// sum of the two durations.
pub fn context_menu_animation_opens_at() -> f32 {
    let press = CONTEXT_MENU_PREVIEW_TIMEOUT.as_millis() as f32;
    press / (press + CONTEXT_MENU_TRANSITION.as_millis() as f32)
}

/// The shadow the child has grown by the time the menu opens.
/// context_menu.dart's `_endBoxShadow`, also `CupertinoContextMenu.kEndBoxShadow`.
pub const CONTEXT_MENU_END_BOX_SHADOW: crate::painting::BoxShadow =
    crate::painting::BoxShadow::new(Color(0x4000_0000), 0.0, 0.0, 10.0, 0.5);

/// The gap the open menu keeps from the screen edges and between the preview
/// and the sheet. `_ContextMenuRouteStaticState._kPadding`.
pub const CONTEXT_MENU_PADDING: f32 = 20.0;

/// How hard the page behind an open menu is blurred: the route's
/// `ui.ImageFilter.blur(sigmaX: 5.0, sigmaY: 5.0)`.
pub const CONTEXT_MENU_BLUR_SIGMA: f32 = 5.0;

/// The longest a single frame may advance the press or the transition by,
/// roughly three frames at sixty a second. See the clamp in
/// [`CupertinoContextMenu::advance`].
const MAX_FRAME_MICROS: i64 = 50_000;

/// Which side of the screen the menu's child was on, which is what decides
/// where the sheet goes. Upstream's `_ContextMenuLocation`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContextMenuLocation {
    #[default]
    Center,
    Left,
    Right,
}

/// Upstream's `_CupertinoContextMenuState._contextMenuLocation`: near enough to
/// the middle counts as centred, and otherwise it is whichever half the child's
/// centre falls in.
pub fn context_menu_location(child: Rect, screen_width: f32) -> ContextMenuLocation {
    let center = screen_width / 2.0;
    let (child_center_x, _) = child.center();
    let center_divides_child = child.left < center && child.right > center;
    let distance_from_center = (center - child_center_x).abs();
    if center_divides_child && distance_from_center <= child.width() / 4.0 {
        return ContextMenuLocation::Center;
    }
    if child_center_x > center {
        ContextMenuLocation::Right
    } else {
        ContextMenuLocation::Left
    }
}

/// How far the child may grow without leaving the safe area. Upstream's
/// `_CupertinoContextMenuState._getScaleFactor`.
pub fn context_menu_scale_factor(child: Rect, padding: EdgeInsets, size: Size) -> f32 {
    let (center_x, center_y) = child.center();
    let left_max = 2.0 * (center_x - padding.left) / child.width();
    let top_max = 2.0 * (center_y - padding.top) / child.height();
    let right_max = 2.0 * (size.width - padding.right - center_x) / child.width();
    let bottom_max = 2.0 * (size.height - padding.bottom - center_y) / child.height();
    let min_width = left_max.min(right_max);
    let min_height = top_max.min(bottom_max);
    min_width
        .min(min_height)
        .clamp(CONTEXT_MENU_MIN_SCALE_FACTOR, CONTEXT_MENU_OPEN_SCALE)
}

/// Which corner the sheet grows out of. Upstream's
/// `_ContextMenuRoute.getSheetAlignment`, resolved -- this crate's `Alignment`
/// is absolute, and the tier is left-to-right.
pub fn context_menu_sheet_alignment(
    location: ContextMenuLocation,
    orientation: crate::presence::Orientation,
) -> Alignment {
    match location {
        ContextMenuLocation::Center if orientation == crate::presence::Orientation::Landscape => {
            Alignment::TOP_LEFT
        }
        ContextMenuLocation::Center => Alignment::TOP_CENTER,
        ContextMenuLocation::Right => Alignment::TOP_RIGHT,
        ContextMenuLocation::Left => Alignment::TOP_LEFT,
    }
}

/// Where the sheet starts from, so that it appears to unfold out of the child.
/// Upstream's `_ContextMenuRoute._getSheetRectBegin`.
pub fn context_menu_sheet_rect_begin(
    orientation: crate::presence::Orientation,
    location: ContextMenuLocation,
    child: Rect,
    sheet: Size,
) -> Rect {
    let portrait = orientation == crate::presence::Orientation::Portrait;
    let y = if portrait { child.bottom } else { child.top };
    let x = match location {
        ContextMenuLocation::Center => {
            let (center_x, _) = child.center();
            center_x - sheet.width / 2.0
        }
        ContextMenuLocation::Right => child.right - sheet.width,
        ContextMenuLocation::Left => child.left,
    };
    Rect::xywh(x, y, sheet.width, sheet.height)
}

/// Corner-by-corner, upstream's `RectTween`.
fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::ltrb(
        a.left + (b.left - a.left) * t,
        a.top + (b.top - a.top) * t,
        a.right + (b.right - a.right) * t,
        a.bottom + (b.bottom - a.bottom) * t,
    )
}

/// The preview's slot in [`ContextMenuLayout`], upstream's
/// `_ContextMenuChild.child`.
pub const CONTEXT_MENU_PREVIEW_SLOT: u64 = 0;
/// The sheet's slot, upstream's `_ContextMenuChild.menuSheet`.
pub const CONTEXT_MENU_SHEET_SLOT: u64 = 1;

/// Where the open menu's preview and sheet go.
///
/// This is upstream's `_ContextMenuAlignedChildrenDelegate` **plus** the rect
/// tweens of `_ContextMenuRoute.buildTransitions`, which upstream keeps apart:
/// there the route renders a `Stack` of two `Positioned.fromRect`s while the
/// transition runs and swaps to `_ContextMenuRouteStatic` once it is over, and
/// the tweens' end points come from measuring that static layout on a frame
/// rendered offstage.
///
/// Folding them together is what lets this port skip the offstage frame: the
/// delegate *is* the static layout, so it has both end points in hand while it
/// is laying out, and interpolating there costs one relayout per frame instead
/// of a second render pass. At `t == 1` it lays out exactly what upstream's
/// static route does.
///
/// The preview is tweened by **size**, not by a scale transform: its widget is
/// a `FittedBox(fit: cover)`, so laying it out at the interpolated rect scales
/// its contents to match, which is what upstream's `Positioned.fromRect` around
/// the same `FittedBox` comes to. The sheet keeps its own size and is scaled by
/// a [`crate::render::RenderTransform`] in the tree, as upstream's is -- a sheet
/// laid out narrow would re-wrap its labels rather than shrink.
pub struct ContextMenuLayout {
    /// The child's rectangle before the press, upstream's `childRect` -- what
    /// portrait placement anchors to.
    pub target_rect: Rect,
    /// The screen less its safe-area padding, upstream's `screenBounds`.
    pub screen_bounds: Rect,
    pub orientation: crate::presence::Orientation,
    pub location: ContextMenuLocation,
    /// Where the preview comes from: the rectangle the press grew it to
    /// (`_previousChildRect`) while opening, and the child's own rectangle
    /// while closing -- upstream's `_rectTween` and `_rectTweenReverse`.
    pub from: Rect,
    /// How far through the open transition, already curved.
    pub t: f32,
}

impl ContextMenuLayout {
    fn same(&self, other: &ContextMenuLayout) -> bool {
        self.target_rect == other.target_rect
            && self.screen_bounds == other.screen_bounds
            && self.orientation == other.orientation
            && self.location == other.location
            && self.from == other.from
            && self.t == other.t
    }
}

impl crate::render::MultiChildLayoutDelegate for ContextMenuLayout {
    fn perform_layout(&self, size: Size, context: &mut crate::render::MultiChildLayoutContext) {
        let landscape = self.orientation == crate::presence::Orientation::Landscape;
        let bounds = self.screen_bounds;

        // Upstream's `performLayout`, up to the point where it positions.
        let available_height_for_child = (bounds.height() - CONTEXT_MENU_PADDING).max(0.0);
        let available_width = (bounds.width() - CONTEXT_MENU_PADDING * 2.0).max(0.0);
        let available_width_for_child = if landscape {
            (available_width - CONTEXT_MENU_SHEET_WIDTH).max(0.0)
        } else {
            available_width
        };

        let child_size = context
            .layout_child(
                CONTEXT_MENU_PREVIEW_SLOT,
                BoxConstraints::new(
                    0.0,
                    available_width_for_child,
                    0.0,
                    available_height_for_child,
                ),
            )
            .unwrap_or(Size::ZERO);

        // Portrait puts the sheet under the preview, so the preview's height
        // has already been spent; landscape puts it beside, so it has not.
        let available_height_for_menu = if landscape {
            available_height_for_child
        } else {
            (available_height_for_child - (child_size.height + CONTEXT_MENU_PADDING)).max(0.0)
        };
        let menu_size = context
            .layout_child(
                CONTEXT_MENU_SHEET_SLOT,
                BoxConstraints::new(0.0, size.width, 0.0, available_height_for_menu),
            )
            .unwrap_or(Size::ZERO);

        let initial_child_left;
        let initial_child_top;
        let max_clamped_left;
        let max_clamped_top;
        let second_child_offset;
        let menu_before_child;
        if landscape {
            menu_before_child = self.location == ContextMenuLocation::Right;
            let total_width = child_size.width + menu_size.width + CONTEXT_MENU_PADDING;
            let (bounds_center_x, bounds_center_y) = bounds.center();
            initial_child_left = bounds_center_x - total_width / 2.0;
            initial_child_top = bounds_center_y - child_size.height.max(menu_size.height) / 2.0;
            let second_child_dx = if menu_before_child {
                menu_size.width
            } else {
                child_size.width
            };
            second_child_offset = Offset::new(second_child_dx + CONTEXT_MENU_PADDING, 0.0);
            max_clamped_left = bounds.right - total_width;
            max_clamped_top = bounds.bottom;
        } else {
            menu_before_child = false;
            let total_height = child_size.height + menu_size.height + CONTEXT_MENU_PADDING;
            let total_width = child_size.width + CONTEXT_MENU_PADDING;
            let (target_center_x, target_center_y) = self.target_rect.center();
            initial_child_left = target_center_x - child_size.width / 2.0;
            initial_child_top = target_center_y - child_size.height;
            let second_child_dx = match self.location {
                ContextMenuLocation::Center => child_size.width / 2.0 - menu_size.width / 2.0,
                ContextMenuLocation::Left => 0.0,
                ContextMenuLocation::Right => child_size.width - menu_size.width,
            };
            second_child_offset =
                Offset::new(second_child_dx, child_size.height + CONTEXT_MENU_PADDING);
            max_clamped_left = bounds.right - total_width;
            max_clamped_top = bounds.bottom - total_height;
        }

        // Upstream clamps with `clampDouble`, whose lower bound wins when the
        // two cross -- which they do on a screen too small for the assembly.
        let clamped_left = initial_child_left
            .max(bounds.left + CONTEXT_MENU_PADDING)
            .min(max_clamped_left.max(bounds.left + CONTEXT_MENU_PADDING));
        let clamped_top = initial_child_top
            .max(bounds.top + CONTEXT_MENU_PADDING)
            .min(max_clamped_top.max(bounds.top + CONTEXT_MENU_PADDING));
        let first = Offset::new(clamped_left, clamped_top);
        let second = Offset::new(
            first.dx + second_child_offset.dx,
            first.dy + second_child_offset.dy,
        );
        let (child_offset, sheet_offset) = if menu_before_child {
            (second, first)
        } else {
            (first, second)
        };

        // The transition, folded in (see the type docs). The preview walks from
        // `from` to where it has just been laid out, and is laid out again at
        // the rectangle it has reached; the sheet walks from the edge of the
        // child it belongs to.
        let child_end = Rect::xywh(
            child_offset.dx,
            child_offset.dy,
            child_size.width,
            child_size.height,
        );
        let child_now = lerp_rect(self.from, child_end, self.t);
        context.layout_child(
            CONTEXT_MENU_PREVIEW_SLOT,
            BoxConstraints::tight(child_now.width(), child_now.height()),
        );
        context.position_child(
            CONTEXT_MENU_PREVIEW_SLOT,
            Offset::new(child_now.left, child_now.top),
        );

        let sheet_end = Rect::xywh(
            sheet_offset.dx,
            sheet_offset.dy,
            menu_size.width,
            menu_size.height,
        );
        let sheet_begin = context_menu_sheet_rect_begin(
            self.orientation,
            self.location,
            self.target_rect,
            menu_size,
        );
        let sheet_now = lerp_rect(sheet_begin, sheet_end, self.t);
        context.position_child(
            CONTEXT_MENU_SHEET_SLOT,
            Offset::new(sheet_now.left, sheet_now.top),
        );
    }

    fn should_relayout(&self, old: &dyn crate::render::MultiChildLayoutDelegate) -> bool {
        match old.as_any().downcast_ref::<ContextMenuLayout>() {
            Some(old) => !self.same(old),
            None => true,
        }
    }

    fn kind_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<ContextMenuLayout>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// What a [`CupertinoContextMenuAction`] remembers between frames.
#[derive(Default)]
pub struct CupertinoContextMenuActionState {
    /// Whether the action is held, for the pressed fill.
    pub pressed: bool,
}

/// One action in a context menu's sheet. Upstream's
/// `CupertinoContextMenuAction` (context_menu_action.dart).
///
/// `Clone` because [`CupertinoContextMenu`] rebuilds its sheet on every frame
/// of the open transition, and a built `AnyWidget` can only be handed over
/// once. Every field is either plain data or a shared callback, so the copy is
/// the same action rather than a second one.
#[derive(Clone)]
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

    /// Upstream's `onPressed`, for a caller whose callback is not a state
    /// change -- closing the menu through a
    /// [`CupertinoContextMenuController`], above all.
    pub fn on_pressed(mut self, action: impl Fn() + 'static) -> Self {
        self.on_pressed = Some(Rc::new(action));
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
/// [`CupertinoContextMenu`] builds one of these itself and places it; this is
/// public for a caller who wants the surface on its own.
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

    /// The same list, already built -- what [`CupertinoContextMenu`] has in
    /// hand by the time it puts the sheet up.
    pub fn with_actions(self, actions: Vec<AnyWidget>) -> Self {
        *self.actions.borrow_mut() = actions;
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
        let actions = self.actions.borrow().clone();
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

/// Closes an open [`CupertinoContextMenu`]. Upstream's actions call
/// `Navigator.pop(context)`; the menu here is an overlay portal rather than a
/// route, so the handle it hands out is what stands in for the navigator.
///
/// Take one from [`CupertinoContextMenu::controller`] *before* the menu is
/// built and wire the actions to it, the way upstream's actions close over
/// their `BuildContext`.
#[derive(Clone, Default)]
pub struct CupertinoContextMenuController {
    close: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
}

impl CupertinoContextMenuController {
    pub fn new() -> CupertinoContextMenuController {
        CupertinoContextMenuController::default()
    }

    /// Closes the menu, if it is open and has been built at least once.
    pub fn close(&self) {
        let close = self.close.borrow().clone();
        if let Some(close) = close {
            close();
        }
    }

    /// Filled in by the menu's build, which is the first moment there is a
    /// state to talk to.
    fn arm(&self, close: Rc<dyn Fn()>) {
        *self.close.borrow_mut() = Some(close);
    }
}

/// Upstream's `CupertinoContextMenu`: a widget that, held down, grows and then
/// opens full-screen over a blurred page with its actions beside it.
///
/// The whole of upstream's shape is here -- the press animation with its
/// shadow, the child hidden in place while the menu is up, the blurred and
/// dimmed barrier, the preview and the sheet placed by
/// [`ContextMenuLayout`], and the open and close transitions. Three things are
/// deliberately different:
///
/// - **The menu is an overlay portal, not a `PopupRoute`.** Upstream pushes on
///   the root navigator, which is what makes the menu cover the application
///   rather than the page that opened it; the portal reaches the same root
///   overlay ([`crate::theatre`]) and covers the same surface, and the price is
///   that the system back gesture does not pop it.
/// - **The preview is `builder`'s widget in a `FittedBox`, and the corner
///   rounding is circular.** Upstream's `_defaultPreviewBuilder` clips to a
///   `ClipRSuperellipse`, which the paint bridge has no shape for (see the
///   module docs).
/// - **There is no drag-to-dismiss.** Upstream's `_ContextMenuRouteStatic`
///   lets the open menu be dragged down, scaling the preview and fading the
///   sheet out until it lets go; here the barrier and the actions are what
///   close it. Nothing else in this tier has a dismissible modal either.
pub struct CupertinoContextMenu {
    id: u64,
    /// Upstream's `builder`. A closure rather than a widget because the child
    /// is built three times over -- in place, in the press decoy, and as the
    /// open preview -- and a widget can only be handed over once.
    builder: Rc<dyn Fn() -> AnyWidget>,
    actions: RefCell<Vec<CupertinoContextMenuAction>>,
    controller: CupertinoContextMenuController,
}

impl CupertinoContextMenu {
    pub fn new(id: u64) -> CupertinoContextMenu {
        CupertinoContextMenu {
            id,
            builder: Rc::new(|| leaf(|| Empty)),
            actions: RefCell::new(Vec::new()),
            controller: CupertinoContextMenuController::new(),
        }
    }

    /// Upstream's `child`, as the builder the port needs (see the field).
    pub fn with_child(self, child: impl Fn() -> AnyWidget + 'static) -> Self {
        CupertinoContextMenu {
            builder: Rc::new(child),
            ..self
        }
    }

    /// Upstream's `actions`, in order.
    pub fn push_action(self, action: CupertinoContextMenuAction) -> Self {
        self.actions.borrow_mut().push(action);
        self
    }

    /// The handle that closes this menu. Wire the actions to it.
    pub fn controller(&self) -> CupertinoContextMenuController {
        self.controller.clone()
    }
}

/// What an open [`CupertinoContextMenu`] remembers.
pub struct CupertinoContextMenuState {
    /// Upstream's `_openController`: the press, which is also the timer that
    /// decides when the menu opens.
    press: Controller,
    /// The route's own animation, upstream's `_ContextMenuRoute.animation`.
    route: Controller,
    /// Upstream's `_childHidden`: the child is drawn by the decoy and then by
    /// the preview, and would otherwise be drawn twice.
    child_hidden: bool,
    /// Whether the menu itself is up, as opposed to the press growing.
    open: bool,
    /// Upstream's `childRect`, measured at tap-down.
    child_rect: Rect,
    /// Upstream's `_decoyChildEndRect`, which is also the route's
    /// `_previousChildRect`.
    decoy_end_rect: Rect,
    location: ContextMenuLocation,
    portal: PortalController,
    anchor: Anchor,
    last_frame_micros: Option<i64>,
}

impl Default for CupertinoContextMenuState {
    fn default() -> CupertinoContextMenuState {
        CupertinoContextMenuState {
            press: Controller::new(CONTEXT_MENU_PREVIEW_TIMEOUT),
            route: Controller::new(CONTEXT_MENU_TRANSITION),
            child_hidden: false,
            open: false,
            child_rect: Rect::ltrb(0.0, 0.0, 0.0, 0.0),
            decoy_end_rect: Rect::ltrb(0.0, 0.0, 0.0, 0.0),
            location: ContextMenuLocation::Center,
            portal: PortalController::new(),
            anchor: Anchor::new(),
            last_frame_micros: None,
        }
    }
}

impl StatefulComponent for CupertinoContextMenu {
    type State = CupertinoContextMenuState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    fn advance(&self, state: &mut CupertinoContextMenuState, frame_time_micros: i64) -> bool {
        let elapsed = match state.last_frame_micros {
            Some(previous) if frame_time_micros > previous => {
                // Clamped, because the gap is not the frame rate. Frames are
                // on demand: nothing draws while the demo sits still, so the
                // press that starts the growth is measured against a frame
                // from however long ago the reader last did anything.
                // Unclamped, the very first tick of the press ran the whole
                // 800ms in one step and the menu opened the instant it was
                // touched.
                let gap = (frame_time_micros - previous).min(MAX_FRAME_MICROS);
                Duration::from_micros(gap as u64)
            }
            _ => Duration::ZERO,
        };
        state.last_frame_micros = Some(frame_time_micros);

        let mut moving = state.press.tick(elapsed);
        moving |= state.route.tick(elapsed);

        // Upstream's `_onDecoyAnimationStatusChange`: a press that runs to the
        // end opens the menu, and one that unwinds puts the child back.
        if !state.open
            && state.press.direction() == Direction::Forward
            && state.press.value() >= 1.0
        {
            state.open = true;
            state.route.set_value(0.0);
            state.route.forward();
            moving = true;
        }
        if !state.open
            && state.press.direction() == Direction::Reverse
            && state.press.is_settled()
            && state.child_hidden
        {
            state.child_hidden = false;
            state.portal.hide();
        }

        // Upstream's `_routeAnimationStatusListener`: the route finished
        // closing, so the child is the child again.
        if state.open && state.route.direction() == Direction::Reverse && state.route.is_settled() {
            state.open = false;
            state.child_hidden = false;
            state.press.set_value(0.0);
            state.portal.hide();
        }
        moving
    }

    fn build(
        &self,
        state: &CupertinoContextMenuState,
        handle: StateHandle<CupertinoContextMenuState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let media = crate::media_query::media_query_of(context);
        let screen = media.size;
        let padding = media.padding;
        let orientation = crate::media_query::orientation_of(context);

        // Upstream's `_onTapDown`/`_onTapCompleted`, which is where the child's
        // rectangle is measured and the growth is armed. `press_change` is this
        // tier's tap-down and tap-cancel in one: it goes true when the press
        // lands and false when it is lifted, cancelled, or dragged past the
        // slop -- the three ways upstream's recogniser completes a tap.
        let anchor = state.anchor.clone();
        let portal = state.portal.clone();
        let pressed_handle = handle.clone();
        let handlers = PointerHandlers::new().with_press_change(move |down| {
            let rect = anchor.rect();
            let portal = portal.clone();
            pressed_handle.set_state(move |state| {
                if down {
                    if state.open {
                        return;
                    }
                    if let Some(rect) = rect {
                        state.child_rect = rect;
                        let scale = context_menu_scale_factor(rect, padding, screen);
                        let (center_x, center_y) = rect.center();
                        state.decoy_end_rect = Rect::from_center(
                            center_x,
                            center_y,
                            rect.width() * scale,
                            rect.height() * scale,
                        );
                        state.location = context_menu_location(rect, screen.width);
                    }
                    state.child_hidden = true;
                    state.press.set_value(0.0);
                    state.press.forward();
                    portal.show();
                } else if !state.open {
                    state.press.reverse();
                }
            });
        });

        // Upstream's `Visibility.maintain(visible: !_childHidden)`: the child
        // keeps its place in the layout, and stops being drawn.
        let id = self.id;
        let child = (self.builder)();
        let child_hidden = state.child_hidden;
        let anchor_for_target = state.anchor.clone();
        let target = many(vec![child], move |mut rendered| {
            let child = rendered.pop().expect("the context menu's child");
            anchor_for_target.set(child.clone());
            // `Visibility.maintain` keeps the space and stops the paint; the
            // nearest thing here is a zero opacity, which is also what upstream
            // falls back to when `maintainSize` is asked for.
            let child: BoxedRender = if child_hidden {
                RenderRef::new(RenderOpacity::new(0.0, child))
            } else {
                child
            };
            crate::render::RenderPointerRegion::new(id, child).with_handlers(handlers.clone())
        });

        // The close handle the actions are wired to, and the barrier's tap.
        let closing_handle = handle.clone();
        let close: Rc<dyn Fn()> = Rc::new(move || {
            closing_handle.set_state(|state| {
                if state.open {
                    state.route.reverse();
                }
            });
        });
        self.controller.arm(Rc::clone(&close));

        // Cloned rather than taken: `advance` marks *this* element dirty on
        // every frame of the animation, so `build` runs again against the same
        // widget while the demo above it sits still. Taking them left the
        // sheet empty from the second frame on -- which is a 250-wide
        // transparent nothing, not a missing widget, so it looked like the
        // sheet had been positioned off the screen.
        let actions = self.actions.borrow().clone();
        let open = state.open;
        let press = state.press.value();
        let route_value = state.route.value();
        let closing = state.route.direction() == Direction::Reverse;
        let child_rect = state.child_rect;
        let decoy_end_rect = state.decoy_end_rect;
        let location = state.location;
        let builder = Rc::clone(&self.builder);
        let barrier_id = self.id;
        let barrier_close = Rc::clone(&close);

        crate::theatre::overlay_portal(state.portal.clone(), target, move || {
            if !open {
                return context_menu_decoy(child_rect, decoy_end_rect, press, (builder)());
            }
            // Upstream runs the open transition on `easeOutBack` and the close
            // on `easeInBack` (`_ContextMenuRoute._curve`/`_curveReverse`).
            let t = if closing {
                Curve::EASE_IN_BACK.transform(route_value)
            } else {
                Curve::EASE_OUT_BACK.transform(route_value)
            };
            // The reverse tween starts from the child's own rectangle rather
            // than the one the press grew it to: upstream's `_rectTweenReverse`.
            let from = if closing { child_rect } else { decoy_end_rect };
            context_menu_route(ContextMenuRoute {
                id: barrier_id,
                preview: (builder)(),
                actions: actions.iter().cloned().map(stateful).collect(),
                layout: ContextMenuLayout {
                    target_rect: child_rect,
                    // Upstream's `screenBounds`, spelled as upstream spells
                    // it: an origin at zero and the padding taken off both
                    // ends, which is only the safe area's own rectangle
                    // because the layout runs inside a `SafeArea` there and
                    // this one does not. The two agree wherever the padding
                    // is zero, which is every desktop and every screen
                    // without a cutout.
                    screen_bounds: Rect::xywh(
                        0.0,
                        0.0,
                        (screen.width - padding.left - padding.right).max(0.0),
                        (screen.height - padding.top - padding.bottom).max(0.0),
                    ),
                    orientation,
                    location,
                    from,
                    t,
                },
                opacity: route_value,
                border_radius: K_OPEN_BORDER_RADIUS * route_value,
                on_dismiss: Rc::clone(&barrier_close),
            })
        })
    }
}

/// The floating copy of the child while the press is held: upstream's
/// `_DecoyChild`, which grows it from `beginRect` to `endRect` and grows a
/// shadow under it.
fn context_menu_decoy(begin: Rect, end: Rect, press: f32, child: AnyWidget) -> AnyWidget {
    // Upstream's `TweenSequence`: a pause for a sixth of the press, then the
    // growth on `easeOutSine`, then a pause for the rest of the combined
    // animation. The trailing pause is time this port's controller does not
    // have -- its press controller runs to 1.0 at the moment the route opens --
    // so what is left is the leading pause and the growth.
    const BEGIN_PAUSE: f32 = 1.0;
    const OPEN_LENGTH: f32 = 5.0;
    let progress =
        (((press * (BEGIN_PAUSE + OPEN_LENGTH)) - BEGIN_PAUSE) / OPEN_LENGTH).clamp(0.0, 1.0);
    let rect = lerp_rect(begin, end, Curve::EASE_OUT_SINE.transform(progress));

    // `DecorationTween` from no shadow to `_endBoxShadow`, on the whole press.
    let shadow = crate::painting::BoxShadow {
        color: CONTEXT_MENU_END_BOX_SHADOW
            .color
            .with_alpha((CONTEXT_MENU_END_BOX_SHADOW.color.alpha() as f32 * press) as u8),
        offset: CONTEXT_MENU_END_BOX_SHADOW.offset,
        blur_radius: CONTEXT_MENU_END_BOX_SHADOW.blur_radius * press,
        spread_radius: CONTEXT_MENU_END_BOX_SHADOW.spread_radius * press,
    };

    single(child, move |child| {
        RenderRef::new(
            RenderStack::new().push_positioned(
                RenderRef::new(
                    Container::new()
                        .with_shadows(vec![shadow])
                        .with_child(child.clone()),
                ),
                StackPosition {
                    left: Some(rect.left),
                    top: Some(rect.top),
                    width: Some(rect.width()),
                    height: Some(rect.height()),
                    ..StackPosition::default()
                },
            ),
        )
    })
}

/// Everything the open menu draws.
struct ContextMenuRoute {
    id: u64,
    preview: AnyWidget,
    actions: Vec<AnyWidget>,
    layout: ContextMenuLayout,
    opacity: f32,
    border_radius: f32,
    on_dismiss: Rc<dyn Fn()>,
}

/// The open menu: the blurred, dimmed barrier, then the preview and the sheet
/// where [`ContextMenuLayout`] puts them.
fn context_menu_route(route: ContextMenuRoute) -> AnyWidget {
    let ContextMenuRoute {
        id,
        preview,
        actions,
        layout,
        opacity,
        border_radius,
        on_dismiss,
    } = route;

    // Upstream's `PopupRoute.barrierColor` under the route's blur `filter`,
    // both of which come in with the route's animation.
    let barrier_color = CONTEXT_MENU_BARRIER_COLOR
        .with_alpha((CONTEXT_MENU_BARRIER_COLOR.alpha() as f32 * opacity) as u8);
    let barrier = leaf(move || {
        let dismiss = Rc::clone(&on_dismiss);
        crate::render::RenderPointerRegion::new(
            id,
            RenderRef::new(crate::widgets::BackdropFilter::new(
                CONTEXT_MENU_BLUR_SIGMA * opacity,
                Container::new().with_color(barrier_color),
            )),
        )
        .with_handlers(PointerHandlers::new().with_tap(move |_| dismiss()))
        .with_behavior(crate::render::HitTestBehavior::Opaque)
    });

    // Upstream's `_defaultPreviewBuilder`: the child fitted to the preview's
    // rectangle, its corners rounding as the menu opens.
    let preview = single(preview, move |child| {
        RenderRef::new(
            RenderClipRect::new(
                crate::render::RenderFittedBox::boxed(child).with_fit(crate::render::BoxFit::Cover),
            )
            .with_corner_radius(border_radius),
        )
    });

    // Upstream's `Transform.scale(alignment: getSheetAlignment, scale:
    // sheetScale)` over a `FadeTransition`.
    let alignment = context_menu_sheet_alignment(layout.location, layout.orientation);
    let scale = layout.t;
    let sheet = component(CupertinoContextMenuSheet::new().with_actions(actions));
    let sheet = single(sheet, move |sheet| {
        RenderRef::new(RenderOpacity::new(
            opacity,
            RenderTransform::new_boxed([scale, 0.0, 0.0, scale, 0.0, 0.0], sheet)
                .with_origin(alignment),
        ))
    });

    let layout = Rc::new(layout);
    many(vec![barrier, preview, sheet], move |mut rendered| {
        let sheet = rendered.pop().expect("the sheet");
        let preview = rendered.pop().expect("the preview");
        let barrier = rendered.pop().expect("the barrier");
        RenderRef::new(
            RenderStack::new()
                .push_positioned(barrier, StackPosition::fill())
                .push(RenderRef::new(
                    crate::render::RenderCustomMultiChildLayoutBox::new(
                        Rc::clone(&layout) as Rc<dyn crate::render::MultiChildLayoutDelegate>,
                        vec![
                            (CONTEXT_MENU_PREVIEW_SLOT, preview),
                            (CONTEXT_MENU_SHEET_SLOT, sheet),
                        ],
                    ),
                )),
        )
    })
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

// -- iOS grouped lists --------------------------------------------------------

/// Upstream `CupertinoListSectionType` (`cupertino/list_section.dart`): the two
/// shapes an iOS grouped list comes in.
///
/// **Every constant in this family comes in a pair, one per variant**, and
/// that is the design rather than an accident: a *base* section runs edge to
/// edge with square corners, while an *inset grouped* one is a rounded card
/// held in from the sides. The two are different enough that sharing a number
/// between them would be wrong somewhere, so upstream shares none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CupertinoListSectionType {
    /// Edge to edge, square corners. iOS's older grouped table.
    #[default]
    Base,
    /// A rounded card inset from the screen's sides. The iOS 13 look.
    InsetGrouped,
}

/// Upstream `CupertinoListTile`: one row of such a list.
///
/// Upstream has two constructors -- the plain one and `.notched`, the iOS 13
/// look -- and they differ in every measurement rather than in a flag or two.
/// Hence [`CupertinoListTileStyle`] rather than a `bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CupertinoListTileStyle {
    #[default]
    Plain,
    /// Upstream's `CupertinoListTile.notched`.
    Notched,
}

/// Upstream `CupertinoListTile`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CupertinoListTile {
    pub style: CupertinoListTileStyle,
    pub has_leading: bool,
    pub has_subtitle: bool,
}

impl CupertinoListTile {
    /// Upstream's `_kLeadingSize`.
    pub const LEADING_SIZE: f32 = 28.0;
    /// Upstream's `_kNotchedLeadingSize`.
    pub const NOTCHED_LEADING_SIZE: f32 = 30.0;
    /// Upstream's `_kLeadingToTitle`.
    pub const LEADING_TO_TITLE: f32 = 16.0;
    /// Upstream's `_kNotchedLeadingToTitle`. Tighter, because a notched row's
    /// leading is larger -- the gap and the glyph together are what the eye
    /// reads as the indent, so the one gives way as the other grows.
    pub const NOTCHED_LEADING_TO_TITLE: f32 = 12.0;
    pub const NOTCHED_TITLE_TO_SUBTITLE: f32 = 3.0;
    pub const ADDITIONAL_INFO_TO_TRAILING: f32 = 6.0;
    pub const SUBTITLE_FONT_SIZE: f32 = 12.0;
    pub const NOTCHED_SUBTITLE_FONT_SIZE: f32 = 14.0;

    pub fn new() -> CupertinoListTile {
        CupertinoListTile::default()
    }

    pub fn notched() -> CupertinoListTile {
        CupertinoListTile {
            style: CupertinoListTileStyle::Notched,
            ..CupertinoListTile::default()
        }
    }

    pub fn with_leading(mut self) -> Self {
        self.has_leading = true;
        self
    }

    pub fn with_subtitle(mut self) -> Self {
        self.has_subtitle = true;
        self
    }

    /// How big the leading glyph's box is.
    pub fn leading_size(&self) -> f32 {
        match self.style {
            CupertinoListTileStyle::Plain => CupertinoListTile::LEADING_SIZE,
            CupertinoListTileStyle::Notched => CupertinoListTile::NOTCHED_LEADING_SIZE,
        }
    }

    /// Upstream's four minimum heights, each written as
    /// `leadingSize + 2 * padding` rather than as a number.
    ///
    /// The arithmetic is the statement: **a row is its leading glyph plus
    /// symmetric breathing room**, so the glyph's size is what sets the row's
    /// and the padding is what is left to choose. A hand-written 44 would say
    /// nothing about why it is 44, and would not follow the glyph if that
    /// changed.
    ///
    /// A subtitle makes the padding *grow* -- 10 instead of 8 -- so a two-line
    /// row is taller by more than the extra line. That is deliberate: two
    /// lines packed to the same margins as one would read as crowded even
    /// though nothing overlaps.
    pub fn min_height(&self) -> f32 {
        match (self.style, self.has_subtitle, self.has_leading) {
            (CupertinoListTileStyle::Plain, false, _) => {
                CupertinoListTile::LEADING_SIZE + 2.0 * 8.0
            }
            (CupertinoListTileStyle::Plain, true, _) => {
                CupertinoListTile::LEADING_SIZE + 2.0 * 10.0
            }
            (CupertinoListTileStyle::Notched, _, true) => {
                CupertinoListTile::NOTCHED_LEADING_SIZE + 2.0 * 12.0
            }
            // Upstream's `_kNotchedMinHeightWithoutLeading`: with no glyph to
            // clear, the row closes up a little.
            (CupertinoListTileStyle::Notched, _, false) => {
                CupertinoListTile::NOTCHED_LEADING_SIZE + 2.0 * 10.0
            }
        }
    }

    /// The gap between the leading glyph and the title.
    pub fn leading_to_title(&self) -> f32 {
        match self.style {
            CupertinoListTileStyle::Plain => CupertinoListTile::LEADING_TO_TITLE,
            CupertinoListTileStyle::Notched => CupertinoListTile::NOTCHED_LEADING_TO_TITLE,
        }
    }

    /// Upstream's `_kPadding` and friends.
    ///
    /// The plain row's `start: 20, end: 14` is **not symmetric**, and the
    /// reason is what sits at each end: the start is a text margin the eye
    /// lines up down the whole list, while the end holds a chevron that
    /// already carries its own optical space.
    pub fn padding(&self) -> EdgeInsets {
        match (self.style, self.has_subtitle, self.has_leading) {
            // Upstream's `_kPadding` and `_kPaddingWithSubtitle` are the same
            // two numbers. Kept as two arms rather than collapsed, because
            // upstream keeps them as two constants -- so a change to one does
            // not silently move the other, which is the only reason to write
            // the same value twice.
            (CupertinoListTileStyle::Plain, false, _) => EdgeInsets::only(20.0, 0.0, 14.0, 0.0),
            (CupertinoListTileStyle::Plain, true, _) => EdgeInsets::only(20.0, 0.0, 14.0, 0.0),
            (CupertinoListTileStyle::Notched, _, true) => EdgeInsets::symmetric(14.0, 0.0),
            // The only one with vertical padding of its own: with no leading
            // glyph to set the row's height, the padding has to.
            (CupertinoListTileStyle::Notched, _, false) => EdgeInsets::only(28.0, 10.0, 14.0, 10.0),
        }
    }

    pub fn subtitle_font_size(&self) -> f32 {
        match self.style {
            CupertinoListTileStyle::Plain => CupertinoListTile::SUBTITLE_FONT_SIZE,
            CupertinoListTileStyle::Notched => CupertinoListTile::NOTCHED_SUBTITLE_FONT_SIZE,
        }
    }
}

/// Upstream `CupertinoListTileChevron`: the grey angle bracket at the end of a
/// row that leads somewhere.
///
/// Its whole body is an icon whose **size is the theme's body font size** --
/// not a constant. So the chevron grows with the reader's text and keeps
/// looking like punctuation on the row rather than a fixed ornament that would
/// shrink beside large text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CupertinoListTileChevron;

impl CupertinoListTileChevron {
    /// The size the chevron is drawn at, given the body text size.
    pub fn size(body_font_size: f32) -> f32 {
        body_font_size
    }
}

/// Upstream `CupertinoListSection`: a run of rows under a header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoListSection {
    pub section_type: CupertinoListSectionType,
    pub has_header: bool,
    /// Whether the rows carry a leading widget -- an icon or a switch at the
    /// start of the row.
    ///
    /// It is not about painting the leading widget, which each row does for
    /// itself. It is about **where the divider starts**: a divider should
    /// begin under the text, not under the icon, so a section whose rows have
    /// icons pushes its dividers further in.
    pub has_leading: bool,
}

impl Default for CupertinoListSection {
    /// Hand-written, because `has_leading` defaults to **true** and `derive`
    /// would make it false.
    ///
    /// Upstream writes `bool hasLeading = true` in both constructors: rows
    /// with icons are the ordinary iOS list, and a section without them is the
    /// exception that says so.
    fn default() -> CupertinoListSection {
        CupertinoListSection {
            section_type: CupertinoListSectionType::Base,
            has_header: false,
            has_leading: true,
        }
    }
}

impl CupertinoListSection {
    /// Upstream's `_kMarginTop`.
    pub const MARGIN_TOP: f32 = 22.0;

    pub fn new() -> CupertinoListSection {
        CupertinoListSection::default()
    }

    pub fn inset_grouped() -> CupertinoListSection {
        CupertinoListSection {
            section_type: CupertinoListSectionType::InsetGrouped,
            ..CupertinoListSection::default()
        }
    }

    /// list_section.dart's `_kBaseDividerMargin`.
    pub const BASE_DIVIDER_MARGIN: f32 = 20.0;
    /// `_kBaseAdditionalDividerMargin`.
    pub const BASE_ADDITIONAL_DIVIDER_MARGIN: f32 = 44.0;
    /// `_kInsetDividerMargin`.
    pub const INSET_DIVIDER_MARGIN: f32 = 14.0;
    /// `_kInsetAdditionalDividerMargin`.
    pub const INSET_ADDITIONAL_DIVIDER_MARGIN: f32 = 42.0;
    /// `_kInsetAdditionalDividerMarginWithoutLeading`, which is **not zero**.
    pub const INSET_ADDITIONAL_DIVIDER_MARGIN_WITHOUT_LEADING: f32 = 14.0;

    pub fn with_header(mut self) -> Self {
        self.has_header = true;
        self
    }

    pub fn without_leading(mut self) -> Self {
        self.has_leading = false;
        self
    }

    /// Upstream's `dividerMargin`: how far in the divider starts before the
    /// leading widget is accounted for.
    pub fn divider_margin(&self) -> f32 {
        match self.section_type {
            CupertinoListSectionType::Base => CupertinoListSection::BASE_DIVIDER_MARGIN,
            CupertinoListSectionType::InsetGrouped => CupertinoListSection::INSET_DIVIDER_MARGIN,
        }
    }

    /// Upstream's `additionalDividerMargin`, which is **four numbers, not a
    /// switch on one**.
    ///
    /// The tempting model is "add the extra when there is a leading widget,
    /// add nothing when there is not". That is right for a base section and
    /// **wrong for an inset one**, where no leading still adds 14. A port that
    /// treated the flag as a gate would put every inset divider 14 too far
    /// out.
    ///
    /// There is no formula to recover the numbers from: upstream's comments
    /// say each was *estimated from* a different shipped app -- the base pair
    /// from Settings, inset-with-leading from Reminders, inset-without from
    /// Notes. They are measurements of what iOS does, so all four are carried
    /// rather than derived.
    pub fn additional_divider_margin(&self) -> f32 {
        match (self.section_type, self.has_leading) {
            (CupertinoListSectionType::Base, true) => {
                CupertinoListSection::BASE_ADDITIONAL_DIVIDER_MARGIN
            }
            (CupertinoListSectionType::Base, false) => 0.0,
            (CupertinoListSectionType::InsetGrouped, true) => {
                CupertinoListSection::INSET_ADDITIONAL_DIVIDER_MARGIN
            }
            (CupertinoListSectionType::InsetGrouped, false) => {
                CupertinoListSection::INSET_ADDITIONAL_DIVIDER_MARGIN_WITHOUT_LEADING
            }
        }
    }

    /// Where the divider actually begins: upstream adds the two.
    pub fn divider_start(&self) -> f32 {
        self.divider_margin() + self.additional_divider_margin()
    }

    /// Whether the section clips its rows to the rounded card.
    ///
    /// `Clip.hardEdge` for an inset-grouped section -- the corners are the
    /// point of the shape, and a row painting into them would square them off.
    /// A base section runs edge to edge with nothing to clip against.
    pub fn clips_rows(&self) -> bool {
        matches!(self.section_type, CupertinoListSectionType::InsetGrouped)
    }

    /// Upstream's assert: `children.length > 0 || header != null`.
    ///
    /// **A header with nothing under it is a legal list section** -- a group
    /// whose rows have all been filtered away still shows its title. Compare
    /// [`CupertinoFormSection::is_legal`], which requires rows.
    pub fn is_legal(rows: usize, has_header: bool) -> bool {
        rows > 0 || has_header
    }

    /// Upstream's header margins.
    ///
    /// The inset-grouped one has a **16 top inset where the base has none**,
    /// because an inset section is a card and its header floats above the
    /// card; a base section runs edge to edge and its header is part of the
    /// same run.
    pub fn header_margin(&self) -> EdgeInsets {
        match self.section_type {
            CupertinoListSectionType::Base => EdgeInsets::only(20.0, 0.0, 20.0, 6.0),
            CupertinoListSectionType::InsetGrouped => EdgeInsets::only(20.0, 16.0, 20.0, 6.0),
        }
    }

    pub fn footer_margin(&self) -> EdgeInsets {
        match self.section_type {
            CupertinoListSectionType::Base => EdgeInsets::only(20.0, 0.0, 20.0, 0.0),
            CupertinoListSectionType::InsetGrouped => EdgeInsets::only(20.0, 0.0, 20.0, 10.0),
        }
    }

    /// Upstream's rows margins, and the one that depends on the header.
    ///
    /// An inset-grouped section with a header drops its **top** margin to
    /// zero: the header's own bottom margin has already made that gap, and
    /// keeping both would open a hole between the label and the card it
    /// labels.
    pub fn rows_margin(&self) -> EdgeInsets {
        match (self.section_type, self.has_header) {
            (CupertinoListSectionType::Base, _) => EdgeInsets::only(0.0, 0.0, 0.0, 8.0),
            (CupertinoListSectionType::InsetGrouped, false) => {
                EdgeInsets::only(20.0, 20.0, 20.0, 10.0)
            }
            (CupertinoListSectionType::InsetGrouped, true) => {
                EdgeInsets::only(20.0, 0.0, 20.0, 10.0)
            }
        }
    }
}

/// Upstream `CupertinoFormRow`: one labelled field of a form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CupertinoFormRow;

impl CupertinoFormRow {
    /// Upstream's `_kDefaultPadding`, `fromSTEB(20, 6, 6, 6)`.
    ///
    /// **The start inset is more than three times the end one**, and that is
    /// the shape of a form row: the label sits against a margin the eye reads
    /// down the column, while the field itself runs out to near the edge, so
    /// there is little to reserve at that end.
    pub const PADDING: EdgeInsets = EdgeInsets::only(20.0, 6.0, 6.0, 6.0);

    /// The weight upstream gives an error label: `FontWeight.w500`.
    pub const ERROR_WEIGHT: u16 = 500;

    /// The colour of the `helper` label: the theme's text colour, **resolved**.
    ///
    /// Upstream builds the row's style as
    /// `theme.textTheme.textStyle.copyWith(color:
    /// CupertinoDynamicColor.maybeResolve(..., context))`, so the helper
    /// follows the appearance like everything else on the page.
    pub fn helper_color(brightness: Brightness) -> Color {
        CupertinoColors::LABEL.resolve(brightness, BASE)
    }

    /// The colour of the `error` label -- **one colour, in both appearances.**
    ///
    /// Three lines below the helper, upstream writes
    ///
    /// ```dart
    /// style: const TextStyle(
    ///   color: CupertinoColors.destructiveRed,
    ///   fontWeight: FontWeight.w500,
    /// ),
    /// ```
    ///
    /// and `const` is the whole story: resolving a `CupertinoDynamicColor`
    /// needs a `BuildContext` at run time, so a compile-time style can only
    /// carry the colour unresolved. An unresolved dynamic colour paints as its
    /// base value, and nothing downstream can fix that -- the widgets layer
    /// has no code dependency on cupertino at all, so `Text` and
    /// `DefaultTextStyle` see a plain `Color` and paint it.
    ///
    /// `destructiveRed` **is** `systemRed`, which is (255, 59, 48) light and
    /// (255, 69, 58) dark. So an error label in dark mode is drawn in the
    /// light red, immediately under a helper label that was carefully
    /// resolved.
    ///
    /// This is ported as upstream has it rather than quietly corrected: an
    /// inconsistency copied on purpose stays visible and stays comparable,
    /// where a local fix would make the port disagree with upstream for a
    /// reason nobody reading either would find.
    pub fn error_color() -> Color {
        CupertinoColors::DESTRUCTIVE_RED.resolve(Brightness::Light, BASE)
    }

    /// What the error label would be if it resolved the way the helper does.
    pub fn error_color_if_resolved(brightness: Brightness) -> Color {
        CupertinoColors::DESTRUCTIVE_RED.resolve(brightness, BASE)
    }

    /// Whether upstream's error colour happens to agree with the appearance.
    ///
    /// True in light -- the base value *is* the light value, so the two agree
    /// by coincidence rather than by resolution -- and false in dark, which is
    /// where the difference shows.
    pub fn error_color_agrees(brightness: Brightness) -> bool {
        CupertinoFormRow::error_color() == CupertinoFormRow::error_color_if_resolved(brightness)
    }
}

/// Upstream `CupertinoFormSection`: a run of form rows.
///
/// It is a [`CupertinoListSection`] with three things decided differently, and
/// the differences are the whole of it -- upstream's `build` styles the header
/// and footer and then hands everything to a list section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CupertinoFormSection {
    pub section_type: CupertinoListSectionType,
}

impl CupertinoFormSection {
    /// Upstream's `_kFormDefaultInsetGroupedRowsMargin`: determined, its
    /// comment says, from SwiftUI's Forms in the iOS 14.2 SDK.
    pub const INSET_GROUPED_ROWS_MARGIN: EdgeInsets = EdgeInsets::only(20.0, 0.0, 20.0, 10.0);

    /// The base constructor's `margin`, which is simply zero.
    pub const BASE_ROWS_MARGIN: EdgeInsets = EdgeInsets::ZERO;

    /// The size both the header and the footer are set in, with
    /// `CupertinoColors.secondaryLabel`.
    ///
    /// Upstream writes the same `TextStyle` out twice rather than sharing it;
    /// the two are one rule, and a form that styled its footer differently
    /// from its header would look like two sections.
    pub const HEADER_FOOTER_FONT_SIZE: f32 = 13.0;

    pub fn inset_grouped() -> CupertinoFormSection {
        CupertinoFormSection {
            section_type: CupertinoListSectionType::InsetGrouped,
        }
    }

    /// The form's rows margin -- **the same either way, header or not.**
    ///
    /// This is where a form parts company with a list section.
    /// [`CupertinoListSection::rows_margin`] picks by whether there is a
    /// header (20 at the top without one, 0 with), but a form **always passes
    /// its own margin down**, so that choice never runs. A form with no header
    /// gets a zero top where a list section would get 20.
    ///
    /// So the zero top is not "the header already made that gap" -- there may
    /// be no header. It is a number measured off SwiftUI, applied
    /// unconditionally.
    pub fn rows_margin(&self) -> EdgeInsets {
        match self.section_type {
            CupertinoListSectionType::Base => CupertinoFormSection::BASE_ROWS_MARGIN,
            CupertinoListSectionType::InsetGrouped => {
                CupertinoFormSection::INSET_GROUPED_ROWS_MARGIN
            }
        }
    }

    /// The list section a form builds: the same shape, **without leading**.
    ///
    /// `hasLeading: false` is the substantive difference. A form's rows are a
    /// label and a field; there is no icon column, so the divider starts at
    /// the plain margin instead of clearing one.
    pub fn section(&self) -> CupertinoListSection {
        CupertinoListSection {
            section_type: self.section_type,
            has_header: false,
            has_leading: false,
        }
    }

    /// Whether the form clips its rows -- **`Clip.none`, unlike a list
    /// section's `Clip.hardEdge`.**
    ///
    /// Both form constructors default to no clipping and pass it down, so an
    /// inset-grouped *form* does not clip to its rounded card where an
    /// inset-grouped *list section* does. A form row is a text field, and a
    /// text field wants its focus ring and its selection handles to be allowed
    /// out past the corner.
    pub fn clips_rows(&self) -> bool {
        false
    }

    /// Upstream's assert: `children.length > 0`.
    ///
    /// **Rows are required**, where [`CupertinoListSection::is_legal`] accepts
    /// a header with nothing under it. A list section is a group that may turn
    /// out empty; a form is a thing to fill in, and one with nothing to fill
    /// in is a mistake rather than an empty state.
    pub fn is_legal(rows: usize, _has_header: bool) -> bool {
        rows > 0
    }
}

/// Upstream `CupertinoUserInterfaceLevelData` (`cupertino/interface_level.dart`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CupertinoUserInterfaceLevelData {
    /// The page itself.
    #[default]
    Base,
    /// Something laid over it -- a sheet, a popup, an alert.
    Elevated,
}

/// Upstream `CupertinoUserInterfaceLevel`: which layer of the interface a
/// subtree is on.
///
/// It exists because **iOS's system colours are two colours, not one**. A
/// `CupertinoDynamicColor` carries an elevated variant alongside its base one,
/// and what picks between them is this -- so the same
/// `systemBackground` is one grey on the page and a lighter one on a sheet
/// laid over it, without either widget knowing which it is.
///
/// Upstream's `of` **throws** where `maybeOf` returns null, and the two are
/// kept apart here for the same reason: a widget that resolves an elevated
/// colour without a level above it is not a widget with a sensible default,
/// it is a widget outside any app.
pub struct CupertinoUserInterfaceLevel;

impl CupertinoUserInterfaceLevel {
    /// Upstream's `maybeOf`.
    pub fn maybe_of(context: &BuildContext) -> Option<CupertinoUserInterfaceLevelData> {
        context
            .inherited::<CupertinoUserInterfaceLevelData>()
            .map(|level| *level)
    }

    /// Upstream's `of`, which throws when there is none. Here that is a
    /// `panic` in debug and the base level in release -- the crate's rule for
    /// a lookup that upstream asserts on, since a released application should
    /// draw a slightly wrong grey rather than stop.
    pub fn of(context: &BuildContext) -> CupertinoUserInterfaceLevelData {
        match CupertinoUserInterfaceLevel::maybe_of(context) {
            Some(level) => level,
            None => {
                debug_assert!(
                    false,
                    "CupertinoUserInterfaceLevel.of() with no level above it"
                );
                CupertinoUserInterfaceLevelData::Base
            }
        }
    }

    /// Puts a level over `child`, upstream's constructor.
    pub fn new(data: CupertinoUserInterfaceLevelData, child: AnyWidget) -> AnyWidget {
        crate::framework::provide(data, child)
    }
}

/// Upstream `CupertinoIconThemeData` (`cupertino/icon_theme_data.dart`).
///
/// The whole of what it adds to a plain icon theme is one line of `resolve`:
/// the colour is put through [`crate::cupertino::CupertinoDynamicColor`]
/// against the ambient brightness and level. Everything else is inherited.
///
/// **And it returns `self` when the colour did not move.** That is upstream's
/// `resolvedColor == color ? this : copyWith(...)`, and it is not an
/// optimisation for its own sake: an icon theme that answered a fresh object
/// every resolve would compare unequal to the one before it and mark every
/// icon beneath it for repaint on every frame.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CupertinoIconThemeData {
    pub size: Option<f32>,
    pub color: Option<Color>,
    pub opacity: Option<f32>,
}

impl CupertinoIconThemeData {
    pub fn new() -> CupertinoIconThemeData {
        CupertinoIconThemeData::default()
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Upstream's `resolve`, and its identity rule -- see the type docs.
    pub fn resolve(&self, resolved_color: Option<Color>) -> CupertinoIconThemeData {
        if resolved_color == self.color {
            return *self;
        }
        CupertinoIconThemeData {
            color: resolved_color,
            ..*self
        }
    }
}

/// Upstream `CupertinoFocusHalo` (`cupertino/cupertino_focus_halo.dart`): the
/// ring iOS draws around whatever the keyboard is on.
///
/// **Outside the control, not on it.** The halo is a border laid around the
/// widget's own shape rather than painted into it, so a focused button is the
/// same size and shape as an unfocused one -- a ring that grew inwards would
/// make the control's contents shift the moment it took focus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoFocusHalo {
    pub border_radius: f32,
}

impl CupertinoFocusHalo {
    /// Upstream's stroke width. Wide for a focus ring -- and deliberately so,
    /// since the one reader who needs it is navigating without a pointer and
    /// has nothing else telling them where they are.
    pub const STROKE_WIDTH: f32 = 3.5;

    pub fn new(border_radius: f32) -> CupertinoFocusHalo {
        CupertinoFocusHalo { border_radius }
    }

    /// The rectangle the halo occupies around a control of `size`, which is
    /// larger than the control by the stroke on every side.
    pub fn bounds(&self, size: Size) -> Rect {
        let stroke = CupertinoFocusHalo::STROKE_WIDTH;
        Rect::ltrb(-stroke, -stroke, size.width + stroke, size.height + stroke)
    }
}

/// Upstream `CupertinoPickerDefaultSelectionOverlay` (`cupertino/picker.dart`):
/// the tinted band across the middle of a picker showing which row is chosen.
///
/// # The two caps, and why they are separate
///
/// A date picker is several pickers side by side, and the band has to read as
/// **one** band across the whole row. So a column in the middle caps neither
/// edge, the first column caps only its start, and the last only its end --
/// which is what makes three separate overlays look like one. Capping both on
/// every column would draw three pills with gaps between them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoPickerDefaultSelectionOverlay {
    pub cap_start_edge: bool,
    pub cap_end_edge: bool,
}

impl CupertinoPickerDefaultSelectionOverlay {
    /// Upstream's `_defaultSelectionOverlayHorizontalMargin`.
    pub const HORIZONTAL_MARGIN: f32 = 9.0;
    /// Upstream's `_defaultSelectionOverlayRadius`.
    pub const RADIUS: f32 = 8.0;

    pub fn new() -> CupertinoPickerDefaultSelectionOverlay {
        CupertinoPickerDefaultSelectionOverlay {
            cap_start_edge: true,
            cap_end_edge: true,
        }
    }

    /// The overlay for a column at `index` of `columns`, capped on the outside
    /// edges of the row only.
    pub fn for_column(index: usize, columns: usize) -> CupertinoPickerDefaultSelectionOverlay {
        CupertinoPickerDefaultSelectionOverlay {
            cap_start_edge: index == 0,
            cap_end_edge: index + 1 == columns,
        }
    }

    /// **The margin goes only where a cap does.** An uncapped edge takes no
    /// margin at all, because that is where this column's band meets the next
    /// one's -- a margin there would be exactly the gap the caps are arranged
    /// to avoid.
    pub fn margin(&self) -> EdgeInsets {
        EdgeInsets::only(
            if self.cap_start_edge {
                CupertinoPickerDefaultSelectionOverlay::HORIZONTAL_MARGIN
            } else {
                0.0
            },
            0.0,
            if self.cap_end_edge {
                CupertinoPickerDefaultSelectionOverlay::HORIZONTAL_MARGIN
            } else {
                0.0
            },
            0.0,
        )
    }

    /// The corner radii, in the same start/end order.
    pub fn radii(&self) -> (f32, f32) {
        (
            if self.cap_start_edge {
                CupertinoPickerDefaultSelectionOverlay::RADIUS
            } else {
                0.0
            },
            if self.cap_end_edge {
                CupertinoPickerDefaultSelectionOverlay::RADIUS
            } else {
                0.0
            },
        )
    }
}

impl Default for CupertinoPickerDefaultSelectionOverlay {
    fn default() -> CupertinoPickerDefaultSelectionOverlay {
        CupertinoPickerDefaultSelectionOverlay::new()
    }
}

/// Upstream `CupertinoLinearActivityIndicator` (`cupertino/activity_indicator.dart`):
/// the thin bar that fills as something finishes.
///
/// The sibling of the spinner in the same file, and the difference is what
/// each *knows*: a spinner turns because the wait has no measurable end, while
/// this takes a `progress` because it does. Showing a bar for an unmeasurable
/// wait would promise the reader an ending the application cannot deliver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoLinearActivityIndicator {
    pub progress: f32,
    pub height: f32,
}

impl CupertinoLinearActivityIndicator {
    /// Upstream's default `height`.
    pub const DEFAULT_HEIGHT: f32 = 4.5;

    /// Upstream asserts both: a height above zero, and a progress inside
    /// 0..1. The progress assert is the one that matters -- a bar told it is
    /// 1.4 done would draw past its own end, and the caller who computed that
    /// has a bug the assert is the fastest way to find.
    pub fn new(progress: f32) -> CupertinoLinearActivityIndicator {
        debug_assert!(
            (0.0..=1.0).contains(&progress),
            "progress is a fraction of the whole"
        );
        CupertinoLinearActivityIndicator {
            progress,
            height: CupertinoLinearActivityIndicator::DEFAULT_HEIGHT,
        }
    }

    pub fn with_height(mut self, height: f32) -> Self {
        debug_assert!(height > 0.0, "a bar with no height is not a bar");
        self.height = height;
        self
    }

    /// How wide the filled part is, in a bar of `width`.
    pub fn filled_width(&self, width: f32) -> f32 {
        width * self.progress.clamp(0.0, 1.0)
    }
}

/// Upstream `ObstructingPreferredSizeWidget` (`cupertino/page_scaffold.dart`):
/// a bar that knows its own height *and* whether the page can be seen through
/// it.
///
/// # Why the second question is separate
///
/// A `CupertinoPageScaffold` has to decide one thing about its body: does it
/// start **below** the bar, or **under** it? Both are right, and which is
/// right depends on something only the bar knows -- whether it is opaque.
///
/// An opaque bar hides whatever passes behind it, so the body has to start
/// below it or its first line would be permanently invisible. A translucent
/// one is meant to be scrolled under: the blur of moving content behind the
/// bar is the effect, and a body that started below it would leave a blank
/// strip where that effect should be.
///
/// So a preferred size alone is not enough, and this is the extra question.
pub trait ObstructingPreferredSizeWidget {
    /// Upstream's `preferredSize`.
    fn preferred_size(&self) -> Size;

    /// Upstream's `shouldFullyObstruct`.
    fn should_fully_obstruct(&self) -> bool;
}

/// Upstream `CupertinoNavigationBar`'s answer, as the rule on its own:
/// **a bar obstructs exactly when its background is fully opaque.**
///
/// One alpha check decides the whole page's layout, which is why it is worth
/// having by itself: a caller who tints a bar with any transparency at all has
/// asked, without saying so, for the content to scroll under it.
pub fn bar_fully_obstructs(background: Color) -> bool {
    background.alpha() == 0xFF
}

/// Upstream `CupertinoPageScaffoldBackgroundColor`: the scaffold's own colour,
/// put where its descendants can read it.
///
/// It exists because a **child sometimes has to paint the page's colour
/// itself** -- a bar's blur needs to know what it is blurring, and a row that
/// wants to look like a hole in the page has to fill with what the page is
/// filled with. Asking the theme would give the *theme's* background, which is
/// not the same thing once a scaffold has been given a colour of its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoPageScaffoldBackgroundColor {
    pub color: Color,
}

impl CupertinoPageScaffoldBackgroundColor {
    pub fn new(color: Color) -> CupertinoPageScaffoldBackgroundColor {
        CupertinoPageScaffoldBackgroundColor { color }
    }

    /// Upstream's `maybeOf`. There is no `of`: a widget outside any scaffold
    /// has a perfectly good answer, which is "ask the theme".
    pub fn maybe_of(context: &BuildContext) -> Option<Color> {
        context
            .inherited::<CupertinoPageScaffoldBackgroundColor>()
            .map(|scaffold| scaffold.color)
    }

    pub fn provide(color: Color, child: AnyWidget) -> AnyWidget {
        crate::framework::provide(CupertinoPageScaffoldBackgroundColor::new(color), child)
    }
}

/// Upstream `CupertinoNavigationBarBackButton`: the chevron and the previous
/// page's title.
///
/// # Two pieces, not one
///
/// Upstream keeps a private `_assemble` constructor so the chevron and the
/// label can be **created and keyed separately**, with the comment saying why:
/// they animate separately during a page transition. The chevron slides
/// straight across while the label fades and slides at its own rate, because
/// the outgoing page's title has to become the incoming page's back label --
/// two things doing one job, which one widget could not.
pub struct CupertinoNavigationBarBackButton {
    pub color: Option<Color>,
    /// Upstream's `previousPageTitle`.
    ///
    /// **This doc used to say the opposite** -- that an unset title shows the
    /// generic word, "because a bare chevron says which way but not to what".
    /// Upstream does the reverse: an unset title shows *nothing* beside the
    /// chevron, and it is a title **too long** that gets replaced by the word.
    /// See [`CupertinoNavigationBarBackButton::label_for`], which is the rule
    /// written out, and the sentence was wrong rather than merely stale --
    /// nothing in the crate had ever checked it.
    pub previous_page_title: Option<String>,
    #[allow(clippy::type_complexity)]
    pub on_pressed: Option<std::rc::Rc<dyn Fn()>>,
    /// Whether this was built by upstream's `_assemble` -- the two-piece form
    /// used mid-transition, which carries no colour or callback of its own
    /// because the bar it is flying between owns both.
    pub assembled: bool,
}

impl CupertinoNavigationBarBackButton {
    pub fn new() -> CupertinoNavigationBarBackButton {
        CupertinoNavigationBarBackButton {
            color: None,
            previous_page_title: None,
            on_pressed: None,
            assembled: false,
        }
    }

    /// Upstream's `_assemble`: the chevron and label as separately keyed
    /// pieces, for a transition to animate apart.
    pub fn assembled() -> CupertinoNavigationBarBackButton {
        CupertinoNavigationBarBackButton {
            color: None,
            previous_page_title: None,
            on_pressed: None,
            assembled: true,
        }
    }

    pub fn with_previous_page_title(mut self, title: impl Into<String>) -> Self {
        self.previous_page_title = Some(title.into());
        self
    }

    pub fn with_on_pressed(mut self, on_pressed: impl Fn() + 'static) -> Self {
        self.on_pressed = Some(std::rc::Rc::new(on_pressed));
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Upstream's bare `12` in `_BackLabel._buildPreviousTitleWidget`.
    ///
    /// Not a width and not an ellipsis threshold: a **count of UTF-16 code
    /// units**, which is what Dart's `String.length` is. `"Ärger"` counts 5
    /// here and in Dart; an emoji outside the basic plane counts 2 in both.
    /// Counting bytes would cut a Cyrillic title in half its true length, and
    /// counting `char`s would let a title of thirteen astral glyphs through
    /// where Dart sees twenty-six.
    pub const MAX_PREVIOUS_TITLE_UNITS: usize = 12;

    /// What sits beside the chevron.
    ///
    /// ```dart
    /// if (previousTitle == null) {
    ///   return const SizedBox.shrink();
    /// }
    /// var textWidget = Text(previousTitle, maxLines: 1, overflow: TextOverflow.ellipsis);
    /// if (previousTitle.length > 12) {
    ///   textWidget = Text(CupertinoLocalizations.of(context).backButtonLabel);
    /// }
    /// ```
    ///
    /// **A long title is replaced, not ellipsized.** The `Text` upstream
    /// builds first -- with `maxLines: 1` and an ellipsis -- is thrown away
    /// whenever the title runs past twelve, so the ellipsis is only ever seen
    /// on a *short* title in a bar too narrow for it. A port that kept the
    /// ellipsis and dropped the length test would show "Notification Set…"
    /// where iOS shows "Back", which is worse than it looks: the point of the
    /// back label is to name where you are going, and half a name is not a
    /// name.
    ///
    /// The `null` case is genuinely nothing -- `SizedBox.shrink()`, not the
    /// generic word. So the three outcomes are: silence, the title, the word;
    /// and which one you get has nothing to do with which is the "default".
    pub fn label_for(previous_title: Option<&str>) -> BackLabel {
        let Some(title) = previous_title else {
            return BackLabel::Nothing;
        };
        if title.encode_utf16().count() > CupertinoNavigationBarBackButton::MAX_PREVIOUS_TITLE_UNITS
        {
            return BackLabel::Generic;
        }
        BackLabel::PreviousTitle(title.to_string())
    }

    /// What a screen reader is told, which is **not** what the button shows.
    ///
    /// ```dart
    /// Semantics(
    ///   container: true,
    ///   excludeSemantics: true,
    ///   label: localizations.backButtonLabel,
    ///   button: true,
    ///   child: ...,
    /// )
    /// ```
    ///
    /// `excludeSemantics: true` throws away everything underneath -- the
    /// chevron and whatever the label turned out to be -- and puts one word in
    /// its place. So a reader on a page whose back button reads "Settings"
    /// hears "Back", always, and never hears the page title twice: the title
    /// is already announced by the page it belongs to.
    pub fn semantics_label() -> &'static str {
        crate::cupertino_app::DefaultCupertinoLocalizations::BACK_BUTTON_LABEL
    }
}

/// What [`CupertinoNavigationBarBackButton::label_for`] decided to put beside
/// the chevron.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackLabel {
    /// `SizedBox.shrink()`: no previous title to name.
    Nothing,
    /// The previous page's title, short enough to fit in a bar.
    PreviousTitle(String),
    /// The localized word, because the title was too long to be one.
    Generic,
}

impl BackLabel {
    /// The text actually drawn, or `None` for the empty case.
    pub fn text(&self) -> Option<&str> {
        match self {
            BackLabel::Nothing => None,
            BackLabel::PreviousTitle(title) => Some(title),
            BackLabel::Generic => {
                Some(crate::cupertino_app::DefaultCupertinoLocalizations::BACK_BUTTON_LABEL)
            }
        }
    }
}

impl Default for CupertinoNavigationBarBackButton {
    fn default() -> CupertinoNavigationBarBackButton {
        CupertinoNavigationBarBackButton::new()
    }
}

/// Upstream `CupertinoCheckbox`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoCheckbox {
    /// Upstream's `value`. **Three states, not two**: `None` is the
    /// *indeterminate* one, which a checkbox standing for a group of others
    /// needs when some of them are ticked and some are not. A `bool` could not
    /// say that, and rounding it to false would tell the reader the group is
    /// empty when it is half full.
    pub value: Option<bool>,
    /// Upstream's `tristate`. Without it, `None` is not a value the reader can
    /// reach -- only one the application may set. So a checkbox showing a
    /// group is tristate and a plain one is not, and cycling stops at two.
    pub tristate: bool,
}

impl CupertinoCheckbox {
    /// Upstream's `CupertinoCheckbox.width`.
    pub const WIDTH: f32 = 14.0;
    /// Upstream's `_kPressedOverlayOpacity`: how dark the box goes while held.
    pub const PRESSED_OVERLAY_OPACITY: f32 = 0.15;

    pub fn new(value: bool) -> CupertinoCheckbox {
        CupertinoCheckbox {
            value: Some(value),
            tristate: false,
        }
    }

    /// A checkbox that can also be indeterminate.
    pub fn tristate(value: Option<bool>) -> CupertinoCheckbox {
        CupertinoCheckbox {
            value,
            tristate: true,
        }
    }

    /// Upstream's constructor assert, `tristate || value != null`: a plain
    /// checkbox has no indeterminate state to be in, so a null there is a
    /// caller who meant `tristate` and forgot to say so.
    pub fn debug_assert_valid(&self) {
        debug_assert!(
            self.tristate || self.value.is_some(),
            "a checkbox that is not tristate must be true or false"
        );
    }

    /// What a press moves the value to.
    ///
    /// Upstream's cycle for a tristate box is **false → true → null**, and the
    /// order is the point: `null` sits after `true` rather than between the
    /// two, so a reader tapping repeatedly passes through both definite
    /// answers before reaching the one that means "leave it as it was".
    pub fn next_value(&self) -> Option<bool> {
        match (self.tristate, self.value) {
            (false, _) => Some(!self.value.unwrap_or(false)),
            (true, Some(false)) => Some(true),
            (true, Some(true)) => None,
            (true, None) => Some(false),
        }
    }

    /// Upstream's pressed overlay, which is **black over a light box and
    /// white over a dark one** -- the same 15%, inverted, so a press reads as
    /// a press against either.
    pub fn pressed_overlay(is_dark: bool) -> Color {
        let alpha = (255.0 * CupertinoCheckbox::PRESSED_OVERLAY_OPACITY).round() as u8;
        if is_dark {
            Color::WHITE.with_alpha(alpha)
        } else {
            Color::BLACK.with_alpha(alpha)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{ElementTree, component, provide};
    use crate::list_wheel::max_visible_radian;
    use crate::presence::Orientation;
    use crate::render::HitTestResult;

    fn lay_out(widget: AnyWidget, width: f32, height: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(CupertinoTheme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(width, height))
    }

    #[test]
    fn dynamic_colors_resolve_against_the_appearance() {
        let label = CupertinoColors::LABEL;
        assert_eq!(label.resolve(Brightness::Light, BASE), Color(0xFF00_0000));
        assert_eq!(label.resolve(Brightness::Dark, BASE), Color(0xFFFF_FFFF));
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
        let mut wheel = RenderListWheelViewport {
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
            perspective: PICKER_PERSPECTIVE,
            viewport_sink: Rc::new(Cell::new(0.0)),
            ..Default::default()
        };
        let size = wheel.layout(BoxConstraints::tight_for(Size::new(300.0, 216.0)));
        assert_eq!(size, Size::new(300.0, 216.0));
        // The layout published the viewport height for the next build.
        assert_eq!(wheel.viewport_sink.get(), 216.0);
        // The child is centered horizontally, and -- at offset 0, which is
        // the offset that selects item 0 -- vertically too: the wheel is
        // anchored at its middle, so the selected row sits under the band
        // rather than at the top edge.
        let mut result = HitTestResult::new();
        assert!(wheel.hit_test_children(Offset::new(150.0, 108.0), &mut result));
        assert!(!result.path.is_empty());
        // Away from the one child there is nothing to hit.
        let mut miss = HitTestResult::new();
        assert!(!wheel.hit_test_children(Offset::new(150.0, 16.0), &mut miss));
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
    fn a_closed_context_menu_is_its_child() {
        // Upstream's "when closed, the CupertinoContextMenu displays the child
        // as if the CupertinoContextMenu were not there".
        let trigger = CupertinoContextMenu::new(1)
            .with_child(|| leaf(|| Container::new().with_size(60.0, 60.0)));
        let size = lay_out(stateful(trigger), 300.0, 300.0);
        assert_eq!(size, Size::new(60.0, 60.0));
    }

    #[test]
    fn the_menu_goes_to_whichever_side_of_the_screen_the_child_is_on() {
        // Upstream's `_contextMenuLocation`: the halves, and the tolerance
        // that keeps a child straddling the middle centred.
        let left = Rect::xywh(20.0, 100.0, 100.0, 100.0);
        let right = Rect::xywh(680.0, 100.0, 100.0, 100.0);
        let middle = Rect::xywh(350.0, 100.0, 100.0, 100.0);
        assert_eq!(
            context_menu_location(left, 800.0),
            ContextMenuLocation::Left
        );
        assert_eq!(
            context_menu_location(right, 800.0),
            ContextMenuLocation::Right
        );
        assert_eq!(
            context_menu_location(middle, 800.0),
            ContextMenuLocation::Center
        );
        // Straddling the middle is not enough on its own: upstream also wants
        // the centre within a quarter of the child's width.
        let barely = Rect::xywh(320.0, 100.0, 100.0, 100.0);
        assert_eq!(
            context_menu_location(barely, 800.0),
            ContextMenuLocation::Left
        );
    }

    #[test]
    fn the_press_grows_the_child_by_the_open_scale_where_there_is_room() {
        let roomy = Rect::xywh(350.0, 250.0, 100.0, 100.0);
        let scale = context_menu_scale_factor(roomy, EdgeInsets::ZERO, Size::new(800.0, 600.0));
        assert!((scale - CONTEXT_MENU_OPEN_SCALE).abs() < 1e-6, "{scale}");

        // Hard against an edge, growth is clamped rather than pushing the
        // child off the screen: upstream's `_kMinScaleFactor` floor.
        let cornered = Rect::xywh(0.0, 0.0, 100.0, 100.0);
        let scale = context_menu_scale_factor(cornered, EdgeInsets::ZERO, Size::new(800.0, 600.0));
        assert!(
            (scale - CONTEXT_MENU_MIN_SCALE_FACTOR).abs() < 1e-6,
            "{scale}"
        );
    }

    #[test]
    fn the_landscape_sheet_goes_on_the_far_side_of_the_preview() {
        // Upstream's `menuBeforeChild`: a child on the right half opens with
        // the sheet to its left, so the pair stays in the middle of the screen
        // rather than running off the edge.
        let screen = Rect::xywh(0.0, 0.0, 900.0, 600.0);
        let child = Rect::xywh(650.0, 250.0, 100.0, 100.0);
        let (preview, sheet) = lay_out_context_menu(child, screen, Orientation::Landscape);
        assert!(
            sheet.right <= preview.left,
            "sheet {sheet:?} should be left of preview {preview:?}"
        );
        assert!(
            (sheet.right - preview.left + CONTEXT_MENU_PADDING).abs() < 1.0,
            "the gap should be _kPadding: {sheet:?} {preview:?}"
        );

        // And the other way round for a child on the left.
        let child = Rect::xywh(150.0, 250.0, 100.0, 100.0);
        let (preview, sheet) = lay_out_context_menu(child, screen, Orientation::Landscape);
        assert!(
            preview.right <= sheet.left,
            "preview {preview:?} should be left of sheet {sheet:?}"
        );
    }

    #[test]
    fn the_portrait_sheet_goes_under_the_preview() {
        let screen = Rect::xywh(0.0, 0.0, 400.0, 800.0);
        let child = Rect::xywh(150.0, 300.0, 100.0, 100.0);
        let (preview, sheet) = lay_out_context_menu(child, screen, Orientation::Portrait);
        assert!(
            sheet.top >= preview.bottom,
            "sheet {sheet:?} should be under preview {preview:?}"
        );
        assert!(
            (sheet.top - preview.bottom - CONTEXT_MENU_PADDING).abs() < 1.0,
            "the gap should be _kPadding: {preview:?} {sheet:?}"
        );
    }

    /// Runs [`ContextMenuLayout`] over a fixed preview and sheet and answers
    /// where the two ended up.
    fn lay_out_context_menu(child: Rect, screen: Rect, orientation: Orientation) -> (Rect, Rect) {
        use crate::render::{MultiChildLayoutDelegate, RenderCustomMultiChildLayoutBox};
        let delegate = ContextMenuLayout {
            target_rect: child,
            screen_bounds: screen,
            orientation,
            location: context_menu_location(child, screen.width()),
            from: child,
            t: 1.0,
        };
        let mut box_ = RenderCustomMultiChildLayoutBox::new(
            Rc::new(delegate) as Rc<dyn MultiChildLayoutDelegate>,
            vec![
                (
                    CONTEXT_MENU_PREVIEW_SLOT,
                    RenderRef::new(RenderConstrainedBox::tight(200.0, 200.0)),
                ),
                (
                    CONTEXT_MENU_SHEET_SLOT,
                    RenderRef::new(RenderConstrainedBox::tight(CONTEXT_MENU_SHEET_WIDTH, 86.0)),
                ),
            ],
        );
        box_.layout(BoxConstraints::tight(screen.width(), screen.height()));
        let mut placed: Vec<Rect> = Vec::new();
        box_.visit_children(&mut |child, offset| {
            let size = child.size();
            placed.push(Rect::xywh(offset.dx, offset.dy, size.width, size.height));
        });
        (placed[0], placed[1])
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

    #[test]
    fn a_rows_height_is_its_leading_glyph_plus_breathing_room() {
        // Upstream writes each minimum as `leadingSize + 2 * padding` rather
        // than as a number, and the arithmetic is the statement: the glyph's
        // size sets the row's, and the padding is what is left to choose.
        assert_eq!(CupertinoListTile::new().min_height(), 28.0 + 16.0);
        assert_eq!(
            CupertinoListTile::notched().with_leading().min_height(),
            30.0 + 24.0
        );
    }

    #[test]
    fn a_subtitle_makes_the_padding_grow_not_only_the_line_count() {
        // Two lines packed to the same margins as one would read as crowded
        // even though nothing overlaps, so the row is taller by more than the
        // extra line.
        let one_line = CupertinoListTile::new();
        let two_lines = CupertinoListTile::new().with_subtitle();
        assert_eq!(one_line.min_height(), 44.0);
        assert_eq!(two_lines.min_height(), 48.0);
        assert!(two_lines.min_height() > one_line.min_height());
    }

    #[test]
    fn a_notched_row_without_a_leading_glyph_closes_up_and_pads_itself() {
        // With no glyph to set the row's height, the padding has to -- which
        // is why it is the one variant whose padding has a vertical part.
        let with_glyph = CupertinoListTile::notched().with_leading();
        let without = CupertinoListTile::notched();
        assert!(without.min_height() < with_glyph.min_height());

        assert_eq!(with_glyph.padding().top, 0.0);
        assert_eq!(without.padding().top, 10.0);
        assert_eq!(without.padding().bottom, 10.0);
    }

    #[test]
    fn the_gap_before_the_title_shrinks_as_the_glyph_grows() {
        // The gap and the glyph together are what the eye reads as the indent,
        // so the one gives way as the other grows.
        let plain = CupertinoListTile::new();
        let notched = CupertinoListTile::notched();
        assert!(notched.leading_size() > plain.leading_size());
        assert!(notched.leading_to_title() < plain.leading_to_title());
    }

    #[test]
    fn a_rows_two_ends_are_not_padded_alike() {
        // The start is a text margin the eye lines up down the whole list; the
        // end holds a chevron that already carries its own optical space.
        let padding = CupertinoListTile::new().padding();
        assert_eq!(padding.left, 20.0);
        assert_eq!(padding.right, 14.0);
        assert!(padding.left > padding.right);
    }

    #[test]
    fn the_chevron_is_sized_by_the_readers_text_not_by_a_constant() {
        // So it keeps looking like punctuation on the row rather than a fixed
        // ornament that would shrink beside large text.
        assert_eq!(CupertinoListTileChevron::size(17.0), 17.0);
        assert_eq!(CupertinoListTileChevron::size(34.0), 34.0);
    }

    #[test]
    fn an_inset_sections_header_floats_above_its_card() {
        // A base section runs edge to edge and its header is part of the same
        // run, so it has no top inset; an inset one is a card, and its header
        // sits above the card.
        assert_eq!(CupertinoListSection::new().header_margin().top, 0.0);
        assert_eq!(
            CupertinoListSection::inset_grouped().header_margin().top,
            16.0
        );
    }

    #[test]
    fn a_header_takes_over_the_gap_above_the_rows() {
        // Keeping both the header's bottom margin and the rows' top margin
        // would open a hole between the label and the card it labels.
        let without = CupertinoListSection::inset_grouped();
        let with_header = CupertinoListSection::inset_grouped().with_header();
        assert_eq!(without.rows_margin().top, 20.0);
        assert_eq!(with_header.rows_margin().top, 0.0);
        // The other three sides are unchanged -- only the disputed one moves.
        assert_eq!(without.rows_margin().left, with_header.rows_margin().left);
        assert_eq!(
            without.rows_margin().bottom,
            with_header.rows_margin().bottom
        );
    }

    #[test]
    fn a_base_section_has_no_side_margins_and_an_inset_one_does() {
        // Which is the whole difference between the two shapes: edge to edge
        // against a card held in from the sides.
        assert_eq!(CupertinoListSection::new().rows_margin().left, 0.0);
        assert_eq!(
            CupertinoListSection::inset_grouped().rows_margin().left,
            20.0
        );
    }

    #[test]
    fn a_form_rows_label_margin_is_far_wider_than_its_field_one() {
        // The label sits against a margin the eye reads down the column; the
        // field runs out to near the edge, so there is little to reserve there.
        let padding = CupertinoFormRow::PADDING;
        assert_eq!(padding.left, 20.0);
        assert_eq!(padding.right, 6.0);
        assert!(padding.left > padding.right * 3.0);
    }

    #[test]
    fn a_form_section_starts_flush_where_a_list_section_does_not() {
        // A form is always the inset-grouped shape and always has something
        // above it, so its rows need no top margin of their own.
        assert_eq!(CupertinoFormSection::INSET_GROUPED_ROWS_MARGIN.top, 0.0);
        assert_eq!(
            CupertinoListSection::inset_grouped().rows_margin().top,
            20.0
        );
    }

    #[test]
    fn a_middle_picker_column_caps_neither_edge() {
        // A date picker is several pickers side by side and the band has to
        // read as *one* band. Capping every column would draw three pills with
        // gaps between them.
        let first = CupertinoPickerDefaultSelectionOverlay::for_column(0, 3);
        assert!(first.cap_start_edge && !first.cap_end_edge);

        let middle = CupertinoPickerDefaultSelectionOverlay::for_column(1, 3);
        assert!(!middle.cap_start_edge && !middle.cap_end_edge);

        let last = CupertinoPickerDefaultSelectionOverlay::for_column(2, 3);
        assert!(!last.cap_start_edge && last.cap_end_edge);

        // A lone column caps both, being the whole band by itself.
        let only = CupertinoPickerDefaultSelectionOverlay::for_column(0, 1);
        assert!(only.cap_start_edge && only.cap_end_edge);
    }

    #[test]
    fn the_margin_goes_only_where_a_cap_does() {
        // An uncapped edge is where this column's band meets the next one's,
        // and a margin there would be exactly the gap the caps are arranged to
        // avoid.
        let middle = CupertinoPickerDefaultSelectionOverlay::for_column(1, 3);
        assert_eq!(middle.margin().left, 0.0);
        assert_eq!(middle.margin().right, 0.0);
        assert_eq!(middle.radii(), (0.0, 0.0));

        let first = CupertinoPickerDefaultSelectionOverlay::for_column(0, 3);
        assert_eq!(first.margin().left, 9.0);
        assert_eq!(first.margin().right, 0.0, "the inner edge stays flush");
        assert_eq!(first.radii(), (8.0, 0.0));
    }

    #[test]
    fn an_icon_theme_that_did_not_move_answers_itself() {
        // Upstream's `resolvedColor == color ? this : copyWith(...)`. Not an
        // optimisation for its own sake: a theme that answered a fresh object
        // every resolve would compare unequal to the one before and mark every
        // icon beneath it for repaint on every frame.
        let theme = CupertinoIconThemeData::new().with_color(Color::WHITE);
        assert_eq!(theme.resolve(Some(Color::WHITE)), theme);

        let moved = theme.resolve(Some(Color::BLACK));
        assert_ne!(moved, theme);
        assert_eq!(moved.color, Some(Color::BLACK));
        // And nothing else moved with it.
        assert_eq!(moved.size, theme.size);
    }

    #[test]
    fn a_focus_halo_grows_outwards_so_the_control_does_not_move() {
        // A ring that grew inwards would shift the control's contents the
        // moment it took focus.
        let halo = CupertinoFocusHalo::new(8.0);
        let bounds = halo.bounds(Size::new(100.0, 40.0));
        assert!(bounds.left < 0.0 && bounds.top < 0.0);
        assert_eq!(
            bounds.width(),
            100.0 + CupertinoFocusHalo::STROKE_WIDTH * 2.0
        );
        assert_eq!(
            bounds.height(),
            40.0 + CupertinoFocusHalo::STROKE_WIDTH * 2.0
        );
    }

    #[test]
    fn the_two_interface_levels_are_distinct_and_base_is_the_default() {
        // iOS's system colours are two colours, and this is what picks between
        // them -- so the same background is one grey on the page and a lighter
        // one on a sheet over it.
        assert_ne!(
            CupertinoUserInterfaceLevelData::Base,
            CupertinoUserInterfaceLevelData::Elevated
        );
        assert_eq!(
            CupertinoUserInterfaceLevelData::default(),
            CupertinoUserInterfaceLevelData::Base
        );
    }

    #[test]
    fn a_linear_indicator_fills_in_proportion_and_no_further() {
        // A bar promises an ending, which is the whole difference from the
        // spinner in the same file: one is for a wait with a measurable end
        // and the other for a wait without one.
        assert_eq!(
            CupertinoLinearActivityIndicator::new(0.0).filled_width(200.0),
            0.0
        );
        assert_eq!(
            CupertinoLinearActivityIndicator::new(0.25).filled_width(200.0),
            50.0
        );
        assert_eq!(
            CupertinoLinearActivityIndicator::new(1.0).filled_width(200.0),
            200.0
        );
    }

    #[test]
    fn a_linear_indicator_keeps_its_default_height_unless_told() {
        assert_eq!(
            CupertinoLinearActivityIndicator::new(0.5).height,
            CupertinoLinearActivityIndicator::DEFAULT_HEIGHT
        );
        assert_eq!(
            CupertinoLinearActivityIndicator::new(0.5)
                .with_height(10.0)
                .height,
            10.0
        );
    }

    #[test]
    fn a_bar_obstructs_exactly_when_it_is_fully_opaque() {
        // One alpha check decides the whole page's layout: an opaque bar hides
        // what passes behind it, so the body must start below it; a
        // translucent one is meant to be scrolled under, and a body that
        // started below would leave a blank strip where the blur should be.
        assert!(bar_fully_obstructs(Color::argb(0xFF, 0xF9, 0xF9, 0xF9)));
        assert!(!bar_fully_obstructs(Color::argb(0xF0, 0xF9, 0xF9, 0xF9)));
        assert!(
            !bar_fully_obstructs(Color::argb(0xFE, 0, 0, 0)),
            "one step short of opaque is still see-through"
        );
    }

    #[test]
    fn a_tristate_checkbox_cycles_through_both_definite_answers_first() {
        // Upstream's false -> true -> null, and the order is the point: null
        // sits *after* true rather than between the two, so a reader tapping
        // repeatedly passes through both definite answers before reaching the
        // one that means "leave it as it was".
        let start = CupertinoCheckbox::tristate(Some(false));
        assert_eq!(start.next_value(), Some(true));
        assert_eq!(CupertinoCheckbox::tristate(Some(true)).next_value(), None);
        assert_eq!(CupertinoCheckbox::tristate(None).next_value(), Some(false));
    }

    #[test]
    fn a_plain_checkbox_never_reaches_the_indeterminate_state() {
        // Without `tristate`, null is not a value the reader can reach -- only
        // one the application may set. So the cycle stops at two.
        assert_eq!(CupertinoCheckbox::new(false).next_value(), Some(true));
        assert_eq!(CupertinoCheckbox::new(true).next_value(), Some(false));
        // And upstream asserts a plain one is never null to begin with.
        CupertinoCheckbox::new(true).debug_assert_valid();
        CupertinoCheckbox::tristate(None).debug_assert_valid();
    }

    #[test]
    fn the_pressed_overlay_inverts_with_the_brightness() {
        // The same fifteen percent, black over a light box and white over a
        // dark one, so a press reads as a press against either.
        let light = CupertinoCheckbox::pressed_overlay(false);
        let dark = CupertinoCheckbox::pressed_overlay(true);
        assert_eq!(light.alpha(), dark.alpha(), "the same strength");
        assert_ne!(light, dark, "and the opposite colour");
        assert_eq!(light.alpha(), 38, "fifteen percent of 255");
    }

    #[test]
    fn an_assembled_back_button_carries_no_colour_or_callback_of_its_own() {
        // The two-piece form upstream uses mid-transition: the bar it is
        // flying between owns both, and the pieces exist only so the chevron
        // and the label can animate apart.
        let assembled = CupertinoNavigationBarBackButton::assembled();
        assert!(assembled.assembled);
        assert!(assembled.color.is_none());
        assert!(assembled.on_pressed.is_none());
        assert!(assembled.previous_page_title.is_none());

        let ordinary = CupertinoNavigationBarBackButton::new()
            .with_previous_page_title("Inbox")
            .with_on_pressed(|| {});
        assert!(!ordinary.assembled);
        assert_eq!(ordinary.previous_page_title.as_deref(), Some("Inbox"));
    }
}

#[cfg(test)]
mod dynamic_color_elevation_tests {
    use super::*;

    const BASE_LEVEL: CupertinoUserInterfaceLevelData = CupertinoUserInterfaceLevelData::Base;
    const UP: CupertinoUserInterfaceLevelData = CupertinoUserInterfaceLevelData::Elevated;

    #[test]
    fn elevating_a_background_gives_the_role_below_it() {
        // iOS's two ways of saying "this is layered over that" -- the numbered
        // role and the elevation trait -- arrive at the same grey.
        assert!(CupertinoColors::elevating_is_one_step_down(
            &CupertinoColors::BACKGROUND_LADDER
        ));
        // And the predicate can say no, which a version reading the constant
        // itself could never be shown to do.
        // The broken pair goes second on purpose: with it first, a predicate
        // that checked only its first pair would still answer no, and the
        // test would pass while proving nothing about the rest.
        let broken = [
            (
                CupertinoColors::SYSTEM_BACKGROUND,
                CupertinoColors::SECONDARY_SYSTEM_BACKGROUND,
            ),
            (
                CupertinoColors::SYSTEM_BACKGROUND,
                CupertinoColors::TERTIARY_SYSTEM_BACKGROUND,
            ),
        ];
        assert!(!CupertinoColors::elevating_is_one_step_down(&broken));
        // `order_sweep`'s cousin: a mutation making the helper check only its
        // first pair survived, because the helper's `all` over a list is true
        // of any prefix. Pin the length and walk the pairs here.
        assert_eq!(CupertinoColors::BACKGROUND_LADDER.len(), 4);
        for (role, below) in CupertinoColors::BACKGROUND_LADDER {
            assert_eq!(
                role.resolve(Brightness::Dark, UP),
                below.resolve(Brightness::Dark, BASE_LEVEL)
            );
        }

        assert_eq!(
            CupertinoColors::SYSTEM_BACKGROUND.resolve(Brightness::Dark, UP),
            CupertinoColors::SECONDARY_SYSTEM_BACKGROUND.resolve(Brightness::Dark, BASE_LEVEL)
        );
        assert_eq!(
            CupertinoColors::SECONDARY_SYSTEM_GROUPED_BACKGROUND.resolve(Brightness::Dark, UP),
            CupertinoColors::TERTIARY_SYSTEM_GROUPED_BACKGROUND
                .resolve(Brightness::Dark, BASE_LEVEL)
        );
    }

    #[test]
    fn and_the_tertiary_step_runs_past_the_end_of_the_named_three() {
        // There is no quaternary background, so the last rung is a value with
        // no role of its own.
        assert_eq!(
            CupertinoColors::TERTIARY_SYSTEM_BACKGROUND.resolve(Brightness::Dark, UP),
            Color::rgb(58, 58, 60)
        );
        assert_eq!(
            CupertinoColors::TERTIARY_SYSTEM_GROUPED_BACKGROUND.resolve(Brightness::Dark, UP),
            Color::rgb(58, 58, 60)
        );
    }

    #[test]
    fn elevation_never_moves_the_light_value() {
        // True of all eighteen of upstream's colours that have an elevated
        // variant: in the dark you raise a surface by lightening it, and in
        // the light there is nowhere lighter than white to go.
        for color in [
            CupertinoColors::SYSTEM_BACKGROUND,
            CupertinoColors::SECONDARY_SYSTEM_BACKGROUND,
            CupertinoColors::TERTIARY_SYSTEM_BACKGROUND,
            CupertinoColors::SYSTEM_GROUPED_BACKGROUND,
            CupertinoColors::SECONDARY_SYSTEM_GROUPED_BACKGROUND,
            CupertinoColors::TERTIARY_SYSTEM_GROUPED_BACKGROUND,
            CupertinoColors::SEPARATOR,
            CupertinoColors::LABEL,
            CupertinoColors::LINK,
        ] {
            assert!(color.elevation_only_moves_the_dark());
            assert_eq!(
                color.resolve(Brightness::Light, UP),
                color.resolve(Brightness::Light, BASE_LEVEL)
            );
        }
    }

    #[test]
    fn only_the_surface_moves_and_not_what_is_drawn_on_it() {
        // Content does not change when the surface under it rises.
        for content in [
            CupertinoColors::LABEL,
            CupertinoColors::SECONDARY_LABEL,
            CupertinoColors::LINK,
            CupertinoColors::OPAQUE_SEPARATOR,
        ] {
            assert!(
                !content.is_interface_elevation_dependent(),
                "content should not depend on elevation"
            );
            assert_eq!(
                content.resolve(Brightness::Dark, UP),
                content.resolve(Brightness::Dark, BASE_LEVEL)
            );
        }

        assert!(CupertinoColors::SYSTEM_BACKGROUND.is_interface_elevation_dependent());
    }

    #[test]
    fn a_translucent_separator_follows_the_surface_and_an_opaque_one_does_not() {
        // The same rule from the other end: one shows what is behind it, the
        // other hides it, so only one has anything to follow.
        assert!(CupertinoColors::SEPARATOR.is_interface_elevation_dependent());
        assert!(!CupertinoColors::OPAQUE_SEPARATOR.is_interface_elevation_dependent());

        // And it outruns the surface: 84,84,88 to 210,210,210, further than
        // any background moves, because it has to stay a line over a lighter
        // ground.
        assert_eq!(
            CupertinoColors::SEPARATOR.resolve(Brightness::Dark, UP),
            Color::argb(153, 210, 210, 210)
        );
    }

    // -- The dependency flags ---------------------------------------------------

    #[test]
    fn a_colour_that_does_not_vary_does_not_depend() {
        // Upstream's point is not the saved comparison: not consulting a trait
        // means not depending on it, so a widget drawn in such a colour is not
        // rebuilt when that trait changes.
        let flat = CupertinoDynamicColor::from(Color::rgb(1, 2, 3));
        assert!(!flat.is_platform_brightness_dependent());
        assert!(!flat.is_interface_elevation_dependent());

        assert!(CupertinoColors::LABEL.is_platform_brightness_dependent());
        assert!(!CupertinoColors::LABEL.is_interface_elevation_dependent());
    }

    #[test]
    fn the_two_flags_are_independent_of_each_other() {
        // A colour can vary along one axis and not the other, in either
        // combination -- the flags are not two names for one thing.
        let brightness_only =
            CupertinoDynamicColor::with_brightness(Color::rgb(1, 1, 1), Color::rgb(2, 2, 2));
        assert!(brightness_only.is_platform_brightness_dependent());
        assert!(!brightness_only.is_interface_elevation_dependent());

        let elevation_only = CupertinoDynamicColor {
            color: Color::rgb(1, 1, 1),
            dark_color: Color::rgb(1, 1, 1),
            elevated_color: Color::rgb(3, 3, 3),
            dark_elevated_color: Color::rgb(3, 3, 3),
        };
        assert!(!elevation_only.is_platform_brightness_dependent());
        assert!(elevation_only.is_interface_elevation_dependent());
    }

    #[test]
    fn a_flat_colour_resolves_to_itself_at_every_corner_of_the_table() {
        let flat = CupertinoDynamicColor::from(Color::rgb(7, 7, 7));
        for brightness in [Brightness::Light, Brightness::Dark] {
            for level in [BASE_LEVEL, UP] {
                assert_eq!(flat.resolve(brightness, level), Color::rgb(7, 7, 7));
            }
        }
    }

    #[test]
    fn with_brightness_leaves_a_colour_flat_along_elevation() {
        // Which is what makes it the right constructor for the colours that
        // upstream declares without elevated variants.
        let pair = CupertinoDynamicColor::with_brightness(Color::WHITE, Color::BLACK);
        assert_eq!(pair.elevated_color, pair.color);
        assert_eq!(pair.dark_elevated_color, pair.dark_color);
        assert!(!pair.is_interface_elevation_dependent());
    }

    #[test]
    fn the_four_corners_of_the_table_are_four_different_answers() {
        // Or the resolution could be ignoring one of its two arguments.
        let corners = CupertinoDynamicColor {
            color: Color::rgb(1, 1, 1),
            dark_color: Color::rgb(2, 2, 2),
            elevated_color: Color::rgb(3, 3, 3),
            dark_elevated_color: Color::rgb(4, 4, 4),
        };
        assert_eq!(
            corners.resolve(Brightness::Light, BASE_LEVEL),
            Color::rgb(1, 1, 1)
        );
        assert_eq!(
            corners.resolve(Brightness::Dark, BASE_LEVEL),
            Color::rgb(2, 2, 2)
        );
        assert_eq!(corners.resolve(Brightness::Light, UP), Color::rgb(3, 3, 3));
        assert_eq!(corners.resolve(Brightness::Dark, UP), Color::rgb(4, 4, 4));
    }
}

#[cfg(test)]
mod switch_on_off_label_tests {
    use super::*;

    #[test]
    fn nothing_is_drawn_unless_the_reader_asked() {
        // Upstream builds the pair or null, so with the setting off there is
        // nothing to draw rather than something drawn transparently.
        assert_eq!(SwitchOnOffLabels::resolve(false, None, None), None);
        assert!(SwitchOnOffLabels::resolve(true, None, None).is_some());
    }

    #[test]
    fn and_a_colour_someone_chose_does_not_turn_them_on() {
        // The setting gates the pair, not the colours: naming one while the
        // reader has not asked still draws nothing.
        assert_eq!(
            SwitchOnOffLabels::resolve(false, Some(Color::WHITE), Some(Color::BLACK)),
            None
        );
    }

    #[test]
    fn the_marks_are_a_bar_and_a_ring() {
        // A one-by-ten rectangle and a circle of radius five: the I and the O
        // of the power marks, drawn as primitives rather than set as text --
        // so they need no font and no translation.
        assert_eq!(SwitchOnOffLabels::ON_SIZE, (1.0, 10.0));
        assert_eq!(SwitchOnOffLabels::OFF_RADIUS, 5.0);

        // The bar is ten times as tall as it is wide, which is what makes it
        // read as a stroke rather than a dot.
        assert!(SwitchOnOffLabels::ON_SIZE.1 / SwitchOnOffLabels::ON_SIZE.0 >= 10.0);
        // And the ring is as tall as the bar, so the pair look like one size.
        assert_eq!(
            SwitchOnOffLabels::OFF_RADIUS * 2.0,
            SwitchOnOffLabels::ON_SIZE.1
        );
    }

    #[test]
    fn the_two_insets_are_not_the_same_inset() {
        // A circle of radius five and a bar one wide do not sit equally far in
        // at the same padding, so upstream gives them different numbers.
        assert_eq!(SwitchOnOffLabels::ON_PADDING, 11.0);
        assert_eq!(SwitchOnOffLabels::OFF_PADDING, 12.0);
        assert_ne!(
            SwitchOnOffLabels::ON_PADDING,
            SwitchOnOffLabels::OFF_PADDING
        );

        // And the difference is the ring's overhang: the bar's inset plus half
        // its width lands where the ring's inset less its radius would not.
        assert_eq!(
            SwitchOnOffLabels::OFF_PADDING - SwitchOnOffLabels::ON_PADDING,
            1.0
        );
    }

    #[test]
    fn the_defaults_are_white_and_a_grey() {
        let labels = SwitchOnOffLabels::resolve(true, None, None).unwrap();
        assert_eq!(labels.on_color, Color::WHITE);
        assert_eq!(labels.off_color, Color::argb(255, 179, 179, 179));
        assert_ne!(
            labels.off_color,
            Color::WHITE,
            "which is what the high-contrast value would have been"
        );
    }

    #[test]
    fn and_either_can_be_overridden_on_its_own() {
        let mine = Color(0xFF00FF00);
        let on_only = SwitchOnOffLabels::resolve(true, Some(mine), None).unwrap();
        assert_eq!(on_only.on_color, mine);
        assert_eq!(on_only.off_color, SwitchOnOffLabels::OFF_COLOR);

        let off_only = SwitchOnOffLabels::resolve(true, None, Some(mine)).unwrap();
        assert_eq!(off_only.on_color, Color::WHITE);
        assert_eq!(off_only.off_color, mine);
    }

    #[test]
    fn but_a_media_query_above_can_still_say_otherwise() {
        // Which is why the field is carried: an application that knows the
        // setting, or a test that wants the marks, puts one in the tree.
        let asked = crate::media_query::MediaQueryData {
            on_off_switch_labels: true,
            ..crate::media_query::MediaQueryData::default()
        };
        assert!(SwitchOnOffLabels::resolve(asked.on_off_switch_labels, None, None).is_some());
    }
}

#[cfg(test)]
mod tab_bar_tests {
    use super::*;
    use crate::framework::{ElementTree, component, provide};
    use crate::media_query::{MediaQuery, MediaQueryData};
    use crate::render::HitTestResult;

    /// Build and lay the bar out for real, under a view whose bottom inset is
    /// `inset`. Returns the laid-out root, so a caller can hit-test it.
    fn lay_out_bar(inset: f32) -> (RenderRef, Size) {
        let bar = CupertinoTabBar::new(
            1,
            vec![
                CupertinoTabItem::new("Home", "H"),
                CupertinoTabItem::new("Settings", "S"),
            ],
            0,
        );
        let data = MediaQueryData {
            view_padding: EdgeInsets::only(0.0, 0.0, 0.0, inset),
            ..MediaQueryData::default()
        };
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            CupertinoTheme::dark(),
            MediaQuery::new(data, component(bar)),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::loose(400.0, 300.0));
        (root, size)
    }

    fn bar_height(inset: f32) -> f32 {
        lay_out_bar(inset).1.height
    }

    /// The ids of the tabs a tap at `y` reaches, given the bar was built with
    /// `first_id` 1 and two items.
    fn tabs_under(inset: f32, y: f32) -> Vec<u64> {
        let (root, _) = lay_out_bar(inset);
        let mut result = HitTestResult::new();
        root.hit_test(Offset::new(100.0, y), &mut result);
        result
            .path
            .iter()
            .map(|entry| entry.target)
            .filter(|target| *target == 1 || *target == 2)
            .collect()
    }

    #[test]
    fn the_bar_reports_its_own_height_and_draws_a_taller_box() {
        // `preferredSize` is `Size.fromHeight(height)` -- the tabs' 50, with no
        // inset in it. The box drawn is `height + bottomPadding`. Run through
        // the build, not the arithmetic: a home indicator makes the box grow
        // and leaves the reported height alone.
        assert_eq!(CupertinoTabBar::preferred_height(), 50.0);
        assert_eq!(bar_height(0.0), 50.0);
        assert_eq!(bar_height(34.0), 84.0);
        assert_eq!(
            CupertinoTabBar::preferred_height(),
            50.0,
            "what a scaffold asks does not move with the inset"
        );
    }

    #[test]
    fn and_the_inset_is_given_straight_back_as_padding() {
        // Which is what keeps the two consistent: the box grows by the inset
        // and the content is pushed up by the same inset, so the items stay in
        // their 50 and the extra is empty space over the home indicator.
        //
        // Checked by tapping rather than by measuring, because the height is
        // the same either way -- a bar that dropped the padding would be just
        // as tall and would put a tab under the indicator, where a swipe up
        // from the bottom edge would tap it. So: a tap in the tabs' 50 finds a
        // tab, and a tap in the strip below finds none.
        let inset = 34.0;
        assert!(
            !tabs_under(inset, 25.0).is_empty(),
            "a tap in the tabs' own 50 hits a tab"
        );
        assert!(
            tabs_under(inset, 84.0 - 8.0).is_empty(),
            "but the strip over the home indicator is empty space"
        );
        // And with no indicator to avoid, the bottom of the bar is a tab
        // again -- the emptiness above is the inset's doing, not a margin the
        // bar always keeps.
        assert!(!tabs_under(0.0, 50.0 - 8.0).is_empty());
    }

    #[test]
    fn and_the_box_grows_by_exactly_the_inset() {
        for inset in [0.0, 8.0, 34.0] {
            assert_eq!(
                bar_height(inset),
                CupertinoTabBar::box_height(inset),
                "at inset {inset}"
            );
            assert_eq!(
                CupertinoTabBar::box_height(inset) - inset,
                CupertinoTabBar::preferred_height(),
                "the room left for tabs at inset {inset}"
            );
        }
    }

    #[test]
    fn opacity_is_a_question_about_the_colour() {
        // Upstream resolves the background and asks whether its alpha is
        // `0xFF`. Nobody sets a flag; a bar is opaque because of what it was
        // painted, and a translucent one gets the blur.
        assert!(CupertinoTabBar::is_opaque(Color(0xFF00_0000)));
        assert!(!CupertinoTabBar::is_opaque(Color(0xF000_0000)));
        assert!(!CupertinoTabBar::is_opaque(Color(0x0000_0000)));
        // Nearly opaque is not opaque: the test is equality, not a threshold.
        assert!(!CupertinoTabBar::is_opaque(Color(0xFE00_0000)));
    }

    #[test]
    fn and_it_is_asked_of_the_resolved_colour_not_the_dynamic_one() {
        // A `CupertinoDynamicColor` can be opaque in one appearance and not in
        // the other, so resolving first is what makes the answer meaningful --
        // upstream's default bar colour is one of these, translucent in both.
        let half_dark =
            CupertinoDynamicColor::with_brightness(Color(0xFFFF_FFFF), Color(0xCC00_0000));
        assert!(CupertinoTabBar::is_opaque(
            half_dark.resolve(Brightness::Light, BASE)
        ));
        assert!(!CupertinoTabBar::is_opaque(
            half_dark.resolve(Brightness::Dark, BASE)
        ));

        // And the default really is see-through, in both appearances -- which
        // is why the blur is the ordinary case rather than the exception.
        // (The theme carries the colour already resolved for its brightness,
        // so these are the values that reach the paint.)
        assert!(!CupertinoTabBar::is_opaque(
            CupertinoTheme::light().bar_background_color
        ));
        assert!(!CupertinoTabBar::is_opaque(
            CupertinoTheme::dark().bar_background_color
        ));
    }

    #[test]
    fn a_back_button_with_nothing_to_name_shows_nothing() {
        // `SizedBox.shrink()`, not the generic word. The doc on
        // `previous_page_title` claimed the opposite until this round, and
        // nothing had ever checked it.
        assert_eq!(
            CupertinoNavigationBarBackButton::label_for(None),
            BackLabel::Nothing
        );
        assert_eq!(
            CupertinoNavigationBarBackButton::label_for(None).text(),
            None
        );
    }

    #[test]
    fn a_title_too_long_to_be_a_name_is_replaced_by_the_word() {
        // Not ellipsized -- replaced. Half a name is not a name, and the
        // point of the back label is to say where you are going.
        let long = "Notification Settings";
        assert!(long.encode_utf16().count() > 12);
        assert_eq!(
            CupertinoNavigationBarBackButton::label_for(Some(long)),
            BackLabel::Generic
        );
        assert_eq!(
            CupertinoNavigationBarBackButton::label_for(Some(long)).text(),
            Some("Back")
        );
    }

    #[test]
    fn the_threshold_is_above_twelve_and_not_at_it() {
        let twelve = "abcdefghijkl";
        assert_eq!(twelve.encode_utf16().count(), 12);
        assert_eq!(
            CupertinoNavigationBarBackButton::label_for(Some(twelve)),
            BackLabel::PreviousTitle(twelve.to_string())
        );
        let thirteen = "abcdefghijklm";
        assert_eq!(
            CupertinoNavigationBarBackButton::label_for(Some(thirteen)),
            BackLabel::Generic
        );
    }

    #[test]
    fn the_count_is_utf16_units_because_dart_string_length_is() {
        // Bytes would cut a Cyrillic title to half its true length; `char`s
        // would let thirteen astral glyphs through where Dart sees
        // twenty-six.
        let cyrillic = "Настройки";
        assert_eq!(cyrillic.encode_utf16().count(), 9, "nine units to Dart");
        assert!(
            cyrillic.len() > 12,
            "and eighteen bytes, which would misjudge it"
        );
        assert_eq!(
            CupertinoNavigationBarBackButton::label_for(Some(cyrillic)),
            BackLabel::PreviousTitle(cyrillic.to_string())
        );

        // Seven astral glyphs: seven `char`s, fourteen UTF-16 units.
        let astral = "\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}";
        assert_eq!(astral.chars().count(), 7);
        assert_eq!(astral.encode_utf16().count(), 14);
        assert_eq!(
            CupertinoNavigationBarBackButton::label_for(Some(astral)),
            BackLabel::Generic,
            "Dart counts fourteen and replaces it"
        );
    }

    #[test]
    fn the_reader_always_hears_back_whatever_the_button_shows() {
        // `excludeSemantics: true` throws the subtree's semantics away and
        // puts one word in its place, so the visible label and the spoken one
        // are independent.
        assert_eq!(CupertinoNavigationBarBackButton::semantics_label(), "Back");
        for title in [None, Some("Inbox"), Some("Notification Settings")] {
            let shown = CupertinoNavigationBarBackButton::label_for(title);
            assert_eq!(
                CupertinoNavigationBarBackButton::semantics_label(),
                "Back",
                "shown as {shown:?}"
            );
        }
        // And on the one page where they agree, they agree by coincidence.
        assert_eq!(
            CupertinoNavigationBarBackButton::label_for(Some("Notification Settings")).text(),
            Some(CupertinoNavigationBarBackButton::semantics_label())
        );
    }

    #[test]
    fn an_unset_search_placeholder_says_search_rather_than_nothing() {
        // Upstream: `widget.placeholder ?? localizations.searchTextFieldPlaceholderLabel`.
        // This crate had no localizations when the field was ported and the
        // doc saying so outlived them, leaving an empty grey well.
        assert_eq!(
            CupertinoSearchTextField::new(1).effective_placeholder(),
            "Search"
        );
        assert_eq!(
            CupertinoSearchTextField::new(1)
                .with_placeholder("Find a demo")
                .effective_placeholder(),
            "Find a demo"
        );
    }

    #[test]
    fn clearing_a_field_that_had_text_announces_the_change() {
        // The field emptied itself and never told anyone, so an application
        // searching as the reader types would keep showing matches for text
        // that is gone.
        assert!(CupertinoSearchTextField::suffix_tap_announces("hello"));
        assert!(CupertinoSearchTextField::suffix_tap_announces(" "));
    }

    #[test]
    fn clearing_a_field_that_was_already_empty_announces_nothing() {
        // Under the default `suffixMode` of `editing` the button is not there
        // to tap, so this guard cannot fire -- but `suffixMode` is a caller's
        // choice, and under `always` the button sits on an empty field.
        // Without the guard, every tap of a button that did nothing would
        // re-run the application's search.
        assert!(!CupertinoSearchTextField::suffix_tap_announces(""));
    }

    #[test]
    fn the_announcement_carries_the_new_text_and_not_the_old() {
        // It is a change notification, so it says what the field now holds.
        // Handing back the old text is the mistake the shape invites -- the
        // controller is in scope and `onChanged(oldText)` reads fine.
        assert_eq!(CupertinoSearchTextField::suffix_tap("hello"), Some(""));
        assert_eq!(CupertinoSearchTextField::suffix_tap(""), None);
    }

    #[test]
    fn the_question_is_asked_of_the_text_that_was_there_and_not_of_the_answer() {
        // `textChanged` is read before the clear. Asked afterwards it is
        // always the empty string, so the guard would never let anything
        // through and the notification would never arrive at all.
        let before = "hello";
        let after = "";
        assert!(CupertinoSearchTextField::suffix_tap_announces(before));
        assert!(
            !CupertinoSearchTextField::suffix_tap_announces(after),
            "which is why it cannot be asked of the cleared field"
        );
    }

    #[test]
    fn an_explicitly_empty_placeholder_is_still_the_callers_choice() {
        // `??` falls back on null, not on empty. Somebody who asked for a
        // blank well gets one; only an unset placeholder takes the default.
        assert_eq!(
            CupertinoSearchTextField::new(1)
                .with_placeholder("")
                .effective_placeholder(),
            ""
        );
    }

    #[test]
    fn a_tab_is_announced_by_a_position_a_person_can_use() {
        // Upstream passes `tabIndex: index + 1` from a zero-based loop. The
        // conversion lives here so no caller has to remember it -- passing
        // the loop variable through gives "Tab 0 of 3" and never says
        // "Tab 3".
        let count = 3;
        let said: Vec<String> = (0..count)
            .map(|i| CupertinoTabBar::tab_semantics_hint(i, count).unwrap())
            .collect();
        assert_eq!(said, ["Tab 1 of 3", "Tab 2 of 3", "Tab 3 of 3"]);
        // Every tab is named, and the last one is named by the count.
        assert!(said.last().unwrap().starts_with(&format!("Tab {count}")));
    }

    #[test]
    fn an_empty_bar_has_no_tab_to_announce() {
        // Upstream's `assert(tabCount >= 1)`. Nothing calls this for a bar
        // with no items, and answering None beats formatting "Tab 1 of 0".
        assert_eq!(CupertinoTabBar::tab_semantics_hint(0, 0), None);
    }

    #[test]
    fn centring_stands_in_for_the_bottom_alignment_only_while_icons_agree() {
        // Upstream's row is `CrossAxisAlignment.end` so labels line up when
        // icons differ in height. Here every icon slot is a fixed square, so
        // the columns are the same height and centring puts them in the same
        // place -- but that is a consequence of the fixed slot, not a licence
        // to ignore the alignment.
        let size = CupertinoTabBar::ICON_SIZE;
        assert_eq!(size, 30.0);
        assert!(CupertinoTabBar::alignment_is_equivalent(&[
            size, size, size
        ]));
        // Let one item size its own icon and the two alignments part company.
        assert!(!CupertinoTabBar::alignment_is_equivalent(&[
            size,
            size - 6.0,
            size
        ]));
        assert!(!CupertinoTabBar::alignment_is_equivalent(&[
            size,
            size + 6.0
        ]));
    }
}

#[cfg(test)]
mod form_section_tests {
    use super::*;

    #[test]
    fn a_section_has_leading_until_it_says_otherwise() {
        // Upstream writes `bool hasLeading = true` in both constructors, so
        // the derive would have got this backwards.
        assert!(CupertinoListSection::new().has_leading);
        assert!(CupertinoListSection::inset_grouped().has_leading);
        assert!(!CupertinoListSection::new().without_leading().has_leading);
    }

    #[test]
    fn no_leading_does_not_mean_no_extra_margin() {
        // The tempting model -- add the extra with a leading widget, add
        // nothing without -- is right for base and wrong for inset.
        assert_eq!(
            CupertinoListSection::new()
                .without_leading()
                .additional_divider_margin(),
            0.0
        );
        assert_eq!(
            CupertinoListSection::inset_grouped()
                .without_leading()
                .additional_divider_margin(),
            14.0,
            "an inset section still clears 14 with no icon to clear"
        );
        assert_ne!(
            CupertinoListSection::inset_grouped()
                .without_leading()
                .additional_divider_margin(),
            CupertinoListSection::new()
                .without_leading()
                .additional_divider_margin()
        );
    }

    #[test]
    fn and_the_four_numbers_are_four_numbers() {
        // Measured off three different shipped apps, so nothing here is
        // derivable from anything else here.
        let starts = [
            (CupertinoListSection::new(), 20.0 + 44.0),
            (CupertinoListSection::new().without_leading(), 20.0),
            (CupertinoListSection::inset_grouped(), 14.0 + 42.0),
            (
                CupertinoListSection::inset_grouped().without_leading(),
                14.0 + 14.0,
            ),
        ];
        for (section, expected) in starts {
            assert_eq!(section.divider_start(), expected, "{section:?}");
        }
        // All four land in different places -- no pair collapses.
        let mut seen: Vec<f32> = starts.iter().map(|(s, _)| s.divider_start()).collect();
        seen.sort_by(|a, b| a.partial_cmp(b).unwrap());
        seen.dedup();
        assert_eq!(seen.len(), 4);
    }

    #[test]
    fn a_form_always_passes_its_own_margin_down() {
        // A list section picks its top margin by whether it has a header; a
        // form never lets that choice run. So the header-less cases disagree.
        let list_without_header = CupertinoListSection::inset_grouped().rows_margin();
        let list_with_header = CupertinoListSection::inset_grouped()
            .with_header()
            .rows_margin();
        assert_ne!(
            list_without_header, list_with_header,
            "the list section's margin does turn on the header"
        );

        let form = CupertinoFormSection::inset_grouped();
        assert_eq!(
            form.rows_margin(),
            CupertinoFormSection::INSET_GROUPED_ROWS_MARGIN
        );
        assert_eq!(
            form.rows_margin(),
            list_with_header,
            "which happens to match the with-header case"
        );
        assert_ne!(
            form.rows_margin(),
            list_without_header,
            "and that is the one that shows the difference: no header, and the \
             form still has a zero top where the list section has 20"
        );
    }

    #[test]
    fn and_a_base_form_has_no_margin_at_all() {
        assert_eq!(
            CupertinoFormSection::default().rows_margin(),
            EdgeInsets::ZERO,
            "a form is not always the inset-grouped shape"
        );
    }

    #[test]
    fn the_form_builds_a_section_without_leading() {
        for form in [
            CupertinoFormSection::default(),
            CupertinoFormSection::inset_grouped(),
        ] {
            let section = form.section();
            assert!(!section.has_leading);
            assert_eq!(section.section_type, form.section_type);
            // Which is the whole point: the divider starts at the plain
            // margin rather than clearing an icon column that is not there.
            assert_eq!(
                section.divider_start(),
                section.divider_margin() + section.additional_divider_margin()
            );
        }
        // A form's inset divider therefore starts well short of a list
        // section's.
        assert!(
            CupertinoFormSection::inset_grouped()
                .section()
                .divider_start()
                < CupertinoListSection::inset_grouped().divider_start()
        );
    }

    #[test]
    fn a_form_does_not_clip_where_a_list_section_does() {
        // Both form constructors default to `Clip.none` and pass it down.
        assert!(CupertinoListSection::inset_grouped().clips_rows());
        assert!(!CupertinoFormSection::inset_grouped().clips_rows());
        // And neither clips when there is no card to clip against.
        assert!(!CupertinoListSection::new().clips_rows());
        assert!(!CupertinoFormSection::default().clips_rows());
    }

    #[test]
    fn a_header_alone_is_a_list_section_but_not_a_form() {
        assert!(CupertinoListSection::is_legal(0, true));
        assert!(!CupertinoFormSection::is_legal(0, true));
        // Rows make both legal; nothing makes either legal.
        assert!(CupertinoListSection::is_legal(1, false));
        assert!(CupertinoFormSection::is_legal(1, false));
        assert!(!CupertinoListSection::is_legal(0, false));
        assert!(!CupertinoFormSection::is_legal(0, false));
    }
}

#[cfg(test)]
mod form_row_tests {
    use super::*;

    #[test]
    fn the_helper_follows_the_appearance() {
        // Upstream resolves the row's text colour with `maybeResolve`, so the
        // helper is a different grey in the two appearances.
        assert_ne!(
            CupertinoFormRow::helper_color(Brightness::Light),
            CupertinoFormRow::helper_color(Brightness::Dark)
        );
    }

    #[test]
    fn and_the_error_does_not() {
        // A `const TextStyle` cannot resolve anything -- resolution wants a
        // context at run time -- so the error carries the base value into both
        // appearances.
        assert!(CupertinoFormRow::error_color_agrees(Brightness::Light));
        assert!(
            !CupertinoFormRow::error_color_agrees(Brightness::Dark),
            "the error label is drawn in the light red on a dark page"
        );
    }

    #[test]
    fn and_the_two_reds_really_are_two_reds() {
        // Without this the previous test would pass for the wrong reason: an
        // inconsistency between arms that compute the same colour is not an
        // inconsistency anybody can see.
        let light = CupertinoFormRow::error_color_if_resolved(Brightness::Light);
        let dark = CupertinoFormRow::error_color_if_resolved(Brightness::Dark);
        assert_ne!(light, dark);
        assert_eq!(light, Color::rgb(255, 59, 48));
        assert_eq!(dark, Color::rgb(255, 69, 58));
        // And the one upstream actually paints is the light one, either way.
        assert_eq!(CupertinoFormRow::error_color(), light);
    }

    #[test]
    fn the_helper_and_the_error_disagree_only_on_a_dark_page() {
        // Which is what makes this hard to notice: in light mode the two
        // labels are consistent, and the port would look right.
        for brightness in [Brightness::Light, Brightness::Dark] {
            let helper_moved = CupertinoFormRow::helper_color(brightness)
                != CupertinoFormRow::helper_color(Brightness::Light);
            let error_moved = CupertinoFormRow::error_color()
                != CupertinoFormRow::error_color_if_resolved(Brightness::Light);
            assert!(!error_moved, "the error never moves");
            if matches!(brightness, Brightness::Dark) {
                assert!(helper_moved, "but the helper next to it does");
            }
        }
    }

    #[test]
    fn an_error_is_set_in_medium() {
        assert_eq!(CupertinoFormRow::ERROR_WEIGHT, 500);
    }

    #[test]
    fn the_row_reserves_far_more_at_the_start_than_the_end() {
        // The label reads down a column; the field runs out to near the edge.
        let padding = CupertinoFormRow::PADDING;
        assert_eq!(padding, EdgeInsets::only(20.0, 6.0, 6.0, 6.0));
        assert!(padding.left > padding.right * 3.0);
    }
}

// -- What the Cupertino glyphs actually put on the canvas ---------------------

#[cfg(test)]
mod glyph_paint_tests {
    use super::{BackChevron, ClearGlyph, SEARCH_FIELD_ITEM_SIZE, SearchGlyph};
    use crate::engine::{Color, LayerTree};
    use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
    use crate::render::{BoxConstraints, Offset, PaintContext, RenderBox, Size};

    const INK: Color = Color(0xff112233);
    const FIELD: Color = Color(0xff445566);

    /// Lays the glyph out at its natural size and paints it at `at`, returning
    /// what the canvas was told.
    ///
    /// These three are drawn rather than set in an icon font -- the module docs
    /// say why -- so what is on the screen *is* these calls, and until the
    /// recorder was pointed at them nothing checked any of it. `unpainted.py`
    /// had cupertino.rs at eight draw calls with no reader, the largest entry
    /// on that list.
    fn painted(mut glyph: impl RenderBox, at: Offset) -> Vec<Drawn> {
        glyph.layout(BoxConstraints::loose(100.0, 100.0));
        let mut layers = LayerTree::new(200, 200);
        reset_drawn();
        {
            let mut context = PaintContext::new(&mut layers, Size::new(200.0, 200.0));
            glyph.paint(&mut context, at);
        }
        drawn()
    }

    fn lines(calls: &[Drawn]) -> Vec<((f32, f32), (f32, f32), u32)> {
        calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Line { from, to, argb, .. } => Some((*from, *to, *argb)),
                _ => None,
            })
            .collect()
    }

    fn chevron(mirror: bool) -> BackChevron {
        BackChevron {
            color: INK,
            mirror,
            laid_out: Size::ZERO,
        }
    }

    #[test]
    fn the_back_chevron_is_two_strokes_meeting_at_a_point() {
        // Upstream's is the `CupertinoIcons.back` glyph; with no icon font here
        // it is two strokes, and the shape is the whole of what it says. They
        // have to *meet*: two strokes that stop short of one another read as a
        // broken arrow rather than a chevron.
        let calls = painted(chevron(false), Offset::ZERO);
        let strokes = lines(&calls);
        assert_eq!(strokes.len(), 2, "{calls:?}");
        assert_eq!(
            strokes[0].1, strokes[1].0,
            "the second starts where the first ended"
        );
        assert_eq!(strokes[0].1, (3.0, 10.0), "and the point is at the tip");
        assert_eq!(strokes[0].2, INK.0, "in the colour it was given");
        assert_eq!(strokes[1].2, INK.0);
    }

    #[test]
    fn and_it_points_at_the_start_of_the_line_whichever_way_that_is() {
        // Upstream's `_BackChevron` mirrors itself under `TextDirection.rtl`.
        // A mirror is a reflection about the middle of the box, not a
        // different pair of numbers that happens to lean the other way -- and
        // the difference shows as a chevron sitting off-centre.
        let box_width = 12.0;
        let middle = box_width / 2.0;

        let ltr = lines(&painted(chevron(false), Offset::ZERO));
        let rtl = lines(&painted(chevron(true), Offset::ZERO));
        assert_eq!(ltr.len(), 2);
        assert_eq!(rtl.len(), 2);

        for (index, (left, right)) in ltr.iter().zip(rtl.iter()).enumerate() {
            assert_eq!(
                left.0.0 + right.0.0,
                box_width,
                "stroke {index} start reflects about {middle}"
            );
            assert_eq!(left.1.0 + right.1.0, box_width, "stroke {index} end");
            assert_eq!(left.0.1, right.0.1, "and nothing moves vertically");
            assert_eq!(left.1.1, right.1.1);
        }

        // Which way each points, said plainly: the tip is nearer the start of
        // the line than the two ends are.
        assert!(ltr[0].1.0 < ltr[0].0.0, "ltr points left");
        assert!(rtl[0].1.0 > rtl[0].0.0, "rtl points right");
    }

    #[test]
    fn the_search_glass_has_its_handle_on_the_far_diagonal() {
        // The glass sits high and to the start; the handle runs out from its
        // lower-right, along the diagonal through the centre. Anything else is
        // a circle with a stick beside it.
        let calls = painted(
            SearchGlyph {
                color: INK,
                laid_out: Size::ZERO,
            },
            Offset::ZERO,
        );
        let circles: Vec<_> = calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Circle {
                    cx,
                    cy,
                    radius,
                    argb,
                    ..
                } => Some((*cx, *cy, *radius, *argb)),
                _ => None,
            })
            .collect();
        assert_eq!(circles.len(), 1, "{calls:?}");
        let (cx, cy, radius, _) = circles[0];

        let strokes = lines(&calls);
        assert_eq!(strokes.len(), 1);
        let ((from_x, from_y), (to_x, to_y), argb) = strokes[0];
        assert_eq!(argb, INK.0, "the handle is the same ink as the glass");

        // On the diagonal: equal steps in x and y, both away from the centre.
        assert_eq!(from_x - cx, from_y - cy, "the near end is on the diagonal");
        assert_eq!(to_x - from_x, to_y - from_y, "and so is the run");
        assert!(to_x > from_x && from_x > cx, "it runs outward");
        // And it starts at about the rim rather than inside the glass.
        let reach = ((from_x - cx).powi(2) + (from_y - cy).powi(2)).sqrt();
        assert!(
            (reach - radius).abs() < 0.2,
            "the handle meets the rim: {reach} against {radius}"
        );
    }

    #[test]
    fn the_clear_marks_cross_is_knocked_out_in_the_background_colour() {
        // The claim the recorder could not see until this tick: `Drawn::Line`
        // carried no colour, so a cross drawn in the item colour instead of
        // the field's would have been a filled circle with an invisible mark
        // in it and nothing to say so.
        let calls = painted(
            ClearGlyph {
                color: INK,
                background: FIELD,
                laid_out: Size::ZERO,
            },
            Offset::ZERO,
        );
        let circles: Vec<_> = calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Circle {
                    cx,
                    cy,
                    radius,
                    argb,
                    ..
                } => Some((*cx, *cy, *radius, *argb)),
                _ => None,
            })
            .collect();
        assert_eq!(circles.len(), 1, "{calls:?}");
        let (cx, cy, radius, fill) = circles[0];
        assert_eq!(fill, INK.0, "the disc is the item colour");
        assert_eq!(radius, SEARCH_FIELD_ITEM_SIZE / 2.0);
        assert_eq!(
            (cx, cy),
            (SEARCH_FIELD_ITEM_SIZE / 2.0, SEARCH_FIELD_ITEM_SIZE / 2.0)
        );

        let strokes = lines(&calls);
        assert_eq!(strokes.len(), 2, "two arms");
        for (index, (from, to, argb)) in strokes.iter().enumerate() {
            assert_eq!(*argb, FIELD.0, "arm {index} is knocked out, not drawn on");
            assert_ne!(*argb, INK.0);
            // Centred on the disc: the two ends are opposite about the middle.
            assert_eq!((from.0 + to.0) / 2.0, cx, "arm {index} is centred in x");
            assert_eq!((from.1 + to.1) / 2.0, cy, "arm {index} is centred in y");
        }
        // And the two arms cross rather than lying on top of one another.
        assert_ne!(strokes[0].0, strokes[1].0);
        assert_eq!(
            strokes[0].0.1 - strokes[0].1.1,
            -(strokes[1].0.1 - strokes[1].1.1),
            "one leans each way"
        );
    }

    #[test]
    fn every_glyph_paints_where_it_was_put() {
        // A glyph that ignores its offset draws in the corner of the screen,
        // which is the kind of thing that survives every test that only counts
        // the calls.
        let at = Offset::new(40.0, 25.0);
        let here = lines(&painted(chevron(false), Offset::ZERO));
        let there = lines(&painted(chevron(false), at));
        assert_eq!(here.len(), there.len());
        for (index, (near, far)) in here.iter().zip(there.iter()).enumerate() {
            assert_eq!(far.0.0 - near.0.0, at.dx, "stroke {index}");
            assert_eq!(far.0.1 - near.0.1, at.dy);
        }
    }
}

// -- Where the activity indicator's ticks go ----------------------------------

#[cfg(test)]
mod activity_tick_tests {
    //! The spinner is one tick shape drawn once per entry in `TICK_ALPHAS`
    //! with a rotation between each, and until the stub recorded `rotate` the
    //! rotation was invisible: a ring of ticks stacked on top of one another
    //! looked exactly like a ring of ticks around a circle, because the only
    //! thing that separates them is a canvas call nothing could see.
    //!
    //! The count is `TICK_ALPHAS.len()` and never a number written here --
    //! upstream's `_kAlphaValues` is what decides how many arms the spinner
    //! has, and a test that spelled it would be asserting its own arithmetic.

    use super::{ActivityIndicatorTicks, TICK_ALPHAS};
    use crate::engine::{Color, LayerTree};
    use crate::engine_test_stubs::{Drawn, drawn, reset_drawn, save_depth};
    use crate::render::{BoxConstraints, Offset, PaintContext, RenderBox, Size};

    const INK: Color = Color(0xff123456);
    const RADIUS: f32 = 10.0;

    fn painted(position: f32, progress: f32, at: Offset) -> Vec<Drawn> {
        let mut ticks = ActivityIndicatorTicks {
            position,
            progress,
            radius: RADIUS,
            color: INK,
            laid_out: Size::ZERO,
        };
        ticks.layout(BoxConstraints::loose(100.0, 100.0));
        let mut layers = LayerTree::new(200, 200);
        reset_drawn();
        {
            let mut context = PaintContext::new(&mut layers, Size::new(200.0, 200.0));
            ticks.paint(&mut context, at);
        }
        drawn()
    }

    fn rotations(calls: &[Drawn]) -> Vec<f32> {
        calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Rotate { degrees } => Some(*degrees),
                _ => None,
            })
            .collect()
    }

    fn ticks_drawn(calls: &[Drawn]) -> usize {
        calls
            .iter()
            .filter(|call| matches!(call, Drawn::Path { .. }))
            .count()
    }

    #[test]
    fn a_full_ring_is_one_tick_per_step_all_the_way_round() {
        // One tick per alpha, and the rotations between them add up to a
        // whole turn. Any other step and the ring is either bunched into an
        // arc or wrapped past itself.
        let calls = painted(0.0, 1.0, Offset::ZERO);
        let count = TICK_ALPHAS.len();
        assert_eq!(ticks_drawn(&calls), count);

        let turns = rotations(&calls);
        assert_eq!(turns.len(), count);
        let step = 360.0 / count as f32;
        assert!(turns.iter().all(|degrees| *degrees == step), "{turns:?}");
        assert_eq!(
            turns.iter().sum::<f32>(),
            360.0,
            "the steps close the circle"
        );
    }

    #[test]
    fn a_partly_revealed_ring_draws_only_what_it_has_revealed() {
        // Upstream's `progress` is how much of the spinner has appeared, which
        // is what makes a Cupertino activity indicator fade in rather than
        // pop.
        let calls = painted(0.0, 0.5, Offset::ZERO);
        assert_eq!(ticks_drawn(&calls), TICK_ALPHAS.len() / 2);
    }

    #[test]
    fn and_nothing_at_all_before_it_has_started() {
        let calls = painted(0.0, 0.0, Offset::ZERO);
        assert_eq!(ticks_drawn(&calls), 0);
        assert!(rotations(&calls).is_empty(), "{calls:?}");
    }

    #[test]
    fn the_ring_turns_about_the_middle_of_the_box_it_was_given() {
        // The translate goes first and is what makes the rotation a rotation
        // about the spinner's centre rather than about the corner of the
        // screen -- where a ring of radius ten would be a quarter visible.
        let at = Offset::new(30.0, 40.0);
        let calls = painted(0.0, 1.0, at);
        let moved = calls.iter().find_map(|call| match call {
            Drawn::Translate { dx, dy } => Some((*dx, *dy)),
            _ => None,
        });
        assert_eq!(
            moved,
            Some((at.dx + RADIUS, at.dy + RADIUS)),
            "the centre of a box two radii across: {calls:?}"
        );
        assert_eq!(save_depth(&calls), Some(0), "and it is put back");
    }

    #[test]
    fn the_ticks_are_drawn_dimmest_first_and_the_head_comes_last() {
        // `TICK_ALPHAS` in order, starting at whichever tick `position` says
        // is the head. What makes a spinner look like it is turning is the
        // *fade*, not the movement -- every tick is in the same place on every
        // frame.
        //
        // The brightest is the **last** drawn, not the first. The first draft
        // of this test asserted the opposite from memory of upstream and was
        // wrong: `_kAlphaValues` runs dim to bright, and the leading arm is
        // the one at the end of the list.
        let calls = painted(0.0, 1.0, Offset::ZERO);
        let alphas: Vec<u8> = calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Path { argb, .. } => Some((*argb >> 24) as u8),
                _ => None,
            })
            .collect();
        assert_eq!(alphas, TICK_ALPHAS.to_vec());
        assert!(
            *alphas.last().expect("ticks") > alphas[0],
            "the head is brighter than the tail: {alphas:?}"
        );
        // And the tail is flat -- four arms at the same dim value, which is
        // what stops the spinner reading as a comet with a long fade.
        assert!(
            alphas.windows(2).all(|pair| pair[0] <= pair[1]),
            "never brightens then dims: {alphas:?}"
        );
    }

    #[test]
    fn and_the_head_moves_with_the_position() {
        // The same twelve alphas, rotated round the ring. A spinner whose
        // position did nothing would be a static ring of dashes.
        let start = painted(0.0, 1.0, Offset::ZERO);
        let later = painted(0.5, 1.0, Offset::ZERO);
        let alphas = |calls: &[Drawn]| -> Vec<u8> {
            calls
                .iter()
                .filter_map(|call| match call {
                    Drawn::Path { argb, .. } => Some((*argb >> 24) as u8),
                    _ => None,
                })
                .collect()
        };
        assert_ne!(alphas(&start), alphas(&later));
        let mut sorted_start = alphas(&start);
        let mut sorted_later = alphas(&later);
        sorted_start.sort_unstable();
        sorted_later.sort_unstable();
        assert_eq!(
            sorted_start, sorted_later,
            "the same alphas, in a different place"
        );
    }

    // -- Whose colour an iOS switch is ---------------------------------------

    use crate::cupertino::{CupertinoSwitch, CupertinoTheme};

    const GREEN: Color = Color(0xff34c759);
    const PRIMARY: Color = Color(0xffcc0044);
    const ONE_OFF: Color = Color(0xff112233);

    fn track(one_off: Option<Color>, apply: Option<bool>, all: bool) -> Color {
        CupertinoSwitch::active_track_color(one_off, apply, all, PRIMARY, GREEN)
    }

    #[test]
    fn a_switch_is_ios_green_until_something_asks_otherwise() {
        // The default at both levels. An iOS switch is green because iOS
        // switches are green, not because the application is: a theme's
        // primary colour is for the things the application chose.
        assert_eq!(track(None, None, false), GREEN);
    }

    #[test]
    fn a_theme_can_take_over_every_switch_at_once() {
        // `CupertinoThemeData.applyThemeToAll`, which this crate had as a
        // field with no reader -- so a theme that asked for it was ignored and
        // every switch stayed green.
        assert_eq!(track(None, None, true), PRIMARY);
    }

    #[test]
    fn and_one_switch_can_disagree_with_its_theme_in_either_direction() {
        // This is what upstream's nullable `applyTheme` buys, and why it is
        // not a plain `bool`: `None` is "whatever the theme says", which is a
        // third answer rather than a missing one.
        assert_eq!(
            track(None, Some(true), false),
            PRIMARY,
            "opting in under a theme that says no"
        );
        assert_eq!(
            track(None, Some(false), true),
            GREEN,
            "and opting out under one that says yes"
        );
    }

    #[test]
    fn a_colour_named_on_the_switch_beats_both() {
        // First in upstream's chain. A caller who named a colour has said
        // more than either flag, and the flags only choose *which default*.
        for apply in [None, Some(false), Some(true)] {
            for all in [false, true] {
                assert_eq!(track(Some(ONE_OFF), apply, all), ONE_OFF, "{apply:?} {all}");
            }
        }
    }

    #[test]
    fn the_default_theme_says_no_at_both_appearances() {
        // The field is on the theme, so a light and a dark theme both have to
        // carry upstream's default rather than one of them picking it up.
        assert!(!CupertinoTheme::light().apply_theme_to_all);
        assert!(!CupertinoTheme::dark().apply_theme_to_all);
    }
}

#[cfg(test)]
mod built_tree_tests {
    use super::*;
    use crate::editable::RenderEditable;
    use crate::framework::{AnyWidget, ElementTree, component, provide, stateful};
    use crate::render::{BoxConstraints, Offset, RenderBox, RenderRef};

    /// Walks the built tree and hands each **concrete** render object to
    /// `look`, stopping at the first `Some`.
    ///
    /// `RenderRef::unwrapped` is what makes this possible: every node a
    /// visitor receives is a handle, so a downcast without it answers `None`
    /// at every step.
    fn find<T>(widget: AnyWidget, look: impl Fn(&dyn RenderBox) -> Option<T> + Copy) -> Option<T> {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(CupertinoTheme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(300.0, 200.0));

        fn walk<T>(
            node: &dyn RenderBox,
            look: impl Fn(&dyn RenderBox) -> Option<T> + Copy,
        ) -> Option<T> {
            if let Some(found) = RenderRef::unwrapped(node, look) {
                return Some(found);
            }
            let mut found = None;
            node.visit_children(&mut |child: &dyn RenderBox, _: Offset| {
                if found.is_none() {
                    found = walk(child, look);
                }
            });
            found
        }
        root.with(|node| walk(node, look))
    }

    fn placeholder_of(widget: AnyWidget) -> Option<String> {
        find(widget, |node| {
            node.as_any()
                .downcast_ref::<RenderEditable>()
                .map(|editable| editable.placeholder().to_string())
        })
    }

    #[test]
    fn the_search_field_hands_its_editable_the_word_it_decided_on() {
        // The assertion round 295 wrote down instead of writing. Testing
        // `effective_placeholder()` tests the decision; this tests that the
        // build used it, which is the part that was actually broken.
        assert_eq!(
            placeholder_of(stateful(CupertinoSearchTextField::new(1))).as_deref(),
            Some("Search")
        );
        assert_eq!(
            placeholder_of(stateful(
                CupertinoSearchTextField::new(2).with_placeholder("Find a demo")
            ))
            .as_deref(),
            Some("Find a demo")
        );
    }

    /// Every string the built tree will paint, in walk order.
    fn painted_strings(widget: AnyWidget) -> Vec<String> {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(CupertinoTheme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(300.0, 200.0));

        fn walk(node: &dyn RenderBox, out: &mut Vec<String>) {
            RenderRef::unwrapped(node, |concrete| {
                if let Some(paragraph) = concrete
                    .as_any()
                    .downcast_ref::<crate::render::RenderParagraph>()
                {
                    out.push(paragraph.content().to_string());
                }
            });
            node.visit_children(&mut |child: &dyn RenderBox, _: Offset| walk(child, out));
        }
        let mut out = Vec::new();
        root.with(|node| walk(node, &mut out));
        out
    }

    #[test]
    fn the_nav_bar_paints_the_word_when_the_previous_title_is_too_long() {
        // Round 298's deferred assertion. `label_for` decides; this checks
        // that the bar's build used the decision -- a long title is replaced
        // by "Back", not ellipsized.
        let long = "Notification Settings";
        assert!(long.encode_utf16().count() > 12);
        let painted = painted_strings(component(
            CupertinoNavigationBar::new().with_back(7, Some(long.to_string())),
        ));
        assert!(
            painted.iter().any(|s| s == "Back"),
            "expected the generic word, painted: {painted:?}"
        );
        assert!(
            !painted.iter().any(|s| s == long),
            "and not the title itself: {painted:?}"
        );
    }

    #[test]
    fn a_short_previous_title_is_painted_as_itself() {
        let painted = painted_strings(component(
            CupertinoNavigationBar::new().with_back(7, Some("Inbox".to_string())),
        ));
        assert!(painted.iter().any(|s| s == "Inbox"), "{painted:?}");
    }

    #[test]
    fn a_walk_without_unwrapping_the_handle_finds_nothing() {
        // Why `RenderRef::unwrapped` has to exist, stated as a test rather
        // than as a paragraph: the same walk, downcasting the node it is
        // handed, never sees a single render object.
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            CupertinoTheme::dark(),
            stateful(CupertinoSearchTextField::new(1)),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(300.0, 200.0));

        fn walk_naively(node: &dyn RenderBox, seen: &mut usize, editables: &mut usize) {
            *seen += 1;
            if node.as_any().downcast_ref::<RenderEditable>().is_some() {
                *editables += 1;
            }
            node.visit_children(&mut |child: &dyn RenderBox, _: Offset| {
                walk_naively(child, seen, editables)
            });
        }
        let (mut seen, mut editables) = (0, 0);
        root.with(|node| walk_naively(node, &mut seen, &mut editables));

        assert!(seen > 1, "the walk itself works: {seen} nodes");
        assert_eq!(editables, 0, "and every one of them is a handle");
    }
}
