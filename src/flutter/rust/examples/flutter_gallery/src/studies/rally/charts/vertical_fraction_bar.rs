// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/rally/charts/vertical_fraction_bar.dart`
//! (flutter/gallery @ d12640d), upstream's `VerticalFractionBar`.
//!
//! Upstream is a `LayoutBuilder` over a `Column` of two `Container`s; here it
//! is one render box that paints the two rectangles, the split being the only
//! thing either version computes.

use rustflutter::engine::{Color, Paint, Rect};
use rustflutter::framework::{AnyWidget, leaf};
use rustflutter::render::{BoxConstraints, Offset, PaintContext, RenderBox, Size};

/// Upstream's bar width: `SizedBox(width: 4)`.
pub const BAR_WIDTH: f32 = 4.0;

/// Upstream's `VerticalFractionBar`: a 4-wide bar, `fraction` of it filled
/// with `color` from the bottom, the rest black.
pub fn vertical_fraction_bar(color: Color, fraction: f32) -> AnyWidget {
    let fraction = fraction.clamp(0.0, 1.0);
    leaf(move || VerticalFractionBarRender {
        color,
        fraction,
        size: Size::ZERO,
    })
}

struct VerticalFractionBarRender {
    color: Color,
    fraction: f32,
    size: Size,
}

impl RenderBox for VerticalFractionBarRender {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // `SizedBox(height: constraints.maxHeight, width: 4)`.
        let height = if constraints.max_height.is_finite() {
            constraints.max_height
        } else {
            0.0
        };
        self.size = constraints.constrain(Size::new(BAR_WIDTH, height));
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let canvas = context.canvas();
        let empty = (1.0 - self.fraction) * self.size.height;
        // The unfilled part on top is black, the filled part below it is the
        // segment's color -- upstream's two `Container`s.
        if empty > 0.0 {
            canvas.draw_rect(
                Rect::xywh(offset.dx, offset.dy, self.size.width, empty),
                &Paint::new(Color::BLACK),
            );
        }
        let filled = self.fraction * self.size.height;
        if filled > 0.0 {
            canvas.draw_rect(
                Rect::xywh(offset.dx, offset.dy + empty, self.size.width, filled),
                &Paint::new(self.color),
            );
        }
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        let height = if constraints.max_height.is_finite() {
            constraints.max_height
        } else {
            0.0
        };
        constraints.constrain(Size::new(BAR_WIDTH, height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;

    #[test]
    fn the_bar_is_four_wide_and_as_tall_as_offered() {
        let mut tree = ElementTree::new();
        tree.rebuild(vertical_fraction_bar(Color::WHITE, 0.5));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::new(0.0, 100.0, 0.0, 32.0));
        assert_eq!(size, Size::new(BAR_WIDTH, 32.0));
    }

    #[test]
    fn the_fraction_splits_the_height() {
        // 60% of 40px filled from the bottom: 16px empty over 24px filled.
        let mut bar = VerticalFractionBarRender {
            color: Color::WHITE,
            fraction: 0.6,
            size: Size::ZERO,
        };
        let size = bar.layout(BoxConstraints::new(0.0, 100.0, 0.0, 40.0));
        assert_eq!(size.height, 40.0);
        // f32: 1.0 - 0.6 is not exactly 0.4.
        assert!(((1.0 - bar.fraction) * size.height - 16.0).abs() < 1e-4);
        assert!((bar.fraction * size.height - 24.0).abs() < 1e-4);
    }
}
