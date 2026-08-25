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
    pub enum RfParagraphBuilder {}
    pub enum RfLayerTree {}
    pub enum RfLayer {}
    pub enum RfPath {}
    pub enum RfImage {}

    unsafe extern "C" {
        pub fn rf_initialize(icu_data_path: *const c_char) -> c_int;

        pub fn rf_paint_new() -> *mut RfPaint;
        pub fn rf_paint_free(paint: *mut RfPaint);
        pub fn rf_paint_set_color(paint: *mut RfPaint, argb: u32);
        pub fn rf_paint_set_stroke(paint: *mut RfPaint, stroke: c_int, width: f32);
        pub fn rf_paint_set_anti_alias(paint: *mut RfPaint, anti_alias: c_int);
        pub fn rf_paint_set_opacity(paint: *mut RfPaint, opacity: f32);
        pub fn rf_paint_set_blend_mode(paint: *mut RfPaint, blend_mode: c_int);
        pub fn rf_paint_set_color_filter(paint: *mut RfPaint, argb: u32, blend_mode: c_int);
        pub fn rf_paint_clear_color_filter(paint: *mut RfPaint);
        pub fn rf_paint_set_stroke_cap(paint: *mut RfPaint, cap: c_int);
        pub fn rf_paint_set_stroke_join(paint: *mut RfPaint, join: c_int);
        pub fn rf_paint_set_blur(paint: *mut RfPaint, sigma: f32);
        pub fn rf_paint_clear_blur(paint: *mut RfPaint);
        pub fn rf_paint_set_linear_gradient(
            paint: *mut RfPaint,
            x0: f32,
            y0: f32,
            x1: f32,
            y1: f32,
            colors: *const u32,
            stops: *const f32,
            stop_count: c_int,
            tile_mode: c_int,
        );
        pub fn rf_paint_set_radial_gradient(
            paint: *mut RfPaint,
            center_x: f32,
            center_y: f32,
            radius: f32,
            colors: *const u32,
            stops: *const f32,
            stop_count: c_int,
            tile_mode: c_int,
        );
        pub fn rf_paint_set_sweep_gradient(
            paint: *mut RfPaint,
            center_x: f32,
            center_y: f32,
            start_degrees: f32,
            end_degrees: f32,
            colors: *const u32,
            stops: *const f32,
            stop_count: c_int,
            tile_mode: c_int,
        );
        pub fn rf_paint_clear_shader(paint: *mut RfPaint);

        pub fn rf_path_new() -> *mut RfPath;
        pub fn rf_path_free(path: *mut RfPath);
        pub fn rf_path_set_fill_type(path: *mut RfPath, fill_type: c_int);
        pub fn rf_path_move_to(path: *mut RfPath, x: f32, y: f32);
        pub fn rf_path_line_to(path: *mut RfPath, x: f32, y: f32);
        pub fn rf_path_quadratic_to(path: *mut RfPath, cx: f32, cy: f32, x: f32, y: f32);
        pub fn rf_path_cubic_to(
            path: *mut RfPath,
            cx1: f32,
            cy1: f32,
            cx2: f32,
            cy2: f32,
            x: f32,
            y: f32,
        );
        pub fn rf_path_close(path: *mut RfPath);
        pub fn rf_path_add_rect(path: *mut RfPath, left: f32, top: f32, right: f32, bottom: f32);
        pub fn rf_path_add_oval(path: *mut RfPath, left: f32, top: f32, right: f32, bottom: f32);
        pub fn rf_path_add_circle(path: *mut RfPath, x: f32, y: f32, radius: f32);
        pub fn rf_path_add_rounded_rect(
            path: *mut RfPath,
            left: f32,
            top: f32,
            right: f32,
            bottom: f32,
            radius_x: f32,
            radius_y: f32,
        );

        pub fn rf_canvas_draw_line(
            canvas: *mut RfCanvas,
            x0: f32,
            y0: f32,
            x1: f32,
            y1: f32,
            paint: *const RfPaint,
        );
        pub fn rf_canvas_draw_oval(
            canvas: *mut RfCanvas,
            left: f32,
            top: f32,
            right: f32,
            bottom: f32,
            paint: *const RfPaint,
        );
        pub fn rf_canvas_draw_path(
            canvas: *mut RfCanvas,
            path: *const RfPath,
            paint: *const RfPaint,
        );
        pub fn rf_canvas_draw_arc(
            canvas: *mut RfCanvas,
            left: f32,
            top: f32,
            right: f32,
            bottom: f32,
            start_degrees: f32,
            sweep_degrees: f32,
            use_center: c_int,
            paint: *const RfPaint,
        );
        pub fn rf_canvas_draw_image(
            canvas: *mut RfCanvas,
            image: *const RfImage,
            x: f32,
            y: f32,
            paint: *const RfPaint,
        );
        pub fn rf_canvas_draw_image_rect(
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
        );

        pub fn rf_canvas_save(canvas: *mut RfCanvas);
        pub fn rf_canvas_save_layer(
            canvas: *mut RfCanvas,
            bounds_ltrb: *const f32,
            paint: *const RfPaint,
        );
        pub fn rf_canvas_restore(canvas: *mut RfCanvas);
        pub fn rf_canvas_save_count(canvas: *mut RfCanvas) -> c_int;
        pub fn rf_canvas_restore_to_count(canvas: *mut RfCanvas, count: c_int);
        pub fn rf_canvas_translate(canvas: *mut RfCanvas, dx: f32, dy: f32);
        pub fn rf_canvas_scale(canvas: *mut RfCanvas, sx: f32, sy: f32);
        pub fn rf_canvas_rotate(canvas: *mut RfCanvas, degrees: f32);
        pub fn rf_canvas_skew(canvas: *mut RfCanvas, sx: f32, sy: f32);
        pub fn rf_canvas_transform(
            canvas: *mut RfCanvas,
            a: f32,
            b: f32,
            c: f32,
            d: f32,
            e: f32,
            f: f32,
        );
        pub fn rf_canvas_clip_rect(
            canvas: *mut RfCanvas,
            left: f32,
            top: f32,
            right: f32,
            bottom: f32,
            clip_op: c_int,
            anti_alias: c_int,
        );
        pub fn rf_canvas_clip_rounded_rect(
            canvas: *mut RfCanvas,
            left: f32,
            top: f32,
            right: f32,
            bottom: f32,
            radius_x: f32,
            radius_y: f32,
            clip_op: c_int,
            anti_alias: c_int,
        );
        pub fn rf_canvas_clip_path(
            canvas: *mut RfCanvas,
            path: *const RfPath,
            clip_op: c_int,
            anti_alias: c_int,
        );

        pub fn rf_layer_tree_push_transform(
            tree: *mut RfLayerTree,
            a: f32,
            b: f32,
            c: f32,
            d: f32,
            e: f32,
            f: f32,
        );
        pub fn rf_layer_tree_push_offset(tree: *mut RfLayerTree, dx: f32, dy: f32);
        pub fn rf_layer_tree_push_clip_rect(
            tree: *mut RfLayerTree,
            left: f32,
            top: f32,
            right: f32,
            bottom: f32,
            clip_behavior: c_int,
        );
        pub fn rf_layer_tree_push_clip_rounded_rect(
            tree: *mut RfLayerTree,
            left: f32,
            top: f32,
            right: f32,
            bottom: f32,
            radius_x: f32,
            radius_y: f32,
            clip_behavior: c_int,
        );
        pub fn rf_layer_tree_push_clip_path(
            tree: *mut RfLayerTree,
            path: *const RfPath,
            clip_behavior: c_int,
        );
        pub fn rf_layer_tree_push_opacity(
            tree: *mut RfLayerTree,
            alpha: u8,
            offset_x: f32,
            offset_y: f32,
        );
        pub fn rf_layer_tree_push_backdrop_blur(tree: *mut RfLayerTree, sigma_x: f32, sigma_y: f32);
        pub fn rf_layer_tree_push_blur(tree: *mut RfLayerTree, sigma_x: f32, sigma_y: f32);
        pub fn rf_layer_tree_pop(tree: *mut RfLayerTree);
        pub fn rf_layer_tree_push_retainable(tree: *mut RfLayerTree);
        pub fn rf_layer_tree_pop_retained(tree: *mut RfLayerTree) -> *mut RfLayer;
        pub fn rf_layer_tree_add_retained(
            tree: *mut RfLayerTree,
            layer: *mut RfLayer,
            dx: f32,
            dy: f32,
        );
        pub fn rf_layer_tree_push_retained(tree: *mut RfLayerTree, layer: *mut RfLayer);
        pub fn rf_layer_free(layer: *mut RfLayer);

        pub fn rf_image_decode(data: *const u8, length: usize) -> *mut RfImage;
        pub fn rf_image_from_pixels(pixels: *const u8, width: c_int, height: c_int)
        -> *mut RfImage;
        pub fn rf_image_free(image: *mut RfImage);
        pub fn rf_image_width(image: *const RfImage) -> c_int;
        pub fn rf_image_height(image: *const RfImage) -> c_int;

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

        pub fn rf_register_font(data: *const u8, length: usize, family: *const c_char) -> c_int;

        // The run-style parameters are the same list twice over -- once for
        // the single-run paragraph, once per run of a rich one -- because a
        // txt style is assembled at PushStyle time and cannot be amended
        // afterwards. Their meanings mirror `txt::TextStyle`: fallbacks are
        // tried after the family, the spacings are 0 for the font's own, the
        // height only applies with its flag, the decoration is a bitmask, and
        // the features are (tag, value) pairs.
        pub fn rf_paragraph_new(
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
            // 0 left .. 5 justify, in TextAlign's order; direction 0 ltr,
            // 1 rtl. The pair is the paragraph's, like dart:ui's
            // ParagraphStyle taking both textAlign and textDirection.
            text_align: c_int,
            text_direction: c_int,
            max_lines: usize,
            ellipsis: bool,
        ) -> *mut RfParagraph;
        pub fn rf_paragraph_free(paragraph: *mut RfParagraph);
        pub fn rf_paragraph_builder_new(
            text_align: c_int,
            text_direction: c_int,
            max_lines: usize,
            ellipsis: bool,
        ) -> *mut RfParagraphBuilder;
        // Declared for completeness: `build` consumes the builder, so nothing
        // here frees one, and a builder is never dropped half-built.
        #[allow(dead_code)]
        pub fn rf_paragraph_builder_free(builder: *mut RfParagraphBuilder);
        pub fn rf_paragraph_builder_push_style(
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
        );
        pub fn rf_paragraph_builder_add_text(
            builder: *mut RfParagraphBuilder,
            text: *const c_char,
            text_len: usize,
        );
        pub fn rf_paragraph_builder_pop(builder: *mut RfParagraphBuilder);
        pub fn rf_paragraph_builder_build(builder: *mut RfParagraphBuilder) -> *mut RfParagraph;
        pub fn rf_paragraph_layout(paragraph: *mut RfParagraph, max_width: f32);
        pub fn rf_paragraph_width(paragraph: *mut RfParagraph) -> f32;
        pub fn rf_paragraph_height(paragraph: *mut RfParagraph) -> f32;
        pub fn rf_paragraph_longest_line(paragraph: *mut RfParagraph) -> f32;
        pub fn rf_paragraph_baseline(paragraph: *mut RfParagraph) -> f32;
        pub fn rf_paragraph_min_intrinsic_width(paragraph: *mut RfParagraph) -> f32;
        pub fn rf_paragraph_max_intrinsic_width(paragraph: *mut RfParagraph) -> f32;

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

/// Makes a font in memory available to every paragraph, under `family`.
///
/// This is what an icon font needs. An icon is a glyph at a private-use
/// codepoint; without a family to find it in, the shaper falls back to a system
/// face that has nothing there and draws a blank rather than an error.
///
/// The data is copied, so the caller's buffer can go away afterwards. Returns
/// false if Skia cannot read it as a font.
pub fn register_font(data: &[u8], family: &str) -> bool {
    let Ok(family) = std::ffi::CString::new(family) else {
        return false;
    };
    if data.is_empty() {
        return false;
    }
    // SAFETY: the pointer and length describe `data`, which outlives the call,
    // and `family` is NUL-terminated for as long as the call runs.
    unsafe { sys::rf_register_font(data.as_ptr(), data.len(), family.as_ptr()) == 0 }
}

/// 0xAARRGGBB, the same encoding as `dart:ui`'s `Color`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    /// Upstream `Color.computeLuminance`: WCAG 2.0 relative luminance.
    ///
    /// Each channel is un-gamma'd -- divided by 12.92 below 0.03928 and put
    /// through `((c + 0.055) / 1.055) ^ 2.4` above it -- and then weighted
    /// 0.2126 / 0.7152 / 0.0722. The weights are not a colour-space
    /// convenience: they are how much the eye gets from each channel, which is
    /// why green carries seven tenths of it and blue under a fourteenth.
    ///
    /// The alpha channel takes no part. Luminance is a property of the colour,
    /// not of what compositing it would produce.
    pub fn compute_luminance(self) -> f32 {
        fn linearize(component: f32) -> f32 {
            if component <= 0.03928 {
                component / 12.92
            } else {
                ((component + 0.055) / 1.055).powf(2.4)
            }
        }
        let red = linearize(self.red() as f32 / 255.0);
        let green = linearize(self.green() as f32 / 255.0);
        let blue = linearize(self.blue() as f32 / 255.0);
        0.2126 * red + 0.7152 * green + 0.0722 * blue
    }

    pub const TRANSPARENT: Color = Color(0x0000_0000);
    pub const BLACK: Color = Color(0xFF00_0000);
    pub const WHITE: Color = Color(0xFFFF_FFFF);

    pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Color {
        Color(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::argb(0xFF, r, g, b)
    }

    pub const fn alpha(self) -> u8 {
        (self.0 >> 24) as u8
    }

    pub const fn red(self) -> u8 {
        (self.0 >> 16) as u8
    }

    pub const fn green(self) -> u8 {
        (self.0 >> 8) as u8
    }

    pub const fn blue(self) -> u8 {
        self.0 as u8
    }

    /// The same colour at a different alpha.
    pub const fn with_alpha(self, alpha: u8) -> Color {
        Color::argb(alpha, self.red(), self.green(), self.blue())
    }

    /// The same colour, `amount` of the way towards black. `amount` is 0 to 1.
    ///
    /// Alpha is left alone: darkening is about the colour, and a press state
    /// that also went transparent would show whatever is behind it.
    pub fn darkened(self, amount: f32) -> Color {
        let keep = 1.0 - amount.clamp(0.0, 1.0);
        let scale = |c: u8| (c as f32 * keep).round().clamp(0.0, 255.0) as u8;
        Color::argb(
            self.alpha(),
            scale(self.red()),
            scale(self.green()),
            scale(self.blue()),
        )
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
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn xywh(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        }
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    /// dart:ui's `Rect.isEmpty`: `left >= right || top >= bottom`.
    ///
    /// **Both axes**, which is the whole reason to have it rather than
    /// checking a width. A rectangle as wide as the screen and no pixels tall
    /// covers nothing, and asking a canvas to fill it is a draw call that
    /// paints nothing -- upstream skips those, and code here that tested only
    /// the width did not.
    pub fn is_empty(&self) -> bool {
        self.left >= self.right || self.top >= self.bottom
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    /// Upstream `Rect.fromCenter`.
    pub fn from_center(center_x: f32, center_y: f32, width: f32, height: f32) -> Rect {
        Rect {
            left: center_x - width / 2.0,
            top: center_y - height / 2.0,
            right: center_x + width / 2.0,
            bottom: center_y + height / 2.0,
        }
    }

    /// Upstream `Rect.fromCircle`.
    pub fn from_circle(center_x: f32, center_y: f32, radius: f32) -> Rect {
        Rect::from_center(center_x, center_y, radius * 2.0, radius * 2.0)
    }

    /// Upstream `Rect.center`, as the pair rather than an `Offset` -- `Offset`
    /// lives a module up from here.
    pub fn center(&self) -> (f32, f32) {
        (
            self.left + self.width() / 2.0,
            self.top + self.height() / 2.0,
        )
    }

    /// Upstream `Rect.shortestSide`.
    pub fn shortest_side(&self) -> f32 {
        self.width().abs().min(self.height().abs())
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
    pub(crate) raw: *mut sys::RfPaint,
}

impl Paint {
    pub fn new(color: Color) -> Paint {
        let raw = unsafe { sys::rf_paint_new() };
        assert!(!raw.is_null(), "engine failed to allocate a paint");
        unsafe { sys::rf_paint_set_color(raw, color.0) };
        Paint { raw }
    }

    /// Reads the colour back through the stub engine's thread-local record;
    /// only meaningful under `cfg(test)`.
    #[cfg(test)]
    pub(crate) fn color_for_test(&self) -> Color {
        Color(crate::engine_test_stubs::last_paint_color())
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
///
/// Upstream's six, from `dart:ui`: `start` and `end` are resolved against the
/// paragraph's direction at shaping time -- for left-to-right text `start` is
/// the left edge, for right-to-left text the right -- which is why they travel
/// with a [`TextDirection`](crate::direction::TextDirection) rather than
/// meaning one side outright.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Right,
    Center,
    /// The leading edge: left in [`TextDirection::Ltr`], right in
    /// [`TextDirection::Rtl`].
    Start,
    /// The trailing edge: right in [`TextDirection::Ltr`], left in
    /// [`TextDirection::Rtl`].
    End,
    /// Stretch lines that end in a soft line break to the full width.
    Justify,
}

impl TextAlign {
    /// Every value, in the order the codes run.
    pub const ALL: [TextAlign; 6] = [
        TextAlign::Left,
        TextAlign::Right,
        TextAlign::Center,
        TextAlign::Start,
        TextAlign::End,
        TextAlign::Justify,
    ];

    /// The code the FFI expects, in the order the variants are declared.
    ///
    /// **Nothing on this side reads it.** `MakeParagraphStyle` in
    /// `rustflutter_ffi.cc` is the other half -- a `switch` whose default arm
    /// is `left`, which is why `Left` is the number this side chooses rather
    /// than one that side names. A row that took its neighbour's number would
    /// centre a paragraph that asked to be right-aligned.
    pub(crate) fn code(self) -> c_int {
        match self {
            TextAlign::Left => 0,
            TextAlign::Right => 1,
            TextAlign::Center => 2,
            TextAlign::Start => 3,
            TextAlign::End => 4,
            TextAlign::Justify => 5,
        }
    }
}

/// A line drawn with the text: under it, over it, or through it.
///
/// A bitmask rather than an enum because several can be asked for at once --
/// `UNDERLINE | LINE_THROUGH` is a book title. The values are `txt::TextDecoration`'s,
/// which are `dart:ui`'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TextDecoration(pub u8);

impl TextDecoration {
    /// No line. The default, and the engine's.
    pub const NONE: TextDecoration = TextDecoration(0);
    /// A line under the baseline the text sits on.
    pub const UNDERLINE: TextDecoration = TextDecoration(1);
    /// A line over the tallest glyph.
    pub const OVERLINE: TextDecoration = TextDecoration(2);
    /// A line through the middle of every glyph.
    pub const LINE_THROUGH: TextDecoration = TextDecoration(4);
}

impl std::ops::BitOr for TextDecoration {
    type Output = TextDecoration;

    fn bitor(self, rhs: TextDecoration) -> TextDecoration {
        TextDecoration(self.0 | rhs.0)
    }
}

/// Text appearance. Mirrors the subset of `txt::TextStyle` the FFI exposes.
///
/// The optional fields are `None` for the engine's own value rather than some
/// number standing in as one: `height: Some(1.0)` is a different paragraph
/// from `height: None`, because a font's natural line height is not its font
/// size.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub font_family: Option<String>,
    /// Families tried, in order, after `font_family` has no glyph for a
    /// codepoint. Upstream's `fontFamilyFallback`.
    pub font_family_fallback: Option<Vec<String>>,
    pub font_size: f32,
    /// CSS-style weight, 100..=900. 400 is normal, 700 is bold.
    pub font_weight: i32,
    /// False is upright, the engine's normal. Upstream's `FontStyle.italic`.
    pub italic: bool,
    /// Extra space after each glyph, in logical pixels. `None` is the font's
    /// own spacing.
    pub letter_spacing: Option<f32>,
    /// Extra space at each word break, in logical pixels. `None` is the
    /// font's own.
    pub word_spacing: Option<f32>,
    /// The line height as a multiple of `font_size`. `None` is the font's own
    /// metrics; `Some(1.0)` is the EM square, which is not the same thing.
    pub height: Option<f32>,
    pub decoration: TextDecoration,
    /// OpenType features as (tag, value) pairs -- `("tnum", 1)` for tabular
    /// figures, `("smcp", 1)` for small caps.
    pub font_features: Option<Vec<(String, u32)>>,
    pub color: Color,
    /// How the paragraph's lines are justified. The default is
    /// [`TextAlign::Start`], which is upstream's default too: `TextPainter`'s
    /// `TextAlign textAlign = TextAlign.start` (`painting/text_painter.dart`),
    /// where a null `Text.textAlign` lands. `Start` travels unresolved to the
    /// shaper, which reads it against the paragraph's text direction; `Left`
    /// is the fixed left edge and never the default.
    pub align: TextAlign,
}

impl TextStyle {
    /// Upstream's `TextStyle.compareTo`: how badly this style differs from
    /// another, in [`crate::painting::RenderComparison`]'s terms.
    ///
    /// # Two buckets, and the boundary is not where it looks
    ///
    /// Upstream tests the layout-affecting fields first and returns `layout`
    /// if any differs; then the paint-affecting ones for `paint`; otherwise
    /// `identical`. What is worth knowing is which side things fall on.
    ///
    /// **`color` is paint. `foreground` is layout.** They are the same ink
    /// expressed two ways, and they land in different buckets, because a
    /// `Paint` may carry a stroke width and stroking a glyph widens it --
    /// upstream cannot see inside a `Paint`, so it assumes the worst. A plain
    /// colour cannot move anything and is known not to.
    ///
    /// `shadows` is on the layout side too, for the same conservatism: a
    /// shadow does not change metrics, but upstream groups it with the fields
    /// it will not reason about.
    ///
    /// This port's `TextStyle` carries neither `foreground` nor `shadows`, so
    /// the rule is recorded rather than exercised; what it does carry splits
    /// as upstream splits it.
    ///
    /// `align` is this port's own addition to the struct -- upstream keeps
    /// text alignment on the painter, not the style -- so it appears in
    /// neither of upstream's lists. It is treated as layout here, which is
    /// where upstream would have put it: alignment moves glyphs.
    pub fn compare_to(&self, other: &TextStyle) -> crate::painting::RenderComparison {
        use crate::painting::RenderComparison;
        if self.font_family != other.font_family
            || self.font_family_fallback != other.font_family_fallback
            || self.font_size != other.font_size
            || self.font_weight != other.font_weight
            || self.italic != other.italic
            || self.letter_spacing != other.letter_spacing
            || self.word_spacing != other.word_spacing
            || self.height != other.height
            || self.font_features != other.font_features
            || self.align != other.align
        {
            return RenderComparison::Layout;
        }
        if self.color != other.color || self.decoration != other.decoration {
            return RenderComparison::Paint;
        }
        RenderComparison::Identical
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            font_family: None,
            font_family_fallback: None,
            font_size: 14.0,
            font_weight: 400,
            italic: false,
            letter_spacing: None,
            word_spacing: None,
            height: None,
            decoration: TextDecoration::NONE,
            font_features: None,
            color: Color::BLACK,
            // `TextAlign.start`, per `TextPainter` above -- not `left`.
            align: TextAlign::Start,
        }
    }
}

/// One run's style as the C ABI wants it: strings NUL-terminated, optional
/// numbers resolved to the engine's defaults, and the lists still alive for
/// the pointers passed alongside them to point into.
struct RunStyleArgs {
    family: Option<CString>,
    // These two are never read: they keep the CStrings alive for the pointer
    // vectors below to point into. Dropping them would dangle the pointers.
    #[allow(dead_code)]
    fallbacks: Vec<CString>,
    fallback_ptrs: Vec<*const c_char>,
    #[allow(dead_code)]
    feature_tags: Vec<CString>,
    feature_tag_ptrs: Vec<*const c_char>,
    feature_values: Vec<u32>,
    letter_spacing: f32,
    word_spacing: f32,
    height: f32,
    has_height: bool,
    decoration: c_int,
    italic: bool,
}

impl RunStyleArgs {
    fn new(style: &TextStyle) -> RunStyleArgs {
        let to_cstring = |text: &str| {
            CString::new(text).expect("font families and feature tags must not contain NUL")
        };
        let fallbacks = style
            .font_family_fallback
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|family| to_cstring(family))
            .collect::<Vec<_>>();
        let features = style.font_features.as_deref().unwrap_or(&[]);
        let feature_tags = features
            .iter()
            .map(|(tag, _)| to_cstring(tag))
            .collect::<Vec<_>>();
        RunStyleArgs {
            family: style.font_family.as_deref().map(|f| to_cstring(f)),
            fallback_ptrs: fallbacks.iter().map(|c| c.as_ptr()).collect(),
            feature_tag_ptrs: feature_tags.iter().map(|c| c.as_ptr()).collect(),
            feature_values: features.iter().map(|(_, value)| *value).collect(),
            fallbacks,
            feature_tags,
            letter_spacing: style.letter_spacing.unwrap_or(0.0),
            word_spacing: style.word_spacing.unwrap_or(0.0),
            // The 1.0 stand-in is meaningless without the flag; see the
            // `has_height_override` note on `TextStyle::height`.
            height: style.height.unwrap_or(1.0),
            has_height: style.height.is_some(),
            decoration: style.decoration.0 as c_int,
            italic: style.italic,
        }
    }

    fn family_ptr(&self) -> *const c_char {
        self.family
            .as_ref()
            .map_or(std::ptr::null(), |c| c.as_ptr())
    }
}

/// A laid-out run of text, shaped by the engine's `txt` / skparagraph stack.
pub struct Paragraph {
    raw: *mut sys::RfParagraph,
}

impl Paragraph {
    /// Builds and lays out `text` within `max_width` logical pixels.
    ///
    /// `max_lines`, `ellipsis`, `align` and `direction` are the paragraph's,
    /// not the run's, and are what upstream's `TextPainter` puts in its
    /// `ui.ParagraphStyle`. The direction is the paragraph's base direction --
    /// what bidi resolution and `TextAlign::start`/`end` are measured against
    /// -- taken by the caller from where the paragraph was built; see
    /// [`crate::direction::current_direction`].
    pub fn new(
        text: &str,
        style: &TextStyle,
        max_lines: Option<usize>,
        ellipsis: bool,
        max_width: f32,
        direction: crate::direction::TextDirection,
    ) -> Paragraph {
        let run = RunStyleArgs::new(style);

        let align = style.align.code();
        let direction = (direction == crate::direction::TextDirection::Rtl) as c_int;

        // The engine takes a pointer + length, so interior NULs are fine and
        // the string does not need to be re-encoded.
        let raw = unsafe {
            sys::rf_paragraph_new(
                text.as_ptr() as *const c_char,
                text.len(),
                run.family_ptr(),
                run.fallback_ptrs.as_ptr(),
                run.fallback_ptrs.len(),
                style.font_size,
                style.font_weight,
                run.italic,
                run.letter_spacing,
                run.word_spacing,
                run.height,
                run.has_height,
                run.decoration,
                run.feature_tag_ptrs.as_ptr(),
                run.feature_values.as_ptr(),
                run.feature_tag_ptrs.len(),
                style.color.0,
                align,
                direction,
                max_lines.unwrap_or(0),
                ellipsis,
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

    /// A paragraph with more than one style in it.
    ///
    /// One paragraph, not several: line breaking, bidi reordering and baseline
    /// alignment all work across the whole of it, so a sentence with a bold
    /// word in the middle has to be built this way rather than as three texts
    /// in a row. Upstream this is `ParagraphBuilder` in `dart:ui`, driven by
    /// `TextPainter` from a tree of `TextSpan`s.
    ///
    /// `align`, `direction` and `max_lines` belong to the paragraph; everything
    /// else comes from each run's own style.
    pub fn rich(
        runs: &[(String, TextStyle)],
        align: TextAlign,
        max_lines: Option<usize>,
        ellipsis: bool,
        max_width: f32,
        direction: crate::direction::TextDirection,
    ) -> Paragraph {
        let align_code = align.code();
        let direction_code = (direction == crate::direction::TextDirection::Rtl) as c_int;
        let builder = unsafe {
            sys::rf_paragraph_builder_new(
                align_code,
                direction_code,
                max_lines.unwrap_or(0),
                ellipsis,
            )
        };
        assert!(
            !builder.is_null(),
            "engine failed to make a paragraph builder"
        );

        for (text, style) in runs {
            // Each run's style is marshalled inside the loop, and the args
            // stay alive until the push that reads them returns.
            let run = RunStyleArgs::new(style);
            unsafe {
                sys::rf_paragraph_builder_push_style(
                    builder,
                    run.family_ptr(),
                    run.fallback_ptrs.as_ptr(),
                    run.fallback_ptrs.len(),
                    style.font_size,
                    style.font_weight,
                    run.italic,
                    run.letter_spacing,
                    run.word_spacing,
                    run.height,
                    run.has_height,
                    run.decoration,
                    run.feature_tag_ptrs.as_ptr(),
                    run.feature_values.as_ptr(),
                    run.feature_tag_ptrs.len(),
                    style.color.0,
                );
                sys::rf_paragraph_builder_add_text(
                    builder,
                    text.as_ptr() as *const c_char,
                    text.len(),
                );
                sys::rf_paragraph_builder_pop(builder);
            }
        }

        // Consumes the builder, whatever happens next.
        let raw = unsafe { sys::rf_paragraph_builder_build(builder) };
        assert!(!raw.is_null(), "engine failed to build a paragraph");

        // Two passes, for the reason `new` gives: the second shrinks the
        // paragraph box to the ink so that alignment is measured against the
        // box the caller was handed.
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

    /// Distance from the top of the paragraph to the first line's alphabetic
    /// baseline. What baseline alignment lines up on.
    pub fn baseline(&self) -> f32 {
        unsafe { sys::rf_paragraph_baseline(self.raw) }
    }

    /// The narrowest width that does not split a word.
    pub fn min_intrinsic_width(&self) -> f32 {
        unsafe { sys::rf_paragraph_min_intrinsic_width(self.raw) }
    }

    /// The width the text would take on a single line.
    pub fn max_intrinsic_width(&self) -> f32 {
        unsafe { sys::rf_paragraph_max_intrinsic_width(self.raw) }
    }
}

impl Drop for Paragraph {
    fn drop(&mut self) {
        unsafe { sys::rf_paragraph_free(self.raw) };
    }
}

/// Records drawing commands into an engine `DisplayList`.
pub struct Canvas {
    pub(crate) raw: *mut sys::RfCanvas,
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
            sys::rf_canvas_draw_rect(
                self.raw,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                paint.raw,
            )
        };
    }

    pub fn draw_rounded_rect(&mut self, rect: Rect, radius: f32, paint: &Paint) {
        unsafe {
            sys::rf_canvas_draw_rrect(
                self.raw,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                radius,
                paint.raw,
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

//------------------------------------------------------------------------------
/// A layer that outlived the tree it was built in.
///
/// What a repaint boundary keeps: upstream's `RenderObject.layer`. The next
/// frame hands the engine the same object rather than a copy, which is the
/// whole point -- a subtree that painted the same thing does not have to be
/// recorded again, and one that only moved costs a matrix.
pub struct RetainedLayer {
    raw: *mut sys::RfLayer,
}

impl RetainedLayer {
    /// The identity of the engine-side layer object behind this handle.
    ///
    /// Stable for as long as the handle is alive, and different for every
    /// layer the engine makes -- which makes it the thing to assert on when
    /// the question is whether a boundary kept its layer or was given a new
    /// one. Upstream asks the same question with `identical()`.
    pub fn id(&self) -> usize {
        self.raw as usize
    }
}

impl Drop for RetainedLayer {
    fn drop(&mut self) {
        unsafe { sys::rf_layer_free(self.raw) };
    }
}

/// The tree the engine rasterizes.
///
/// This is the actual handoff point: upstream, `RuntimeDelegate::Render()`
/// takes exactly this object from the framework and everything downstream
/// (rasterizer -> display_list -> Impeller) runs unmodified.
pub struct LayerTree {
    pub(crate) raw: *mut sys::RfLayerTree,
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
        unsafe {
            sys::rf_layer_tree_add_display_list(self.raw, display_list.raw, offset_x, offset_y)
        };
    }

    //--------------------------------------------------------------------------
    /// Opens a layer that [`pop_retained`](LayerTree::pop_retained) can keep.
    ///
    /// Content inside is recorded in the layer's own coordinates, because where
    /// it goes is decided when it is added rather than when it is drawn -- that
    /// is what lets a subtree that only moved be reused rather than recorded
    /// again.
    pub fn push_retainable(&mut self) {
        unsafe { sys::rf_layer_tree_push_retainable(self.raw) };
    }

    /// Closes the layer a matching [`push_retainable`](LayerTree::push_retainable)
    /// opened, and keeps it. The layer stays in this tree as well.
    pub fn pop_retained(&mut self) -> Option<RetainedLayer> {
        let raw = unsafe { sys::rf_layer_tree_pop_retained(self.raw) };
        (!raw.is_null()).then_some(RetainedLayer { raw })
    }

    /// Adds a layer kept from an earlier frame, at `(dx, dy)`.
    pub fn add_retained(&mut self, layer: &RetainedLayer, dx: f32, dy: f32) {
        unsafe { sys::rf_layer_tree_add_retained(self.raw, layer.raw, dx, dy) };
    }

    /// Re-records into `layer`, an earlier frame's kept layer.
    ///
    /// The layer's old children are dropped and it becomes the container the
    /// next recording lands in -- the same object, which is the whole point:
    /// trees that already hold it composite the new content without anything
    /// above recording again. Upstream does this with the layer a repaint
    /// boundary keeps (`_repaintCompositedChild` clears its children and hands
    /// a `PaintingContext` bound to it); the enclosing layer tree here plays
    /// the part of that context's canvas.
    ///
    /// The layer is *not* added to this tree; close the recording with
    /// [`LayerTree::pop`], and composite the layer where it goes with
    /// [`LayerTree::add_retained`].
    pub fn push_retained(&mut self, layer: &RetainedLayer) {
        unsafe { sys::rf_layer_tree_push_retained(self.raw, layer.raw) };
    }

    /// Rasterizes and writes a PNG. Headless, no GPU context required.
    pub fn write_png(&mut self, path: &Path) -> Result<(), RenderError> {
        let path_str = path.to_str().ok_or(RenderError::InvalidPath)?;
        let c_path = CString::new(path_str).map_err(|_| RenderError::InvalidPath)?;
        let rc = unsafe { sys::rf_layer_tree_write_png(self.raw, c_path.as_ptr()) };
        if rc == 0 {
            Ok(())
        } else {
            Err(RenderError::from_code(rc))
        }
    }

    /// Rasterizes into a freshly allocated BGRA8888 buffer.
    pub fn rasterize_bgra(&mut self) -> Result<Vec<u8>, RenderError> {
        let len = (self.width as usize) * (self.height as usize) * 4;
        let mut pixels = vec![0u8; len];
        let rc = unsafe { sys::rf_layer_tree_rasterize_bgra(self.raw, pixels.as_mut_ptr(), len) };
        if rc == 0 {
            Ok(pixels)
        } else {
            Err(RenderError::from_code(rc))
        }
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
        if rc == 0 {
            Ok(())
        } else {
            Err(RenderError::from_code(rc))
        }
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
    /// Every error the engine has a code for, in code order. `Unknown` is not
    /// here: it is what happens to a code this list does not cover.
    pub const ALL: [RenderError; 6] = [
        RenderError::InvalidPath,
        RenderError::RasterizationFailed,
        RenderError::SnapshotFailed,
        RenderError::WriteFailed,
        RenderError::EncodeFailed,
        RenderError::NoPresenter,
    ];

    /// The numbers `rf_layer_tree_write_png` in `rustflutter_ffi.cc` returns.
    /// The other half of a hand-written ABI, like [`TextAlign::code`], and
    /// read in the other direction: this side turns the engine's number back
    /// into something to say.
    pub(crate) fn from_code(code: c_int) -> RenderError {
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
            RenderError::RasterizationFailed => {
                write!(f, "engine failed to rasterize the layer tree")
            }
            RenderError::SnapshotFailed => write!(f, "engine failed to snapshot the surface"),
            RenderError::WriteFailed => write!(f, "could not open the output file"),
            RenderError::EncodeFailed => write!(f, "PNG encoding failed"),
            RenderError::NoPresenter => {
                write!(
                    f,
                    "no window presenter on this platform yet; render to a PNG instead"
                )
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

    #[test]
    fn a_default_style_aligns_to_the_start_not_the_left() {
        // `TextPainter`'s default is `TextAlign.start`
        // (`painting/text_painter.dart`), so a style that never set an
        // alignment must resolve against the paragraph's direction, not pin
        // the left edge.
        assert_eq!(TextStyle::default().align, TextAlign::Start);
    }
}

// -- The two tables that cross the FFI in this file ---------------------------

#[cfg(test)]
mod ffi_table_tests {
    //! `variant_sweep` found four arms here nothing was looking at, and both
    //! groups are the shape that has dominated every sweep so far: a table
    //! this side writes or reads and the engine owns the other half of.

    use super::{RenderError, TextAlign};

    #[test]
    fn every_alignment_sends_the_number_make_paragraph_style_reads() {
        // `MakeParagraphStyle` in src/flutter/rust/ffi/rustflutter_ffi.cc:
        // 1 right, 2 center, 3 start, 4 end, 5 justify, anything else left.
        assert_eq!(TextAlign::ALL.map(TextAlign::code), [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn and_no_two_alignments_share_a_number() {
        // Two alignments with one code is a paragraph the engine cannot lay
        // out the way it was asked.
        for (index, one) in TextAlign::ALL.iter().enumerate() {
            for other in TextAlign::ALL.iter().skip(index + 1) {
                assert_ne!(one.code(), other.code(), "{one:?} and {other:?}");
            }
        }
    }

    #[test]
    fn every_render_error_comes_back_from_the_number_the_engine_returns() {
        // `rf_layer_tree_write_png` in the same file: -1 no path, -2 rasterize,
        // -3 snapshot, -4 open or write the file, -5 encode. -100 is the
        // window presenter's, from the host rather than the drawing side.
        let mapped = [
            (-1, RenderError::InvalidPath),
            (-2, RenderError::RasterizationFailed),
            (-3, RenderError::SnapshotFailed),
            (-4, RenderError::WriteFailed),
            (-5, RenderError::EncodeFailed),
            (-100, RenderError::NoPresenter),
        ];
        // Every error with a code of its own is in the list below, so a
        // seventh added to the enum without a code here fails to compile
        // rather than quietly becoming `Unknown`.
        assert_eq!(
            RenderError::ALL.to_vec(),
            mapped.iter().map(|(_, error)| *error).collect::<Vec<_>>()
        );
        for (code, expected) in [
            (-1, RenderError::InvalidPath),
            (-2, RenderError::RasterizationFailed),
            (-3, RenderError::SnapshotFailed),
            (-4, RenderError::WriteFailed),
            (-5, RenderError::EncodeFailed),
            (-100, RenderError::NoPresenter),
        ] {
            assert_eq!(RenderError::from_code(code), expected, "code {code}");
        }
    }

    #[test]
    fn a_code_nobody_has_claimed_is_carried_rather_than_guessed() {
        // The fallback arm, and it keeps the number: an engine that grows a
        // new failure should say so in the message rather than be reported as
        // whichever known error happens to be nearest.
        assert_eq!(RenderError::from_code(-7), RenderError::Unknown(-7));
        assert_eq!(RenderError::from_code(42), RenderError::Unknown(42));
        assert!(RenderError::from_code(-7).to_string().contains("-7"));
    }

    #[test]
    fn no_two_render_errors_say_the_same_thing() {
        // The arms `variant_sweep` reached: "could not open the output file"
        // and "PNG encoding failed" could take each other's text, and a
        // headless render that ran out of disk would have reported an
        // encoding fault. Two messages that read alike are one diagnosis.
        for (index, one) in RenderError::ALL.iter().enumerate() {
            for other in RenderError::ALL.iter().skip(index + 1) {
                assert_ne!(
                    one.to_string(),
                    other.to_string(),
                    "{one:?} and {other:?}"
                );
            }
        }
    }

    #[test]
    fn and_each_says_which_step_failed() {
        // Not a spelling check: each message has to name the step, because it
        // is the only thing a caller gets. The words below are the ones that
        // tell the three failures of `rf_layer_tree_write_png` apart.
        assert!(RenderError::RasterizationFailed.to_string().contains("rasterize"));
        assert!(RenderError::SnapshotFailed.to_string().contains("snapshot"));
        assert!(RenderError::WriteFailed.to_string().contains("open"));
        assert!(RenderError::EncodeFailed.to_string().contains("encoding"));
        assert!(RenderError::InvalidPath.to_string().contains("path"));
        assert!(RenderError::NoPresenter.to_string().contains("presenter"));
    }
}

