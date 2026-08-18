// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/shrine/colors.dart` (flutter/gallery @ d12640d):
//! the `shrine*` color constants, the whole palette the study's theme and
//! pages are built from.

use rustflutter::prelude::Color;

pub const SHRINE_PINK_50: Color = Color::rgb(0xFE, 0xEA, 0xE6);
pub const SHRINE_PINK_100: Color = Color::rgb(0xFE, 0xDB, 0xD0);
pub const SHRINE_PINK_300: Color = Color::rgb(0xFB, 0xB8, 0xAC);
pub const SHRINE_PINK_400: Color = Color::rgb(0xEA, 0xA4, 0xA4);

pub const SHRINE_BROWN_900: Color = Color::rgb(0x44, 0x2B, 0x2D);
pub const SHRINE_BROWN_600: Color = Color::rgb(0x7D, 0x4F, 0x52);

pub const SHRINE_ERROR_RED: Color = Color::rgb(0xC5, 0x03, 0x2B);

pub const SHRINE_SURFACE_WHITE: Color = Color::rgb(0xFF, 0xFB, 0xFA);
pub const SHRINE_BACKGROUND_WHITE: Color = Color::WHITE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constants_carry_upstreams_values() {
        // A typo here is invisible on screen until someone compares against
        // the real Shrine, so the values are pinned verbatim.
        assert_eq!(SHRINE_PINK_50, Color(0xFFFEEAE6));
        assert_eq!(SHRINE_PINK_100, Color(0xFFFEDBD0));
        assert_eq!(SHRINE_PINK_300, Color(0xFFFBB8AC));
        assert_eq!(SHRINE_PINK_400, Color(0xFFEAA4A4));
        assert_eq!(SHRINE_BROWN_900, Color(0xFF442B2D));
        assert_eq!(SHRINE_BROWN_600, Color(0xFF7D4F52));
        assert_eq!(SHRINE_ERROR_RED, Color(0xFFC5032B));
        assert_eq!(SHRINE_SURFACE_WHITE, Color(0xFFFFFBFA));
        assert_eq!(SHRINE_BACKGROUND_WHITE, Color(0xFFFFFFFF));
    }
}
