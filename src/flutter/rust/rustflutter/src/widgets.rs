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
//! the work. That third layer arrives with M6; until then a widget *is* its
//! render object, so a tree is built fresh each frame.

use crate::engine::{Color, TextAlign};
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
    RenderPointerRegion, RenderStack, RenderTransform, RenderViewport, RenderWrap,
};

/// A widget with its concrete type erased, which is what a `build` method
/// returns and what a parent stores for a child.
pub type BoxedWidget = BoxedRender;

// -- Text ---------------------------------------------------------------------

/// A run of text, shaped by the engine's `txt` / skparagraph stack.
pub type Text = RenderParagraph;

impl RenderParagraph {
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

/// A box that paints a background, pads, sizes and aligns a child.
///
/// Upstream `Container` is famously a composition rather than a render object,
/// and it is the same here: whichever of padding, decoration, sizing and
/// alignment were asked for get layered, and the ones that were not cost
/// nothing.
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
        self.child = Some(Box::new(child));
        self
    }

    /// Builds the render tree this container describes, innermost first.
    fn compose(&mut self) -> BoxedWidget {
        let mut current: Option<BoxedWidget> = self.child.take();

        if let Some(alignment) = self.alignment {
            let inner = current.take().unwrap_or_else(|| Box::new(Empty));
            current = Some(Box::new(RenderAlign::new(alignment, inner)));
        }

        if self.padding != EdgeInsets::ZERO {
            let inner = current.take().unwrap_or_else(|| Box::new(Empty));
            current = Some(Box::new(RenderPadding::new(self.padding, inner)));
        }

        let has_decoration =
            self.fill.is_some() || self.border_width > 0.0 || !self.shadows.is_empty();
        if has_decoration {
            let mut decorated = RenderDecoratedBox::new()
                .with_corner_radius(self.corner_radius)
                .with_shadows(std::mem::take(&mut self.shadows))
                .with_border(self.border_width, self.border_color);
            if let Some(fill) = self.fill.clone() {
                decorated = decorated.with_fill(fill);
            }
            if let Some(inner) = current.take() {
                decorated = decorated.with_child(inner);
            }
            current = Some(Box::new(decorated));
        }

        if self.width.is_some() || self.height.is_some() {
            let extra = BoxConstraints::new(
                self.width.unwrap_or(0.0),
                self.width.unwrap_or(f32::INFINITY),
                self.height.unwrap_or(0.0),
                self.height.unwrap_or(f32::INFINITY),
            );
            let mut sized = RenderConstrainedBox::new(extra);
            if let Some(inner) = current.take() {
                sized = sized.with_child(inner);
            }
            current = Some(Box::new(sized));
        }

        let mut result = current.unwrap_or_else(|| Box::new(Empty));
        if self.margin != EdgeInsets::ZERO {
            result = Box::new(RenderPadding::new(self.margin, result));
        }
        result
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderBox for Container {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        if self.composed.is_none() {
            self.composed = Some(self.compose());
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
        self.children.push(Box::new(child));
        self
    }

    /// How far this list can still scroll. Zero until it has been laid out.
    pub fn max_scroll_extent(&self) -> f32 {
        self.composed.as_ref().map_or(0.0, |v| v.max_scroll_extent())
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
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        if self.composed.is_none() {
            let mut flex = RenderFlex::new(self.axis)
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(self.spacing);
            // Computed here rather than at construction because it depends on
            // the size being handed down, which the caller does not know.
            let inset = self.centred_item.and_then(|extent| {
                let available = match self.axis {
                    Axis::Horizontal => constraints.max_width,
                    Axis::Vertical => constraints.max_height,
                };
                // Unbounded means there is no middle to sit in.
                (available.is_finite() && available > extent)
                    .then(|| (available - extent) / 2.0)
            });
            if let Some(inset) = inset {
                flex = flex.push(spacer(self.axis, inset));
            }
            for child in self.children.drain(..) {
                flex = flex.push(child);
            }
            if let Some(inset) = inset {
                flex = flex.push(spacer(self.axis, inset));
            }
            self.composed =
                Some(RenderViewport::new(self.axis, flex).with_offset(self.offset));
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
}
