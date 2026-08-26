// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Paths, gradients, images, and the canvas state stack.
//!
//! Upstream this is the drawing half of `dart:ui` -- `Path`, `Gradient`,
//! `Image`, and the transform/clip/save methods on `Canvas`. The engine objects
//! underneath are the same ones; only the way the arguments arrive changes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::os::raw::c_int;
use std::rc::Rc;

use crate::engine::{Canvas, Color, LayerTree, Paint, Paragraph, Rect, TextAlign, TextStyle, sys};

// -- Enums --------------------------------------------------------------------

/// What a shader does outside the geometry it was defined for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TileMode {
    /// Extend the edge colour. The usual choice for a gradient that fills its
    /// own bounds exactly.
    #[default]
    Clamp,
    Repeat,
    Mirror,
    /// Draw nothing outside the geometry.
    Decal,
}

impl TileMode {
    /// Every value, so a test can walk the table rather than sample it.
    pub const ALL: [TileMode; 4] = [
        TileMode::Clamp,
        TileMode::Repeat,
        TileMode::Mirror,
        TileMode::Decal,
    ];

    /// The number the engine reads, and **nothing on this side reads it**:
    /// `ToTileMode` in `rustflutter_ffi_draw.cc` is the other half, and the
    /// two are hand-written mirrors of one ABI. A row that took its
    /// neighbour's number would tile a gradient the wrong way with nothing
    /// here to notice, which is what `variant_sweep` found for three of these
    /// four.
    pub(crate) fn code(self) -> c_int {
        match self {
            TileMode::Clamp => 0,
            TileMode::Repeat => 1,
            TileMode::Mirror => 2,
            TileMode::Decal => 3,
        }
    }
}

/// How source and destination colours are combined. The discriminants match
/// `flutter::DlBlendMode`, which in turn matches `dart:ui`'s `BlendMode`.
///
/// # Separable, and the four that are not
///
/// `Multiply` is upstream's last *separable* mode -- one that works on each
/// colour channel independently -- and its own comment says so. The four after
/// it (`Hue`, `Saturation`, `Color`, `Luminosity`) take the whole colour at
/// once, which is why they come last and why a port stops there without
/// meaning to.
///
/// This one did, and the doc line above still claimed the discriminants
/// matched. They do, and did: what was missing was the tail. It is here now,
/// because a `static_cast` in `rf_paint_set_blend_mode` is all that stands
/// between these numbers and the engine, and a mode above `kLastMode` is
/// **silently dropped** by the guard there rather than reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum BlendMode {
    Clear = 0,
    Src = 1,
    Dst = 2,
    #[default]
    SrcOver = 3,
    DstOver = 4,
    SrcIn = 5,
    DstIn = 6,
    SrcOut = 7,
    DstOut = 8,
    SrcATop = 9,
    DstATop = 10,
    Xor = 11,
    Plus = 12,
    Modulate = 13,
    Screen = 14,
    Overlay = 15,
    Darken = 16,
    Lighten = 17,
    ColorDodge = 18,
    ColorBurn = 19,
    HardLight = 20,
    SoftLight = 21,
    Difference = 22,
    Exclusion = 23,
    /// Upstream's "last separable mode".
    Multiply = 24,
    /// The four non-separable modes, which take a whole colour rather than a
    /// channel at a time.
    Hue = 25,
    Saturation = 26,
    Color = 27,
    Luminosity = 28,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StrokeCap {
    #[default]
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StrokeJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// Whether a clip keeps what is inside the shape or what is outside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClipOp {
    #[default]
    Intersect,
    Difference,
}

/// How hard the compositor works on a clip's edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClipBehavior {
    /// No clip at all. Cheapest, and wrong if anything overflows.
    None,
    /// Aliased edge, no offscreen pass.
    HardEdge,
    #[default]
    AntiAlias,
    /// Anti-aliased through an offscreen buffer. Needed when the clipped
    /// content itself is composited (an opacity group, a blend mode).
    AntiAliasWithSaveLayer,
}

impl ClipBehavior {
    /// Every value, in the order the codes run.
    pub const ALL: [ClipBehavior; 4] = [
        ClipBehavior::None,
        ClipBehavior::HardEdge,
        ClipBehavior::AntiAlias,
        ClipBehavior::AntiAliasWithSaveLayer,
    ];

    /// The number the engine reads. `ToClipBehavior` in
    /// `rustflutter_ffi_draw.cc` is the other half; see [`TileMode::code`] for
    /// why a table like this needs a test on this side at all.
    pub(crate) fn code(self) -> c_int {
        match self {
            ClipBehavior::None => 0,
            ClipBehavior::HardEdge => 1,
            ClipBehavior::AntiAlias => 2,
            ClipBehavior::AntiAliasWithSaveLayer => 3,
        }
    }
}

/// Which rule decides whether a point is inside a self-intersecting path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FillType {
    #[default]
    NonZero,
    EvenOdd,
}

// -- Gradients ----------------------------------------------------------------

/// A colour ramp. Stops are positions in 0..1; leaving them out spaces the
/// colours evenly.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    colors: Vec<u32>,
    stops: Option<Vec<f32>>,
    tile_mode: TileMode,
}

impl Gradient {
    /// Fewer than two colours is not a gradient; such a `Gradient` is accepted
    /// and then ignored when applied, rather than panicking mid-paint.
    pub fn new(colors: &[Color]) -> Gradient {
        Gradient {
            colors: colors.iter().map(|c| c.0).collect(),
            stops: None,
            tile_mode: TileMode::default(),
        }
    }

    pub fn with_stops(mut self, stops: &[f32]) -> Gradient {
        self.stops = Some(stops.to_vec());
        self
    }

    pub fn with_tile_mode(mut self, tile_mode: TileMode) -> Gradient {
        self.tile_mode = tile_mode;
        self
    }

    /// Returns (colors, stops, count) ready for the C ABI, or None if this
    /// cannot describe a gradient.
    fn parts(&self) -> Option<(*const u32, *const f32, c_int)> {
        if self.colors.len() < 2 {
            return None;
        }
        let stops = match &self.stops {
            // A stop list that does not match the colours would be read out of
            // bounds on the C side, so it is dropped rather than trusted.
            Some(stops) if stops.len() == self.colors.len() => stops.as_ptr(),
            _ => std::ptr::null(),
        };
        Some((self.colors.as_ptr(), stops, self.colors.len() as c_int))
    }
}

// -- Paint extensions ---------------------------------------------------------

impl Paint {
    pub fn with_opacity(self, opacity: f32) -> Paint {
        unsafe { sys::rf_paint_set_opacity(self.raw, opacity) };
        self
    }

    /// Upstream's `Paint.colorFilter` as `ColorFilter.mode`: every pixel this
    /// paint draws is replaced by `colour` blended into it under `mode`.
    ///
    /// # Not the same thing as [`Paint::with_blend_mode`]
    ///
    /// A blend mode decides how the drawing composites against what is already
    /// on the canvas. A colour filter rewrites the drawing's own pixels before
    /// any compositing happens. Reaching for the first to tint an image is a
    /// mistake that reads plausibly -- both take a `BlendMode`, and both change
    /// the colours that end up on screen.
    ///
    /// `BlendMode::SrcIn` is the one a caller usually wants here: it keeps the
    /// destination's shape and takes the source's colour, so an image becomes
    /// a solid silhouette in `colour` with its own alpha preserved. That
    /// default belongs to the caller rather than to this method, because it is
    /// upstream's `colorBlendMode ?? BlendMode.srcIn` and writing it twice is
    /// how two defaults stop agreeing.
    pub fn with_color_filter(self, colour: Color, mode: BlendMode) -> Paint {
        unsafe { sys::rf_paint_set_color_filter(self.raw, colour.0, mode as c_int) };
        self
    }

    /// Drops any colour filter, so the paint draws what it was given.
    pub fn without_color_filter(self) -> Paint {
        unsafe { sys::rf_paint_clear_color_filter(self.raw) };
        self
    }

    pub fn with_blend_mode(self, mode: BlendMode) -> Paint {
        unsafe { sys::rf_paint_set_blend_mode(self.raw, mode as c_int) };
        self
    }

    pub fn with_stroke_cap(self, cap: StrokeCap) -> Paint {
        unsafe { sys::rf_paint_set_stroke_cap(self.raw, cap as c_int) };
        self
    }

    pub fn with_stroke_join(self, join: StrokeJoin) -> Paint {
        unsafe { sys::rf_paint_set_stroke_join(self.raw, join as c_int) };
        self
    }

    /// Blurs the shape's coverage mask. This is a soft shadow, not a blur of
    /// what is behind the shape -- for that, see
    /// [`LayerTree::push_backdrop_blur`].
    pub fn with_blur(self, sigma: f32) -> Paint {
        unsafe { sys::rf_paint_set_blur(self.raw, sigma) };
        self
    }

    /// Fills along the line from `start` to `end`.
    pub fn with_linear_gradient(
        self,
        start: (f32, f32),
        end: (f32, f32),
        gradient: &Gradient,
    ) -> Paint {
        if let Some((colors, stops, count)) = gradient.parts() {
            unsafe {
                sys::rf_paint_set_linear_gradient(
                    self.raw,
                    start.0,
                    start.1,
                    end.0,
                    end.1,
                    colors,
                    stops,
                    count,
                    gradient.tile_mode.code(),
                )
            };
        }
        self
    }

    /// Fills outwards from `center`.
    pub fn with_radial_gradient(
        self,
        center: (f32, f32),
        radius: f32,
        gradient: &Gradient,
    ) -> Paint {
        if let Some((colors, stops, count)) = gradient.parts() {
            unsafe {
                sys::rf_paint_set_radial_gradient(
                    self.raw,
                    center.0,
                    center.1,
                    radius,
                    colors,
                    stops,
                    count,
                    gradient.tile_mode.code(),
                )
            };
        }
        self
    }

    /// Fills around `center`, sweeping from `start_degrees` to `end_degrees`.
    pub fn with_sweep_gradient(
        self,
        center: (f32, f32),
        start_degrees: f32,
        end_degrees: f32,
        gradient: &Gradient,
    ) -> Paint {
        if let Some((colors, stops, count)) = gradient.parts() {
            unsafe {
                sys::rf_paint_set_sweep_gradient(
                    self.raw,
                    center.0,
                    center.1,
                    start_degrees,
                    end_degrees,
                    colors,
                    stops,
                    count,
                    gradient.tile_mode.code(),
                )
            };
        }
        self
    }

    pub fn without_blur(self) -> Paint {
        unsafe { sys::rf_paint_clear_blur(self.raw) };
        self
    }

    pub fn without_shader(self) -> Paint {
        unsafe { sys::rf_paint_clear_shader(self.raw) };
        self
    }
}

// -- Shadows ------------------------------------------------------------------

/// A shadow cast by a box.
///
/// Upstream's `BoxShadow`, which is `dart:ui`'s `Shadow` plus a spread. The
/// three numbers do different jobs and it is worth keeping them apart: the
/// `offset` moves the shadow, the `spread_radius` grows the *shape* before it
/// is blurred -- a bigger object casting the same shadow -- and the
/// `blur_radius` softens the edge without moving it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    pub color: crate::engine::Color,
    pub offset: crate::render::Offset,
    pub blur_radius: f32,
    pub spread_radius: f32,
}

impl BoxShadow {
    pub const fn new(
        color: crate::engine::Color,
        dx: f32,
        dy: f32,
        blur_radius: f32,
        spread_radius: f32,
    ) -> BoxShadow {
        BoxShadow {
            color,
            offset: crate::render::Offset { dx, dy },
            blur_radius,
            spread_radius,
        }
    }

    /// The blur radius as the sigma a mask filter wants.
    ///
    /// `Shadow.convertRadiusToSigma` upstream, and the same constant: a radius
    /// is the visible extent of the blur, a sigma is the standard deviation of
    /// the gaussian that produces it, and one is not the other.
    pub fn blur_sigma(&self) -> f32 {
        if self.blur_radius > 0.0 {
            self.blur_radius * 0.577_35 + 0.5
        } else {
            0.0
        }
    }

    /// The paint this shadow is drawn with. The offset and the spread are not
    /// in it -- the caller moves and inflates the shape instead.
    pub fn to_paint(&self) -> Paint {
        Paint::new(self.color).with_blur(self.blur_sigma())
    }

    /// Upstream `BoxShadow.scale`: offset, blur and spread multiplied, the
    /// colour untouched.
    pub fn scale(&self, factor: f32) -> BoxShadow {
        BoxShadow {
            color: self.color,
            offset: crate::render::Offset {
                dx: self.offset.dx * factor,
                dy: self.offset.dy * factor,
            },
            blur_radius: self.blur_radius * factor,
            spread_radius: self.spread_radius * factor,
        }
    }

    /// Upstream `BoxShadow.lerp`: a missing side is the other's colour at
    /// zero offset, blur and spread.
    pub fn lerp(a: &BoxShadow, b: &BoxShadow, t: f32) -> BoxShadow {
        BoxShadow {
            color: crate::borders::color_lerp(a.color, b.color, t),
            offset: crate::render::Offset::new(
                a.offset.dx + (b.offset.dx - a.offset.dx) * t,
                a.offset.dy + (b.offset.dy - a.offset.dy) * t,
            ),
            blur_radius: a.blur_radius + (b.blur_radius - a.blur_radius) * t,
            spread_radius: a.spread_radius + (b.spread_radius - a.spread_radius) * t,
        }
    }

    /// Upstream `BoxShadow.lerpList`: excess items on either side scale in
    /// or out.
    pub fn lerp_list(a: &[BoxShadow], b: &[BoxShadow], t: f32) -> Vec<BoxShadow> {
        let common = a.len().min(b.len());
        let mut shadows = Vec::with_capacity(a.len().max(b.len()));
        for index in 0..common {
            shadows.push(BoxShadow::lerp(&a[index], &b[index], t));
        }
        for shadow in &a[common..] {
            shadows.push(shadow.scale(1.0 - t));
        }
        for shadow in &b[common..] {
            shadows.push(shadow.scale(t));
        }
        shadows
    }
}

/// Material's elevation table: how high something is, as shadows.
///
/// A port of `kElevationToShadow` from `material/shadows.dart`, including the
/// three-shadow structure. The three are not arbitrary -- they are the key
/// light's umbra, its penumbra and the ambient light, and dropping any of them
/// gives a shadow that looks wrong in a way that is hard to name.
///
/// Upstream defines only the elevations some widget actually uses; this returns
/// the nearest defined one at or below `elevation`, so an in-between number
/// still casts something.
pub fn elevation_shadows(elevation: u32) -> &'static [BoxShadow] {
    use crate::engine::Color;
    const UMBRA: Color = Color(0x33000000);
    const PENUMBRA: Color = Color(0x24000000);
    const AMBIENT: Color = Color(0x1f000000);

    const E1: [BoxShadow; 3] = [
        BoxShadow::new(UMBRA, 0.0, 2.0, 1.0, -1.0),
        BoxShadow::new(PENUMBRA, 0.0, 1.0, 1.0, 0.0),
        BoxShadow::new(AMBIENT, 0.0, 1.0, 3.0, 0.0),
    ];
    const E2: [BoxShadow; 3] = [
        BoxShadow::new(UMBRA, 0.0, 3.0, 1.0, -2.0),
        BoxShadow::new(PENUMBRA, 0.0, 2.0, 2.0, 0.0),
        BoxShadow::new(AMBIENT, 0.0, 1.0, 5.0, 0.0),
    ];
    const E3: [BoxShadow; 3] = [
        BoxShadow::new(UMBRA, 0.0, 3.0, 3.0, -2.0),
        BoxShadow::new(PENUMBRA, 0.0, 3.0, 4.0, 0.0),
        BoxShadow::new(AMBIENT, 0.0, 1.0, 8.0, 0.0),
    ];
    const E4: [BoxShadow; 3] = [
        BoxShadow::new(UMBRA, 0.0, 2.0, 4.0, -1.0),
        BoxShadow::new(PENUMBRA, 0.0, 4.0, 5.0, 0.0),
        BoxShadow::new(AMBIENT, 0.0, 1.0, 10.0, 0.0),
    ];
    const E6: [BoxShadow; 3] = [
        BoxShadow::new(UMBRA, 0.0, 3.0, 5.0, -1.0),
        BoxShadow::new(PENUMBRA, 0.0, 6.0, 10.0, 0.0),
        BoxShadow::new(AMBIENT, 0.0, 1.0, 18.0, 0.0),
    ];
    const E8: [BoxShadow; 3] = [
        BoxShadow::new(UMBRA, 0.0, 5.0, 5.0, -3.0),
        BoxShadow::new(PENUMBRA, 0.0, 8.0, 10.0, 1.0),
        BoxShadow::new(AMBIENT, 0.0, 3.0, 14.0, 2.0),
    ];
    const E9: [BoxShadow; 3] = [
        BoxShadow::new(UMBRA, 0.0, 5.0, 6.0, -3.0),
        BoxShadow::new(PENUMBRA, 0.0, 9.0, 12.0, 1.0),
        BoxShadow::new(AMBIENT, 0.0, 3.0, 16.0, 2.0),
    ];
    const E12: [BoxShadow; 3] = [
        BoxShadow::new(UMBRA, 0.0, 7.0, 8.0, -4.0),
        BoxShadow::new(PENUMBRA, 0.0, 12.0, 17.0, 2.0),
        BoxShadow::new(AMBIENT, 0.0, 5.0, 22.0, 4.0),
    ];
    const E16: [BoxShadow; 3] = [
        BoxShadow::new(UMBRA, 0.0, 8.0, 10.0, -5.0),
        BoxShadow::new(PENUMBRA, 0.0, 16.0, 24.0, 2.0),
        BoxShadow::new(AMBIENT, 0.0, 6.0, 30.0, 5.0),
    ];
    const E24: [BoxShadow; 3] = [
        BoxShadow::new(UMBRA, 0.0, 11.0, 15.0, -7.0),
        BoxShadow::new(PENUMBRA, 0.0, 24.0, 38.0, 3.0),
        BoxShadow::new(AMBIENT, 0.0, 9.0, 46.0, 8.0),
    ];

    match elevation {
        0 => &[],
        1 => &E1,
        2 => &E2,
        3 => &E3,
        4 | 5 => &E4,
        6 | 7 => &E6,
        8 => &E8,
        9..=11 => &E9,
        12..=15 => &E12,
        16..=23 => &E16,
        _ => &E24,
    }
}

// -- Path ---------------------------------------------------------------------

/// An arbitrary outline, built imperatively.
///
/// Reusable: drawing a path does not consume it, and it may be added to
/// afterwards. Upstream this is `dart:ui`'s `Path`.
pub struct RenderPath {
    raw: *mut sys::RfPath,
}

impl RenderPath {
    pub fn new() -> RenderPath {
        let raw = unsafe { sys::rf_path_new() };
        assert!(!raw.is_null(), "engine failed to allocate a path");
        RenderPath { raw }
    }

    pub fn with_fill_type(self, fill_type: FillType) -> RenderPath {
        unsafe { sys::rf_path_set_fill_type(self.raw, fill_type as c_int) };
        self
    }

    pub fn move_to(&mut self, x: f32, y: f32) -> &mut RenderPath {
        unsafe { sys::rf_path_move_to(self.raw, x, y) };
        self
    }

    pub fn line_to(&mut self, x: f32, y: f32) -> &mut RenderPath {
        unsafe { sys::rf_path_line_to(self.raw, x, y) };
        self
    }

    pub fn quadratic_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) -> &mut RenderPath {
        unsafe { sys::rf_path_quadratic_to(self.raw, cx, cy, x, y) };
        self
    }

    pub fn cubic_to(
        &mut self,
        cx1: f32,
        cy1: f32,
        cx2: f32,
        cy2: f32,
        x: f32,
        y: f32,
    ) -> &mut RenderPath {
        unsafe { sys::rf_path_cubic_to(self.raw, cx1, cy1, cx2, cy2, x, y) };
        self
    }

    pub fn close(&mut self) -> &mut RenderPath {
        unsafe { sys::rf_path_close(self.raw) };
        self
    }

    pub fn add_rect(&mut self, rect: Rect) -> &mut RenderPath {
        unsafe { sys::rf_path_add_rect(self.raw, rect.left, rect.top, rect.right, rect.bottom) };
        self
    }

    pub fn add_oval(&mut self, rect: Rect) -> &mut RenderPath {
        unsafe { sys::rf_path_add_oval(self.raw, rect.left, rect.top, rect.right, rect.bottom) };
        self
    }

    pub fn add_circle(&mut self, x: f32, y: f32, radius: f32) -> &mut RenderPath {
        unsafe { sys::rf_path_add_circle(self.raw, x, y, radius) };
        self
    }

    /// Upstream `Path.arcTo`: the piece of the ellipse inscribed in `rect`
    /// between two angles, appended to the path.
    ///
    /// Angles are in radians, clockwise from three o'clock, which is what
    /// upstream's `Path.arcTo` takes -- the neighbouring
    /// [`Canvas::draw_arc`](crate::painting::Canvas::draw_arc) takes degrees
    /// only because the binding underneath it does.
    ///
    /// `force_move_to` starts a new subpath at the arc's first point rather
    /// than joining it to whatever came before with a line.
    ///
    /// The binding has no `arcTo`, so the arc is written as cubic Béziers:
    /// the sweep is cut into pieces of at most a quarter turn, and each piece
    /// gets the curve whose control points are `4/3 * tan(delta/4)` of the
    /// tangent away from its ends. That is the standard approximation, and it
    /// is exact at the two ends and within a fraction of a pixel between them
    /// at any radius a slider draws at.
    pub fn arc_to(
        &mut self,
        rect: Rect,
        start_radians: f32,
        sweep_radians: f32,
        force_move_to: bool,
    ) -> &mut RenderPath {
        let (cx, cy) = rect.center();
        let rx = rect.width() / 2.0;
        let ry = rect.height() / 2.0;
        let point_at = |angle: f32| (cx + rx * angle.cos(), cy + ry * angle.sin());

        let start = point_at(start_radians);
        if force_move_to {
            self.move_to(start.0, start.1);
        } else {
            self.line_to(start.0, start.1);
        }
        if sweep_radians == 0.0 {
            return self;
        }

        for [(cx1, cy1), (cx2, cy2), (x, y)] in arc_cubics(rect, start_radians, sweep_radians) {
            self.cubic_to(cx1, cy1, cx2, cy2, x, y);
        }
        self
    }

    pub fn add_rounded_rect(
        &mut self,
        rect: Rect,
        radius_x: f32,
        radius_y: f32,
    ) -> &mut RenderPath {
        unsafe {
            sys::rf_path_add_rounded_rect(
                self.raw,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                radius_x,
                radius_y,
            )
        };
        self
    }
}

/// The cubic segments an elliptical arc is written as: two control points
/// and an end point each, in the order [`RenderPath::cubic_to`] wants them.
///
/// Split out from [`RenderPath::arc_to`] so that the approximation can be
/// checked against the ellipse it is standing in for.
pub(crate) fn arc_cubics(
    rect: Rect,
    start_radians: f32,
    sweep_radians: f32,
) -> Vec<[(f32, f32); 3]> {
    if sweep_radians == 0.0 {
        return Vec::new();
    }
    let (cx, cy) = rect.center();
    let rx = rect.width() / 2.0;
    let ry = rect.height() / 2.0;
    let point_at = |angle: f32| (cx + rx * angle.cos(), cy + ry * angle.sin());
    // The tangent at an angle, scaled by the radii.
    let tangent_at = |angle: f32| (-rx * angle.sin(), ry * angle.cos());

    let pieces = (sweep_radians.abs() / std::f32::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let delta = sweep_radians / pieces as f32;
    let k = 4.0 / 3.0 * (delta / 4.0).tan();
    let mut segments = Vec::with_capacity(pieces);
    let mut angle = start_radians;
    for _ in 0..pieces {
        let next = angle + delta;
        let (x0, y0) = point_at(angle);
        let (x1, y1) = point_at(next);
        let (tx0, ty0) = tangent_at(angle);
        let (tx1, ty1) = tangent_at(next);
        segments.push([
            (x0 + k * tx0, y0 + k * ty0),
            (x1 - k * tx1, y1 - k * ty1),
            (x1, y1),
        ]);
        angle = next;
    }
    segments
}
impl Default for RenderPath {
    fn default() -> RenderPath {
        RenderPath::new()
    }
}

impl Drop for RenderPath {
    fn drop(&mut self) {
        unsafe { sys::rf_path_free(self.raw) };
    }
}

// -- Image --------------------------------------------------------------------

// -- Shaped text --------------------------------------------------------------

/// Everything a shaped paragraph depends on.
///
/// Floats are keyed by their bits rather than their value: `f32` is not `Eq`,
/// and two sizes that differ only by a rounding step are two different
/// paragraphs anyway.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ShapeKey {
    /// One entry per styled run. A single-style paragraph has one; a sentence
    /// with a bold word in it has three, and the whole list is the key,
    /// because changing any run reshapes the paragraph.
    runs: Vec<RunKey>,
    align: u8,
    /// The paragraph's base direction, as a code. Part of the key for the
    /// same reason `align` is: it is a paragraph-style input, and
    /// `TextAlign::start` in one direction and `TextAlign::end` in the other
    /// are the *same* code shaped two different ways.
    direction: u8,
    max_lines: usize,
    /// Part of the key because it goes into the paragraph style: the same text
    /// in the same width is a different paragraph with an ellipsis than
    /// without one.
    ellipsis: bool,
    max_width_bits: u32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct RunKey {
    text: String,
    family: Option<String>,
    /// Empty and `None` are the same request -- no fallback -- so the empty
    /// list is folded away here rather than cached twice.
    fallback: Option<Vec<String>>,
    size_bits: u32,
    weight: i32,
    italic: bool,
    letter_spacing_bits: Option<u32>,
    word_spacing_bits: Option<u32>,
    /// Bits rather than value, and `None` kept apart from `Some(1.0)`: the
    /// font's own line height is not its font size.
    height_bits: Option<u32>,
    decoration: u8,
    /// Folded the same way as `fallback`.
    font_features: Option<Vec<(String, u32)>>,
    color: u32,
}

impl RunKey {
    fn new(text: &str, style: &TextStyle) -> RunKey {
        let bits = |value: Option<f32>| value.map(f32::to_bits);
        // An empty list asks for nothing, which is what `None` asks for.
        fn non_empty<T: Clone>(list: Option<&Vec<T>>) -> Option<Vec<T>> {
            list.filter(|list| !list.is_empty()).cloned()
        }
        RunKey {
            text: text.to_string(),
            family: style.font_family.clone(),
            fallback: non_empty(style.font_family_fallback.as_ref()),
            size_bits: style.font_size.to_bits(),
            weight: style.font_weight,
            italic: style.italic,
            letter_spacing_bits: bits(style.letter_spacing),
            word_spacing_bits: bits(style.word_spacing),
            height_bits: bits(style.height),
            decoration: style.decoration.0,
            font_features: non_empty(style.font_features.as_ref()),
            color: style.color.0,
        }
    }
}

/// The FFI's code for an alignment, in [`TextAlign`](crate::engine::TextAlign)'s
/// variant order: 0 left .. 5 justify. The enum owns the mapping; see
/// `TextAlign::code` for why start and end are codes of their own rather than
/// resolved here.
fn align_code(align: TextAlign) -> u8 {
    align.code() as u8
}

/// The FFI's code for a direction: 0 ltr, 1 rtl.
fn direction_code(direction: crate::direction::TextDirection) -> u8 {
    (direction == crate::direction::TextDirection::Rtl) as u8
}

impl ShapeKey {
    fn new(
        text: &str,
        style: &TextStyle,
        direction: crate::direction::TextDirection,
        max_lines: Option<usize>,
        ellipsis: bool,
        max_width: f32,
    ) -> ShapeKey {
        ShapeKey {
            runs: vec![RunKey::new(text, style)],
            align: align_code(style.align),
            direction: direction_code(direction),
            max_lines: max_lines.unwrap_or(0),
            ellipsis,
            max_width_bits: max_width.to_bits(),
        }
    }

    fn rich(
        runs: &[(String, TextStyle)],
        align: TextAlign,
        direction: crate::direction::TextDirection,
        max_lines: Option<usize>,
        ellipsis: bool,
        max_width: f32,
    ) -> ShapeKey {
        ShapeKey {
            runs: runs
                .iter()
                .map(|(text, style)| RunKey::new(text, style))
                .collect(),
            align: align_code(align),
            direction: direction_code(direction),
            max_lines: max_lines.unwrap_or(0),
            ellipsis,
            max_width_bits: max_width.to_bits(),
        }
    }
}

/// Paragraphs shaped this frame and last.
///
/// Two generations rather than one map: a paragraph is looked up in `current`,
/// then in `previous` -- where a hit is promoted -- and only shaped if neither
/// has it. At the end of a frame `current` becomes `previous` and a fresh
/// `current` starts. Anything that stopped being drawn therefore falls out
/// after two frames, which is what keeps a label that counts upwards from
/// filling memory with every number it has ever shown.
#[derive(Default)]
struct ShapeCache {
    current: HashMap<ShapeKey, Rc<Paragraph>>,
    previous: HashMap<ShapeKey, Rc<Paragraph>>,
}

thread_local! {
    static SHAPED: RefCell<ShapeCache> = RefCell::new(ShapeCache::default());
}

/// Shapes `text`, or returns the paragraph shaped for the same request earlier.
///
/// Upstream a `RenderParagraph` owns a `TextPainter` and re-shapes only when
/// the text, the style or the constraints change. Here the render tree is
/// rebuilt every frame, so a render object has nowhere to keep anything and the
/// cache has to sit beside the tree instead of inside it. The effect is the
/// same, and the cost it removes is real: shaping is font matching, itemisation,
/// line breaking and glyph positioning, and a screen of static text was paying
/// for all four sixty times a second.
///
/// Thread-local, because a paragraph is a raw engine handle and only the UI
/// thread lays out.
///
/// # The reader's text size
///
/// `scale` is the reader's text size setting, and it is applied here -- every
/// size on screen is a font size that came through this function, and every
/// measurement the framework makes comes back out of the paragraph rather than
/// out of the style, so this is the only place it has to be applied.
///
/// It is a parameter rather than a global read because a subtree can have its
/// own: an icon font that should not grow, a preview showing what some other
/// setting looks like. Upstream the same value is a `TextScaler` field on
/// `TextPainter`, put there by whoever built the `RenderParagraph` after
/// reading `MediaQuery.textScalerOf(context)`. The callers here do the same;
/// see [`crate::media_query::current_text_scale`].
///
/// The cache needs no help with this: the scale changes the style it keys on,
/// so text shaped at the old size is simply never asked for again.
///
/// # The direction text runs in
///
/// The paragraph's base direction is read here rather than passed in, from
/// [`crate::direction::current_direction`]: it is what
/// `TextAlign::start`/`end` and bidi resolution are measured against, and it
/// is part of the cache key for the same reason the alignment is. Once the
/// render-tree side of directionality lands, a paragraph's own direction
/// travels the way the scale does -- captured where the object was built,
/// passed in here -- and this read becomes the fallback for paragraphs shaped
/// outside a tree.
pub fn shape(
    text: &str,
    style: &TextStyle,
    max_lines: Option<usize>,
    ellipsis: bool,
    max_width: f32,
    scale: f32,
) -> Rc<Paragraph> {
    let scaled;
    let style = if scale == 1.0 {
        style
    } else {
        scaled = TextStyle {
            font_size: style.font_size * scale,
            ..style.clone()
        };
        &scaled
    };
    let direction = crate::direction::current_direction();
    let key = ShapeKey::new(text, style, direction, max_lines, ellipsis, max_width);
    SHAPED.with(|cache| {
        {
            let cache = cache.borrow();
            if let Some(hit) = cache.current.get(&key) {
                return hit.clone();
            }
        }
        // Taken out of the old generation rather than copied, so the entry
        // cannot end up in both.
        let carried = cache.borrow_mut().previous.remove(&key);
        if let Some(hit) = carried {
            cache.borrow_mut().current.insert(key, hit.clone());
            return hit;
        }
        let shaped = Rc::new(Paragraph::new(
            text, style, max_lines, ellipsis, max_width, direction,
        ));
        cache.borrow_mut().current.insert(key, shaped.clone());
        shaped
    })
}

/// Shapes a paragraph made of differently styled runs, through the same
/// cache.
///
/// The runs are one paragraph rather than several: line breaking, bidi
/// reordering and baselines all work across the whole of it. Upstream reaches
/// this through `TextPainter` walking a tree of `TextSpan`s and pushing each
/// one's style; the tree is flattened before it gets here, because a nested
/// span's style is resolved against its parent's at that point anyway.
pub fn shape_rich(
    runs: &[(String, TextStyle)],
    align: TextAlign,
    max_lines: Option<usize>,
    ellipsis: bool,
    max_width: f32,
    scale: f32,
) -> Rc<Paragraph> {
    let scaled: Vec<(String, TextStyle)> = if scale == 1.0 {
        runs.to_vec()
    } else {
        runs.iter()
            .map(|(text, style)| {
                (
                    text.clone(),
                    TextStyle {
                        font_size: style.font_size * scale,
                        ..style.clone()
                    },
                )
            })
            .collect()
    };
    let direction = crate::direction::current_direction();
    let key = ShapeKey::rich(&scaled, align, direction, max_lines, ellipsis, max_width);
    SHAPED.with(|cache| {
        {
            let cache = cache.borrow();
            if let Some(hit) = cache.current.get(&key) {
                return hit.clone();
            }
        }
        let carried = cache.borrow_mut().previous.remove(&key);
        if let Some(hit) = carried {
            cache.borrow_mut().current.insert(key, hit.clone());
            return hit;
        }
        let shaped = Rc::new(Paragraph::rich(
            &scaled, align, max_lines, ellipsis, max_width, direction,
        ));
        cache.borrow_mut().current.insert(key, shaped.clone());
        shaped
    })
}

/// Ages the shape cache by one frame. Called once per frame, after painting.
pub fn end_text_frame() {
    SHAPED.with(|cache| {
        let mut cache = cache.borrow_mut();
        let live = std::mem::take(&mut cache.current);
        cache.previous = live;
    });
}

/// How many paragraphs the cache is holding, for tests and diagnostics.
pub fn shaped_paragraph_count() -> usize {
    SHAPED.with(|cache| {
        let cache = cache.borrow();
        cache.current.len() + cache.previous.len()
    })
}

// -- Decoding off the UI thread -----------------------------------------------

/// A decoded handle on its way back from a worker thread.
///
/// `Image` is a raw engine pointer, so it is not `Send` by default and should
/// not be: two threads holding one would be a double free waiting to happen.
/// This wrapper is, and the reason it is sound is the handoff -- the worker
/// builds the handle, sends it, and never names it again, so the receiving
/// thread is the only one that can reach it afterwards. `rf_image_decode`
/// itself shares nothing between calls; its one piece of global state is a
/// `std::call_once` codec registration.
struct Handoff(Option<Image>);

// SAFETY: see above. Ownership moves with the value and is never duplicated.
unsafe impl Send for Handoff {}

struct Request {
    key: String,
    data: Vec<u8>,
}

struct Decoded {
    key: String,
    image: Handoff,
}

/// What is known about one image.
enum Slot {
    /// A worker has it; ask again next frame.
    Decoding,
    /// Decoded, or decoded and found to be unreadable.
    Done(Option<Rc<Image>>),
}

/// The images this thread has asked for, and the workers decoding them.
///
/// One pool per thread that builds, which in practice means one: the UI thread.
/// A pool shared between threads would need results routed back to whoever
/// asked, and nothing here asks from anywhere else.
struct ImageCache {
    entries: HashMap<String, Slot>,
    requests: std::sync::mpsc::Sender<Request>,
    results: std::sync::mpsc::Receiver<Decoded>,
    /// Requests sent and not yet collected.
    outstanding: usize,
    /// A decode has landed and nothing has been rebuilt around it yet.
    arrived: bool,
}

impl ImageCache {
    fn new() -> ImageCache {
        let (request_tx, request_rx) = std::sync::mpsc::channel::<Request>();
        let (result_tx, result_rx) = std::sync::mpsc::channel::<Decoded>();
        let request_rx = std::sync::Arc::new(std::sync::Mutex::new(request_rx));

        // Enough to overlap decoding with building, not so many that a screen
        // of thumbnails saturates the machine the UI thread is running on.
        let workers = std::thread::available_parallelism()
            .map_or(2, |count| count.get().saturating_sub(1).clamp(1, 4));
        for index in 0..workers {
            let requests = std::sync::Arc::clone(&request_rx);
            let results = result_tx.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("rf.image.{index}"))
                .spawn(move || {
                    loop {
                        // The lock is held across the receive on purpose: the
                        // workers queue on it rather than on the channel, which
                        // is the same arrangement with one fewer moving part.
                        // Whoever holds it takes the next request and lets go.
                        let request = {
                            let Ok(receiver) = requests.lock() else {
                                return;
                            };
                            receiver.recv()
                        };
                        let Ok(request) = request else {
                            // The cache went away with its thread.
                            return;
                        };
                        let image = Image::decode(&request.data);
                        let decoded = Decoded {
                            key: request.key,
                            image: Handoff(image),
                        };
                        if results.send(decoded).is_err() {
                            return;
                        }
                    }
                });
            if spawned.is_err() {
                // Out of threads. Decoding then falls back to this one, which
                // is slow but correct -- see `get_or_request`.
                break;
            }
        }

        ImageCache {
            entries: HashMap::new(),
            requests: request_tx,
            results: result_rx,
            outstanding: 0,
            arrived: false,
        }
    }

    /// Files everything the workers have finished. Cheap, and called from every
    /// lookup, so a finished decode never waits for a frame boundary to be
    /// noticed.
    fn collect(&mut self) {
        while let Ok(decoded) = self.results.try_recv() {
            self.outstanding = self.outstanding.saturating_sub(1);
            self.arrived = true;
            self.entries
                .insert(decoded.key, Slot::Done(decoded.image.0.map(Rc::new)));
        }
    }

    fn get_or_request(&mut self, key: &str, data: &[u8]) -> Option<Rc<Image>> {
        self.collect();
        match self.entries.get(key) {
            Some(Slot::Done(image)) => return image.clone(),
            Some(Slot::Decoding) => return None,
            None => {}
        }

        // The bytes are copied because the worker outlives this call and the
        // caller's slice may not. For baked-in assets that is a few hundred
        // kilobytes in flight, freed as each decode finishes.
        let request = Request {
            key: key.to_string(),
            data: data.to_vec(),
        };
        match self.requests.send(request) {
            Ok(()) => {
                self.entries.insert(key.to_string(), Slot::Decoding);
                self.outstanding += 1;
                None
            }
            Err(returned) => {
                // No workers came up. Decode here rather than never.
                let image = Image::decode(&returned.0.data).map(Rc::new);
                self.entries
                    .insert(key.to_string(), Slot::Done(image.clone()));
                image
            }
        }
    }

    fn get_or_decode(&mut self, key: &str, data: &[u8]) -> Option<Rc<Image>> {
        self.collect();
        if let Some(Slot::Done(image)) = self.entries.get(key) {
            return image.clone();
        }
        // Already with a worker: wait for that one rather than decoding the
        // same bytes a second time.
        if matches!(self.entries.get(key), Some(Slot::Decoding)) {
            self.wait();
            if let Some(Slot::Done(image)) = self.entries.get(key) {
                return image.clone();
            }
        }
        let image = Image::decode(data).map(Rc::new);
        self.entries
            .insert(key.to_string(), Slot::Done(image.clone()));
        image
    }

    /// Blocks until every outstanding decode has been filed.
    fn wait(&mut self) {
        while self.outstanding > 0 {
            let Ok(decoded) = self.results.recv() else {
                // Every worker is gone; nothing else is coming.
                self.outstanding = 0;
                break;
            };
            self.outstanding -= 1;
            self.arrived = true;
            self.entries
                .insert(decoded.key, Slot::Done(decoded.image.0.map(Rc::new)));
        }
    }
}

thread_local! {
    static IMAGES: RefCell<ImageCache> = RefCell::new(ImageCache::new());
}

/// Reaches the cache, having first checked that this is the thread that has
/// one.
///
/// Worse here than for the messenger: `ImageCache::new` spawns workers, so
/// touching this from another thread does not merely miss the cache -- it
/// stands up a second decode pool, on a thread that will never draw. The
/// comment on `ImageCache` states the assumption ("one pool per thread that
/// builds, which in practice means one"); this is what enforces it.
fn with_images<R>(body: impl FnOnce(&mut ImageCache) -> R) -> R {
    crate::task::debug_assert_ui_thread("the image cache");
    IMAGES.with(|images| body(&mut images.borrow_mut()))
}

/// Drops a cache entry, upstream `ImageCache.evict` narrowed to the key
/// spellings the crate caches under. Whether anything was there.
pub fn image_cache_evict(key: &str) -> bool {
    with_images(|images| images.entries.remove(key).is_some())
}

/// Upstream `ImageCache.statusForKey`, in the three states the slot has.
pub fn image_cache_status(key: &str) -> crate::image::ImageCacheStatus {
    with_images(|images| match images.entries.get(key) {
        Some(Slot::Decoding) => crate::image::ImageCacheStatus::Pending,
        Some(Slot::Done(Some(_))) => crate::image::ImageCacheStatus::Live,
        _ => crate::image::ImageCacheStatus::Uncached,
    })
}

/// Whether any image asked for is still being decoded.
///
/// A frame that sees this true has drawn without an image it wanted and should
/// ask for another; that is how the picture arrives once it is ready.
pub fn images_pending() -> bool {
    with_images(|images| {
        images.collect();
        images.outstanding > 0
    })
}

/// Whether a decode has landed since this was last asked, clearing the flag.
///
/// The frame that asked for an image got `None` and drew a placeholder; when
/// the picture arrives, whoever asked has to be built again to see it. Nothing
/// here knows who that was, so the answer is "everyone" -- the same full
/// rebuild a resize triggers, and for the same reason. Upstream is narrower:
/// the decoder completes a future the image widget is holding, and only that
/// widget rebuilds. Getting there needs `Image::shared` to know which element
/// is calling it, which is the same machinery `InheritedWidget` dependency
/// tracking wants.
pub fn take_images_arrived() -> bool {
    with_images(|images| {
        images.collect();
        std::mem::replace(&mut images.arrived, false)
    })
}

/// Blocks until every image asked for has been decoded.
///
/// Returns whether anything was waited for, so a caller with only one frame to
/// get right -- a headless render, a golden -- can rebuild once and know the
/// result is complete.
pub fn wait_for_images() -> bool {
    with_images(|images| {
        let waited = images.outstanding > 0;
        images.wait();
        waited
    })
}

/// A decoded image.
pub struct Image {
    raw: *mut sys::RfImage,
}

impl Image {
    /// Decodes PNG, JPEG, WebP, GIF or BMP bytes. Returns None if the format
    /// was not recognised or the data was truncated.
    pub fn decode(data: &[u8]) -> Option<Image> {
        let raw = unsafe { sys::rf_image_decode(data.as_ptr(), data.len()) };
        if raw.is_null() {
            None
        } else {
            Some(Image { raw })
        }
    }

    /// Wraps pixels somebody else decoded: `width * height * 4` bytes, tightly
    /// packed, RGBA8888 with premultiplied alpha.
    ///
    /// For the images Skia has no codec for and the platform does. Windows
    /// reads HEIC through WIC, and an album of phone photographs is mostly
    /// HEIC; the decode happens there and the pixels arrive here.
    ///
    /// Returns None if the dimensions are not positive, `pixels` is shorter
    /// than they claim, or the engine could not allocate.
    pub fn from_pixels(pixels: &[u8], width: i32, height: i32) -> Option<Image> {
        if width <= 0 || height <= 0 {
            return None;
        }
        // Checked rather than trusted: the length is what the engine reads
        // against, and a short buffer is a read past the end of it.
        let needed = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if pixels.len() < needed {
            return None;
        }
        let raw = unsafe { sys::rf_image_from_pixels(pixels.as_ptr(), width, height) };
        if raw.is_null() {
            None
        } else {
            Some(Image { raw })
        }
    }

    pub fn width(&self) -> i32 {
        unsafe { sys::rf_image_width(self.raw) }
    }

    pub fn height(&self) -> i32 {
        unsafe { sys::rf_image_height(self.raw) }
    }

    pub fn size(&self) -> (i32, i32) {
        (self.width(), self.height())
    }

    /// The image for `key`, decoding it on a worker thread if this is the first
    /// time it has been asked for.
    ///
    /// Returns `None` until the decode lands, so a caller has to be able to
    /// draw without it -- a placeholder, or nothing. That is deliberate, and it
    /// is what upstream does too: `ImageDecoder` runs on a concurrent worker
    /// because a screen of photographs decoded on the UI thread is a screen of
    /// dropped frames. Thirty-eight of Shrine's product shots cost six
    /// milliseconds, which is a third of a frame spent not building one.
    ///
    /// The frame that gets a `None` asks for another frame, so the image
    /// appears as soon as it is ready; see `images_pending`.
    ///
    /// A failed decode is remembered as a failure. A PNG that could not be read
    /// will not read next frame either, and retrying it sixty times a second is
    /// the same waste in a different shape.
    ///
    /// Thread-local, because a decoded image is a raw engine handle and the UI
    /// thread is the only one that builds.
    pub fn shared(key: &str, data: &[u8]) -> Option<Rc<Image>> {
        with_images(|images| images.get_or_request(key, data))
    }

    /// Decodes `data` on this thread, blocking until it is done.
    ///
    /// For the paths that have exactly one frame to get right and no next frame
    /// to fall back on -- a headless render, a golden test.
    pub fn shared_now(key: &str, data: &[u8]) -> Option<Rc<Image>> {
        with_images(|images| images.get_or_decode(key, data))
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe { sys::rf_image_free(self.raw) };
    }
}

// -- Canvas extensions --------------------------------------------------------

impl Canvas {
    pub fn draw_line(&mut self, from: (f32, f32), to: (f32, f32), paint: &Paint) {
        unsafe { sys::rf_canvas_draw_line(self.raw, from.0, from.1, to.0, to.1, paint.raw) };
    }

    pub fn draw_oval(&mut self, rect: Rect, paint: &Paint) {
        unsafe {
            sys::rf_canvas_draw_oval(
                self.raw,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                paint.raw,
            )
        };
    }

    pub fn draw_path(&mut self, path: &RenderPath, paint: &Paint) {
        unsafe { sys::rf_canvas_draw_path(self.raw, path.raw, paint.raw) };
    }

    /// Angles are in degrees, clockwise from three o'clock. `use_center` draws
    /// the wedge (a pie slice) rather than the bare arc.
    pub fn draw_arc(
        &mut self,
        rect: Rect,
        start_degrees: f32,
        sweep_degrees: f32,
        use_center: bool,
        paint: &Paint,
    ) {
        unsafe {
            sys::rf_canvas_draw_arc(
                self.raw,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                start_degrees,
                sweep_degrees,
                use_center as c_int,
                paint.raw,
            )
        };
    }

    pub fn draw_image(&mut self, image: &Image, x: f32, y: f32, paint: Option<&Paint>) {
        let paint = paint.map_or(std::ptr::null(), |p| p.raw as *const _);
        unsafe { sys::rf_canvas_draw_image(self.raw, image.raw, x, y, paint) };
    }

    pub fn draw_image_rect(
        &mut self,
        image: &Image,
        source: Rect,
        destination: Rect,
        paint: Option<&Paint>,
    ) {
        let paint = paint.map_or(std::ptr::null(), |p| p.raw as *const _);
        unsafe {
            sys::rf_canvas_draw_image_rect(
                self.raw,
                image.raw,
                source.left,
                source.top,
                source.right,
                source.bottom,
                destination.left,
                destination.top,
                destination.right,
                destination.bottom,
                paint,
            )
        };
    }

    // -- State ----------------------------------------------------------------

    pub fn save(&mut self) {
        unsafe { sys::rf_canvas_save(self.raw) };
    }

    /// Starts an offscreen group, composited with `paint` on restore. This is
    /// what group opacity and non-trivial blend modes need; a plain `save` is
    /// cheaper when neither applies.
    pub fn save_layer(&mut self, bounds: Option<Rect>, paint: Option<&Paint>) {
        let bounds_array;
        let bounds_ptr = match bounds {
            Some(rect) => {
                bounds_array = [rect.left, rect.top, rect.right, rect.bottom];
                bounds_array.as_ptr()
            }
            None => std::ptr::null(),
        };
        let paint = paint.map_or(std::ptr::null(), |p| p.raw as *const _);
        unsafe { sys::rf_canvas_save_layer(self.raw, bounds_ptr, paint) };
    }

    pub fn restore(&mut self) {
        unsafe { sys::rf_canvas_restore(self.raw) };
    }

    pub fn save_count(&self) -> i32 {
        unsafe { sys::rf_canvas_save_count(self.raw) }
    }

    pub fn restore_to_count(&mut self, count: i32) {
        unsafe { sys::rf_canvas_restore_to_count(self.raw, count) };
    }

    /// Runs `body` between a save and a restore, so an early return inside it
    /// cannot leave the stack unbalanced.
    pub fn saved<R>(&mut self, body: impl FnOnce(&mut Canvas) -> R) -> R {
        let count = self.save_count();
        self.save();
        let result = body(self);
        self.restore_to_count(count);
        result
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        unsafe { sys::rf_canvas_translate(self.raw, dx, dy) };
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        unsafe { sys::rf_canvas_scale(self.raw, sx, sy) };
    }

    pub fn rotate(&mut self, degrees: f32) {
        unsafe { sys::rf_canvas_rotate(self.raw, degrees) };
    }

    pub fn skew(&mut self, sx: f32, sy: f32) {
        unsafe { sys::rf_canvas_skew(self.raw, sx, sy) };
    }

    /// A 2D affine `[a c e / b d f]`: `x' = a*x + c*y + e`.
    pub fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        unsafe { sys::rf_canvas_transform(self.raw, a, b, c, d, e, f) };
    }

    pub fn clip_rect(&mut self, rect: Rect, op: ClipOp, anti_alias: bool) {
        unsafe {
            sys::rf_canvas_clip_rect(
                self.raw,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                op as c_int,
                anti_alias as c_int,
            )
        };
    }

    pub fn clip_rounded_rect(
        &mut self,
        rect: Rect,
        radius_x: f32,
        radius_y: f32,
        op: ClipOp,
        anti_alias: bool,
    ) {
        unsafe {
            sys::rf_canvas_clip_rounded_rect(
                self.raw,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                radius_x,
                radius_y,
                op as c_int,
                anti_alias as c_int,
            )
        };
    }

    pub fn clip_path(&mut self, path: &RenderPath, op: ClipOp, anti_alias: bool) {
        unsafe { sys::rf_canvas_clip_path(self.raw, path.raw, op as c_int, anti_alias as c_int) };
    }
}

// -- Layer stack --------------------------------------------------------------

impl LayerTree {
    /// A 2D affine, applied to everything pushed until the matching `pop`.
    pub fn push_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        unsafe { sys::rf_layer_tree_push_transform(self.raw, a, b, c, d, e, f) };
    }

    pub fn push_offset(&mut self, dx: f32, dy: f32) {
        unsafe { sys::rf_layer_tree_push_offset(self.raw, dx, dy) };
    }

    pub fn push_clip_rect(&mut self, rect: Rect, behavior: ClipBehavior) {
        unsafe {
            sys::rf_layer_tree_push_clip_rect(
                self.raw,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                behavior.code(),
            )
        };
    }

    pub fn push_clip_rounded_rect(
        &mut self,
        rect: Rect,
        radius_x: f32,
        radius_y: f32,
        behavior: ClipBehavior,
    ) {
        unsafe {
            sys::rf_layer_tree_push_clip_rounded_rect(
                self.raw,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                radius_x,
                radius_y,
                behavior.code(),
            )
        };
    }

    pub fn push_clip_path(&mut self, path: &RenderPath, behavior: ClipBehavior) {
        unsafe { sys::rf_layer_tree_push_clip_path(self.raw, path.raw, behavior.code()) };
    }

    /// Group opacity. `alpha` is 0..255.
    pub fn push_opacity(&mut self, alpha: u8, offset_x: f32, offset_y: f32) {
        unsafe { sys::rf_layer_tree_push_opacity(self.raw, alpha, offset_x, offset_y) };
    }

    /// Blurs whatever is already painted behind this layer -- frosted glass.
    pub fn push_backdrop_blur(&mut self, sigma_x: f32, sigma_y: f32) {
        unsafe { sys::rf_layer_tree_push_backdrop_blur(self.raw, sigma_x, sigma_y) };
    }

    /// Blurs this layer's own subtree.
    pub fn push_blur(&mut self, sigma_x: f32, sigma_y: f32) {
        unsafe { sys::rf_layer_tree_push_blur(self.raw, sigma_x, sigma_y) };
    }

    /// Closes the innermost open layer. Popping past the root is ignored.
    pub fn pop(&mut self) {
        unsafe { sys::rf_layer_tree_pop(self.raw) };
    }
}

// -- Colour spaces (upstream colors.dart) --------------------------------------

/// Upstream `_getHue`: the hue of an RGB triple, 0 at red, NaN folded to 0
/// for greys.
fn get_hue(red: f32, green: f32, blue: f32, max: f32, delta: f32) -> f32 {
    let hue = if max == 0.0 {
        0.0
    } else if max == red {
        60.0 * (((green - blue) / delta) % 6.0)
    } else if max == green {
        60.0 * (((blue - red) / delta) + 2.0)
    } else {
        60.0 * (((red - green) / delta) + 4.0)
    };
    if hue.is_nan() { 0.0 } else { hue }
}

/// Upstream `_colorFromHue`.
fn color_from_hue(alpha: f32, hue: f32, chroma: f32, secondary: f32, match_value: f32) -> Color {
    let channel = |value: f32| ((value + match_value) * 255.0).round().clamp(0.0, 255.0) as u8;
    let (red, green, blue) = if hue < 60.0 {
        (chroma, secondary, 0.0)
    } else if hue < 120.0 {
        (secondary, chroma, 0.0)
    } else if hue < 180.0 {
        (0.0, chroma, secondary)
    } else if hue < 240.0 {
        (0.0, secondary, chroma)
    } else if hue < 300.0 {
        (secondary, 0.0, chroma)
    } else {
        (chroma, 0.0, secondary)
    };
    Color::argb(
        (alpha * 255.0).round().clamp(0.0, 255.0) as u8,
        channel(red),
        channel(green),
        channel(blue),
    )
}

/// A colour in alpha/hue/saturation/value -- pigment space, upstream
/// `HSVColor`. Picking and interpolating here reads better to the eye than
/// RGB channels do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HSVColor {
    pub alpha: f32,
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
}

impl HSVColor {
    pub fn from_ahsv(alpha: f32, hue: f32, saturation: f32, value: f32) -> HSVColor {
        debug_assert!((0.0..=1.0).contains(&alpha));
        debug_assert!((0.0..=360.0).contains(&hue));
        debug_assert!((0.0..=1.0).contains(&saturation));
        debug_assert!((0.0..=1.0).contains(&value));
        HSVColor {
            alpha,
            hue,
            saturation,
            value,
        }
    }

    /// Upstream `HSVColor.fromColor` (round-trips only approximately).
    pub fn from_color(color: Color) -> HSVColor {
        let red = color.red() as f32 / 255.0;
        let green = color.green() as f32 / 255.0;
        let blue = color.blue() as f32 / 255.0;
        let max = red.max(green.max(blue));
        let min = red.min(green.min(blue));
        let delta = max - min;
        HSVColor::from_ahsv(
            color.alpha() as f32 / 255.0,
            get_hue(red, green, blue, max, delta),
            if max == 0.0 { 0.0 } else { delta / max },
            max,
        )
    }

    pub fn with_alpha(mut self, alpha: f32) -> HSVColor {
        self.alpha = alpha;
        self
    }

    pub fn with_hue(mut self, hue: f32) -> HSVColor {
        self.hue = hue;
        self
    }

    pub fn with_saturation(mut self, saturation: f32) -> HSVColor {
        self.saturation = saturation;
        self
    }

    pub fn with_value(mut self, value: f32) -> HSVColor {
        self.value = value;
        self
    }

    /// Upstream `HSVColor.toColor`.
    pub fn to_color(self) -> Color {
        let chroma = self.saturation * self.value;
        let secondary = chroma * (1.0 - (((self.hue / 60.0) % 2.0) - 1.0).abs());
        let match_value = self.value - chroma;
        color_from_hue(self.alpha, self.hue, chroma, secondary, match_value)
    }

    /// Upstream `HSVColor.lerp`: each channel separately, the hue wrapping,
    /// a missing side a transparent instance of the other.
    pub fn lerp(a: Option<HSVColor>, b: Option<HSVColor>, t: f32) -> Option<HSVColor> {
        if a == b {
            return a;
        }
        let scale_alpha = |color: HSVColor, factor: f32| color.with_alpha(color.alpha * factor);
        match (a, b) {
            (None, Some(b)) => Some(scale_alpha(b, t)),
            (Some(a), None) => Some(scale_alpha(a, 1.0 - t)),
            (Some(a), Some(b)) => Some(HSVColor::from_ahsv(
                (a.alpha + (b.alpha - a.alpha) * t).clamp(0.0, 1.0),
                (a.hue + (b.hue - a.hue) * t).rem_euclid(360.0),
                (a.saturation + (b.saturation - a.saturation) * t).clamp(0.0, 1.0),
                (a.value + (b.value - a.value) * t).clamp(0.0, 1.0),
            )),
            (None, None) => None,
        }
    }
}

/// A colour in alpha/hue/saturation/lightness -- coloured-light space,
/// upstream `HSLColor`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HSLColor {
    pub alpha: f32,
    pub hue: f32,
    pub saturation: f32,
    pub lightness: f32,
}

impl HSLColor {
    pub fn from_ahsl(alpha: f32, hue: f32, saturation: f32, lightness: f32) -> HSLColor {
        debug_assert!((0.0..=1.0).contains(&alpha));
        debug_assert!((0.0..=360.0).contains(&hue));
        debug_assert!((0.0..=1.0).contains(&saturation));
        debug_assert!((0.0..=1.0).contains(&lightness));
        HSLColor {
            alpha,
            hue,
            saturation,
            lightness,
        }
    }

    /// Upstream `HSLColor.fromColor`.
    pub fn from_color(color: Color) -> HSLColor {
        let red = color.red() as f32 / 255.0;
        let green = color.green() as f32 / 255.0;
        let blue = color.blue() as f32 / 255.0;
        let max = red.max(green.max(blue));
        let min = red.min(green.min(blue));
        let delta = max - min;
        let lightness = (max + min) / 2.0;
        // Rounding can push saturation past one, so it is clamped.
        let saturation = if min == max {
            0.0
        } else {
            (delta / (1.0 - (2.0 * lightness - 1.0).abs())).clamp(0.0, 1.0)
        };
        HSLColor::from_ahsl(
            color.alpha() as f32 / 255.0,
            get_hue(red, green, blue, max, delta),
            saturation,
            lightness,
        )
    }

    pub fn with_alpha(mut self, alpha: f32) -> HSLColor {
        self.alpha = alpha;
        self
    }

    pub fn with_hue(mut self, hue: f32) -> HSLColor {
        self.hue = hue;
        self
    }

    pub fn with_saturation(mut self, saturation: f32) -> HSLColor {
        self.saturation = saturation;
        self
    }

    pub fn with_lightness(mut self, lightness: f32) -> HSLColor {
        self.lightness = lightness;
        self
    }

    /// Upstream `HSLColor.toColor`.
    pub fn to_color(self) -> Color {
        let chroma = (1.0 - (2.0 * self.lightness - 1.0).abs()) * self.saturation;
        let secondary = chroma * (1.0 - (((self.hue / 60.0) % 2.0) - 1.0).abs());
        let match_value = self.lightness - chroma / 2.0;
        color_from_hue(self.alpha, self.hue, chroma, secondary, match_value)
    }

    /// Upstream `HSLColor.lerp`.
    pub fn lerp(a: Option<HSLColor>, b: Option<HSLColor>, t: f32) -> Option<HSLColor> {
        if a == b {
            return a;
        }
        let scale_alpha = |color: HSLColor, factor: f32| color.with_alpha(color.alpha * factor);
        match (a, b) {
            (None, Some(b)) => Some(scale_alpha(b, t)),
            (Some(a), None) => Some(scale_alpha(a, 1.0 - t)),
            (Some(a), Some(b)) => Some(HSLColor::from_ahsl(
                (a.alpha + (b.alpha - a.alpha) * t).clamp(0.0, 1.0),
                (a.hue + (b.hue - a.hue) * t).rem_euclid(360.0),
                (a.saturation + (b.saturation - a.saturation) * t).clamp(0.0, 1.0),
                (a.lightness + (b.lightness - a.lightness) * t).clamp(0.0, 1.0),
            )),
            (None, None) => None,
        }
    }
}

/// A colour with a small table of related colours, upstream `ColorSwatch`:
/// the primary colour is the swatch's own value, and the shades hang off it
/// by key. `MaterialColor` and friends are spellings of this.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorSwatch<T: Eq + std::hash::Hash + Clone> {
    pub primary: Color,
    swatch: std::collections::HashMap<T, Color>,
}

impl<T: Eq + std::hash::Hash + Clone> ColorSwatch<T> {
    pub fn new(primary: Color, shades: impl IntoIterator<Item = (T, Color)>) -> ColorSwatch<T> {
        ColorSwatch {
            primary,
            swatch: shades.into_iter().collect(),
        }
    }

    /// The upstream `operator []`.
    pub fn get(&self, key: &T) -> Option<Color> {
        self.swatch.get(key).copied()
    }

    pub fn keys(&self) -> impl Iterator<Item = &T> {
        self.swatch.keys()
    }
}

// -- Text scaling (upstream text_scaler.dart) -----------------------------------

/// How font sizes scale for readability, upstream `TextScaler`. The engine's
/// platform setting arrives as a bare factor, so the linear spelling is the
/// only one there is today -- non-linear platform scalers would arrive as a
/// new variant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextScaler {
    pub text_scale_factor: f32,
}

impl TextScaler {
    pub const NO_SCALING: TextScaler = TextScaler {
        text_scale_factor: 1.0,
    };

    pub const fn linear(text_scale_factor: f32) -> TextScaler {
        TextScaler { text_scale_factor }
    }

    /// Upstream `TextScaler.scale`.
    pub fn scale(&self, font_size: f32) -> f32 {
        font_size * self.text_scale_factor
    }
}

impl Default for TextScaler {
    fn default() -> TextScaler {
        TextScaler::NO_SCALING
    }
}

// -- Geometric gradients (upstream gradient.dart) --------------------------------

use crate::direction::TextDirection;
use crate::render::{Alignment, AlignmentGeometry};

/// An affine 2D transform, the 2D slice of upstream's `Matrix4`. Gradients
/// carry one to rotate the ramp without rotating the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// The rotation about `center` that upstream's `GradientRotation`
    /// builds as a translation-then-rotation matrix.
    pub fn rotation_about_center(center: (f32, f32), radians: f32) -> Affine {
        let (sin_r, cos_r) = radians.sin_cos();
        let one_minus_cos = 1.0 - cos_r;
        Affine {
            m11: cos_r,
            m12: -sin_r,
            m21: sin_r,
            m22: cos_r,
            tx: sin_r * center.1 + one_minus_cos * center.0,
            ty: -sin_r * center.0 + one_minus_cos * center.1,
        }
    }

    pub fn map_point(&self, point: (f32, f32)) -> (f32, f32) {
        (
            self.m11 * point.0 + self.m12 * point.1 + self.tx,
            self.m21 * point.0 + self.m22 * point.1 + self.ty,
        )
    }

    /// How much this transform scales a radius: the largest singular value
    /// of the 2x2 part (an over-estimate for shears, exact for rotations
    /// and uniform scales, which is all the gradient transforms here are).
    pub fn max_scale(&self) -> f32 {
        let scale_x = (self.m11 * self.m11 + self.m21 * self.m21).sqrt();
        let scale_y = (self.m12 * self.m12 + self.m22 * self.m22).sqrt();
        scale_x.max(scale_y)
    }
}

/// Upstream `GradientTransform`/`GradientRotation`: the closed set is one
/// transform, a rotation; the enum keeps room for the skews and scales
/// upstream's abstract class leaves open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientTransform {
    Rotation { radians: f32 },
}

impl GradientTransform {
    /// Upstream `GradientRotation.transform`.
    pub fn transform(&self, bounds: Rect) -> Affine {
        match *self {
            GradientTransform::Rotation { radians } => Affine::rotation_about_center(
                (
                    (bounds.left + bounds.right) / 2.0,
                    (bounds.top + bounds.bottom) / 2.0,
                ),
                radians,
            ),
        }
    }
}

/// Upstream `_sample`: the colour a gradient's ramp shows at a fractional
/// stop, lerping inside the segment that contains it.
fn sample_gradient(colors: &[Color], stops: &[f32], t: f32) -> Color {
    if t <= stops[0] {
        return colors[0];
    }
    for index in 0..stops.len() - 1 {
        if t < stops[index + 1] {
            let span = stops[index + 1] - stops[index];
            let fraction = if span <= 0.0 {
                0.0
            } else {
                (t - stops[index]) / span
            };
            return crate::borders::color_lerp(colors[index], colors[index + 1], fraction);
        }
    }
    colors[colors.len() - 1]
}

/// Upstream `_interpolateColorsAndStops`: the union of both stop lists, each
/// stop's colour sampled from both ramps and lerped.
fn interpolate_colors_and_stops(
    a_colors: &[Color],
    a_stops: &[f32],
    b_colors: &[Color],
    b_stops: &[f32],
    t: f32,
) -> (Vec<Color>, Vec<f32>) {
    // The union, sorted -- `SplayTreeSet<double>` upstream. Stops are finite
    // fractions in 0..=1, so a bit-pattern key gives a safe total order.
    let stops: Vec<f32> = a_stops
        .iter()
        .chain(b_stops.iter())
        .map(|stop| stop.to_bits() as i64)
        .collect::<std::collections::BTreeSet<i64>>()
        .into_iter()
        .map(|key| f32::from_bits(key as u32))
        .collect();
    let colors = stops
        .iter()
        .map(|stop| {
            crate::borders::color_lerp(
                sample_gradient(a_colors, a_stops, *stop),
                sample_gradient(b_colors, b_stops, *stop),
                t,
            )
        })
        .collect();
    (colors, stops)
}

/// A gradient with geometry, upstream `LinearGradient`: two anchors in
/// alignment space, a ramp between them.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearGradient {
    pub begin: AlignmentGeometry,
    pub end: AlignmentGeometry,
    pub colors: Vec<Color>,
    pub stops: Option<Vec<f32>>,
    pub tile_mode: TileMode,
    pub transform: Option<GradientTransform>,
}

impl LinearGradient {
    pub fn new(colors: &[Color]) -> LinearGradient {
        LinearGradient {
            begin: AlignmentGeometry::Absolute(Alignment::CENTER_LEFT),
            end: AlignmentGeometry::Absolute(Alignment::CENTER_RIGHT),
            colors: colors.to_vec(),
            stops: None,
            tile_mode: TileMode::Clamp,
            transform: None,
        }
    }

    pub fn with_begin(mut self, begin: AlignmentGeometry) -> LinearGradient {
        self.begin = begin;
        self
    }

    pub fn with_end(mut self, end: AlignmentGeometry) -> LinearGradient {
        self.end = end;
        self
    }

    pub fn with_stops(mut self, stops: &[f32]) -> LinearGradient {
        self.stops = Some(stops.to_vec());
        self
    }

    pub fn with_tile_mode(mut self, tile_mode: TileMode) -> LinearGradient {
        self.tile_mode = tile_mode;
        self
    }

    pub fn with_transform(mut self, transform: GradientTransform) -> LinearGradient {
        self.transform = Some(transform);
        self
    }

    /// Upstream `Gradient._impliedStops`.
    pub fn implied_stops(&self) -> Vec<f32> {
        match &self.stops {
            Some(stops) => stops.clone(),
            None => {
                let separation = 1.0 / (self.colors.len() as f32 - 1.0);
                (0..self.colors.len())
                    .map(|index| index as f32 * separation)
                    .collect()
            }
        }
    }

    /// Upstream `LinearGradient.scale`: the geometry holds, every colour's
    /// alpha scales.
    pub fn scale(&self, factor: f32) -> LinearGradient {
        LinearGradient {
            colors: scale_color_alphas(&self.colors, factor),
            ..self.clone()
        }
    }

    /// Upstream `Gradient.fromColor`: the geometry held, the ramp one colour.
    pub fn from_color(&self, color: Color) -> LinearGradient {
        LinearGradient {
            colors: vec![color; self.colors.len()],
            ..self.clone()
        }
    }

    /// Upstream `LinearGradient.lerp`.
    pub fn lerp(a: Option<&LinearGradient>, b: Option<&LinearGradient>, t: f32) -> LinearGradient {
        match (a, b) {
            (None, Some(b)) => b.scale(t),
            (Some(a), None) => a.scale(1.0 - t),
            (Some(a), Some(b)) => {
                let (colors, stops) = interpolate_colors_and_stops(
                    &a.colors,
                    &a.implied_stops(),
                    &b.colors,
                    &b.implied_stops(),
                    t,
                );
                LinearGradient {
                    begin: AlignmentGeometry::lerp(Some(a.begin), Some(b.begin), t)
                        .unwrap_or(a.begin),
                    end: AlignmentGeometry::lerp(Some(a.end), Some(b.end), t).unwrap_or(a.end),
                    colors,
                    stops: Some(stops),
                    tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
                    transform: if t < 0.5 { a.transform } else { b.transform },
                }
            }
            (None, None) => LinearGradient::new(&[Color::TRANSPARENT, Color::TRANSPARENT]),
        }
    }

    /// Upstream `createShader`, resolved into the paint the engine wants.
    /// The shader-space transform is baked into the anchor points -- an
    /// affine map of a line's endpoints is the same line's gradient.
    pub fn to_paint(&self, rect: Rect, direction: TextDirection) -> Paint {
        let affine = self
            .transform
            .map(|transform| transform.transform(rect))
            .unwrap_or(Affine::IDENTITY);
        let map = |alignment: AlignmentGeometry| -> (f32, f32) {
            let point = alignment.resolve(direction).within_rect(rect);
            affine.map_point((point.dx, point.dy))
        };
        let begin = map(self.begin);
        let end = map(self.end);
        let ramp = self.ramp();
        Paint::new(self.colors[0]).with_linear_gradient(begin, end, &ramp)
    }

    fn ramp(&self) -> Gradient {
        let mut ramp = Gradient::new(&self.colors).with_tile_mode(self.tile_mode);
        if let Some(stops) = &self.stops {
            ramp = ramp.with_stops(stops);
        }
        ramp
    }
}

/// A gradient in concentric circles, upstream `RadialGradient`. The focal
/// point (`RadialGradient.focal`/`focalRadius`) has no engine spelling and
/// is carried but not yet painted -- see PORTING_STATUS.
#[derive(Clone, Debug, PartialEq)]
pub struct RadialGradient {
    pub center: AlignmentGeometry,
    pub radius: f32,
    pub colors: Vec<Color>,
    pub stops: Option<Vec<f32>>,
    pub tile_mode: TileMode,
    pub focal: Option<AlignmentGeometry>,
    pub focal_radius: f32,
    pub transform: Option<GradientTransform>,
}

impl RadialGradient {
    pub fn new(colors: &[Color]) -> RadialGradient {
        RadialGradient {
            center: AlignmentGeometry::Absolute(Alignment::CENTER),
            radius: 0.5,
            colors: colors.to_vec(),
            stops: None,
            tile_mode: TileMode::Clamp,
            focal: None,
            focal_radius: 0.0,
            transform: None,
        }
    }

    pub fn with_center(mut self, center: AlignmentGeometry) -> RadialGradient {
        self.center = center;
        self
    }

    pub fn with_radius(mut self, radius: f32) -> RadialGradient {
        self.radius = radius;
        self
    }

    pub fn with_stops(mut self, stops: &[f32]) -> RadialGradient {
        self.stops = Some(stops.to_vec());
        self
    }

    pub fn with_tile_mode(mut self, tile_mode: TileMode) -> RadialGradient {
        self.tile_mode = tile_mode;
        self
    }

    pub fn with_transform(mut self, transform: GradientTransform) -> RadialGradient {
        self.transform = Some(transform);
        self
    }

    pub fn implied_stops(&self) -> Vec<f32> {
        match &self.stops {
            Some(stops) => stops.clone(),
            None => {
                let separation = 1.0 / (self.colors.len() as f32 - 1.0);
                (0..self.colors.len())
                    .map(|index| index as f32 * separation)
                    .collect()
            }
        }
    }

    pub fn scale(&self, factor: f32) -> RadialGradient {
        RadialGradient {
            colors: scale_color_alphas(&self.colors, factor),
            ..self.clone()
        }
    }

    pub fn lerp(a: Option<&RadialGradient>, b: Option<&RadialGradient>, t: f32) -> RadialGradient {
        match (a, b) {
            (None, Some(b)) => b.scale(t),
            (Some(a), None) => a.scale(1.0 - t),
            (Some(a), Some(b)) => {
                let (colors, stops) = interpolate_colors_and_stops(
                    &a.colors,
                    &a.implied_stops(),
                    &b.colors,
                    &b.implied_stops(),
                    t,
                );
                RadialGradient {
                    center: AlignmentGeometry::lerp(Some(a.center), Some(b.center), t)
                        .unwrap_or(a.center),
                    radius: a.radius + (b.radius - a.radius) * t,
                    colors,
                    stops: Some(stops),
                    tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
                    focal: match (a.focal, b.focal) {
                        (Some(a), Some(b)) => {
                            AlignmentGeometry::lerp(Some(a), Some(b), t).or(Some(a))
                        }
                        (None, Some(b)) => Some(b),
                        (Some(a), None) => Some(a),
                        (None, None) => None,
                    },
                    focal_radius: a.focal_radius + (b.focal_radius - a.focal_radius) * t,
                    transform: if t < 0.5 { a.transform } else { b.transform },
                }
            }
            (None, None) => RadialGradient::new(&[Color::TRANSPARENT, Color::TRANSPARENT]),
        }
    }

    /// Upstream `createShader`; the transform moves the centre and scales
    /// the radius, which is exact for the rotations the enum holds.
    pub fn to_paint(&self, rect: Rect, direction: TextDirection) -> Paint {
        let affine = self
            .transform
            .map(|transform| transform.transform(rect))
            .unwrap_or(Affine::IDENTITY);
        let center_point = self.center.resolve(direction).within_rect(rect);
        let center = affine.map_point((center_point.dx, center_point.dy));
        // The radius is in alignment units; the shortest side is the
        // yardstick upstream's `createShader` maps it with.
        let radius = self.radius * rect_shortest_side(rect) / 2.0 * affine.max_scale();
        let mut ramp = Gradient::new(&self.colors).with_tile_mode(self.tile_mode);
        if let Some(stops) = &self.stops {
            ramp = ramp.with_stops(stops);
        }
        Paint::new(self.colors[0]).with_radial_gradient(center, radius, &ramp)
    }
}

/// A gradient sweeping around a centre, upstream `SweepGradient`.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepGradient {
    pub center: AlignmentGeometry,
    /// Radians, clockwise from three o'clock.
    pub start_angle: f32,
    pub end_angle: f32,
    pub colors: Vec<Color>,
    pub stops: Option<Vec<f32>>,
    pub tile_mode: TileMode,
    pub transform: Option<GradientTransform>,
}

impl SweepGradient {
    pub fn new(colors: &[Color]) -> SweepGradient {
        SweepGradient {
            center: AlignmentGeometry::Absolute(Alignment::CENTER),
            start_angle: 0.0,
            end_angle: std::f32::consts::TAU,
            colors: colors.to_vec(),
            stops: None,
            tile_mode: TileMode::Clamp,
            transform: None,
        }
    }

    pub fn with_center(mut self, center: AlignmentGeometry) -> SweepGradient {
        self.center = center;
        self
    }

    pub fn with_angles(mut self, start_radians: f32, end_radians: f32) -> SweepGradient {
        self.start_angle = start_radians;
        self.end_angle = end_radians;
        self
    }

    pub fn with_stops(mut self, stops: &[f32]) -> SweepGradient {
        self.stops = Some(stops.to_vec());
        self
    }

    pub fn with_tile_mode(mut self, tile_mode: TileMode) -> SweepGradient {
        self.tile_mode = tile_mode;
        self
    }

    pub fn with_transform(mut self, transform: GradientTransform) -> SweepGradient {
        self.transform = Some(transform);
        self
    }

    pub fn implied_stops(&self) -> Vec<f32> {
        match &self.stops {
            Some(stops) => stops.clone(),
            None => {
                let separation = 1.0 / (self.colors.len() as f32 - 1.0);
                (0..self.colors.len())
                    .map(|index| index as f32 * separation)
                    .collect()
            }
        }
    }

    pub fn scale(&self, factor: f32) -> SweepGradient {
        SweepGradient {
            colors: scale_color_alphas(&self.colors, factor),
            ..self.clone()
        }
    }

    pub fn lerp(a: Option<&SweepGradient>, b: Option<&SweepGradient>, t: f32) -> SweepGradient {
        match (a, b) {
            (None, Some(b)) => b.scale(t),
            (Some(a), None) => a.scale(1.0 - t),
            (Some(a), Some(b)) => {
                let (colors, stops) = interpolate_colors_and_stops(
                    &a.colors,
                    &a.implied_stops(),
                    &b.colors,
                    &b.implied_stops(),
                    t,
                );
                SweepGradient {
                    center: AlignmentGeometry::lerp(Some(a.center), Some(b.center), t)
                        .unwrap_or(a.center),
                    start_angle: a.start_angle + (b.start_angle - a.start_angle) * t,
                    end_angle: a.end_angle + (b.end_angle - a.end_angle) * t,
                    colors,
                    stops: Some(stops),
                    tile_mode: if t < 0.5 { a.tile_mode } else { b.tile_mode },
                    transform: if t < 0.5 { a.transform } else { b.transform },
                }
            }
            (None, None) => SweepGradient::new(&[Color::TRANSPARENT, Color::TRANSPARENT]),
        }
    }

    /// Upstream `createShader`; a rotation transform shifts the sweep's
    /// angles with it. The engine speaks degrees.
    pub fn to_paint(&self, rect: Rect, direction: TextDirection) -> Paint {
        let shift = match self.transform {
            Some(GradientTransform::Rotation { radians }) => radians,
            None => 0.0,
        };
        let center_point = self.center.resolve(direction).within_rect(rect);
        let mut ramp = Gradient::new(&self.colors).with_tile_mode(self.tile_mode);
        if let Some(stops) = &self.stops {
            ramp = ramp.with_stops(stops);
        }
        Paint::new(self.colors[0]).with_sweep_gradient(
            (center_point.dx, center_point.dy),
            (self.start_angle + shift).to_degrees(),
            (self.end_angle + shift).to_degrees(),
            &ramp,
        )
    }
}

/// Upstream `Gradient`'s hierarchy root: any one gradient, with the lerp
/// discipline of `Gradient.lerp` -- same kinds interpolate as themselves,
/// different kinds fade out and in over the two halves.
#[derive(Clone, Debug, PartialEq)]
pub enum ShaderGradient {
    Linear(LinearGradient),
    Radial(RadialGradient),
    Sweep(SweepGradient),
}

impl ShaderGradient {
    pub fn scale(&self, factor: f32) -> ShaderGradient {
        match self {
            ShaderGradient::Linear(gradient) => ShaderGradient::Linear(gradient.scale(factor)),
            ShaderGradient::Radial(gradient) => ShaderGradient::Radial(gradient.scale(factor)),
            ShaderGradient::Sweep(gradient) => ShaderGradient::Sweep(gradient.scale(factor)),
        }
    }

    pub fn from_color(&self, color: Color) -> ShaderGradient {
        // A uniform-colour gradient paints as a solid regardless of geometry,
        // so the linear spelling is enough -- upstream's own default.
        let (stops, tile_mode, transform, count) = match self {
            ShaderGradient::Linear(gradient) => {
                return ShaderGradient::Linear(gradient.from_color(color));
            }
            ShaderGradient::Radial(gradient) => (
                gradient.stops.clone(),
                gradient.tile_mode,
                gradient.transform,
                gradient.colors.len(),
            ),
            ShaderGradient::Sweep(gradient) => (
                gradient.stops.clone(),
                gradient.tile_mode,
                gradient.transform,
                gradient.colors.len(),
            ),
        };
        ShaderGradient::Linear(LinearGradient {
            begin: AlignmentGeometry::CENTER,
            end: AlignmentGeometry::Absolute(Alignment::CENTER_RIGHT),
            colors: vec![color; count],
            stops,
            tile_mode,
            transform,
        })
    }

    /// Upstream `Gradient.lerp`.
    pub fn lerp(
        a: Option<ShaderGradient>,
        b: Option<ShaderGradient>,
        t: f32,
    ) -> Option<ShaderGradient> {
        if a == b {
            return a;
        }
        match (a.as_ref(), b.as_ref()) {
            (Some(ShaderGradient::Linear(a)), Some(ShaderGradient::Linear(b))) => Some(
                ShaderGradient::Linear(LinearGradient::lerp(Some(a), Some(b), t)),
            ),
            (Some(ShaderGradient::Radial(a)), Some(ShaderGradient::Radial(b))) => Some(
                ShaderGradient::Radial(RadialGradient::lerp(Some(a), Some(b), t)),
            ),
            (Some(ShaderGradient::Sweep(a)), Some(ShaderGradient::Sweep(b))) => Some(
                ShaderGradient::Sweep(SweepGradient::lerp(Some(a), Some(b), t)),
            ),
            // One side missing: that side's own scale-in path.
            (None, Some(b)) => Some(b.scale(t)),
            (Some(a), None) => Some(a.scale(1.0 - t)),
            (None, None) => None,
            // Different kinds: out then in, over the two halves.
            (Some(a), Some(b)) => {
                if t < 0.5 {
                    Some(a.scale(1.0 - t * 2.0))
                } else {
                    Some(b.scale((t - 0.5) * 2.0))
                }
            }
        }
    }

    pub fn to_paint(&self, rect: Rect, direction: TextDirection) -> Paint {
        match self {
            ShaderGradient::Linear(gradient) => gradient.to_paint(rect, direction),
            ShaderGradient::Radial(gradient) => gradient.to_paint(rect, direction),
            ShaderGradient::Sweep(gradient) => gradient.to_paint(rect, direction),
        }
    }
}

/// The alpha scaling every gradient's `scale` shares: the geometry holds,
/// each colour's alpha multiplies by the factor.
fn scale_color_alphas(colors: &[Color], factor: f32) -> Vec<Color> {
    colors
        .iter()
        .map(|color| {
            Color::argb(
                ((color.alpha() as f32) * factor).round().clamp(0.0, 255.0) as u8,
                color.red(),
                color.green(),
                color.blue(),
            )
        })
        .collect()
}

fn rect_shortest_side(rect: Rect) -> f32 {
    (rect.right - rect.left).min(rect.bottom - rect.top)
}

// -- Matrix4 and MatrixUtils (upstream matrix_utils.dart, vector_math's Matrix4) --

/// A 4x4 matrix in column-major storage, the `Matrix4` upstream gets from
/// `vector_math`. Transforms, gradient shaders and hit-testing all speak it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix4 {
    /// Column-major: `storage[col * 4 + row]`.
    pub storage: [f32; 16],
}

impl Default for Matrix4 {
    fn default() -> Matrix4 {
        Matrix4::IDENTITY
    }
}

impl Matrix4 {
    pub const IDENTITY: Matrix4 = Matrix4 {
        storage: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    pub const fn zero() -> Matrix4 {
        Matrix4 { storage: [0.0; 16] }
    }

    /// `Matrix4.translationValues`.
    pub const fn translation(x: f32, y: f32, z: f32) -> Matrix4 {
        Matrix4 {
            storage: [
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                x, y, z, 1.0,
            ],
        }
    }

    /// `Matrix4.diagonal3Values` -- a scale, one factor per axis.
    pub const fn diagonal3_values(x: f32, y: f32, z: f32) -> Matrix4 {
        Matrix4 {
            storage: [
                x, 0.0, 0.0, 0.0, //
                0.0, y, 0.0, 0.0, //
                0.0, 0.0, z, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    /// `Matrix4.rotationZ` -- the rotation the 2D world cares about.
    pub fn rotation_z(radians: f32) -> Matrix4 {
        let (sin_r, cos_r) = radians.sin_cos();
        Matrix4 {
            storage: [
                cos_r, sin_r, 0.0, 0.0, //
                -sin_r, cos_r, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    /// `Matrix4.rotationX` (the cylindrical projection needs it).
    pub fn rotation_x(radians: f32) -> Matrix4 {
        let (sin_r, cos_r) = radians.sin_cos();
        Matrix4 {
            storage: [
                1.0, 0.0, 0.0, 0.0, //
                0.0, cos_r, sin_r, 0.0, //
                0.0, -sin_r, cos_r, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    /// `Matrix4.rotationY`.
    pub fn rotation_y(radians: f32) -> Matrix4 {
        let (sin_r, cos_r) = radians.sin_cos();
        Matrix4 {
            storage: [
                cos_r, 0.0, -sin_r, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                sin_r, 0.0, cos_r, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    /// `Matrix4.setEntry`.
    pub fn set_entry(mut self, row: usize, column: usize, value: f32) -> Matrix4 {
        self.storage[column * 4 + row] = value;
        self
    }

    /// `a * b`, the `operator *` upstream chains.
    pub fn mul(a: Matrix4, b: Matrix4) -> Matrix4 {
        let mut result = Matrix4::zero();
        for column in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += a.storage[k * 4 + row] * b.storage[column * 4 + k];
                }
                result.storage[column * 4 + row] = sum;
            }
        }
        result
    }

    /// Inverts in place by Gauss-Jordan elimination with partial pivoting;
    /// returns false and leaves a zero matrix when there is no inverse.
    pub fn invert(&mut self) -> bool {
        let mut inverse = Matrix4::IDENTITY.storage;
        let mut m = self.storage;
        for column in 0..4 {
            // Pivot on the largest magnitude in this column, for stability.
            let mut pivot_row = column;
            let mut pivot_mag = m[column * 4 + column].abs();
            for row in (column + 1)..4 {
                let magnitude = m[column * 4 + row].abs();
                if magnitude > pivot_mag {
                    pivot_mag = magnitude;
                    pivot_row = row;
                }
            }
            if pivot_mag == 0.0 {
                self.storage = [0.0; 16];
                return false;
            }
            if pivot_row != column {
                for c in 0..4 {
                    m.swap(c * 4 + column, c * 4 + pivot_row);
                    inverse.swap(c * 4 + column, c * 4 + pivot_row);
                }
            }
            let pivot = m[column * 4 + column];
            for c in 0..4 {
                m[c * 4 + column] /= pivot;
                inverse[c * 4 + column] /= pivot;
            }
            for row in 0..4 {
                if row == column {
                    continue;
                }
                let factor = m[column * 4 + row];
                if factor == 0.0 {
                    continue;
                }
                for c in 0..4 {
                    m[c * 4 + row] -= factor * m[c * 4 + column];
                    inverse[c * 4 + row] -= factor * inverse[c * 4 + column];
                }
            }
        }
        self.storage = inverse;
        true
    }
}

/// Upstream `MatrixUtils`, a bag of statics over [`Matrix4`].
pub mod matrix_utils {
    use super::Matrix4;
    use crate::engine::Rect;
    use crate::render::{Axis, Offset};

    /// `MatrixUtils.getAsTranslation`: the offset, if the matrix is nothing
    /// but a translation.
    pub fn get_as_translation(transform: Matrix4) -> Option<Offset> {
        let s = &transform.storage;
        let is_translation = s[0] == 1.0
            && s[1] == 0.0
            && s[2] == 0.0
            && s[3] == 0.0
            && s[4] == 0.0
            && s[5] == 1.0
            && s[6] == 0.0
            && s[7] == 0.0
            && s[8] == 0.0
            && s[9] == 0.0
            && s[10] == 1.0
            && s[11] == 0.0
            && s[14] == 0.0
            && s[15] == 1.0;
        if is_translation {
            Some(Offset::new(s[12], s[13]))
        } else {
            None
        }
    }

    /// `MatrixUtils.getAsScale`: the uniform 2D scale, if that is all it is.
    pub fn get_as_scale(transform: Matrix4) -> Option<f32> {
        let s = &transform.storage;
        let is_scale = s[1] == 0.0
            && s[2] == 0.0
            && s[3] == 0.0
            && s[4] == 0.0
            && s[6] == 0.0
            && s[7] == 0.0
            && s[8] == 0.0
            && s[9] == 0.0
            && s[10] == 1.0
            && s[11] == 0.0
            && s[12] == 0.0
            && s[13] == 0.0
            && s[14] == 0.0
            && s[15] == 1.0
            && s[0] == s[5];
        if is_scale { Some(s[0]) } else { None }
    }

    /// `MatrixUtils.multiplyInPlace`: `a x b` stored into `b`.
    pub fn multiply_in_place(a: &Matrix4, b: &mut Matrix4) {
        *b = Matrix4::mul(*a, *b);
    }

    /// `MatrixUtils.matrixEquals`: nulls read as the identity.
    pub fn matrix_equals(a: Option<Matrix4>, b: Option<Matrix4>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => a.storage == b.storage,
            (None, Some(b)) => is_identity(b),
            (Some(a), None) => is_identity(a),
            (None, None) => true,
        }
    }

    /// `MatrixUtils.isIdentity`.
    pub fn is_identity(a: Matrix4) -> bool {
        a.storage == Matrix4::IDENTITY.storage
    }

    /// `MatrixUtils.transformPoint`: the point at z=0, projected back to z=0.
    /// May go NaN when the point lands at infinity -- upstream says the same.
    pub fn transform_point(transform: Matrix4, point: Offset) -> Offset {
        let s = &transform.storage;
        let x = point.dx;
        let y = point.dy;
        let rx = s[0] * x + s[4] * y + s[12];
        let ry = s[1] * x + s[5] * y + s[13];
        let rw = s[3] * x + s[7] * y + s[15];
        if rw == 1.0 {
            Offset::new(rx, ry)
        } else {
            Offset::new(rx / rw, ry / rw)
        }
    }

    fn min4(a: f32, b: f32, c: f32, d: f32) -> f32 {
        a.min(b).min(c).min(d)
    }

    fn max4(a: f32, b: f32, c: f32, d: f32) -> f32 {
        a.max(b).max(c).max(d)
    }

    /// `MatrixUtils._safeTransformRect`/`_accumulate`: transform the four
    /// corners, normalizing each, and take the bounds.
    fn safe_transform_rect(transform: Matrix4, rect: Rect) -> Rect {
        let s = &transform.storage;
        let is_affine = s[3] == 0.0 && s[7] == 0.0 && s[15] == 1.0;
        let mut accumulate = |x: f32, y: f32, min_max: &mut [f32; 4], first: bool| {
            let w = if is_affine {
                1.0
            } else {
                1.0 / (s[3] * x + s[7] * y + s[15])
            };
            let tx = (s[0] * x + s[4] * y + s[12]) * w;
            let ty = (s[1] * x + s[5] * y + s[13]) * w;
            if first {
                min_max[0] = tx;
                min_max[1] = ty;
                min_max[2] = tx;
                min_max[3] = ty;
            } else {
                if tx < min_max[0] {
                    min_max[0] = tx;
                }
                if ty < min_max[1] {
                    min_max[1] = ty;
                }
                if tx > min_max[2] {
                    min_max[2] = tx;
                }
                if ty > min_max[3] {
                    min_max[3] = ty;
                }
            }
        };
        let mut min_max = [0.0; 4];
        accumulate(rect.left, rect.top, &mut min_max, true);
        accumulate(rect.right, rect.top, &mut min_max, false);
        accumulate(rect.left, rect.bottom, &mut min_max, false);
        accumulate(rect.right, rect.bottom, &mut min_max, false);
        Rect::ltrb(min_max[0], min_max[1], min_max[2], min_max[3])
    }

    /// `MatrixUtils.transformRect`: the bounding box of the rect under the
    /// matrix, the fast way when there is no perspective term.
    pub fn transform_rect(transform: Matrix4, rect: Rect) -> Rect {
        let s = &transform.storage;
        let x = rect.left;
        let y = rect.top;
        let w = rect.right - x;
        let h = rect.bottom - y;
        // A non-finite rect would turn finite math infinite; the slow path
        // keeps that from happening where it can.
        if !w.is_finite() || !h.is_finite() {
            return safe_transform_rect(transform, rect);
        }

        let wx = s[0] * w;
        let hx = s[4] * h;
        let rx = s[0] * x + s[4] * y + s[12];
        let wy = s[1] * w;
        let hy = s[5] * h;
        let ry = s[1] * x + s[5] * y + s[13];
        if s[3] == 0.0 && s[7] == 0.0 && s[15] == 1.0 {
            // No perspective: a parallelogram whose walls each relative
            // vector pushes one way by its own sign.
            let mut left = rx;
            let mut right = rx;
            if wx < 0.0 {
                left += wx;
            } else {
                right += wx;
            }
            if hx < 0.0 {
                left += hx;
            } else {
                right += hx;
            }
            let mut top = ry;
            let mut bottom = ry;
            if wy < 0.0 {
                top += wy;
            } else {
                bottom += wy;
            }
            if hy < 0.0 {
                top += hy;
            } else {
                bottom += hy;
            }
            Rect::ltrb(left, top, right, bottom)
        } else {
            let ww = s[3] * w;
            let hw = s[7] * h;
            let rw = s[3] * x + s[7] * y + s[15];
            let ulx = rx / rw;
            let uly = ry / rw;
            let urx = (rx + wx) / (rw + ww);
            let ury = (ry + wy) / (rw + ww);
            let llx = (rx + hx) / (rw + hw);
            let lly = (ry + hy) / (rw + hw);
            let lrx = (rx + wx + hx) / (rw + ww + hw);
            let lry = (ry + wy + hy) / (rw + ww + hw);
            Rect::ltrb(
                min4(ulx, urx, llx, lrx),
                min4(uly, ury, lly, lry),
                max4(ulx, urx, llx, lrx),
                max4(uly, ury, lly, lry),
            )
        }
    }

    /// `MatrixUtils.inverseTransformRect`.
    pub fn inverse_transform_rect(transform: Matrix4, rect: Rect) -> Rect {
        if is_identity(transform) {
            return rect;
        }
        let mut inverse = transform;
        inverse.invert();
        transform_rect(inverse, rect)
    }

    /// `MatrixUtils.createCylindricalProjectionTransform`: perspective *
    /// view * model, the wrap-a-plane-around-a-cylinder matrix.
    pub fn create_cylindrical_projection_transform(
        radius: f32,
        angle: f32,
        perspective: f32,
        orientation: Axis,
    ) -> Matrix4 {
        debug_assert!((0.0..=1.0).contains(&perspective));
        // Perspective * view, pre-multiplied.
        let mut result = Matrix4::IDENTITY
            .set_entry(3, 2, -perspective)
            .set_entry(2, 3, -radius)
            .set_entry(3, 3, perspective * radius + 1.0);
        // Model: translate out by the radius, then rotate against the world.
        let rotation = match orientation {
            Axis::Horizontal => Matrix4::rotation_y(angle),
            Axis::Vertical => Matrix4::rotation_x(angle),
        };
        result = Matrix4::mul(
            result,
            Matrix4::mul(rotation, Matrix4::translation(0.0, 0.0, radius)),
        );
        result
    }

    /// `MatrixUtils.forceToPoint`: every point lands on `offset`.
    pub fn force_to_point(offset: Offset) -> Matrix4 {
        Matrix4::zero()
            .set_entry(2, 2, 1.0)
            .set_entry(0, 3, offset.dx)
            .set_entry(1, 3, offset.dy)
            .set_entry(3, 3, 1.0)
    }
}

// -- Text measurement and painting (upstream text_painter.dart, strut_style.dart, inline_span.dart) --

/// Upstream `TextPainter`: the object form of this module's `shape` family.
/// Lay it out at a width, ask it what it measured, paint it where it goes.
///
/// Divergence: the engine exposes no minimum-intrinsic width and no
/// per-placeholder geometry, so [`TextPainter::min_intrinsic_width`] answers
/// with the longest line and [`PlaceholderDimensions`] is carried data.
pub struct TextPainter {
    runs: Vec<(String, TextStyle)>,
    align: TextAlign,
    max_lines: Option<usize>,
    ellipsis: bool,
    scale: f32,
    paragraph: Option<Rc<Paragraph>>,
}

impl Default for TextPainter {
    fn default() -> TextPainter {
        TextPainter {
            runs: Vec::new(),
            align: TextAlign::Start,
            max_lines: None,
            ellipsis: false,
            scale: 1.0,
            paragraph: None,
        }
    }
}

impl TextPainter {
    pub fn new() -> TextPainter {
        TextPainter::default()
    }

    pub fn text(mut self, text: impl Into<String>, style: TextStyle) -> TextPainter {
        self.runs.push((text.into(), style));
        self
    }

    pub fn with_align(mut self, align: TextAlign) -> TextPainter {
        self.align = align;
        self
    }

    pub fn with_max_lines(mut self, max_lines: Option<usize>) -> TextPainter {
        self.max_lines = max_lines;
        self
    }

    pub fn with_ellipsis(mut self, ellipsis: bool) -> TextPainter {
        self.ellipsis = ellipsis;
        self
    }

    pub fn with_scale(mut self, scale: f32) -> TextPainter {
        self.scale = scale;
        self
    }

    /// Upstream `TextPainter.layout`: shape at `max_width` and keep the
    /// answer.
    pub fn layout(&mut self, max_width: f32) {
        self.paragraph = Some(shape_rich(
            &self.runs,
            self.align,
            self.max_lines,
            self.ellipsis,
            max_width,
            self.scale,
        ));
    }

    fn paragraph(&self) -> &Rc<Paragraph> {
        self.paragraph
            .as_ref()
            .expect("layout before asking a laid-out question")
    }

    /// Upstream `TextPainter.width`.
    pub fn width(&self) -> f32 {
        self.paragraph().width()
    }

    /// Upstream `TextPainter.height`.
    pub fn height(&self) -> f32 {
        self.paragraph().height()
    }

    /// Upstream `TextPainter.minIntrinsicWidth` -- the engine answers only
    /// the longest line, which is the maximum; see the type's docs.
    pub fn min_intrinsic_width(&self, _max_width: f32) -> f32 {
        self.paragraph().longest_line()
    }

    /// Upstream `TextPainter.maxIntrinsicWidth`.
    pub fn max_intrinsic_width(&self, _max_width: f32) -> f32 {
        self.paragraph().longest_line()
    }

    /// Upstream `TextPainter.paint`.
    pub fn paint(&self, canvas: &mut Canvas, offset: (f32, f32)) {
        canvas.draw_paragraph(self.paragraph(), offset.0, offset.1);
    }
}

/// Upstream `PlaceholderDimensions`: the size a placeholder span occupies
/// and how it sits against the text baseline. The shaper here flattens
/// runs, so nothing consumes this yet; it is the data the engine slot needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaceholderDimensions {
    pub size: (f32, f32),
    pub alignment: PlaceholderAlignment,
    pub baseline: TextBaseline,
}

/// Upstream `PlaceholderAlignment`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaceholderAlignment {
    #[default]
    Baseline,
    AboveBaseline,
    BelowBaseline,
    Top,
    Bottom,
    Middle,
}

/// Upstream `TextBaseline`, the pair of baselines text can align on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextBaseline {
    /// The line the letters sit on.
    #[default]
    Alphabetic,
    /// The hanging line scripts like Devanagari hang from.
    Ideographic,
}

/// Upstream `WordBoundary`: the text on either side of a word edge.
#[derive(Clone, Debug, PartialEq)]
pub struct WordBoundary {
    pub prefix: String,
    pub suffix: String,
}

/// Upstream `StrutStyle`: a line-height floor the paragraph enforces before
/// any of its own styles count. The engine's paragraph builder takes no
/// strut, so this is carried configuration until that lands -- see
/// PORTING_STATUS.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StrutStyle {
    pub strut_enabled: bool,
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub height: Option<f32>,
    pub leading: Option<f32>,
    pub force_strut_height: bool,
}

impl StrutStyle {
    pub fn new(strut_enabled: bool) -> StrutStyle {
        StrutStyle {
            strut_enabled,
            ..StrutStyle::default()
        }
    }
}

/// Upstream `Accumulator`: a running index through an inline-span walk.
#[derive(Clone, Copy, Debug, Default)]
pub struct Accumulator {
    pub value: i32,
}

impl Accumulator {
    pub fn increment(&mut self, addend: i32) {
        self.value += addend;
    }
}

/// Upstream `InlineSpanSemanticsInformation`: what semantics says about one
/// span.
///
/// # When a span becomes its own thing to a reader
///
/// Upstream computes it in the constructor:
///
/// ```text
/// requiresOwnNode = isPlaceholder || recognizer != null || semanticsIdentifier != null;
/// ```
///
/// Three ways in, and a **`semanticsLabel` is not one of them.** A label
/// changes what the surrounding run of text says; it does not split it. What
/// splits it is being separately *reachable*: a placeholder is a widget in the
/// text and a reader must be able to land on it, a span with a recognizer can
/// be activated and a reader must be able to activate it, and an identifier is
/// something a test or a tool means to find on its own.
///
/// Renaming a stretch of a sentence leaves it one sentence. Making a stretch
/// of it tappable does not.
///
/// # A placeholder is exactly one character, and may say nothing of its own
///
/// The other half of upstream's constructor:
///
/// ```text
/// assert(!isPlaceholder || (text == '\uFFFC' && semanticsLabel == null && recognizer == null));
/// ```
///
/// Its text is the object-replacement character and nothing else, and it can
/// carry neither a label nor a recognizer -- because the widget standing in
/// that slot brings its own semantics, and a second label over the top would
/// be the text layer talking about something it cannot see.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineSpanSemanticsInformation {
    pub text: String,
    pub is_placeholder: bool,
    /// Upstream's `semanticsLabel`: what is said instead of [`Self::text`].
    pub semantics_label: Option<String>,
    /// Upstream's `semanticsIdentifier`, which a test or a tool looks it up by.
    pub semantics_identifier: Option<String>,
    /// Whether a gesture recognizer is attached. The recognizer itself does
    /// not live here -- what semantics needs to know is that there is one.
    pub has_recognizer: bool,
}

impl InlineSpanSemanticsInformation {
    /// Upstream's `PlaceholderSpan.placeholderCodeUnit`, U+FFFC.
    pub const PLACEHOLDER_CHARACTER: char = '\u{FFFC}';

    pub fn text(text: impl Into<String>) -> InlineSpanSemanticsInformation {
        InlineSpanSemanticsInformation {
            text: text.into(),
            ..Default::default()
        }
    }

    pub fn placeholder() -> InlineSpanSemanticsInformation {
        InlineSpanSemanticsInformation {
            text: InlineSpanSemanticsInformation::PLACEHOLDER_CHARACTER.to_string(),
            is_placeholder: true,
            ..Default::default()
        }
    }

    /// The same, saying something else.
    pub fn spoken_as(mut self, label: impl Into<String>) -> InlineSpanSemanticsInformation {
        self.semantics_label = Some(label.into());
        self
    }

    /// The same, with something a reader can activate on it.
    pub fn with_recognizer(mut self) -> InlineSpanSemanticsInformation {
        self.has_recognizer = true;
        self
    }

    /// The same, findable by name.
    pub fn with_identifier(
        mut self,
        identifier: impl Into<String>,
    ) -> InlineSpanSemanticsInformation {
        self.semantics_identifier = Some(identifier.into());
        self
    }

    /// Upstream's `requiresOwnNode` -- see the type's docs for why a label is
    /// not on this list.
    pub fn requires_own_node(&self) -> bool {
        self.is_placeholder || self.has_recognizer || self.semantics_identifier.is_some()
    }

    /// What a reader hears: the label where there is one.
    pub fn spoken(&self) -> &str {
        self.semantics_label.as_deref().unwrap_or(&self.text)
    }

    /// Upstream's placeholder assert, returned rather than panicked so it can
    /// be checked.
    pub fn check(&self) -> Result<(), &'static str> {
        if !self.is_placeholder {
            return Ok(());
        }
        let mut characters = self.text.chars();
        let one = characters.next();
        if one != Some(InlineSpanSemanticsInformation::PLACEHOLDER_CHARACTER)
            || characters.next().is_some()
        {
            return Err("a placeholder's text is U+FFFC and nothing else");
        }
        if self.semantics_label.is_some() || self.has_recognizer {
            return Err(
                "a placeholder carries neither a label nor a recognizer -- the \
                 widget in that slot brings its own",
            );
        }
        Ok(())
    }
}

// -- Decoration images (upstream decoration_image.dart) ---------------------------

use crate::image::{ImageConfiguration, ImageProvider};
use crate::render::{AlignmentGeometry as RenderAlignmentGeometry, BoxFit, Size, apply_box_fit};

/// Upstream `ImageRepeat` (`painting/decoration_image.dart`): which axes an
/// image tiles along to fill its box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageRepeat {
    /// Tile both ways until the box is full.
    Repeat,
    /// Tile across only; leave the rest uncovered.
    RepeatX,
    /// Tile down only.
    RepeatY,
    /// Draw it once and leave the uncovered part transparent. Upstream's
    /// default, and the one every constructor starts from.
    #[default]
    NoRepeat,
}

impl ImageRepeat {
    /// The two questions upstream's tiler actually asks. It never matches on
    /// the enum as four cases -- it asks each axis separately, and `Repeat` is
    /// simply the value that answers yes to both.
    pub fn repeats_x(self) -> bool {
        matches!(self, ImageRepeat::Repeat | ImageRepeat::RepeatX)
    }

    pub fn repeats_y(self) -> bool {
        matches!(self, ImageRepeat::Repeat | ImageRepeat::RepeatY)
    }

    /// Upstream's collapse at the top of `paintImage`:
    ///
    /// ```dart
    /// if (repeat != ImageRepeat.noRepeat && destinationSize == outputSize) {
    ///   // There's no need to repeat the image because we're exactly filling
    ///   // the output rect with the image.
    ///   repeat = ImageRepeat.noRepeat;
    /// }
    /// ```
    ///
    /// An image that already fills its box exactly gains nothing from tiling,
    /// and the collapse is observable rather than cosmetic: it is the
    /// difference between generating one tile rect and generating one *after*
    /// doing the arithmetic for a grid.
    pub fn collapsed_when_exactly_filled(self, exactly_fills: bool) -> ImageRepeat {
        if exactly_fills {
            ImageRepeat::NoRepeat
        } else {
            self
        }
    }

    /// Upstream's `_generateImageTileRects` index range for one axis.
    ///
    /// The two ends are **not** measured from the same edge: the start is
    /// `outputRect.left - fundamentalRect.left` and the stop is
    /// `outputRect.right - fundamentalRect.right`, each against the matching
    /// edge of the tile. Measuring both from the tile's near edge would give a
    /// range one too long whenever the tile overhangs.
    ///
    /// Returns an **inclusive** `(start, stop)`, so an axis that does not
    /// repeat still yields `(0, 0)` -- one tile, not none. `noRepeat` draws
    /// the image; it does not skip it.
    pub fn tile_range(
        repeats: bool,
        output_near: f32,
        output_far: f32,
        tile_near: f32,
        tile_far: f32,
        stride: f32,
    ) -> (i32, i32) {
        if !repeats || stride <= 0.0 {
            return (0, 0);
        }
        (
            ((output_near - tile_near) / stride).floor() as i32,
            ((output_far - tile_far) / stride).ceil() as i32,
        )
    }

    /// How many tiles a range covers. Inclusive on both ends.
    pub fn tile_count(range: (i32, i32)) -> i32 {
        (range.1 - range.0 + 1).max(0)
    }
}

/// Upstream `DecorationImage`: an image painted into a decoration's shape,
/// fitted and aligned. Painting resolves the provider synchronously -- the
/// headless render's path; a widget-facing resolve arrives with the image
/// widget wave.
#[derive(Clone)]
pub struct DecorationImage {
    pub provider: ImageProvider,
    pub fit: Option<BoxFit>,
    pub alignment: RenderAlignmentGeometry,
    pub repeat: ImageRepeat,
}

impl DecorationImage {
    pub fn new(provider: ImageProvider) -> DecorationImage {
        DecorationImage {
            provider,
            fit: None,
            alignment: RenderAlignmentGeometry::CENTER,
            repeat: ImageRepeat::NoRepeat,
        }
    }

    pub fn with_fit(mut self, fit: BoxFit) -> DecorationImage {
        self.fit = Some(fit);
        self
    }

    pub fn with_alignment(mut self, alignment: RenderAlignmentGeometry) -> DecorationImage {
        self.alignment = alignment;
        self
    }

    /// Upstream `DecorationImagePainter.paint`: fit the decoded frame into
    /// `rect`, aligned. Returns whether a frame was there to paint.
    pub fn paint(&self, canvas: &mut Canvas, rect: Rect, direction: TextDirection) -> bool {
        let stream = self.provider.resolve_now(ImageConfiguration::EMPTY);
        let Some(completer) = stream.completer() else {
            return false;
        };
        let frame = {
            let completer = completer.borrow();
            completer.image().map(|info| info.image.clone())
        };
        let Some(frame) = frame else {
            return false;
        };
        let source = Rect::xywh(0.0, 0.0, frame.width() as f32, frame.height() as f32);
        // Fill defaults to scale-down, the way upstream's
        // `DecorationImage` documents for a `null` fit.
        let fit = self.fit.unwrap_or(BoxFit::ScaleDown);
        let fitted = apply_box_fit(
            fit,
            Size::new(source.width(), source.height()),
            Size::new(rect.width(), rect.height()),
        );
        let center = self.alignment.resolve(direction).within_rect(rect);
        let destination = Rect::xywh(
            center.dx - fitted.destination.width / 2.0,
            center.dy - fitted.destination.height / 2.0,
            fitted.destination.width,
            fitted.destination.height,
        );
        canvas.draw_image_rect(&frame, source, destination, None);
        true
    }
}

// -- Tests --------------------------------------------------------------------

/// Upstream `WidgetSpan`: a widget sitting inline in a paragraph.
///
/// It is a **leaf** of the span tree -- the widget it holds is not part of the
/// text at all, and the paragraph only reserves a box for it. That is why the
/// class carries an alignment and a baseline rather than any text: those are
/// the only questions the shaper can answer about something it cannot measure.
///
/// The assertion is the interesting part: the three baseline-relative
/// alignments **require** a baseline to be named. Aligning to a baseline
/// without saying which one is not a stricter request than the default, it is
/// an unanswerable one, and upstream refuses it rather than picking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetSpan {
    pub alignment: PlaceholderAlignment,
    /// `None` unless the alignment needs it.
    pub baseline: Option<TextBaseline>,
}

impl Default for WidgetSpan {
    fn default() -> WidgetSpan {
        WidgetSpan::new(PlaceholderAlignment::Bottom, None).expect("bottom needs no baseline")
    }
}

impl WidgetSpan {
    pub fn new(
        alignment: PlaceholderAlignment,
        baseline: Option<TextBaseline>,
    ) -> Option<WidgetSpan> {
        if Self::needs_baseline(alignment) && baseline.is_none() {
            return None;
        }
        Some(WidgetSpan {
            alignment,
            baseline,
        })
    }

    /// Upstream's assertion, stated positively.
    pub fn needs_baseline(alignment: PlaceholderAlignment) -> bool {
        matches!(
            alignment,
            PlaceholderAlignment::AboveBaseline
                | PlaceholderAlignment::BelowBaseline
                | PlaceholderAlignment::Baseline
        )
    }

    /// Upstream's `extractFromInlineSpan` scale factor, per span.
    ///
    /// The scaler is asked about the **font size in effect at this span**
    /// rather than about the span itself, and the factor handed to the widget
    /// is the ratio. A widget inline in a heading should grow with the heading
    /// when the reader turns text scaling up, and the heading's own size is
    /// the only thing that says by how much.
    ///
    /// The zero case is guarded because the ratio would otherwise divide by
    /// it: a font size of zero scales to zero, not to one.
    pub fn text_scale_factor(font_size: f32, scaled_font_size: f32) -> f32 {
        if font_size == 0.0 {
            return 0.0;
        }
        scaled_font_size / font_size
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn aligning_to_a_baseline_without_naming_one_is_unanswerable() {
        // Not a stricter request than the default -- an impossible one, so
        // upstream refuses it rather than picking a baseline.
        for alignment in [
            PlaceholderAlignment::Baseline,
            PlaceholderAlignment::AboveBaseline,
            PlaceholderAlignment::BelowBaseline,
        ] {
            assert!(WidgetSpan::needs_baseline(alignment), "{alignment:?}");
            assert!(WidgetSpan::new(alignment, None).is_none(), "{alignment:?}");
            assert!(WidgetSpan::new(alignment, Some(TextBaseline::Alphabetic)).is_some());
        }
    }

    #[test]
    fn the_alignments_that_do_not_touch_the_baseline_do_not_need_one() {
        for alignment in [
            PlaceholderAlignment::Top,
            PlaceholderAlignment::Bottom,
            PlaceholderAlignment::Middle,
        ] {
            assert!(!WidgetSpan::needs_baseline(alignment), "{alignment:?}");
            assert!(WidgetSpan::new(alignment, None).is_some(), "{alignment:?}");
        }
    }

    #[test]
    fn an_inline_widget_grows_with_the_text_it_sits_in() {
        // The scaler is asked about the font size in effect at the span, and
        // the factor is the ratio -- so a widget in a heading grows by the
        // heading's amount.
        assert_eq!(WidgetSpan::text_scale_factor(14.0, 21.0), 1.5);
        assert_eq!(WidgetSpan::text_scale_factor(14.0, 14.0), 1.0);
    }

    #[test]
    fn a_font_size_of_zero_scales_to_zero_rather_than_dividing_by_it() {
        assert_eq!(WidgetSpan::text_scale_factor(0.0, 10.0), 0.0);
    }

    use super::*;
    use crate::engine::TextDecoration;

    // Each test gets the cache to itself: they run on separate threads and the
    // cache is thread-local, but a name shared between two of them would still
    // be shaped twice and prove nothing. Distinct text per test avoids that
    // without any coordination.

    #[test]
    fn a_blur_radius_is_not_a_sigma() {
        // The conversion upstream applies before handing a radius to a mask
        // filter. Getting this wrong is a shadow that is too tight or too
        // vague, which looks like a theme problem rather than an arithmetic
        // one.
        let shadow = BoxShadow::new(crate::engine::Color(0x33000000), 0.0, 2.0, 4.0, 0.0);
        assert!((shadow.blur_sigma() - (4.0 * 0.577_35 + 0.5)).abs() < 1e-6);
    }

    #[test]
    fn no_blur_is_no_sigma() {
        let hard = BoxShadow::new(crate::engine::Color(0xff000000), 0.0, 1.0, 0.0, 0.0);
        assert_eq!(hard.blur_sigma(), 0.0);
    }

    #[test]
    fn flat_things_cast_nothing() {
        assert!(elevation_shadows(0).is_empty());
    }

    #[test]
    fn every_elevation_casts_the_three_material_shadows() {
        for elevation in [1, 2, 3, 4, 6, 8, 9, 12, 16, 24] {
            assert_eq!(
                elevation_shadows(elevation).len(),
                3,
                "elevation {elevation} should have an umbra, a penumbra and an ambient shadow"
            );
        }
    }

    #[test]
    fn an_elevation_between_two_defined_ones_takes_the_lower() {
        // Upstream's table is sparse and its keys are the ones some widget
        // uses; anything in between should still cast a shadow rather than
        // none.
        assert_eq!(elevation_shadows(5), elevation_shadows(4));
        assert_eq!(elevation_shadows(20), elevation_shadows(16));
        assert_eq!(elevation_shadows(100), elevation_shadows(24));
    }

    #[test]
    fn higher_is_softer_and_further() {
        let low = elevation_shadows(1)[2];
        let high = elevation_shadows(24)[2];
        assert!(high.blur_radius > low.blur_radius);
        assert!(high.offset.dy > low.offset.dy);
    }

    #[test]
    fn the_same_request_is_shaped_once() {
        let style = TextStyle::default();
        let first = shape("shaped once", &style, None, false, 200.0, 1.0);
        let second = shape("shaped once", &style, None, false, 200.0, 1.0);
        assert!(Rc::ptr_eq(&first, &second), "the second ask re-shaped");
    }

    #[test]
    fn a_different_width_is_a_different_paragraph() {
        let style = TextStyle::default();
        let narrow = shape("wraps differently", &style, None, false, 100.0, 1.0);
        let wide = shape("wraps differently", &style, None, false, 400.0, 1.0);
        // Line breaking depends on the width, so sharing one shaping between
        // two widths would put the breaks in the wrong place.
        assert!(!Rc::ptr_eq(&narrow, &wide));
    }

    #[test]
    fn a_different_style_is_a_different_paragraph() {
        let plain = TextStyle::default();
        let bold = TextStyle {
            font_weight: 700,
            ..TextStyle::default()
        };
        let a = shape("weight matters", &plain, None, false, 200.0, 1.0);
        let b = shape("weight matters", &bold, None, false, 200.0, 1.0);
        assert!(!Rc::ptr_eq(&a, &b));
    }

    #[test]
    fn a_different_letter_spacing_is_a_different_paragraph() {
        let plain = TextStyle::default();
        let tracked = TextStyle {
            letter_spacing: Some(2.0),
            ..TextStyle::default()
        };
        let a = shape("tracked out", &plain, None, false, 200.0, 1.0);
        let b = shape("tracked out", &tracked, None, false, 200.0, 1.0);
        // Spacing changes the glyph advances, so sharing a shaping between
        // the two would draw them back together.
        assert!(!Rc::ptr_eq(&a, &b));
    }

    #[test]
    fn a_taller_line_is_a_different_paragraph() {
        let plain = TextStyle::default();
        let airy = TextStyle {
            height: Some(2.0),
            ..TextStyle::default()
        };
        let a = shape("deep breath", &plain, None, false, 200.0, 1.0);
        let b = shape("deep breath", &airy, None, false, 200.0, 1.0);
        // The multiplier moves every line after the first, and the first
        // one's baseline too.
        assert!(!Rc::ptr_eq(&a, &b));
    }

    #[test]
    fn italic_and_decorated_text_is_shaped_again() {
        let plain = TextStyle::default();
        let slanted = TextStyle {
            italic: true,
            ..TextStyle::default()
        };
        let ruled = TextStyle {
            decoration: TextDecoration::UNDERLINE,
            ..TextStyle::default()
        };
        let a = shape("emphasis", &plain, None, false, 200.0, 1.0);
        let b = shape("emphasis", &slanted, None, false, 200.0, 1.0);
        let c = shape("emphasis", &ruled, None, false, 200.0, 1.0);
        assert!(!Rc::ptr_eq(&a, &b), "italic glyphs are different glyphs");
        assert!(!Rc::ptr_eq(&a, &c), "a decorated run has more to draw");
        assert!(!Rc::ptr_eq(&b, &c));
    }

    #[test]
    fn unset_fields_are_the_engine_defaults() {
        // `None` is the neutral case: leaving the new fields unset keys the
        // same as asking for nothing explicitly, and neither asks the engine
        // for a second paragraph where one would do.
        let unset = TextStyle::default();
        let explicit = TextStyle {
            font_family_fallback: Some(vec![]),
            font_features: Some(vec![]),
            ..TextStyle::default()
        };
        let a = shape("unchanged defaults", &unset, None, false, 200.0, 1.0);
        let b = shape("unchanged defaults", &explicit, None, false, 200.0, 1.0);
        assert!(Rc::ptr_eq(&a, &b));
    }

    // -- Direction and alignment ----------------------------------------------
    //
    // The stub engine cannot measure where a line landed, so these assert the
    // pair of codes the FFI was handed -- start (3) in an rtl base direction
    // (1) is what the real engine resolves to the right edge, the same
    // resolution txt::ParagraphStyle::effective_align makes.

    #[test]
    fn start_in_an_rtl_context_reaches_the_engine_as_start_plus_rtl() {
        crate::engine_test_stubs::reset_paragraph_styles();
        let style = TextStyle {
            align: TextAlign::Start,
            ..TextStyle::default()
        };
        crate::direction::with_direction(crate::direction::TextDirection::Rtl, || {
            shape("rtl start", &style, None, false, 300.0, 1.0);
        });
        assert_eq!(
            crate::engine_test_stubs::paragraph_style_requests(),
            vec![(3, 1)],
            "start in an rtl context must travel unresolved, with the direction"
        );
    }

    #[test]
    fn justify_reaches_the_engine_as_code_five() {
        crate::engine_test_stubs::reset_paragraph_styles();
        let style = TextStyle {
            align: TextAlign::Justify,
            ..TextStyle::default()
        };
        shape(
            "justify me across the width",
            &style,
            None,
            false,
            300.0,
            1.0,
        );
        assert_eq!(
            crate::engine_test_stubs::paragraph_style_requests(),
            vec![(5, 0)]
        );
    }

    #[test]
    fn end_in_an_ltr_context_is_the_default_paragraph() {
        // The default direction is ltr, so a plain `end` needs no
        // directionality around it to mean the right edge.
        crate::engine_test_stubs::reset_paragraph_styles();
        let style = TextStyle {
            align: TextAlign::End,
            ..TextStyle::default()
        };
        shape("plain end", &style, None, false, 300.0, 1.0);
        assert_eq!(
            crate::engine_test_stubs::paragraph_style_requests(),
            vec![(4, 0)]
        );
    }

    #[test]
    fn a_different_direction_is_a_different_paragraph() {
        // `start` means the left edge in ltr and the right one in rtl, so two
        // asks that differ only in direction must not share a shaping -- the
        // cache key carries the direction for exactly this reason.
        let style = TextStyle {
            align: TextAlign::Start,
            ..TextStyle::default()
        };
        let ltr = shape("direction matters", &style, None, false, 300.0, 1.0);
        let rtl = crate::direction::with_direction(crate::direction::TextDirection::Rtl, || {
            shape("direction matters", &style, None, false, 300.0, 1.0)
        });
        assert!(!Rc::ptr_eq(&ltr, &rtl));
    }

    #[test]
    fn rich_paragraphs_carry_their_direction_too() {
        crate::engine_test_stubs::reset_paragraph_styles();
        let runs = vec![(String::from("rich rtl run"), TextStyle::default())];
        crate::direction::with_direction(crate::direction::TextDirection::Rtl, || {
            shape_rich(&runs, TextAlign::End, None, false, 300.0, 1.0);
        });
        assert_eq!(
            crate::engine_test_stubs::paragraph_style_requests(),
            vec![(4, 1)]
        );
    }

    #[test]
    fn rich_runs_style_one_run_at_a_time() {
        let plain = TextStyle::default();
        let slanted = TextStyle {
            italic: true,
            ..plain.clone()
        };
        let runs = |first: TextStyle| {
            vec![
                (String::from("one style"), first),
                (String::from(" another"), plain.clone()),
            ]
        };
        let straight = shape_rich(
            &runs(plain.clone()),
            TextAlign::Left,
            None,
            false,
            200.0,
            1.0,
        );
        let mixed = shape_rich(&runs(slanted), TextAlign::Left, None, false, 200.0, 1.0);
        // The same words in the same order: only the style of the first run
        // differs, and that is still a different paragraph.
        assert!(!Rc::ptr_eq(&straight, &mixed));
        // And asking for the plain one again finds it, not the italic one.
        assert!(Rc::ptr_eq(
            &straight,
            &shape_rich(
                &runs(plain.clone()),
                TextAlign::Left,
                None,
                false,
                200.0,
                1.0
            )
        ));
    }

    #[test]
    fn text_still_on_screen_survives_a_frame() {
        let style = TextStyle::default();
        let first = shape("still drawn", &style, None, false, 200.0, 1.0);
        end_text_frame();
        let second = shape("still drawn", &style, None, false, 200.0, 1.0);
        assert!(
            Rc::ptr_eq(&first, &second),
            "a live paragraph was re-shaped"
        );
    }

    #[test]
    fn the_readers_text_size_reaches_the_shaper() {
        // This used to check the cache alone, because every metric the stubbed
        // engine returned was zero and there was no width to compare. The stub
        // models metrics now, so the claim can be made directly: a reader who
        // asked for larger text gets a wider paragraph.
        //
        // The cache half stays, because it is a second and separate fact --
        // the same words at two sizes are two shaping requests, not one
        // answer reused.
        let style = TextStyle::default();
        let before = shaped_paragraph_count();
        let unscaled = shape("the reader's size", &style, None, false, 200.0, 1.0);
        assert_eq!(shaped_paragraph_count(), before + 1);

        let scaled = shape("the reader's size", &style, None, false, 200.0, 1.5);
        assert!(
            !Rc::ptr_eq(&unscaled, &scaled),
            "the same text at a different size must be shaped again"
        );

        // And the larger request measures larger, which is the part that says
        // the scale reached the engine rather than merely the cache key.
        assert!(
            scaled.max_intrinsic_width() > unscaled.max_intrinsic_width(),
            "{} should be wider than {}",
            scaled.max_intrinsic_width(),
            unscaled.max_intrinsic_width()
        );
        assert!(
            scaled.height() > unscaled.height(),
            "and taller: {} against {}",
            scaled.height(),
            unscaled.height()
        );
        assert_eq!(shaped_paragraph_count(), before + 2);

        // And back, which the cache still has: the scale changes the style the
        // entry is keyed on rather than invalidating anything.
        assert!(Rc::ptr_eq(
            &unscaled,
            &shape("the reader's size", &style, None, false, 200.0, 1.0)
        ));
    }

    #[test]
    fn two_subtrees_can_be_shaped_at_two_sizes_at_once() {
        // The whole point of the scale being an argument: a dense table that
        // opted out and the page around it are on screen together, and the
        // cache has to hold both rather than one evicting the other.
        let style = TextStyle::default();
        let big = shape("side by side", &style, None, false, 200.0, 2.0);
        let small = shape("side by side", &style, None, false, 200.0, 1.0);
        assert!(!Rc::ptr_eq(&big, &small));
        assert!(Rc::ptr_eq(
            &big,
            &shape("side by side", &style, None, false, 200.0, 2.0)
        ));
        assert!(Rc::ptr_eq(
            &small,
            &shape("side by side", &style, None, false, 200.0, 1.0)
        ));
    }

    #[test]
    fn text_that_stopped_being_drawn_is_dropped() {
        let style = TextStyle::default();
        let before = shaped_paragraph_count();
        let _ = shape("shown briefly", &style, None, false, 200.0, 1.0);
        assert_eq!(shaped_paragraph_count(), before + 1);
        // Two frames: the first moves it to the previous generation, the second
        // drops that generation entirely.
        end_text_frame();
        end_text_frame();
        assert_eq!(shaped_paragraph_count(), before);
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;

    // Every test names its own key: the cache is thread-local and the tests run
    // on separate threads, but a shared name would still make one test's
    // request satisfy another's and prove nothing.

    const PNG: &[u8] = b"not really a png, but the stubs do not look";

    #[test]
    fn the_first_ask_does_not_block_and_returns_nothing() {
        // The part that cannot race: a decode is never synchronous, so the
        // first ask is always empty. Whether the worker has *finished* by the
        // next line is a race with a real thread -- it used to be asserted
        // here and failed on a loaded machine.
        assert!(Image::shared("async:first", PNG).is_none());
        assert!(wait_for_images(), "nothing was queued");
        assert!(
            Image::shared("async:first", PNG).is_some(),
            "and then it arrives"
        );
    }

    #[test]
    fn the_picture_is_there_once_the_worker_is_done() {
        assert!(Image::shared("async:lands", PNG).is_none());
        assert!(wait_for_images(), "there was nothing to wait for");
        assert!(Image::shared("async:lands", PNG).is_some());
        assert!(!images_pending());
    }

    #[test]
    fn the_same_key_is_decoded_once() {
        assert!(Image::shared("async:once", PNG).is_none());
        // Asking again must not queue a second decode. Whether this second ask
        // finds it still in flight or already landed is up to the worker, so
        // what is asserted is the part that does not race: everyone ends up
        // holding the same picture, which two decodes could not produce.
        let _ = Image::shared("async:once", PNG);
        wait_for_images();
        let first = Image::shared("async:once", PNG).expect("decoded");
        let second = Image::shared("async:once", PNG).expect("decoded");
        assert!(Rc::ptr_eq(&first, &second));
    }

    #[test]
    fn an_arrival_is_reported_once() {
        assert!(!take_images_arrived(), "nothing has arrived yet");
        assert!(Image::shared("async:arrival", PNG).is_none());
        wait_for_images();
        assert!(take_images_arrived(), "the arrival went unreported");
        // Reported once: a frame that already rebuilt for it must not rebuild
        // again every frame afterwards.
        assert!(!take_images_arrived());
    }

    #[test]
    fn a_blocking_ask_has_the_picture_straight_away() {
        // The single-frame path -- a headless render, a golden -- has no next
        // frame to pick the image up in.
        assert!(Image::shared_now("async:now", PNG).is_some());
        assert!(!images_pending());
    }

    #[test]
    fn a_blocking_ask_joins_one_already_in_flight() {
        assert!(Image::shared("async:joins", PNG).is_none());
        let image = Image::shared_now("async:joins", PNG).expect("decoded");
        // The same handle the worker produced, not a second decode of the same
        // bytes.
        let again = Image::shared("async:joins", PNG).expect("decoded");
        assert!(Rc::ptr_eq(&image, &again));
        assert!(!images_pending());
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;

    #[test]
    fn hsv_round_trips_a_primary() {
        let red = Color::argb(255, 255, 0, 0);
        let hsv = HSVColor::from_color(red);
        assert_eq!(hsv.hue, 0.0);
        assert_eq!(hsv.saturation, 1.0);
        assert_eq!(hsv.value, 1.0);
        assert_eq!(hsv.to_color(), red);

        // Half-way between red and green is a 60-degree yellow.
        let yellow = Color::argb(255, 255, 255, 0);
        let hsv = HSVColor::from_color(yellow);
        assert_eq!(hsv.hue, 60.0);
        assert_eq!(hsv.to_color(), yellow);

        // Greys have no hue and no saturation.
        let grey = Color::argb(255, 128, 128, 128);
        let hsv = HSVColor::from_color(grey);
        assert_eq!(hsv.hue, 0.0);
        assert_eq!(hsv.saturation, 0.0);
    }

    #[test]
    fn hsl_round_trips_a_primary() {
        let blue = Color::argb(255, 0, 0, 255);
        let hsl = HSLColor::from_color(blue);
        assert_eq!(hsl.hue, 240.0);
        assert_eq!(hsl.saturation, 1.0);
        assert_eq!(hsl.lightness, 0.5);
        assert_eq!(hsl.to_color(), blue);

        // White and black sit at the lightness ends.
        assert_eq!(HSLColor::from_color(Color::WHITE).lightness, 1.0);
        assert_eq!(HSLColor::from_color(Color::BLACK).lightness, 0.0);
    }

    #[test]
    fn hsv_and_hsl_lerp_wrap_the_hue() {
        // The hue lerps raw and wraps with `% 360` -- 300 to 60 passes
        // through 180, it does not take the short way round.
        let a = HSVColor::from_ahsv(1.0, 300.0, 1.0, 1.0);
        let b = HSVColor::from_ahsv(1.0, 60.0, 1.0, 1.0);
        let mid = HSVColor::lerp(Some(a), Some(b), 0.5).unwrap();
        assert!((mid.hue - 180.0).abs() < 1e-6);
        // Extrapolated past the ends, the modulo wraps -- and stays
        // non-negative the way Dart's `%` does: -160 arrives as 200.
        let a = HSVColor::from_ahsv(1.0, 350.0, 1.0, 1.0);
        let b = HSVColor::from_ahsv(1.0, 10.0, 1.0, 1.0);
        let lerped = HSVColor::lerp(Some(a), Some(b), 1.5).unwrap();
        assert!((lerped.hue - 200.0).abs() < 1e-6);

        // A missing side is a transparent instance of the other.
        let faded =
            HSLColor::lerp(None, Some(HSLColor::from_ahsl(1.0, 120.0, 0.5, 0.5)), 0.5).unwrap();
        assert!((faded.alpha - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_swatch_holds_its_shades() {
        let swatch = ColorSwatch::new(
            Color::argb(255, 0xF4, 0x43, 0x36),
            [
                (100u32, Color::argb(255, 0xB7, 0x1C, 0x1C)),
                (500, Color::argb(255, 0xF4, 0x43, 0x36)),
                (900, Color::argb(255, 0xB7, 0x1C, 0x1C)),
            ],
        );
        assert_eq!(swatch.get(&500), Some(Color::argb(255, 0xF4, 0x43, 0x36)));
        assert_eq!(swatch.get(&900), Some(Color::argb(255, 0xB7, 0x1C, 0x1C)));
        assert_eq!(swatch.get(&200), None);
        assert_eq!(swatch.keys().count(), 3);
    }

    #[test]
    fn text_scaler_scales_linearly() {
        assert_eq!(TextScaler::NO_SCALING.scale(14.0), 14.0);
        assert_eq!(TextScaler::linear(1.3).scale(10.0), 13.0);
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn an_arc_is_written_as_cubics_that_stay_on_the_ellipse() {
        // The approximation is exact at the two ends of each piece; between
        // them it must not wander. A sign slip in the tangent is the way to
        // get this wrong, and it draws a curve that bulges the wrong way
        // without ever leaving the end points -- which is why the midpoint is
        // what this checks.
        let rect = Rect::ltrb(-30.0, -20.0, 30.0, 20.0);
        let (cx, cy) = rect.center();
        let (rx, ry) = (rect.width() / 2.0, rect.height() / 2.0);
        let start = 0.4;
        let sweep = std::f32::consts::PI * 1.3;
        let segments = arc_cubics(rect, start, sweep);
        // A sweep over a quarter turn is cut into pieces.
        assert_eq!(segments.len(), 3);

        let mut from = (cx + rx * start.cos(), cy + ry * start.sin());
        let delta = sweep / segments.len() as f32;
        for (index, [c1, c2, end]) in segments.iter().enumerate() {
            let angle = start + delta * (index as f32 + 1.0);
            let expected = (cx + rx * angle.cos(), cy + ry * angle.sin());
            assert!(
                (end.0 - expected.0).abs() < 1e-3 && (end.1 - expected.1).abs() < 1e-3,
                "segment {index} ends at {end:?}, not on the ellipse at {expected:?}"
            );
            // The curve at its midpoint, which is where a bad approximation
            // shows up.
            let mid = |a: f32, b: f32, c: f32, d: f32| (a + 3.0 * b + 3.0 * c + d) / 8.0;
            let point = (
                mid(from.0, c1.0, c2.0, end.0),
                mid(from.1, c1.1, c2.1, end.1),
            );
            let on_ellipse = ((point.0 - cx) / rx).powi(2) + ((point.1 - cy) / ry).powi(2);
            assert!(
                (on_ellipse - 1.0).abs() < 1e-3,
                "segment {index} bulges off the ellipse at its midpoint ({on_ellipse})"
            );
            from = *end;
        }
    }

    #[test]
    fn an_arc_of_no_sweep_contributes_no_curve() {
        assert!(arc_cubics(Rect::ltrb(0.0, 0.0, 10.0, 10.0), 0.0, 0.0).is_empty());
        // And a sweep the other way round walks backwards rather than the
        // long way round.
        let back = arc_cubics(Rect::ltrb(0.0, 0.0, 10.0, 10.0), 0.0, -1.0);
        assert_eq!(back.len(), 1);
        assert!(
            back[0][2].1 < 5.0,
            "a negative sweep should go up, not down"
        );
    }
}

#[cfg(test)]
mod gradient_tests {
    use super::*;
    use crate::direction::TextDirection;
    use crate::render::Alignment;

    const RED: Color = Color(0xFF0000FF);
    const BLUE: Color = Color(0xFFFF0000);

    #[test]
    fn implied_stops_space_colours_evenly() {
        let gradient = LinearGradient::new(&[RED, Color::WHITE, BLUE]);
        assert_eq!(gradient.implied_stops(), vec![0.0, 0.5, 1.0]);
        let explicit = LinearGradient::new(&[RED, BLUE]).with_stops(&[0.25, 0.75]);
        assert_eq!(explicit.implied_stops(), vec![0.25, 0.75]);
    }

    #[test]
    fn linear_lerp_unions_the_stops_and_samples_both_ramps() {
        let a = LinearGradient::new(&[RED, RED]);
        let b = LinearGradient::new(&[BLUE, BLUE]);
        let mid = LinearGradient::lerp(Some(&a), Some(&b), 0.5);
        assert_eq!(mid.stops, Some(vec![0.0, 1.0]));
        for color in &mid.colors {
            assert_eq!(*color, crate::borders::color_lerp(RED, BLUE, 0.5));
        }
        // Mismatched stop lists union: 0, 0.5, 1.
        let a = LinearGradient::new(&[RED, BLUE]).with_stops(&[0.0, 0.5]);
        let b = LinearGradient::new(&[RED, BLUE]).with_stops(&[0.5, 1.0]);
        let mid = LinearGradient::lerp(Some(&a), Some(&b), 0.5);
        assert_eq!(mid.stops, Some(vec![0.0, 0.5, 1.0]));
    }

    #[test]
    fn linear_lerp_moves_the_anchors() {
        let a = LinearGradient::new(&[RED, BLUE])
            .with_begin(AlignmentGeometry::Absolute(Alignment::TOP_LEFT));
        let b = LinearGradient::new(&[RED, BLUE])
            .with_begin(AlignmentGeometry::Absolute(Alignment::BOTTOM_RIGHT));
        let mid = LinearGradient::lerp(Some(&a), Some(&b), 0.5);
        assert_eq!(mid.begin, AlignmentGeometry::Absolute(Alignment::CENTER));
    }

    #[test]
    fn gradient_scale_fades_the_alphas() {
        let gradient = RadialGradient::new(&[RED, BLUE]);
        let faded = gradient.scale(0.5);
        assert_eq!(faded.colors[0].alpha(), 128);
        assert_eq!(faded.radius, 0.5);
    }

    #[test]
    fn shader_gradient_lerps_across_kinds_over_two_halves() {
        let a = ShaderGradient::Linear(LinearGradient::new(&[RED, BLUE]));
        let b = ShaderGradient::Sweep(SweepGradient::new(&[BLUE, RED]));
        match ShaderGradient::lerp(Some(a.clone()), Some(b.clone()), 0.25) {
            Some(ShaderGradient::Linear(fading)) => {
                assert_eq!(fading.colors[0].alpha(), 128)
            }
            other => panic!("expected a fading linear gradient, got {other:?}"),
        }
        match ShaderGradient::lerp(Some(a.clone()), Some(b.clone()), 0.75) {
            Some(ShaderGradient::Sweep(arriving)) => {
                assert_eq!(arriving.colors[0].alpha(), 128)
            }
            other => panic!("expected an arriving sweep, got {other:?}"),
        }
    }

    #[test]
    fn gradient_rotation_turns_the_ramp_about_the_centre() {
        let affine = GradientTransform::Rotation {
            radians: std::f32::consts::FRAC_PI_2,
        }
        .transform(Rect::xywh(0.0, 0.0, 100.0, 100.0));
        // The left-middle of the box moves to the top-middle (within the
        // trig round-off a rotated matrix carries).
        let turned = affine.map_point((0.0, 50.0));
        assert!((turned.0 - 50.0).abs() < 1e-4 && turned.1.abs() < 1e-4);
        // The centre stays put, to the same round-off.
        let centre = affine.map_point((50.0, 50.0));
        assert!((centre.0 - 50.0).abs() < 1e-4 && (centre.1 - 50.0).abs() < 1e-4);
    }

    #[test]
    fn gradients_paint_without_panicking_under_the_stubs() {
        let rect = Rect::xywh(0.0, 0.0, 120.0, 80.0);
        let _ = LinearGradient::new(&[RED, BLUE]).to_paint(rect, TextDirection::Ltr);
        let _ = RadialGradient::new(&[RED, BLUE])
            .with_transform(GradientTransform::Rotation { radians: 0.5 })
            .to_paint(rect, TextDirection::Ltr);
        let _ = SweepGradient::new(&[RED, BLUE])
            .with_angles(0.0, std::f32::consts::PI)
            .to_paint(rect, TextDirection::Ltr);
    }
}

#[cfg(test)]
mod matrix_tests {
    use super::*;
    use crate::render::Offset;

    #[test]
    fn a_translation_reads_back_as_one() {
        let translation = Matrix4::translation(3.0, 4.0, 0.0);
        assert_eq!(
            matrix_utils::get_as_translation(translation),
            Some(Offset::new(3.0, 4.0))
        );
        // A rotation is not a translation.
        assert_eq!(
            matrix_utils::get_as_translation(Matrix4::rotation_z(0.5)),
            None
        );
    }

    #[test]
    fn a_uniform_scale_reads_back_as_one() {
        let scale = Matrix4::IDENTITY.set_entry(0, 0, 2.0).set_entry(1, 1, 2.0);
        assert_eq!(matrix_utils::get_as_scale(scale), Some(2.0));
        // Non-uniform or rotated is not a scale.
        let non_uniform = Matrix4::IDENTITY.set_entry(0, 0, 2.0);
        assert_eq!(matrix_utils::get_as_scale(non_uniform), None);
    }

    #[test]
    fn transform_point_and_rect_agree_for_a_rotation() {
        let quarter = Matrix4::rotation_z(std::f32::consts::FRAC_PI_2);
        let point = matrix_utils::transform_point(quarter, Offset::new(10.0, 0.0));
        // A quarter turn clockwise (y grows down) puts +x on +y.
        assert!((point.dx).abs() < 1e-4 && (point.dy - 10.0).abs() < 1e-4);
        let rect = matrix_utils::transform_rect(quarter, Rect::xywh(10.0, 0.0, 10.0, 5.0));
        assert!((rect.left + 5.0).abs() < 1e-4 && (rect.top - 10.0).abs() < 1e-4);
    }

    #[test]
    fn an_affine_rect_transform_pushes_walls_by_sign() {
        // Scale by 2 about the origin: every wall doubles.
        let scale = Matrix4::IDENTITY.set_entry(0, 0, 2.0).set_entry(1, 1, 2.0);
        let rect = matrix_utils::transform_rect(scale, Rect::xywh(1.0, 2.0, 3.0, 4.0));
        assert_eq!(rect, Rect::xywh(2.0, 4.0, 6.0, 8.0));
    }

    #[test]
    fn inversion_round_trips_a_rect() {
        // Axis-aligned, or the bounding box of the transformed rect is not
        // the transformed rect and the round trip has nothing to come back to.
        let transform = Matrix4::mul(
            Matrix4::translation(11.0, -7.0, 0.0),
            Matrix4::IDENTITY.set_entry(0, 0, 2.0).set_entry(1, 1, 3.0),
        );
        let rect = Rect::xywh(1.0, 2.0, 30.0, 40.0);
        let there = matrix_utils::transform_rect(transform, rect);
        let back = matrix_utils::inverse_transform_rect(transform, there);
        assert!((back.left - rect.left).abs() < 1e-3);
        assert!((back.top - rect.top).abs() < 1e-3);
        assert!((back.right - rect.right).abs() < 1e-3);
        assert!((back.bottom - rect.bottom).abs() < 1e-3);
    }

    #[test]
    fn force_to_point_collapses_everything_onto_the_offset() {
        let transform = matrix_utils::force_to_point(Offset::new(7.0, 9.0));
        assert_eq!(
            matrix_utils::transform_point(transform, Offset::new(100.0, -50.0)),
            Offset::new(7.0, 9.0)
        );
    }

    #[test]
    fn a_cylindrical_projection_smokes() {
        let _ = matrix_utils::create_cylindrical_projection_transform(
            100.0,
            0.5,
            0.001,
            crate::render::Axis::Vertical,
        );
    }
}

#[cfg(test)]
mod text_painter_tests {
    use super::*;

    #[test]
    fn a_text_painter_lays_out_and_measures() {
        // Under the stub engine nothing measures, so the observable contract
        // is that layout runs and the numbers agree with the paragraph; the
        // relations below bite wherever a real shaper answers.
        let mut painter = TextPainter::new()
            .text("hello shaped world", TextStyle::default())
            .with_max_lines(None);
        painter.layout(300.0);
        assert_eq!(painter.width(), painter.width());
        if painter.width() > 0.0 {
            assert!(painter.height() > 0.0);
            let narrow_height = {
                let mut painter =
                    TextPainter::new().text("hello shaped world", TextStyle::default());
                painter.layout(30.0);
                painter.height()
            };
            assert!(narrow_height >= painter.height() * 1.9);
        }
    }

    #[test]
    fn max_lines_clamps_the_height() {
        let mut free = TextPainter::new().text("one two three four five six", TextStyle::default());
        free.layout(40.0);
        let mut clamped = TextPainter::new()
            .text("one two three four five six", TextStyle::default())
            .with_max_lines(Some(1));
        clamped.layout(40.0);
        if free.height() > 0.0 {
            assert!(clamped.height() < free.height());
        }
    }

    #[test]
    fn placeholder_and_word_boundary_are_carried_data() {
        let placeholder = PlaceholderDimensions {
            size: (50.0, 20.0),
            alignment: PlaceholderAlignment::Middle,
            baseline: TextBaseline::Alphabetic,
        };
        assert_eq!(placeholder.alignment, PlaceholderAlignment::Middle);
        let boundary = WordBoundary {
            prefix: "hello ".to_string(),
            suffix: "world".to_string(),
        };
        assert_eq!(boundary.prefix, "hello ");
    }

    #[test]
    fn an_accumulator_counts_a_span_walk() {
        let mut accumulator = Accumulator::default();
        accumulator.increment(5);
        accumulator.increment(3);
        assert_eq!(accumulator.value, 8);
    }

    #[test]
    fn span_semantics_mark_placeholders() {
        let text = InlineSpanSemanticsInformation::text("label");
        assert_eq!(text.text, "label");
        assert!(!text.is_placeholder);
        assert!(InlineSpanSemanticsInformation::placeholder().is_placeholder);
    }
}

// -- Turning a clip behaviour into canvas calls --------------------------------

/// What one clip-and-paint did to the canvas, in order.
///
/// The point of recording it is that the sequence is the whole of
/// [`ClipContext`], and it is not the same for every behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipStep {
    Save,
    /// The clip itself. The flag is upstream's `doAntiAlias`.
    Clip {
        anti_alias: bool,
    },
    SaveLayer,
    Paint,
    Restore,
}

/// Upstream `ClipContext`: the four clip behaviours, as the canvas calls they
/// stand for.
///
/// This is not a debug helper -- upstream's `PaintingContext extends
/// ClipContext`, and every clipped paint in the framework goes through it.
///
/// # The behaviours are four different sequences, not four flags
///
/// * `none` saves and restores and clips nothing, so a caller need not branch;
/// * `hardEdge` clips without anti-aliasing;
/// * `antiAlias` clips with it;
/// * `antiAliasWithSaveLayer` clips with it **and opens a save layer**, which
///   is a second restore on the way out.
///
/// The last one is the reason this is a type. Anti-aliasing a clip blends the
/// edge pixels against what is already on the canvas -- fine over an opaque
/// background, and visibly wrong when the clipped content is itself composited,
/// because the edge gets blended twice. The save layer gives the content its
/// own buffer so the blend happens once. It costs an offscreen pass, which is
/// why it is not simply the default.
pub trait ClipContext {
    /// Records one canvas call. An implementer drives the real canvas here.
    fn record(&mut self, step: ClipStep);

    /// Upstream's private `_clipAndPaint`, which the three public methods all
    /// funnel through. They differ only in *which* clip call they make, and the
    /// order around it is what is shared.
    fn clip_and_paint(&mut self, behavior: ClipBehavior, paint: impl FnOnce(&mut Self)) {
        self.record(ClipStep::Save);
        match behavior {
            ClipBehavior::None => {}
            ClipBehavior::HardEdge => self.record(ClipStep::Clip { anti_alias: false }),
            ClipBehavior::AntiAlias => self.record(ClipStep::Clip { anti_alias: true }),
            ClipBehavior::AntiAliasWithSaveLayer => {
                self.record(ClipStep::Clip { anti_alias: true });
                self.record(ClipStep::SaveLayer);
            }
        }
        paint(self);
        if behavior == ClipBehavior::AntiAliasWithSaveLayer {
            self.record(ClipStep::Restore);
        }
        self.record(ClipStep::Restore);
    }

    /// Upstream's `clipRectAndPaint`.
    fn clip_rect_and_paint(&mut self, behavior: ClipBehavior, paint: impl FnOnce(&mut Self)) {
        self.clip_and_paint(behavior, paint);
    }

    /// Upstream's `clipRRectAndPaint`.
    fn clip_rrect_and_paint(&mut self, behavior: ClipBehavior, paint: impl FnOnce(&mut Self)) {
        self.clip_and_paint(behavior, paint);
    }

    /// Upstream's `clipPathAndPaint`.
    fn clip_path_and_paint(&mut self, behavior: ClipBehavior, paint: impl FnOnce(&mut Self)) {
        self.clip_and_paint(behavior, paint);
    }
}

// -- Telling a developer their image is bigger than the space it is in ---------

/// Upstream `ImageSizeInfo`: a decoded image, and the size it was actually
/// drawn at.
///
/// # What it is for
///
/// An image decoded at 4000 by 3000 and drawn into a 100 by 75 box costs about
/// forty times the memory it needs to, and nothing on screen says so -- it
/// looks right. Upstream collects these during a debug frame and reports the
/// ones that are wasteful, which is how a developer finds out.
///
/// The comparison is by **area at device pixels**, not by either dimension:
/// upstream's rule is that the decoded size is excessive when it is more than
/// twice the display size in each direction, which is four times the pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageSizeInfo {
    /// Where the image came from, for the message. `None` when the caller has
    /// no name for it.
    pub source: Option<&'static str>,
    /// The size the image is drawn at, in logical pixels.
    pub display_size: crate::render::Size,
    /// The size it was decoded at.
    pub image_size: crate::render::Size,
}

impl ImageSizeInfo {
    /// Upstream's `displaySizeInBytes`, at four bytes a pixel.
    pub fn display_size_in_bytes(&self) -> usize {
        ImageSizeInfo::size_in_bytes(self.display_size)
    }

    /// Upstream's `decodedSizeInBytes`.
    pub fn decoded_size_in_bytes(&self) -> usize {
        ImageSizeInfo::size_in_bytes(self.image_size)
    }

    fn size_in_bytes(size: crate::render::Size) -> usize {
        (size.width.max(0.0) * size.height.max(0.0)) as usize * 4
    }

    /// Upstream's `isOversized`: more than twice the display size **in each
    /// direction**.
    ///
    /// Each direction and not area, because an image that is wide and short
    /// relative to its box is not being wasted -- it is being letterboxed, and
    /// the developer chose that.
    pub fn is_oversized(&self) -> bool {
        self.image_size.width > self.display_size.width * 2.0
            && self.image_size.height > self.display_size.height * 2.0
    }

    /// The wasted bytes, which is what makes the report worth reading: a
    /// percentage says nothing about whether it matters.
    pub fn wasted_bytes(&self) -> usize {
        self.decoded_size_in_bytes()
            .saturating_sub(self.display_size_in_bytes())
    }
}

#[cfg(test)]
mod clip_context_tests {
    use super::*;

    #[derive(Default)]
    struct Recorder {
        steps: Vec<ClipStep>,
    }

    impl ClipContext for Recorder {
        fn record(&mut self, step: ClipStep) {
            self.steps.push(step);
        }
    }

    fn steps(behavior: ClipBehavior) -> Vec<ClipStep> {
        let mut recorder = Recorder::default();
        recorder.clip_rect_and_paint(behavior, |context| context.record(ClipStep::Paint));
        recorder.steps
    }

    #[test]
    fn none_still_saves_and_restores_so_a_caller_need_not_branch() {
        assert_eq!(
            steps(ClipBehavior::None),
            vec![ClipStep::Save, ClipStep::Paint, ClipStep::Restore]
        );
    }

    #[test]
    fn hard_edge_and_anti_alias_differ_only_in_the_flag() {
        assert_eq!(
            steps(ClipBehavior::HardEdge),
            vec![
                ClipStep::Save,
                ClipStep::Clip { anti_alias: false },
                ClipStep::Paint,
                ClipStep::Restore
            ]
        );
        assert_eq!(
            steps(ClipBehavior::AntiAlias),
            vec![
                ClipStep::Save,
                ClipStep::Clip { anti_alias: true },
                ClipStep::Paint,
                ClipStep::Restore
            ]
        );
    }

    #[test]
    fn the_save_layer_form_opens_a_buffer_and_closes_it_again() {
        // The extra restore is not decoration: without it the layer is left
        // open and everything painted afterwards goes into it.
        assert_eq!(
            steps(ClipBehavior::AntiAliasWithSaveLayer),
            vec![
                ClipStep::Save,
                ClipStep::Clip { anti_alias: true },
                ClipStep::SaveLayer,
                ClipStep::Paint,
                ClipStep::Restore,
                ClipStep::Restore
            ]
        );
    }

    #[test]
    fn every_behaviour_balances_its_saves_and_restores() {
        for behavior in [
            ClipBehavior::None,
            ClipBehavior::HardEdge,
            ClipBehavior::AntiAlias,
            ClipBehavior::AntiAliasWithSaveLayer,
        ] {
            let steps = steps(behavior);
            let opened = steps
                .iter()
                .filter(|s| matches!(s, ClipStep::Save | ClipStep::SaveLayer))
                .count();
            let closed = steps.iter().filter(|s| **s == ClipStep::Restore).count();
            assert_eq!(opened, closed, "{behavior:?} leaves the canvas unbalanced");
        }
    }

    #[test]
    fn the_three_shapes_share_the_sequence_and_differ_only_in_the_clip_call() {
        let mut a = Recorder::default();
        a.clip_rrect_and_paint(ClipBehavior::AntiAlias, |c| c.record(ClipStep::Paint));
        let mut b = Recorder::default();
        b.clip_path_and_paint(ClipBehavior::AntiAlias, |c| c.record(ClipStep::Paint));
        assert_eq!(a.steps, b.steps);
        assert_eq!(a.steps, steps(ClipBehavior::AntiAlias));
    }
}

/// Upstream `RenderComparison` (`painting/basic_types.dart`): how badly two
/// objects differ, from the render tree's point of view.
///
/// **The variants are ordered, and the order is the whole of it.** Upstream
/// compares `.index` rather than the values -- `comparison.index >=
/// RenderComparison.layout.index` asks for a relayout, `>= paint.index` for a
/// repaint -- so this is a four-rung ladder, not four names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum RenderComparison {
    /// Deeply equal. Upstream's doc is careful to say this does not mean
    /// `identical()` in the Dart sense.
    #[default]
    Identical,
    /// The same for layout and for paint, different in some other way --
    /// upstream's example is "maybe some event handlers changed".
    ///
    /// **Nothing in this port produces it yet.** Upstream reaches it from
    /// `TextSpan.compareTo`, where a differing `recognizer` is a difference
    /// that neither moves nor recolours anything; this port's spans carry no
    /// recognizer. The rung is kept because the ladder's numbering is
    /// upstream's: taking it out would make `paint` the second rung here and
    /// the third there.
    Metadata,
    /// Different in ways that affect paint but not layout -- "only the colour
    /// is changed".
    Paint,
    /// Different in ways that affect layout, and so paint as well. Upstream
    /// calls this "the most drastic level of change possible", which is what
    /// licenses the early exit in [`RenderComparison::worse_of`]'s callers.
    Layout,
}

impl RenderComparison {
    /// The worst change there is. Upstream says so in prose; the callers rely
    /// on it in code.
    pub const WORST: RenderComparison = RenderComparison::Layout;

    /// Upstream's `.index`.
    pub fn rank(self) -> u8 {
        match self {
            RenderComparison::Identical => 0,
            RenderComparison::Metadata => 1,
            RenderComparison::Paint => 2,
            RenderComparison::Layout => 3,
        }
    }

    /// The running maximum upstream keeps while walking a span's style and
    /// then its children:
    ///
    /// ```dart
    /// if (candidate.index > result.index) { result = candidate; }
    /// if (result == RenderComparison.layout) { return result; }
    /// ```
    ///
    /// Two differences do not add up to something worse than either; the
    /// answer is whichever demands more work.
    pub fn worse_of(self, other: RenderComparison) -> RenderComparison {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    /// Whether upstream would stop walking here.
    ///
    /// **Sound only because `Layout` is the top of the ladder.** The early
    /// return is not an approximation: no remaining child could raise the
    /// answer, so reading them would cost work and change nothing.
    pub fn is_final(self) -> bool {
        self == RenderComparison::WORST
    }

    /// `comparison.index >= RenderComparison.layout.index`.
    pub fn needs_layout(self) -> bool {
        self.rank() >= RenderComparison::Layout.rank()
    }

    /// `comparison.index >= RenderComparison.paint.index`.
    ///
    /// Note this is true of `Layout` as well, which is why upstream's consumer
    /// can write `if (needs layout) ... else if (needs paint)` and be right:
    /// the layout branch already covers the repaint.
    pub fn needs_paint(self) -> bool {
        self.rank() >= RenderComparison::Paint.rank()
    }
}

#[cfg(test)]
mod image_size_tests {
    use super::*;
    use crate::render::Size;

    fn info(display: (f32, f32), decoded: (f32, f32)) -> ImageSizeInfo {
        ImageSizeInfo {
            source: Some("test"),
            display_size: Size::new(display.0, display.1),
            image_size: Size::new(decoded.0, decoded.1),
        }
    }

    #[test]
    fn a_photograph_in_a_thumbnail_is_oversized() {
        let waste = info((100.0, 75.0), (4000.0, 3000.0));
        assert!(waste.is_oversized());
        assert_eq!(waste.decoded_size_in_bytes(), 4000 * 3000 * 4);
        assert_eq!(waste.display_size_in_bytes(), 100 * 75 * 4);
        assert_eq!(waste.wasted_bytes(), 4000 * 3000 * 4 - 100 * 75 * 4);
    }

    #[test]
    fn exactly_twice_is_not_oversized() {
        // Upstream's test is `>`, not `>=`, and doubling is what a 2x display
        // asks for.
        assert!(!info((100.0, 100.0), (200.0, 200.0)).is_oversized());
        assert!(info((100.0, 100.0), (201.0, 201.0)).is_oversized());
        // Each axis is tested on its own account, so one of them sitting
        // exactly on the boundary is enough to say no.
        assert!(!info((100.0, 100.0), (200.0, 400.0)).is_oversized());
        assert!(!info((100.0, 100.0), (400.0, 200.0)).is_oversized());
    }

    #[test]
    fn a_letterboxed_image_is_not_a_waste() {
        // Oversized in each direction and not by area: an image far wider than
        // its box but no taller is being letterboxed, which the developer
        // chose.
        let wide = info((100.0, 100.0), (1000.0, 100.0));
        assert!(!wide.is_oversized(), "ten times the area, and still not it");
        assert!(wide.wasted_bytes() > 0, "though it does waste bytes");
    }

    #[test]
    fn an_image_smaller_than_its_box_wastes_nothing() {
        let small = info((100.0, 100.0), (50.0, 50.0));
        assert!(!small.is_oversized());
        assert_eq!(small.wasted_bytes(), 0, "saturating, not negative");
    }
}

#[cfg(test)]
mod render_comparison_tests {
    use crate::engine::{Color, TextAlign, TextDecoration, TextStyle};
    use crate::painting::RenderComparison;

    const LADDER: [RenderComparison; 4] = [
        RenderComparison::Identical,
        RenderComparison::Metadata,
        RenderComparison::Paint,
        RenderComparison::Layout,
    ];

    #[test]
    fn the_four_are_a_ladder_and_layout_is_the_top() {
        for (rung, comparison) in LADDER.iter().enumerate() {
            assert_eq!(comparison.rank() as usize, rung, "{comparison:?}");
        }
        for comparison in LADDER {
            assert!(comparison.rank() <= RenderComparison::WORST.rank());
        }
        assert_eq!(RenderComparison::WORST, RenderComparison::Layout);
    }

    #[test]
    fn two_differences_do_not_add_up_to_a_worse_one() {
        // The walk keeps a running maximum, not a total.
        for a in LADDER {
            for b in LADDER {
                let worse = a.worse_of(b);
                assert!(worse == a || worse == b, "{a:?} {b:?}");
                assert!(worse.rank() >= a.rank() && worse.rank() >= b.rank());
                assert_eq!(worse, b.worse_of(a), "order should not matter");
            }
            assert_eq!(a.worse_of(a), a);
            assert_eq!(a.worse_of(RenderComparison::Identical), a);
            assert_eq!(
                a.worse_of(RenderComparison::Layout),
                RenderComparison::Layout
            );
        }
    }

    #[test]
    fn the_early_exit_is_sound_because_nothing_beats_layout() {
        // Upstream stops walking children the moment it reaches layout. That
        // is exact rather than approximate: no later child could raise it.
        assert!(RenderComparison::Layout.is_final());
        for comparison in LADDER {
            assert_eq!(
                RenderComparison::Layout.worse_of(comparison),
                RenderComparison::Layout,
                "{comparison:?} could not have raised it"
            );
            if comparison != RenderComparison::Layout {
                assert!(!comparison.is_final(), "{comparison:?}");
            }
        }
    }

    #[test]
    fn needing_layout_already_means_needing_paint() {
        // Which is why upstream's consumer can write `if layout ... else if
        // paint` and not lose the repaint.
        assert!(RenderComparison::Layout.needs_layout());
        assert!(RenderComparison::Layout.needs_paint());
        assert!(!RenderComparison::Paint.needs_layout());
        assert!(RenderComparison::Paint.needs_paint());
        for comparison in LADDER {
            if comparison.needs_layout() {
                assert!(comparison.needs_paint(), "{comparison:?}");
            }
        }
    }

    #[test]
    fn and_the_two_quiet_rungs_ask_for_no_work() {
        // Metadata is the interesting one: a real difference that costs
        // nothing to draw. Nothing in this port produces it yet.
        for quiet in [RenderComparison::Identical, RenderComparison::Metadata] {
            assert!(!quiet.needs_layout(), "{quiet:?}");
            assert!(!quiet.needs_paint(), "{quiet:?}");
        }
        // But it is still a difference, and it still outranks identical.
        assert!(RenderComparison::Metadata.rank() > RenderComparison::Identical.rank());
    }

    #[test]
    fn a_colour_is_a_repaint_and_a_size_is_a_relayout() {
        let base = TextStyle::default();
        assert_eq!(base.compare_to(&base), RenderComparison::Identical);

        let recoloured = TextStyle {
            color: Color(0xFF00_FF00),
            ..TextStyle::default()
        };
        assert_ne!(recoloured.color, base.color);
        assert_eq!(base.compare_to(&recoloured), RenderComparison::Paint);

        let resized = TextStyle {
            font_size: base.font_size + 1.0,
            ..TextStyle::default()
        };
        assert_eq!(base.compare_to(&resized), RenderComparison::Layout);
    }

    #[test]
    fn and_a_style_differing_both_ways_answers_with_the_worse() {
        let base = TextStyle::default();
        let both = TextStyle {
            color: Color(0xFF00_FF00),
            font_size: base.font_size + 1.0,
            ..TextStyle::default()
        };
        assert_eq!(base.compare_to(&both), RenderComparison::Layout);
        // And it is genuinely different in the paint way too, so this is the
        // maximum doing its job rather than the layout test firing alone.
        let colour_only = TextStyle {
            color: both.color,
            ..TextStyle::default()
        };
        assert_eq!(base.compare_to(&colour_only), RenderComparison::Paint);
    }

    #[test]
    fn every_layout_field_really_is_a_layout_field() {
        let base = TextStyle::default();
        let variants = [
            TextStyle {
                font_family: Some("Serif".into()),
                ..TextStyle::default()
            },
            TextStyle {
                font_weight: 700,
                ..TextStyle::default()
            },
            TextStyle {
                italic: true,
                ..TextStyle::default()
            },
            TextStyle {
                letter_spacing: Some(1.0),
                ..TextStyle::default()
            },
            TextStyle {
                word_spacing: Some(1.0),
                ..TextStyle::default()
            },
            TextStyle {
                height: Some(2.0),
                ..TextStyle::default()
            },
            TextStyle {
                align: TextAlign::Center,
                ..TextStyle::default()
            },
        ];
        for variant in variants {
            assert_eq!(
                base.compare_to(&variant),
                RenderComparison::Layout,
                "{variant:?}"
            );
        }
        // And the decoration is on the other side of the line.
        let underlined = TextStyle {
            decoration: TextDecoration::UNDERLINE,
            ..TextStyle::default()
        };
        assert_eq!(base.compare_to(&underlined), RenderComparison::Paint);
    }
}

#[cfg(test)]
mod image_repeat_tests {
    use crate::painting::{DecorationImage, ImageRepeat};

    const ALL: [ImageRepeat; 4] = [
        ImageRepeat::Repeat,
        ImageRepeat::RepeatX,
        ImageRepeat::RepeatY,
        ImageRepeat::NoRepeat,
    ];

    #[test]
    fn the_tiler_asks_each_axis_separately() {
        // Upstream never matches on four cases; it asks `repeat == repeat ||
        // repeat == repeatX` and the same for y. `Repeat` is just the value
        // that answers yes twice.
        assert!(ImageRepeat::Repeat.repeats_x() && ImageRepeat::Repeat.repeats_y());
        assert!(ImageRepeat::RepeatX.repeats_x() && !ImageRepeat::RepeatX.repeats_y());
        assert!(!ImageRepeat::RepeatY.repeats_x() && ImageRepeat::RepeatY.repeats_y());
        assert!(!ImageRepeat::NoRepeat.repeats_x() && !ImageRepeat::NoRepeat.repeats_y());
        // And the four values are exactly the four (x, y) answers -- no two
        // of them agree on both axes.
        let mut answers: Vec<(bool, bool)> =
            ALL.iter().map(|r| (r.repeats_x(), r.repeats_y())).collect();
        answers.sort();
        answers.dedup();
        assert_eq!(answers.len(), 4);
    }

    #[test]
    fn an_axis_that_does_not_repeat_still_draws_one_tile() {
        // The range is inclusive, so (0, 0) is one tile rather than none.
        // noRepeat draws the image; it does not skip it.
        let still = ImageRepeat::tile_range(false, 0.0, 500.0, 0.0, 50.0, 50.0);
        assert_eq!(still, (0, 0));
        assert_eq!(ImageRepeat::tile_count(still), 1);
        assert_eq!(ImageRepeat::tile_count((0, 0)), 1);
    }

    #[test]
    fn the_two_ends_are_measured_from_different_edges() {
        // Upstream: start from (output.left - tile.left), stop from
        // (output.right - tile.right). A tile sitting at the box's origin,
        // 50 wide, in a 500-wide box: the far end is (500 - 50) / 50 = 9, so
        // tiles 0..=9, which is ten -- exactly covering 500.
        let range = ImageRepeat::tile_range(true, 0.0, 500.0, 0.0, 50.0, 50.0);
        assert_eq!(range, (0, 9));
        assert_eq!(ImageRepeat::tile_count(range), 10);

        // Measuring both ends from the tile's near edge would have given
        // (0, 10) -- eleven tiles, one more than the box can show.
        let wrong_way = (0, (500.0f32 / 50.0).ceil() as i32);
        assert_eq!(wrong_way, (0, 10));
        assert_ne!(range, wrong_way);
    }

    #[test]
    fn and_a_box_that_does_not_divide_evenly_gets_a_tile_that_overhangs() {
        // 500 wide, 60-wide tiles: (500 - 60) / 60 = 7.33, ceil 8, so 0..=8 is
        // nine tiles covering 540. The last one hangs over, which is what the
        // ceiling is for -- a floor would leave a gap.
        let range = ImageRepeat::tile_range(true, 0.0, 500.0, 0.0, 60.0, 60.0);
        assert_eq!(range, (0, 8));
        assert_eq!(ImageRepeat::tile_count(range) * 60, 540);
        assert!(ImageRepeat::tile_count(range) * 60 >= 500, "no gap left");
    }

    #[test]
    fn a_tile_starting_before_the_box_gets_a_negative_index() {
        // The image is centred, so its rect starts left of the box. The floor
        // then runs the index negative rather than clipping to zero.
        let range = ImageRepeat::tile_range(true, 0.0, 100.0, -25.0, 25.0, 50.0);
        // floor((0 - -25) / 50) = 0 and ceil((100 - 25) / 50) = 2: three tiles
        // at -25, 25 and 75, the first and last overhanging the box.
        assert_eq!(range, (0, 2));
        assert_eq!(ImageRepeat::tile_count(range), 3);
        let leftwards = ImageRepeat::tile_range(true, -100.0, 100.0, 0.0, 50.0, 50.0);
        assert_eq!(leftwards, (-2, 1));
        assert_eq!(ImageRepeat::tile_count(leftwards), 4);
    }

    #[test]
    fn filling_the_box_exactly_turns_repeating_off() {
        // Upstream downgrades to noRepeat before generating anything, because
        // an image already filling its box gains nothing from a grid.
        for repeat in ALL {
            assert_eq!(
                repeat.collapsed_when_exactly_filled(true),
                ImageRepeat::NoRepeat,
                "{repeat:?}"
            );
            assert_eq!(repeat.collapsed_when_exactly_filled(false), repeat);
        }
        // And the collapse is observable: it is one tile instead of a grid.
        let collapsed = ImageRepeat::Repeat.collapsed_when_exactly_filled(true);
        assert_eq!(
            ImageRepeat::tile_count(ImageRepeat::tile_range(
                collapsed.repeats_x(),
                0.0,
                500.0,
                0.0,
                50.0,
                50.0
            )),
            1
        );
    }

    #[test]
    fn a_decoration_image_does_not_tile_unless_asked() {
        // Upstream's constructors all start from `ImageRepeat.noRepeat`.
        assert_eq!(ImageRepeat::default(), ImageRepeat::NoRepeat);
        let image = DecorationImage::new(crate::image::ImageProvider::Asset {
            key: "x".into(),
            scale: 1.0,
            bundle: None,
        });
        assert_eq!(image.repeat, ImageRepeat::NoRepeat);
    }

    #[test]
    fn a_zero_stride_cannot_be_tiled() {
        // Guards the division: an empty tile would repeat forever.
        assert_eq!(
            ImageRepeat::tile_range(true, 0.0, 500.0, 0.0, 0.0, 0.0),
            (0, 0)
        );
    }
}

// -- What a DecorationImage puts on the canvas --------------------------------

#[cfg(test)]
mod decoration_image_paint_tests {
    //! `DecorationImage::paint` was one draw call nothing could see, and it
    //! could not have been seen even with a recorder: the stub's decoder
    //! reported every provider's picture as nought by nought, so `apply_box_fit`
    //! was fitting an empty image into a box and agreed with any answer.
    //!
    //! `engine_test_stubs::encoded_image` gives a provider a picture with a
    //! shape. What is pinned here is the pair `ImageRect` carries: the source
    //! window in **image pixels** and where it lands in **logical ones**, which
    //! is the mix-up this recorder was built for in the first place.
    //!
    //! Both are `(left, top, right, bottom)`. The first draft of this module
    //! read them as `(x, y, width, height)` and three of its tests failed with
    //! the right numbers in the wrong slots, which is the more useful of the
    //! two ways to find out.

    use super::{BoxFit, DecorationImage};
    use crate::direction::TextDirection;
    use crate::engine::{LayerTree, Rect};
    use crate::engine_test_stubs::{Drawn, drawn, encoded_image, reset_drawn};
    use crate::image::ImageProvider;
    use crate::render::{Alignment, AlignmentGeometry, PaintContext, Size};
    use std::rc::Rc;

    /// A box twice as wide as it is tall.
    const BOX: Rect = Rect {
        left: 10.0,
        top: 20.0,
        right: 210.0,
        bottom: 120.0,
    };

    fn provider(width: u16, height: u16) -> ImageProvider {
        ImageProvider::Memory {
            bytes: Rc::new(encoded_image(width, height)),
            scale: 1.0,
        }
    }

    #[allow(clippy::type_complexity)]
    fn painted(
        decoration: DecorationImage,
    ) -> Option<((f32, f32, f32, f32), (f32, f32, f32, f32))> {
        let mut layers = LayerTree::new(400, 400);
        reset_drawn();
        let drew = {
            let mut context = PaintContext::new(&mut layers, Size::new(400.0, 400.0));
            decoration.paint(context.canvas(), BOX, TextDirection::Ltr)
        };
        let calls = drawn();
        let found = calls.iter().find_map(|call| match call {
            Drawn::ImageRect {
                source,
                destination,
            } => Some((*source, *destination)),
            _ => None,
        });
        assert_eq!(
            drew,
            found.is_some(),
            "the return value and the canvas disagree: {calls:?}"
        );
        found
    }

    /// `(right - left, bottom - top)`. Both rectangles the recorder carries
    /// are `(left, top, right, bottom)`.
    fn size_of_rect(rect: (f32, f32, f32, f32)) -> (f32, f32) {
        (rect.2 - rect.0, rect.3 - rect.1)
    }

    #[test]
    fn the_source_window_is_the_whole_picture_in_its_own_pixels() {
        // The unit mix-up this recorder exists for. Taken in logical pixels
        // instead, a picture wider than its box is cropped rather than
        // scaled -- and every test that only looked at the destination passed.
        let (source, _) = painted(DecorationImage::new(provider(64, 32))).expect("a draw");
        assert_eq!(source, (0.0, 0.0, 64.0, 32.0));
    }

    #[test]
    fn a_fit_that_was_not_given_is_scale_down_rather_than_fill() {
        // Upstream documents `null` fit as `scaleDown`, which is the one fit
        // that leaves a small picture alone. `fill` in its place stretches
        // every icon to the shape of its box.
        let (_, destination) = painted(DecorationImage::new(provider(40, 20))).expect("a draw");
        assert_eq!(
            size_of_rect(destination),
            (40.0, 20.0),
            "a picture smaller than the box is not grown"
        );

        let filled =
            painted(DecorationImage::new(provider(40, 20)).with_fit(BoxFit::Fill)).expect("a draw");
        assert_eq!(
            size_of_rect(filled.1),
            (BOX.width(), BOX.height()),
            "and fill is a different answer, so the default is doing something"
        );
    }

    #[test]
    fn and_scale_down_does_shrink_one_that_is_too_big() {
        // The other half of `scaleDown`: it is `contain` for anything larger.
        // A test that only tried a small picture could not tell the two apart.
        let (_, destination) = painted(DecorationImage::new(provider(400, 400))).expect("a draw");
        let (width, height) = size_of_rect(destination);
        assert_eq!(height, BOX.height(), "fitted to the shorter side");
        assert_eq!(width, BOX.height(), "and kept square");
        assert!(width < BOX.width());
    }

    #[test]
    fn the_picture_is_centred_on_the_alignment_and_not_on_the_box() {
        // `alignment` picks a point in the box and the fitted picture is hung
        // around it. The default is the centre; anything else has to move.
        let centre = painted(DecorationImage::new(provider(400, 400))).expect("a draw");
        let (left, top, right, bottom) = centre.1;
        assert_eq!((left + right) / 2.0, (BOX.left + BOX.right) / 2.0);
        assert_eq!((top + bottom) / 2.0, (BOX.top + BOX.bottom) / 2.0);

        let left_aligned = painted(
            DecorationImage::new(provider(400, 400))
                .with_alignment(AlignmentGeometry::Absolute(Alignment::CENTER_LEFT)),
        )
        .expect("a draw");
        assert!(
            left_aligned.1.0 < left,
            "aligned to the start, it moves that way: {} against {left}",
            left_aligned.1.0
        );
        assert_eq!(left_aligned.1.1, top, "and not vertically");
    }

    #[test]
    fn a_provider_that_cannot_load_draws_nothing_and_says_so() {
        // The return value is what a decoration painter above uses to decide
        // whether to draw a placeholder, so "drew nothing" and "returned
        // false" have to be the same event -- which is what `painted` asserts
        // on every call in this module.
        let missing = ImageProvider::File {
            path: "no/such/picture.png".to_string(),
            scale: 1.0,
        };
        assert!(painted(DecorationImage::new(missing)).is_none());
    }

    #[test]
    fn but_a_picture_that_decoded_to_nothing_is_still_a_picture() {
        // The distinction the test above is not: a payload this stub does not
        // recognise decodes to an image of nought by nought rather than to a
        // failure, so the paint happens and draws an empty window. Worth
        // saying out loud, because a test that asserted "no picture, no draw"
        // against *this* case would be asserting the wrong thing about the
        // wrong path.
        let unrecognised = ImageProvider::Memory {
            bytes: Rc::new(b"not a picture".to_vec()),
            scale: 1.0,
        };
        let (source, _) = painted(DecorationImage::new(unrecognised)).expect("it still draws");
        assert_eq!(size_of_rect(source), (0.0, 0.0));
    }
}

// -- The two code tables, and which way a cylinder turns ----------------------

#[cfg(test)]
mod abi_table_tests {
    //! Numbers that leave this crate and are read in C++.
    //!
    //! `variant_sweep` found seven arms in this file that nothing was looking
    //! at, and six of them were these two tables: every row but the first
    //! could take its neighbour's number with the whole suite green. That is
    //! the shape the sweep's own docs put first -- a table this side writes
    //! and only the engine reads -- and the reason is structural rather than
    //! an oversight, so the fix is a test rather than a rule.

    use super::matrix_utils;
    use super::{ClipBehavior, TileMode};
    use crate::render::{Axis, Offset};

    #[test]
    fn every_tile_mode_sends_the_number_to_tile_mode_reads() {
        // `ToTileMode` in src/flutter/rust/ffi/rustflutter_ffi_draw.cc:
        // 1 repeat, 2 mirror, 3 decal, anything else clamp. Clamp is the
        // default arm there, so its number is the one this side chooses.
        assert_eq!(TileMode::ALL.map(TileMode::code), [0, 1, 2, 3]);
    }

    #[test]
    fn every_clip_behaviour_sends_the_number_to_clip_behaviour_reads() {
        // `ToClipBehavior` in the same file: 0 none, 1 hard edge,
        // 3 anti-alias with a save layer, anything else plain anti-alias.
        assert_eq!(ClipBehavior::ALL.map(ClipBehavior::code), [0, 1, 2, 3]);
    }

    #[test]
    fn and_no_two_rows_of_either_table_share_a_number() {
        // What makes a neighbour swap detectable at all. Two tile modes with
        // one code is a gradient the engine cannot tell apart from another.
        for (index, one) in TileMode::ALL.iter().enumerate() {
            for other in TileMode::ALL.iter().skip(index + 1) {
                assert_ne!(one.code(), other.code(), "{one:?} and {other:?}");
            }
        }
        for (index, one) in ClipBehavior::ALL.iter().enumerate() {
            for other in ClipBehavior::ALL.iter().skip(index + 1) {
                assert_ne!(one.code(), other.code(), "{one:?} and {other:?}");
            }
        }
    }

    /// Where a point off the axis of rotation lands, once the cylinder has
    /// turned. A quarter turn, so the movement is unmistakable.
    fn turned(orientation: Axis, point: Offset) -> Offset {
        let transform = matrix_utils::create_cylindrical_projection_transform(
            100.0,
            std::f32::consts::FRAC_PI_4,
            0.001,
            orientation,
        );
        matrix_utils::transform_point(transform, point)
    }

    #[test]
    fn a_horizontal_wheel_turns_about_the_upright_axis_and_a_vertical_one_about_the_flat() {
        // Upstream's `createCylindricalProjectionTransform`: horizontal is
        // `rotationY`, vertical is `rotationX`. The vertical arm could take
        // the horizontal one's with the suite green -- a list wheel that
        // scrolls up and down would have spun sideways, and every existing
        // test of this matrix used the default orientation.
        //
        // The model translates every point out to `z = radius` before it
        // turns, so both rotations move a point's x -- the first draft of this
        // test assumed otherwise and failed. What separates them is which
        // coordinate stays at **zero**: turning about the upright axis leaves
        // y alone, so a point on the horizon lands on the horizon; turning
        // about the flat axis leaves x alone, so a point on the centre line
        // stays on it.
        let on_the_horizon = Offset::new(40.0, 0.0);
        let on_the_centre_line = Offset::new(0.0, 40.0);

        assert_eq!(
            turned(Axis::Horizontal, on_the_horizon).dy,
            0.0,
            "an upright axis does not lift a point off the horizon"
        );
        assert!(
            turned(Axis::Vertical, on_the_horizon).dy.abs() > 1.0,
            "and a flat one does: {:?}",
            turned(Axis::Vertical, on_the_horizon)
        );

        assert_eq!(
            turned(Axis::Vertical, on_the_centre_line).dx,
            0.0,
            "a flat axis does not push a point off the centre line"
        );
        assert!(
            turned(Axis::Horizontal, on_the_centre_line).dx.abs() > 1.0,
            "and an upright one does: {:?}",
            turned(Axis::Horizontal, on_the_centre_line)
        );
    }
}

// -- The tables whose wire format is the declaration order --------------------

#[cfg(test)]
mod discriminant_table_tests {
    //! Five enums whose *number* crosses the FFI without a `match` anywhere.
    //!
    //! `variant_sweep` rewrites match arms, so none of these is visible to it:
    //! `stroke_cap as c_int` has no arms to rewrite. That is the same blind
    //! spot `PlatformProvidedMenuItemType` fell into, and it is worth saying
    //! that these were found by grepping for the *shape* -- a discriminant
    //! cast at an FFI call -- rather than by either queue.
    //!
    //! Every number below is checked against the C++ that reads it, and the
    //! file and switch are named so the pair can be re-read together.

    use super::{BlendMode, ClipOp, FillType, StrokeCap, StrokeJoin};

    #[test]
    fn the_stroke_caps_are_the_numbers_the_paint_setter_reads() {
        // `rf_paint_set_stroke_cap` in rustflutter_ffi_draw.cc: 1 round,
        // 2 square, anything else butt. Butt's number is this side's choice,
        // being the default arm there.
        assert_eq!(StrokeCap::Butt as i32, 0);
        assert_eq!(StrokeCap::Round as i32, 1);
        assert_eq!(StrokeCap::Square as i32, 2);
    }

    #[test]
    fn the_stroke_joins_are_the_numbers_the_paint_setter_reads() {
        // Same file: 1 round, 2 bevel, anything else miter. Note that round is
        // 1 in **both** tables and square/bevel are 2 -- so a cap and a join
        // cannot be told apart by their numbers, only by which setter they are
        // handed to.
        assert_eq!(StrokeJoin::Miter as i32, 0);
        assert_eq!(StrokeJoin::Round as i32, 1);
        assert_eq!(StrokeJoin::Bevel as i32, 2);
    }

    #[test]
    fn a_path_fills_by_the_non_zero_rule_unless_it_says_otherwise() {
        // `rf_path_set_fill_type`: `fill_type == 1` is odd, everything else is
        // non-zero. The two rules disagree about the inside of a
        // self-intersecting path -- a five-pointed star is solid under one and
        // hollow in the middle under the other.
        assert_eq!(FillType::NonZero as i32, 0);
        assert_eq!(FillType::EvenOdd as i32, 1);
        assert_eq!(FillType::default(), FillType::NonZero);
    }

    #[test]
    fn a_clip_keeps_what_is_inside_it_unless_it_says_otherwise() {
        // `ToClipOp`: `clip_op == 1` is difference, everything else is
        // intersect. Getting this backwards shows the whole screen except the
        // part that was meant to be visible.
        assert_eq!(ClipOp::Intersect as i32, 0);
        assert_eq!(ClipOp::Difference as i32, 1);
        assert_eq!(ClipOp::default(), ClipOp::Intersect);
    }

    #[test]
    fn every_blend_mode_is_its_position_in_dart_uis_list() {
        // `rf_paint_set_blend_mode` does a `static_cast` straight to
        // `flutter::DlBlendMode`, so these are not codes this side chose --
        // they are the engine's enum, and dart:ui's, spelled again.
        assert_eq!(BlendMode::Clear as i32, 0);
        assert_eq!(BlendMode::SrcOver as i32, 3);
        assert_eq!(BlendMode::default(), BlendMode::SrcOver);
        assert_eq!(BlendMode::Modulate as i32, 13);
        assert_eq!(BlendMode::Multiply as i32, 24);
        // The four this port was missing, and the reason it stopped: Multiply
        // is upstream's last separable mode.
        assert_eq!(BlendMode::Hue as i32, 25);
        assert_eq!(BlendMode::Saturation as i32, 26);
        assert_eq!(BlendMode::Color as i32, 27);
        assert_eq!(BlendMode::Luminosity as i32, 28);
    }

    #[test]
    fn and_the_list_runs_from_zero_without_a_gap() {
        // A gap would be a mode the engine reads as its neighbour, and an
        // explicit discriminant is exactly how one gets introduced. Twenty-nine
        // of them, 0 through 28, which is `DlBlendMode::kLastMode` -- the guard
        // in `rf_paint_set_blend_mode` drops anything above it without saying
        // so, which is why a number too large fails silently rather than
        // loudly.
        let modes = [
            BlendMode::Clear as i32,
            BlendMode::Src as i32,
            BlendMode::Dst as i32,
            BlendMode::SrcOver as i32,
            BlendMode::DstOver as i32,
            BlendMode::SrcIn as i32,
            BlendMode::DstIn as i32,
            BlendMode::SrcOut as i32,
            BlendMode::DstOut as i32,
            BlendMode::SrcATop as i32,
            BlendMode::DstATop as i32,
            BlendMode::Xor as i32,
            BlendMode::Plus as i32,
            BlendMode::Modulate as i32,
            BlendMode::Screen as i32,
            BlendMode::Overlay as i32,
            BlendMode::Darken as i32,
            BlendMode::Lighten as i32,
            BlendMode::ColorDodge as i32,
            BlendMode::ColorBurn as i32,
            BlendMode::HardLight as i32,
            BlendMode::SoftLight as i32,
            BlendMode::Difference as i32,
            BlendMode::Exclusion as i32,
            BlendMode::Multiply as i32,
            BlendMode::Hue as i32,
            BlendMode::Saturation as i32,
            BlendMode::Color as i32,
            BlendMode::Luminosity as i32,
        ];
        assert_eq!(modes.len(), 29);
        for (position, number) in modes.iter().enumerate() {
            assert_eq!(*number, position as i32, "at {position}");
        }
    }
}
