// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_FFI_RUSTFLUTTER_FFI_H_
#define FLUTTER_RUST_FFI_RUSTFLUTTER_FFI_H_

// The C ABI that the Rust framework drives the engine through.
//
// Upstream this boundary is the 231 bindings registered in lib/ui/dart_ui.cc
// plus the 20 tonic::DartPersistentValue callbacks on PlatformConfiguration.
// Here it is a plain extern "C" surface over the same C++ objects, so the
// engine's rendering stack (display_list -> flow -> Impeller/Skia) is used
// unmodified.
//
// Ownership: every rf_*_new returns a handle the caller owns and must release
// with the matching rf_*_free. Handles are not thread safe; drive them from a
// single thread the way the engine drives the UI thread.

#include <stddef.h>
#include <stdint.h>

#if defined(__cplusplus)
extern "C" {
#endif

typedef struct RfPaint RfPaint;
typedef struct RfCanvas RfCanvas;
typedef struct RfDisplayList RfDisplayList;
typedef struct RfParagraph RfParagraph;
typedef struct RfParagraphBuilder RfParagraphBuilder;
typedef struct RfLayerTree RfLayerTree;
typedef struct RfLayer RfLayer;
typedef struct RfPath RfPath;
typedef struct RfImage RfImage;

// Colors are 0xAARRGGBB, matching Flutter's dart:ui Color encoding.

// -- Process setup ----------------------------------------------------------

// Loads the ICU data the text stack needs for line breaking and word
// segmentation. Pass NULL to look for icudtl.dat next to the executable.
// Idempotent; safe to call from every app entry point. Returns 0 on success.
int32_t rf_initialize(const char* icu_data_path);

// Tells the recording side which backend will rasterise what it records.
//
// A display list is not backend-neutral in two places, and both of them
// segfault rather than degrade if they get it wrong:
//
//   text    Impeller and Skia consume different ops -- DlTextImpeller carries a
//           shaped TextFrame, DlTextSkia carries an SkTextBlob -- and neither
//           dispatcher can read the other's.
//   images  Impeller's dispatcher calls DlImage::asImpellerImage() and
//           dereferences it without checking. A Skia-backed DlImage returns
//           null there.
//
// The recorder cannot work this out for itself: it never sees the surface. The
// host calls this once, on the raster thread, as soon as it knows whether
// Impeller came up; the headless path leaves it off.
void rf_set_impeller_backend(int32_t enabled);

// -- Paint ------------------------------------------------------------------

RfPaint* rf_paint_new(void);
void rf_paint_free(RfPaint* paint);
void rf_paint_set_color(RfPaint* paint, uint32_t argb);
// stroke = 0 fills, stroke != 0 strokes with the given width.
void rf_paint_set_stroke(RfPaint* paint, int32_t stroke, float width);
void rf_paint_set_anti_alias(RfPaint* paint, int32_t anti_alias);
// 0..1. Multiplies into the paint's alpha.
void rf_paint_set_opacity(RfPaint* paint, float opacity);
// Index into flutter::DlBlendMode, in declaration order (3 = srcOver, the
// default).
void rf_paint_set_blend_mode(RfPaint* paint, int32_t blend_mode);
void rf_paint_set_stroke_cap(RfPaint* paint, int32_t cap);    // 0 butt 1 round 2 square
void rf_paint_set_stroke_join(RfPaint* paint, int32_t join);  // 0 miter 1 round 2 bevel
// Gaussian blur applied to the shape's coverage mask.
void rf_paint_set_blur(RfPaint* paint, float sigma);
void rf_paint_clear_blur(RfPaint* paint);

// Gradients. `colors` is `stop_count` packed 0xAARRGGBB values; `stops` is
// `stop_count` positions in 0..1, or NULL for an even distribution.
// tile_mode: 0 clamp, 1 repeat, 2 mirror, 3 decal.
void rf_paint_set_linear_gradient(RfPaint* paint,
                                  float x0,
                                  float y0,
                                  float x1,
                                  float y1,
                                  const uint32_t* colors,
                                  const float* stops,
                                  int32_t stop_count,
                                  int32_t tile_mode);
void rf_paint_set_radial_gradient(RfPaint* paint,
                                  float center_x,
                                  float center_y,
                                  float radius,
                                  const uint32_t* colors,
                                  const float* stops,
                                  int32_t stop_count,
                                  int32_t tile_mode);
void rf_paint_set_sweep_gradient(RfPaint* paint,
                                 float center_x,
                                 float center_y,
                                 float start_degrees,
                                 float end_degrees,
                                 const uint32_t* colors,
                                 const float* stops,
                                 int32_t stop_count,
                                 int32_t tile_mode);
void rf_paint_clear_shader(RfPaint* paint);

// -- Path -------------------------------------------------------------------
//
// Built imperatively, then handed to rf_canvas_draw_path or a clip. A path may
// be used any number of times; the handle stays valid until freed.

RfPath* rf_path_new(void);
void rf_path_free(RfPath* path);
// 0 non-zero winding (the default), 1 even-odd.
void rf_path_set_fill_type(RfPath* path, int32_t fill_type);
void rf_path_move_to(RfPath* path, float x, float y);
void rf_path_line_to(RfPath* path, float x, float y);
void rf_path_quadratic_to(RfPath* path, float cx, float cy, float x, float y);
void rf_path_cubic_to(RfPath* path,
                      float cx1,
                      float cy1,
                      float cx2,
                      float cy2,
                      float x,
                      float y);
void rf_path_close(RfPath* path);
void rf_path_add_rect(RfPath* path,
                      float left,
                      float top,
                      float right,
                      float bottom);
void rf_path_add_oval(RfPath* path,
                      float left,
                      float top,
                      float right,
                      float bottom);
void rf_path_add_circle(RfPath* path, float x, float y, float radius);
void rf_path_add_rounded_rect(RfPath* path,
                              float left,
                              float top,
                              float right,
                              float bottom,
                              float radius_x,
                              float radius_y);

// -- Canvas (DisplayListBuilder) --------------------------------------------

RfCanvas* rf_canvas_new(float width, float height);
void rf_canvas_free(RfCanvas* canvas);
void rf_canvas_draw_color(RfCanvas* canvas, uint32_t argb);
void rf_canvas_draw_rect(RfCanvas* canvas,
                         float left,
                         float top,
                         float right,
                         float bottom,
                         const RfPaint* paint);
void rf_canvas_draw_rrect(RfCanvas* canvas,
                          float left,
                          float top,
                          float right,
                          float bottom,
                          float radius,
                          const RfPaint* paint);
void rf_canvas_draw_circle(RfCanvas* canvas,
                           float center_x,
                           float center_y,
                           float radius,
                           const RfPaint* paint);
// Paints an already laid-out paragraph with its top-left at (x, y).
void rf_canvas_draw_paragraph(RfCanvas* canvas,
                              RfParagraph* paragraph,
                              float x,
                              float y);
void rf_canvas_draw_line(RfCanvas* canvas,
                         float x0,
                         float y0,
                         float x1,
                         float y1,
                         const RfPaint* paint);
void rf_canvas_draw_oval(RfCanvas* canvas,
                         float left,
                         float top,
                         float right,
                         float bottom,
                         const RfPaint* paint);
void rf_canvas_draw_path(RfCanvas* canvas,
                         const RfPath* path,
                         const RfPaint* paint);
void rf_canvas_draw_arc(RfCanvas* canvas,
                        float left,
                        float top,
                        float right,
                        float bottom,
                        float start_degrees,
                        float sweep_degrees,
                        int32_t use_center,
                        const RfPaint* paint);
// Draws `image` with its top-left at (x, y). `paint` may be NULL.
void rf_canvas_draw_image(RfCanvas* canvas,
                          const RfImage* image,
                          float x,
                          float y,
                          const RfPaint* paint);
// Scales the source rect into the destination rect.
void rf_canvas_draw_image_rect(RfCanvas* canvas,
                               const RfImage* image,
                               float src_left,
                               float src_top,
                               float src_right,
                               float src_bottom,
                               float dst_left,
                               float dst_top,
                               float dst_right,
                               float dst_bottom,
                               const RfPaint* paint);

// -- Canvas state -----------------------------------------------------------
//
// The usual save/restore stack. `save_layer` starts an offscreen group that is
// composited with `paint` on restore -- what group opacity and blend modes
// need. Pass NULL bounds to let the engine work them out.

void rf_canvas_save(RfCanvas* canvas);
void rf_canvas_save_layer(RfCanvas* canvas,
                          const float* bounds_ltrb,  // 4 floats, or NULL
                          const RfPaint* paint);
void rf_canvas_restore(RfCanvas* canvas);
int32_t rf_canvas_save_count(RfCanvas* canvas);
void rf_canvas_restore_to_count(RfCanvas* canvas, int32_t count);

void rf_canvas_translate(RfCanvas* canvas, float dx, float dy);
void rf_canvas_scale(RfCanvas* canvas, float sx, float sy);
void rf_canvas_rotate(RfCanvas* canvas, float degrees);
void rf_canvas_skew(RfCanvas* canvas, float sx, float sy);
// A 2D affine in the order [a c e / b d f]: x2 = a*x + c*y + e.
void rf_canvas_transform(RfCanvas* canvas,
                         float a,
                         float b,
                         float c,
                         float d,
                         float e,
                         float f);

// clip_op: 0 intersect, 1 difference.
void rf_canvas_clip_rect(RfCanvas* canvas,
                         float left,
                         float top,
                         float right,
                         float bottom,
                         int32_t clip_op,
                         int32_t anti_alias);
void rf_canvas_clip_rounded_rect(RfCanvas* canvas,
                                 float left,
                                 float top,
                                 float right,
                                 float bottom,
                                 float radius_x,
                                 float radius_y,
                                 int32_t clip_op,
                                 int32_t anti_alias);
void rf_canvas_clip_path(RfCanvas* canvas,
                         const RfPath* path,
                         int32_t clip_op,
                         int32_t anti_alias);

// Consumes nothing; the canvas stays usable but Build is normally called once.
RfDisplayList* rf_canvas_build(RfCanvas* canvas);

void rf_display_list_free(RfDisplayList* display_list);

// -- Text -------------------------------------------------------------------

// Registers a font from memory under `family`, so that a paragraph asking for
// that family gets it instead of a system font. `data` is copied.
//
// This is what makes an icon font usable: an icon is a glyph at a private-use
// codepoint, and without a family to find it in, the shaper falls back to a
// system face that has nothing at that codepoint and draws a blank.
//
// Returns 0 on success, -1 if the data is not a font Skia can read.
int32_t rf_register_font(const uint8_t* data, size_t length, const char* family);

// text must be UTF-8. font_family may be NULL for the platform default.
RfParagraph* rf_paragraph_new(const char* text,
                              size_t text_len,
                              const char* font_family,
                              float font_size,
                              int32_t font_weight,  // 100..900, 400 = normal
                              uint32_t argb,
                              int32_t text_align);  // 0 left .. 2 center
void rf_paragraph_free(RfParagraph* paragraph);
void rf_paragraph_layout(RfParagraph* paragraph, float max_width);
float rf_paragraph_width(RfParagraph* paragraph);
float rf_paragraph_height(RfParagraph* paragraph);
float rf_paragraph_longest_line(RfParagraph* paragraph);
// Distance from the paragraph's top to the first line's alphabetic baseline.
// What CrossAxisAlignment::Baseline aligns on.
float rf_paragraph_baseline(RfParagraph* paragraph);
// Widths the paragraph would take if it could choose: the narrowest that
// avoids splitting a word, and the width of the whole thing on one line.
// Valid without a prior layout.
float rf_paragraph_min_intrinsic_width(RfParagraph* paragraph);
float rf_paragraph_max_intrinsic_width(RfParagraph* paragraph);

// A paragraph with more than one style in it, assembled run by run. The same
// push/add/pop/build sequence as txt::ParagraphBuilder and as dart:ui's
// ParagraphBuilder, and for the same reason: line breaking, bidi and baselines
// have to see the whole paragraph, so a sentence with a bold word in it is one
// paragraph and not three.
RfParagraphBuilder* rf_paragraph_builder_new(int32_t text_align,
                                             size_t max_lines);  // 0 = no limit
void rf_paragraph_builder_free(RfParagraphBuilder* builder);
void rf_paragraph_builder_push_style(RfParagraphBuilder* builder,
                                     const char* font_family,
                                     float font_size,
                                     int32_t font_weight,
                                     uint32_t argb);
void rf_paragraph_builder_add_text(RfParagraphBuilder* builder,
                                   const char* text,
                                   size_t text_len);
void rf_paragraph_builder_pop(RfParagraphBuilder* builder);
// Consumes the builder and returns the paragraph, or NULL if the builder was.
RfParagraph* rf_paragraph_builder_build(RfParagraphBuilder* builder);

// -- Layer tree -------------------------------------------------------------

RfLayerTree* rf_layer_tree_new(int32_t width, int32_t height);
void rf_layer_tree_free(RfLayerTree* tree);
// Adds a display list as a child layer at the given offset. Takes a reference;
// the caller still owns its own handle.
void rf_layer_tree_add_display_list(RfLayerTree* tree,
                                    RfDisplayList* display_list,
                                    float offset_x,
                                    float offset_y);

// Container layers. Each push makes a new layer the current parent; pop returns
// to the previous one. Unbalanced pushes leave the tree deeper than intended;
// pop on an empty stack is ignored.
//
// Upstream these are what SceneBuilder.push* build. They matter for
// compositing rather than drawing: a transform or opacity the compositor knows
// about can be re-applied without repainting the subtree, and a platform view
// can only be hoisted out of a layer, never out of a display list.
// clip_behavior: 0 none, 1 hard edge, 2 anti-alias, 3 anti-alias with save
// layer.
void rf_layer_tree_push_transform(RfLayerTree* tree,
                                  float a,
                                  float b,
                                  float c,
                                  float d,
                                  float e,
                                  float f);
void rf_layer_tree_push_offset(RfLayerTree* tree, float dx, float dy);
void rf_layer_tree_push_clip_rect(RfLayerTree* tree,
                                  float left,
                                  float top,
                                  float right,
                                  float bottom,
                                  int32_t clip_behavior);
void rf_layer_tree_push_clip_rounded_rect(RfLayerTree* tree,
                                          float left,
                                          float top,
                                          float right,
                                          float bottom,
                                          float radius_x,
                                          float radius_y,
                                          int32_t clip_behavior);
void rf_layer_tree_push_clip_path(RfLayerTree* tree,
                                  const RfPath* path,
                                  int32_t clip_behavior);
void rf_layer_tree_push_opacity(RfLayerTree* tree,
                                uint8_t alpha,
                                float offset_x,
                                float offset_y);
// Blurs everything already painted behind the layer.
void rf_layer_tree_push_backdrop_blur(RfLayerTree* tree,
                                      float sigma_x,
                                      float sigma_y);
// Blurs the layer's own subtree.
void rf_layer_tree_push_blur(RfLayerTree* tree, float sigma_x, float sigma_y);
void rf_layer_tree_pop(RfLayerTree* tree);

// -- Retained layers ---------------------------------------------------------
//
// What a repaint boundary keeps. A subtree that painted the same thing last
// frame does not have to be recorded again: the layer it produced is handed
// back to the next tree as it is, which is what upstream's
// `RenderObject.layer` is and how `PaintingContext` reuses it.
//
// The retained layer holds no position of its own -- it is added under a fresh
// transform each frame -- so a boundary that only moved costs one matrix.

// Opens a layer that `rf_layer_tree_pop_retained` can keep. Otherwise exactly
// `rf_layer_tree_push_offset(tree, 0, 0)` without the transform: content
// inside is recorded in the layer's own coordinates.
void rf_layer_tree_push_retainable(RfLayerTree* tree);

// Closes the layer a matching push opened and returns a handle that keeps it
// alive after this tree is gone. The layer stays where it was added, so the
// frame being built is unaffected. NULL if nothing was open.
RfLayer* rf_layer_tree_pop_retained(RfLayerTree* tree);

// Adds a layer kept from an earlier frame, at `dx, dy` in the current layer's
// coordinates. The layer is shared rather than copied -- which is the point,
// and is what upstream does with `RenderObject.layer` -- so it must not be
// changed while a tree that holds it is still being rasterized.
void rf_layer_tree_add_retained(RfLayerTree* tree,
                                RfLayer* layer,
                                float dx,
                                float dy);

void rf_layer_free(RfLayer* layer);

// -- Images -----------------------------------------------------------------

// Decodes an encoded image (PNG, JPEG, WebP, GIF, BMP) into a CPU-backed
// image. Returns NULL if the bytes could not be decoded.
RfImage* rf_image_decode(const uint8_t* data, size_t length);

// Wraps already-decoded pixels in an image, for callers that decode with
// something other than Skia -- a platform codec, a camera, a generated bitmap.
//
// `pixels` is `width * height * 4` bytes, tightly packed, RGBA8888 with
// premultiplied alpha. That one layout rather than a format enum because it is
// what both backends take without a conversion: it is the SkImageInfo
// rf_image_decode itself decodes into, and it is Impeller's
// kR8G8B8A8UNormInt. Anything else would be swizzled here, on the thread that
// can least afford it.
//
// The bytes are copied, so the caller may free them as soon as this returns.
// Returns NULL if the size is not positive or the allocation failed.
RfImage* rf_image_from_pixels(const uint8_t* pixels,
                              int32_t width,
                              int32_t height);
void rf_image_free(RfImage* image);
int32_t rf_image_width(const RfImage* image);
int32_t rf_image_height(const RfImage* image);

// -- Rasterization ----------------------------------------------------------

// Flattens the layer tree and rasterizes it into a CPU surface, writing a PNG.
// Returns 0 on success.
int32_t rf_layer_tree_write_png(RfLayerTree* tree, const char* path);

// Flattens and rasterizes into caller-provided BGRA8888 storage, row-major,
// `width * height * 4` bytes. Returns 0 on success.
int32_t rf_layer_tree_rasterize_bgra(RfLayerTree* tree,
                                     uint8_t* out_pixels,
                                     size_t out_len);

// -- Presentation -----------------------------------------------------------

// Opens a window showing one BGRA8888 frame and pumps messages until the user
// closes it (or presses Escape). Blocking. Returns 0 on a normal close.
//
// This is a stopgap presenter, not the production path: that is
// shell/platform/* driving Impeller through the Shell, which still needs the
// Engine/RuntimeController rework. Only implemented on Windows so far; other
// platforms return -100.
int32_t rf_window_show(int32_t width,
                       int32_t height,
                       const uint8_t* bgra,
                       size_t bgra_len,
                       const char* title);

// -- Diagnostics ------------------------------------------------------------

const char* rustflutter_version(void);
int32_t rustflutter_smoke_increment(int32_t value);

#if defined(__cplusplus)
}  // extern "C"
#endif

#endif  // FLUTTER_RUST_FFI_RUSTFLUTTER_FFI_H_
