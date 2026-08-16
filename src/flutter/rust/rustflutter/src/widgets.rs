// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The widgets an application writes against.
//!
//! Everything here is a thin, named front for something in [`crate::render`]:
//! `Center` is a `RenderAlign`, `Row` and `Column` are both a `RenderFlex`,
//! `ListView` is a `RenderViewport` over one. The split exists because the
//! render layer is written for the layout algorithm and this one is written for
//! the reader.
//!
//! Upstream the same names sit at a further remove -- a `Widget` is an immutable
//! description, an `Element` holds the state, and only the `RenderObject` does
//! the work. That split is here too, in [`crate::framework`]; what this file
//! holds is the third layer. A render object here outlives the frame that built
//! it, which is why the two types that assemble a subtree of their own --
//! [`Container`] and [`ListView`] -- have to reconcile that subtree themselves,
//! the job the element tree does for everything else.

use crate::engine::{Color, TextAlign, TextStyle};
pub use crate::render::{
    Alignment, Axis, BoxConstraints, BoxFit, BoxedRender, Constraints, CrossAxisAlignment,
    EdgeInsets, Fill, FlexChild, HitTestEntry, HitTestResult, MainAxisAlignment, MainAxisSize,
    Offset, PaintContext, RenderBox as Widget, Size, StackPosition,
};
use crate::painting::{Gradient, Image, RenderPath};
use crate::render::{
    RenderAlign, RenderAspectRatio, RenderBox, RenderClipPath, RenderClipRect,
    RenderConstrainedBox, RenderDecoratedBox, RenderFlex, RenderFullWidth, RenderImage,
    RenderIntrinsicHeight, RenderIntrinsicWidth, RenderOpacity, RenderPadding, RenderParagraph,
    RenderPointerRegion, RenderRef, RenderStack, RenderTransform, RenderViewport, RenderWrap,
    UpdateEffect,
};

/// A widget with its concrete type erased, which is what a `build` method
/// returns and what a parent stores for a child.
pub type BoxedWidget = BoxedRender;

/// Paints `child` into a layer of its own, and keeps it.
///
/// Upstream's `RepaintBoundary`. A frame in which nothing under it changed
/// hands the engine the layer it made last time instead of recording the same
/// drawing again -- so it is worth putting where a sibling changes often and
/// this does not. See [`crate::render::RenderRepaintBoundary`].
pub fn repaint_boundary(child: crate::framework::AnyWidget) -> crate::framework::AnyWidget {
    crate::framework::single(child, crate::render::RenderRepaintBoundary::new)
}

/// Wraps a render object as a [`BoxedWidget`].
pub fn boxed(render: impl crate::render::RenderBox + 'static) -> BoxedWidget {
    crate::render::RenderRef::new(render)
}

// -- Text ---------------------------------------------------------------------

/// A run of text, shaped by the engine's `txt` / skparagraph stack.
pub type Text = RenderParagraph;

/// One run of a rich paragraph: some text and the style it is set in.
///
/// Upstream's `TextSpan`, flattened. There a span may contain children and a
/// child inherits what its parent did not override; here the inheriting is
/// done by the caller, because by the time a run reaches the shaper the answer
/// is a single resolved style either way.
pub struct TextSpan {
    pub text: String,
    pub style: TextStyle,
}

impl TextSpan {
    pub fn new(text: impl Into<String>, style: TextStyle) -> TextSpan {
        TextSpan { text: text.into(), style }
    }

    /// The same text in a bolder weight, which is what a run inside a sentence
    /// usually differs by.
    pub fn bold(text: impl Into<String>, style: &TextStyle) -> TextSpan {
        TextSpan::new(text, TextStyle { font_weight: 700, ..style.clone() })
    }
}

impl RenderParagraph {
    /// A paragraph of differently styled runs, as one paragraph.
    ///
    /// ```ignore
    /// Text::rich(vec![
    ///     TextSpan::new("Hold ", body.clone()),
    ///     TextSpan::bold("Shift", &body),
    ///     TextSpan::new(" to select a range.", body),
    /// ])
    /// ```
    #[allow(clippy::should_implement_trait)]
    pub fn rich_spans(spans: Vec<TextSpan>) -> RenderParagraph {
        RenderParagraph::rich(spans.into_iter().map(|s| (s.text, s.style)).collect())
    }

    pub fn with_size(mut self, font_size: f32) -> Self {
        self.style_mut().font_size = font_size;
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.style_mut().color = color;
        self
    }

    /// CSS weight: 400 is normal, 700 is bold.
    pub fn with_weight(mut self, weight: i32) -> Self {
        self.style_mut().font_weight = weight;
        self
    }

    pub fn with_align(mut self, align: TextAlign) -> Self {
        self.style_mut().align = align;
        self
    }

    /// Shapes this text with a named font family instead of the system default.
    ///
    /// The family has to have been registered first, with
    /// [`crate::engine::register_font`]. This is how an icon is drawn: an icon
    /// is a glyph at a private-use codepoint, so an icon is a one-character
    /// string in the family that has that codepoint.
    pub fn with_font_family(mut self, family: impl Into<String>) -> Self {
        self.style_mut().font_family = Some(family.into());
        self
    }

    pub fn centered(self) -> Self {
        self.with_align(TextAlign::Center)
    }
}

// -- Container ----------------------------------------------------------------

/// One wrapper a container puts around its child.
///
/// The order is the order [`Container::compose`] applies them, innermost
/// first. Recording which ones a container actually built is what lets the
/// next configuration be handed to them instead of replacing them: two
/// containers line up only when they asked for the same wrappers in the same
/// order, which is upstream's rule that an element survives only where the
/// widget at its slot kept its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Align,
    Padding,
    Decoration,
    Sizing,
    Margin,
}

/// A box that paints a background, pads, sizes and aligns a child.
///
/// Upstream `Container` is famously a composition rather than a render object,
/// and it is the same here: whichever of padding, decoration, sizing and
/// alignment were asked for get layered, and the ones that were not cost
/// nothing.
///
/// The difference is who reconciles that composition. Upstream `Container` is a
/// `StatelessWidget` and the wrappers its `build` returns are widgets, so the
/// element tree updates them one slot at a time and never asks the container
/// about it. Here the wrappers are render objects this one made, so it has to
/// do that itself -- see [`Container::update_from`].
pub struct Container {
    fill: Option<Fill>,
    corner_radius: f32,
    border_width: f32,
    border_color: Color,
    shadows: Vec<crate::painting::BoxShadow>,
    padding: EdgeInsets,
    margin: EdgeInsets,
    width: Option<f32>,
    height: Option<f32>,
    alignment: Option<Alignment>,
    child: Option<BoxedWidget>,
    /// The wrappers the last [`Container::compose`] built, innermost first, so
    /// the next one can hand them their configuration rather than replace them.
    layers: Vec<(Layer, BoxedWidget)>,
    composed: Option<BoxedWidget>,
}

impl Container {
    pub fn new() -> Container {
        Container {
            fill: None,
            corner_radius: 0.0,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            shadows: Vec::new(),
            padding: EdgeInsets::ZERO,
            margin: EdgeInsets::ZERO,
            width: None,
            height: None,
            alignment: None,
            child: None,
            layers: Vec::new(),
            composed: None,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.fill = Some(Fill::Solid(color));
        self
    }

    pub fn with_gradient(mut self, start: Alignment, end: Alignment, gradient: Gradient) -> Self {
        self.fill = Some(Fill::Linear { start, end, gradient });
        self
    }

    pub fn with_fill(mut self, fill: Fill) -> Self {
        self.fill = Some(fill);
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

    /// Casts these shadows under the container's decoration.
    pub fn with_shadows(mut self, shadows: Vec<crate::painting::BoxShadow>) -> Self {
        self.shadows = shadows;
        self
    }

    /// How high off the surface this sits, as Material's shadows for that
    /// height. Zero is flat.
    pub fn with_elevation(self, elevation: u32) -> Self {
        self.with_shadows(crate::painting::elevation_shadows(elevation).to_vec())
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    /// Space outside the decoration, unlike padding which is inside it.
    pub fn with_margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = margin;
        self
    }

    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Where the child sits when the container is larger than it. Setting this
    /// also makes the container take all the space it is offered, the way
    /// upstream's does.
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    pub fn with_child(mut self, child: impl RenderBox + 'static) -> Self {
        self.child = Some(RenderRef::new(child));
        self
    }

    /// The wrappers this configuration asks for, innermost first.
    fn shape(&self) -> Vec<Layer> {
        let mut shape = Vec::new();
        if self.alignment.is_some() {
            shape.push(Layer::Align);
        }
        if self.padding != EdgeInsets::ZERO {
            shape.push(Layer::Padding);
        }
        if self.fill.is_some() || self.border_width > 0.0 || !self.shadows.is_empty() {
            shape.push(Layer::Decoration);
        }
        if self.width.is_some() || self.height.is_some() {
            shape.push(Layer::Sizing);
        }
        if self.margin != EdgeInsets::ZERO {
            shape.push(Layer::Margin);
        }
        shape
    }

    /// Builds one wrapper around `inner`, as this container is configured now.
    fn build_layer(&self, kind: Layer, inner: Option<BoxedWidget>) -> BoxedWidget {
        match kind {
            // Aligning and padding need something to align and pad, even when
            // that is nothing at all.
            Layer::Align => {
                let inner = inner.unwrap_or_else(|| RenderRef::new(Empty));
                let alignment = self.alignment.expect("the shape said there was one");
                RenderRef::new(RenderAlign::new(alignment, inner))
            }
            Layer::Padding => {
                let inner = inner.unwrap_or_else(|| RenderRef::new(Empty));
                RenderRef::new(RenderPadding::new(self.padding, inner))
            }
            Layer::Decoration => {
                let mut decorated = RenderDecoratedBox::new()
                    .with_corner_radius(self.corner_radius)
                    .with_shadows(self.shadows.clone())
                    .with_border(self.border_width, self.border_color);
                if let Some(fill) = self.fill.clone() {
                    decorated = decorated.with_fill(fill);
                }
                if let Some(inner) = inner {
                    decorated = decorated.with_child(inner);
                }
                RenderRef::new(decorated)
            }
            Layer::Sizing => {
                let extra = BoxConstraints::new(
                    self.width.unwrap_or(0.0),
                    self.width.unwrap_or(f32::INFINITY),
                    self.height.unwrap_or(0.0),
                    self.height.unwrap_or(f32::INFINITY),
                );
                let mut sized = RenderConstrainedBox::new(extra);
                if let Some(inner) = inner {
                    sized = sized.with_child(inner);
                }
                RenderRef::new(sized)
            }
            // Margin is padding on the outside of the decoration rather than
            // the inside, which is the only thing that makes it a second one.
            Layer::Margin => {
                let inner = inner.unwrap_or_else(|| RenderRef::new(Empty));
                RenderRef::new(RenderPadding::new(self.margin, inner))
            }
        }
    }

    /// Builds the render tree this container describes, innermost first.
    ///
    /// `onto` is the chain the last build left behind, in the same order, or
    /// `None` for a first build. Where it is given, each wrapper is handed its
    /// new configuration instead of being replaced and the handle that comes
    /// back out is the one that was already there -- which is the whole point,
    /// since a measured size and a kept layer are both held by identity.
    ///
    /// `None` comes back when a wrapper would not take what it was given. A
    /// first build has nothing to refuse it and always answers `Some`.
    fn compose(&mut self, onto: Option<&[BoxedWidget]>) -> Option<BoxedWidget> {
        let shape = self.shape();
        let mut layers = Vec::with_capacity(shape.len());
        let mut current: Option<BoxedWidget> = self.child.clone();
        for (step, kind) in shape.into_iter().enumerate() {
            let fresh = self.build_layer(kind, current.take());
            let handle = match onto {
                Some(onto) => {
                    let old = onto.get(step)?;
                    if !old.reconfigure(fresh) {
                        return None;
                    }
                    old.clone()
                }
                None => fresh,
            };
            layers.push((kind, handle.clone()));
            current = Some(handle);
        }
        self.layers = layers;
        Some(current.unwrap_or_else(|| RenderRef::new(Empty)))
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Container {
    /// Copies across everything the new container was configured with, which is
    /// what an upstream `updateRenderObject` does field by field.
    ///
    /// The child is not here. It is a subtree rather than a value, so it gets
    /// offered to the one already in place instead of overwriting it.
    fn take_configuration(&mut self, fresh: &Container) {
        self.fill = fresh.fill.clone();
        self.corner_radius = fresh.corner_radius;
        self.border_width = fresh.border_width;
        self.border_color = fresh.border_color;
        self.shadows = fresh.shadows.clone();
        self.padding = fresh.padding;
        self.margin = fresh.margin;
        self.width = fresh.width;
        self.height = fresh.height;
        self.alignment = fresh.alignment;
    }
}

impl RenderBox for Container {
    /// Upstream there is no `Container.updateRenderObject` to copy, because
    /// upstream `Container` is not a render object: it is a `StatelessWidget`
    /// whose `build` nests a handful of others, and the element tree reconciles
    /// that nesting one slot at a time. This does the same job in one place,
    /// because here the nesting is render objects and there is no element
    /// between them to do it.
    ///
    /// The two rules are upstream's. A slot whose widget kept its type keeps
    /// its element, so a wrapper this container asks for again is told rather
    /// than replaced. A slot whose widget changed type gets a new element, so a
    /// container asking for a different set of wrappers declines and the caller
    /// makes a new one -- which is what returning `None` means.
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<Container>()?;
        // Nothing composed yet means nothing to keep, and nothing that could go
        // stale either: the first layout builds the tree out of what is taken
        // here.
        if self.composed.is_none() {
            self.take_configuration(fresh);
            self.child = fresh.child.take();
            return Some(UpdateEffect::Relayout);
        }
        // Both of the checks that can refuse come before anything is taken, so
        // that declining leaves this container exactly as it was.
        if fresh.shape() != self.layers.iter().map(|(kind, _)| *kind).collect::<Vec<_>>() {
            return None;
        }
        // The child is the one part of this tree that came from outside, so it
        // is the one part that can refuse. Offer it the new one the way an
        // element offers the new widget at its slot, and keep the object that
        // was there when it takes.
        let child = match (self.child.clone(), fresh.child.take()) {
            (Some(old), Some(new)) => {
                if !old.reconfigure(new) {
                    return None;
                }
                Some(old)
            }
            (None, None) => None,
            // A child that appeared or vanished changes the tree rather than
            // its configuration, and there is no wrapper to hand that to.
            _ => return None,
        };
        self.take_configuration(fresh);
        self.child = child;
        if self.layers.is_empty() {
            // Wrapping nothing, this container *is* its child -- or an `Empty`
            // when it has none, and an `Empty` has nothing to be told.
            if self.child.is_some() {
                self.composed = self.child.clone();
            }
            return Some(UpdateEffect::Nothing);
        }
        let onto: Vec<BoxedWidget> = self.layers.iter().map(|(_, handle)| handle.clone()).collect();
        // The shape matched, so every handle in `onto` is the type the wrapper
        // at that step builds and none of them can refuse.
        let root = self.compose(Some(&onto))?;
        self.composed = Some(root);
        // Whatever changed has already marked itself, and those marks walk all
        // the way to the root -- so by the time they are done this container is
        // marked too and has nothing of its own left to ask for.
        Some(UpdateEffect::Nothing)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        if self.composed.is_none() {
            // A first build has nothing to line up against, so it cannot refuse.
            self.composed = self.compose(None);
        }
        self.composed.as_mut().unwrap().layout(constraints)
    }

    fn size(&self) -> Size {
        self.composed.as_ref().map_or(Size::ZERO, |c| c.size())
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        if let Some(composed) = &self.composed {
            composed.paint(context, offset);
        }
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.composed
            .as_ref()
            .is_some_and(|c| c.hit_test(position, result))
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.composed.as_ref().map_or(0.0, |c| c.min_intrinsic_width(height))
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.composed.as_ref().map_or(0.0, |c| c.max_intrinsic_width(height))
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.composed.as_ref().map_or(0.0, |c| c.min_intrinsic_height(width))
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.composed.as_ref().map_or(0.0, |c| c.max_intrinsic_height(width))
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.composed.as_ref().and_then(|c| c.distance_to_baseline())
    }
}

/// Takes no space and paints nothing. What a `Container` with no child
/// collapses to, and a useful placeholder in a conditional tree.
pub struct Empty;

impl RenderBox for Empty {
    /// Nothing to describe, so nothing can have changed. The one render object
    /// whose answer is a foregone conclusion.
    fn update_from(
        &mut self,
        fresh: &mut dyn RenderBox,
    ) -> Option<crate::render::UpdateEffect> {
        fresh.as_any_mut().downcast_mut::<Empty>()?;
        Some(crate::render::UpdateEffect::Nothing)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        constraints.smallest()
    }
    fn size(&self) -> Size {
        Size::ZERO
    }
    fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
    fn hit_test(&self, _position: Offset, _result: &mut HitTestResult) -> bool {
        false
    }
}

// -- Positioning --------------------------------------------------------------

/// Fills its constraints and centres its child.
pub struct Center;

impl Center {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> RenderAlign {
        RenderAlign::new(Alignment::CENTER, child)
    }
}

/// Fills its constraints and places its child at `alignment`.
pub struct Align;

impl Align {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(alignment: Alignment, child: impl RenderBox + 'static) -> RenderAlign {
        RenderAlign::new(alignment, child)
    }
}

/// Insets its child.
pub struct Padding;

impl Padding {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(insets: EdgeInsets, child: impl RenderBox + 'static) -> RenderPadding {
        RenderPadding::new(insets, child)
    }

    pub fn all(value: f32, child: impl RenderBox + 'static) -> RenderPadding {
        RenderPadding::new(EdgeInsets::all(value), child)
    }
}

/// A box of a fixed size.
pub struct SizedBox;

impl SizedBox {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(width: f32, height: f32) -> RenderConstrainedBox {
        RenderConstrainedBox::tight(width, height)
    }

    /// Takes everything it is offered.
    pub fn expand() -> RenderConstrainedBox {
        RenderConstrainedBox::new(BoxConstraints::new(
            f32::INFINITY,
            f32::INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        ))
    }

    /// A gap of `height` in a column, or `width` in a row.
    pub fn height(height: f32) -> RenderConstrainedBox {
        RenderConstrainedBox::new(BoxConstraints::new(0.0, 0.0, height, height))
    }

    pub fn width(width: f32) -> RenderConstrainedBox {
        RenderConstrainedBox::new(BoxConstraints::new(width, width, 0.0, 0.0))
    }
}

// -- Layout -------------------------------------------------------------------

/// Lays children out top to bottom.
pub struct Column;

impl Column {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> RenderFlex {
        // Shrink-wrapping is the useful default for a column that is being
        // centred or padded; a column that should fill says so with
        // `with_main_axis_size(MainAxisSize::Max)`.
        RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
    }

    /// Fills the height it is offered.
    pub fn expanded() -> RenderFlex {
        RenderFlex::column().with_main_axis_size(MainAxisSize::Max)
    }
}

/// Lays children out left to right.
pub struct Row;

impl Row {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> RenderFlex {
        RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
    }

    pub fn expanded() -> RenderFlex {
        RenderFlex::row().with_main_axis_size(MainAxisSize::Max)
    }
}

/// Lays children out in lines, starting a new one when a line fills up.
///
/// What a `Row` cannot do: a row of unknown things overflows, and this wraps.
pub struct Wrap;

impl Wrap {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> RenderWrap {
        RenderWrap::horizontal()
    }

    pub fn vertical() -> RenderWrap {
        RenderWrap::new(Axis::Vertical)
    }
}

/// Draws its child and lets every pointer through it.
pub struct IgnorePointer;

impl IgnorePointer {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> crate::render::RenderIgnorePointer {
        crate::render::RenderIgnorePointer::new(child)
    }
}

/// Sizes itself to a width-over-height ratio.
pub struct AspectRatio;

impl AspectRatio {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(ratio: f32, child: impl RenderBox + 'static) -> RenderAspectRatio {
        RenderAspectRatio::new(ratio, child)
    }
}

/// Sizes its child to the width the child would like to be. Expensive; see
/// [`RenderIntrinsicWidth`].
pub struct IntrinsicWidth;

impl IntrinsicWidth {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> RenderIntrinsicWidth {
        RenderIntrinsicWidth::new(child)
    }
}

/// Sizes its child to the height the child would like to be.
pub struct IntrinsicHeight;

impl IntrinsicHeight {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> RenderIntrinsicHeight {
        RenderIntrinsicHeight::new(child)
    }
}

/// A child that takes a share of a row or column's free space.
pub struct Expanded;

impl Expanded {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> FlexChild {
        FlexChild::expanded(child, 1)
    }

    pub fn flex(child: impl RenderBox + 'static, flex: u32) -> FlexChild {
        FlexChild::expanded(child, flex)
    }
}

/// A child that may take up to its share, but no more than it wants.
pub struct Flexible;

impl Flexible {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> FlexChild {
        FlexChild::flexible(child, 1)
    }
}

/// Overlays children.
pub struct Stack;

impl Stack {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> RenderStack {
        RenderStack::new()
    }
}

/// Anchors a stacked child to one or more edges.
pub struct Positioned;

impl Positioned {
    pub fn at(left: f32, top: f32) -> StackPosition {
        StackPosition { left: Some(left), top: Some(top), ..Default::default() }
    }

    pub fn fill() -> StackPosition {
        StackPosition {
            left: Some(0.0),
            top: Some(0.0),
            right: Some(0.0),
            bottom: Some(0.0),
            ..Default::default()
        }
    }
}

// -- Scrolling ----------------------------------------------------------------

/// A scrollable column.
///
/// The list is laid out in full -- there is no viewport culling yet, so a
/// thousand rows means a thousand layouts. Lazy building is a sliver protocol
/// away and belongs with the widgets layer.
pub struct ListView {
    axis: Axis,
    offset: f32,
    spacing: f32,
    centred_item: Option<f32>,
    extent_sink: Option<std::rc::Rc<std::cell::Cell<f32>>>,
    children: Vec<BoxedWidget>,
    /// How much padding each end got so an item could sit in the middle. Not
    /// known until layout, because it depends on the constraints.
    inset: Option<f32>,
    /// The column inside the viewport, kept so a new set of children can be
    /// given to it rather than replace it.
    flex: Option<BoxedWidget>,
    composed: Option<RenderViewport>,
}

impl ListView {
    pub fn new() -> ListView {
        ListView {
            axis: Axis::Vertical,
            offset: 0.0,
            spacing: 0.0,
            centred_item: None,
            extent_sink: None,
            children: Vec::new(),
            inset: None,
            flex: None,
            composed: None,
        }
    }

    pub fn horizontal() -> ListView {
        ListView { axis: Axis::Horizontal, ..ListView::new() }
    }

    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// How far the content is scrolled. Clamped to the scrollable extent once
    /// the content has been measured.
    pub fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    /// Reports how far this list can scroll, once it has been laid out.
    ///
    /// A scroll offset has to be clamped to something, and that something is
    /// not known until the content has been measured -- which happens inside
    /// the tree, a frame after whoever holds the offset needs it. The cell is
    /// the way back out. Upstream solves the same problem with a
    /// `ScrollPosition` that the viewport attaches itself to at layout.
    pub fn with_extent_sink(mut self, sink: std::rc::Rc<std::cell::Cell<f32>>) -> Self {
        self.extent_sink = Some(sink);
        self
    }

    /// Pads both ends so that an item this big can sit in the middle.
    ///
    /// What a carousel wants: the first card centred rather than jammed against
    /// the leading edge, and the last one able to reach the middle too. The
    /// padding cannot be a constant, because it depends on how wide the list
    /// turns out to be -- which is why this is a request rather than a number.
    pub fn with_centred_item(mut self, extent: f32) -> Self {
        self.centred_item = Some(extent);
        self
    }

    pub fn push(mut self, child: impl RenderBox + 'static) -> Self {
        self.children.push(RenderRef::new(child));
        self
    }

    /// How far this list can still scroll. Zero until it has been laid out.
    pub fn max_scroll_extent(&self) -> f32 {
        self.composed.as_ref().map_or(0.0, |v| v.max_scroll_extent())
    }

    /// How much padding each end needs for an item to be able to sit in the
    /// middle, at these constraints.
    fn inset_for(&self, constraints: BoxConstraints) -> Option<f32> {
        self.centred_item.and_then(|extent| {
            let available = match self.axis {
                Axis::Horizontal => constraints.max_width,
                Axis::Vertical => constraints.max_height,
            };
            // Unbounded means there is no middle to sit in.
            (available.is_finite() && available > extent).then(|| (available - extent) / 2.0)
        })
    }

    /// Builds the column this list scrolls, children and end padding and all.
    ///
    /// The children are cloned rather than taken, so that a later build -- a
    /// resize that changes the end padding -- still has them.
    fn build_flex(&self) -> RenderFlex {
        let mut flex = RenderFlex::new(self.axis)
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(self.spacing);
        if let Some(inset) = self.inset {
            flex = flex.push(spacer(self.axis, inset));
        }
        for child in &self.children {
            flex = flex.push(child.clone());
        }
        if let Some(inset) = self.inset {
            flex = flex.push(spacer(self.axis, inset));
        }
        flex
    }
}

/// A fixed gap along one axis, zero across it.
fn spacer(axis: Axis, extent: f32) -> RenderConstrainedBox {
    match axis {
        Axis::Horizontal => RenderConstrainedBox::tight(extent, 0.0),
        Axis::Vertical => RenderConstrainedBox::tight(0.0, extent),
    }
}

impl Default for ListView {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderBox for ListView {
    /// Upstream a `ListView` is a widget as well, and the viewport and slivers
    /// under it are reconciled by the elements between them. Here the viewport
    /// and the column it scrolls are this object's own, so it hands them their
    /// new configuration itself -- the same job [`Container::update_from`] does
    /// for its wrappers.
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<ListView>()?;
        self.axis = fresh.axis;
        self.offset = fresh.offset;
        self.spacing = fresh.spacing;
        self.centred_item = fresh.centred_item;
        self.extent_sink = fresh.extent_sink.take();
        self.children = std::mem::take(&mut fresh.children);
        // Never laid out, so there is nothing to keep and no end padding to
        // keep it with: the first layout builds the tree out of what was just
        // taken.
        let Some(flex) = self.flex.clone() else {
            return Some(UpdateEffect::Relayout);
        };
        // The column takes the new children, and every child that came back the
        // same object is one it will not measure or draw again.
        if !flex.reconfigure(RenderRef::new(self.build_flex())) {
            return None;
        }
        // The viewport takes the axis and the scroll offset. It is not behind a
        // handle of its own -- this list is its handle -- so its answer is this
        // one's answer.
        let mut staged = RenderViewport::new(self.axis, flex).with_offset(self.offset);
        self.composed.as_mut().expect("built with the column")
            .update_from(&mut staged)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let inset = self.inset_for(constraints);
        if self.composed.is_none() {
            // Computed here rather than at construction because it depends on
            // the size being handed down, which the caller does not know.
            self.inset = inset;
            let flex = RenderRef::new(self.build_flex());
            self.flex = Some(flex.clone());
            self.composed = Some(RenderViewport::new(self.axis, flex).with_offset(self.offset));
        } else if self.inset != inset {
            // The space to centre an item in changed, so the padding at both
            // ends did. This is the only place the new constraints are known,
            // and the column keeps its identity across the change.
            self.inset = inset;
            let flex = self.flex.clone().expect("built with the viewport");
            flex.reconfigure(RenderRef::new(self.build_flex()));
        }
        let viewport = self.composed.as_mut().expect("built just above");
        let size = viewport.layout(constraints);
        if let Some(sink) = &self.extent_sink {
            sink.set(viewport.max_scroll_extent());
        }
        size
    }

    fn size(&self) -> Size {
        self.composed.as_ref().map_or(Size::ZERO, |v| v.size())
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        if let Some(composed) = &self.composed {
            composed.paint(context, offset);
        }
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.composed
            .as_ref()
            .is_some_and(|v| v.hit_test(position, result))
    }
}

// -- Effects ------------------------------------------------------------------

/// Draws its child at a uniform opacity.
pub struct Opacity;

impl Opacity {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(opacity: f32, child: impl RenderBox + 'static) -> RenderOpacity {
        RenderOpacity::new(opacity, child)
    }
}

/// Applies a 2D affine to its child without affecting layout.
pub struct Transform;

impl Transform {
    pub fn rotate(degrees: f32, child: impl RenderBox + 'static) -> RenderTransform {
        RenderTransform::rotate(degrees, child)
    }

    pub fn scale(factor: f32, child: impl RenderBox + 'static) -> RenderTransform {
        RenderTransform::scale(factor, factor, child)
    }

    pub fn matrix(matrix: [f32; 6], child: impl RenderBox + 'static) -> RenderTransform {
        RenderTransform::new(matrix, child)
    }
}

/// Clips its child to a rounded rectangle.
pub struct ClipRRect;

impl ClipRRect {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(radius: f32, child: impl RenderBox + 'static) -> RenderClipRect {
        RenderClipRect::new(child).with_corner_radius(radius)
    }
}

/// Clips its child to an arbitrary path.
pub struct ClipPath;

impl ClipPath {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(path: RenderPath, child: impl RenderBox + 'static) -> RenderClipPath {
        RenderClipPath::new(path, child)
    }
}

/// Draws a decoded image.
///
/// The image is shared rather than handed over: a render tree is rebuilt every
/// frame, so an image that were owned here would have to be decoded again for
/// every one of them. Decode once, keep the handle, clone the handle.
pub struct ImageView;

impl ImageView {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(image: std::rc::Rc<Image>) -> RenderImage {
        RenderImage::new(image)
    }

    pub fn with_fit(image: std::rc::Rc<Image>, fit: BoxFit) -> RenderImage {
        RenderImage::new(image).with_fit(fit)
    }
}

/// Takes the full width on offer, so that siblings in a column line up rather
/// than each shrinking to its own contents.
pub struct FullWidth;

impl FullWidth {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> RenderFullWidth {
        RenderFullWidth::new().with_child(child)
    }
}

/// Gives its subtree a hit-test identity, so a pointer that lands on it can be
/// traced back to whatever the application cares about.
pub struct Pointer;

impl Pointer {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: u64, child: impl RenderBox + 'static) -> RenderPointerRegion {
        RenderPointerRegion::new(id, child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedBox(Size, Size);

    impl FixedBox {
        fn new(width: f32, height: f32) -> FixedBox {
            FixedBox(Size::new(width, height), Size::ZERO)
        }
    }

    impl RenderBox for FixedBox {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            self.1 = constraints.constrain(self.0);
            self.1
        }
        fn size(&self) -> Size {
            self.1
        }
        fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
    }

    #[test]
    fn container_padding_grows_the_box() {
        let mut container = Container::new()
            .with_padding(EdgeInsets::all(8.0))
            .with_child(FixedBox::new(20.0, 10.0));
        let size = container.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(36.0, 26.0));
    }

    #[test]
    fn container_honours_an_explicit_size_over_its_child() {
        let mut container = Container::new()
            .with_size(80.0, 40.0)
            .with_child(FixedBox::new(20.0, 10.0));
        let size = container.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(80.0, 40.0));
    }

    #[test]
    fn container_with_no_child_takes_what_it_is_offered() {
        let mut container = Container::new().with_color(Color::WHITE);
        let size = container.layout(BoxConstraints::tight(50.0, 30.0));
        assert_eq!(size, Size::new(50.0, 30.0));
    }

    #[test]
    fn center_reports_full_size_and_centres_child() {
        let mut center = Center::new(FixedBox::new(40.0, 20.0));
        let size = center.layout(BoxConstraints::tight(100.0, 100.0));
        assert_eq!(size, Size::new(100.0, 100.0));
        assert_eq!(center.child_offset(), Offset::new(30.0, 40.0));
    }

    #[test]
    fn column_stacks_children_with_spacing() {
        let mut column = Column::new()
            .with_spacing(10.0)
            .push(FixedBox::new(30.0, 20.0))
            .push(FixedBox::new(50.0, 20.0));
        let size = column.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(50.0, 50.0));
        assert_eq!(column.child_offsets()[0], Offset::new(10.0, 0.0));
        assert_eq!(column.child_offsets()[1], Offset::new(0.0, 30.0));
    }

    #[test]
    fn sized_box_gaps_take_only_one_axis() {
        let mut gap = SizedBox::height(12.0);
        let size = gap.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(0.0, 12.0));
    }

    #[test]
    fn a_horizontal_list_keeps_a_leading_spacer_at_its_width() {
        // A fixed-width child at the head of a sideways list is how a carousel
        // gets its first card off the left edge.
        let mut list = ListView::horizontal()
            .push(Container::new().with_size(28.0, 1.0))
            .push(Container::new().with_size(100.0, 40.0))
            .push(Container::new().with_size(100.0, 40.0));
        let size = list.layout(BoxConstraints::tight(200.0, 60.0));
        assert_eq!(size.width, 200.0);
        assert_eq!(list.max_scroll_extent(), 28.0 + 200.0 - 200.0);
    }

    #[test]
    fn list_view_scrolls_and_reports_its_extent() {
        let mut list = ListView::new()
            .push(FixedBox::new(100.0, 200.0))
            .push(FixedBox::new(100.0, 200.0))
            .push(FixedBox::new(100.0, 200.0));
        let size = list.layout(BoxConstraints::tight(100.0, 150.0));
        assert_eq!(size, Size::new(100.0, 150.0));
        // 600 of content in a 150 window.
        assert_eq!(list.max_scroll_extent(), 450.0);
    }

    #[test]
    fn constraints_clamp_desired_size() {
        let c = BoxConstraints::loose(100.0, 50.0);
        assert_eq!(c.constrain(Size::new(200.0, 10.0)), Size::new(100.0, 10.0));
    }

    // -- The two that assemble their own subtrees -----------------------------

    /// A child that says how many times it has been measured, and takes a new
    /// configuration the way a real render object does.
    struct Counted {
        extent: f32,
        laid_out: std::rc::Rc<std::cell::Cell<u32>>,
        size: Size,
    }

    impl Counted {
        fn new(extent: f32, laid_out: &std::rc::Rc<std::cell::Cell<u32>>) -> Counted {
            Counted { extent, laid_out: std::rc::Rc::clone(laid_out), size: Size::ZERO }
        }
    }

    impl RenderBox for Counted {
        fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
            let fresh = fresh.as_any_mut().downcast_mut::<Counted>()?;
            let effect = UpdateEffect::relayout_if(self.extent != fresh.extent);
            self.extent = fresh.extent;
            self.laid_out = std::rc::Rc::clone(&fresh.laid_out);
            Some(effect)
        }
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            self.laid_out.set(self.laid_out.get() + 1);
            self.size = constraints.constrain(Size::square(self.extent));
            self.size
        }
        fn size(&self) -> Size {
            self.size
        }
        fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
    }

    fn counter() -> std::rc::Rc<std::cell::Cell<u32>> {
        std::rc::Rc::new(std::cell::Cell::new(0))
    }

    #[test]
    fn a_container_that_did_not_change_keeps_its_wrappers() {
        let laid_out = counter();
        let describe = || {
            Container::new()
                .with_padding(EdgeInsets::all(8.0))
                .with_color(Color::WHITE)
                .with_child(Counted::new(20.0, &laid_out))
        };
        let mut container = describe();
        assert_eq!(container.layout(BoxConstraints::loose(200.0, 200.0)), Size::square(36.0));
        assert_eq!(laid_out.get(), 1);

        let before: Vec<BoxedWidget> = container.layers.iter().map(|(_, h)| h.clone()).collect();
        assert_eq!(container.update_from(&mut describe()), Some(UpdateEffect::Nothing));

        let after: Vec<BoxedWidget> = container.layers.iter().map(|(_, h)| h.clone()).collect();
        assert_eq!(before.len(), 2, "a padded, decorated container is two wrappers");
        assert!(
            before.iter().zip(&after).all(|(a, b)| a.is(b)),
            "the wrappers were replaced instead of told"
        );
        assert_eq!(container.layout(BoxConstraints::loose(200.0, 200.0)), Size::square(36.0));
        assert_eq!(laid_out.get(), 1, "a container that did not change measured again");
    }

    #[test]
    fn a_container_whose_padding_changed_still_shows_it() {
        // The half worth being afraid of: keeping the wrappers and not telling
        // them would show the old layout forever, and no test of identity would
        // notice.
        let laid_out = counter();
        let mut container = Container::new()
            .with_padding(EdgeInsets::all(8.0))
            .with_child(Counted::new(20.0, &laid_out));
        assert_eq!(container.layout(BoxConstraints::loose(200.0, 200.0)), Size::square(36.0));

        let mut fresh = Container::new()
            .with_padding(EdgeInsets::all(4.0))
            .with_child(Counted::new(20.0, &laid_out));
        assert_eq!(container.update_from(&mut fresh), Some(UpdateEffect::Nothing));
        assert_eq!(
            container.layout(BoxConstraints::loose(200.0, 200.0)),
            Size::square(28.0),
            "the wrapper was kept and the new padding was not"
        );
    }

    #[test]
    fn a_container_that_changed_tells_the_tree_above_it() {
        // A container answers `Nothing` and leans on its wrappers having marked
        // themselves. That is only true if the mark walks out of the container
        // and up, which is what the parent chain a layout records is for.
        let laid_out = counter();
        let container = RenderRef::new(
            Container::new()
                .with_padding(EdgeInsets::all(8.0))
                .with_child(Counted::new(20.0, &laid_out)),
        );
        let mut root = RenderRef::new(RenderPadding::new(EdgeInsets::all(2.0), container.clone()));
        assert_eq!(root.layout(BoxConstraints::loose(200.0, 200.0)), Size::square(40.0));

        let fresh = RenderRef::new(
            Container::new()
                .with_padding(EdgeInsets::all(4.0))
                .with_child(Counted::new(20.0, &laid_out)),
        );
        assert!(container.reconfigure(fresh));
        assert_eq!(
            root.layout(BoxConstraints::loose(200.0, 200.0)),
            Size::square(32.0),
            "the change stopped inside the container"
        );
    }

    #[test]
    fn a_container_that_wants_a_different_wrapper_will_not_take_it() {
        // Upstream a slot whose widget changed type gets a new element. The
        // same answer here is to decline, and the caller makes a new container.
        let laid_out = counter();
        let mut container = Container::new()
            .with_padding(EdgeInsets::all(8.0))
            .with_child(Counted::new(20.0, &laid_out));
        container.layout(BoxConstraints::loose(200.0, 200.0));

        let mut fresh = Container::new()
            .with_padding(EdgeInsets::all(8.0))
            .with_color(Color::WHITE)
            .with_child(Counted::new(20.0, &laid_out));
        assert_eq!(container.update_from(&mut fresh), None);
    }

    #[test]
    fn a_container_whose_child_will_not_take_it_declines_too() {
        // The child came from outside and can be any type at all, including one
        // that answers `None`. Keeping the wrappers around a child that had to
        // be replaced would leave them wrapping the old one.
        let mut container = Container::new()
            .with_padding(EdgeInsets::all(8.0))
            .with_child(FixedBox::new(20.0, 20.0));
        assert_eq!(container.layout(BoxConstraints::loose(200.0, 200.0)), Size::square(36.0));

        let mut fresh = Container::new()
            .with_padding(EdgeInsets::all(4.0))
            .with_child(FixedBox::new(20.0, 20.0));
        assert_eq!(container.update_from(&mut fresh), None);
        assert_eq!(
            container.layout(BoxConstraints::loose(200.0, 200.0)),
            Size::square(36.0),
            "declining left the container half updated"
        );
    }

    #[test]
    fn a_list_that_kept_its_rows_keeps_its_column() {
        let first = counter();
        let second = counter();
        let a = RenderRef::new(Counted::new(40.0, &first));
        let b = RenderRef::new(Counted::new(40.0, &second));

        let mut list = ListView::new().push(a.clone()).push(b.clone());
        assert_eq!(list.layout(BoxConstraints::tight(100.0, 60.0)), Size::new(100.0, 60.0));
        assert_eq!((first.get(), second.get()), (1, 1));
        assert_eq!(list.max_scroll_extent(), 20.0);
        let column = list.flex.clone().expect("built by the layout");

        // A rebuild that handed back the same rows and a new scroll offset.
        let mut fresh = ListView::new().with_offset(10.0).push(a.clone()).push(b.clone());
        assert_eq!(list.update_from(&mut fresh), Some(UpdateEffect::Relayout));

        assert!(list.flex.as_ref().expect("kept").is(&column), "the column was replaced");
        assert_eq!(
            list.max_scroll_extent(),
            20.0,
            "a list that was told rather than replaced forgot how far it can scroll"
        );
        list.layout(BoxConstraints::tight(100.0, 60.0));
        assert_eq!(
            (first.get(), second.get()),
            (1, 1),
            "a row that did not change was measured again"
        );
        assert_eq!(
            list.composed.as_ref().expect("laid out").offset(),
            10.0,
            "the list took the new offset and did not scroll"
        );
    }

    #[test]
    fn a_list_whose_rows_changed_still_shows_them() {
        // The scary half again: a column that kept its identity and not its
        // children would scroll the list it had last frame.
        let laid_out = counter();
        let a = RenderRef::new(Counted::new(40.0, &laid_out));
        let mut list = ListView::new().push(a.clone());
        assert_eq!(list.layout(BoxConstraints::tight(100.0, 200.0)), Size::new(100.0, 200.0));
        assert_eq!(list.max_scroll_extent(), 0.0);

        let b = RenderRef::new(Counted::new(300.0, &laid_out));
        let mut fresh = ListView::new().push(a.clone()).push(b.clone());
        assert_eq!(list.update_from(&mut fresh), Some(UpdateEffect::Nothing));
        list.layout(BoxConstraints::tight(100.0, 200.0));
        assert_eq!(
            list.max_scroll_extent(),
            140.0,
            "the column was kept and the second row was not"
        );
    }
}
