// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Making a raised surface look raised in the dark (upstream
//! `material/elevation_overlay.dart`).
//!
//! A shadow is how a raised thing reads on a light background: the surface
//! below it darkens. In the dark that does not work -- there is no darker to
//! go -- so Material lightens the raised surface itself instead, more the
//! higher it is. Material 2 does that with a white overlay and Material 3
//! with a tint of the seed colour; both are here, because upstream keeps
//! both and a theme picks.
//!
//! # Recorded divergences
//!
//! * Upstream's `applyOverlay` and `overlayColor` take a `BuildContext` and
//!   read the theme out of it. They take the pieces here --
//!   [`apply_overlay`](ElevationOverlay::apply_overlay) is handed the two
//!   colours and the flags -- because the arithmetic is what is worth having
//!   and a context adds nothing to it that the caller does not already hold.

use crate::engine::Color;

/// One row of upstream's generated surface-tint table: the opacity of the
/// tint at one of Material 3's six elevation levels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ElevationOpacity {
    elevation: f32,
    opacity: f32,
}

/// Upstream's `_surfaceTintElevationOpacities`, generated from the Material
/// token database. Ordered by increasing elevation, which the lookup relies
/// on.
const SURFACE_TINT_ELEVATION_OPACITIES: [ElevationOpacity; 6] = [
    ElevationOpacity {
        elevation: 0.0,
        opacity: 0.0,
    },
    ElevationOpacity {
        elevation: 1.0,
        opacity: 0.05,
    },
    ElevationOpacity {
        elevation: 3.0,
        opacity: 0.08,
    },
    ElevationOpacity {
        elevation: 6.0,
        opacity: 0.11,
    },
    ElevationOpacity {
        elevation: 8.0,
        opacity: 0.12,
    },
    ElevationOpacity {
        elevation: 12.0,
        opacity: 0.14,
    },
];

/// Upstream `ElevationOverlay`.
pub struct ElevationOverlay;

impl ElevationOverlay {
    /// Upstream `applySurfaceTint`: Material 3's way.
    ///
    /// A transparent tint is the same as none -- upstream checks for both,
    /// and it matters because a theme that cleared its tint by setting it
    /// transparent should not get a blend of nothing.
    pub fn apply_surface_tint(color: Color, surface_tint: Option<Color>, elevation: f32) -> Color {
        match surface_tint {
            Some(tint) if tint != Color::TRANSPARENT => alpha_blend(
                with_opacity(
                    tint,
                    ElevationOverlay::surface_tint_opacity_for_elevation(elevation),
                ),
                color,
            ),
            _ => color,
        }
    }

    /// Upstream's `_surfaceTintOpacityForElevation`: the table, interpolated.
    ///
    /// Below the first entry and above the last it clamps rather than
    /// extrapolating -- an elevation of a hundred is not a hundred times as
    /// tinted, it is as tinted as the table goes.
    pub fn surface_tint_opacity_for_elevation(elevation: f32) -> f32 {
        let table = &SURFACE_TINT_ELEVATION_OPACITIES;
        if elevation < table[0].elevation {
            return table[0].opacity;
        }
        let mut index = 0;
        while elevation >= table[index].elevation {
            if elevation == table[index].elevation || index + 1 == table.len() {
                return table[index].opacity;
            }
            index += 1;
        }
        // Between two rows: straight interpolation, which is what makes the
        // six levels read as a continuum rather than six steps.
        let lower = table[index - 1];
        let upper = table[index];
        let t = (elevation - lower.elevation) / (upper.elevation - lower.elevation);
        lower.opacity + t * (upper.opacity - lower.opacity)
    }

    /// Upstream `applyOverlay`, with the theme's answers passed in.
    ///
    /// Four conditions, and every one of them earns its place. Elevation
    /// above zero, because a flat surface is not raised. The theme opting in,
    /// because Material 3 uses the tint instead. Dark, because in the light a
    /// shadow already says it. And the colour actually being the theme's
    /// surface, because the overlay is defined against that one colour --
    /// applying it to an arbitrary colour would lighten something the
    /// specification says nothing about.
    pub fn apply_overlay(
        color: Color,
        elevation: f32,
        apply_elevation_overlay_color: bool,
        is_dark: bool,
        surface: Color,
        on_surface: Color,
    ) -> Color {
        if elevation > 0.0
            && apply_elevation_overlay_color
            && is_dark
            && color.with_alpha(0xFF) == surface.with_alpha(0xFF)
        {
            return ElevationOverlay::color_with_overlay(color, on_surface, elevation);
        }
        color
    }

    /// Upstream `colorWithOverlay`.
    pub fn color_with_overlay(surface: Color, overlay: Color, elevation: f32) -> Color {
        alpha_blend(ElevationOverlay::overlay_color(overlay, elevation), surface)
    }

    /// Upstream's `_overlayColor`, which upstream notes matches the values in
    /// the Material 2 dark-theme specification.
    ///
    /// A logarithm and not a line: the first millimetre of lift is worth far
    /// more than the twentieth, which is also true of how a shadow reads.
    pub fn overlay_color(color: Color, elevation: f32) -> Color {
        let opacity = (4.5 * (elevation + 1.0).ln() + 2.0) / 100.0;
        with_opacity(color, opacity)
    }
}

/// Upstream `Color.withOpacity`: the same colour at a given alpha, where the
/// alpha is a fraction rather than a byte.
///
/// Out-of-range fractions clamp; upstream asserts instead, and a clamp is the
/// same answer without taking the application down over a rounding error.
pub fn with_opacity(color: Color, opacity: f32) -> Color {
    color.with_alpha((opacity.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Upstream `Color.alphaBlend`: `foreground` composited over `background`.
///
/// The source-over rule. Two cases are worth having by name because they are
/// the common ones and the general formula divides by the result's alpha: an
/// opaque foreground is itself, and a fully transparent one leaves the
/// background alone.
pub fn alpha_blend(foreground: Color, background: Color) -> Color {
    let alpha = foreground.alpha() as u32;
    if alpha == 0xFF {
        return foreground;
    }
    if alpha == 0 {
        return background;
    }
    let inverse = 0xFF - alpha;
    let background_alpha = background.alpha() as u32;
    let out_alpha = alpha * 0xFF + background_alpha * inverse;
    debug_assert!(out_alpha > 0, "a zero foreground alpha returned above");
    let channel = |front: u32, back: u32| -> u8 {
        (((front * 0xFF * alpha) + (back * background_alpha * inverse)) / out_alpha) as u8
    };
    Color::argb(
        (out_alpha / 0xFF) as u8,
        channel(foreground.red() as u32, background.red() as u32),
        channel(foreground.green() as u32, background.green() as u32),
        channel(foreground.blue() as u32, background.blue() as u32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Color = Color(0xFF00_0000);
    const WHITE: Color = Color(0xFFFF_FFFF);

    #[test]
    fn an_opaque_foreground_is_itself_and_a_clear_one_leaves_the_background() {
        // The two cases the general formula cannot take, because it divides
        // by the result's alpha.
        assert_eq!(alpha_blend(WHITE, BLACK), WHITE);
        assert_eq!(alpha_blend(Color(0x0000_0000), BLACK), BLACK);
    }

    #[test]
    fn half_white_over_black_is_grey() {
        // Source-over, which is what every overlay in this file is built on.
        let blended = alpha_blend(with_opacity(WHITE, 0.5), BLACK);
        assert_eq!(blended.alpha(), 0xFF, "over an opaque background");
        // 128/255 of the way from black to white, allowing for the rounding
        // of the opacity into a byte.
        assert!(
            (blended.red() as i32 - 128).abs() <= 1,
            "got {}",
            blended.red()
        );
        assert_eq!(blended.red(), blended.green());
        assert_eq!(blended.red(), blended.blue());
    }

    #[test]
    fn the_surface_tint_table_is_read_exactly_at_its_six_levels() {
        // Material 3's six elevation levels, which is what the generated
        // table is: a wrong number here tints every card in the application
        // by the wrong amount.
        for (elevation, opacity) in [
            (0.0, 0.0),
            (1.0, 0.05),
            (3.0, 0.08),
            (6.0, 0.11),
            (8.0, 0.12),
            (12.0, 0.14),
        ] {
            assert_eq!(
                ElevationOverlay::surface_tint_opacity_for_elevation(elevation),
                opacity,
                "at elevation {elevation}"
            );
        }
    }

    #[test]
    fn between_two_levels_the_tint_is_interpolated() {
        // Which is what makes six levels read as a continuum rather than six
        // steps -- an animated elevation would visibly jump otherwise.
        let two = ElevationOverlay::surface_tint_opacity_for_elevation(2.0);
        assert!(two > 0.05 && two < 0.08, "got {two}");
        // Half way between 1 and 3 is half way between 0.05 and 0.08.
        assert!((two - 0.065).abs() < 1e-6, "got {two}");
    }

    #[test]
    fn outside_the_table_the_tint_clamps_rather_than_extrapolating() {
        // An elevation of a hundred is not a hundred times as tinted; it is
        // as tinted as the table goes. Extrapolating would drive the opacity
        // past one and wash the surface out entirely.
        assert_eq!(
            ElevationOverlay::surface_tint_opacity_for_elevation(100.0),
            0.14
        );
        assert_eq!(
            ElevationOverlay::surface_tint_opacity_for_elevation(-5.0),
            0.0
        );
    }

    #[test]
    fn a_transparent_tint_is_the_same_as_no_tint() {
        // A theme that cleared its tint by setting it transparent should get
        // its colour back untouched, not a blend of nothing.
        let surface = Color(0xFF12_3456);
        assert_eq!(
            ElevationOverlay::apply_surface_tint(surface, None, 6.0),
            surface
        );
        assert_eq!(
            ElevationOverlay::apply_surface_tint(surface, Some(Color::TRANSPARENT), 6.0),
            surface
        );
        assert_ne!(
            ElevationOverlay::apply_surface_tint(surface, Some(WHITE), 6.0),
            surface
        );
    }

    #[test]
    fn the_overlay_grows_with_elevation_but_less_and_less() {
        // A logarithm and not a line: the first millimetre of lift is worth
        // far more than the twentieth, which is also how a shadow reads.
        let at = |elevation: f32| ElevationOverlay::overlay_color(WHITE, elevation).alpha();
        assert!(at(1.0) > at(0.0));
        assert!(at(24.0) > at(1.0));
        let first_step = at(1.0) as i32 - at(0.0) as i32;
        let later_step = at(24.0) as i32 - at(23.0) as i32;
        assert!(
            first_step > later_step,
            "first {first_step}, later {later_step}"
        );
    }

    /// A dark theme's two surface colours.
    const SURFACE: Color = Color(0xFF12_1212);
    const ON_SURFACE: Color = Color(0xFFFF_FFFF);

    #[test]
    fn all_four_conditions_have_to_hold_for_an_overlay() {
        // Each one earns its place, so each one is checked by taking it away.
        let raised = ElevationOverlay::apply_overlay(SURFACE, 4.0, true, true, SURFACE, ON_SURFACE);
        assert_ne!(raised, SURFACE, "the overlay applies");

        // Flat: not raised, so nothing to say.
        assert_eq!(
            ElevationOverlay::apply_overlay(SURFACE, 0.0, true, true, SURFACE, ON_SURFACE),
            SURFACE
        );
        // The theme opted out -- Material 3 uses the tint instead.
        assert_eq!(
            ElevationOverlay::apply_overlay(SURFACE, 4.0, false, true, SURFACE, ON_SURFACE),
            SURFACE
        );
        // Light: a shadow already says it.
        assert_eq!(
            ElevationOverlay::apply_overlay(SURFACE, 4.0, true, false, SURFACE, ON_SURFACE),
            SURFACE
        );
        // Not the theme's surface colour: the overlay is defined against that
        // one colour, and lightening an arbitrary card would be inventing
        // behaviour the specification says nothing about.
        let other = Color(0xFF00_5599);
        assert_eq!(
            ElevationOverlay::apply_overlay(other, 4.0, true, true, SURFACE, ON_SURFACE),
            other
        );
    }

    #[test]
    fn the_surface_is_matched_ignoring_its_own_alpha() {
        // Upstream compares both at full opacity, so a half-transparent
        // surface of the right colour still gets its overlay -- otherwise a
        // card fading in would lose its lift part way through the fade.
        let translucent = Color(0x8012_1212);
        assert_ne!(
            ElevationOverlay::apply_overlay(translucent, 4.0, true, true, SURFACE, ON_SURFACE),
            translucent
        );
    }
}
