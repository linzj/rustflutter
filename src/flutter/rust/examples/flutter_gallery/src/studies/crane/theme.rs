// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/theme.dart` (flutter/gallery @ d12640d):
//! the Crane `ThemeData`.
//!
//! Upstream builds on `ThemeData.light()` unconditionally -- Crane has no
//! dark variant, so it does not follow the app's brightness. The framework's
//! [`Theme`] is a flatter thing than `ThemeData`: the roles that exist are
//! mapped one-to-one (primary, on-primary, error → danger, the grey text),
//! and `colorScheme.secondary` (`craneRed700`) and the text-selection colour
//! (`cranePurple700`) have no counterpart field and are read from
//! [`crate::studies::crane::colors`] directly at the sites that need them.
//! The type scale collapses the same way: `titleMedium`/`bodyLarge` are both
//! 16, which is what `body_size`/`title_size` carry.
//!
//! The face is Raleway, upstream's `GoogleFonts.ralewayTextTheme`: the four
//! weights the text theme asks for (Light 300, Regular 400, Medium 500,
//! SemiBold 600) ship with `flutter_gallery_assets` and are registered from
//! here, the way the gallery registers Montserrat and Oswald from
//! `src/data/demos.rs`.

use rustflutter::components::Theme;
use rustflutter::prelude::*;

use super::colors;

/// The family Crane sets everything in.
pub const RALEWAY: &str = "Raleway";

/// The four weights upstream's text theme uses, from
/// `flutter_gallery_assets`' `fonts/google_fonts/`.
const RALEWAY_FONTS: &[&[u8]] = &[
    include_bytes!("../../../assets/fonts/Raleway-Light.ttf"),
    include_bytes!("../../../assets/fonts/Raleway-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Raleway-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Raleway-SemiBold.ttf"),
];

/// Registers Raleway, once. Called by [`crane_theme`], so any screen that
/// takes the theme takes the face with it.
pub fn ensure_fonts_registered() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for data in RALEWAY_FONTS {
            rustflutter::engine::register_font(data, RALEWAY);
        }
    });
}

/// Upstream's `craneTheme`: light-based, white surfaces on purple.
pub fn crane_theme() -> Theme {
    ensure_fonts_registered();
    Theme {
        // `scaffoldBackgroundColor` and `cardColor`.
        background: colors::CRANE_PRIMARY_WHITE,
        surface: colors::CRANE_PRIMARY_WHITE,
        // The text-field fill, upstream's `InputDecoration.fillColor`.
        surface_variant: colors::CRANE_PURPLE_700,
        // Dividers on the white front layer read as upstream's default
        // `dividerColor`, black at 12%.
        outline: Color::argb(0x1F, 0x00, 0x00, 0x00),
        primary: colors::CRANE_PURPLE_800,
        on_primary: colors::CRANE_PRIMARY_WHITE,
        danger: colors::CRANE_ERROR_ORANGE,
        text: colors::CRANE_BLACK,
        // Upstream's `titleSmall`/`bodySmall` colour.
        text_muted: colors::CRANE_GREY,
        // Crane's cards and fields are 4, not the gallery default's 12.
        radius: 4.0,
        spacing: 8.0,
        body_size: 16.0,
        title_size: 16.0,
        font_family: Some(RALEWAY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_is_light_and_purple() {
        let theme = crane_theme();
        assert_eq!(theme.primary, colors::CRANE_PURPLE_800);
        assert_eq!(theme.surface, colors::CRANE_PRIMARY_WHITE);
        assert_eq!(theme.background, colors::CRANE_PRIMARY_WHITE);
        assert_eq!(theme.on_primary, colors::CRANE_PRIMARY_WHITE);
        assert_eq!(theme.danger, colors::CRANE_ERROR_ORANGE);
        assert_eq!(theme.text_muted, colors::CRANE_GREY);
        assert_eq!(theme.font_family, Some(RALEWAY));
        // Registering twice must be safe -- the Once is what makes a theme
        // built per frame affordable.
        let again = crane_theme();
        assert_eq!(theme, again);
    }
}
