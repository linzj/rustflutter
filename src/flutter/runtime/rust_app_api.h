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

// -- Semantics ----------------------------------------------------------------
//
// What the interface says, for a reader who is not looking at it. Upstream
// this crosses as a SemanticsUpdate built by SemanticsUpdateBuilder; the
// payload is the same set of facts, flattened into one struct because there is
// no builder on this side of the ABI to accumulate into.

// Bits of RfSemanticsNode::flags. A subset of flutter::SemanticsFlags: the
// ones that change what a screen reader says out loud, rather than how a
// particular platform arranges its own accessibility tree.
enum {
  kRfSemanticsIsButton = 1 << 0,
  kRfSemanticsIsTextField = 1 << 1,
  kRfSemanticsIsHeader = 1 << 2,
  kRfSemanticsIsImage = 1 << 3,
  kRfSemanticsIsLink = 1 << 4,
  kRfSemanticsIsSlider = 1 << 5,
  kRfSemanticsIsObscured = 1 << 6,
  kRfSemanticsIsReadOnly = 1 << 7,
  kRfSemanticsIsLiveRegion = 1 << 8,
  // "Has a checked state" is separate from "is checked" because it is what
  // makes *off* sayable: a node without it is a label, and a reader is told
  // nothing about which way the switch is.
  kRfSemanticsHasCheckedState = 1 << 9,
  kRfSemanticsIsChecked = 1 << 10,
  kRfSemanticsHasEnabledState = 1 << 11,
  kRfSemanticsIsEnabled = 1 << 12,
  kRfSemanticsIsSelected = 1 << 13,
  kRfSemanticsIsFocused = 1 << 14,
  // The fourth check state, which two bits could not carry. A node with the
  // checked state *and* this one is partly checked -- the "select all" box
  // above a list where some rows are chosen -- and without it that box
  // crossed as plain unchecked, which is one of the two things it is not.
  //
  // It is read only when kRfSemanticsHasCheckedState is set, so an old
  // sender that never raises it is unchanged.
  kRfSemanticsIsCheckStateMixed = 1 << 15,
  // A switch, not a checkbox: a reader says "on" and "off" for these and
  // "checked" and "not checked" for the pair above. Three more tristates,
  // each a "has it" bit gating an "is it" one, for the same reason the
  // checked pair is two bits -- "no opinion" is a third thing.
  kRfSemanticsHasToggledState = 1 << 16,
  kRfSemanticsIsToggled = 1 << 17,
  kRfSemanticsHasExpandedState = 1 << 18,
  kRfSemanticsIsExpanded = 1 << 19,
  kRfSemanticsHasRequiredState = 1 << 20,
  kRfSemanticsIsRequired = 1 << 21,
};

typedef struct RfSemanticsNode {
  // Stable while the widget that produced it keeps the same identifier. The
  // platform keys its own accessibility nodes on this; an id that changed is,
  // to a screen reader, a new thing where the old one was.
  int32_t id;
  int32_t flags;
  // A bit set of flutter::SemanticsAction values.
  int32_t actions;
  // In root logical coordinates: where on the glass this is.
  float left;
  float top;
  float right;
  float bottom;
  // NUL-terminated UTF-8, never NULL; empty means "nothing to say".
  const char* label;
  const char* value;
  const char* hint;
  const char* increased_value;
  const char* decreased_value;
  // NaN for a node that does not scroll, which is what upstream uses for the
  // same "no answer".
  double scroll_position;
  double scroll_extent_min;
  double scroll_extent_max;
  const int32_t* children;
  size_t child_count;
  // The reading direction of the label and its kin, in the embedder's
  // FlutterTextDirection encoding: 0 = unknown, 1 = rtl, 2 = ltr. A node
  // with nothing to read aloud crosses as 0.
  int32_t text_direction;
} RfSemanticsNode;

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

  // Hands over the semantics tree for a view, in the order a reader should
  // meet it: a node's children come after it, and `children` names them by id.
  // Equivalent to dart:ui's PlatformDispatcher.updateSemantics.
  //
  // Only ever called between rf_app_set_semantics_enabled(true) and the next
  // (false), because a tree nobody is reading is not built. The array and
  // every string in it belong to the framework and are valid only for the
  // duration of the call.
  void (*update_semantics)(void* user_data,
                           int64_t view_id,
                           const RfSemanticsNode* nodes,
                           size_t count);

  // Asks for rf_app_run_tasks to be called on the UI thread, soon.
  //
  // This is the one callback that may be invoked from ANY thread, and the
  // reason it exists rather than reusing schedule_frame: a Waker handed to a
  // decode worker has to be able to say "come back and poll me", and
  // schedule_frame cannot be called from off the UI thread -- it reaches
  // Animator::RequestFrame, which touches UI-thread-only state.
  // fml::TaskRunner::PostTask is thread-safe, which is exactly what this needs.
  //
  // The framework coalesces: between one post_task and the rf_app_run_tasks
  // that answers it, further wakes queue rather than post again. An embedder
  // that drops the request instead of posting it stalls every waiting task, so
  // this is not optional once it is non-NULL.
  //
  // NULL is allowed and means "no task runner here": the framework falls back
  // to draining tasks at the top of the next frame, which is correct but
  // quantised to the vsync and cannot serve cross-thread wakes.
  void (*post_task)(void* user_data);

  // The same, but not before `delay_micros` have passed. Also callable from
  // any thread.
  //
  // This is the framework's only clock other than the frame's. Everything the
  // port turned into a frame-clock deadline -- a long press, a tooltip, a
  // snackbar -- stays on the frame clock, where it belongs: a tooltip that
  // expires between two frames cannot be drawn until the next one anyway. What
  // needs this is application code that wants to wait without drawing.
  //
  // The embedder may fire late and may coalesce; the framework re-checks its
  // own deadlines when it drains, so an early call is harmless and a late one
  // costs only lateness. NULL means no clock, and a task waiting on one then
  // advances no sooner than the next frame that happens for another reason.
  void (*post_delayed_task)(void* user_data, int64_t delay_micros);
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

// -- Tasks --------------------------------------------------------------------

// Polls every task the framework has ready, until none is.
//
// Upstream's counterpart is FlushMicrotasksNow, and this is called from the
// two places upstream calls that one: between the animation phase and the
// build phase of a frame, and in answer to a RfAppHost::post_task request.
//
// The position inside a frame is load-bearing, not incidental: a task that
// completes during the animation phase must be visible to the build that
// follows it, the same way an animation started in onBeginFrame must be.
// dart:ui's own scheduleWarmUpFrame uses two timers rather than one "to ensure
// that microtasks flush in between".
//
// Must be called on the UI thread. Cheap when nothing is ready.
void rf_app_run_tasks(RfApp* app);

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

// -- Accessibility ------------------------------------------------------------

// Turns the semantics tree on or off. Nothing is built while it is off, which
// is upstream's arrangement too (PlatformView::SetSemanticsEnabled): the shell
// says so when an assistive technology arrives, and says so again when it
// leaves.
void rf_app_set_semantics_enabled(RfApp* app, bool enabled);

// Delivers an action a screen reader asked for. `action` is one
// flutter::SemanticsAction bit. Returns whether anything took it -- false for
// a node that has gone, which is a race with the reader rather than an error.
bool rf_app_dispatch_semantics_action(RfApp* app, int32_t node_id, int32_t action);

#if defined(__cplusplus)
}  // extern "C"
#endif

#endif  // FLUTTER_RUNTIME_RUST_APP_API_H_
