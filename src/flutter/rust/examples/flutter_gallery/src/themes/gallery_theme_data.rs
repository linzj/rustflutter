// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The gallery's colours, taken from upstream's `ColorScheme`.
//!
//! Ported from `lib/themes/gallery_theme_data.dart` (flutter/gallery @
//! d12640d). The framework's
//! own [`rustflutter::components::Theme`] is a smaller thing than a Material
//! `ColorScheme` -- it has the handful of roles the component library actually
//! reads -- so this module holds the full scheme and hands the framework the
//! part it understands.
//!
//! The type faces are upstream's too. It draws in Montserrat and Oswald,
//! fetched at runtime through `google_fonts`; both ship inside
//! `flutter_gallery_assets` as well, and that is where the copies in
//! `assets/fonts` came from. Nothing is fetched at runtime here -- there is no
//! network and no asset bundle -- so they are baked in and registered before
//! the first frame.

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
            surface_variant: if self.is_dark {
                Color(0xFF2C2438)
            } else {
                self.secondary
            },
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
            font_family: Some(crate::data::demos::MONTSERRAT),
        }
    }
}

/// Upstream's text roles, by the names Material gives them.
///
/// Face, size and weight all come from `gallery_theme_data.dart`. The weight
/// matters as much as the size: each weight is a separate file, and asking for
/// one that was never registered gets the nearest weight smeared into shape,
/// which reads as a different typeface rather than as a bolder one.
#[allow(dead_code)] // Upstream's complete set of roles.
pub mod text {
    use crate::data::demos::{MONTSERRAT, OSWALD};
    use rustflutter::engine::{Color, TextStyle};

    /// One of upstream's text roles.
    #[derive(Clone, Copy, Debug)]
    pub struct Role {
        pub family: &'static str,
        pub size: f32,
        pub weight: i32,
    }

    impl Role {
        pub fn styled(self, color: Color) -> TextStyle {
            TextStyle {
                font_family: Some(self.family.to_string()),
                font_size: self.size,
                font_weight: self.weight,
                color,
                ..TextStyle::default()
            }
        }
    }

    const fn role(family: &'static str, size: f32, weight: i32) -> Role {
        Role {
            family,
            size,
            weight,
        }
    }

    /// The "Gallery" and "Categories" headers.
    pub const HEADLINE_MEDIUM: Role = role(MONTSERRAT, 20.0, 700);
    /// A category's own header.
    pub const HEADLINE_SMALL: Role = role(OSWALD, 16.0, 500);
    /// A carousel card's title.
    pub const BODY_SMALL: Role = role(OSWALD, 16.0, 600);
    /// A demo row's title.
    pub const TITLE_MEDIUM: Role = role(MONTSERRAT, 16.0, 500);
    pub const TITLE_LARGE: Role = role(MONTSERRAT, 16.0, 700);
    /// Subtitles, and a carousel card's second line.
    pub const LABEL_SMALL: Role = role(MONTSERRAT, 12.0, 500);
    pub const BODY_LARGE: Role = role(MONTSERRAT, 14.0, 400);
    pub const BODY_MEDIUM: Role = role(MONTSERRAT, 16.0, 400);
    pub const LABEL_LARGE: Role = role(MONTSERRAT, 14.0, 600);
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
