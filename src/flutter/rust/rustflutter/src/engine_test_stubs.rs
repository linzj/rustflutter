// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Inert stand-ins for the engine C ABI, compiled under `cfg(test)` and under
//! `--cfg rustflutter_stubs`.
//!
//! The crate's `#[test]` binary is built by rustc directly and does not link
//! the C++ engine, but `RenderBox` is a trait object, so every implementor's
//! `paint` lands in a vtable and every engine symbol it reaches has to
//! resolve. These stubs make that work, and in doing so make the layout and
//! hit-testing logic testable without a GPU, a font stack or an ICU bundle.
//!
//! They are deliberately inert rather than plausible: an allocator returns a
//! unique non-null handle so `Drop` stays sound, everything else does nothing
//! and every metric reads zero. Faking metrics would let a test pass by
//! agreeing with the fake. What the engine actually draws is covered by
//! `rust/ffi_unittests.cc`, which links the real thing and reads pixels back.

#![allow(unused_variables)]

// The window host, stubbed for the same reason as the rest. Under plain
// `cfg(test)` the crate compiles no call to this, but a dependent crate built
// with `--cfg rustflutter_stubs` still has `run` in it, and a `main` that calls
// it. Returning non-zero rather than zero means a test that reaches this by
// accident fails rather than passing against a window that never opened.
#[cfg(rustflutter_stubs)]
#[unsafe(no_mangle)]
pub extern "C" fn rf_host_run(_options: *const std::ffi::c_void) -> std::os::raw::c_int {
    -1
}

use std::os::raw::{c_char, c_int};

use crate::engine::sys::*;

/// Backing allocation for a stub handle. One byte, so distinct handles have
/// distinct addresses and a double free is caught by the allocator.
/// What the stub remembers about an image.
///
/// Every other handle here is a one-byte allocation, because nothing reads
/// anything back out of them. An image is the exception: its width and height
/// are read by the crate itself, and returning zero for both made the whole of
/// image geometry untestable -- `natural`, the box fit, the destination rect
/// and the intrinsics all measure against a size that was always zero, so
/// every one of them agreed with every possible implementation.
///
/// Found while adding `RenderImage::scale`, whose test could not tell a
/// division from a multiplication when both sides were nought.
struct StubImage {
    width: c_int,
    height: c_int,
}

fn stub_image(width: c_int, height: c_int) -> *mut RfImage {
    Box::into_raw(Box::new(StubImage { width, height })) as *mut RfImage
}

/// # Safety
/// `image` must be null or have come from `stub_image` and not been freed.
unsafe fn stub_image_ref<'a>(image: *const RfImage) -> Option<&'a StubImage> {
    if image.is_null() {
        None
    } else {
        Some(unsafe { &*(image as *const StubImage) })
    }
}

fn allocate<T>() -> *mut T {
    Box::into_raw(Box::new(0u8)) as *mut T
}

/// # Safety
/// `handle` must have come from `allocate` and not been released yet.
unsafe fn release<T>(handle: *mut T) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle as *mut u8) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_initialize(icu_data_path: *const c_char) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_new() -> *mut RfPaint {
    Box::into_raw(Box::new(StubPaint { argb: 0 })) as *mut RfPaint
}

/// What the stub remembers about a paint.
///
/// The colour was already kept, but in one thread-local holding whichever
/// paint was coloured last. That answers "what colour was the most recent
/// thing" and not "what colour was *that* rectangle" -- and a paint method
/// that draws two rectangles in two colours is exactly where the difference
/// matters. `track_paints` swapping its two colours in RTL is the case that
/// prompted this: with one global there is nothing to compare.
struct StubPaint {
    argb: u32,
}

/// # Safety
/// `paint` must be null or have come from `rf_paint_new` and not been freed.
unsafe fn stub_paint_ref<'a>(paint: *const RfPaint) -> Option<&'a StubPaint> {
    if paint.is_null() {
        None
    } else {
        Some(unsafe { &*(paint as *const StubPaint) })
    }
}

/// The colour a draw call's paint carried, or fully transparent for a call
/// given no paint at all.
unsafe fn paint_argb(paint: *const RfPaint) -> u32 {
    unsafe { stub_paint_ref(paint) }.map_or(0, |paint| paint.argb)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_free(paint: *mut RfPaint) {
    if !paint.is_null() {
        drop(unsafe { Box::from_raw(paint as *mut StubPaint) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_color(paint: *mut RfPaint, argb: u32) {
    LAST_PAINT_COLOR.with(|c| c.set(argb));
    if let Some(paint) = unsafe { (paint as *mut StubPaint).as_mut() } {
        paint.argb = argb;
    }
}

thread_local! {
    static LAST_PAINT_COLOR: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The ARGB most recently handed to `rf_paint_set_color`, for tests that need
/// to know what colour something was painted.
pub fn last_paint_color() -> u32 {
    LAST_PAINT_COLOR.with(|c| c.get())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_stroke(paint: *mut RfPaint, stroke: c_int, width: f32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_anti_alias(paint: *mut RfPaint, anti_alias: c_int) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_opacity(paint: *mut RfPaint, opacity: f32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_blend_mode(paint: *mut RfPaint, blend_mode: c_int) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_color_filter(
    paint: *mut RfPaint,
    argb: u32,
    blend_mode: c_int,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_clear_color_filter(paint: *mut RfPaint) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_stroke_cap(paint: *mut RfPaint, cap: c_int) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_stroke_join(paint: *mut RfPaint, join: c_int) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_blur(paint: *mut RfPaint, sigma: f32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_clear_blur(paint: *mut RfPaint) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_linear_gradient(
    paint: *mut RfPaint,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    colors: *const u32,
    stops: *const f32,
    stop_count: c_int,
    tile_mode: c_int,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_radial_gradient(
    paint: *mut RfPaint,
    center_x: f32,
    center_y: f32,
    radius: f32,
    colors: *const u32,
    stops: *const f32,
    stop_count: c_int,
    tile_mode: c_int,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_sweep_gradient(
    paint: *mut RfPaint,
    center_x: f32,
    center_y: f32,
    start_degrees: f32,
    end_degrees: f32,
    colors: *const u32,
    stops: *const f32,
    stop_count: c_int,
    tile_mode: c_int,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_clear_shader(paint: *mut RfPaint) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_new() -> *mut RfPath {
    Box::into_raw(Box::new(StubPath { bounds: None })) as *mut RfPath
}

/// What the stub remembers about a path: **where it is, not what it is.**
///
/// Twenty-seven of the crate's draw calls hand the canvas a path, and a path
/// here is a handle with nothing readable behind it. Accumulating a bounding
/// box from the points each command is given is cheap and true, and it catches
/// the mistake these calls actually make -- a border or a shadow drawn at the
/// wrong inset, offset or size.
///
/// It does **not** identify a shape. A rounded rectangle and a rectangle of
/// the same extent record identically, and a test that reads these bounds is
/// entitled to conclude where something was drawn and nothing more. That is a
/// smaller claim than a shape comparison and it is one the recording can
/// actually support; the alternative -- noting that *a path* was drawn and
/// calling that coverage -- is the kind of test that proves less than it looks
/// like it does.
struct StubPath {
    bounds: Option<(f32, f32, f32, f32)>,
}

impl StubPath {
    fn include(&mut self, x: f32, y: f32) {
        self.bounds = Some(match self.bounds {
            None => (x, y, x, y),
            Some((left, top, right, bottom)) => {
                (left.min(x), top.min(y), right.max(x), bottom.max(y))
            }
        });
    }
}

/// # Safety
/// `path` must be null or have come from `rf_path_new` and not been freed.
unsafe fn stub_path<'a>(path: *mut RfPath) -> Option<&'a mut StubPath> {
    unsafe { (path as *mut StubPath).as_mut() }
}

/// Adds a point to a path's bounds, ignoring a null handle.
unsafe fn extend_path(path: *mut RfPath, points: &[(f32, f32)]) {
    if let Some(path) = unsafe { stub_path(path) } {
        for (x, y) in points {
            path.include(*x, *y);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_free(path: *mut RfPath) {
    if !path.is_null() {
        drop(unsafe { Box::from_raw(path as *mut StubPath) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_set_fill_type(path: *mut RfPath, fill_type: c_int) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_move_to(path: *mut RfPath, x: f32, y: f32) {
    unsafe { extend_path(path, &[(x, y)]) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_line_to(path: *mut RfPath, x: f32, y: f32) {
    unsafe { extend_path(path, &[(x, y)]) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_quadratic_to(path: *mut RfPath, cx: f32, cy: f32, x: f32, y: f32) {
    // The control point counts towards the bounds. A true curve stays inside
    // its hull, so this over-reports rather than under-reports -- the right
    // direction for a bound something is being compared against.
    unsafe { extend_path(path, &[(cx, cy), (x, y)]) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_cubic_to(
    path: *mut RfPath,
    cx1: f32,
    cy1: f32,
    cx2: f32,
    cy2: f32,
    x: f32,
    y: f32,
) {
    unsafe { extend_path(path, &[(cx1, cy1), (cx2, cy2), (x, y)]) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_close(path: *mut RfPath) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_add_rect(
    path: *mut RfPath,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) {
    unsafe { extend_path(path, &[(left, top), (right, bottom)]) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_add_oval(
    path: *mut RfPath,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) {
    unsafe { extend_path(path, &[(left, top), (right, bottom)]) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_add_circle(path: *mut RfPath, x: f32, y: f32, radius: f32) {
    unsafe { extend_path(path, &[(x - radius, y - radius), (x + radius, y + radius)]) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_add_rounded_rect(
    path: *mut RfPath,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius_x: f32,
    radius_y: f32,
) {
    unsafe { extend_path(path, &[(left, top), (right, bottom)]) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_line(
    canvas: *mut RfCanvas,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    paint: *const RfPaint,
) {
    record(Drawn::Line {
        from: (x0, y0),
        to: (x1, y1),
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_oval(
    canvas: *mut RfCanvas,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    paint: *const RfPaint,
) {
    record(Drawn::Oval {
        left,
        top,
        right,
        bottom,
        argb: unsafe { paint_argb(paint) },
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_path(
    canvas: *mut RfCanvas,
    path: *const RfPath,
    paint: *const RfPaint,
) {
    let bounds = unsafe { stub_path(path as *mut RfPath) }
        .and_then(|path| path.bounds)
        .unwrap_or((0.0, 0.0, 0.0, 0.0));
    record(Drawn::Path {
        left: bounds.0,
        top: bounds.1,
        right: bounds.2,
        bottom: bounds.3,
        argb: unsafe { paint_argb(paint) },
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_arc(
    canvas: *mut RfCanvas,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    start_degrees: f32,
    sweep_degrees: f32,
    use_center: c_int,
    paint: *const RfPaint,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_image(
    canvas: *mut RfCanvas,
    image: *const RfImage,
    x: f32,
    y: f32,
    paint: *const RfPaint,
) {
    record(Drawn::Image { x, y });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_image_rect(
    canvas: *mut RfCanvas,
    image: *const RfImage,
    src_left: f32,
    src_top: f32,
    src_right: f32,
    src_bottom: f32,
    dst_left: f32,
    dst_top: f32,
    dst_right: f32,
    dst_bottom: f32,
    paint: *const RfPaint,
) {
    record(Drawn::ImageRect {
        source: (src_left, src_top, src_right, src_bottom),
        destination: (dst_left, dst_top, dst_right, dst_bottom),
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_save(canvas: *mut RfCanvas) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_save_layer(
    canvas: *mut RfCanvas,
    bounds_ltrb: *const f32,
    paint: *const RfPaint,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_restore(canvas: *mut RfCanvas) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_save_count(canvas: *mut RfCanvas) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_restore_to_count(canvas: *mut RfCanvas, count: c_int) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_translate(canvas: *mut RfCanvas, dx: f32, dy: f32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_scale(canvas: *mut RfCanvas, sx: f32, sy: f32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_rotate(canvas: *mut RfCanvas, degrees: f32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_skew(canvas: *mut RfCanvas, sx: f32, sy: f32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_transform(
    canvas: *mut RfCanvas,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_clip_rect(
    canvas: *mut RfCanvas,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    clip_op: c_int,
    anti_alias: c_int,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_clip_rounded_rect(
    canvas: *mut RfCanvas,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius_x: f32,
    radius_y: f32,
    clip_op: c_int,
    anti_alias: c_int,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_clip_path(
    canvas: *mut RfCanvas,
    path: *const RfPath,
    clip_op: c_int,
    anti_alias: c_int,
) {
}

/// What the framework asked the compositor for.
///
/// The stubs are otherwise inert on purpose, but *whether a call happened* is
/// not a metric that can be faked into agreeing with itself -- it is the thing
/// under test. A framework that records a clip as a display list operation and
/// one that records it as a layer are indistinguishable from the pixels; they
/// are told apart here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerCalls {
    pub transforms: u32,
    pub offsets: u32,
    pub clip_rects: u32,
    pub clip_rounded_rects: u32,
    pub clip_paths: u32,
    pub opacities: u32,
    pub pops: u32,
    pub display_lists: u32,
    /// Layers opened for keeping, by a repaint boundary that had to record.
    pub retainable: u32,
    /// Layers handed back from an earlier frame instead of being recorded.
    pub retained: u32,
    /// Layers recorded into again in place, by a boundary that kept one and
    /// had something under it change.
    pub rerecorded: u32,
    /// Rectangles drawn onto a canvas, square and rounded together.
    ///
    /// The layer counters above say how a frame was *structured*; this says how
    /// much was drawn into it. Without it a claim like "a zero-width border is
    /// not drawn" has nothing to be checked against, because the colours and
    /// the geometry go straight into a display list nothing here reads back.
    pub rects: u32,
}

// The three below are what a *dependent* crate's tests read -- this module is
// compiled into them with `--cfg rustflutter_stubs`, where nothing in this
// crate calls them. They are the stub's public surface, not dead code.
#[allow(dead_code)]
impl LayerCalls {
    /// Every layer opened, whatever kind.
    pub fn pushes(&self) -> u32 {
        self.transforms
            + self.offsets
            + self.clip_rects
            + self.clip_rounded_rects
            + self.clip_paths
            + self.opacities
    }
}

thread_local! {
    static LAYER_CALLS: std::cell::Cell<LayerCalls> =
        const { std::cell::Cell::new(LayerCalls {
            transforms: 0, offsets: 0, clip_rects: 0, clip_rounded_rects: 0,
            clip_paths: 0, opacities: 0, pops: 0, display_lists: 0,
            retainable: 0, retained: 0, rerecorded: 0, rects: 0,
        }) };
}

fn note(update: impl FnOnce(&mut LayerCalls)) {
    LAYER_CALLS.with(|calls| {
        let mut current = calls.get();
        update(&mut current);
        calls.set(current);
    });
}

//------------------------------------------------------------------------------
// Where each retained layer's pictures went, so a test can tell "the same
// layer object, with a new picture in it" from "a new layer object".
//
// The stubs stay inert about what a picture looks like on purpose; what they
// can answer honestly is which layer object a recording landed in, because
// that is bookkeeping about the calls and not an opinion about the drawing.
// The bookkeeping is a stack of the layers the calls opened, `None` for the
// containers a clip or a transform opens and a handle address for a retained
// one, because the stub has no tree of its own and this is the whole of the
// tree it needs.
thread_local! {
    static OPEN_LAYERS: std::cell::RefCell<Vec<Option<usize>>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Pictures added inside each retained layer, by handle address. Reset by
    /// a re-record into that layer, which drops the old children first.
    static RETAINED_PICTURES: std::cell::RefCell<std::collections::HashMap<usize, u32>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// How many pictures the retained layer with this handle address holds now.
///
/// `id` is what `crate::engine::RetainedLayer::id` reports. A layer that was
/// re-recorded in place starts from zero again, so a count that comes back the
/// same after a re-record is a *new* picture, not the old one surviving.
#[allow(dead_code)]
pub fn retained_picture_count(id: usize) -> u32 {
    RETAINED_PICTURES.with(|pictures| pictures.borrow().get(&id).copied().unwrap_or(0))
}

fn open_container() {
    OPEN_LAYERS.with(|open| open.borrow_mut().push(None));
}

fn open_retained(id: usize) {
    RETAINED_PICTURES.with(|pictures| {
        pictures.borrow_mut().insert(id, 0);
    });
    OPEN_LAYERS.with(|open| open.borrow_mut().push(Some(id)));
}

fn close_top() {
    OPEN_LAYERS.with(|open| {
        open.borrow_mut().pop();
    });
}

/// The retained layer a picture lands in: the innermost open one, past any
/// clips and transforms the recording opened inside it.
fn record_picture() {
    OPEN_LAYERS.with(|open| {
        let inside = open.borrow().iter().rev().find_map(|layer| *layer);
        if let Some(layer) = inside {
            RETAINED_PICTURES.with(|pictures| {
                *pictures.borrow_mut().entry(layer).or_insert(0) += 1;
            });
        }
    });
}

/// The calls made since the last reset, for tests.
#[allow(dead_code)]
pub fn layer_calls() -> LayerCalls {
    LAYER_CALLS.with(|calls| calls.get())
}

/// One drawing the canvas was asked to make, with the numbers it was given.
///
/// # Why this exists
///
/// Nothing in this crate could see what a canvas was told. The stubs took
/// every `rf_canvas_draw_*` call and dropped it, so a hundred and thirteen
/// draw calls across the crate agreed with every possible implementation of
/// themselves -- a `paint` that drew the wrong rectangle, the wrong part of an
/// image, or nothing at all, passed exactly as well as one that was right.
///
/// Found while adding `RenderImage`'s scale, where taking the source rect from
/// the logical size instead of the pixel size drew the top-left quarter of
/// every image stretched over the whole box, and no test could reach it.
///
/// Only the calls whose arguments are numbers are recorded. A path is a handle
/// with nothing readable behind it here, and recording that it happened
/// without recording its shape would invite a test that proves less than it
/// appears to.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub enum Drawn {
    Rect {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        argb: u32,
    },
    RRect {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        radius: f32,
        argb: u32,
    },
    Oval {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        argb: u32,
    },
    Circle {
        cx: f32,
        cy: f32,
        radius: f32,
        argb: u32,
    },
    Line {
        from: (f32, f32),
        to: (f32, f32),
    },
    /// Both rectangles, because the pair is the point: the source is in image
    /// pixels and the destination in logical ones, and a test that sees only
    /// one of them cannot tell a unit mix-up from a correct draw.
    ImageRect {
        source: (f32, f32, f32, f32),
        destination: (f32, f32, f32, f32),
    },
    Image {
        x: f32,
        y: f32,
    },
    /// A translation that went into the layer tree rather than into
    /// coordinates.
    ///
    /// Recorded because it is the other way a render object can move what it
    /// draws, and a test that only watched coordinates would call a lost
    /// offset correct: a child painted at the origin inside an offset layer
    /// and a child painted at the origin with the offset dropped on the floor
    /// look identical from the canvas. Upstream puts the translation in the
    /// layer on purpose -- it lets the compositor move a cached subtree
    /// without re-rasterising it -- so this is a shape to check, not one to
    /// flag.
    OffsetLayer {
        dx: f32,
        dy: f32,
    },
    /// A path, by **where it was drawn and not what shape it is** -- see
    /// `StubPath`. A rounded rectangle and a rectangle of the same extent
    /// record identically.
    Path {
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        argb: u32,
    },
}

impl Drawn {
    /// The same call moved by `(dx, dy)`.
    ///
    /// For asking whether a `paint` respects the offset it is given: paint at
    /// the origin, paint again somewhere else, and the second should be the
    /// first translated. A render object that ignores its offset draws in the
    /// wrong place, which is a whole class of mistake that nothing could see
    /// while the stubs discarded every call.
    ///
    /// A radius is a length and does not move.
    #[allow(dead_code)]
    pub fn translated(self, dx: f32, dy: f32) -> Drawn {
        match self {
            Drawn::Rect {
                left,
                top,
                right,
                bottom,
                argb,
            } => Drawn::Rect {
                left: left + dx,
                top: top + dy,
                right: right + dx,
                bottom: bottom + dy,
                argb,
            },
            Drawn::RRect {
                left,
                top,
                right,
                bottom,
                radius,
                argb,
            } => Drawn::RRect {
                left: left + dx,
                top: top + dy,
                right: right + dx,
                bottom: bottom + dy,
                radius,
                argb,
            },
            Drawn::Oval {
                left,
                top,
                right,
                bottom,
                argb,
            } => Drawn::Oval {
                left: left + dx,
                top: top + dy,
                right: right + dx,
                bottom: bottom + dy,
                argb,
            },
            Drawn::Circle {
                cx,
                cy,
                radius,
                argb,
            } => Drawn::Circle {
                cx: cx + dx,
                cy: cy + dy,
                radius,
                argb,
            },
            Drawn::Line { from, to } => Drawn::Line {
                from: (from.0 + dx, from.1 + dy),
                to: (to.0 + dx, to.1 + dy),
            },
            Drawn::ImageRect {
                source,
                destination,
            } => Drawn::ImageRect {
                // The source is a window on the picture and does not move with
                // the box; only where it lands does.
                source,
                destination: (
                    destination.0 + dx,
                    destination.1 + dy,
                    destination.2 + dx,
                    destination.3 + dy,
                ),
            },
            Drawn::Image { x, y } => Drawn::Image {
                x: x + dx,
                y: y + dy,
            },
            Drawn::Path {
                left,
                top,
                right,
                bottom,
                argb,
            } => Drawn::Path {
                left: left + dx,
                top: top + dy,
                right: right + dx,
                bottom: bottom + dy,
                argb,
            },
            // A layer's own translation is what moves; translating it again
            // would be counting the same movement twice.
            Drawn::OffsetLayer { .. } => self,
        }
    }
}

thread_local! {
    static DRAWN: std::cell::RefCell<Vec<Drawn>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

fn record(call: Drawn) {
    DRAWN.with(|drawn| drawn.borrow_mut().push(call));
}

/// Everything drawn since the last [`reset_drawn`], in order.
#[allow(dead_code)]
pub fn drawn() -> Vec<Drawn> {
    DRAWN.with(|drawn| drawn.borrow().clone())
}

/// Starts recording again. Thread-local, so tests need not coordinate -- but
/// a test that reads [`drawn`] must call this first, because the paint of
/// whatever ran before it is still in the list.
#[allow(dead_code)]
pub fn reset_drawn() {
    DRAWN.with(|drawn| drawn.borrow_mut().clear());
}

fn count_rect() {
    LAYER_CALLS.with(|calls| {
        let mut counts = calls.get();
        counts.rects += 1;
        calls.set(counts);
    });
}

/// Starts counting again. Thread-local, so tests do not need to coordinate.
#[allow(dead_code)]
pub fn reset_layer_calls() {
    LAYER_CALLS.with(|calls| calls.set(LayerCalls::default()));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_push_transform(
    tree: *mut RfLayerTree,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
) {
    note(|calls| calls.transforms += 1);
    open_container();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_push_offset(tree: *mut RfLayerTree, dx: f32, dy: f32) {
    note(|calls| calls.offsets += 1);
    record(Drawn::OffsetLayer { dx, dy });
    open_container();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_push_clip_rect(
    tree: *mut RfLayerTree,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    clip_behavior: c_int,
) {
    note(|calls| calls.clip_rects += 1);
    open_container();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_push_clip_rounded_rect(
    tree: *mut RfLayerTree,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius_x: f32,
    radius_y: f32,
    clip_behavior: c_int,
) {
    note(|calls| calls.clip_rounded_rects += 1);
    open_container();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_push_clip_path(
    tree: *mut RfLayerTree,
    path: *const RfPath,
    clip_behavior: c_int,
) {
    note(|calls| calls.clip_paths += 1);
    open_container();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_push_opacity(
    tree: *mut RfLayerTree,
    alpha: u8,
    offset_x: f32,
    offset_y: f32,
) {
    note(|calls| calls.opacities += 1);
    open_container();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_push_backdrop_blur(
    tree: *mut RfLayerTree,
    sigma_x: f32,
    sigma_y: f32,
) {
    open_container();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_push_blur(
    tree: *mut RfLayerTree,
    sigma_x: f32,
    sigma_y: f32,
) {
    open_container();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_pop(tree: *mut RfLayerTree) {
    note(|calls| calls.pops += 1);
    close_top();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_push_retainable(tree: *mut RfLayerTree) {
    note(|calls| calls.retainable += 1);
    open_retained(allocate::<RfLayer>() as usize);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_pop_retained(tree: *mut RfLayerTree) -> *mut RfLayer {
    note(|calls| calls.pops += 1);
    // The layer a matching push opened, which is the one on top: returning
    // the same address is what keeps a later re-record pointing at the layer
    // that was kept rather than a fresh handle.
    OPEN_LAYERS.with(|open| {
        let mut stack = open.borrow_mut();
        match stack.pop() {
            Some(Some(layer)) => layer as *mut RfLayer,
            _ => std::ptr::null_mut(),
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_add_retained(
    tree: *mut RfLayerTree,
    layer: *mut RfLayer,
    dx: f32,
    dy: f32,
) {
    note(|calls| calls.retained += 1);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_push_retained(tree: *mut RfLayerTree, layer: *mut RfLayer) {
    note(|calls| calls.rerecorded += 1);
    // The children the layer held are dropped first -- the real engine's
    // `RemoveAllChildren` -- so the recording that follows starts from zero
    // and a test can see the picture it lands is a new one.
    open_retained(layer as usize);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_free(layer: *mut RfLayer) {
    if !layer.is_null() {
        RETAINED_PICTURES.with(|pictures| {
            pictures.borrow_mut().remove(&(layer as usize));
        });
    }
    unsafe { release(layer) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_image_decode(data: *const u8, length: usize) -> *mut RfImage {
    // Nothing here decodes, so there are no dimensions to report and this is
    // the one image handle that still measures zero. A test that needs a size
    // builds one from pixels.
    stub_image(0, 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_image_from_pixels(
    pixels: *const u8,
    width: c_int,
    height: c_int,
) -> *mut RfImage {
    if pixels.is_null() || width <= 0 || height <= 0 {
        return std::ptr::null_mut();
    }
    stub_image(width, height)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_image_free(image: *mut RfImage) {
    if !image.is_null() {
        drop(unsafe { Box::from_raw(image as *mut StubImage) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_image_width(image: *const RfImage) -> c_int {
    unsafe { stub_image_ref(image) }.map_or(0, |image| image.width)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_image_height(image: *const RfImage) -> c_int {
    unsafe { stub_image_ref(image) }.map_or(0, |image| image.height)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_new(width: f32, height: f32) -> *mut RfCanvas {
    allocate::<RfCanvas>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_free(canvas: *mut RfCanvas) {
    unsafe { release(canvas) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_color(canvas: *mut RfCanvas, argb: u32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_rect(
    canvas: *mut RfCanvas,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    paint: *const RfPaint,
) {
    count_rect();
    record(Drawn::Rect {
        left,
        top,
        right,
        bottom,
        argb: unsafe { paint_argb(paint) },
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_rrect(
    canvas: *mut RfCanvas,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius: f32,
    paint: *const RfPaint,
) {
    count_rect();
    record(Drawn::RRect {
        left,
        top,
        right,
        bottom,
        radius,
        argb: unsafe { paint_argb(paint) },
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_circle(
    canvas: *mut RfCanvas,
    cx: f32,
    cy: f32,
    radius: f32,
    paint: *const RfPaint,
) {
    record(Drawn::Circle {
        cx,
        cy,
        radius,
        argb: unsafe { paint_argb(paint) },
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_paragraph(
    canvas: *mut RfCanvas,
    paragraph: *mut RfParagraph,
    x: f32,
    y: f32,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_build(canvas: *mut RfCanvas) -> *mut RfDisplayList {
    allocate::<RfDisplayList>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_display_list_free(display_list: *mut RfDisplayList) {
    unsafe { release(display_list) }
}

// -- The paragraph style the framework asked for --------------------------------
//
// Same reasoning as LayerCalls: the stub cannot draw, so "the paragraph was
// right-aligned in an rtl base direction" is asserted as the pair of codes the
// FFI was handed -- which is not the stub's opinion about the drawing, it is
// bookkeeping about the call. dart:ui carries the same pair in ParagraphStyle,
// and the real engine resolves start/end against the direction; see
// txt::ParagraphStyle::effective_align.

thread_local! {
    static PARAGRAPH_STYLES: std::cell::RefCell<Vec<(c_int, c_int)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn note_paragraph_style(text_align: c_int, text_direction: c_int) {
    PARAGRAPH_STYLES.with(|styles| styles.borrow_mut().push((text_align, text_direction)));
}

/// The (align, direction) code pairs the paragraph styles carried, oldest
/// first, since the last reset. For tests.
#[allow(dead_code)]
pub fn paragraph_style_requests() -> Vec<(i32, i32)> {
    PARAGRAPH_STYLES.with(|styles| styles.borrow().clone())
}

/// Starts collecting again. Thread-local, so tests do not need to coordinate.
#[allow(dead_code)]
pub fn reset_paragraph_styles() {
    PARAGRAPH_STYLES.with(|styles| styles.borrow_mut().clear());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_new(
    text: *const c_char,
    text_len: usize,
    font_family: *const c_char,
    font_fallbacks: *const *const c_char,
    font_fallback_count: usize,
    font_size: f32,
    font_weight: c_int,
    italic: bool,
    letter_spacing: f32,
    word_spacing: f32,
    height: f32,
    has_height: bool,
    decoration: c_int,
    feature_tags: *const *const c_char,
    feature_values: *const u32,
    feature_count: usize,
    argb: u32,
    text_align: c_int,
    text_direction: c_int,
    max_lines: usize,
    ellipsis: bool,
) -> *mut RfParagraph {
    note_paragraph_style(text_align, text_direction);
    let text = if text.is_null() {
        String::new()
    } else {
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(text as *const u8, text_len) })
            .into_owned()
    };
    Box::into_raw(Box::new(StubParagraph {
        text,
        font_size: if font_size > 0.0 { font_size } else { 14.0 },
        max_lines,
        lines: Vec::new(),
        constraint: 0.0,
    })) as *mut RfParagraph
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_free(paragraph: *mut RfParagraph) {
    if !paragraph.is_null() {
        drop(unsafe { Box::from_raw(paragraph as *mut StubParagraph) });
    }
}

/// A paragraph, measured as if every glyph were the same width.
///
/// # Why there is a model here at all
///
/// These returned a hard zero until now, so every string in the crate measured
/// nought by nought. Nothing wrapped, no caret had a position, no selection had
/// an extent, and every intrinsic width of text was the same number -- which
/// meant a test could not tell a correct line breaker from one that returned
/// the whole string, and two were written that could not pass and one that
/// passed by having nothing to check.
///
/// # What it is not
///
/// It is not shaping. Every character is one `ADVANCE` wide regardless of what
/// it is, so "iii" and "WWW" measure alike, and a real font would disagree with
/// every number here. Nothing in a test should assert a width against a number
/// it worked out from a font.
///
/// What it does give is the two properties the crate's own arithmetic needs:
/// **a longer string measures wider**, and **a string too wide for its box
/// breaks at a space**. Those are the ones the line breaker, the caret and the
/// selection are written against.
///
/// The break is greedy and by whitespace, with a word longer than the whole
/// box left to overflow rather than split -- which is what upstream's default
/// does with an unbreakable run.
struct StubParagraph {
    text: String,
    font_size: f32,
    max_lines: usize,
    /// Filled in by `layout`, which the crate calls before reading anything.
    lines: Vec<String>,
    constraint: f32,
}

/// One character's width, as a fraction of the font size. Half is close enough
/// to a real proportional face to keep the numbers plausible, and being wrong
/// costs nothing as long as it is wrong consistently.
const ADVANCE: f32 = 0.5;
/// One line's height, likewise.
const LINE_HEIGHT: f32 = 1.2;

impl StubParagraph {
    fn advance(&self) -> f32 {
        self.font_size * ADVANCE
    }

    fn line_height(&self) -> f32 {
        self.font_size * LINE_HEIGHT
    }

    fn measure(&self, text: &str) -> f32 {
        text.chars().count() as f32 * self.advance()
    }

    /// Greedy wrapping at whitespace, then the explicit newlines the text
    /// already had.
    fn wrap(&mut self, max_width: f32) {
        self.constraint = max_width;
        let mut lines = Vec::new();
        for hard in self.text.split('\n') {
            let mut current = String::new();
            for word in hard.split_inclusive(' ') {
                let candidate = current.clone() + word;
                if !current.is_empty() && self.measure(candidate.trim_end()) > max_width {
                    lines.push(current.trim_end().to_string());
                    current = word.to_string();
                } else {
                    current = candidate;
                }
            }
            lines.push(current.trim_end().to_string());
        }
        if self.max_lines > 0 && lines.len() > self.max_lines {
            lines.truncate(self.max_lines);
        }
        self.lines = lines;
    }

    fn longest_line(&self) -> f32 {
        self.lines
            .iter()
            .map(|line| self.measure(line))
            .fold(0.0f32, f32::max)
    }
}

/// # Safety
/// `paragraph` must be null or have come from `rf_paragraph_builder_build`.
unsafe fn stub_paragraph<'a>(paragraph: *mut RfParagraph) -> Option<&'a mut StubParagraph> {
    unsafe { (paragraph as *mut StubParagraph).as_mut() }
}

thread_local! {
    /// The builder in progress. One at a time is enough -- the crate builds a
    /// paragraph and consumes it before starting another.
    static BUILDING: std::cell::RefCell<(String, f32, usize)> =
        const { std::cell::RefCell::new((String::new(), 14.0, 0)) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_builder_new(
    text_align: c_int,
    text_direction: c_int,
    max_lines: usize,
    ellipsis: bool,
) -> *mut RfParagraphBuilder {
    note_paragraph_style(text_align, text_direction);
    BUILDING.with(|building| *building.borrow_mut() = (String::new(), 14.0, max_lines));
    allocate::<RfParagraphBuilder>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_builder_free(builder: *mut RfParagraphBuilder) {
    unsafe { release(builder) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_builder_push_style(
    builder: *mut RfParagraphBuilder,
    font_family: *const c_char,
    font_fallbacks: *const *const c_char,
    font_fallback_count: usize,
    font_size: f32,
    font_weight: c_int,
    italic: bool,
    letter_spacing: f32,
    word_spacing: f32,
    height: f32,
    has_height: bool,
    decoration: c_int,
    feature_tags: *const *const c_char,
    feature_values: *const u32,
    feature_count: usize,
    argb: u32,
) {
    // The last style wins, which is enough for a model that has one advance
    // for every glyph anyway.
    if font_size > 0.0 {
        BUILDING.with(|building| building.borrow_mut().1 = font_size);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_builder_add_text(
    builder: *mut RfParagraphBuilder,
    text: *const c_char,
    text_len: usize,
) {
    if text.is_null() {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(text as *const u8, text_len) };
    let text = String::from_utf8_lossy(bytes).into_owned();
    BUILDING.with(|building| building.borrow_mut().0.push_str(&text));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_builder_pop(builder: *mut RfParagraphBuilder) {}

/// Consumes the builder and hands back a paragraph, as the engine does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_builder_build(
    builder: *mut RfParagraphBuilder,
) -> *mut RfParagraph {
    unsafe { release(builder) };
    let (text, font_size, max_lines) = BUILDING.with(|building| building.borrow().clone());
    Box::into_raw(Box::new(StubParagraph {
        text,
        font_size,
        max_lines,
        lines: Vec::new(),
        constraint: 0.0,
    })) as *mut RfParagraph
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_layout(paragraph: *mut RfParagraph, max_width: f32) {
    if let Some(paragraph) = unsafe { stub_paragraph(paragraph) } {
        paragraph.wrap(max_width);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_width(paragraph: *mut RfParagraph) -> f32 {
    // dart:ui's `Paragraph.width` is the width it was laid out *in*, not the
    // width of the glyphs -- `longestLine` is that. The crate's two-pass
    // layout depends on the difference.
    unsafe { stub_paragraph(paragraph) }.map_or(0.0, |paragraph| paragraph.constraint)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_height(paragraph: *mut RfParagraph) -> f32 {
    unsafe { stub_paragraph(paragraph) }.map_or(0.0, |p| p.lines.len() as f32 * p.line_height())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_longest_line(paragraph: *mut RfParagraph) -> f32 {
    unsafe { stub_paragraph(paragraph) }.map_or(0.0, |p| p.longest_line())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_baseline(paragraph: *mut RfParagraph) -> f32 {
    // Four fifths of the way down the first line, which is about where an
    // alphabetic baseline sits and is a number no test should lean on.
    unsafe { stub_paragraph(paragraph) }.map_or(0.0, |p| p.line_height() * 0.8)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_min_intrinsic_width(paragraph: *mut RfParagraph) -> f32 {
    // The widest single word: the narrowest box the text can be poured into
    // without a word having to be broken.
    unsafe { stub_paragraph(paragraph) }.map_or(0.0, |p| {
        p.text
            .split_whitespace()
            .map(|word| p.measure(word))
            .fold(0.0f32, f32::max)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_max_intrinsic_width(paragraph: *mut RfParagraph) -> f32 {
    // The longest hard line, laid out in all the room it wants -- so no
    // wrapping, and the explicit newlines the text already had are the only
    // breaks.
    unsafe { stub_paragraph(paragraph) }.map_or(0.0, |p| {
        p.text
            .split('\n')
            .map(|line| p.measure(line))
            .fold(0.0f32, f32::max)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_new(width: c_int, height: c_int) -> *mut RfLayerTree {
    allocate::<RfLayerTree>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_free(tree: *mut RfLayerTree) {
    unsafe { release(tree) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_add_display_list(
    tree: *mut RfLayerTree,
    display_list: *mut RfDisplayList,
    offset_x: f32,
    offset_y: f32,
) {
    note(|calls| calls.display_lists += 1);
    record_picture();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_write_png(
    tree: *mut RfLayerTree,
    path: *const c_char,
) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_layer_tree_rasterize_bgra(
    tree: *mut RfLayerTree,
    out_pixels: *mut u8,
    out_len: usize,
) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_window_show(
    width: c_int,
    height: c_int,
    bgra: *const u8,
    bgra_len: usize,
    title: *const c_char,
) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn rf_register_font(
    data: *const u8,
    length: usize,
    family: *const std::os::raw::c_char,
) -> std::os::raw::c_int {
    -1
}
