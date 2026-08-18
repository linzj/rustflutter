// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/border_tab_indicator.dart` (flutter/gallery
//! @ d12640d): `BorderTabIndicator`, the white pill outline Crane draws around
//! the selected tab instead of Material's underline.
//!
//! Upstream this is a `Decoration` whose `BoxPainter` computes a rect from
//! the tab's bounds and strokes it. The framework's `TabBar` paints its own
//! underline and takes no custom indicator, so the drawing half lives where
//! the tab is drawn (`backdrop.rs`'s app bar); what is here is the geometry
//! and the paint constants, kept as pure functions so the arithmetic upstream
//! wrote down is the arithmetic that is tested.

use rustflutter::prelude::{Color, Rect};

/// Upstream's `Radius.circular(56)`.
pub const INDICATOR_RADIUS: f32 = 56.0;
/// Upstream's `paint.strokeWidth = 2`.
pub const INDICATOR_STROKE_WIDTH: f32 = 2.0;
/// Upstream's `paint.color = Colors.white`.
pub const INDICATOR_COLOR: Color = Color::WHITE;

/// Upstream's `BorderPainter.paint`: the rect the pill is stroked into,
/// given the tab's own bounds.
///
/// The tab is inset horizontally by `16 - 4 * textScaleFactor` on each side
/// and the pill is centred vertically, one pixel above the midpoint --
/// upstream's `- 1`, mirrored as written.
pub fn indicator_rect(tab: Rect, indicator_height: f32, text_scale_factor: f32) -> Rect {
    let horizontal_inset = 16.0 - 4.0 * text_scale_factor;
    Rect::xywh(
        tab.left + horizontal_inset,
        tab.top + (tab.height() / 2.0) - indicator_height / 2.0 - 1.0,
        tab.width() - 2.0 * horizontal_inset,
        indicator_height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_centres_on_the_tab_one_pixel_high() {
        // A 100x46 tab at the origin, mobile's 32-tall indicator at the
        // default text scale.
        let rect = indicator_rect(Rect::xywh(0.0, 0.0, 100.0, 46.0), 32.0, 1.0);
        assert_eq!(rect.left, 12.0);
        assert_eq!(rect.width(), 76.0);
        // 23 - 16 - 1 = 6, and the pill's own centre is 6 + 16 = 22, one
        // above the tab's 23.
        assert_eq!(rect.top, 6.0);
        assert_eq!(rect.height(), 32.0);
    }

    #[test]
    fn a_larger_text_scale_narrows_the_pill() {
        let tab = Rect::xywh(0.0, 0.0, 120.0, 46.0);
        let normal = indicator_rect(tab, 32.0, 1.0);
        let larger = indicator_rect(tab, 32.0, 2.0);
        assert_eq!(larger.left, 8.0);
        assert!(larger.width() > normal.width());
    }

    #[test]
    fn the_pill_keeps_the_tab_s_offset() {
        let rect = indicator_rect(Rect::xywh(40.0, 10.0, 80.0, 46.0), 28.0, 1.0);
        assert_eq!(rect.left, 52.0);
        assert_eq!(rect.top, 10.0 + 23.0 - 14.0 - 1.0);
    }
}
