// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// The macOS host: a Cocoa window, the engine's own thread model, and a real
// Shell driving the Rust framework.
//
// Structure, and why:
//
//   * The window lives on the process's main thread and owns the run loop,
//     because AppKit refuses to be driven from anywhere else -- every NSWindow,
//     NSView and NSEvent call below is a main-thread call.
//
//   * The shell's platform / UI / raster / IO threads come from ThreadHost, so
//     they are the same fml threads the engine uses everywhere else. The window
//     thread is deliberately *not* the platform thread, for the reason the
//     Windows host gives: making it so would mean interleaving fml::MessageLoop
//     with the AppKit run loop, and the two want to own the same thread.
//
//   * Everything the window learns (size, close, input) is posted to the
//     platform task runner; everything the raster thread produces is posted
//     back with dispatch_async to the main queue. Neither side touches the
//     other's state directly.
//
// Rendering is `GPUSurfaceSoftware` -- Skia rasterises into a bitmap and the
// view blits it. That is the surface the shell brings with it and it needs
// nothing from the platform, which is what makes this file a window and an
// input pump rather than a graphics port. Impeller on macOS is Metal, and
// Metal is a different `PlatformView` hook (`CreateRenderingSurface` returning
// `GPUSurfaceMetalImpeller` over a `CAMetalLayer`) rather than more of this
// one; the seam it would attach to is
// `HostPlatformView::CreateRenderingSurface` below, exactly where the Windows
// host attaches ANGLE.
//
// What this host does not do yet, stated rather than implied: no IME (a
// composing input method gets the committed text and not the marked text), no
// accessibility tree (the Windows host serves UI Automation; the macOS
// counterpart is NSAccessibility and is its own file), and no Impeller.

#include "flutter/rust/host/rustflutter_host.h"

#import <Cocoa/Cocoa.h>
#import <ImageIO/ImageIO.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>

#include <CoreGraphics/CoreGraphics.h>
#include <CoreVideo/CoreVideo.h>

#include <atomic>
#include <cstdlib>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <vector>

#include "flutter/common/constants.h"
#include "flutter/common/settings.h"
#include "flutter/common/task_runners.h"
#include "flutter/fml/logging.h"
#include "flutter/fml/make_copyable.h"
#include "flutter/fml/message_loop.h"
#include "flutter/fml/paths.h"
#include "flutter/fml/synchronization/waitable_event.h"
#include "flutter/fml/task_runner.h"
#include "flutter/lib/ui/window/key_data.h"
#include "flutter/lib/ui/window/key_data_packet.h"
#include "flutter/lib/ui/window/platform_message.h"
#include "flutter/lib/ui/window/pointer_data.h"
#include "flutter/lib/ui/window/pointer_data_packet.h"
#include "flutter/lib/ui/window/viewport_metrics.h"
#include "flutter/rust/ffi/rustflutter_ffi.h"
#include "flutter/rust/host/rustflutter_key_map_mac.h"
#include "flutter/shell/common/display.h"
#include "flutter/shell/common/platform_view.h"
#include "flutter/shell/common/rasterizer.h"
#include "flutter/shell/common/run_configuration.h"
#include "flutter/shell/common/shell.h"
#include "flutter/shell/common/thread_host.h"
#include "flutter/shell/common/vsync_waiter.h"
#include "flutter/shell/gpu/gpu_surface_software.h"
#include "flutter/shell/gpu/gpu_surface_software_delegate.h"
#include "flutter/shell/platform/common/client_wrapper/include/flutter/standard_method_codec.h"
#include "flutter/shell/platform/common/text_input_model.h"
#include "rapidjson/document.h"
#include "rapidjson/stringbuffer.h"
#include "rapidjson/writer.h"
#include "third_party/skia/include/core/SkSurface.h"

@class RfContentView;

/// Tells the view's input context to drop whatever the IME was composing, on
/// the main thread, where input contexts live. Defined below the view's
/// interface; the platform view that calls it is defined above it.
static void RfDiscardMarkedText(RfContentView* view);

namespace flutter {
namespace {

/// Where key events go. Matched by RuntimeController, which is the only reader.
/// Upstream this same string is in embedder.cc, platform_dispatcher.dart,
/// KeyData.java and FlutterEngine.mm -- an embedder is expected to spell it
/// out.
constexpr char kKeyDataChannel[] = "flutter/keydata";
constexpr char kPlatformChannel[] = "flutter/platform";
constexpr char kTextInputChannel[] = "flutter/textinput";
constexpr char kLifecycleChannel[] = "flutter/lifecycle";
constexpr char kSettingsChannel[] = "flutter/settings";
constexpr char kLocalizationChannel[] = "flutter/localization";
constexpr char kMouseCursorChannel[] = "flutter/mousecursor";

constexpr char kClipboardError[] = "Clipboard error";
constexpr char kUnknownClipboardFormatMessage[] = "Unknown clipboard format";
constexpr char kTextPlainFormat[] = "text/plain";
constexpr char kExitRequestError[] = "ExitApplication error";
constexpr char kInvalidExitRequestMessage[] = "Invalid application exit request";
constexpr char kExitTypeCancelable[] = "cancelable";

//------------------------------------------------------------------------------
/// The last frame the rasterizer produced, waiting for the window to draw it.
///
/// Two threads meet here and nowhere else: the raster thread stores, the main
/// thread paints. Both under one lock, because a frame half-replaced while it
/// is being drawn is a torn frame.
class FrameBuffer {
 public:
  void Store(const void* pixels, int32_t width, int32_t height, bool blue_first) {
    if (pixels == nullptr || width <= 0 || height <= 0) {
      return;
    }
    const size_t bytes = static_cast<size_t>(width) * height * 4;
    std::lock_guard<std::mutex> lock(mutex_);
    pixels_.resize(bytes);
    std::memcpy(pixels_.data(), pixels, bytes);
    width_ = width;
    height_ = height;
    blue_first_ = blue_first;
  }

  /// Draws the stored frame into `context`, scaled to `bounds`.
  ///
  /// `context` is expected to be y-down -- top-left origin, the way a flipped
  /// NSView and the engine both count. CoreGraphics is y-up underneath that, so
  /// the transform below is what turns one into the other; without it the frame
  /// comes out upside down, which is a mistake that still produces a picture.
  ///
  /// Returns false when there is nothing to draw, which is every moment before
  /// the first frame arrives.
  bool Paint(CGContextRef context, CGRect bounds) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (pixels_.empty() || width_ <= 0 || height_ <= 0) {
      return false;
    }
    // Which way round the channels are is not assumed: the surface says, and
    // `PresentBackingStore` passes the answer along. Getting it wrong does not
    // fail -- it swaps red and blue, which is the kind of bug that survives
    // review because the picture is still a picture.
    //
    // BGRA is `premultipliedFirst` with little-endian words; RGBA is
    // `premultipliedLast` with big-endian ones, which is CoreGraphics' way of
    // spelling "in memory order".
    const uint32_t channels = blue_first_ ? static_cast<uint32_t>(kCGImageAlphaPremultipliedFirst) |
                                                static_cast<uint32_t>(kCGBitmapByteOrder32Little)
                                          : static_cast<uint32_t>(kCGImageAlphaPremultipliedLast) |
                                                static_cast<uint32_t>(kCGBitmapByteOrder32Big);
    CGColorSpaceRef space = CGColorSpaceCreateDeviceRGB();
    CGContextRef bitmap = CGBitmapContextCreate(pixels_.data(), width_, height_, 8,
                                                static_cast<size_t>(width_) * 4, space, channels);
    bool painted = false;
    if (bitmap != nullptr) {
      CGImageRef image = CGBitmapContextCreateImage(bitmap);
      if (image != nullptr) {
        CGContextSaveGState(context);
        CGContextTranslateCTM(context, 0, CGRectGetMaxY(bounds) + CGRectGetMinY(bounds));
        CGContextScaleCTM(context, 1, -1);
        // The frame is in physical pixels and the bounds are in points; the
        // scale between them is the backing scale factor, and letting
        // CoreGraphics do the division is both correct and free.
        CGContextDrawImage(context, bounds, image);
        CGContextRestoreGState(context);
        CGImageRelease(image);
        painted = true;
      }
      CGContextRelease(bitmap);
    }
    CGColorSpaceRelease(space);
    return painted;
  }

  /// Draws one frame the way the view draws it -- through the same `Paint`, in
  /// a context flipped the same way -- and writes the result to `path`.
  ///
  /// A window is the only way to look at this host's output, and looking at a
  /// window needs a screen recorder's permission, which a build machine does
  /// not have. This is how the blit gets checked instead: channel order and
  /// orientation are both visible in the file, and both are the kind of mistake
  /// that still produces a picture.
  ///
  /// Enabled by RUSTFLUTTER_DUMP_FRAME=<path>, and it writes the first frame
  /// only.
  bool WritePng(const char* path) {
    int32_t width = 0;
    int32_t height = 0;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      width = width_;
      height = height_;
    }
    if (width <= 0 || height <= 0) {
      return false;
    }

    CGColorSpaceRef space = CGColorSpaceCreateDeviceRGB();
    CGContextRef canvas =
        CGBitmapContextCreate(nullptr, width, height, 8, 0, space,
                              static_cast<uint32_t>(kCGImageAlphaPremultipliedLast) |
                                  static_cast<uint32_t>(kCGBitmapByteOrder32Big));
    CGColorSpaceRelease(space);
    if (canvas == nullptr) {
      return false;
    }

    // A bitmap context is y-up and a view is y-down, so this is where the two
    // are reconciled: flip the context and `Paint` then sees exactly what it
    // sees on screen. Without it the file and the window would disagree, and
    // the file is the only one anybody can check.
    CGContextSaveGState(canvas);
    CGContextTranslateCTM(canvas, 0, height);
    CGContextScaleCTM(canvas, 1, -1);
    const bool painted = Paint(canvas, CGRectMake(0, 0, width, height));
    CGContextRestoreGState(canvas);

    bool written = false;
    if (painted) {
      CGImageRef image = CGBitmapContextCreateImage(canvas);
      if (image != nullptr) {
        NSURL* url = [NSURL fileURLWithPath:@(path)];
        CGImageDestinationRef destination = CGImageDestinationCreateWithURL(
            (__bridge CFURLRef)url, (__bridge CFStringRef)UTTypePNG.identifier, 1, nullptr);
        if (destination != nullptr) {
          CGImageDestinationAddImage(destination, image, nullptr);
          written = CGImageDestinationFinalize(destination);
          CFRelease(destination);
        }
        CGImageRelease(image);
      }
    }
    CGContextRelease(canvas);
    return written;
  }

 private:
  std::mutex mutex_;
  std::vector<uint8_t> pixels_;
  int32_t width_ = 0;
  int32_t height_ = 0;
  /// Whether the stored pixels are BGRA rather than RGBA.
  bool blue_first_ = true;
};

//------------------------------------------------------------------------------
/// How often the display this window is on refreshes.
///
/// `CGDisplayModeGetRefreshRate` answers zero for the built-in displays of most
/// Macs -- they are driven by the compositor rather than by a fixed mode -- so
/// a zero is not a failure and the fallback is not an error path. Sixty is the
/// right guess for those panels; a ProMotion display reports its own 120.
double DisplayRefreshRate() {
  double hz = 0.0;
  CGDirectDisplayID display = CGMainDisplayID();
  CGDisplayModeRef mode = CGDisplayCopyDisplayMode(display);
  if (mode != nullptr) {
    hz = CGDisplayModeGetRefreshRate(mode);
    CGDisplayModeRelease(mode);
  }
  if (hz <= 1.0) {
    // AppKit knows what the compositor will actually deliver, which is the
    // number that matters and the one CGDisplayMode does not have.
    if (NSScreen* screen = [NSScreen mainScreen]) {
      if (@available(macOS 12.0, *)) {
        hz = static_cast<double>([screen maximumFramesPerSecond]);
      }
    }
  }
  return hz > 1.0 ? hz : 60.0;
}

//------------------------------------------------------------------------------
/// A vsync waiter paced by the display rather than by a fixed sixty hertz.
///
/// The algorithm is `VsyncWaiterFallback`'s, and the Windows host's: a phase
/// fixed at construction, each frame snapped forward onto that grid, and the
/// callback posted for that time. What changes is only where the interval comes
/// from.
///
/// Not a `CVDisplayLink`. A display link would be the platform's own answer,
/// and it calls back on a thread of its own that must not block -- so the
/// callback would have to hop to the UI task runner anyway, and what it bought
/// over a snapped timer would be phase accuracy this host has no way to use:
/// the software surface presents through `drawRect:`, which the window server
/// schedules on its own terms.
class VsyncWaiterMac final : public VsyncWaiter {
 public:
  explicit VsyncWaiterMac(const TaskRunners& task_runners)
      : VsyncWaiter(task_runners), phase_(fml::TimePoint::Now()) {}

  ~VsyncWaiterMac() override = default;

 private:
  /// Rounds `value` up onto the grid that passes through `phase` every
  /// `interval`.
  static fml::TimePoint SnapToNextTick(fml::TimePoint value,
                                       fml::TimePoint phase,
                                       fml::TimeDelta interval) {
    fml::TimeDelta offset = (phase - value) % interval;
    if (offset != fml::TimeDelta::Zero()) {
      offset = offset + interval;
    }
    return value + offset;
  }

  /// The frame interval, re-read about once a second -- a display's rate
  /// changes when a laptop moves to battery or a monitor is swapped, and
  /// reading it every frame is a syscall for an answer that almost never
  /// changes.
  fml::TimeDelta FrameInterval() {
    const fml::TimePoint now = fml::TimePoint::Now();
    if (interval_ != fml::TimeDelta::Zero() &&
        now - interval_read_at_ <= fml::TimeDelta::FromSeconds(1)) {
      return interval_;
    }

    // A rate this machine does not have, for testing the pacing itself.
    double hz = 0.0;
    if (const char* forced = std::getenv("RUSTFLUTTER_FORCE_HZ")) {
      hz = std::atof(forced);
    }
    if (hz <= 0.0) {
      hz = DisplayRefreshRate();
    }

    static double reported = 0.0;
    if (hz != reported) {
      reported = hz;
      FML_LOG(IMPORTANT) << "Pacing frames at " << hz << " Hz.";
    }
    interval_ = fml::TimeDelta::FromSecondsF(1.0 / hz);
    interval_read_at_ = now;
    return interval_;
  }

  // |VsyncWaiter|
  void AwaitVSync() override {
    const fml::TimeDelta interval = FrameInterval();
    const fml::TimePoint frame_start_time = SnapToNextTick(fml::TimePoint::Now(), phase_, interval);
    const fml::TimePoint frame_target_time = frame_start_time + interval;

    std::weak_ptr<VsyncWaiterMac> weak_this =
        std::static_pointer_cast<VsyncWaiterMac>(shared_from_this());
    task_runners_.GetUITaskRunner()->PostTaskForTime(
        [frame_start_time, frame_target_time, weak_this]() {
          if (auto waiter = weak_this.lock()) {
            waiter->FireCallback(frame_start_time, frame_target_time, true);
          }
        },
        frame_start_time);
  }

  const fml::TimePoint phase_;
  fml::TimeDelta interval_ = fml::TimeDelta::Zero();
  fml::TimePoint interval_read_at_;

  FML_DISALLOW_COPY_AND_ASSIGN(VsyncWaiterMac);
};

//------------------------------------------------------------------------------
// Platform channels.
//
// `flutter/platform` speaks JSON -- `{"method": ..., "args": ...}` in, a
// one-element array out on success and a three-element one on failure. Not a
// choice: the channel predates the binary codec and its Android, iOS, Linux and
// macOS halves are all written against JSON.

template <typename Body>
std::string SuccessEnvelope(Body&& body) {
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartArray();
  body(writer);
  writer.EndArray();
  return buffer.GetString();
}

std::string NullEnvelope() {
  return SuccessEnvelope([](auto& writer) { writer.Null(); });
}

std::string ErrorEnvelope(const char* code, const std::string& message) {
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartArray();
  writer.String(code);
  writer.String(message.c_str(), static_cast<rapidjson::SizeType>(message.size()));
  writer.Null();
  writer.EndArray();
  return buffer.GetString();
}

/// What a `System.exitApplication` asks the window to do. The window is on
/// another thread, so the request is a callback rather than a call.
class ExitRequester {
 public:
  virtual ~ExitRequester() = default;
  /// Asks the framework whether it minds, and closes if it does not.
  virtual void RequestAppExit(bool cancelable, int exit_code) = 0;
  /// Closes without asking.
  virtual void QuitApplication(int exit_code) = 0;
};

std::string ClipboardText() {
  NSPasteboard* pasteboard = [NSPasteboard generalPasteboard];
  NSString* text = [pasteboard stringForType:NSPasteboardTypeString];
  return text == nil ? std::string() : std::string([text UTF8String]);
}

bool ClipboardHasText() {
  NSPasteboard* pasteboard = [NSPasteboard generalPasteboard];
  return [pasteboard canReadObjectForClasses:@[ [NSString class] ] options:@{}] == YES;
}

void SetClipboardText(const std::string& text) {
  NSPasteboard* pasteboard = [NSPasteboard generalPasteboard];
  [pasteboard clearContents];
  [pasteboard setString:@(text.c_str()) forType:NSPasteboardTypeString];
}

//------------------------------------------------------------------------------
// Text input.
//
// `flutter/textinput` is the channel a text field talks to the platform on.
// The framework opens an editing session (`TextInput.setClient`) and from then
// on the *platform* owns the editing: every key the reader types is applied to
// a model here and reported back as `TextInputClient.updateEditingState`.
// Without this half, a focused field waits forever -- which is exactly what
// typing on this host did before it existed.
//
// The editing model is the engine's own `flutter::TextInputModel`, the same
// class the Windows host edits (`rustflutter_host_win.cc`, whose handler this
// is a trimmed copy of). What is trimmed is the IME: upstream's macOS plugin
// (`FlutterTextInputPlugin.mm`) is an `NSTextInputClient` with marked text and
// candidate windows; this host takes the committed character off the key event
// and no more, as its header states.
//
// Channel calls arrive on the platform thread and keys on the main thread, so
// the model sits behind a mutex, as it does on Windows.

/// The framework's text field, as the platform sees it.
class TextInputHandler {
 public:
  /// How a state update leaves here. Set once, by the platform view.
  using Sender = std::function<void(const std::string& method, const std::string& arguments_json)>;

  void SetSender(Sender sender) { sender_ = std::move(sender); }

  /// True once the framework has attached a field. Everything typed while
  /// this is false goes nowhere, which is correct: there is nothing to type
  /// into.
  bool attached() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return model_ != nullptr;
  }

  /// Handles one call on `flutter/textinput`. Platform thread.
  std::optional<std::string> HandleMethodCall(const std::string& method,
                                              const rapidjson::Value* args) {
    if (method == "TextInput.show" || method == "TextInput.hide") {
      // No-ops, as upstream: there is no on-screen keyboard to raise.
      return NullEnvelope();
    }

    if (method == "TextInput.setClient") {
      // `[clientId, config]`. The config carries the action and the type.
      if (args == nullptr || !args->IsArray() || args->Size() < 2) {
        return ErrorEnvelope("TextInput.badArgument",
                             "setClient needs a client id and a configuration");
      }
      const rapidjson::Value& client = (*args)[0];
      const rapidjson::Value& config = (*args)[1];
      if (!client.IsInt()) {
        return ErrorEnvelope("TextInput.badArgument", "the client id is not a number");
      }
      std::lock_guard<std::mutex> lock(mutex_);
      client_id_ = client.GetInt();
      input_action_.clear();
      input_type_.clear();
      if (config.IsObject()) {
        auto action = config.FindMember("inputAction");
        if (action != config.MemberEnd() && action->value.IsString()) {
          input_action_ = action->value.GetString();
        }
        auto type = config.FindMember("inputType");
        if (type != config.MemberEnd() && type->value.IsObject()) {
          auto name = type->value.FindMember("name");
          if (name != type->value.MemberEnd() && name->value.IsString()) {
            input_type_ = name->value.GetString();
          }
        }
      }
      model_ = std::make_unique<TextInputModel>();
      return NullEnvelope();
    }

    if (method == "TextInput.clearClient") {
      std::lock_guard<std::mutex> lock(mutex_);
      model_.reset();
      return NullEnvelope();
    }

    if (method == "TextInput.setEditingState") {
      if (args == nullptr || !args->IsObject()) {
        return ErrorEnvelope("TextInput.badArgument", "setEditingState needs a state");
      }
      auto text = args->FindMember("text");
      if (text == args->MemberEnd() || !text->value.IsString()) {
        return ErrorEnvelope("TextInput.badArgument", "the state has no text");
      }
      auto number = [args](const char* key, int fallback) {
        auto found = args->FindMember(key);
        return found != args->MemberEnd() && found->value.IsInt() ? found->value.GetInt()
                                                                  : fallback;
      };
      const int base = number("selectionBase", -1);
      const int extent = number("selectionExtent", -1);

      std::lock_guard<std::mutex> lock(mutex_);
      if (model_ == nullptr) {
        return ErrorEnvelope("TextInput.noClient",
                             "the editing state was set with no client attached");
      }
      // The framework is the authority here: this is it telling the platform
      // what the field now holds, which is how a programmatic edit -- a
      // paste, a tap moving the caret -- reaches the model.
      model_->SetText(text->value.GetString(),
                      TextRange(static_cast<size_t>(base < 0 ? 0 : base),
                                static_cast<size_t>(extent < 0 ? 0 : extent)));
      return NullEnvelope();
    }

    if (method == "TextInput.setMarkedTextRect") {
      // Where the caret is, in the editable's own coordinates. This plus the
      // transform is where an IME's candidate window goes.
      if (args == nullptr || !args->IsObject()) {
        return ErrorEnvelope("TextInput.badArgument", "Method invoked without args");
      }
      auto number = [args](const char* key, bool* found_it) {
        auto found = args->FindMember(key);
        *found_it = found != args->MemberEnd() && found->value.IsNumber();
        return *found_it ? found->value.GetDouble() : 0.0;
      };
      bool ok[4] = {};
      const double x = number("x", &ok[0]);
      const double y = number("y", &ok[1]);
      const double width = number("width", &ok[2]);
      const double height = number("height", &ok[3]);
      if (!ok[0] || !ok[1] || !ok[2] || !ok[3]) {
        return ErrorEnvelope("TextInput.badArgument", "Composing rect values invalid.");
      }
      std::lock_guard<std::mutex> lock(mutex_);
      marked_x_ = x;
      marked_y_ = y;
      marked_width_ = width;
      marked_height_ = height;
      caret_valid_ = true;
      return NullEnvelope();
    }

    if (method == "TextInput.setEditableSizeAndTransform") {
      // A 4x4 matrix, row-major; only its translation is used, which is
      // entries 12 and 13 -- a candidate window cannot be rotated.
      if (args == nullptr || !args->IsObject()) {
        return ErrorEnvelope("TextInput.badArgument", "Method invoked without args");
      }
      auto transform = args->FindMember("transform");
      if (transform == args->MemberEnd() || !transform->value.IsArray() ||
          transform->value.Size() != 16) {
        return ErrorEnvelope("TextInput.badArgument", "EditableText transform invalid.");
      }
      const rapidjson::Value& matrix = transform->value;
      if (!matrix[12].IsNumber() || !matrix[13].IsNumber()) {
        return ErrorEnvelope("TextInput.badArgument",
                             "EditableText transform contains null value.");
      }
      std::lock_guard<std::mutex> lock(mutex_);
      transform_x_ = matrix[12].GetDouble();
      transform_y_ = matrix[13].GetDouble();
      caret_valid_ = true;
      return NullEnvelope();
    }

    if (method == "TextInput.setCaretRect") {
      return NullEnvelope();
    }

    return std::nullopt;
  }

  // -- The IME's half, through `NSTextInputClient` -----------------------------

  /// Committed text -- `insertText:replacementRange:`. During a composition
  /// this is the IME cashing in the marked text; outside one it is a plain
  /// keystroke arriving through `interpretKeyEvents:`.
  void OnInsertText(const std::u16string& text) {
    if (Edit([&text](TextInputModel& model) {
          if (model.composing()) {
            model.UpdateComposingText(text);
            model.CommitComposing();
            model.EndComposing();
          } else {
            model.AddText(text);
          }
          return true;
        })) {
      SendStateUpdate();
    }
  }

  /// The composition as it stands -- `setMarkedText:selectedRange:...`.
  /// `cursor` is where the IME's own caret sits inside the marked text.
  void OnSetMarkedText(const std::u16string& text, long cursor, long length) {
    if (Edit([&](TextInputModel& model) {
          if (!model.composing()) {
            model.BeginComposing();
          }
          const size_t base = cursor < 0 ? 0 : static_cast<size_t>(cursor);
          const size_t extent = base + (length < 0 ? 0 : static_cast<size_t>(length));
          model.UpdateComposingText(text, TextRange(base, extent));
          return true;
        })) {
      SendStateUpdate();
    }
  }

  /// `unmarkText`: the composition is taken as it stands.
  void OnUnmarkText() {
    if (Edit([](TextInputModel& model) {
          if (!model.composing()) {
            return false;
          }
          model.CommitComposing();
          model.EndComposing();
          return true;
        })) {
      SendStateUpdate();
    }
  }

  bool Composing() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return model_ != nullptr && model_->composing();
  }

  /// The marked range in the text, UTF-16 units; `location` is -1 when
  /// nothing is being composed. `NSRange`'s own units, which is the point.
  void GetMarkedRange(long* location, long* length) const {
    std::lock_guard<std::mutex> lock(mutex_);
    if (model_ == nullptr || !model_->composing()) {
      *location = -1;
      *length = 0;
      return;
    }
    const TextRange range = model_->composing_range();
    *location = static_cast<long>(range.start());
    *length = static_cast<long>(range.length());
  }

  void GetSelectedRange(long* location, long* length) const {
    std::lock_guard<std::mutex> lock(mutex_);
    if (model_ == nullptr) {
      *location = -1;
      *length = 0;
      return;
    }
    const TextRange range = model_->selection();
    *location = static_cast<long>(range.start());
    *length = static_cast<long>(range.length());
  }

  /// Where the caret is in the view, logical pixels: the marked rectangle's
  /// origin put through the editable's transform, both reported by the
  /// framework at paint. What `firstRectForCharacterRange:` answers with.
  bool GetCaretRect(double* x, double* y, double* width, double* height) const {
    std::lock_guard<std::mutex> lock(mutex_);
    if (!caret_valid_) {
      return false;
    }
    *x = marked_x_ + transform_x_;
    *y = marked_y_ + transform_y_;
    *width = marked_width_;
    *height = marked_height_;
    return true;
  }

  /// An editing key: backspace, forward delete, the arrows, home and end.
  /// `key_code` is the AppKit virtual key code.
  ///
  /// Returns true if the field used it.
  bool OnEditingKey(unsigned short key_code, bool shift) {
    bool changed = false;
    const bool handled = Edit([&](TextInputModel& model) {
      switch (key_code) {
        case 0x33:  // Delete (backspace).
          changed = model.Backspace();
          return true;
        case 0x75:  // Forward delete.
          changed = model.Delete();
          return true;
        case 0x7B:  // Left arrow.
          changed = model.MoveCursorBack();
          return true;
        case 0x7C:  // Right arrow.
          changed = model.MoveCursorForward();
          return true;
        case 0x73:  // Home.
          changed = shift ? model.SelectToBeginning() : model.MoveCursorToBeginning();
          return true;
        case 0x77:  // End.
          changed = shift ? model.SelectToEnd() : model.MoveCursorToEnd();
          return true;
        default:
          return false;
      }
    });
    if (changed) {
      SendStateUpdate();
    }
    return handled;
  }

  /// Return, which submits rather than edits -- except in a multiline field
  /// whose action is newline, which gets both, upstream's `EnterPressed`.
  void OnAction() {
    int client_id = 0;
    std::string action;
    bool newline = false;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (model_ == nullptr) {
        return;
      }
      client_id = client_id_;
      action = input_action_.empty() ? "TextInputAction.done" : input_action_;
      newline = input_type_ == "TextInputType.multiline" && action == "TextInputAction.newline";
      if (newline) {
        model_->AddText(std::u16string(u"\n"));
      }
    }
    if (newline) {
      SendStateUpdate();
    }

    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.StartArray();
    writer.Int(client_id);
    writer.String(action.c_str());
    writer.EndArray();
    if (sender_) {
      sender_("TextInputClient.performAction", std::string(buffer.GetString(), buffer.GetSize()));
    }
  }

 private:
  /// Runs `edit` against the model, if there is one. Returns what it
  /// returned, or false when no client is attached.
  bool Edit(const std::function<bool(TextInputModel&)>& edit) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (model_ == nullptr) {
      return false;
    }
    return edit(*model_);
  }

  void SendStateUpdate() {
    int client_id = 0;
    std::string text;
    int selection_base = 0;
    int selection_extent = 0;
    int composing_base = -1;
    int composing_extent = -1;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (model_ == nullptr) {
        return;
      }
      client_id = client_id_;
      text = model_->GetText();
      selection_base = static_cast<int>(model_->selection().base());
      selection_extent = static_cast<int>(model_->selection().extent());
      if (model_->composing()) {
        composing_base = static_cast<int>(model_->composing_range().base());
        composing_extent = static_cast<int>(model_->composing_range().extent());
      }
    }

    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.StartArray();
    writer.Int(client_id);
    writer.StartObject();
    // The keys, and their order, are upstream's. A field the framework does
    // not find is a field it substitutes a default for, silently.
    writer.Key("selectionAffinity");
    writer.String("TextAffinity.downstream");
    writer.Key("selectionBase");
    writer.Int(selection_base);
    writer.Key("selectionExtent");
    writer.Int(selection_extent);
    writer.Key("selectionIsDirectional");
    writer.Bool(false);
    writer.Key("composingBase");
    writer.Int(composing_base);
    writer.Key("composingExtent");
    writer.Int(composing_extent);
    writer.Key("text");
    writer.String(text.c_str(), static_cast<rapidjson::SizeType>(text.size()));
    writer.EndObject();
    writer.EndArray();

    if (sender_) {
      sender_("TextInputClient.updateEditingState",
              std::string(buffer.GetString(), buffer.GetSize()));
    }
  }

  mutable std::mutex mutex_;
  std::unique_ptr<TextInputModel> model_;
  int client_id_ = 0;
  std::string input_action_;
  std::string input_type_;
  double marked_x_ = 0;
  double marked_y_ = 0;
  double marked_width_ = 0;
  double marked_height_ = 0;
  double transform_x_ = 0;
  double transform_y_ = 0;
  bool caret_valid_ = false;
  Sender sender_;
};

/// Handles one call on `flutter/platform`.
///
/// Returns the reply, or nothing for a method this host does not implement --
/// which is answered with an empty message rather than an error, because that
/// is what tells the framework nobody served it. `SystemChannels.platform` is
/// an `OptionalMethodChannel` precisely so that an unimplemented method is
/// quiet rather than an exception.
std::optional<std::string> HandlePlatformCall(ExitRequester* requester,
                                              const std::string& method,
                                              const rapidjson::Value* args) {
  if (method == "System.exitApplication") {
    if (args == nullptr || !args->IsObject()) {
      return ErrorEnvelope(kExitRequestError, kInvalidExitRequestMessage);
    }
    auto type = args->FindMember("type");
    if (type == args->MemberEnd() || !type->value.IsString()) {
      return ErrorEnvelope(kExitRequestError, kInvalidExitRequestMessage);
    }
    auto code = args->FindMember("exitCode");
    if (code == args->MemberEnd() || !code->value.IsInt()) {
      return ErrorEnvelope(kExitRequestError, kInvalidExitRequestMessage);
    }
    const int exit_code = code->value.GetInt();
    const bool cancelable = std::string(type->value.GetString()) == kExitTypeCancelable;

    if (!cancelable) {
      requester->QuitApplication(exit_code);
      return SuccessEnvelope([](auto& writer) {
        writer.StartObject();
        writer.Key("response");
        writer.String("exit");
        writer.EndObject();
      });
    }
    // The surprising answer, and upstream's: a cancelable request is answered
    // "cancel" straight away, because the actual question is only being asked
    // now. If the framework says yes, the window closes; the reply to *this*
    // call is not where that shows up.
    requester->RequestAppExit(/*cancelable=*/true, exit_code);
    return SuccessEnvelope([](auto& writer) {
      writer.StartObject();
      writer.Key("response");
      writer.String("cancel");
      writer.EndObject();
    });
  }

  if (method == "SystemNavigator.pop") {
    requester->QuitApplication(0);
    return NullEnvelope();
  }

  if (method == "SystemSound.play") {
    const std::string sound =
        args != nullptr && args->IsString() ? args->GetString() : std::string();
    if (sound == "SystemSoundType.alert") {
      dispatch_async(dispatch_get_main_queue(), ^{
        NSBeep();
      });
      return NullEnvelope();
    }
    // A click and a tick have no system sound on macOS either, and succeeding
    // silently is what upstream's macOS handler does.
    if (sound == "SystemSoundType.click" || sound == "SystemSoundType.tick") {
      return NullEnvelope();
    }
    return std::nullopt;
  }

  if (method == "Clipboard.getData") {
    // "text/plain" is the only format the channel has ever defined.
    if (args == nullptr || !args->IsString() ||
        std::string(args->GetString()) != kTextPlainFormat) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    const std::string text = ClipboardText();
    if (text.empty()) {
      // Nothing to paste, which is distinct from the clipboard being
      // unavailable -- and on macOS it cannot be unavailable.
      return NullEnvelope();
    }
    return SuccessEnvelope([&text](auto& writer) {
      writer.StartObject();
      writer.Key("text");
      writer.String(text.c_str(), static_cast<rapidjson::SizeType>(text.size()));
      writer.EndObject();
    });
  }

  if (method == "Clipboard.setData") {
    if (args == nullptr || !args->IsObject()) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    auto text = args->FindMember("text");
    if (text == args->MemberEnd() || !text->value.IsString()) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    SetClipboardText(text->value.GetString());
    return NullEnvelope();
  }

  if (method == "Clipboard.hasStrings") {
    // Deliberately not a read: a paste button only needs to know whether to be
    // enabled, which is the whole reason this method exists apart from getData.
    if (args == nullptr || !args->IsString() ||
        std::string(args->GetString()) != kTextPlainFormat) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    const bool has_text = ClipboardHasText();
    return SuccessEnvelope([has_text](auto& writer) {
      writer.StartObject();
      writer.Key("value");
      writer.Bool(has_text);
      writer.EndObject();
    });
  }

  if (method == "System.initializationComplete") {
    // Deprecated upstream, and answered rather than refused for the reason the
    // comment there gives: an application that still sends it should not see an
    // error for it.
    return NullEnvelope();
  }

  return std::nullopt;
}

//------------------------------------------------------------------------------
// The mouse cursor.
//
// `flutter/mousecursor` speaks the binary standard codec rather than JSON.
// The handler is upstream's `FlutterMouseCursorPlugin`, name for name: the
// framework picks the names, so the table is a protocol and not a preference.

NSCursor* CursorByName(const std::string& name) {
  if (name == "click" || name == "grab") {
    return [NSCursor pointingHandCursor];
  }
  if (name == "text") {
    return [NSCursor IBeamCursor];
  }
  if (name == "verticalText") {
    return [NSCursor IBeamCursorForVerticalLayout];
  }
  if (name == "grabbing" || name == "move" || name == "allScroll") {
    return [NSCursor closedHandCursor];
  }
  if (name == "resizeLeftRight" || name == "resizeColumn") {
    return [NSCursor resizeLeftRightCursor];
  }
  if (name == "resizeUpDown" || name == "resizeRow") {
    return [NSCursor resizeUpDownCursor];
  }
  if (name == "contextMenu") {
    return [NSCursor contextualMenuCursor];
  }
  if (name == "copy" || name == "alias") {
    return [NSCursor dragCopyCursor];
  }
  if (name == "disappearing") {
    return [NSCursor disappearingItemCursor];
  }
  if (name == "forbidden" || name == "noDrop") {
    return [NSCursor operationNotAllowedCursor];
  }
  if (name == "crosshair" || name == "precise" || name == "cell") {
    return [NSCursor crosshairCursor];
  }
  // An unknown name falls back to the arrow rather than failing, as upstream
  // does: a cursor is a hint, and refusing one is worse than showing the
  // default.
  return [NSCursor arrowCursor];
}

/// What the framework last asked for, and whether it asked for none at all.
struct CursorRequest {
  std::atomic<bool> hidden{false};
  /// Guarded by the main thread, which is the only thread that reads it.
  NSCursor* cursor = nil;
};

std::vector<uint8_t> HandleMouseCursorCall(CursorRequest* request, const MethodCall<>& call) {
  auto& codec = StandardMethodCodec::GetInstance();
  if (call.method_name() != "activateSystemCursor") {
    return *codec.EncodeErrorEnvelope("unimplemented", "", nullptr);
  }
  const auto* arguments = std::get_if<EncodableMap>(call.arguments());
  if (arguments == nullptr) {
    return *codec.EncodeErrorEnvelope("error", "Missing arguments", nullptr);
  }
  auto kind = arguments->find(EncodableValue("kind"));
  if (kind == arguments->end() || !std::holds_alternative<std::string>(kind->second)) {
    return *codec.EncodeErrorEnvelope("error", "Missing 'kind'", nullptr);
  }
  const std::string name = std::get<std::string>(kind->second);

  // "none" is not a cursor, it is the absence of one, and NSCursor has no
  // member for it -- hiding is a separate call with its own balance rules.
  const bool hide = name == "none";
  const bool was_hidden = request->hidden.exchange(hide);
  NSCursor* cursor = hide ? nil : CursorByName(name);
  dispatch_async(dispatch_get_main_queue(), ^{
    if (hide) {
      if (!was_hidden) {
        [NSCursor hide];
      }
    } else {
      if (was_hidden) {
        [NSCursor unhide];
      }
      [cursor set];
    }
  });
  request->cursor = cursor;
  return *codec.EncodeSuccessEnvelope();
}

//------------------------------------------------------------------------------
// Platform settings.

/// Whether the reader has asked for dark mode. Upstream's macOS engine reads
/// the same appearance name.
bool PrefersDarkTheme() {
  NSAppearance* appearance = [NSApp effectiveAppearance];
  if (appearance == nil) {
    return false;
  }
  NSAppearanceName name = [appearance
      bestMatchFromAppearancesWithNames:@[ NSAppearanceNameAqua, NSAppearanceNameDarkAqua ]];
  return [name isEqualToString:NSAppearanceNameDarkAqua];
}

/// Whether times should be written 13:00 rather than 1:00 PM.
///
/// macOS has no switch for this: the answer is a property of the locale's own
/// time format, which is what upstream's `FlutterEngine` reads too.
bool AlwaysUse24HourFormat() {
  NSString* format = [NSDateFormatter dateFormatFromTemplate:@"j"
                                                     options:0
                                                      locale:[NSLocale currentLocale]];
  return format != nil && [format rangeOfString:@"a"].location == NSNotFound;
}

std::string SettingsPayload() {
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartObject();
  writer.Key("alwaysUse24HourFormat");
  writer.Bool(AlwaysUse24HourFormat());
  // macOS has no global text scale the way Windows and Android do -- the
  // accessibility setting there scales individual applications' own text, and
  // AppKit exposes nothing to read. One is the honest answer rather than a
  // guess at one.
  writer.Key("textScaleFactor");
  writer.Double(1.0);
  writer.Key("platformBrightness");
  writer.String(PrefersDarkTheme() ? "dark" : "light");
  writer.EndObject();
  return buffer.GetString();
}

/// The `flutter/localization` payload: the locales the reader has chosen, in
/// order, each as the four strings `setLocale` expects.
std::optional<std::string> LocalizationPayload() {
  NSArray<NSString*>* preferred = [NSLocale preferredLanguages];
  if (preferred == nil || [preferred count] == 0) {
    return std::nullopt;
  }
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartObject();
  writer.Key("method");
  writer.String("setLocale");
  writer.Key("args");
  writer.StartArray();
  for (NSString* identifier in preferred) {
    NSLocale* locale = [NSLocale localeWithLocaleIdentifier:identifier];
    NSString* language = [locale objectForKey:NSLocaleLanguageCode];
    NSString* country = [locale objectForKey:NSLocaleCountryCode];
    NSString* script = [locale objectForKey:NSLocaleScriptCode];
    NSString* variant = [locale objectForKey:NSLocaleVariantCode];
    // Four strings per locale, in this order, empty where there is nothing --
    // `PlatformDispatcher.setLocale` reads them positionally.
    writer.String(language == nil ? "" : [language UTF8String]);
    writer.String(country == nil ? "" : [country UTF8String]);
    writer.String(script == nil ? "" : [script UTF8String]);
    writer.String(variant == nil ? "" : [variant UTF8String]);
  }
  writer.EndArray();
  writer.EndObject();
  return std::string(buffer.GetString());
}

//------------------------------------------------------------------------------
/// A reply that has to reach the window thread.
class HostPlatformMessageResponse : public PlatformMessageResponse {
 public:
  using Handler = std::function<void(const uint8_t*, size_t)>;

  HostPlatformMessageResponse(fml::RefPtr<fml::TaskRunner> runner, Handler handler)
      : runner_(std::move(runner)), handler_(std::move(handler)) {}

  void Complete(std::unique_ptr<fml::Mapping> data) override {
    if (data == nullptr) {
      CompleteEmpty();
      return;
    }
    std::vector<uint8_t> reply(data->GetMapping(), data->GetMapping() + data->GetSize());
    Post(std::move(reply));
  }

  void CompleteEmpty() override { Post({}); }

 private:
  void Post(std::vector<uint8_t> reply) {
    if (is_complete_) {
      return;
    }
    is_complete_ = true;
    auto handler = handler_;
    runner_->PostTask(fml::MakeCopyable(
        [handler, reply = std::move(reply)]() { handler(reply.data(), reply.size()); }));
  }

  fml::RefPtr<fml::TaskRunner> runner_;
  Handler handler_;
};

//------------------------------------------------------------------------------
/// What the window and the shell share. The window thread writes the geometry,
/// the platform thread reads it.
struct WindowState {
  Shell* shell = nullptr;
  class HostPlatformView* platform_view = nullptr;
  fml::RefPtr<fml::TaskRunner> platform_task_runner;
  FrameBuffer frame_buffer;
  CursorRequest cursor;
  double device_pixel_ratio = 1.0;
  int32_t physical_width = 0;
  int32_t physical_height = 0;
  std::string lifecycle_state;
  /// The mouse's last position, in physical pixels, for the pointer deltas.
  double last_x = 0.0;
  double last_y = 0.0;
  /// Whether any mouse button is currently down, so a move can be told apart
  /// from a drag.
  bool pressed = false;
  /// **Which** buttons are down, as `kPointerButtonMouse*` bits — upstream
  /// FlutterViewController's `MouseState.buttons`.
  ///
  /// Not derivable from `pressed`: the framework routes a secondary press to
  /// different handlers -- `onSecondaryTap` is what opens a text field's
  /// context menu -- so a press reported without its button is a right click
  /// that arrives as a left one.
  int64_t buttons = 0;
  /// Which modifier bits were set last time, so a `flagsChanged` can say which
  /// key moved and in which direction.
  uint64_t modifier_flags = 0;
  /// The platform half of `flutter/textinput`: the editing model typing is
  /// applied to. Channel calls reach it on the platform thread, keys on the
  /// main thread; it locks internally.
  TextInputHandler text_input;
  RfContentView* view = nil;
  NSWindow* window = nil;
};

//------------------------------------------------------------------------------
/// The platform view: the shell's window onto this host.
class HostPlatformView final : public PlatformView,
                               public GPUSurfaceSoftwareDelegate,
                               public ExitRequester {
 public:
  HostPlatformView(PlatformView::Delegate& delegate,
                   const TaskRunners& task_runners,
                   WindowState* state)
      : PlatformView(delegate, task_runners), state_(state) {}

  ~HostPlatformView() override = default;

  // |PlatformView|
  std::unique_ptr<Surface> CreateRenderingSurface() override {
    // Where a Metal/Impeller surface would attach. The software surface needs
    // nothing from the platform, which is why this host can be a window and an
    // input pump and nothing else.
    return std::make_unique<GPUSurfaceSoftware>(this,
                                                /*render_to_surface=*/true);
  }

  // |PlatformView|
  std::unique_ptr<VsyncWaiter> CreateVSyncWaiter() override {
    return std::make_unique<VsyncWaiterMac>(task_runners_);
  }

  // |GPUSurfaceSoftwareDelegate|
  sk_sp<SkSurface> AcquireBackingStore(const DlISize& size) override {
    if (size.width <= 0 || size.height <= 0) {
      return nullptr;
    }
    if (backing_store_ != nullptr && backing_store_->width() == size.width &&
        backing_store_->height() == size.height) {
      return backing_store_;
    }
    SkImageInfo info = SkImageInfo::MakeN32Premul(size.width, size.height);
    backing_store_ = SkSurfaces::Raster(info);
    return backing_store_;
  }

  // |GPUSurfaceSoftwareDelegate|
  bool PresentBackingStore(sk_sp<SkSurface> backing_store) override;

  /// Sends one pointer event to the engine. Called from the main thread, which
  /// is why it hops: PlatformView is not thread safe, and the pointer
  /// dispatcher expects to run on the platform thread.
  void SendPointer(const PointerData& data) {
    auto packet = std::make_unique<PointerDataPacket>(1);
    packet->SetPointerData(0, data);
    task_runners_.GetPlatformTaskRunner()->PostTask(fml::MakeCopyable([weak = GetWeakPtr(),
                                                                       packet = std::move(
                                                                           packet)]() mutable {
      if (weak) {
        static_cast<HostPlatformView*>(weak.get())->DispatchPointerDataPacket(std::move(packet));
      }
    }));
  }

  /// Sends one key event to the framework.
  ///
  /// Keys are a platform message rather than a call of their own, which is what
  /// every Flutter embedder does: the packet on `flutter/keydata` is the same
  /// bytes on Windows, Android, iOS and Linux, and no key-shaped method exists
  /// on PlatformView to add one to.
  ///
  /// No response handle is asked for. The reply says whether the framework used
  /// the key, and the only thing to do with that answer is to hand the
  /// unhandled ones back to the system -- which this host does not withhold in
  /// the first place, so there is nothing to hand back.
  void SendKey(const KeyData& data, const std::string& character) {
    KeyDataPacket packet(data, character.empty() ? nullptr : character.c_str());
    auto message = std::make_unique<PlatformMessage>(
        kKeyDataChannel, fml::MallocMapping::Copy(packet.data().data(), packet.data().size()),
        /*response=*/nullptr);
    task_runners_.GetPlatformTaskRunner()->PostTask(
        fml::MakeCopyable([weak = GetWeakPtr(), message = std::move(message)]() mutable {
          if (weak) {
            weak->DispatchPlatformMessage(std::move(message));
          }
        }));
  }

  void SendPlatformSettings() {
    SendOnChannel(kSettingsChannel, SettingsPayload());
    if (auto locales = LocalizationPayload()) {
      SendOnChannel(kLocalizationChannel, *locales);
    }
  }

  /// Sends a JSON method call the framework listens for --
  /// `TextInputClient.updateEditingState` is the caller that made this exist.
  void SendMethodCall(const char* channel,
                      const std::string& method,
                      const std::string& arguments_json) {
    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.StartObject();
    writer.Key("method");
    writer.String(method.c_str());
    writer.Key("args");
    // Already JSON, so it goes in as it is rather than through the writer.
    writer.RawValue(arguments_json.c_str(), arguments_json.size(), rapidjson::kArrayType);
    writer.EndObject();
    SendOnChannel(channel, std::string(buffer.GetString(), buffer.GetSize()));
  }

  /// Tells the framework what the application is doing. One bare string on
  /// `flutter/lifecycle`, with no codec and no envelope -- there is nothing
  /// else the channel could ever need to say.
  void SendLifecycleState(const char* state) {
    SendOnChannel(kLifecycleChannel, std::string(state));
  }

  // |PlatformView|
  //
  // A message from the framework, on the platform thread. Upstream this is
  // where an embedder's plugins are dispatched to; here it is the two channels
  // this host serves. Anything else falls through to an empty reply, which the
  // framework reads as "nobody implements this".
  void HandlePlatformMessage(std::unique_ptr<PlatformMessage> message) override {
    const auto& data = message->data();
    std::optional<std::vector<uint8_t>> reply;

    if (message->channel() == kMouseCursorChannel) {
      auto call =
          StandardMethodCodec::GetInstance().DecodeMethodCall(data.GetMapping(), data.GetSize());
      if (call) {
        reply = HandleMouseCursorCall(&state_->cursor, *call);
      }
    } else if (message->channel() == kPlatformChannel || message->channel() == kTextInputChannel) {
      const bool editing = message->channel() == kTextInputChannel;
      rapidjson::Document document;
      document.Parse(reinterpret_cast<const char*>(data.GetMapping()), data.GetSize());
      if (!document.HasParseError() && document.IsObject()) {
        auto method = document.FindMember("method");
        if (method != document.MemberEnd() && method->value.IsString()) {
          auto args = document.FindMember("args");
          const rapidjson::Value* arguments = args != document.MemberEnd() ? &args->value : nullptr;
          const std::string method_name = method->value.GetString();
          auto answer = editing ? state_->text_input.HandleMethodCall(method_name, arguments)
                                : HandlePlatformCall(this, method_name.c_str(), arguments);
          if (answer) {
            reply = std::vector<uint8_t>(answer->begin(), answer->end());
          }
          if (editing && method_name == "TextInput.clearClient") {
            // The IME may still be composing into the field that just went
            // away; the input context lives on the main thread.
            RfDiscardMarkedText(state_->view);
          }
        }
      }
    }

    if (auto response = message->response()) {
      if (reply) {
        response->Complete(std::make_unique<fml::DataMapping>(*reply));
      } else {
        response->CompleteEmpty();
      }
    }
  }

  // |ExitRequester|
  void RequestAppExit(bool cancelable, int exit_code) override;
  // |ExitRequester|
  void QuitApplication(int exit_code) override;

 private:
  void SendOnChannel(const char* channel, const std::string& payload) {
    auto message = std::make_unique<PlatformMessage>(
        channel, fml::MallocMapping::Copy(payload.data(), payload.size()),
        /*response=*/nullptr);
    task_runners_.GetPlatformTaskRunner()->PostTask(
        fml::MakeCopyable([weak = GetWeakPtr(), message = std::move(message)]() mutable {
          if (weak) {
            weak->DispatchPlatformMessage(std::move(message));
          }
        }));
  }

  WindowState* state_ = nullptr;
  sk_sp<SkSurface> backing_store_;

  FML_DISALLOW_COPY_AND_ASSIGN(HostPlatformView);
};

//------------------------------------------------------------------------------
/// Builds a PointerData for one mouse event.
///
/// macOS reports a single system mouse, so device and pointer identity are both
/// constant. Coordinates arrive in points and are converted to the physical
/// pixels the engine works in.
PointerData MakePointerData(WindowState* state, PointerData::Change change, double x, double y) {
  PointerData data;
  data.Clear();
  data.time_stamp = fml::TimePoint::Now().ToEpochDelta().ToMicroseconds();
  data.change = change;
  data.kind = PointerData::DeviceKind::kMouse;
  data.signal_kind = PointerData::SignalKind::kNone;
  data.device = 0;
  data.pointer_identifier = 0;
  data.physical_x = x;
  data.physical_y = y;
  data.physical_delta_x = x - state->last_x;
  data.physical_delta_y = y - state->last_y;
  data.buttons = state->buttons;
  data.pressure = state->pressed ? 1.0 : 0.0;
  data.pressure_max = 1.0;
  data.view_id = kFlutterImplicitViewId;
  state->last_x = x;
  state->last_y = y;
  return data;
}

/// A wheel turn or a trackpad swipe, as a hover carrying a scroll signal.
///
/// It is not its own change: the pointer did not go anywhere, and a recogniser
/// that read the change would see a mouse being moved. The signal is what says
/// otherwise.
PointerData MakeScrollData(WindowState* state, double x, double y, double delta_x, double delta_y) {
  PointerData data = MakePointerData(state, PointerData::Change::kHover, x, y);
  data.signal_kind = PointerData::SignalKind::kScroll;
  data.scroll_delta_x = delta_x;
  data.scroll_delta_y = delta_y;
  return data;
}

void SendViewportMetrics(WindowState* state, int32_t width, int32_t height) {
  if (state->shell == nullptr || width <= 0 || height <= 0) {
    return;
  }
  ViewportMetrics metrics;
  metrics.device_pixel_ratio = state->device_pixel_ratio;
  metrics.physical_width = width;
  metrics.physical_height = height;
  metrics.physical_max_width_constraint = width;
  metrics.physical_max_height_constraint = height;

  state->platform_task_runner->PostTask([view = state->shell->GetPlatformView(), metrics]() {
    if (view) {
      view->SetViewportMetrics(kFlutterImplicitViewId, metrics);
    }
  });
}

void SendLifecycle(WindowState* state, const char* next) {
  if (state == nullptr || state->platform_view == nullptr) {
    return;
  }
  if (state->lifecycle_state == next) {
    return;
  }
  state->lifecycle_state = next;
  state->platform_view->SendLifecycleState(next);
}

std::string DefaultIcuDataPath() {
  auto directory = fml::paths::GetExecutableDirectoryPath();
  if (!directory.first) {
    return "";
  }
  return fml::paths::JoinPaths({directory.second, "icudtl.dat"});
}

}  // namespace
}  // namespace flutter

//------------------------------------------------------------------------------
// The window.
//
// Cocoa objects cannot live in a namespace, so these sit at file scope with a
// prefix and hold a pointer to the state the C++ half owns.

@interface RfContentView : NSView <NSTextInputClient>
@property(nonatomic, assign) flutter::WindowState* state;
@end

static void RfDiscardMarkedText(RfContentView* view) {
  dispatch_async(dispatch_get_main_queue(), ^{
    [[view inputContext] discardMarkedText];
  });
}

@implementation RfContentView

/// Top-left origin, which is the coordinate system the engine works in. Without
/// this every pointer would arrive mirrored vertically.
- (BOOL)isFlipped {
  return YES;
}

- (BOOL)acceptsFirstResponder {
  return YES;
}

/// So a click on an unfocused window both focuses it and reaches the framework,
/// which is what a Flutter application on macOS does.
- (BOOL)acceptsFirstMouse:(NSEvent*)event {
  return YES;
}

- (void)drawRect:(NSRect)dirty {
  CGContextRef context = [[NSGraphicsContext currentContext] CGContext];
  if (context == nullptr || _state == nullptr) {
    return;
  }
  // Straight through: the view is flipped, which is the coordinate system
  // `Paint` is written against.
  if (!_state->frame_buffer.Paint(context, self.bounds)) {
    // Nothing has been rasterised yet. Painting the background rather than
    // leaving whatever was in the window means the first moment of the app is
    // a blank window rather than a torn one.
    CGContextSetRGBFillColor(context, 0, 0, 0, 1);
    CGContextFillRect(context, dirty);
  }
}

// -- pointers ----------------------------------------------------------------

- (flutter::PointerData)pointerFor:(NSEvent*)event change:(flutter::PointerData::Change)change {
  NSPoint point = [self convertPoint:event.locationInWindow fromView:nil];
  const double scale = _state->device_pixel_ratio;
  return flutter::MakePointerData(_state, change, point.x * scale, point.y * scale);
}

- (void)send:(const flutter::PointerData&)data {
  if (_state != nullptr && _state->platform_view != nullptr) {
    _state->platform_view->SendPointer(data);
  }
}

- (void)mouseDown:(NSEvent*)event {
  // The button state is set before the event is built: `MakePointerData` reads
  // it, and a down that reported no buttons would be a hover as far as the
  // gesture recognisers are concerned. Upstream's `mouseDown:`
  // (FlutterViewController.mm) sets `_mouseState.buttons` the same way.
  _state->buttons |= flutter::kPointerButtonMousePrimary;
  _state->pressed = true;
  [self send:[self pointerFor:event change:flutter::PointerData::Change::kDown]];
}

- (void)mouseUp:(NSEvent*)event {
  // The up still carries the button that is being released: the framework
  // reads which button a tap was from the *down*, and a release reporting
  // nothing held is what ends the gesture.
  flutter::PointerData data = [self pointerFor:event change:flutter::PointerData::Change::kUp];
  _state->buttons &= ~static_cast<int64_t>(flutter::kPointerButtonMousePrimary);
  _state->pressed = _state->buttons != 0;
  [self send:data];
}

- (void)mouseDragged:(NSEvent*)event {
  [self send:[self pointerFor:event change:flutter::PointerData::Change::kMove]];
}

- (void)rightMouseDown:(NSEvent*)event {
  _state->buttons |= flutter::kPointerButtonMouseSecondary;
  _state->pressed = true;
  [self send:[self pointerFor:event change:flutter::PointerData::Change::kDown]];
}

- (void)rightMouseUp:(NSEvent*)event {
  flutter::PointerData data = [self pointerFor:event change:flutter::PointerData::Change::kUp];
  _state->buttons &= ~static_cast<int64_t>(flutter::kPointerButtonMouseSecondary);
  _state->pressed = _state->buttons != 0;
  [self send:data];
}

- (void)rightMouseDragged:(NSEvent*)event {
  [self send:[self pointerFor:event change:flutter::PointerData::Change::kMove]];
}

// A third button's bit is its AppKit button number, which is what upstream's
// `otherMouseDown:` uses too -- number 2, the wheel, lands on
// `kPointerButtonMouseMiddle`.
- (void)otherMouseDown:(NSEvent*)event {
  _state->buttons |= (1 << event.buttonNumber);
  _state->pressed = true;
  [self send:[self pointerFor:event change:flutter::PointerData::Change::kDown]];
}

- (void)otherMouseUp:(NSEvent*)event {
  flutter::PointerData data = [self pointerFor:event change:flutter::PointerData::Change::kUp];
  _state->buttons &= ~static_cast<int64_t>(1 << event.buttonNumber);
  _state->pressed = _state->buttons != 0;
  [self send:data];
}

- (void)otherMouseDragged:(NSEvent*)event {
  [self send:[self pointerFor:event change:flutter::PointerData::Change::kMove]];
}

- (void)mouseMoved:(NSEvent*)event {
  [self send:[self pointerFor:event change:flutter::PointerData::Change::kHover]];
}

- (void)mouseEntered:(NSEvent*)event {
  [self send:[self pointerFor:event change:flutter::PointerData::Change::kAdd]];
}

- (void)mouseExited:(NSEvent*)event {
  [self send:[self pointerFor:event change:flutter::PointerData::Change::kRemove]];
}

- (void)scrollWheel:(NSEvent*)event {
  NSPoint point = [self convertPoint:event.locationInWindow fromView:nil];
  const double scale = _state->device_pixel_ratio;
  // A trackpad reports precise deltas in points; a wheel reports notches, and
  // upstream's macOS engine multiplies those by the same hundred-pixels-per-
  // three-lines Chromium uses. Either way the sign is inverted, because the
  // deltas describe the finger and the framework wants where the reader is
  // going.
  double delta_x = event.scrollingDeltaX;
  double delta_y = event.scrollingDeltaY;
  if (!event.hasPreciseScrollingDeltas) {
    delta_x *= 100.0 / 3.0;
    delta_y *= 100.0 / 3.0;
  }
  [self send:flutter::MakeScrollData(_state, point.x * scale, point.y * scale, -delta_x * scale,
                                     -delta_y * scale)];
}

// -- keys --------------------------------------------------------------------

/// The first code point of a string, or zero for an empty one.
static uint32_t FirstCodePoint(NSString* text) {
  if (text == nil || text.length == 0) {
    return 0;
  }
  const unichar first = [text characterAtIndex:0];
  if (CFStringIsSurrogateHighCharacter(first) && text.length > 1) {
    return CFStringGetLongCharacterForSurrogatePair(first, [text characterAtIndex:1]);
  }
  return first;
}

- (void)sendKey:(NSEvent*)event type:(flutter::KeyEventType)type synthesized:(BOOL)synthesized {
  if (_state == nullptr || _state->platform_view == nullptr) {
    return;
  }
  NSString* unmodified = nil;
  NSString* characters = nil;
  // A modifier event has neither, and asking for them throws.
  if (event.type == NSEventTypeKeyDown || event.type == NSEventTypeKeyUp) {
    unmodified = event.charactersIgnoringModifiers;
    characters = event.characters;
  }

  flutter::KeyData data;
  data.Clear();
  data.timestamp = static_cast<uint64_t>(event.timestamp * 1000000.0);
  data.type = type;
  data.physical = flutter::PhysicalKeyForKeyCode(event.keyCode);
  data.logical = flutter::LogicalKeyForKeyCode(event.keyCode, FirstCodePoint(unmodified));
  data.synthesized = synthesized ? 1 : 0;

  // The character is what the key produced, and only a press produces one. A
  // repeat carries it too, which is what makes held keys type.
  std::string text;
  if (type != flutter::KeyEventType::kUp && characters != nil && characters.length > 0) {
    const uint32_t code_point = FirstCodePoint(characters);
    // Control characters are what a key *is*, not what it typed: enter is not
    // a carriage return in a text field, it is an action.
    if (code_point >= 0x20 && code_point != 0x7f) {
      text = std::string([characters UTF8String]);
    }
  }
  _state->platform_view->SendKey(data, text);
}

- (void)keyDown:(NSEvent*)event {
  [self sendKey:event
           type:(event.isARepeat ? flutter::KeyEventType::kRepeat
                                 : flutter::KeyEventType::kDown)synthesized:NO];

  // The editing half: the framework owns the session, the platform owns the
  // typing (see `TextInputHandler`). A key with command or control on it is a
  // shortcut -- the framework's clipboard handlers among them -- and is not
  // typed. This host does not wait for the framework's verdict the way the
  // Windows host redispatches; the only text-bearing keys the framework
  // consumes are those shortcuts, and they are skipped here.
  if (_state == nullptr ||
      (event.modifierFlags & (NSEventModifierFlagCommand | NSEventModifierFlagControl)) != 0) {
    return;
  }
  if (!_state->text_input.attached()) {
    return;
  }
  // Outside a composition the editing keys belong to the field: Return
  // submits, backspace deletes, the arrows move the caret. *Inside* one they
  // belong to the input method -- Return takes the composition, the arrows
  // walk the candidate list -- so everything goes through
  // `interpretKeyEvents:`, which is what hands the event to the IME and calls
  // back on the `NSTextInputClient` methods below. A plain key with no IME
  // engaged comes straight back as `insertText:`, which is how ordinary
  // typing arrives too.
  if (!_state->text_input.Composing()) {
    if (event.keyCode == 0x24 || event.keyCode == 0x4C) {  // Return, keypad Enter.
      _state->text_input.OnAction();
      return;
    }
    if (_state->text_input.OnEditingKey(event.keyCode,
                                        (event.modifierFlags & NSEventModifierFlagShift) != 0)) {
      return;
    }
  }
  [self interpretKeyEvents:@[ event ]];
}

// -- NSTextInputClient --------------------------------------------------------
//
// The half an input method talks to. Upstream this is
// `FlutterTextInputPlugin.mm`, an `NSTextInputClient` over the same editing
// model; this is that shape with the committed and marked text and the caret
// rectangle, and none of the attributed-string detail -- the framework draws
// the text, so the attributes have nowhere to go.

- (void)insertText:(id)string replacementRange:(NSRange)replacementRange {
  if (_state == nullptr) {
    return;
  }
  NSString* text =
      [string isKindOfClass:[NSAttributedString class]] ? [string string] : (NSString*)string;
  if (text == nil || text.length == 0) {
    return;
  }
  std::u16string units;
  units.reserve(text.length);
  for (NSUInteger i = 0; i < text.length; i++) {
    const unichar unit = [text characterAtIndex:i];
    // AppKit spells arrows, F-keys and friends as code points in the Unicode
    // function-key block; those are keys, not text. Control characters are
    // what a key *is*, not what it typed.
    if (unit < 0x20 || unit == 0x7F || (unit >= 0xF700 && unit <= 0xF8FF)) {
      return;
    }
    units.push_back(unit);
  }
  _state->text_input.OnInsertText(units);
}

- (void)setMarkedText:(id)string
        selectedRange:(NSRange)selectedRange
     replacementRange:(NSRange)replacementRange {
  if (_state == nullptr) {
    return;
  }
  NSString* text =
      [string isKindOfClass:[NSAttributedString class]] ? [string string] : (NSString*)string;
  if (text == nil) {
    return;
  }
  std::u16string units;
  units.reserve(text.length);
  for (NSUInteger i = 0; i < text.length; i++) {
    units.push_back([text characterAtIndex:i]);
  }
  _state->text_input.OnSetMarkedText(
      units,
      selectedRange.location == NSNotFound ? static_cast<long>(units.size())
                                           : static_cast<long>(selectedRange.location),
      selectedRange.location == NSNotFound ? 0 : static_cast<long>(selectedRange.length));
}

- (void)unmarkText {
  if (_state != nullptr) {
    _state->text_input.OnUnmarkText();
  }
}

- (BOOL)hasMarkedText {
  return _state != nullptr && _state->text_input.Composing();
}

- (NSRange)markedRange {
  long location = -1;
  long length = 0;
  if (_state != nullptr) {
    _state->text_input.GetMarkedRange(&location, &length);
  }
  if (location < 0) {
    return NSMakeRange(NSNotFound, 0);
  }
  return NSMakeRange(static_cast<NSUInteger>(location), static_cast<NSUInteger>(length));
}

- (NSRange)selectedRange {
  long location = -1;
  long length = 0;
  if (_state != nullptr) {
    _state->text_input.GetSelectedRange(&location, &length);
  }
  if (location < 0) {
    return NSMakeRange(NSNotFound, 0);
  }
  return NSMakeRange(static_cast<NSUInteger>(location), static_cast<NSUInteger>(length));
}

- (NSAttributedString*)attributedSubstringForProposedRange:(NSRange)range
                                               actualRange:(NSRangePointer)actualRange {
  return nil;
}

- (NSArray<NSAttributedStringKey>*)validAttributesForMarkedText {
  return @[];
}

/// Where the candidate window goes: the caret's rectangle, reported by the
/// framework at paint in logical pixels, converted view -> window -> screen.
- (NSRect)firstRectForCharacterRange:(NSRange)range actualRange:(NSRangePointer)actualRange {
  double x = 0;
  double y = 0;
  double width = 0;
  double height = 0;
  if (_state == nullptr || !_state->text_input.GetCaretRect(&x, &y, &width, &height)) {
    return NSZeroRect;
  }
  const NSRect local = NSMakeRect(x, y, width, height);
  const NSRect in_window = [self convertRect:local toView:nil];
  return [self.window convertRectToScreen:in_window];
}

- (NSUInteger)characterIndexForPoint:(NSPoint)point {
  return NSNotFound;
}

/// The selectors `interpretKeyEvents:` sends for keys that are not text.
/// Everything this host answers is already handled before the event was
/// offered to the IME (see `keyDown:`), except Return committing through with
/// no composition open -- and the rest must not fall through to `NSView`,
/// whose answer is the system beep.
- (void)doCommandBySelector:(SEL)selector {
  if (_state == nullptr) {
    return;
  }
  if (selector == @selector(insertNewline:)) {
    _state->text_input.OnAction();
  }
}

- (void)keyUp:(NSEvent*)event {
  [self sendKey:event type:flutter::KeyEventType::kUp synthesized:NO];
}

/// macOS reports a modifier as a change of state rather than as a press and a
/// release, so which it was has to be worked out: ask which bit this key owns,
/// and look at whether it is now set.
- (void)flagsChanged:(NSEvent*)event {
  if (_state == nullptr) {
    return;
  }
  const uint64_t bit = flutter::ModifierFlagForKeyCode(event.keyCode);
  if (bit == 0) {
    // Caps lock, which has no left/right bit of its own and toggles rather than
    // being held. Reported as a press and a release together, which is what
    // upstream's macOS engine does with it.
    return;
  }
  const uint64_t flags = static_cast<uint64_t>(event.modifierFlags);
  const bool now_down = (flags & bit) != 0;
  _state->modifier_flags = flags;
  [self
      sendKey:event
         type:(now_down ? flutter::KeyEventType::kDown : flutter::KeyEventType::kUp)synthesized:NO];
}

// -- geometry ----------------------------------------------------------------

- (void)updateMetrics {
  if (_state == nullptr) {
    return;
  }
  const double scale = self.window.backingScaleFactor > 0 ? self.window.backingScaleFactor : 1.0;
  _state->device_pixel_ratio = scale;
  const NSSize size = self.bounds.size;
  _state->physical_width = static_cast<int32_t>(size.width * scale);
  _state->physical_height = static_cast<int32_t>(size.height * scale);
  flutter::SendViewportMetrics(_state, _state->physical_width, _state->physical_height);
}

- (void)setFrameSize:(NSSize)size {
  [super setFrameSize:size];
  [self updateMetrics];
}

- (void)viewDidChangeBackingProperties {
  [super viewDidChangeBackingProperties];
  [self updateMetrics];
}

/// The tracking area is what makes hover and enter/exit arrive at all; without
/// one, `mouseMoved:` is never called.
- (void)updateTrackingAreas {
  for (NSTrackingArea* area in [self.trackingAreas copy]) {
    [self removeTrackingArea:area];
  }
  NSTrackingArea* area =
      [[NSTrackingArea alloc] initWithRect:self.bounds
                                   options:(NSTrackingMouseEnteredAndExited | NSTrackingMouseMoved |
                                            NSTrackingActiveInKeyWindow | NSTrackingInVisibleRect)
                                     owner:self
                                  userInfo:nil];
  [self addTrackingArea:area];
  [super updateTrackingAreas];
}

@end

//------------------------------------------------------------------------------
/// Watches the window for the four things the framework needs to know about it.
@interface RfWindowDelegate : NSObject <NSWindowDelegate>
@property(nonatomic, assign) flutter::WindowState* state;
@end

@implementation RfWindowDelegate

- (void)windowDidResize:(NSNotification*)notification {
  [_state->view updateMetrics];
}

- (void)windowDidChangeBackingProperties:(NSNotification*)notification {
  [_state->view updateMetrics];
}

- (void)windowDidBecomeKey:(NSNotification*)notification {
  flutter::SendLifecycle(_state, "AppLifecycleState.resumed");
}

- (void)windowDidResignKey:(NSNotification*)notification {
  flutter::SendLifecycle(_state, "AppLifecycleState.inactive");
}

- (void)windowDidMiniaturize:(NSNotification*)notification {
  flutter::SendLifecycle(_state, "AppLifecycleState.paused");
}

- (void)windowDidDeminiaturize:(NSNotification*)notification {
  flutter::SendLifecycle(_state, "AppLifecycleState.resumed");
}

- (BOOL)windowShouldClose:(NSWindow*)sender {
  // Closing the window ends the application, which is what a single-window
  // desktop app means by it.
  [NSApp stop:nil];
  // And a stopped NSApp does not notice until it gets another event, so give it
  // one.
  NSEvent* wake = [NSEvent otherEventWithType:NSEventTypeApplicationDefined
                                     location:NSZeroPoint
                                modifierFlags:0
                                    timestamp:0
                                 windowNumber:0
                                      context:nil
                                      subtype:0
                                        data1:0
                                        data2:0];
  [NSApp postEvent:wake atStart:YES];
  return YES;
}

@end

namespace flutter {
namespace {

bool HostPlatformView::PresentBackingStore(sk_sp<SkSurface> backing_store) {
  if (backing_store == nullptr) {
    return false;
  }
  SkPixmap pixmap;
  if (!backing_store->peekPixels(&pixmap)) {
    return false;
  }
  const bool blue_first = pixmap.colorType() == kBGRA_8888_SkColorType;
  // Said once, because the alternative is a swapped-channel picture that still
  // looks like a picture -- and because which one Skia's N32 turns out to be is
  // a property of the build rather than of this file.
  static bool reported = false;
  if (!reported) {
    reported = true;
    FML_LOG(IMPORTANT) << "Presenting " << (blue_first ? "BGRA" : "RGBA") << " frames.";
  }
  state_->frame_buffer.Store(pixmap.addr(), pixmap.width(), pixmap.height(), blue_first);

  // The first frame, to a file, when asked. See FrameBuffer::WritePng.
  static bool dumped = false;
  if (!dumped) {
    if (const char* path = std::getenv("RUSTFLUTTER_DUMP_FRAME")) {
      dumped = true;
      FML_LOG(IMPORTANT) << "Wrote the first frame to " << path << ": "
                         << (state_->frame_buffer.WritePng(path) ? "ok" : "failed");
    }
  }
  // Wakes the main thread, which repaints from the buffer. `dispatch_async`
  // rather than a direct call because this is the raster thread and every
  // AppKit call below belongs to the main one.
  RfContentView* view = state_->view;
  dispatch_async(dispatch_get_main_queue(), ^{
    [view setNeedsDisplay:YES];
  });
  return true;
}

void HostPlatformView::RequestAppExit(bool cancelable, int exit_code) {
  // What upstream does: ask the framework, and close if it does not object.
  // The answer comes back on `System.requestAppExit`, whose reply carries a
  // response of "exit" or "cancel".
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartObject();
  writer.Key("method");
  writer.String("System.requestAppExit");
  writer.Key("args");
  writer.StartObject();
  writer.Key("type");
  writer.String(cancelable ? "cancelable" : "required");
  writer.Key("exitCode");
  writer.Int(exit_code);
  writer.EndObject();
  writer.EndObject();
  const std::string payload = buffer.GetString();

  auto response = fml::MakeRefCounted<HostPlatformMessageResponse>(
      task_runners_.GetPlatformTaskRunner(), [](const uint8_t* reply, size_t length) {
        // A reply of `["exit"]`-shaped JSON means go; anything else, including
        // no reply at all, means stay. Parsing is deliberately forgiving: the
        // only decision is whether the word "exit" is in the answer.
        const std::string text(reinterpret_cast<const char*>(reply), length);
        if (text.find("exit") == std::string::npos) {
          return;
        }
        dispatch_async(dispatch_get_main_queue(), ^{
          [NSApp stop:nil];
        });
      });

  auto message = std::make_unique<PlatformMessage>(
      kPlatformChannel, fml::MallocMapping::Copy(payload.data(), payload.size()),
      std::move(response));
  task_runners_.GetPlatformTaskRunner()->PostTask(
      fml::MakeCopyable([weak = GetWeakPtr(), message = std::move(message)]() mutable {
        if (weak) {
          weak->DispatchPlatformMessage(std::move(message));
        }
      }));
}

void HostPlatformView::QuitApplication(int exit_code) {
  dispatch_async(dispatch_get_main_queue(), ^{
    [NSApp stop:nil];
    NSEvent* wake = [NSEvent otherEventWithType:NSEventTypeApplicationDefined
                                       location:NSZeroPoint
                                  modifierFlags:0
                                      timestamp:0
                                   windowNumber:0
                                        context:nil
                                        subtype:0
                                          data1:0
                                          data2:0];
    [NSApp postEvent:wake atStart:YES];
  });
}

}  // namespace
}  // namespace flutter

int32_t rf_host_run(const RfHostOptions* options) {
  using namespace flutter;  // NOLINT(build/namespaces)

  if (options == nullptr || options->width <= 0 || options->height <= 0) {
    return -1;
  }

  @autoreleasepool {
    Settings settings;
    // Impeller on macOS would be Metal, and there is no Metal surface here yet.
    // The application's preference is read and reported rather than silently
    // ignored, so an app that asked for Impeller learns it did not get it.
    if (options->enable_impeller != 0) {
      FML_LOG(IMPORTANT) << "Impeller was requested; this host renders with the Skia software "
                            "surface. See rustflutter_host_mac.mm.";
    }
    settings.enable_impeller = false;
    settings.enable_software_rendering = true;
    settings.icu_initialization_required = true;
    settings.icu_data_path = options->icu_data_path != nullptr ? std::string(options->icu_data_path)
                                                               : DefaultIcuDataPath();
    // Nothing to prefetch and nothing to warn about: there is no Dart snapshot,
    // and the Impeller opt-out warning is aimed at applications that still have
    // a choice.
    settings.warn_on_impeller_opt_out = false;

    // Text and images are recorded for the backend that will draw them, and
    // this is the software one.
    rf_set_impeller_backend(0);

    WindowState state;

    // -- Window (this thread) -------------------------------------------------

    [NSApplication sharedApplication];
    // A regular application: one that has a menu bar, takes focus, and appears
    // in the Dock. Without this a binary run from a terminal draws a window
    // that can never become key, and every key event goes to the terminal.
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];

    const NSRect frame = NSMakeRect(0, 0, options->width, options->height);
    NSWindow* window = [[NSWindow alloc]
        initWithContentRect:frame
                  styleMask:(NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                             NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable)
                    backing:NSBackingStoreBuffered
                      defer:NO];
    [window setTitle:@(options->title != nullptr ? options->title : "rustflutter")];
    [window center];

    RfContentView* view = [[RfContentView alloc] initWithFrame:frame];
    view.state = &state;
    [window setContentView:view];
    [window makeFirstResponder:view];

    RfWindowDelegate* delegate = [[RfWindowDelegate alloc] init];
    delegate.state = &state;
    [window setDelegate:delegate];

    state.window = window;
    state.view = view;
    state.device_pixel_ratio = window.backingScaleFactor > 0 ? window.backingScaleFactor : 1.0;
    state.physical_width = static_cast<int32_t>(options->width * state.device_pixel_ratio);
    state.physical_height = static_cast<int32_t>(options->height * state.device_pixel_ratio);

    // -- Threads --------------------------------------------------------------

    ThreadHost thread_host("rf", ThreadHost::Type::kPlatform | ThreadHost::Type::kUi |
                                     ThreadHost::Type::kRaster | ThreadHost::Type::kIo);

    TaskRunners task_runners("rustflutter", thread_host.platform_thread->GetTaskRunner(),
                             thread_host.raster_thread->GetTaskRunner(),
                             thread_host.ui_thread->GetTaskRunner(),
                             thread_host.io_thread->GetTaskRunner());

    // -- Shell ----------------------------------------------------------------

    PlatformData platform_data;
    std::unique_ptr<Shell> shell = Shell::Create(
        platform_data, task_runners, settings,
        [&state](Shell& shell) {
          auto view = std::make_unique<HostPlatformView>(shell, shell.GetTaskRunners(), &state);
          // The window needs to reach the view to send pointers and keys. The
          // shell owns it and outlives the run loop, so a raw pointer is
          // enough.
          state.platform_view = view.get();
          state.text_input.SetSender(
              [sender = view.get()](const std::string& method, const std::string& arguments) {
                sender->SendMethodCall(kTextInputChannel, method, arguments);
              });
          return view;
        },
        [](Shell& shell) { return std::make_unique<Rasterizer>(shell); });

    if (shell == nullptr || !shell->IsSetup()) {
      return -4;
    }
    state.shell = shell.get();
    state.platform_task_runner = task_runners.GetPlatformTaskRunner();

    // Everything below belongs to the platform thread: RunEngine checks for it,
    // and NotifyCreated / SetViewportMetrics reach the platform view directly.
    // Ordering matters -- the surface has to exist before the first frame is
    // rasterized, and the framework needs a size before it can lay anything
    // out.
    task_runners.GetPlatformTaskRunner()->PostTask(
        fml::MakeCopyable([shell = shell.get(), &state]() mutable {
          shell->RunEngine(RunConfiguration{});
          if (auto view = shell->GetPlatformView()) {
            view->NotifyCreated();
          }
          // The engine asks the display manager for the refresh rate when it
          // reports frame timings and when it decides how far ahead to
          // schedule. Without this it has no displays at all and falls back to
          // a guess.
          std::vector<std::unique_ptr<Display>> displays;
          displays.push_back(std::make_unique<Display>(
              /*display_id=*/0, DisplayRefreshRate(), state.physical_width, state.physical_height,
              state.device_pixel_ratio));
          shell->OnDisplayUpdates(std::move(displays));
          SendViewportMetrics(&state, state.physical_width, state.physical_height);
          // Before the first frame, so that an application choosing between the
          // light and the dark theme in its first `build` chooses correctly
          // rather than showing one frame of the wrong one.
          state.platform_view->SendPlatformSettings();
        }));

    [window makeKeyAndOrderFront:nil];
    [NSApp activateIgnoringOtherApps:YES];

    // Every lifecycle report is made from this thread, including this first
    // one. `windowDidBecomeKey:` has usually made it by now, in which case this
    // is a no-op.
    SendLifecycle(&state, "AppLifecycleState.resumed");

    // -- Run loop -------------------------------------------------------------

    [NSApp run];

    // The shell must be destroyed on the platform thread -- its destructor
    // checks, because it drains the UI, raster and IO threads in order and
    // would deadlock if it were not the one holding the platform thread.
    // Tearing the surface down first stops the rasterizer before the window it
    // draws into goes away.
    state.shell = nullptr;
    state.platform_view = nullptr;
    view.state = nullptr;
    delegate.state = nullptr;
    fml::AutoResetWaitableEvent latch;
    task_runners.GetPlatformTaskRunner()->PostTask(
        fml::MakeCopyable([shell = std::move(shell), &latch]() mutable {
          if (auto view = shell->GetPlatformView()) {
            view->NotifyDestroyed();
          }
          shell.reset();
          latch.Signal();
        }));
    latch.Wait();

    [window setDelegate:nil];
    [window close];
  }

  return 0;
}
