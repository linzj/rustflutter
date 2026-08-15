// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The rendering layer: geometry, the box protocol, and the render objects.
//!
//! This is the Rust counterpart of `packages/flutter/lib/src/rendering`. The
//! protocol is upstream's, unchanged, because it is the part that makes layout
//! composable at all:
//!
//! > **Constraints go down, sizes come up, the parent positions the child.**
//!
//! A parent hands each child a [`BoxConstraints`]; the child picks a [`Size`]
//! inside them and reports it; the parent then decides where the child sits and
//! records that offset. Nothing reads its parent, which is what lets a subtree
//! be laid out in isolation.
//!
//! # What is here and what is not
//!
//! Upstream `RenderObject` carries a parent pointer so that `markNeedsLayout`
//! can walk up to the nearest relayout boundary and dirty only that. This tree
//! is owned top-down -- a parent owns its children as `Box<dyn RenderBox>` --
//! and lays out in full each frame. That is a real cost on deep trees and the
//! obvious thing to fix next; it is not a correctness compromise, and it keeps
//! the ownership story simple enough to read.
//!
//! Hit testing is here rather than with input, because only a render object
//! knows its own geometry. [`RenderBox::hit_test`] walks the tree back to front
//! and records the entries a gesture recogniser will later arbitrate over.

use std::rc::Rc;

use crate::engine::{Canvas, Color, Paint, Paragraph, Rect, Style, TextStyle};
use crate::gestures::PointerHandlers;
use crate::painting::{ClipOp, Gradient, Image, RenderPath};

// -- Geometry -----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Size = Size { width: 0.0, height: 0.0 };

    pub const fn new(width: f32, height: f32) -> Size {
        Size { width, height }
    }

    pub const fn square(side: f32) -> Size {
        Size { width: side, height: side }
    }

    pub fn contains(&self, point: Offset) -> bool {
        point.dx >= 0.0 && point.dy >= 0.0 && point.dx < self.width && point.dy < self.height
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

    pub fn plus(&self, other: Offset) -> Offset {
        Offset { dx: self.dx + other.dx, dy: self.dy + other.dy }
    }

    pub fn minus(&self, other: Offset) -> Offset {
        Offset { dx: self.dx - other.dx, dy: self.dy - other.dy }
    }
}

/// Per-side insets.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct EdgeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    pub const ZERO: EdgeInsets = EdgeInsets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };

    pub const fn all(value: f32) -> EdgeInsets {
        EdgeInsets { left: value, top: value, right: value, bottom: value }
    }

    pub const fn symmetric(horizontal: f32, vertical: f32) -> EdgeInsets {
        EdgeInsets { left: horizontal, top: vertical, right: horizontal, bottom: vertical }
    }

    pub const fn only(left: f32, top: f32, right: f32, bottom: f32) -> EdgeInsets {
        EdgeInsets { left, top, right, bottom }
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// The size a box may choose, as a range on each axis.
///
/// `f32::INFINITY` on a maximum means unbounded -- which a child must not
/// return as its own size. A scroll viewport hands its child unbounded
/// constraints on the scroll axis for exactly this reason: the child sizes
/// itself to its content and the viewport shows a window onto it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxConstraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl BoxConstraints {
    pub const fn new(min_width: f32, max_width: f32, min_height: f32, max_height: f32) -> Self {
        BoxConstraints { min_width, max_width, min_height, max_height }
    }

    /// Exactly this size, no choice.
    pub const fn tight(width: f32, height: f32) -> Self {
        BoxConstraints {
            min_width: width,
            max_width: width,
            min_height: height,
            max_height: height,
        }
    }

    pub const fn tight_for(size: Size) -> Self {
        Self::tight(size.width, size.height)
    }

    /// At most this size, and any smaller size is allowed.
    pub const fn loose(max_width: f32, max_height: f32) -> Self {
        BoxConstraints { min_width: 0.0, max_width, min_height: 0.0, max_height }
    }

    /// Unbounded on both axes.
    pub const fn unbounded() -> Self {
        BoxConstraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }

    /// Same maxima, no minima. What a parent hands a child it intends to
    /// position itself rather than stretch.
    pub fn loosen(&self) -> Self {
        BoxConstraints {
            min_width: 0.0,
            max_width: self.max_width,
            min_height: 0.0,
            max_height: self.max_height,
        }
    }

    /// Shrinks by `insets` on each axis, never below zero.
    pub fn deflate(&self, insets: EdgeInsets) -> Self {
        let horizontal = insets.horizontal();
        let vertical = insets.vertical();
        BoxConstraints {
            min_width: (self.min_width - horizontal).max(0.0),
            max_width: (self.max_width - horizontal).max(0.0),
            min_height: (self.min_height - vertical).max(0.0),
            max_height: (self.max_height - vertical).max(0.0),
        }
    }

    /// Clamps a desired size into this box. Every render object should return
    /// `constraints.constrain(...)` rather than its raw preference.
    pub fn constrain(&self, size: Size) -> Size {
        Size {
            width: size.width.clamp(self.min_width, self.max_width),
            height: size.height.clamp(self.min_height, self.max_height),
        }
    }

    /// The largest size these constraints allow. Infinite maxima collapse to
    /// the minimum, since a box cannot be infinitely large.
    pub fn biggest(&self) -> Size {
        Size {
            width: if self.max_width.is_finite() { self.max_width } else { self.min_width },
            height: if self.max_height.is_finite() { self.max_height } else { self.min_height },
        }
    }

    pub fn smallest(&self) -> Size {
        Size { width: self.min_width, height: self.min_height }
    }

    /// Narrows these constraints to fit inside `other`. Used where a child's
    /// own preference must not escape its parent's limits.
    pub fn enforce(&self, other: BoxConstraints) -> Self {
        BoxConstraints {
            min_width: self.min_width.clamp(other.min_width, other.max_width),
            max_width: self.max_width.clamp(other.min_width, other.max_width),
            min_height: self.min_height.clamp(other.min_height, other.max_height),
            max_height: self.max_height.clamp(other.min_height, other.max_height),
        }
    }

    pub fn has_tight_width(&self) -> bool {
        self.min_width >= self.max_width
    }

    pub fn has_tight_height(&self) -> bool {
        self.min_height >= self.max_height
    }

    pub fn is_tight(&self) -> bool {
        self.has_tight_width() && self.has_tight_height()
    }

    pub fn has_bounded_width(&self) -> bool {
        self.max_width.is_finite()
    }

    pub fn has_bounded_height(&self) -> bool {
        self.max_height.is_finite()
    }
}

/// Kept for source compatibility with the M1 API.
pub type Constraints = BoxConstraints;

impl BoxConstraints {
    /// The M1 spelling of [`BoxConstraints::deflate`], taking bare numbers.
    pub fn deflate_by(&self, horizontal: f32, vertical: f32) -> Self {
        self.deflate(EdgeInsets::symmetric(horizontal / 2.0, vertical / 2.0))
    }
}

// -- Painting -----------------------------------------------------------------

/// What a render object paints into.
///
/// Upstream this also decides where to cut a new compositing layer. Here it is
/// a canvas plus the estimated bounds of the frame, which
/// [`RenderClipRect`] and friends need in order to clip sensibly.
pub struct PaintContext<'a> {
    pub canvas: &'a mut Canvas,
}

impl<'a> PaintContext<'a> {
    pub fn new(canvas: &'a mut Canvas) -> PaintContext<'a> {
        PaintContext { canvas }
    }

    /// Paints `child` at `offset`, which is where every parent should route a
    /// child rather than calling `paint` directly -- it is the one place a
    /// future repaint boundary can be inserted.
    pub fn paint_child(&mut self, child: &dyn RenderBox, offset: Offset) {
        child.paint(self, offset);
    }
}

// -- Hit testing --------------------------------------------------------------

/// One render object that a pointer landed on, innermost first.
///
/// Not `Debug`: the handlers are closures, which have nothing useful to print.
#[derive(Clone, Default)]
pub struct HitTestEntry {
    /// Identifies the render object that was hit. Assigned by whoever built the
    /// tree; zero means "no identity", which a gesture layer should ignore.
    pub target: u64,
    /// The pointer position in that object's local coordinates.
    pub local_position: Offset,
    /// What the object wants to hear about. Carried in the entry rather than
    /// looked up afterwards, because by then the tree may already have been
    /// replaced by the next frame's.
    pub handlers: Option<Rc<PointerHandlers>>,
}

/// The path a pointer takes through the tree, innermost entry first.
///
/// Upstream this is `HitTestResult`, and the ordering is what makes gesture
/// arena resolution work: the deepest target gets first refusal.
#[derive(Clone, Default)]
pub struct HitTestResult {
    pub path: Vec<HitTestEntry>,
}

impl HitTestResult {
    pub fn new() -> HitTestResult {
        HitTestResult { path: Vec::new() }
    }

    /// Records a target. A target of zero is dropped: it means the object has
    /// no identity, and letting those into the path would bury the real
    /// innermost target behind every anonymous box that happened to contain
    /// the pointer.
    pub fn add(&mut self, target: u64, local_position: Offset) {
        self.add_with_handlers(target, local_position, None);
    }

    pub fn add_with_handlers(
        &mut self,
        target: u64,
        local_position: Offset,
        handlers: Option<Rc<PointerHandlers>>,
    ) {
        if target == 0 {
            return;
        }
        self.path.push(HitTestEntry { target, local_position, handlers });
    }

    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }

    /// The innermost target, which is the one a tap should go to first.
    pub fn innermost(&self) -> Option<HitTestEntry> {
        self.path.first().cloned()
    }
}

// -- The protocol -------------------------------------------------------------

/// A box in the render tree.
///
/// Implementors must obey three rules, all of which the built-in objects here
/// follow and all of which upstream also requires:
///
/// 1. `layout` returns a size inside the constraints it was given.
/// 2. `size` returns what the last `layout` returned.
/// 3. `paint` draws at the offset it is given and nowhere else; a render object
///    never knows its absolute position.
pub trait RenderBox {
    /// Chooses a size for the given constraints, laying out children as needed.
    fn layout(&mut self, constraints: BoxConstraints) -> Size;

    /// The size chosen by the last `layout`.
    fn size(&self) -> Size;

    fn paint(&self, context: &mut PaintContext, offset: Offset);

    /// Records the objects under `position` (local coordinates), innermost
    /// first. Returns whether this object or a descendant was hit.
    ///
    /// The default tests the box itself and stops there, which is right for a
    /// leaf; anything with children should override and test them first, since
    /// children paint on top.
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if self.size().contains(position) {
            result.add(self.hit_test_id(), position);
            true
        } else {
            false
        }
    }

    /// Identity recorded in a [`HitTestResult`]. Zero -- the default -- means
    /// this object is not an event target and only routes to its children.
    fn hit_test_id(&self) -> u64 {
        0
    }

    // -- Intrinsics -----------------------------------------------------------
    //
    // What the box would like, ignoring the constraints it will actually get.
    // Flex uses these to size children before it knows the free space, and
    // IntrinsicWidth/Height exist to expose them directly. They are allowed to
    // be expensive; upstream warns about exactly this.

    /// The narrowest width at which nothing is clipped.
    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// The width at which adding more would not help.
    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    fn min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Distance from the top of this box to the text baseline it should be
    /// aligned on, or None if it has no baseline.
    fn distance_to_baseline(&self) -> Option<f32> {
        None
    }
}

pub type BoxedRender = Box<dyn RenderBox>;

/// So a boxed render object works anywhere an unboxed one does.
impl<R: RenderBox + ?Sized> RenderBox for Box<R> {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        (**self).layout(constraints)
    }
    fn size(&self) -> Size {
        (**self).size()
    }
    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        (**self).paint(context, offset)
    }
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        (**self).hit_test(position, result)
    }
    fn hit_test_id(&self) -> u64 {
        (**self).hit_test_id()
    }
    fn min_intrinsic_width(&self, height: f32) -> f32 {
        (**self).min_intrinsic_width(height)
    }
    fn max_intrinsic_width(&self, height: f32) -> f32 {
        (**self).max_intrinsic_width(height)
    }
    fn min_intrinsic_height(&self, width: f32) -> f32 {
        (**self).min_intrinsic_height(width)
    }
    fn max_intrinsic_height(&self, width: f32) -> f32 {
        (**self).max_intrinsic_height(width)
    }
    fn distance_to_baseline(&self) -> Option<f32> {
        (**self).distance_to_baseline()
    }
}

// -- Alignment ----------------------------------------------------------------

/// A point in a box, as a fraction: (-1, -1) top-left, (0, 0) centre,
/// (1, 1) bottom-right. Matches upstream's `Alignment`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alignment {
    pub x: f32,
    pub y: f32,
}

impl Alignment {
    pub const TOP_LEFT: Alignment = Alignment { x: -1.0, y: -1.0 };
    pub const TOP_CENTER: Alignment = Alignment { x: 0.0, y: -1.0 };
    pub const TOP_RIGHT: Alignment = Alignment { x: 1.0, y: -1.0 };
    pub const CENTER_LEFT: Alignment = Alignment { x: -1.0, y: 0.0 };
    pub const CENTER: Alignment = Alignment { x: 0.0, y: 0.0 };
    pub const CENTER_RIGHT: Alignment = Alignment { x: 1.0, y: 0.0 };
    pub const BOTTOM_LEFT: Alignment = Alignment { x: -1.0, y: 1.0 };
    pub const BOTTOM_CENTER: Alignment = Alignment { x: 0.0, y: 1.0 };
    pub const BOTTOM_RIGHT: Alignment = Alignment { x: 1.0, y: 1.0 };

    pub const fn new(x: f32, y: f32) -> Alignment {
        Alignment { x, y }
    }

    /// Where a `child` sits inside a box of `size`.
    pub fn inscribe(&self, child: Size, size: Size) -> Offset {
        let free_width = (size.width - child.width).max(0.0);
        let free_height = (size.height - child.height).max(0.0);
        Offset::new(
            free_width * (self.x + 1.0) / 2.0,
            free_height * (self.y + 1.0) / 2.0,
        )
    }
}

impl Default for Alignment {
    fn default() -> Alignment {
        Alignment::CENTER
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Axis {
    Horizontal,
    #[default]
    Vertical,
}

/// How the free space along the main axis is distributed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MainAxisAlignment {
    #[default]
    Start,
    End,
    Center,
    /// Free space between children, none at the ends.
    SpaceBetween,
    /// Half a gap at each end, a full gap between children.
    SpaceAround,
    /// Equal gaps everywhere, including the ends.
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CrossAxisAlignment {
    Start,
    End,
    #[default]
    Center,
    /// Children are given a tight cross-axis constraint.
    Stretch,
    /// Children are aligned on their text baselines. Falls back to Start for
    /// children that have no baseline.
    Baseline,
}

/// Whether the flex shrinks to its children or fills its constraints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MainAxisSize {
    Min,
    #[default]
    Max,
}

// -- Leaf: a solid or gradient-filled box -------------------------------------

/// How a [`RenderDecoratedBox`] fills itself.
#[derive(Clone, Debug)]
pub enum Fill {
    Solid(Color),
    Linear { start: Alignment, end: Alignment, gradient: Gradient },
    Radial { center: Alignment, radius: f32, gradient: Gradient },
}

/// Paints a background behind an optional child.
///
/// Upstream this is `RenderDecoratedBox` with a `BoxDecoration`. Border radius,
/// fill and a border are the parts that earn their place; shadows are a paint
/// blur away and are left to the caller.
pub struct RenderDecoratedBox {
    fill: Option<Fill>,
    corner_radius: f32,
    border_width: f32,
    border_color: Color,
    child: Option<BoxedRender>,
    size: Size,
}

impl RenderDecoratedBox {
    pub fn new() -> RenderDecoratedBox {
        RenderDecoratedBox {
            fill: None,
            corner_radius: 0.0,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            child: None,
            size: Size::ZERO,
        }
    }

    pub fn with_fill(mut self, fill: Fill) -> Self {
        self.fill = Some(fill);
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.fill = Some(Fill::Solid(color));
        self
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }

    pub fn with_border(mut self, width: f32, color: Color) -> Self {
        self.border_width = width;
        self.border_color = color;
        self
    }

    pub fn with_child(mut self, child: impl RenderBox + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    fn paint_rect(&self, offset: Offset) -> Rect {
        Rect::xywh(offset.dx, offset.dy, self.size.width, self.size.height)
    }

    fn build_paint(&self, rect: Rect) -> Option<Paint> {
        match self.fill.as_ref()? {
            Fill::Solid(color) => Some(Paint::new(*color)),
            Fill::Linear { start, end, gradient } => {
                let from = point_in(rect, *start);
                let to = point_in(rect, *end);
                Some(Paint::new(Color::WHITE).with_linear_gradient(from, to, gradient))
            }
            Fill::Radial { center, radius, gradient } => {
                let at = point_in(rect, *center);
                Some(Paint::new(Color::WHITE).with_radial_gradient(at, *radius, gradient))
            }
        }
    }
}

fn point_in(rect: Rect, alignment: Alignment) -> (f32, f32) {
    (
        rect.left + rect.width() * (alignment.x + 1.0) / 2.0,
        rect.top + rect.height() * (alignment.y + 1.0) / 2.0,
    )
}

impl Default for RenderDecoratedBox {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderBox for RenderDecoratedBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = match &mut self.child {
            Some(child) => child.layout(constraints),
            // With no child there is nothing to measure, so the box takes as
            // much as it is allowed -- the same choice upstream makes.
            None => constraints.biggest(),
        };
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let rect = self.paint_rect(offset);
        if let Some(paint) = self.build_paint(rect) {
            if self.corner_radius > 0.0 {
                context.canvas.draw_rounded_rect(rect, self.corner_radius, &paint);
            } else {
                context.canvas.draw_rect(rect, &paint);
            }
        }
        if let Some(child) = &self.child {
            context.paint_child(child.as_ref(), offset);
        }
        if self.border_width > 0.0 {
            // Stroked on the boundary, so half the width falls outside. Insetting
            // by half keeps the border inside the box, which is what a caller who
            // sized the box expects.
            let half = self.border_width / 2.0;
            let inset = Rect::ltrb(
                rect.left + half,
                rect.top + half,
                rect.right - half,
                rect.bottom - half,
            );
            let paint = Paint::new(self.border_color)
                .with_style(Style::Stroke { width: self.border_width });
            if self.corner_radius > 0.0 {
                context.canvas.draw_rounded_rect(
                    inset,
                    (self.corner_radius - half).max(0.0),
                    &paint,
                );
            } else {
                context.canvas.draw_rect(inset, &paint);
            }
        }
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        if let Some(child) = &self.child {
            child.hit_test(position, result);
        }
        result.add(self.hit_test_id(), position);
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.as_ref().map_or(0.0, |c| c.min_intrinsic_width(height))
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.as_ref().map_or(0.0, |c| c.max_intrinsic_width(height))
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.as_ref().map_or(0.0, |c| c.min_intrinsic_height(width))
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.as_ref().map_or(0.0, |c| c.max_intrinsic_height(width))
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.child.as_ref().and_then(|c| c.distance_to_baseline())
    }
}

// -- Leaf: text ---------------------------------------------------------------

/// A run of text, shaped by the engine's `txt` / skparagraph stack.
pub struct RenderParagraph {
    content: String,
    style: TextStyle,
    max_lines: Option<usize>,
    paragraph: Option<Paragraph>,
    size: Size,
}

impl RenderParagraph {
    pub fn new(content: impl Into<String>) -> RenderParagraph {
        RenderParagraph {
            content: content.into(),
            style: TextStyle::default(),
            max_lines: None,
            paragraph: None,
            size: Size::ZERO,
        }
    }

    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_max_lines(mut self, lines: usize) -> Self {
        self.max_lines = Some(lines);
        self
    }

    pub fn style_mut(&mut self) -> &mut TextStyle {
        &mut self.style
    }

    /// Shapes at `width` without keeping the result, for intrinsics.
    fn measure(&self, width: f32) -> Size {
        let paragraph = Paragraph::new(&self.content, &self.style, width);
        Size::new(paragraph.width(), paragraph.height())
    }
}

impl RenderBox for RenderParagraph {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // An unbounded width means "as wide as you like", which for text means
        // one line; Paragraph cannot shape against infinity.
        let width = if constraints.has_bounded_width() {
            constraints.max_width
        } else {
            f32::MAX / 4.0
        };
        let paragraph = Paragraph::new(&self.content, &self.style, width);
        // Paragraph::new re-lays out at the ink width, so width() is the tight
        // box around the glyphs. That is what makes centring a text inside a
        // larger box actually look centred.
        self.size = constraints.constrain(Size::new(paragraph.width(), paragraph.height()));
        self.paragraph = Some(paragraph);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        if let Some(paragraph) = &self.paragraph {
            context.canvas.draw_paragraph(paragraph, offset.dx, offset.dy);
        }
    }

    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        Paragraph::new(&self.content, &self.style, f32::MAX / 4.0).min_intrinsic_width()
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        Paragraph::new(&self.content, &self.style, f32::MAX / 4.0).max_intrinsic_width()
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.measure(width).height
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.measure(width).height
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.paragraph.as_ref().map(|p| p.baseline())
    }
}

// -- Leaf: image --------------------------------------------------------------

/// How an image is scaled into the box it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BoxFit {
    /// As large as possible while still fitting entirely inside.
    #[default]
    Contain,
    /// As small as possible while still covering the box. Overflows.
    Cover,
    /// Stretch to exactly the box, ignoring the aspect ratio.
    Fill,
    /// Natural size, no scaling.
    None,
}

pub struct RenderImage {
    image: Image,
    fit: BoxFit,
    size: Size,
}

impl RenderImage {
    pub fn new(image: Image) -> RenderImage {
        RenderImage { image, fit: BoxFit::default(), size: Size::ZERO }
    }

    pub fn with_fit(mut self, fit: BoxFit) -> Self {
        self.fit = fit;
        self
    }

    fn natural(&self) -> Size {
        let (w, h) = self.image.size();
        Size::new(w as f32, h as f32)
    }

    /// The rect the image is drawn into, inside a box of `self.size`.
    fn destination(&self, offset: Offset) -> Rect {
        let natural = self.natural();
        if natural.width <= 0.0 || natural.height <= 0.0 {
            return Rect::xywh(offset.dx, offset.dy, 0.0, 0.0);
        }
        let scale = match self.fit {
            BoxFit::Fill => {
                return Rect::xywh(offset.dx, offset.dy, self.size.width, self.size.height);
            }
            BoxFit::None => 1.0,
            BoxFit::Contain => {
                (self.size.width / natural.width).min(self.size.height / natural.height)
            }
            BoxFit::Cover => {
                (self.size.width / natural.width).max(self.size.height / natural.height)
            }
        };
        let width = natural.width * scale;
        let height = natural.height * scale;
        Rect::xywh(
            offset.dx + (self.size.width - width) / 2.0,
            offset.dy + (self.size.height - height) / 2.0,
            width,
            height,
        )
    }
}

impl RenderBox for RenderImage {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = constraints.constrain(self.natural());
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let natural = self.natural();
        if natural.width <= 0.0 || natural.height <= 0.0 {
            return;
        }
        let source = Rect::xywh(0.0, 0.0, natural.width, natural.height);
        context
            .canvas
            .draw_image_rect(&self.image, source, self.destination(offset), None);
    }

    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        self.natural().width
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        self.natural().width
    }

    fn min_intrinsic_height(&self, _width: f32) -> f32 {
        self.natural().height
    }

    fn max_intrinsic_height(&self, _width: f32) -> f32 {
        self.natural().height
    }
}

// -- Single child: constrained box --------------------------------------------

/// Forces extra constraints on its child, or takes a fixed size with no child.
///
/// Upstream this is `RenderConstrainedBox`, behind `SizedBox` and the
/// `width`/`height` arguments of `Container`.
pub struct RenderConstrainedBox {
    extra: BoxConstraints,
    child: Option<BoxedRender>,
    size: Size,
}

impl RenderConstrainedBox {
    pub fn new(extra: BoxConstraints) -> RenderConstrainedBox {
        RenderConstrainedBox { extra, child: None, size: Size::ZERO }
    }

    pub fn tight(width: f32, height: f32) -> RenderConstrainedBox {
        Self::new(BoxConstraints::tight(width, height))
    }

    pub fn with_child(mut self, child: impl RenderBox + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }
}

impl RenderBox for RenderConstrainedBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let inner = self.extra.enforce(constraints);
        self.size = match &mut self.child {
            Some(child) => child.layout(inner),
            None => inner.constrain(inner.smallest()),
        };
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        if let Some(child) = &self.child {
            context.paint_child(child.as_ref(), offset);
        }
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        if let Some(child) = &self.child {
            child.hit_test(position, result);
        }
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        if self.extra.has_tight_width() {
            return self.extra.min_width;
        }
        self.child.as_ref().map_or(0.0, |c| c.min_intrinsic_width(height))
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        if self.extra.has_tight_width() {
            return self.extra.min_width;
        }
        self.child.as_ref().map_or(0.0, |c| c.max_intrinsic_width(height))
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        if self.extra.has_tight_height() {
            return self.extra.min_height;
        }
        self.child.as_ref().map_or(0.0, |c| c.min_intrinsic_height(width))
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        if self.extra.has_tight_height() {
            return self.extra.min_height;
        }
        self.child.as_ref().map_or(0.0, |c| c.max_intrinsic_height(width))
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.child.as_ref().and_then(|c| c.distance_to_baseline())
    }
}

// -- Single child: padding ----------------------------------------------------

pub struct RenderPadding {
    insets: EdgeInsets,
    child: BoxedRender,
    size: Size,
}

impl RenderPadding {
    pub fn new(insets: EdgeInsets, child: impl RenderBox + 'static) -> RenderPadding {
        RenderPadding { insets, child: Box::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderPadding {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let inner = constraints.deflate(self.insets);
        let child_size = self.child.layout(inner);
        self.size = constraints.constrain(Size::new(
            child_size.width + self.insets.horizontal(),
            child_size.height + self.insets.vertical(),
        ));
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(
            self.child.as_ref(),
            offset.translate(self.insets.left, self.insets.top),
        );
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        self.child
            .hit_test(position.translate(-self.insets.left, -self.insets.top), result);
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width((height - self.insets.vertical()).max(0.0))
            + self.insets.horizontal()
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width((height - self.insets.vertical()).max(0.0))
            + self.insets.horizontal()
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height((width - self.insets.horizontal()).max(0.0))
            + self.insets.vertical()
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height((width - self.insets.horizontal()).max(0.0))
            + self.insets.vertical()
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.child.distance_to_baseline().map(|b| b + self.insets.top)
    }
}

// -- Single child: alignment --------------------------------------------------

/// Expands to fill its constraints and places its child at `alignment`.
///
/// Upstream this is `RenderPositionedBox`, behind both `Center` and `Align`.
/// The width/height factors let it shrink-wrap instead: a factor of 1.0 on an
/// axis makes it exactly its child's size on that axis.
pub struct RenderAlign {
    alignment: Alignment,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
    child: BoxedRender,
    child_offset: Offset,
    size: Size,
}

impl RenderAlign {
    pub fn new(alignment: Alignment, child: impl RenderBox + 'static) -> RenderAlign {
        RenderAlign {
            alignment,
            width_factor: None,
            height_factor: None,
            child: Box::new(child),
            child_offset: Offset::ZERO,
            size: Size::ZERO,
        }
    }

    pub fn with_factors(mut self, width: Option<f32>, height: Option<f32>) -> Self {
        self.width_factor = width;
        self.height_factor = height;
        self
    }

    pub fn child_offset(&self) -> Offset {
        self.child_offset
    }
}

impl RenderBox for RenderAlign {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let child_size = self.child.layout(constraints.loosen());

        // Shrink-wrap where a factor was given or the axis is unbounded;
        // otherwise take everything, which is what makes Center fill.
        let width = match self.width_factor {
            Some(factor) => child_size.width * factor,
            None if constraints.has_bounded_width() => constraints.max_width,
            None => child_size.width,
        };
        let height = match self.height_factor {
            Some(factor) => child_size.height * factor,
            None if constraints.has_bounded_height() => constraints.max_height,
            None => child_size.height,
        };
        self.size = constraints.constrain(Size::new(width, height));
        self.child_offset = self.alignment.inscribe(child_size, self.size);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(
            self.child.as_ref(),
            offset.plus(self.child_offset),
        );
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        self.child.hit_test(position.minus(self.child_offset), result);
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.child.distance_to_baseline().map(|b| b + self.child_offset.dy)
    }
}

// -- Multi child: flex --------------------------------------------------------

/// A child of a [`RenderFlex`], with how it should be sized.
pub struct FlexChild {
    pub render: BoxedRender,
    /// 0 means the child sizes itself; anything higher means it takes a share
    /// of the free space proportional to this number.
    pub flex: u32,
    /// With `flex > 0`: whether the child must exactly fill its share (tight)
    /// or may take less (loose). Upstream this is `FlexFit`.
    pub tight: bool,
}

impl FlexChild {
    pub fn new(render: impl RenderBox + 'static) -> FlexChild {
        FlexChild { render: Box::new(render), flex: 0, tight: true }
    }

    pub fn expanded(render: impl RenderBox + 'static, flex: u32) -> FlexChild {
        FlexChild { render: Box::new(render), flex: flex.max(1), tight: true }
    }

    pub fn flexible(render: impl RenderBox + 'static, flex: u32) -> FlexChild {
        FlexChild { render: Box::new(render), flex: flex.max(1), tight: false }
    }
}

/// Lays children out in a line, giving flexible children a share of what is
/// left. Row and Column are both this, differing only in [`Axis`].
///
/// The algorithm is upstream's, and the order matters:
///
/// 1. Lay out every inflexible child with the main axis unbounded, so it
///    reports what it actually wants.
/// 2. Divide whatever main-axis space remains among the flexible children by
///    flex factor.
/// 3. Size the flex itself, then position children by the two alignments.
pub struct RenderFlex {
    direction: Axis,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    main_axis_size: MainAxisSize,
    spacing: f32,
    children: Vec<FlexChild>,
    offsets: Vec<Offset>,
    size: Size,
}

impl RenderFlex {
    pub fn new(direction: Axis) -> RenderFlex {
        RenderFlex {
            direction,
            main_axis_alignment: MainAxisAlignment::default(),
            cross_axis_alignment: CrossAxisAlignment::default(),
            main_axis_size: MainAxisSize::default(),
            spacing: 0.0,
            children: Vec::new(),
            offsets: Vec::new(),
            size: Size::ZERO,
        }
    }

    pub fn row() -> RenderFlex {
        Self::new(Axis::Horizontal)
    }

    pub fn column() -> RenderFlex {
        Self::new(Axis::Vertical)
    }

    pub fn with_main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.main_axis_alignment = alignment;
        self
    }

    pub fn with_cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    pub fn with_main_axis_size(mut self, size: MainAxisSize) -> Self {
        self.main_axis_size = size;
        self
    }

    /// Fixed gap inserted between children, before the alignment distributes
    /// what is left.
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn push(mut self, child: impl RenderBox + 'static) -> Self {
        self.children.push(FlexChild::new(child));
        self
    }

    pub fn push_flex(mut self, child: FlexChild) -> Self {
        self.children.push(child);
        self
    }

    pub fn child_offsets(&self) -> &[Offset] {
        &self.offsets
    }

    fn is_horizontal(&self) -> bool {
        self.direction == Axis::Horizontal
    }

    fn main_of(&self, size: Size) -> f32 {
        if self.is_horizontal() { size.width } else { size.height }
    }

    fn cross_of(&self, size: Size) -> f32 {
        if self.is_horizontal() { size.height } else { size.width }
    }

    fn compose(&self, main: f32, cross: f32) -> Size {
        if self.is_horizontal() {
            Size::new(main, cross)
        } else {
            Size::new(cross, main)
        }
    }

    fn offset_of(&self, main: f32, cross: f32) -> Offset {
        if self.is_horizontal() {
            Offset::new(main, cross)
        } else {
            Offset::new(cross, main)
        }
    }

    /// Constraints for one child, given the cross-axis limits and, for a
    /// flexible child, the main-axis extent it has been allotted.
    fn child_constraints(&self, constraints: BoxConstraints, main: Option<(f32, bool)>) -> BoxConstraints {
        let (cross_min, cross_max) = if self.is_horizontal() {
            (constraints.min_height, constraints.max_height)
        } else {
            (constraints.min_width, constraints.max_width)
        };
        // Only Stretch forces a cross-axis minimum. Passing the parent's own
        // minimum down would make every child as tall as the row, which is
        // wrong for a 44px avatar in a 64px row -- and is exactly what upstream
        // avoids by starting the inner constraints at zero.
        //
        // Stretch also needs a cross axis to stretch to. A row inside a scroll
        // viewport has an unbounded height, and "stretch to unbounded" is an
        // infinitely tall child and then an infinitely tall row: the layout
        // does not fail, it silently produces nothing anyone can see. Upstream
        // asserts here; degrading to "do not force" keeps the frame.
        let _ = cross_min;
        let stretches =
            self.cross_axis_alignment == CrossAxisAlignment::Stretch && cross_max.is_finite();
        let cross_min = if stretches { cross_max } else { 0.0 };

        let (main_min, main_max) = match main {
            Some((extent, true)) => (extent, extent),
            Some((extent, false)) => (0.0, extent),
            // Unbounded, so the child reports what it actually wants.
            None => (0.0, f32::INFINITY),
        };

        if self.is_horizontal() {
            BoxConstraints::new(main_min, main_max, cross_min, cross_max)
        } else {
            BoxConstraints::new(cross_min, cross_max, main_min, main_max)
        }
    }
}

impl RenderBox for RenderFlex {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let count = self.children.len();
        let total_spacing = if count > 1 { self.spacing * (count - 1) as f32 } else { 0.0 };

        let mut sizes: Vec<Size> = vec![Size::ZERO; count];
        let mut allocated = 0.0f32;
        let mut cross = 0.0f32;
        let mut total_flex = 0u32;

        // Pass one: the children that size themselves.
        for index in 0..count {
            if self.children[index].flex > 0 {
                total_flex += self.children[index].flex;
                continue;
            }
            let child_constraints = self.child_constraints(constraints, None);
            let size = self.children[index].render.layout(child_constraints);
            sizes[index] = size;
            allocated += self.main_of(size);
            cross = cross.max(self.cross_of(size));
        }

        // Pass two: divide the remainder among the flexible ones. With an
        // unbounded main axis there is no remainder to divide, so a flexible
        // child gets to size itself -- upstream errors here instead, but
        // degrading is friendlier than aborting a frame.
        let main_limit = if self.is_horizontal() {
            constraints.max_width
        } else {
            constraints.max_height
        };
        let free = if main_limit.is_finite() {
            (main_limit - allocated - total_spacing).max(0.0)
        } else {
            f32::INFINITY
        };

        if total_flex > 0 {
            let per_flex = if free.is_finite() { free / total_flex as f32 } else { f32::INFINITY };
            for index in 0..count {
                let flex = self.children[index].flex;
                if flex == 0 {
                    continue;
                }
                let tight = self.children[index].tight;
                let extent = if per_flex.is_finite() {
                    Some((per_flex * flex as f32, tight))
                } else {
                    None
                };
                let child_constraints = self.child_constraints(constraints, extent);
                let size = self.children[index].render.layout(child_constraints);
                sizes[index] = size;
                allocated += self.main_of(size);
                cross = cross.max(self.cross_of(size));
            }
        }

        // Size ourselves.
        let content_main = allocated + total_spacing;
        let main_extent = match self.main_axis_size {
            MainAxisSize::Min => content_main,
            MainAxisSize::Max if main_limit.is_finite() => main_limit,
            MainAxisSize::Max => content_main,
        };
        self.size = constraints.constrain(self.compose(main_extent, cross));
        let actual_main = self.main_of(self.size);
        let actual_cross = self.cross_of(self.size);

        // Distribute whatever main-axis slack the alignment asks for.
        let slack = (actual_main - content_main).max(0.0);
        let (leading, between) = match self.main_axis_alignment {
            MainAxisAlignment::Start => (0.0, 0.0),
            MainAxisAlignment::End => (slack, 0.0),
            MainAxisAlignment::Center => (slack / 2.0, 0.0),
            MainAxisAlignment::SpaceBetween if count > 1 => (0.0, slack / (count - 1) as f32),
            MainAxisAlignment::SpaceBetween => (0.0, 0.0),
            MainAxisAlignment::SpaceAround if count > 0 => {
                let gap = slack / count as f32;
                (gap / 2.0, gap)
            }
            MainAxisAlignment::SpaceAround => (0.0, 0.0),
            MainAxisAlignment::SpaceEvenly if count > 0 => {
                let gap = slack / (count + 1) as f32;
                (gap, gap)
            }
            MainAxisAlignment::SpaceEvenly => (0.0, 0.0),
        };

        // Baseline alignment needs the deepest baseline before it can place
        // anything, so it is resolved up front.
        let max_baseline = if self.cross_axis_alignment == CrossAxisAlignment::Baseline {
            self.children
                .iter()
                .filter_map(|c| c.render.distance_to_baseline())
                .fold(0.0f32, f32::max)
        } else {
            0.0
        };

        self.offsets.clear();
        self.offsets.reserve(count);
        let mut main_position = leading;
        for index in 0..count {
            if index > 0 {
                main_position += self.spacing + between;
            }
            let size = sizes[index];
            let child_cross = self.cross_of(size);
            let cross_position = match self.cross_axis_alignment {
                CrossAxisAlignment::Start | CrossAxisAlignment::Stretch => 0.0,
                CrossAxisAlignment::End => (actual_cross - child_cross).max(0.0),
                CrossAxisAlignment::Center => ((actual_cross - child_cross) / 2.0).max(0.0),
                CrossAxisAlignment::Baseline => match self.children[index]
                    .render
                    .distance_to_baseline()
                {
                    Some(baseline) => (max_baseline - baseline).max(0.0),
                    None => 0.0,
                },
            };
            self.offsets.push(self.offset_of(main_position, cross_position));
            main_position += self.main_of(size);
        }

        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        for (child, placement) in self.children.iter().zip(self.offsets.iter()) {
            context.paint_child(child.render.as_ref(), offset.plus(*placement));
        }
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        // Back to front: the last child painted is on top.
        for (child, placement) in self.children.iter().zip(self.offsets.iter()).rev() {
            if child.render.hit_test(position.minus(*placement), result) {
                break;
            }
        }
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        if self.is_horizontal() {
            self.children.iter().map(|c| c.render.min_intrinsic_width(height)).sum::<f32>()
        } else {
            self.children
                .iter()
                .map(|c| c.render.min_intrinsic_width(height))
                .fold(0.0, f32::max)
        }
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        if self.is_horizontal() {
            self.children.iter().map(|c| c.render.max_intrinsic_width(height)).sum::<f32>()
        } else {
            self.children
                .iter()
                .map(|c| c.render.max_intrinsic_width(height))
                .fold(0.0, f32::max)
        }
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        if self.is_horizontal() {
            self.children
                .iter()
                .map(|c| c.render.min_intrinsic_height(width))
                .fold(0.0, f32::max)
        } else {
            self.children.iter().map(|c| c.render.min_intrinsic_height(width)).sum::<f32>()
        }
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        if self.is_horizontal() {
            self.children
                .iter()
                .map(|c| c.render.max_intrinsic_height(width))
                .fold(0.0, f32::max)
        } else {
            self.children.iter().map(|c| c.render.max_intrinsic_height(width)).sum::<f32>()
        }
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        // The first child that has one, offset by where it was placed.
        self.children
            .iter()
            .zip(self.offsets.iter())
            .find_map(|(child, offset)| {
                child.render.distance_to_baseline().map(|b| b + offset.dy)
            })
    }
}

// -- Multi child: stack -------------------------------------------------------

/// How a stacked child is positioned. `None` on a side means "not anchored
/// there"; anchoring both sides of an axis stretches the child across it.
#[derive(Clone, Copy, Debug, Default)]
pub struct StackPosition {
    pub left: Option<f32>,
    pub top: Option<f32>,
    pub right: Option<f32>,
    pub bottom: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

impl StackPosition {
    pub fn is_positioned(&self) -> bool {
        self.left.is_some()
            || self.top.is_some()
            || self.right.is_some()
            || self.bottom.is_some()
            || self.width.is_some()
            || self.height.is_some()
    }
}

pub struct StackChild {
    pub render: BoxedRender,
    pub position: StackPosition,
}

/// Overlays children, sizing itself to the largest unpositioned one.
pub struct RenderStack {
    alignment: Alignment,
    children: Vec<StackChild>,
    offsets: Vec<Offset>,
    size: Size,
}

impl RenderStack {
    pub fn new() -> RenderStack {
        RenderStack {
            alignment: Alignment::TOP_LEFT,
            children: Vec::new(),
            offsets: Vec::new(),
            size: Size::ZERO,
        }
    }

    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn push(mut self, child: impl RenderBox + 'static) -> Self {
        self.children.push(StackChild {
            render: Box::new(child),
            position: StackPosition::default(),
        });
        self
    }

    pub fn push_positioned(
        mut self,
        child: impl RenderBox + 'static,
        position: StackPosition,
    ) -> Self {
        self.children.push(StackChild { render: Box::new(child), position });
        self
    }

    pub fn child_offsets(&self) -> &[Offset] {
        &self.offsets
    }
}

impl Default for RenderStack {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderBox for RenderStack {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let mut widest = constraints.min_width;
        let mut tallest = constraints.min_height;
        let mut sizes: Vec<Size> = vec![Size::ZERO; self.children.len()];

        // Unpositioned children decide the stack's size.
        let mut has_unpositioned = false;
        for (index, child) in self.children.iter_mut().enumerate() {
            if child.position.is_positioned() {
                continue;
            }
            has_unpositioned = true;
            let size = child.render.layout(constraints.loosen());
            sizes[index] = size;
            widest = widest.max(size.width);
            tallest = tallest.max(size.height);
        }

        self.size = if has_unpositioned {
            constraints.constrain(Size::new(widest, tallest))
        } else {
            constraints.biggest()
        };

        // Positioned children are laid out against the resolved size.
        for (index, child) in self.children.iter_mut().enumerate() {
            if !child.position.is_positioned() {
                continue;
            }
            let p = child.position;
            let width = match (p.left, p.right, p.width) {
                (Some(left), Some(right), _) => Some((self.size.width - left - right).max(0.0)),
                (_, _, Some(width)) => Some(width),
                _ => None,
            };
            let height = match (p.top, p.bottom, p.height) {
                (Some(top), Some(bottom), _) => Some((self.size.height - top - bottom).max(0.0)),
                (_, _, Some(height)) => Some(height),
                _ => None,
            };
            let child_constraints = BoxConstraints::new(
                width.unwrap_or(0.0),
                width.unwrap_or(self.size.width),
                height.unwrap_or(0.0),
                height.unwrap_or(self.size.height),
            );
            sizes[index] = child.render.layout(child_constraints);
        }

        // Position everything.
        self.offsets.clear();
        self.offsets.reserve(self.children.len());
        for (index, child) in self.children.iter().enumerate() {
            let size = sizes[index];
            let offset = if child.position.is_positioned() {
                let p = child.position;
                let x = match (p.left, p.right) {
                    (Some(left), _) => left,
                    (None, Some(right)) => self.size.width - right - size.width,
                    (None, None) => self.alignment.inscribe(size, self.size).dx,
                };
                let y = match (p.top, p.bottom) {
                    (Some(top), _) => top,
                    (None, Some(bottom)) => self.size.height - bottom - size.height,
                    (None, None) => self.alignment.inscribe(size, self.size).dy,
                };
                Offset::new(x, y)
            } else {
                self.alignment.inscribe(size, self.size)
            };
            self.offsets.push(offset);
        }

        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        for (child, placement) in self.children.iter().zip(self.offsets.iter()) {
            context.paint_child(child.render.as_ref(), offset.plus(*placement));
        }
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        for (child, placement) in self.children.iter().zip(self.offsets.iter()).rev() {
            if child.render.hit_test(position.minus(*placement), result) {
                break;
            }
        }
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.children
            .iter()
            .filter(|c| !c.position.is_positioned())
            .map(|c| c.render.min_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.children
            .iter()
            .filter(|c| !c.position.is_positioned())
            .map(|c| c.render.max_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.children
            .iter()
            .filter(|c| !c.position.is_positioned())
            .map(|c| c.render.min_intrinsic_height(width))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.children
            .iter()
            .filter(|c| !c.position.is_positioned())
            .map(|c| c.render.max_intrinsic_height(width))
            .fold(0.0, f32::max)
    }
}

// -- Single child: effects ----------------------------------------------------

/// Applies a 2D affine to its child. Layout is unaffected -- the child is laid
/// out and sized as if untransformed, which is why a rotated box still
/// occupies its original slot.
pub struct RenderTransform {
    matrix: [f32; 6],
    origin: Alignment,
    child: BoxedRender,
    size: Size,
}

impl RenderTransform {
    /// `[a, b, c, d, e, f]`, applied as `x' = a*x + c*y + e`.
    pub fn new(matrix: [f32; 6], child: impl RenderBox + 'static) -> RenderTransform {
        RenderTransform {
            matrix,
            origin: Alignment::CENTER,
            child: Box::new(child),
            size: Size::ZERO,
        }
    }

    pub fn rotate(degrees: f32, child: impl RenderBox + 'static) -> RenderTransform {
        let radians = degrees.to_radians();
        let (sin, cos) = radians.sin_cos();
        Self::new([cos, sin, -sin, cos, 0.0, 0.0], child)
    }

    pub fn scale(sx: f32, sy: f32, child: impl RenderBox + 'static) -> RenderTransform {
        Self::new([sx, 0.0, 0.0, sy, 0.0, 0.0], child)
    }

    pub fn with_origin(mut self, origin: Alignment) -> Self {
        self.origin = origin;
        self
    }
}

impl RenderBox for RenderTransform {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let pivot = self.origin.inscribe(Size::ZERO, self.size);
        let [a, b, c, d, e, f] = self.matrix;
        let child = self.child.as_ref();
        context.canvas.saved(|canvas| {
            // Move the origin to the pivot, transform, move back, all in the
            // parent's coordinates.
            canvas.translate(offset.dx + pivot.dx, offset.dy + pivot.dy);
            canvas.transform(a, b, c, d, e, f);
            canvas.translate(-pivot.dx, -pivot.dy);
            let mut inner = PaintContext::new(canvas);
            child.paint(&mut inner, Offset::ZERO);
        });
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Inverting the affine to hit-test through a transform is M3 work; a
        // transformed subtree currently tests against its untransformed
        // geometry, which is right for the common identity and translate cases.
        if !self.size.contains(position) {
            return false;
        }
        self.child.hit_test(position, result);
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }
}

/// Draws its child at a uniform opacity, through an offscreen group so that
/// overlapping parts of the child do not show through each other.
pub struct RenderOpacity {
    opacity: f32,
    child: BoxedRender,
    size: Size,
}

impl RenderOpacity {
    pub fn new(opacity: f32, child: impl RenderBox + 'static) -> RenderOpacity {
        RenderOpacity { opacity: opacity.clamp(0.0, 1.0), child: Box::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderOpacity {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        if self.opacity <= 0.0 {
            return;
        }
        if self.opacity >= 1.0 {
            context.paint_child(self.child.as_ref(), offset);
            return;
        }
        let bounds = Rect::xywh(offset.dx, offset.dy, self.size.width, self.size.height);
        let paint = Paint::new(Color::WHITE).with_opacity(self.opacity);
        let child = self.child.as_ref();
        context.canvas.saved(|canvas| {
            canvas.save_layer(Some(bounds), Some(&paint));
            let mut inner = PaintContext::new(canvas);
            child.paint(&mut inner, offset);
        });
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // A fully transparent subtree is not a target, matching upstream.
        if self.opacity <= 0.0 || !self.size.contains(position) {
            return false;
        }
        self.child.hit_test(position, result);
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }
}

/// Clips its child to its own bounds, optionally with rounded corners.
pub struct RenderClipRect {
    corner_radius: f32,
    child: BoxedRender,
    size: Size,
}

impl RenderClipRect {
    pub fn new(child: impl RenderBox + 'static) -> RenderClipRect {
        RenderClipRect { corner_radius: 0.0, child: Box::new(child), size: Size::ZERO }
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }
}

impl RenderBox for RenderClipRect {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let bounds = Rect::xywh(offset.dx, offset.dy, self.size.width, self.size.height);
        let radius = self.corner_radius;
        let child = self.child.as_ref();
        context.canvas.saved(|canvas| {
            if radius > 0.0 {
                canvas.clip_rounded_rect(bounds, radius, radius, ClipOp::Intersect, true);
            } else {
                canvas.clip_rect(bounds, ClipOp::Intersect, true);
            }
            let mut inner = PaintContext::new(canvas);
            child.paint(&mut inner, offset);
        });
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Outside the clip nothing is visible, so nothing is hittable.
        if !self.size.contains(position) {
            return false;
        }
        self.child.hit_test(position, result);
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }
}

/// Clips its child to an arbitrary path, in the child's own coordinates.
pub struct RenderClipPath {
    path: RenderPath,
    child: BoxedRender,
    size: Size,
}

impl RenderClipPath {
    pub fn new(path: RenderPath, child: impl RenderBox + 'static) -> RenderClipPath {
        RenderClipPath { path, child: Box::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderClipPath {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let path = &self.path;
        let child = self.child.as_ref();
        context.canvas.saved(|canvas| {
            canvas.translate(offset.dx, offset.dy);
            canvas.clip_path(path, ClipOp::Intersect, true);
            let mut inner = PaintContext::new(canvas);
            child.paint(&mut inner, Offset::ZERO);
        });
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        self.child.hit_test(position, result);
        true
    }
}

// -- Scrolling ----------------------------------------------------------------

/// A window onto a child that is larger than it.
///
/// The child is laid out with the scroll axis unbounded, so it sizes itself to
/// its content; the viewport then shows the slice at `offset` and clips the
/// rest. That is the whole of scrolling -- the physics and the gesture that
/// drive `offset` belong to M3.
pub struct RenderViewport {
    axis: Axis,
    offset: f32,
    child: BoxedRender,
    child_size: Size,
    size: Size,
}

impl RenderViewport {
    pub fn new(axis: Axis, child: impl RenderBox + 'static) -> RenderViewport {
        RenderViewport {
            axis,
            offset: 0.0,
            child: Box::new(child),
            child_size: Size::ZERO,
            size: Size::ZERO,
        }
    }

    /// How far the content is scrolled, in logical pixels. Clamped to the
    /// scrollable extent on the next layout.
    pub fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    pub fn set_offset(&mut self, offset: f32) {
        self.offset = offset.clamp(0.0, self.max_scroll_extent());
    }

    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// How far the content can scroll before it runs out. Zero when the
    /// content fits.
    pub fn max_scroll_extent(&self) -> f32 {
        let content = match self.axis {
            Axis::Vertical => self.child_size.height,
            Axis::Horizontal => self.child_size.width,
        };
        let window = match self.axis {
            Axis::Vertical => self.size.height,
            Axis::Horizontal => self.size.width,
        };
        (content - window).max(0.0)
    }

    fn scroll_offset(&self) -> Offset {
        match self.axis {
            Axis::Vertical => Offset::new(0.0, -self.offset),
            Axis::Horizontal => Offset::new(-self.offset, 0.0),
        }
    }
}

impl RenderBox for RenderViewport {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let child_constraints = match self.axis {
            Axis::Vertical => BoxConstraints::new(
                constraints.min_width,
                constraints.max_width,
                0.0,
                f32::INFINITY,
            ),
            Axis::Horizontal => BoxConstraints::new(
                0.0,
                f32::INFINITY,
                constraints.min_height,
                constraints.max_height,
            ),
        };
        self.child_size = self.child.layout(child_constraints);
        self.size = constraints.biggest();
        // Content may have shrunk since the offset was set, so re-clamp.
        self.offset = self.offset.clamp(0.0, self.max_scroll_extent());
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let bounds = Rect::xywh(offset.dx, offset.dy, self.size.width, self.size.height);
        let scrolled = offset.plus(self.scroll_offset());
        let child = self.child.as_ref();
        context.canvas.saved(|canvas| {
            canvas.clip_rect(bounds, ClipOp::Intersect, false);
            let mut inner = PaintContext::new(canvas);
            child.paint(&mut inner, scrolled);
        });
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        self.child.hit_test(position.minus(self.scroll_offset()), result);
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        match self.axis {
            Axis::Vertical => self.child.min_intrinsic_width(height),
            Axis::Horizontal => 0.0,
        }
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        match self.axis {
            Axis::Vertical => self.child.max_intrinsic_width(height),
            Axis::Horizontal => 0.0,
        }
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => self.child.min_intrinsic_height(width),
            Axis::Vertical => 0.0,
        }
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        match self.axis {
            Axis::Horizontal => self.child.max_intrinsic_height(width),
            Axis::Vertical => 0.0,
        }
    }
}

// -- Identity -----------------------------------------------------------------

/// Wraps a child with a hit-test identity, so a pointer that lands on it can be
/// traced back to whatever the caller cares about.
pub struct RenderPointerRegion {
    id: u64,
    handlers: Rc<PointerHandlers>,
    child: BoxedRender,
    size: Size,
}

impl RenderPointerRegion {
    pub fn new(id: u64, child: impl RenderBox + 'static) -> RenderPointerRegion {
        RenderPointerRegion {
            id,
            handlers: Rc::new(PointerHandlers::default()),
            child: Box::new(child),
            size: Size::ZERO,
        }
    }

    /// What this region wants to hear about. A region with no handlers still
    /// takes part in hit testing, so it can shield whatever is behind it.
    pub fn with_handlers(mut self, handlers: PointerHandlers) -> Self {
        self.handlers = Rc::new(handlers);
        self
    }
}

impl RenderBox for RenderPointerRegion {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(self.child.as_ref(), offset);
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        self.child.hit_test(position, result);
        result.add_with_handlers(self.id, position, Some(Rc::clone(&self.handlers)));
        true
    }

    fn hit_test_id(&self) -> u64 {
        self.id
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.child.distance_to_baseline()
    }
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A box that reports a fixed size, clamped into whatever it is given.
    struct FixedBox {
        preferred: Size,
        size: Size,
        baseline: Option<f32>,
    }

    impl FixedBox {
        fn new(width: f32, height: f32) -> FixedBox {
            FixedBox {
                preferred: Size::new(width, height),
                size: Size::ZERO,
                baseline: None,
            }
        }

        fn with_baseline(mut self, baseline: f32) -> FixedBox {
            self.baseline = Some(baseline);
            self
        }
    }

    impl RenderBox for FixedBox {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            self.size = constraints.constrain(self.preferred);
            self.size
        }
        fn size(&self) -> Size {
            self.size
        }
        fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
        fn min_intrinsic_width(&self, _height: f32) -> f32 {
            self.preferred.width
        }
        fn max_intrinsic_width(&self, _height: f32) -> f32 {
            self.preferred.width
        }
        fn min_intrinsic_height(&self, _width: f32) -> f32 {
            self.preferred.height
        }
        fn max_intrinsic_height(&self, _width: f32) -> f32 {
            self.preferred.height
        }
        fn distance_to_baseline(&self) -> Option<f32> {
            self.baseline
        }
    }

    #[test]
    fn constraints_clamp_desired_size() {
        let c = BoxConstraints::loose(100.0, 50.0);
        assert_eq!(c.constrain(Size::new(200.0, 10.0)), Size::new(100.0, 10.0));
    }

    #[test]
    fn biggest_collapses_infinity_to_the_minimum() {
        let c = BoxConstraints::new(10.0, f32::INFINITY, 5.0, 20.0);
        assert_eq!(c.biggest(), Size::new(10.0, 20.0));
    }

    #[test]
    fn align_fills_and_positions_its_child() {
        let mut align = RenderAlign::new(Alignment::CENTER, FixedBox::new(40.0, 20.0));
        let size = align.layout(BoxConstraints::tight(100.0, 100.0));
        assert_eq!(size, Size::new(100.0, 100.0));
        assert_eq!(align.child_offset(), Offset::new(30.0, 40.0));
    }

    #[test]
    fn align_shrink_wraps_with_a_factor() {
        let mut align = RenderAlign::new(Alignment::CENTER, FixedBox::new(40.0, 20.0))
            .with_factors(Some(1.0), Some(1.0));
        let size = align.layout(BoxConstraints::loose(100.0, 100.0));
        assert_eq!(size, Size::new(40.0, 20.0));
    }

    #[test]
    fn padding_grows_the_box_and_offsets_the_child() {
        let mut padding = RenderPadding::new(EdgeInsets::all(8.0), FixedBox::new(20.0, 10.0));
        let size = padding.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(36.0, 26.0));
        assert_eq!(padding.min_intrinsic_width(100.0), 36.0);
    }

    #[test]
    fn column_stacks_children_with_spacing() {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(10.0)
            .push(FixedBox::new(30.0, 20.0))
            .push(FixedBox::new(50.0, 20.0));
        let size = column.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(50.0, 50.0));
        assert_eq!(column.child_offsets()[0], Offset::new(10.0, 0.0));
        assert_eq!(column.child_offsets()[1], Offset::new(0.0, 30.0));
    }

    #[test]
    fn flex_divides_free_space_by_flex_factor() {
        let mut row = RenderFlex::row()
            .push(FixedBox::new(20.0, 10.0))
            .push_flex(FlexChild::expanded(FixedBox::new(0.0, 10.0), 1))
            .push_flex(FlexChild::expanded(FixedBox::new(0.0, 10.0), 3));
        let size = row.layout(BoxConstraints::tight(100.0, 50.0));
        assert_eq!(size, Size::new(100.0, 50.0));
        // 80 free, split 1:3 -> 20 and 60, laid out after the fixed 20.
        let offsets = row.child_offsets();
        assert_eq!(offsets[0].dx, 0.0);
        assert_eq!(offsets[1].dx, 20.0);
        assert_eq!(offsets[2].dx, 40.0);
    }

    #[test]
    fn space_between_pushes_children_to_the_ends() {
        let mut row = RenderFlex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .push(FixedBox::new(20.0, 10.0))
            .push(FixedBox::new(20.0, 10.0));
        row.layout(BoxConstraints::tight(100.0, 50.0));
        let offsets = row.child_offsets();
        assert_eq!(offsets[0].dx, 0.0);
        assert_eq!(offsets[1].dx, 80.0);
    }

    #[test]
    fn baseline_alignment_lines_up_the_deepest_baseline() {
        let mut row = RenderFlex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Baseline)
            .push(FixedBox::new(20.0, 40.0).with_baseline(30.0))
            .push(FixedBox::new(20.0, 20.0).with_baseline(10.0));
        row.layout(BoxConstraints::tight(100.0, 50.0));
        let offsets = row.child_offsets();
        // The deeper baseline sits at the top; the shallower one drops by the
        // difference so both baselines land on the same line.
        assert_eq!(offsets[0].dy, 0.0);
        assert_eq!(offsets[1].dy, 20.0);
    }

    #[test]
    fn stretch_on_an_unbounded_cross_axis_does_not_become_infinite() {
        // A row inside a scroll viewport: the height is unbounded, so there is
        // nothing to stretch to. The row must take its children's height, not
        // infinity.
        let mut row = RenderFlex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .push(FixedBox::new(20.0, 30.0))
            .push(FixedBox::new(20.0, 50.0));
        let size = row.layout(BoxConstraints::new(0.0, 200.0, 0.0, f32::INFINITY));
        assert!(size.height.is_finite(), "{size:?}");
        assert_eq!(size.height, 50.0);
    }

    #[test]
    fn stretch_on_a_bounded_cross_axis_still_stretches() {
        let mut row = RenderFlex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .push(FixedBox::new(20.0, 30.0));
        row.layout(BoxConstraints::new(0.0, 200.0, 0.0, 80.0));
        // The child was forced to the full height rather than its own 30.
        assert_eq!(row.size().height, 80.0);
    }

    #[test]
    fn stack_sizes_to_its_largest_unpositioned_child() {
        let mut stack = RenderStack::new()
            .push(FixedBox::new(40.0, 20.0))
            .push(FixedBox::new(20.0, 60.0));
        let size = stack.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(40.0, 60.0));
    }

    #[test]
    fn stack_anchors_a_positioned_child() {
        let mut stack = RenderStack::new()
            .push(FixedBox::new(100.0, 100.0))
            .push_positioned(
                FixedBox::new(10.0, 10.0),
                StackPosition { right: Some(5.0), bottom: Some(5.0), ..Default::default() },
            );
        stack.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(stack.child_offsets()[1], Offset::new(85.0, 85.0));
    }

    #[test]
    fn stretching_a_positioned_child_across_both_edges() {
        let mut stack = RenderStack::new()
            .push(FixedBox::new(100.0, 100.0))
            .push_positioned(
                FixedBox::new(1000.0, 1000.0),
                StackPosition {
                    left: Some(10.0),
                    right: Some(10.0),
                    top: Some(0.0),
                    ..Default::default()
                },
            );
        stack.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(stack.child_offsets()[1], Offset::new(10.0, 0.0));
    }

    #[test]
    fn viewport_lets_its_child_exceed_it_and_reports_the_extent() {
        let mut viewport = RenderViewport::new(Axis::Vertical, FixedBox::new(50.0, 500.0));
        let size = viewport.layout(BoxConstraints::tight(100.0, 200.0));
        assert_eq!(size, Size::new(100.0, 200.0));
        assert_eq!(viewport.max_scroll_extent(), 300.0);
    }

    #[test]
    fn viewport_clamps_an_out_of_range_offset() {
        let mut viewport =
            RenderViewport::new(Axis::Vertical, FixedBox::new(50.0, 500.0)).with_offset(1000.0);
        viewport.layout(BoxConstraints::tight(100.0, 200.0));
        assert_eq!(viewport.offset(), 300.0);
    }

    #[test]
    fn hit_test_records_the_innermost_target_first() {
        let mut stack = RenderStack::new()
            .push(RenderPointerRegion::new(1, FixedBox::new(100.0, 100.0)))
            .push_positioned(
                RenderPointerRegion::new(2, FixedBox::new(20.0, 20.0)),
                StackPosition { left: Some(10.0), top: Some(10.0), ..Default::default() },
            );
        stack.layout(BoxConstraints::loose(200.0, 200.0));

        let mut result = HitTestResult::new();
        assert!(stack.hit_test(Offset::new(15.0, 15.0), &mut result));
        assert_eq!(result.innermost().unwrap().target, 2);
        assert_eq!(result.innermost().unwrap().local_position, Offset::new(5.0, 5.0));

        // Outside the small child, only the big one is hit.
        let mut result = HitTestResult::new();
        assert!(stack.hit_test(Offset::new(60.0, 60.0), &mut result));
        assert_eq!(result.innermost().unwrap().target, 1);
    }

    #[test]
    fn hit_test_misses_outside_the_box() {
        let region = {
            let mut r = RenderPointerRegion::new(7, FixedBox::new(10.0, 10.0));
            r.layout(BoxConstraints::loose(100.0, 100.0));
            r
        };
        let mut result = HitTestResult::new();
        assert!(!region.hit_test(Offset::new(50.0, 50.0), &mut result));
        assert!(result.is_empty());
    }

    #[test]
    fn transparent_subtrees_are_not_hit_targets() {
        let mut opacity = RenderOpacity::new(0.0, RenderPointerRegion::new(3, FixedBox::new(50.0, 50.0)));
        opacity.layout(BoxConstraints::loose(100.0, 100.0));
        let mut result = HitTestResult::new();
        assert!(!opacity.hit_test(Offset::new(10.0, 10.0), &mut result));
    }

    #[test]
    fn scrolling_moves_the_hit_test_with_the_content() {
        let mut viewport = RenderViewport::new(
            Axis::Vertical,
            RenderPointerRegion::new(9, FixedBox::new(100.0, 400.0)),
        );
        viewport.layout(BoxConstraints::tight(100.0, 100.0));
        viewport.set_offset(50.0);

        let mut result = HitTestResult::new();
        assert!(viewport.hit_test(Offset::new(10.0, 10.0), &mut result));
        // A point 10 down the window is 60 down the content.
        assert_eq!(result.innermost().unwrap().local_position.dy, 60.0);
    }
}
