// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Safe Rust bindings for the rustflutter engine boundary.
//!
//! The C ABI these wrap is declared in `rustflutter_ffi.h` and implemented in
//! `rustflutter_ffi.cc` over the engine's own display_list / flow / txt code.
//! Upstream that boundary is the 231 bindings in `lib/ui/dart_ui.cc` plus the
//! 20 `tonic::DartPersistentValue` callbacks on `PlatformConfiguration`.
//!
//! Ownership note: upstream, `Picture` / `Image` / `Path` are
//! `RefCountedDartWrappable` and freed whenever the Dart GC gets around to it.
//! Here every handle is owned by a Rust value with a `Drop` impl, so engine
//! objects are released deterministically at end of scope.

use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::path::Path;

pub(crate) mod sys {
    use std::os::raw::{c_char, c_int};

    pub enum RfPaint {}
    pub enum RfCanvas {}
    pub enum RfDisplayList {}
    pub enum RfParagraph {}
    pub enum RfLayerTree {}

    unsafe extern "C" {
        pub fn rf_initialize(icu_data_path: *const c_char) -> c_int;

        pub fn rf_paint_new() -> *mut RfPaint;
        pub fn rf_paint_free(paint: *mut RfPaint);
        pub fn rf_paint_set_color(paint: *mut RfPaint, argb: u32);
        pub fn rf_paint_set_stroke(paint: *mut RfPaint, stroke: c_int, width: f32);
        pub fn rf_paint_set_anti_alias(paint: *mut RfPaint, anti_alias: c_int);

        pub fn rf_canvas_new(width: f32, height: f32) -> *mut RfCanvas;
        pub fn rf_canvas_free(canvas: *mut RfCanvas);
        pub fn rf_canvas_draw_color(canvas: *mut RfCanvas, argb: u32);
        pub fn rf_canvas_draw_rect(
            canvas: *mut RfCanvas,
            left: f32,
            top: f32,
            right: f32,
            bottom: f32,
            paint: *const RfPaint,
        );
        pub fn rf_canvas_draw_rrect(
            canvas: *mut RfCanvas,
            left: f32,
            top: f32,
            right: f32,
            bottom: f32,
            radius: f32,
            paint: *const RfPaint,
        );
        pub fn rf_canvas_draw_circle(
            canvas: *mut RfCanvas,
            cx: f32,
            cy: f32,
            radius: f32,
            paint: *const RfPaint,
        );
        pub fn rf_canvas_draw_paragraph(
            canvas: *mut RfCanvas,
            paragraph: *mut RfParagraph,
            x: f32,
            y: f32,
        );
        pub fn rf_canvas_build(canvas: *mut RfCanvas) -> *mut RfDisplayList;
        pub fn rf_display_list_free(display_list: *mut RfDisplayList);

        pub fn rf_paragraph_new(
            text: *const c_char,
            text_len: usize,
            font_family: *const c_char,
            font_size: f32,
            font_weight: c_int,
            argb: u32,
            text_align: c_int,
        ) -> *mut RfParagraph;
        pub fn rf_paragraph_free(paragraph: *mut RfParagraph);
        pub fn rf_paragraph_layout(paragraph: *mut RfParagraph, max_width: f32);
        pub fn rf_paragraph_width(paragraph: *mut RfParagraph) -> f32;
        pub fn rf_paragraph_height(paragraph: *mut RfParagraph) -> f32;
        pub fn rf_paragraph_longest_line(paragraph: *mut RfParagraph) -> f32;

        pub fn rf_layer_tree_new(width: c_int, height: c_int) -> *mut RfLayerTree;
        pub fn rf_layer_tree_free(tree: *mut RfLayerTree);
        pub fn rf_layer_tree_add_display_list(
            tree: *mut RfLayerTree,
            display_list: *mut RfDisplayList,
            offset_x: f32,
            offset_y: f32,
        );
        pub fn rf_layer_tree_write_png(tree: *mut RfLayerTree, path: *const c_char) -> c_int;
        pub fn rf_layer_tree_rasterize_bgra(
            tree: *mut RfLayerTree,
            out_pixels: *mut u8,
            out_len: usize,
        ) -> c_int;

        pub fn rf_window_show(
            width: c_int,
            height: c_int,
            bgra: *const u8,
            bgra_len: usize,
            title: *const c_char,
        ) -> c_int;
    }
}

/// Loads the engine's ICU data so the text stack can break lines.
///
/// Idempotent. [`crate::App::new`] calls this, so most apps never need to.
pub fn initialize() {
    // NULL asks the engine to look for icudtl.dat next to the executable.
    unsafe { sys::rf_initialize(std::ptr::null()) };
}

/// 0xAARRGGBB, the same encoding as `dart:ui`'s `Color`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const TRANSPARENT: Color = Color(0x0000_0000);
    pub const BLACK: Color = Color(0xFF00_0000);
    pub const WHITE: Color = Color(0xFFFF_FFFF);

    pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Color {
        Color(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::argb(0xFF, r, g, b)
    }
}

/// A rectangle in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub const fn ltrb(left: f32, top: f32, right: f32, bottom: f32) -> Rect {
        Rect { left, top, right, bottom }
    }

    pub const fn xywh(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect { left: x, top: y, right: x + width, bottom: y + height }
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

/// How a shape is filled or outlined.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Style {
    Fill,
    Stroke { width: f32 },
}

/// Fill/stroke description handed to draw calls.
pub struct Paint {
    raw: *mut sys::RfPaint,
}

impl Paint {
    pub fn new(color: Color) -> Paint {
        let raw = unsafe { sys::rf_paint_new() };
        assert!(!raw.is_null(), "engine failed to allocate a paint");
        unsafe { sys::rf_paint_set_color(raw, color.0) };
        Paint { raw }
    }

    pub fn with_style(self, style: Style) -> Paint {
        match style {
            Style::Fill => unsafe { sys::rf_paint_set_stroke(self.raw, 0, 0.0) },
            Style::Stroke { width } => unsafe { sys::rf_paint_set_stroke(self.raw, 1, width) },
        }
        self
    }

    pub fn with_anti_alias(self, anti_alias: bool) -> Paint {
        unsafe { sys::rf_paint_set_anti_alias(self.raw, anti_alias as c_int) };
        self
    }
}

impl Drop for Paint {
    fn drop(&mut self) {
        unsafe { sys::rf_paint_free(self.raw) };
    }
}

/// Horizontal alignment of a laid-out paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Right,
    Center,
}

/// Text appearance. Mirrors the subset of `txt::TextStyle` the FFI exposes.
#[derive(Clone, Debug)]
pub struct TextStyle {
    pub font_family: Option<String>,
    pub font_size: f32,
    /// CSS-style weight, 100..=900. 400 is normal, 700 is bold.
    pub font_weight: i32,
    pub color: Color,
    pub align: TextAlign,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            font_family: None,
            font_size: 14.0,
            font_weight: 400,
            color: Color::BLACK,
            align: TextAlign::Left,
        }
    }
}

/// A laid-out run of text, shaped by the engine's `txt` / skparagraph stack.
pub struct Paragraph {
    raw: *mut sys::RfParagraph,
}

impl Paragraph {
    /// Builds and lays out `text` within `max_width` logical pixels.
    pub fn new(text: &str, style: &TextStyle, max_width: f32) -> Paragraph {
        let family = style
            .font_family
            .as_deref()
            .map(|f| CString::new(f).expect("font family must not contain NUL"));
        let family_ptr = family.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

        let align = match style.align {
            TextAlign::Left => 0,
            TextAlign::Right => 1,
            TextAlign::Center => 2,
        };

        // The engine takes a pointer + length, so interior NULs are fine and
        // the string does not need to be re-encoded.
        let raw = unsafe {
            sys::rf_paragraph_new(
                text.as_ptr() as *const c_char,
                text.len(),
                family_ptr,
                style.font_size,
                style.font_weight,
                style.color.0,
                align,
            )
        };
        assert!(!raw.is_null(), "engine failed to build a paragraph");

        // Two passes. The first lays the text out in all the space available
        // and tells us how wide the glyphs actually are; the second shrinks the
        // paragraph box to that width. Without the second pass a centred or
        // right-aligned paragraph positions its glyphs relative to `max_width`
        // rather than to its own box, so the caller's paint origin no longer
        // matches what it measured. This mirrors what RenderParagraph does
        // upstream when a Text is given loose constraints.
        unsafe { sys::rf_paragraph_layout(raw, max_width) };
        let ink_width = unsafe { sys::rf_paragraph_longest_line(raw) };
        if ink_width > 0.0 && ink_width < max_width {
            unsafe { sys::rf_paragraph_layout(raw, ink_width.ceil()) };
        }

        Paragraph { raw }
    }

    /// The width the paragraph was laid out into.
    pub fn width(&self) -> f32 {
        unsafe { sys::rf_paragraph_width(self.raw) }
    }

    pub fn height(&self) -> f32 {
        unsafe { sys::rf_paragraph_height(self.raw) }
    }

    /// Width of the widest line -- the paragraph's actual ink extent.
    pub fn longest_line(&self) -> f32 {
        unsafe { sys::rf_paragraph_longest_line(self.raw) }
    }
}

impl Drop for Paragraph {
    fn drop(&mut self) {
        unsafe { sys::rf_paragraph_free(self.raw) };
    }
}

/// Records drawing commands into an engine `DisplayList`.
pub struct Canvas {
    raw: *mut sys::RfCanvas,
}

impl Canvas {
    pub fn new(width: f32, height: f32) -> Canvas {
        let raw = unsafe { sys::rf_canvas_new(width, height) };
        assert!(!raw.is_null(), "engine failed to allocate a canvas");
        Canvas { raw }
    }

    pub fn draw_color(&mut self, color: Color) {
        unsafe { sys::rf_canvas_draw_color(self.raw, color.0) };
    }

    pub fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        unsafe {
            sys::rf_canvas_draw_rect(self.raw, rect.left, rect.top, rect.right, rect.bottom, paint.raw)
        };
    }

    pub fn draw_rounded_rect(&mut self, rect: Rect, radius: f32, paint: &Paint) {
        unsafe {
            sys::rf_canvas_draw_rrect(
                self.raw, rect.left, rect.top, rect.right, rect.bottom, radius, paint.raw,
            )
        };
    }

    pub fn draw_circle(&mut self, cx: f32, cy: f32, radius: f32, paint: &Paint) {
        unsafe { sys::rf_canvas_draw_circle(self.raw, cx, cy, radius, paint.raw) };
    }

    pub fn draw_paragraph(&mut self, paragraph: &Paragraph, x: f32, y: f32) {
        unsafe { sys::rf_canvas_draw_paragraph(self.raw, paragraph.raw, x, y) };
    }

    /// Finishes recording. The canvas is consumed because a `DisplayList` is
    /// immutable once built, matching the engine's own contract.
    pub fn build(self) -> DisplayList {
        let raw = unsafe { sys::rf_canvas_build(self.raw) };
        assert!(!raw.is_null(), "engine failed to build a display list");
        DisplayList { raw }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe { sys::rf_canvas_free(self.raw) };
    }
}

/// An immutable recorded list of drawing commands.
pub struct DisplayList {
    raw: *mut sys::RfDisplayList,
}

impl Drop for DisplayList {
    fn drop(&mut self) {
        unsafe { sys::rf_display_list_free(self.raw) };
    }
}

/// The tree the engine rasterizes.
///
/// This is the actual handoff point: upstream, `RuntimeDelegate::Render()`
/// takes exactly this object from the framework and everything downstream
/// (rasterizer -> display_list -> Impeller) runs unmodified.
pub struct LayerTree {
    raw: *mut sys::RfLayerTree,
    width: i32,
    height: i32,
}

impl LayerTree {
    pub fn new(width: i32, height: i32) -> LayerTree {
        let raw = unsafe { sys::rf_layer_tree_new(width, height) };
        assert!(!raw.is_null(), "engine failed to allocate a layer tree");
        LayerTree { raw, width, height }
    }

    pub fn add_display_list(&mut self, display_list: &DisplayList, offset_x: f32, offset_y: f32) {
        unsafe { sys::rf_layer_tree_add_display_list(self.raw, display_list.raw, offset_x, offset_y) };
    }

    /// Rasterizes and writes a PNG. Headless, no GPU context required.
    pub fn write_png(&mut self, path: &Path) -> Result<(), RenderError> {
        let path_str = path.to_str().ok_or(RenderError::InvalidPath)?;
        let c_path = CString::new(path_str).map_err(|_| RenderError::InvalidPath)?;
        let rc = unsafe { sys::rf_layer_tree_write_png(self.raw, c_path.as_ptr()) };
        if rc == 0 { Ok(()) } else { Err(RenderError::from_code(rc)) }
    }

    /// Rasterizes into a freshly allocated BGRA8888 buffer.
    pub fn rasterize_bgra(&mut self) -> Result<Vec<u8>, RenderError> {
        let len = (self.width as usize) * (self.height as usize) * 4;
        let mut pixels = vec![0u8; len];
        let rc = unsafe { sys::rf_layer_tree_rasterize_bgra(self.raw, pixels.as_mut_ptr(), len) };
        if rc == 0 { Ok(pixels) } else { Err(RenderError::from_code(rc)) }
    }

    /// Opens a window showing this frame and blocks until it is closed.
    ///
    /// Stopgap presenter -- see `rf_window_show` in the C ABI header. The
    /// production path is the engine's own platform embedders driving Impeller.
    pub fn show(&mut self, title: &str) -> Result<(), RenderError> {
        let pixels = self.rasterize_bgra()?;
        let c_title = CString::new(title).map_err(|_| RenderError::InvalidPath)?;
        let rc = unsafe {
            sys::rf_window_show(
                self.width,
                self.height,
                pixels.as_ptr(),
                pixels.len(),
                c_title.as_ptr(),
            )
        };
        if rc == 0 { Ok(()) } else { Err(RenderError::from_code(rc)) }
    }

    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    /// Gives the underlying handle to the caller and forgets it here.
    ///
    /// Used to hand a finished frame to the shell through `RfAppHost::render`,
    /// which takes ownership and converts it into a `flow::LayerTree`. Nothing
    /// else should call this: leaking is the failure mode if the receiver drops
    /// the pointer.
    pub(crate) fn into_raw(self) -> *mut sys::RfLayerTree {
        let raw = self.raw;
        std::mem::forget(self);
        raw
    }
}

impl Drop for LayerTree {
    fn drop(&mut self) {
        unsafe { sys::rf_layer_tree_free(self.raw) };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    InvalidPath,
    RasterizationFailed,
    SnapshotFailed,
    WriteFailed,
    EncodeFailed,
    /// No window presenter is implemented for this platform yet.
    NoPresenter,
    Unknown(i32),
}

impl RenderError {
    fn from_code(code: c_int) -> RenderError {
        match code {
            -1 => RenderError::InvalidPath,
            -2 => RenderError::RasterizationFailed,
            -3 => RenderError::SnapshotFailed,
            -4 => RenderError::WriteFailed,
            -5 => RenderError::EncodeFailed,
            -100 => RenderError::NoPresenter,
            other => RenderError::Unknown(other),
        }
    }
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::InvalidPath => write!(f, "invalid output path"),
            RenderError::RasterizationFailed => write!(f, "engine failed to rasterize the layer tree"),
            RenderError::SnapshotFailed => write!(f, "engine failed to snapshot the surface"),
            RenderError::WriteFailed => write!(f, "could not open the output file"),
            RenderError::EncodeFailed => write!(f, "PNG encoding failed"),
            RenderError::NoPresenter => {
                write!(f, "no window presenter on this platform yet; render to a PNG instead")
            }
            RenderError::Unknown(c) => write!(f, "unknown engine error ({c})"),
        }
    }
}

impl std::error::Error for RenderError {}

// -- Diagnostics --------------------------------------------------------------

/// Version string for the Rust side, as a NUL-terminated C string.
///
/// The returned pointer has static lifetime; the caller must not free it.
#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_version() -> *const c_char {
    "0.1.0-m1\0".as_ptr() as *const c_char
}

/// Round-trip smoke check used by the FFI unit tests.
#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_smoke_increment(value: c_int) -> c_int {
    value.wrapping_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_wraps_instead_of_panicking() {
        assert_eq!(rustflutter_smoke_increment(41), 42);
        assert_eq!(rustflutter_smoke_increment(c_int::MAX), c_int::MIN);
    }

    #[test]
    fn rect_geometry() {
        let r = Rect::xywh(10.0, 20.0, 30.0, 40.0);
        assert_eq!(r.right, 40.0);
        assert_eq!(r.width(), 30.0);
        assert_eq!(r.height(), 40.0);
    }

    #[test]
    fn color_packing() {
        assert_eq!(Color::rgb(0x12, 0x34, 0x56).0, 0xFF12_3456);
    }
}
