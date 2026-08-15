// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUNTIME_RUST_APP_API_H_
#define FLUTTER_RUNTIME_RUST_APP_API_H_

// The contract between the shell and the Rust framework layer.
//
// Upstream this is two things at once: the 231 dart:ui bindings the framework
// calls down through, and the 20 tonic::DartPersistentValue callbacks the
// engine calls up through (PlatformConfiguration::BeginFrame, DispatchPointer-
// DataPacket, UpdateWindowMetrics, ...). Those are split here:
//
//   * Downward calls (framework -> engine objects: Canvas, Paragraph, layers)
//     live in rust/ffi/rustflutter_ffi.h and are called from Rust.
//
//   * Upward calls (engine -> framework: begin frame, pointer events, metrics)
//     are the rf_app_* functions below. Rust implements them; RuntimeController
//     calls them on the UI thread.
//
//   * The framework's one output travels back through RfAppHost::render, which
//     hands over an RfLayerTree the shell converts and forwards to
//     RuntimeDelegate::Render.
//
// Threading: every rf_app_* function is called on the UI task runner and must
// not block. RfAppHost callbacks are invoked synchronously from inside them, on
// that same thread.

#include <stddef.h>
#include <stdint.h>

#include "flutter/rust/ffi/rustflutter_ffi.h"

#if defined(__cplusplus)
extern "C" {
#endif

// Opaque handle to the Rust-side application instance.
typedef struct RfApp RfApp;

// Mirrors flutter::ViewportMetrics. Physical pixels throughout, matching the
// engine; the framework divides by device_pixel_ratio to get logical pixels.
typedef struct RfViewMetrics {
  double device_pixel_ratio;
  double width;
  double height;
  double padding_top;
  double padding_right;
  double padding_bottom;
  double padding_left;
  double view_inset_top;
  double view_inset_right;
  double view_inset_bottom;
  double view_inset_left;
} RfViewMetrics;

// Callbacks into the shell. Populated by RuntimeController, stored by the Rust
// app for its lifetime.
typedef struct RfAppHost {
  void* user_data;

  // Hands a finished frame to the shell. Takes ownership of `tree`; the
  // framework must not touch it afterwards. This is the seam that upstream
  // reaches through PlatformConfiguration::Render.
  void (*render)(void* user_data,
                 int64_t view_id,
                 RfLayerTree* tree,
                 double device_pixel_ratio);

  // Requests another vsync. Equivalent to dart:ui's
  // PlatformDispatcher.scheduleFrame.
  void (*schedule_frame)(void* user_data);
} RfAppHost;

// -- Lifecycle ----------------------------------------------------------------

// Creates the application. `host` is copied. Returns NULL on failure.
RfApp* rf_app_create(const RfAppHost* host);
void rf_app_destroy(RfApp* app);

// Runs the app's entry point. Separate from create so the shell can wire up
// views and metrics first, the way LaunchRootIsolate ran after the isolate was
// prepared. Returns 0 on success.
int32_t rf_app_launch(RfApp* app);

// -- Views --------------------------------------------------------------------

void rf_app_add_view(RfApp* app,
                     int64_t view_id,
                     const RfViewMetrics* metrics);
void rf_app_remove_view(RfApp* app, int64_t view_id);
void rf_app_set_view_metrics(RfApp* app,
                             int64_t view_id,
                             const RfViewMetrics* metrics);

// -- Frames -------------------------------------------------------------------

// Animation phase: advance tickers and transitions. Called from Animator.
void rf_app_begin_frame(RfApp* app,
                        int64_t frame_time_micros,
                        uint64_t frame_number);

// Build / layout / paint phase, ending in one RfAppHost::render per dirty view.
void rf_app_draw_frame(RfApp* app);

// -- Input --------------------------------------------------------------------

// One pointer event, in physical pixels.
//
// Upstream flutter::PointerData carries thirty-odd fields, most of them for
// stylus tilt, trackpad pan and pressure ranges. This is the subset a framework
// needs before it has a use for the rest; the shell drops the others rather
// than making Rust parse a struct layout it would have to keep in sync.
typedef struct RfPointerEvent {
  int64_t view_id;
  int64_t device;
  int64_t pointer_id;
  // 0 cancel, 1 add, 2 remove, 3 hover, 4 down, 5 move, 6 up,
  // 7 pan-zoom start, 8 pan-zoom update, 9 pan-zoom end.
  int32_t change;
  // 0 touch, 1 mouse, 2 stylus, 3 inverted stylus, 4 trackpad.
  int32_t kind;
  // 0 none, 1 scroll, 2 scroll inertia cancel, 3 scale.
  int32_t signal_kind;
  int32_t buttons;
  int64_t time_stamp_micros;
  double physical_x;
  double physical_y;
  double delta_x;
  double delta_y;
  double scroll_delta_x;
  double scroll_delta_y;
  double pressure;
} RfPointerEvent;

// Delivers a batch of events, in the order the platform produced them.
void rf_app_dispatch_pointers(RfApp* app,
                              const RfPointerEvent* events,
                              size_t count);

#if defined(__cplusplus)
}  // extern "C"
#endif

#endif  // FLUTTER_RUNTIME_RUST_APP_API_H_
