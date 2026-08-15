// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "flutter/rust/ffi/rustflutter_ffi.h"

#include "flutter/rust/ffi/rustflutter_ffi_handles.h"

#include <atomic>
#include <cstring>
#include <memory>
#include <string>
#include <vector>

#include "flutter/display_list/dl_builder.h"
#include "flutter/display_list/dl_color.h"
#include "flutter/display_list/dl_paint.h"
#include "flutter/display_list/geometry/dl_geometry_types.h"
#include "flutter/display_list/skia/dl_sk_canvas.h"
#include "flutter/flow/layers/container_layer.h"
#include "flutter/flow/layers/display_list_layer.h"
#include "flutter/flow/layers/layer_tree.h"
#include "flutter/fml/file.h"
#include "flutter/fml/icu_util.h"
#include "flutter/fml/logging.h"
#include "flutter/fml/paths.h"
#include "third_party/skia/include/core/SkCanvas.h"
#include "third_party/skia/include/core/SkData.h"
#include "third_party/skia/include/core/SkFontMgr.h"
#include "third_party/skia/include/core/SkStream.h"
#include "third_party/skia/include/core/SkSurface.h"
#include "third_party/skia/include/encode/SkPngEncoder.h"
#include "txt/font_collection.h"
#include "txt/paragraph.h"
#include "txt/platform.h"
#include "txt/paragraph_builder.h"
#include "txt/paragraph_style.h"
#include "txt/text_style.h"

namespace {

// The engine expects exactly one FontCollection per process for the default
// (system) font manager; building one per paragraph would re-scan the system
// font list on every call.
std::shared_ptr<txt::FontCollection> GetFontCollection() {
  static std::shared_ptr<txt::FontCollection>* collection = [] {
    auto* c = new std::shared_ptr<txt::FontCollection>(
        std::make_shared<txt::FontCollection>());
    (*c)->SetupDefaultFontManager(0);
    return c;
  }();
  return *collection;
}

flutter::DlColor ToDlColor(uint32_t argb) {
  return flutter::DlColor(argb);
}

// Set once on the raster thread during shell startup, read on the UI thread
// for every paragraph. Atomic rather than plain because of that crossing, and
// relaxed because the write happens-before the first read by way of the
// shell's own startup barriers.
std::atomic<bool> g_impeller_text{false};

}  // namespace


// -- Process setup ------------------------------------------------------------

int32_t rf_initialize(const char* icu_data_path) {
  static bool initialized = false;
  if (initialized) {
    return 0;
  }

  std::string path;
  if (icu_data_path != nullptr && icu_data_path[0] != '\0') {
    path = icu_data_path;
  } else {
    // Ship icudtl.dat alongside the binary, the way the engine's own host
    // test shells do. Without it skparagraph cannot break lines and every
    // Layout() call logs U_MISSING_RESOURCE_ERROR.
    auto executable_directory = fml::paths::GetExecutableDirectoryPath();
    if (!executable_directory.first) {
      FML_LOG(ERROR) << "rf_initialize: could not locate the executable "
                        "directory to find icudtl.dat.";
      return -1;
    }
    path = fml::paths::JoinPaths({executable_directory.second, "icudtl.dat"});
  }

  fml::icu::InitializeICU(path);
  initialized = true;
  return 0;
}

void rf_set_impeller_text(int32_t enabled) {
  g_impeller_text.store(enabled != 0, std::memory_order_relaxed);
}

// -- Paint --------------------------------------------------------------------

RfPaint* rf_paint_new() {
  auto* paint = new RfPaint();
  paint->paint.setAntiAlias(true);
  return paint;
}

void rf_paint_free(RfPaint* paint) {
  delete paint;
}

void rf_paint_set_color(RfPaint* paint, uint32_t argb) {
  if (paint == nullptr) {
    return;
  }
  paint->paint.setColor(ToDlColor(argb));
}

void rf_paint_set_stroke(RfPaint* paint, int32_t stroke, float width) {
  if (paint == nullptr) {
    return;
  }
  paint->paint.setDrawStyle(stroke != 0 ? flutter::DlDrawStyle::kStroke
                                        : flutter::DlDrawStyle::kFill);
  paint->paint.setStrokeWidth(width);
}

void rf_paint_set_anti_alias(RfPaint* paint, int32_t anti_alias) {
  if (paint == nullptr) {
    return;
  }
  paint->paint.setAntiAlias(anti_alias != 0);
}

// -- Canvas -------------------------------------------------------------------

RfCanvas* rf_canvas_new(float width, float height) {
  return new RfCanvas(flutter::DlRect::MakeWH(width, height));
}

void rf_canvas_free(RfCanvas* canvas) {
  delete canvas;
}

void rf_canvas_draw_color(RfCanvas* canvas, uint32_t argb) {
  if (canvas == nullptr) {
    return;
  }
  canvas->builder.DrawColor(ToDlColor(argb), flutter::DlBlendMode::kSrc);
}

void rf_canvas_draw_rect(RfCanvas* canvas,
                         float left,
                         float top,
                         float right,
                         float bottom,
                         const RfPaint* paint) {
  if (canvas == nullptr || paint == nullptr) {
    return;
  }
  canvas->builder.DrawRect(flutter::DlRect::MakeLTRB(left, top, right, bottom),
                           paint->paint);
}

void rf_canvas_draw_rrect(RfCanvas* canvas,
                          float left,
                          float top,
                          float right,
                          float bottom,
                          float radius,
                          const RfPaint* paint) {
  if (canvas == nullptr || paint == nullptr) {
    return;
  }
  auto rect = flutter::DlRect::MakeLTRB(left, top, right, bottom);
  canvas->builder.DrawRoundRect(
      flutter::DlRoundRect::MakeRectXY(rect, radius, radius), paint->paint);
}

void rf_canvas_draw_circle(RfCanvas* canvas,
                           float center_x,
                           float center_y,
                           float radius,
                           const RfPaint* paint) {
  if (canvas == nullptr || paint == nullptr) {
    return;
  }
  canvas->builder.DrawCircle(flutter::DlPoint(center_x, center_y), radius,
                             paint->paint);
}

void rf_canvas_draw_paragraph(RfCanvas* canvas,
                              RfParagraph* paragraph,
                              float x,
                              float y) {
  if (canvas == nullptr || paragraph == nullptr ||
      paragraph->paragraph == nullptr) {
    return;
  }
  if (!paragraph->laid_out) {
    FML_LOG(ERROR) << "rf_canvas_draw_paragraph: paragraph was not laid out; "
                      "call rf_paragraph_layout first.";
    return;
  }
  paragraph->paragraph->Paint(&canvas->builder, x, y);
}

RfDisplayList* rf_canvas_build(RfCanvas* canvas) {
  if (canvas == nullptr) {
    return nullptr;
  }
  auto* out = new RfDisplayList();
  out->list = canvas->builder.Build();
  return out;
}

void rf_display_list_free(RfDisplayList* display_list) {
  delete display_list;
}

// -- Text ---------------------------------------------------------------------

RfParagraph* rf_paragraph_new(const char* text,
                              size_t text_len,
                              const char* font_family,
                              float font_size,
                              int32_t font_weight,
                              uint32_t argb,
                              int32_t text_align) {
  if (text == nullptr) {
    return nullptr;
  }

  txt::ParagraphStyle paragraph_style;
  switch (text_align) {
    case 1:
      paragraph_style.text_align = txt::TextAlign::right;
      break;
    case 2:
      paragraph_style.text_align = txt::TextAlign::center;
      break;
    default:
      paragraph_style.text_align = txt::TextAlign::left;
      break;
  }

  auto builder = txt::ParagraphBuilder::CreateSkiaBuilder(
      paragraph_style, GetFontCollection(),
      g_impeller_text.load(std::memory_order_relaxed));

  txt::TextStyle style;
  style.font_size = font_size;
  style.color = argb;
  // txt::FontWeight is a plain int holding the CSS weight (400 == normal),
  // so the value passes straight through after clamping.
  style.font_weight = font_weight < 100 ? 100 : (font_weight > 900 ? 900 : font_weight);
  if (font_family != nullptr && font_family[0] != '\0') {
    style.font_families = std::vector<std::string>{std::string(font_family)};
  } else {
    style.font_families = txt::GetDefaultFontFamilies();
  }

  builder->PushStyle(style);
  builder->AddText(reinterpret_cast<const uint8_t*>(text), text_len);
  builder->Pop();

  auto* out = new RfParagraph();
  out->paragraph = builder->Build();
  return out;
}

void rf_paragraph_free(RfParagraph* paragraph) {
  delete paragraph;
}

void rf_paragraph_layout(RfParagraph* paragraph, float max_width) {
  if (paragraph == nullptr || paragraph->paragraph == nullptr) {
    return;
  }
  paragraph->paragraph->Layout(max_width);
  paragraph->laid_out = true;
}

float rf_paragraph_width(RfParagraph* paragraph) {
  if (paragraph == nullptr || paragraph->paragraph == nullptr) {
    return 0.0f;
  }
  return static_cast<float>(paragraph->paragraph->GetMaxWidth());
}

float rf_paragraph_height(RfParagraph* paragraph) {
  if (paragraph == nullptr || paragraph->paragraph == nullptr) {
    return 0.0f;
  }
  return static_cast<float>(paragraph->paragraph->GetHeight());
}

float rf_paragraph_longest_line(RfParagraph* paragraph) {
  if (paragraph == nullptr || paragraph->paragraph == nullptr) {
    return 0.0f;
  }
  return static_cast<float>(paragraph->paragraph->GetLongestLine());
}

float rf_paragraph_baseline(RfParagraph* paragraph) {
  if (paragraph == nullptr || paragraph->paragraph == nullptr) {
    return 0.0f;
  }
  return static_cast<float>(paragraph->paragraph->GetAlphabeticBaseline());
}

float rf_paragraph_min_intrinsic_width(RfParagraph* paragraph) {
  if (paragraph == nullptr || paragraph->paragraph == nullptr) {
    return 0.0f;
  }
  return static_cast<float>(paragraph->paragraph->GetMinIntrinsicWidth());
}

float rf_paragraph_max_intrinsic_width(RfParagraph* paragraph) {
  if (paragraph == nullptr || paragraph->paragraph == nullptr) {
    return 0.0f;
  }
  return static_cast<float>(paragraph->paragraph->GetMaxIntrinsicWidth());
}

// -- Layer tree ---------------------------------------------------------------

RfLayerTree* rf_layer_tree_new(int32_t width, int32_t height) {
  auto* tree = new RfLayerTree();
  tree->width = width;
  tree->height = height;
  return tree;
}

void rf_layer_tree_free(RfLayerTree* tree) {
  delete tree;
}

void rf_layer_tree_add_display_list(RfLayerTree* tree,
                                    RfDisplayList* display_list,
                                    float offset_x,
                                    float offset_y) {
  if (tree == nullptr || display_list == nullptr ||
      display_list->list == nullptr) {
    return;
  }
  tree->Current().Add(std::make_shared<flutter::DisplayListLayer>(
      flutter::DlPoint(offset_x, offset_y), display_list->list,
      /*is_complex=*/false, /*will_change=*/false));
}

namespace flutter {

std::unique_ptr<LayerTree> RfLayerTreeTake(RfLayerTree* handle) {
  if (handle == nullptr || handle->width <= 0 || handle->height <= 0) {
    delete handle;
    return nullptr;
  }
  auto tree = std::make_unique<LayerTree>(
      handle->root, DlISize(handle->width, handle->height));
  delete handle;
  return tree;
}

}  // namespace flutter

// -- Rasterization ------------------------------------------------------------

namespace {

// Flattens the tree the same way the engine does before handing it to the
// rasterizer, then replays it into a CPU-backed Skia surface. Impeller is the
// production path; a raster surface keeps this dependency-free and byte-exact
// for tests and for headless rendering.
sk_sp<SkSurface> RasterizeToSurface(RfLayerTree* tree) {
  if (tree == nullptr || tree->width <= 0 || tree->height <= 0) {
    return nullptr;
  }

  flutter::LayerTree layer_tree(tree->root,
                                flutter::DlISize(tree->width, tree->height));
  auto bounds = flutter::DlRect::MakeWH(static_cast<float>(tree->width),
                                        static_cast<float>(tree->height));
  sk_sp<flutter::DisplayList> flattened = layer_tree.Flatten(bounds);
  if (flattened == nullptr) {
    return nullptr;
  }

  SkImageInfo info = SkImageInfo::MakeN32Premul(tree->width, tree->height);
  sk_sp<SkSurface> surface = SkSurfaces::Raster(info);
  if (surface == nullptr) {
    return nullptr;
  }

  surface->getCanvas()->clear(SK_ColorTRANSPARENT);
  flutter::DlSkCanvasAdapter canvas(surface->getCanvas());
  canvas.DrawDisplayList(flattened);
  return surface;
}

}  // namespace

int32_t rf_layer_tree_write_png(RfLayerTree* tree, const char* path) {
  if (path == nullptr) {
    return -1;
  }
  sk_sp<SkSurface> surface = RasterizeToSurface(tree);
  if (surface == nullptr) {
    return -2;
  }
  sk_sp<SkImage> image = surface->makeImageSnapshot();
  if (image == nullptr) {
    return -3;
  }
  // The GrDirectContext overload accepts a null context for CPU-backed images.
  sk_sp<SkData> png =
      SkPngEncoder::Encode(nullptr, image.get(), SkPngEncoder::Options{});
  if (png == nullptr) {
    return -5;
  }
  SkFILEWStream stream(path);
  if (!stream.isValid()) {
    return -4;
  }
  if (!stream.write(png->data(), png->size())) {
    return -4;
  }
  stream.flush();
  return 0;
}

int32_t rf_layer_tree_rasterize_bgra(RfLayerTree* tree,
                                     uint8_t* out_pixels,
                                     size_t out_len) {
  if (out_pixels == nullptr || tree == nullptr) {
    return -1;
  }
  const size_t needed = static_cast<size_t>(tree->width) *
                        static_cast<size_t>(tree->height) * 4u;
  if (out_len < needed) {
    return -1;
  }
  sk_sp<SkSurface> surface = RasterizeToSurface(tree);
  if (surface == nullptr) {
    return -2;
  }
  SkImageInfo info = SkImageInfo::Make(tree->width, tree->height,
                                       kBGRA_8888_SkColorType,
                                       kPremul_SkAlphaType);
  if (!surface->readPixels(info, out_pixels, tree->width * 4, 0, 0)) {
    return -3;
  }
  return 0;
}
