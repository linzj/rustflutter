// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The gallery's colours, taken from upstream's `ColorScheme`.
//!
//! Ported from `new_gallery/lib/themes/gallery_theme_data.dart`. The framework's
//! own [`rustflutter::components::Theme`] is a smaller thing than a Material
//! `ColorScheme` -- it has the handful of roles the component library actually
//! reads -- so this module holds the full scheme and hands the framework the
//! part it understands.
//!
//! The type faces are not ported. Upstream draws in Montserrat and Oswald,
//! fetched at runtime by `google_fonts`; there is no font fetching here and
//! neither face ships with the engine, so the text is shaped in the system
//! font. That is the one deliberate visual difference on every screen.

use rustflutter::components::Theme;
use rustflutter::engine::Color;

/// Upstream's `ColorScheme`, in full.
///
/// Kept whole rather than folded into [`Theme`] because the home page reads
/// roles the component library has no name for: `primary_container` tints the
/// two big headers, and `on_background` is the category header's fill.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // Upstream's complete ColorScheme, roles included that no
                    // screen reads yet.
pub struct Scheme {
    pub primary: Color,
    pub primary_container: Color,
    pub secondary: Color,
    pub secondary_container: Color,
    pub background: Color,
    pub surface: Color,
    /// White at 5% on dark, opaque white on light. Upstream's category header
    /// fill, which is why it is a background-ish colour with a foreground name.
    pub on_background: Color,
    pub on_primary: Color,
    pub on_secondary: Color,
    pub on_surface: Color,
    pub is_dark: bool,
}

impl Scheme {
    pub fn dark() -> Scheme {
        Scheme {
            primary: Color(0xFFFF8383),
            primary_container: Color(0xFF1CDEC9),
            secondary: Color(0xFF4D1F7C),
            secondary_container: Color(0xFF451B6F),
            background: Color(0xFF241E30),
            surface: Color(0xFF1F1929),
            on_background: Color(0x0DFFFFFF),
            on_primary: Color::WHITE,
            on_secondary: Color::WHITE,
            on_surface: Color::WHITE,
            is_dark: true,
        }
    }

    pub fn light() -> Scheme {
        Scheme {
            primary: Color(0xFFB93C5D),
            primary_container: Color(0xFF117378),
            secondary: Color(0xFFEFF3F3),
            secondary_container: Color(0xFFFAFBFB),
            background: Color(0xFFE6EBEB),
            surface: Color(0xFFFAFBFB),
            on_background: Color::WHITE,
            on_primary: Color::BLACK,
            on_secondary: Color(0xFF322942),
            on_surface: Color(0xFF241E30),
            is_dark: false,
        }
    }

    /// `on_surface`, faded. Upstream writes demo subtitles at half opacity
    /// rather than in a second colour.
    pub fn muted(self) -> Color {
        self.on_surface.with_alpha(0x80)
    }

    /// The component library's view of this scheme.
    ///
    /// The mapping is lossy in one direction on purpose: `surface_variant` has
    /// no counterpart upstream, so it takes the secondary, and `outline` takes
    /// a faint fill rather than a border colour -- upstream draws separators,
    /// not outlines.
    pub fn theme(self) -> Theme {
        Theme {
            background: self.background,
            surface: self.surface,
            surface_variant: if self.is_dark { Color(0xFF2C2438) } else { self.secondary },
            outline: self.on_surface.with_alpha(0x1F),
            primary: self.primary,
            on_primary: self.on_primary,
            danger: self.primary,
            text: self.on_surface,
            text_muted: self.muted(),
            radius: 10.0,
            spacing: 8.0,
            // Upstream's bodyLarge and titleLarge.
            body_size: 14.0,
            title_size: 16.0,
        }
    }
}

/// Upstream's text sizes and weights, by the names Material gives them.
///
/// Only the ones the gallery actually uses. The faces differ -- see the module
/// comment -- but the sizes and weights are upstream's.
#[allow(dead_code)] // Upstream's complete set of roles.
pub mod text {
    /// Montserrat 20 bold. The "Gallery" and "Categories" headers.
    pub const HEADLINE_MEDIUM: (f32, i32) = (20.0, 700);
    /// Oswald 16 medium. A category's own header.
    pub const HEADLINE_SMALL: (f32, i32) = (16.0, 500);
    /// Oswald 16 semibold. A carousel card's title.
    pub const BODY_SMALL: (f32, i32) = (16.0, 600);
    /// Montserrat 16 medium. A demo row's title.
    pub const TITLE_MEDIUM: (f32, i32) = (16.0, 500);
    /// Montserrat 16 bold.
    pub const TITLE_LARGE: (f32, i32) = (16.0, 700);
    /// Montserrat 12 medium. Subtitles, and a carousel card's second line.
    pub const LABEL_SMALL: (f32, i32) = (12.0, 500);
    /// Montserrat 14 regular.
    pub const BODY_LARGE: (f32, i32) = (14.0, 400);
    /// Montserrat 16 regular.
    pub const BODY_MEDIUM: (f32, i32) = (16.0, 400);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scheme_survives_the_trip_into_a_theme() {
        for scheme in [Scheme::dark(), Scheme::light()] {
            let theme = scheme.theme();
            assert_eq!(theme.background, scheme.background);
            assert_eq!(theme.primary, scheme.primary);
            assert_eq!(theme.text, scheme.on_surface);
        }
    }

    #[test]
    fn the_two_schemes_are_actually_different() {
        // A theme switch that changed nothing would still pass every other
        // test in this file.
        assert_ne!(Scheme::dark().background, Scheme::light().background);
        assert_ne!(Scheme::dark().primary, Scheme::light().primary);
    }
}
