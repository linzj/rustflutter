// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Paths, gradients, images, and the canvas state stack.
//!
//! Upstream this is the drawing half of `dart:ui` -- `Path`, `Gradient`,
//! `Image`, and the transform/clip/save methods on `Canvas`. The engine objects
//! underneath are the same ones; only the way the arguments arrive changes.

use std::os::raw::c_int;

use crate::engine::{Canvas, Color, LayerTree, Paint, Rect, sys};

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
    fn code(self) -> c_int {
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
    Multiply = 24,
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
    fn code(self) -> c_int {
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
#[derive(Clone, Debug)]
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

    pub fn add_rounded_rect(&mut self, rect: Rect, radius_x: f32, radius_y: f32) -> &mut RenderPath {
        unsafe {
            sys::rf_path_add_rounded_rect(
                self.raw, rect.left, rect.top, rect.right, rect.bottom, radius_x, radius_y,
            )
        };
        self
    }
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

/// A decoded image.
pub struct Image {
    raw: *mut sys::RfImage,
}

impl Image {
    /// Decodes PNG, JPEG, WebP, GIF or BMP bytes. Returns None if the format
    /// was not recognised or the data was truncated.
    pub fn decode(data: &[u8]) -> Option<Image> {
        let raw = unsafe { sys::rf_image_decode(data.as_ptr(), data.len()) };
        if raw.is_null() { None } else { Some(Image { raw }) }
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
            sys::rf_canvas_draw_oval(self.raw, rect.left, rect.top, rect.right, rect.bottom, paint.raw)
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
