// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/shrine/theme.dart` (flutter/gallery @ d12640d):
//! the Shrine `ThemeData` builders, mapped onto the framework's `Theme`
//! (which is a flat bag of colours and metrics rather than a `ColorScheme`
//! plus two `TextTheme`s), plus the letter-spacing constants and the text
//! styles the pages set their text in.
//!
//! Upstream sets the text theme in Rubik through `GoogleFonts.rubikTextTheme`;
//! Rubik ships with `flutter_gallery_assets`, and the three weights the study
//! uses (400/500, plus 600/700 synthesised from Bold) are registered by
//! [`register_fonts`]. The study provides this theme at its own root
//! (`app.rs`), so it no longer shares the gallery's theme (the
//! "studies-share-gallery-theme" divergence in PORTING.md is closed for
//! Shrine). Upstream's theme is light-only -- `_buildShrineTheme` starts from
//! `ThemeData.light()` regardless of the app's brightness -- so this one is
//! too.

use rustflutter::prelude::*;

use super::colors::*;

/// Upstream's `defaultLetterSpacing`.
pub const DEFAULT_LETTER_SPACING: f32 = 0.03;
/// Upstream's `mediumLetterSpacing`.
pub const MEDIUM_LETTER_SPACING: f32 = 0.04;
/// Upstream's `largeLetterSpacing`.
pub const LARGE_LETTER_SPACING: f32 = 1.0;

/// The family the text styles below are set in.
pub const RUBIK: &str = "Rubik";

const RUBIK_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Rubik-Regular.ttf");
const RUBIK_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Rubik-Medium.ttf");
const RUBIK_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Rubik-Bold.ttf");

/// Registers Rubik, once. The study is the only user, so the registration
/// lives here rather than in the gallery-wide `register_fonts`; an
/// unregistered family falls back to a system face, which is not Shrine.
pub fn register_fonts() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        rustflutter::engine::register_font(RUBIK_REGULAR, RUBIK);
        rustflutter::engine::register_font(RUBIK_MEDIUM, RUBIK);
        rustflutter::engine::register_font(RUBIK_BOLD, RUBIK);
    });
}

/// Upstream's `shrineTheme` (`_buildShrineTheme`), as the framework's flatter
/// `Theme`. The `ColorScheme` maps role by role: `primary` is pink 100,
/// `onPrimary` brown 900, `surface` the surface white, `background` white,
/// `error` the error red.
pub fn shrine_theme() -> Theme {
    Theme {
        background: SHRINE_BACKGROUND_WHITE,
        surface: SHRINE_SURFACE_WHITE,
        // The cart sheet's pink; the role upstream calls `secondary`.
        surface_variant: SHRINE_PINK_50,
        outline: SHRINE_BROWN_900,
        primary: SHRINE_PINK_100,
        on_primary: SHRINE_BROWN_900,
        danger: SHRINE_ERROR_RED,
        text: SHRINE_BROWN_900,
        text_muted: SHRINE_BROWN_600,
        // The cut-corner buttons cut 7; the framework's one radius is the
        // closest it has to a bevel.
        radius: 7.0,
        spacing: 8.0,
        body_size: 14.0,
        title_size: 24.0,
        font_family: Some(RUBIK),
    }
}

fn style(size: f32, weight: i32, letter_spacing: f32) -> TextStyle {
    TextStyle {
        font_family: Some(RUBIK.to_string()),
        font_size: size,
        font_weight: weight,
        letter_spacing: Some(letter_spacing),
        color: SHRINE_BROWN_900,
        ..TextStyle::default()
    }
}

/// `textTheme.headlineSmall`: 24, w500, default letter spacing. The "SHRINE"
/// wordmark is set in this.
pub fn headline_small() -> TextStyle {
    style(24.0, 500, DEFAULT_LETTER_SPACING)
}

/// `primaryTextTheme.titleLarge`: 18, default letter spacing. The backdrop
/// title is set in this.
pub fn title_large() -> TextStyle {
    style(18.0, 400, DEFAULT_LETTER_SPACING)
}

/// `textTheme.titleMedium`: 16, default letter spacing.
pub fn title_medium() -> TextStyle {
    style(16.0, 500, DEFAULT_LETTER_SPACING)
}

/// `textTheme.headlineMedium`: 28, used by the cart summary's total.
pub fn headline_medium() -> TextStyle {
    style(28.0, 400, MEDIUM_LETTER_SPACING)
}

/// `textTheme.bodyLarge`: 16, w500. The category menu's entries are set in
/// this, resized to 17/19 by the menu itself.
pub fn body_large() -> TextStyle {
    style(16.0, 500, DEFAULT_LETTER_SPACING)
}

/// `textTheme.bodyMedium`: 14, default letter spacing.
pub fn body_medium() -> TextStyle {
    style(14.0, 400, DEFAULT_LETTER_SPACING)
}

/// `textTheme.bodySmall`: 14, w400. Product card prices are set in this.
pub fn body_small() -> TextStyle {
    style(14.0, 400, DEFAULT_LETTER_SPACING)
}

/// `textTheme.labelLarge`: 14, w500. Product card names are set in this.
pub fn label_large() -> TextStyle {
    style(14.0, 500, DEFAULT_LETTER_SPACING)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_maps_the_color_scheme_roles() {
        let theme = shrine_theme();
        assert_eq!(theme.primary, SHRINE_PINK_100);
        assert_eq!(theme.on_primary, SHRINE_BROWN_900);
        assert_eq!(theme.surface, SHRINE_SURFACE_WHITE);
        assert_eq!(theme.background, SHRINE_BACKGROUND_WHITE);
        assert_eq!(theme.danger, SHRINE_ERROR_RED);
        assert_eq!(theme.text, SHRINE_BROWN_900);
        assert_eq!(theme.text_muted, SHRINE_BROWN_600);
        assert_eq!(theme.font_family, Some(RUBIK));
    }

    #[test]
    fn the_letter_spacings_are_upstreams() {
        assert_eq!(DEFAULT_LETTER_SPACING, 0.03);
        assert_eq!(MEDIUM_LETTER_SPACING, 0.04);
        assert_eq!(LARGE_LETTER_SPACING, 1.0);
    }

    #[test]
    fn the_text_styles_are_set_in_rubik_on_brown() {
        for style in [
            headline_small(),
            title_large(),
            title_medium(),
            headline_medium(),
            body_large(),
            body_medium(),
            body_small(),
            label_large(),
        ] {
            assert_eq!(style.font_family.as_deref(), Some(RUBIK));
            assert_eq!(style.color, SHRINE_BROWN_900);
        }
        // Upstream's sizes, `theme.dart`'s `_buildShrineTextTheme`.
        assert_eq!(headline_small().font_weight, 500);
        assert_eq!(title_large().font_size, 18.0);
        assert_eq!(body_small().font_size, 14.0);
        assert_eq!(body_large().font_size, 16.0);
        assert_eq!(label_large().font_weight, 500);
    }
}
