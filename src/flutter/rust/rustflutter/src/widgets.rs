// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! A minimal widget layer over the engine boundary.
//!
//! This is the Rust counterpart of `packages/flutter/lib/src/rendering`
//! (52,223 lines upstream) reduced to the part that makes the protocol
//! observable: **constraints go down, sizes come up, the parent positions the
//! child**. Everything here paints through [`crate::engine::Canvas`], so the
//! output goes into a real `DisplayList` and through the engine's own
//! rasterizer.
//!
//! It is deliberately small. The point of M1 is to prove the boundary works
//! end to end, not to reimplement the widget catalogue.

use crate::engine::{Canvas, Color, Paint, Paragraph, Rect, TextAlign, TextStyle};

/// Box constraints, mirroring `BoxConstraints` in the upstream framework.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl Constraints {
    pub const fn tight(width: f32, height: f32) -> Constraints {
        Constraints {
            min_width: width,
            max_width: width,
            min_height: height,
            max_height: height,
        }
    }

    pub const fn loose(max_width: f32, max_height: f32) -> Constraints {
        Constraints {
            min_width: 0.0,
            max_width,
            min_height: 0.0,
            max_height,
        }
    }

    pub fn loosen(&self) -> Constraints {
        Constraints {
            min_width: 0.0,
            max_width: self.max_width,
            min_height: 0.0,
            max_height: self.max_height,
        }
    }

    /// Clamps a desired size into this constraint box.
    pub fn constrain(&self, size: Size) -> Size {
        Size {
            width: size.width.clamp(self.min_width, self.max_width),
            height: size.height.clamp(self.min_height, self.max_height),
        }
    }

    pub fn deflate(&self, horizontal: f32, vertical: f32) -> Constraints {
        Constraints {
            min_width: (self.min_width - horizontal).max(0.0),
            max_width: (self.max_width - horizontal).max(0.0),
            min_height: (self.min_height - vertical).max(0.0),
            max_height: (self.max_height - vertical).max(0.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Size {
        Size { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Offset {
    pub dx: f32,
    pub dy: f32,
}

impl Offset {
    pub const ZERO: Offset = Offset { dx: 0.0, dy: 0.0 };

    pub const fn new(dx: f32, dy: f32) -> Offset {
        Offset { dx, dy }
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Offset {
        Offset { dx: self.dx + dx, dy: self.dy + dy }
    }
}

/// Symmetric or per-side insets.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct EdgeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    pub const fn all(value: f32) -> EdgeInsets {
        EdgeInsets { left: value, top: value, right: value, bottom: value }
    }

    pub const fn symmetric(horizontal: f32, vertical: f32) -> EdgeInsets {
        EdgeInsets {
            left: horizontal,
            top: vertical,
            right: horizontal,
            bottom: vertical,
        }
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// The layout/paint protocol.
///
/// `layout` is called once per frame with the parent's constraints and returns
/// the size the widget chose; `paint` is then called with the offset the parent
/// decided. Same contract as `RenderBox` upstream.
pub trait Widget {
    fn layout(&mut self, constraints: Constraints) -> Size;
    fn paint(&self, canvas: &mut Canvas, offset: Offset);
}

pub type BoxedWidget = Box<dyn Widget>;

// -- Text ---------------------------------------------------------------------

/// A run of text, shaped by the engine's `txt` / skparagraph stack.
pub struct Text {
    content: String,
    style: TextStyle,
    paragraph: Option<Paragraph>,
    size: Size,
}

impl Text {
    pub fn new(content: impl Into<String>) -> Text {
        Text {
            content: content.into(),
            style: TextStyle::default(),
            paragraph: None,
            size: Size::default(),
        }
    }

    pub fn with_style(mut self, style: TextStyle) -> Text {
        self.style = style;
        self
    }

    pub fn with_size(mut self, font_size: f32) -> Text {
        self.style.font_size = font_size;
        self
    }

    pub fn with_color(mut self, color: Color) -> Text {
        self.style.color = color;
        self
    }

    pub fn with_weight(mut self, weight: i32) -> Text {
        self.style.font_weight = weight;
        self
    }

    pub fn centered(mut self) -> Text {
        self.style.align = TextAlign::Center;
        self
    }
}

impl Widget for Text {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let paragraph = Paragraph::new(&self.content, &self.style, constraints.max_width);
        // Paragraph::new re-lays out at the ink width, so width() is now the
        // tight box around the glyphs. That is what makes centring a Text
        // inside a Center actually look centred.
        self.size = constraints.constrain(Size::new(paragraph.width(), paragraph.height()));
        self.paragraph = Some(paragraph);
        self.size
    }

    fn paint(&self, canvas: &mut Canvas, offset: Offset) {
        if let Some(paragraph) = &self.paragraph {
            canvas.draw_paragraph(paragraph, offset.dx, offset.dy);
        }
    }
}

// -- Container ----------------------------------------------------------------

/// A painted box that optionally pads and wraps a child.
pub struct Container {
    color: Option<Color>,
    corner_radius: f32,
    padding: EdgeInsets,
    width: Option<f32>,
    height: Option<f32>,
    child: Option<BoxedWidget>,
    size: Size,
    child_offset: Offset,
}

impl Container {
    pub fn new() -> Container {
        Container {
            color: None,
            corner_radius: 0.0,
            padding: EdgeInsets::default(),
            width: None,
            height: None,
            child: None,
            size: Size::default(),
            child_offset: Offset::ZERO,
        }
    }

    pub fn with_color(mut self, color: Color) -> Container {
        self.color = Some(color);
        self
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Container {
        self.corner_radius = radius;
        self
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Container {
        self.padding = padding;
        self
    }

    pub fn with_size(mut self, width: f32, height: f32) -> Container {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    pub fn with_child(mut self, child: impl Widget + 'static) -> Container {
        self.child = Some(Box::new(child));
        self
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Container {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let inner = constraints
            .loosen()
            .deflate(self.padding.horizontal(), self.padding.vertical());

        let child_size = match &mut self.child {
            Some(child) => child.layout(inner),
            None => Size::default(),
        };

        let desired = Size::new(
            self.width
                .unwrap_or(child_size.width + self.padding.horizontal()),
            self.height
                .unwrap_or(child_size.height + self.padding.vertical()),
        );
        self.size = constraints.constrain(desired);

        // Centre the child in whatever space is left after padding, so an
        // explicitly sized Container still positions its child sensibly.
        let free_width = (self.size.width - self.padding.horizontal() - child_size.width).max(0.0);
        let free_height = (self.size.height - self.padding.vertical() - child_size.height).max(0.0);
        self.child_offset = Offset::new(
            self.padding.left + free_width / 2.0,
            self.padding.top + free_height / 2.0,
        );

        self.size
    }

    fn paint(&self, canvas: &mut Canvas, offset: Offset) {
        if let Some(color) = self.color {
            let rect = Rect::xywh(offset.dx, offset.dy, self.size.width, self.size.height);
            let paint = Paint::new(color);
            if self.corner_radius > 0.0 {
                canvas.draw_rounded_rect(rect, self.corner_radius, &paint);
            } else {
                canvas.draw_rect(rect, &paint);
            }
        }
        if let Some(child) = &self.child {
            child.paint(
                canvas,
                offset.translate(self.child_offset.dx, self.child_offset.dy),
            );
        }
    }
}

// -- Center -------------------------------------------------------------------

/// Expands to fill its constraints and centres its child inside them.
pub struct Center {
    child: BoxedWidget,
    size: Size,
    child_offset: Offset,
}

impl Center {
    pub fn new(child: impl Widget + 'static) -> Center {
        Center {
            child: Box::new(child),
            size: Size::default(),
            child_offset: Offset::ZERO,
        }
    }
}

impl Widget for Center {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let child_size = self.child.layout(constraints.loosen());
        self.size = Size::new(constraints.max_width, constraints.max_height);
        self.child_offset = Offset::new(
            ((self.size.width - child_size.width) / 2.0).max(0.0),
            ((self.size.height - child_size.height) / 2.0).max(0.0),
        );
        self.size
    }

    fn paint(&self, canvas: &mut Canvas, offset: Offset) {
        self.child.paint(
            canvas,
            offset.translate(self.child_offset.dx, self.child_offset.dy),
        );
    }
}

// -- Column -------------------------------------------------------------------

/// Stacks children vertically, centring each horizontally.
pub struct Column {
    children: Vec<BoxedWidget>,
    spacing: f32,
    placements: Vec<Offset>,
    size: Size,
}

impl Column {
    pub fn new() -> Column {
        Column {
            children: Vec::new(),
            spacing: 0.0,
            placements: Vec::new(),
            size: Size::default(),
        }
    }

    pub fn with_spacing(mut self, spacing: f32) -> Column {
        self.spacing = spacing;
        self
    }

    pub fn push(mut self, child: impl Widget + 'static) -> Column {
        self.children.push(Box::new(child));
        self
    }
}

impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Column {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let inner = constraints.loosen();
        let mut sizes = Vec::with_capacity(self.children.len());
        let mut total_height = 0.0f32;
        let mut widest = 0.0f32;

        for (i, child) in self.children.iter_mut().enumerate() {
            let size = child.layout(inner);
            if i > 0 {
                total_height += self.spacing;
            }
            total_height += size.height;
            widest = widest.max(size.width);
            sizes.push(size);
        }

        self.size = constraints.constrain(Size::new(widest, total_height));

        self.placements.clear();
        let mut y = 0.0f32;
        for (i, size) in sizes.iter().enumerate() {
            if i > 0 {
                y += self.spacing;
            }
            self.placements
                .push(Offset::new((self.size.width - size.width) / 2.0, y));
            y += size.height;
        }

        self.size
    }

    fn paint(&self, canvas: &mut Canvas, offset: Offset) {
        for (child, placement) in self.children.iter().zip(self.placements.iter()) {
            child.paint(canvas, offset.translate(placement.dx, placement.dy));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedBox(Size);

    impl Widget for FixedBox {
        fn layout(&mut self, constraints: Constraints) -> Size {
            constraints.constrain(self.0)
        }
        fn paint(&self, _canvas: &mut Canvas, _offset: Offset) {}
    }

    #[test]
    fn constraints_clamp_desired_size() {
        let c = Constraints::loose(100.0, 50.0);
        assert_eq!(
            c.constrain(Size::new(200.0, 10.0)),
            Size::new(100.0, 10.0)
        );
    }

    #[test]
    fn center_reports_full_size_and_centres_child() {
        let mut center = Center::new(FixedBox(Size::new(40.0, 20.0)));
        let size = center.layout(Constraints::tight(100.0, 100.0));
        assert_eq!(size, Size::new(100.0, 100.0));
        assert_eq!(center.child_offset, Offset::new(30.0, 40.0));
    }

    #[test]
    fn column_stacks_children_with_spacing() {
        let mut column = Column::new()
            .with_spacing(10.0)
            .push(FixedBox(Size::new(30.0, 20.0)))
            .push(FixedBox(Size::new(50.0, 20.0)));
        let size = column.layout(Constraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(50.0, 50.0));
        assert_eq!(column.placements[0], Offset::new(10.0, 0.0));
        assert_eq!(column.placements[1], Offset::new(0.0, 30.0));
    }

    #[test]
    fn container_padding_grows_the_box() {
        let mut container = Container::new()
            .with_padding(EdgeInsets::all(8.0))
            .with_child(FixedBox(Size::new(20.0, 10.0)));
        let size = container.layout(Constraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(36.0, 26.0));
    }
}
