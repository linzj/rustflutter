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
    max_lines: usize,
    max_width_bits: u32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct RunKey {
    text: String,
    family: Option<String>,
    size_bits: u32,
    weight: i32,
    color: u32,
}

impl RunKey {
    fn new(text: &str, style: &TextStyle) -> RunKey {
        RunKey {
            text: text.to_string(),
            family: style.font_family.clone(),
            size_bits: style.font_size.to_bits(),
            weight: style.font_weight,
            color: style.color.0,
        }
    }
}

fn align_code(align: TextAlign) -> u8 {
    match align {
        TextAlign::Left => 0,
        TextAlign::Right => 1,
        TextAlign::Center => 2,
    }
}

impl ShapeKey {
    fn new(text: &str, style: &TextStyle, max_width: f32) -> ShapeKey {
        ShapeKey {
            runs: vec![RunKey::new(text, style)],
            align: align_code(style.align),
            max_lines: 0,
            max_width_bits: max_width.to_bits(),
        }
    }

    fn rich(
        runs: &[(String, TextStyle)],
        align: TextAlign,
        max_lines: Option<usize>,
        max_width: f32,
    ) -> ShapeKey {
        ShapeKey {
            runs: runs.iter().map(|(text, style)| RunKey::new(text, style)).collect(),
            align: align_code(align),
            max_lines: max_lines.unwrap_or(0),
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
/// The platform's text scale is applied here, which makes this the one place
/// in the framework that obeys it -- every size on screen is a font size that
/// came through this function, and every measurement the framework makes comes
/// back out of the paragraph rather than out of the style.
///
/// It belongs one layer up. Upstream it is `MediaQuery.textScaler`, read by
/// each `Text` from the widget tree, so a subtree can be given a different one
/// -- a dense table that opts out, a preview that shows what another size would
/// look like. [`crate::media_query::MediaQueryData`] carries the scale now and
/// a subtree can publish its own, but nothing reads it here: `Text` is a render
/// object built inside a closure, with no `BuildContext` to read it from, and
/// the scale is needed at shaping time rather than at build time. Applying it
/// to all text is the closest thing to right until `Text` is a widget that
/// knows where it is; the alternative is ignoring an accessibility setting the
/// reader has already asked every application for.
///
/// The cache needs no help with this: the scale changes the style it keys on,
/// so text shaped at the old size is simply never asked for again.
pub fn shape(text: &str, style: &TextStyle, max_width: f32) -> Rc<Paragraph> {
    let scale = crate::platform::text_scale_factor() as f32;
    let scaled;
    let style = if scale == 1.0 {
        style
    } else {
        scaled = TextStyle { font_size: style.font_size * scale, ..style.clone() };
        &scaled
    };
    let key = ShapeKey::new(text, style, max_width);
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
        let shaped = Rc::new(Paragraph::new(text, style, max_width));
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
    max_width: f32,
) -> Rc<Paragraph> {
    let scale = crate::platform::text_scale_factor() as f32;
    let scaled: Vec<(String, TextStyle)> = if scale == 1.0 {
        runs.to_vec()
    } else {
        runs.iter()
            .map(|(text, style)| {
                (
                    text.clone(),
                    TextStyle { font_size: style.font_size * scale, ..style.clone() },
                )
            })
            .collect()
    };
    let key = ShapeKey::rich(&scaled, align, max_lines, max_width);
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
        let shaped = Rc::new(Paragraph::rich(&scaled, align, max_lines, max_width));
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
                            let Ok(receiver) = requests.lock() else { return };
                            receiver.recv()
                        };
                        let Ok(request) = request else {
                            // The cache went away with its thread.
                            return;
                        };
                        let image = Image::decode(&request.data);
                        let decoded =
                            Decoded { key: request.key, image: Handoff(image) };
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
        let request = Request { key: key.to_string(), data: data.to_vec() };
        match self.requests.send(request) {
            Ok(()) => {
                self.entries.insert(key.to_string(), Slot::Decoding);
                self.outstanding += 1;
                None
            }
            Err(returned) => {
                // No workers came up. Decode here rather than never.
                let image = Image::decode(&returned.0.data).map(Rc::new);
                self.entries.insert(key.to_string(), Slot::Done(image.clone()));
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
        self.entries.insert(key.to_string(), Slot::Done(image.clone()));
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

/// Whether any image asked for is still being decoded.
///
/// A frame that sees this true has drawn without an image it wanted and should
/// ask for another; that is how the picture arrives once it is ready.
pub fn images_pending() -> bool {
    IMAGES.with(|images| {
        let mut images = images.borrow_mut();
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
    IMAGES.with(|images| {
        let mut images = images.borrow_mut();
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
    IMAGES.with(|images| {
        let mut images = images.borrow_mut();
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
        if raw.is_null() { None } else { Some(Image { raw }) }
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
        let needed = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
        if pixels.len() < needed {
            return None;
        }
        let raw = unsafe { sys::rf_image_from_pixels(pixels.as_ptr(), width, height) };
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
        IMAGES.with(|images| images.borrow_mut().get_or_request(key, data))
    }

    /// Decodes `data` on this thread, blocking until it is done.
    ///
    /// For the paths that have exactly one frame to get right and no next frame
    /// to fall back on -- a headless render, a golden test.
    pub fn shared_now(key: &str, data: &[u8]) -> Option<Rc<Image>> {
        IMAGES.with(|images| images.borrow_mut().get_or_decode(key, data))
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

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        let first = shape("shaped once", &style, 200.0);
        let second = shape("shaped once", &style, 200.0);
        assert!(Rc::ptr_eq(&first, &second), "the second ask re-shaped");
    }

    #[test]
    fn a_different_width_is_a_different_paragraph() {
        let style = TextStyle::default();
        let narrow = shape("wraps differently", &style, 100.0);
        let wide = shape("wraps differently", &style, 400.0);
        // Line breaking depends on the width, so sharing one shaping between
        // two widths would put the breaks in the wrong place.
        assert!(!Rc::ptr_eq(&narrow, &wide));
    }

    #[test]
    fn a_different_style_is_a_different_paragraph() {
        let plain = TextStyle::default();
        let bold = TextStyle { font_weight: 700, ..TextStyle::default() };
        let a = shape("weight matters", &plain, 200.0);
        let b = shape("weight matters", &bold, 200.0);
        assert!(!Rc::ptr_eq(&a, &b));
    }

    #[test]
    fn text_still_on_screen_survives_a_frame() {
        let style = TextStyle::default();
        let first = shape("still drawn", &style, 200.0);
        end_text_frame();
        let second = shape("still drawn", &style, 200.0);
        assert!(Rc::ptr_eq(&first, &second), "a live paragraph was re-shaped");
    }

    #[test]
    fn the_readers_text_size_reaches_the_shaper() {
        // The setting has one consumer and this is it. Checked through the
        // cache rather than through a metric, because the stubbed engine every
        // unit test shapes against reports zero for every measurement -- what
        // can be shown is that the scale is part of the request, which is what
        // decides the size the engine is asked for.
        let style = TextStyle::default();
        let before = shaped_paragraph_count();
        let unscaled = shape("the reader's size", &style, 200.0);
        assert_eq!(shaped_paragraph_count(), before + 1);

        crate::platform::set_user_settings(r#"{"textScaleFactor":1.5}"#);
        let scaled = shape("the reader's size", &style, 200.0);
        assert!(
            !Rc::ptr_eq(&unscaled, &scaled),
            "the same text at a different size must be shaped again"
        );
        assert_eq!(shaped_paragraph_count(), before + 2);

        // And back, which the cache still has: the scale changes the style the
        // entry is keyed on rather than invalidating anything.
        crate::platform::set_user_settings(r#"{"textScaleFactor":1.0}"#);
        assert!(Rc::ptr_eq(&unscaled, &shape("the reader's size", &style, 200.0)));
        crate::platform::reset();
    }

    #[test]
    fn text_that_stopped_being_drawn_is_dropped() {
        let style = TextStyle::default();
        let before = shaped_paragraph_count();
        let _ = shape("shown briefly", &style, 200.0);
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
        assert!(Image::shared("async:first", PNG).is_some(), "and then it arrives");
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
