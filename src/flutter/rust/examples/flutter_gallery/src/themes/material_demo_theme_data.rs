// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The theme the demo pages run under.
//!
//! Ported from `lib/themes/material_demo_theme_data.dart` (flutter/gallery @
//! d12640d). Upstream's demo page (`lib/pages/demo.dart`) wraps each demo in
//! this theme -- always light, Material's own purple -- rather than the
//! gallery's theme, so a demo shows the component the way a plain Material
//! app would. The demos here still draw under the gallery theme; applying
//! this one to the demo pages is batch M-C/M-D work, and the delta is logged
//! in PORTING.md.
//!
//! What is not carried: `typography` (upstream's is `Typography.material2018`
//! in Roboto, which is not among the fonts the gallery ships), and
//! `visualDensity`, which the framework has no counterpart of. Both are
//! upstream defaults that change nothing a demo here can show.

use rustflutter::components::Theme;
use rustflutter::engine::Color;

use crate::data::gallery_options::TargetPlatform;

/// Upstream's `_colorScheme`, in full.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)] // Upstream's complete ColorScheme; the framework's Theme
                    // reads only part of it, like the gallery's own scheme.
pub struct ColorScheme {
    pub primary: Color,
    pub primary_container: Color,
    pub secondary: Color,
    pub secondary_container: Color,
    pub background: Color,
    pub surface: Color,
    pub on_background: Color,
    pub on_surface: Color,
    pub error: Color,
    pub on_error: Color,
    pub on_primary: Color,
    pub on_secondary: Color,
}

/// Upstream's `_colorScheme` value. The demo theme is always light; upstream
/// spells that as a `brightness` field on the scheme rather than as a second
/// scheme.
pub const COLOR_SCHEME: ColorScheme = ColorScheme {
    primary: Color(0xFF6200EE),
    primary_container: Color(0xFF6200EE),
    secondary: Color(0xFFFF5722),
    secondary_container: Color(0xFFFF5722),
    background: Color::WHITE,
    surface: Color(0xFFF2F2F2),
    on_background: Color::BLACK,
    on_surface: Color::BLACK,
    error: Color(0xFFF44336), // Colors.red
    on_error: Color::WHITE,
    on_primary: Color::WHITE,
    on_secondary: Color::WHITE,
};

/// Upstream's `highlightColor: Colors.transparent`.
#[allow(dead_code)]
pub const HIGHLIGHT_COLOR: Color = Color(0x00000000);
/// Upstream's `indicatorColor: _colorScheme.onPrimary`.
#[allow(dead_code)]
pub const INDICATOR_COLOR: Color = COLOR_SCHEME.on_primary;
/// Upstream's `secondaryHeaderColor: _colorScheme.background`.
#[allow(dead_code)]
pub const SECONDARY_HEADER_COLOR: Color = COLOR_SCHEME.background;

/// Upstream's `MaterialDemoThemeData`.
pub struct MaterialDemoThemeData;

/// The accessors beyond `theme_data` are the component themes: the part of
/// upstream's `ThemeData` the framework's `Theme` has no slots for, ported as
/// data. The demo-page wrapper that reads them is M-C/M-D work, so they are
/// unused today.
#[allow(dead_code)]
impl MaterialDemoThemeData {
    /// Upstream's `themeData`, as the framework's smaller `Theme`.
    ///
    /// The component themes below (`app_bar_theme` and friends) are the part
    /// of upstream's `ThemeData` the framework's `Theme` has no slots for;
    /// they are ported as data rather than dropped.
    pub fn theme_data() -> Theme {
        Theme {
            background: COLOR_SCHEME.background,
            surface: COLOR_SCHEME.surface,
            // No upstream counterpart, as with the gallery theme: take the
            // quietest fill the scheme has.
            surface_variant: COLOR_SCHEME.surface,
            outline: COLOR_SCHEME.on_surface.with_alpha(0x1F),
            primary: COLOR_SCHEME.primary,
            on_primary: COLOR_SCHEME.on_primary,
            danger: COLOR_SCHEME.error,
            text: COLOR_SCHEME.on_surface,
            text_muted: COLOR_SCHEME.on_surface.with_alpha(0x80),
            // Material's default corner radius; the gallery theme's 10 is its
            // own look, not a demo's.
            radius: 4.0,
            spacing: 8.0,
            body_size: 14.0,
            title_size: 16.0,
            // Roboto is not shipped; the framework default stands in.
            font_family: None,
        }
    }

    /// Upstream's `appBarTheme`: primary fill, on-primary icons.
    pub fn app_bar_theme() -> (Color, Color) {
        (COLOR_SCHEME.primary, COLOR_SCHEME.on_primary)
    }

    /// Upstream's `bottomAppBarTheme`.
    pub fn bottom_app_bar_color() -> Color {
        COLOR_SCHEME.primary
    }

    /// Upstream's `checkboxTheme` / `radioTheme` fill resolver: no override
    /// when disabled, primary when selected, no override otherwise.
    pub fn selection_fill_color(disabled: bool, selected: bool) -> Option<Color> {
        if disabled {
            return None;
        }
        if selected {
            Some(COLOR_SCHEME.primary)
        } else {
            None
        }
    }

    /// Upstream's `switchTheme` thumb resolver, the same rule as
    /// [`selection_fill_color`](Self::selection_fill_color).
    pub fn switch_thumb_color(disabled: bool, selected: bool) -> Option<Color> {
        Self::selection_fill_color(disabled, selected)
    }

    /// Upstream's `switchTheme` track resolver: primary at half alpha.
    pub fn switch_track_color(disabled: bool, selected: bool) -> Option<Color> {
        if disabled {
            return None;
        }
        if selected {
            Some(COLOR_SCHEME.primary.with_alpha(0x80))
        } else {
            None
        }
    }

    /// Upstream's `snackBarTheme`: snackbars float.
    pub const SNACK_BAR_FLOATING: bool = true;

    /// Upstream passes `GalleryOptions.platform` through to the demo theme's
    /// typography. Typography is not carried (see the module header), so the
    /// platform has nothing to act on here; the parameter exists so the call
    /// shape is upstream's.
    pub fn theme_data_for_platform(_platform: Option<TargetPlatform>) -> Theme {
        Self::theme_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scheme_is_upstreams() {
        assert_eq!(COLOR_SCHEME.primary, Color(0xFF6200EE));
        assert_eq!(COLOR_SCHEME.secondary, Color(0xFFFF5722));
        assert_eq!(COLOR_SCHEME.background, Color::WHITE);
        assert_eq!(COLOR_SCHEME.surface, Color(0xFFF2F2F2));
    }

    #[test]
    fn the_theme_maps_the_scheme_the_way_the_gallerys_does() {
        let theme = MaterialDemoThemeData::theme_data();
        assert_eq!(theme.background, COLOR_SCHEME.background);
        assert_eq!(theme.primary, COLOR_SCHEME.primary);
        assert_eq!(theme.text, COLOR_SCHEME.on_surface);
    }

    #[test]
    fn the_selection_resolvers_follow_upstreams_rules() {
        // Disabled wins first: upstream returns null before looking at
        // selected, and null means "no override".
        assert_eq!(
            MaterialDemoThemeData::selection_fill_color(true, true),
            None
        );
        assert_eq!(
            MaterialDemoThemeData::selection_fill_color(false, true),
            Some(COLOR_SCHEME.primary)
        );
        assert_eq!(
            MaterialDemoThemeData::selection_fill_color(false, false),
            None
        );
        assert_eq!(
            MaterialDemoThemeData::switch_track_color(false, true),
            Some(COLOR_SCHEME.primary.with_alpha(0x80))
        );
    }
}
