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

  // Sends a platform message out of the framework, on its way to the embedder.
  // Equivalent to dart:ui's PlatformDispatcher.sendPlatformMessage.
  //
  // `response_id` is an id the *framework* allocated, or 0 if it wants no
  // reply. When it is non-zero, exactly one
  // rf_app_complete_platform_message_reply carrying it comes back, whether the
  // embedder answers or not -- a caller that is never answered waits forever.
  void (*send_platform_message)(void* user_data,
                                const char* channel,
                                const uint8_t* message,
                                size_t length,
                                int64_t response_id);

  // Answers a message the embedder sent in. `response_id` is the one that
  // arrived with it. `reply` is NULL when nothing handled the message, which is
  // a different fact from a reply of zero bytes: upstream it is the difference
  // between MissingPluginException and a null result.
  void (*respond_to_platform_message)(void* user_data,
                                      int64_t response_id,
                                      const uint8_t* reply,
                                      size_t length);

  // Tells the embedder a channel gained or lost its handler. Equivalent to
  // dart:ui's PlatformDispatcher.sendChannelUpdate; the Windows embedder uses
  // it to hold back messages nobody would hear.
  void (*send_channel_update)(void* user_data,
                              const char* channel,
                              bool listening);
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

// -- Platform state -----------------------------------------------------------
//
// What the platform says about itself. Not platform messages, even though it
// arrives as one: `Engine` consumes `flutter/settings` and `flutter/localization`
// on the way past and hands the contents here, exactly as upstream hands them
// to `PlatformConfiguration` rather than letting them reach a channel.

// The `flutter/settings` payload, verbatim: a JSON object with
// `textScaleFactor`, `alwaysUse24HourFormat` and `platformBrightness`. Passed
// as text rather than parsed here because the framework already has a JSON
// reader and the shell does not.
void rf_app_set_user_settings(RfApp* app, const char* json, size_t length);

// The preferred locales, most preferred first, as a flat array of four strings
// each: language code, country code, script code, variant code, in that order.
// `count` is the number of locales, so the array holds `count * 4` pointers.
// Any of the last three may be empty; the language code never is.
//
// Flat because that is the shape `flutter/localization` already carries and
// upstream's `_updateLocales` already unpacks.
void rf_app_set_locales(RfApp* app,
                        const char* const* locales,
                        size_t count);

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

// One key event. Mirrors flutter::KeyData, which is what the shell narrows to
// this, plus the character that KeyData deliberately leaves out because it is
// variable-length.
//
// Unlike pointers, keys do not arrive here as a call of their own. The embedder
// sends a KeyDataPacket as a platform message on `flutter/keydata` -- the same
// channel, the same bytes, on every Flutter platform -- and RuntimeController
// unpacks it. This struct is that unpacking, not a second transport.
//
// A key is identified twice because the two identities answer different
// questions. `physical` is where the finger went -- a USB HID usage code, the
// same on every layout -- and is what "is this key still down" is asked about,
// since the layout can change between the press and the release. `logical` is
// what the key meant under the layout in force, and is what a shortcut is
// written against.
typedef struct RfKeyEvent {
  int64_t time_stamp_micros;
  // 0 down, 1 up, 2 repeat. Matches flutter::KeyEventType.
  int32_t change;
  uint64_t physical;
  uint64_t logical;
  // True when the embedder made this event up rather than observing it, to
  // reconcile its record of what is held with what the platform reports. A
  // modifier released while another window had focus comes back this way.
  bool synthesized;
  // The text the key produced, UTF-8 and NUL-terminated, or NULL for a key that
  // produced none. Borrowed for the duration of the call.
  const char* character;
} RfKeyEvent;

// Delivers one key event. Returns true if the framework used it.
//
// The answer becomes the platform message's reply -- one byte, exactly as
// dart:ui's _keyDataListener writes it. No host here reads it: suppressing an
// unhandled key from the platform means re-posting it to the message queue
// afterwards, which is the bulk of upstream's KeyboardManager and is not built.
bool rf_app_dispatch_key(RfApp* app, const RfKeyEvent* event);

// -- Platform messages --------------------------------------------------------
//
// The engine's one extension point, and the only part of this API that is not
// shaped by what the framework needs but by what already exists: the bytes on a
// channel are the same bytes on every Flutter platform, so an existing plugin's
// Android and iOS halves work against this without knowing what is at the other
// end.
//
// Both directions are asynchronous and both may be answered, because neither
// end is obliged to be ready: an embedder asked for the clipboard has to talk
// to the operating system, and a framework asked to handle a message may have
// to wait for a frame. A reply is therefore an id that comes back later rather
// than a return value.

// Delivers a message from the embedder. `response_id` is 0 when the embedder
// wants no reply; otherwise the framework must call
// RfAppHost::respond_to_platform_message with it exactly once. Failing to do so
// leaks the shell's response handle.
void rf_app_dispatch_platform_message(RfApp* app,
                                      const char* channel,
                                      const uint8_t* message,
                                      size_t length,
                                      int64_t response_id);

// Answers a message the framework sent, carrying the id it allocated for it.
// `reply` is NULL when nothing on the far end handled the message.
void rf_app_complete_platform_message_reply(RfApp* app,
                                            int64_t response_id,
                                            const uint8_t* reply,
                                            size_t length);

#if defined(__cplusplus)
}  // extern "C"
#endif

#endif  // FLUTTER_RUNTIME_RUST_APP_API_H_
