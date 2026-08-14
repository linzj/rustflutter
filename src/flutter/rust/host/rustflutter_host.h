// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_HOST_RUSTFLUTTER_HOST_H_
#define FLUTTER_RUST_HOST_RUSTFLUTTER_HOST_H_

// Starts a real Shell and runs the Rust framework in it.
//
// This replaces the M1 stopgap, where Rust called Flatten() and blitted the
// result itself. Everything the engine normally does now happens:
//
//     platform thread  window, input, Shell lifecycle
//     UI thread        Animator -> Engine -> RuntimeController -> Rust
//     raster thread    Rasterizer -> Surface -> present
//     IO thread        ShellIOManager
//
// Frames are driven by the vsync waiter, not by a call from the app, and the
// layer tree travels the engine's own pipeline instead of being flattened on
// the spot.

#include <stddef.h>
#include <stdint.h>

#if defined(__cplusplus)
extern "C" {
#endif

typedef struct RfHostOptions {
  // Window size in logical pixels. The host multiplies by the display scale.
  int32_t width;
  int32_t height;
  const char* title;
  // Path to icudtl.dat. NULL looks next to the executable.
  const char* icu_data_path;
  // Non-zero renders with Impeller. Zero uses the Skia software surface, which
  // needs no GPU context and is what the host defaults to today.
  int32_t enable_impeller;
} RfHostOptions;

//------------------------------------------------------------------------------
/// Opens a window, starts the shell, and pumps until the window closes.
///
/// Blocking; call from the process's main thread. Returns 0 on a normal exit.
/// An application must already be registered on the Rust side.
int32_t rf_host_run(const RfHostOptions* options);

#if defined(__cplusplus)
}  // extern "C"
#endif

#endif  // FLUTTER_RUST_HOST_RUSTFLUTTER_HOST_H_
