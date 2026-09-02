// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from the theme half of `lib/studies/reply/app.dart` (flutter/gallery
//! @ d12640d): `_buildReplyLightTheme`, `_buildReplyDarkTheme` and the two
//! text themes.
//!
//! Upstream keeps these beside `ReplyApp` in `app.dart`; they are a file of
//! their own here for the reason `crane/theme.rs` is -- the font registration
//! belongs with the family that needs it, and `app.rs` is then only the root.
//!
//! Reply is the one study that follows the application's brightness: upstream
//! passes both themes to its `MaterialApp` and lets `themeMode` choose. Crane
//! and Fortnightly are light whatever the app is, and say so; this one is not.
//!
//! # What the `Theme` struct cannot carry
//!
//! Upstream's `ColorScheme` has a `secondary` -- orange500 light, orange300
//! dark -- and it is what the compose button is painted with. This port's
//! [`Theme`] has no secondary slot, so [`secondary`] answers it beside the
//! theme rather than inside it, and the button reads it from here. Same for
//! [`on_secondary`], which upstream leaves at the scheme's default black.

use rustflutter::components::Theme;
use rustflutter::engine::Color;
use rustflutter::platform::Brightness;

use super::colors::reply_colors;

/// The family upstream sets every Reply text style in
/// (`GoogleFonts.workSans`).
pub const WORK_SANS: &str = "Work Sans";

/// The four weights the two text themes use: w400 for the body styles, w600
/// for `titleLarge`/`titleSmall`/`headlineMedium`, bold for `headlineSmall`.
/// Medium is carried because the drawer's selected label asks for it.
const WORK_SANS_FONTS: &[&[u8]] = &[
    include_bytes!("../../../assets/fonts/WorkSans-Regular.ttf"),
    include_bytes!("../../../assets/fonts/WorkSans-Medium.ttf"),
    include_bytes!("../../../assets/fonts/WorkSans-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/WorkSans-Bold.ttf"),
];

/// Registers Work Sans, once. Called by [`reply_theme`], so any screen that
/// takes the theme takes the faces with it.
pub fn ensure_fonts_registered() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for data in WORK_SANS_FONTS {
            rustflutter::engine::register_font(data, WORK_SANS);
        }
    });
}

/// Upstream's `colorScheme.secondary`: the compose button's fill.
/// See the module header for why it is not on the [`Theme`].
pub fn secondary(brightness: Brightness) -> Color {
    match brightness {
        Brightness::Dark => reply_colors::ORANGE300,
        Brightness::Light => reply_colors::ORANGE500,
    }
}

/// Upstream's `colorScheme.onSecondary`, which both schemes leave at the
/// Material default -- black. The compose icon is drawn in it.
pub fn on_secondary() -> Color {
    reply_colors::BLACK900
}

/// The bottom app bar's fill, upstream's `bottomAppBarTheme.color`.
pub fn bottom_app_bar(brightness: Brightness) -> Color {
    match brightness {
        Brightness::Dark => reply_colors::DARK_BOTTOM_APP_BAR_BACKGROUND,
        Brightness::Light => reply_colors::BLUE700,
    }
}

/// Upstream's `_buildReplyLightTheme` / `_buildReplyDarkTheme`, as far as
/// this port's [`Theme`] reaches.
///
/// The mapping, field by field, so a reader can check it against
/// `app.dart:152-229`:
///
/// | upstream | here |
/// |---|---|
/// | `scaffoldBackgroundColor` | `background` |
/// | `cardColor` / `canvasColor` | `surface` |
/// | `colorScheme.primary` | `primary` |
/// | `colorScheme.error` | `danger` |
/// | `textTheme.*.color` | `text` |
///
/// The type sizes are upstream's `bodyLarge` (18) and `titleLarge` (20); the
/// card's own sizes -- `headlineSmall` 24 for the subject, `bodySmall` 12 for
/// the sender line -- are asked for at the site, because a `Theme` carries
/// two sizes and the card uses four.
pub fn reply_theme(brightness: Brightness) -> Theme {
    ensure_fonts_registered();
    let dark = brightness == Brightness::Dark;
    Theme {
        // `scaffoldBackgroundColor`: blue50 light, black900 dark.
        background: if dark {
            reply_colors::BLACK900
        } else {
            reply_colors::BLUE50
        },
        // `cardColor`, which is what a mail card is painted with.
        surface: if dark {
            reply_colors::DARK_CARD_BACKGROUND
        } else {
            reply_colors::WHITE50
        },
        // `chipTheme`'s background, the nearest thing this slot names.
        surface_variant: if dark {
            reply_colors::DARK_CHIP_BACKGROUND
        } else {
            reply_colors::LIGHT_CHIP_BACKGROUND
        },
        // Upstream leaves `dividerColor` at the base theme's, which is white
        // at 12% on dark and black at 12% on light.
        outline: if dark {
            Color::argb(0x1F, 0xFF, 0xFF, 0xFF)
        } else {
            Color::argb(0x1F, 0x00, 0x00, 0x00)
        },
        // `colorScheme.primary`: blue700 light, blue200 dark.
        primary: if dark {
            reply_colors::BLUE200
        } else {
            reply_colors::BLUE700
        },
        on_primary: reply_colors::WHITE50,
        // `colorScheme.error`.
        danger: if dark {
            reply_colors::RED200
        } else {
            reply_colors::RED400
        },
        // Every text style in both themes is one colour: white50 dark,
        // black900 light.
        text: if dark {
            reply_colors::WHITE50
        } else {
            reply_colors::BLACK900
        },
        text_muted: if dark {
            reply_colors::GREY_LABEL
        } else {
            reply_colors::BLUE600
        },
        // A mail card has square corners (`closedShape:
        // RoundedRectangleBorder()`); the drawer's 12 is asked for at its own
        // site.
        radius: 0.0,
        spacing: 8.0,
        // `bodyLarge` and `titleLarge`.
        body_size: 18.0,
        title_size: 20.0,
        font_family: Some(WORK_SANS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_theme_is_upstreams_blues() {
        let theme = reply_theme(Brightness::Light);
        assert_eq!(theme.background, reply_colors::BLUE50, "scaffold");
        assert_eq!(theme.surface, reply_colors::WHITE50, "cardColor");
        assert_eq!(theme.primary, reply_colors::BLUE700);
        assert_eq!(theme.danger, reply_colors::RED400);
        assert_eq!(theme.text, reply_colors::BLACK900);
        assert_eq!(theme.font_family, Some(WORK_SANS));
    }

    #[test]
    fn the_dark_theme_is_the_other_column_of_the_same_table() {
        let theme = reply_theme(Brightness::Dark);
        assert_eq!(theme.background, reply_colors::BLACK900);
        assert_eq!(theme.surface, reply_colors::DARK_CARD_BACKGROUND);
        assert_eq!(theme.primary, reply_colors::BLUE200);
        assert_eq!(theme.danger, reply_colors::RED200);
        assert_eq!(theme.text, reply_colors::WHITE50);
    }

    #[test]
    fn the_compose_button_is_orange_either_way() {
        // Upstream's `colorScheme.secondary`, which is the one colour the
        // two schemes disagree about only in shade.
        assert_eq!(secondary(Brightness::Light), reply_colors::ORANGE500);
        assert_eq!(secondary(Brightness::Dark), reply_colors::ORANGE300);
        assert_eq!(on_secondary(), reply_colors::BLACK900);
    }

    #[test]
    fn the_bar_is_blue_on_light_and_grey_on_dark() {
        assert_eq!(bottom_app_bar(Brightness::Light), reply_colors::BLUE700);
        assert_eq!(
            bottom_app_bar(Brightness::Dark),
            reply_colors::DARK_BOTTOM_APP_BAR_BACKGROUND
        );
    }

    #[test]
    fn registering_twice_is_safe() {
        // A theme is built per frame; the Once is what makes that affordable.
        let first = reply_theme(Brightness::Light);
        let again = reply_theme(Brightness::Light);
        assert_eq!(first, again);
    }
}
