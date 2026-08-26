// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The Material colour roles, from upstream `material/color_scheme.dart`.
//!
//! A Material control does not pick a colour; it picks a *role* -- primary,
//! surface, onSurface, outline -- and the scheme says what colour that is.
//! That is what makes one theme swap recolour a whole application, and it is
//! what every widget in the material wave reads its paint through.
//!
//! # Set roles and derived roles
//!
//! Only nine roles are ever set outright: the brightness, and the four pairs
//! (primary, secondary, error, surface) with their `on` colours. Every other
//! role falls back to one of those when it was not given, and the fallbacks
//! are upstream's getters one for one -- `tertiary` falls back to
//! `secondary`, every surface container to `surface`, `outline` to
//! `onBackground` (which itself falls back to `onSurface`).
//!
//! Keeping them as `Option`s rather than resolving once is what makes
//! [`ColorScheme::with_primary`] and friends behave: a scheme whose primary
//! is changed and whose primary container was never set gets a container
//! that follows the new primary, exactly as upstream's `copyWith` does.
//!
//! # Recorded divergences
//!
//! * `ColorScheme.fromSeed` and `fromImageProvider` are not here. Both run
//!   the Material 3 tonal-palette algorithm (HCT and CAM16, upstream's
//!   vendored `material_color_utilities`), which is its own body of work.
//!   Until it lands, a scheme is written out role by role, which is what
//!   [`ColorScheme::light`] and [`ColorScheme::dark`] do.
//! * `background`, `onBackground` and `surfaceVariant` are deprecated
//!   upstream in favour of `surface`, `onSurface` and
//!   `surfaceContainerHighest`. They are kept, because upstream keeps them
//!   and because `outline` still falls back through `onBackground`.

use crate::animation::{ColorTween, Tween};
use crate::engine::Color;
use crate::platform::Brightness;

/// Upstream `ColorScheme`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorScheme {
    pub brightness: Brightness,
    /// The colour of the most prominent parts of the interface.
    pub primary: Color,
    /// What is legible on top of [`ColorScheme::primary`].
    pub on_primary: Color,
    pub secondary: Color,
    pub on_secondary: Color,
    pub error: Color,
    pub on_error: Color,
    pub surface: Color,
    pub on_surface: Color,
    primary_container: Option<Color>,
    on_primary_container: Option<Color>,
    primary_fixed: Option<Color>,
    primary_fixed_dim: Option<Color>,
    on_primary_fixed: Option<Color>,
    on_primary_fixed_variant: Option<Color>,
    secondary_container: Option<Color>,
    on_secondary_container: Option<Color>,
    secondary_fixed: Option<Color>,
    secondary_fixed_dim: Option<Color>,
    on_secondary_fixed: Option<Color>,
    on_secondary_fixed_variant: Option<Color>,
    tertiary: Option<Color>,
    on_tertiary: Option<Color>,
    tertiary_container: Option<Color>,
    on_tertiary_container: Option<Color>,
    tertiary_fixed: Option<Color>,
    tertiary_fixed_dim: Option<Color>,
    on_tertiary_fixed: Option<Color>,
    on_tertiary_fixed_variant: Option<Color>,
    error_container: Option<Color>,
    on_error_container: Option<Color>,
    surface_variant: Option<Color>,
    surface_dim: Option<Color>,
    surface_bright: Option<Color>,
    surface_container_lowest: Option<Color>,
    surface_container_low: Option<Color>,
    surface_container: Option<Color>,
    surface_container_high: Option<Color>,
    surface_container_highest: Option<Color>,
    on_surface_variant: Option<Color>,
    outline: Option<Color>,
    outline_variant: Option<Color>,
    shadow: Option<Color>,
    scrim: Option<Color>,
    inverse_surface: Option<Color>,
    on_inverse_surface: Option<Color>,
    inverse_primary: Option<Color>,
    surface_tint: Option<Color>,
    background: Option<Color>,
    on_background: Option<Color>,
}

impl ColorScheme {
    /// Upstream `ColorScheme.light`: the baseline Material 2 light scheme.
    ///
    /// Upstream's own documentation says not to use this one for Material 3
    /// -- there `fromSeed` is the way in. It is here because it is the
    /// constructor with values in it, and because a scheme written out by
    /// hand starts from one.
    pub const fn light() -> ColorScheme {
        ColorScheme {
            brightness: Brightness::Light,
            primary: Color(0xff6200ee),
            on_primary: Color::WHITE,
            secondary: Color(0xff03dac6),
            on_secondary: Color::BLACK,
            error: Color(0xffb00020),
            on_error: Color::WHITE,
            surface: Color::WHITE,
            on_surface: Color::BLACK,
            ..ColorScheme::UNSET
        }
    }

    /// Upstream `ColorScheme.dark`: the baseline Material 2 dark scheme.
    pub const fn dark() -> ColorScheme {
        ColorScheme {
            brightness: Brightness::Dark,
            primary: Color(0xffbb86fc),
            on_primary: Color::BLACK,
            secondary: Color(0xff03dac6),
            on_secondary: Color::BLACK,
            error: Color(0xffcf6679),
            on_error: Color::BLACK,
            surface: Color(0xff121212),
            on_surface: Color::WHITE,
            ..ColorScheme::UNSET
        }
    }

    /// Every derived role unset, so that each falls back to the one it
    /// follows. The nine set roles here are placeholders the constructors
    /// above overwrite.
    const UNSET: ColorScheme = ColorScheme {
        brightness: Brightness::Light,
        primary: Color::BLACK,
        on_primary: Color::BLACK,
        secondary: Color::BLACK,
        on_secondary: Color::BLACK,
        error: Color::BLACK,
        on_error: Color::BLACK,
        surface: Color::BLACK,
        on_surface: Color::BLACK,
        primary_container: None,
        on_primary_container: None,
        primary_fixed: None,
        primary_fixed_dim: None,
        on_primary_fixed: None,
        on_primary_fixed_variant: None,
        secondary_container: None,
        on_secondary_container: None,
        secondary_fixed: None,
        secondary_fixed_dim: None,
        on_secondary_fixed: None,
        on_secondary_fixed_variant: None,
        tertiary: None,
        on_tertiary: None,
        tertiary_container: None,
        on_tertiary_container: None,
        tertiary_fixed: None,
        tertiary_fixed_dim: None,
        on_tertiary_fixed: None,
        on_tertiary_fixed_variant: None,
        error_container: None,
        on_error_container: None,
        surface_variant: None,
        surface_dim: None,
        surface_bright: None,
        surface_container_lowest: None,
        surface_container_low: None,
        surface_container: None,
        surface_container_high: None,
        surface_container_highest: None,
        on_surface_variant: None,
        outline: None,
        outline_variant: None,
        shadow: None,
        scrim: None,
        inverse_surface: None,
        on_inverse_surface: None,
        inverse_primary: None,
        surface_tint: None,
        background: None,
        on_background: None,
    };

    /// Upstream `primaryContainer`.
    pub fn primary_container(&self) -> Color {
        self.primary_container.unwrap_or_else(|| self.primary)
    }

    /// Upstream `onPrimaryContainer`.
    pub fn on_primary_container(&self) -> Color {
        self.on_primary_container.unwrap_or_else(|| self.on_primary)
    }

    /// Upstream `primaryFixed`.
    pub fn primary_fixed(&self) -> Color {
        self.primary_fixed.unwrap_or_else(|| self.primary)
    }

    /// Upstream `primaryFixedDim`.
    pub fn primary_fixed_dim(&self) -> Color {
        self.primary_fixed_dim.unwrap_or_else(|| self.primary)
    }

    /// Upstream `onPrimaryFixed`.
    pub fn on_primary_fixed(&self) -> Color {
        self.on_primary_fixed.unwrap_or_else(|| self.on_primary)
    }

    /// Upstream `onPrimaryFixedVariant`.
    pub fn on_primary_fixed_variant(&self) -> Color {
        self.on_primary_fixed_variant
            .unwrap_or_else(|| self.on_primary)
    }

    /// Upstream `secondaryContainer`.
    pub fn secondary_container(&self) -> Color {
        self.secondary_container.unwrap_or_else(|| self.secondary)
    }

    /// Upstream `onSecondaryContainer`.
    pub fn on_secondary_container(&self) -> Color {
        self.on_secondary_container
            .unwrap_or_else(|| self.on_secondary)
    }

    /// Upstream `secondaryFixed`.
    pub fn secondary_fixed(&self) -> Color {
        self.secondary_fixed.unwrap_or_else(|| self.secondary)
    }

    /// Upstream `secondaryFixedDim`.
    pub fn secondary_fixed_dim(&self) -> Color {
        self.secondary_fixed_dim.unwrap_or_else(|| self.secondary)
    }

    /// Upstream `onSecondaryFixed`.
    pub fn on_secondary_fixed(&self) -> Color {
        self.on_secondary_fixed.unwrap_or_else(|| self.on_secondary)
    }

    /// Upstream `onSecondaryFixedVariant`.
    pub fn on_secondary_fixed_variant(&self) -> Color {
        self.on_secondary_fixed_variant
            .unwrap_or_else(|| self.on_secondary)
    }

    /// Upstream `tertiary`.
    pub fn tertiary(&self) -> Color {
        self.tertiary.unwrap_or_else(|| self.secondary)
    }

    /// Upstream `onTertiary`.
    pub fn on_tertiary(&self) -> Color {
        self.on_tertiary.unwrap_or_else(|| self.on_secondary)
    }

    /// Upstream `tertiaryContainer`.
    pub fn tertiary_container(&self) -> Color {
        self.tertiary_container.unwrap_or_else(|| self.tertiary())
    }

    /// Upstream `onTertiaryContainer`.
    pub fn on_tertiary_container(&self) -> Color {
        self.on_tertiary_container
            .unwrap_or_else(|| self.on_tertiary())
    }

    /// Upstream `tertiaryFixed`.
    pub fn tertiary_fixed(&self) -> Color {
        self.tertiary_fixed.unwrap_or_else(|| self.tertiary())
    }

    /// Upstream `tertiaryFixedDim`.
    pub fn tertiary_fixed_dim(&self) -> Color {
        self.tertiary_fixed_dim.unwrap_or_else(|| self.tertiary())
    }

    /// Upstream `onTertiaryFixed`.
    pub fn on_tertiary_fixed(&self) -> Color {
        self.on_tertiary_fixed.unwrap_or_else(|| self.on_tertiary())
    }

    /// Upstream `onTertiaryFixedVariant`.
    pub fn on_tertiary_fixed_variant(&self) -> Color {
        self.on_tertiary_fixed_variant
            .unwrap_or_else(|| self.on_tertiary())
    }

    /// Upstream `errorContainer`.
    pub fn error_container(&self) -> Color {
        self.error_container.unwrap_or_else(|| self.error)
    }

    /// Upstream `onErrorContainer`.
    pub fn on_error_container(&self) -> Color {
        self.on_error_container.unwrap_or_else(|| self.on_error)
    }

    /// Upstream `surfaceVariant`.
    pub fn surface_variant(&self) -> Color {
        self.surface_variant.unwrap_or_else(|| self.surface)
    }

    /// Upstream `surfaceDim`.
    pub fn surface_dim(&self) -> Color {
        self.surface_dim.unwrap_or_else(|| self.surface)
    }

    /// Upstream `surfaceBright`.
    pub fn surface_bright(&self) -> Color {
        self.surface_bright.unwrap_or_else(|| self.surface)
    }

    /// Upstream `surfaceContainerLowest`.
    pub fn surface_container_lowest(&self) -> Color {
        self.surface_container_lowest
            .unwrap_or_else(|| self.surface)
    }

    /// Upstream `surfaceContainerLow`.
    pub fn surface_container_low(&self) -> Color {
        self.surface_container_low.unwrap_or_else(|| self.surface)
    }

    /// Upstream `surfaceContainer`.
    pub fn surface_container(&self) -> Color {
        self.surface_container.unwrap_or_else(|| self.surface)
    }

    /// Upstream `surfaceContainerHigh`.
    pub fn surface_container_high(&self) -> Color {
        self.surface_container_high.unwrap_or_else(|| self.surface)
    }

    /// Upstream `surfaceContainerHighest`.
    pub fn surface_container_highest(&self) -> Color {
        self.surface_container_highest
            .unwrap_or_else(|| self.surface)
    }

    /// Upstream `onSurfaceVariant`.
    pub fn on_surface_variant(&self) -> Color {
        self.on_surface_variant.unwrap_or_else(|| self.on_surface)
    }

    /// Upstream `outline`.
    pub fn outline(&self) -> Color {
        self.outline.unwrap_or_else(|| self.on_background())
    }

    /// Upstream `outlineVariant`.
    pub fn outline_variant(&self) -> Color {
        self.outline_variant.unwrap_or_else(|| self.on_background())
    }

    /// Upstream `shadow`.
    pub fn shadow(&self) -> Color {
        self.shadow.unwrap_or_else(|| Color::BLACK)
    }

    /// Upstream `scrim`.
    pub fn scrim(&self) -> Color {
        self.scrim.unwrap_or_else(|| Color::BLACK)
    }

    /// Upstream `inverseSurface`.
    pub fn inverse_surface(&self) -> Color {
        self.inverse_surface.unwrap_or_else(|| self.on_surface)
    }

    /// Upstream `onInverseSurface`.
    pub fn on_inverse_surface(&self) -> Color {
        self.on_inverse_surface.unwrap_or_else(|| self.surface)
    }

    /// Upstream `inversePrimary`.
    pub fn inverse_primary(&self) -> Color {
        self.inverse_primary.unwrap_or_else(|| self.on_primary)
    }

    /// Upstream `surfaceTint`.
    pub fn surface_tint(&self) -> Color {
        self.surface_tint.unwrap_or_else(|| self.primary)
    }

    /// Upstream `background`.
    pub fn background(&self) -> Color {
        self.background.unwrap_or_else(|| self.surface)
    }

    /// Upstream `onBackground`.
    pub fn on_background(&self) -> Color {
        self.on_background.unwrap_or_else(|| self.on_surface)
    }

    /// Upstream `copyWith`, one role at a time: this role set outright and
    /// every other role as it was -- including the ones that were following a
    /// fallback, which go on following it.
    pub fn with_brightness(mut self, brightness: Brightness) -> ColorScheme {
        self.brightness = brightness;
        self
    }

    pub fn with_primary(mut self, primary: Color) -> ColorScheme {
        self.primary = primary;
        self
    }

    pub fn with_on_primary(mut self, on_primary: Color) -> ColorScheme {
        self.on_primary = on_primary;
        self
    }

    pub fn with_secondary(mut self, secondary: Color) -> ColorScheme {
        self.secondary = secondary;
        self
    }

    pub fn with_on_secondary(mut self, on_secondary: Color) -> ColorScheme {
        self.on_secondary = on_secondary;
        self
    }

    pub fn with_error(mut self, error: Color) -> ColorScheme {
        self.error = error;
        self
    }

    pub fn with_on_error(mut self, on_error: Color) -> ColorScheme {
        self.on_error = on_error;
        self
    }

    pub fn with_surface(mut self, surface: Color) -> ColorScheme {
        self.surface = surface;
        self
    }

    pub fn with_on_surface(mut self, on_surface: Color) -> ColorScheme {
        self.on_surface = on_surface;
        self
    }

    /// Sets `primaryContainer` outright, so it stops following its fallback.
    pub fn with_primary_container(mut self, primary_container: Color) -> ColorScheme {
        self.primary_container = Some(primary_container);
        self
    }

    /// Sets `onPrimaryContainer` outright, so it stops following its fallback.
    pub fn with_on_primary_container(mut self, on_primary_container: Color) -> ColorScheme {
        self.on_primary_container = Some(on_primary_container);
        self
    }

    /// Sets `primaryFixed` outright, so it stops following its fallback.
    pub fn with_primary_fixed(mut self, primary_fixed: Color) -> ColorScheme {
        self.primary_fixed = Some(primary_fixed);
        self
    }

    /// Sets `primaryFixedDim` outright, so it stops following its fallback.
    pub fn with_primary_fixed_dim(mut self, primary_fixed_dim: Color) -> ColorScheme {
        self.primary_fixed_dim = Some(primary_fixed_dim);
        self
    }

    /// Sets `onPrimaryFixed` outright, so it stops following its fallback.
    pub fn with_on_primary_fixed(mut self, on_primary_fixed: Color) -> ColorScheme {
        self.on_primary_fixed = Some(on_primary_fixed);
        self
    }

    /// Sets `onPrimaryFixedVariant` outright, so it stops following its fallback.
    pub fn with_on_primary_fixed_variant(mut self, on_primary_fixed_variant: Color) -> ColorScheme {
        self.on_primary_fixed_variant = Some(on_primary_fixed_variant);
        self
    }

    /// Sets `secondaryContainer` outright, so it stops following its fallback.
    pub fn with_secondary_container(mut self, secondary_container: Color) -> ColorScheme {
        self.secondary_container = Some(secondary_container);
        self
    }

    /// Sets `onSecondaryContainer` outright, so it stops following its fallback.
    pub fn with_on_secondary_container(mut self, on_secondary_container: Color) -> ColorScheme {
        self.on_secondary_container = Some(on_secondary_container);
        self
    }

    /// Sets `secondaryFixed` outright, so it stops following its fallback.
    pub fn with_secondary_fixed(mut self, secondary_fixed: Color) -> ColorScheme {
        self.secondary_fixed = Some(secondary_fixed);
        self
    }

    /// Sets `secondaryFixedDim` outright, so it stops following its fallback.
    pub fn with_secondary_fixed_dim(mut self, secondary_fixed_dim: Color) -> ColorScheme {
        self.secondary_fixed_dim = Some(secondary_fixed_dim);
        self
    }

    /// Sets `onSecondaryFixed` outright, so it stops following its fallback.
    pub fn with_on_secondary_fixed(mut self, on_secondary_fixed: Color) -> ColorScheme {
        self.on_secondary_fixed = Some(on_secondary_fixed);
        self
    }

    /// Sets `onSecondaryFixedVariant` outright, so it stops following its fallback.
    pub fn with_on_secondary_fixed_variant(
        mut self,
        on_secondary_fixed_variant: Color,
    ) -> ColorScheme {
        self.on_secondary_fixed_variant = Some(on_secondary_fixed_variant);
        self
    }

    /// Sets `tertiary` outright, so it stops following its fallback.
    pub fn with_tertiary(mut self, tertiary: Color) -> ColorScheme {
        self.tertiary = Some(tertiary);
        self
    }

    /// Sets `onTertiary` outright, so it stops following its fallback.
    pub fn with_on_tertiary(mut self, on_tertiary: Color) -> ColorScheme {
        self.on_tertiary = Some(on_tertiary);
        self
    }

    /// Sets `tertiaryContainer` outright, so it stops following its fallback.
    pub fn with_tertiary_container(mut self, tertiary_container: Color) -> ColorScheme {
        self.tertiary_container = Some(tertiary_container);
        self
    }

    /// Sets `onTertiaryContainer` outright, so it stops following its fallback.
    pub fn with_on_tertiary_container(mut self, on_tertiary_container: Color) -> ColorScheme {
        self.on_tertiary_container = Some(on_tertiary_container);
        self
    }

    /// Sets `tertiaryFixed` outright, so it stops following its fallback.
    pub fn with_tertiary_fixed(mut self, tertiary_fixed: Color) -> ColorScheme {
        self.tertiary_fixed = Some(tertiary_fixed);
        self
    }

    /// Sets `tertiaryFixedDim` outright, so it stops following its fallback.
    pub fn with_tertiary_fixed_dim(mut self, tertiary_fixed_dim: Color) -> ColorScheme {
        self.tertiary_fixed_dim = Some(tertiary_fixed_dim);
        self
    }

    /// Sets `onTertiaryFixed` outright, so it stops following its fallback.
    pub fn with_on_tertiary_fixed(mut self, on_tertiary_fixed: Color) -> ColorScheme {
        self.on_tertiary_fixed = Some(on_tertiary_fixed);
        self
    }

    /// Sets `onTertiaryFixedVariant` outright, so it stops following its fallback.
    pub fn with_on_tertiary_fixed_variant(
        mut self,
        on_tertiary_fixed_variant: Color,
    ) -> ColorScheme {
        self.on_tertiary_fixed_variant = Some(on_tertiary_fixed_variant);
        self
    }

    /// Sets `errorContainer` outright, so it stops following its fallback.
    pub fn with_error_container(mut self, error_container: Color) -> ColorScheme {
        self.error_container = Some(error_container);
        self
    }

    /// Sets `onErrorContainer` outright, so it stops following its fallback.
    pub fn with_on_error_container(mut self, on_error_container: Color) -> ColorScheme {
        self.on_error_container = Some(on_error_container);
        self
    }

    /// Sets `surfaceVariant` outright, so it stops following its fallback.
    pub fn with_surface_variant(mut self, surface_variant: Color) -> ColorScheme {
        self.surface_variant = Some(surface_variant);
        self
    }

    /// Sets `surfaceDim` outright, so it stops following its fallback.
    pub fn with_surface_dim(mut self, surface_dim: Color) -> ColorScheme {
        self.surface_dim = Some(surface_dim);
        self
    }

    /// Sets `surfaceBright` outright, so it stops following its fallback.
    pub fn with_surface_bright(mut self, surface_bright: Color) -> ColorScheme {
        self.surface_bright = Some(surface_bright);
        self
    }

    /// Sets `surfaceContainerLowest` outright, so it stops following its fallback.
    pub fn with_surface_container_lowest(mut self, surface_container_lowest: Color) -> ColorScheme {
        self.surface_container_lowest = Some(surface_container_lowest);
        self
    }

    /// Sets `surfaceContainerLow` outright, so it stops following its fallback.
    pub fn with_surface_container_low(mut self, surface_container_low: Color) -> ColorScheme {
        self.surface_container_low = Some(surface_container_low);
        self
    }

    /// Sets `surfaceContainer` outright, so it stops following its fallback.
    pub fn with_surface_container(mut self, surface_container: Color) -> ColorScheme {
        self.surface_container = Some(surface_container);
        self
    }

    /// Sets `surfaceContainerHigh` outright, so it stops following its fallback.
    pub fn with_surface_container_high(mut self, surface_container_high: Color) -> ColorScheme {
        self.surface_container_high = Some(surface_container_high);
        self
    }

    /// Sets `surfaceContainerHighest` outright, so it stops following its fallback.
    pub fn with_surface_container_highest(
        mut self,
        surface_container_highest: Color,
    ) -> ColorScheme {
        self.surface_container_highest = Some(surface_container_highest);
        self
    }

    /// Sets `onSurfaceVariant` outright, so it stops following its fallback.
    pub fn with_on_surface_variant(mut self, on_surface_variant: Color) -> ColorScheme {
        self.on_surface_variant = Some(on_surface_variant);
        self
    }

    /// Sets `outline` outright, so it stops following its fallback.
    pub fn with_outline(mut self, outline: Color) -> ColorScheme {
        self.outline = Some(outline);
        self
    }

    /// Sets `outlineVariant` outright, so it stops following its fallback.
    pub fn with_outline_variant(mut self, outline_variant: Color) -> ColorScheme {
        self.outline_variant = Some(outline_variant);
        self
    }

    /// Sets `shadow` outright, so it stops following its fallback.
    pub fn with_shadow(mut self, shadow: Color) -> ColorScheme {
        self.shadow = Some(shadow);
        self
    }

    /// Sets `scrim` outright, so it stops following its fallback.
    pub fn with_scrim(mut self, scrim: Color) -> ColorScheme {
        self.scrim = Some(scrim);
        self
    }

    /// Sets `inverseSurface` outright, so it stops following its fallback.
    pub fn with_inverse_surface(mut self, inverse_surface: Color) -> ColorScheme {
        self.inverse_surface = Some(inverse_surface);
        self
    }

    /// Sets `onInverseSurface` outright, so it stops following its fallback.
    pub fn with_on_inverse_surface(mut self, on_inverse_surface: Color) -> ColorScheme {
        self.on_inverse_surface = Some(on_inverse_surface);
        self
    }

    /// Sets `inversePrimary` outright, so it stops following its fallback.
    pub fn with_inverse_primary(mut self, inverse_primary: Color) -> ColorScheme {
        self.inverse_primary = Some(inverse_primary);
        self
    }

    /// Sets `surfaceTint` outright, so it stops following its fallback.
    pub fn with_surface_tint(mut self, surface_tint: Color) -> ColorScheme {
        self.surface_tint = Some(surface_tint);
        self
    }

    /// Sets `background` outright, so it stops following its fallback.
    pub fn with_background(mut self, background: Color) -> ColorScheme {
        self.background = Some(background);
        self
    }

    /// Sets `onBackground` outright, so it stops following its fallback.
    pub fn with_on_background(mut self, on_background: Color) -> ColorScheme {
        self.on_background = Some(on_background);
        self
    }

    /// Upstream `ColorScheme.lerp`: every role interpolated.
    ///
    /// Upstream lerps the resolved roles, so a role that was following a
    /// fallback is interpolated from wherever that fallback put it. This
    /// resolves both ends first for the same reason.
    pub fn lerp(a: &ColorScheme, b: &ColorScheme, t: f32) -> ColorScheme {
        let mix = |first: Color, second: Color| {
            ColorTween {
                begin: first,
                end: second,
            }
            .lerp(t)
        };
        let mut scheme = ColorScheme {
            brightness: if t < 0.5 { a.brightness } else { b.brightness },
            primary: mix(a.primary, b.primary),
            on_primary: mix(a.on_primary, b.on_primary),
            secondary: mix(a.secondary, b.secondary),
            on_secondary: mix(a.on_secondary, b.on_secondary),
            error: mix(a.error, b.error),
            on_error: mix(a.on_error, b.on_error),
            surface: mix(a.surface, b.surface),
            on_surface: mix(a.on_surface, b.on_surface),
            ..ColorScheme::UNSET
        };
        scheme.primary_container = Some(mix(a.primary_container(), b.primary_container()));
        scheme.on_primary_container = Some(mix(a.on_primary_container(), b.on_primary_container()));
        scheme.primary_fixed = Some(mix(a.primary_fixed(), b.primary_fixed()));
        scheme.primary_fixed_dim = Some(mix(a.primary_fixed_dim(), b.primary_fixed_dim()));
        scheme.on_primary_fixed = Some(mix(a.on_primary_fixed(), b.on_primary_fixed()));
        scheme.on_primary_fixed_variant = Some(mix(
            a.on_primary_fixed_variant(),
            b.on_primary_fixed_variant(),
        ));
        scheme.secondary_container = Some(mix(a.secondary_container(), b.secondary_container()));
        scheme.on_secondary_container =
            Some(mix(a.on_secondary_container(), b.on_secondary_container()));
        scheme.secondary_fixed = Some(mix(a.secondary_fixed(), b.secondary_fixed()));
        scheme.secondary_fixed_dim = Some(mix(a.secondary_fixed_dim(), b.secondary_fixed_dim()));
        scheme.on_secondary_fixed = Some(mix(a.on_secondary_fixed(), b.on_secondary_fixed()));
        scheme.on_secondary_fixed_variant = Some(mix(
            a.on_secondary_fixed_variant(),
            b.on_secondary_fixed_variant(),
        ));
        scheme.tertiary = Some(mix(a.tertiary(), b.tertiary()));
        scheme.on_tertiary = Some(mix(a.on_tertiary(), b.on_tertiary()));
        scheme.tertiary_container = Some(mix(a.tertiary_container(), b.tertiary_container()));
        scheme.on_tertiary_container =
            Some(mix(a.on_tertiary_container(), b.on_tertiary_container()));
        scheme.tertiary_fixed = Some(mix(a.tertiary_fixed(), b.tertiary_fixed()));
        scheme.tertiary_fixed_dim = Some(mix(a.tertiary_fixed_dim(), b.tertiary_fixed_dim()));
        scheme.on_tertiary_fixed = Some(mix(a.on_tertiary_fixed(), b.on_tertiary_fixed()));
        scheme.on_tertiary_fixed_variant = Some(mix(
            a.on_tertiary_fixed_variant(),
            b.on_tertiary_fixed_variant(),
        ));
        scheme.error_container = Some(mix(a.error_container(), b.error_container()));
        scheme.on_error_container = Some(mix(a.on_error_container(), b.on_error_container()));
        scheme.surface_variant = Some(mix(a.surface_variant(), b.surface_variant()));
        scheme.surface_dim = Some(mix(a.surface_dim(), b.surface_dim()));
        scheme.surface_bright = Some(mix(a.surface_bright(), b.surface_bright()));
        scheme.surface_container_lowest = Some(mix(
            a.surface_container_lowest(),
            b.surface_container_lowest(),
        ));
        scheme.surface_container_low =
            Some(mix(a.surface_container_low(), b.surface_container_low()));
        scheme.surface_container = Some(mix(a.surface_container(), b.surface_container()));
        scheme.surface_container_high =
            Some(mix(a.surface_container_high(), b.surface_container_high()));
        scheme.surface_container_highest = Some(mix(
            a.surface_container_highest(),
            b.surface_container_highest(),
        ));
        scheme.on_surface_variant = Some(mix(a.on_surface_variant(), b.on_surface_variant()));
        scheme.outline = Some(mix(a.outline(), b.outline()));
        scheme.outline_variant = Some(mix(a.outline_variant(), b.outline_variant()));
        scheme.shadow = Some(mix(a.shadow(), b.shadow()));
        scheme.scrim = Some(mix(a.scrim(), b.scrim()));
        scheme.inverse_surface = Some(mix(a.inverse_surface(), b.inverse_surface()));
        scheme.on_inverse_surface = Some(mix(a.on_inverse_surface(), b.on_inverse_surface()));
        scheme.inverse_primary = Some(mix(a.inverse_primary(), b.inverse_primary()));
        scheme.surface_tint = Some(mix(a.surface_tint(), b.surface_tint()));
        scheme.background = Some(mix(a.background(), b.background()));
        scheme.on_background = Some(mix(a.on_background(), b.on_background()));
        scheme
    }
}

impl ColorScheme {
    /// The Material 3 baseline light scheme.
    ///
    /// Upstream `_colorSchemeLightM3`, which is what `ThemeData()` uses when
    /// nobody names a scheme -- so it is the scheme a Material 3
    /// application actually runs on.
    pub const fn light_m3() -> ColorScheme {
        ColorScheme {
            brightness: Brightness::Light,
            primary: Color(0xff6750a4),
            on_primary: Color(0xffffffff),
            secondary: Color(0xff625b71),
            on_secondary: Color(0xffffffff),
            error: Color(0xffb3261e),
            on_error: Color(0xffffffff),
            surface: Color(0xfffef7ff),
            on_surface: Color(0xff1d1b20),
            primary_container: Some(Color(0xffeaddff)),
            on_primary_container: Some(Color(0xff4f378b)),
            primary_fixed: Some(Color(0xffeaddff)),
            primary_fixed_dim: Some(Color(0xffd0bcff)),
            on_primary_fixed: Some(Color(0xff21005d)),
            on_primary_fixed_variant: Some(Color(0xff4f378b)),
            secondary_container: Some(Color(0xffe8def8)),
            on_secondary_container: Some(Color(0xff4a4458)),
            secondary_fixed: Some(Color(0xffe8def8)),
            secondary_fixed_dim: Some(Color(0xffccc2dc)),
            on_secondary_fixed: Some(Color(0xff1d192b)),
            on_secondary_fixed_variant: Some(Color(0xff4a4458)),
            tertiary: Some(Color(0xff7d5260)),
            on_tertiary: Some(Color(0xffffffff)),
            tertiary_container: Some(Color(0xffffd8e4)),
            on_tertiary_container: Some(Color(0xff633b48)),
            tertiary_fixed: Some(Color(0xffffd8e4)),
            tertiary_fixed_dim: Some(Color(0xffefb8c8)),
            on_tertiary_fixed: Some(Color(0xff31111d)),
            on_tertiary_fixed_variant: Some(Color(0xff633b48)),
            error_container: Some(Color(0xfff9dedc)),
            on_error_container: Some(Color(0xff8c1d18)),
            background: Some(Color(0xfffef7ff)),
            on_background: Some(Color(0xff1d1b20)),
            surface_bright: Some(Color(0xfffef7ff)),
            surface_container_lowest: Some(Color(0xffffffff)),
            surface_container_low: Some(Color(0xfff7f2fa)),
            surface_container: Some(Color(0xfff3edf7)),
            surface_container_high: Some(Color(0xffece6f0)),
            surface_container_highest: Some(Color(0xffe6e0e9)),
            surface_dim: Some(Color(0xffded8e1)),
            surface_variant: Some(Color(0xffe7e0ec)),
            on_surface_variant: Some(Color(0xff49454f)),
            outline: Some(Color(0xff79747e)),
            outline_variant: Some(Color(0xffcac4d0)),
            shadow: Some(Color(0xff000000)),
            scrim: Some(Color(0xff000000)),
            inverse_surface: Some(Color(0xff322f35)),
            on_inverse_surface: Some(Color(0xfff5eff7)),
            inverse_primary: Some(Color(0xffd0bcff)),
            surface_tint: Some(Color(0xff6750a4)),
            ..ColorScheme::UNSET
        }
    }

    /// The Material 3 baseline dark scheme.
    ///
    /// Upstream `_colorSchemeDarkM3`, which is what `ThemeData()` uses when
    /// nobody names a scheme -- so it is the scheme a Material 3
    /// application actually runs on.
    pub const fn dark_m3() -> ColorScheme {
        ColorScheme {
            brightness: Brightness::Dark,
            primary: Color(0xffd0bcff),
            on_primary: Color(0xff381e72),
            secondary: Color(0xffccc2dc),
            on_secondary: Color(0xff332d41),
            error: Color(0xfff2b8b5),
            on_error: Color(0xff601410),
            surface: Color(0xff141218),
            on_surface: Color(0xffe6e0e9),
            primary_container: Some(Color(0xff4f378b)),
            on_primary_container: Some(Color(0xffeaddff)),
            primary_fixed: Some(Color(0xffeaddff)),
            primary_fixed_dim: Some(Color(0xffd0bcff)),
            on_primary_fixed: Some(Color(0xff21005d)),
            on_primary_fixed_variant: Some(Color(0xff4f378b)),
            secondary_container: Some(Color(0xff4a4458)),
            on_secondary_container: Some(Color(0xffe8def8)),
            secondary_fixed: Some(Color(0xffe8def8)),
            secondary_fixed_dim: Some(Color(0xffccc2dc)),
            on_secondary_fixed: Some(Color(0xff1d192b)),
            on_secondary_fixed_variant: Some(Color(0xff4a4458)),
            tertiary: Some(Color(0xffefb8c8)),
            on_tertiary: Some(Color(0xff492532)),
            tertiary_container: Some(Color(0xff633b48)),
            on_tertiary_container: Some(Color(0xffffd8e4)),
            tertiary_fixed: Some(Color(0xffffd8e4)),
            tertiary_fixed_dim: Some(Color(0xffefb8c8)),
            on_tertiary_fixed: Some(Color(0xff31111d)),
            on_tertiary_fixed_variant: Some(Color(0xff633b48)),
            error_container: Some(Color(0xff8c1d18)),
            on_error_container: Some(Color(0xfff9dedc)),
            background: Some(Color(0xff141218)),
            on_background: Some(Color(0xffe6e0e9)),
            surface_bright: Some(Color(0xff3b383e)),
            surface_container_lowest: Some(Color(0xff0f0d13)),
            surface_container_low: Some(Color(0xff1d1b20)),
            surface_container: Some(Color(0xff211f26)),
            surface_container_high: Some(Color(0xff2b2930)),
            surface_container_highest: Some(Color(0xff36343b)),
            surface_dim: Some(Color(0xff141218)),
            surface_variant: Some(Color(0xff49454f)),
            on_surface_variant: Some(Color(0xffcac4d0)),
            outline: Some(Color(0xff938f99)),
            outline_variant: Some(Color(0xff49454f)),
            shadow: Some(Color(0xff000000)),
            scrim: Some(Color(0xff000000)),
            inverse_surface: Some(Color(0xffe6e0e9)),
            on_inverse_surface: Some(Color(0xff322f35)),
            inverse_primary: Some(Color(0xff6750a4)),
            surface_tint: Some(Color(0xffd0bcff)),
            ..ColorScheme::UNSET
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_baseline_schemes_are_upstreams() {
        let light = ColorScheme::light();
        assert_eq!(light.brightness, Brightness::Light);
        assert_eq!(light.primary, Color(0xff6200ee));
        assert_eq!(light.secondary, Color(0xff03dac6));
        assert_eq!(light.error, Color(0xffb00020));
        assert_eq!(light.surface, Color::WHITE);

        let dark = ColorScheme::dark();
        assert_eq!(dark.brightness, Brightness::Dark);
        assert_eq!(dark.primary, Color(0xffbb86fc));
        assert_eq!(dark.error, Color(0xffcf6679));
        assert_eq!(dark.surface, Color(0xff121212));
        assert_eq!(dark.on_surface, Color::WHITE);
    }

    #[test]
    fn an_unset_role_follows_the_one_upstream_says_it_follows() {
        let scheme = ColorScheme::light();
        // The container roles follow their base.
        assert_eq!(scheme.primary_container(), scheme.primary);
        assert_eq!(scheme.on_primary_container(), scheme.on_primary);
        assert_eq!(scheme.secondary_container(), scheme.secondary);
        // Tertiary follows secondary, and everything tertiary follows
        // tertiary -- so a scheme with no tertiary at all still answers.
        assert_eq!(scheme.tertiary(), scheme.secondary);
        assert_eq!(scheme.tertiary_container(), scheme.secondary);
        assert_eq!(scheme.on_tertiary_fixed_variant(), scheme.on_secondary);
        // Every surface container is the surface.
        assert_eq!(scheme.surface_container_lowest(), scheme.surface);
        assert_eq!(scheme.surface_container_highest(), scheme.surface);
        assert_eq!(scheme.surface_dim(), scheme.surface);
        // The deprecated pair, and outline through it.
        assert_eq!(scheme.background(), scheme.surface);
        assert_eq!(scheme.on_background(), scheme.on_surface);
        assert_eq!(scheme.outline(), scheme.on_surface);
        assert_eq!(scheme.outline_variant(), scheme.on_surface);
        // The ones with a colour of their own rather than a fallback.
        assert_eq!(scheme.shadow(), Color::BLACK);
        assert_eq!(scheme.scrim(), Color::BLACK);
        assert_eq!(scheme.surface_tint(), scheme.primary);
        assert_eq!(scheme.inverse_primary(), scheme.on_primary);
        assert_eq!(scheme.inverse_surface(), scheme.on_surface);
        assert_eq!(scheme.on_inverse_surface(), scheme.surface);
    }

    #[test]
    fn a_role_that_follows_a_fallback_follows_it_after_a_change_too() {
        // This is why the derived roles are kept as options rather than
        // resolved once: upstream's `copyWith` passes the unset ones through,
        // so a container that was never set follows the new primary.
        let recoloured = ColorScheme::light().with_primary(Color(0xff112233));
        assert_eq!(recoloured.primary_container(), Color(0xff112233));
        assert_eq!(recoloured.surface_tint(), Color(0xff112233));

        // And one that was set outright stops following.
        let pinned = recoloured.with_primary_container(Color(0xff445566));
        assert_eq!(pinned.primary_container(), Color(0xff445566));
        assert_eq!(
            pinned.with_primary(Color(0xff778899)).primary_container(),
            Color(0xff445566),
            "a role somebody set does not move when its base does"
        );
    }

    #[test]
    fn lerping_resolves_both_ends_first() {
        let a = ColorScheme::light()
            .with_primary(Color::argb(255, 0, 0, 0))
            .with_surface(Color::argb(255, 0, 0, 0));
        let b = ColorScheme::light()
            .with_primary(Color::argb(255, 255, 255, 255))
            .with_surface(Color::argb(255, 255, 255, 255));
        let half = ColorScheme::lerp(&a, &b, 0.5);
        assert_eq!(half.primary, Color::argb(255, 128, 128, 128));
        // The container was following primary on both ends, so it lands
        // halfway between where the two fallbacks put it.
        assert_eq!(half.primary_container(), Color::argb(255, 128, 128, 128));
        assert_eq!(
            half.surface_container_high(),
            Color::argb(255, 128, 128, 128)
        );
    }

    #[test]
    fn the_brightness_changes_over_at_the_halfway_point() {
        let light = ColorScheme::light();
        let dark = ColorScheme::dark();
        assert_eq!(
            ColorScheme::lerp(&light, &dark, 0.49).brightness,
            Brightness::Light
        );
        assert_eq!(
            ColorScheme::lerp(&light, &dark, 0.5).brightness,
            Brightness::Dark
        );
    }

    // -- Every role, and every line reading its own -------------------------
    //
    // `ColorScheme::lerp` is forty-nine lines of `mix(a.x(), b.x())`, and the
    // test above watched three of them, at the midpoint. Three of forty-nine
    // is not coverage, and a midpoint cannot tell the two ends apart. This
    // gives every role a number no other role uses and reads it back a
    // quarter of the way, so a line naming its neighbour answers with a
    // value that is not its own and a line written backwards answers with
    // the wrong end.
    //
    // Generated rather than typed: forty-nine hand-written names would be
    // forty-nine chances to make exactly the mistake being tested for.

    /// A scheme whose every role is a different number.
    fn numbered_scheme(base: u8) -> ColorScheme {
        let mut n = 0;
        let mut next = || {
            n += 1;
            Color::argb(255, 0, 0, base + n)
        };
        ColorScheme::light()
            .with_primary(next())
            .with_on_primary(next())
            .with_secondary(next())
            .with_on_secondary(next())
            .with_error(next())
            .with_on_error(next())
            .with_surface(next())
            .with_on_surface(next())
            .with_primary_container(next())
            .with_on_primary_container(next())
            .with_primary_fixed(next())
            .with_primary_fixed_dim(next())
            .with_on_primary_fixed(next())
            .with_on_primary_fixed_variant(next())
            .with_secondary_container(next())
            .with_on_secondary_container(next())
            .with_secondary_fixed(next())
            .with_secondary_fixed_dim(next())
            .with_on_secondary_fixed(next())
            .with_on_secondary_fixed_variant(next())
            .with_tertiary(next())
            .with_on_tertiary(next())
            .with_tertiary_container(next())
            .with_on_tertiary_container(next())
            .with_tertiary_fixed(next())
            .with_tertiary_fixed_dim(next())
            .with_on_tertiary_fixed(next())
            .with_on_tertiary_fixed_variant(next())
            .with_error_container(next())
            .with_on_error_container(next())
            .with_surface_variant(next())
            .with_surface_dim(next())
            .with_surface_bright(next())
            .with_surface_container_lowest(next())
            .with_surface_container_low(next())
            .with_surface_container(next())
            .with_surface_container_high(next())
            .with_surface_container_highest(next())
            .with_on_surface_variant(next())
            .with_outline(next())
            .with_outline_variant(next())
            .with_shadow(next())
            .with_scrim(next())
            .with_inverse_surface(next())
            .with_on_inverse_surface(next())
            .with_inverse_primary(next())
            .with_surface_tint(next())
            .with_background(next())
            .with_on_background(next())
    }

    #[test]
    fn every_role_blends_and_every_line_names_its_own_role() {
        let quarter = ColorScheme::lerp(&numbered_scheme(0), &numbered_scheme(80), 0.25);
        let expected = numbered_scheme(20);
        assert_eq!(quarter.primary, expected.primary, "primary");
        assert_eq!(quarter.on_primary, expected.on_primary, "on_primary");
        assert_eq!(quarter.secondary, expected.secondary, "secondary");
        assert_eq!(quarter.on_secondary, expected.on_secondary, "on_secondary");
        assert_eq!(quarter.error, expected.error, "error");
        assert_eq!(quarter.on_error, expected.on_error, "on_error");
        assert_eq!(quarter.surface, expected.surface, "surface");
        assert_eq!(quarter.on_surface, expected.on_surface, "on_surface");
        assert_eq!(
            quarter.primary_container(),
            expected.primary_container(),
            "primary_container"
        );
        assert_eq!(
            quarter.on_primary_container(),
            expected.on_primary_container(),
            "on_primary_container"
        );
        assert_eq!(
            quarter.primary_fixed(),
            expected.primary_fixed(),
            "primary_fixed"
        );
        assert_eq!(
            quarter.primary_fixed_dim(),
            expected.primary_fixed_dim(),
            "primary_fixed_dim"
        );
        assert_eq!(
            quarter.on_primary_fixed(),
            expected.on_primary_fixed(),
            "on_primary_fixed"
        );
        assert_eq!(
            quarter.on_primary_fixed_variant(),
            expected.on_primary_fixed_variant(),
            "on_primary_fixed_variant"
        );
        assert_eq!(
            quarter.secondary_container(),
            expected.secondary_container(),
            "secondary_container"
        );
        assert_eq!(
            quarter.on_secondary_container(),
            expected.on_secondary_container(),
            "on_secondary_container"
        );
        assert_eq!(
            quarter.secondary_fixed(),
            expected.secondary_fixed(),
            "secondary_fixed"
        );
        assert_eq!(
            quarter.secondary_fixed_dim(),
            expected.secondary_fixed_dim(),
            "secondary_fixed_dim"
        );
        assert_eq!(
            quarter.on_secondary_fixed(),
            expected.on_secondary_fixed(),
            "on_secondary_fixed"
        );
        assert_eq!(
            quarter.on_secondary_fixed_variant(),
            expected.on_secondary_fixed_variant(),
            "on_secondary_fixed_variant"
        );
        assert_eq!(quarter.tertiary(), expected.tertiary(), "tertiary");
        assert_eq!(quarter.on_tertiary(), expected.on_tertiary(), "on_tertiary");
        assert_eq!(
            quarter.tertiary_container(),
            expected.tertiary_container(),
            "tertiary_container"
        );
        assert_eq!(
            quarter.on_tertiary_container(),
            expected.on_tertiary_container(),
            "on_tertiary_container"
        );
        assert_eq!(
            quarter.tertiary_fixed(),
            expected.tertiary_fixed(),
            "tertiary_fixed"
        );
        assert_eq!(
            quarter.tertiary_fixed_dim(),
            expected.tertiary_fixed_dim(),
            "tertiary_fixed_dim"
        );
        assert_eq!(
            quarter.on_tertiary_fixed(),
            expected.on_tertiary_fixed(),
            "on_tertiary_fixed"
        );
        assert_eq!(
            quarter.on_tertiary_fixed_variant(),
            expected.on_tertiary_fixed_variant(),
            "on_tertiary_fixed_variant"
        );
        assert_eq!(
            quarter.error_container(),
            expected.error_container(),
            "error_container"
        );
        assert_eq!(
            quarter.on_error_container(),
            expected.on_error_container(),
            "on_error_container"
        );
        assert_eq!(
            quarter.surface_variant(),
            expected.surface_variant(),
            "surface_variant"
        );
        assert_eq!(quarter.surface_dim(), expected.surface_dim(), "surface_dim");
        assert_eq!(
            quarter.surface_bright(),
            expected.surface_bright(),
            "surface_bright"
        );
        assert_eq!(
            quarter.surface_container_lowest(),
            expected.surface_container_lowest(),
            "surface_container_lowest"
        );
        assert_eq!(
            quarter.surface_container_low(),
            expected.surface_container_low(),
            "surface_container_low"
        );
        assert_eq!(
            quarter.surface_container(),
            expected.surface_container(),
            "surface_container"
        );
        assert_eq!(
            quarter.surface_container_high(),
            expected.surface_container_high(),
            "surface_container_high"
        );
        assert_eq!(
            quarter.surface_container_highest(),
            expected.surface_container_highest(),
            "surface_container_highest"
        );
        assert_eq!(
            quarter.on_surface_variant(),
            expected.on_surface_variant(),
            "on_surface_variant"
        );
        assert_eq!(quarter.outline(), expected.outline(), "outline");
        assert_eq!(
            quarter.outline_variant(),
            expected.outline_variant(),
            "outline_variant"
        );
        assert_eq!(quarter.shadow(), expected.shadow(), "shadow");
        assert_eq!(quarter.scrim(), expected.scrim(), "scrim");
        assert_eq!(
            quarter.inverse_surface(),
            expected.inverse_surface(),
            "inverse_surface"
        );
        assert_eq!(
            quarter.on_inverse_surface(),
            expected.on_inverse_surface(),
            "on_inverse_surface"
        );
        assert_eq!(
            quarter.inverse_primary(),
            expected.inverse_primary(),
            "inverse_primary"
        );
        assert_eq!(
            quarter.surface_tint(),
            expected.surface_tint(),
            "surface_tint"
        );
        assert_eq!(quarter.background(), expected.background(), "background");
        assert_eq!(
            quarter.on_background(),
            expected.on_background(),
            "on_background"
        );

        // And the other way: a lerp is symmetric at the midpoint, so only an
        // off-centre t can tell `lerp(a, b, t)` from `lerp(b, a, t)`.
        let back = ColorScheme::lerp(&numbered_scheme(80), &numbered_scheme(0), 0.25);
        let expected = numbered_scheme(60);
        assert_eq!(back.primary, expected.primary, "primary reversed");
        assert_eq!(back.on_primary, expected.on_primary, "on_primary reversed");
        assert_eq!(back.secondary, expected.secondary, "secondary reversed");
        assert_eq!(
            back.on_secondary, expected.on_secondary,
            "on_secondary reversed"
        );
        assert_eq!(back.error, expected.error, "error reversed");
        assert_eq!(back.on_error, expected.on_error, "on_error reversed");
        assert_eq!(back.surface, expected.surface, "surface reversed");
        assert_eq!(back.on_surface, expected.on_surface, "on_surface reversed");
        assert_eq!(
            back.primary_container(),
            expected.primary_container(),
            "primary_container reversed"
        );
        assert_eq!(
            back.on_primary_container(),
            expected.on_primary_container(),
            "on_primary_container reversed"
        );
        assert_eq!(
            back.primary_fixed(),
            expected.primary_fixed(),
            "primary_fixed reversed"
        );
        assert_eq!(
            back.primary_fixed_dim(),
            expected.primary_fixed_dim(),
            "primary_fixed_dim reversed"
        );
        assert_eq!(
            back.on_primary_fixed(),
            expected.on_primary_fixed(),
            "on_primary_fixed reversed"
        );
        assert_eq!(
            back.on_primary_fixed_variant(),
            expected.on_primary_fixed_variant(),
            "on_primary_fixed_variant reversed"
        );
        assert_eq!(
            back.secondary_container(),
            expected.secondary_container(),
            "secondary_container reversed"
        );
        assert_eq!(
            back.on_secondary_container(),
            expected.on_secondary_container(),
            "on_secondary_container reversed"
        );
        assert_eq!(
            back.secondary_fixed(),
            expected.secondary_fixed(),
            "secondary_fixed reversed"
        );
        assert_eq!(
            back.secondary_fixed_dim(),
            expected.secondary_fixed_dim(),
            "secondary_fixed_dim reversed"
        );
        assert_eq!(
            back.on_secondary_fixed(),
            expected.on_secondary_fixed(),
            "on_secondary_fixed reversed"
        );
        assert_eq!(
            back.on_secondary_fixed_variant(),
            expected.on_secondary_fixed_variant(),
            "on_secondary_fixed_variant reversed"
        );
        assert_eq!(back.tertiary(), expected.tertiary(), "tertiary reversed");
        assert_eq!(
            back.on_tertiary(),
            expected.on_tertiary(),
            "on_tertiary reversed"
        );
        assert_eq!(
            back.tertiary_container(),
            expected.tertiary_container(),
            "tertiary_container reversed"
        );
        assert_eq!(
            back.on_tertiary_container(),
            expected.on_tertiary_container(),
            "on_tertiary_container reversed"
        );
        assert_eq!(
            back.tertiary_fixed(),
            expected.tertiary_fixed(),
            "tertiary_fixed reversed"
        );
        assert_eq!(
            back.tertiary_fixed_dim(),
            expected.tertiary_fixed_dim(),
            "tertiary_fixed_dim reversed"
        );
        assert_eq!(
            back.on_tertiary_fixed(),
            expected.on_tertiary_fixed(),
            "on_tertiary_fixed reversed"
        );
        assert_eq!(
            back.on_tertiary_fixed_variant(),
            expected.on_tertiary_fixed_variant(),
            "on_tertiary_fixed_variant reversed"
        );
        assert_eq!(
            back.error_container(),
            expected.error_container(),
            "error_container reversed"
        );
        assert_eq!(
            back.on_error_container(),
            expected.on_error_container(),
            "on_error_container reversed"
        );
        assert_eq!(
            back.surface_variant(),
            expected.surface_variant(),
            "surface_variant reversed"
        );
        assert_eq!(
            back.surface_dim(),
            expected.surface_dim(),
            "surface_dim reversed"
        );
        assert_eq!(
            back.surface_bright(),
            expected.surface_bright(),
            "surface_bright reversed"
        );
        assert_eq!(
            back.surface_container_lowest(),
            expected.surface_container_lowest(),
            "surface_container_lowest reversed"
        );
        assert_eq!(
            back.surface_container_low(),
            expected.surface_container_low(),
            "surface_container_low reversed"
        );
        assert_eq!(
            back.surface_container(),
            expected.surface_container(),
            "surface_container reversed"
        );
        assert_eq!(
            back.surface_container_high(),
            expected.surface_container_high(),
            "surface_container_high reversed"
        );
        assert_eq!(
            back.surface_container_highest(),
            expected.surface_container_highest(),
            "surface_container_highest reversed"
        );
        assert_eq!(
            back.on_surface_variant(),
            expected.on_surface_variant(),
            "on_surface_variant reversed"
        );
        assert_eq!(back.outline(), expected.outline(), "outline reversed");
        assert_eq!(
            back.outline_variant(),
            expected.outline_variant(),
            "outline_variant reversed"
        );
        assert_eq!(back.shadow(), expected.shadow(), "shadow reversed");
        assert_eq!(back.scrim(), expected.scrim(), "scrim reversed");
        assert_eq!(
            back.inverse_surface(),
            expected.inverse_surface(),
            "inverse_surface reversed"
        );
        assert_eq!(
            back.on_inverse_surface(),
            expected.on_inverse_surface(),
            "on_inverse_surface reversed"
        );
        assert_eq!(
            back.inverse_primary(),
            expected.inverse_primary(),
            "inverse_primary reversed"
        );
        assert_eq!(
            back.surface_tint(),
            expected.surface_tint(),
            "surface_tint reversed"
        );
        assert_eq!(
            back.background(),
            expected.background(),
            "background reversed"
        );
        assert_eq!(
            back.on_background(),
            expected.on_background(),
            "on_background reversed"
        );
    }
}
