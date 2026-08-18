// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/colors.dart` (flutter/gallery @ d12640d):
//! the `crane*` color constants. One Rust constant per Dart one, same values.

use rustflutter::prelude::Color;

pub const CRANE_PURPLE_700: Color = Color::rgb(0x72, 0x0D, 0x5D);
pub const CRANE_PURPLE_800: Color = Color::rgb(0x5D, 0x10, 0x49);
pub const CRANE_PURPLE_900: Color = Color::rgb(0x4E, 0x0D, 0x3A);

pub const CRANE_RED_700: Color = Color::rgb(0xE3, 0x04, 0x25);

pub const CRANE_WHITE_60: Color = Color::argb(0x99, 0xFF, 0xFF, 0xFF);
pub const CRANE_PRIMARY_WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);
pub const CRANE_ERROR_ORANGE: Color = Color::rgb(0xFF, 0x91, 0x00);

#[allow(dead_code)] // Part of the published palette; nothing reads it yet.
pub const CRANE_ALPHA: Color = Color::argb(0x00, 0xFF, 0xFF, 0xFF);

pub const CRANE_GREY: Color = Color::rgb(0x74, 0x74, 0x74);
pub const CRANE_BLACK: Color = Color::rgb(0x1E, 0x25, 0x2D);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_matches_upstream() {
        // The values are the port: a drift here is a wrong-coloured study.
        assert_eq!(CRANE_PURPLE_700, Color::rgb(0x72, 0x0D, 0x5D));
        assert_eq!(CRANE_PURPLE_800, Color::rgb(0x5D, 0x10, 0x49));
        assert_eq!(CRANE_PURPLE_900, Color::rgb(0x4E, 0x0D, 0x3A));
        assert_eq!(CRANE_RED_700, Color::rgb(0xE3, 0x04, 0x25));
        assert_eq!(CRANE_ERROR_ORANGE, Color::rgb(0xFF, 0x91, 0x00));
        assert_eq!(CRANE_GREY, Color::rgb(0x74, 0x74, 0x74));
        assert_eq!(CRANE_BLACK, Color::rgb(0x1E, 0x25, 0x2D));
    }

    #[test]
    fn white60_is_white_at_sixty_percent() {
        assert_eq!(CRANE_WHITE_60, CRANE_PRIMARY_WHITE.with_alpha(0x99));
    }
}
