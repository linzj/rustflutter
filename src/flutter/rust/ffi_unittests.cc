// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <cstdint>
#include <cstring>
#include <vector>

#include "flutter/rust/ffi/rustflutter_ffi.h"
#include "flutter/testing/testing.h"

// Declared by //flutter/rust:rust_lib (flutter/rust/rustflutter/src/lib.rs).
// Hand written for now; once the FFI surface grows past a handful of symbols
// this should be generated (cbindgen) so the two sides cannot drift.
extern "C" {
const char* rustflutter_version();
int rustflutter_smoke_increment(int value);
}

namespace flutter {
namespace testing {
namespace {

// Reads a BGRA8888 pixel out of a rasterized buffer as 0xAARRGGBB.
uint32_t PixelAt(const std::vector<uint8_t>& pixels,
                 int32_t width,
                 int32_t x,
                 int32_t y) {
  const size_t index = (static_cast<size_t>(y) * width + x) * 4u;
  const uint8_t b = pixels[index + 0];
  const uint8_t g = pixels[index + 1];
  const uint8_t r = pixels[index + 2];
  const uint8_t a = pixels[index + 3];
  return (static_cast<uint32_t>(a) << 24) | (static_cast<uint32_t>(r) << 16) |
         (static_cast<uint32_t>(g) << 8) | b;
}

}  // namespace

TEST(RustFFI, CallsIntoRustAndGetsAValueBack) {
  EXPECT_EQ(rustflutter_smoke_increment(41), 42);
}

TEST(RustFFI, ReturnsAReadableStaticString) {
  const char* version = rustflutter_version();
  ASSERT_NE(version, nullptr);
  EXPECT_STREQ(version, "0.1.0-m1");
}

// The real boundary check: build a display list, wrap it in a layer tree and
// rasterize it through the engine's own flatten + raster path, then read the
// pixels back. If this passes, the Dart-free rendering stack is being driven
// end to end.
TEST(RustFFI, RasterizesALayerTreeThroughTheEngine) {
  constexpr int32_t kWidth = 64;
  constexpr int32_t kHeight = 48;
  constexpr uint32_t kBackground = 0xFF102030;
  constexpr uint32_t kForeground = 0xFFEE2244;

  RfCanvas* canvas = rf_canvas_new(kWidth, kHeight);
  ASSERT_NE(canvas, nullptr);
  rf_canvas_draw_color(canvas, kBackground);

  RfPaint* paint = rf_paint_new();
  ASSERT_NE(paint, nullptr);
  rf_paint_set_color(paint, kForeground);
  // Anti-aliasing would blend the edges; keep it off so the corner pixels of
  // the rect are exactly the requested colour.
  rf_paint_set_anti_alias(paint, 0);
  rf_canvas_draw_rect(canvas, 16, 12, 48, 36, paint);

  RfDisplayList* display_list = rf_canvas_build(canvas);
  ASSERT_NE(display_list, nullptr);

  RfLayerTree* tree = rf_layer_tree_new(kWidth, kHeight);
  ASSERT_NE(tree, nullptr);
  rf_layer_tree_add_display_list(tree, display_list, 0, 0);

  std::vector<uint8_t> pixels(static_cast<size_t>(kWidth) * kHeight * 4u);
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(tree, pixels.data(), pixels.size()), 0);

  // Outside the rect is the background; inside it is the fill.
  EXPECT_EQ(PixelAt(pixels, kWidth, 2, 2), kBackground);
  EXPECT_EQ(PixelAt(pixels, kWidth, kWidth - 3, kHeight - 3), kBackground);
  EXPECT_EQ(PixelAt(pixels, kWidth, 32, 24), kForeground);
  EXPECT_EQ(PixelAt(pixels, kWidth, 16, 12), kForeground);
  EXPECT_EQ(PixelAt(pixels, kWidth, 47, 35), kForeground);
  // Just outside the rect's bottom-right corner.
  EXPECT_EQ(PixelAt(pixels, kWidth, 48, 36), kBackground);

  rf_layer_tree_free(tree);
  rf_display_list_free(display_list);
  rf_paint_free(paint);
  rf_canvas_free(canvas);
}

// Text goes through txt / skparagraph, which needs the engine's ICU data.
TEST(RustFFI, ShapesTextAndReportsItsMetrics) {
  ASSERT_EQ(rf_initialize(nullptr), 0);

  const char kText[] = "Hello, World!";
  RfParagraph* paragraph =
      rf_paragraph_new(kText, std::strlen(kText), nullptr, 24.0f, 400,
                       0xFF000000, /*text_align=*/0);
  ASSERT_NE(paragraph, nullptr);

  rf_paragraph_layout(paragraph, 400.0f);
  EXPECT_GT(rf_paragraph_height(paragraph), 0.0f);
  EXPECT_GT(rf_paragraph_longest_line(paragraph), 0.0f);
  // The text is far narrower than the 400px it was laid out in, so the ink
  // extent must be strictly smaller than the layout width.
  EXPECT_LT(rf_paragraph_longest_line(paragraph), 400.0f);

  rf_paragraph_free(paragraph);
}

// Painting text must actually put ink on the surface, not silently no-op.
TEST(RustFFI, PaintsTextIntoTheSurface) {
  ASSERT_EQ(rf_initialize(nullptr), 0);

  constexpr int32_t kWidth = 200;
  constexpr int32_t kHeight = 60;
  constexpr uint32_t kBackground = 0xFF000000;

  const char kText[] = "Hello";
  RfParagraph* paragraph =
      rf_paragraph_new(kText, std::strlen(kText), nullptr, 32.0f, 700,
                       0xFFFFFFFF, /*text_align=*/0);
  ASSERT_NE(paragraph, nullptr);
  rf_paragraph_layout(paragraph, static_cast<float>(kWidth));

  RfCanvas* canvas = rf_canvas_new(kWidth, kHeight);
  rf_canvas_draw_color(canvas, kBackground);
  rf_canvas_draw_paragraph(canvas, paragraph, 4.0f, 4.0f);
  RfDisplayList* display_list = rf_canvas_build(canvas);

  RfLayerTree* tree = rf_layer_tree_new(kWidth, kHeight);
  rf_layer_tree_add_display_list(tree, display_list, 0, 0);

  std::vector<uint8_t> pixels(static_cast<size_t>(kWidth) * kHeight * 4u);
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(tree, pixels.data(), pixels.size()), 0);

  size_t lit = 0;
  for (int32_t y = 0; y < kHeight; ++y) {
    for (int32_t x = 0; x < kWidth; ++x) {
      if (PixelAt(pixels, kWidth, x, y) != kBackground) {
        ++lit;
      }
    }
  }
  // Five glyphs at 32px cannot come out as a handful of stray pixels.
  EXPECT_GT(lit, 200u);

  rf_layer_tree_free(tree);
  rf_display_list_free(display_list);
  rf_canvas_free(canvas);
  rf_paragraph_free(paragraph);
}

}  // namespace testing
}  // namespace flutter
