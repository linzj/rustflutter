// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The gallery's settings model.
//!
//! Ported from `lib/data/gallery_options.dart` (flutter/gallery @ d12640d):
//! the `GalleryOptions` value type and its resolution rules. What upstream
//! reads from `MediaQuery` / `PlatformDispatcher` is read here from the
//! framework's platform API (`rustflutter::platform`), and no change callback
//! is needed: the shell schedules a frame when the platform's settings or
//! locales change, and everything below is resolved in `build`.
//!
//! Upstream's `ModelBinding` / `_ModelBindingScope` boilerplate -- the
//! inherited widget that carries the options down the tree -- has no
//! counterpart: the one `GalleryState` at the root holds the options, and
//! every screen reads them from there. `ApplyTextOptions`, which pushes the
//! resolved scale and direction into the tree, is likewise unwired until the
//! settings UI lands (batch M-C); see PORTING.md.

use rustflutter::direction::TextDirection;
use rustflutter::platform::{self, Brightness, Locale};

use crate::constants::SYSTEM_TEXT_SCALE_FACTOR_OPTION;

/// Upstream's `ThemeMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeMode {
    /// Follow the platform's light/dark choice.
    #[default]
    System,
    Light,
    Dark,
}

/// Upstream's `CustomTextDirection`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)] // `Ltr` is one of the three options upstream's settings
                    // offers; the UI that selects it is M-C.
pub enum CustomTextDirection {
    /// Derive the direction from the locale's language.
    #[default]
    LocaleBased,
    Ltr,
    Rtl,
}

/// Upstream's `TargetPlatform`, which the framework has no equivalent of:
/// there is one embedder, so the value is carried (upstream's demo theme keys
/// typography off it) but never read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // Carried, not read: see above.
pub enum TargetPlatform {
    Android,
    Fuchsia,
    Ios,
    Linux,
    Macos,
    Windows,
}

/// Upstream's `SystemUiOverlayStyle`, reduced to the one thing upstream's
/// gallery reads off it: whether the status bar foreground is light or dark.
/// The framework does not style system chrome yet; the value is resolved so
/// that the rule is ported, and the divergence is logged in PORTING.md.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // Resolved, not yet applied: the framework does not style
                    // system chrome (see PORTING.md).
pub enum SystemUiOverlayStyle {
    Light,
    Dark,
}

/// See <http://en.wikipedia.org/wiki/Right-to-left>.
#[allow(dead_code)] // Read by `resolved_text_direction`, which nothing calls
                    // until RTL layout is wired (see PORTING.md).
pub const RTL_LANGUAGES: &[&str] = &[
    "ar", // Arabic
    "fa", // Farsi
    "he", // Hebrew
    "ps", // Pashto
    "ur", // Urdu
];

/// The options upstream's settings page edits, as one value.
///
/// Upstream's class is immutable with a `copyWith`; here it is a plain struct
/// with `with_*` builders, which is the same thing in Rust's idiom. The two
/// fields upstream keeps private (`_textScaleFactor`, `_locale`) stay private
/// for the same reason: both have a sentinel or a fallback that only the
/// accessors interpret.
#[derive(Clone, Debug, PartialEq)]
pub struct GalleryOptions {
    pub theme_mode: ThemeMode,
    text_scale_factor: f64,
    pub custom_text_direction: CustomTextDirection,
    locale: Option<Locale>,
    pub time_dilation: f64,
    pub platform: Option<TargetPlatform>,
    /// True for integration tests.
    pub is_test_mode: bool,
}

impl Default for GalleryOptions {
    /// Upstream's initial model (`lib/main.dart`, the `ModelBinding` at the
    /// root): follow the system for theme, scale, direction and locale, run
    /// at full speed.
    fn default() -> GalleryOptions {
        GalleryOptions {
            theme_mode: ThemeMode::System,
            text_scale_factor: SYSTEM_TEXT_SCALE_FACTOR_OPTION,
            custom_text_direction: CustomTextDirection::LocaleBased,
            locale: None,
            time_dilation: 1.0,
            platform: None,
            is_test_mode: false,
        }
    }
}

impl GalleryOptions {
    // The accessors other than `resolved_brightness` are resolved but not yet
    // applied to rendering (no scaled text, no RTL layout, no locale
    // switching, no system chrome) -- see PORTING.md. They are the settings
    // UI's surface (M-C/M-J), and the tests below exercise them.

    /// The text scale. A sentinel value means "whatever the platform says",
    /// which is what upstream reads out of `MediaQuery`; `use_sentinel`
    /// returns the sentinel itself, for the settings UI that has to display
    /// "system" rather than a number.
    #[allow(dead_code)]
    pub fn text_scale_factor(&self, use_sentinel: bool) -> f64 {
        if self.text_scale_factor == SYSTEM_TEXT_SCALE_FACTOR_OPTION {
            if use_sentinel {
                SYSTEM_TEXT_SCALE_FACTOR_OPTION
            } else {
                platform::text_scale_factor()
            }
        } else {
            self.text_scale_factor
        }
    }

    /// The chosen locale, or the device's. Upstream caches the first reported
    /// device locale in a static; `platform::locale()` is exactly that (the
    /// platform's most preferred locale, `en` before any has been reported).
    #[allow(dead_code)]
    pub fn locale(&self) -> Locale {
        self.locale.clone().unwrap_or_else(platform::locale)
    }

    /// Upstream's `resolvedTextDirection`: the direction the setting implies,
    /// or `None` when it is locale-based and no locale can be determined. A
    /// locale can always be determined here (see [`locale`]), so `None` is
    /// unreachable in practice; the signature keeps upstream's.
    #[allow(dead_code)]
    pub fn resolved_text_direction(&self) -> Option<TextDirection> {
        match self.custom_text_direction {
            CustomTextDirection::LocaleBased => {
                let language = self.locale().language_code.to_lowercase();
                Some(if RTL_LANGUAGES.contains(&language.as_str()) {
                    TextDirection::Rtl
                } else {
                    TextDirection::Ltr
                })
            }
            CustomTextDirection::Rtl => Some(TextDirection::Rtl),
            CustomTextDirection::Ltr => Some(TextDirection::Ltr),
        }
    }

    /// Light or dark, with `ThemeMode::System` resolved against the platform.
    ///
    /// Upstream keeps this switch in two places (`lib/main.dart` and
    /// `lib/pages/demo.dart`, each resolving `themeMode` against
    /// `MediaQuery.platformBrightness`); it is one method here so the two
    /// cannot drift.
    pub fn resolved_brightness(&self) -> Brightness {
        match self.theme_mode {
            ThemeMode::Light => Brightness::Light,
            ThemeMode::Dark => Brightness::Dark,
            ThemeMode::System => platform::brightness(),
        }
    }

    /// Upstream's `resolvedSystemUiOverlayStyle`: the inverse of the resolved
    /// brightness, so the status bar foreground contrasts with the app.
    #[allow(dead_code)]
    pub fn resolved_system_ui_overlay_style(&self) -> SystemUiOverlayStyle {
        match self.resolved_brightness() {
            Brightness::Dark => SystemUiOverlayStyle::Light,
            Brightness::Light => SystemUiOverlayStyle::Dark,
        }
    }

    // The `with_*` builders are upstream's `copyWith`, one argument at a time.
    // A single method taking seven `Option`s would port the letter of
    // `copyWith` at the cost of unreadable call sites.

    pub fn with_theme_mode(mut self, theme_mode: ThemeMode) -> GalleryOptions {
        self.theme_mode = theme_mode;
        self
    }

    #[allow(dead_code)]
    pub fn with_text_scale_factor(mut self, text_scale_factor: f64) -> GalleryOptions {
        self.text_scale_factor = text_scale_factor;
        self
    }

    #[allow(dead_code)]
    pub fn with_custom_text_direction(
        mut self,
        custom_text_direction: CustomTextDirection,
    ) -> GalleryOptions {
        self.custom_text_direction = custom_text_direction;
        self
    }

    #[allow(dead_code)]
    pub fn with_locale(mut self, locale: Option<Locale>) -> GalleryOptions {
        self.locale = locale;
        self
    }

    #[allow(dead_code)]
    pub fn with_time_dilation(mut self, time_dilation: f64) -> GalleryOptions {
        self.time_dilation = time_dilation;
        self
    }

    #[allow(dead_code)]
    pub fn with_platform(mut self, platform: Option<TargetPlatform>) -> GalleryOptions {
        self.platform = platform;
        self
    }

    #[allow(dead_code)]
    pub fn with_is_test_mode(mut self, is_test_mode: bool) -> GalleryOptions {
        self.is_test_mode = is_test_mode;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_upstreams_initial_model() {
        let options = GalleryOptions::default();
        assert_eq!(options.theme_mode, ThemeMode::System);
        assert_eq!(
            options.text_scale_factor(true),
            SYSTEM_TEXT_SCALE_FACTOR_OPTION
        );
        assert_eq!(
            options.custom_text_direction,
            CustomTextDirection::LocaleBased
        );
        assert_eq!(options.time_dilation, 1.0);
        assert!(!options.is_test_mode);
    }

    #[test]
    fn the_sentinel_scale_resolves_to_the_platforms() {
        let options = GalleryOptions::default();
        // The platform default, before any settings message has arrived.
        assert_eq!(options.text_scale_factor(false), 1.0);
        assert_eq!(
            options
                .clone()
                .with_text_scale_factor(1.5)
                .text_scale_factor(false),
            1.5
        );
        // The sentinel survives for the UI that has to show "system".
        assert_eq!(
            options.with_text_scale_factor(1.5).text_scale_factor(true),
            1.5
        );
    }

    #[test]
    fn an_explicit_mode_wins_over_the_platform() {
        let options = GalleryOptions::default().with_theme_mode(ThemeMode::Dark);
        assert_eq!(options.resolved_brightness(), Brightness::Dark);
        assert_eq!(
            options.resolved_system_ui_overlay_style(),
            SystemUiOverlayStyle::Light
        );
    }

    #[test]
    fn direction_follows_the_languages_upstream_lists() {
        let options = GalleryOptions::default().with_locale(Some(Locale::new("ar")));
        assert_eq!(options.resolved_text_direction(), Some(TextDirection::Rtl));
        let options = options.with_locale(Some(Locale::new("en")));
        assert_eq!(options.resolved_text_direction(), Some(TextDirection::Ltr));
        let options = options.with_custom_text_direction(CustomTextDirection::Rtl);
        assert_eq!(options.resolved_text_direction(), Some(TextDirection::Rtl));
    }

    #[test]
    fn the_locale_falls_back_to_the_devices() {
        // Before the platform has said anything, that is `en` -- the same
        // fallback upstream lands on through `deviceLocale`.
        let options = GalleryOptions::default();
        assert_eq!(options.locale().language_code, "en");
    }
}
