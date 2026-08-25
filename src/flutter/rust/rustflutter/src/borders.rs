//! The border family, from upstream `painting/`:
//! `border_radius.dart`, `borders.dart`, `box_border.dart`,
//! `circle_border.dart`, `oval_border.dart`, `stadium_border.dart`,
//! `rounded_rectangle_border.dart`, `beveled_rectangle_border.dart`,
//! `continuous_rectangle_border.dart`, `shape_decoration.dart`,
//! `notched_shapes.dart` -- one module for the cluster, the way the rest of
//! this crate maps several upstream files onto one topic file.
//!
//! Upstream models the family as a class hierarchy (`ShapeBorder` subclasses
//! and `_CompoundBorder`); here it is a closed enum, one variant per upstream
//! concrete class, with the private transition helpers
//! (`_StadiumToCircleBorder` and friends) as variants too so that `lerp`
//! keeps its exact shape-to-shape arithmetic.
//!
//! Recorded divergences (see PORTING_STATUS.md):
//!
//! * `RoundedSuperellipseBorder` draws with `ContinuousRectangleBorder`'s
//!   cubic corners -- the engine has no `RSuperellipse` primitive.
//! * `AutomaticNotchedShape` needs `Path.combine(PathOperation.difference)`;
//!   the engine ABI has no path boolean ops, so it paints the host shape
//!   alone until that lands.
//! * `ShapeDecoration` has no `image` field yet -- `DecorationImage` is a
//!   later wave.

use std::ops::{Add, Mul, Neg, Sub};

use crate::direction::TextDirection;
use crate::engine::{Canvas, Color, Paint, Rect, Style};
use crate::painting::{BoxShadow, FillType, RenderPath};
use crate::render::{EdgeInsets, EdgeInsetsDirectional, Offset};

/// Control-point factor that makes a cubic Bézier trace a quarter circle.
/// Quarter-circle arcs are all this file needs; the engine path ABI has no
/// arc primitives, so rounded corners are built from cubics.
const KAPPA: f32 = 0.552_284_75;

fn lerp_double(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Upstream `ui.lerpDouble` over an ARGB colour, alpha included.
pub fn color_lerp(a: Color, b: Color, t: f32) -> Color {
    let channel = |x: u8, y: u8| lerp_double(x as f32, y as f32, t).round().clamp(0.0, 255.0) as u8;
    Color::argb(
        channel(a.alpha(), b.alpha()),
        channel(a.red(), b.red()),
        channel(a.green(), b.green()),
        channel(a.blue(), b.blue()),
    )
}

/// `Offset.zero & size` upstream -- the rect a size occupies at an origin.
fn rect_at(offset: Offset, size: (f32, f32)) -> Rect {
    Rect::xywh(offset.dx, offset.dy, size.0, size.1)
}

// -- Radius (upstream basic_types.dart) ----------------------------------------

/// A radius for a box corner: `x` along the horizontal, `y` along the
/// vertical. `Radius.circular(r)` is `Radius.elliptical(r, r)`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Radius {
    pub x: f32,
    pub y: f32,
}

impl Radius {
    pub const ZERO: Radius = Radius { x: 0.0, y: 0.0 };

    pub const fn circular(radius: f32) -> Radius {
        Radius {
            x: radius,
            y: radius,
        }
    }

    pub const fn elliptical(x: f32, y: f32) -> Radius {
        Radius { x, y }
    }

    /// Whether both axes carry the same radius, i.e. the corner is an arc of
    /// a circle rather than of an ellipse.
    pub fn is_circular(&self) -> bool {
        self.x == self.y
    }

    /// `Radius.clamp(minimum: Radius.zero)` upstream: negative components
    /// are not drawable and become zero.
    pub fn clamped_nonnegative(&self) -> Radius {
        Radius::elliptical(self.x.max(0.0), self.y.max(0.0))
    }

    /// Upstream `Radius.lerp`.
    pub fn lerp(a: Radius, b: Radius, t: f32) -> Radius {
        Radius::elliptical(lerp_double(a.x, b.x, t), lerp_double(a.y, b.y, t))
    }
}

impl Add for Radius {
    type Output = Radius;
    fn add(self, other: Radius) -> Radius {
        Radius::elliptical(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Radius {
    type Output = Radius;
    fn sub(self, other: Radius) -> Radius {
        Radius::elliptical(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f32> for Radius {
    type Output = Radius;
    fn mul(self, factor: f32) -> Radius {
        Radius::elliptical(self.x * factor, self.y * factor)
    }
}

impl Neg for Radius {
    type Output = Radius;
    fn neg(self) -> Radius {
        Radius::elliptical(-self.x, -self.y)
    }
}

// -- RRect ----------------------------------------------------------------------

/// A rectangle with a radius per corner -- the parts of dart:ui's `RRect`
/// that the border family uses. Radii are stored as given; the path builder
/// applies Skia's proportional shrink when neighbours overrun a side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RRect {
    pub rect: Rect,
    pub top_left: Radius,
    pub top_right: Radius,
    pub bottom_right: Radius,
    pub bottom_left: Radius,
}

impl RRect {
    pub fn from_rect_and_radius(rect: Rect, radius: Radius) -> RRect {
        RRect::from_rect_and_corners(rect, radius, radius, radius, radius)
    }

    pub fn from_rect_and_corners(
        rect: Rect,
        top_left: Radius,
        top_right: Radius,
        bottom_right: Radius,
        bottom_left: Radius,
    ) -> RRect {
        RRect {
            rect,
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    /// Skia's rule, applied when adjacent radii add up to more than the side
    /// between them: scale both back proportionally.
    fn scaled(&self) -> [Radius; 4] {
        let width = self.rect.width().max(0.0);
        let height = self.rect.height().max(0.0);
        let mut radii = [
            self.top_left.clamped_nonnegative(),
            self.top_right.clamped_nonnegative(),
            self.bottom_right.clamped_nonnegative(),
            self.bottom_left.clamped_nonnegative(),
        ];
        for (a, b, side) in [
            (0usize, 1usize, width),
            (2, 3, width),
            (0, 3, height),
            (1, 2, height),
        ] {
            let sum_x = radii[a].x + radii[b].x;
            if sum_x > side && sum_x > 0.0 {
                let scale = side / sum_x;
                radii[a].x *= scale;
                radii[b].x *= scale;
            }
            let sum_y = radii[a].y + radii[b].y;
            if sum_y > side && sum_y > 0.0 {
                let scale = side / sum_y;
                radii[a].y *= scale;
                radii[b].y *= scale;
            }
        }
        radii
    }

    /// Outsets the edges by `delta` and grows the radii with it, the way
    /// dart:ui's `RRect.inflate` does; negative `delta` insets. Radii never
    /// go below zero.
    pub fn inflate(&self, delta: f32) -> RRect {
        let grow = |r: Radius| Radius::elliptical((r.x + delta).max(0.0), (r.y + delta).max(0.0));
        RRect {
            rect: Rect::ltrb(
                self.rect.left - delta,
                self.rect.top - delta,
                self.rect.right + delta,
                self.rect.bottom + delta,
            ),
            top_left: grow(self.top_left),
            top_right: grow(self.top_right),
            bottom_right: grow(self.bottom_right),
            bottom_left: grow(self.bottom_left),
        }
    }

    pub fn deflate(&self, delta: f32) -> RRect {
        self.inflate(-delta)
    }

    /// Per-side inset (negative to outset), the way
    /// `EdgeInsets.inflateRRect`/`deflateRRect` move each edge and shift the
    /// adjacent radii with it. Radii never go below zero.
    pub fn inset_insets(&self, left: f32, top: f32, right: f32, bottom: f32) -> RRect {
        let shift = |r: Radius, dx: f32, dy: f32| {
            Radius::elliptical((r.x - dx).max(0.0), (r.y - dy).max(0.0))
        };
        RRect {
            rect: Rect::ltrb(
                self.rect.left + left,
                self.rect.top + top,
                self.rect.right - right,
                self.rect.bottom - bottom,
            ),
            top_left: shift(self.top_left, left, top),
            top_right: shift(self.top_right, right, top),
            bottom_right: shift(self.bottom_right, right, bottom),
            bottom_left: shift(self.bottom_left, left, bottom),
        }
    }

    pub fn shortest_side(&self) -> f32 {
        self.rect.width().min(self.rect.height())
    }

    /// Appends the outline -- four straight runs joined by kappa cubics, so
    /// a corner of radius zero degenerates to a sharp angle. When every
    /// corner is zero this is the plain rectangle.
    pub fn append_to(&self, path: &mut RenderPath) {
        let [tl, tr, br, bl] = self.scaled();
        let (l, t, r, b) = (
            self.rect.left,
            self.rect.top,
            self.rect.right,
            self.rect.bottom,
        );
        if tl == Radius::ZERO && tr == Radius::ZERO && br == Radius::ZERO && bl == Radius::ZERO {
            path.add_rect(self.rect);
            return;
        }
        path.move_to(l + tl.x, t + tl.y);
        path.line_to(r - tr.x, t + tr.y);
        path.cubic_to(
            r - tr.x + tr.x * KAPPA,
            t,
            r,
            t + tr.y - tr.y * KAPPA,
            r,
            t + tr.y,
        );
        path.line_to(r, b - br.y);
        path.cubic_to(
            r,
            b - br.y + br.y * KAPPA,
            r - br.x + br.x * KAPPA,
            b,
            r - br.x,
            b,
        );
        path.line_to(l + bl.x, b);
        path.cubic_to(
            l + bl.x - bl.x * KAPPA,
            b,
            l,
            b - bl.y + bl.y * KAPPA,
            l,
            b - bl.y,
        );
        path.line_to(l, t + tl.y);
        path.cubic_to(
            l,
            t + tl.y - tl.y * KAPPA,
            l + tl.x - tl.x * KAPPA,
            t,
            l + tl.x,
            t,
        );
        path.close();
    }

    pub fn to_path(&self) -> RenderPath {
        let mut path = RenderPath::new();
        self.append_to(&mut path);
        path
    }

    /// Inside the rectangle, and inside each corner's ellipse where the
    /// corner is rounded -- dart:ui's `RRect.contains` for axis-aligned
    /// corners.
    pub fn contains(&self, position: Offset) -> bool {
        if !rect_contains(self.rect, position) {
            return false;
        }
        let [tl, tr, br, bl] = self.scaled();
        let (l, t, r, b) = (
            self.rect.left,
            self.rect.top,
            self.rect.right,
            self.rect.bottom,
        );
        let in_corner_ellipse = |corner: (f32, f32), radius: Radius, position: Offset| -> bool {
            if radius == Radius::ZERO {
                return true;
            }
            let dx = (position.dx - corner.0) / radius.x;
            let dy = (position.dy - corner.1) / radius.y;
            dx * dx + dy * dy <= 1.0
        };
        // Each corner's ellipse is centred a full radius in from both edges.
        if position.dx < l + tl.x
            && position.dy < t + tl.y
            && !in_corner_ellipse((l + tl.x, t + tl.y), tl, position)
        {
            return false;
        }
        if position.dx > r - tr.x
            && position.dy < t + tr.y
            && !in_corner_ellipse((r - tr.x, t + tr.y), tr, position)
        {
            return false;
        }
        if position.dx > r - br.x
            && position.dy > b - br.y
            && !in_corner_ellipse((r - br.x, b - br.y), br, position)
        {
            return false;
        }
        if position.dx < l + bl.x
            && position.dy > b - bl.y
            && !in_corner_ellipse((l + bl.x, b - bl.y), bl, position)
        {
            return false;
        }
        true
    }
}

// -- Rect helpers (upstream Rect, the parts borders need) -----------------------

pub(crate) fn rect_center(rect: Rect) -> Offset {
    Offset::new(
        (rect.left + rect.right) / 2.0,
        (rect.top + rect.bottom) / 2.0,
    )
}

pub(crate) fn rect_shortest_side(rect: Rect) -> f32 {
    rect.width().min(rect.height())
}

pub(crate) fn rect_inflate(rect: Rect, delta: f32) -> Rect {
    Rect::ltrb(
        rect.left - delta,
        rect.top - delta,
        rect.right + delta,
        rect.bottom + delta,
    )
}

pub(crate) fn rect_deflate(rect: Rect, delta: f32) -> Rect {
    rect_inflate(rect, -delta)
}

pub(crate) fn rect_deflate_insets(rect: Rect, insets: EdgeInsets) -> Rect {
    Rect::ltrb(
        rect.left + insets.left,
        rect.top + insets.top,
        rect.right - insets.right,
        rect.bottom - insets.bottom,
    )
}

pub(crate) fn rect_contains(rect: Rect, position: Offset) -> bool {
    position.dx >= rect.left
        && position.dx < rect.right
        && position.dy >= rect.top
        && position.dy < rect.bottom
}

pub(crate) fn rect_overlaps(a: Rect, b: Rect) -> bool {
    a.right > b.left && b.right > a.left && a.bottom > b.top && b.bottom > a.top
}

// -- EdgeInsetsGeometry (the directional pair, for dimensions) ------------------

/// `EdgeInsets` or `EdgeInsetsDirectional`, resolved against a reading
/// direction the way `ShapeBorder.dimensions` hands it back upstream.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum EdgeInsetsGeometry {
    #[default]
    Zero,
    Absolute(EdgeInsets),
    Directional(EdgeInsetsDirectional),
}

impl EdgeInsetsGeometry {
    pub fn resolve(&self, direction: TextDirection) -> EdgeInsets {
        match *self {
            EdgeInsetsGeometry::Zero => EdgeInsets::ZERO,
            EdgeInsetsGeometry::Absolute(insets) => insets,
            EdgeInsetsGeometry::Directional(insets) => match direction {
                TextDirection::Ltr => EdgeInsets {
                    left: insets.start,
                    top: insets.top,
                    right: insets.end,
                    bottom: insets.bottom,
                },
                TextDirection::Rtl => EdgeInsets {
                    left: insets.end,
                    top: insets.top,
                    right: insets.start,
                    bottom: insets.bottom,
                },
            },
        }
    }

    pub fn add(self, other: EdgeInsetsGeometry) -> EdgeInsetsGeometry {
        let expand = |a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)| {
            (a.0 + b.0, a.1 + b.1, a.2 + b.2, a.3 + b.3)
        };
        match (self, other) {
            (EdgeInsetsGeometry::Zero, other) => other,
            (this, EdgeInsetsGeometry::Zero) => this,
            (EdgeInsetsGeometry::Directional(a), EdgeInsetsGeometry::Directional(b)) => {
                let (start, top, end, bottom) = expand(
                    (a.start, a.top, a.end, a.bottom),
                    (b.start, b.top, b.end, b.bottom),
                );
                EdgeInsetsGeometry::Directional(EdgeInsetsDirectional {
                    start,
                    top,
                    end,
                    bottom,
                })
            }
            (EdgeInsetsGeometry::Absolute(a), EdgeInsetsGeometry::Absolute(b)) => {
                let (left, top, right, bottom) = expand(
                    (a.left, a.top, a.right, a.bottom),
                    (b.left, b.top, b.right, b.bottom),
                );
                EdgeInsetsGeometry::Absolute(EdgeInsets {
                    left,
                    top,
                    right,
                    bottom,
                })
            }
            (a, b) => {
                // A mix resolves by adding both contributions per edge.
                let a = a.resolve(TextDirection::Ltr);
                let b = b.resolve(TextDirection::Ltr);
                let (left, top, right, bottom) = expand(
                    (a.left, a.top, a.right, a.bottom),
                    (b.left, b.top, b.right, b.bottom),
                );
                EdgeInsetsGeometry::Absolute(EdgeInsets {
                    left,
                    top,
                    right,
                    bottom,
                })
            }
        }
    }

    pub fn deflate_rect(&self, rect: Rect, direction: TextDirection) -> Rect {
        rect_deflate_insets(rect, self.resolve(direction))
    }
}

// -- BorderRadius (upstream border_radius.dart) ---------------------------------

/// An immutable set of radii for each physical corner of a rectangle.
/// Not affected by text direction.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BorderRadius {
    pub top_left: Radius,
    pub top_right: Radius,
    pub bottom_left: Radius,
    pub bottom_right: Radius,
}

impl BorderRadius {
    pub const ZERO: BorderRadius = BorderRadius {
        top_left: Radius::ZERO,
        top_right: Radius::ZERO,
        bottom_left: Radius::ZERO,
        bottom_right: Radius::ZERO,
    };

    pub const fn all(radius: Radius) -> BorderRadius {
        BorderRadius {
            top_left: radius,
            top_right: radius,
            bottom_left: radius,
            bottom_right: radius,
        }
    }

    pub const fn circular(radius: f32) -> BorderRadius {
        BorderRadius::all(Radius::circular(radius))
    }

    pub const fn vertical(top: Radius, bottom: Radius) -> BorderRadius {
        BorderRadius {
            top_left: top,
            top_right: top,
            bottom_left: bottom,
            bottom_right: bottom,
        }
    }

    pub const fn horizontal(left: Radius, right: Radius) -> BorderRadius {
        BorderRadius {
            top_left: left,
            top_right: right,
            bottom_left: left,
            bottom_right: right,
        }
    }

    pub const fn only(
        top_left: Radius,
        top_right: Radius,
        bottom_left: Radius,
        bottom_right: Radius,
    ) -> BorderRadius {
        BorderRadius {
            top_left,
            top_right,
            bottom_left,
            bottom_right,
        }
    }

    pub fn is_zero(&self) -> bool {
        *self == BorderRadius::ZERO
    }

    /// `BorderRadius.toRRect`: negative components clamp away before the
    /// rectangle is built.
    pub fn to_rrect(&self, rect: Rect) -> RRect {
        RRect::from_rect_and_corners(
            rect,
            self.top_left.clamped_nonnegative(),
            self.top_right.clamped_nonnegative(),
            self.bottom_right.clamped_nonnegative(),
            self.bottom_left.clamped_nonnegative(),
        )
    }

    /// Physical corners are their own resolution.
    pub fn resolve(&self, _direction: TextDirection) -> BorderRadius {
        *self
    }

    /// Upstream `BorderRadius.lerp`.
    pub fn lerp(a: BorderRadius, b: BorderRadius, t: f32) -> BorderRadius {
        BorderRadius::only(
            Radius::lerp(a.top_left, b.top_left, t),
            Radius::lerp(a.top_right, b.top_right, t),
            Radius::lerp(a.bottom_left, b.bottom_left, t),
            Radius::lerp(a.bottom_right, b.bottom_right, t),
        )
    }
}

impl Add for BorderRadius {
    type Output = BorderRadius;
    fn add(self, other: BorderRadius) -> BorderRadius {
        BorderRadius::only(
            self.top_left + other.top_left,
            self.top_right + other.top_right,
            self.bottom_left + other.bottom_left,
            self.bottom_right + other.bottom_right,
        )
    }
}

impl Sub for BorderRadius {
    type Output = BorderRadius;
    fn sub(self, other: BorderRadius) -> BorderRadius {
        BorderRadius::only(
            self.top_left - other.top_left,
            self.top_right - other.top_right,
            self.bottom_left - other.bottom_left,
            self.bottom_right - other.bottom_right,
        )
    }
}

impl Mul<f32> for BorderRadius {
    type Output = BorderRadius;
    fn mul(self, factor: f32) -> BorderRadius {
        BorderRadius::only(
            self.top_left * factor,
            self.top_right * factor,
            self.bottom_left * factor,
            self.bottom_right * factor,
        )
    }
}

/// The same radii, but pinned to the reading direction: `topStart` is the
/// top-left in left-to-right text and the top-right in right-to-left.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BorderRadiusDirectional {
    pub top_start: Radius,
    pub top_end: Radius,
    pub bottom_start: Radius,
    pub bottom_end: Radius,
}

impl BorderRadiusDirectional {
    pub const ZERO: BorderRadiusDirectional = BorderRadiusDirectional {
        top_start: Radius::ZERO,
        top_end: Radius::ZERO,
        bottom_start: Radius::ZERO,
        bottom_end: Radius::ZERO,
    };

    pub const fn all(radius: Radius) -> BorderRadiusDirectional {
        BorderRadiusDirectional {
            top_start: radius,
            top_end: radius,
            bottom_start: radius,
            bottom_end: radius,
        }
    }

    pub const fn circular(radius: f32) -> BorderRadiusDirectional {
        BorderRadiusDirectional::all(Radius::circular(radius))
    }

    pub const fn vertical(top: Radius, bottom: Radius) -> BorderRadiusDirectional {
        BorderRadiusDirectional {
            top_start: top,
            top_end: top,
            bottom_start: bottom,
            bottom_end: bottom,
        }
    }

    pub const fn horizontal(start: Radius, end: Radius) -> BorderRadiusDirectional {
        BorderRadiusDirectional {
            top_start: start,
            top_end: end,
            bottom_start: start,
            bottom_end: end,
        }
    }

    pub const fn only(
        top_start: Radius,
        top_end: Radius,
        bottom_start: Radius,
        bottom_end: Radius,
    ) -> BorderRadiusDirectional {
        BorderRadiusDirectional {
            top_start,
            top_end,
            bottom_start,
            bottom_end,
        }
    }

    /// Upstream `BorderRadiusDirectional.resolve`.
    pub fn resolve(&self, direction: TextDirection) -> BorderRadius {
        match direction {
            TextDirection::Rtl => BorderRadius::only(
                self.top_end,
                self.top_start,
                self.bottom_end,
                self.bottom_start,
            ),
            TextDirection::Ltr => BorderRadius::only(
                self.top_start,
                self.top_end,
                self.bottom_start,
                self.bottom_end,
            ),
        }
    }

    /// Upstream `BorderRadiusDirectional.lerp`.
    pub fn lerp(
        a: BorderRadiusDirectional,
        b: BorderRadiusDirectional,
        t: f32,
    ) -> BorderRadiusDirectional {
        BorderRadiusDirectional::only(
            Radius::lerp(a.top_start, b.top_start, t),
            Radius::lerp(a.top_end, b.top_end, t),
            Radius::lerp(a.bottom_start, b.bottom_start, t),
            Radius::lerp(a.bottom_end, b.bottom_end, t),
        )
    }
}

impl Add for BorderRadiusDirectional {
    type Output = BorderRadiusDirectional;
    fn add(self, other: BorderRadiusDirectional) -> BorderRadiusDirectional {
        BorderRadiusDirectional::only(
            self.top_start + other.top_start,
            self.top_end + other.top_end,
            self.bottom_start + other.bottom_start,
            self.bottom_end + other.bottom_end,
        )
    }
}

impl Sub for BorderRadiusDirectional {
    type Output = BorderRadiusDirectional;
    fn sub(self, other: BorderRadiusDirectional) -> BorderRadiusDirectional {
        BorderRadiusDirectional::only(
            self.top_start - other.top_start,
            self.top_end - other.top_end,
            self.bottom_start - other.bottom_start,
            self.bottom_end - other.bottom_end,
        )
    }
}

impl Mul<f32> for BorderRadiusDirectional {
    type Output = BorderRadiusDirectional;
    fn mul(self, factor: f32) -> BorderRadiusDirectional {
        BorderRadiusDirectional::only(
            self.top_start * factor,
            self.top_end * factor,
            self.bottom_start * factor,
            self.bottom_end * factor,
        )
    }
}

/// Upstream `_MixedBorderRadius`: what adding or subtracting a physical and
/// a directional radius produces -- both sets of corners, summed at
/// `resolve` time.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MixedBorderRadius {
    absolute: BorderRadius,
    directional: BorderRadiusDirectional,
}

/// A corner radius that may be physical (`BorderRadius`), directional
/// (`BorderRadiusDirectional`), or a combination of both. Resolve it against
/// a `TextDirection` to get physical corners.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum BorderRadiusGeometry {
    #[default]
    Zero,
    Absolute(BorderRadius),
    Directional(BorderRadiusDirectional),
    Mixed(MixedBorderRadius),
}

impl BorderRadiusGeometry {
    pub const fn circular(radius: f32) -> BorderRadiusGeometry {
        BorderRadiusGeometry::Absolute(BorderRadius::circular(radius))
    }

    pub const fn all(radius: Radius) -> BorderRadiusGeometry {
        BorderRadiusGeometry::Absolute(BorderRadius::all(radius))
    }

    /// Upstream `BorderRadiusGeometry.subtract`: same-kind operands keep
    /// their kind, anything else becomes a mix.
    pub fn subtract(self, other: BorderRadiusGeometry) -> BorderRadiusGeometry {
        match (self, other) {
            (BorderRadiusGeometry::Absolute(a), BorderRadiusGeometry::Absolute(b)) => {
                BorderRadiusGeometry::Absolute(a - b)
            }
            (BorderRadiusGeometry::Directional(a), BorderRadiusGeometry::Directional(b)) => {
                BorderRadiusGeometry::Directional(a - b)
            }
            (BorderRadiusGeometry::Zero, BorderRadiusGeometry::Zero) => BorderRadiusGeometry::Zero,
            (this, other) => {
                let (abs, dir) = this.parts();
                let (other_abs, other_dir) = other.parts();
                BorderRadiusGeometry::Mixed(MixedBorderRadius {
                    absolute: abs - other_abs,
                    directional: dir - other_dir,
                })
            }
        }
    }

    /// Upstream `BorderRadiusGeometry.add`.
    pub fn add(self, other: BorderRadiusGeometry) -> BorderRadiusGeometry {
        match (self, other) {
            (BorderRadiusGeometry::Absolute(a), BorderRadiusGeometry::Absolute(b)) => {
                BorderRadiusGeometry::Absolute(a + b)
            }
            (BorderRadiusGeometry::Directional(a), BorderRadiusGeometry::Directional(b)) => {
                BorderRadiusGeometry::Directional(a + b)
            }
            (BorderRadiusGeometry::Zero, other) => other,
            (this, BorderRadiusGeometry::Zero) => this,
            (this, other) => {
                let (abs, dir) = this.parts();
                let (other_abs, other_dir) = other.parts();
                BorderRadiusGeometry::Mixed(MixedBorderRadius {
                    absolute: abs + other_abs,
                    directional: dir + other_dir,
                })
            }
        }
    }

    /// Both contributions of this radius, with the missing one zeroed.
    fn parts(self) -> (BorderRadius, BorderRadiusDirectional) {
        match self {
            BorderRadiusGeometry::Zero => (BorderRadius::ZERO, BorderRadiusDirectional::ZERO),
            BorderRadiusGeometry::Absolute(radius) => (radius, BorderRadiusDirectional::ZERO),
            BorderRadiusGeometry::Directional(radius) => (BorderRadius::ZERO, radius),
            BorderRadiusGeometry::Mixed(mixed) => (mixed.absolute, mixed.directional),
        }
    }

    pub fn scale(self, factor: f32) -> BorderRadiusGeometry {
        match self {
            BorderRadiusGeometry::Zero => BorderRadiusGeometry::Zero,
            BorderRadiusGeometry::Absolute(radius) => {
                BorderRadiusGeometry::Absolute(radius * factor)
            }
            BorderRadiusGeometry::Directional(radius) => {
                BorderRadiusGeometry::Directional(radius * factor)
            }
            BorderRadiusGeometry::Mixed(mixed) => BorderRadiusGeometry::Mixed(MixedBorderRadius {
                absolute: mixed.absolute * factor,
                directional: mixed.directional * factor,
            }),
        }
    }

    /// Upstream `BorderRadiusGeometry.lerp`: `a.add((b.subtract(a)) * t)`.
    pub fn lerp(a: BorderRadiusGeometry, b: BorderRadiusGeometry, t: f32) -> BorderRadiusGeometry {
        a.add(b.subtract(a).scale(t))
    }

    /// Upstream resolve: the mix sums its two contributions per corner.
    pub fn resolve(&self, direction: TextDirection) -> BorderRadius {
        match *self {
            BorderRadiusGeometry::Zero => BorderRadius::ZERO,
            BorderRadiusGeometry::Absolute(radius) => radius,
            BorderRadiusGeometry::Directional(radius) => radius.resolve(direction),
            BorderRadiusGeometry::Mixed(mixed) => {
                let absolute = mixed.absolute;
                let directional = mixed.directional.resolve(direction);
                absolute + directional
            }
        }
    }

    pub fn is_zero(&self) -> bool {
        matches!(self, BorderRadiusGeometry::Zero)
            || *self == BorderRadiusGeometry::Absolute(BorderRadius::ZERO)
    }
}

impl From<BorderRadius> for BorderRadiusGeometry {
    fn from(radius: BorderRadius) -> BorderRadiusGeometry {
        BorderRadiusGeometry::Absolute(radius)
    }
}

impl From<BorderRadiusDirectional> for BorderRadiusGeometry {
    fn from(radius: BorderRadiusDirectional) -> BorderRadiusGeometry {
        BorderRadiusGeometry::Directional(radius)
    }
}

// -- BorderSide (upstream borders.dart) -----------------------------------------

/// Whether a side is drawn at all. Upstream has only these two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BorderStyle {
    /// Skip the border -- but the side still has a width for layout.
    None,
    #[default]
    Solid,
}

/// A side of a border of a box: colour, weight, and where the stroke sits
/// across the edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderSide {
    pub color: Color,
    pub width: f32,
    pub style: BorderStyle,
    /// `-1.0` inside the edge, `0.0` centred on it, `1.0` fully outside.
    pub stroke_align: f32,
}

/// `BorderSide.strokeAlignInside`.
pub const STROKE_ALIGN_INSIDE: f32 = -1.0;
/// `BorderSide.strokeAlignCenter`.
pub const STROKE_ALIGN_CENTER: f32 = 0.0;
/// `BorderSide.strokeAlignOutside`.
pub const STROKE_ALIGN_OUTSIDE: f32 = 1.0;

impl Default for BorderSide {
    fn default() -> BorderSide {
        BorderSide {
            color: Color::argb(255, 0, 0, 0),
            width: 1.0,
            style: BorderStyle::Solid,
            stroke_align: STROKE_ALIGN_INSIDE,
        }
    }
}

impl BorderSide {
    /// A hairline black side that is not drawn.
    pub const NONE: BorderSide = BorderSide {
        color: Color(0xFF000000),
        width: 0.0,
        style: BorderStyle::None,
        stroke_align: STROKE_ALIGN_INSIDE,
    };

    /// How much of the stroke lies inside the edge: the full width for
    /// `strokeAlign` of -1, half at 0, nothing at 1.
    pub fn stroke_inset(&self) -> f32 {
        self.width * (1.0 - (1.0 + self.stroke_align) / 2.0)
    }

    /// How much of the stroke lies outside the edge.
    pub fn stroke_outset(&self) -> f32 {
        self.width * (1.0 + self.stroke_align) / 2.0
    }

    /// The centre of the stroke relative to the edge.
    pub fn stroke_offset(&self) -> f32 {
        self.width * self.stroke_align
    }

    /// Upstream `BorderSide.scale`: negative factors clamp the width, and
    /// the nil side flips its style to `none` (a zero width draws a
    /// hairline otherwise).
    pub fn scale(&self, t: f32) -> BorderSide {
        BorderSide {
            color: self.color,
            width: (self.width * t).max(0.0),
            style: if t <= 0.0 {
                BorderStyle::None
            } else {
                self.style
            },
            stroke_align: self.stroke_align,
        }
    }

    /// Upstream `BorderSide.toPaint` -- `strokeAlign` is not represented
    /// here; callers inset or outset the region themselves.
    pub fn to_paint(&self) -> Paint {
        match self.style {
            BorderStyle::Solid => {
                Paint::new(self.color).with_style(Style::Stroke { width: self.width })
            }
            BorderStyle::None => {
                Paint::new(Color(0x00000000)).with_style(Style::Stroke { width: 0.0 })
            }
        }
    }

    /// Whether `merge` may combine the two sides: either is nil, or colour
    /// and style match.
    pub fn can_merge(a: BorderSide, b: BorderSide) -> bool {
        if (a.style == BorderStyle::None && a.width == 0.0)
            || (b.style == BorderStyle::None && b.width == 0.0)
        {
            return true;
        }
        a.style == b.style && a.color == b.color
    }

    /// Upstream `BorderSide.merge`: nil sides drop out, otherwise widths
    /// add under the stronger stroke alignment.
    pub fn merge(a: BorderSide, b: BorderSide) -> BorderSide {
        debug_assert!(BorderSide::can_merge(a, b));
        let a_is_none = a.style == BorderStyle::None && a.width == 0.0;
        let b_is_none = b.style == BorderStyle::None && b.width == 0.0;
        if a_is_none && b_is_none {
            return BorderSide::NONE;
        }
        if a_is_none {
            return b;
        }
        if b_is_none {
            return a;
        }
        BorderSide {
            color: a.color,
            width: a.width + b.width,
            style: a.style,
            stroke_align: a.stroke_align.max(b.stroke_align),
        }
    }

    /// Upstream `BorderSide.lerp`, including its two special paths: a width
    /// that interpolates below zero yields the nil side, and mismatched
    /// styles lerp their colours with the missing side's alpha zeroed.
    pub fn lerp(a: BorderSide, b: BorderSide, t: f32) -> BorderSide {
        if a == b {
            return a;
        }
        if t == 0.0 {
            return a;
        }
        if t == 1.0 {
            return b;
        }
        let width = lerp_double(a.width, b.width, t);
        if width < 0.0 {
            return BorderSide::NONE;
        }
        if a.style == b.style && a.stroke_align == b.stroke_align {
            return BorderSide {
                color: color_lerp(a.color, b.color, t),
                width,
                style: a.style,
                stroke_align: a.stroke_align,
            };
        }
        let color_a = match a.style {
            BorderStyle::Solid => a.color,
            BorderStyle::None => a.color.with_alpha(0),
        };
        let color_b = match b.style {
            BorderStyle::Solid => b.color,
            BorderStyle::None => b.color.with_alpha(0),
        };
        if a.stroke_align != b.stroke_align {
            return BorderSide {
                color: color_lerp(color_a, color_b, t),
                width,
                style: BorderStyle::Solid,
                stroke_align: lerp_double(a.stroke_align, b.stroke_align, t),
            };
        }
        BorderSide {
            color: color_lerp(color_a, color_b, t),
            width,
            style: BorderStyle::Solid,
            stroke_align: a.stroke_align,
        }
    }
}

/// Upstream `paintBorder`: four filled trapezoids in top, right, bottom,
/// left order -- only notable when sides overlap. Hairline sides stroke
/// instead of filling.
pub fn paint_border(
    canvas: &mut Canvas,
    rect: Rect,
    top: BorderSide,
    right: BorderSide,
    bottom: BorderSide,
    left: BorderSide,
) {
    let (l, t, r, b) = (rect.left, rect.top, rect.right, rect.bottom);

    fn draw_side(
        canvas: &mut Canvas,
        from: (f32, f32),
        along: (f32, f32),
        inner_a: (f32, f32),
        inner_b: (f32, f32),
        side: BorderSide,
    ) {
        if side.style == BorderStyle::None {
            return;
        }
        let paint = if side.width == 0.0 {
            Paint::new(side.color).with_style(Style::Stroke { width: 0.0 })
        } else {
            Paint::new(side.color)
        };
        let mut path = RenderPath::new();
        path.move_to(from.0, from.1);
        path.line_to(along.0, along.1);
        if side.width != 0.0 {
            path.line_to(inner_a.0, inner_a.1);
            path.line_to(inner_b.0, inner_b.1);
        }
        path.close();
        canvas.draw_path(&path, &paint);
    }

    draw_side(
        canvas,
        (l, t),
        (r, t),
        (r - right.width, t + top.width),
        (l + left.width, t + top.width),
        top,
    );
    draw_side(
        canvas,
        (r, t),
        (r, b),
        (r - right.width, b - bottom.width),
        (r - right.width, t + top.width),
        right,
    );
    draw_side(
        canvas,
        (r, b),
        (l, b),
        (l + left.width, b - bottom.width),
        (r - right.width, b - bottom.width),
        bottom,
    );
    draw_side(
        canvas,
        (l, b),
        (l, t),
        (l + left.width, t + top.width),
        (l + left.width, b - bottom.width),
        left,
    );
}

// -- Border / BorderDirectional / BoxBorder (upstream box_border.dart) ----------

/// The shape to render a border as: rectangle or circle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BoxShape {
    #[default]
    Rectangle,
    Circle,
}

/// A border of a box: four physical sides.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Border {
    pub top: BorderSide,
    pub right: BorderSide,
    pub bottom: BorderSide,
    pub left: BorderSide,
}

impl Border {
    pub const fn new(
        top: BorderSide,
        right: BorderSide,
        bottom: BorderSide,
        left: BorderSide,
    ) -> Border {
        Border {
            top,
            right,
            bottom,
            left,
        }
    }

    pub const fn from_border_side(side: BorderSide) -> Border {
        Border {
            top: side,
            right: side,
            bottom: side,
            left: side,
        }
    }

    pub const fn symmetric(vertical: BorderSide, horizontal: BorderSide) -> Border {
        Border {
            top: horizontal,
            bottom: horizontal,
            left: vertical,
            right: vertical,
        }
    }

    pub fn all(color: Color, width: f32, style: BorderStyle, stroke_align: f32) -> Border {
        Border::from_border_side(BorderSide {
            color,
            width,
            style,
            stroke_align,
        })
    }

    pub fn merge(a: Border, b: Border) -> Border {
        debug_assert!(BorderSide::can_merge(a.top, b.top));
        debug_assert!(BorderSide::can_merge(a.right, b.right));
        debug_assert!(BorderSide::can_merge(a.bottom, b.bottom));
        debug_assert!(BorderSide::can_merge(a.left, b.left));
        Border {
            top: BorderSide::merge(a.top, b.top),
            right: BorderSide::merge(a.right, b.right),
            bottom: BorderSide::merge(a.bottom, b.bottom),
            left: BorderSide::merge(a.left, b.left),
        }
    }

    pub fn color_is_uniform(&self) -> bool {
        self.left.color == self.top.color
            && self.bottom.color == self.top.color
            && self.right.color == self.top.color
    }

    pub fn width_is_uniform(&self) -> bool {
        self.left.width == self.top.width
            && self.bottom.width == self.top.width
            && self.right.width == self.top.width
    }

    pub fn style_is_uniform(&self) -> bool {
        self.left.style == self.top.style
            && self.bottom.style == self.top.style
            && self.right.style == self.top.style
    }

    pub fn stroke_align_is_uniform(&self) -> bool {
        self.left.stroke_align == self.top.stroke_align
            && self.bottom.stroke_align == self.top.stroke_align
            && self.right.stroke_align == self.top.stroke_align
    }

    pub fn is_uniform(&self) -> bool {
        self.color_is_uniform()
            && self.width_is_uniform()
            && self.style_is_uniform()
            && self.stroke_align_is_uniform()
    }

    /// The colours that would actually be painted, in top/right/bottom/left
    /// order, deduplicated.
    fn distinct_visible_colors(&self) -> Vec<Color> {
        let mut colors = Vec::new();
        for side in [self.top, self.right, self.bottom, self.left] {
            if side.style != BorderStyle::None && !colors.contains(&side.color) {
                colors.push(side.color);
            }
        }
        colors
    }

    fn has_hairline_border(&self) -> bool {
        [self.top, self.right, self.bottom, self.left]
            .iter()
            .any(|side| side.style == BorderStyle::Solid && side.width == 0.0)
    }

    pub fn dimensions(&self) -> EdgeInsetsGeometry {
        EdgeInsetsGeometry::Absolute(EdgeInsets {
            left: self.left.stroke_inset(),
            top: self.top.stroke_inset(),
            right: self.right.stroke_inset(),
            bottom: self.bottom.stroke_inset(),
        })
    }

    pub fn scale(&self, t: f32) -> Border {
        Border {
            top: self.top.scale(t),
            right: self.right.scale(t),
            bottom: self.bottom.scale(t),
            left: self.left.scale(t),
        }
    }

    /// Upstream `Border.lerp`; a missing side of the interpolation treats
    /// that border as all-`none`.
    pub fn lerp(a: Option<Border>, b: Option<Border>, t: f32) -> Border {
        match (a, b) {
            (None, None) => Border::default(),
            (None, Some(b)) => b.scale(t),
            (Some(a), None) => a.scale(1.0 - t),
            (Some(a), Some(b)) => Border {
                top: BorderSide::lerp(a.top, b.top, t),
                right: BorderSide::lerp(a.right, b.right, t),
                bottom: BorderSide::lerp(a.bottom, b.bottom, t),
                left: BorderSide::lerp(a.left, b.left, t),
            },
        }
    }

    /// Upstream `Border.paint`, shared with `BorderDirectional` once its
    /// start/end sides have resolved to left/right.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        rect: Rect,
        border_radius: Option<BorderRadius>,
        shape: BoxShape,
    ) {
        paint_box_border_sides(
            canvas,
            rect,
            self.top,
            self.right,
            self.bottom,
            self.left,
            border_radius,
            shape,
        );
    }
}

/// A border whose lateral sides flip with the reading direction.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BorderDirectional {
    pub top: BorderSide,
    pub start: BorderSide,
    pub end: BorderSide,
    pub bottom: BorderSide,
}

impl BorderDirectional {
    pub const fn new(
        top: BorderSide,
        start: BorderSide,
        end: BorderSide,
        bottom: BorderSide,
    ) -> BorderDirectional {
        BorderDirectional {
            top,
            start,
            end,
            bottom,
        }
    }

    pub fn merge(a: BorderDirectional, b: BorderDirectional) -> BorderDirectional {
        debug_assert!(BorderSide::can_merge(a.top, b.top));
        debug_assert!(BorderSide::can_merge(a.start, b.start));
        debug_assert!(BorderSide::can_merge(a.end, b.end));
        debug_assert!(BorderSide::can_merge(a.bottom, b.bottom));
        BorderDirectional {
            top: BorderSide::merge(a.top, b.top),
            start: BorderSide::merge(a.start, b.start),
            end: BorderSide::merge(a.end, b.end),
            bottom: BorderSide::merge(a.bottom, b.bottom),
        }
    }

    pub fn color_is_uniform(&self) -> bool {
        self.start.color == self.top.color
            && self.bottom.color == self.top.color
            && self.end.color == self.top.color
    }

    pub fn width_is_uniform(&self) -> bool {
        self.start.width == self.top.width
            && self.bottom.width == self.top.width
            && self.end.width == self.top.width
    }

    pub fn style_is_uniform(&self) -> bool {
        self.start.style == self.top.style
            && self.bottom.style == self.top.style
            && self.end.style == self.top.style
    }

    pub fn stroke_align_is_uniform(&self) -> bool {
        self.start.stroke_align == self.top.stroke_align
            && self.bottom.stroke_align == self.top.stroke_align
            && self.end.stroke_align == self.top.stroke_align
    }

    pub fn is_uniform(&self) -> bool {
        self.color_is_uniform()
            && self.width_is_uniform()
            && self.style_is_uniform()
            && self.stroke_align_is_uniform()
    }

    // Upstream also keeps `distinctVisibleColors`/`hasHairlineBorder` here;
    // paint resolves start/end to left/right first and probes through
    // `Border`, so the pair lives there only.

    pub fn dimensions(&self) -> EdgeInsetsGeometry {
        EdgeInsetsGeometry::Directional(EdgeInsetsDirectional {
            start: self.start.stroke_inset(),
            top: self.top.stroke_inset(),
            end: self.end.stroke_inset(),
            bottom: self.bottom.stroke_inset(),
        })
    }

    pub fn scale(&self, t: f32) -> BorderDirectional {
        BorderDirectional {
            top: self.top.scale(t),
            start: self.start.scale(t),
            end: self.end.scale(t),
            bottom: self.bottom.scale(t),
        }
    }

    pub fn lerp(
        a: Option<BorderDirectional>,
        b: Option<BorderDirectional>,
        t: f32,
    ) -> BorderDirectional {
        match (a, b) {
            (None, None) => BorderDirectional::default(),
            (None, Some(b)) => b.scale(t),
            (Some(a), None) => a.scale(1.0 - t),
            (Some(a), Some(b)) => BorderDirectional {
                top: BorderSide::lerp(a.top, b.top, t),
                start: BorderSide::lerp(a.start, b.start, t),
                end: BorderSide::lerp(a.end, b.end, t),
                bottom: BorderSide::lerp(a.bottom, b.bottom, t),
            },
        }
    }

    pub fn paint(
        &self,
        canvas: &mut Canvas,
        rect: Rect,
        direction: TextDirection,
        border_radius: Option<BorderRadius>,
        shape: BoxShape,
    ) {
        let (left, right) = match direction {
            TextDirection::Rtl => (self.end, self.start),
            TextDirection::Ltr => (self.start, self.end),
        };
        paint_box_border_sides(
            canvas,
            rect,
            self.top,
            right,
            self.bottom,
            left,
            border_radius,
            shape,
        );
    }
}

/// `Border` or `BorderDirectional` behind one type, for `BoxBorder.lerp`
/// and anything that stores either.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum BoxBorder {
    #[default]
    None,
    Uniform(Border),
    Directional(BorderDirectional),
}

impl BoxBorder {
    /// Upstream `BoxBorder.lerp`. Between a physical and a directional
    /// border the lateral sides hand over at the half-way point: the old
    /// ones fade over the first half, the new ones arrive over the second.
    pub fn lerp(a: Option<BoxBorder>, b: Option<BoxBorder>, t: f32) -> BoxBorder {
        use BoxBorder::{Directional, None as NoBorder, Uniform};
        let a = a.unwrap_or(NoBorder);
        let b = b.unwrap_or(NoBorder);
        if a == b {
            return a;
        }
        if matches!(a, NoBorder | Uniform(_)) && matches!(b, NoBorder | Uniform(_)) {
            let a = match a {
                Uniform(border) => Some(border),
                _ => None,
            };
            let b = match b {
                Uniform(border) => Some(border),
                _ => None,
            };
            return Uniform(Border::lerp(a, b, t));
        }
        if matches!(a, NoBorder | Directional(_)) && matches!(b, NoBorder | Directional(_)) {
            let a = match a {
                Directional(border) => Some(border),
                _ => None,
            };
            let b = match b {
                Directional(border) => Some(border),
                _ => None,
            };
            return Directional(BorderDirectional::lerp(a, b, t));
        }
        let (mut a, mut b, mut t) = (a, b, t);
        if matches!(b, Uniform(_)) && matches!(a, Directional(_)) {
            std::mem::swap(&mut a, &mut b);
            t = 1.0 - t;
        }
        if let (Uniform(a), Directional(b)) = (a, b) {
            if b.start == BorderSide::NONE && b.end == BorderSide::NONE {
                return Uniform(Border::new(
                    BorderSide::lerp(a.top, b.top, t),
                    BorderSide::lerp(a.right, BorderSide::NONE, t),
                    BorderSide::lerp(a.bottom, b.bottom, t),
                    BorderSide::lerp(a.left, BorderSide::NONE, t),
                ));
            }
            if a.left == BorderSide::NONE && a.right == BorderSide::NONE {
                return Directional(BorderDirectional::new(
                    BorderSide::lerp(a.top, b.top, t),
                    BorderSide::lerp(BorderSide::NONE, b.start, t),
                    BorderSide::lerp(BorderSide::NONE, b.end, t),
                    BorderSide::lerp(a.bottom, b.bottom, t),
                ));
            }
            if t < 0.5 {
                return Uniform(Border::new(
                    BorderSide::lerp(a.top, b.top, t),
                    BorderSide::lerp(a.right, BorderSide::NONE, t * 2.0),
                    BorderSide::lerp(a.bottom, b.bottom, t),
                    BorderSide::lerp(a.left, BorderSide::NONE, t * 2.0),
                ));
            }
            return Directional(BorderDirectional::new(
                BorderSide::lerp(a.top, b.top, t),
                BorderSide::lerp(BorderSide::NONE, b.start, (t - 0.5) * 2.0),
                BorderSide::lerp(BorderSide::NONE, b.end, (t - 0.5) * 2.0),
                BorderSide::lerp(a.bottom, b.bottom, t),
            ));
        }
        if t < 0.5 { a } else { b }
    }

    pub fn dimensions(&self, direction: TextDirection) -> EdgeInsets {
        match *self {
            BoxBorder::None => EdgeInsets::ZERO,
            BoxBorder::Uniform(border) => border.dimensions().resolve(direction),
            BoxBorder::Directional(border) => border.dimensions().resolve(direction),
        }
    }

    pub fn is_uniform(&self) -> bool {
        match *self {
            BoxBorder::None => true,
            BoxBorder::Uniform(border) => border.is_uniform(),
            BoxBorder::Directional(border) => border.is_uniform(),
        }
    }

    pub fn scale(&self, t: f32) -> BoxBorder {
        match *self {
            BoxBorder::None => BoxBorder::None,
            BoxBorder::Uniform(border) => BoxBorder::Uniform(border.scale(t)),
            BoxBorder::Directional(border) => BoxBorder::Directional(border.scale(t)),
        }
    }

    pub fn paint(
        &self,
        canvas: &mut Canvas,
        rect: Rect,
        direction: TextDirection,
        border_radius: Option<BorderRadius>,
        shape: BoxShape,
    ) {
        match *self {
            BoxBorder::None => {}
            BoxBorder::Uniform(border) => border.paint(canvas, rect, border_radius, shape),
            BoxBorder::Directional(border) => {
                border.paint(canvas, rect, direction, border_radius, shape)
            }
        }
    }
}

/// The body of `Border.paint` / `BorderDirectional.paint` once both have
/// resolved to physical sides.
fn paint_box_border_sides(
    canvas: &mut Canvas,
    rect: Rect,
    top: BorderSide,
    right: BorderSide,
    bottom: BorderSide,
    left: BorderSide,
    border_radius: Option<BorderRadius>,
    shape: BoxShape,
) {
    let uniform_side = top;
    if top == right && right == bottom && bottom == left {
        if uniform_side.style == BorderStyle::None {
            return;
        }
        match shape {
            BoxShape::Circle => {
                paint_uniform_border_with_circle(canvas, rect, uniform_side);
            }
            BoxShape::Rectangle => {
                if let Some(radius) = border_radius {
                    if radius != BorderRadius::ZERO {
                        paint_uniform_border_with_radius(canvas, rect, uniform_side, radius);
                        return;
                    }
                }
                paint_uniform_border_with_rectangle(canvas, rect, uniform_side);
            }
        }
        return;
    }

    if top.style == right.style
        && right.style == bottom.style
        && bottom.style == left.style
        && top.style == BorderStyle::None
    {
        return;
    }

    // Non-uniform, but one visible colour and no hairlines: drawDRRect
    // between per-side insets and outsets, on any radius.
    let probe = Border::new(top, right, bottom, left);
    let visible_colors = probe.distinct_visible_colors();
    let has_hairline = probe.has_hairline_border();
    let radius_nonzero = border_radius.is_some_and(|radius| radius != BorderRadius::ZERO);
    if visible_colors.len() == 1 && !has_hairline && (shape == BoxShape::Circle || radius_nonzero) {
        let nil = BorderSide::NONE;
        paint_non_uniform_border(
            canvas,
            rect,
            if top.style == BorderStyle::None {
                nil
            } else {
                top
            },
            if right.style == BorderStyle::None {
                nil
            } else {
                right
            },
            if bottom.style == BorderStyle::None {
                nil
            } else {
                bottom
            },
            if left.style == BorderStyle::None {
                nil
            } else {
                left
            },
            visible_colors[0],
            border_radius,
            shape,
        );
        return;
    }

    // The remaining cases -- mixed colours without a radius, hairlines --
    // upstream rejects with asserts when a radius or circle shape is set;
    // here they fall through to the trapezoid painter.
    paint_border(canvas, rect, top, right, bottom, left);
}

/// `BoxBorder._paintUniformBorderWithRadius`: a double rounded rect, the
/// ring between the stroke outset and inset, or a plain stroke for a
/// hairline.
fn paint_uniform_border_with_radius(
    canvas: &mut Canvas,
    rect: Rect,
    side: BorderSide,
    border_radius: BorderRadius,
) {
    if side.width == 0.0 {
        let rrect = border_radius.to_rrect(rect);
        let paint = Paint::new(side.color).with_style(Style::Stroke { width: 0.0 });
        canvas.draw_path(&rrect.to_path(), &paint);
        return;
    }
    let border_rect = border_radius.to_rrect(rect);
    let inner = border_rect.deflate(side.stroke_inset());
    let outer = border_rect.inflate(side.stroke_outset());
    let mut path = RenderPath::new().with_fill_type(FillType::EvenOdd);
    outer.append_to(&mut path);
    inner.append_to(&mut path);
    canvas.draw_path(&path, &Paint::new(side.color));
}

fn paint_uniform_border_with_circle(canvas: &mut Canvas, rect: Rect, side: BorderSide) {
    let radius = (rect_shortest_side(rect) + side.stroke_offset()) / 2.0;
    let center = rect_center(rect);
    canvas.draw_circle(center.dx, center.dy, radius, &side.to_paint());
}

fn paint_uniform_border_with_rectangle(canvas: &mut Canvas, rect: Rect, side: BorderSide) {
    canvas.draw_rect(
        rect_inflate(rect, side.stroke_offset() / 2.0),
        &side.to_paint(),
    );
}

/// `BoxBorder.paintNonUniformBorder`: different widths per side, one colour,
/// any radius or circle.
fn paint_non_uniform_border(
    canvas: &mut Canvas,
    rect: Rect,
    top: BorderSide,
    right: BorderSide,
    bottom: BorderSide,
    left: BorderSide,
    color: Color,
    border_radius: Option<BorderRadius>,
    shape: BoxShape,
) {
    let border_rect = match shape {
        BoxShape::Rectangle => border_radius.unwrap_or(BorderRadius::ZERO).to_rrect(rect),
        BoxShape::Circle => {
            // Upstream: the oval of the shortest side, inflated to a
            // stadium by a radius as wide as the rect -- an oval after
            // radius clamping.
            let center = rect_center(rect);
            let radius = rect_shortest_side(rect) / 2.0;
            let circle = Rect::xywh(
                center.dx - radius,
                center.dy - radius,
                radius * 2.0,
                radius * 2.0,
            );
            RRect::from_rect_and_radius(circle, Radius::circular(rect.width()))
        }
    };
    let inner = border_rect.inset_insets(
        left.stroke_inset(),
        top.stroke_inset(),
        right.stroke_inset(),
        bottom.stroke_inset(),
    );
    let outer = border_rect.inset_insets(
        -left.stroke_outset(),
        -top.stroke_outset(),
        -right.stroke_outset(),
        -bottom.stroke_outset(),
    );
    let mut path = RenderPath::new().with_fill_type(FillType::EvenOdd);
    outer.append_to(&mut path);
    inner.append_to(&mut path);
    canvas.draw_path(&path, &Paint::new(color));
}

// -- Outlined shapes (one struct per upstream file) ------------------------------

/// `painting/rounded_rectangle_border.dart`: a rectangle with rounded
/// corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedRectangleBorder {
    pub side: BorderSide,
    pub border_radius: BorderRadiusGeometry,
}

impl Default for RoundedRectangleBorder {
    fn default() -> Self {
        RoundedRectangleBorder {
            side: BorderSide::NONE,
            border_radius: BorderRadiusGeometry::Zero,
        }
    }
}

impl RoundedRectangleBorder {
    pub fn new(side: BorderSide, border_radius: BorderRadiusGeometry) -> Self {
        RoundedRectangleBorder {
            side,
            border_radius,
        }
    }

    pub fn resolved_radius(&self, direction: TextDirection) -> BorderRadius {
        self.border_radius.resolve(direction)
    }

    pub fn outer_rrect(&self, rect: Rect, direction: TextDirection) -> RRect {
        self.resolved_radius(direction).to_rrect(rect)
    }

    pub fn inner_rrect(&self, rect: Rect, direction: TextDirection) -> RRect {
        self.outer_rrect(rect, direction)
            .deflate(self.side.stroke_inset())
    }
}

/// `painting/rounded_rectangle_border.dart`: the iOS-style smooth-cornered
/// rectangle. Divergence: drawn with `ContinuousRectangleBorder`'s cubic
/// corners because the engine has no `RSuperellipse`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedSuperellipseBorder {
    pub side: BorderSide,
    pub border_radius: BorderRadiusGeometry,
}

impl Default for RoundedSuperellipseBorder {
    fn default() -> Self {
        RoundedSuperellipseBorder {
            side: BorderSide::NONE,
            border_radius: BorderRadiusGeometry::Zero,
        }
    }
}

impl RoundedSuperellipseBorder {
    pub fn new(side: BorderSide, border_radius: BorderRadiusGeometry) -> Self {
        RoundedSuperellipseBorder {
            side,
            border_radius,
        }
    }

    fn zero_radius(&self) -> bool {
        self.border_radius.is_zero()
    }
}

/// `painting/circle_border.dart`: a circle that fits the space, deforming
/// toward an oval as `eccentricity` grows from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircleBorder {
    pub side: BorderSide,
    pub eccentricity: f32,
}

impl Default for CircleBorder {
    fn default() -> Self {
        CircleBorder {
            side: BorderSide::NONE,
            eccentricity: 0.0,
        }
    }
}

impl CircleBorder {
    pub fn new(side: BorderSide, eccentricity: f32) -> Self {
        debug_assert!((0.0..=1.0).contains(&eccentricity));
        CircleBorder { side, eccentricity }
    }

    /// Upstream `CircleBorder._adjustRect`.
    pub fn adjust_rect(&self, rect: Rect) -> Rect {
        if self.eccentricity == 0.0 || rect.width() == rect.height() {
            let radius = rect_shortest_side(rect) / 2.0;
            let center = rect_center(rect);
            return Rect::xywh(
                center.dx - radius,
                center.dy - radius,
                radius * 2.0,
                radius * 2.0,
            );
        }
        if rect.width() < rect.height() {
            let delta = (1.0 - self.eccentricity) * (rect.height() - rect.width()) / 2.0;
            Rect::ltrb(rect.left, rect.top + delta, rect.right, rect.bottom - delta)
        } else {
            let delta = (1.0 - self.eccentricity) * (rect.width() - rect.height()) / 2.0;
            Rect::ltrb(rect.left + delta, rect.top, rect.right - delta, rect.bottom)
        }
    }
}

/// `painting/oval_border.dart`: `CircleBorder` with the eccentricity pinned
/// at 1.0 -- an oval touching every edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OvalBorder {
    pub side: BorderSide,
}

impl Default for OvalBorder {
    fn default() -> Self {
        OvalBorder {
            side: BorderSide::NONE,
        }
    }
}

impl OvalBorder {
    pub fn new(side: BorderSide) -> Self {
        OvalBorder { side }
    }
}

/// `painting/stadium_border.dart`: a box with semicircular ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StadiumBorder {
    pub side: BorderSide,
}

impl Default for StadiumBorder {
    fn default() -> Self {
        StadiumBorder {
            side: BorderSide::NONE,
        }
    }
}

/// What a stadium becomes part-way to another shape.
///
/// Upstream does not interpolate the two outlines. It builds a **third
/// shape**, parameterised by how far along it is, and that shape knows how to
/// draw itself at any point between: `_StadiumToCircleBorder` and
/// `_StadiumToRoundedRectangleBorder`. Interpolating paths point by point
/// would need the two to have the same points in the same order, which a
/// stadium and a circle do not.
///
/// This carries the **decision** -- which shape, with which parameter -- and
/// not the intermediate outlines themselves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StadiumLerp {
    /// Both ends were stadiums, so the answer is one too: only the side moves.
    Stadium(StadiumBorder),
    /// Upstream `_StadiumToCircleBorder`.
    ToCircle {
        side: BorderSide,
        /// **How circular**, not `t`: 0 is the stadium and 1 is the circle.
        circularity: f32,
        /// Taken from whichever operand was the circle -- a stadium has no
        /// eccentricity of its own to lerp with.
        eccentricity: f32,
    },
    /// Upstream `_StadiumToRoundedRectangleBorder`.
    ToRoundedRectangle {
        side: BorderSide,
        /// **How rectilinear**: 0 is the stadium and 1 is the rounded rect.
        rectilinearity: f32,
        /// From whichever operand was the rounded rectangle.
        border_radius: BorderRadiusGeometry,
    },
    /// Neither of the shapes upstream knows how to meet: `super.lerpFrom`,
    /// which fades one out and the other in rather than morphing.
    NotSpecial,
}

/// The other end of a lerp, as far as [`StadiumBorder`] cares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LerpPartner {
    Stadium(StadiumBorder),
    Circle(CircleBorder),
    RoundedRectangle(RoundedRectangleBorder),
    /// Anything else, including nothing at all.
    Other,
}

impl StadiumBorder {
    pub fn new(side: BorderSide) -> Self {
        StadiumBorder { side }
    }

    /// Upstream `scale`: the side scales and the shape does not.
    ///
    /// A stadium's radius is its own height, so there is nothing else to
    /// scale -- which is why this is one line here and several in the shapes
    /// that carry a radius of their own.
    pub fn scale(&self, t: f32) -> StadiumBorder {
        StadiumBorder {
            side: self.side.scale(t),
        }
    }

    /// Upstream `copyWith`.
    pub fn copy_with(&self, side: Option<BorderSide>) -> StadiumBorder {
        StadiumBorder {
            side: side.unwrap_or(self.side),
        }
    }

    /// Upstream `preferPaintInterior`, which is `true` here.
    ///
    /// A stadium **is** a rounded rectangle, so the canvas can fill it in one
    /// call rather than being handed a path to fill. The flag is what lets a
    /// caller take the cheap route and it is only true for the shapes that
    /// have one.
    pub fn prefer_paint_interior(&self) -> bool {
        true
    }

    /// Upstream `hitTest`: inside the rounded rectangle, corners and all.
    ///
    /// The corners matter -- a press just outside the curve at the end of a
    /// pill is a press on whatever is behind it, and testing the bounding
    /// rectangle instead would swallow it.
    pub fn hit_test(&self, rect: Rect, position: Offset) -> bool {
        stadium_rrect(rect).contains(position)
    }

    /// Upstream `lerpFrom`: this stadium is the **destination**, `a` the
    /// start, and `t` runs from `a` to here.
    ///
    /// ```dart
    /// if (a is CircleBorder) {
    ///   return _StadiumToCircleBorder(side: ..., circularity: 1.0 - t, eccentricity: a.eccentricity);
    /// }
    /// ```
    ///
    /// **`circularity` is `1.0 - t` here and `t` in [`StadiumBorder::lerp_to`]**,
    /// and that is not a sign error to tidy away. The parameter always means
    /// *how circular*, so it has to count from whichever end the circle is at:
    /// coming **from** a circle it starts at 1 and falls, going **to** one it
    /// starts at 0 and rises. Only `t` changes direction.
    pub fn lerp_from(&self, a: LerpPartner, t: f32) -> StadiumLerp {
        match a {
            LerpPartner::Stadium(a) => {
                StadiumLerp::Stadium(StadiumBorder::new(BorderSide::lerp(a.side, self.side, t)))
            }
            LerpPartner::Circle(a) => StadiumLerp::ToCircle {
                side: BorderSide::lerp(a.side, self.side, t),
                circularity: 1.0 - t,
                eccentricity: a.eccentricity,
            },
            LerpPartner::RoundedRectangle(a) => StadiumLerp::ToRoundedRectangle {
                side: BorderSide::lerp(a.side, self.side, t),
                rectilinearity: 1.0 - t,
                border_radius: a.border_radius,
            },
            LerpPartner::Other => StadiumLerp::NotSpecial,
        }
    }

    /// Upstream `lerpTo`: this stadium is the **start** and `b` the
    /// destination. See [`StadiumBorder::lerp_from`] for why the parameter is
    /// `t` here and `1.0 - t` there.
    pub fn lerp_to(&self, b: LerpPartner, t: f32) -> StadiumLerp {
        match b {
            LerpPartner::Stadium(b) => {
                StadiumLerp::Stadium(StadiumBorder::new(BorderSide::lerp(self.side, b.side, t)))
            }
            LerpPartner::Circle(b) => StadiumLerp::ToCircle {
                side: BorderSide::lerp(self.side, b.side, t),
                circularity: t,
                eccentricity: b.eccentricity,
            },
            LerpPartner::RoundedRectangle(b) => StadiumLerp::ToRoundedRectangle {
                side: BorderSide::lerp(self.side, b.side, t),
                rectilinearity: t,
                border_radius: b.border_radius,
            },
            LerpPartner::Other => StadiumLerp::NotSpecial,
        }
    }
}

pub(crate) fn stadium_rrect(rect: Rect) -> RRect {
    RRect::from_rect_and_radius(rect, Radius::circular(rect_shortest_side(rect) / 2.0))
}

/// `painting/beveled_rectangle_border.dart`: corners cut by straight lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeveledRectangleBorder {
    pub side: BorderSide,
    pub border_radius: BorderRadiusGeometry,
}

impl Default for BeveledRectangleBorder {
    fn default() -> Self {
        BeveledRectangleBorder {
            side: BorderSide::NONE,
            border_radius: BorderRadiusGeometry::Zero,
        }
    }
}

impl BeveledRectangleBorder {
    pub fn new(side: BorderSide, border_radius: BorderRadiusGeometry) -> Self {
        BeveledRectangleBorder {
            side,
            border_radius,
        }
    }
}

/// Upstream `BeveledRectangleBorder._getPath`: the eight vertices where each
/// side stops, offset by its corner radii but never past the side's centre.
pub(crate) fn beveled_path(rrect: RRect) -> RenderPath {
    let vertices = beveled_vertices(rrect);
    let mut path = RenderPath::new();
    path.move_to(vertices[0].0, vertices[0].1);
    for (x, y) in vertices.iter().skip(1) {
        path.line_to(*x, *y);
    }
    path.close();
    path
}

/// Even-odd point-in-polygon, for the beveled outline's hit test.
fn polygon_contains(vertices: &[(f32, f32)], position: Offset) -> bool {
    let mut inside = false;
    let mut j = vertices.len() - 1;
    for i in 0..vertices.len() {
        let (xi, yi) = vertices[i];
        let (xj, yj) = vertices[j];
        if (yi > position.dy) != (yj > position.dy)
            && position.dx < (xj - xi) * (position.dy - yi) / (yj - yi) + xi
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn beveled_vertices(rrect: RRect) -> [(f32, f32); 8] {
    let rect = rrect.rect;
    let center = rect_center(rect);
    let [tl, tr, br, bl] = rrect.scaled();
    [
        (rect.left, center.dy.min(rect.top + tl.y)),
        (center.dx.min(rect.left + tl.x), rect.top),
        (center.dx.max(rect.right - tr.x), rect.top),
        (rect.right, center.dy.min(rect.top + tr.y)),
        (rect.right, center.dy.max(rect.bottom - br.y)),
        (center.dx.max(rect.right - br.x), rect.bottom),
        (center.dx.min(rect.left + bl.x), rect.bottom),
        (rect.left, center.dy.max(rect.bottom - bl.y)),
    ]
}

fn rect_is_empty(rect: Rect) -> bool {
    rect.right <= rect.left || rect.bottom <= rect.top
}

/// `painting/continuous_rectangle_border.dart`: straight sides that ease
/// into their corners. Upstream `ContinuousRectangleBorder._getPath`; also
/// the stand-in geometry for `RoundedSuperellipseBorder`.
pub(crate) fn continuous_path(rrect: RRect) -> RenderPath {
    let rect = rrect.rect;
    let (l, t, r, b) = (rect.left, rect.top, rect.right, rect.bottom);
    // Radii clamp to the shortest side to avoid tie-fighter shapes.
    let clamp = |value: f32| value.max(0.0).min(rrect.shortest_side());
    let [tl, tr, br, bl] = rrect.scaled();
    let tl = Radius::elliptical(clamp(tl.x), clamp(tl.y));
    let tr = Radius::elliptical(clamp(tr.x), clamp(tr.y));
    let br = Radius::elliptical(clamp(br.x), clamp(br.y));
    let bl = Radius::elliptical(clamp(bl.x), clamp(bl.y));

    let mut path = RenderPath::new();
    path.move_to(l, t + tl.x);
    path.cubic_to(l, t, l, t, l + tl.y, t);
    path.line_to(r - tr.x, t);
    path.cubic_to(r, t, r, t, r, t + tr.y);
    path.line_to(r, b - br.x);
    path.cubic_to(r, b, r, b, r - br.y, b);
    path.line_to(l + bl.x, b);
    path.cubic_to(l, b, l, b, l, b - bl.y);
    path.close();
    path
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContinuousRectangleBorder {
    pub side: BorderSide,
    pub border_radius: BorderRadiusGeometry,
}

impl Default for ContinuousRectangleBorder {
    fn default() -> Self {
        ContinuousRectangleBorder {
            side: BorderSide::NONE,
            border_radius: BorderRadiusGeometry::Zero,
        }
    }
}

impl ContinuousRectangleBorder {
    pub fn new(side: BorderSide, border_radius: BorderRadiusGeometry) -> Self {
        ContinuousRectangleBorder {
            side,
            border_radius,
        }
    }
}

/// Upstream `_StadiumToCircleBorder`: the shape while a stadium morphs into
/// a circle (`circularity` 0→1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StadiumToCircleBorder {
    pub side: BorderSide,
    pub circularity: f32,
    pub eccentricity: f32,
}

impl StadiumToCircleBorder {
    pub fn new(side: BorderSide, circularity: f32, eccentricity: f32) -> Self {
        StadiumToCircleBorder {
            side,
            circularity,
            eccentricity,
        }
    }

    /// Upstream `_StadiumToCircleBorder._adjustRect`.
    fn adjust_rect(&self, rect: Rect) -> Rect {
        if self.circularity == 0.0 || rect.width() == rect.height() {
            return rect;
        }
        if rect.width() < rect.height() {
            let delta = self.circularity
                * ((rect.height() - rect.width()) / 2.0)
                * (1.0 - self.eccentricity);
            Rect::ltrb(rect.left, rect.top + delta, rect.right, rect.bottom - delta)
        } else {
            let delta = self.circularity
                * ((rect.width() - rect.height()) / 2.0)
                * (1.0 - self.eccentricity);
            Rect::ltrb(rect.left + delta, rect.top, rect.right - delta, rect.bottom)
        }
    }

    /// Upstream `_StadiumToCircleBorder._adjustBorderRadius`.
    fn adjust_border_radius(&self, rect: Rect) -> BorderRadius {
        let circle_radius = BorderRadius::circular(rect_shortest_side(rect) / 2.0);
        if self.eccentricity != 0.0 {
            if rect.width() < rect.height() {
                return BorderRadius::lerp(
                    circle_radius,
                    BorderRadius::all(Radius::elliptical(
                        rect.width() / 2.0,
                        (0.5 + self.eccentricity / 2.0) * rect.height() / 2.0,
                    )),
                    self.circularity,
                );
            }
            return BorderRadius::lerp(
                circle_radius,
                BorderRadius::all(Radius::elliptical(
                    (0.5 + self.eccentricity / 2.0) * rect.width() / 2.0,
                    rect.height() / 2.0,
                )),
                self.circularity,
            );
        }
        circle_radius
    }

    fn rrect(&self, rect: Rect) -> RRect {
        self.adjust_border_radius(rect)
            .to_rrect(self.adjust_rect(rect))
    }
}

/// Upstream `_StadiumToRoundedRectangleBorder`: the shape while a stadium
/// morphs into a rounded rectangle (`rectilinearity` 0→1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StadiumToRoundedRectBorder {
    pub side: BorderSide,
    pub border_radius: BorderRadiusGeometry,
    pub rectilinearity: f32,
}

impl StadiumToRoundedRectBorder {
    pub fn new(side: BorderSide, border_radius: BorderRadiusGeometry, rectilinearity: f32) -> Self {
        StadiumToRoundedRectBorder {
            side,
            border_radius,
            rectilinearity,
        }
    }

    /// Upstream `_StadiumToRoundedRectangleBorder._adjustBorderRadius`.
    fn adjust_border_radius(&self, rect: Rect) -> BorderRadiusGeometry {
        BorderRadiusGeometry::lerp(
            self.border_radius,
            BorderRadiusGeometry::all(Radius::circular(rect_shortest_side(rect) / 2.0)),
            1.0 - self.rectilinearity,
        )
    }

    fn rrect(&self, rect: Rect, direction: TextDirection) -> RRect {
        self.adjust_border_radius(rect)
            .resolve(direction)
            .to_rrect(rect)
    }
}

/// Upstream `_ShapeToCircleBorder` family
/// (`_RoundedRectangleToCircleBorder`, `_RoundedSuperellipseToCircleBorder`):
/// the shape while a rounded rectangle morphs into a circle. `smooth_corners`
/// picks which end shape the paths follow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedToCircleBorder {
    pub side: BorderSide,
    pub border_radius: BorderRadiusGeometry,
    pub circularity: f32,
    pub eccentricity: f32,
    pub smooth_corners: bool,
}

impl RoundedToCircleBorder {
    pub fn new(
        side: BorderSide,
        border_radius: BorderRadiusGeometry,
        circularity: f32,
        eccentricity: f32,
        smooth_corners: bool,
    ) -> Self {
        RoundedToCircleBorder {
            side,
            border_radius,
            circularity,
            eccentricity,
            smooth_corners,
        }
    }

    /// Upstream `_ShapeToCircleBorder._adjustRect`.
    fn adjust_rect(&self, rect: Rect) -> Rect {
        if self.circularity == 0.0 || rect.width() == rect.height() {
            return rect;
        }
        if rect.width() < rect.height() {
            let delta = self.circularity
                * ((rect.height() - rect.width()) / 2.0)
                * (1.0 - self.eccentricity);
            Rect::ltrb(rect.left, rect.top + delta, rect.right, rect.bottom - delta)
        } else {
            let delta = self.circularity
                * ((rect.width() - rect.height()) / 2.0)
                * (1.0 - self.eccentricity);
            Rect::ltrb(rect.left + delta, rect.top, rect.right - delta, rect.bottom)
        }
    }

    /// Upstream `_ShapeToCircleBorder._adjustBorderRadius`.
    fn adjust_border_radius(&self, rect: Rect, direction: TextDirection) -> BorderRadius {
        let resolved = self.border_radius.resolve(direction);
        if self.circularity == 0.0 {
            return resolved;
        }
        if self.eccentricity != 0.0 {
            if rect.width() < rect.height() {
                return BorderRadius::lerp(
                    resolved,
                    BorderRadius::all(Radius::elliptical(
                        rect.width() / 2.0,
                        (0.5 + self.eccentricity / 2.0) * rect.height() / 2.0,
                    )),
                    self.circularity,
                );
            }
            return BorderRadius::lerp(
                resolved,
                BorderRadius::all(Radius::elliptical(
                    (0.5 + self.eccentricity / 2.0) * rect.width() / 2.0,
                    rect.height() / 2.0,
                )),
                self.circularity,
            );
        }
        BorderRadius::lerp(
            resolved,
            BorderRadius::circular(rect_shortest_side(rect) / 2.0),
            self.circularity,
        )
    }

    fn rrect(&self, rect: Rect, direction: TextDirection) -> RRect {
        self.adjust_border_radius(rect, direction)
            .to_rrect(self.adjust_rect(rect))
    }
}

// -- ShapeBorder: the closed set of shapes ---------------------------------------

/// Upstream `ShapeBorder` and every concrete subclass, as one enum -- plus
/// `_CompoundBorder` as a variant and the private transition shapes as
// -- Input borders (upstream `material/input_border.dart`) --------------------

/// Upstream `UnderlineInputBorder`: a rule under a text field, with the top
/// two corners rounded so the filled shape above it reads as one box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnderlineInputBorder {
    pub side: BorderSide,
    pub border_radius: BorderRadius,
}

impl UnderlineInputBorder {
    /// Upstream's default: four at the top, square at the bottom, so the rule
    /// meets the fill's edges flush.
    pub fn new(side: BorderSide) -> UnderlineInputBorder {
        UnderlineInputBorder {
            side,
            border_radius: BorderRadius::only(
                Radius::circular(4.0),
                Radius::circular(4.0),
                Radius::ZERO,
                Radius::ZERO,
            ),
        }
    }

    pub fn with_border_radius(mut self, border_radius: BorderRadius) -> Self {
        self.border_radius = border_radius;
        self
    }

    /// Upstream `getOuterPath`: the whole rounded box, not the rule -- the
    /// path is what the field is clipped and filled to.
    pub fn outer_path(&self, rect: Rect) -> RenderPath {
        self.border_radius.to_rrect(rect).to_path()
    }

    /// Upstream `getInnerPath`: the box less the rule's width, which is what
    /// the fill stops at.
    pub fn inner_rect(&self, rect: Rect) -> Rect {
        Rect::ltrb(
            rect.left,
            rect.top,
            rect.right,
            rect.top + (rect.height() - self.side.width).max(0.0),
        )
    }

    /// Upstream `paint`: the rule alone, along the bottom edge.
    ///
    /// Upstream clamps the two bottom radii to half the height before
    /// drawing, "to prevent the border from leaking the color due to
    /// anti-aliasing rounding errors"; the same clamp is here.
    pub fn paint(&self, canvas: &mut Canvas, rect: Rect) {
        if self.side.style == BorderStyle::None {
            return;
        }
        let half_height = Radius::circular(rect.height() / 2.0);
        let bottom_left = clamp_radius(self.border_radius.bottom_left, half_height);
        let bottom_right = clamp_radius(self.border_radius.bottom_right, half_height);
        if bottom_left != Radius::ZERO || bottom_right != Radius::ZERO {
            // A rounded rule is the bottom of a rounded rect, so it is drawn
            // as the stroke of one whose top corners are square.
            let rounded = BorderRadius::only(Radius::ZERO, Radius::ZERO, bottom_left, bottom_right);
            let inset = self.side.width / 2.0;
            let stroked = Rect::ltrb(
                rect.left + inset,
                rect.top,
                rect.right - inset,
                rect.bottom - inset,
            );
            canvas.draw_path(&rounded.to_rrect(stroked).to_path(), &self.side.to_paint());
            return;
        }
        let y = rect.bottom - self.side.width / 2.0;
        let mut path = RenderPath::new();
        path.move_to(rect.left, y);
        path.line_to(rect.right, y);
        canvas.draw_path(&path, &self.side.to_paint());
    }

    /// Upstream `scale`.
    pub fn scale(&self, t: f32) -> UnderlineInputBorder {
        UnderlineInputBorder {
            side: self.side.scale(t),
            border_radius: scale_border_radius(self.border_radius, t),
        }
    }

    /// Upstream `lerpFrom` / `lerpTo` between two of these.
    pub fn lerp(
        a: &UnderlineInputBorder,
        b: &UnderlineInputBorder,
        t: f32,
    ) -> UnderlineInputBorder {
        UnderlineInputBorder {
            side: BorderSide::lerp(a.side, b.side, t),
            border_radius: BorderRadius::lerp(a.border_radius, b.border_radius, t),
        }
    }
}

/// Upstream `OutlineInputBorder`: a rounded rectangle around the field, with
/// a gap in its top edge for the floating label to sit in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineInputBorder {
    pub side: BorderSide,
    pub border_radius: BorderRadius,
    /// How much clear space the gap leaves on each side of the label.
    pub gap_padding: f32,
}

impl OutlineInputBorder {
    pub fn new(side: BorderSide) -> OutlineInputBorder {
        OutlineInputBorder {
            side,
            // Upstream's default: four all round.
            border_radius: BorderRadius::circular(4.0),
            gap_padding: 4.0,
        }
    }

    pub fn with_border_radius(mut self, border_radius: BorderRadius) -> Self {
        self.border_radius = border_radius;
        self
    }

    pub fn with_gap_padding(mut self, gap_padding: f32) -> Self {
        debug_assert!(gap_padding >= 0.0, "a gap's padding is not negative");
        self.gap_padding = gap_padding;
        self
    }

    /// Upstream `getOuterPath`.
    pub fn outer_path(&self, rect: Rect) -> RenderPath {
        self.border_radius.to_rrect(rect).to_path()
    }

    /// Upstream `paint`, with the gap the floating label sits in.
    ///
    /// `gap_start` is where the label begins along the top edge and
    /// `gap_extent` how wide it is; `gap_percentage` is how far the label has
    /// floated up, so that the gap opens as the label rises rather than
    /// appearing all at once. No gap at all is the plain rounded rectangle.
    pub fn paint_with_gap(
        &self,
        canvas: &mut Canvas,
        rect: Rect,
        gap_start: Option<f32>,
        gap_extent: f32,
        gap_percentage: f32,
    ) {
        debug_assert!((0.0..=1.0).contains(&gap_percentage));
        let paint = self.side.to_paint();
        // Upstream deflates by half the stroke, so the border is drawn on the
        // rectangle rather than half outside it.
        let deflated = rect_inflate(rect, -self.side.width / 2.0);
        let Some(gap_start) = gap_start else {
            canvas.draw_path(&self.border_radius.to_rrect(deflated).to_path(), &paint);
            return;
        };
        if gap_extent <= 0.0 || gap_percentage == 0.0 {
            canvas.draw_path(&self.border_radius.to_rrect(deflated).to_path(), &paint);
            return;
        }
        let extent = (gap_extent + self.gap_padding * 2.0) * gap_percentage;
        let start = (gap_start - self.gap_padding).max(0.0);
        canvas.draw_path(&self.gap_path(deflated, start, extent), &paint);
    }

    /// Upstream `_gapBorderPath`: the rounded rectangle walked as an open
    /// path, from the far side of the gap round to the near side of it.
    fn gap_path(&self, rect: Rect, start: f32, extent: f32) -> RenderPath {
        let radius = self.border_radius.to_rrect(rect);
        let mut path = RenderPath::new();
        // The top edge, right of the gap, to the top-right corner.
        let gap_end = (start + extent).min(rect.right - rect.left);
        path.move_to(rect.left + gap_end, rect.top);
        path.line_to(rect.right - radius.top_right.x, rect.top);
        path.quadratic_to(
            rect.right,
            rect.top,
            rect.right,
            rect.top + radius.top_right.y,
        );
        // Down the right side, along the bottom, up the left side.
        path.line_to(rect.right, rect.bottom - radius.bottom_right.y);
        path.quadratic_to(
            rect.right,
            rect.bottom,
            rect.right - radius.bottom_right.x,
            rect.bottom,
        );
        path.line_to(rect.left + radius.bottom_left.x, rect.bottom);
        path.quadratic_to(
            rect.left,
            rect.bottom,
            rect.left,
            rect.bottom - radius.bottom_left.y,
        );
        path.line_to(rect.left, rect.top + radius.top_left.y);
        path.quadratic_to(rect.left, rect.top, rect.left + radius.top_left.x, rect.top);
        // And back along the top edge to where the gap starts.
        path.line_to(rect.left + start, rect.top);
        path
    }

    /// Upstream `scale`.
    pub fn scale(&self, t: f32) -> OutlineInputBorder {
        OutlineInputBorder {
            side: self.side.scale(t),
            border_radius: scale_border_radius(self.border_radius, t),
            gap_padding: self.gap_padding,
        }
    }

    /// Upstream `lerpFrom` / `lerpTo` between two of these.
    ///
    /// `gapPadding` is not interpolated: upstream asserts the two are equal
    /// and keeps `a`'s, because a gap that changed width mid-animation would
    /// slide the label's clearance out from under it.
    pub fn lerp(a: &OutlineInputBorder, b: &OutlineInputBorder, t: f32) -> OutlineInputBorder {
        OutlineInputBorder {
            side: BorderSide::lerp(a.side, b.side, t),
            border_radius: BorderRadius::lerp(a.border_radius, b.border_radius, t),
            gap_padding: a.gap_padding,
        }
    }
}

/// A radius set scaled -- upstream's `BorderRadius * t`.
fn scale_border_radius(radius: BorderRadius, t: f32) -> BorderRadius {
    let scale = |r: Radius| Radius {
        x: r.x * t,
        y: r.y * t,
    };
    BorderRadius::only(
        scale(radius.top_left),
        scale(radius.top_right),
        scale(radius.bottom_left),
        scale(radius.bottom_right),
    )
}

/// A radius clamped to a maximum on both axes -- upstream's
/// `Radius.clamp(maximum:)`.
fn clamp_radius(radius: Radius, maximum: Radius) -> Radius {
    Radius {
        x: radius.x.min(maximum.x),
        y: radius.y.min(maximum.y),
    }
}

/// variants, so `lerp` keeps upstream's exact morph arithmetic.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeBorder {
    Border(Border),
    Directional(BorderDirectional),
    Rounded(RoundedRectangleBorder),
    /// `RoundedSuperellipseBorder`; see the module docs for the corner
    /// approximation.
    Superellipse(RoundedSuperellipseBorder),
    Circle(CircleBorder),
    Oval(OvalBorder),
    Stadium(StadiumBorder),
    Beveled(BeveledRectangleBorder),
    Continuous(ContinuousRectangleBorder),
    /// `LinearBorder`, the zero-to-four-lines border.
    Linear(LinearBorder),
    /// `StarBorder`, stars and polygons.
    Star(StarBorder),
    /// `UnderlineInputBorder`: a rule under a text field.
    Underline(UnderlineInputBorder),
    /// `OutlineInputBorder`: a box round one, with a gap for the label.
    Outline(OutlineInputBorder),
    /// `_StadiumToCircleBorder`.
    StadiumToCircle(StadiumToCircleBorder),
    /// `_StadiumToRoundedRectangleBorder`.
    StadiumToRoundedRect(StadiumToRoundedRectBorder),
    /// `_RoundedRectangleToCircleBorder` / `_RoundedSuperellipseToCircleBorder`.
    RoundedToCircle(RoundedToCircleBorder),
    /// `_CompoundBorder`: borders listed outside-to-inside.
    Compound(Vec<ShapeBorder>),
}

impl ShapeBorder {
    /// Upstream `ShapeBorder.dimensions` -- how far a rectangle insets to
    /// keep clear of the border.
    pub fn dimensions(&self) -> EdgeInsetsGeometry {
        let outlined = |side: &BorderSide| {
            EdgeInsetsGeometry::Absolute(EdgeInsets::all(side.stroke_inset().max(0.0)))
        };
        match self {
            ShapeBorder::Border(border) => border.dimensions(),
            ShapeBorder::Directional(border) => border.dimensions(),
            ShapeBorder::Rounded(shape) => outlined(&shape.side),
            ShapeBorder::Superellipse(shape) => outlined(&shape.side),
            ShapeBorder::Circle(shape) => outlined(&shape.side),
            ShapeBorder::Oval(shape) => outlined(&shape.side),
            ShapeBorder::Stadium(shape) => outlined(&shape.side),
            ShapeBorder::Linear(shape) => shape.dimensions(),
            // Upstream `InputBorder.dimensions`: the rule insets the bottom
            // alone, the outline insets all four.
            ShapeBorder::Underline(shape) => EdgeInsetsGeometry::Absolute(EdgeInsets {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: shape.side.width,
            }),
            ShapeBorder::Outline(shape) => outlined(&shape.side),
            ShapeBorder::Star(shape) => outlined(&shape.side),
            ShapeBorder::Beveled(shape) => outlined(&shape.side),
            // Upstream `ContinuousRectangleBorder.dimensions`.
            ShapeBorder::Continuous(shape) => {
                EdgeInsetsGeometry::Absolute(EdgeInsets::all(shape.side.width))
            }
            ShapeBorder::StadiumToCircle(shape) => outlined(&shape.side),
            ShapeBorder::StadiumToRoundedRect(shape) => outlined(&shape.side),
            ShapeBorder::RoundedToCircle(shape) => outlined(&shape.side),
            ShapeBorder::Compound(borders) => borders
                .iter()
                .map(ShapeBorder::dimensions)
                .fold(EdgeInsetsGeometry::Zero, EdgeInsetsGeometry::add),
        }
    }

    /// The `side` of an `OutlinedBorder`, for callers that adjust their
    /// interior by it (`ShapeDecoration` does).
    pub fn outlined_side(&self) -> Option<BorderSide> {
        match self {
            ShapeBorder::Rounded(shape) => Some(shape.side),
            ShapeBorder::Superellipse(shape) => Some(shape.side),
            ShapeBorder::Circle(shape) => Some(shape.side),
            ShapeBorder::Oval(shape) => Some(shape.side),
            ShapeBorder::Stadium(shape) => Some(shape.side),
            ShapeBorder::Linear(shape) => Some(shape.side),
            ShapeBorder::Underline(shape) => Some(shape.side),
            ShapeBorder::Outline(shape) => Some(shape.side),
            ShapeBorder::Star(shape) => Some(shape.side),
            ShapeBorder::Beveled(shape) => Some(shape.side),
            ShapeBorder::Continuous(shape) => Some(shape.side),
            ShapeBorder::StadiumToCircle(shape) => Some(shape.side),
            ShapeBorder::StadiumToRoundedRect(shape) => Some(shape.side),
            ShapeBorder::RoundedToCircle(shape) => Some(shape.side),
            ShapeBorder::Border(_) | ShapeBorder::Directional(_) | ShapeBorder::Compound(_) => None,
        }
    }

    /// Upstream `getOuterPath`.
    pub fn outer_path(&self, rect: Rect, direction: TextDirection) -> RenderPath {
        match self {
            ShapeBorder::Border(_) | ShapeBorder::Directional(_) => {
                let mut path = RenderPath::new();
                path.add_rect(rect);
                path
            }
            ShapeBorder::Rounded(shape) => shape.outer_rrect(rect, direction).to_path(),
            ShapeBorder::Superellipse(shape) => {
                if shape.zero_radius() {
                    let mut path = RenderPath::new();
                    path.add_rect(rect);
                    path
                } else {
                    continuous_path(shape.border_radius.resolve(direction).to_rrect(rect))
                }
            }
            ShapeBorder::Circle(shape) => oval_path(shape.adjust_rect(rect)),
            ShapeBorder::Oval(shape) => {
                oval_path(CircleBorder::new(shape.side, 1.0).adjust_rect(rect))
            }
            ShapeBorder::Stadium(_) => stadium_rrect(rect).to_path(),
            // Upstream `LinearBorder.getOuterPath` is the whole rect.
            ShapeBorder::Linear(_) => {
                let mut path = RenderPath::new();
                path.add_rect(rect);
                path
            }
            ShapeBorder::Star(shape) => shape.generator().generate(rect),
            ShapeBorder::Beveled(shape) => {
                beveled_path(shape.border_radius.resolve(direction).to_rrect(rect))
            }
            ShapeBorder::Continuous(shape) => {
                continuous_path(shape.border_radius.resolve(direction).to_rrect(rect))
            }
            ShapeBorder::StadiumToCircle(shape) => shape.rrect(rect).to_path(),
            ShapeBorder::StadiumToRoundedRect(shape) => shape.rrect(rect, direction).to_path(),
            ShapeBorder::RoundedToCircle(shape) => {
                let rrect = shape.rrect(rect, direction);
                if shape.smooth_corners {
                    continuous_path(rrect)
                } else {
                    rrect.to_path()
                }
            }
            ShapeBorder::Underline(shape) => shape.outer_path(rect),
            ShapeBorder::Outline(shape) => shape.outer_path(rect),
            ShapeBorder::Compound(borders) => borders[0].outer_path(rect, direction),
        }
    }

    /// Upstream `getInnerPath`.
    pub fn inner_path(&self, rect: Rect, direction: TextDirection) -> RenderPath {
        match self {
            ShapeBorder::Border(border) => {
                let mut path = RenderPath::new();
                path.add_rect(rect_deflate_insets(
                    rect,
                    border.dimensions().resolve(direction),
                ));
                path
            }
            ShapeBorder::Directional(border) => {
                let mut path = RenderPath::new();
                path.add_rect(rect_deflate_insets(
                    rect,
                    border.dimensions().resolve(direction),
                ));
                path
            }
            ShapeBorder::Rounded(shape) => shape.inner_rrect(rect, direction).to_path(),
            ShapeBorder::Superellipse(shape) => {
                if shape.zero_radius() {
                    let mut path = RenderPath::new();
                    path.add_rect(rect_deflate(rect, shape.side.stroke_inset()));
                    path
                } else {
                    continuous_path(
                        shape
                            .border_radius
                            .resolve(direction)
                            .to_rrect(rect)
                            .deflate(shape.side.stroke_inset()),
                    )
                }
            }
            ShapeBorder::Circle(shape) => oval_path(rect_deflate(
                shape.adjust_rect(rect),
                shape.side.stroke_inset(),
            )),
            ShapeBorder::Oval(shape) => oval_path(rect_deflate(rect, shape.side.stroke_inset())),
            ShapeBorder::Stadium(shape) => stadium_rrect(rect)
                .deflate(shape.side.stroke_inset())
                .to_path(),
            // Upstream `LinearBorder.getInnerPath`: the rect deflated by
            // whichever edges are present.
            ShapeBorder::Linear(shape) => {
                let mut path = RenderPath::new();
                path.add_rect(rect_deflate_insets(
                    rect,
                    shape.dimensions().resolve(direction),
                ));
                path
            }
            ShapeBorder::Star(shape) => shape
                .generator()
                .generate(rect_deflate(rect, shape.side.stroke_inset())),
            ShapeBorder::Beveled(shape) => beveled_path(
                shape
                    .border_radius
                    .resolve(direction)
                    .to_rrect(rect)
                    .deflate(shape.side.stroke_inset()),
            ),
            ShapeBorder::Continuous(shape) => continuous_path(
                shape
                    .border_radius
                    .resolve(direction)
                    .to_rrect(rect)
                    .deflate(shape.side.width),
            ),
            // Upstream `UnderlineInputBorder.getInnerPath` is the box less
            // the rule's width; the outline's is the box deflated all round.
            ShapeBorder::Underline(shape) => {
                let mut path = RenderPath::new();
                path.add_rect(shape.inner_rect(rect));
                path
            }
            ShapeBorder::Outline(shape) => shape
                .border_radius
                .to_rrect(rect)
                .deflate(shape.side.width)
                .to_path(),
            ShapeBorder::StadiumToCircle(shape) => shape
                .rrect(rect)
                .deflate(shape.side.stroke_inset())
                .to_path(),
            ShapeBorder::StadiumToRoundedRect(shape) => shape
                .rrect(rect, direction)
                .deflate(lerp_double(shape.side.width, 0.0, shape.side.stroke_align))
                .to_path(),
            ShapeBorder::RoundedToCircle(shape) => shape
                .rrect(rect, direction)
                .inflate(-lerp_double(shape.side.width, 0.0, shape.side.stroke_align))
                .to_path(),
            ShapeBorder::Compound(borders) => {
                // Fold every outer border's dimensions off the rect, then
                // take the innermost border's inner path.
                let mut rect = rect;
                for border in &borders[..borders.len() - 1] {
                    rect = border.dimensions().deflate_rect(rect, direction);
                }
                borders[borders.len() - 1].inner_path(rect, direction)
            }
        }
    }

    /// Upstream `ShapeBorder.hitTest`.
    pub fn hit_test(&self, rect: Rect, position: Offset, direction: TextDirection) -> bool {
        match self {
            ShapeBorder::Border(_) | ShapeBorder::Directional(_) => rect_contains(rect, position),
            ShapeBorder::Rounded(shape) => {
                let radius = shape.resolved_radius(direction);
                if radius.is_zero() {
                    rect_contains(rect, position)
                } else {
                    radius.to_rrect(rect).contains(position)
                }
            }
            ShapeBorder::Superellipse(shape) => {
                let radius = shape.border_radius.resolve(direction);
                if radius.is_zero() {
                    rect_contains(rect, position)
                } else {
                    radius.to_rrect(rect).contains(position)
                }
            }
            ShapeBorder::Circle(shape) => ellipse_contains(shape.adjust_rect(rect), position),
            ShapeBorder::Oval(_) => ellipse_contains(rect, position),
            ShapeBorder::Stadium(_) => stadium_rrect(rect).contains(position),
            // The outer path is the whole rectangle.
            ShapeBorder::Linear(_) => rect_contains(rect, position),
            // Both input borders are the field's own rectangle as far as a
            // finger is concerned: upstream's `getOuterPath` for each is the
            // rounded box, not the rule.
            ShapeBorder::Underline(shape) => shape.border_radius.to_rrect(rect).contains(position),
            ShapeBorder::Outline(shape) => shape.border_radius.to_rrect(rect).contains(position),
            ShapeBorder::Star(shape) => polygon_contains(&star_hit_vertices(shape, rect), position),
            ShapeBorder::Beveled(shape) => polygon_contains(
                &beveled_vertices(shape.border_radius.resolve(direction).to_rrect(rect)),
                position,
            ),
            // The continuous corner is within a hair of its rounding
            // circle; the rrect test is the honest approximation.
            ShapeBorder::Continuous(shape) => {
                let radius = shape.border_radius.resolve(direction);
                if radius.is_zero() {
                    rect_contains(rect, position)
                } else {
                    radius.to_rrect(rect).contains(position)
                }
            }
            ShapeBorder::StadiumToCircle(shape) => shape.rrect(rect).contains(position),
            ShapeBorder::StadiumToRoundedRect(shape) => {
                let radius = shape.adjust_border_radius(rect).resolve(direction);
                if radius.is_zero() {
                    rect_contains(rect, position)
                } else {
                    radius.to_rrect(rect).contains(position)
                }
            }
            ShapeBorder::RoundedToCircle(shape) => {
                let radius = shape.adjust_border_radius(rect, direction);
                if radius == BorderRadius::ZERO {
                    rect_contains(shape.adjust_rect(rect), position)
                } else {
                    radius.to_rrect(shape.adjust_rect(rect)).contains(position)
                }
            }
            ShapeBorder::Compound(borders) => borders[0].hit_test(rect, position, direction),
        }
    }

    /// Upstream `ShapeBorder.scale`.
    pub fn scale(&self, t: f32) -> ShapeBorder {
        match self {
            ShapeBorder::Border(border) => ShapeBorder::Border(border.scale(t)),
            ShapeBorder::Directional(border) => ShapeBorder::Directional(border.scale(t)),
            ShapeBorder::Rounded(shape) => ShapeBorder::Rounded(RoundedRectangleBorder::new(
                shape.side.scale(t),
                shape.border_radius.scale(t),
            )),
            ShapeBorder::Superellipse(shape) => ShapeBorder::Superellipse(
                RoundedSuperellipseBorder::new(shape.side.scale(t), shape.border_radius.scale(t)),
            ),
            ShapeBorder::Circle(shape) => {
                ShapeBorder::Circle(CircleBorder::new(shape.side.scale(t), shape.eccentricity))
            }
            ShapeBorder::Oval(shape) => ShapeBorder::Oval(OvalBorder::new(shape.side.scale(t))),
            ShapeBorder::Stadium(shape) => {
                ShapeBorder::Stadium(StadiumBorder::new(shape.side.scale(t)))
            }
            ShapeBorder::Linear(shape) => ShapeBorder::Linear(shape.scale(t)),
            ShapeBorder::Underline(shape) => ShapeBorder::Underline(shape.scale(t)),
            ShapeBorder::Outline(shape) => ShapeBorder::Outline(shape.scale(t)),
            ShapeBorder::Star(shape) => ShapeBorder::Star(shape.scale(t)),
            ShapeBorder::Beveled(shape) => ShapeBorder::Beveled(BeveledRectangleBorder::new(
                shape.side.scale(t),
                shape.border_radius.scale(t),
            )),
            ShapeBorder::Continuous(shape) => ShapeBorder::Continuous(
                ContinuousRectangleBorder::new(shape.side.scale(t), shape.border_radius.scale(t)),
            ),
            ShapeBorder::StadiumToCircle(shape) => ShapeBorder::StadiumToCircle(
                StadiumToCircleBorder::new(shape.side.scale(t), t, shape.eccentricity),
            ),
            ShapeBorder::StadiumToRoundedRect(shape) => {
                ShapeBorder::StadiumToRoundedRect(StadiumToRoundedRectBorder::new(
                    shape.side.scale(t),
                    shape.border_radius.scale(t),
                    t,
                ))
            }
            ShapeBorder::RoundedToCircle(shape) => {
                ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    shape.side.scale(t),
                    shape.border_radius.scale(t),
                    t,
                    shape.eccentricity,
                    shape.smooth_corners,
                ))
            }
            ShapeBorder::Compound(borders) => {
                ShapeBorder::Compound(borders.iter().map(|b| b.scale(t)).collect())
            }
        }
    }

    /// Upstream `ShapeBorder.add`: what `a + b` uses before falling back to
    /// a compound. Only the box borders and compounds know how to add.
    /// `reversed` says `self` sat on the right of the `+`; only compounds
    /// care (the new border joins their inner edge instead of the outer).
    pub fn add(&self, other: &ShapeBorder, reversed: bool) -> Option<ShapeBorder> {
        match (self, other) {
            (ShapeBorder::Border(a), ShapeBorder::Border(b)) => {
                if BorderSide::can_merge(a.top, b.top)
                    && BorderSide::can_merge(a.right, b.right)
                    && BorderSide::can_merge(a.bottom, b.bottom)
                    && BorderSide::can_merge(a.left, b.left)
                {
                    Some(ShapeBorder::Border(Border::merge(*a, *b)))
                } else {
                    None
                }
            }
            (ShapeBorder::Directional(a), ShapeBorder::Directional(b)) => {
                if BorderSide::can_merge(a.top, b.top)
                    && BorderSide::can_merge(a.start, b.start)
                    && BorderSide::can_merge(a.end, b.end)
                    && BorderSide::can_merge(a.bottom, b.bottom)
                {
                    Some(ShapeBorder::Directional(BorderDirectional::merge(*a, *b)))
                } else {
                    None
                }
            }
            // Upstream `BorderDirectional.add`'s cross-type rules: lateral
            // sides hand over only when one side's are nil.
            (ShapeBorder::Directional(a), ShapeBorder::Border(b)) => {
                if !BorderSide::can_merge(b.top, a.top)
                    || !BorderSide::can_merge(b.bottom, a.bottom)
                {
                    return None;
                }
                if a.start != BorderSide::NONE || a.end != BorderSide::NONE {
                    if b.left != BorderSide::NONE || b.right != BorderSide::NONE {
                        return None;
                    }
                    return Some(ShapeBorder::Directional(BorderDirectional::new(
                        BorderSide::merge(b.top, a.top),
                        a.start,
                        a.end,
                        BorderSide::merge(b.bottom, a.bottom),
                    )));
                }
                Some(ShapeBorder::Border(Border::new(
                    BorderSide::merge(b.top, a.top),
                    b.right,
                    BorderSide::merge(b.bottom, a.bottom),
                    b.left,
                )))
            }
            // Upstream `_CompoundBorder.add`.
            (ShapeBorder::Compound(borders), other) => {
                if let ShapeBorder::Compound(other_borders) = other {
                    let mut list = Vec::with_capacity(borders.len() + other_borders.len());
                    if reversed {
                        list.extend(borders.iter().cloned());
                        list.extend(other_borders.iter().cloned());
                    } else {
                        list.extend(other_borders.iter().cloned());
                        list.extend(borders.iter().cloned());
                    }
                    return Some(ShapeBorder::Compound(list));
                }
                // Try to merge the new border with the one it touches.
                let ours = if reversed {
                    borders.last().unwrap()
                } else {
                    &borders[0]
                };
                if let Some(joined) = ours.add(other, false).or_else(|| other.add(ours, true)) {
                    let mut list = borders.clone();
                    let index = if reversed { list.len() - 1 } else { 0 };
                    list[index] = joined;
                    return Some(ShapeBorder::Compound(list));
                }
                let mut list = Vec::with_capacity(borders.len() + 1);
                if reversed {
                    list.extend(borders.iter().cloned());
                    list.push(other.clone());
                } else {
                    list.push(other.clone());
                    list.extend(borders.iter().cloned());
                }
                Some(ShapeBorder::Compound(list))
            }
            _ => None,
        }
    }

    /// Upstream `operator +`: add if the shapes know how, otherwise a
    /// compound painting `other` outside `self`.
    pub fn combine(self, other: ShapeBorder) -> ShapeBorder {
        if let Some(joined) = self.add(&other, false) {
            return joined;
        }
        if let Some(joined) = other.add(&self, true) {
            return joined;
        }
        ShapeBorder::Compound(vec![other, self])
    }

    /// Upstream `ShapeBorder.lerp` -- the four-way attempt, then the
    /// before/after-half switch.
    pub fn lerp(a: Option<ShapeBorder>, b: Option<ShapeBorder>, t: f32) -> Option<ShapeBorder> {
        if a == b {
            return a;
        }
        let result = b
            .as_ref()
            .and_then(|b| b.lerp_from(a.as_ref(), t))
            .or_else(|| a.as_ref().and_then(|a| a.lerp_to(b.as_ref(), t)))
            .or_else(|| b.as_ref().and_then(|b| b.lerp_to(a.as_ref(), 1.0 - t)))
            .or_else(|| a.as_ref().and_then(|a| a.lerp_from(b.as_ref(), 1.0 - t)));
        result.or(if t < 0.5 { a } else { b })
    }

    /// Upstream `lerpFrom`: interpolate from `a` (missing = from nothing)
    /// into `self`. `None` means this shape cannot take that source.
    pub fn lerp_from(&self, a: Option<&ShapeBorder>, t: f32) -> Option<ShapeBorder> {
        let a = match a {
            None => return Some(self.scale(t)),
            Some(a) => a,
        };
        match (a, self) {
            (ShapeBorder::Border(a), ShapeBorder::Border(b)) => {
                Some(ShapeBorder::Border(Border::lerp(Some(*a), Some(*b), t)))
            }
            (ShapeBorder::Directional(a), ShapeBorder::Directional(b)) => Some(
                ShapeBorder::Directional(BorderDirectional::lerp(Some(*a), Some(*b), t)),
            ),
            (ShapeBorder::Rounded(a), ShapeBorder::Rounded(b)) => {
                Some(ShapeBorder::Rounded(RoundedRectangleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                )))
            }
            (ShapeBorder::Superellipse(a), ShapeBorder::Superellipse(b)) => {
                Some(ShapeBorder::Superellipse(RoundedSuperellipseBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                )))
            }
            (ShapeBorder::Circle(a), ShapeBorder::Circle(b)) => {
                Some(ShapeBorder::Circle(CircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    lerp_double(a.eccentricity, b.eccentricity, t).clamp(0.0, 1.0),
                )))
            }
            (ShapeBorder::Oval(a), ShapeBorder::Oval(b)) => Some(ShapeBorder::Oval(
                OvalBorder::new(BorderSide::lerp(a.side, b.side, t)),
            )),
            // A circle and an oval, either way round.
            //
            // Upstream has no arm for this because it does not need one:
            // `OvalBorder extends CircleBorder`, so a circle asked to lerp
            // with an oval finds `b is CircleBorder` and interpolates the two
            // eccentricities -- 0 to 1, which *is* the circle opening into an
            // ellipse. This crate makes them two variants, and without this
            // arm the pair fell through to the crossfade at the end of
            // [`ShapeBorder::lerp`]: an animation between them snapped
            // instead of morphing.
            //
            // The helpers already said as much -- "the oval is the circle
            // with the eccentricity pinned" -- and the arm that would have
            // used them was missing. A screen that swapped the two ends of
            // every lerp in this file found it: the swap changed nothing
            // here, because nothing was being interpolated.
            (a, b) if is_circle_like(a) && is_circle_like(b) => {
                Some(ShapeBorder::Circle(CircleBorder::new(
                    BorderSide::lerp(circle_side(a), circle_side(b), t),
                    lerp_double(circle_eccentricity(a), circle_eccentricity(b), t)
                        .clamp(0.0, 1.0),
                )))
            }
            (ShapeBorder::Stadium(a), ShapeBorder::Stadium(b)) => Some(ShapeBorder::Stadium(
                StadiumBorder::new(BorderSide::lerp(a.side, b.side, t)),
            )),
            (ShapeBorder::Linear(a), ShapeBorder::Linear(b)) => {
                Some(ShapeBorder::Linear(LinearBorder::lerp(a, b, t)))
            }
            (ShapeBorder::Underline(a), ShapeBorder::Underline(b)) => {
                Some(ShapeBorder::Underline(UnderlineInputBorder::lerp(a, b, t)))
            }
            (ShapeBorder::Outline(a), ShapeBorder::Outline(b)) => {
                Some(ShapeBorder::Outline(OutlineInputBorder::lerp(a, b, t)))
            }
            (ShapeBorder::Star(a), ShapeBorder::Star(b)) => {
                Some(ShapeBorder::Star(StarBorder::lerp_star(a, b, t)))
            }
            (ShapeBorder::Beveled(a), ShapeBorder::Beveled(b)) => {
                Some(ShapeBorder::Beveled(BeveledRectangleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                )))
            }
            (ShapeBorder::Continuous(a), ShapeBorder::Continuous(b)) => {
                Some(ShapeBorder::Continuous(ContinuousRectangleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                )))
            }
            // A circle (or oval) growing into a rounded shape.
            (a, ShapeBorder::Rounded(b)) if is_circle_like(a) => {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(circle_side(a), b.side, t),
                    b.border_radius,
                    1.0 - t,
                    circle_eccentricity(a),
                    false,
                )))
            }
            (a, ShapeBorder::Superellipse(b)) if is_circle_like(a) => {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(circle_side(a), b.side, t),
                    b.border_radius,
                    1.0 - t,
                    circle_eccentricity(a),
                    true,
                )))
            }
            (a, ShapeBorder::Stadium(b)) if is_circle_like(a) => {
                Some(ShapeBorder::StadiumToCircle(StadiumToCircleBorder::new(
                    BorderSide::lerp(circle_side(a), b.side, t),
                    1.0 - t,
                    circle_eccentricity(a),
                )))
            }
            // A circle becoming a star: the points count in from its nearest
            // whole while the rounding grows.
            (a, ShapeBorder::Star(b)) if is_circle_like(a) => {
                Some(ShapeBorder::Star(StarBorder::lerp_from_circle(
                    b,
                    BorderSide::lerp(circle_side(a), b.side, t),
                    circle_eccentricity(a),
                    t,
                )))
            }
            // A stadium becoming a star goes through a circle, in two phases.
            (ShapeBorder::Stadium(a), ShapeBorder::Star(b)) => {
                let side = BorderSide::lerp(a.side, b.side, t);
                let circle = ShapeBorder::Circle(CircleBorder::new(side, 0.0));
                Some(StarBorder::two_phase_lerp(
                    t,
                    0.5,
                    |t| {
                        ShapeBorder::lerp(Some(ShapeBorder::Stadium(*a)), Some(circle.clone()), t)
                            .expect("stadium to circle interpolates")
                    },
                    |t| {
                        ShapeBorder::lerp(Some(circle.clone()), Some(ShapeBorder::Star(*b)), t)
                            .expect("circle to star interpolates")
                    },
                ))
            }
            // A rounded rectangle becoming a star: to a stadium, to a circle,
            // to a star, in three phases.
            (ShapeBorder::Rounded(a), ShapeBorder::Star(b)) => {
                let side = BorderSide::lerp(a.side, b.side, t);
                let circle = ShapeBorder::Circle(CircleBorder::new(side, 0.0));
                Some(StarBorder::two_phase_lerp(
                    t,
                    1.0 / 3.0,
                    |t| {
                        ShapeBorder::lerp(
                            Some(ShapeBorder::Rounded(*a)),
                            Some(ShapeBorder::Stadium(StadiumBorder::new(side))),
                            t,
                        )
                        .expect("rounded to stadium interpolates")
                    },
                    |t| {
                        StarBorder::two_phase_lerp(
                            t,
                            0.5,
                            |t| {
                                ShapeBorder::lerp(
                                    Some(ShapeBorder::Stadium(StadiumBorder::new(side))),
                                    Some(circle.clone()),
                                    t,
                                )
                                .expect("stadium to circle interpolates")
                            },
                            |t| {
                                ShapeBorder::lerp(
                                    Some(circle.clone()),
                                    Some(ShapeBorder::Star(*b)),
                                    t,
                                )
                                .expect("circle to star interpolates")
                            },
                        )
                    },
                ))
            }
            (ShapeBorder::Rounded(a), ShapeBorder::Stadium(b)) => Some(
                ShapeBorder::StadiumToRoundedRect(StadiumToRoundedRectBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    a.border_radius,
                    1.0 - t,
                )),
            ),
            // The transition shapes' own arithmetic.
            (ShapeBorder::Stadium(a), ShapeBorder::StadiumToCircle(b)) => {
                Some(ShapeBorder::StadiumToCircle(StadiumToCircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    b.circularity * t,
                    b.eccentricity,
                )))
            }
            (a, ShapeBorder::StadiumToCircle(b)) if is_circle_like(a) => {
                Some(ShapeBorder::StadiumToCircle(StadiumToCircleBorder::new(
                    BorderSide::lerp(circle_side(a), b.side, t),
                    b.circularity + (1.0 - b.circularity) * (1.0 - t),
                    circle_eccentricity(a),
                )))
            }
            (ShapeBorder::StadiumToCircle(a), ShapeBorder::StadiumToCircle(b)) => {
                Some(ShapeBorder::StadiumToCircle(StadiumToCircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    lerp_double(a.circularity, b.circularity, t),
                    lerp_double(a.eccentricity, b.eccentricity, t),
                )))
            }
            (ShapeBorder::Stadium(a), ShapeBorder::StadiumToRoundedRect(b)) => Some(
                ShapeBorder::StadiumToRoundedRect(StadiumToRoundedRectBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    b.border_radius,
                    b.rectilinearity * t,
                )),
            ),
            (ShapeBorder::Rounded(a), ShapeBorder::StadiumToRoundedRect(b)) => Some(
                ShapeBorder::StadiumToRoundedRect(StadiumToRoundedRectBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    b.border_radius,
                    b.rectilinearity + (1.0 - b.rectilinearity) * (1.0 - t),
                )),
            ),
            (ShapeBorder::StadiumToRoundedRect(a), ShapeBorder::StadiumToRoundedRect(b)) => Some(
                ShapeBorder::StadiumToRoundedRect(StadiumToRoundedRectBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                    lerp_double(a.rectilinearity, b.rectilinearity, t),
                )),
            ),
            (ShapeBorder::Rounded(a), ShapeBorder::RoundedToCircle(b)) if !b.smooth_corners => {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                    b.circularity * t,
                    b.eccentricity,
                    false,
                )))
            }
            (ShapeBorder::Superellipse(a), ShapeBorder::RoundedToCircle(b)) if b.smooth_corners => {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                    b.circularity * t,
                    b.eccentricity,
                    true,
                )))
            }
            (a, ShapeBorder::RoundedToCircle(b)) if is_circle_like(a) => {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(circle_side(a), b.side, t),
                    b.border_radius,
                    b.circularity + (1.0 - b.circularity) * (1.0 - t),
                    circle_eccentricity(a),
                    b.smooth_corners,
                )))
            }
            (ShapeBorder::RoundedToCircle(a), ShapeBorder::RoundedToCircle(b))
                if a.smooth_corners == b.smooth_corners =>
            {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                    lerp_double(a.circularity, b.circularity, t),
                    a.eccentricity,
                    a.smooth_corners,
                )))
            }
            (ShapeBorder::Rounded(_), ShapeBorder::RoundedToCircle(_))
            | (ShapeBorder::Superellipse(_), ShapeBorder::RoundedToCircle(_)) => None,
            (ShapeBorder::Compound(_), ShapeBorder::Compound(_)) => Some(compound_lerp(a, self, t)),
            (a, ShapeBorder::Compound(_)) => Some(compound_lerp(a, self, t)),
            (ShapeBorder::Compound(_), b) => Some(compound_lerp(a, b, t)),
            _ => None,
        }
    }

    /// Upstream `lerpTo`: from `self` into `b` (missing = to nothing).
    /// This is the source-side dispatch -- a genuinely different set of
    /// rules from `lerp_from`, not a timeline flip of it.
    pub fn lerp_to(&self, b: Option<&ShapeBorder>, t: f32) -> Option<ShapeBorder> {
        let b = match b {
            None => return Some(self.scale(1.0 - t)),
            Some(b) => b,
        };
        match (self, b) {
            (ShapeBorder::Border(a), ShapeBorder::Border(b)) => {
                Some(ShapeBorder::Border(Border::lerp(Some(*a), Some(*b), t)))
            }
            (ShapeBorder::Directional(a), ShapeBorder::Directional(b)) => Some(
                ShapeBorder::Directional(BorderDirectional::lerp(Some(*a), Some(*b), t)),
            ),
            (ShapeBorder::Rounded(a), ShapeBorder::Rounded(b)) => {
                Some(ShapeBorder::Rounded(RoundedRectangleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                )))
            }
            (ShapeBorder::Superellipse(a), ShapeBorder::Superellipse(b)) => {
                Some(ShapeBorder::Superellipse(RoundedSuperellipseBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                )))
            }
            (ShapeBorder::Circle(a), ShapeBorder::Circle(b)) => {
                Some(ShapeBorder::Circle(CircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    lerp_double(a.eccentricity, b.eccentricity, t).clamp(0.0, 1.0),
                )))
            }
            (ShapeBorder::Oval(a), ShapeBorder::Oval(b)) => Some(ShapeBorder::Oval(
                OvalBorder::new(BorderSide::lerp(a.side, b.side, t)),
            )),
            // The mirror of the arm in `lerp_from`, which carries the
            // reasoning. Both are needed for the same reason upstream needs
            // both `lerpFrom` and `lerpTo`: `ShapeBorder::lerp` asks one and
            // then the other, and a pair handled in only one of them answers
            // for one direction and crossfades the other.
            (a, b) if is_circle_like(a) && is_circle_like(b) => {
                Some(ShapeBorder::Circle(CircleBorder::new(
                    BorderSide::lerp(circle_side(a), circle_side(b), t),
                    lerp_double(circle_eccentricity(a), circle_eccentricity(b), t)
                        .clamp(0.0, 1.0),
                )))
            }
            (ShapeBorder::Stadium(a), ShapeBorder::Stadium(b)) => Some(ShapeBorder::Stadium(
                StadiumBorder::new(BorderSide::lerp(a.side, b.side, t)),
            )),
            (ShapeBorder::Beveled(a), ShapeBorder::Beveled(b)) => {
                Some(ShapeBorder::Beveled(BeveledRectangleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                )))
            }
            (ShapeBorder::Continuous(a), ShapeBorder::Continuous(b)) => {
                Some(ShapeBorder::Continuous(ContinuousRectangleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                )))
            }
            (ShapeBorder::Linear(a), ShapeBorder::Linear(b)) => {
                Some(ShapeBorder::Linear(LinearBorder::lerp(a, b, t)))
            }
            (ShapeBorder::Underline(a), ShapeBorder::Underline(b)) => {
                Some(ShapeBorder::Underline(UnderlineInputBorder::lerp(a, b, t)))
            }
            (ShapeBorder::Outline(a), ShapeBorder::Outline(b)) => {
                Some(ShapeBorder::Outline(OutlineInputBorder::lerp(a, b, t)))
            }
            (ShapeBorder::Star(a), ShapeBorder::Star(b)) => {
                Some(ShapeBorder::Star(StarBorder::lerp_star(a, b, t)))
            }
            // A star collapsing into a circle.
            (ShapeBorder::Star(a), b) if is_circle_like(b) => {
                Some(ShapeBorder::Star(StarBorder::lerp_to_circle(
                    a,
                    BorderSide::lerp(a.side, circle_side(b), t),
                    circle_eccentricity(b),
                    t,
                )))
            }
            // A star becoming a stadium goes through a circle, in two phases.
            (ShapeBorder::Star(a), ShapeBorder::Stadium(b)) => {
                let side = BorderSide::lerp(a.side, b.side, t);
                let circle = ShapeBorder::Circle(CircleBorder::new(side, 0.0));
                Some(StarBorder::two_phase_lerp(
                    t,
                    0.5,
                    |t| {
                        ShapeBorder::lerp(Some(ShapeBorder::Star(*a)), Some(circle.clone()), t)
                            .expect("star to circle interpolates")
                    },
                    |t| {
                        ShapeBorder::lerp(Some(circle.clone()), Some(ShapeBorder::Stadium(*b)), t)
                            .expect("circle to stadium interpolates")
                    },
                ))
            }
            // A star becoming a rounded rectangle: to a circle, to a stadium,
            // to the rectangle, in three phases.
            (ShapeBorder::Star(a), ShapeBorder::Rounded(b)) => {
                let side = BorderSide::lerp(a.side, b.side, t);
                let circle = ShapeBorder::Circle(CircleBorder::new(side, 0.0));
                Some(StarBorder::two_phase_lerp(
                    t,
                    2.0 / 3.0,
                    |t| {
                        StarBorder::two_phase_lerp(
                            t,
                            0.5,
                            |t| {
                                ShapeBorder::lerp(
                                    Some(ShapeBorder::Star(*a)),
                                    Some(circle.clone()),
                                    t,
                                )
                                .expect("star to circle interpolates")
                            },
                            |t| {
                                ShapeBorder::lerp(
                                    Some(circle.clone()),
                                    Some(ShapeBorder::Stadium(StadiumBorder::new(side))),
                                    t,
                                )
                                .expect("circle to stadium interpolates")
                            },
                        )
                    },
                    |t| {
                        ShapeBorder::lerp(
                            Some(ShapeBorder::Stadium(StadiumBorder::new(side))),
                            Some(ShapeBorder::Rounded(*b)),
                            t,
                        )
                        .expect("stadium to rounded interpolates")
                    },
                ))
            }
            // A rounded shape collapsing into a circle.
            (ShapeBorder::Rounded(a), b) if is_circle_like(b) => {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(a.side, circle_side(b), t),
                    a.border_radius,
                    t,
                    circle_eccentricity(b),
                    false,
                )))
            }
            (ShapeBorder::Superellipse(a), b) if is_circle_like(b) => {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(a.side, circle_side(b), t),
                    a.border_radius,
                    t,
                    circle_eccentricity(b),
                    true,
                )))
            }
            (ShapeBorder::Stadium(a), b) if is_circle_like(b) => {
                Some(ShapeBorder::StadiumToCircle(StadiumToCircleBorder::new(
                    BorderSide::lerp(a.side, circle_side(b), t),
                    t,
                    circle_eccentricity(b),
                )))
            }
            (ShapeBorder::Stadium(a), ShapeBorder::Rounded(b)) => Some(
                ShapeBorder::StadiumToRoundedRect(StadiumToRoundedRectBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    b.border_radius,
                    t,
                )),
            ),
            // The transition shapes' own arithmetic.
            (ShapeBorder::StadiumToCircle(a), ShapeBorder::Stadium(b)) => {
                Some(ShapeBorder::StadiumToCircle(StadiumToCircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    a.circularity * (1.0 - t),
                    a.eccentricity,
                )))
            }
            (ShapeBorder::StadiumToCircle(a), b) if is_circle_like(b) => {
                Some(ShapeBorder::StadiumToCircle(StadiumToCircleBorder::new(
                    BorderSide::lerp(a.side, circle_side(b), t),
                    a.circularity + (1.0 - a.circularity) * t,
                    circle_eccentricity(b),
                )))
            }
            (ShapeBorder::StadiumToCircle(a), ShapeBorder::StadiumToCircle(b)) => {
                Some(ShapeBorder::StadiumToCircle(StadiumToCircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    lerp_double(a.circularity, b.circularity, t),
                    lerp_double(a.eccentricity, b.eccentricity, t),
                )))
            }
            (ShapeBorder::StadiumToRoundedRect(a), ShapeBorder::Stadium(b)) => Some(
                ShapeBorder::StadiumToRoundedRect(StadiumToRoundedRectBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    a.border_radius,
                    a.rectilinearity * (1.0 - t),
                )),
            ),
            (ShapeBorder::StadiumToRoundedRect(a), ShapeBorder::Rounded(b)) => Some(
                ShapeBorder::StadiumToRoundedRect(StadiumToRoundedRectBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    a.border_radius,
                    a.rectilinearity + (1.0 - a.rectilinearity) * t,
                )),
            ),
            (ShapeBorder::StadiumToRoundedRect(a), ShapeBorder::StadiumToRoundedRect(b)) => Some(
                ShapeBorder::StadiumToRoundedRect(StadiumToRoundedRectBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                    lerp_double(a.rectilinearity, b.rectilinearity, t),
                )),
            ),
            (ShapeBorder::RoundedToCircle(a), ShapeBorder::Rounded(b)) if !a.smooth_corners => {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                    a.circularity * (1.0 - t),
                    a.eccentricity,
                    false,
                )))
            }
            (ShapeBorder::RoundedToCircle(a), ShapeBorder::Superellipse(b)) if a.smooth_corners => {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                    a.circularity * (1.0 - t),
                    a.eccentricity,
                    true,
                )))
            }
            (ShapeBorder::RoundedToCircle(a), b) if is_circle_like(b) => {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(a.side, circle_side(b), t),
                    a.border_radius,
                    a.circularity + (1.0 - a.circularity) * t,
                    circle_eccentricity(b),
                    a.smooth_corners,
                )))
            }
            (ShapeBorder::RoundedToCircle(a), ShapeBorder::RoundedToCircle(b))
                if a.smooth_corners == b.smooth_corners =>
            {
                Some(ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    BorderSide::lerp(a.side, b.side, t),
                    BorderRadiusGeometry::lerp(a.border_radius, b.border_radius, t),
                    lerp_double(a.circularity, b.circularity, t),
                    a.eccentricity,
                    a.smooth_corners,
                )))
            }
            (ShapeBorder::Compound(_), ShapeBorder::Compound(_)) => Some(compound_lerp(self, b, t)),
            (ShapeBorder::Compound(_), b) => Some(compound_lerp(self, b, t)),
            (a, ShapeBorder::Compound(_)) => Some(compound_lerp(a, b, t)),
            _ => None,
        }
    }

    /// Upstream `ShapeBorder.paint` (the plain, rectangular-argument form;
    /// the box-border extension lives on `Border::paint`).
    pub fn paint(&self, canvas: &mut Canvas, rect: Rect, direction: TextDirection) {
        match self {
            ShapeBorder::Border(border) => border.paint(canvas, rect, None, BoxShape::Rectangle),
            ShapeBorder::Directional(border) => {
                border.paint(canvas, rect, direction, None, BoxShape::Rectangle)
            }
            ShapeBorder::Rounded(shape) => {
                if shape.side.style == BorderStyle::None {
                    return;
                }
                if shape.side.width == 0.0 {
                    let rrect = shape.outer_rrect(rect, direction);
                    canvas.draw_path(&rrect.to_path(), &shape.side.to_paint());
                } else {
                    let border_rect = shape.outer_rrect(rect, direction);
                    let inner = border_rect.deflate(shape.side.stroke_inset());
                    let outer = border_rect.inflate(shape.side.stroke_outset());
                    let mut path = RenderPath::new().with_fill_type(FillType::EvenOdd);
                    outer.append_to(&mut path);
                    inner.append_to(&mut path);
                    canvas.draw_path(&path, &Paint::new(shape.side.color));
                }
            }
            ShapeBorder::Superellipse(shape) => {
                if shape.side.style == BorderStyle::None {
                    return;
                }
                let stroke_offset = (shape.side.stroke_outset() - shape.side.stroke_inset()) / 2.0;
                let path = if shape.zero_radius() {
                    let mut path = RenderPath::new();
                    path.add_rect(rect_inflate(rect, stroke_offset));
                    path
                } else {
                    continuous_path(
                        shape
                            .border_radius
                            .resolve(direction)
                            .to_rrect(rect)
                            .inflate(stroke_offset),
                    )
                };
                canvas.draw_path(&path, &shape.side.to_paint());
            }
            ShapeBorder::Circle(shape) => {
                if shape.side.style == BorderStyle::None {
                    return;
                }
                if shape.eccentricity == 0.0 {
                    let center = rect_center(rect);
                    let radius = (rect_shortest_side(rect) + shape.side.stroke_offset()) / 2.0;
                    canvas.draw_circle(center.dx, center.dy, radius, &shape.side.to_paint());
                } else {
                    let border_rect = shape.adjust_rect(rect);
                    canvas.draw_oval(
                        rect_inflate(border_rect, shape.side.stroke_offset() / 2.0),
                        &shape.side.to_paint(),
                    );
                }
            }
            ShapeBorder::Oval(shape) => {
                if shape.side.style == BorderStyle::None {
                    return;
                }
                canvas.draw_oval(
                    rect_inflate(rect, shape.side.stroke_offset() / 2.0),
                    &shape.side.to_paint(),
                );
            }
            ShapeBorder::Stadium(shape) => {
                if shape.side.style == BorderStyle::None {
                    return;
                }
                let border_rect = stadium_rrect(rect).inflate(shape.side.stroke_offset() / 2.0);
                canvas.draw_path(&border_rect.to_path(), &shape.side.to_paint());
            }
            ShapeBorder::Linear(shape) => shape.paint(canvas, rect, direction),
            ShapeBorder::Underline(shape) => shape.paint(canvas, rect),
            // A shape border has no gap to be told about; a text field that
            // wants one calls `paint_with_gap` itself.
            ShapeBorder::Outline(shape) => shape.paint_with_gap(canvas, rect, None, 0.0, 0.0),
            ShapeBorder::Star(shape) => {
                if shape.side.style == BorderStyle::None {
                    return;
                }
                let adjusted_rect = rect_inflate(rect, shape.side.stroke_offset() / 2.0);
                let path = shape.generator().generate(adjusted_rect);
                canvas.draw_path(&path, &shape.side.to_paint());
            }
            ShapeBorder::Beveled(shape) => {
                if rect_is_empty(rect) || shape.side.style == BorderStyle::None {
                    return;
                }
                let border_rect = shape.border_radius.resolve(direction).to_rrect(rect);
                let outer = beveled_path(border_rect.inflate(shape.side.stroke_outset()));
                let inner = self.inner_path(rect, direction);
                // Upstream strokes both subpaths with one drawPath; two
                // strokes of the same paint land on the same pixels.
                canvas.draw_path(&outer, &shape.side.to_paint());
                canvas.draw_path(&inner, &shape.side.to_paint());
            }
            ShapeBorder::Continuous(shape) => {
                if rect_is_empty(rect) || shape.side.style == BorderStyle::None {
                    return;
                }
                let path = self.outer_path(rect, direction);
                canvas.draw_path(&path, &shape.side.to_paint());
            }
            ShapeBorder::StadiumToCircle(shape) => {
                if shape.side.style == BorderStyle::None {
                    return;
                }
                let border_rect = shape.rrect(rect).inflate(shape.side.stroke_offset() / 2.0);
                canvas.draw_path(&border_rect.to_path(), &shape.side.to_paint());
            }
            ShapeBorder::StadiumToRoundedRect(shape) => {
                if shape.side.style == BorderStyle::None {
                    return;
                }
                let border_rect = shape
                    .rrect(rect, direction)
                    .inflate(shape.side.stroke_offset() / 2.0);
                canvas.draw_path(&border_rect.to_path(), &shape.side.to_paint());
            }
            ShapeBorder::RoundedToCircle(shape) => {
                if shape.side.style == BorderStyle::None {
                    return;
                }
                let rrect = shape
                    .rrect(rect, direction)
                    .inflate(shape.side.stroke_offset() / 2.0);
                let path = if shape.smooth_corners {
                    continuous_path(rrect)
                } else {
                    rrect.to_path()
                };
                canvas.draw_path(&path, &shape.side.to_paint());
            }
            ShapeBorder::Compound(borders) => {
                let mut rect = rect;
                for border in borders {
                    border.paint(canvas, rect, direction);
                    rect = border.dimensions().deflate_rect(rect, direction);
                }
            }
        }
    }
}

/// Upstream `_CompoundBorder.lerp`: pairwise where both slots have a shape,
/// otherwise the incoming shape scales up in front of the outgoing one.
fn compound_lerp(a: &ShapeBorder, b: &ShapeBorder, t: f32) -> ShapeBorder {
    let a_list: Vec<&ShapeBorder> = match a {
        ShapeBorder::Compound(borders) => borders.iter().collect(),
        single => vec![single],
    };
    let b_list: Vec<&ShapeBorder> = match b {
        ShapeBorder::Compound(borders) => borders.iter().collect(),
        single => vec![single],
    };
    let mut results = Vec::new();
    for index in 0..a_list.len().max(b_list.len()) {
        let local_a = a_list.get(index).copied();
        let local_b = b_list.get(index).copied();
        if let (Some(local_a), Some(local_b)) = (local_a, local_b) {
            let local_result = local_a
                .lerp_to(Some(local_b), t)
                .or_else(|| local_b.lerp_from(Some(local_a), t));
            if let Some(local_result) = local_result {
                results.push(local_result);
                continue;
            }
        }
        // The shape coming in lands before the one going away, so the outer
        // path switches to the new border early.
        if let Some(local_b) = local_b {
            results.push(local_b.scale(t));
        }
        if let Some(local_a) = local_a {
            results.push(local_a.scale(1.0 - t));
        }
    }
    ShapeBorder::Compound(results)
}

/// The star's sharp vertices for hit testing -- `Path.contains` on the
/// generated path upstream, approximated here by the unrounded polygon
/// under the same squash transform (the engine reads no points back out of
/// a path).
fn star_hit_vertices(star: &StarBorder, rect: Rect) -> Vec<(f32, f32)> {
    let radius = rect_shortest_side(rect) / 2.0;
    let center = rect_center(rect);
    const MIN_INNER_RADIUS_RATIO: f32 = 0.002;
    let inner = radius
        * (star.inner_radius_ratio() * (1.0 - MIN_INNER_RADIUS_RATIO) + MIN_INNER_RADIUS_RATIO);

    let step = std::f32::consts::PI / star.points;
    let total = (star.points * 2.0).round() as usize;
    let mut angle = -std::f32::consts::FRAC_PI_2 - step;
    let mut vertices = Vec::with_capacity(total);
    for index in 0..total {
        // Alternately a valley on the inner radius and a point on the outer.
        let r = if index % 2 == 0 { inner } else { radius };
        vertices.push((center.dx + angle.cos() * r, center.dy + angle.sin() * r));
        angle += step;
    }

    // The same squash transform the generator applies.
    let mut scale = (
        rect.width() / (2.0 * radius),
        rect.height() / (2.0 * radius),
    );
    if rect_shortest_side(rect) == rect.width() {
        scale.1 = star.squash * scale.1 + (1.0 - star.squash) * scale.0;
    } else {
        scale.0 = star.squash * scale.0 + (1.0 - star.squash) * scale.1;
    }
    let (sin_r, cos_r) = star.rotation_radians.sin_cos();
    vertices
        .into_iter()
        .map(|(x, y)| {
            let dx = x - center.dx;
            let dy = y - center.dy;
            let rotated = (dx * cos_r - dy * sin_r, dx * sin_r + dy * cos_r);
            (
                center.dx + rotated.0 * scale.0,
                center.dy + rotated.1 * scale.1,
            )
        })
        .collect()
}

fn oval_path(rect: Rect) -> RenderPath {
    let mut path = RenderPath::new();
    path.add_oval(rect);
    path
}

/// `CircleBorder` and `OvalBorder` share one lerp arithmetic; the oval is
/// the circle with the eccentricity pinned.
fn is_circle_like(shape: &ShapeBorder) -> bool {
    matches!(shape, ShapeBorder::Circle(_) | ShapeBorder::Oval(_))
}

fn circle_side(shape: &ShapeBorder) -> BorderSide {
    match shape {
        ShapeBorder::Circle(shape) => shape.side,
        ShapeBorder::Oval(shape) => shape.side,
        _ => BorderSide::NONE,
    }
}

fn circle_eccentricity(shape: &ShapeBorder) -> f32 {
    match shape {
        ShapeBorder::Circle(shape) => shape.eccentricity,
        ShapeBorder::Oval(_) => 1.0,
        _ => 0.0,
    }
}

pub(crate) fn ellipse_contains(rect: Rect, position: Offset) -> bool {
    let rx = rect.width() / 2.0;
    let ry = rect.height() / 2.0;
    if rx <= 0.0 || ry <= 0.0 {
        return false;
    }
    let center = rect_center(rect);
    let dx = (position.dx - center.dx) / rx;
    let dy = (position.dy - center.dy) / ry;
    dx * dx + dy * dy <= 1.0
}

// -- LinearBorder (upstream linear_border.dart) -----------------------------------

/// One edge of a [`LinearBorder`]: how much of its side it spans, and how
/// the remainder is split.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LinearBorderEdge {
    /// The length of the line as a fraction of its box edge, 0.0 to 1.0.
    pub size: f32,
    /// -1.0 towards the start, 0.0 centred, 1.0 towards the end.
    pub alignment: f32,
}

impl LinearBorderEdge {
    pub const fn new(size: f32, alignment: f32) -> LinearBorderEdge {
        debug_assert!(size >= 0.0 && size <= 1.0);
        LinearBorderEdge { size, alignment }
    }

    /// Upstream `LinearBorderEdge.lerp`: a missing side adopts the other's
    /// alignment and grows from size zero.
    pub fn lerp(
        a: Option<LinearBorderEdge>,
        b: Option<LinearBorderEdge>,
        t: f32,
    ) -> Option<LinearBorderEdge> {
        if a == b {
            return a;
        }
        let a = match (a, b) {
            (Some(a), _) => a,
            (None, Some(b)) => LinearBorderEdge {
                size: 0.0,
                alignment: b.alignment,
            },
            (None, None) => return None,
        };
        let b = b.unwrap_or(LinearBorderEdge {
            size: 0.0,
            alignment: a.alignment,
        });
        Some(LinearBorderEdge {
            size: lerp_double(a.size, b.size, t),
            alignment: lerp_double(a.alignment, b.alignment, t),
        })
    }
}

/// An `OutlinedBorder` that draws zero to four single lines, one per side --
/// the border Cupertino's buttons use.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LinearBorder {
    pub side: BorderSide,
    pub start: Option<LinearBorderEdge>,
    pub end: Option<LinearBorderEdge>,
    pub top: Option<LinearBorderEdge>,
    pub bottom: Option<LinearBorderEdge>,
}

impl LinearBorder {
    pub const fn new(
        side: BorderSide,
        start: Option<LinearBorderEdge>,
        end: Option<LinearBorderEdge>,
        top: Option<LinearBorderEdge>,
        bottom: Option<LinearBorderEdge>,
    ) -> LinearBorder {
        LinearBorder {
            side,
            start,
            end,
            top,
            bottom,
        }
    }

    pub fn start_edge(side: BorderSide, alignment: f32, size: f32) -> LinearBorder {
        LinearBorder::new(
            side,
            Some(LinearBorderEdge::new(size, alignment)),
            None,
            None,
            None,
        )
    }

    pub fn end_edge(side: BorderSide, alignment: f32, size: f32) -> LinearBorder {
        LinearBorder::new(
            side,
            None,
            Some(LinearBorderEdge::new(size, alignment)),
            None,
            None,
        )
    }

    pub fn top_edge(side: BorderSide, alignment: f32, size: f32) -> LinearBorder {
        LinearBorder::new(
            side,
            None,
            None,
            Some(LinearBorderEdge::new(size, alignment)),
            None,
        )
    }

    pub fn bottom_edge(side: BorderSide, alignment: f32, size: f32) -> LinearBorder {
        LinearBorder::new(
            side,
            None,
            None,
            None,
            Some(LinearBorderEdge::new(size, alignment)),
        )
    }

    /// Upstream `LinearBorder.scale`.
    pub fn scale(&self, t: f32) -> LinearBorder {
        LinearBorder {
            side: self.side.scale(t),
            ..*self
        }
    }

    /// Upstream `LinearBorder.dimensions`: each present edge insets by the
    /// side's full width, directionally.
    pub fn dimensions(&self) -> EdgeInsetsGeometry {
        let width = self.side.width;
        EdgeInsetsGeometry::Directional(EdgeInsetsDirectional {
            start: self.start.map_or(0.0, |_| width),
            top: self.top.map_or(0.0, |_| width),
            end: self.end.map_or(0.0, |_| width),
            bottom: self.bottom.map_or(0.0, |_| width),
        })
    }

    pub fn lerp(a: &LinearBorder, b: &LinearBorder, t: f32) -> LinearBorder {
        LinearBorder {
            side: BorderSide::lerp(a.side, b.side, t),
            start: LinearBorderEdge::lerp(a.start, b.start, t),
            end: LinearBorderEdge::lerp(a.end, b.end, t),
            top: LinearBorderEdge::lerp(a.top, b.top, t),
            bottom: LinearBorderEdge::lerp(a.bottom, b.bottom, t),
        }
    }

    /// Upstream `LinearBorder.paint`: one filled (or strobed, for a
    /// zero-width edge) rectangle per present edge, laid out along the side
    /// and positioned by the edge's alignment.
    pub fn paint(&self, canvas: &mut Canvas, rect: Rect, direction: TextDirection) {
        if self.side.style == BorderStyle::None {
            return;
        }
        let insets = self.dimensions().resolve(direction);
        let insets_vertical = insets.top + insets.bottom;
        let rtl = direction == TextDirection::Rtl;

        let draw_edge = |canvas: &mut Canvas, edge_rect: Rect| {
            let vertical_line = edge_rect.width() == 0.0;
            let horizontal_line = edge_rect.height() == 0.0;
            let paint = if vertical_line || horizontal_line {
                Paint::new(self.side.color).with_style(Style::Stroke { width: 0.0 })
            } else {
                Paint::new(self.side.color)
            };
            let mut path = RenderPath::new();
            path.move_to(edge_rect.left, edge_rect.top);
            if vertical_line {
                path.line_to(edge_rect.left, edge_rect.bottom);
            } else if horizontal_line {
                path.line_to(edge_rect.right, edge_rect.top);
            } else {
                path.line_to(edge_rect.right, edge_rect.top);
                path.line_to(edge_rect.right, edge_rect.bottom);
                path.line_to(edge_rect.left, edge_rect.bottom);
            }
            path.close();
            canvas.draw_path(&path, &paint);
        };

        let vertical_span = |edge: LinearBorderEdge| {
            let height = (rect.height() - insets_vertical) * edge.size;
            let y = (rect.height() - insets_vertical - height) * ((edge.alignment + 1.0) / 2.0);
            (y, height)
        };

        if let Some(edge) = self.start.filter(|edge| edge.size != 0.0) {
            let (y, height) = vertical_span(edge);
            let (x, width) = if rtl {
                (rect.right - insets.right, insets.right)
            } else {
                (rect.left, insets.left)
            };
            draw_edge(canvas, Rect::xywh(x, y + insets.top, width, height));
        }
        if let Some(edge) = self.end.filter(|edge| edge.size != 0.0) {
            let (y, height) = vertical_span(edge);
            let (x, width) = if rtl {
                (rect.left, insets.left)
            } else {
                (rect.right - insets.right, insets.right)
            };
            draw_edge(canvas, Rect::xywh(x, y + insets.top, width, height));
        }
        if let Some(edge) = self.top.filter(|edge| edge.size != 0.0) {
            let width = rect.width() * edge.size;
            let start_x = (rect.width() - width) * ((edge.alignment + 1.0) / 2.0);
            let x = if rtl {
                rect.width() - start_x - width
            } else {
                start_x
            };
            draw_edge(canvas, Rect::xywh(x, rect.top, width, insets.top));
        }
        if let Some(edge) = self.bottom.filter(|edge| edge.size != 0.0) {
            let width = rect.width() * edge.size;
            let start_x = (rect.width() - width) * ((edge.alignment + 1.0) / 2.0);
            let x = if rtl {
                rect.width() - start_x - width
            } else {
                start_x
            };
            draw_edge(
                canvas,
                Rect::xywh(x, rect.bottom - insets.bottom, width, self.side.width),
            );
        }
    }
}

// -- StarBorder (upstream star_border.dart) ----------------------------------------

/// A star or regular-polygon border. `inner_radius_ratio` of `None` is the
/// polygon spelling, where the inner radius comes from the incircle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StarBorder {
    pub side: BorderSide,
    /// The number of points (or sides), as a fraction so it can animate.
    pub points: f32,
    inner_radius_ratio: Option<f32>,
    /// Rounding of each point/corner, 0.0 sharp to 1.0 a circular arc.
    pub point_rounding: f32,
    /// Rounding of each valley, 0.0 to 1.0; zero for polygons.
    pub valley_rounding: f32,
    rotation_radians: f32,
    /// How much of the widget's aspect ratio to take on, 0.0 to 1.0.
    pub squash: f32,
}

impl Default for StarBorder {
    fn default() -> Self {
        StarBorder::new(BorderSide::NONE, 5.0, 0.4, 0.0, 0.0, 0.0, 0.0)
    }
}

impl StarBorder {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        side: BorderSide,
        points: f32,
        inner_radius_ratio: f32,
        point_rounding: f32,
        valley_rounding: f32,
        rotation_degrees: f32,
        squash: f32,
    ) -> StarBorder {
        debug_assert!(points >= 2.0);
        debug_assert!((0.0..=1.0).contains(&inner_radius_ratio));
        debug_assert!(point_rounding + valley_rounding <= 1.0);
        StarBorder {
            side,
            points,
            inner_radius_ratio: Some(inner_radius_ratio),
            point_rounding,
            valley_rounding,
            rotation_radians: rotation_degrees.to_radians(),
            squash,
        }
    }

    pub fn polygon(
        side: BorderSide,
        sides: f32,
        point_rounding: f32,
        rotation_degrees: f32,
        squash: f32,
    ) -> StarBorder {
        debug_assert!(sides >= 2.0);
        StarBorder {
            side,
            points: sides,
            inner_radius_ratio: None,
            point_rounding,
            valley_rounding: 0.0,
            rotation_radians: rotation_degrees.to_radians(),
            squash,
        }
    }

    /// The polygon's incircle radius when this is a polygon.
    pub fn inner_radius_ratio(&self) -> f32 {
        self.inner_radius_ratio
            .unwrap_or_else(|| (std::f32::consts::PI / self.points).cos())
    }

    pub fn rotation(&self) -> f32 {
        self.rotation_radians.to_degrees()
    }

    pub fn scale(&self, t: f32) -> StarBorder {
        StarBorder {
            side: self.side.scale(t),
            ..*self
        }
    }

    fn generator(&self) -> StarGenerator {
        StarGenerator {
            points: self.points,
            inner_radius_ratio: self.inner_radius_ratio(),
            point_rounding: self.point_rounding,
            valley_rounding: self.valley_rounding,
            rotation: self.rotation_radians,
            squash: self.squash,
        }
    }

    /// Upstream `StarBorder.lerpFrom`'s circle path: points animate toward
    /// their nearest whole count while the rounding grows in.
    fn lerp_from_circle(
        star: &StarBorder,
        side: BorderSide,
        from_eccentricity: f32,
        t: f32,
    ) -> StarBorder {
        if star.points >= 2.5 {
            let lerped_points = lerp_double(star.points.round(), star.points, t);
            StarBorder {
                side,
                points: lerped_points,
                inner_radius_ratio: Some(lerp_double(
                    (std::f32::consts::PI / lerped_points).cos(),
                    star.inner_radius_ratio(),
                    t,
                )),
                point_rounding: lerp_double(1.0, star.point_rounding, t),
                valley_rounding: lerp_double(0.0, star.valley_rounding, t),
                rotation_radians: star.rotation_radians,
                squash: lerp_double(from_eccentricity, star.squash, t),
            }
        } else {
            // Two-pointed stars get squirrelly near a zero inner radius.
            let lerped_points = lerp_double(star.points, 2.0, t);
            StarBorder {
                side,
                points: lerped_points,
                inner_radius_ratio: Some(lerp_double(1.0, star.inner_radius_ratio(), t)),
                point_rounding: lerp_double(0.5, star.point_rounding, t),
                valley_rounding: lerp_double(0.5, star.valley_rounding, t),
                rotation_radians: star.rotation_radians,
                squash: lerp_double(from_eccentricity, star.squash, t),
            }
        }
    }

    /// Upstream `StarBorder.lerpTo`'s circle path, the same walk backwards.
    fn lerp_to_circle(
        star: &StarBorder,
        side: BorderSide,
        to_eccentricity: f32,
        t: f32,
    ) -> StarBorder {
        if star.points >= 2.5 {
            let lerped_points = lerp_double(star.points, star.points.round(), t);
            StarBorder {
                side,
                points: lerped_points,
                inner_radius_ratio: Some(lerp_double(
                    star.inner_radius_ratio(),
                    (std::f32::consts::PI / lerped_points).cos(),
                    t,
                )),
                point_rounding: lerp_double(star.point_rounding, 1.0, t),
                valley_rounding: lerp_double(star.valley_rounding, 0.0, t),
                rotation_radians: star.rotation_radians,
                squash: lerp_double(star.squash, to_eccentricity, t),
            }
        } else {
            let lerped_points = lerp_double(star.points, 2.0, t);
            StarBorder {
                side,
                points: lerped_points,
                inner_radius_ratio: Some(lerp_double(star.inner_radius_ratio(), 1.0, t)),
                point_rounding: lerp_double(star.point_rounding, 0.5, t),
                valley_rounding: lerp_double(star.valley_rounding, 0.5, t),
                rotation_radians: star.rotation_radians,
                squash: lerp_double(star.squash, to_eccentricity, t),
            }
        }
    }

    fn lerp_star(a: &StarBorder, b: &StarBorder, t: f32) -> StarBorder {
        StarBorder {
            side: BorderSide::lerp(a.side, b.side, t),
            points: lerp_double(a.points, b.points, t),
            inner_radius_ratio: Some(lerp_double(
                a.inner_radius_ratio(),
                b.inner_radius_ratio(),
                t,
            )),
            point_rounding: lerp_double(a.point_rounding, b.point_rounding, t),
            valley_rounding: lerp_double(a.valley_rounding, b.valley_rounding, t),
            rotation_radians: lerp_double(a.rotation_radians, b.rotation_radians, t),
            squash: lerp_double(a.squash, b.squash, t),
        }
    }

    /// Upstream `_twoPhaseLerp`: two lerps over the two halves of the
    /// timeline, each re-parameterised to its own half.
    fn two_phase_lerp(
        t: f32,
        split: f32,
        first: impl FnOnce(f32) -> ShapeBorder,
        second: impl FnOnce(f32) -> ShapeBorder,
    ) -> ShapeBorder {
        if t < split {
            first(t * (1.0 / split))
        } else {
            second((1.0 / (1.0 - split)) * (t - split))
        }
    }
}

/// The sharp vertices one star arm contributes, and where its rounded
/// portions start and end -- upstream `_PointInfo`.
#[derive(Clone, Copy, Debug, Default)]
struct StarPointInfo {
    valley: (f32, f32),
    point: (f32, f32),
    valley_arc1: (f32, f32),
    point_arc1: (f32, f32),
    point_arc2: (f32, f32),
    valley_arc2: (f32, f32),
}

/// Upstream `_StarGenerator`. Divergences, both engine-bound: the conics
/// that round the points and valleys become cubics at the w/3 approximation
/// (the weights here stay within 0..=1, where the error is a hair), and the
/// squash/rotation matrix is baked into the generated points instead of
/// transforming the finished path -- affine moves of Bézier controls are
/// exact, so only the conic approximation carries any error.
struct StarGenerator {
    points: f32,
    inner_radius_ratio: f32,
    point_rounding: f32,
    valley_rounding: f32,
    rotation: f32,
    squash: f32,
}

impl StarGenerator {
    fn generate(&self, rect: Rect) -> RenderPath {
        let radius = rect_shortest_side(rect) / 2.0;
        let center = rect_center(rect);

        // Map away from a near-zero inner radius, where the path degenerates.
        const MIN_INNER_RADIUS_RATIO: f32 = 0.002;
        let mapped_inner_radius_ratio =
            self.inner_radius_ratio * (1.0 - MIN_INNER_RADIUS_RATIO) + MIN_INNER_RADIUS_RATIO;

        let mut star_points = Vec::new();
        let max_radius = self.generate_points(
            &mut star_points,
            center,
            radius,
            radius * mapped_inner_radius_ratio,
        );

        let mut scale = (
            rect.width() / (2.0 * max_radius),
            rect.height() / (2.0 * max_radius),
        );
        if rect_shortest_side(rect) == rect.width() {
            scale.1 = self.squash * scale.1 + (1.0 - self.squash) * scale.0;
        } else {
            scale.0 = self.squash * scale.0 + (1.0 - self.squash) * scale.1;
        }
        // p' = center + scale·R(rotation)·(p - center), the squash matrix
        // upstream builds and applies to the finished path.
        let (sin_r, cos_r) = self.rotation.sin_cos();
        let transform = |p: (f32, f32)| -> (f32, f32) {
            let dx = p.0 - center.dx;
            let dy = p.1 - center.dy;
            let rotated = (dx * cos_r - dy * sin_r, dx * sin_r + dy * cos_r);
            (
                center.dx + rotated.0 * scale.0,
                center.dy + rotated.1 * scale.1,
            )
        };
        let transformed: Vec<StarPointInfo> = star_points
            .iter()
            .map(|info| StarPointInfo {
                valley: transform(info.valley),
                point: transform(info.point),
                valley_arc1: transform(info.valley_arc1),
                point_arc1: transform(info.point_arc1),
                point_arc2: transform(info.point_arc2),
                valley_arc2: transform(info.valley_arc2),
            })
            .collect();

        let mut path = RenderPath::new();
        self.draw_points(&mut path, &transformed);
        path
    }

    /// The furthest radius the rounded star reaches, upstream's
    /// `_generatePoints` return.
    fn generate_points(
        &self,
        point_list: &mut Vec<StarPointInfo>,
        center: Offset,
        radius: f32,
        inner_radius: f32,
    ) -> f32 {
        let step = std::f32::consts::PI / self.points;
        // Start one step before zero.
        let mut angle = -std::f32::consts::FRAC_PI_2 - step;
        let mut valley = (
            center.dx + angle.cos() * inner_radius,
            center.dy + angle.sin() * inner_radius,
        );

        // The rational quadratic's midpoint, to measure where the rounding
        // actually reaches.
        let curve_midpoint = |a: (f32, f32),
                              b: (f32, f32),
                              c: (f32, f32),
                              a1: (f32, f32),
                              c1: (f32, f32)|
         -> (f32, f32) {
            let angle = angle_between(a, b, c);
            let w = weight_for(angle) / 2.0;
            (
                (a1.0 / 4.0 + b.0 * w + c1.0 / 4.0) / (0.5 + w),
                (a1.1 / 4.0 + b.1 * w + c1.1 / 4.0) / (0.5 + w),
            )
        };

        // One star arm: the point between two valleys, with the rounded
        // portions' start and end points. A fn rather than a closure so the
        // point list stays borrowable after the loop.
        fn add_point(
            generator: &StarGenerator,
            point_list: &mut Vec<StarPointInfo>,
            angle: f32,
            point_step: f32,
            point_radius: f32,
            point_inner_radius: f32,
            center: Offset,
            valley: &mut (f32, f32),
        ) -> f32 {
            let mut angle = angle + point_step;
            let point = (
                center.dx + angle.cos() * point_radius,
                center.dy + angle.sin() * point_radius,
            );
            angle += point_step;
            let next_valley = (
                center.dx + angle.cos() * point_inner_radius,
                center.dy + angle.sin() * point_inner_radius,
            );
            let lerp_point = |from: (f32, f32), to: (f32, f32), t: f32| {
                (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
            };
            point_list.push(StarPointInfo {
                valley: *valley,
                point,
                valley_arc1: lerp_point(*valley, point, generator.valley_rounding),
                point_arc1: lerp_point(point, *valley, generator.point_rounding),
                point_arc2: lerp_point(point, next_valley, generator.point_rounding),
                valley_arc2: lerp_point(next_valley, point, generator.valley_rounding),
            });
            *valley = next_valley;
            angle
        }

        let remainder = self.points - self.points.trunc();
        let has_integer_sides = remainder < 1e-6;
        let whole_sides = self.points - if has_integer_sides { 0.0 } else { 1.0 };
        for _ in 0..whole_sides as usize {
            angle = add_point(
                self,
                point_list,
                angle,
                step,
                radius,
                inner_radius,
                center,
                &mut valley,
            );
        }

        let this_point = point_list[0];
        let next_point = point_list[1];

        let point_midpoint = curve_midpoint(
            this_point.valley,
            this_point.point,
            next_point.valley,
            this_point.point_arc1,
            this_point.point_arc2,
        );
        let valley_midpoint = curve_midpoint(
            this_point.point,
            next_point.valley,
            next_point.point,
            this_point.valley_arc2,
            next_point.valley_arc1,
        );
        let distance = |p: (f32, f32)| -> f32 {
            ((p.0 - center.dx).powi(2) + (p.1 - center.dy).powi(2)).sqrt()
        };
        let valley_radius = distance(valley_midpoint);
        let point_radius = distance(point_midpoint);

        // A fractional side count finishes the shape with a short arm.
        if !has_integer_sides {
            let effective_inner_radius = valley_radius.max(inner_radius);
            let ending_radius =
                effective_inner_radius + remainder * (radius - effective_inner_radius);
            add_point(
                self,
                point_list,
                angle,
                step * remainder,
                ending_radius,
                inner_radius,
                center,
                &mut valley,
            );
        }

        // Whichever reaches further -- valley rounding can push past point
        // rounding -- sizes the shape, and must stay finite and non-zero.
        valley_radius.max(point_radius).max(f32::MIN_POSITIVE)
    }

    fn draw_points(&self, path: &mut RenderPath, points: &[StarPointInfo]) {
        let starting_point = points[0].point_arc1;
        path.move_to(starting_point.0, starting_point.1);
        let point_angle = angle_between(points[0].valley, points[0].point, points[1].valley);
        let point_weight = weight_for(point_angle);
        let valley_angle = angle_between(points[1].point, points[1].valley, points[0].point);
        let valley_weight = weight_for(valley_angle);

        for index in 0..points.len() {
            let point = points[index];
            let next_point = points[(index + 1) % points.len()];
            // Each conic starts where the line before it ended.
            path.line_to(point.point_arc1.0, point.point_arc1.1);
            if point_angle != 180.0 && point_angle != 0.0 {
                conic_as_cubic(
                    path,
                    point.point_arc1,
                    point.point,
                    point.point_arc2,
                    point_weight,
                );
            } else {
                path.line_to(point.point_arc2.0, point.point_arc2.1);
            }
            path.line_to(point.valley_arc2.0, point.valley_arc2.1);
            if valley_angle != 180.0 && valley_angle != 0.0 {
                conic_as_cubic(
                    path,
                    point.valley_arc2,
                    next_point.valley,
                    next_point.valley_arc1,
                    valley_weight,
                );
            } else {
                path.line_to(next_point.valley_arc1.0, next_point.valley_arc1.1);
            }
        }
        path.close();
    }
}

/// Upstream `_getWeight`.
fn weight_for(angle: f32) -> f32 {
    ((angle / 2.0) % (std::f32::consts::FRAC_PI_2)).cos()
}

/// Upstream `_getAngle`: the included angle ABC in radians.
fn angle_between(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    if a == c || b == c || b == a {
        return 0.0;
    }
    let u = (a.0 - b.0, a.1 - b.1);
    let v = (c.0 - b.0, c.1 - b.1);
    let dot = u.0 * v.0 + u.1 * v.1;
    let m1 = if b.0 == a.0 { f32::INFINITY } else { u.1 / u.0 };
    let m2 = if b.0 == c.0 { f32::INFINITY } else { v.1 / v.0 };
    let mut angle = (m1 - m2).atan2(1.0 + m1 * m2).abs();
    if dot < 0.0 {
        angle += std::f32::consts::PI;
    }
    angle
}

/// A conic (rational quadratic) as a cubic: controls at one third of the way
/// from each end, scaled by the weight. Exact for `w == 1` (a parabola,
/// drawn as a quadratic) and a hair's-width approximation for the weights
/// this file sees, which stay within 0..=1.
fn conic_as_cubic(
    path: &mut RenderPath,
    from: (f32, f32),
    control: (f32, f32),
    to: (f32, f32),
    weight: f32,
) {
    if (weight - 1.0).abs() < 1e-6 {
        path.quadratic_to(control.0, control.1, to.0, to.1);
        return;
    }
    path.cubic_to(
        from.0 + (control.0 - from.0) * weight / 3.0,
        from.1 + (control.1 - from.1) * weight / 3.0,
        to.0 + (control.0 - to.0) * weight / 3.0,
        to.1 + (control.1 - to.1) * weight / 3.0,
        to.0,
        to.1,
    );
}

// -- ShapeDecoration (upstream shape_decoration.dart) -----------------------------

use crate::render::Fill;

/// An immutable description of how to paint an arbitrary shape: an interior
/// fill, a shape, and the shadows the shape casts. Upstream also carries an
/// `image`; `DecorationImage` is a later wave (see the module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeDecoration {
    pub fill: Option<Fill>,
    pub shadows: Vec<BoxShadow>,
    pub shape: ShapeBorder,
}

impl ShapeDecoration {
    pub fn new(shape: ShapeBorder) -> ShapeDecoration {
        ShapeDecoration {
            fill: None,
            shadows: Vec::new(),
            shape,
        }
    }

    /// Upstream `ShapeDecoration.fromBoxDecoration`: the same box, spelled
    /// as a shape -- a circle as `CircleBorder`, a rounded rectangle as
    /// `RoundedRectangleBorder`, everything else as the border itself.
    pub fn from_box_decoration(source: &crate::decoration::BoxDecoration) -> ShapeDecoration {
        let shape = match source.shape {
            BoxShape::Circle => match &source.border {
                Some(BoxBorder::Uniform(border)) => {
                    ShapeBorder::Circle(CircleBorder::new(border.top, 0.0))
                }
                _ => ShapeBorder::Circle(CircleBorder::default()),
            },
            BoxShape::Rectangle => match &source.border_radius {
                Some(radius) => {
                    let side = match &source.border {
                        Some(BoxBorder::Uniform(border)) => border.top,
                        Some(BoxBorder::Directional(border)) => border.top,
                        Some(BoxBorder::None) | None => BorderSide::NONE,
                    };
                    ShapeBorder::Rounded(RoundedRectangleBorder::new(side, *radius))
                }
                None => match &source.border {
                    Some(BoxBorder::Uniform(border)) => ShapeBorder::Border(*border),
                    Some(BoxBorder::Directional(border)) => ShapeBorder::Directional(*border),
                    Some(BoxBorder::None) | None => ShapeBorder::Border(Border::default()),
                },
            },
        };
        ShapeDecoration {
            fill: source.fill.clone(),
            shadows: source.box_shadow.clone(),
            shape,
        }
    }

    pub fn with_fill(mut self, fill: Fill) -> ShapeDecoration {
        self.fill = Some(fill);
        self
    }

    pub fn with_shadows(mut self, shadows: Vec<BoxShadow>) -> ShapeDecoration {
        self.shadows = shadows;
        self
    }

    /// Upstream `ShapeDecoration.padding`.
    pub fn padding(&self) -> EdgeInsetsGeometry {
        self.shape.dimensions()
    }

    /// Upstream `ShapeDecoration.hitTest`.
    pub fn hit_test(&self, size: (f32, f32), position: Offset, direction: TextDirection) -> bool {
        self.shape
            .hit_test(rect_at(Offset::ZERO, size), position, direction)
    }

    /// Upstream `_ShapeDecorationPainter.paint`: shadows under the interior,
    /// the interior under the border.
    pub fn paint(&self, canvas: &mut Canvas, rect: Rect, direction: TextDirection) {
        for shadow in &self.shadows {
            let spread = shadow.spread_radius;
            let shadow_rect = Rect::ltrb(
                rect.left + shadow.offset.dx - spread,
                rect.top + shadow.offset.dy - spread,
                rect.right + shadow.offset.dx + spread,
                rect.bottom + shadow.offset.dy + spread,
            );
            if rect_is_empty(shadow_rect) {
                continue;
            }
            let paint = shadow.to_paint();
            canvas.draw_path(&self.shape.outer_path(shadow_rect, direction), &paint);
        }
        if let Some(fill) = &self.fill {
            if let Some(paint) = fill_paint(fill, rect) {
                canvas.draw_path(&self.shape.outer_path(rect, direction), &paint);
            }
        }
        self.shape.paint(canvas, rect, direction);
    }

    /// Upstream `ShapeDecoration.lerp`, narrowed: shapes morph exactly as
    /// upstream; a solid fill lerps its colour; gradient fills swap at the
    /// half-way point (upstream lerps gradients stop-by-stop -- pending the
    /// painting wave).
    pub fn lerp(
        a: Option<&ShapeDecoration>,
        b: Option<&ShapeDecoration>,
        t: f32,
    ) -> Option<ShapeDecoration> {
        if a == b {
            return a.cloned();
        }
        let (a, b) = (a?, b?);
        if t == 0.0 {
            return Some(a.clone());
        }
        if t == 1.0 {
            return Some(b.clone());
        }
        let fill = match (&a.fill, &b.fill) {
            (Some(Fill::Solid(from)), Some(Fill::Solid(to))) => {
                Some(Fill::Solid(color_lerp(*from, *to, t)))
            }
            (None, None) => None,
            // Gradient fills swap at the half-way point; upstream lerps
            // them stop-by-stop (pending the painting wave).
            _ => {
                if t < 0.5 {
                    a.fill.clone()
                } else {
                    b.fill.clone()
                }
            }
        };
        let mut shadows = Vec::new();
        for index in 0..a.shadows.len().max(b.shadows.len()) {
            let from = a.shadows.get(index);
            let to = b.shadows.get(index);
            let shadow = match (from, to) {
                (Some(from), Some(to)) => BoxShadow::lerp(from, to, t),
                (Some(from), None) => from.scale(1.0 - t),
                (None, Some(to)) => to.scale(t),
                (None, None) => continue,
            };
            shadows.push(shadow);
        }
        Some(ShapeDecoration {
            fill,
            shadows,
            shape: ShapeBorder::lerp(Some(a.shape.clone()), Some(b.shape.clone()), t)?,
        })
    }
}

/// `RenderDecoratedBox::build_paint`'s fill-to-paint mapping, shared with
/// the shape decoration's interior.
pub(crate) fn fill_paint(fill: &Fill, rect: Rect) -> Option<Paint> {
    match fill {
        Fill::Solid(color) => Some(Paint::new(*color)),
        Fill::Linear {
            start,
            end,
            gradient,
        } => {
            let from = crate::render::point_in(rect, *start);
            let to = crate::render::point_in(rect, *end);
            Some(Paint::new(Color::WHITE).with_linear_gradient(from, to, gradient))
        }
        Fill::Radial {
            center,
            radius,
            gradient,
        } => {
            let at = crate::render::point_in(rect, *center);
            Some(Paint::new(Color::WHITE).with_radial_gradient(at, *radius, gradient))
        }
    }
}

// -- TableBorder (upstream rendering/table_border.dart) ----------------------------

/// Upstream `TableBorder`: the four outer sides of a table plus the two
/// interior ones, painted between the rows and columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableBorder {
    pub top: BorderSide,
    pub right: BorderSide,
    pub bottom: BorderSide,
    pub left: BorderSide,
    pub horizontal_inside: BorderSide,
    pub vertical_inside: BorderSide,
    /// Applied to the outer border only -- the DataTable-in-Material case.
    pub border_radius: BorderRadius,
}

impl Default for TableBorder {
    fn default() -> TableBorder {
        TableBorder {
            top: BorderSide::NONE,
            right: BorderSide::NONE,
            bottom: BorderSide::NONE,
            left: BorderSide::NONE,
            horizontal_inside: BorderSide::NONE,
            vertical_inside: BorderSide::NONE,
            border_radius: BorderRadius::ZERO,
        }
    }
}

impl TableBorder {
    /// Upstream `TableBorder.all`.
    pub fn all(side: BorderSide) -> TableBorder {
        TableBorder {
            top: side,
            right: side,
            bottom: side,
            left: side,
            horizontal_inside: side,
            vertical_inside: side,
            border_radius: BorderRadius::ZERO,
        }
    }

    /// Upstream `TableBorder.symmetric` / the `only` spelling.
    pub fn only(
        top: BorderSide,
        right: BorderSide,
        bottom: BorderSide,
        left: BorderSide,
        horizontal_inside: BorderSide,
        vertical_inside: BorderSide,
    ) -> TableBorder {
        TableBorder {
            top,
            right,
            bottom,
            left,
            horizontal_inside,
            vertical_inside,
            border_radius: BorderRadius::ZERO,
        }
    }

    /// Upstream `TableBorder.dimensions`.
    pub fn dimensions(&self) -> EdgeInsetsGeometry {
        EdgeInsetsGeometry::Absolute(EdgeInsets {
            left: self.left.width,
            top: self.top.width,
            right: self.right.width,
            bottom: self.bottom.width,
        })
    }

    fn all_sides_match(&self, selector: impl Fn(BorderSide) -> bool) -> bool {
        selector(self.top)
            && selector(self.right)
            && selector(self.bottom)
            && selector(self.left)
            && selector(self.horizontal_inside)
            && selector(self.vertical_inside)
    }

    fn outer_sides_match(&self, selector: impl Fn(BorderSide) -> bool) -> bool {
        selector(self.top) && selector(self.right) && selector(self.bottom) && selector(self.left)
    }

    /// Upstream `isUniform`.
    pub fn is_uniform(&self) -> bool {
        let top = self.top;
        self.all_sides_match(|side| side.color == top.color)
            && self.all_sides_match(|side| side.width == top.width)
            && self.all_sides_match(|side| side.style == top.style)
    }

    fn outer_border_is_uniform(&self) -> bool {
        let top = self.top;
        self.outer_sides_match(|side| side.color == top.color)
            && self.outer_sides_match(|side| side.width == top.width)
            && self.outer_sides_match(|side| side.style == top.style)
    }

    /// The colours the outer sides would actually paint, deduplicated.
    fn distinct_visible_outer_colors(&self) -> Vec<Color> {
        let mut colors = Vec::new();
        for side in [self.top, self.right, self.bottom, self.left] {
            if side.style != BorderStyle::None && !colors.contains(&side.color) {
                colors.push(side.color);
            }
        }
        colors
    }

    /// Upstream `_paintTableBorder`: the outer rectangle.
    fn paint_table_border(&self, canvas: &mut Canvas, rect: Rect) {
        if self.outer_border_is_uniform() && self.border_radius != BorderRadius::ZERO {
            // The uniform rounded spelling: a double rrect.
            let outer = self.border_radius.to_rrect(rect);
            let inner = outer.deflate(self.top.width);
            let mut path = RenderPath::new().with_fill_type(FillType::EvenOdd);
            outer.append_to(&mut path);
            inner.append_to(&mut path);
            canvas.draw_path(&path, &Paint::new(self.top.color));
            return;
        }
        let visible_colors = self.distinct_visible_outer_colors();
        if visible_colors.len() == 1 && self.border_radius != BorderRadius::ZERO {
            // One colour, per-side widths: the non-uniform rounded spelling,
            // a double rrect between per-side insets and outsets.
            let nil = BorderSide::NONE;
            let border_rect = self.border_radius.to_rrect(rect);
            let inner = border_rect.inset_insets(
                if self.left.style == BorderStyle::None {
                    nil
                } else {
                    self.left
                }
                .stroke_inset(),
                if self.top.style == BorderStyle::None {
                    nil
                } else {
                    self.top
                }
                .stroke_inset(),
                if self.right.style == BorderStyle::None {
                    nil
                } else {
                    self.right
                }
                .stroke_inset(),
                if self.bottom.style == BorderStyle::None {
                    nil
                } else {
                    self.bottom
                }
                .stroke_inset(),
            );
            let outer = border_rect.inset_insets(
                -if self.left.style == BorderStyle::None {
                    nil
                } else {
                    self.left
                }
                .stroke_outset(),
                -if self.top.style == BorderStyle::None {
                    nil
                } else {
                    self.top
                }
                .stroke_outset(),
                -if self.right.style == BorderStyle::None {
                    nil
                } else {
                    self.right
                }
                .stroke_outset(),
                -if self.bottom.style == BorderStyle::None {
                    nil
                } else {
                    self.bottom
                }
                .stroke_outset(),
            );
            let mut path = RenderPath::new().with_fill_type(FillType::EvenOdd);
            outer.append_to(&mut path);
            inner.append_to(&mut path);
            canvas.draw_path(&path, &Paint::new(visible_colors[0]));
            return;
        }
        // The plain spelling: four trapezoids.
        paint_border(canvas, rect, self.top, self.right, self.bottom, self.left);
    }

    /// Upstream `TableBorder.paint`: the interior grid first, the outer
    /// border last. `rows`/`columns` are the interior boundary offsets.
    pub fn paint(&self, canvas: &mut Canvas, rect: Rect, rows: &[f32], columns: &[f32]) {
        if !columns.is_empty() && self.vertical_inside.style == BorderStyle::Solid {
            let paint = Paint::new(self.vertical_inside.color).with_style(Style::Stroke {
                width: self.vertical_inside.width,
            });
            let mut path = RenderPath::new();
            for x in columns {
                path.move_to(rect.left + x, rect.top);
                path.line_to(rect.left + x, rect.bottom);
            }
            canvas.draw_path(&path, &paint);
        }
        if !rows.is_empty() && self.horizontal_inside.style == BorderStyle::Solid {
            let paint = Paint::new(self.horizontal_inside.color).with_style(Style::Stroke {
                width: self.horizontal_inside.width,
            });
            let mut path = RenderPath::new();
            for y in rows {
                path.move_to(rect.left, rect.top + y);
                path.line_to(rect.right, rect.top + y);
            }
            canvas.draw_path(&path, &paint);
        }
        self.paint_table_border(canvas, rect);
    }

    /// Upstream `TableBorder.scale`.
    pub fn scale(&self, t: f32) -> TableBorder {
        TableBorder {
            top: self.top.scale(t),
            right: self.right.scale(t),
            bottom: self.bottom.scale(t),
            left: self.left.scale(t),
            horizontal_inside: self.horizontal_inside.scale(t),
            vertical_inside: self.vertical_inside.scale(t),
            border_radius: self.border_radius,
        }
    }

    /// Upstream `TableBorder.lerp`.
    pub fn lerp(a: Option<&TableBorder>, b: Option<&TableBorder>, t: f32) -> TableBorder {
        match (a, b) {
            (None, Some(b)) => b.scale(t),
            (Some(a), None) => a.scale(1.0 - t),
            (Some(a), Some(b)) => TableBorder {
                top: BorderSide::lerp(a.top, b.top, t),
                right: BorderSide::lerp(a.right, b.right, t),
                bottom: BorderSide::lerp(a.bottom, b.bottom, t),
                left: BorderSide::lerp(a.left, b.left, t),
                horizontal_inside: BorderSide::lerp(a.horizontal_inside, b.horizontal_inside, t),
                vertical_inside: BorderSide::lerp(a.vertical_inside, b.vertical_inside, t),
                // **Upstream drops the radius.** Its `lerp` builds a
                // `TableBorder(...)` with the six sides and no
                // `borderRadius:` argument at all, so the result takes the
                // constructor's default of zero -- a rounded table border
                // animating to another rounded one loses its corners for the
                // whole of the animation and gets them back at the end.
                //
                // That reads like an oversight and it is ported as written,
                // for the reason tick 206 gives about the floating cursor:
                // guessing an upstream omission into the shape it looks like
                // it should have is inventing behaviour. This port said
                // something else -- "the border carries whichever side `t`
                // has passed" -- which was kinder and was **also not what
                // upstream does**, and nothing could tell, because nothing
                // asked what the radius was after a lerp.
                border_radius: BorderRadius::ZERO,
            },
            (None, None) => TableBorder::default(),
        }
    }
}

// -- Notched shapes (upstream notched_shapes.dart) ---------------------------------

/// A shape with a notch in its outline: the host rectangle minus a guest,
/// as a bottom app bar makes room for a floating action button.
#[derive(Clone, Debug, PartialEq)]
pub enum NotchedShape {
    /// `CircularNotchedRectangle`: a rectangle with a smooth circular notch.
    Circular { inverted: bool },
    /// `AutomaticNotchedShape`: a host `ShapeBorder` with a guest
    /// subtracted. Needs `Path.combine(PathOperation.difference)`; the
    /// engine ABI has no path boolean ops yet, so until that lands the
    /// guest is ignored (see the module docs).
    Automatic {
        host: ShapeBorder,
        guest: Option<ShapeBorder>,
    },
}

impl NotchedShape {
    pub fn outer_path(&self, host: Rect, guest: Option<Rect>) -> RenderPath {
        match self {
            NotchedShape::Circular { inverted } => circular_notched_path(host, guest, *inverted),
            NotchedShape::Automatic {
                host: host_shape, ..
            } => host_shape.outer_path(host, TextDirection::Ltr),
        }
    }
}

/// Upstream `CircularNotchedRectangle.getOuterPath`: the notch is three
/// segments -- a Bézier down from the host's edge, an arc around the
/// guest, and a Bézier back up. The arc and the quadratics become cubics
/// here (the engine path ABI has neither `arcToPoint` nor quadratics kept
/// as-is on the way through -- `RenderPath::quadratic_to` exists, but the
/// arc still needs the kappa treatment).
fn circular_notched_path(host: Rect, guest: Option<Rect>, inverted: bool) -> RenderPath {
    let guest = match guest {
        Some(guest) if rect_overlaps(host, guest) => guest,
        _ => {
            let mut path = RenderPath::new();
            path.add_rect(host);
            return path;
        }
    };

    // The guest is a circle bounded by its rectangle.
    let r = guest.width() / 2.0;
    let guest_center = rect_center(guest);
    let invert_multiplier = if inverted { -1.0 } else { 1.0 };

    // Derivation upstream links to a design doc; the constants are theirs.
    const S1: f32 = 15.0;
    const S2: f32 = 1.0;

    let a = -r - S2;
    let b = (if inverted { host.bottom } else { host.top }) - guest_center.dy;

    let n2 = (b * b * r * r * (a * a + b * b - r * r)).sqrt();
    let p2xa = ((a * r * r) - n2) / (a * a + b * b);
    let p2xb = ((a * r * r) + n2) / (a * a + b * b);
    let p2ya = (r * r - p2xa * p2xa).sqrt() * invert_multiplier;
    let p2yb = (r * r - p2xb * p2xb).sqrt() * invert_multiplier;

    // p0/p1/p2 control the segment from the host's edge toward the guest;
    // p3/p4/p5 mirror it on the far side.
    let mut p = [(0.0f32, 0.0f32); 6];
    p[0] = (a - S1, b);
    p[1] = (a, b);
    let cmp = if b < 0.0 { -1.0 } else { 1.0 };
    p[2] = if cmp * p2ya > cmp * p2yb {
        (p2xa, p2ya)
    } else {
        (p2xb, p2yb)
    };
    p[3] = (-p[2].0, p[2].1);
    p[4] = (-p[1].0, p[1].1);
    p[5] = (-p[0].0, p[0].1);

    for point in &mut p {
        *point = (point.0 + guest_center.dx, point.1 + guest_center.dy);
    }

    // The path cursor, tracked here because the engine does not read it
    // back out of a path under construction.
    let mut current: (f32, f32) = if inverted {
        (host.left, host.top)
    } else {
        (host.left, host.top)
    };
    let mut path = RenderPath::new();
    path.move_to(current.0, current.1);

    // A quadratic Bézier kept as a quadratic.
    let quad_to =
        |path: &mut RenderPath, current: &mut (f32, f32), control: (f32, f32), to: (f32, f32)| {
            path.quadratic_to(control.0, control.1, to.0, to.1);
            *current = to;
        };
    // A counter-clockwise (on screen) arc of the guest circle, as a cubic
    // with kappa scaled by the sweep -- screen-CCW is decreasing atan2
    // angle when y grows downward.
    let arc_to = |path: &mut RenderPath, current: &mut (f32, f32), to: (f32, f32)| {
        let from = *current;
        let a0 = (from.1 - guest_center.dy).atan2(from.0 - guest_center.dx);
        let a1 = (to.1 - guest_center.dy).atan2(to.0 - guest_center.dx);
        let mut sweep = a1 - a0;
        while sweep >= 0.0 {
            sweep -= std::f32::consts::TAU;
        }
        let kappa = KAPPA * (-sweep) / std::f32::consts::FRAC_PI_2;
        // Tangent along decreasing angle: (sin a, -cos a).
        let (sin0, cos0) = a0.sin_cos();
        let (sin1, cos1) = a1.sin_cos();
        path.cubic_to(
            from.0 + kappa * r * sin0,
            from.1 - kappa * r * cos0,
            to.0 - kappa * r * sin1,
            to.1 + kappa * r * cos1,
            to.0,
            to.1,
        );
        *current = to;
    };

    if !inverted {
        path.line_to(p[0].0, p[0].1);
        current = p[0];
        quad_to(&mut path, &mut current, p[1], p[2]);
        arc_to(&mut path, &mut current, p[3]);
        quad_to(&mut path, &mut current, p[4], p[5]);
        path.line_to(host.right, host.top);
        path.line_to(host.right, host.bottom);
        path.line_to(host.left, host.bottom);
    } else {
        path.line_to(host.right, host.top);
        path.line_to(host.right, host.bottom);
        path.line_to(p[5].0, p[5].1);
        current = p[5];
        quad_to(&mut path, &mut current, p[4], p[3]);
        arc_to(&mut path, &mut current, p[2]);
        quad_to(&mut path, &mut current, p[1], p[0]);
        path.line_to(host.left, host.bottom);
    }
    path.close();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color(0xFF0000FF);
    const BLUE: Color = Color(0xFFFF0000);

    fn side(color: Color, width: f32) -> BorderSide {
        BorderSide {
            color,
            width,
            ..BorderSide::default()
        }
    }

    // -- Radius / BorderRadius --------------------------------------------------

    #[test]
    fn radius_ops_and_lerp() {
        let a = Radius::elliptical(10.0, 20.0);
        let b = Radius::elliptical(30.0, 60.0);
        assert_eq!(a + b, Radius::elliptical(40.0, 80.0));
        assert_eq!(b - a, Radius::elliptical(20.0, 40.0));
        assert_eq!(a * 2.0, Radius::elliptical(20.0, 40.0));
        assert_eq!(-a, Radius::elliptical(-10.0, -20.0));
        assert_eq!(Radius::lerp(a, b, 0.5), Radius::elliptical(20.0, 40.0));
        assert!(Radius::circular(4.0).is_circular());
        assert!(!a.is_circular());
    }

    #[test]
    fn border_radius_add_sub_and_lerp() {
        let a = BorderRadius::only(
            Radius::circular(1.0),
            Radius::circular(2.0),
            Radius::circular(3.0),
            Radius::circular(4.0),
        );
        let b = BorderRadius::all(Radius::circular(10.0));
        assert_eq!(
            a + b,
            BorderRadius::only(
                Radius::circular(11.0),
                Radius::circular(12.0),
                Radius::circular(13.0),
                Radius::circular(14.0),
            )
        );
        assert_eq!((a + b) - b, a);
        assert_eq!(BorderRadius::lerp(a, b, 0.5), (a + b) * 0.5);
        // Upstream lerp is per-corner, not the add-subtract path.
        assert_eq!(
            BorderRadius::lerp(BorderRadius::circular(0.0), b, 0.5),
            BorderRadius::all(Radius::circular(5.0))
        );
    }

    #[test]
    fn directional_radius_resolves_by_direction() {
        let radius = BorderRadiusDirectional::only(
            Radius::circular(1.0),
            Radius::circular(2.0),
            Radius::circular(3.0),
            Radius::circular(4.0),
        );
        assert_eq!(
            radius.resolve(TextDirection::Ltr),
            BorderRadius::only(
                Radius::circular(1.0),
                Radius::circular(2.0),
                Radius::circular(3.0),
                Radius::circular(4.0),
            )
        );
        assert_eq!(
            radius.resolve(TextDirection::Rtl),
            BorderRadius::only(
                Radius::circular(2.0),
                Radius::circular(1.0),
                Radius::circular(4.0),
                Radius::circular(3.0),
            )
        );
    }

    #[test]
    fn mixed_radius_adds_both_kinds_per_corner() {
        let physical = BorderRadius::all(Radius::circular(4.0));
        let logical =
            BorderRadiusDirectional::horizontal(Radius::circular(1.0), Radius::circular(2.0));
        let mixed = BorderRadiusGeometry::Absolute(physical)
            .add(BorderRadiusGeometry::Directional(logical));
        // Left-to-right: start is left, so left corners get 4+1.
        assert_eq!(
            mixed.resolve(TextDirection::Ltr).top_left,
            Radius::circular(5.0)
        );
        assert_eq!(
            mixed.resolve(TextDirection::Ltr).top_right,
            Radius::circular(6.0)
        );
        assert_eq!(
            mixed.resolve(TextDirection::Rtl).top_left,
            Radius::circular(6.0)
        );
        assert_eq!(
            mixed.resolve(TextDirection::Rtl).top_right,
            Radius::circular(5.0)
        );
    }

    #[test]
    fn radius_geometry_lerp_is_add_subtract() {
        let a = BorderRadiusGeometry::circular(0.0);
        let b = BorderRadiusGeometry::circular(10.0);
        assert_eq!(
            BorderRadiusGeometry::lerp(a, b, 0.25),
            BorderRadiusGeometry::circular(2.5)
        );
    }

    // -- RRect -------------------------------------------------------------------

    // -- Which end is which ------------------------------------------------
    //
    // A lerp is **symmetric at `t = 0.5`**, so a test that only checks the
    // midpoint cannot tell `lerp(a, b, t)` from `lerp(b, a, t)`. Tick 212
    // wrote exactly that test and the swap stayed green; a screen that
    // swapped every `lerp(a, b, t)` in this file then found **101 of 107**
    // going unnoticed. The whole family's direction was unasserted.
    //
    // These tests run at a quarter of the way along, where the two ends give
    // different answers, and they walk the shapes rather than sampling them:
    // one arm of a match nobody visits is one arm that can be backwards.

    /// A side of a given width, so a lerp of it reads as a number.
    fn wide(width: f32) -> BorderSide {
        BorderSide {
            width,
            ..BorderSide::NONE
        }
    }

    /// The corner radius a lerped shape came out with, where it has one.
    ///
    /// Half the arms in `lerp_from` interpolate a radius as well as a side,
    /// on their own line, and a test that only ever reads the side cannot see
    /// a swap in either of them. A shape whose outline opens forwards while
    /// its corners close backwards is a specific, watchable wrongness.
    fn radius_of(shape: &ShapeBorder) -> Option<f32> {
        let geometry = match shape {
            ShapeBorder::Rounded(shape) => shape.border_radius,
            ShapeBorder::Beveled(shape) => shape.border_radius,
            ShapeBorder::Continuous(shape) => shape.border_radius,
            ShapeBorder::Superellipse(shape) => shape.border_radius,
            ShapeBorder::StadiumToRoundedRect(shape) => shape.border_radius,
            ShapeBorder::RoundedToCircle(shape) => shape.border_radius,
            ShapeBorder::Underline(shape) => {
                return Some(shape.border_radius.top_left.x);
            }
            ShapeBorder::Outline(shape) => {
                return Some(shape.border_radius.top_left.x);
            }
            _ => return None,
        };
        Some(
            geometry
                .resolve(crate::direction::TextDirection::Ltr)
                .top_left
                .x,
        )
    }

    /// Every shape this crate can lerp, each carrying the side and the corner
    /// radius it was given.
    fn shapes_with(side: BorderSide, radius: f32) -> Vec<(&'static str, ShapeBorder)> {
        let flat = BorderRadius::circular(radius);
        let corners = BorderRadiusGeometry::Absolute(flat);
        vec![
            ("circle", ShapeBorder::Circle(CircleBorder::new(side, 0.0))),
            ("oval", ShapeBorder::Oval(OvalBorder::new(side))),
            ("stadium", ShapeBorder::Stadium(StadiumBorder::new(side))),
            (
                "rounded",
                ShapeBorder::Rounded(RoundedRectangleBorder::new(side, corners)),
            ),
            (
                "beveled",
                ShapeBorder::Beveled(BeveledRectangleBorder::new(side, corners)),
            ),
            (
                "continuous",
                ShapeBorder::Continuous(ContinuousRectangleBorder::new(side, corners)),
            ),
            (
                "superellipse",
                ShapeBorder::Superellipse(RoundedSuperellipseBorder::new(side, corners)),
            ),
            (
                "star",
                ShapeBorder::Star(StarBorder::new(side, 5.0, 0.4, 0.0, 0.0, 0.0, 1.0)),
            ),
            (
                "underline",
                ShapeBorder::Underline(UnderlineInputBorder {
                    side,
                    border_radius: flat,
                }),
            ),
            (
                "outline",
                ShapeBorder::Outline(OutlineInputBorder {
                    side,
                    border_radius: flat,
                    gap_padding: 4.0,
                }),
            ),
            // The three shapes that only exist part-way through another
            // lerp. They can be lerped again from there -- an animation
            // interrupted and redirected lands here -- so their arms are as
            // reachable as any other.
            (
                "stadium-to-circle",
                ShapeBorder::StadiumToCircle(StadiumToCircleBorder::new(side, 0.5, 0.0)),
            ),
            (
                "stadium-to-rounded",
                ShapeBorder::StadiumToRoundedRect(StadiumToRoundedRectBorder::new(
                    side, corners, 0.5,
                )),
            ),
            (
                "rounded-to-circle",
                ShapeBorder::RoundedToCircle(RoundedToCircleBorder::new(
                    side, corners, 0.5, 0.0, false,
                )),
            ),
        ]
    }

    /// The same table with square corners, for the tests that are not asking
    /// about them.
    fn shapes(side: BorderSide) -> Vec<(&'static str, ShapeBorder)> {
        shapes_with(side, 0.0)
    }

    #[test]
    fn every_pair_of_shapes_lerps_from_the_first_towards_the_second() {
        // A quarter of the way, the answer has to be on **`a`'s side of the
        // midpoint**, and that is one claim covering the three things a pair
        // can do.
        //
        // * A pair upstream morphs gives 3, a quarter of the way from a
        //   2-wide side to a 6-wide one.
        // * A pair it does not -- a circle and a beveled rectangle, say --
        //   crossfades, and `ShapeBorder::lerp` ends with
        //   `if t < 0.5 { a } else { b }`. That gives 2.
        // * A pair that goes **through** another shape gives something else
        //   again: a stadium becoming a star passes through a circle in two
        //   phases, and a quarter of the way overall is halfway through the
        //   first phase. That gives 2.5.
        //
        // All three are on `a`'s side and all three fail on a swap, which is
        // what this asks. Demanding 3 everywhere would be asserting more than
        // upstream does -- `BeveledRectangleBorder.lerpFrom` only knows
        // another beveled border, and the multi-phase shapes are not moving
        // at a constant rate at all. The exact numbers for the ordinary pairs
        // are pinned in the tests below.
        //
        // The two ends differ in their **corner radius** as well as their
        // side, 4 against 12, because half these arms interpolate a radius on
        // a line of its own. A test that only read the side could not see a
        // swap there, and a shape whose outline opens forwards while its
        // corners close backwards is a specific, watchable wrongness.
        for (from_name, from) in shapes_with(wide(2.0), 4.0) {
            for (to_name, to) in shapes_with(wide(6.0), 12.0) {
                let quarter = ShapeBorder::lerp(Some(from.clone()), Some(to.clone()), 0.25)
                    .unwrap_or_else(|| panic!("{from_name} to {to_name} interpolates"));
                let Some(side) = quarter.outlined_side() else {
                    continue;
                };
                assert!(
                    side.width < 4.0,
                    "{from_name} -> {to_name}: {} is past the midpoint of 2..6",
                    side.width
                );

                // And the same pair the other way round lands on the other
                // side, which is what says the number above is a direction
                // and not a coincidence of the arithmetic.
                let back = ShapeBorder::lerp(Some(to.clone()), Some(from.clone()), 0.25)
                    .unwrap_or_else(|| panic!("{to_name} to {from_name} interpolates"));
                if let Some(back) = back.outlined_side() {
                    assert!(
                        back.width > 4.0,
                        "{to_name} -> {from_name}: {} is past the midpoint",
                        back.width
                    );
                    assert_ne!(
                        side.width, back.width,
                        "{from_name}/{to_name}: the two directions must differ"
                    );
                }

                // And a claim about the corners, stated so that it holds for
                // both kinds of arm.
                //
                // Some arms interpolate the radius and some **carry one end's
                // through**: a circle has no radius, so
                // `RoundedRectangleBorder.lerpFrom(a is CircleBorder)` keeps
                // the rectangle's own and lets `circularity` do the morphing,
                // and `_StadiumToRoundedRectangleBorder` does the same with
                // `borderRadius: borderRadius`. Demanding an interpolation
                // there would be demanding one upstream does not perform.
                //
                // So: **where the two directions disagree at all, the forward
                // one is the smaller** -- 4 to 12 read from the 4 end. An arm
                // that carries a radius through gives the same answer both
                // ways and says nothing; an arm that lerps gives 6 and 10, and
                // a swap gives 10 and 6.
                if let (Some(forward), Some(backward)) = (
                    radius_of(&quarter),
                    ShapeBorder::lerp(Some(to.clone()), Some(from.clone()), 0.25)
                        .as_ref()
                        .and_then(radius_of),
                ) {
                    if forward != backward {
                        assert!(
                            forward < backward,
                            "{from_name} -> {to_name}: corners run backwards                              ({forward} forward, {backward} back)"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn both_halves_of_a_lerp_agree_about_which_way_the_journey_runs() {
        // `ShapeBorder::lerp` asks `b.lerp_from(a)` first and only falls to
        // `a.lerp_to(b)` when that declines, so **every arm `lerp_from`
        // handles makes the mirrored arm in `lerp_to` unreachable through
        // `lerp`**. Half this file's match arms are in the half that never
        // runs for the pairs the other half knows.
        //
        // They are not dead: both are public, and upstream's `lerp` has the
        // same two-step. So they are called directly here -- one journey,
        // asked for in the two ways a caller can ask.
        for (from_name, from) in shapes_with(wide(2.0), 4.0) {
            for (to_name, to) in shapes_with(wide(6.0), 12.0) {
                let asked_of_the_destination = to.lerp_from(Some(&from), 0.25);
                let asked_of_the_start = from.lerp_to(Some(&to), 0.25);
                for (how, result) in [
                    ("lerp_from", asked_of_the_destination),
                    ("lerp_to", asked_of_the_start),
                ] {
                    let Some(result) = result else {
                        continue;
                    };
                    if let Some(side) = result.outlined_side() {
                        assert!(
                            side.width < 4.0,
                            "{from_name} -> {to_name} via {how}: {} is past the midpoint",
                            side.width
                        );
                    }
                }

                // And the corners, by the same rule as above: where the two
                // directions disagree, the forward one is the smaller.
                let forward = to.lerp_from(Some(&from), 0.25).as_ref().and_then(radius_of);
                let backward = from.lerp_from(Some(&to), 0.25).as_ref().and_then(radius_of);
                if let (Some(forward), Some(backward)) = (forward, backward) {
                    if forward != backward {
                        assert!(
                            forward < backward,
                            "{from_name} -> {to_name} via lerp_from: corners run                              backwards ({forward} forward, {backward} back)"
                        );
                    }
                }
                let forward = from.lerp_to(Some(&to), 0.25).as_ref().and_then(radius_of);
                let backward = to.lerp_to(Some(&from), 0.25).as_ref().and_then(radius_of);
                if let (Some(forward), Some(backward)) = (forward, backward) {
                    if forward != backward {
                        assert!(
                            forward < backward,
                            "{from_name} -> {to_name} via lerp_to: corners run                              backwards ({forward} forward, {backward} back)"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn and_a_circle_opening_into_an_oval_morphs_rather_than_fading() {
        // The pair this screen turned up. Upstream needs no arm for it --
        // `OvalBorder extends CircleBorder`, so the circle's own lerp handles
        // it and interpolates the eccentricities, 0 to 1. This crate makes
        // them two variants, and the pair was falling through to the
        // crossfade: the shape snapped instead of opening.
        let circle = ShapeBorder::Circle(CircleBorder::new(wide(2.0), 0.0));
        let oval = ShapeBorder::Oval(OvalBorder::new(wide(6.0)));
        let quarter = ShapeBorder::lerp(Some(circle.clone()), Some(oval.clone()), 0.25)
            .expect("a circle and an oval interpolate");
        assert_eq!(
            quarter.outlined_side().map(|side| side.width),
            Some(3.0),
            "the side is interpolated, so the pair is not being faded"
        );
        match quarter {
            ShapeBorder::Circle(circle) => assert_eq!(
                circle.eccentricity, 0.25,
                "and the eccentricity is what opens it"
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_shape_lerped_with_nothing_still_knows_which_end_it_is() {
        // `lerp(Some(x), None, t)` and `lerp(None, Some(x), t)` are not the
        // same journey: one is fading a shape out and the other is fading it
        // in, and at a quarter of the way they are different shapes.
        for (name, shape) in shapes(wide(4.0)) {
            let out = ShapeBorder::lerp(Some(shape.clone()), None, 0.25);
            let into = ShapeBorder::lerp(None, Some(shape.clone()), 0.25);
            match (out.and_then(|s| s.outlined_side()), into.and_then(|s| s.outlined_side())) {
                (Some(out), Some(into)) => assert_ne!(
                    out.width, into.width,
                    "{name}: fading out and fading in are not the same"
                ),
                (None, None) => {}
                _ => {}
            }
        }
    }

    #[test]
    fn the_rectangular_shapes_lerp_their_radius_as_well_as_their_side() {
        // Two numbers moving together, and each has its own line in the
        // match arm. A shape whose side interpolates and whose corners jump
        // is a specific, visible wrongness -- and the side test above cannot
        // see it, because it only ever reads the side.
        let rounded = |side: f32, radius: f32| {
            ShapeBorder::Rounded(RoundedRectangleBorder::new(
                wide(side),
                BorderRadiusGeometry::Absolute(BorderRadius::circular(radius)),
            ))
        };
        let beveled = |side: f32, radius: f32| {
            ShapeBorder::Beveled(BeveledRectangleBorder::new(
                wide(side),
                BorderRadiusGeometry::Absolute(BorderRadius::circular(radius)),
            ))
        };

        for build in [rounded, beveled] {
            let near = build(1.0, 4.0);
            let far = build(9.0, 12.0);
            let quarter = ShapeBorder::lerp(Some(near.clone()), Some(far.clone()), 0.25)
                .expect("the same shape interpolates with itself");
            let radius_of = |shape: &ShapeBorder| match shape {
                ShapeBorder::Rounded(shape) => shape.border_radius,
                ShapeBorder::Beveled(shape) => shape.border_radius,
                other => panic!("{other:?}"),
            };
            assert_eq!(quarter.outlined_side().map(|side| side.width), Some(3.0));
            assert_eq!(
                radius_of(&quarter)
                    .resolve(crate::direction::TextDirection::Ltr)
                    .top_left
                    .x,
                6.0
            );

            let back = ShapeBorder::lerp(Some(far), Some(near), 0.25).expect("and back");
            assert_eq!(back.outlined_side().map(|side| side.width), Some(7.0));
            assert_eq!(
                radius_of(&back)
                    .resolve(crate::direction::TextDirection::Ltr)
                    .top_left
                    .x,
                10.0
            );
        }
    }

    #[test]
    fn a_box_border_lerps_each_of_its_four_sides_on_its_own_line() {
        // Four independent lines, and a swap in one of them is one edge of a
        // box animating backwards while the other three go forwards. Each
        // side gets a different pair so a line reading the wrong field fails
        // as well.
        let near = Border {
            top: wide(1.0),
            right: wide(2.0),
            bottom: wide(3.0),
            left: wide(4.0),
        };
        let far = Border {
            top: wide(9.0),
            right: wide(10.0),
            bottom: wide(11.0),
            left: wide(12.0),
        };
        let quarter = Border::lerp(Some(near), Some(far), 0.25);
        assert_eq!(
            [
                quarter.top.width,
                quarter.right.width,
                quarter.bottom.width,
                quarter.left.width
            ],
            [3.0, 4.0, 5.0, 6.0]
        );
        let back = Border::lerp(Some(far), Some(near), 0.25);
        assert_eq!(
            [
                back.top.width,
                back.right.width,
                back.bottom.width,
                back.left.width
            ],
            [7.0, 8.0, 9.0, 10.0]
        );
    }

    #[test]
    fn and_a_directional_one_lerps_its_start_and_end_rather_than_left_and_right() {
        // The directional border's whole point: `start` and `end` resolve
        // against the reading direction later, so lerping them into `left`
        // and `right` here would settle a question that is not this
        // function's to settle.
        let near = BorderDirectional {
            top: wide(1.0),
            start: wide(2.0),
            bottom: wide(3.0),
            end: wide(4.0),
        };
        let far = BorderDirectional {
            top: wide(9.0),
            start: wide(10.0),
            bottom: wide(11.0),
            end: wide(12.0),
        };
        let quarter = BorderDirectional::lerp(Some(near), Some(far), 0.25);
        assert_eq!(
            [
                quarter.top.width,
                quarter.start.width,
                quarter.bottom.width,
                quarter.end.width
            ],
            [3.0, 4.0, 5.0, 6.0]
        );
        let back = BorderDirectional::lerp(Some(far), Some(near), 0.25);
        assert_eq!(
            [
                back.top.width,
                back.start.width,
                back.bottom.width,
                back.end.width
            ],
            [7.0, 8.0, 9.0, 10.0]
        );
    }

    #[test]
    fn a_directional_radius_lerps_its_four_corners_by_reading_order() {
        // The same argument as the border above, one type down.
        let near = BorderRadiusDirectional::only(
            Radius::circular(1.0),
            Radius::circular(2.0),
            Radius::circular(3.0),
            Radius::circular(4.0),
        );
        let far = BorderRadiusDirectional::only(
            Radius::circular(9.0),
            Radius::circular(10.0),
            Radius::circular(11.0),
            Radius::circular(12.0),
        );
        let quarter = BorderRadiusDirectional::lerp(near, far, 0.25);
        assert_eq!(
            [
                quarter.top_start.x,
                quarter.top_end.x,
                quarter.bottom_start.x,
                quarter.bottom_end.x
            ],
            [3.0, 4.0, 5.0, 6.0]
        );
        let back = BorderRadiusDirectional::lerp(far, near, 0.25);
        assert_eq!(
            [
                back.top_start.x,
                back.top_end.x,
                back.bottom_start.x,
                back.bottom_end.x
            ],
            [7.0, 8.0, 9.0, 10.0]
        );
    }

    #[test]
    fn the_two_input_borders_lerp_their_side_and_their_radius_the_same_way() {
        // A field's border animating on focus is the most-seen lerp in a
        // Material application, and it is two numbers moving together: the
        // line thickens and the corners open.
        let near_underline = UnderlineInputBorder {
            side: wide(1.0),
            border_radius: BorderRadius::circular(4.0),
        };
        let far_underline = UnderlineInputBorder {
            side: wide(9.0),
            border_radius: BorderRadius::circular(12.0),
        };
        let quarter = UnderlineInputBorder::lerp(&near_underline, &far_underline, 0.25);
        assert_eq!(quarter.side.width, 3.0);
        assert_eq!(quarter.border_radius.top_left.x, 6.0);
        let back = UnderlineInputBorder::lerp(&far_underline, &near_underline, 0.25);
        assert_eq!(back.side.width, 7.0);
        assert_eq!(back.border_radius.top_left.x, 10.0);

        let near_outline = OutlineInputBorder {
            side: wide(1.0),
            border_radius: BorderRadius::circular(4.0),
            gap_padding: 7.0,
        };
        let far_outline = OutlineInputBorder {
            side: wide(9.0),
            border_radius: BorderRadius::circular(12.0),
            gap_padding: 99.0,
        };
        let quarter = OutlineInputBorder::lerp(&near_outline, &far_outline, 0.25);
        assert_eq!(quarter.side.width, 3.0);
        assert_eq!(quarter.border_radius.top_left.x, 6.0);
        assert_eq!(
            quarter.gap_padding, 7.0,
            "the gap padding is taken from `a` and not interpolated, which is \
             upstream's own `gapPadding: a.gapPadding`"
        );
        let back = OutlineInputBorder::lerp(&far_outline, &near_outline, 0.25);
        assert_eq!(back.side.width, 7.0);
        assert_eq!(back.border_radius.top_left.x, 10.0);
        assert_eq!(back.gap_padding, 99.0, "from whichever end is `a`");
    }

    #[test]
    fn a_table_border_lerps_every_one_of_its_six_sides_the_same_way_round() {
        // Six sides and a radius, each lerped on its own line. A swap in one
        // of them is one edge of a table animating backwards while the other
        // five go forwards -- which is exactly the sort of thing that reads as
        // "the animation is a bit odd" and never gets traced.
        //
        // Every field is given a **different** pair, so a line that read the
        // wrong field would fail too.
        let near = TableBorder {
            top: wide(1.0),
            right: wide(2.0),
            bottom: wide(3.0),
            left: wide(4.0),
            horizontal_inside: wide(5.0),
            vertical_inside: wide(6.0),
            border_radius: BorderRadius::circular(8.0),
        };
        let far = TableBorder {
            top: wide(9.0),
            right: wide(10.0),
            bottom: wide(11.0),
            left: wide(12.0),
            horizontal_inside: wide(13.0),
            vertical_inside: wide(14.0),
            border_radius: BorderRadius::circular(16.0),
        };
        // A quarter of the way is two more than where it started, for each.
        let quarter = TableBorder::lerp(Some(&near), Some(&far), 0.25);
        assert_eq!(quarter.top.width, 3.0);
        assert_eq!(quarter.right.width, 4.0);
        assert_eq!(quarter.bottom.width, 5.0);
        assert_eq!(quarter.left.width, 6.0);
        assert_eq!(quarter.horizontal_inside.width, 7.0);
        assert_eq!(quarter.vertical_inside.width, 8.0);
        assert_eq!(
            quarter.border_radius,
            BorderRadius::ZERO,
            "upstream's lerp builds a border with no radius argument at all"
        );

        // And the other way round, two less than where *that* started.
        let back = TableBorder::lerp(Some(&far), Some(&near), 0.25);
        assert_eq!(back.top.width, 7.0);
        assert_eq!(back.right.width, 8.0);
        assert_eq!(back.bottom.width, 9.0);
        assert_eq!(back.left.width, 10.0);
        assert_eq!(back.horizontal_inside.width, 11.0);
        assert_eq!(back.vertical_inside.width, 12.0);
        assert_eq!(back.border_radius, BorderRadius::ZERO, "either way round");
    }

    #[test]
    fn a_linear_border_lerps_its_four_edges_the_same_way_round() {
        // The four edges are four independent lines, and each carries a size
        // and an alignment of its own -- eight numbers that can be backwards
        // one at a time.
        let edge = |size: f32, alignment: f32| Some(LinearBorderEdge::new(size, alignment));
        let near = LinearBorder::new(
            wide(1.0),
            edge(0.1, -1.0),
            edge(0.2, -0.8),
            edge(0.3, -0.6),
            edge(0.4, -0.4),
        );
        let far = LinearBorder::new(
            wide(5.0),
            edge(0.5, 1.0),
            edge(0.6, 0.8),
            edge(0.7, 0.6),
            edge(0.8, 0.4),
        );

        let quarter = LinearBorder::lerp(&near, &far, 0.25);
        assert_eq!(quarter.side.width, 2.0);
        let sizes = |border: &LinearBorder| {
            [
                border.start.map(|edge| edge.size),
                border.end.map(|edge| edge.size),
                border.top.map(|edge| edge.size),
                border.bottom.map(|edge| edge.size),
            ]
        };
        let alignments = |border: &LinearBorder| {
            [
                border.start.map(|edge| edge.alignment),
                border.end.map(|edge| edge.alignment),
                border.top.map(|edge| edge.alignment),
                border.bottom.map(|edge| edge.alignment),
            ]
        };
        assert_eq!(sizes(&quarter), [Some(0.2), Some(0.3), Some(0.4), Some(0.5)]);
        assert_eq!(
            alignments(&quarter),
            [Some(-0.5), Some(-0.4), Some(-0.3), Some(-0.2)]
        );

        let back = LinearBorder::lerp(&far, &near, 0.25);
        assert_eq!(back.side.width, 4.0);
        assert_eq!(sizes(&back), [Some(0.4), Some(0.5), Some(0.6), Some(0.7)]);
        assert_eq!(
            alignments(&back),
            [Some(0.5), Some(0.4), Some(0.3), Some(0.2)]
        );
    }

    #[test]
    fn the_smaller_pieces_lerp_the_way_round_they_say_they_do() {
        // The shapes above are built out of these, and a swap in one of them
        // is invisible to a test that only ever asks about shapes.
        assert_eq!(BorderSide::lerp(wide(2.0), wide(6.0), 0.25).width, 3.0);
        assert_eq!(BorderSide::lerp(wide(6.0), wide(2.0), 0.25).width, 5.0);

        assert_eq!(Radius::lerp(Radius::circular(4.0), Radius::circular(8.0), 0.25).x, 5.0);
        assert_eq!(Radius::lerp(Radius::circular(8.0), Radius::circular(4.0), 0.25).x, 7.0);

        let near = BorderRadius::circular(4.0);
        let far = BorderRadius::circular(8.0);
        assert_eq!(BorderRadius::lerp(near, far, 0.25).top_left.x, 5.0);
        assert_eq!(BorderRadius::lerp(far, near, 0.25).top_left.x, 7.0);
    }

    /// A visible side, so a lerp of it is observable.
    fn thick(width: f32) -> BorderSide {
        BorderSide {
            width,
            ..BorderSide::NONE
        }
    }

    #[test]
    fn two_stadiums_lerp_to_a_stadium_and_only_the_side_moves() {
        // A stadium's radius is its own height, so there is nothing else that
        // could move.
        //
        // **Not at `t = 0.5`**: a lerp is symmetric there, so a test at the
        // midpoint cannot tell `lerp(a, b, t)` from `lerp(b, a, t)` and
        // swapping the two ends stays green. A quarter of the way along says
        // which end is which.
        let from = StadiumBorder::new(thick(2.0));
        let to = StadiumBorder::new(thick(6.0));
        match to.lerp_from(LerpPartner::Stadium(from), 0.25) {
            StadiumLerp::Stadium(result) => {
                assert_eq!(result.side.width, 3.0, "a quarter of the way from 2 to 6")
            }
            other => panic!("{other:?}"),
        }
        match to.lerp_to(LerpPartner::Stadium(from), 0.25) {
            StadiumLerp::Stadium(result) => {
                assert_eq!(result.side.width, 5.0, "and the other way round")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_stadium_meeting_a_circle_becomes_a_third_shape() {
        // Upstream does not interpolate the two outlines; it builds a shape
        // parameterised by how far along it is. Interpolating paths point by
        // point would need the two to have the same points in the same order,
        // which a stadium and a circle do not.
        let stadium = StadiumBorder::new(thick(2.0));
        let circle = CircleBorder {
            side: thick(2.0),
            eccentricity: 0.25,
        };
        let eccentricity = |lerp: StadiumLerp| match lerp {
            StadiumLerp::ToCircle {
                circularity,
                eccentricity,
                ..
            } => (circularity, eccentricity),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            eccentricity(stadium.lerp_to(LerpPartner::Circle(circle), 0.25)),
            (0.25, 0.25),
            "taken from the circle -- a stadium has none of its own"
        );
        // Both directions: whichever operand is the circle is the one asked,
        // and a test of only one leaves the other free to answer zero.
        assert_eq!(
            eccentricity(stadium.lerp_from(LerpPartner::Circle(circle), 0.25)),
            (0.75, 0.25)
        );
    }

    #[test]
    fn the_parameter_counts_from_whichever_end_the_circle_is_at() {
        // `circularity` is `1.0 - t` in `lerpFrom` and `t` in `lerpTo`, and
        // that is not a sign error to tidy away: the parameter always means
        // *how circular*, so only `t` changes direction.
        let stadium = StadiumBorder::new(thick(2.0));
        let circle = CircleBorder {
            side: thick(2.0),
            eccentricity: 0.0,
        };

        let circularity = |lerp: StadiumLerp| match lerp {
            StadiumLerp::ToCircle { circularity, .. } => circularity,
            other => panic!("{other:?}"),
        };

        // Going towards the circle: nothing circular at the start, all of it
        // at the end.
        assert_eq!(circularity(stadium.lerp_to(LerpPartner::Circle(circle), 0.0)), 0.0);
        assert_eq!(circularity(stadium.lerp_to(LerpPartner::Circle(circle), 1.0)), 1.0);

        // Coming from it: the other way round, at the same `t`.
        assert_eq!(circularity(stadium.lerp_from(LerpPartner::Circle(circle), 0.0)), 1.0);
        assert_eq!(circularity(stadium.lerp_from(LerpPartner::Circle(circle), 1.0)), 0.0);

        // And the two agree about the middle, which is what says they are one
        // parameterisation seen from two ends rather than two rules.
        assert_eq!(
            circularity(stadium.lerp_to(LerpPartner::Circle(circle), 0.5)),
            circularity(stadium.lerp_from(LerpPartner::Circle(circle), 0.5))
        );
    }

    #[test]
    fn and_a_rounded_rectangle_is_the_same_story_with_its_own_word() {
        let stadium = StadiumBorder::new(thick(2.0));
        let rounded = RoundedRectangleBorder {
            side: thick(2.0),
            border_radius: BorderRadiusGeometry::Zero,
        };
        let rectilinearity = |lerp: StadiumLerp| match lerp {
            StadiumLerp::ToRoundedRectangle { rectilinearity, .. } => rectilinearity,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            rectilinearity(stadium.lerp_to(LerpPartner::RoundedRectangle(rounded), 0.25)),
            0.25
        );
        assert_eq!(
            rectilinearity(stadium.lerp_from(LerpPartner::RoundedRectangle(rounded), 0.25)),
            0.75
        );
    }

    #[test]
    fn anything_else_is_not_a_shape_a_stadium_knows_how_to_meet() {
        // `super.lerpFrom`, which fades one out and the other in rather than
        // morphing. Saying so is the point: a port that quietly produced a
        // stadium here would animate a shape change that upstream crossfades.
        let stadium = StadiumBorder::new(thick(2.0));
        assert_eq!(
            stadium.lerp_to(LerpPartner::Other, 0.5),
            StadiumLerp::NotSpecial
        );
        assert_eq!(
            stadium.lerp_from(LerpPartner::Other, 0.5),
            StadiumLerp::NotSpecial
        );
    }

    #[test]
    fn a_press_past_the_curve_at_the_end_of_a_pill_misses_it() {
        // The corners are the point of the hit test: testing the bounding
        // rectangle instead would swallow a press meant for whatever is
        // behind the button.
        let stadium = StadiumBorder::default();
        let rect = Rect::ltrb(0.0, 0.0, 100.0, 40.0);
        assert!(stadium.hit_test(rect, Offset::new(50.0, 20.0)), "the middle");
        assert!(stadium.hit_test(rect, Offset::new(1.0, 20.0)), "the left end");
        assert!(
            !stadium.hit_test(rect, Offset::new(1.0, 1.0)),
            "the top-left corner is outside the curve"
        );
        assert!(!stadium.hit_test(rect, Offset::new(99.0, 39.0)));
    }

    #[test]
    fn scaling_moves_the_side_and_nothing_else() {
        // A stadium's radius is its own height. There is nothing else to
        // scale, which is why upstream's `scale` is one line here and several
        // in the shapes that carry a radius.
        let scaled = StadiumBorder::new(thick(4.0)).scale(0.5);
        assert_eq!(scaled.side.width, 2.0);
        assert!(StadiumBorder::default().prefer_paint_interior());
    }

    #[test]
    fn rrect_contains_respects_corners() {
        let rect = Rect::xywh(0.0, 0.0, 100.0, 50.0);
        let rrect = BorderRadius::circular(10.0).to_rrect(rect);
        // The middle of an edge is inside; just outside a rounded corner's
        // arc is not.
        assert!(rrect.contains(Offset::new(50.0, 0.5)));
        assert!(rrect.contains(Offset::new(50.0, 49.5)));
        assert!(!rrect.contains(Offset::new(0.5, 0.5)));
        assert!(!rrect.contains(Offset::new(99.5, 49.5)));
        assert!(rrect.contains(Offset::new(50.0, 25.0)));
        assert!(!rrect.contains(Offset::new(-1.0, 25.0)));
    }

    #[test]
    fn rrect_inflate_grows_radii_with_it() {
        let rect = Rect::xywh(0.0, 0.0, 100.0, 100.0);
        let rrect = BorderRadius::circular(10.0).to_rrect(rect).inflate(5.0);
        assert_eq!(rrect.top_left, Radius::circular(15.0));
        assert_eq!(rrect.rect, Rect::xywh(-5.0, -5.0, 110.0, 110.0));
        assert_eq!(rrect.deflate(5.0).top_left, Radius::circular(10.0));
        // A deflate past the radius clamps the radius at zero.
        assert_eq!(rrect.deflate(20.0).top_left, Radius::circular(0.0));
    }

    #[test]
    fn rrect_scaled_shrinks_overrunning_neighbours() {
        // Left and right radii sum past the width: both shrink by half.
        let rect = Rect::xywh(0.0, 0.0, 20.0, 40.0);
        let rrect =
            BorderRadius::horizontal(Radius::circular(15.0), Radius::circular(15.0)).to_rrect(rect);
        assert_eq!(rrect.scaled()[0], Radius::circular(10.0));
    }

    // -- BorderSide ----------------------------------------------------------------

    #[test]
    fn stroke_inset_outset_and_offset() {
        let mut s = side(RED, 10.0);
        assert_eq!(s.stroke_inset(), 10.0);
        assert_eq!(s.stroke_outset(), 0.0);
        s.stroke_align = STROKE_ALIGN_CENTER;
        assert_eq!(s.stroke_inset(), 5.0);
        assert_eq!(s.stroke_outset(), 5.0);
        assert_eq!(s.stroke_offset(), 0.0);
        s.stroke_align = STROKE_ALIGN_OUTSIDE;
        assert_eq!(s.stroke_inset(), 0.0);
        assert_eq!(s.stroke_outset(), 10.0);
        assert_eq!(s.stroke_offset(), 10.0);
    }

    #[test]
    fn border_side_scale_and_merge() {
        let s = side(RED, 10.0);
        assert_eq!(s.scale(0.5).width, 5.0);
        assert_eq!(s.scale(-1.0).style, BorderStyle::None);
        assert_eq!(s.scale(0.0).style, BorderStyle::None);

        let nil = BorderSide::NONE;
        assert_eq!(BorderSide::merge(nil, nil), nil);
        assert_eq!(BorderSide::merge(nil, s), s);
        assert_eq!(BorderSide::merge(s, nil), s);
        assert_eq!(BorderSide::merge(s, s).width, 20.0);
        assert!(BorderSide::can_merge(s, s));
        assert!(!BorderSide::can_merge(side(RED, 1.0), side(BLUE, 1.0)));
    }

    #[test]
    fn border_side_lerp() {
        let a = side(RED, 10.0);
        let b = side(BLUE, 20.0);
        let mid = BorderSide::lerp(a, b, 0.5);
        assert_eq!(mid.width, 15.0);
        // Red to blue over alpha-preserving channels.
        assert_eq!(mid.color.alpha(), 255);

        // A width that dips below zero yields the nil side.
        let shrinking = BorderSide::lerp(side(RED, 0.0), side(BLUE, -10.0), 0.5);
        assert_eq!(shrinking, BorderSide::NONE);

        // Mismatched styles lerp through zeroed alphas.
        let none_style = BorderSide {
            style: BorderStyle::None,
            ..a
        };
        let mixed = BorderSide::lerp(a, none_style, 0.5);
        assert_eq!(mixed.style, BorderStyle::Solid);
        // 255 to 0 over t=0.5 lands on 127.5, rounding away from zero.
        assert_eq!(mixed.color.alpha(), 128);
    }

    // -- Border / BoxBorder ---------------------------------------------------------

    #[test]
    fn border_dimensions_follow_stroke_insets() {
        let border = Border::new(
            side(RED, 1.0),
            side(RED, 2.0),
            side(RED, 3.0),
            side(RED, 4.0),
        );
        let insets = border.dimensions().resolve(TextDirection::Ltr);
        assert_eq!(
            insets,
            EdgeInsets {
                left: 4.0,
                top: 1.0,
                right: 2.0,
                bottom: 3.0
            }
        );
    }

    #[test]
    fn border_uniformity_and_merge() {
        let uniform = Border::all(RED, 2.0, BorderStyle::Solid, STROKE_ALIGN_INSIDE);
        assert!(uniform.is_uniform());
        let mixed = Border::new(uniform.top, uniform.right, side(BLUE, 2.0), uniform.left);
        assert!(!mixed.color_is_uniform());
        assert!(mixed.width_is_uniform());

        assert_eq!(
            Border::merge(uniform, uniform),
            Border::all(RED, 4.0, BorderStyle::Solid, STROKE_ALIGN_INSIDE)
        );
        let shape = ShapeBorder::Border(uniform).combine(ShapeBorder::Border(uniform));
        assert_eq!(shape, ShapeBorder::Border(Border::merge(uniform, uniform)));
    }

    #[test]
    fn border_lerp_and_scale() {
        let a = Border::all(RED, 10.0, BorderStyle::Solid, STROKE_ALIGN_INSIDE);
        let b = Border::all(BLUE, 20.0, BorderStyle::Solid, STROKE_ALIGN_INSIDE);
        assert_eq!(
            Border::lerp(Some(a), Some(b), 0.5),
            Border::all(
                color_lerp(RED, BLUE, 0.5),
                15.0,
                BorderStyle::Solid,
                STROKE_ALIGN_INSIDE,
            )
        );
        // From nothing: scaled up by t.
        assert_eq!(Border::lerp(None, Some(b), 0.5), b.scale(0.5));
        assert_eq!(Border::lerp(Some(a), None, 0.5), a.scale(0.5));
    }

    #[test]
    fn box_border_lerp_swaps_lateral_sides_at_the_halfway_point() {
        let lateral = Border::new(
            BorderSide::NONE,
            side(RED, 5.0),
            BorderSide::NONE,
            side(RED, 5.0),
        );
        let directional = BorderDirectional::new(
            BorderSide::NONE,
            side(BLUE, 7.0),
            side(BLUE, 7.0),
            BorderSide::NONE,
        );
        // Before the half: still a Border, laterals fading at double speed.
        let before = BoxBorder::lerp(
            Some(BoxBorder::Uniform(lateral)),
            Some(BoxBorder::Directional(directional)),
            0.25,
        );
        match before {
            BoxBorder::Uniform(border) => {
                assert_eq!(border.right.width, 2.5);
                assert_eq!(border.left.width, 2.5);
            }
            _ => panic!("expected a physical border before the halfway point"),
        }
        // After the half: a BorderDirectional, laterals arriving at double
        // speed from nothing.
        let after = BoxBorder::lerp(
            Some(BoxBorder::Uniform(lateral)),
            Some(BoxBorder::Directional(directional)),
            0.75,
        );
        match after {
            BoxBorder::Directional(border) => {
                assert_eq!(border.start.width, 3.5);
                assert_eq!(border.end.width, 3.5);
            }
            _ => panic!("expected a directional border after the halfway point"),
        }
    }

    // -- The shapes -------------------------------------------------------------------

    #[test]
    fn rounded_border_hit_test_and_dimensions() {
        let rect = Rect::xywh(0.0, 0.0, 100.0, 100.0);
        let sharp = ShapeBorder::Rounded(RoundedRectangleBorder::default());
        assert!(sharp.hit_test(rect, Offset::new(0.5, 0.5), TextDirection::Ltr));
        let rounded = ShapeBorder::Rounded(RoundedRectangleBorder::new(
            BorderSide::NONE,
            BorderRadiusGeometry::circular(20.0),
        ));
        assert!(!rounded.hit_test(rect, Offset::new(0.5, 0.5), TextDirection::Ltr));
        assert!(rounded.hit_test(rect, Offset::new(50.0, 0.5), TextDirection::Ltr));

        let outlined = ShapeBorder::Rounded(RoundedRectangleBorder::new(
            side(RED, 10.0),
            BorderRadiusGeometry::circular(20.0),
        ));
        assert_eq!(
            outlined.dimensions().resolve(TextDirection::Ltr),
            EdgeInsets::all(10.0)
        );
    }

    #[test]
    fn circle_border_eccentricity_adjusts_the_rect() {
        let rect = Rect::xywh(0.0, 0.0, 100.0, 50.0);
        // Zero eccentricity: a circle of the shortest side, centred.
        let circle = ShapeBorder::Circle(CircleBorder::new(BorderSide::NONE, 0.0));
        assert!(circle.hit_test(rect, Offset::new(50.0, 25.0), TextDirection::Ltr));
        assert!(!circle.hit_test(rect, Offset::new(95.0, 25.0), TextDirection::Ltr));
        // Full eccentricity: an oval touching every edge.
        let oval = ShapeBorder::Circle(CircleBorder::new(BorderSide::NONE, 1.0));
        assert!(oval.hit_test(rect, Offset::new(95.0, 25.0), TextDirection::Ltr));
        assert!(!oval.hit_test(rect, Offset::new(95.0, 5.0), TextDirection::Ltr));
    }

    #[test]
    fn stadium_border_hit_test() {
        let rect = Rect::xywh(0.0, 0.0, 100.0, 40.0);
        let stadium = ShapeBorder::Stadium(StadiumBorder::default());
        assert!(stadium.hit_test(rect, Offset::new(50.0, 0.5), TextDirection::Ltr));
        assert!(!stadium.hit_test(rect, Offset::new(0.5, 0.5), TextDirection::Ltr));
        assert!(stadium.hit_test(rect, Offset::new(0.5, 20.0), TextDirection::Ltr));
    }

    #[test]
    fn beveled_border_cuts_corners_with_straight_lines() {
        let rect = Rect::xywh(0.0, 0.0, 100.0, 100.0);
        let diamond = ShapeBorder::Beveled(BeveledRectangleBorder::new(
            BorderSide::NONE,
            BorderRadiusGeometry::all(Radius::circular(100.0)),
        ));
        // Radii past the half-way point meet at the centres: a diamond.
        assert!(diamond.hit_test(rect, Offset::new(50.0, 50.0), TextDirection::Ltr));
        assert!(diamond.hit_test(rect, Offset::new(70.0, 70.0), TextDirection::Ltr));
        assert!(!diamond.hit_test(rect, Offset::new(90.0, 90.0), TextDirection::Ltr));
        assert!(!diamond.hit_test(rect, Offset::new(10.0, 10.0), TextDirection::Ltr));
    }

    #[test]
    fn continuous_border_clamps_radii_to_the_shortest_side() {
        let rect = Rect::xywh(0.0, 0.0, 100.0, 50.0);
        let continuous = ShapeBorder::Continuous(ContinuousRectangleBorder::new(
            BorderSide::NONE,
            BorderRadiusGeometry::circular(80.0),
        ));
        // The radius clamps to 50: the middle of the top edge is inside,
        // the extreme corners are not.
        assert!(continuous.hit_test(rect, Offset::new(50.0, 0.5), TextDirection::Ltr));
        assert!(!continuous.hit_test(rect, Offset::new(0.5, 0.5), TextDirection::Ltr));
    }

    #[test]
    fn oval_border_is_the_full_eccentric_circle() {
        let rect = Rect::xywh(0.0, 0.0, 100.0, 50.0);
        let oval = ShapeBorder::Oval(OvalBorder::default());
        assert!(oval.hit_test(rect, Offset::new(95.0, 25.0), TextDirection::Ltr));
        assert!(!oval.hit_test(rect, Offset::new(95.0, 5.0), TextDirection::Ltr));
    }

    // -- ShapeBorder lerp ----------------------------------------------------------

    #[test]
    fn shape_border_lerp_between_like_shapes() {
        let a = ShapeBorder::Rounded(RoundedRectangleBorder::new(
            side(RED, 1.0),
            BorderRadiusGeometry::circular(0.0),
        ));
        let b = ShapeBorder::Rounded(RoundedRectangleBorder::new(
            side(BLUE, 3.0),
            BorderRadiusGeometry::circular(10.0),
        ));
        let mid = ShapeBorder::lerp(Some(a), Some(b), 0.5).unwrap();
        assert_eq!(
            mid,
            ShapeBorder::Rounded(RoundedRectangleBorder::new(
                side(color_lerp(RED, BLUE, 0.5), 2.0),
                BorderRadiusGeometry::circular(5.0),
            ))
        );
    }

    #[test]
    fn shape_border_lerp_between_stadium_and_circle_goes_through_the_transition() {
        let stadium = ShapeBorder::Stadium(StadiumBorder::new(side(RED, 1.0)));
        let circle = ShapeBorder::Circle(CircleBorder::new(side(BLUE, 1.0), 0.0));
        let mid = ShapeBorder::lerp(Some(stadium.clone()), Some(circle.clone()), 0.5).unwrap();
        match mid {
            ShapeBorder::StadiumToCircle(transition) => {
                assert!((transition.circularity - 0.5).abs() < 1e-6);
            }
            other => panic!("expected a stadium-to-circle transition, got {other:?}"),
        }
        // Back the other way, from the circle's side.
        let back = ShapeBorder::lerp(Some(circle), Some(stadium), 0.5).unwrap();
        match back {
            ShapeBorder::StadiumToCircle(transition) => {
                assert!((transition.circularity - 0.5).abs() < 1e-6);
            }
            other => panic!("expected a stadium-to-circle transition, got {other:?}"),
        }
    }

    #[test]
    fn shape_border_lerp_between_rounded_and_stadium_uses_target_radius() {
        let rounded = ShapeBorder::Rounded(RoundedRectangleBorder::new(
            side(RED, 1.0),
            BorderRadiusGeometry::circular(4.0),
        ));
        let stadium = ShapeBorder::Stadium(StadiumBorder::new(side(BLUE, 1.0)));
        let mid = ShapeBorder::lerp(Some(rounded.clone()), Some(stadium), 0.5).unwrap();
        match mid {
            ShapeBorder::StadiumToRoundedRect(transition) => {
                assert!((transition.rectilinearity - 0.5).abs() < 1e-6);
                assert_eq!(
                    transition.border_radius,
                    BorderRadiusGeometry::circular(4.0)
                );
            }
            other => panic!("expected a stadium-to-rounded transition, got {other:?}"),
        }
    }

    #[test]
    fn shape_border_lerp_from_nothing_scales() {
        let shape = ShapeBorder::Rounded(RoundedRectangleBorder::new(
            side(RED, 8.0),
            BorderRadiusGeometry::circular(4.0),
        ));
        let half = ShapeBorder::lerp(None, Some(shape.clone()), 0.5).unwrap();
        assert_eq!(half, shape.scale(0.5));
    }

    #[test]
    fn shape_border_lerp_of_incompatible_shapes_switches_at_the_half() {
        let a = ShapeBorder::Stadium(StadiumBorder::new(side(RED, 1.0)));
        let b = ShapeBorder::Beveled(BeveledRectangleBorder::new(
            side(BLUE, 1.0),
            BorderRadiusGeometry::circular(4.0),
        ));
        assert_eq!(
            ShapeBorder::lerp(Some(a.clone()), Some(b.clone()), 0.25).unwrap(),
            a
        );
        assert_eq!(
            ShapeBorder::lerp(Some(a), Some(b.clone()), 0.75).unwrap(),
            b
        );
    }

    #[test]
    fn compound_add_merges_compatible_borders_at_the_edge() {
        let inner = ShapeBorder::Border(Border::all(
            RED,
            2.0,
            BorderStyle::Solid,
            STROKE_ALIGN_INSIDE,
        ));
        let outer = ShapeBorder::Border(Border::all(
            BLUE,
            3.0,
            BorderStyle::Solid,
            STROKE_ALIGN_INSIDE,
        ));
        // Colours differ, so no merge: a compound painting the right operand
        // outside.
        let compound = inner.clone().combine(outer.clone());
        match &compound {
            ShapeBorder::Compound(borders) => {
                assert_eq!(borders.len(), 2);
                assert_eq!(borders[0], outer);
                assert_eq!(borders[1], inner);
            }
            other => panic!("expected a compound, got {other:?}"),
        }
        // Same colour: they merge into one border.
        let same = ShapeBorder::Border(Border::all(
            RED,
            2.0,
            BorderStyle::Solid,
            STROKE_ALIGN_INSIDE,
        ));
        assert_eq!(
            inner.clone().combine(same.clone()),
            ShapeBorder::Border(Border::merge(
                match inner {
                    ShapeBorder::Border(b) => b,
                    _ => unreachable!(),
                },
                match same {
                    ShapeBorder::Border(b) => b,
                    _ => unreachable!(),
                },
            ))
        );
    }

    #[test]
    fn compound_dimensions_fold() {
        let compound = ShapeBorder::Compound(vec![
            ShapeBorder::Rounded(RoundedRectangleBorder::new(
                side(RED, 2.0),
                BorderRadiusGeometry::Zero,
            )),
            ShapeBorder::Rounded(RoundedRectangleBorder::new(
                side(RED, 3.0),
                BorderRadiusGeometry::Zero,
            )),
        ]);
        assert_eq!(
            compound.dimensions().resolve(TextDirection::Ltr),
            EdgeInsets::all(5.0)
        );
    }

    // -- ShapeDecoration --------------------------------------------------------------

    #[test]
    fn shape_decoration_padding_is_the_shape_dimensions() {
        let decoration = ShapeDecoration::new(ShapeBorder::Rounded(RoundedRectangleBorder::new(
            side(RED, 6.0),
            BorderRadiusGeometry::circular(8.0),
        )));
        assert_eq!(
            decoration.padding().resolve(TextDirection::Ltr),
            EdgeInsets::all(6.0)
        );
    }

    #[test]
    fn shape_decoration_hit_test_follows_the_shape() {
        let decoration = ShapeDecoration::new(ShapeBorder::Circle(CircleBorder::default()));
        assert!(decoration.hit_test((100.0, 100.0), Offset::new(50.0, 50.0), TextDirection::Ltr));
        assert!(!decoration.hit_test((100.0, 100.0), Offset::new(5.0, 5.0), TextDirection::Ltr));
    }

    #[test]
    fn shape_decoration_lerp_morphs_the_shape_and_fades_the_fill() {
        let a = ShapeDecoration::new(ShapeBorder::Stadium(StadiumBorder::new(side(RED, 1.0))))
            .with_fill(Fill::Solid(RED));
        let b = ShapeDecoration::new(ShapeBorder::Circle(CircleBorder::new(side(BLUE, 1.0), 0.0)))
            .with_fill(Fill::Solid(BLUE));
        let mid = ShapeDecoration::lerp(Some(&a), Some(&b), 0.5).unwrap();
        assert_eq!(mid.fill, Some(Fill::Solid(color_lerp(RED, BLUE, 0.5))));
        assert!(matches!(mid.shape, ShapeBorder::StadiumToCircle(_)));
    }

    // -- Notched -------------------------------------------------------------------------

    #[test]
    fn notched_rectangle_without_a_guest_is_the_host() {
        let host = Rect::xywh(0.0, 0.0, 300.0, 80.0);
        let notched = NotchedShape::Circular { inverted: false };
        // Building the path is the contract here; the stub engine records
        // nothing to compare against, but a non-overlapping guest must not
        // turn into a panic or a degenerate path.
        let _ = notched.outer_path(host, Some(Rect::xywh(0.0, -200.0, 40.0, 40.0)));
        let _ = notched.outer_path(host, None);
    }

    #[test]
    fn automatic_notched_shape_uses_the_host_until_path_ops_land() {
        let host = Rect::xywh(0.0, 0.0, 300.0, 80.0);
        let shape = NotchedShape::Automatic {
            host: ShapeBorder::Rounded(RoundedRectangleBorder::new(
                BorderSide::NONE,
                BorderRadiusGeometry::circular(12.0),
            )),
            guest: Some(ShapeBorder::Circle(CircleBorder::default())),
        };
        let _ = shape.outer_path(host, Some(Rect::xywh(130.0, -20.0, 40.0, 40.0)));
    }

    // -- LinearBorder -----------------------------------------------------------

    #[test]
    fn linear_border_edge_lerp_adopts_the_present_alignment() {
        let present = LinearBorderEdge::new(0.8, -1.0);
        // From nothing: the alignment comes from the side that exists.
        assert_eq!(
            LinearBorderEdge::lerp(None, Some(present), 0.5),
            Some(LinearBorderEdge::new(0.4, -1.0))
        );
        // To nothing: shrinks back the same way.
        assert_eq!(
            LinearBorderEdge::lerp(Some(present), None, 0.5),
            Some(LinearBorderEdge::new(0.4, -1.0))
        );
        // Both present: both fields interpolate.
        assert_eq!(
            LinearBorderEdge::lerp(Some(LinearBorderEdge::new(0.0, 1.0)), Some(present), 0.5),
            Some(LinearBorderEdge::new(0.4, 0.0))
        );
    }

    #[test]
    fn linear_border_dimensions_count_only_present_edges() {
        let underlined = LinearBorder::bottom_edge(side(RED, 2.0), 0.0, 1.0);
        let insets = underlined.dimensions().resolve(TextDirection::Ltr);
        assert_eq!(
            insets,
            EdgeInsets {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 2.0
            }
        );

        let sided = LinearBorder::start_edge(side(RED, 3.0), 0.0, 1.0);
        assert_eq!(sided.dimensions().resolve(TextDirection::Ltr).left, 3.0);
        // Start is the right edge in right-to-left text.
        assert_eq!(sided.dimensions().resolve(TextDirection::Rtl).right, 3.0);
    }

    #[test]
    fn linear_border_hit_test_is_the_whole_rectangle() {
        let rect = Rect::xywh(0.0, 0.0, 100.0, 100.0);
        let shape = ShapeBorder::Linear(LinearBorder::bottom_edge(BorderSide::NONE, 0.0, 1.0));
        assert!(shape.hit_test(rect, Offset::new(0.0, 0.0), TextDirection::Ltr));
    }

    #[test]
    fn linear_border_lerp_grows_edges_from_nothing() {
        let a = LinearBorder::default();
        let b = LinearBorder::new(
            side(RED, 2.0),
            None,
            None,
            Some(LinearBorderEdge::new(1.0, 0.0)),
            Some(LinearBorderEdge::new(0.5, -1.0)),
        );
        let mid = LinearBorder::lerp(&a, &b, 0.5);
        assert_eq!(mid.top.map(|edge| edge.size), Some(0.5));
        assert_eq!(
            mid.bottom.map(|edge| (edge.size, edge.alignment)),
            Some((0.25, -1.0))
        );
    }

    // -- StarBorder ----------------------------------------------------------------

    #[test]
    fn star_border_polygon_touches_its_circumcircle_at_the_points() {
        let rect = Rect::xywh(0.0, 0.0, 100.0, 100.0);
        // A five-sided polygon: the centre is inside; a corner of the
        // bounding box, between two vertices, is not.
        let shape = ShapeBorder::Star(StarBorder::polygon(BorderSide::NONE, 5.0, 0.0, 0.0, 0.0));
        assert!(shape.hit_test(rect, Offset::new(50.0, 50.0), TextDirection::Ltr));
        assert!(!shape.hit_test(rect, Offset::new(1.0, 1.0), TextDirection::Ltr));
        // A vertex sits at the top, dead centre horizontally.
        assert!(shape.hit_test(rect, Offset::new(50.0, 2.0), TextDirection::Ltr));
    }

    #[test]
    fn star_border_star_rejects_the_valleys() {
        let rect = Rect::xywh(0.0, 0.0, 100.0, 100.0);
        let shape = ShapeBorder::Star(StarBorder::new(
            BorderSide::NONE,
            5.0,
            0.4,
            0.0,
            0.0,
            0.0,
            0.0,
        ));
        assert!(shape.hit_test(rect, Offset::new(50.0, 50.0), TextDirection::Ltr));
        // Straight up from centre is a point; straight out to the side is a
        // valley -- inner radius 0.4 keeps it inside.
        assert!(shape.hit_test(rect, Offset::new(50.0, 3.0), TextDirection::Ltr));
        assert!(!shape.hit_test(rect, Offset::new(97.0, 50.0), TextDirection::Ltr));
    }

    #[test]
    fn star_border_paths_generate_without_panicking() {
        let rect = Rect::xywh(10.0, 20.0, 120.0, 80.0);
        let cases = [
            StarBorder::polygon(BorderSide::NONE, 3.0, 0.0, 0.0, 0.0),
            StarBorder::polygon(BorderSide::NONE, 6.0, 0.5, 30.0, 0.0),
            StarBorder::new(BorderSide::NONE, 5.0, 0.4, 0.2, 0.1, 15.0, 0.5),
            // A fractional point count closes the shape with a short arm.
            StarBorder::new(BorderSide::NONE, 4.5, 0.4, 0.0, 0.0, 0.0, 0.0),
        ];
        for star in cases {
            let shape = ShapeBorder::Star(star);
            let _ = shape.outer_path(rect, TextDirection::Ltr);
            let _ = shape.inner_path(rect, TextDirection::Ltr);
        }
    }

    #[test]
    fn star_border_lerp_between_stars_and_from_a_circle() {
        let a = ShapeBorder::Star(StarBorder::new(
            side(RED, 1.0),
            4.0,
            0.4,
            0.0,
            0.0,
            0.0,
            0.0,
        ));
        let b = ShapeBorder::Star(StarBorder::new(
            side(BLUE, 1.0),
            8.0,
            0.6,
            0.1,
            0.0,
            0.0,
            0.0,
        ));
        let mid = ShapeBorder::lerp(Some(a), Some(b), 0.5).unwrap();
        match mid {
            ShapeBorder::Star(star) => {
                assert!((star.points - 6.0).abs() < 1e-6);
                assert!((star.inner_radius_ratio() - 0.5).abs() < 1e-6);
            }
            other => panic!("expected a star, got {other:?}"),
        }

        // A circle becoming a five-point star snaps the point count in from
        // its nearest whole and grows the point rounding.
        let circle = ShapeBorder::Circle(CircleBorder::new(side(RED, 1.0), 0.0));
        let star = ShapeBorder::Star(StarBorder::new(
            side(BLUE, 1.0),
            5.0,
            0.4,
            0.0,
            0.0,
            0.0,
            0.0,
        ));
        let mid = ShapeBorder::lerp(Some(circle), Some(star), 0.5).unwrap();
        match mid {
            ShapeBorder::Star(star) => {
                assert!((star.points - 5.0).abs() < 1e-6);
                assert!((star.point_rounding - 0.5).abs() < 1e-6);
                assert!((star.valley_rounding - 0.0).abs() < 1e-6);
            }
            other => panic!("expected a star, got {other:?}"),
        }
    }

    #[test]
    fn star_border_lerps_through_a_circle_to_a_stadium() {
        let star = ShapeBorder::Star(StarBorder::new(
            side(RED, 1.0),
            5.0,
            0.4,
            0.0,
            0.0,
            0.0,
            0.0,
        ));
        let stadium = ShapeBorder::Stadium(StadiumBorder::new(side(BLUE, 1.0)));
        // Early in the walk it is still star-ish; late, stadium-ish -- at
        // every step it is one interpolable shape, never a hard switch.
        for (t, expect_star) in [(0.1, true), (0.9, false)] {
            let shape = ShapeBorder::lerp(Some(star.clone()), Some(stadium.clone()), t).unwrap();
            assert_eq!(
                matches!(shape, ShapeBorder::Star(_)),
                expect_star,
                "at t={t}"
            );
        }
    }
}

#[cfg(test)]
mod table_border_tests {
    use super::*;

    const RED: Color = Color(0xFF0000FF);
    const BLUE: Color = Color(0xFFFF0000);

    #[test]
    fn dimensions_come_from_the_outer_sides() {
        let border = TableBorder::only(
            side(RED, 1.0),
            side(RED, 2.0),
            side(RED, 3.0),
            side(RED, 4.0),
            BorderSide::NONE,
            BorderSide::NONE,
        );
        assert_eq!(
            border.dimensions().resolve(TextDirection::Ltr),
            EdgeInsets {
                left: 4.0,
                top: 1.0,
                right: 2.0,
                bottom: 3.0
            }
        );
    }

    fn side(color: Color, width: f32) -> BorderSide {
        BorderSide {
            color,
            width,
            ..BorderSide::default()
        }
    }

    #[test]
    fn uniformity_reads_all_six() {
        let uniform = TableBorder::all(side(RED, 1.0));
        assert!(uniform.is_uniform());
        let one_off = TableBorder {
            vertical_inside: side(BLUE, 1.0),
            ..uniform
        };
        assert!(!one_off.is_uniform());
    }

    #[test]
    fn scale_shrinks_every_side() {
        let border = TableBorder::all(side(RED, 4.0));
        assert_eq!(border.scale(0.5).top.width, 2.0);
        assert_eq!(border.scale(0.0).top.style, BorderStyle::None);
    }

    #[test]
    fn lerp_interpolates_six_sides() {
        let a = TableBorder::all(side(RED, 2.0));
        let b = TableBorder::all(side(BLUE, 6.0));
        let mid = TableBorder::lerp(Some(&a), Some(&b), 0.5);
        assert_eq!(mid.top.width, 4.0);
        assert_eq!(mid.top.color, color_lerp(RED, BLUE, 0.5));
        // From nothing: scaled in.
        assert_eq!(TableBorder::lerp(None, Some(&b), 0.5).bottom.width, 3.0);
    }

    #[test]
    fn painting_the_grid_does_not_panic() {
        let border = TableBorder::all(side(RED, 1.0));
        let mut canvas = Canvas::new(200.0, 100.0);
        border.paint(
            &mut canvas,
            Rect::xywh(0.0, 0.0, 200.0, 100.0),
            &[30.0, 60.0],
            &[100.0],
        );
        // Rounded outer border, same contract.
        let rounded = TableBorder {
            border_radius: BorderRadius::circular(8.0),
            ..border
        };
        rounded.paint(&mut canvas, Rect::xywh(0.0, 0.0, 200.0, 100.0), &[], &[]);
    }

    #[test]
    fn an_underline_border_insets_only_its_bottom() {
        let side = BorderSide {
            color: Color::BLACK,
            width: 2.0,
            ..BorderSide::NONE
        };
        let shape = ShapeBorder::Underline(UnderlineInputBorder::new(side));
        let insets = shape
            .dimensions()
            .resolve(crate::direction::TextDirection::Ltr);
        assert_eq!(insets.bottom, 2.0);
        assert_eq!(insets.top, 0.0);
        assert_eq!(insets.left, 0.0);

        // The inner path is the box less the rule, which is where the fill
        // stops -- upstream's `getInnerPath`.
        let border = UnderlineInputBorder::new(side);
        let inner = border.inner_rect(Rect::ltrb(0.0, 0.0, 100.0, 40.0));
        assert_eq!(inner.bottom, 38.0);
        assert_eq!(inner.right, 100.0);
    }

    #[test]
    fn an_outline_border_insets_all_four_and_keeps_its_gap_padding_across_a_lerp() {
        let thin = OutlineInputBorder::new(BorderSide {
            color: Color::BLACK,
            width: 1.0,
            ..BorderSide::NONE
        })
        .with_gap_padding(6.0);
        let thick = OutlineInputBorder::new(BorderSide {
            color: Color::BLACK,
            width: 5.0,
            ..BorderSide::NONE
        })
        .with_gap_padding(6.0);

        let half = OutlineInputBorder::lerp(&thin, &thick, 0.5);
        assert_eq!(half.side.width, 3.0);
        // Upstream asserts the two paddings are equal and keeps the first:
        // a gap that changed width mid-animation would slide the label's
        // clearance out from under it.
        assert_eq!(half.gap_padding, 6.0);

        let shape = ShapeBorder::Outline(thin);
        let insets = shape
            .dimensions()
            .resolve(crate::direction::TextDirection::Ltr);
        assert_eq!(insets.top, insets.bottom);
        assert_eq!(insets.left, insets.right);
    }

    #[test]
    fn a_gapped_outline_is_an_open_path_and_a_gapless_one_is_closed() {
        let border = OutlineInputBorder::new(BorderSide {
            color: Color::BLACK,
            width: 1.0,
            ..BorderSide::NONE
        });
        let rect = Rect::ltrb(0.0, 0.0, 200.0, 56.0);
        let mut canvas = Canvas::new(200.0, 100.0);

        // No gap: the plain rounded rectangle.
        border.paint_with_gap(&mut canvas, rect, None, 0.0, 0.0);
        // A gap that has not opened yet is the same.
        border.paint_with_gap(&mut canvas, rect, Some(40.0), 60.0, 0.0);
        // And an open one draws the walked path instead. Both reach the
        // canvas without panicking, which is what a geometry with a
        // percentage in it has to be checked for at the ends of its range.
        border.paint_with_gap(&mut canvas, rect, Some(40.0), 60.0, 1.0);
        border.paint_with_gap(&mut canvas, rect, Some(40.0), 60.0, 0.5);
    }
}

#[cfg(test)]
/// # What these cannot see
///
/// A path records as its bounding box (see `StubPath` in the stubs), so these
/// tests pin **where** each side was drawn and how deep it reaches. They
/// cannot see the mitre: `paintBorder` pulls each band's inner edge in by the
/// neighbouring sides' widths so the corners meet cleanly, and that point is
/// interior to the bounds. Replacing `l + left.width` with `l` in the top
/// side's inner edge leaves every assertion below green -- checked, not
/// assumed.
///
/// Said here rather than left for a reader to discover, because a test that
/// looks like it covers a shape and covers only its extent is worse than no
/// test at the same place.
mod paint_border_geometry_tests {
    use super::{BorderSide, BorderStyle, paint_border};
    use crate::engine::{Color, LayerTree, Rect};
    use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
    use crate::render::{PaintContext, Size};

    const RED: Color = Color(0xffff0000);
    const BLUE: Color = Color(0xff0000ff);

    fn side(width: f32, colour: Color) -> BorderSide {
        BorderSide {
            color: colour,
            width,
            style: BorderStyle::Solid,
            stroke_align: super::STROKE_ALIGN_INSIDE,
        }
    }

    /// Paints a border round a 100x40 box at the origin and returns what the
    /// canvas was told.
    fn painted(
        top: BorderSide,
        right: BorderSide,
        bottom: BorderSide,
        left: BorderSide,
    ) -> Vec<Drawn> {
        let mut layers = LayerTree::new(200, 200);
        reset_drawn();
        {
            let mut context = PaintContext::new(&mut layers, Size::new(200.0, 200.0));
            paint_border(
                context.canvas(),
                Rect::ltrb(0.0, 0.0, 100.0, 40.0),
                top,
                right,
                bottom,
                left,
            );
        }
        drawn()
    }

    /// A border side, as the canvas was told it.
    ///
    /// **A filled band, not a stroke** -- upstream's `paintBorder` builds a
    /// four-cornered path from the outer edge to the inner one and fills it,
    /// and this port does the same. The paint was not recorded until now, so
    /// a side drawn as a stroke of its own width would have occupied the same
    /// box, in the same colour, and passed.
    fn path(left: f32, top: f32, right: f32, bottom: f32, colour: Color) -> Drawn {
        Drawn::Path {
            left,
            top,
            right,
            bottom,
            argb: colour.0,
            stroke: None,
        }
    }

    /// The other branch: a side of **zero** width is a stroke of zero width,
    /// which is upstream's hairline -- the thinnest line the device can draw
    /// rather than nothing at all. A fill of an empty quadrilateral would be
    /// nothing, which is why the branch exists.
    fn hairline(left: f32, top: f32, right: f32, bottom: f32, colour: Color) -> Drawn {
        Drawn::Path {
            left,
            top,
            right,
            bottom,
            argb: colour.0,
            stroke: Some(0.0),
        }
    }

    #[test]
    fn each_side_is_a_band_the_full_length_of_its_edge() {
        // Four paths, one per side, each spanning its own edge and reaching
        // inwards by its own width.
        let calls = painted(
            side(4.0, RED),
            side(4.0, RED),
            side(4.0, RED),
            side(4.0, RED),
        );
        assert_eq!(
            calls,
            vec![
                path(0.0, 0.0, 100.0, 4.0, RED),
                path(96.0, 0.0, 100.0, 40.0, RED),
                path(0.0, 36.0, 100.0, 40.0, RED),
                path(0.0, 0.0, 4.0, 40.0, RED),
            ]
        );
    }

    #[test]
    fn a_thicker_side_reaches_further_in_and_the_others_do_not() {
        // The band's depth is its own width. Nothing else about the box
        // changes because one side got thicker.
        let calls = painted(
            side(10.0, RED),
            side(2.0, BLUE),
            side(2.0, BLUE),
            side(2.0, BLUE),
        );
        assert_eq!(calls[0], path(0.0, 0.0, 100.0, 10.0, RED), "ten deep");
        assert_eq!(calls[3], path(0.0, 0.0, 2.0, 40.0, BLUE), "still two wide");
    }

    #[test]
    fn a_side_styled_none_is_not_drawn_and_the_others_still_are() {
        let calls = painted(
            BorderSide::NONE,
            side(4.0, RED),
            side(4.0, RED),
            side(4.0, RED),
        );
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert_eq!(
            calls[0],
            path(96.0, 0.0, 100.0, 40.0, RED),
            "the right side, first"
        );
    }

    #[test]
    fn a_border_of_no_sides_draws_nothing() {
        assert_eq!(
            painted(
                BorderSide::NONE,
                BorderSide::NONE,
                BorderSide::NONE,
                BorderSide::NONE
            ),
            vec![]
        );
    }

    #[test]
    fn a_side_of_zero_width_is_a_line_rather_than_a_band() {
        // Upstream keeps it: a zero-width side is a hairline stroke, and the
        // path is the outer edge alone with no inner points to close a band
        // with. So it records as a band of no depth rather than as nothing.
        let calls = painted(
            side(0.0, RED),
            BorderSide::NONE,
            BorderSide::NONE,
            BorderSide::NONE,
        );
        assert_eq!(calls, vec![hairline(0.0, 0.0, 100.0, 0.0, RED)]);
        // Said twice on purpose: the box is the same either way -- a band of
        // zero depth and a hairline both have `bottom == top` -- so the paint
        // is the only thing that separates "the thinnest line the device can
        // draw" from "nothing at all".
        assert_ne!(
            calls,
            vec![path(0.0, 0.0, 100.0, 0.0, RED)],
            "a fill of an empty quadrilateral would be invisible"
        );

    }

    #[test]
    fn the_sides_are_drawn_top_right_bottom_left() {
        // The order is a fact about overlap at the corners, where the later
        // side is drawn over the earlier one.
        let calls = painted(
            side(4.0, RED),
            side(4.0, BLUE),
            side(4.0, RED),
            side(4.0, BLUE),
        );
        let colours: Vec<u32> = calls
            .iter()
            .map(|call| match call {
                Drawn::Path { argb, .. } => *argb,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(colours, vec![RED.0, BLUE.0, RED.0, BLUE.0]);
    }
}
