// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_FFI_RUSTFLUTTER_FFI_INTERNAL_H_
#define FLUTTER_RUST_FFI_RUSTFLUTTER_FFI_INTERNAL_H_

// C++-only companion to rustflutter_ffi.h.
//
// The C ABI deliberately never exposes a C++ type, so RfLayerTree is opaque to
// Rust. The shell, however, is C++ and needs the flow::LayerTree inside it --
// that tree is the single value the framework layer produces, and handing it to
// RuntimeDelegate::Render is the whole seam. This header is the one place that
// unwraps the handle.

#include <memory>

#include "flutter/flow/layers/layer_tree.h"
#include "flutter/rust/ffi/rustflutter_ffi.h"

namespace flutter {

//------------------------------------------------------------------------------
/// Consumes an RfLayerTree handle and returns the flow layer tree it wraps.
///
/// The handle is freed; the caller must not use it afterwards. Returns nullptr
/// if `handle` is null or has a degenerate size.
std::unique_ptr<LayerTree> RfLayerTreeTake(RfLayerTree* handle);

}  // namespace flutter

#endif  // FLUTTER_RUST_FFI_RUSTFLUTTER_FFI_INTERNAL_H_
