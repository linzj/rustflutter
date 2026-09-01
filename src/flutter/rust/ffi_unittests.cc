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

#include "flutter/runtime/rust_semantics.h"
#include "flutter/rust/ffi/rustflutter_ffi.h"
#include "flutter/rust/ffi/rustflutter_ffi_internal.h"
#include "flutter/rust/host/rustflutter_key_map_android.h"
#include "flutter/rust/host/rustflutter_key_sync_android.h"
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
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(tree, pixels.data(), pixels.size()),
            0);

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
  RfParagraph* paragraph = rf_paragraph_new(
      kText, std::strlen(kText), nullptr,
      /*font_fallbacks=*/nullptr, /*font_fallback_count=*/0, 24.0f, 400,
      /*italic=*/false,
      /*letter_spacing=*/0.0f, /*word_spacing=*/0.0f,
      /*height=*/1.0f, /*has_height=*/false,
      /*decoration=*/0,
      /*feature_tags=*/nullptr, /*feature_values=*/nullptr,
      /*feature_count=*/0, 0xFF000000, /*text_align=*/0, /*text_direction=*/0,
      /*max_lines=*/0, /*ellipsis=*/false);
  ASSERT_NE(paragraph, nullptr);

  rf_paragraph_layout(paragraph, 400.0f);
  EXPECT_GT(rf_paragraph_height(paragraph), 0.0f);
  EXPECT_GT(rf_paragraph_longest_line(paragraph), 0.0f);
  // The text is far narrower than the 400px it was laid out in, so the ink
  // extent must be strictly smaller than the layout width.
  EXPECT_LT(rf_paragraph_longest_line(paragraph), 400.0f);

  rf_paragraph_free(paragraph);
}

namespace {

// Builds and lays out a paragraph of `text`, for the word-boundary tests.
RfParagraph* LayOut(const char* text, size_t length) {
  RfParagraph* paragraph = rf_paragraph_new(
      text, length, nullptr,
      /*font_fallbacks=*/nullptr, /*font_fallback_count=*/0, 24.0f, 400,
      /*italic=*/false,
      /*letter_spacing=*/0.0f, /*word_spacing=*/0.0f,
      /*height=*/1.0f, /*has_height=*/false,
      /*decoration=*/0,
      /*feature_tags=*/nullptr, /*feature_values=*/nullptr,
      /*feature_count=*/0, 0xFF000000, /*text_align=*/0, /*text_direction=*/0,
      /*max_lines=*/0, /*ellipsis=*/false);
  if (paragraph != nullptr) {
    rf_paragraph_layout(paragraph, 2000.0f);
  }
  return paragraph;
}

}  // namespace

// What a long press selects. The framework cannot work this out for itself,
// which is the whole reason the call exists -- see the two cases below.
TEST(RustFFI, FindsTheWordAroundAnOffset) {
  ASSERT_EQ(rf_initialize(nullptr), 0);

  const char kText[] = "Hello, brave world!";
  //                    0123456789...
  RfParagraph* paragraph = LayOut(kText, std::strlen(kText));
  ASSERT_NE(paragraph, nullptr);

  // Inside "brave", which runs [7, 12).
  size_t start = 0;
  size_t end = 0;
  rf_paragraph_word_boundary(paragraph, 9, &start, &end);
  EXPECT_EQ(start, 7u);
  EXPECT_EQ(end, 12u);

  // Inside "Hello", which stops at the comma rather than running into it.
  rf_paragraph_word_boundary(paragraph, 2, &start, &end);
  EXPECT_EQ(start, 0u);
  EXPECT_EQ(end, 5u);

  rf_paragraph_free(paragraph);
}

// The case a Rust implementation could not have got right: Chinese is written
// without spaces, so "a run of letters between spaces" would select the whole
// line. ICU's dictionary is what breaks it into words.
TEST(RustFFI, BreaksChineseIntoWordsWithoutSpaces) {
  ASSERT_EQ(rf_initialize(nullptr), 0);

  // "I love programming" -- 我 喜欢 编程, six characters and no spaces.
  const char kText[] =
      "\xE6\x88\x91\xE5\x96\x9C\xE6\xAC\xA2\xE7\xBC\x96\xE7\xA8\x8B";
  RfParagraph* paragraph = LayOut(kText, std::strlen(kText));
  ASSERT_NE(paragraph, nullptr);

  // Offsets are UTF-16 units; each of these characters is one.
  size_t start = 0;
  size_t end = 0;
  rf_paragraph_word_boundary(paragraph, 1, &start, &end);
  // Whatever ICU decides the word is, it must not be the whole line -- that
  // is the failure a hand-rolled boundary would have had.
  EXPECT_LT(end - start, 5u) << "selected " << (end - start) << " of 5";
  EXPECT_GT(end, start);
  EXPECT_LE(end, 5u);

  rf_paragraph_free(paragraph);
}

// Words are a fact about the string, not about the box it was measured into,
// so an unmeasured paragraph answers the same as a measured one. That is
// upstream's shape -- `Paragraph::getWordBoundary` passes straight through and
// skparagraph breaks the text with ICU on first use -- and it is worth pinning
// because a layout guard here would look harmless and would break a long press
// on a field that has not painted yet.
TEST(RustFFI, KnowsItsWordsBeforeItIsLaidOut) {
  ASSERT_EQ(rf_initialize(nullptr), 0);

  const char kText[] = "Hello, world";
  RfParagraph* paragraph = rf_paragraph_new(
      kText, std::strlen(kText), nullptr,
      /*font_fallbacks=*/nullptr, /*font_fallback_count=*/0, 24.0f, 400,
      /*italic=*/false,
      /*letter_spacing=*/0.0f, /*word_spacing=*/0.0f,
      /*height=*/1.0f, /*has_height=*/false,
      /*decoration=*/0,
      /*feature_tags=*/nullptr, /*feature_values=*/nullptr,
      /*feature_count=*/0, 0xFF000000, /*text_align=*/0, /*text_direction=*/0,
      /*max_lines=*/0, /*ellipsis=*/false);
  ASSERT_NE(paragraph, nullptr);

  size_t start = 0;
  size_t end = 0;
  rf_paragraph_word_boundary(paragraph, 2, &start, &end);
  EXPECT_EQ(start, 0u);
  EXPECT_EQ(end, 5u);

  rf_paragraph_free(paragraph);
}

// Painting text must actually put ink on the surface, not silently no-op.
TEST(RustFFI, PaintsTextIntoTheSurface) {
  ASSERT_EQ(rf_initialize(nullptr), 0);

  constexpr int32_t kWidth = 200;
  constexpr int32_t kHeight = 60;
  constexpr uint32_t kBackground = 0xFF000000;

  const char kText[] = "Hello";
  RfParagraph* paragraph = rf_paragraph_new(
      kText, std::strlen(kText), nullptr,
      /*font_fallbacks=*/nullptr, /*font_fallback_count=*/0, 32.0f, 700,
      /*italic=*/false,
      /*letter_spacing=*/0.0f, /*word_spacing=*/0.0f,
      /*height=*/1.0f, /*has_height=*/false,
      /*decoration=*/0,
      /*feature_tags=*/nullptr, /*feature_values=*/nullptr,
      /*feature_count=*/0, 0xFFFFFFFF, /*text_align=*/0, /*text_direction=*/0,
      /*max_lines=*/0, /*ellipsis=*/false);
  ASSERT_NE(paragraph, nullptr);
  rf_paragraph_layout(paragraph, static_cast<float>(kWidth));

  RfCanvas* canvas = rf_canvas_new(kWidth, kHeight);
  rf_canvas_draw_color(canvas, kBackground);
  rf_canvas_draw_paragraph(canvas, paragraph, 4.0f, 4.0f);
  RfDisplayList* display_list = rf_canvas_build(canvas);

  RfLayerTree* tree = rf_layer_tree_new(kWidth, kHeight);
  rf_layer_tree_add_display_list(tree, display_list, 0, 0);

  std::vector<uint8_t> pixels(static_cast<size_t>(kWidth) * kHeight * 4u);
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(tree, pixels.data(), pixels.size()),
            0);

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

// start resolves against the paragraph's direction, the way
// txt::ParagraphStyle::effective_align does: the same narrow line in the same
// layout width hugs the left edge in ltr and the right edge in rtl.
TEST(RustFFI, ResolvesStartAlignmentAgainstDirection) {
  ASSERT_EQ(rf_initialize(nullptr), 0);

  constexpr int32_t kWidth = 200;
  constexpr int32_t kHeight = 100;
  constexpr uint32_t kBackground = 0xFF000000;

  const char kText[] = "short text";
  const int32_t kDirections[] = {0, 1};  // ltr, rtl
  const float kBandTop[] = {4.0f, 54.0f};

  RfCanvas* canvas = rf_canvas_new(kWidth, kHeight);
  rf_canvas_draw_color(canvas, kBackground);
  RfParagraph* paragraphs[2] = {nullptr, nullptr};
  for (int i = 0; i < 2; ++i) {
    paragraphs[i] = rf_paragraph_new(
        kText, std::strlen(kText), nullptr,
        /*font_fallbacks=*/nullptr, /*font_fallback_count=*/0, 20.0f, 400,
        /*italic=*/false,
        /*letter_spacing=*/0.0f, /*word_spacing=*/0.0f,
        /*height=*/1.0f, /*has_height=*/false,
        /*decoration=*/0,
        /*feature_tags=*/nullptr, /*feature_values=*/nullptr,
        /*feature_count=*/0, 0xFFFFFFFF, /*text_align=*/3 /* start */,
        kDirections[i], /*max_lines=*/0, /*ellipsis=*/false);
    ASSERT_NE(paragraphs[i], nullptr);
    rf_paragraph_layout(paragraphs[i], static_cast<float>(kWidth));
    rf_canvas_draw_paragraph(canvas, paragraphs[i], 0.0f, kBandTop[i]);
  }
  RfDisplayList* display_list = rf_canvas_build(canvas);
  std::vector<uint8_t> pixels = Rasterize(display_list, kWidth, kHeight);

  // The ink extents of each band: which edge the line hugged.
  auto ink_extents = [&](int32_t y0, int32_t y1, int32_t* min_x,
                         int32_t* max_x) {
    *min_x = kWidth;
    *max_x = -1;
    for (int32_t y = y0; y < y1; ++y) {
      for (int32_t x = 0; x < kWidth; ++x) {
        if (PixelAt(pixels, kWidth, x, y) != kBackground) {
          *min_x = std::min(*min_x, x);
          *max_x = std::max(*max_x, x);
        }
      }
    }
  };
  int32_t ltr_min = 0, ltr_max = 0, rtl_min = 0, rtl_max = 0;
  ink_extents(0, 50, &ltr_min, &ltr_max);
  ink_extents(50, kHeight, &rtl_min, &rtl_max);

  // A line of ~80px at 20px font in a 200px width: ltr starts at the left
  // edge and ends well before the middle; rtl is its mirror.
  EXPECT_GE(ltr_min, 0);
  EXPECT_LT(ltr_min, 20);
  EXPECT_LT(ltr_max, 100);
  EXPECT_GT(rtl_max, kWidth - 20);
  EXPECT_GT(rtl_min, 100);

  rf_display_list_free(display_list);
  rf_canvas_free(canvas);
  rf_paragraph_free(paragraphs[0]);
  rf_paragraph_free(paragraphs[1]);
}

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
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(tree, pixels.data(), pixels.size()),
            0);

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

// A layer built in one frame and handed to the next.
//
// What a repaint boundary keeps. The layer holds the drawing and not the
// position, so the second tree puts the same object down somewhere else and the
// pixels land there -- which is the thing worth having, because a scrolling
// list moves every row it keeps and redraws none of them.
TEST(RustFFI, KeepsALayerAcrossTwoTrees) {
  constexpr int32_t kSize = 64;

  RfCanvas* canvas = rf_canvas_new(kSize, kSize);
  RfPaint* paint = rf_paint_new();
  rf_paint_set_color(paint, 0xFFFFFFFF);
  rf_paint_set_anti_alias(paint, 0);
  // Recorded at the origin, which is where a retained layer's content lives.
  rf_canvas_draw_rect(canvas, 0, 0, 8, 8, paint);
  RfDisplayList* square = rf_canvas_build(canvas);

  // First frame: record the square into a layer at (8, 8) and keep it.
  RfLayerTree* first = rf_layer_tree_new(kSize, kSize);
  rf_layer_tree_push_offset(first, 8, 8);
  rf_layer_tree_push_retainable(first);
  rf_layer_tree_add_display_list(first, square, 0, 0);
  RfLayer* kept = rf_layer_tree_pop_retained(first);
  ASSERT_NE(kept, nullptr);
  rf_layer_tree_pop(first);

  std::vector<uint8_t> pixels(static_cast<size_t>(kSize) * kSize * 4u);
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(first, pixels.data(), pixels.size()),
            0);
  EXPECT_EQ(PixelAt(pixels, kSize, 10, 10), 0xFFFFFFFF)
      << "the layer did not draw where it was put";
  EXPECT_NE(PixelAt(pixels, kSize, 42, 42), 0xFFFFFFFF);

  // Second frame: the same layer, forty rows down, with nothing recorded.
  RfLayerTree* second = rf_layer_tree_new(kSize, kSize);
  rf_layer_tree_add_retained(second, kept, 8, 40);

  std::vector<uint8_t> moved(static_cast<size_t>(kSize) * kSize * 4u);
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(second, moved.data(), moved.size()),
            0);
  EXPECT_EQ(PixelAt(moved, kSize, 10, 42), 0xFFFFFFFF)
      << "the kept layer did not move with its new offset";
  EXPECT_NE(PixelAt(moved, kSize, 10, 10), 0xFFFFFFFF)
      << "and it should not still be where it was";

  // The first tree still holds it, so freeing the handle is not freeing the
  // layer -- which is the whole reason the two trees can share one.
  rf_layer_free(kept);
  rf_layer_tree_free(second);
  rf_layer_tree_free(first);
  rf_display_list_free(square);
  rf_paint_free(paint);
  rf_canvas_free(canvas);
}

// Nothing is open at the root, so there is nothing to keep. A framework that
// popped one too many should get a null rather than the root of its own tree.
TEST(RustFFI, RefusesToRetainTheRoot) {
  RfLayerTree* tree = rf_layer_tree_new(8, 8);
  EXPECT_EQ(rf_layer_tree_pop_retained(tree), nullptr);
  rf_layer_tree_free(tree);
}

// A repaint boundary whose subtree changed, under trees that did not.
//
// What upstream's `_repaintCompositedChild` does with the layer a boundary
// keeps: the old children are dropped and the new picture lands in the same
// layer *object*, so every tree holding that object -- an enclosing boundary's
// kept layer, in particular -- composites the new content without a single
// call above the boundary. The pixels here prove the sharing: the first tree
// is rasterized again after the re-record and shows the new picture, with no
// new add into that tree.
TEST(RustFFI, RerecordsIntoTheLayerItKept) {
  constexpr int32_t kSize = 64;

  RfPaint* paint = rf_paint_new();
  rf_paint_set_anti_alias(paint, 0);

  rf_paint_set_color(paint, 0xFFFF0000);
  RfCanvas* red_canvas = rf_canvas_new(kSize, kSize);
  rf_canvas_draw_rect(red_canvas, 0, 0, 8, 8, paint);
  RfDisplayList* red = rf_canvas_build(red_canvas);

  rf_paint_set_color(paint, 0xFF0000FF);
  RfCanvas* blue_canvas = rf_canvas_new(kSize, kSize);
  rf_canvas_draw_rect(blue_canvas, 0, 0, 8, 8, paint);
  RfDisplayList* blue = rf_canvas_build(blue_canvas);

  // First frame: a boundary records the red square and keeps the layer, and
  // the frame's tree carries it at (8, 8).
  RfLayerTree* first = rf_layer_tree_new(kSize, kSize);
  rf_layer_tree_push_offset(first, 8, 8);
  rf_layer_tree_push_retainable(first);
  rf_layer_tree_add_display_list(first, red, 0, 0);
  RfLayer* kept = rf_layer_tree_pop_retained(first);
  ASSERT_NE(kept, nullptr);
  rf_layer_tree_pop(first);

  std::vector<uint8_t> pixels(static_cast<size_t>(kSize) * kSize * 4u);
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(first, pixels.data(), pixels.size()),
            0);
  EXPECT_EQ(PixelAt(pixels, kSize, 10, 10), 0xFFFF0000)
      << "the layer did not draw what it was recorded with";

  // The repaint: whatever changed under the boundary is drawn again, straight
  // into the layer it kept. The tree this happens in is only the recording
  // context -- the layer is not added to it, so rasterizing it draws nothing.
  RfLayerTree* rerecorded = rf_layer_tree_new(kSize, kSize);
  rf_layer_tree_push_retained(rerecorded, kept);
  rf_layer_tree_add_display_list(rerecorded, blue, 0, 0);
  rf_layer_tree_pop(rerecorded);

  std::vector<uint8_t> nowhere(static_cast<size_t>(kSize) * kSize * 4u);
  ASSERT_EQ(
      rf_layer_tree_rasterize_bgra(rerecorded, nowhere.data(), nowhere.size()),
      0);
  EXPECT_EQ(PixelAt(nowhere, kSize, 10, 10), 0u)
      << "a re-record attached the layer to the tree it was recorded in";

  // The first tree was not touched, and rasterizing it again shows the new
  // picture: it holds the object, and the object holds the blue square now.
  // This is the whole mechanism -- an enclosing boundary's repaint becomes
  // visible without anything above it recording again.
  std::vector<uint8_t> shared(static_cast<size_t>(kSize) * kSize * 4u);
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(first, shared.data(), shared.size()),
            0);
  EXPECT_EQ(PixelAt(shared, kSize, 10, 10), 0xFF0000FF)
      << "the tree holding the layer did not see the re-record";
  EXPECT_NE(PixelAt(shared, kSize, 10, 10), 0xFFFF0000)
      << "the picture it was recorded with survived the re-record";

  // And a later frame composites the same object the ordinary way, still
  // showing what the re-record left in it.
  RfLayerTree* next = rf_layer_tree_new(kSize, kSize);
  rf_layer_tree_add_retained(next, kept, 8, 8);
  std::vector<uint8_t> carried(static_cast<size_t>(kSize) * kSize * 4u);
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(next, carried.data(), carried.size()),
            0);
  EXPECT_EQ(PixelAt(carried, kSize, 10, 10), 0xFF0000FF)
      << "the layer handed to a later frame lost the re-recorded picture";

  rf_layer_tree_free(next);
  rf_layer_tree_free(rerecorded);
  rf_layer_tree_free(first);
  rf_layer_free(kept);
  rf_display_list_free(blue);
  rf_display_list_free(red);
  rf_canvas_free(blue_canvas);
  rf_canvas_free(red_canvas);
  rf_paint_free(paint);
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
  ASSERT_EQ(rf_layer_tree_rasterize_bgra(tree, pixels.data(), pixels.size()),
            0);

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

// Pixels decoded by something other than Skia -- a platform codec, a camera --
// become an image and draw like any other. The album example reaches WIC this
// way, which is how it opens HEIC.
TEST(RustFFI, BuildsAnImageFromRawPixels) {
  constexpr int32_t kWidth = 4;
  constexpr int32_t kHeight = 3;

  // Premultiplied RGBA, which is the one layout rf_image_from_pixels takes.
  // Opaque, so premultiplied and straight are the same bytes and the expected
  // colour is readable in the source.
  std::vector<uint8_t> pixels(static_cast<size_t>(kWidth) * kHeight * 4);
  for (size_t i = 0; i < pixels.size(); i += 4) {
    pixels[i + 0] = 0x33;  // R
    pixels[i + 1] = 0x66;  // G
    pixels[i + 2] = 0xCC;  // B
    pixels[i + 3] = 0xFF;  // A
  }

  RfImage* image = rf_image_from_pixels(pixels.data(), kWidth, kHeight);
  ASSERT_NE(image, nullptr);
  EXPECT_EQ(rf_image_width(image), kWidth);
  EXPECT_EQ(rf_image_height(image), kHeight);

  constexpr int32_t kCanvasSize = 8;
  RfCanvas* canvas = rf_canvas_new(kCanvasSize, kCanvasSize);
  rf_canvas_draw_color(canvas, 0xFF000000);
  rf_canvas_draw_image(canvas, image, 0, 0, nullptr);
  RfDisplayList* display_list = rf_canvas_build(canvas);
  std::vector<uint8_t> drawn =
      Rasterize(display_list, kCanvasSize, kCanvasSize);
  ASSERT_FALSE(drawn.empty());

  // Inside the image, and outside it. The second half is what catches a width
  // read as a stride: the picture would then be wider than it was handed.
  EXPECT_EQ(PixelAt(drawn, kCanvasSize, 1, 1), 0xFF3366CCu);
  EXPECT_EQ(PixelAt(drawn, kCanvasSize, 6, 6), 0xFF000000u);

  rf_display_list_free(display_list);
  rf_canvas_free(canvas);
  rf_image_free(image);
}

TEST(RustFFI, RejectsPixelsThatCannotDescribeAnImage) {
  const uint8_t pixel[] = {0xFF, 0xFF, 0xFF, 0xFF};
  EXPECT_EQ(rf_image_from_pixels(nullptr, 1, 1), nullptr);
  EXPECT_EQ(rf_image_from_pixels(pixel, 0, 1), nullptr);
  EXPECT_EQ(rf_image_from_pixels(pixel, 1, 0), nullptr);
  EXPECT_EQ(rf_image_from_pixels(pixel, -1, -1), nullptr);
}

// -- The semantics tree, coming back out of the C ABI
// --------------------------

namespace {

// Two nodes that disagree in every field, so that a value taken from the wrong
// one of them shows up as something belonging elsewhere rather than as a
// plausible number. The strings are owned here because RfSemanticsNode only
// borrows them.
struct SemanticsFixture {
  std::string first_label = "first label";
  std::string first_value = "first value";
  std::string first_hint = "first hint";
  std::string first_up = "first up";
  std::string first_down = "first down";
  std::string first_tip = "first tip";
  std::string second_label = "second label";
  std::string second_value = "second value";
  std::string second_hint = "second hint";
  std::string second_up = "second up";
  std::string second_down = "second down";
  std::string second_tip = "second tip";
  std::vector<int32_t> first_children = {22};
  std::vector<int32_t> second_children = {};

  std::vector<RfSemanticsNode> Nodes() {
    RfSemanticsNode first = {};
    first.id = 11;
    first.flags = kRfSemanticsIsButton | kRfSemanticsHasCheckedState |
                  kRfSemanticsIsChecked;
    first.actions = 5;
    first.left = 1.0f;
    first.top = 2.0f;
    first.right = 3.0f;
    first.bottom = 4.0f;
    first.label = first_label.c_str();
    first.value = first_value.c_str();
    first.hint = first_hint.c_str();
    first.increased_value = first_up.c_str();
    first.decreased_value = first_down.c_str();
    first.tooltip = first_tip.c_str();
    first.role = 9;  // flutter::SemanticsRole::kColumnHeader
    first.flags |= kRfSemanticsScopesRoute;
    first.scroll_position = 12.0;
    first.scroll_extent_min = 1.0;
    first.scroll_extent_max = 100.0;
    first.children = first_children.data();
    first.child_count = first_children.size();
    first.text_direction = 2;
    first.scroll_index = 3;
    first.scroll_children = 40;

    RfSemanticsNode second = {};
    second.id = 22;
    second.flags = kRfSemanticsIsHeader;
    second.actions = 6;
    second.left = 5.0f;
    second.top = 6.0f;
    second.right = 7.0f;
    second.bottom = 8.0f;
    second.label = second_label.c_str();
    second.value = second_value.c_str();
    second.hint = second_hint.c_str();
    second.increased_value = second_up.c_str();
    second.decreased_value = second_down.c_str();
    second.tooltip = second_tip.c_str();
    second.role = 15;  // flutter::SemanticsRole::kMenuItem
    second.children = nullptr;
    second.child_count = 0;
    second.text_direction = 1;
    // The framework's "no answer" for both, which must not become a zero.
    second.scroll_index = -1;
    second.scroll_children = -1;
    return {first, second};
  }
};

}  // namespace

// The other half of the pair tested in app.rs. Forty lines of field-by-field
// copying that nothing ran until now: the conversion used to live inside
// RuntimeController::OnUpdateSemantics, which needs a controller and a
// delegate to reach.
TEST(RustSemantics, EveryFieldArrivesFromTheNodeItBelongsTo) {
  SemanticsFixture fixture;
  std::vector<RfSemanticsNode> nodes = fixture.Nodes();
  SemanticsNodeUpdates update =
      RustSemanticsNodesToUpdates(nodes.data(), nodes.size());

  ASSERT_EQ(update.size(), 2u);
  ASSERT_EQ(update.count(11), 1u);
  ASSERT_EQ(update.count(22), 1u);

  const SemanticsNode& first = update.at(11);
  EXPECT_EQ(first.id, 11);
  EXPECT_EQ(first.actions, 5);
  EXPECT_EQ(first.rect, SkRect::MakeLTRB(1.0f, 2.0f, 3.0f, 4.0f));
  // The five strings, in the order the struct lists them. Their slots are the
  // easiest thing here to transpose and the hardest to notice: a hint read as
  // a value is still announced, just in the wrong voice.
  EXPECT_EQ(first.label, "first label");
  EXPECT_EQ(first.value, "first value");
  EXPECT_EQ(first.hint, "first hint");
  EXPECT_EQ(first.increasedValue, "first up");
  EXPECT_EQ(first.decreasedValue, "first down");
  EXPECT_EQ(first.tooltip, "first tip");
  EXPECT_EQ(first.role, flutter::SemanticsRole::kColumnHeader);
  EXPECT_TRUE(first.flags.scopesRoute);
  EXPECT_EQ(first.scrollPosition, 12.0);
  EXPECT_EQ(first.scrollExtentMin, 1.0);
  EXPECT_EQ(first.scrollExtentMax, 100.0);
  EXPECT_EQ(first.textDirection, 2);
  EXPECT_TRUE(first.flags.isButton);
  EXPECT_FALSE(first.flags.isHeader);

  const SemanticsNode& second = update.at(22);
  EXPECT_EQ(second.id, 22);
  EXPECT_EQ(second.actions, 6);
  EXPECT_EQ(second.rect, SkRect::MakeLTRB(5.0f, 6.0f, 7.0f, 8.0f));
  EXPECT_EQ(second.label, "second label");
  EXPECT_EQ(second.value, "second value");
  EXPECT_EQ(second.hint, "second hint");
  EXPECT_EQ(second.increasedValue, "second up");
  EXPECT_EQ(second.decreasedValue, "second down");
  EXPECT_EQ(second.tooltip, "second tip");
  EXPECT_EQ(second.role, flutter::SemanticsRole::kMenuItem);
  EXPECT_FALSE(second.flags.scopesRoute);
  EXPECT_EQ(second.textDirection, 1);
  EXPECT_TRUE(second.flags.isHeader);
  EXPECT_FALSE(second.flags.isButton);
}

// The pair added in the round before this one, and the reason -1 is the null:
// the engine's fields are plain int32_t that default to 0, and row 0 of a list
// is a real answer. A framework that says "not a list" must leave the zero
// alone rather than assign one.
TEST(RustSemantics, TheAbsentScrollCountsLeaveTheEngineDefaultsAlone) {
  SemanticsFixture fixture;
  std::vector<RfSemanticsNode> nodes = fixture.Nodes();
  SemanticsNodeUpdates update =
      RustSemanticsNodesToUpdates(nodes.data(), nodes.size());

  EXPECT_EQ(update.at(11).scrollIndex, 3);
  EXPECT_EQ(update.at(11).scrollChildren, 40);
  EXPECT_EQ(update.at(22).scrollIndex, 0);
  EXPECT_EQ(update.at(22).scrollChildren, 0);
}

// Row 0 is an answer and has to survive as one. This is the case the -1 null
// exists to keep distinguishable from the node above it.
TEST(RustSemantics, RowZeroIsAnAnswerAndNotAnAbsence) {
  SemanticsFixture fixture;
  std::vector<RfSemanticsNode> nodes = fixture.Nodes();
  nodes[0].scroll_index = 0;
  nodes[0].scroll_children = 0;
  SemanticsNodeUpdates update =
      RustSemanticsNodesToUpdates(nodes.data(), nodes.size());
  EXPECT_EQ(update.at(11).scrollIndex, 0);
  EXPECT_EQ(update.at(11).scrollChildren, 0);
}

// The children cross as a pointer and a length that have to be read together,
// and both orders are filled from the one list: nothing here separates reading
// order from hit-test order, and saying so is what keeps them equal.
TEST(RustSemantics, ChildrenArriveInBothOrdersAndTheSameOne) {
  SemanticsFixture fixture;
  std::vector<RfSemanticsNode> nodes = fixture.Nodes();
  SemanticsNodeUpdates update =
      RustSemanticsNodesToUpdates(nodes.data(), nodes.size());

  const SemanticsNode& first = update.at(11);
  EXPECT_EQ(first.childrenInTraversalOrder, std::vector<int32_t>({22}));
  EXPECT_EQ(first.childrenInHitTestOrder, first.childrenInTraversalOrder);

  // A null child pointer is a leaf, not a crash: the framework sends one for
  // every node that has nothing under it.
  const SemanticsNode& second = update.at(22);
  EXPECT_TRUE(second.childrenInTraversalOrder.empty());
  EXPECT_TRUE(second.childrenInHitTestOrder.empty());
}

// Three bits for four states. The mixed bit outranks the checked one, and the
// "has a checked state at all" bit gates both -- so a node that was never
// checkable must not be announced as unchecked.
TEST(RustSemantics, TheFourthCheckStateSurvivesThreeBits) {
  SemanticsFixture fixture;
  const auto state_for = [&fixture](int32_t flags) {
    std::vector<RfSemanticsNode> nodes = fixture.Nodes();
    nodes[0].flags = flags;
    SemanticsNodeUpdates update =
        RustSemanticsNodesToUpdates(nodes.data(), nodes.size());
    return update.at(11).flags.isChecked;
  };

  EXPECT_EQ(state_for(0), SemanticsCheckState::kNone);
  EXPECT_EQ(state_for(kRfSemanticsHasCheckedState),
            SemanticsCheckState::kFalse);
  EXPECT_EQ(state_for(kRfSemanticsHasCheckedState | kRfSemanticsIsChecked),
            SemanticsCheckState::kTrue);
  EXPECT_EQ(state_for(kRfSemanticsHasCheckedState | kRfSemanticsIsChecked |
                      kRfSemanticsIsCheckStateMixed),
            SemanticsCheckState::kMixed)
      << "mixed outranks checked";
  EXPECT_EQ(state_for(kRfSemanticsIsChecked), SemanticsCheckState::kNone)
      << "checked without a checked state is not checkable";
}

// An empty tree is a tree: a frame in which everything went away has to arrive
// as an empty update rather than as no update at all, or the platform keeps
// showing what is no longer there.
TEST(RustSemantics, AnEmptyTreeConvertsToAnEmptyUpdate) {
  SemanticsNodeUpdates update = RustSemanticsNodesToUpdates(nullptr, 0);
  EXPECT_TRUE(update.empty());
}

// -- The application ABI
// -------------------------------------------------------

namespace {

// What the shell would have done, counted instead.
//
// Every rf_app_* entry point that changes something the shell can see asks for
// a frame on the way out, and asking is a call back through RfAppHost. So a
// host that counts its calls is enough to tell "the framework took this in"
// from "the framework ignored this", without a window or a rasterizer.
struct RecordingHost {
  int frames_requested = 0;

  static void ScheduleFrame(void* user_data) {
    static_cast<RecordingHost*>(user_data)->frames_requested++;
  }

  RfAppHost AsHost() {
    RfAppHost host = {};
    host.user_data = this;
    host.schedule_frame = &RecordingHost::ScheduleFrame;
    return host;
  }
};

RfViewMetrics MetricsOf(double width, double height) {
  RfViewMetrics metrics = {};
  metrics.width = width;
  metrics.height = height;
  metrics.device_pixel_ratio = 1.0;
  return metrics;
}

}  // namespace

// The crate's own #[test] binary excludes `mod abi` -- it does not link the
// engine, so keeping the #[no_mangle] symbols would leave every rf_* undefined.
// That exclusion is what leaves these seventeen functions to be covered here,
// which is the only binary that links both halves.
TEST(RustAppABI, CreateRefusesAHostThatIsNotThere) {
  // A shell that hands over nothing gets nothing back, rather than an app
  // holding a null host it will call on the first frame.
  EXPECT_EQ(rf_app_create(nullptr), nullptr);
}

TEST(RustAppABI, AnAppIsCreatedAndDestroyed) {
  RecordingHost recorder;
  RfAppHost host = recorder.AsHost();
  RfApp* app = rf_app_create(&host);
  ASSERT_NE(app, nullptr);
  rf_app_destroy(app);
}

TEST(RustAppABI, AddingAViewAsksForAFrame) {
  // The first thing the shell does after creating the app, and the reason the
  // ABI has a schedule_frame at all: a view that was just added has nothing on
  // it, and nothing will be drawn until somebody asks for a vsync.
  //
  // This also runs the other direction of the boundary -- the framework
  // calling out through RfAppHost -- which is the half `send_semantics` in
  // app.rs still notes as uncovered.
  RecordingHost recorder;
  RfAppHost host = recorder.AsHost();
  RfApp* app = rf_app_create(&host);
  ASSERT_NE(app, nullptr);
  EXPECT_EQ(recorder.frames_requested, 0) << "creating one asks for nothing";

  RfViewMetrics metrics = MetricsOf(400, 300);
  rf_app_add_view(app, 1, &metrics);
  EXPECT_EQ(recorder.frames_requested, 1);

  // Changing the size of a view it already has is news too: the same window at
  // a new size has to be laid out again.
  RfViewMetrics resized = MetricsOf(500, 300);
  rf_app_set_view_metrics(app, 1, &resized);
  EXPECT_GT(recorder.frames_requested, 1);

  rf_app_destroy(app);
}

TEST(RustAppABI, AddingAViewWithoutMetricsChangesNothing) {
  // Null metrics is a shell bug, and the framework declines rather than
  // reading through the pointer -- a view added with no size would be laid out
  // into nothing on the next frame.
  RecordingHost recorder;
  RfAppHost host = recorder.AsHost();
  RfApp* app = rf_app_create(&host);
  ASSERT_NE(app, nullptr);
  rf_app_add_view(app, 1, nullptr);
  EXPECT_EQ(recorder.frames_requested, 0);
  rf_app_destroy(app);
}

TEST(RustAppABI, LaunchSaysWhyItCouldNotRun) {
  // Three numbered answers, and this binary can reach two of them. The third
  // (-2, already launched) needs a successful launch first, and a successful
  // launch needs an application registered through register_application --
  // which is a process-wide OnceLock. **No test can have it both ways**: a
  // process where the factory is set cannot also observe -3. This binary is
  // the one where nothing is registered, so it is the one that can say what
  // happens to a shell with no framework behind it.
  EXPECT_EQ(rf_app_launch(nullptr), -1) << "no app at all";

  RecordingHost recorder;
  RfAppHost host = recorder.AsHost();
  RfApp* app = rf_app_create(&host);
  ASSERT_NE(app, nullptr);
  EXPECT_EQ(rf_app_launch(app), -3) << "an app, but no framework registered";
  rf_app_destroy(app);
}

TEST(RustAppABI, EveryEntryPointSurvivesANullApp) {
  // Seventeen functions, each of which casts its first argument back to an
  // AppInstance. The shell holds a null RfApp between destroy and the next
  // create, and any of these arriving in that window must decline rather than
  // dereference. Nothing here asserts an answer beyond "it returned" -- that
  // is the whole claim, and it is one that only shows up as a crash.
  rf_app_destroy(nullptr);
  EXPECT_EQ(rf_app_launch(nullptr), -1);

  RfViewMetrics metrics = MetricsOf(100, 100);
  rf_app_add_view(nullptr, 1, &metrics);
  rf_app_remove_view(nullptr, 1);
  rf_app_set_view_metrics(nullptr, 1, &metrics);

  rf_app_set_user_settings(nullptr, "{}", 2);
  const char* locale[] = {"en", "US", "", ""};
  rf_app_set_locales(nullptr, locale, 1);

  rf_app_begin_frame(nullptr, 0, 1);
  rf_app_draw_frame(nullptr);
  rf_app_run_tasks(nullptr);

  rf_app_dispatch_pointers(nullptr, nullptr, 0);
  RfKeyEvent key = {};
  EXPECT_FALSE(rf_app_dispatch_key(nullptr, &key));
  rf_app_dispatch_platform_message(nullptr, "flutter/system", nullptr, 0, 0);
  rf_app_complete_platform_message_reply(nullptr, 0, nullptr, 0);

  rf_app_set_semantics_enabled(nullptr, true);
  EXPECT_FALSE(rf_app_dispatch_semantics_action(nullptr, 1, 1));

  SUCCEED() << "every entry point declined a null app without dereferencing it";
}

// -- The Android key map ------------------------------------------------------
//
// Checked here rather than on a device because this is where a test can run at
// all: the table is compiled on every platform (see rust/host/BUILD.gn) and is
// arithmetic over constant data, with no Android type anywhere in it.
//
// The values asserted below are read from upstream's KeyboardMap.java, not
// from the generated file next door -- an expectation copied out of the code
// it is checking proves only that copying worked.

TEST(AndroidKeyMap, AKeyOnTheKeyboardIsTheKeyUpstreamSays) {
  // Tab: evdev scan code 0x0f, Android keyCode 0x3d (KEYCODE_TAB). Physical is
  // the USB HID usage 0x0007002b; logical is 0x0100000009 -- the control plane,
  // not the bare 0x09, because Tab is a key that means something rather than a
  // character that was typed.
  EXPECT_EQ(PhysicalKeyForAndroidKey(0x0f, 0x3d), 0x0007002bu);
  EXPECT_EQ(LogicalKeyForAndroidKeyCode(0x3d), 0x0100000009u);

  // A letter, whose logical value is the lower-case character itself. Android
  // does put these in its table, unlike Windows, so this is a lookup and not
  // the arithmetic the Windows map falls back on.
  EXPECT_EQ(PhysicalKeyForAndroidKey(0x1e, 0x1d), 0x00070004u);  // keyA
  EXPECT_EQ(LogicalKeyForAndroidKeyCode(0x1d), 0x61u);           // 'a'

  // Left and right Shift are one key code apart and are meant to stay apart:
  // a shortcut that does not care still has to see a press and a release
  // cancel each other, and that is the physical key's job.
  EXPECT_EQ(PhysicalKeyForAndroidKey(0x2a, 0x3b), 0x000700e1u);  // shiftLeft
  EXPECT_EQ(PhysicalKeyForAndroidKey(0x36, 0x3c), 0x000700e5u);  // shiftRight
  EXPECT_NE(LogicalKeyForAndroidKeyCode(0x3b),
            LogicalKeyForAndroidKeyCode(0x3c));
}

TEST(AndroidKeyMap, AKeyWithNoScanCodeIsStillTwoDifferentKeys) {
  // `adb shell input keyevent` and the emulator both send a scan code of zero.
  // Upstream synthesizes the physical key from the key code there, and the
  // reason is not tidiness: two keys sharing one physical value means the
  // release of one cancels the press of the other, and the framework then
  // believes a key is still down that nobody is holding.
  const uint64_t tab = PhysicalKeyForAndroidKey(0, 0x3d);
  const uint64_t enter = PhysicalKeyForAndroidKey(0, 0x42);
  EXPECT_EQ(tab, kAndroidPlane | 0x3d);
  EXPECT_EQ(enter, kAndroidPlane | 0x42);
  EXPECT_NE(tab, enter);

  // The logical key does not need the scan code at all, so it is the same one
  // a real keyboard would have produced -- which is what lets a synthetic Tab
  // drive a shortcut.
  EXPECT_EQ(LogicalKeyForAndroidKeyCode(0x3d), 0x0100000009u);
}

TEST(AndroidKeyMap, AKeyNobodyNamesKeepsItsOwnNumber) {
  // 0x1fff is past the end of both tables. An unnamed key still has to be a
  // *stable* identity, and it must not land on a real Unicode codepoint: 'a'
  // is 0x61, and a key that reported 0x61 raw would otherwise be
  // indistinguishable from the letter.
  //
  // The two numbers are deliberately different. With one number for both, the
  // physical fallback could use the key code instead of the scan code and no
  // assertion here could tell -- which is exactly what a mutation proved.
  EXPECT_EQ(PhysicalKeyForAndroidKey(0x1fff, 0x1ffe), kAndroidPlane | 0x1fff);
  EXPECT_EQ(LogicalKeyForAndroidKeyCode(0x1fff), kAndroidPlane | 0x1fff);
  EXPECT_NE(LogicalKeyForAndroidKeyCode(0x61), 0x61u)
      << "key code 0x61 is a game button, not the letter a";
}

TEST(AndroidKeyMap, AGapInTheMiddleOfTheTableIsStillAGap) {
  // Past the end is the easy miss: the search runs off the end and there is
  // nothing to return by mistake. A number *inside* the table's range with no
  // entry of its own is the hard one -- the search lands on the next entry
  // along, which is a real row with a real value, and only the "is this
  // actually the key I asked for" check stops it being returned.
  //
  // Scan code 0x54 has no entry and sits between 0x53 and 0x56; key code 0x4e
  // has none and sits between 0x4d and 0x4f.
  EXPECT_EQ(PhysicalKeyForAndroidKey(0x54, 0x54), kAndroidPlane | 0x54);
  EXPECT_NE(PhysicalKeyForAndroidKey(0x54, 0x54),
            PhysicalKeyForAndroidKey(0x56, 0x56))
      << "the gap took its neighbour's value";

  EXPECT_EQ(LogicalKeyForAndroidKeyCode(0x4e), kAndroidPlane | 0x4e);
  EXPECT_NE(LogicalKeyForAndroidKeyCode(0x4e),
            LogicalKeyForAndroidKeyCode(0x4f))
      << "the gap took its neighbour's value";
}

// -- Android modifier synchronisation -----------------------------------------

namespace {

// Physical and logical values for the keys these tests press, read from
// upstream's KeyboardMap.java rather than from the generated table.
constexpr uint64_t kShiftLeftPhysical = 0x000700e1;
constexpr uint64_t kShiftLeftLogical = 0x0200000102;
constexpr uint64_t kShiftRightPhysical = 0x000700e5;
constexpr uint32_t kShiftLeftKeyCode = 0x3b;
constexpr uint32_t kShiftLeftScanCode = 0x2a;
constexpr uint32_t kTabKeyCode = 0x3d;
constexpr uint32_t kTabScanCode = 0x0f;
constexpr uint64_t kTabPhysical = 0x0007002b;
constexpr int32_t kMetaShiftOn = 0x00000001;
// CapsLock. Note that 0x3a is its *scan* code; as a key code, 0x3a is
// altRight. The two numbers name different spaces and never stand in for one
// another.
constexpr uint32_t kCapsLockScanCode = 0x3a;
constexpr uint32_t kCapsLockKeyCode = 0x73;
constexpr uint64_t kCapsLockPhysical = 0x00070039;
constexpr uint64_t kCapsLockLogical = 0x0100000104;
constexpr int32_t kMetaCapsLockOn = 0x00100000;

/// One emitted event, flattened to what the assertions read.
struct Emitted {
  KeyEventType type;
  uint64_t physical;
  uint64_t logical;
  bool synthesized;
  std::string character;
};

/// Runs one event through and appends what came out.
AndroidKeyboard::Emit Collect(std::vector<Emitted>* out) {
  return [out](const KeyData& data, const std::string& character) {
    out->push_back({data.type, data.physical, data.logical,
                    data.synthesized != 0, character});
  };
}

AndroidKeyEvent Press(uint32_t key_code, uint32_t scan_code, int32_t meta) {
  AndroidKeyEvent event;
  event.key_code = key_code;
  event.scan_code = scan_code;
  event.meta_state = meta;
  event.down = true;
  return event;
}

AndroidKeyEvent Release(uint32_t key_code, uint32_t scan_code, int32_t meta) {
  AndroidKeyEvent event = Press(key_code, scan_code, meta);
  event.down = false;
  return event;
}

}  // namespace

TEST(AndroidKeySync, AnOrdinaryKeyIsPassedStraightThrough) {
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  EXPECT_TRUE(
      keyboard.Handle(Press(kTabKeyCode, kTabScanCode, 0), "", Collect(&out)));
  ASSERT_EQ(out.size(), 1u) << "nothing needed inventing";
  EXPECT_EQ(out[0].type, KeyEventType::kDown);
  EXPECT_EQ(out[0].physical, kTabPhysical);
  EXPECT_FALSE(out[0].synthesized);
  EXPECT_TRUE(keyboard.IsPressed(kTabPhysical));

  out.clear();
  EXPECT_TRUE(keyboard.Handle(Release(kTabKeyCode, kTabScanCode, 0), "",
                              Collect(&out)));
  ASSERT_EQ(out.size(), 1u);
  EXPECT_EQ(out[0].type, KeyEventType::kUp);
  EXPECT_FALSE(keyboard.IsPressed(kTabPhysical));
}

TEST(AndroidKeySync, AModifierHeldSinceBeforeTheAppStartedIsPressedForIt) {
  // The case this whole mechanism exists for. The user holds Shift, then
  // presses Tab; the Shift press happened before this host was listening, so
  // the framework has never seen it and Shift+Tab would traverse *forwards*.
  // Android says otherwise on every event, in `getMetaState()`.
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  ASSERT_FALSE(keyboard.IsPressed(kShiftLeftPhysical));
  EXPECT_TRUE(keyboard.Handle(Press(kTabKeyCode, kTabScanCode, kMetaShiftOn),
                              "", Collect(&out)));

  ASSERT_EQ(out.size(), 2u) << "a Shift press had to be invented first";
  EXPECT_EQ(out[0].type, KeyEventType::kDown);
  EXPECT_EQ(out[0].physical, kShiftLeftPhysical);
  EXPECT_EQ(out[0].logical, kShiftLeftLogical);
  EXPECT_TRUE(out[0].synthesized)
      << "nobody pressed this; the framework must know";
  EXPECT_TRUE(out[0].character.empty()) << "an invented press types nothing";

  EXPECT_EQ(out[1].type, KeyEventType::kDown);
  EXPECT_EQ(out[1].physical, kTabPhysical);
  EXPECT_FALSE(out[1].synthesized);

  // And it stays held, so the next key does not get a second invented press.
  // Tab is released first: a second press of a key still recorded as held is
  // its own case, and would add an invented release that has nothing to do
  // with the modifier.
  EXPECT_TRUE(keyboard.IsPressed(kShiftLeftPhysical));
  ASSERT_TRUE(keyboard.Handle(Release(kTabKeyCode, kTabScanCode, kMetaShiftOn),
                              "", Collect(&out)));
  out.clear();
  EXPECT_TRUE(keyboard.Handle(Press(kTabKeyCode, kTabScanCode, kMetaShiftOn),
                              "", Collect(&out)));
  ASSERT_EQ(out.size(), 1u) << "already in step";
  EXPECT_EQ(out[0].physical, kTabPhysical);
}

TEST(AndroidKeySync, AModifierReleasedOutOfSightIsReleasedForTheFramework) {
  // The other direction, and the one that wedges a running app: the release
  // went somewhere else -- another window, the system -- so the framework
  // still believes Shift is down and every subsequent shortcut is wrong.
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  ASSERT_TRUE(keyboard.Handle(
      Press(kShiftLeftKeyCode, kShiftLeftScanCode, kMetaShiftOn), "",
      Collect(&out)));
  ASSERT_TRUE(keyboard.IsPressed(kShiftLeftPhysical));

  out.clear();
  // A plain Tab, with the Shift bit now clear.
  EXPECT_TRUE(
      keyboard.Handle(Press(kTabKeyCode, kTabScanCode, 0), "", Collect(&out)));
  ASSERT_EQ(out.size(), 2u) << "the stale Shift had to be released first";
  EXPECT_EQ(out[0].type, KeyEventType::kUp);
  EXPECT_EQ(out[0].physical, kShiftLeftPhysical);
  EXPECT_TRUE(out[0].synthesized);
  EXPECT_EQ(out[1].physical, kTabPhysical);
  EXPECT_FALSE(keyboard.IsPressed(kShiftLeftPhysical));
}

TEST(AndroidKeySync, AnOnScreenKeyboardIsBelievedRatherThanCorrected) {
  // Gboard sets the Shift bit without ever sending a Shift key event. Inventing
  // the press it implies would leave a modifier held that nothing ever
  // releases -- the bit clears silently and no event carries the release.
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  AndroidKeyEvent event = Press(kTabKeyCode, kTabScanCode, kMetaShiftOn);
  event.virtual_keyboard = true;
  EXPECT_TRUE(keyboard.Handle(event, "", Collect(&out)));

  ASSERT_EQ(out.size(), 1u) << "no Shift press was invented";
  EXPECT_EQ(out[0].physical, kTabPhysical);
  EXPECT_FALSE(keyboard.IsPressed(kShiftLeftPhysical));
}

TEST(AndroidKeySync, AReleaseNobodySawThePressOfIsDropped) {
  // Passing it on would ask the framework to take a key out of its held set
  // that something else put there -- or that is not there at all.
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  EXPECT_FALSE(keyboard.Handle(Release(kTabKeyCode, kTabScanCode, 0), "",
                               Collect(&out)));
  EXPECT_TRUE(out.empty());
}

TEST(AndroidKeySync, APressOfAKeyAlreadyHeldReleasesItFirst) {
  // Two downs with no up between them and no repeat flag: the release was
  // lost. Without the invented release the framework would record a second
  // press of a key it already has down.
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  ASSERT_TRUE(
      keyboard.Handle(Press(kTabKeyCode, kTabScanCode, 0), "a", Collect(&out)));
  out.clear();

  EXPECT_TRUE(
      keyboard.Handle(Press(kTabKeyCode, kTabScanCode, 0), "a", Collect(&out)));
  ASSERT_EQ(out.size(), 2u);
  EXPECT_EQ(out[0].type, KeyEventType::kUp);
  EXPECT_TRUE(out[0].synthesized);
  EXPECT_EQ(out[1].type, KeyEventType::kDown);
  EXPECT_FALSE(out[1].synthesized);
}

TEST(AndroidKeySync, AHeldKeyRepeatsRatherThanPressingAgain) {
  // A repeat is not a press: the framework's held set already has the key, and
  // a second down would be a second keystroke. The distinction is drawn from
  // the record, not from the repeat flag alone.
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  ASSERT_TRUE(
      keyboard.Handle(Press(kTabKeyCode, kTabScanCode, 0), "", Collect(&out)));
  out.clear();

  AndroidKeyEvent again = Press(kTabKeyCode, kTabScanCode, 0);
  again.repeat = true;
  EXPECT_TRUE(keyboard.Handle(again, "", Collect(&out)));
  ASSERT_EQ(out.size(), 1u);
  EXPECT_EQ(out[0].type, KeyEventType::kRepeat);

  // A repeat flag on a key that is *not* recorded is still a first press --
  // the record decides, not the flag.
  AndroidKeyboard fresh;
  out.clear();
  AndroidKeyEvent orphan = Press(kTabKeyCode, kTabScanCode, 0);
  orphan.repeat = true;
  EXPECT_TRUE(fresh.Handle(orphan, "", Collect(&out)));
  ASSERT_EQ(out.size(), 1u);
  EXPECT_EQ(out[0].type, KeyEventType::kDown);
}

TEST(AndroidKeySync, TheEventsOwnModifierIsNotInventedAroundItself) {
  // Pressing Shift itself, with the bit already set: the press *is* the thing
  // that makes it true, so nothing should be invented before it, and exactly
  // one event goes out.
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  EXPECT_TRUE(keyboard.Handle(
      Press(kShiftLeftKeyCode, kShiftLeftScanCode, kMetaShiftOn), "",
      Collect(&out)));
  ASSERT_EQ(out.size(), 1u);
  EXPECT_EQ(out[0].physical, kShiftLeftPhysical);
  EXPECT_FALSE(out[0].synthesized);

  // The other side of the pair was never touched.
  EXPECT_FALSE(keyboard.IsPressed(kShiftRightPhysical));
}

TEST(AndroidKeySync, AReleaseCarriesNoCharacter) {
  // Android will map a key code to a character on the way up as readily as on
  // the way down. Only a press types.
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  ASSERT_TRUE(keyboard.Handle(Press(0x1d, 0x1e, 0), "a", Collect(&out)));
  ASSERT_EQ(out.size(), 1u);
  EXPECT_EQ(out[0].character, "a");

  out.clear();
  ASSERT_TRUE(keyboard.Handle(Release(0x1d, 0x1e, 0), "a", Collect(&out)));
  ASSERT_EQ(out.size(), 1u);
  EXPECT_TRUE(out[0].character.empty());
}

TEST(AndroidKeySync, ALockThatWentOnUnseenIsToggledWithAPairOfEvents) {
  // A lock is not a modifier: it is on while nobody is touching its key, so
  // the framework has no held key to read it off. It flips its mode on each
  // *key down* of the lock key, which is why one synthesized event cannot do
  // this -- an up would flip nothing, and a lone down would flip it and leave
  // CapsLock recorded as held for the rest of the session.
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  ASSERT_FALSE(keyboard.IsLockEnabled(kCapsLockLogical));
  EXPECT_TRUE(keyboard.Handle(Press(kTabKeyCode, kTabScanCode, kMetaCapsLockOn),
                              "", Collect(&out)));

  ASSERT_EQ(out.size(), 3u) << "a press and a release, then the real key";
  EXPECT_EQ(out[0].type, KeyEventType::kDown);
  EXPECT_EQ(out[0].physical, kCapsLockPhysical);
  EXPECT_TRUE(out[0].synthesized);
  EXPECT_EQ(out[1].type, KeyEventType::kUp);
  EXPECT_EQ(out[1].physical, kCapsLockPhysical);
  EXPECT_TRUE(out[1].synthesized);
  EXPECT_EQ(out[2].physical, kTabPhysical);

  EXPECT_TRUE(keyboard.IsLockEnabled(kCapsLockLogical));
  EXPECT_FALSE(keyboard.IsPressed(kCapsLockPhysical))
      << "the pair left the key as it found it";

  // In step now, so the next key invents nothing.
  out.clear();
  ASSERT_TRUE(keyboard.Handle(
      Release(kTabKeyCode, kTabScanCode, kMetaCapsLockOn), "", Collect(&out)));
  EXPECT_EQ(out.size(), 1u);
}

TEST(AndroidKeySync, ALockThatWentOffUnseenIsToggledBack) {
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  ASSERT_TRUE(keyboard.Handle(Press(kTabKeyCode, kTabScanCode, kMetaCapsLockOn),
                              "", Collect(&out)));
  ASSERT_TRUE(keyboard.IsLockEnabled(kCapsLockLogical));

  out.clear();
  ASSERT_TRUE(keyboard.Handle(Release(kTabKeyCode, kTabScanCode, 0), "",
                              Collect(&out)));
  ASSERT_EQ(out.size(), 3u) << "off is as much a change as on";
  EXPECT_EQ(out[0].type, KeyEventType::kDown) << "a down is what flips it";
  EXPECT_EQ(out[1].type, KeyEventType::kUp);
  EXPECT_FALSE(keyboard.IsLockEnabled(kCapsLockLogical));
}

TEST(AndroidKeySync, TheLockKeysOwnEventIsLeftAlone) {
  // Upstream skips the lock's own events, and the reason is ChromeOS: there,
  // CapsLock's own events carry the bit as though it were a held modifier --
  // set on the way down, clear on the way up -- while every other event
  // carries it as the lock state. Reconciling against it here would toggle the
  // lock a second time on top of the press itself.
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  EXPECT_TRUE(keyboard.Handle(
      Press(kCapsLockKeyCode, kCapsLockScanCode, kMetaCapsLockOn), "",
      Collect(&out)));
  ASSERT_EQ(out.size(), 1u) << "nothing was invented around it";
  EXPECT_FALSE(out[0].synthesized);
  EXPECT_EQ(out[0].physical, kCapsLockPhysical);

  // The press itself is what flipped the lock, exactly once.
  EXPECT_TRUE(keyboard.IsLockEnabled(kCapsLockLogical));
  EXPECT_TRUE(keyboard.IsPressed(kCapsLockPhysical));

  // And the release does not flip it back.
  out.clear();
  ASSERT_TRUE(keyboard.Handle(
      Release(kCapsLockKeyCode, kCapsLockScanCode, kMetaCapsLockOn), "",
      Collect(&out)));
  EXPECT_EQ(out.size(), 1u);
  EXPECT_TRUE(keyboard.IsLockEnabled(kCapsLockLogical));
}

TEST(AndroidKeySync, AnEventWithNoNumbersAtAllIsNotAKey) {
  AndroidKeyboard keyboard;
  std::vector<Emitted> out;

  EXPECT_FALSE(keyboard.Handle(Press(0, 0, 0), "", Collect(&out)));
  EXPECT_TRUE(out.empty());
}

}  // namespace testing
}  // namespace flutter
