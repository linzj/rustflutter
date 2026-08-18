// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/rally/colors.dart` (flutter/gallery @ d12640d),
//! upstream's `RallyColors` palette.
//!
//! Most color assignments in Rally are not like the typical color assignments
//! that are common in other apps. Instead of primarily mapping to component
//! type and part, they are assigned round robin based on layout -- hence the
//! cycled per-position lists.

use rustflutter::engine::Color;

/// Upstream's `RallyColors.accountColors`.
pub const ACCOUNT_COLORS: [Color; 4] = [
    Color(0xFF005D57),
    Color(0xFF04B97F),
    Color(0xFF37EFBA),
    Color(0xFF007D51),
];

/// Upstream's `RallyColors.billColors`.
pub const BILL_COLORS: [Color; 4] = [
    Color(0xFFFFDC78),
    Color(0xFFFF6951),
    Color(0xFFFFD7D0),
    Color(0xFFFFAC12),
];

/// Upstream's `RallyColors.budgetColors`.
pub const BUDGET_COLORS: [Color; 4] = [
    Color(0xFFB2F2FF),
    Color(0xFFB15DFF),
    Color(0xFF72DEFF),
    Color(0xFF0082FB),
];

pub const GRAY: Color = Color(0xFFD8D8D8);
pub const GRAY60: Color = Color(0x99D8D8D8);
pub const GRAY25: Color = Color(0x40D8D8D8);
pub const WHITE60: Color = Color(0x99FFFFFF);
pub const PRIMARY_BACKGROUND: Color = Color(0xFF33333D);
pub const INPUT_BACKGROUND: Color = Color(0xFF26282F);
pub const CARD_BACKGROUND: Color = Color(0x03FEFEFE);
pub const BUTTON_COLOR: Color = Color(0xFF09AF79);
pub const FOCUS_COLOR: Color = Color(0xCCFFFFFF);
pub const DIVIDER_COLOR: Color = Color(0xAA282828);

/// Upstream's `RallyColors.accountColor`.
pub fn account_color(i: usize) -> Color {
    cycled_color(&ACCOUNT_COLORS, i)
}

/// Upstream's `RallyColors.billColor`.
pub fn bill_color(i: usize) -> Color {
    cycled_color(&BILL_COLORS, i)
}

/// Upstream's `RallyColors.budgetColor`.
pub fn budget_color(i: usize) -> Color {
    cycled_color(&BUDGET_COLORS, i)
}

/// Upstream's `RallyColors.cycledColor`: a color from a list that is
/// considered to be infinitely repeating.
pub fn cycled_color(colors: &[Color], i: usize) -> Color {
    colors[i % colors.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_is_upstream() {
        assert_eq!(ACCOUNT_COLORS[0], Color(0xFF005D57));
        assert_eq!(BILL_COLORS[0], Color(0xFFFFDC78));
        assert_eq!(BUDGET_COLORS[0], Color(0xFFB2F2FF));
        assert_eq!(PRIMARY_BACKGROUND, Color(0xFF33333D));
        assert_eq!(BUTTON_COLOR, Color(0xFF09AF79));
    }

    #[test]
    fn colors_cycle_round_robin() {
        // Upstream's `colors[i % colors.length]`.
        assert_eq!(account_color(0), ACCOUNT_COLORS[0]);
        assert_eq!(account_color(4), ACCOUNT_COLORS[0]);
        assert_eq!(account_color(5), ACCOUNT_COLORS[1]);
        assert_eq!(bill_color(7), BILL_COLORS[3]);
        assert_eq!(budget_color(9), BUDGET_COLORS[1]);
    }
}
