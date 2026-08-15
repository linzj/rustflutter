// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <vector>

#include "flutter/rust/ffi/rustflutter_ffi.h"
#include "flutter/rust/ffi/rustflutter_ffi_internal.h"
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

// -- M4: paths, gradients, transforms, clips, layers, images ------------------

namespace {

// Rasterizes one display list at `size` and returns the pixels. Frees the
// intermediates; the caller only cares about the result.
std::vector<uint8_t> Rasterize(RfDisplayList* display_list,
                               int32_t width,
                               int32_t height) {
  RfLayerTree* tree = rf_layer_tree_new(width, height);
  rf_layer_tree_add_display_list(tree, display_list, 0, 0);
  std::vector<uint8_t> pixels(static_cast<size_t>(width) * height * 4u);
  const int32_t result =
      rf_layer_tree_rasterize_bgra(tree, pixels.data(), pixels.size());
  rf_layer_tree_free(tree);
  if (result != 0) {
    pixels.clear();
  }
  return pixels;
}

}  // namespace

// A path is a real outline, not a bounding box: the far corner of a triangle's
// bounding box must stay unpainted.
TEST(RustFFI, FillsAPath) {
  constexpr int32_t kSize = 64;
  constexpr uint32_t kBackground = 0xFF000000;
  constexpr uint32_t kFill = 0xFF00FF00;

  RfPath* path = rf_path_new();
  ASSERT_NE(path, nullptr);
  rf_path_move_to(path, 0, 0);
  rf_path_line_to(path, 63, 0);
  rf_path_line_to(path, 0, 63);
  rf_path_close(path);

  RfCanvas* canvas = rf_canvas_new(kSize, kSize);
  rf_canvas_draw_color(canvas, kBackground);
  RfPaint* paint = rf_paint_new();
  rf_paint_set_color(paint, kFill);
  rf_paint_set_anti_alias(paint, 0);
  rf_canvas_draw_path(canvas, path, paint);

  RfDisplayList* display_list = rf_canvas_build(canvas);
  std::vector<uint8_t> pixels = Rasterize(display_list, kSize, kSize);
  ASSERT_FALSE(pixels.empty());

  // Inside the triangle.
  EXPECT_EQ(PixelAt(pixels, kSize, 4, 4), kFill);
  EXPECT_EQ(PixelAt(pixels, kSize, 20, 20), kFill);
  // The bottom-right corner is inside the bounding box but outside the shape.
  EXPECT_EQ(PixelAt(pixels, kSize, 60, 60), kBackground);

  rf_display_list_free(display_list);
  rf_paint_free(paint);
  rf_canvas_free(canvas);
  rf_path_free(path);
}

// A gradient must actually vary across the geometry.
TEST(RustFFI, FillsWithALinearGradient) {
  constexpr int32_t kWidth = 64;
  constexpr int32_t kHeight = 16;
  const uint32_t colors[2] = {0xFFFF0000, 0xFF0000FF};

  RfCanvas* canvas = rf_canvas_new(kWidth, kHeight);
  rf_canvas_draw_color(canvas, 0xFF000000);
  RfPaint* paint = rf_paint_new();
  rf_paint_set_anti_alias(paint, 0);
  rf_paint_set_linear_gradient(paint, 0, 0, kWidth, 0, colors, nullptr, 2,
                               /*tile_mode=*/0);
  rf_canvas_draw_rect(canvas, 0, 0, kWidth, kHeight, paint);

  RfDisplayList* display_list = rf_canvas_build(canvas);
  std::vector<uint8_t> pixels = Rasterize(display_list, kWidth, kHeight);
  ASSERT_FALSE(pixels.empty());

  const uint32_t left = PixelAt(pixels, kWidth, 1, 8);
  const uint32_t right = PixelAt(pixels, kWidth, kWidth - 2, 8);
  // Red dominates on the left, blue on the right, and the two ends differ.
  EXPECT_GT((left >> 16) & 0xFF, (left >> 0) & 0xFF);
  EXPECT_GT((right >> 0) & 0xFF, (right >> 16) & 0xFF);
  EXPECT_NE(left, right);

  rf_display_list_free(display_list);
  rf_paint_free(paint);
  rf_canvas_free(canvas);
}

// Transform and clip both have to survive save/restore.
TEST(RustFFI, TransformsAndClips) {
  constexpr int32_t kSize = 64;
  constexpr uint32_t kBackground = 0xFF000000;
  constexpr uint32_t kFill = 0xFFFFFFFF;

  RfCanvas* canvas = rf_canvas_new(kSize, kSize);
  rf_canvas_draw_color(canvas, kBackground);
  RfPaint* paint = rf_paint_new();
  rf_paint_set_color(paint, kFill);
  rf_paint_set_anti_alias(paint, 0);

  const int32_t base = rf_canvas_save_count(canvas);
  rf_canvas_save(canvas);
  // Shift right by 32 and clip to the left half of *that* space, so only the
  // 32..48 band can be painted.
  rf_canvas_translate(canvas, 32, 0);
  rf_canvas_clip_rect(canvas, 0, 0, 16, kSize, /*clip_op=*/0, /*anti_alias=*/0);
  rf_canvas_draw_rect(canvas, 0, 0, kSize, kSize, paint);
  rf_canvas_restore(canvas);
  EXPECT_EQ(rf_canvas_save_count(canvas), base);

  RfDisplayList* display_list = rf_canvas_build(canvas);
  std::vector<uint8_t> pixels = Rasterize(display_list, kSize, kSize);
  ASSERT_FALSE(pixels.empty());

  EXPECT_EQ(PixelAt(pixels, kSize, 16, 32), kBackground);  // left of the shift
  EXPECT_EQ(PixelAt(pixels, kSize, 40, 32), kFill);        // inside the band
  EXPECT_EQ(PixelAt(pixels, kSize, 56, 32), kBackground);  // clipped away

  rf_display_list_free(display_list);
  rf_paint_free(paint);
  rf_canvas_free(canvas);
}

// The compositor's layer stack: a transform layer moves its subtree, and an
// opacity layer blends it. Neither is expressible in a single display list.
TEST(RustFFI, ComposesThroughTheLayerStack) {
  constexpr int32_t kSize = 64;
  constexpr uint32_t kFill = 0xFFFFFFFF;

  RfCanvas* background_canvas = rf_canvas_new(kSize, kSize);
  rf_canvas_draw_color(background_canvas, 0xFF000000);
  RfDisplayList* background = rf_canvas_build(background_canvas);

  RfCanvas* canvas = rf_canvas_new(kSize, kSize);
  RfPaint* paint = rf_paint_new();
  rf_paint_set_color(paint, kFill);
  rf_paint_set_anti_alias(paint, 0);
  rf_canvas_draw_rect(canvas, 0, 0, 16, 16, paint);
  RfDisplayList* square = rf_canvas_build(canvas);

  RfLayerTree* tree = rf_layer_tree_new(kSize, kSize);
  rf_layer_tree_add_display_list(tree, background, 0, 0);
  // Move the square to (32, 32) and draw it at half opacity.
  rf_layer_tree_push_offset(tree, 32, 32);
  rf_layer_tree_push_opacity(tree, 128, 0, 0);
  rf_layer_tree_add_display_list(tree, square, 0, 0);
  rf_layer_tree_pop(tree);
  rf_layer_tree_pop(tree);

  std::vector<uint8_t> pixels(static_cast<size_t>(kSize) * kSize * 4u);
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(tree, pixels.data(), pixels.size()), 0);

  // The square is where the transform put it, not where it was recorded.
  EXPECT_EQ(PixelAt(pixels, kSize, 8, 8), 0xFF000000);
  const uint32_t moved = PixelAt(pixels, kSize, 40, 40);
  const uint32_t red = (moved >> 16) & 0xFF;
  // Half of white over black, with a little room for rounding.
  EXPECT_GT(red, 100u);
  EXPECT_LT(red, 160u);

  rf_layer_tree_free(tree);
  rf_display_list_free(square);
  rf_display_list_free(background);
  rf_paint_free(paint);
  rf_canvas_free(canvas);
  rf_canvas_free(background_canvas);
}

// The root transform every frame is composed under on a display that is not at
// 100%: the framework paints in logical pixels, the tree is measured in
// physical ones, and a transform layer at the root is what reconciles them.
//
// Guards the case that was silently wrong for as long as the device pixel ratio
// was pinned to one -- a picture recorded at logical size and added to a
// physical-sized tree unscaled, which puts the whole interface in the top-left
// corner and leaves the rest blank.
TEST(RustFFI, ScalesAFrameToPhysicalPixels) {
  constexpr int32_t kLogical = 32;
  constexpr float kRatio = 2.0f;
  constexpr int32_t kPhysical = static_cast<int32_t>(kLogical * kRatio);
  constexpr uint32_t kFill = 0xFF2196F3;

  // A square filling the whole logical viewport.
  RfCanvas* canvas = rf_canvas_new(kLogical, kLogical);
  RfPaint* paint = rf_paint_new();
  rf_paint_set_color(paint, kFill);
  rf_paint_set_anti_alias(paint, 0);
  rf_canvas_draw_rect(canvas, 0, 0, kLogical, kLogical, paint);
  RfDisplayList* frame = rf_canvas_build(canvas);

  RfLayerTree* tree = rf_layer_tree_new(kPhysical, kPhysical);
  rf_layer_tree_push_transform(tree, kRatio, 0, 0, kRatio, 0, 0);
  rf_layer_tree_add_display_list(tree, frame, 0, 0);
  rf_layer_tree_pop(tree);

  std::vector<uint8_t> pixels(static_cast<size_t>(kPhysical) * kPhysical * 4u);
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(tree, pixels.data(), pixels.size()), 0);

  // Every corner of the physical surface, including the far one that only the
  // scale can reach.
  EXPECT_EQ(PixelAt(pixels, kPhysical, 1, 1), kFill);
  EXPECT_EQ(PixelAt(pixels, kPhysical, kPhysical - 2, 1), kFill);
  EXPECT_EQ(PixelAt(pixels, kPhysical, 1, kPhysical - 2), kFill);
  EXPECT_EQ(PixelAt(pixels, kPhysical, kPhysical - 2, kPhysical - 2), kFill);

  rf_layer_tree_free(tree);
  rf_display_list_free(frame);
  rf_paint_free(paint);
  rf_canvas_free(canvas);
}

// Round-trips an image through the encoder and the decoder, then draws it.
TEST(RustFFI, DecodesAndDrawsAnImage) {
  constexpr int32_t kSize = 32;
  constexpr uint32_t kFill = 0xFF3366CC;

  // Produce a PNG by rendering one.
  RfCanvas* source_canvas = rf_canvas_new(kSize, kSize);
  rf_canvas_draw_color(source_canvas, kFill);
  RfDisplayList* source = rf_canvas_build(source_canvas);
  RfLayerTree* source_tree = rf_layer_tree_new(kSize, kSize);
  rf_layer_tree_add_display_list(source_tree, source, 0, 0);

  const std::string png_path =
      (std::filesystem::temp_directory_path() / "rf_image_test.png").string();
  ASSERT_EQ(rf_layer_tree_write_png(source_tree, png_path.c_str()), 0);

  std::ifstream file(png_path, std::ios::binary);
  ASSERT_TRUE(file.good());
  std::vector<uint8_t> encoded((std::istreambuf_iterator<char>(file)),
                               std::istreambuf_iterator<char>());
  file.close();
  ASSERT_FALSE(encoded.empty());

  RfImage* image = rf_image_decode(encoded.data(), encoded.size());
  ASSERT_NE(image, nullptr);
  EXPECT_EQ(rf_image_width(image), kSize);
  EXPECT_EQ(rf_image_height(image), kSize);

  // Draw it into the top-left corner of a larger surface.
  constexpr int32_t kCanvasSize = 64;
  RfCanvas* canvas = rf_canvas_new(kCanvasSize, kCanvasSize);
  rf_canvas_draw_color(canvas, 0xFF000000);
  rf_canvas_draw_image(canvas, image, 0, 0, nullptr);
  RfDisplayList* display_list = rf_canvas_build(canvas);
  std::vector<uint8_t> pixels =
      Rasterize(display_list, kCanvasSize, kCanvasSize);
  ASSERT_FALSE(pixels.empty());

  EXPECT_EQ(PixelAt(pixels, kCanvasSize, 16, 16), kFill);
  EXPECT_EQ(PixelAt(pixels, kCanvasSize, 48, 48), 0xFF000000);

  rf_display_list_free(display_list);
  rf_canvas_free(canvas);
  rf_image_free(image);
  rf_layer_tree_free(source_tree);
  rf_display_list_free(source);
  rf_canvas_free(source_canvas);
  std::filesystem::remove(png_path);
}

// A display list is not backend-neutral, and getting it wrong is a segfault
// rather than a wrong pixel: Impeller's dispatcher calls asImpellerImage() and
// dereferences the result without checking, and a Skia-backed DlImage returns
// null there. This pins the invariant -- whichever backend is going to draw,
// the recorded image is one it can read.
TEST(RustFFI, RecordsAnImageTheActiveBackendCanRead) {
  // A one-pixel PNG, so the test does not depend on the encoder.
  constexpr int32_t kSize = 4;
  RfCanvas* source_canvas = rf_canvas_new(kSize, kSize);
  rf_canvas_draw_color(source_canvas, 0xFF112233);
  RfDisplayList* source = rf_canvas_build(source_canvas);
  RfLayerTree* source_tree = rf_layer_tree_new(kSize, kSize);
  rf_layer_tree_add_display_list(source_tree, source, 0, 0);
  const std::string png_path =
      (std::filesystem::temp_directory_path() / "rf_backend_test.png").string();
  ASSERT_EQ(rf_layer_tree_write_png(source_tree, png_path.c_str()), 0);
  std::ifstream file(png_path, std::ios::binary);
  std::vector<uint8_t> encoded((std::istreambuf_iterator<char>(file)),
                               std::istreambuf_iterator<char>());
  file.close();

  RfImage* image = rf_image_decode(encoded.data(), encoded.size());
  ASSERT_NE(image, nullptr);

  rf_set_impeller_backend(1);
  const sk_sp<DlImage>& for_impeller = RfImageDrawable(image);
  ASSERT_NE(for_impeller, nullptr);
  EXPECT_EQ(for_impeller->GetImageType(), DlImage::Type::kImpeller);
  EXPECT_NE(for_impeller->asImpellerImage(), nullptr)
      << "Impeller would dereference this null and crash.";

  // Left as the rest of the process expects to find it.
  rf_set_impeller_backend(0);
  const sk_sp<DlImage>& for_skia = RfImageDrawable(image);
  ASSERT_NE(for_skia, nullptr);
  EXPECT_EQ(for_skia->GetImageType(), DlImage::Type::kSkia);

  rf_image_free(image);
  rf_layer_tree_free(source_tree);
  rf_display_list_free(source);
  rf_canvas_free(source_canvas);
  std::filesystem::remove(png_path);
}

// Rejecting bad input is part of the contract: a decoder that returns a handle
// for garbage would crash later instead of here.
TEST(RustFFI, RejectsUndecodableImageData) {
  const uint8_t garbage[] = {0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07};
  EXPECT_EQ(rf_image_decode(garbage, sizeof(garbage)), nullptr);
  EXPECT_EQ(rf_image_decode(nullptr, 0), nullptr);
}

}  // namespace testing
}  // namespace flutter
