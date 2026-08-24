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
    allocate::<RfPaint>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_free(paint: *mut RfPaint) {
    unsafe { release(paint) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paint_set_color(paint: *mut RfPaint, argb: u32) {
    LAST_PAINT_COLOR.with(|c| c.set(argb));
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
    allocate::<RfPath>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_free(path: *mut RfPath) {
    unsafe { release(path) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_set_fill_type(path: *mut RfPath, fill_type: c_int) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_move_to(path: *mut RfPath, x: f32, y: f32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_line_to(path: *mut RfPath, x: f32, y: f32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_quadratic_to(path: *mut RfPath, cx: f32, cy: f32, x: f32, y: f32) {
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
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_add_oval(
    path: *mut RfPath,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_path_add_circle(path: *mut RfPath, x: f32, y: f32, radius: f32) {}

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
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_path(
    canvas: *mut RfCanvas,
    path: *const RfPath,
    paint: *const RfPaint,
) {
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
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_canvas_draw_circle(
    canvas: *mut RfCanvas,
    cx: f32,
    cy: f32,
    radius: f32,
    paint: *const RfPaint,
) {
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
    allocate::<RfParagraph>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_free(paragraph: *mut RfParagraph) {
    unsafe { release(paragraph) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_builder_new(
    text_align: c_int,
    text_direction: c_int,
    max_lines: usize,
    ellipsis: bool,
) -> *mut RfParagraphBuilder {
    note_paragraph_style(text_align, text_direction);
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
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_builder_add_text(
    builder: *mut RfParagraphBuilder,
    text: *const c_char,
    text_len: usize,
) {
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_builder_pop(builder: *mut RfParagraphBuilder) {}

/// Consumes the builder and hands back a paragraph, as the engine does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_builder_build(
    builder: *mut RfParagraphBuilder,
) -> *mut RfParagraph {
    unsafe { release(builder) };
    allocate::<RfParagraph>()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_layout(paragraph: *mut RfParagraph, max_width: f32) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_width(paragraph: *mut RfParagraph) -> f32 {
    0.0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_height(paragraph: *mut RfParagraph) -> f32 {
    0.0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_longest_line(paragraph: *mut RfParagraph) -> f32 {
    0.0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_baseline(paragraph: *mut RfParagraph) -> f32 {
    0.0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_min_intrinsic_width(paragraph: *mut RfParagraph) -> f32 {
    0.0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_paragraph_max_intrinsic_width(paragraph: *mut RfParagraph) -> f32 {
    0.0
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
