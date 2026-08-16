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
//! Upstream keeps its bookkeeping on `RenderObject`, the base class every
//! render object extends: the parent pointer, whether the layout is stale,
//! what it was last laid out against. There is no base class here -- `RenderBox`
//! is a trait and each implementor has only its own fields -- so it lives on
//! [`RenderRef`], the handle every render object is reached through and that
//! every parent stores its children as. That makes the handle the one place a
//! question can be asked about *any* render object, which is what a base class
//! is for.
//!
//! Three things follow from it, and they are the same three upstream gets:
//! a frame does not lay out what it laid out before ([`RenderRef::layout`]),
//! a rebuild does not replace what it can tell instead
//! ([`RenderRef::reconfigure`]), and a subtree that drew the same thing hands
//! back the drawing ([`RenderRepaintBoundary`]). What is missing is the
//! *relayout boundary*: upstream can begin a frame part-way down the tree
//! because `PipelineOwner` keeps the dirty ones and visits each, and there is
//! no pipeline owner here, so [`RenderRef::mark_needs_layout`] walks to the
//! root and the saving is in the siblings the descent never enters.
//!
//! Hit testing is here rather than with input, because only a render object
//! knows its own geometry. [`RenderBox::hit_test`] walks the tree back to front
//! and records the entries a gesture recogniser will later arbitrate over.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

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

    //--------------------------------------------------------------------------
    /// Records `child` into a layer of its own and keeps it.
    ///
    /// The content is recorded at the origin, not at `offset`: where the layer
    /// goes is decided when it is added, so a boundary that only moved is
    /// re-added rather than re-recorded. Upstream's `PaintingContext` does the
    /// same for the same reason -- the layer is `RenderObject.layer` and its
    /// position is the parent's business.
    pub fn record_retained(
        &mut self,
        child: &dyn RenderBox,
        offset: Offset,
    ) -> Option<crate::engine::RetainedLayer> {
        self.flush();
        // The transform that puts the recorded content where it belongs. The
        // layer inside it holds only the drawing, which is what makes it worth
        // keeping.
        self.tree.push_offset(offset.dx, offset.dy);
        self.tree.push_retainable();
        child.paint(self, Offset::ZERO);
        self.flush();
        let kept = self.tree.pop_retained();
        self.tree.pop();
        kept
    }

    /// Adds a layer kept from an earlier frame, at `offset`.
    pub fn add_retained(&mut self, layer: &crate::engine::RetainedLayer, offset: Offset) {
        // Whatever is being recorded has to be closed first, or it would land
        // on top of a subtree that was painted before it.
        self.flush();
        self.tree.add_retained(layer, offset.dx, offset.dy);
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

/// Reaching a render object's concrete type again after it has been erased.
///
/// Needed because taking a new configuration means comparing against an object
/// of the same type, and by then the type is behind `dyn RenderBox`. The
/// blanket implementation means no render object writes this itself; upstream
/// gets the same thing for nothing, since `updateRenderObject` is declared
/// `covariant` and the framework has already guaranteed the type.
pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// What taking a new configuration changed.
///
/// Upstream says this by which method a setter calls: `set padding` calls
/// `markNeedsLayout`, `set color` calls `markNeedsPaint`, and `set onTap` calls
/// neither. The three are ordered because an object with several changed fields
/// is worth the loudest of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum UpdateEffect {
    /// Nothing that is measured or drawn: a callback was replaced, and a
    /// callback is neither. Upstream's `set onTap`.
    #[default]
    Nothing,
    /// Drawn differently, measured the same. Upstream's `markNeedsPaint`.
    Repaint,
    /// Measured differently -- and so drawn differently too, since a box that
    /// changed size did not draw the same thing. Upstream's `markNeedsLayout`.
    Relayout,
}

impl UpdateEffect {
    /// The louder of the two. What an object accumulates as it takes each
    /// field, standing in for upstream's several separate `markNeeds…` calls.
    pub fn and(self, other: UpdateEffect) -> UpdateEffect {
        if other > self { other } else { self }
    }

    /// `Relayout` if `changed`, otherwise nothing. The shape of nearly every
    /// line in an `update_from`, and upstream's `if (_field == value) return;`
    /// read the other way round.
    pub fn relayout_if(changed: bool) -> UpdateEffect {
        if changed { UpdateEffect::Relayout } else { UpdateEffect::Nothing }
    }

    /// `Repaint` if `changed`. For a field the layout does not read.
    pub fn repaint_if(changed: bool) -> UpdateEffect {
        if changed { UpdateEffect::Repaint } else { UpdateEffect::Nothing }
    }
}

/// A box in the render tree.
///
/// Implementors must obey three rules, all of which the built-in objects here
/// follow and all of which upstream also requires:
///
/// 1. `layout` returns a size inside the constraints it was given.
/// 2. `size` returns what the last `layout` returned.
/// 3. `paint` draws at the offset it is given and nowhere else; a render object
///    never knows its absolute position.
pub trait RenderBox: AsAny {
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

    // -- Walking the tree without painting it ---------------------------------

    /// Visits the children in paint order, each with where it is painted
    /// relative to this box's own origin.
    ///
    /// This is upstream's `visitChildren` and `applyPaintTransform` in one
    /// method, and they are one here because there is no `parentData` to keep
    /// them apart. Upstream a child carries a `BoxParentData.offset` that its
    /// parent wrote during layout, and `applyPaintTransform` reads it back off
    /// the child; here the parent keeps that offset in whatever field suits it
    /// -- `RenderFlex` has a vector of them, `RenderPadding` computes one from
    /// its insets -- so the parent is the only one who can answer, and it
    /// answers both questions at once.
    ///
    /// **Only a translation.** Upstream's is a `Matrix4`, because a
    /// `RenderTransform` can rotate its child. Here it is an offset, and
    /// [`RenderTransform`] reports its child untransformed -- see the comment
    /// there for why that is the answer that agrees with `hit_test`.
    ///
    /// A box that draws children and does not override this is invisible to
    /// everything that walks without painting, which today is the semantics
    /// tree.
    fn visit_children(&self, _visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {}

    /// The children a screen reader should meet, in reading order.
    ///
    /// Upstream's `visitChildrenForSemantics`, with the same default and the
    /// same reason to override it: a box that would not *paint* a child should
    /// not describe it either, because a thing that is not on the screen is not
    /// on the screen for a reader who is exploring it by touch.
    fn visit_children_for_semantics(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        self.visit_children(visit)
    }

    /// What this box says about itself to a screen reader, if anything.
    ///
    /// Upstream's `describeSemanticsConfiguration`, which fills in a
    /// `SemanticsConfiguration` the framework then assembles into a node. The
    /// two things that answer here are the annotation put there on purpose and
    /// the paragraph, which describes itself because the text on the screen is
    /// the text a reader came for.
    fn describe_semantics(&self) -> Option<crate::semantics::SemanticsAnnotation> {
        None
    }

    // -- Taking a new configuration -------------------------------------------

    /// Takes over `fresh`'s configuration -- `fresh` being a newly built object
    /// of this same type, describing this same position after a rebuild -- and
    /// says what that changed.
    ///
    /// This is upstream's `RenderObjectWidget.updateRenderObject` and the
    /// comparing setters it writes through, in one method. Upstream `Padding`
    /// says `renderObject.padding = padding`, and `RenderPadding.set padding`
    /// returns without marking anything when the value is the one already
    /// there. The two halves are together here because there is no widget class
    /// holding the fields separately to assign from: the new configuration
    /// arrives as a whole object, so it is unpicked in one place per type.
    ///
    /// Returning `None` means "will not", and the caller then makes a new object
    /// as it always did. That is never wrong -- it is what every type did before
    /// any of them answered this -- so an object with a field it cannot compare
    /// should say so rather than guess.
    ///
    /// **Every field is either taken or compared.** A field taken without its
    /// effect being reported shows a stale frame; a field neither taken nor
    /// compared shows a stale value forever. The tests in this module walk the
    /// list.
    fn update_from(&mut self, _fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        None
    }

    /// Throws away whatever was kept from an earlier frame's painting.
    ///
    /// The far end of [`RenderRef::mark_needs_paint`], which is upstream's
    /// `markNeedsPaint` -- there it clears `_layer` on the enclosing repaint
    /// boundary. Only [`RenderRepaintBoundary`] keeps anything, so only it
    /// overrides.
    fn discard_retained(&self) {}
}

//------------------------------------------------------------------------------
/// A subtree that keeps what it painted.
///
/// Upstream this is `RepaintBoundary`, over a `RenderObject` whose
/// `isRepaintBoundary` is true. The layer it produces is kept on the render
/// object, and a frame in which nothing under it changed hands the engine the
/// same layer rather than recording the same drawing again -- which is also
/// what lets the raster cache keep the pixels.
///
/// **When the layer is still good.** Upstream tracks it with `markNeedsPaint`,
/// which walks up to the enclosing boundary. Here it is object identity, the
/// same answer the layout skip uses: a render object that survived the frame is
/// one the element tree did not rebuild, and the drawing of a subtree that was
/// not rebuilt is the drawing it was. A boundary that is laid out again throws
/// its layer away, because a subtree that changed size did not draw the same
/// thing.
///
/// So this is worth putting somewhere a sibling changes often and this does
/// not -- which is why upstream puts one around every item of a lazy list, and
/// why [`crate::scrolling::LazyList`] does too.
pub struct RenderRepaintBoundary {
    child: RenderRef,
    size: Size,
    /// What was painted last time, if it is still what would be painted.
    layer: RefCell<Option<crate::engine::RetainedLayer>>,
}

impl RenderRepaintBoundary {
    pub fn new(child: impl RenderBox + 'static) -> RenderRepaintBoundary {
        RenderRepaintBoundary {
            child: RenderRef::new(child),
            size: Size::ZERO,
            layer: RefCell::new(None),
        }
    }
}

impl RenderBox for RenderRepaintBoundary {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Reaching here at all means the answer was not already known -- the
        // handle would have returned it otherwise -- so whatever was drawn was
        // drawn for a different question.
        *self.layer.borrow_mut() = None;
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        {
            let layer = self.layer.borrow();
            if let Some(layer) = layer.as_ref() {
                context.add_retained(layer, offset);
                return;
            }
        }
        let kept = context.record_retained(&self.child, offset);
        *self.layer.borrow_mut() = kept;
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
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

    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderRepaintBoundary>()?;
        // A boundary has no configuration of its own beyond what it wraps, so
        // the whole question is whether it is still wrapping the same object.
        // If it is, the layer it kept is still the drawing of that object.
        if self.child.is(&fresh.child) {
            return Some(UpdateEffect::Nothing);
        }
        self.child = fresh.child.clone();
        Some(UpdateEffect::Relayout)
    }

    /// Upstream's `markNeedsPaint` clearing `_layer`: what was drawn is no
    /// longer what would be drawn.
    fn discard_retained(&self) {
        *self.layer.borrow_mut() = None;
    }
}

/// A render object, held by more than one thing at once.
///
/// The parent holds it because it is a child; the element that produced it
/// holds it because it owns it across frames. Upstream both hold the same
/// `RenderObject` and the garbage collector makes that unremarkable; here it
/// takes a reference count and a cell, which is what `Rc<RefCell<..>>` is.
///
/// The cell is not a lock in disguise. Layout is the only thing that mutates a
/// render object, it happens on one thread, and a tree has one path to each
/// node -- so the borrow is uncontended by construction, and a panic here would
/// mean the tree had stopped being a tree.
///
/// The object is behind a `Box` inside the cell rather than being the cell, so
/// that it can be *replaced* without the handle changing: an element that keeps
/// its render object across a rebuild has to keep the same handle too, or the
/// parent holding it would be holding the old one. Upstream has no equivalent
/// because a Dart field simply points at whatever it points at.
#[derive(Clone)]
pub struct RenderRef {
    render: Rc<RefCell<Box<dyn RenderBox>>>,
    state: Rc<RenderState>,
}

/// The bookkeeping upstream keeps on `RenderObject` itself.
///
/// There is no base class here to put it on -- `RenderBox` is a trait and each
/// implementor has only its own fields -- so it goes on the handle, which every
/// render object is reached through and which every parent stores its children
/// as. That makes it the one place a question can be asked about *any* render
/// object, which is what a base class is for.
struct RenderState {
    /// Whether this object's last answer is still good. False after a layout,
    /// true when it is made and whenever [`RenderRef::mark_needs_layout`] says
    /// so.
    needs_layout: Cell<bool>,
    /// Whether what this drew last time is still what it would draw.
    ///
    /// Only a repaint boundary keeps a drawing, so only a boundary reads this;
    /// it is on the handle because the walk that sets it is the walk up the
    /// parents, and the parents are here. Upstream's `_needsPaint`.
    needs_paint: Cell<bool>,
    /// What it was last laid out against, and what came out.
    constraints: Cell<Option<BoxConstraints>>,
    size: Cell<Size>,
    /// Who laid it out last. Upstream is told this in `adoptChild`; there is no
    /// adoption step here, so it is noticed during layout instead -- a child's
    /// `layout` is always called from inside its parent's, so the parent is
    /// whatever was already laying out when this one started.
    parent: RefCell<Weak<RenderState>>,
}

thread_local! {
    /// The render objects whose layout is running, innermost last.
    static LAYING_OUT: RefCell<Vec<Rc<RenderState>>> =
        const { RefCell::new(Vec::new()) };
}

/// Keeps the stack right even if a layout panics.
struct LayoutFrame;

impl LayoutFrame {
    fn push(state: &Rc<RenderState>) -> LayoutFrame {
        LAYING_OUT.with(|stack| stack.borrow_mut().push(Rc::clone(state)));
        LayoutFrame
    }
}

impl Drop for LayoutFrame {
    fn drop(&mut self) {
        LAYING_OUT.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

impl RenderRef {
    pub fn new<R: RenderBox + 'static>(render: R) -> RenderRef {
        let mut render = render;
        // A handle wrapped in a handle is two objects where the tree has one,
        // and the outer one is new every frame -- which hides the inner one's
        // identity from the two things that ask for it, the layout skip and the
        // kept layer. It happens because a combinator hands its children's
        // handles to a constructor that wraps whatever it is given. Upstream
        // cannot reach this shape at all: a `RenderObject` is never another
        // `RenderObject`'s entire content.
        if let Some(handle) = (&mut render as &mut dyn Any).downcast_mut::<RenderRef>() {
            return handle.clone();
        }
        RenderRef {
            render: Rc::new(RefCell::new(Box::new(render) as Box<dyn RenderBox>)),
            state: Rc::new(RenderState {
                needs_layout: Cell::new(true),
                needs_paint: Cell::new(false),
                constraints: Cell::new(None),
                size: Cell::new(Size::ZERO),
                parent: RefCell::new(Weak::new()),
            }),
        }
    }

    /// Whether two handles are the same render object.
    ///
    /// What "persistent" means, and the only way to ask: an object that
    /// survived a frame is the same object, not an equal one.
    pub fn is(&self, other: &RenderRef) -> bool {
        Rc::ptr_eq(&self.render, &other.render)
    }

    /// Says this object's layout is no longer good, so the next frame does it
    /// again even at the same constraints.
    ///
    /// Upstream's `markNeedsLayout`, including its early return: an object
    /// already marked has already marked its ancestors, so there is nothing
    /// above it left to tell.
    ///
    /// The marking has to reach the root, because the root is the only place a
    /// layout is ever started from. Upstream stops at the nearest *relayout
    /// boundary* -- an ancestor whose own size this cannot change -- and lays
    /// that subtree out directly, which it can do because `PipelineOwner` keeps
    /// the list of dirty boundaries and visits each. There is no pipeline owner
    /// here; a frame descends from the root or it does not happen. So the
    /// saving is not in starting lower down, it is in the siblings the descent
    /// never enters.
    pub fn mark_needs_layout(&self) {
        let mut state = Rc::clone(&self.state);
        loop {
            if state.needs_layout.get() {
                return;
            }
            state.needs_layout.set(true);
            let parent = state.parent.borrow().upgrade();
            match parent {
                Some(parent) => state = parent,
                None => return,
            }
        }
    }

    /// Says what this object drew is no longer what it would draw, without
    /// saying its size changed.
    ///
    /// Upstream's `markNeedsPaint`, and the reason it is separate from
    /// `markNeedsLayout` is the reason upstream keeps them separate: a box that
    /// changed colour is the same size, and re-measuring the screen to repaint
    /// a swatch is work for nothing.
    ///
    /// Upstream stops at the nearest enclosing repaint boundary, since that is
    /// the layer that has to be recorded again and no layer above it contains
    /// anything but a reference to it. This walks to the root instead and drops
    /// every kept layer on the way, for the same reason
    /// [`RenderRef::mark_needs_layout`] does: there is nothing here holding a
    /// list of boundaries to visit, so the walk cannot stop somewhere it would
    /// then have to be resumed from.
    pub fn mark_needs_paint(&self) {
        let mut state = Rc::clone(&self.state);
        loop {
            if state.needs_paint.get() {
                return;
            }
            state.needs_paint.set(true);
            let parent = state.parent.borrow().upgrade();
            match parent {
                Some(parent) => state = parent,
                None => return,
            }
        }
    }

    /// Whether the next `layout` at these constraints would do any work.
    pub fn needs_layout(&self, constraints: BoxConstraints) -> bool {
        self.state.needs_layout.get() || self.state.constraints.get() != Some(constraints)
    }

    //--------------------------------------------------------------------------
    /// Gives this object a freshly built one describing the same position, and
    /// says whether it took it.
    ///
    /// This is upstream's `RenderObjectElement.update`, which is the whole
    /// point of there being an element in the middle: when its widget changes,
    /// the element does not make a new render object, it hands the new
    /// configuration to the one it already has. What that object had measured,
    /// shaped and drawn survives, and -- because the handle is the same handle
    /// -- so does every parent's belief about which child it is holding, all the
    /// way up. Without it, one changed leaf remakes its whole spine.
    ///
    /// False means the object would not take it: a type that has not answered
    /// [`RenderBox::update_from`], or a `fresh` that turned out to be shared
    /// rather than newly made. The caller then makes a new object, which is what
    /// it did before this existed.
    pub fn reconfigure(&self, fresh: RenderRef) -> bool {
        // The same handle, because whatever built `fresh` had nothing of its
        // own and handed back what it was given. There is no configuration here
        // to take, and taking one from itself would deadlock the cell.
        if self.is(&fresh) {
            return true;
        }
        // Shared means somebody else is already holding it, so it is not a
        // description that was just built for this -- and it cannot be taken
        // apart while they hold it.
        let Ok(cell) = Rc::try_unwrap(fresh.render) else {
            return false;
        };
        let mut fresh = cell.into_inner();
        let Some(effect) = self.render.borrow_mut().update_from(&mut *fresh) else {
            return false;
        };
        match effect {
            UpdateEffect::Nothing => {}
            UpdateEffect::Repaint => self.mark_needs_paint(),
            UpdateEffect::Relayout => self.mark_needs_layout(),
        }
        true
    }
}

impl RenderBox for RenderRef {
    //--------------------------------------------------------------------------
    /// Lays this object out, unless the last answer is still the answer.
    ///
    /// Upstream's `RenderObject.layout` opens with the same test -- if the
    /// object is not dirty and the constraints have not changed, it returns
    /// without descending -- and the reason it is worth having is the same too:
    /// a frame usually changes one thing, and everything else is asked the
    /// question it was asked last time.
    ///
    /// The saving is from above rather than from below: a subtree the element
    /// tree did not rebuild is the same objects it was, so the first of them to
    /// be asked ends the descent for all of them. See
    /// [`RenderRef::mark_needs_layout`] for what that costs -- upstream can
    /// start a layout part-way down and this cannot.
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Noted before the early return, not after: a child that moved to a
        // new parent and did not need laying out has still moved, and a later
        // `mark_needs_layout` would otherwise walk up a chain it left.
        *self.state.parent.borrow_mut() =
            LAYING_OUT.with(|stack| match stack.borrow().last() {
                Some(parent) => Rc::downgrade(parent),
                None => Weak::new(),
            });

        if !self.state.needs_layout.get() && self.state.constraints.get() == Some(constraints) {
            return self.state.size.get();
        }

        let size = {
            let _frame = LayoutFrame::push(&self.state);
            self.render.borrow_mut().layout(constraints)
        };
        self.state.needs_layout.set(false);
        self.state.constraints.set(Some(constraints));
        self.state.size.set(size);
        size
    }
    fn size(&self) -> Size {
        self.render.borrow().size()
    }
    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let render = self.render.borrow();
        // The near end of `mark_needs_paint`. Cleared as it is acted on, so a
        // frame that draws is a frame that answered the question.
        if self.state.needs_paint.replace(false) {
            render.discard_retained();
        }
        render.paint(context, offset)
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        self.render.borrow().visit_children(visit)
    }
    fn visit_children_for_semantics(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        self.render.borrow().visit_children_for_semantics(visit)
    }
    fn describe_semantics(&self) -> Option<crate::semantics::SemanticsAnnotation> {
        self.render.borrow().describe_semantics()
    }
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.render.borrow().hit_test(position, result)
    }
    fn hit_test_id(&self) -> u64 {
        self.render.borrow().hit_test_id()
    }
    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.render.borrow().min_intrinsic_width(height)
    }
    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.render.borrow().max_intrinsic_width(height)
    }
    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.render.borrow().min_intrinsic_height(width)
    }
    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.render.borrow().max_intrinsic_height(width)
    }
    fn distance_to_baseline(&self) -> Option<f32> {
        self.render.borrow().distance_to_baseline()
    }
}

/// What a parent stores for each of its children, and what a build closure
/// hands back.
pub type BoxedRender = RenderRef;

/// Whether two optional children are the same object -- the same one, not an
/// equal one. Identity is the only thing a render tree is reconciled on, and
/// the only thing that says a child survived the rebuild.
fn same_child(a: &Option<BoxedRender>, b: &Option<BoxedRender>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.is(b),
        (None, None) => true,
        _ => false,
    }
}

/// Whether two child lists are the same objects in the same order.
fn same_children(a: &[BoxedRender], b: &[BoxedRender]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.is(b))
}

/// Whether two optional callbacks are the same closure. A closure has no
/// equality beyond where it lives, which is all this needs: a rebuild that
/// handed back the same `Rc` handed back the same behaviour.
pub(crate) fn same_callback<T: ?Sized>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// So a boxed render object works anywhere an unboxed one does.
impl<R: RenderBox + ?Sized + 'static> RenderBox for Box<R> {
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

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        (**self).visit_children(visit)
    }
    fn visit_children_for_semantics(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        (**self).visit_children_for_semantics(visit)
    }
    fn describe_semantics(&self) -> Option<crate::semantics::SemanticsAnnotation> {
        (**self).describe_semantics()
    }
    /// Forwards to the object, and only unwraps `self`.
    ///
    /// `fresh` arrives already unwrapped -- [`RenderRef::reconfigure`] takes it
    /// out of the box it was built in -- so there is nothing to peel off it.
    /// That is also why a *second* box, put there by a combinator whose closure
    /// says `Box::new(...)`, would stop the object recognising the
    /// configuration as its own: the two sides would be one layer apart.
    /// Nothing goes wrong when they are -- the object declines and is replaced,
    /// as everything was before this existed -- but the saving quietly does not
    /// happen, so a combinator should hand back the object, not a box round it.
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        (**self).update_from(fresh)
    }
    fn discard_retained(&self) {
        (**self).discard_retained()
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
#[derive(Clone, Debug, PartialEq)]
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
        self.child = Some(RenderRef::new(child));
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
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderDecoratedBox>()?;
        // Nothing a decoration says is read by `layout`, which measures only
        // the child. Upstream splits the same way: `RenderDecoratedBox`'s
        // `set decoration` calls `markNeedsPaint`, never `markNeedsLayout`.
        let mut effect = UpdateEffect::repaint_if(
            self.fill != fresh.fill
                || self.corner_radius != fresh.corner_radius
                || self.border_width != fresh.border_width
                || self.border_color != fresh.border_color
                || self.shadows != fresh.shadows,
        );
        self.fill = fresh.fill.take();
        self.corner_radius = fresh.corner_radius;
        self.border_width = fresh.border_width;
        self.border_color = fresh.border_color;
        self.shadows = std::mem::take(&mut fresh.shadows);
        effect = effect.and(UpdateEffect::relayout_if(!same_child(&self.child, &fresh.child)));
        self.child = fresh.child.take();
        Some(effect)
    }
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
            context.paint_child(child, offset);
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

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        if let Some(child) = &self.child {
            visit(child, Offset::ZERO);
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
    /// The accessibility node this paragraph is, once anything has asked.
    ///
    /// Taken lazily and kept: a render object outlives the frame now, so an id
    /// taken once stays this paragraph's for as long as it exists -- which is
    /// how long a screen reader should go on treating it as the same text.
    semantics_id: std::cell::Cell<i32>,
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
            semantics_id: std::cell::Cell::new(0),
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
            semantics_id: std::cell::Cell::new(0),
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
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderParagraph>()?;
        let changed = self.content != fresh.content
            || self.style != fresh.style
            || self.runs != fresh.runs
            || self.max_lines != fresh.max_lines
            || self.text_scale != fresh.text_scale;
        if !changed {
            // Everything the shaping depends on is the same, so the shaping is
            // the same -- and shaping is nearly all of what a paragraph costs.
            // The semantics id stays with it: a reader that has been told about
            // this text goes on hearing about the same node.
            return Some(UpdateEffect::Nothing);
        }
        self.content = std::mem::take(&mut fresh.content);
        self.style = fresh.style.clone();
        self.runs = std::mem::take(&mut fresh.runs);
        self.max_lines = fresh.max_lines;
        self.text_scale = fresh.text_scale;
        // What was shaped was shaped for text this no longer holds.
        self.paragraph = None;
        Some(UpdateEffect::Relayout)
    }
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

    /// Text on screen is text a reader came for, and nothing had to ask for it.
    ///
    /// Upstream `Text` reaches this by wrapping itself in a `Semantics` widget
    /// during its own build; here the paragraph is the only thing that knows
    /// what it says, so it says it itself.
    fn describe_semantics(&self) -> Option<crate::semantics::SemanticsAnnotation> {
        if self.content.trim().is_empty() {
            return None;
        }
        if self.semantics_id.get() == 0 {
            self.semantics_id.set(crate::semantics::take_text_id());
        }
        Some(crate::semantics::SemanticsAnnotation::text(
            self.semantics_id.get(),
            &self.content,
        ))
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
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderImage>()?;
        // The same pixels, not equal pixels: an `Image` is a handle to a
        // decoded bitmap, and comparing two of them any other way would mean
        // reading both.
        let mut effect = UpdateEffect::relayout_if(!Rc::ptr_eq(&self.image, &fresh.image));
        self.image = Rc::clone(&fresh.image);
        // The fit decides the destination rect, which only `paint` asks for.
        effect = effect.and(UpdateEffect::repaint_if(self.fit != fresh.fit));
        self.fit = fresh.fit;
        Some(effect)
    }
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
        self.child = Some(RenderRef::new(child));
        self
    }
}

impl Default for RenderFullWidth {
    fn default() -> RenderFullWidth {
        RenderFullWidth::new()
    }
}

impl RenderBox for RenderFullWidth {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderFullWidth>()?;
        let effect = UpdateEffect::relayout_if(!same_child(&self.child, &fresh.child));
        self.child = fresh.child.take();
        Some(effect)
    }
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
            context.paint_child(child, offset);
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

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        if let Some(child) = &self.child {
            visit(child, Offset::ZERO);
        }
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
        self.child = Some(RenderRef::new(child));
        self
    }
}

impl RenderBox for RenderConstrainedBox {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderConstrainedBox>()?;
        let effect = UpdateEffect::relayout_if(
            self.extra != fresh.extra || !same_child(&self.child, &fresh.child),
        );
        self.extra = fresh.extra;
        self.child = fresh.child.take();
        Some(effect)
    }
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
            context.paint_child(child, offset);
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

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        if let Some(child) = &self.child {
            visit(child, Offset::ZERO);
        }
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
        RenderPadding { insets, child: RenderRef::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderPadding {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderPadding>()?;
        // Upstream's `Padding.updateRenderObject` and `RenderPadding.set
        // padding`, which is the pair this whole method stands for.
        let effect = UpdateEffect::relayout_if(
            self.insets != fresh.insets || !self.child.is(&fresh.child),
        );
        self.insets = fresh.insets;
        self.child = fresh.child.clone();
        Some(effect)
    }
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
            &self.child,
            offset.translate(self.insets.left, self.insets.top),
        );
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::new(self.insets.left, self.insets.top));
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
            child: RenderRef::new(child),
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
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderAlign>()?;
        // The alignment is resolved into `child_offset` during layout rather
        // than during paint, so it is a layout field here even though all it
        // does is move something. Upstream resolves it in `performLayout` too.
        let effect = UpdateEffect::relayout_if(
            self.alignment != fresh.alignment
                || self.width_factor != fresh.width_factor
                || self.height_factor != fresh.height_factor
                || !self.child.is(&fresh.child),
        );
        self.alignment = fresh.alignment;
        self.width_factor = fresh.width_factor;
        self.height_factor = fresh.height_factor;
        self.child = fresh.child.clone();
        Some(effect)
    }
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
            &self.child,
            offset.plus(self.child_offset),
        );
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, self.child_offset);
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
        FlexChild { render: RenderRef::new(render), flex: 0, tight: true }
    }

    pub fn expanded(render: impl RenderBox + 'static, flex: u32) -> FlexChild {
        FlexChild { render: RenderRef::new(render), flex: flex.max(1), tight: true }
    }

    pub fn flexible(render: impl RenderBox + 'static, flex: u32) -> FlexChild {
        FlexChild { render: RenderRef::new(render), flex: flex.max(1), tight: false }
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
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderFlex>()?;
        let kept_children = self.children.len() == fresh.children.len()
            && self
                .children
                .iter()
                .zip(&fresh.children)
                .all(|(a, b)| a.render.is(&b.render) && a.flex == b.flex && a.tight == b.tight);
        let effect = UpdateEffect::relayout_if(
            self.direction != fresh.direction
                || self.main_axis_alignment != fresh.main_axis_alignment
                || self.cross_axis_alignment != fresh.cross_axis_alignment
                || self.main_axis_size != fresh.main_axis_size
                || self.spacing != fresh.spacing
                || !kept_children,
        );
        self.direction = fresh.direction;
        self.main_axis_alignment = fresh.main_axis_alignment;
        self.cross_axis_alignment = fresh.cross_axis_alignment;
        self.main_axis_size = fresh.main_axis_size;
        self.spacing = fresh.spacing;
        self.children = std::mem::take(&mut fresh.children);
        // `offsets` is where the last layout put them, and the next layout
        // rebuilds it -- which the effect above has just asked for if anything
        // that decides an offset moved.
        Some(effect)
    }
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
            context.paint_child(&child.render, offset.plus(*placement));
        }
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        for (child, placement) in self.children.iter().zip(self.offsets.iter()) {
            visit(&child.render, *placement);
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
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
            render: RenderRef::new(child),
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
        self.children.push(StackChild { render: RenderRef::new(child), position });
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
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderStack>()?;
        let kept_children = self.children.len() == fresh.children.len()
            && self
                .children
                .iter()
                .zip(&fresh.children)
                .all(|(a, b)| a.render.is(&b.render) && a.position == b.position);
        let effect = UpdateEffect::relayout_if(self.alignment != fresh.alignment || !kept_children);
        self.alignment = fresh.alignment;
        self.children = std::mem::take(&mut fresh.children);
        Some(effect)
    }
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
            context.paint_child(&child.render, offset.plus(*placement));
        }
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        for (child, placement) in self.children.iter().zip(self.offsets.iter()) {
            visit(&child.render, *placement);
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
        RenderIgnorePointer { child: RenderRef::new(child), size: Size::ZERO }
    }

    pub fn boxed(child: BoxedRender) -> RenderIgnorePointer {
        RenderIgnorePointer { child, size: Size::ZERO }
    }
}

impl RenderBox for RenderIgnorePointer {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderIgnorePointer>()?;
        let effect = UpdateEffect::relayout_if(!self.child.is(&fresh.child));
        self.child = fresh.child.clone();
        Some(effect)
    }
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        // Invisible to the pointer, not to a reader. Upstream draws the same
        // line -- `RenderIgnorePointer` blocks user actions and leaves the
        // description alone, and hiding a subtree from a screen reader is a
        // different widget (`ExcludeSemantics`).
        visit(&self.child, Offset::ZERO);
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
        RenderSizeReporter { sink, child: RenderRef::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderSizeReporter {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderSizeReporter>()?;
        // A different cell has not been told anything yet, and it is `layout`
        // that tells it -- so a new sink is a reason to lay out again even
        // though nothing about the geometry changed.
        let effect = UpdateEffect::relayout_if(
            !Rc::ptr_eq(&self.sink, &fresh.sink) || !self.child.is(&fresh.child),
        );
        self.sink = Rc::clone(&fresh.sink);
        self.child = fresh.child.clone();
        Some(effect)
    }
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.sink.set(self.size);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
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
        self.children.push(RenderRef::new(child));
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
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderWrap>()?;
        let effect = UpdateEffect::relayout_if(
            self.direction != fresh.direction
                || self.spacing != fresh.spacing
                || self.run_spacing != fresh.run_spacing
                || self.alignment != fresh.alignment
                || self.cross_alignment != fresh.cross_alignment
                || !same_children(&self.children, &fresh.children),
        );
        self.direction = fresh.direction;
        self.spacing = fresh.spacing;
        self.run_spacing = fresh.run_spacing;
        self.alignment = fresh.alignment;
        self.cross_alignment = fresh.cross_alignment;
        self.children = std::mem::take(&mut fresh.children);
        Some(effect)
    }
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
            context.paint_child(child, offset.plus(*child_offset));
        }
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        for (child, child_offset) in self.children.iter().zip(&self.offsets) {
            visit(child, *child_offset);
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
        RenderAspectRatio { ratio, child: RenderRef::new(child), size: Size::ZERO }
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
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderAspectRatio>()?;
        let effect =
            UpdateEffect::relayout_if(self.ratio != fresh.ratio || !self.child.is(&fresh.child));
        self.ratio = fresh.ratio;
        self.child = fresh.child.clone();
        Some(effect)
    }
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.applied(constraints);
        self.child.layout(BoxConstraints::tight(self.size.width, self.size.height));
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
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
        RenderIntrinsicWidth { child: RenderRef::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderIntrinsicWidth {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderIntrinsicWidth>()?;
        let effect = UpdateEffect::relayout_if(!self.child.is(&fresh.child));
        self.child = fresh.child.clone();
        Some(effect)
    }
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
        context.paint_child(&self.child, offset);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
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
        RenderIntrinsicHeight { child: RenderRef::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderIntrinsicHeight {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderIntrinsicHeight>()?;
        let effect = UpdateEffect::relayout_if(!self.child.is(&fresh.child));
        self.child = fresh.child.clone();
        Some(effect)
    }
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
        context.paint_child(&self.child, offset);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
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
            child: RenderRef::new(child),
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
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderTransform>()?;
        // A transform does not change what the child was measured at -- it is
        // applied on the way to the canvas. Upstream's `set transform` calls
        // `markNeedsPaint` for the same reason.
        let mut effect =
            UpdateEffect::repaint_if(self.matrix != fresh.matrix || self.origin != fresh.origin);
        self.matrix = fresh.matrix;
        self.origin = fresh.origin;
        effect = effect.and(UpdateEffect::relayout_if(!self.child.is(&fresh.child)));
        self.child = fresh.child.clone();
        Some(effect)
    }
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let pivot = self.origin.inscribe(Size::ZERO, self.size);
        context.push_transform(self.matrix, pivot, offset, &self.child);
    }

    /// The child where it would be without the transform.
    ///
    /// Upstream applies the matrix here, because upstream's is a `Matrix4` and
    /// a semantics node carries one. This carries an offset, so a rotation has
    /// no expression -- and reporting the untransformed rectangle is not a
    /// worse guess, it is the *agreeing* one: the rectangle exists so a finger
    /// dragged across the glass can find the node, and what the finger finds
    /// comes from `hit_test`, which tests against the untransformed geometry
    /// for the same reason. Two answers to "where is this" that disagreed
    /// would be worse than one that is approximate.
    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
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
        RenderOpacity { opacity: opacity.clamp(0.0, 1.0), child: RenderRef::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderOpacity {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderOpacity>()?;
        let mut effect = UpdateEffect::repaint_if(self.opacity != fresh.opacity);
        self.opacity = fresh.opacity;
        effect = effect.and(UpdateEffect::relayout_if(!self.child.is(&fresh.child)));
        self.child = fresh.child.clone();
        Some(effect)
    }
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
            context.paint_child(&self.child, offset);
            return;
        }
        // 0..255, the alpha an OpacityLayer carries.
        let alpha = (self.opacity * 255.0).round().clamp(0.0, 255.0) as u8;
        context.push_opacity(alpha, offset, &self.child);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
    }

    /// Nothing at all is drawn at zero, so there is nothing to describe.
    /// Upstream's `RenderOpacity.visitChildrenForSemantics` skips for the same
    /// reason, and `hit_test` here already refuses for it.
    fn visit_children_for_semantics(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        if self.opacity > 0.0 {
            visit(&self.child, Offset::ZERO);
        }
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
        RenderClipRect { corner_radius: 0.0, child: RenderRef::new(child), size: Size::ZERO }
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }
}

impl RenderBox for RenderClipRect {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderClipRect>()?;
        let mut effect = UpdateEffect::repaint_if(self.corner_radius != fresh.corner_radius);
        self.corner_radius = fresh.corner_radius;
        effect = effect.and(UpdateEffect::relayout_if(!self.child.is(&fresh.child)));
        self.child = fresh.child.clone();
        Some(effect)
    }
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
            &self.child,
            offset,
        );
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
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
        RenderClipPath { path, child: RenderRef::new(child), size: Size::ZERO }
    }
}

impl RenderBox for RenderClipPath {
    // No `update_from`. The path is the configuration, and two paths cannot be
    // told apart without walking both -- which is the work the comparison
    // exists to save. A rebuilt clip path makes a new render object, as
    // everything did before any of this.
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
            &self.child,
            offset,
        );
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
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
            child: RenderRef::new(child),
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
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderViewport>()?;
        // The offset is read by `layout`, which also re-clamps it against
        // content that may have shrunk -- so a scroll is a relayout. It is
        // upstream too: `RenderViewport` listens to its `ViewportOffset` and
        // answers with `markNeedsLayout`.
        let effect = UpdateEffect::relayout_if(
            self.axis != fresh.axis
                || self.offset != fresh.offset
                || !self.child.is(&fresh.child),
        );
        self.axis = fresh.axis;
        self.offset = fresh.offset;
        self.child = fresh.child.clone();
        Some(effect)
    }
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
            &self.child,
            offset.plus(self.scroll_offset()),
        );
    }

    /// The scrolled column, moved by however far it is scrolled.
    ///
    /// Everything outside the viewport is reported too, with a rectangle that
    /// falls outside it. Upstream trims those against the clip in
    /// `_SemanticsGeometry` and marks what is left over as hidden; the same
    /// rows were reported before this walk existed, because the clip is a layer
    /// and painting into it still painted them.
    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, self.scroll_offset());
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
            child: RenderRef::new(child),
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
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<RenderPointerRegion>()?;
        // Neither the id nor the handlers are measured or drawn: they are read
        // when a finger arrives, out of whatever this object holds then.
        // Upstream's gesture setters mark nothing about the frame either.
        self.id = fresh.id;
        self.handlers = Rc::clone(&fresh.handlers);
        let effect = UpdateEffect::relayout_if(!self.child.is(&fresh.child));
        self.child = fresh.child.clone();
        Some(effect)
    }
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset);
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
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

// -- Taking a new configuration -----------------------------------------------
//
// Upstream tests this by asserting that a rebuilt widget did not produce a
// second render object, which it can do because a Dart object has an identity
// anyone can compare. Here the same question is asked of the handle, and the
// answers a type gives -- nothing, repaint, relayout -- are asserted directly,
// because they are the part that can be wrong quietly.

#[cfg(test)]
mod reconfiguring_tests {
    use super::*;

    thread_local! {
        static LAYOUTS: Cell<usize> = const { Cell::new(0) };
    }

    /// A leaf that says how many times it has been measured.
    struct Counted(f32);

    impl RenderBox for Counted {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            LAYOUTS.with(|n| n.set(n.get() + 1));
            constraints.constrain(Size::square(self.0))
        }
        fn size(&self) -> Size {
            Size::square(self.0)
        }
        fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
        fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
            let fresh = fresh.as_any_mut().downcast_mut::<Counted>()?;
            let effect = UpdateEffect::relayout_if(self.0 != fresh.0);
            self.0 = fresh.0;
            Some(effect)
        }
    }

    fn layouts() -> usize {
        LAYOUTS.with(|n| n.get())
    }

    fn reset() {
        LAYOUTS.with(|n| n.set(0));
    }

    /// Reaches the object inside a handle.
    ///
    /// Deliberately through `&**cell` rather than through the `Ref`: a `Box`
    /// is itself `Any`, so asking the box would answer about the box.
    fn with_paragraph<T>(handle: &RenderRef, read: impl FnOnce(&RenderParagraph) -> T) -> T {
        let cell = handle.render.borrow();
        let object: &dyn RenderBox = &**cell;
        read(object.as_any().downcast_ref::<RenderParagraph>().expect("a paragraph"))
    }

    #[test]
    fn a_handle_is_never_wrapped_in_a_second_handle() {
        // The combinators hand a child's handle to a constructor that wraps
        // whatever it is given, so without this every parent would be holding a
        // brand new outer handle every frame -- and the identity the whole
        // scheme rests on would be invisible one level down.
        let inner = RenderRef::new(Counted(10.0));
        let again = RenderRef::new(inner.clone());
        assert!(inner.is(&again), "wrapping a handle made a second object");
    }

    #[test]
    fn a_padding_that_did_not_change_asks_for_nothing() {
        reset();
        let child = RenderRef::new(Counted(10.0));
        let padded = RenderRef::new(RenderPadding::new(EdgeInsets::all(4.0), child.clone()));
        let mut root = padded.clone();
        root.layout(BoxConstraints::loose(100.0, 100.0));
        assert_eq!(layouts(), 1);

        // What a rebuild produces when nothing about it moved.
        let same = RenderRef::new(RenderPadding::new(EdgeInsets::all(4.0), child.clone()));
        assert!(padded.reconfigure(same), "a padding would not take a padding");
        root.layout(BoxConstraints::loose(100.0, 100.0));
        assert_eq!(layouts(), 1, "nothing changed and it was measured again");
    }

    #[test]
    fn a_padding_that_did_change_asks_to_be_measured_again() {
        // The half that matters more: a skip that skips too much shows a stale
        // interface, and no amount of saved measuring is worth that.
        reset();
        let child = RenderRef::new(Counted(10.0));
        let padded = RenderRef::new(RenderPadding::new(EdgeInsets::all(4.0), child.clone()));
        let mut root = padded.clone();
        root.layout(BoxConstraints::loose(100.0, 100.0));

        let wider = RenderRef::new(RenderPadding::new(EdgeInsets::all(12.0), child.clone()));
        assert!(padded.reconfigure(wider));
        root.layout(BoxConstraints::loose(100.0, 100.0));
        assert_eq!(layouts(), 2, "the child sits somewhere else now");
        assert_eq!(root.size(), Size::new(34.0, 34.0), "and the padding is the new one");
    }

    #[test]
    fn a_colour_is_not_a_reason_to_measure_anything() {
        // Upstream's `RenderDecoratedBox.set decoration` calls `markNeedsPaint`
        // and stops there. Nothing about a fill is read while measuring, and a
        // screen that re-measures itself to change a swatch is doing the whole
        // frame for a rectangle.
        reset();
        let child = RenderRef::new(Counted(10.0));
        let decorated = RenderRef::new(
            RenderDecoratedBox::new().with_color(Color(0xFF00FF00)).with_child(child.clone()),
        );
        let mut root = decorated.clone();
        root.layout(BoxConstraints::loose(100.0, 100.0));
        assert_eq!(layouts(), 1);

        let repainted = RenderRef::new(
            RenderDecoratedBox::new().with_color(Color(0xFFFF0000)).with_child(child.clone()),
        );
        assert!(decorated.reconfigure(repainted));
        root.layout(BoxConstraints::loose(100.0, 100.0));
        assert_eq!(layouts(), 1, "a new colour was measured");
    }

    #[test]
    fn a_handler_that_changed_costs_nothing_at_all() {
        // A callback is neither measured nor drawn: it is read when a finger
        // arrives, out of whatever the object holds then. Upstream's gesture
        // setters mark nothing about the frame either.
        reset();
        let child = RenderRef::new(Counted(10.0));
        let region = RenderRef::new(RenderPointerRegion::new(7, child.clone()));
        let mut root = region.clone();
        root.layout(BoxConstraints::loose(100.0, 100.0));
        assert_eq!(layouts(), 1);

        let rebuilt = RenderRef::new(
            RenderPointerRegion::new(7, child.clone())
                .with_handlers(crate::gestures::PointerHandlers::default()),
        );
        assert!(region.reconfigure(rebuilt));
        root.layout(BoxConstraints::loose(100.0, 100.0));
        assert_eq!(layouts(), 1, "a new closure was measured");
    }

    #[test]
    fn text_that_says_the_same_thing_is_not_shaped_again() {
        // Shaping is nearly all of what a paragraph costs, and a list of a
        // hundred rows rebuilt for one of them would otherwise shape all
        // hundred. The shaped paragraph is kept on the render object, so
        // keeping the render object is what keeps it.
        let text = RenderRef::new(RenderParagraph::new("Hello"));
        let mut root = text.clone();
        root.layout(BoxConstraints::loose(200.0, 200.0));
        let shaped = with_paragraph(&text, |p| p.paragraph.clone()).expect("laying out shapes it");

        assert!(text.reconfigure(RenderRef::new(RenderParagraph::new("Hello"))));
        let after = with_paragraph(&text, |p| p.paragraph.clone()).expect("still shaped");
        assert!(Rc::ptr_eq(&shaped, &after), "the same words were shaped twice");

        // And different words are not the same words.
        assert!(text.reconfigure(RenderRef::new(RenderParagraph::new("Goodbye"))));
        assert!(
            with_paragraph(&text, |p| p.paragraph.is_none()),
            "it kept a shaping of the old text"
        );
        assert_eq!(with_paragraph(&text, |p| p.content.clone()), "Goodbye");
    }

    #[test]
    fn a_type_that_will_not_take_one_says_so() {
        // `None` is always a safe answer -- it is what every type said before
        // any of them said anything else -- and a clip path says it, because
        // two paths cannot be compared without walking both.
        let child = RenderRef::new(Counted(10.0));
        let clipped = RenderRef::new(RenderClipPath::new(
            crate::painting::RenderPath::new(),
            child.clone(),
        ));
        let other = RenderRef::new(RenderClipPath::new(
            crate::painting::RenderPath::new(),
            child.clone(),
        ));
        assert!(!clipped.reconfigure(other), "a clip path claimed it could compare paths");
    }

    #[test]
    fn a_boundary_over_a_new_layout_draws_it_again() {
        use crate::engine::LayerTree;
        use crate::engine_test_stubs::{layer_calls, reset_layer_calls};

        let child = RenderRef::new(Counted(10.0));
        let padding = RenderRef::new(RenderPadding::new(EdgeInsets::all(4.0), child.clone()));
        let mut boundary = RenderRef::new(RenderRepaintBoundary::new(padding.clone()));

        let frame = |root: &mut RenderRef| {
            root.layout(BoxConstraints::loose(200.0, 200.0));
            reset_layer_calls();
            let mut layers = LayerTree::new(200, 200);
            {
                let mut context = PaintContext::new(&mut layers, Size::new(200.0, 200.0));
                root.paint(&mut context, Offset::ZERO);
            }
            layer_calls()
        };

        let first = frame(&mut boundary);
        assert_eq!((first.retainable, first.retained), (1, 0), "the first frame has to draw");
        assert_eq!(boundary.size(), Size::square(18.0));

        let quiet = frame(&mut boundary);
        assert_eq!((quiet.retainable, quiet.retained), (0, 1), "nothing changed and it drew");

        // A wider padding is a relayout and not a repaint -- and the layer the
        // boundary kept is a drawing of the layout that is now gone.
        assert!(padding
            .reconfigure(RenderRef::new(RenderPadding::new(EdgeInsets::all(8.0), child.clone()))));
        let second = frame(&mut boundary);
        assert_eq!(boundary.size(), Size::square(26.0));
        assert_eq!(
            (second.retainable, second.retained),
            (1, 0),
            "the boundary handed back a drawing of the layout it used to have"
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
    fn a_repaint_boundary_records_once_and_then_hands_the_same_layer_back() {
        // The whole point of a render object that outlives its frame, for
        // painting: the drawing of a subtree that was not rebuilt is the
        // drawing it was. Upstream keeps it as `RenderObject.layer` and
        // `PaintingContext` re-adds it; the same layer comes back here.
        let mut boundary = RenderRepaintBoundary::new(Spot);
        boundary.layout(BoxConstraints::tight(50.0, 50.0));

        let first = paint_into(&mut boundary);
        assert_eq!(first.retainable, 1, "the first frame has to record it");
        assert_eq!(first.retained, 0);
        assert_eq!(first.display_lists, 1, "and the recording is a picture");

        let second = paint_into(&mut boundary);
        assert_eq!(second.retainable, 0, "it was recorded a second time");
        assert_eq!(second.retained, 1, "rather than handed back");
        assert_eq!(second.display_lists, 0, "nothing was drawn at all");
    }

    #[test]
    fn a_boundary_that_moved_is_not_recorded_again() {
        // What a scrolling list does to every row it keeps. The layer holds
        // the drawing and not the position, so moving it costs the transform
        // it is added under.
        let mut boundary = RenderRepaintBoundary::new(Spot);
        boundary.layout(BoxConstraints::tight(50.0, 50.0));

        reset_layer_calls();
        let mut tree = LayerTree::new(100, 100);
        {
            let mut context = PaintContext::new(&mut tree, Size::new(100.0, 100.0));
            boundary.paint(&mut context, Offset::new(0.0, 0.0));
            boundary.paint(&mut context, Offset::new(0.0, 40.0));
        }
        let calls = layer_calls();
        assert_eq!(calls.retainable, 1, "recorded once");
        assert_eq!(calls.retained, 1, "and put down again somewhere else");
    }

    #[test]
    fn a_boundary_asked_a_new_question_draws_again() {
        // A window that resized. The layer holds what was drawn for the old
        // size, and a subtree that changed size did not draw the same thing.
        let mut boundary = RenderRepaintBoundary::new(Spot);
        boundary.layout(BoxConstraints::tight(50.0, 50.0));
        let _ = paint_into(&mut boundary);

        boundary.layout(BoxConstraints::tight(80.0, 50.0));
        let after = paint_into(&mut boundary);
        assert_eq!(after.retained, 0, "a stale layer was handed back");
        assert_eq!(after.retainable, 1, "it should have been recorded again");
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
