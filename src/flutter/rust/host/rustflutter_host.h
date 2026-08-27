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

#include "flutter/rust/ffi/rustflutter_export.h"

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
RF_EXPORT
int32_t rf_host_run(const RfHostOptions* options);

//------------------------------------------------------------------------------
/// The application's own entry point: what `main` calls on a desktop, and what
/// an application compiles `rustflutter_app_main` to everywhere.
typedef int32_t (*RfAppMain)(int32_t argc, const char** argv);

//------------------------------------------------------------------------------
/// Tells the host how to start the application.
///
/// Only Android needs this, and needs it because there the *host* decides when
/// the application starts: the Activity owns the process, and nothing can run
/// until it has a Surface. On a desktop the application owns `main` and calls
/// its own entry point, so it registers nothing and this stays NULL.
///
/// It exists at all for the reason `rf_set_app_interface` does. The host used
/// to declare `rustflutter_app_main` and call it by name, which resolves when
/// the host and the application are one binary and does not when the host is in
/// a shared library the application loads -- the call points out of the library
/// and up into its consumer. Registering turns it back around.
///
/// Called before the Surface exists, from a load-time initialiser in the
/// application: the C++ shim in rustflutter_app.gni for a GN application, and
/// the framework's own `.init_array` entry for a Cargo one. Writing it twice is
/// harmless and writes the same pointer.
RF_EXPORT
void rf_set_app_main(RfAppMain app_main);

//------------------------------------------------------------------------------
/// The registered entry point, or NULL if nothing registered one.
RF_EXPORT
RfAppMain rf_app_main(void);

#if defined(__cplusplus)
}  // extern "C"
#endif

#endif  // FLUTTER_RUST_HOST_RUSTFLUTTER_HOST_H_
