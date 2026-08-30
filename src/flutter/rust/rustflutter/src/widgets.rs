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
use crate::painting::{Gradient, Image, RenderPath};
pub use crate::render::{
    Alignment, AlignmentDirectional, Axis, AxisDirection, BoxConstraints, BoxFit, BoxedRender,
    Constraints, CrossAxisAlignment, EdgeInsets, EdgeInsetsDirectional, Fill, FlexChild,
    HitTestEntry, HitTestResult, MainAxisAlignment, MainAxisSize, Offset, PaintContext,
    RenderBox as Widget, Size, StackFit, StackPosition, VerticalDirection,
};
use crate::render::{
    RenderAlign, RenderAspectRatio, RenderBaseline, RenderBox, RenderClipPath, RenderClipRect,
    RenderConstrainedBox, RenderDecoratedBox, RenderFittedBox, RenderFlex,
    RenderFractionallySizedBox, RenderFullWidth, RenderImage, RenderIndexedStack,
    RenderIntrinsicHeight, RenderIntrinsicWidth, RenderLimitedBox, RenderOpacity,
    RenderOverflowBox, RenderPadding, RenderParagraph, RenderPointerRegion, RenderRef,
    RenderSizedOverflowBox, RenderStack, RenderTransform, RenderViewport, RenderWrap, UpdateEffect,
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

/// Clips `child` to its own bounds.
///
/// Upstream's `ClipRect`. The render object has been here all along; what was
/// missing was the widget in front of it, which is what a caller building a
/// tree actually reaches for. See [`crate::render::RenderClipRect`].
pub fn clip_rect(child: crate::framework::AnyWidget) -> crate::framework::AnyWidget {
    crate::framework::single(child, crate::render::RenderClipRect::new)
}

/// Lays `children` out along one axis.
///
/// Upstream's `Flex`, which `Row` and `Column` are the two directions of.
/// See [`crate::render::RenderFlex`].
pub fn flex(
    direction: crate::render::Axis,
    children: Vec<crate::framework::AnyWidget>,
) -> crate::framework::AnyWidget {
    crate::framework::many(children, move |children| {
        let mut flex = crate::render::RenderFlex::new(direction);
        for child in children {
            flex = flex.push(child);
        }
        flex
    })
}

/// Animates its own size to whatever `child` asks for.
///
/// Upstream's `AnimatedSize`. See [`crate::render::RenderAnimatedSize`].
pub fn animated_size(
    alignment: crate::render::Alignment,
    duration_ms: u32,
    child: crate::framework::AnyWidget,
) -> crate::framework::AnyWidget {
    crate::framework::single(child, move |child| {
        crate::render::RenderAnimatedSize::new(alignment, child).with_duration(duration_ms)
    })
}

/// The window on a scrollable's contents: what is on screen of something
/// taller than the screen.
///
/// Upstream's `Viewport`. See [`crate::render::RenderViewport`].
pub fn viewport(
    axis: crate::render::Axis,
    offset: f32,
    child: crate::framework::AnyWidget,
) -> crate::framework::AnyWidget {
    crate::framework::single(child, move |child| {
        crate::render::RenderViewport::new(axis, child).with_offset(offset)
    })
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
    /// Upstream's `semanticsLabel`: what a reader hears where the text is what
    /// a reader *sees*.
    ///
    /// Upstream's own example is `TextSpan(text: r'$$', semanticsLabel:
    /// 'Double dollars')`. The two strings are not two spellings of one thing;
    /// they are what the glyphs are and what the words are, and a screen
    /// reader given `$$` says "dollar dollar".
    pub semantics_label: Option<String>,
}

impl TextSpan {
    pub fn new(text: impl Into<String>, style: TextStyle) -> TextSpan {
        TextSpan {
            text: text.into(),
            style,
            semantics_label: None,
        }
    }

    /// What a reader hears for this span: its label where it has one, and its
    /// text where it does not.
    pub fn semantics_text(&self) -> &str {
        self.semantics_label.as_deref().unwrap_or(&self.text)
    }

    /// Upstream's `assert(!(text == null && semanticsLabel != null))`.
    ///
    /// A label with no text under it is not a label of anything. Upstream's
    /// `text` is nullable and a span may be children-only; here the empty
    /// string stands for that, so the rule reads the same: **you cannot
    /// rename nothing.**
    pub fn check(&self) -> Result<(), &'static str> {
        if self.text.is_empty() && self.semantics_label.is_some() {
            return Err("a semanticsLabel needs text to stand in for");
        }
        Ok(())
    }

    /// The same span, saying something else to a reader.
    pub fn spoken_as(mut self, label: impl Into<String>) -> TextSpan {
        self.semantics_label = Some(label.into());
        self
    }

    /// The same text in a bolder weight, which is what a run inside a sentence
    /// usually differs by.
    pub fn bold(text: impl Into<String>, style: &TextStyle) -> TextSpan {
        TextSpan::new(
            text,
            TextStyle {
                font_weight: 700,
                ..style.clone()
            },
        )
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
        // Built only if some span asked to be heard differently -- see
        // `RenderParagraph::with_semantics_content`.
        let spoken = spans
            .iter()
            .any(|span| span.semantics_label.is_some())
            .then(|| {
                spans
                    .iter()
                    .map(|span| span.semantics_text())
                    .collect::<String>()
            });
        RenderParagraph::rich(spans.into_iter().map(|s| (s.text, s.style)).collect())
            .with_semantics_content(spoken)
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
    /// Per-corner rounding when the caller wants more than one radius;
    /// upstream `BoxDecoration.borderRadius`.
    border_radius: Option<crate::borders::BorderRadius>,
    /// A whole decoration, upstream `Container.decoration`; when set it
    /// replaces the fill/radius/border fields and drives the padding.
    decoration: Option<crate::decoration::Decoration>,
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
            border_radius: None,
            decoration: None,
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
        self.fill = Some(Fill::Linear {
            start,
            end,
            gradient,
        });
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

    /// Per-corner rounding, upstream `BoxDecoration.borderRadius`. Takes
    /// precedence over [`Container::with_corner_radius`].
    pub fn with_border_radius(mut self, radius: crate::borders::BorderRadius) -> Self {
        self.border_radius = Some(radius);
        self
    }

    /// A whole decoration, upstream `Container(decoration:)`.
    pub fn with_decoration(mut self, decoration: crate::decoration::Decoration) -> Self {
        self.decoration = Some(decoration);
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
        // Upstream pads whenever `_paddingIncludingDecoration` is non-null,
        // which is whenever there is padding of the container's own or a
        // decoration carrying a border: a border insets the content, and the
        // container says so with a padding wrapper rather than by making the
        // decoration one.
        if self.padding != EdgeInsets::ZERO
            || self.border_width > 0.0
            || self.decoration.as_ref().is_some_and(|decoration| {
                decoration
                    .padding()
                    .resolve(crate::direction::current_direction())
                    != EdgeInsets::ZERO
            })
        {
            shape.push(Layer::Padding);
        }
        if self.decoration.is_some()
            || self.fill.is_some()
            || self.border_width > 0.0
            || !self.shadows.is_empty()
        {
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

    /// The container's own padding folded together with its decoration's
    /// border, upstream's `_paddingIncludingDecoration` over
    /// `BoxDecoration.padding`, which answers `border?.dimensions` -- the
    /// border's widths as insets, so that what the border frames is the
    /// content and not the middle of the stroke.
    fn padding_including_decoration(&self) -> EdgeInsets {
        match &self.decoration {
            // Upstream's `_paddingIncludingDecoration` over
            // `decoration.padding` -- the border's widths as insets.
            Some(decoration) => self.padding.add(
                decoration
                    .padding()
                    .resolve(crate::direction::current_direction()),
            ),
            None => self.padding.add(EdgeInsets::all(self.border_width)),
        }
    }

    /// Builds one wrapper around `inner`, as this container is configured now.
    fn build_layer(&self, kind: Layer, inner: Option<BoxedWidget>) -> BoxedWidget {
        match kind {
            // Aligning and padding need something to align and pad, even when
            // that is nothing but the space itself.
            Layer::Align => {
                let inner = inner.unwrap_or_else(|| RenderRef::new(Expand::new()));
                let alignment = self.alignment.expect("the shape said there was one");
                RenderRef::new(RenderAlign::new(alignment, inner))
            }
            Layer::Padding => {
                let inner = inner.unwrap_or_else(|| RenderRef::new(Expand::new()));
                RenderRef::new(RenderPadding::new(
                    self.padding_including_decoration(),
                    inner,
                ))
            }
            Layer::Decoration => {
                let mut decorated = match self.decoration.clone() {
                    Some(decoration) => RenderDecoratedBox::new().with_decoration(decoration),
                    None => {
                        let mut decorated = RenderDecoratedBox::new()
                            .with_corner_radius(self.corner_radius)
                            .with_shadows(self.shadows.clone())
                            .with_border(self.border_width, self.border_color);
                        if let Some(radius) = self.border_radius {
                            decorated = decorated.with_border_radius(radius);
                        }
                        if let Some(fill) = self.fill.clone() {
                            decorated = decorated.with_fill(fill);
                        }
                        decorated
                    }
                };
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
                let inner = inner.unwrap_or_else(|| RenderRef::new(Expand::new()));
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
        Some(current.unwrap_or_else(|| RenderRef::new(Expand::new())))
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
        if fresh.shape()
            != self
                .layers
                .iter()
                .map(|(kind, _)| *kind)
                .collect::<Vec<_>>()
        {
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
            // Wrapping nothing, this container *is* its child -- or an
            // `Expand` when it has none, and an `Expand` has nothing to be
            // told.
            if self.child.is_some() {
                self.composed = self.child.clone();
            }
            return Some(UpdateEffect::Nothing);
        }
        let onto: Vec<BoxedWidget> = self
            .layers
            .iter()
            .map(|(_, handle)| handle.clone())
            .collect();
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

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        if let Some(composed) = &self.composed {
            visit(composed, Offset::ZERO);
        }
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.composed
            .as_ref()
            .is_some_and(|c| c.hit_test(position, result))
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.composed
            .as_ref()
            .map_or(0.0, |c| c.min_intrinsic_width(height))
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.composed
            .as_ref()
            .map_or(0.0, |c| c.max_intrinsic_width(height))
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.composed
            .as_ref()
            .map_or(0.0, |c| c.min_intrinsic_height(width))
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.composed
            .as_ref()
            .map_or(0.0, |c| c.max_intrinsic_height(width))
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.composed
            .as_ref()
            .and_then(|c| c.distance_to_baseline())
    }
}

/// What a childless container holds: everything it is offered, and nothing
/// when nothing is bounded.
///
/// Upstream `Container.build` puts exactly this in for a childless container
/// whose constraints are not tight: a `LimitedBox(maxWidth: 0, maxHeight: 0)`
/// around a `ConstrainedBox(constraints: BoxConstraints.expand())`. The
/// `expand` wins wherever an axis is bounded, so the box is the biggest size
/// the constraints allow; the `LimitedBox` clamps unbounded axes to zero,
/// because a box may not be infinitely large. [`BoxConstraints::biggest`]
/// answers both halves in one call, safely: this port's `biggest`
/// deliberately collapses an unbounded axis to its minimum (a documented
/// safety of this crate), and the minimum a container is ever handed is zero
/// -- the same clamp the `LimitedBox` was there to make.
struct Expand {
    size: Size,
}

impl Expand {
    fn new() -> Expand {
        Expand { size: Size::ZERO }
    }
}

impl Default for Expand {
    fn default() -> Self {
        Expand::new()
    }
}

impl RenderBox for Expand {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<crate::render::UpdateEffect> {
        fresh.as_any_mut().downcast_mut::<Expand>()?;
        Some(crate::render::UpdateEffect::Nothing)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = constraints.biggest();
        self.size
    }
    fn size(&self) -> Size {
        self.size
    }
    fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
    fn hit_test(&self, _position: Offset, _result: &mut HitTestResult) -> bool {
        false
    }
}

/// Takes no space and paints nothing. A useful placeholder in a conditional
/// tree.
pub struct Empty;

impl RenderBox for Empty {
    /// Nothing to describe, so nothing can have changed. The one render object
    /// whose answer is a foregone conclusion.
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<crate::render::UpdateEffect> {
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

// -- Navigation toolbar -------------------------------------------------------

/// The default spacing around the middle widget. Upstream's
/// `NavigationToolbar.kMiddleSpacing`.
pub const K_MIDDLE_SPACING: f32 = 16.0;

/// Lays out the three parts of a title bar: something at each edge and
/// something filling what is left.
///
/// This is upstream's `NavigationToolbar`
/// (`widgets/navigation_toolbar.dart`), whose whole substance is the
/// `_ToolbarLayout` delegate; [`RenderNavigationToolbar::layout`] below is that
/// delegate's `performLayout` line for line.
///
/// It is a render object rather than a `Row` because the order matters and a
/// flex cannot express it: **the edges are measured first and keep their
/// widths**, and only then is the middle told how much is left. A row of
/// [leading, middle, trailing] would let a long middle push the trailing off
/// the end of the bar -- which is exactly the bug this replaced. Nothing about
/// a title bar is negotiable except the title.
pub struct RenderNavigationToolbar {
    leading: Option<BoxedRender>,
    middle: Option<BoxedRender>,
    trailing: Option<BoxedRender>,
    /// Whether the middle widget is centred on the bar or only spaced off the
    /// leading widget. Upstream's default is `NavigationToolbar`'s
    /// `this.centerMiddle = true` (`widgets/navigation_toolbar.dart`): the
    /// title sits in the middle of the bar, not hugging the leading edge.
    center_middle: bool,
    middle_spacing: f32,
    /// Which way the bar reads. Upstream's `NavigationToolbar` builds its
    /// delegate with `Directionality.of(context)` (`widgets/basic.dart`, the
    /// consumption every text-direction-sensitive widget makes); here the
    /// ambient direction at construction stands in for that, the same moment
    /// relative to the build that upstream resolves it in.
    text_direction: crate::direction::TextDirection,
    /// Where the last layout put leading, middle and trailing.
    offsets: [Offset; 3],
    size: Size,
}

impl RenderNavigationToolbar {
    pub fn new() -> RenderNavigationToolbar {
        RenderNavigationToolbar {
            leading: None,
            middle: None,
            trailing: None,
            // `this.centerMiddle = true` up top in the constructor.
            center_middle: true,
            middle_spacing: K_MIDDLE_SPACING,
            text_direction: crate::direction::current_direction(),
            offsets: [Offset::ZERO; 3],
            size: Size::ZERO,
        }
    }

    pub fn with_leading(mut self, leading: impl RenderBox + 'static) -> Self {
        self.leading = Some(RenderRef::new(leading));
        self
    }

    pub fn with_middle(mut self, middle: impl RenderBox + 'static) -> Self {
        self.middle = Some(RenderRef::new(middle));
        self
    }

    pub fn with_trailing(mut self, trailing: impl RenderBox + 'static) -> Self {
        self.trailing = Some(RenderRef::new(trailing));
        self
    }

    /// Whether the middle is centred in the whole bar rather than started next
    /// to the leading. Upstream `AppBar` decides this per platform in
    /// `_getEffectiveCenterTitle`: true on iOS and macOS, false everywhere
    /// else.
    pub fn with_center_middle(mut self, center_middle: bool) -> Self {
        self.center_middle = center_middle;
        self
    }

    pub fn with_middle_spacing(mut self, spacing: f32) -> Self {
        self.middle_spacing = spacing;
        self
    }

    fn parts(&self) -> [&Option<BoxedRender>; 3] {
        [&self.leading, &self.middle, &self.trailing]
    }
}

impl Default for RenderNavigationToolbar {
    fn default() -> Self {
        RenderNavigationToolbar::new()
    }
}

impl RenderBox for RenderNavigationToolbar {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh
            .as_any_mut()
            .downcast_mut::<RenderNavigationToolbar>()?;
        let same_part = |a: &Option<BoxedRender>, b: &Option<BoxedRender>| match (a, b) {
            (Some(a), Some(b)) => a.is(b),
            (None, None) => true,
            _ => false,
        };
        let kept = same_part(&self.leading, &fresh.leading)
            && same_part(&self.middle, &fresh.middle)
            && same_part(&self.trailing, &fresh.trailing);
        let effect = UpdateEffect::relayout_if(
            !kept
                || self.center_middle != fresh.center_middle
                || self.middle_spacing != fresh.middle_spacing
                // Upstream's `shouldRelayout`, which compares the direction
                // along with everything else.
                || self.text_direction != fresh.text_direction,
        );
        self.leading = fresh.leading.take();
        self.middle = fresh.middle.take();
        self.trailing = fresh.trailing.take();
        self.center_middle = fresh.center_middle;
        self.middle_spacing = fresh.middle_spacing;
        self.text_direction = fresh.text_direction;
        Some(effect)
    }

    /// Upstream `_ToolbarLayout.performLayout`, in its order, which is the
    /// whole point of it. The `switch (textDirection)` each placement ends in
    /// is written out here too: in rtl the leading widget goes to the right
    /// edge, the trailing to the left, and the middle is mirrored against the
    /// bar rather than placed from its left.
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // `MultiChildLayoutDelegate.getSize` is `constraints.biggest`, and the
        // delegate lays out against that.
        let size = constraints.biggest();
        self.size = size;
        self.offsets = [Offset::ZERO; 3];
        let rtl = self.text_direction == crate::direction::TextDirection::Rtl;

        let mut leading_width = 0.0f32;
        let mut trailing_width = 0.0f32;

        // "The height should be exactly the height of the bar."
        let leading_size = self.leading.as_mut().map(|leading| {
            leading.layout(BoxConstraints::new(
                0.0,
                size.width,
                size.height,
                size.height,
            ))
        });
        if let Some(leading_size) = leading_size {
            leading_width = leading_size.width;
            let leading_x = if rtl { size.width - leading_width } else { 0.0 };
            self.offsets[0] = Offset::new(leading_x, 0.0);
        }

        // `BoxConstraints.loose(size)`.
        let trailing_size = self
            .trailing
            .as_mut()
            .map(|trailing| trailing.layout(BoxConstraints::loose(size.width, size.height)));
        if let Some(trailing_size) = trailing_size {
            trailing_width = trailing_size.width;
            let trailing_x = if rtl {
                0.0
            } else {
                size.width - trailing_size.width
            };
            self.offsets[2] = Offset::new(trailing_x, (size.height - trailing_size.height) / 2.0);
        }

        let middle_spacing = self.middle_spacing;
        let max_width =
            (size.width - leading_width - trailing_width - middle_spacing * 2.0).max(0.0);
        // `BoxConstraints.loose(size).copyWith(maxWidth: maxWidth)`.
        let middle_size = self
            .middle
            .as_mut()
            .map(|middle| middle.layout(BoxConstraints::loose(max_width, size.height)));

        if let Some(middle_size) = middle_size {
            let middle_start_margin = leading_width + self.middle_spacing;
            let mut middle_start = middle_start_margin;
            let middle_y = (size.height - middle_size.height) / 2.0;
            // If the centred middle will not fit between the leading and
            // trailing widgets, align its edge with the adjacent boundary.
            if self.center_middle {
                middle_start = (size.width - middle_size.width) / 2.0;
                if middle_start + middle_size.width > size.width - trailing_width {
                    middle_start =
                        size.width - trailing_width - middle_size.width - self.middle_spacing;
                } else if middle_start < middle_start_margin {
                    middle_start = middle_start_margin;
                }
            }
            let middle_x = if rtl {
                size.width - middle_size.width - middle_start
            } else {
                middle_start
            };
            self.offsets[1] = Offset::new(middle_x, middle_y);
        }

        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        for (part, placement) in self.parts().iter().zip(self.offsets.iter()) {
            if let Some(part) = part {
                context.paint_child(part, offset.plus(*placement));
            }
        }
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        for (part, placement) in self.parts().iter().zip(self.offsets.iter()) {
            if let Some(part) = part {
                visit(part, *placement);
            }
        }
    }

    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Back to front, as everywhere else: the last painted is the first
        // asked.
        for (part, placement) in self.parts().iter().zip(self.offsets.iter()).rev() {
            if let Some(part) = part {
                if part.hit_test(position.minus(*placement), result) {
                    return true;
                }
            }
        }
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

    /// Places the child at an alignment that names its edges by reading
    /// order, resolved against the ambient direction.
    ///
    /// Upstream's `Align(alignment: AlignmentDirectional....)`. The direction
    /// is captured here rather than read later, so the alignment settles the
    /// moment the widget is built -- see [`crate::direction`].
    pub fn directional(
        alignment: AlignmentDirectional,
        child: impl RenderBox + 'static,
    ) -> RenderAlign {
        RenderAlign::directional(alignment, child)
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

    /// Insets by edges named in reading order, resolved against the ambient
    /// direction when this is built.
    ///
    /// Upstream's `Padding(padding: EdgeInsetsDirectional....)`: `start` is
    /// the left in an LTR subtree and the right in an RTL one, which is a
    /// different `EdgeInsets` and so a different render object -- resolved
    /// here rather than at layout, the same moment upstream's `build` runs.
    pub fn directional(
        insets: EdgeInsetsDirectional,
        child: impl RenderBox + 'static,
    ) -> RenderPadding {
        RenderPadding::new(insets.resolve(crate::direction::current_direction()), child)
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
    ///
    /// Upstream is `SizedBox(height: h)`, whose `_additionalConstraints` is
    /// `BoxConstraints.tightFor(height: h)`: the height axis tight, the width
    /// axis untouched -- a minimum of zero and no maximum, so a child keeps
    /// whatever width it was offered rather than being clamped to none.
    pub fn height(height: f32) -> RenderConstrainedBox {
        RenderConstrainedBox::new(BoxConstraints::new(0.0, f32::INFINITY, height, height))
    }

    /// The same, the other axis: `SizedBox(width: w)` / `tightFor(width: w)`.
    pub fn width(width: f32) -> RenderConstrainedBox {
        RenderConstrainedBox::new(BoxConstraints::new(width, width, 0.0, f32::INFINITY))
    }
}

// -- Layout -------------------------------------------------------------------

/// Lays children out top to bottom.
pub struct Column;

impl Column {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> RenderFlex {
        // Upstream `Flex` (hence `Column`) defaults to `MainAxisSize.max`,
        // stretching to the incoming height when it is bounded and
        // degrading to the content height when it is not (upstream
        // `RenderFlex` degrades the same way for an unbounded max). A column
        // that should shrink-wrap says so with
        // `with_main_axis_size(MainAxisSize::Min)`.
        RenderFlex::column().with_cross_axis_alignment(CrossAxisAlignment::Center)
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
        // Upstream `Row` defaults to `MainAxisSize.max`; see `Column::new`.
        RenderFlex::row().with_cross_axis_alignment(CrossAxisAlignment::Center)
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

/// Limits its child only where the constraints are unbounded.
///
/// Upstream's `LimitedBox` (`widgets/basic.dart`): `.with_max_width` /
/// `.with_max_height` say what the child should be sized to in an unbounded
/// direction, and change nothing in a bounded one.
pub struct LimitedBox;

impl LimitedBox {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> RenderLimitedBox {
        RenderLimitedBox::new(child)
    }
}

/// Scales and positions its child within itself according to a `BoxFit`.
///
/// Upstream's `FittedBox` (`widgets/basic.dart`): the child is laid out at
/// its natural size, then scaled into the box; `.with_fit` picks the
/// discipline, `.with_alignment` where the result sits.
pub struct FittedBox;

impl FittedBox {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> RenderFittedBox {
        RenderFittedBox::new(child)
    }
}

/// Positions its child so the child's baseline is a fixed distance from the
/// top.
///
/// Upstream's `Baseline` (`widgets/basic.dart`). The `baselineType` upstream
/// carries has no counterpart here because there is only one baseline to ask
/// a child for.
pub struct Baseline;

impl Baseline {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(baseline: f32, child: impl RenderBox + 'static) -> RenderBaseline {
        RenderBaseline::new(baseline, child)
    }
}

/// Sizes its child to a fraction of the space it is given.
///
/// Upstream's `FractionallySizedBox` (`widgets/basic.dart`):
/// `.with_width_factor(0.5)` is "half of whatever this turns out to be".
pub struct FractionallySizedBox;

impl FractionallySizedBox {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> RenderFractionallySizedBox {
        RenderFractionallySizedBox::new(child)
    }
}

/// Imposes different constraints on its child than it gets, letting the
/// child overflow.
///
/// Upstream's `OverflowBox` (`widgets/basic.dart`): each `.with_*_width` /
/// `.with_*_height` overrides that one constraint, and the box itself stays
/// the size of its constraints.
pub struct OverflowBox;

impl OverflowBox {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> RenderOverflowBox {
        RenderOverflowBox::new(child)
    }
}

/// A box of a given size that passes its original constraints through to its
/// child, which may then overflow.
///
/// Upstream's `SizedOverflowBox` (`widgets/basic.dart`).
pub struct SizedOverflowBox;

impl SizedOverflowBox {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(size: Size, child: impl RenderBox + 'static) -> RenderSizedOverflowBox {
        RenderSizedOverflowBox::new(size, child)
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
        StackPosition {
            left: Some(left),
            top: Some(top),
            ..Default::default()
        }
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

/// Anchors a stacked child to edges named in reading order.
///
/// Upstream's `PositionedDirectional` (`basic.dart`): `start` is whichever of
/// left and right the ambient direction reads from, so an RTL subtree anchors
/// `start` to the right edge. Which is which is settled here -- the same
/// moment upstream's `build` turns the widget into a `Positioned` with left
/// and right already chosen -- because by layout the walk that knew the
/// direction is over.
pub struct PositionedDirectional;

impl PositionedDirectional {
    /// All four edges, any of which may be left off.
    ///
    /// Upstream's build is two lines -- `left = rtl ? end : start;
    /// right = rtl ? start : end` -- and they are the two lines here.
    pub fn new(
        start: Option<f32>,
        top: Option<f32>,
        end: Option<f32>,
        bottom: Option<f32>,
    ) -> StackPosition {
        let (left, right) =
            if crate::direction::current_direction() == crate::direction::TextDirection::Rtl {
                (end, start)
            } else {
                (start, end)
            };
        StackPosition {
            left,
            top,
            right,
            bottom,
            ..Default::default()
        }
    }

    /// `start` and `top`, the common two.
    pub fn at(start: f32, top: f32) -> StackPosition {
        Self::new(Some(start), Some(top), None, None)
    }
}

/// A stack that shows a single child from a list.
///
/// Upstream's `IndexedStack` (`widgets/indexed_stack.dart`): every child is
/// laid out -- so each keeps its state -- the stack is as big as the largest
/// child, and only the child at `index` is painted, hit-tested or described.
/// `with_index(None)` is upstream's `index: null`: nothing is shown.
pub struct IndexedStack;

impl IndexedStack {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> RenderIndexedStack {
        RenderIndexedStack::new()
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
    /// Which way the list scrolls, upstream's `AxisDirection`. Down for a
    /// column; a row is right in an LTR subtree and left in an RTL one
    /// (`getAxisDirectionFromAxisReverseAndDirectionality`, the same line
    /// `ScrollView` builds its viewport with).
    ///
    /// Captured at construction, because the viewport this feeds is composed
    /// at layout -- outside the walk, when the ambient direction is no longer
    /// the one this list was built under.
    axis_direction: AxisDirection,
    offset: f32,
    spacing: f32,
    centred_item: Option<f32>,
    link: Option<std::rc::Rc<crate::scrolling::ScrollLink>>,
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
            axis_direction: AxisDirection::Down,
            offset: 0.0,
            spacing: 0.0,
            centred_item: None,
            link: None,
            children: Vec::new(),
            inset: None,
            flex: None,
            composed: None,
        }
    }

    pub fn horizontal() -> ListView {
        // An RTL row starts at the right edge, so that is the way it scrolls
        // too: `textDirectionToAxisDirection` over the direction in force
        // where the list was built.
        let axis_direction =
            if crate::direction::current_direction() == crate::direction::TextDirection::Rtl {
                AxisDirection::Left
            } else {
                AxisDirection::Right
            };
        ListView {
            axis: Axis::Horizontal,
            axis_direction,
            ..ListView::new()
        }
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

    /// The [`Scroll`](crate::scrolling::Scroll) this list belongs to, as a
    /// handle.
    ///
    /// A scroll offset has to be clamped to something, and that something is
    /// not known until the content has been measured -- which happens inside
    /// the tree, a frame after whoever holds the offset needs it. And a
    /// focused field that has to be scrolled into view is decided in the other
    /// direction, inside the tree, by something that cannot reach the offset.
    /// The handle carries both. Upstream solves the pair with a
    /// `ScrollPosition` the viewport attaches itself to at layout.
    ///
    /// A list without one still scrolls under a finger; it just never reports
    /// its extent and never scrolls itself. See
    /// [`crate::scrolling::ScrollLink`].
    pub fn with_link(mut self, link: std::rc::Rc<crate::scrolling::ScrollLink>) -> Self {
        self.link = Some(link);
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
        self.composed
            .as_ref()
            .map_or(0.0, |v| v.max_scroll_extent())
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
            .with_spacing(self.spacing)
            // The column is laid out outside the walk too, so its direction
            // comes from this list rather than from whatever is ambient at
            // layout. A leftward (RTL) row lays its first child at the right
            // edge -- the order upstream's slivers lay theirs in -- and an
            // upward one lays it at the bottom.
            .with_text_direction(if self.axis_direction == AxisDirection::Left {
                crate::direction::TextDirection::Rtl
            } else {
                crate::direction::TextDirection::Ltr
            })
            .with_vertical_direction(match self.axis_direction {
                AxisDirection::Up => VerticalDirection::Up,
                _ => VerticalDirection::Down,
            });
        if let Some(inset) = self.inset {
            flex = flex.push(spacer(self.axis, inset));
        }
        for (index, child) in self.children.iter().enumerate() {
            // Numbered, which is upstream's `addSemanticIndexes` on the
            // delegate a `ListView` builds by default. Without it a reader is
            // told the list has forty rows and never which of them they are
            // standing on -- and the number cannot be recovered from the walk,
            // because the spacers at either end are children too.
            flex = flex.push(crate::render::RenderIndexedSemanticsBox::new(
                index as i64,
                child.clone(),
            ));
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
        self.axis_direction = fresh.axis_direction;
        self.offset = fresh.offset;
        self.spacing = fresh.spacing;
        self.centred_item = fresh.centred_item;
        self.link = fresh.link.take();
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
        let mut staged = RenderViewport::new(self.axis, flex)
            .with_offset(self.offset)
            // How many rows went in, which only this list knows: the viewport
            // was handed one column.
            //
            // **Untested, and said so rather than assumed.** A rebuild in the
            // tests replaces this render object rather than updating it, so
            // this branch is not reached and a mutation blanking it stays
            // green -- as does one stopping `RenderViewport::update_from`
            // taking the fresh count. Both are written the way the layout path
            // below is written, and whether anything reaches them is a
            // question about how a rebuilt `ListView` is matched, not about
            // these two lines.
            .with_semantic_child_count(Some(self.children.len() as i32))
            .with_axis_direction(self.axis_direction);
        if let Some(link) = &self.link {
            staged = staged.with_link(std::rc::Rc::clone(link));
        }
        self.composed
            .as_mut()
            .expect("built with the column")
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
            let mut viewport = RenderViewport::new(self.axis, flex)
                .with_offset(self.offset)
                .with_semantic_child_count(Some(self.children.len() as i32))
                .with_axis_direction(self.axis_direction);
            if let Some(link) = &self.link {
                viewport = viewport.with_link(std::rc::Rc::clone(link));
            }
            self.composed = Some(viewport);
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
        if let Some(link) = &self.link {
            link.set_measurements(viewport.max_scroll_extent(), viewport.size().height);
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

    /// The scrolled column, where the viewport draws it.
    ///
    /// Straight through the viewport rather than reporting it, because the
    /// viewport is not behind a handle -- this list is its handle, as
    /// [`ListView::update_from`] says -- and so the column's parent, claimed
    /// when it was laid out, is *this* object. A walk up from anything inside
    /// the list arrives here and asks this method where its child is; an
    /// answer naming the viewport is an answer about something that walk will
    /// never reach, and `Offset::ZERO` with it, which is the scroll offset
    /// thrown away.
    ///
    /// [`RenderRef::offset_in`] is what asks, on behalf of
    /// [`RenderRef::transform_to`], so what this reports is where a tooltip
    /// anchored inside a scrolled list opens and how far a reveal decides to
    /// scroll.
    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        if let Some(composed) = &self.composed {
            composed.visit_children(visit);
        }
    }

    /// The viewport's, for the reason the clip below gives: **the window is one
    /// field in**, and `visit_children` hands the walk the column's children
    /// rather than the viewport, so nothing else would ever ask it.
    ///
    /// Without this a `ListView` reached a reader as a plain box -- no word
    /// that it scrolls, no position in it, no count, and no gesture offered --
    /// while the viewport inside it had the answer to all four ready.
    fn describe_semantics(&self) -> Option<crate::semantics::SemanticsAnnotation> {
        self.composed
            .as_ref()
            .and_then(|composed| composed.describe_semantics())
    }

    /// The viewport's, for the same reason: what clips the column is the
    /// window, and the window is one field in.
    fn describe_approximate_paint_clip(
        &self,
        child: &dyn RenderBox,
    ) -> Option<crate::engine::Rect> {
        self.composed
            .as_ref()
            .and_then(|composed| composed.describe_approximate_paint_clip(child))
    }

    fn as_viewport(&self) -> Option<&crate::render::RenderViewport> {
        self.composed.as_ref()
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

    /// Per-corner rounding, upstream `ClipRRect(borderRadius:)`.
    #[allow(clippy::new_ret_no_self)]
    pub fn rounded(
        radius: crate::borders::BorderRadius,
        child: impl RenderBox + 'static,
    ) -> RenderClipRect {
        RenderClipRect::new(child).with_border_radius(radius)
    }

    /// `ClipRRect(borderRadius: BorderRadiusDirectional(...))`: resolved
    /// against the ambient direction here, since the render tree carries
    /// physical corners only.
    #[allow(clippy::new_ret_no_self)]
    pub fn directional(
        radius: crate::borders::BorderRadiusDirectional,
        child: impl RenderBox + 'static,
    ) -> RenderClipRect {
        RenderClipRect::new(child)
            .with_border_radius(radius.resolve(crate::direction::current_direction()))
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

// -- The rest of `basic.dart` -------------------------------------------------
//
// One name apiece over a render object that is already here, in upstream's
// order. Each is upstream's constructor with upstream's defaults; where a
// parameter has nowhere to go it says so.

/// Composites its child through a shader -- upstream `ShaderMask`.
///
/// Upstream's `shaderCallback` is given the bounds and answers a `Shader`;
/// here it answers the whole `Paint` the layer composites with, because the
/// engine ABI takes a paint and not a bare shader.
pub struct ShaderMask;

impl ShaderMask {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        shader: std::rc::Rc<
            dyn Fn(crate::engine::Rect, crate::painting::BlendMode) -> crate::engine::Paint,
        >,
        child: impl RenderBox + 'static,
    ) -> crate::render::RenderShaderMask {
        crate::render::RenderShaderMask::new(shader, child)
    }
}

/// Blurs whatever is already painted behind its child -- upstream
/// `BackdropFilter`.
///
/// Upstream takes an `ImageFilter` and a blend mode. The engine's backdrop
/// ABI takes a blur sigma and nothing else, so that is what this takes; the
/// divergence is recorded with `RenderBackdropFilter`.
pub struct BackdropFilter;

impl BackdropFilter {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(sigma: f32, child: impl RenderBox + 'static) -> crate::render::RenderBackdropFilter {
        crate::render::RenderBackdropFilter::new(sigma, child)
    }
}

/// Draws with a painter, under and over its child -- upstream `CustomPaint`.
pub struct CustomPaint;

impl CustomPaint {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> crate::render::RenderCustomPaint {
        crate::render::RenderCustomPaint::new(child)
    }

    /// Upstream's `CustomPaint(size:)` with no child: the painter decides the
    /// size, so there is nothing to measure.
    pub fn sized(
        size: Size,
        painter: std::rc::Rc<dyn crate::render::CustomPainter>,
    ) -> crate::render::RenderCustomPaint {
        crate::render::RenderCustomPaint::bare()
            .with_preferred_size(size)
            .with_painter(painter)
    }
}

/// Clips its child to an oval inscribed in its box -- upstream `ClipOval`.
pub struct ClipOval;

impl ClipOval {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> crate::render::RenderClipOval {
        crate::render::RenderClipOval::new(child)
    }
}

/// Clips its child to a rounded superellipse -- upstream
/// `ClipRSuperellipse`, drawn with continuous corners here (the engine has
/// no superellipse primitive; the divergence is recorded).
pub struct ClipRSuperellipse;

impl ClipRSuperellipse {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        radius: crate::borders::BorderRadius,
        child: impl RenderBox + 'static,
    ) -> crate::render::RenderClipRSuperellipse {
        crate::render::RenderClipRSuperellipse::new(radius, child)
    }
}

/// A box that casts a shadow and clips its child to its own shape --
/// upstream `PhysicalModel`.
pub struct PhysicalModel;

impl PhysicalModel {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        color: Color,
        elevation: f32,
        child: impl RenderBox + 'static,
    ) -> crate::render::RenderPhysicalModel {
        crate::render::RenderPhysicalModel::new(child)
            .with_color(color)
            .with_elevation(elevation)
    }
}

/// [`PhysicalModel`] for an arbitrary shape -- upstream `PhysicalShape`,
/// whose `clipper` is a `ShapeBorder` here.
pub struct PhysicalShape;

impl PhysicalShape {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        shape: crate::borders::ShapeBorder,
        color: Color,
        elevation: f32,
        child: impl RenderBox + 'static,
    ) -> crate::render::RenderPhysicalShape {
        crate::render::RenderPhysicalShape::new(shape, child)
            .with_color(color)
            .with_elevation(elevation)
    }
}

/// Moves its child by a fraction of its own size, after layout -- upstream
/// `FractionalTranslation`.
pub struct FractionalTranslation;

impl FractionalTranslation {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        translation: Offset,
        child: impl RenderBox + 'static,
    ) -> crate::render::RenderFractionalTranslation {
        crate::render::RenderFractionalTranslation::new((translation.dx, translation.dy), child)
    }
}

/// Rotates its child by whole quarter turns, layout included -- upstream
/// `RotatedBox`, which unlike [`Transform`] does change what the child is
/// measured against.
pub struct RotatedBox;

impl RotatedBox {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        quarter_turns: usize,
        child: impl RenderBox + 'static,
    ) -> crate::render::RenderRotatedBox {
        crate::render::RenderRotatedBox::new(quarter_turns, child)
    }
}

/// Sizes and places one child by a delegate -- upstream
/// `CustomSingleChildLayout`.
pub struct CustomSingleChildLayout;

impl CustomSingleChildLayout {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        delegate: std::rc::Rc<dyn crate::render::SingleChildLayoutDelegate>,
        child: impl RenderBox + 'static,
    ) -> crate::render::RenderCustomSingleChildLayoutBox {
        crate::render::RenderCustomSingleChildLayoutBox::new(delegate, child)
    }
}

/// Places several identified children by a delegate -- upstream
/// `CustomMultiChildLayout`.
///
/// Upstream identifies each child with a `LayoutId` wrapper, which is a
/// `ParentDataWidget`; this crate's parents take their children's parent data
/// directly, so the identifier is passed alongside the child. [`LayoutId`] is
/// the pairing.
pub struct CustomMultiChildLayout;

impl CustomMultiChildLayout {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        delegate: std::rc::Rc<dyn crate::render::MultiChildLayoutDelegate>,
        children: Vec<(u64, BoxedRender)>,
    ) -> crate::render::RenderCustomMultiChildLayoutBox {
        crate::render::RenderCustomMultiChildLayoutBox::new(delegate, children)
    }
}

/// A child with the identifier its layout delegate knows it by -- upstream
/// `LayoutId`.
pub struct LayoutId;

impl LayoutId {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: u64, child: impl RenderBox + 'static) -> (u64, BoxedRender) {
        (id, RenderRef::new(child))
    }
}

/// Imposes extra constraints on its child -- upstream `ConstrainedBox`.
pub struct ConstrainedBox;

impl ConstrainedBox {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        constraints: BoxConstraints,
        child: impl RenderBox + 'static,
    ) -> RenderConstrainedBox {
        RenderConstrainedBox::new(constraints).with_child(child)
    }
}

/// Lays its child out against transformed constraints and aligns the result
/// -- upstream `ConstraintsTransformBox`.
pub struct ConstraintsTransformBox;

impl ConstraintsTransformBox {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        transform: crate::render::ConstraintsTransform,
        alignment: Alignment,
        child: impl RenderBox + 'static,
    ) -> crate::render::RenderConstraintsTransformBox {
        crate::render::RenderConstraintsTransformBox::new(transform, alignment, child)
    }
}

/// [`ConstraintsTransformBox`] with the constraints cleared -- upstream
/// `UnconstrainedBox`, which is that one specialisation of it: the child
/// renders at its natural size and overflows if it does not fit.
pub struct UnconstrainedBox;

impl UnconstrainedBox {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        alignment: Alignment,
        child: impl RenderBox + 'static,
    ) -> crate::render::RenderConstraintsTransformBox {
        crate::render::RenderConstraintsTransformBox::new(
            crate::render::ConstraintsTransform::Unconstrained,
            alignment,
            child,
        )
    }

    /// Upstream's `constrainedAxis`: the named axis keeps the constraints it
    /// was given and the other is freed -- `_axisToTransform`'s switch.
    pub fn along(
        constrained_axis: Axis,
        alignment: Alignment,
        child: impl RenderBox + 'static,
    ) -> crate::render::RenderConstraintsTransformBox {
        crate::render::RenderConstraintsTransformBox::new(
            match constrained_axis {
                Axis::Horizontal => crate::render::ConstraintsTransform::HeightUnconstrained,
                Axis::Vertical => crate::render::ConstraintsTransform::WidthUnconstrained,
            },
            alignment,
            child,
        )
    }
}

/// Lays its child out and then draws nothing, takes no space and is never
/// hit -- upstream `Offstage`, whose default is to be offstage.
pub struct Offstage;

impl Offstage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(offstage: bool, child: impl RenderBox + 'static) -> crate::render::RenderOffstage {
        crate::render::RenderOffstage::new(offstage, child)
    }
}

/// A box inside a scroll view -- upstream `SliverToBoxAdapter`.
///
/// The sliver protocol here is a method on the box protocol rather than a
/// second one (see `RenderSliverSingleBoxAdapter`'s ledger entry), so a box in
/// a viewport is the box itself and the adapter is the identity. It exists as
/// a name because reading `SliverToBoxAdapter` at the call site is what says
/// the child is a box and its parent is a sliver.
pub struct SliverToBoxAdapter;

impl SliverToBoxAdapter {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(child: impl RenderBox + 'static) -> BoxedRender {
        RenderRef::new(child)
    }
}

/// Insets a sliver -- upstream `SliverPadding`.
pub struct SliverPadding;

impl SliverPadding {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        padding: EdgeInsets,
        sliver: impl RenderBox + 'static,
    ) -> crate::render::RenderSliverPadding {
        crate::render::RenderSliverPadding::new(padding, sliver)
    }
}

/// Stacks its children along an axis, each at its own size, with no
/// scrolling of its own -- upstream `ListBody`.
pub struct ListBody;

impl ListBody {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(axis: Axis, children: Vec<BoxedRender>) -> crate::render::RenderListBody {
        crate::render::RenderListBody::new(ListBody::direction(axis, false), children)
    }

    /// Upstream's `reverse: true`: the same axis, filled from the far end.
    pub fn reversed(axis: Axis, children: Vec<BoxedRender>) -> crate::render::RenderListBody {
        crate::render::RenderListBody::new(ListBody::direction(axis, true), children)
    }

    /// Upstream's `_getDirection`, which is
    /// `getAxisDirectionFromAxisReverseAndDirectionality`: a vertical body
    /// runs down unless reversed, and a horizontal one runs in reading order
    /// unless reversed. The ambient direction is read here, at build, for the
    /// reason every other directional widget in this file reads it here.
    fn direction(axis: Axis, reverse: bool) -> AxisDirection {
        let forward = match axis {
            Axis::Vertical => AxisDirection::Down,
            Axis::Horizontal => match crate::direction::current_direction() {
                crate::direction::TextDirection::Ltr => AxisDirection::Right,
                crate::direction::TextDirection::Rtl => AxisDirection::Left,
            },
        };
        if reverse {
            crate::render::flip_axis_direction(forward)
        } else {
            forward
        }
    }
}

/// Places its children by a delegate at paint time -- upstream `Flow`, which
/// is the one layout that can move its children without laying them out
/// again.
pub struct Flow;

impl Flow {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        delegate: std::rc::Rc<dyn crate::render::FlowDelegate>,
        children: Vec<BoxedRender>,
    ) -> crate::render::RenderFlow {
        crate::render::RenderFlow::new(delegate, children)
    }
}

/// A paragraph of differently styled runs -- upstream `RichText`.
pub struct RichText;

impl RichText {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(spans: Vec<TextSpan>) -> RenderParagraph {
        RenderParagraph::rich_spans(spans)
    }
}

/// A decoded image, drawn -- upstream `RawImage`, which is what `Image`
/// builds once it has pixels.
pub struct RawImage;

impl RawImage {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(image: std::rc::Rc<Image>) -> RenderImage {
        RenderImage::new(image)
    }
}

/// Hears pointers passing through it without changing what they hit --
/// upstream `Listener`.
pub struct Listener;

impl Listener {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        id: u64,
        handlers: crate::gestures::PointerHandlers,
        child: impl RenderBox + 'static,
    ) -> RenderPointerRegion {
        RenderPointerRegion::new(id, child).with_handlers(handlers)
    }
}

/// Hears a mouse entering, moving inside and leaving -- upstream
/// `MouseRegion`, which is the same render object as [`Listener`] here (the
/// two are merged; see `RenderPointerRegion`'s ledger entry).
pub struct MouseRegion;

impl MouseRegion {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        id: u64,
        handlers: crate::gestures::PointerHandlers,
        child: impl RenderBox + 'static,
    ) -> RenderPointerRegion {
        RenderPointerRegion::new(id, child).with_handlers(handlers)
    }

    /// Upstream's `opaque: false`: the region hears what passes over it but
    /// does not become a target itself.
    pub fn transparent(
        id: u64,
        handlers: crate::gestures::PointerHandlers,
        child: impl RenderBox + 'static,
    ) -> RenderPointerRegion {
        RenderPointerRegion::new(id, child)
            .with_handlers(handlers)
            .with_behavior(crate::render::HitTestBehavior::Translucent)
    }
}

/// Takes the hit its child would have taken -- upstream `AbsorbPointer`,
/// whose default is to absorb.
pub struct AbsorbPointer;

impl AbsorbPointer {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        absorbing: bool,
        child: impl RenderBox + 'static,
    ) -> crate::render::RenderAbsorbPointer {
        crate::render::RenderAbsorbPointer::new(absorbing, child)
    }
}

/// Carries a payload out on whatever hit test finds it -- upstream
/// `MetaData`.
pub struct MetaData;

impl MetaData {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(meta_id: u64, child: impl RenderBox + 'static) -> crate::render::RenderMetaData {
        crate::render::RenderMetaData::new(meta_id, child)
    }
}

/// A box filled with one colour and nothing else -- upstream `ColoredBox`,
/// the cheap `Container(color:)`.
pub struct ColoredBox;

impl ColoredBox {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(color: Color, child: impl RenderBox + 'static) -> RenderDecoratedBox {
        RenderDecoratedBox::new()
            .with_color(color)
            .with_child(child)
    }

    /// Upstream's childless `ColoredBox`, which fills whatever it is given.
    pub fn filled(color: Color) -> RenderDecoratedBox {
        RenderDecoratedBox::new().with_color(color)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_list_view_says_how_many_rows_it_has_and_which_one_each_is() {
        // The half rounds 403 and 404 left: a `LazyList` is not a viewport and
        // a `SliverListView` is the other kind, while **this** is what the
        // gallery actually scrolls -- `ListView` over a `RenderViewport`. Its
        // rows carried no index and its viewport declared no count, so a
        // reader met a run of rows with no sense of how many or where.
        //
        // Upstream's `ListView(children:)` builds a `SliverChildListDelegate`,
        // whose `addSemanticIndexes` is on by default and whose
        // `semanticChildCount` is `children.length`.
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox, Size};

        crate::semantics::set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| {
            let mut list = ListView::new().with_offset(0.0);
            for index in 0..3 {
                list = list.push(crate::render::RenderRef::new(crate::widgets::Text::new(
                    format!("Row {index}"),
                )));
            }
            list
        }));
        let mut root = tree.build_render_tree().expect("mounted");
        RenderBox::layout(&mut root, BoxConstraints::tight(200.0, 400.0));
        // **Rebuilt with a different length before it is read.** A list is
        // rebuilt whenever its contents change, and the second build updates
        // the viewport that is already there rather than making one -- so a
        // count supplied only on the first layout, or a viewport that kept its
        // first answer, would go on announcing three rows for a list of four.
        // Round 404 found exactly that on the sliver side.
        tree.rebuild(leaf(|| {
            let mut list = ListView::new().with_offset(0.0);
            for index in 0..4 {
                list = list.push(crate::render::RenderRef::new(crate::widgets::Text::new(
                    format!("Row {index}"),
                )));
            }
            list
        }));
        let mut root = tree.build_render_tree().expect("mounted");
        RenderBox::layout(&mut root, BoxConstraints::tight(200.0, 400.0));
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(Size::new(200.0, 400.0), &root).unwrap_or_default();
        crate::semantics::set_enabled(false);

        let listed = nodes
            .iter()
            .find(|node| node.properties.scroll_child_count.is_some())
            .expect("no node said it was a list");
        assert_eq!(listed.properties.scroll_child_count, Some(4));

        for (label, index) in nodes
            .iter()
            .filter(|node| node.properties.label.starts_with("Row "))
            .map(|node| (node.properties.label.clone(), node.index_in_parent))
        {
            let expected: i32 = label.trim_start_matches("Row ").parse().expect("a number");
            assert_eq!(index, Some(expected), "{label} did not say which row it is");
        }
    }

    use super::*;
    use crate::framework::leaf;

    #[test]
    fn the_new_facades_build_the_render_object_they_name() {
        use crate::framework::ElementTree;
        use crate::render::{Alignment, Axis};

        // Each of these had its render object in the crate and no widget in
        // front of it, which is what a caller building a tree reaches for.
        let mut tree = ElementTree::new();
        tree.rebuild(clip_rect(leaf(|| SizedBox::new(10.0, 10.0))));
        assert_eq!(
            tree.build_render_tree()
                .expect("a root")
                .layout(BoxConstraints::loose(100.0, 100.0)),
            Size::new(10.0, 10.0)
        );

        let mut tree = ElementTree::new();
        tree.rebuild(flex(
            Axis::Horizontal,
            vec![
                leaf(|| SizedBox::new(10.0, 4.0)),
                leaf(|| SizedBox::new(20.0, 6.0)),
            ],
        ));
        // As tall as the tallest child, and -- like upstream's `Flex` -- as
        // wide as it is allowed to be, because the default main axis size is
        // max and not shrink-wrap. The height is what says the children
        // actually got in.
        assert_eq!(
            tree.build_render_tree()
                .expect("a root")
                .layout(BoxConstraints::loose(100.0, 100.0)),
            Size::new(100.0, 6.0)
        );

        let mut tree = ElementTree::new();
        tree.rebuild(animated_size(
            Alignment::CENTER,
            200,
            leaf(|| SizedBox::new(10.0, 10.0)),
        ));
        assert_eq!(
            tree.build_render_tree()
                .expect("a root")
                .layout(BoxConstraints::loose(100.0, 100.0)),
            Size::new(10.0, 10.0)
        );

        let mut tree = ElementTree::new();
        tree.rebuild(viewport(
            Axis::Vertical,
            0.0,
            leaf(|| SizedBox::new(10.0, 400.0)),
        ));
        // A viewport is the window, not the contents: it takes the height it
        // is offered and lets the child overflow behind it.
        assert_eq!(
            tree.build_render_tree()
                .expect("a root")
                .layout(BoxConstraints::tight(50.0, 60.0)),
            Size::new(50.0, 60.0)
        );
    }

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

    /// The bar is 300 wide and 56 tall, with a 60-wide trailing and a middle
    /// that would rather be 400 wide than fit.
    fn overfull_toolbar() -> RenderNavigationToolbar {
        RenderNavigationToolbar::new()
            .with_middle(FixedBox::new(400.0, 24.0))
            .with_trailing(FixedBox::new(60.0, 40.0))
    }

    #[test]
    fn a_title_too_long_for_the_bar_does_not_push_the_action_off_it() {
        let mut toolbar = overfull_toolbar();
        toolbar.layout(BoxConstraints::tight(300.0, 56.0));
        // Upstream pins the trailing at `size.width - trailingSize.width`, and
        // it is measured before the middle is told anything, so no title can
        // move it.
        assert_eq!(toolbar.offsets[2].dx, 240.0, "the action left the bar");
        assert_eq!(
            toolbar.offsets[2].dx + 60.0,
            300.0,
            "the action is not flush right"
        );
    }

    #[test]
    fn the_title_gets_what_is_left_after_the_action_and_the_spacing() {
        let mut toolbar = overfull_toolbar();
        toolbar.layout(BoxConstraints::tight(300.0, 56.0));
        // `size.width - leadingWidth - trailingWidth - middleSpacing * 2`
        // = 300 - 0 - 60 - 32.
        assert_eq!(toolbar.middle.as_ref().unwrap().size().width, 208.0);
        // `middleStart = leadingWidth + middleSpacing`.
        assert_eq!(toolbar.offsets[1].dx, K_MIDDLE_SPACING);
    }

    #[test]
    fn the_edges_are_vertically_centred_and_the_leading_fills_the_height() {
        // `centerMiddle: false`: the spaced-off-the-leading placement.
        let mut toolbar = RenderNavigationToolbar::new()
            .with_center_middle(false)
            .with_leading(FixedBox::new(40.0, 10.0))
            .with_middle(FixedBox::new(50.0, 24.0))
            .with_trailing(FixedBox::new(60.0, 40.0));
        toolbar.layout(BoxConstraints::tight(300.0, 56.0));
        // The leading is given `minHeight: size.height` -- "the height should
        // be exactly the height of the bar" -- so it does not get centred.
        assert_eq!(toolbar.leading.as_ref().unwrap().size().height, 56.0);
        assert_eq!(toolbar.offsets[0], Offset::new(0.0, 0.0));
        // `(size.height - trailingSize.height) / 2`.
        assert_eq!(toolbar.offsets[2].dy, 8.0);
        // A leading pushes the title along: `leadingWidth + middleSpacing`.
        assert_eq!(toolbar.offsets[1].dx, 40.0 + K_MIDDLE_SPACING);
    }

    #[test]
    fn a_centred_title_backs_off_when_it_would_reach_the_action() {
        // Room to centre: the middle is narrow, so `(300 - 50) / 2` stands.
        let mut roomy = RenderNavigationToolbar::new()
            .with_center_middle(true)
            .with_middle(FixedBox::new(50.0, 24.0))
            .with_trailing(FixedBox::new(60.0, 40.0));
        roomy.layout(BoxConstraints::tight(300.0, 56.0));
        assert_eq!(roomy.offsets[1].dx, 125.0);

        // No room: centring would run under the trailing, so upstream aligns
        // the middle's right edge with the trailing's left, less the spacing.
        let mut tight = RenderNavigationToolbar::new()
            .with_center_middle(true)
            .with_middle(FixedBox::new(200.0, 24.0))
            .with_trailing(FixedBox::new(60.0, 40.0));
        tight.layout(BoxConstraints::tight(300.0, 56.0));
        // The middle was clamped to 300 - 60 - 32 = 208, so it took 200; then
        // `size.width - trailingWidth - middleSize.width - middleSpacing`.
        assert_eq!(tight.offsets[1].dx, 300.0 - 60.0 - 200.0 - K_MIDDLE_SPACING);
    }

    #[test]
    fn a_toolbar_centres_its_middle_by_default() {
        // The constructor default is upstream's `this.centerMiddle = true`
        // (`widgets/navigation_toolbar.dart`), so a toolbar built with nothing
        // but a middle puts it in the centre of the bar: `(300 - 50) / 2`.
        let mut toolbar = RenderNavigationToolbar::new()
            .with_middle(FixedBox::new(50.0, 24.0))
            .with_trailing(FixedBox::new(60.0, 40.0));
        toolbar.layout(BoxConstraints::tight(300.0, 56.0));
        assert_eq!(toolbar.offsets[1].dx, 125.0);
    }

    #[test]
    fn the_leading_moves_to_the_far_edge_in_rtl() {
        // `_ToolbarLayout`'s `switch (textDirection)`: the leading widget is
        // placed at `size.width - leadingWidth` in rtl, at 0 in ltr. The
        // direction is read when the toolbar is built, the moment upstream's
        // `NavigationToolbar` hands it to the delegate.
        let rtl = crate::direction::with_direction(crate::direction::TextDirection::Rtl, || {
            RenderNavigationToolbar::new().with_leading(FixedBox::new(40.0, 10.0))
        });
        let mut toolbar = rtl;
        toolbar.layout(BoxConstraints::tight(300.0, 56.0));
        assert_eq!(toolbar.offsets[0], Offset::new(260.0, 0.0));
    }

    #[test]
    fn the_trailing_and_middle_mirror_in_rtl() {
        // The trailing goes to the near (left) edge, and the middle is placed
        // `size.width - middleSize.width - middleStart` -- mirrored against
        // the bar rather than measured from its left. `centerMiddle: false`
        // so `middleStart` is the plain leading-plus-spacing margin.
        let rtl = crate::direction::with_direction(crate::direction::TextDirection::Rtl, || {
            RenderNavigationToolbar::new()
                .with_center_middle(false)
                .with_middle(FixedBox::new(50.0, 24.0))
                .with_trailing(FixedBox::new(60.0, 40.0))
        });
        let mut toolbar = rtl;
        toolbar.layout(BoxConstraints::tight(300.0, 56.0));
        assert_eq!(toolbar.offsets[2].dx, 0.0);
        // `middleStart = leadingWidth + middleSpacing` = 16, mirrored:
        // 300 - 50 - 16.
        assert_eq!(toolbar.offsets[1].dx, 300.0 - 50.0 - K_MIDDLE_SPACING);
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
    fn a_childless_container_expands_to_fill_bounded_constraints() {
        // Upstream's childless `Container.build` is a `LimitedBox(0, 0)`
        // around a `ConstrainedBox(BoxConstraints.expand())`: everything on
        // offer when the constraints bound an axis, and nothing on one they
        // leave unbounded -- the `LimitedBox`'s clamp, which this port's
        // `BoxConstraints::biggest` already makes.
        let mut container = Container::new().with_color(Color::WHITE);
        let size = container.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(200.0, 200.0));

        let unbounded = BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);
        let size = container.layout(unbounded);
        assert_eq!(size, Size::ZERO);
    }

    #[test]
    fn a_border_insets_the_child_it_surrounds() {
        // `Container._paddingIncludingDecoration` folds the border's
        // dimensions into the padding, so the child is laid out inside the
        // border rather than underneath it: 20 + 4 on each side.
        let mut container = Container::new()
            .with_border(4.0, Color::BLACK)
            .with_child(FixedBox::new(20.0, 10.0));
        let size = container.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(28.0, 18.0));

        // The border and an explicit padding add, rather than one hiding the
        // other: 20 + (8 + 4) on each side.
        let mut container = Container::new()
            .with_padding(EdgeInsets::all(8.0))
            .with_border(4.0, Color::BLACK)
            .with_child(FixedBox::new(20.0, 10.0));
        let size = container.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(44.0, 34.0));
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
        // The upstream default is `MainAxisSize.max`, so a bounded height is
        // taken even though the children only need 50 of it.
        assert_eq!(size, Size::new(50.0, 200.0));
        assert_eq!(column.child_offsets()[0], Offset::new(10.0, 0.0));
        assert_eq!(column.child_offsets()[1], Offset::new(0.0, 30.0));
    }

    #[test]
    fn column_and_row_default_to_main_axis_size_max() {
        // Upstream `Flex` -- hence `Column` and `Row` -- defaults to
        // `MainAxisSize.max`: fill what the parent offers on the main axis.
        let mut column = Column::new().push(FixedBox::new(30.0, 20.0));
        let size = column.layout(BoxConstraints::loose(100.0, 80.0));
        assert_eq!(size, Size::new(30.0, 80.0));

        let mut row = Row::new().push(FixedBox::new(30.0, 20.0));
        let size = row.layout(BoxConstraints::loose(100.0, 80.0));
        assert_eq!(size, Size::new(100.0, 20.0));
    }

    #[test]
    fn a_max_column_degrades_to_its_content_in_an_unbounded_parent() {
        // Upstream `RenderFlex` picks `maxMainSize` only when it is finite;
        // against an unbounded main axis max degrades to the content extent
        // (its `_computeSizes` idealMainSize switch), which is why a column in
        // a vertical scroll view shrink-wraps without asking to.
        let mut column = Column::new()
            .with_spacing(10.0)
            .push(FixedBox::new(30.0, 20.0))
            .push(FixedBox::new(50.0, 20.0));
        let size = column.layout(BoxConstraints::new(0.0, 200.0, 0.0, f32::INFINITY));
        assert_eq!(size, Size::new(50.0, 50.0));
    }

    #[test]
    fn sized_box_gaps_take_only_one_axis() {
        let mut gap = SizedBox::height(12.0);
        let size = gap.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(0.0, 12.0));
    }

    #[test]
    fn a_gap_leaves_the_cross_axis_to_its_child() {
        // `tightFor(height: h)` tightens only the height axis: the width axis
        // keeps the min of zero and max of infinity it was offered, so a child
        // is not clamped to a zero-width strip.
        let mut gap = SizedBox::height(12.0).with_child(FixedBox::new(30.0, 20.0));
        let size = gap.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(30.0, 12.0));

        let mut gap = SizedBox::width(20.0).with_child(FixedBox::new(30.0, 20.0));
        let size = gap.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size, Size::new(20.0, 20.0));
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
            Counted {
                extent,
                laid_out: std::rc::Rc::clone(laid_out),
                size: Size::ZERO,
            }
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
        assert_eq!(
            container.layout(BoxConstraints::loose(200.0, 200.0)),
            Size::square(36.0)
        );
        assert_eq!(laid_out.get(), 1);

        let before: Vec<BoxedWidget> = container.layers.iter().map(|(_, h)| h.clone()).collect();
        assert_eq!(
            container.update_from(&mut describe()),
            Some(UpdateEffect::Nothing)
        );

        let after: Vec<BoxedWidget> = container.layers.iter().map(|(_, h)| h.clone()).collect();
        assert_eq!(
            before.len(),
            2,
            "a padded, decorated container is two wrappers"
        );
        assert!(
            before.iter().zip(&after).all(|(a, b)| a.is(b)),
            "the wrappers were replaced instead of told"
        );
        assert_eq!(
            container.layout(BoxConstraints::loose(200.0, 200.0)),
            Size::square(36.0)
        );
        assert_eq!(
            laid_out.get(),
            1,
            "a container that did not change measured again"
        );
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
        assert_eq!(
            container.layout(BoxConstraints::loose(200.0, 200.0)),
            Size::square(36.0)
        );

        let mut fresh = Container::new()
            .with_padding(EdgeInsets::all(4.0))
            .with_child(Counted::new(20.0, &laid_out));
        assert_eq!(
            container.update_from(&mut fresh),
            Some(UpdateEffect::Nothing)
        );
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
        assert_eq!(
            root.layout(BoxConstraints::loose(200.0, 200.0)),
            Size::square(40.0)
        );

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
        assert_eq!(
            container.layout(BoxConstraints::loose(200.0, 200.0)),
            Size::square(36.0)
        );

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
        assert_eq!(
            list.layout(BoxConstraints::tight(100.0, 60.0)),
            Size::new(100.0, 60.0)
        );
        assert_eq!((first.get(), second.get()), (1, 1));
        assert_eq!(list.max_scroll_extent(), 20.0);
        let column = list.flex.clone().expect("built by the layout");

        // A rebuild that handed back the same rows and a new scroll offset.
        let mut fresh = ListView::new()
            .with_offset(10.0)
            .push(a.clone())
            .push(b.clone());
        assert_eq!(list.update_from(&mut fresh), Some(UpdateEffect::Relayout));

        assert!(
            list.flex.as_ref().expect("kept").is(&column),
            "the column was replaced"
        );
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
        assert_eq!(
            list.layout(BoxConstraints::tight(100.0, 200.0)),
            Size::new(100.0, 200.0)
        );
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

    #[test]
    fn positioned_directional_in_rtl_anchors_start_to_the_right_edge() {
        use crate::direction::{TextDirection, with_direction};
        // `PositionedDirectional`'s build is two lines -- `left = rtl ? end :
        // start; right = rtl ? start : end` -- settled where the ambient
        // direction is still in force, because by layout the walk is over.
        let position = with_direction(TextDirection::Rtl, || PositionedDirectional::at(5.0, 5.0));
        assert_eq!(position.right, Some(5.0), "start is the right edge in rtl");
        assert_eq!(position.left, None);
        // And the stack honours it like any absolute anchor: 100 - 5 - 10.
        let mut stack = RenderStack::new()
            .push(FixedBox::new(100.0, 100.0))
            .push_positioned(FixedBox::new(10.0, 10.0), position);
        stack.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(stack.child_offsets()[1], Offset::new(85.0, 5.0));
    }

    #[test]
    fn a_horizontal_list_in_rtl_starts_at_the_right_edge() {
        use crate::direction::{TextDirection, with_direction};
        // `textDirectionToAxisDirection` says an rtl row scrolls leftwards,
        // and the rows inside it read right to left -- so the first row built
        // is the first thing seen, at the window's right edge.
        let mut list = with_direction(TextDirection::Rtl, || {
            ListView::horizontal()
                .push(FixedBox::new(60.0, 40.0))
                .push(FixedBox::new(60.0, 40.0))
                .push(FixedBox::new(60.0, 40.0))
        });
        let size = list.layout(BoxConstraints::tight(100.0, 40.0));
        assert_eq!(size, Size::new(100.0, 40.0));
        assert_eq!(list.max_scroll_extent(), 80.0);
        // The scroll offset puts the content's end against the window's right
        // edge...
        let mut scrolled = None;
        list.composed
            .as_ref()
            .expect("laid out")
            .visit_children(&mut |_, offset| scrolled = Some(offset));
        assert_eq!(scrolled, Some(Offset::new(-80.0, 0.0)));
        // ...and the rows inside it were laid out right to left: the first
        // ends at the content's right edge (180), the last starts at its left.
        let mut offsets = Vec::new();
        list.flex
            .as_ref()
            .expect("built with the viewport")
            .visit_children(&mut |_, offset| offsets.push(offset));
        assert_eq!(offsets[0].dx, 120.0);
        assert_eq!(offsets[1].dx, 60.0);
        assert_eq!(offsets[2].dx, 0.0);
    }

    #[test]
    fn a_wrap_places_its_lines_by_its_run_alignment() {
        // `Wrap::new()` hands back the render object, and the facade fronts
        // its builder chain: `runAlignment` reaches it without `new` changing
        // -- three chips of 40 in a 100-wide window, two lines of two.
        let mut wrap = Wrap::new()
            .with_run_alignment(MainAxisAlignment::Center)
            .push(FixedBox::new(40.0, 20.0))
            .push(FixedBox::new(40.0, 20.0))
            .push(FixedBox::new(40.0, 20.0));
        wrap.layout(BoxConstraints::tight(100.0, 100.0));
        // 40 of lines in a 100-tall wrap, so 60 free: half of it above.
        let offsets = wrap.child_offsets();
        assert_eq!(offsets[0].dy, 30.0);
        assert_eq!(offsets[2].dy, 50.0);

        // And a wrap that says nothing keeps stacking them from the top.
        let mut wrap = Wrap::new();
        for _ in 0..3 {
            wrap = wrap.push(FixedBox::new(40.0, 20.0));
        }
        wrap.layout(BoxConstraints::tight(100.0, 100.0));
        assert_eq!(wrap.child_offsets()[0].dy, 0.0);
    }

    // -- The rest of `basic.dart` ---------------------------------------------

    /// Where a laid-out box put its children, in paint order.
    fn offsets_of(render: &dyn RenderBox) -> Vec<Offset> {
        let mut seen = Vec::new();
        render.visit_children(&mut |_, offset| seen.push(offset));
        seen
    }

    #[test]
    fn a_list_body_runs_down_by_default_and_up_when_reversed() {
        let children = || -> Vec<BoxedRender> {
            vec![
                RenderRef::new(FixedBox::new(20.0, 10.0)),
                RenderRef::new(FixedBox::new(30.0, 10.0)),
            ]
        };
        // A list body wants an unbounded main axis and a tight cross one.
        let contract = BoxConstraints::new(50.0, 50.0, 0.0, f32::INFINITY);

        let mut body = ListBody::new(Axis::Vertical, children());
        body.layout(contract);
        assert_eq!(
            offsets_of(&body),
            vec![Offset::ZERO, Offset::new(0.0, 10.0)]
        );

        // Reversed is an axis direction, not a reordering: the first child is
        // laid at the far end.
        let mut reversed = ListBody::reversed(Axis::Vertical, children());
        reversed.layout(contract);
        assert_eq!(
            offsets_of(&reversed),
            vec![Offset::new(0.0, 10.0), Offset::ZERO]
        );
    }

    #[test]
    fn a_horizontal_list_body_in_rtl_runs_leftwards() {
        use crate::direction::{TextDirection, with_direction};
        let mut body = with_direction(TextDirection::Rtl, || {
            ListBody::new(
                Axis::Horizontal,
                vec![
                    RenderRef::new(FixedBox::new(20.0, 10.0)),
                    RenderRef::new(FixedBox::new(30.0, 10.0)),
                ],
            )
        });
        body.layout(BoxConstraints::new(0.0, f32::INFINITY, 30.0, 30.0));
        assert_eq!(body.size(), Size::new(50.0, 30.0));
        // Reading order: the first child sits at the right end of the 50 wide
        // body, the second to its left.
        assert_eq!(
            offsets_of(&body),
            vec![Offset::new(30.0, 0.0), Offset::ZERO]
        );
    }

    #[test]
    fn an_unconstrained_box_measures_its_child_without_the_constraints() {
        // Tight 50x50 would otherwise force the child to 50x50.
        let mut boxed = UnconstrainedBox::new(Alignment::CENTER, FixedBox::new(80.0, 20.0));
        let size = boxed.layout(BoxConstraints::tight_for(Size::new(50.0, 50.0)));
        assert_eq!(size, Size::new(50.0, 50.0), "the box itself is still tight");
        // Centred inside a box smaller than it is, so the offset is negative
        // on the axis it overflows.
        assert_eq!(offsets_of(&boxed), vec![Offset::new(-15.0, 15.0)]);
    }

    #[test]
    fn a_constrained_box_adds_its_own_constraints_to_the_incoming_ones() {
        let mut boxed = ConstrainedBox::new(
            BoxConstraints {
                min_width: 60.0,
                ..BoxConstraints::loose(200.0, 200.0)
            },
            FixedBox::new(10.0, 10.0),
        );
        assert_eq!(
            boxed.layout(BoxConstraints::loose(200.0, 200.0)),
            Size::new(60.0, 10.0)
        );
    }

    #[test]
    fn a_coloured_box_fills_what_it_is_given_when_it_has_no_child() {
        let mut filled = ColoredBox::filled(Color::WHITE);
        assert_eq!(
            filled.layout(BoxConstraints::tight_for(Size::new(80.0, 40.0))),
            Size::new(80.0, 40.0)
        );

        let mut around = ColoredBox::new(Color::WHITE, FixedBox::new(20.0, 10.0));
        assert_eq!(
            around.layout(BoxConstraints::loose(80.0, 40.0)),
            Size::new(20.0, 10.0),
            "with a child it takes the child's size"
        );
    }

    #[test]
    fn an_offstage_child_is_laid_out_and_takes_no_space() {
        let mut offstage = Offstage::new(true, FixedBox::new(20.0, 10.0));
        assert_eq!(
            offstage.layout(BoxConstraints::loose(80.0, 40.0)),
            Size::ZERO,
            "offstage takes no space"
        );

        let mut onstage = Offstage::new(false, FixedBox::new(20.0, 10.0));
        assert_eq!(
            onstage.layout(BoxConstraints::loose(80.0, 40.0)),
            Size::new(20.0, 10.0)
        );
    }

    #[test]
    fn a_rotated_box_swaps_the_axes_on_an_odd_turn() {
        let mut turned = RotatedBox::new(1, FixedBox::new(40.0, 10.0));
        assert_eq!(
            turned.layout(BoxConstraints::loose(100.0, 100.0)),
            Size::new(10.0, 40.0)
        );

        let mut twice = RotatedBox::new(2, FixedBox::new(40.0, 10.0));
        assert_eq!(
            twice.layout(BoxConstraints::loose(100.0, 100.0)),
            Size::new(40.0, 10.0),
            "an even turn keeps the axes"
        );
    }

    #[test]
    fn a_fractional_translation_moves_by_a_fraction_of_its_own_size() {
        let mut moved =
            FractionalTranslation::new(Offset::new(0.5, 0.0), FixedBox::new(40.0, 10.0));
        moved.layout(BoxConstraints::loose(100.0, 100.0));
        assert_eq!(offsets_of(&moved), vec![Offset::new(20.0, 0.0)]);
    }

    #[test]
    fn a_keyed_subtree_writes_the_key_onto_the_widget_itself() {
        use crate::framework::{ensure_unique_keys_for_list, keyed_subtree, leaf};

        // An unkeyed item takes its index; a keyed one keeps its own key.
        let items = ensure_unique_keys_for_list(
            vec![
                leaf(|| SizedBox::new(10.0, 10.0)),
                keyed_subtree(7, leaf(|| SizedBox::new(10.0, 10.0))),
            ],
            0,
        );
        assert_eq!(items[0].key(), Some(0));
        assert_eq!(items[1].key(), Some(7));

        // And a base index shifts the ones that had none.
        let shifted = ensure_unique_keys_for_list(vec![leaf(|| SizedBox::new(1.0, 1.0))], 100);
        assert_eq!(shifted[0].key(), Some(100));
    }

    #[test]
    fn a_stateful_builder_rebuilds_from_the_handle_it_was_given() {
        use crate::framework::{
            ElementTree, StateHandle, StatefulBuilderState, leaf, stateful_builder,
        };
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        let width = Rc::new(Cell::new(10.0_f32));
        let builds = Rc::new(Cell::new(0));
        let held: Rc<RefCell<Option<StateHandle<StatefulBuilderState>>>> =
            Rc::new(RefCell::new(None));

        let (w, b, h) = (Rc::clone(&width), Rc::clone(&builds), Rc::clone(&held));
        let mut tree = ElementTree::new();
        tree.rebuild(stateful_builder(move |handle| {
            b.set(b.get() + 1);
            // The handle is the `setState` upstream hands the builder.
            *h.borrow_mut() = Some(handle);
            let width = w.get();
            leaf(move || SizedBox::new(width, 10.0))
        }));
        assert_eq!(builds.get(), 1);

        width.set(20.0);
        assert!(
            held.borrow()
                .as_ref()
                .expect("built once")
                .set_state(|_| {}),
            "the handle is live"
        );
        tree.rebuild_dirty();
        assert_eq!(builds.get(), 2, "the handle asked for the rebuild");
    }
}

#[cfg(test)]
mod spoken_text_tests {
    use super::*;
    use crate::painting::InlineSpanSemanticsInformation;

    fn body() -> TextStyle {
        TextStyle::default()
    }

    // -- Two strings from one paragraph ----------------------------------------

    #[test]
    fn a_paragraph_is_painted_as_one_thing_and_heard_as_another() {
        // Upstream's own example. `$$` is what the glyphs are; "Double
        // dollars" is what the words are, and a reader given the glyphs says
        // "dollar dollar".
        let paragraph = RenderParagraph::rich_spans(vec![
            TextSpan::new("Costs ", body()),
            TextSpan::new("$$", body()).spoken_as("Double dollars"),
        ]);
        assert_eq!(paragraph.content(), "Costs $$");
        assert_eq!(paragraph.spoken(), "Costs Double dollars");
    }

    #[test]
    fn and_a_paragraph_nobody_relabelled_carries_no_second_string() {
        // Built only when some span asks, so the ordinary case costs nothing.
        let paragraph = RenderParagraph::rich_spans(vec![
            TextSpan::new("Hold ", body()),
            TextSpan::bold("Shift", &body()),
        ]);
        assert_eq!(paragraph.content(), "Hold Shift");
        assert_eq!(paragraph.spoken(), "Hold Shift");
        assert!(!paragraph.has_separate_spoken_text());
    }

    #[test]
    fn one_relabelled_span_relabels_only_itself() {
        let paragraph = RenderParagraph::rich_spans(vec![
            TextSpan::new("a", body()).spoken_as("alpha"),
            TextSpan::new("b", body()),
            TextSpan::new("c", body()).spoken_as("charlie"),
        ]);
        assert_eq!(paragraph.content(), "abc");
        assert_eq!(paragraph.spoken(), "alphabcharlie");
    }

    #[test]
    fn a_span_speaks_its_text_where_it_has_no_label() {
        assert_eq!(TextSpan::new("plain", body()).semantics_text(), "plain");
        assert_eq!(
            TextSpan::new("$$", body())
                .spoken_as("dollars")
                .semantics_text(),
            "dollars"
        );
    }

    #[test]
    fn you_cannot_rename_nothing() {
        // Upstream's `assert(!(text == null && semanticsLabel != null))`. A
        // label with no text under it is not a label of anything.
        assert!(TextSpan::new("", body()).check().is_ok());
        assert!(TextSpan::new("x", body()).spoken_as("ex").check().is_ok());
        assert!(TextSpan::new("", body()).spoken_as("ex").check().is_err());
    }

    // -- When a span becomes its own node --------------------------------------

    #[test]
    fn a_label_does_not_split_the_run_and_being_reachable_does() {
        // Renaming a stretch of a sentence leaves it one sentence; making a
        // stretch of it tappable does not.
        let renamed = InlineSpanSemanticsInformation::text("$$").spoken_as("Double dollars");
        assert!(!renamed.requires_own_node());
        assert_eq!(renamed.spoken(), "Double dollars");

        assert!(
            InlineSpanSemanticsInformation::text("terms")
                .with_recognizer()
                .requires_own_node()
        );
        assert!(
            InlineSpanSemanticsInformation::text("terms")
                .with_identifier("tos")
                .requires_own_node()
        );
        assert!(InlineSpanSemanticsInformation::placeholder().requires_own_node());
    }

    #[test]
    fn plain_text_is_not_its_own_node() {
        // Or every word would be a separate stop for a reader.
        assert!(!InlineSpanSemanticsInformation::text("ordinary").requires_own_node());
    }

    #[test]
    fn a_placeholder_is_exactly_one_character() {
        let placeholder = InlineSpanSemanticsInformation::placeholder();
        assert_eq!(placeholder.text.chars().count(), 1);
        assert_eq!(
            placeholder.text.chars().next(),
            Some(InlineSpanSemanticsInformation::PLACEHOLDER_CHARACTER)
        );
        assert!(placeholder.check().is_ok());

        let mut wrong = InlineSpanSemanticsInformation::placeholder();
        wrong.text = String::from("an image");
        assert!(wrong.check().is_err());

        let mut two = InlineSpanSemanticsInformation::placeholder();
        two.text
            .push(InlineSpanSemanticsInformation::PLACEHOLDER_CHARACTER);
        assert!(two.check().is_err(), "two of them is not one of them");
    }

    #[test]
    fn and_says_nothing_of_its_own() {
        // The widget in that slot brings its own semantics; a second label
        // over the top would be the text layer talking about something it
        // cannot see.
        assert!(
            InlineSpanSemanticsInformation::placeholder()
                .spoken_as("a picture")
                .check()
                .is_err()
        );
        assert!(
            InlineSpanSemanticsInformation::placeholder()
                .with_recognizer()
                .check()
                .is_err()
        );
    }

    #[test]
    fn but_ordinary_text_may_carry_both_of_those() {
        // The rule is the placeholder's, not everyone's.
        assert!(
            InlineSpanSemanticsInformation::text("terms")
                .spoken_as("terms of service")
                .with_recognizer()
                .check()
                .is_ok()
        );
    }
}
