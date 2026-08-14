// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_FFI_RUSTFLUTTER_FFI_HANDLES_H_
#define FLUTTER_RUST_FFI_RUSTFLUTTER_FFI_HANDLES_H_

// The C++ behind the opaque handles in rustflutter_ffi.h.
//
// Each handle type is a plain struct wrapping an engine object, so the C ABI
// never exposes a C++ type and ownership is obvious from the Rust side. This
// header exists only so the implementation can be split across several files;
// nothing outside rust/ffi should include it.

#include <memory>
#include <vector>

#include "flutter/display_list/dl_builder.h"
#include "flutter/display_list/dl_paint.h"
#include "flutter/display_list/geometry/dl_geometry_types.h"
#include "flutter/display_list/geometry/dl_path.h"
#include "flutter/display_list/geometry/dl_path_builder.h"
#include "flutter/display_list/image/dl_image.h"
#include "flutter/flow/layers/container_layer.h"
#include "txt/paragraph.h"

struct RfPaint {
  flutter::DlPaint paint;
};

struct RfCanvas {
  explicit RfCanvas(const flutter::DlRect& cull)
      : builder(cull, /*prepare_rtree=*/false) {}
  flutter::DisplayListBuilder builder;
};

struct RfDisplayList {
  sk_sp<flutter::DisplayList> list;
};

struct RfParagraph {
  std::unique_ptr<txt::Paragraph> paragraph;
  bool laid_out = false;
};

//------------------------------------------------------------------------------
/// A path under construction, and the built result.
///
/// DlPathBuilder is single-shot -- TakePath() consumes it -- but a path handle
/// may be drawn many times and added to after being drawn. `built` therefore
/// caches the last result and is invalidated by every mutation.
struct RfPath {
  flutter::DlPathBuilder builder;
  std::optional<flutter::DlPath> built;

  const flutter::DlPath& Path() {
    if (!built.has_value()) {
      built = builder.CopyPath();
    }
    return built.value();
  }

  void Invalidate() { built.reset(); }
};

struct RfImage {
  sk_sp<flutter::DlImage> image;
};

//------------------------------------------------------------------------------
/// The layer tree under construction.
///
/// `stack` is the open container layers, innermost last, with `root` always at
/// the bottom. Everything added lands in stack.back(), which is what makes
/// push/pop work the way SceneBuilder does.
struct RfLayerTree {
  RfLayerTree() { stack.push_back(root); }

  int32_t width = 0;
  int32_t height = 0;
  std::shared_ptr<flutter::ContainerLayer> root =
      std::make_shared<flutter::ContainerLayer>();
  std::vector<std::shared_ptr<flutter::ContainerLayer>> stack;

  flutter::ContainerLayer& Current() { return *stack.back(); }

  void Push(const std::shared_ptr<flutter::ContainerLayer>& layer) {
    stack.back()->Add(layer);
    stack.push_back(layer);
  }

  void Pop() {
    // The root is not poppable: an unbalanced pop from the framework should
    // not be able to leave the tree without somewhere to add to.
    if (stack.size() > 1) {
      stack.pop_back();
    }
  }
};

#endif  // FLUTTER_RUST_FFI_RUSTFLUTTER_FFI_HANDLES_H_
