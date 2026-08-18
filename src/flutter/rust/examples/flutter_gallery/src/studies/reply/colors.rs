// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/reply/colors.dart` (flutter/gallery @ d12640d):
//! the `ReplyColors` palette. One constant per upstream static, same names
//! snake-cased, same hex values.
//!
//! Unlike the earlier study batches this study's own theme IS applied, inside
//! the study subtree (`reply/app.rs` provides it), which undoes the
//! "studies-share-gallery-theme" divergence for Reply.

/// Upstream's `ReplyColors`. Associated constants rather than a class, which
/// is all the Dart class was.
#[allow(dead_code)] // The whole palette, like upstream's; not every constant
                    // has a reader yet (black800, the alpha variants).
pub mod reply_colors {
    use rustflutter::engine::Color;

    pub const WHITE50: Color = Color(0xFFFFFFFF);

    pub const BLACK800: Color = Color(0xFF121212);
    pub const BLACK900: Color = Color(0xFF000000);

    pub const BLUE50: Color = Color(0xFFEEF0F2);
    pub const BLUE100: Color = Color(0xFFD2DBE0);
    pub const BLUE200: Color = Color(0xFFADBBC4);
    pub const BLUE300: Color = Color(0xFF8CA2AE);
    pub const BLUE600: Color = Color(0xFF4A6572);
    pub const BLUE700: Color = Color(0xFF344955);
    pub const BLUE800: Color = Color(0xFF232F34);

    pub const ORANGE300: Color = Color(0xFFFBD790);
    pub const ORANGE400: Color = Color(0xFFF9BE64);
    pub const ORANGE500: Color = Color(0xFFF9AA33);

    pub const RED200: Color = Color(0xFFCF7779);
    pub const RED400: Color = Color(0xFFFF4C5D);

    pub const WHITE50_ALPHA060: Color = Color(0x99FFFFFF);

    pub const BLUE50_ALPHA060: Color = Color(0x99EEF0F2);

    pub const BLACK900_ALPHA020: Color = Color(0x33000000);
    pub const BLACK900_ALPHA087: Color = Color(0xDE000000);
    pub const BLACK900_ALPHA060: Color = Color(0x99000000);

    pub const GREY_LABEL: Color = Color(0xFFAEAEAE);
    pub const DARK_BOTTOM_APP_BAR_BACKGROUND: Color = Color(0xFF2D2D2D);
    pub const DARK_DRAWER_BACKGROUND: Color = Color(0xFF353535);
    pub const DARK_CARD_BACKGROUND: Color = Color(0xFF1E1E1E);
    pub const DARK_CHIP_BACKGROUND: Color = Color(0xFF2A2A2A);
    pub const LIGHT_CHIP_BACKGROUND: Color = Color(0xFFE5E5E5);
}

#[cfg(test)]
mod tests {
    use super::reply_colors as colors;

    #[test]
    fn the_palette_is_upstreams_hex() {
        // Spot-checked against colors.dart @ d12640d: one per family.
        assert_eq!(colors::BLUE700.0, 0xFF344955);
        assert_eq!(colors::ORANGE500.0, 0xFFF9AA33);
        assert_eq!(colors::RED400.0, 0xFFFF4C5D);
        assert_eq!(colors::BLACK900_ALPHA020.0, 0x33000000);
        assert_eq!(colors::DARK_DRAWER_BACKGROUND.0, 0xFF353535);
        assert_eq!(colors::LIGHT_CHIP_BACKGROUND.0, 0xFFE5E5E5);
    }
}
