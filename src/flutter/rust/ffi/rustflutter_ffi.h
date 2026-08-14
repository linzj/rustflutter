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
typedef struct RfLayerTree RfLayerTree;

// Colors are 0xAARRGGBB, matching Flutter's dart:ui Color encoding.

// -- Process setup ----------------------------------------------------------

// Loads the ICU data the text stack needs for line breaking and word
// segmentation. Pass NULL to look for icudtl.dat next to the executable.
// Idempotent; safe to call from every app entry point. Returns 0 on success.
int32_t rf_initialize(const char* icu_data_path);

// -- Paint ------------------------------------------------------------------

RfPaint* rf_paint_new(void);
void rf_paint_free(RfPaint* paint);
void rf_paint_set_color(RfPaint* paint, uint32_t argb);
// stroke = 0 fills, stroke != 0 strokes with the given width.
void rf_paint_set_stroke(RfPaint* paint, int32_t stroke, float width);
void rf_paint_set_anti_alias(RfPaint* paint, int32_t anti_alias);

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
// Consumes nothing; the canvas stays usable but Build is normally called once.
RfDisplayList* rf_canvas_build(RfCanvas* canvas);

void rf_display_list_free(RfDisplayList* display_list);

// -- Text -------------------------------------------------------------------

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

// -- Layer tree -------------------------------------------------------------

RfLayerTree* rf_layer_tree_new(int32_t width, int32_t height);
void rf_layer_tree_free(RfLayerTree* tree);
// Adds a display list as a child layer at the given offset. Takes a reference;
// the caller still owns its own handle.
void rf_layer_tree_add_display_list(RfLayerTree* tree,
                                    RfDisplayList* display_list,
                                    float offset_x,
                                    float offset_y);

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
