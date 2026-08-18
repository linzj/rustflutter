// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/shrine/supplemental/cut_corners_border.dart`
//! (flutter/gallery @ d12640d): the `CutCornersBorder` outline input border.
//!
//! Upstream this is an `OutlineInputBorder` subclass -- a rectangle with its
//! four corners cut at 45 degrees, drawn as a stroke around a text field,
//! with a gap in the top edge for the floating label. The label gap machinery
//! (`_notchedCornerPath(center, start, extent)`, `gapPadding`, the lerp
//! overrides) supports a floating-placeholder animation the framework's
//! `TextField` has no counterpart of, so what is ported is the border's
//! shape and its stroke, which is what the login page's fields show. The
//! shape is [`cut_corners_path`], the stroke is [`CutCornersOutline`], and
//! the login page wraps its fields in it.

use rustflutter::prelude::*;
use rustflutter::render::{
    BoxConstraints, BoxedRender, HitTestResult, Offset, PaintContext, RenderBox, RenderRef, Size,
};

/// The default cut, upstream's `cut = 7`.
pub const DEFAULT_CUT: f32 = 7.0;

/// Upstream's `_notchedCornerPath` without a label gap: the border's outline
/// around `rect`, each corner replaced by a diagonal `cut` logical pixels
/// long, walked clockwise from the top edge (`_notchedSidesAndBottom`).
pub fn cut_corners_points(rect: Rect, cut: f32) -> [(f32, f32); 8] {
    [
        (rect.left + cut, rect.top),
        (rect.right - cut, rect.top),
        (rect.right, rect.top + cut),
        (rect.right, rect.bottom - cut),
        (rect.right - cut, rect.bottom),
        (rect.left + cut, rect.bottom),
        (rect.left, rect.bottom - cut),
        (rect.left, rect.top + cut),
    ]
}

/// The outline as a closed path, from [`cut_corners_points`].
pub fn cut_corners_path(rect: Rect, cut: f32) -> RenderPath {
    let points = cut_corners_points(rect, cut);
    let mut path = RenderPath::new();
    path.move_to(points[0].0, points[0].1);
    for &(x, y) in &points[1..] {
        path.line_to(x, y);
    }
    path.close();
    path
}

/// A box that strokes a cut-corners outline around its child.
///
/// Upstream draws the border as part of the field's `InputDecoration`; here
/// it is a wrapper render object, because the framework's `TextField` draws
/// text and caret only.
pub struct CutCornersOutline {
    child: BoxedRender,
    color: Color,
    width: f32,
    cut: f32,
    size: Size,
}

impl CutCornersOutline {
    pub fn new(child: impl RenderBox + 'static, color: Color) -> CutCornersOutline {
        // Upstream's `InputDecorationTheme(border: CutCornersBorder(
        //   borderSide: BorderSide(color: shrineBrown900, width: 0.5)))`.
        CutCornersOutline {
            child: RenderRef::new(child),
            color,
            width: 0.5,
            cut: DEFAULT_CUT,
            size: Size::ZERO,
        }
    }
}

impl RenderBox for CutCornersOutline {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout_child(constraints, true);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset);
        // The stroke is centered on the path, so the path runs half a stroke
        // width inside the bounds, the way `BorderSide` insets to
        // `middleRect`.
        let inset = self.width / 2.0;
        let rect = Rect::ltrb(
            offset.dx + inset,
            offset.dy + inset,
            offset.dx + self.size.width - inset,
            offset.dy + self.size.height - inset,
        );
        let path = cut_corners_path(rect, self.cut);
        let paint = Paint::new(self.color).with_style(Style::Stroke { width: self.width });
        context.canvas().draw_path(&path, &paint);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
    }

    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.child.hit_test(position, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_cuts_all_four_corners() {
        // A 100x50 rect with the default 7px cut: the path's vertices are
        // the eight cut endpoints, walked clockwise from the top edge.
        let rect = Rect::xywh(10.0, 20.0, 100.0, 50.0);
        let expected = [
            (17.0, 20.0),
            (103.0, 20.0),
            (110.0, 27.0),
            (110.0, 63.0),
            (103.0, 70.0),
            (17.0, 70.0),
            (10.0, 63.0),
            (10.0, 27.0),
        ];
        assert_eq!(cut_corners_points(rect, DEFAULT_CUT), expected);
    }
}
