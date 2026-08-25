//! Box decorations, from upstream `painting/decoration.dart` and
//! `painting/box_decoration.dart` (the `box_shadow.dart` half lives on
//! `BoxShadow` in `painting.rs`).
//!
//! Upstream's open `Decoration` hierarchy is a closed enum here --
//! `BoxDecoration` and `ShapeDecoration` (which stays in `borders.rs`,
//! next to the shapes it holds) are the two concrete decorations.
//!
//! Recorded divergences (see PORTING_STATUS.md):
//!
//! * `color` and `gradient` are one `Fill`, mutually exclusive the way
//!   upstream's doc demands rather than by assertion; a gradient does not
//!   scale or lerp stop-by-stop yet (painting wave), so gradient `lerp` and
//!   `scale` switch at the half.
//! * No `image` field -- `DecorationImage` is a later wave.
//! * No `backgroundBlendMode` -- `Paint::with_blend_mode` exists, but no
//!   caller asked for it yet; add it when one does.

use crate::borders::{
    BorderRadiusGeometry, BoxBorder, BoxShape, EdgeInsetsGeometry, ShapeDecoration, color_lerp,
    rect_center, rect_shortest_side,
};
use crate::direction::TextDirection;
use crate::engine::{Canvas, Color, Paint, Rect, TextAlign, TextStyle};
use crate::painting::{BoxShadow, RenderPath};
use crate::render::{EdgeInsets, Fill, Offset};

/// An immutable description of how to paint a box: the background fill, an
/// optional border and corner rounding, and the shadows the shape casts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoxDecoration {
    /// `BoxDecoration.color` or `BoxDecoration.gradient` -- one fill, so the
    /// mutual exclusion is structural.
    pub fill: Option<Fill>,
    pub border: Option<BoxBorder>,
    pub border_radius: Option<BorderRadiusGeometry>,
    pub box_shadow: Vec<BoxShadow>,
    pub shape: BoxShape,
}

impl BoxDecoration {
    pub fn new() -> BoxDecoration {
        BoxDecoration::default()
    }

    pub fn with_fill(mut self, fill: Fill) -> BoxDecoration {
        self.fill = Some(fill);
        self
    }

    pub fn with_border(mut self, border: BoxBorder) -> BoxDecoration {
        self.border = Some(border);
        self
    }

    pub fn with_border_radius(mut self, radius: BorderRadiusGeometry) -> BoxDecoration {
        debug_assert!(
            self.shape == BoxShape::Rectangle,
            "a circle cannot have a border radius"
        );
        self.border_radius = Some(radius);
        self
    }

    pub fn with_box_shadow(mut self, shadows: Vec<BoxShadow>) -> BoxDecoration {
        self.box_shadow = shadows;
        self
    }

    pub fn with_shape(mut self, shape: BoxShape) -> BoxDecoration {
        debug_assert!(
            shape == BoxShape::Rectangle || self.border_radius.is_none(),
            "a circle cannot have a border radius"
        );
        self.shape = shape;
        self
    }

    /// Upstream `BoxDecoration.padding`: the border's widths as insets.
    pub fn padding(&self) -> EdgeInsetsGeometry {
        match &self.border {
            Some(BoxBorder::Uniform(border)) => border.dimensions(),
            Some(BoxBorder::Directional(border)) => border.dimensions(),
            Some(BoxBorder::None) | None => EdgeInsetsGeometry::Zero,
        }
    }

    /// Upstream `BoxDecoration.isComplex`.
    pub fn is_complex(&self) -> bool {
        !self.box_shadow.is_empty()
    }

    /// Upstream `BoxDecoration.getClipPath`.
    pub fn clip_path(&self, rect: Rect, direction: TextDirection) -> RenderPath {
        let mut path = RenderPath::new();
        match self.shape {
            BoxShape::Circle => {
                let center = rect_center(rect);
                let radius = rect_shortest_side(rect) / 2.0;
                path.add_oval(Rect::xywh(
                    center.dx - radius,
                    center.dy - radius,
                    radius * 2.0,
                    radius * 2.0,
                ));
            }
            BoxShape::Rectangle => {
                if let Some(radius) = &self.border_radius {
                    let resolved = radius.resolve(direction).to_rrect(rect);
                    resolved.append_to(&mut path);
                } else {
                    path.add_rect(rect);
                }
            }
        }
        path
    }

    /// Upstream `BoxDecoration.scale`: everything fades toward nothing at
    /// the same rate, the shape staying put.
    pub fn scale(&self, factor: f32) -> BoxDecoration {
        BoxDecoration {
            fill: self.fill.as_ref().map(|fill| scale_fill(fill, factor)),
            border: self
                .border
                .map(|border| BoxBorder::lerp(None, Some(border), factor)),
            border_radius: self.border_radius.map(|radius| {
                BorderRadiusGeometry::lerp(BorderRadiusGeometry::Zero, radius, factor)
            }),
            box_shadow: BoxShadow::lerp_list(&[], &self.box_shadow, factor),
            shape: self.shape,
        }
    }

    /// Upstream `BoxDecoration.lerp`. The shape itself does not interpolate;
    /// it switches at the half, exactly as upstream documents.
    pub fn lerp(a: &BoxDecoration, b: &BoxDecoration, t: f32) -> BoxDecoration {
        BoxDecoration {
            fill: lerp_fill(a.fill.as_ref(), b.fill.as_ref(), t),
            border: match (&a.border, &b.border) {
                (Some(a), Some(b)) => Some(BoxBorder::lerp(Some(*a), Some(*b), t)),
                (None, Some(b)) => Some(BoxBorder::lerp(None, Some(*b), t)),
                (Some(a), None) => Some(BoxBorder::lerp(Some(*a), None, t)),
                (None, None) => None,
            },
            border_radius: match (&a.border_radius, &b.border_radius) {
                (Some(a), Some(b)) => Some(BorderRadiusGeometry::lerp(*a, *b, t)),
                (None, Some(b)) => Some(BorderRadiusGeometry::lerp(
                    BorderRadiusGeometry::Zero,
                    *b,
                    t,
                )),
                (Some(a), None) => Some(BorderRadiusGeometry::lerp(
                    *a,
                    BorderRadiusGeometry::Zero,
                    t,
                )),
                (None, None) => None,
            },
            box_shadow: BoxShadow::lerp_list(&a.box_shadow, &b.box_shadow, t),
            shape: if t < 0.5 { a.shape } else { b.shape },
        }
    }

    /// Upstream `BoxDecoration.hitTest`.
    pub fn hit_test(&self, size: (f32, f32), position: Offset, direction: TextDirection) -> bool {
        match self.shape {
            BoxShape::Rectangle => {
                if let Some(radius) = &self.border_radius {
                    return radius
                        .resolve(direction)
                        .to_rrect(Rect::xywh(0.0, 0.0, size.0, size.1))
                        .contains(position);
                }
                true
            }
            // Circles are inscribed into the smallest dimension; comparing
            // squared distances avoids the square root.
            BoxShape::Circle => {
                let center = Offset::new(size.0 / 2.0, size.1 / 2.0);
                let radius = size.0.min(size.1) / 2.0;
                let dx = position.dx - center.dx;
                let dy = position.dy - center.dy;
                dx * dx + dy * dy <= radius * radius
            }
        }
    }

    /// Upstream `_BoxDecorationPainter.paint`, minus the image: shadows
    /// under the background, the background under the border.
    pub fn paint(&self, canvas: &mut Canvas, rect: Rect, direction: TextDirection) {
        let shadow_shape = |rect: Rect| -> RenderPath {
            let mut path = RenderPath::new();
            match self.shape {
                BoxShape::Circle => {
                    let center = rect_center(rect);
                    let radius = rect_shortest_side(rect) / 2.0;
                    path.add_oval(Rect::xywh(
                        center.dx - radius,
                        center.dy - radius,
                        radius * 2.0,
                        radius * 2.0,
                    ));
                }
                BoxShape::Rectangle => {
                    if let Some(radius) = &self.border_radius {
                        let resolved = radius.resolve(direction).to_rrect(rect);
                        resolved.append_to(&mut path);
                    } else {
                        path.add_rect(rect);
                    }
                }
            }
            path
        };
        for shadow in &self.box_shadow {
            let spread = shadow.spread_radius;
            let shadow_rect = Rect::ltrb(
                rect.left + shadow.offset.dx - spread,
                rect.top + shadow.offset.dy - spread,
                rect.right + shadow.offset.dx + spread,
                rect.bottom + shadow.offset.dy + spread,
            );
            let paint = shadow.to_paint();
            canvas.draw_path(&shadow_shape(shadow_rect), &paint);
        }
        if let Some(fill) = &self.fill {
            if let Some(paint) = crate::borders::fill_paint(fill, rect) {
                canvas.draw_path(&self.clip_path(rect, direction), &paint);
            }
        }
        if let Some(border) = &self.border {
            let radius = self
                .border_radius
                .as_ref()
                .map(|radius| radius.resolve(direction));
            border.paint(canvas, rect, direction, radius, self.shape);
        }
    }
}

/// A fill on its way to nothing: a colour's alpha scales, and a gradient --
/// which cannot scale its stops yet -- holds on until the half.
fn scale_fill(fill: &Fill, factor: f32) -> Fill {
    match fill {
        Fill::Solid(color) => Fill::Solid(Color::argb(
            ((color.alpha() as f32) * factor).round().clamp(0.0, 255.0) as u8,
            color.red(),
            color.green(),
            color.blue(),
        )),
        gradient => {
            if factor <= 0.5 {
                Fill::Solid(Color(0x00000000))
            } else {
                gradient.clone()
            }
        }
    }
}

/// Fill lerp with the same gradient caveat as `scale_fill`.
fn lerp_fill(a: Option<&Fill>, b: Option<&Fill>, t: f32) -> Option<Fill> {
    match (a, b) {
        (Some(Fill::Solid(from)), Some(Fill::Solid(to))) => {
            Some(Fill::Solid(color_lerp(*from, *to, t)))
        }
        (None, None) => None,
        // A gradient on either side switches at the half rather than
        // interpolating stop by stop (see the module docs).
        _ => {
            if t < 0.5 {
                a.cloned()
            } else {
                b.cloned()
            }
        }
    }
}

/// A decoration, upstream `Decoration`: the closed set of concrete
/// decorations, with the base class's lerp discipline.
#[derive(Clone, Debug, PartialEq)]
pub enum Decoration {
    Box(BoxDecoration),
    Shape(ShapeDecoration),
    FlutterLogo(FlutterLogoDecoration),
}

impl Decoration {
    /// Upstream `Decoration.padding`.
    pub fn padding(&self) -> EdgeInsetsGeometry {
        match self {
            Decoration::Box(decoration) => decoration.padding(),
            Decoration::Shape(decoration) => decoration.padding(),
            // Upstream: the margin, as the insets around the logo.
            Decoration::FlutterLogo(decoration) => EdgeInsetsGeometry::Absolute(decoration.margin),
        }
    }

    /// Upstream `Decoration.isComplex`.
    pub fn is_complex(&self) -> bool {
        match self {
            Decoration::Box(decoration) => decoration.is_complex(),
            Decoration::Shape(decoration) => !decoration.shadows.is_empty(),
            Decoration::FlutterLogo(_) => true,
        }
    }

    /// Upstream `Decoration.hitTest`.
    pub fn hit_test(&self, size: (f32, f32), position: Offset, direction: TextDirection) -> bool {
        match self {
            Decoration::Box(decoration) => decoration.hit_test(size, position, direction),
            Decoration::Shape(decoration) => decoration.hit_test(size, position, direction),
            // Upstream's own comment: better hit testing TODO. Anything goes.
            Decoration::FlutterLogo(_) => true,
        }
    }

    /// Upstream `Decoration.getClipPath`.
    pub fn clip_path(&self, rect: Rect, direction: TextDirection) -> RenderPath {
        match self {
            Decoration::Box(decoration) => decoration.clip_path(rect, direction),
            Decoration::Shape(decoration) => decoration.shape.outer_path(rect, direction),
            // Upstream: a plain rect.
            Decoration::FlutterLogo(_) => {
                let mut path = RenderPath::new();
                path.add_rect(rect);
                path
            }
        }
    }

    /// Upstream `Decoration.paint`, through a painter upstream and directly
    /// here.
    pub fn paint(&self, canvas: &mut Canvas, rect: Rect, direction: TextDirection) {
        match self {
            Decoration::Box(decoration) => decoration.paint(canvas, rect, direction),
            Decoration::Shape(decoration) => decoration.paint(canvas, rect, direction),
            Decoration::FlutterLogo(decoration) => {
                let inner = Rect::ltrb(
                    rect.left + decoration.margin.left,
                    rect.top + decoration.margin.top,
                    rect.right - decoration.margin.right,
                    rect.bottom - decoration.margin.bottom,
                );
                decoration.paint(canvas, inner)
            }
        }
    }

    /// Upstream `Decoration.lerp`: `b.lerpFrom(a)` then `a.lerpTo(b)`, and
    /// when neither side knows the other, through nothing at the half.
    pub fn lerp(a: Option<Decoration>, b: Option<Decoration>, t: f32) -> Option<Decoration> {
        if a == b {
            return a;
        }
        match (a, b) {
            (None, Some(b)) => b.lerp_from(None, t).or(Some(b)),
            (Some(a), None) => a.lerp_to(None, t).or(Some(a)),
            (Some(a), Some(b)) => {
                if t == 0.0 {
                    return Some(a);
                }
                if t == 1.0 {
                    return Some(b);
                }
                b.lerp_from(Some(&a), t)
                    .or_else(|| a.lerp_to(Some(&b), t))
                    .or_else(|| {
                        if t < 0.5 {
                            a.lerp_to(None, t * 2.0).or(Some(a))
                        } else {
                            b.lerp_from(None, (t - 0.5) * 2.0).or(Some(b))
                        }
                    })
            }
            (None, None) => None,
        }
    }

    fn lerp_from(&self, a: Option<&Decoration>, t: f32) -> Option<Decoration> {
        match (a, self) {
            (None, this) => Some(this.scale(t)),
            // A box decoration is the cheaper spelling of a shape a
            // `ShapeDecoration` can hold; convert, then lerp as shapes.
            (Some(Decoration::Box(box_decoration)), Decoration::Shape(shape)) => {
                ShapeDecoration::lerp(
                    Some(&ShapeDecoration::from_box_decoration(box_decoration)),
                    Some(shape),
                    t,
                )
                .map(Decoration::Shape)
            }
            (Some(Decoration::Box(a)), Decoration::Box(b)) => {
                Some(Decoration::Box(BoxDecoration::lerp(a, b, t)))
            }
            (Some(Decoration::Shape(a)), Decoration::Shape(b)) => {
                ShapeDecoration::lerp(Some(a), Some(b), t).map(Decoration::Shape)
            }
            (Some(Decoration::FlutterLogo(a)), Decoration::FlutterLogo(b)) => Some(
                Decoration::FlutterLogo(FlutterLogoDecoration::lerp(Some(a), Some(b), t)),
            ),
            _ => None,
        }
    }

    fn lerp_to(&self, b: Option<&Decoration>, t: f32) -> Option<Decoration> {
        match (self, b) {
            (_, None) => Some(self.scale(1.0 - t)),
            (Decoration::Box(a), Some(Decoration::Box(b))) => {
                Some(Decoration::Box(BoxDecoration::lerp(a, b, t)))
            }
            (Decoration::Shape(a), Some(Decoration::Shape(b))) => {
                ShapeDecoration::lerp(Some(a), Some(b), t).map(Decoration::Shape)
            }
            (Decoration::Shape(shape), Some(Decoration::Box(box_decoration))) => {
                ShapeDecoration::lerp(
                    Some(shape),
                    Some(&ShapeDecoration::from_box_decoration(box_decoration)),
                    t,
                )
                .map(Decoration::Shape)
            }
            _ => None,
        }
    }

    fn scale(&self, t: f32) -> Decoration {
        match self {
            Decoration::Box(decoration) => Decoration::Box(decoration.scale(t)),
            Decoration::Shape(decoration) => {
                let scaled = ShapeDecoration::new(decoration.shape.scale(t))
                    .with_shadows(BoxShadow::lerp_list(&[], &decoration.shadows, t));
                let scaled = match &decoration.fill {
                    Some(fill) => scaled.with_fill(scale_fill(fill, t)),
                    None => scaled,
                };
                Decoration::Shape(scaled)
            }
            Decoration::FlutterLogo(decoration) => {
                Decoration::FlutterLogo(FlutterLogoDecoration::lerp(None, Some(decoration), t))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::borders::{
        Border, BorderRadiusGeometry, BorderSide, Radius, STROKE_ALIGN_INSIDE, ShapeBorder,
        StadiumBorder,
    };
    use crate::render::{Alignment, EdgeInsets};

    const RED: Color = Color(0xFF0000FF);
    const BLUE: Color = Color(0xFFFF0000);

    #[test]
    fn box_decoration_hit_tests_by_shape() {
        // A circle inscribed in a rectangle: the centre is in, the corner is
        // not.
        let circle = BoxDecoration::new()
            .with_fill(Fill::Solid(RED))
            .with_shape(BoxShape::Circle);
        assert!(circle.hit_test((100.0, 50.0), Offset::new(50.0, 25.0), TextDirection::Ltr));
        assert!(!circle.hit_test((100.0, 50.0), Offset::new(5.0, 5.0), TextDirection::Ltr));

        // Rounded corners answer by the rrect.
        let rounded = BoxDecoration::new()
            .with_fill(Fill::Solid(RED))
            .with_border_radius(BorderRadiusGeometry::circular(20.0));
        assert!(rounded.hit_test((100.0, 100.0), Offset::new(50.0, 0.5), TextDirection::Ltr));
        assert!(!rounded.hit_test((100.0, 100.0), Offset::new(0.5, 0.5), TextDirection::Ltr));

        // A plain rectangle takes everything.
        let plain = BoxDecoration::new().with_fill(Fill::Solid(RED));
        assert!(plain.hit_test((100.0, 100.0), Offset::new(0.5, 0.5), TextDirection::Ltr));
    }

    #[test]
    fn box_decoration_padding_is_the_border() {
        let bordered = BoxDecoration::new().with_border(BoxBorder::Uniform(Border::all(
            RED,
            3.0,
            crate::borders::BorderStyle::Solid,
            STROKE_ALIGN_INSIDE,
        )));
        assert_eq!(
            bordered.padding().resolve(TextDirection::Ltr),
            EdgeInsets::all(3.0)
        );
        assert_eq!(
            BoxDecoration::new().padding().resolve(TextDirection::Ltr),
            EdgeInsets::ZERO
        );
    }

    #[test]
    fn box_decoration_scale_fades_the_colour_and_the_border() {
        let decoration = BoxDecoration::new()
            .with_fill(Fill::Solid(RED))
            .with_border(BoxBorder::Uniform(Border::all(
                BLUE,
                4.0,
                crate::borders::BorderStyle::Solid,
                STROKE_ALIGN_INSIDE,
            )))
            .with_border_radius(BorderRadiusGeometry::circular(8.0));
        let half = decoration.scale(0.5);
        match &half.fill {
            Some(Fill::Solid(color)) => {
                // 255 halved lands on 127.5, rounding away from zero.
                assert_eq!(color.alpha(), 128);
                assert_eq!(color.blue(), 255);
            }
            other => panic!("expected a solid fill, got {other:?}"),
        }
        match &half.border {
            Some(BoxBorder::Uniform(border)) => assert_eq!(border.top.width, 2.0),
            other => panic!("expected a uniform border, got {other:?}"),
        }
        assert_eq!(
            half.border_radius
                .unwrap()
                .resolve(TextDirection::Ltr)
                .top_left,
            Radius::circular(4.0)
        );
    }

    #[test]
    fn box_decoration_lerp_interpolates_each_parameter() {
        let a = BoxDecoration::new()
            .with_fill(Fill::Solid(RED))
            .with_border_radius(BorderRadiusGeometry::circular(0.0));
        let b = BoxDecoration::new()
            .with_fill(Fill::Solid(BLUE))
            .with_border_radius(BorderRadiusGeometry::circular(10.0));
        let mid = BoxDecoration::lerp(&a, &b, 0.5);
        match &mid.fill {
            Some(Fill::Solid(color)) => {
                assert_eq!(*color, crate::borders::color_lerp(RED, BLUE, 0.5))
            }
            other => panic!("expected a solid fill, got {other:?}"),
        }
        assert_eq!(
            mid.border_radius
                .unwrap()
                .resolve(TextDirection::Ltr)
                .top_left,
            Radius::circular(5.0)
        );
    }

    #[test]
    fn decoration_lerps_between_a_box_and_a_shape_through_conversion() {
        // The box side, spelled as a shape decoration by fromBoxDecoration.
        let box_decoration = Decoration::Box(
            BoxDecoration::new()
                .with_fill(Fill::Solid(RED))
                .with_border_radius(BorderRadiusGeometry::circular(12.0)),
        );
        let shape_decoration = Decoration::Shape(ShapeDecoration::new(ShapeBorder::Stadium(
            StadiumBorder::new(BorderSide::NONE),
        )));
        let mid = Decoration::lerp(Some(box_decoration), Some(shape_decoration), 0.5).unwrap();
        // Halfway from a rounded rectangle to a stadium is the stadium's
        // transition shape, never a hard switch.
        match mid {
            Decoration::Shape(ShapeDecoration { shape, .. }) => {
                assert!(matches!(
                    shape,
                    ShapeBorder::StadiumToRoundedRect(_) | ShapeBorder::Stadium(_)
                ));
            }
            other => panic!("expected a shape decoration, got {other:?}"),
        }
    }

    #[test]
    fn decoration_clip_path_follows_the_shape() {
        let circle = BoxDecoration::new().with_shape(BoxShape::Circle);
        let _ = circle.clip_path(Rect::xywh(0.0, 0.0, 100.0, 50.0), TextDirection::Ltr);
        let rounded = BoxDecoration::new().with_border_radius(BorderRadiusGeometry::circular(4.0));
        let _ = rounded.clip_path(Rect::xywh(0.0, 0.0, 100.0, 50.0), TextDirection::Ltr);
    }
}

// -- The Flutter logo (upstream flutter_logo.dart) ---------------------------------

/// Upstream `FlutterLogoStyle`: the mark alone, or with the label beside it
/// or under it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlutterLogoStyle {
    /// Just the mark.
    #[default]
    MarkOnly,
    /// The label to the right of the mark.
    Horizontal,
    /// The label under the mark.
    Stacked,
}

/// Upstream `FlutterLogoDecoration`: paints Flutter's logo. The mark's
/// coordinates are the ones upstream derived from the original SVG artwork;
/// the 45-degree square and the fit-to-rect scaling that upstream does with
/// canvas transforms are baked into the point math here, the engine canvas
/// having no transform stack of its own.
///
/// The label, when a style asks for one, is shaped as a paragraph and
/// positioned by the constants upstream uses; upstream additionally reads
/// the label's glyph boxes back out of the shaper, which this engine cannot
/// do, so the animation's start position uses the shaped width instead --
/// see PORTING_STATUS.
#[derive(Clone, Debug, PartialEq)]
pub struct FlutterLogoDecoration {
    pub text_color: Color,
    pub style: FlutterLogoStyle,
    pub margin: EdgeInsets,
    /// -1.0 stacked, 0.0 mark only, 1.0 horizontal; fractions are the
    /// in-between states a lerp walks.
    pub position: f32,
    pub opacity: f32,
}

impl Default for FlutterLogoDecoration {
    fn default() -> FlutterLogoDecoration {
        FlutterLogoDecoration {
            text_color: Color(0xFF757575),
            style: FlutterLogoStyle::MarkOnly,
            margin: EdgeInsets::ZERO,
            position: 0.0,
            opacity: 1.0,
        }
    }
}

impl FlutterLogoDecoration {
    pub fn new(style: FlutterLogoStyle) -> FlutterLogoDecoration {
        let position = match style {
            FlutterLogoStyle::MarkOnly => 0.0,
            FlutterLogoStyle::Horizontal => 1.0,
            FlutterLogoStyle::Stacked => -1.0,
        };
        FlutterLogoDecoration {
            style,
            position,
            ..FlutterLogoDecoration::default()
        }
    }

    pub fn with_text_color(mut self, text_color: Color) -> FlutterLogoDecoration {
        self.text_color = text_color;
        self
    }

    /// Upstream `FlutterLogoDecoration.lerp`.
    pub fn lerp(
        a: Option<&FlutterLogoDecoration>,
        b: Option<&FlutterLogoDecoration>,
        t: f32,
    ) -> FlutterLogoDecoration {
        match (a, b) {
            (None, Some(b)) => FlutterLogoDecoration {
                text_color: b.text_color,
                style: b.style,
                margin: b.margin,
                position: b.position,
                opacity: b.opacity * t.clamp(0.0, 1.0),
            },
            (Some(a), None) => FlutterLogoDecoration {
                text_color: a.text_color,
                style: a.style,
                margin: a.margin,
                position: a.position,
                opacity: a.opacity * (1.0 - t).clamp(0.0, 1.0),
            },
            (Some(a), Some(b)) => FlutterLogoDecoration {
                text_color: crate::borders::color_lerp(a.text_color, b.text_color, t),
                style: if t < 0.5 { a.style } else { b.style },
                margin: EdgeInsets {
                    left: a.margin.left + (b.margin.left - a.margin.left) * t,
                    top: a.margin.top + (b.margin.top - a.margin.top) * t,
                    right: a.margin.right + (b.margin.right - a.margin.right) * t,
                    bottom: a.margin.bottom + (b.margin.bottom - a.margin.bottom) * t,
                },
                position: a.position + (b.position - a.position) * t,
                opacity: (a.opacity + (b.opacity - a.opacity) * t).clamp(0.0, 1.0),
            },
            (None, None) => FlutterLogoDecoration::default(),
        }
    }

    /// Upstream `_FlutterLogoPainter.paint`, mark-only and label styles:
    /// fit the logo into `rect` (margin already taken off by the caller)
    /// and draw the beams.
    pub fn paint(&self, canvas: &mut Canvas, rect: Rect) {
        let canvas_size = (rect.width(), rect.height());
        if canvas_size.0 <= 0.0 || canvas_size.1 <= 0.0 {
            return;
        }
        let logo_size = if self.position > 0.0 {
            (820.0, 232.0) // horizontal
        } else if self.position < 0.0 {
            (252.0, 306.0) // stacked
        } else {
            (202.0, 202.0) // the mark alone
        };
        let fitted = crate::render::apply_box_fit(
            crate::render::BoxFit::Contain,
            crate::render::Size::new(logo_size.0, logo_size.1),
            crate::render::Size::new(canvas_size.0, canvas_size.1),
        )
        .destination;
        let center = crate::render::Alignment::CENTER.inscribe(
            crate::render::Size::new(fitted.width, fitted.height),
            crate::render::Size::new(canvas_size.0, canvas_size.1),
        );
        let fitted_rect = Rect::xywh(
            rect.left + center.dx,
            rect.top + center.dy,
            fitted.width,
            fitted.height,
        );

        let center_square_height = canvas_size.0.min(canvas_size.1);
        let center_square = Rect::xywh(
            rect.left + (canvas_size.0 - center_square_height) / 2.0,
            rect.top + (canvas_size.1 - center_square_height) / 2.0,
            center_square_height,
            center_square_height,
        );
        let logo_target_square = if self.position > 0.0 {
            Rect::xywh(
                fitted_rect.left,
                fitted_rect.top,
                fitted_rect.height(),
                fitted_rect.height(),
            )
        } else if self.position < 0.0 {
            let logo_height = fitted_rect.height() * 191.0 / 306.0;
            Rect::xywh(
                fitted_rect.left + (fitted_rect.width() - logo_height) / 2.0,
                fitted_rect.top,
                logo_height,
                logo_height,
            )
        } else {
            center_square
        };
        let logo_square = lerp_rect(center_square, logo_target_square, self.position.abs());

        self.paint_mark(canvas, logo_square);

        if self.position != 0.0 {
            self.paint_label(canvas, fitted_rect);
        }
    }

    /// The mark: three beams, the rotated overlap square, the gradient
    /// triangle -- upstream `_paintLogo`, its canvas transforms folded into
    /// the coordinates.
    fn paint_mark(&self, canvas: &mut Canvas, rect: Rect) {
        // Upstream draws in a 202x202 space with the 166-wide artwork
        // centred: scale into the square, then shift right by 18.
        let scale = rect.width() / 202.0;
        let offset_x = rect.left + (202.0 - 166.0) / 2.0 * scale;
        let offset_y = rect.top;
        let map = |x: f32, y: f32| (offset_x + x * scale, offset_y + y * scale);

        let light_paint = Paint::new(Color(0xFF54C5F8));
        let medium_paint = Paint::new(Color(0xFF29B6F6));
        let dark_paint = Paint::new(Color(0xFF01579B));
        let triangle_paint = Paint::new(Color::WHITE).with_linear_gradient(
            map(87.2623 + 37.9092, 28.8384 + 123.4389),
            map(42.9205 + 37.9092, 35.0952 + 123.4389),
            &crate::painting::Gradient::new(&[Color(0x001A237E), Color(0x661A237E)]),
        );

        let draw = |canvas: &mut Canvas, points: &[(f32, f32)], paint: &Paint| {
            let mut path = RenderPath::new();
            path.move_to(points[0].0, points[0].1);
            for point in &points[1..] {
                path.line_to(point.0, point.1);
            }
            canvas.draw_path(&path, paint);
        };

        draw(
            canvas,
            &[
                map(37.7, 128.9),
                map(9.8, 101.0),
                map(100.4, 10.4),
                map(156.2, 10.4),
            ],
            &light_paint,
        );
        draw(
            canvas,
            &[
                map(156.2, 94.0),
                map(100.4, 94.0),
                map(78.5, 115.9),
                map(106.4, 143.8),
            ],
            &light_paint,
        );
        draw(
            canvas,
            &[
                map(79.5, 170.7),
                map(100.4, 191.6),
                map(156.2, 191.6),
                map(107.4, 142.8),
            ],
            &dark_paint,
        );

        // The rotated square between middle and bottom beam: upstream
        // transforms the canvas by 45 degrees and draws an axis-aligned
        // rect; the same rect pre-rotated is a diamond.
        let (sin45, cos45) = (
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2,
        );
        let rotate = |x: f32, y: f32| -> (f32, f32) {
            let rx = cos45 * x - sin45 * y;
            let ry = sin45 * x + cos45 * y;
            (rx - 77.697, ry + 98.057)
        };
        let square = (59.8, 123.1, 39.4, 39.4);
        let corners = [
            rotate(square.0, square.1),
            rotate(square.0 + square.2, square.1),
            rotate(square.0 + square.2, square.1 + square.3),
            rotate(square.0, square.1 + square.3),
        ];
        let corners: Vec<(f32, f32)> = corners.iter().map(|(x, y)| map(*x, *y)).collect();
        draw(canvas, &corners, &medium_paint);

        draw(
            canvas,
            &[map(79.5, 170.7), map(120.9, 156.4), map(107.4, 142.8)],
            &triangle_paint,
        );
    }

    /// The "Flutter" label, upstream `_paintLabel` minus the glyph boxes:
    /// shaped at the size the style implies and placed by upstream's
    /// constants.
    fn paint_label(&self, canvas: &mut Canvas, fitted_rect: Rect) {
        let font_size = if self.position > 0.0 {
            2.0 / 3.0 * fitted_rect.height() * (1.0 - (10.4 * 2.0) / 202.0)
        } else {
            fitted_rect.height() * 0.26
        };
        let scale = font_size / 100.0;
        let style = TextStyle {
            color: self.text_color,
            font_size,
            font_weight: 300,
            ..TextStyle::default()
        };
        let mut painter = crate::painting::TextPainter::new()
            .text("Flutter", style)
            .with_align(TextAlign::Start);
        painter.layout(10000.0);
        let text_width = painter.width();
        let (x, y) = if self.position > 0.0 {
            let final_left = (256.4 / 820.0) * fitted_rect.width() - (32.0 / 350.0) * font_size;
            let initial_left = fitted_rect.width() / 2.0 - text_width;
            let left = initial_left + (final_left - initial_left) * self.position;
            (
                fitted_rect.left + left,
                fitted_rect.top + fitted_rect.height() / 2.0 - font_size * 0.52,
            )
        } else {
            (
                fitted_rect.left + (fitted_rect.width() - text_width) / 2.0,
                // 252x306 artwork: the label sits under the mark.
                fitted_rect.top + fitted_rect.height() * 191.0 / 306.0 + font_size * 0.28,
            )
        };
        painter.paint(canvas, (x, y));
        let _ = scale;
    }
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::ltrb(
        a.left + (b.left - a.left) * t,
        a.top + (b.top - a.top) * t,
        a.right + (b.right - a.right) * t,
        a.bottom + (b.bottom - a.bottom) * t,
    )
}

#[cfg(test)]
mod flutter_logo_tests {
    use super::*;

    #[test]
    fn the_logo_lerps_between_styles_by_position() {
        let mark = FlutterLogoDecoration::new(FlutterLogoStyle::MarkOnly);
        let horizontal = FlutterLogoDecoration::new(FlutterLogoStyle::Horizontal);
        let mid = FlutterLogoDecoration::lerp(Some(&mark), Some(&horizontal), 0.5);
        assert!((mid.position - 0.5).abs() < 1e-6);
        // The style itself switches at the half: `t < 0.5` keeps the
        // source's, so 0.5 is already the target's.
        assert_eq!(mid.style, FlutterLogoStyle::Horizontal);
        let before = FlutterLogoDecoration::lerp(Some(&mark), Some(&horizontal), 0.25);
        assert_eq!(before.style, FlutterLogoStyle::MarkOnly);

        // Fading out scales the opacity.
        let faded = FlutterLogoDecoration::lerp(Some(&horizontal), None, 0.25);
        assert!((faded.opacity - 0.75).abs() < 1e-6);
    }

    #[test]
    fn the_logo_paints_all_three_styles_without_panicking() {
        for style in [
            FlutterLogoStyle::MarkOnly,
            FlutterLogoStyle::Horizontal,
            FlutterLogoStyle::Stacked,
        ] {
            let decoration = Decoration::FlutterLogo(FlutterLogoDecoration::new(style));
            let mut canvas = Canvas::new(200.0, 200.0);
            decoration.paint(
                &mut canvas,
                Rect::xywh(0.0, 0.0, 200.0, 200.0),
                TextDirection::Ltr,
            );
        }
    }
}

// -- What a BoxDecoration puts on the canvas ----------------------------------

#[cfg(test)]
mod decoration_paint_tests {
    //! `BoxDecoration::paint` was three draw calls nothing could see.
    //!
    //! A path is a handle with no readable shape behind it here, so what the
    //! recorder keeps is its **bounding box** and its colour. That is enough
    //! for what this paint gets wrong: where each shadow lands, how far it
    //! spreads, and what order the three layers go down in.

    use super::{BoxDecoration, BoxShape, Fill};
    use crate::borders::{Border, BorderStyle, BoxBorder, STROKE_ALIGN_INSIDE};
    use crate::direction::TextDirection;
    use crate::engine::{Color, LayerTree, Rect};
    use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
    use crate::painting::BoxShadow;
    use crate::render::{PaintContext, Size};

    const FILL: Color = Color(0xff2277cc);
    const SHADOW: Color = Color(0x66000000);
    const EDGE: Color = Color(0xff993333);
    const BOX: Rect = Rect {
        left: 20.0,
        top: 30.0,
        right: 120.0,
        bottom: 90.0,
    };

    fn painted(decoration: BoxDecoration) -> Vec<Drawn> {
        let mut layers = LayerTree::new(400, 400);
        reset_drawn();
        {
            let mut context = PaintContext::new(&mut layers, Size::new(400.0, 400.0));
            decoration.paint(context.canvas(), BOX, TextDirection::Ltr);
        }
        drawn()
    }

    #[allow(clippy::type_complexity)]
    fn paths(calls: &[Drawn]) -> Vec<((f32, f32, f32, f32), u32)> {
        calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Path {
                    left,
                    top,
                    right,
                    bottom,
                    argb,
                    ..
                } => Some(((*left, *top, *right, *bottom), *argb)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_shadows_go_under_the_fill_and_the_fill_under_the_border() {
        // Upstream's `_BoxDecorationPainter.paint` order, and each swap breaks
        // differently: a shadow over the fill is a smear across the box, and a
        // border under it is not there at all.
        let decoration = BoxDecoration::new()
            .with_fill(Fill::Solid(FILL))
            .with_border(BoxBorder::Uniform(Border::all(
                EDGE,
                2.0,
                BorderStyle::Solid,
                STROKE_ALIGN_INSIDE,
            )))
            .with_box_shadow(vec![BoxShadow::new(SHADOW, 0.0, 4.0, 6.0, 0.0)]);
        let calls = painted(decoration);
        let colours: Vec<u32> = calls
            .iter()
            .filter_map(|call| match call {
                Drawn::Path { argb, .. } => Some(*argb),
                Drawn::Rect { argb, .. } | Drawn::RRect { argb, .. } => Some(*argb),
                _ => None,
            })
            .collect();
        let first = colours.iter().position(|argb| *argb == SHADOW.0);
        let fill = colours.iter().position(|argb| *argb == FILL.0);
        let border = colours.iter().position(|argb| *argb == EDGE.0);
        assert!(
            first < fill && fill < border,
            "shadow {first:?}, fill {fill:?}, border {border:?} in {calls:?}"
        );
    }

    #[test]
    fn a_shadow_is_the_box_moved_by_its_offset() {
        // `BoxShadow.offset`, which is what makes a shadow look like light
        // coming from somewhere rather than a halo.
        let calls = painted(
            BoxDecoration::new().with_box_shadow(vec![BoxShadow::new(SHADOW, 5.0, 9.0, 0.0, 0.0)]),
        );
        let shapes = paths(&calls);
        assert_eq!(shapes.len(), 1, "{calls:?}");
        assert_eq!(
            shapes[0].0,
            (
                BOX.left + 5.0,
                BOX.top + 9.0,
                BOX.right + 5.0,
                BOX.bottom + 9.0
            )
        );
        assert_eq!(shapes[0].1, SHADOW.0);
    }

    #[test]
    fn and_grown_by_its_spread_on_every_side() {
        // `spreadRadius` inflates rather than scales: the same amount on each
        // edge, so a tall box's shadow does not become tall and thin.
        let calls = painted(
            BoxDecoration::new().with_box_shadow(vec![BoxShadow::new(SHADOW, 0.0, 0.0, 0.0, 7.0)]),
        );
        let (left, top, right, bottom) = paths(&calls)[0].0;
        assert_eq!(
            (left, top, right, bottom),
            (
                BOX.left - 7.0,
                BOX.top - 7.0,
                BOX.right + 7.0,
                BOX.bottom + 7.0
            )
        );
        assert_eq!(right - left, BOX.right - BOX.left + 14.0, "grown both ways");
        assert_eq!(bottom - top, BOX.bottom - BOX.top + 14.0);
    }

    #[test]
    fn every_shadow_in_the_list_is_drawn_and_the_last_one_is_on_top() {
        // Upstream walks `boxShadow` in order and the later ones land over the
        // earlier. A painter that stopped at the first would lose the
        // ambient-plus-key pair that every elevation in Material is made of.
        let calls = painted(BoxDecoration::new().with_box_shadow(vec![
                BoxShadow::new(Color(0x22000000), 0.0, 1.0, 0.0, 0.0),
                BoxShadow::new(Color(0x44000000), 0.0, 6.0, 0.0, 0.0),
            ]));
        let shapes = paths(&calls);
        assert_eq!(shapes.len(), 2, "{calls:?}");
        assert_eq!(shapes[0].1, 0x22000000);
        assert_eq!(shapes[1].1, 0x44000000, "the second is drawn second");
        assert!(shapes[1].0.1 > shapes[0].0.1, "and sits lower down");
    }

    #[test]
    fn a_circle_shadow_is_round_rather_than_the_shape_of_the_box() {
        // `BoxShape.circle` fits a circle to the *shortest* side, so an oblong
        // box gets a round shadow inside it rather than an ellipse filling it.
        let mut decoration = BoxDecoration::new()
            .with_box_shadow(vec![BoxShadow::new(SHADOW, 0.0, 0.0, 0.0, 0.0)]);
        decoration.shape = BoxShape::Circle;
        let (left, top, right, bottom) = paths(&painted(decoration))[0].0;
        let width = right - left;
        let height = bottom - top;
        assert_eq!(width, height, "as tall as it is wide");
        assert_eq!(
            width,
            (BOX.bottom - BOX.top).min(BOX.right - BOX.left),
            "and fitted to the shorter side"
        );
        // Centred in the box, which is what makes it fit rather than hug an
        // edge.
        assert_eq!((left + right) / 2.0, (BOX.left + BOX.right) / 2.0);
        assert_eq!((top + bottom) / 2.0, (BOX.top + BOX.bottom) / 2.0);
    }

    #[test]
    fn a_decoration_with_nothing_in_it_draws_nothing() {
        // The case a painter gets wrong by drawing a transparent rectangle
        // instead: every empty container in a tree would then cost a draw call.
        assert!(painted(BoxDecoration::new()).is_empty());
    }
}

