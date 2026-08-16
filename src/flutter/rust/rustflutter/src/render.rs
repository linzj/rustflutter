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

use crate::engine::{Canvas, Color, LayerTree, Paint, Paragraph, Rect, Style, TextStyle};
use crate::gestures::PointerHandlers;
use crate::painting::{ClipBehavior, Gradient, Image, RenderPath};

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

    pub fn scaled(&self, factor: f32) -> Offset {
        Offset { dx: self.dx * factor, dy: self.dy * factor }
    }

    /// How long this offset is. Upstream's `Offset.distance`.
    pub fn distance(&self) -> f32 {
        self.distance_squared().sqrt()
    }

    /// The same without the square root, for comparing against a threshold --
    /// which is all the gesture code ever does with it.
    pub fn distance_squared(&self) -> f32 {
        self.dx * self.dx + self.dy * self.dy
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

/// Composes two 2D affines: the result applies `right` and then `left`.
///
/// A matrix is `[a, b, c, d, e, f]`, read as the rows `(a c e)` and `(b d f)`,
/// so a point goes to `(a·x + c·y + e, b·x + d·y + f)`. Same convention as
/// `Canvas::transform` and `LayerTree::push_transform`.
fn compose_affine(left: [f32; 6], right: [f32; 6]) -> [f32; 6] {
    let [a1, b1, c1, d1, e1, f1] = left;
    let [a2, b2, c2, d2, e2, f2] = right;
    [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]
}

/// What a render object paints into.
///
/// Two things at once, exactly as upstream's `PaintingContext` is: a canvas to
/// draw on, and the place where a subtree gets a compositing layer of its own.
///
/// The canvas is owned rather than borrowed, and it is not the same canvas for
/// the whole frame. Opening a layer ends the picture in progress, hands it to
/// the tree, and starts a fresh one inside the new layer; closing it does the
/// same in reverse. A frame is therefore a tree of layers with pictures at the
/// leaves -- which is what the compositor is built to consume -- rather than
/// one picture with the clips and the transforms recorded inside it, where
/// `Preroll` has nothing to look at, the raster cache has nothing to key on,
/// and every frame damages the whole screen.
///
/// Pictures are started lazily, so a layer boundary with nothing drawn on
/// either side of it does not leave an empty display list behind.
pub struct PaintContext<'a> {
    /// The picture being recorded, if anything has asked to draw yet.
    canvas: Option<Canvas>,
    /// Where finished pictures and opened layers go.
    tree: &'a mut LayerTree,
    /// Cull rectangle for every picture in this frame -- the viewport, in
    /// logical pixels.
    cull: Size,
}

impl<'a> PaintContext<'a> {
    /// Starts painting a frame into `tree`. `cull` is the viewport in logical
    /// pixels; anything recorded outside it is dropped at record time.
    pub fn new(tree: &'a mut LayerTree, cull: Size) -> PaintContext<'a> {
        PaintContext { canvas: None, tree, cull }
    }

    /// The picture being recorded, started if this is the first draw since the
    /// last layer boundary.
    pub fn canvas(&mut self) -> &mut Canvas {
        let cull = self.cull;
        self.canvas
            .get_or_insert_with(|| Canvas::new(cull.width, cull.height))
    }

    /// Ends the picture in progress and adds it to the current layer. A picture
    /// nothing drew into is not started, so there is nothing to add.
    fn flush(&mut self) {
        if let Some(canvas) = self.canvas.take() {
            let list = canvas.build();
            self.tree.add_display_list(&list, 0.0, 0.0);
        }
    }

    /// Paints `child` at `offset`.
    ///
    /// Every parent should route a child through here rather than calling
    /// `paint` directly: it is the one place a repaint boundary can be
    /// introduced without touching each render object again.
    pub fn paint_child(&mut self, child: &dyn RenderBox, offset: Offset) {
        child.paint(self, offset);
    }

    /// Paints `body` inside a compositing layer that `open` pushes.
    ///
    /// The picture in progress is closed first, so the layer's content starts
    /// clean, and closed again afterwards, so whatever the caller draws next
    /// lands outside the layer rather than inside it.
    fn in_layer(&mut self, open: impl FnOnce(&mut LayerTree), body: impl FnOnce(&mut Self)) {
        self.flush();
        open(self.tree);
        body(self);
        self.flush();
        self.tree.pop();
    }

    /// Clips `child` to `rect`, optionally with rounded corners.
    ///
    /// `rect` is in the same coordinates the caller paints in, and `child` is
    /// painted at `offset` unchanged -- matching upstream's `pushClipRect`,
    /// which shifts the clip rather than the child.
    pub fn push_clip_rect(
        &mut self,
        rect: Rect,
        corner_radius: f32,
        behavior: ClipBehavior,
        child: &dyn RenderBox,
        offset: Offset,
    ) {
        self.in_layer(
            |tree| {
                if corner_radius > 0.0 {
                    tree.push_clip_rounded_rect(rect, corner_radius, corner_radius, behavior);
                } else {
                    tree.push_clip_rect(rect, behavior);
                }
            },
            |context| child.paint(context, offset),
        );
    }

    /// Clips `child` to `path`, which is in the child's own coordinates.
    ///
    /// Two layers, because a clip layer holds its path in the parent's
    /// coordinates and a `RenderPath` cannot be shifted: the transform layer
    /// moves the origin, the clip layer then reads the path where it was built.
    pub fn push_clip_path(
        &mut self,
        path: &RenderPath,
        behavior: ClipBehavior,
        child: &dyn RenderBox,
        offset: Offset,
    ) {
        self.in_layer(
            |tree| tree.push_offset(offset.dx, offset.dy),
            |context| {
                context.in_layer(
                    |tree| tree.push_clip_path(path, behavior),
                    |context| child.paint(context, Offset::ZERO),
                );
            },
        );
    }

    /// Composites `child` at `alpha`, translated by `offset`.
    ///
    /// The translation belongs to the layer rather than to a transform around
    /// it because that is how `OpacityLayer` is built, and it is what lets the
    /// compositor move a cached subtree without re-rasterizing it.
    pub fn push_opacity(&mut self, alpha: u8, offset: Offset, child: &dyn RenderBox) {
        self.in_layer(
            |tree| tree.push_opacity(alpha, offset.dx, offset.dy),
            |context| child.paint(context, Offset::ZERO),
        );
    }

    /// Applies `matrix` about `pivot`, positioned at `offset`, to `child`.
    pub fn push_transform(
        &mut self,
        matrix: [f32; 6],
        pivot: Offset,
        offset: Offset,
        child: &dyn RenderBox,
    ) {
        // Move the origin to the pivot, transform, move back -- one composed
        // affine rather than three canvas calls, because a layer takes one.
        let to_pivot = [1.0, 0.0, 0.0, 1.0, offset.dx + pivot.dx, offset.dy + pivot.dy];
        let from_pivot = [1.0, 0.0, 0.0, 1.0, -pivot.dx, -pivot.dy];
        let composed = compose_affine(compose_affine(to_pivot, matrix), from_pivot);
        let [a, b, c, d, e, f] = composed;
        self.in_layer(
            |tree| tree.push_transform(a, b, c, d, e, f),
            |context| child.paint(context, Offset::ZERO),
        );
    }
}

impl Drop for PaintContext<'_> {
    /// Hands over whatever was still being recorded. Dropping is how a frame
    /// ends, so forgetting to close it is not a way to lose the last picture.
    fn drop(&mut self) {
        self.flush();
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
    /// Painted under the box, in order. Empty for anything sitting flat on the
    /// surface, which is most things.
    shadows: Vec<crate::painting::BoxShadow>,
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
            shadows: Vec::new(),
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

    /// Casts these shadows under the box.
    pub fn with_shadows(mut self, shadows: Vec<crate::painting::BoxShadow>) -> Self {
        self.shadows = shadows;
        self
    }

    /// Casts the shadows Material gives something at this elevation.
    pub fn with_elevation(self, elevation: u32) -> Self {
        self.with_shadows(crate::painting::elevation_shadows(elevation).to_vec())
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
        // Under everything, in the order they were given: the shape moved by
        // the shadow's offset and grown by its spread, filled with a blurred
        // paint. That is what `BoxDecoration._paintShadows` does, and the
        // reason the spread inflates the rect rather than widening the blur is
        // that a spread is a bigger object, not a softer edge.
        for shadow in &self.shadows {
            let spread = shadow.spread_radius;
            let shadow_rect = Rect::ltrb(
                rect.left + shadow.offset.dx - spread,
                rect.top + shadow.offset.dy - spread,
                rect.right + shadow.offset.dx + spread,
                rect.bottom + shadow.offset.dy + spread,
            );
            if shadow_rect.right <= shadow_rect.left || shadow_rect.bottom <= shadow_rect.top {
                // Spread far enough inwards to leave nothing to draw.
                continue;
            }
            let paint = shadow.to_paint();
            if self.corner_radius > 0.0 {
                let radius = (self.corner_radius + spread).max(0.0);
                context.canvas().draw_rounded_rect(shadow_rect, radius, &paint);
            } else {
                context.canvas().draw_rect(shadow_rect, &paint);
            }
        }
        if let Some(paint) = self.build_paint(rect) {
            if self.corner_radius > 0.0 {
                context.canvas().draw_rounded_rect(rect, self.corner_radius, &paint);
            } else {
                context.canvas().draw_rect(rect, &paint);
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
                context.canvas().draw_rounded_rect(
                    inset,
                    (self.corner_radius - half).max(0.0),
                    &paint,
                );
            } else {
                context.canvas().draw_rect(inset, &paint);
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
    /// The styled runs, when there is more than one. Empty for the ordinary
    /// case of a single style, which keeps `content` and `style` as the whole
    /// description and costs nothing.
    ///
    /// Upstream the two cases are the same thing -- a `Text` builds a
    /// `TextSpan` either way -- but a paragraph of one run is most paragraphs,
    /// and it is worth not allocating for it.
    runs: Vec<(String, TextStyle)>,
    max_lines: Option<usize>,
    /// The reader's text size, as it was where this paragraph was built.
    ///
    /// Taken at construction rather than read at layout, because by layout the
    /// walk that knew which `MediaQuery` this sits under is over. Upstream
    /// `RenderParagraph` keeps the same value in the same way, as a
    /// `textScaler` field set by `Text.build`.
    text_scale: f32,
    /// Shared with the cache rather than owned, so a tree rebuilt around
    /// unchanged text re-uses the shaping instead of repeating it.
    paragraph: Option<Rc<Paragraph>>,
    size: Size,
}

impl RenderParagraph {
    pub fn new(content: impl Into<String>) -> RenderParagraph {
        RenderParagraph {
            content: content.into(),
            style: TextStyle::default(),
            runs: Vec::new(),
            max_lines: None,
            text_scale: crate::media_query::current_text_scale(),
            paragraph: None,
            size: Size::ZERO,
        }
    }

    /// A paragraph of differently styled runs.
    ///
    /// One paragraph, not a row of texts: the line breaking has to see the
    /// whole sentence, or a bold word near the right margin wraps as though it
    /// were the start of a new paragraph. Upstream this is `Text.rich` over a
    /// tree of `TextSpan`s; the tree is flat here because a nested span's
    /// style is resolved against its parent's before shaping anyway.
    pub fn rich(runs: Vec<(String, TextStyle)>) -> RenderParagraph {
        let content = runs.iter().map(|(text, _)| text.as_str()).collect::<String>();
        let style = runs.first().map(|(_, style)| style.clone()).unwrap_or_default();
        RenderParagraph {
            content,
            style,
            runs,
            max_lines: None,
            text_scale: crate::media_query::current_text_scale(),
            paragraph: None,
            size: Size::ZERO,
        }
    }

    /// Whether this paragraph has more than one style in it.
    fn is_rich(&self) -> bool {
        self.runs.len() > 1
    }

    /// Shapes this paragraph at `width`, however many runs it has.
    fn shape_at(&self, width: f32) -> Rc<Paragraph> {
        if self.is_rich() {
            crate::painting::shape_rich(
                &self.runs,
                self.style.align,
                self.max_lines,
                width,
                self.text_scale,
            )
        } else {
            crate::painting::shape(&self.content, &self.style, width, self.text_scale)
        }
    }

    /// Shapes at a scale of the caller's choosing rather than the one that was
    /// in force where this was built.
    ///
    /// Upstream's `Text.textScaler` argument, which overrides the
    /// `MediaQuery`'s for that one paragraph.
    pub fn with_text_scale(mut self, scale: f32) -> Self {
        self.text_scale = scale;
        self
    }

    /// The scale this paragraph will be shaped at.
    pub fn text_scale(&self) -> f32 {
        self.text_scale
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
        let paragraph = self.shape_at(width);
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
        let paragraph = self.shape_at(width);
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
            context.canvas().draw_paragraph(paragraph, offset.dx, offset.dy);
        }
    }

    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        self.shape_at(f32::MAX / 4.0).min_intrinsic_width()
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        self.shape_at(f32::MAX / 4.0).max_intrinsic_width()
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
    image: Rc<Image>,
    fit: BoxFit,
    size: Size,
}

impl RenderImage {
    /// Shared rather than owned, because a render tree is rebuilt every frame
    /// and decoding a PNG sixty times a second to draw the same picture is not
    /// a thing anyone wants. The caller decodes once and keeps the handle.
    pub fn new(image: Rc<Image>) -> RenderImage {
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
            .canvas()
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

// -- Single child: full width -------------------------------------------------

/// Takes the full width it is offered, and its child's height.
///
/// This is what makes a run of cards in a column line up: without it each one
/// is as wide as its own contents, so a card holding a sentence and a card
/// holding a progress bar come out different widths in the same list.
///
/// It exists as its own object rather than as a tight-width `SizedBox` because
/// "the full width" is not a number the caller knows -- and because the width
/// on offer is not always finite. Inside a horizontally scrolling viewport
/// there is no full width to take, and forcing one there produces an infinite
/// box rather than an error. In that case it defers to its child, which does
/// know how wide it wants to be.
pub struct RenderFullWidth {
    child: Option<BoxedRender>,
    size: Size,
}

impl RenderFullWidth {
    pub fn new() -> RenderFullWidth {
        RenderFullWidth { child: None, size: Size::ZERO }
    }

    pub fn with_child(mut self, child: impl RenderBox + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }
}

impl Default for RenderFullWidth {
    fn default() -> RenderFullWidth {
        RenderFullWidth::new()
    }
}

impl RenderBox for RenderFullWidth {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let inner = if constraints.has_bounded_width() {
            BoxConstraints {
                min_width: constraints.max_width,
                max_width: constraints.max_width,
                ..constraints
            }
        } else {
            constraints
        };
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
        self.child.as_ref().map_or(0.0, |child| child.min_intrinsic_width(height))
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.as_ref().map_or(0.0, |child| child.max_intrinsic_width(height))
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.as_ref().map_or(0.0, |child| child.min_intrinsic_height(width))
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.as_ref().map_or(0.0, |child| child.max_intrinsic_height(width))
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

    /// Pinned to all four edges, so the child is exactly as big as the stack.
    ///
    /// The common case for a background or an overlay, and worth a name because
    /// spelling it out four times invites getting one of them wrong.
    pub fn fill() -> StackPosition {
        StackPosition {
            left: Some(0.0),
            top: Some(0.0),
            right: Some(0.0),
            bottom: Some(0.0),
            width: None,
            height: None,
        }
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

    /// Adds an already-boxed child, for a caller that has one.
    pub fn push_boxed(mut self, child: BoxedRender) -> Self {
        self.children.push(StackChild { render: child, position: StackPosition::default() });
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

/// Draws its child and is invisible to the pointer.
///
/// Upstream's `IgnorePointer`. What it is for: something drawn over the top --
/// a scrollbar, a gradient, a watermark -- that must not take the taps meant
/// for what is underneath it. Without it the topmost thing in a stack takes
/// every press that lands on it, whether or not it wanted one.
pub struct RenderIgnorePointer {
    child: BoxedRender,
    size: Size,
}

impl RenderIgnorePointer {
    pub fn new(child: impl RenderBox + 'static) -> RenderIgnorePointer {
        RenderIgnorePointer { child: Box::new(child), size: Size::ZERO }
    }

    pub fn boxed(child: BoxedRender) -> RenderIgnorePointer {
        RenderIgnorePointer { child, size: Size::ZERO }
    }
}

impl RenderBox for RenderIgnorePointer {
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

    fn hit_test(&self, _position: Offset, _result: &mut HitTestResult) -> bool {
        // The whole point: nothing here, look further down.
        false
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

/// Passes its child through and writes down how big it turned out.
///
/// Layout answers a question the build could not: how much room this widget
/// actually got. Anything that needs the answer *next* frame -- a splash that
/// has to reach the corners, a scroll offset that has to be clamped to an
/// extent -- reads it from the cell this fills in. `ListView`'s extent sink is
/// the same arrangement, and upstream reaches the same answer by keeping the
/// render object across frames and asking it.
pub struct RenderSizeReporter {
    sink: Rc<std::cell::Cell<Size>>,
    child: BoxedRender,
    size: Size,
}

impl RenderSizeReporter {
    pub fn new(
        sink: Rc<std::cell::Cell<Size>>,
        child: impl RenderBox + 'static,
    ) -> RenderSizeReporter {
        RenderSizeReporter { sink, child: Box::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderSizeReporter {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.sink.set(self.size);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(self.child.as_ref(), offset);
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.child.hit_test(position, result)
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

// -- Wrap ---------------------------------------------------------------------

/// A flex that starts a new line when it runs out of room.
///
/// Upstream's `RenderWrap`. A `Row` overflows; this one wraps, which is what
/// anything made of an unknown number of small things wants -- a bag of chips,
/// a set of tags, a keyboard.
pub struct RenderWrap {
    direction: Axis,
    /// Between children in a line, and between the lines themselves.
    spacing: f32,
    run_spacing: f32,
    alignment: MainAxisAlignment,
    cross_alignment: CrossAxisAlignment,
    children: Vec<BoxedRender>,
    /// Where each child ended up, filled in by layout.
    offsets: Vec<Offset>,
    size: Size,
}

impl RenderWrap {
    pub fn new(direction: Axis) -> RenderWrap {
        RenderWrap {
            direction,
            spacing: 0.0,
            run_spacing: 0.0,
            alignment: MainAxisAlignment::Start,
            cross_alignment: CrossAxisAlignment::Start,
            children: Vec::new(),
            offsets: Vec::new(),
            size: Size::ZERO,
        }
    }

    pub fn horizontal() -> RenderWrap {
        RenderWrap::new(Axis::Horizontal)
    }

    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Between one line and the next.
    pub fn with_run_spacing(mut self, run_spacing: f32) -> Self {
        self.run_spacing = run_spacing;
        self
    }

    pub fn with_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// How a child sits inside its line, when the line is taller than it is.
    pub fn with_cross_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_alignment = alignment;
        self
    }

    pub fn push(mut self, child: impl RenderBox + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn push_boxed(mut self, child: BoxedRender) -> Self {
        self.children.push(child);
        self
    }

    fn main(&self, size: Size) -> f32 {
        match self.direction {
            Axis::Horizontal => size.width,
            Axis::Vertical => size.height,
        }
    }

    fn cross(&self, size: Size) -> f32 {
        match self.direction {
            Axis::Horizontal => size.height,
            Axis::Vertical => size.width,
        }
    }
}

/// One line of a [`RenderWrap`]: which children, how long, how thick.
struct Run {
    first: usize,
    count: usize,
    main: f32,
    cross: f32,
}

impl RenderBox for RenderWrap {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.offsets.clear();
        self.offsets.resize(self.children.len(), Offset::ZERO);
        if self.children.is_empty() {
            self.size = constraints.constrain(Size::ZERO);
            return self.size;
        }

        // Each child is measured against the line's length and nothing else:
        // it may be as thick as it likes, because the line grows to fit it.
        let limit = match self.direction {
            Axis::Horizontal => constraints.max_width,
            Axis::Vertical => constraints.max_height,
        };
        let child_constraints = match self.direction {
            Axis::Horizontal => BoxConstraints::new(0.0, limit, 0.0, f32::INFINITY),
            Axis::Vertical => BoxConstraints::new(0.0, f32::INFINITY, 0.0, limit),
        };

        let direction = self.direction;
        let spacing = self.spacing;
        let mut runs: Vec<Run> = Vec::new();
        let mut sizes: Vec<Size> = Vec::with_capacity(self.children.len());
        let mut current = Run { first: 0, count: 0, main: 0.0, cross: 0.0 };

        for (index, child) in self.children.iter_mut().enumerate() {
            let size = child.layout(child_constraints);
            sizes.push(size);
            let child_main = match direction {
                Axis::Horizontal => size.width,
                Axis::Vertical => size.height,
            };
            let child_cross = match direction {
                Axis::Horizontal => size.height,
                Axis::Vertical => size.width,
            };
            let with_spacing =
                if current.count == 0 { child_main } else { current.main + spacing + child_main };
            // A line that is already full starts another. The first child of a
            // line stays on it however long it is: there is nowhere else for it
            // to go, and moving it would leave an empty line.
            if current.count > 0 && with_spacing > limit {
                runs.push(current);
                current = Run { first: index, count: 0, main: 0.0, cross: 0.0 };
            }
            current.main =
                if current.count == 0 { child_main } else { current.main + spacing + child_main };
            current.cross = current.cross.max(child_cross);
            current.count += 1;
        }
        runs.push(current);

        let longest = runs.iter().fold(0.0f32, |longest, run| longest.max(run.main));
        let total_cross: f32 = runs.iter().map(|run| run.cross).sum::<f32>()
            + self.run_spacing * (runs.len() as f32 - 1.0).max(0.0);
        self.size = constraints.constrain(match self.direction {
            Axis::Horizontal => Size::new(longest, total_cross),
            Axis::Vertical => Size::new(total_cross, longest),
        });

        // Position: along each line by the main-axis alignment, across it by
        // the cross-axis one.
        let available_main = self.main(self.size);
        let mut cross_offset = 0.0;
        for run in &runs {
            let free = (available_main - run.main).max(0.0);
            let (mut main_offset, gap) = match self.alignment {
                MainAxisAlignment::Start => (0.0, 0.0),
                MainAxisAlignment::Center => (free / 2.0, 0.0),
                MainAxisAlignment::End => (free, 0.0),
                MainAxisAlignment::SpaceBetween if run.count > 1 => {
                    (0.0, free / (run.count as f32 - 1.0))
                }
                MainAxisAlignment::SpaceBetween => (0.0, 0.0),
                MainAxisAlignment::SpaceAround => {
                    let each = free / run.count as f32;
                    (each / 2.0, each)
                }
                MainAxisAlignment::SpaceEvenly => {
                    let each = free / (run.count as f32 + 1.0);
                    (each, each)
                }
            };
            for index in run.first..run.first + run.count {
                let size = sizes[index];
                let child_cross = self.cross(size);
                let within = match self.cross_alignment {
                    // Baseline alignment needs every child in the line
                    // measured against one another's text, which a wrap does
                    // not do -- upstream's `Wrap` has no baseline option
                    // either. It sits at the start, like everything else that
                    // is not centred or ended.
                    CrossAxisAlignment::Start
                    | CrossAxisAlignment::Stretch
                    | CrossAxisAlignment::Baseline => 0.0,
                    CrossAxisAlignment::Center => (run.cross - child_cross) / 2.0,
                    CrossAxisAlignment::End => run.cross - child_cross,
                };
                self.offsets[index] = match self.direction {
                    Axis::Horizontal => Offset::new(main_offset, cross_offset + within),
                    Axis::Vertical => Offset::new(cross_offset + within, main_offset),
                };
                main_offset += self.main(size) + self.spacing + gap;
            }
            cross_offset += run.cross + self.run_spacing;
        }

        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        for (child, child_offset) in self.children.iter().zip(&self.offsets) {
            context.paint_child(child.as_ref(), offset.plus(*child_offset));
        }
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        for (child, child_offset) in self.children.iter().zip(&self.offsets).rev() {
            if child.hit_test(position.minus(*child_offset), result) {
                return true;
            }
        }
        false
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        // Everything on one line, which is the width at which nothing wraps.
        match self.direction {
            Axis::Horizontal => {
                let sum: f32 =
                    self.children.iter().map(|c| c.max_intrinsic_width(height)).sum();
                sum + self.spacing * (self.children.len() as f32 - 1.0).max(0.0)
            }
            Axis::Vertical => self
                .children
                .iter()
                .map(|c| c.max_intrinsic_width(height))
                .fold(0.0, f32::max),
        }
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        // One child per line, so the widest child is the narrowest this can be.
        self.children
            .iter()
            .map(|c| c.min_intrinsic_width(height))
            .fold(0.0, f32::max)
    }
}

// -- Aspect ratio -------------------------------------------------------------

/// Sizes itself to a width-over-height ratio, inside its constraints.
///
/// Upstream's `RenderAspectRatio`, including the order it tries things in:
/// take the width if there is one, work the height out from it, then walk the
/// result back inside each constraint in turn. The order matters -- doing it
/// the other way round gives a box that satisfies the ratio and breaks the
/// constraints.
pub struct RenderAspectRatio {
    ratio: f32,
    child: BoxedRender,
    size: Size,
}

impl RenderAspectRatio {
    pub fn new(ratio: f32, child: impl RenderBox + 'static) -> RenderAspectRatio {
        RenderAspectRatio { ratio, child: Box::new(child), size: Size::ZERO }
    }

    fn applied(&self, constraints: BoxConstraints) -> Size {
        if constraints.is_tight() {
            return constraints.smallest();
        }
        let ratio = if self.ratio > 0.0 { self.ratio } else { 1.0 };
        let mut width = constraints.max_width;
        let mut height;
        if width.is_finite() {
            height = width / ratio;
        } else {
            height = constraints.max_height;
            width = height * ratio;
        }
        if width > constraints.max_width {
            width = constraints.max_width;
            height = width / ratio;
        }
        if height > constraints.max_height {
            height = constraints.max_height;
            width = height * ratio;
        }
        if width < constraints.min_width {
            width = constraints.min_width;
            height = width / ratio;
        }
        if height < constraints.min_height {
            height = constraints.min_height;
            width = height * ratio;
        }
        constraints.constrain(Size::new(width, height))
    }
}

impl RenderBox for RenderAspectRatio {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.applied(constraints);
        self.child.layout(BoxConstraints::tight(self.size.width, self.size.height));
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
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        if height.is_finite() { height * self.ratio } else { self.child.min_intrinsic_width(height) }
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        if height.is_finite() { height * self.ratio } else { self.child.max_intrinsic_width(height) }
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        if width.is_finite() { width / self.ratio } else { self.child.min_intrinsic_height(width) }
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        if width.is_finite() { width / self.ratio } else { self.child.max_intrinsic_height(width) }
    }
}

// -- Intrinsics ---------------------------------------------------------------

/// Sizes its child to the child's own preferred width.
///
/// Upstream's `RenderIntrinsicWidth`, and upstream's warning applies word for
/// word: this asks the child how wide it would like to be, which means laying
/// it out speculatively, which is expensive. It is the answer to "make these
/// buttons all as wide as the widest of them" and not much else.
pub struct RenderIntrinsicWidth {
    child: BoxedRender,
    size: Size,
}

impl RenderIntrinsicWidth {
    pub fn new(child: impl RenderBox + 'static) -> RenderIntrinsicWidth {
        RenderIntrinsicWidth { child: Box::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderIntrinsicWidth {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let wanted = self.child.max_intrinsic_width(constraints.max_height);
        let width = wanted.clamp(constraints.min_width, constraints.max_width);
        let tightened = BoxConstraints::new(
            width,
            width,
            constraints.min_height,
            constraints.max_height,
        );
        self.size = self.child.layout(tightened);
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
        true
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
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

/// Sizes its child to the child's own preferred height. See
/// [`RenderIntrinsicWidth`], including the cost.
pub struct RenderIntrinsicHeight {
    child: BoxedRender,
    size: Size,
}

impl RenderIntrinsicHeight {
    pub fn new(child: impl RenderBox + 'static) -> RenderIntrinsicHeight {
        RenderIntrinsicHeight { child: Box::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderIntrinsicHeight {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let wanted = self.child.max_intrinsic_height(constraints.max_width);
        let height = wanted.clamp(constraints.min_height, constraints.max_height);
        let tightened =
            BoxConstraints::new(constraints.min_width, constraints.max_width, height, height);
        self.size = self.child.layout(tightened);
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
        true
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }
}

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
        context.push_transform(self.matrix, pivot, offset, self.child.as_ref());
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
        // 0..255, the alpha an OpacityLayer carries.
        let alpha = (self.opacity * 255.0).round().clamp(0.0, 255.0) as u8;
        context.push_opacity(alpha, offset, self.child.as_ref());
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
        context.push_clip_rect(
            bounds,
            self.corner_radius,
            ClipBehavior::AntiAlias,
            self.child.as_ref(),
            offset,
        );
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
        context.push_clip_path(
            &self.path,
            ClipBehavior::AntiAlias,
            self.child.as_ref(),
            offset,
        );
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
        // A hard edge: the viewport's clip is axis aligned and pixel aligned,
        // so anti-aliasing it would buy nothing and cost an offscreen pass.
        context.push_clip_rect(
            bounds,
            0.0,
            ClipBehavior::HardEdge,
            self.child.as_ref(),
            offset.plus(self.scroll_offset()),
        );
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
    fn full_width_takes_what_it_is_offered_not_what_its_child_wants() {
        let mut box_ = RenderFullWidth::new().with_child(FixedBox::new(30.0, 20.0));
        let size = box_.layout(BoxConstraints::loose(300.0, 100.0));
        assert_eq!(size.width, 300.0);
        // The height still follows the child: only one axis is being forced.
        assert_eq!(size.height, 20.0);
    }

    #[test]
    fn full_width_defers_to_its_child_when_there_is_no_full_width() {
        // Inside a horizontally scrolling viewport there is no width to fill,
        // and taking "all of it" would mean an infinite box.
        let mut box_ = RenderFullWidth::new().with_child(FixedBox::new(30.0, 20.0));
        let size = box_.layout(BoxConstraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: 100.0,
        });
        assert_eq!(size.width, 30.0);
        assert!(size.width.is_finite());
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
    #[test]
    fn a_rich_paragraph_is_one_paragraph() {
        // The reason this exists at all: a sentence with a bold word in it has
        // to break lines as one paragraph. What the stubbed engine can show is
        // the shape of the request -- one paragraph object for three runs --
        // and that the runs are what the cache keys on.
        let style = TextStyle::default();
        let bold = TextStyle { font_weight: 700, ..style.clone() };
        let mut text = RenderParagraph::rich(vec![
            (String::from("Hold "), style.clone()),
            (String::from("Shift"), bold),
            (String::from(" to select"), style.clone()),
        ]);
        text.layout(BoxConstraints::new(0.0, 200.0, 0.0, f32::INFINITY));
        assert_eq!(text.content, "Hold Shift to select", "the runs make one string");
    }

    #[test]
    fn a_paragraph_with_one_style_is_not_rich() {
        let plain = RenderParagraph::new("just text");
        assert!(!plain.is_rich(), "the single-style case should stay the cheap one");
        let one_run = RenderParagraph::rich(vec![(String::from("x"), TextStyle::default())]);
        assert!(!one_run.is_rich());
    }

    #[test]
    fn a_wrap_starts_a_new_line_when_one_fills_up() {
        let mut wrap = RenderWrap::horizontal().with_spacing(10.0).with_run_spacing(4.0);
        for _ in 0..3 {
            wrap = wrap.push(FixedBox::new(40.0, 20.0));
        }
        // Two fit on a line: 40 + 10 + 40 = 90, and a third would need 140.
        let size = wrap.layout(BoxConstraints::new(0.0, 100.0, 0.0, f32::INFINITY));
        assert_eq!(size.width, 90.0);
        assert_eq!(size.height, 44.0, "two lines and the gap between them");
    }

    #[test]
    fn a_wrap_that_fits_is_a_row() {
        let mut wrap = RenderWrap::horizontal().with_spacing(10.0);
        for _ in 0..3 {
            wrap = wrap.push(FixedBox::new(20.0, 20.0));
        }
        let size = wrap.layout(BoxConstraints::new(0.0, 200.0, 0.0, f32::INFINITY));
        assert_eq!(size, Size::new(80.0, 20.0));
    }

    #[test]
    fn a_child_too_wide_for_any_line_still_gets_one() {
        // Otherwise it would start a new line for ever, or vanish.
        let mut wrap = RenderWrap::horizontal().push(FixedBox::new(300.0, 10.0));
        let size = wrap.layout(BoxConstraints::new(0.0, 100.0, 0.0, f32::INFINITY));
        assert_eq!(size.height, 10.0);
    }

    #[test]
    fn an_aspect_ratio_takes_the_width_and_works_out_the_height() {
        let mut box_ = RenderAspectRatio::new(2.0, FixedBox::new(0.0, 0.0));
        let size = box_.layout(BoxConstraints::new(0.0, 200.0, 0.0, f32::INFINITY));
        assert_eq!(size, Size::new(200.0, 100.0));
    }

    #[test]
    fn an_aspect_ratio_gives_up_the_ratio_before_the_constraints() {
        // 2:1 inside a box that is only 50 tall: the width comes down to
        // match, rather than the box overflowing.
        let mut box_ = RenderAspectRatio::new(2.0, FixedBox::new(0.0, 0.0));
        let size = box_.layout(BoxConstraints::new(0.0, 200.0, 0.0, 50.0));
        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn intrinsic_width_asks_the_child_how_wide_it_wants_to_be() {
        // A paragraph's max intrinsic width is its one-line width, which is
        // the case IntrinsicWidth exists for.
        let text = RenderParagraph::new("a short line").with_style(TextStyle::default());
        let wanted = text.max_intrinsic_width(f32::INFINITY);
        let mut sized = RenderIntrinsicWidth::new(text);
        let size = sized.layout(BoxConstraints::new(0.0, 1000.0, 0.0, f32::INFINITY));
        assert!(
            (size.width - wanted).abs() < 1.0,
            "{} should be the text's own width, {wanted}",
            size.width
        );
    }

}

// -- Compositing tests --------------------------------------------------------
//
// These check the *shape of the scene*, which no pixel test can see: a clip
// recorded into a display list and a clip that is its own layer produce the
// same picture and completely different compositing behaviour. The counters
// come from the engine stubs, which record that a call happened without
// pretending to have an opinion about what it did.

#[cfg(test)]
mod compositing_tests {
    use super::*;
    use crate::engine::LayerTree;
    use crate::engine_test_stubs::{layer_calls, reset_layer_calls};

    /// A box that paints one rectangle, so a picture is definitely recorded.
    struct Spot;

    impl RenderBox for Spot {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            constraints.biggest()
        }
        fn size(&self) -> Size {
            Size::new(10.0, 10.0)
        }
        fn paint(&self, context: &mut PaintContext, offset: Offset) {
            let bounds = Rect::xywh(offset.dx, offset.dy, 10.0, 10.0);
            context.canvas().draw_rect(bounds, &Paint::new(Color::WHITE));
        }
    }

    fn paint_into(root: &mut dyn RenderBox) -> crate::engine_test_stubs::LayerCalls {
        reset_layer_calls();
        let mut tree = LayerTree::new(100, 100);
        {
            let mut context = PaintContext::new(&mut tree, Size::new(100.0, 100.0));
            root.paint(&mut context, Offset::ZERO);
        }
        layer_calls()
    }

    #[test]
    fn a_plain_subtree_is_one_picture_and_no_layers() {
        let calls = paint_into(&mut Spot);
        assert_eq!(calls.pushes(), 0, "nothing asked for a layer");
        assert_eq!(calls.display_lists, 1);
    }

    #[test]
    fn a_clip_becomes_a_layer_rather_than_an_operation() {
        let mut clipped = RenderClipRect::new(Spot);
        clipped.layout(BoxConstraints::tight(50.0, 50.0));
        let calls = paint_into(&mut clipped);
        assert_eq!(calls.clip_rects, 1, "the clip stayed inside the picture");
        assert_eq!(calls.pops, 1, "the layer was left open");
    }

    #[test]
    fn a_rounded_clip_uses_the_rounded_layer() {
        let mut clipped = RenderClipRect::new(Spot).with_corner_radius(8.0);
        clipped.layout(BoxConstraints::tight(50.0, 50.0));
        let calls = paint_into(&mut clipped);
        assert_eq!(calls.clip_rounded_rects, 1);
        assert_eq!(calls.clip_rects, 0);
    }

    #[test]
    fn opacity_becomes_a_layer() {
        let mut faded = RenderOpacity::new(0.5, Spot);
        faded.layout(BoxConstraints::tight(50.0, 50.0));
        let calls = paint_into(&mut faded);
        assert_eq!(calls.opacities, 1);
        assert_eq!(calls.pops, 1);
    }

    #[test]
    fn a_fully_opaque_subtree_costs_no_layer() {
        // Upstream skips the layer at alpha 1 for the same reason: it would
        // composite an offscreen buffer to change nothing.
        let mut faded = RenderOpacity::new(1.0, Spot);
        faded.layout(BoxConstraints::tight(50.0, 50.0));
        let calls = paint_into(&mut faded);
        assert_eq!(calls.pushes(), 0);
    }

    #[test]
    fn an_invisible_subtree_is_not_painted_at_all() {
        let mut faded = RenderOpacity::new(0.0, Spot);
        faded.layout(BoxConstraints::tight(50.0, 50.0));
        let calls = paint_into(&mut faded);
        assert_eq!(calls.pushes(), 0);
        assert_eq!(calls.display_lists, 0, "an invisible subtree still recorded");
    }

    #[test]
    fn a_transform_becomes_a_layer() {
        let mut turned = RenderTransform::rotate(30.0, Spot);
        turned.layout(BoxConstraints::tight(50.0, 50.0));
        let calls = paint_into(&mut turned);
        assert_eq!(calls.transforms, 1);
        assert_eq!(calls.pops, 1);
    }

    #[test]
    fn a_scrolling_viewport_clips_with_a_layer() {
        let mut viewport = RenderViewport::new(Axis::Vertical, Spot);
        viewport.layout(BoxConstraints::tight(50.0, 50.0));
        let calls = paint_into(&mut viewport);
        assert_eq!(calls.clip_rects, 1);
        assert_eq!(calls.pops, 1);
    }

    #[test]
    fn every_layer_that_opens_is_closed() {
        // Nested three deep, with a picture at each level, so an unbalanced pop
        // anywhere shows up as a mismatch rather than cancelling out.
        let mut nested = RenderClipRect::new(RenderOpacity::new(
            0.5,
            RenderTransform::scale(2.0, 2.0, Spot),
        ));
        nested.layout(BoxConstraints::tight(50.0, 50.0));
        let calls = paint_into(&mut nested);
        assert_eq!(calls.pushes(), 3);
        assert_eq!(calls.pops, calls.pushes());
    }

    #[test]
    fn a_layer_boundary_splits_the_picture_in_two() {
        // A parent that draws, then a child in a layer, then draws again: the
        // drawing before and after cannot share a display list with the layer's
        // contents, because they are on the other side of it in paint order.
        struct BeforeAndAfter {
            inner: RenderClipRect,
        }

        impl RenderBox for BeforeAndAfter {
            fn layout(&mut self, constraints: BoxConstraints) -> Size {
                self.inner.layout(constraints)
            }
            fn size(&self) -> Size {
                self.inner.size()
            }
            fn paint(&self, context: &mut PaintContext, offset: Offset) {
                let paint = Paint::new(Color::WHITE);
                context.canvas().draw_rect(Rect::xywh(0.0, 0.0, 5.0, 5.0), &paint);
                context.paint_child(&self.inner, offset);
                context.canvas().draw_rect(Rect::xywh(0.0, 0.0, 5.0, 5.0), &paint);
            }
        }

        let mut root = BeforeAndAfter { inner: RenderClipRect::new(Spot) };
        root.layout(BoxConstraints::tight(50.0, 50.0));
        let calls = paint_into(&mut root);
        // Before, inside, after.
        assert_eq!(calls.display_lists, 3);
        assert_eq!(calls.clip_rects, 1);
    }

    #[test]
    fn an_empty_layer_leaves_no_empty_picture() {
        // Nothing draws, so no picture should be started -- an empty display
        // list still costs a layer, a preroll and a dispatch.
        struct Blank;

        impl RenderBox for Blank {
            fn layout(&mut self, constraints: BoxConstraints) -> Size {
                constraints.biggest()
            }
            fn size(&self) -> Size {
                Size::new(10.0, 10.0)
            }
            fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
        }

        let mut clipped = RenderClipRect::new(Blank);
        clipped.layout(BoxConstraints::tight(50.0, 50.0));
        let calls = paint_into(&mut clipped);
        assert_eq!(calls.clip_rects, 1);
        assert_eq!(calls.display_lists, 0);
    }
}
