// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// The iOS host: a UIKit window, the engine's own thread model, and a real
// Shell driving the Rust framework.
//
// Structure, and why:
//
//   * UIKit owns the process's main thread. `rf_host_run` hands it over with
//     `UIApplicationMain`, which never returns -- an iOS process does not
//     outlive its application object, so there is no teardown path and none is
//     written. Everything the macOS host does after `[NSApp run]` has no
//     counterpart here.
//
//   * The shell's platform / UI / raster / IO threads come from ThreadHost,
//     exactly as on macOS and for the same reason: the main thread wants to be
//     UIKit's, and fml's Darwin message loop runs the same under both.
//
//   * Everything the view learns (size, touches, lifecycle) is posted to the
//     platform task runner; everything the raster thread produces is posted
//     back with dispatch_async to the main queue.
//
// Rendering is `GPUSurfaceSoftware` -- Skia rasterises into a bitmap and the
// view blits it in `drawRect:`, the exact arrangement the macOS host uses.
// Impeller on iOS is Metal (`GPUSurfaceMetalImpeller` over a `CAMetalLayer`),
// and the seam it would attach to is `HostPlatformView::CreateRenderingSurface`
// below.
//
// What this host does not do yet, stated rather than implied: no text input
// (the `flutter/textinput` half arrives with the UITextInput work), no
// hardware-keyboard key events, no accessibility tree, and no Impeller.

#include "flutter/rust/host/rustflutter_host.h"

#import <UIKit/UIKit.h>

#include <CoreGraphics/CoreGraphics.h>

#include <atomic>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "flutter/common/constants.h"
#include "flutter/common/settings.h"
#include "flutter/common/task_runners.h"
#include "flutter/fml/logging.h"
#include "flutter/fml/make_copyable.h"
#include "flutter/fml/message_loop.h"
#include "flutter/fml/paths.h"
#include "flutter/fml/task_runner.h"
#include "flutter/lib/ui/window/platform_message.h"
#include "flutter/lib/ui/window/pointer_data.h"
#include "flutter/lib/ui/window/pointer_data_packet.h"
#include "flutter/lib/ui/window/viewport_metrics.h"
#include "flutter/rust/ffi/rustflutter_ffi.h"
#include "flutter/rust/host/rustflutter_text_input.h"
#include "flutter/shell/common/display.h"
#include "flutter/shell/common/platform_view.h"
#include "flutter/shell/common/rasterizer.h"
#include "flutter/shell/common/run_configuration.h"
#include "flutter/shell/common/shell.h"
#include "flutter/shell/common/thread_host.h"
#include "flutter/shell/common/vsync_waiter.h"
#include "flutter/shell/gpu/gpu_surface_software.h"
#include "flutter/shell/gpu/gpu_surface_software_delegate.h"
#include "rapidjson/document.h"
#include "rapidjson/stringbuffer.h"
#include "rapidjson/writer.h"
#include "third_party/skia/include/core/SkSurface.h"

@class RfHostView;
@class RfTextInputView;

/// Raises or dismisses the software keyboard, on the main thread. Defined
/// below the input view's interface; the platform view that calls it is
/// defined above it.
static void RfSetKeyboardVisible(RfTextInputView* view, bool visible);

/// Tells the input system the framework rewrote the editing state, on the
/// main thread, so the keyboard re-reads text and selection.
static void RfNotifyFrameworkStateChanged(RfTextInputView* view);

namespace flutter {
namespace {

constexpr char kPlatformChannel[] = "flutter/platform";
constexpr char kTextInputChannel[] = "flutter/textinput";
constexpr char kLifecycleChannel[] = "flutter/lifecycle";
constexpr char kSettingsChannel[] = "flutter/settings";
constexpr char kLocalizationChannel[] = "flutter/localization";

constexpr char kClipboardError[] = "Clipboard error";
constexpr char kUnknownClipboardFormatMessage[] = "Unknown clipboard format";
constexpr char kTextPlainFormat[] = "text/plain";

//------------------------------------------------------------------------------
/// The last frame the rasterizer produced, waiting for the view to draw it.
///
/// Two threads meet here and nowhere else: the raster thread stores, the main
/// thread paints. Both under one lock, because a frame half-replaced while it
/// is being drawn is a torn frame. The macOS host's `FrameBuffer`, verbatim,
/// minus the PNG dump the simulator does not need -- `simctl io screenshot`
/// is the way to look at this host's output.
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
  /// `context` is expected to be y-down, which a UIKit `drawRect:` context is.
  /// CoreGraphics is y-up underneath that, so the transform below is what
  /// turns one into the other.
  bool Paint(CGContextRef context, CGRect bounds) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (pixels_.empty() || width_ <= 0 || height_ <= 0) {
      return false;
    }
    // BGRA is `premultipliedFirst` with little-endian words; RGBA is
    // `premultipliedLast` with big-endian ones. The surface says which and
    // `PresentBackingStore` passes the answer along.
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

 private:
  std::mutex mutex_;
  std::vector<uint8_t> pixels_;
  int32_t width_ = 0;
  int32_t height_ = 0;
  bool blue_first_ = true;
};

//------------------------------------------------------------------------------
/// How often this device's display refreshes. ProMotion reports 120; everything
/// else reports its own answer, and UIKit has known it since iOS 10.3.
double DisplayRefreshRate() {
  const double hz = static_cast<double>([UIScreen mainScreen].maximumFramesPerSecond);
  return hz > 1.0 ? hz : 60.0;
}

//------------------------------------------------------------------------------
/// A vsync waiter paced by the display rather than by a fixed sixty hertz.
///
/// The macOS host's snapped-timer waiter, with the rate read from UIKit. Not a
/// `CADisplayLink` for the same reason that host gives for not using
/// `CVDisplayLink`: the software surface presents through `drawRect:`, which
/// the window server schedules on its own terms, so the link's phase accuracy
/// has nothing to be spent on -- and the link calls back on a run loop the
/// callback would have to hop off anyway.
class VsyncWaiterIos final : public VsyncWaiter {
 public:
  explicit VsyncWaiterIos(const TaskRunners& task_runners)
      : VsyncWaiter(task_runners), phase_(fml::TimePoint::Now()) {}

  ~VsyncWaiterIos() override = default;

 private:
  static fml::TimePoint SnapToNextTick(fml::TimePoint value,
                                       fml::TimePoint phase,
                                       fml::TimeDelta interval) {
    fml::TimeDelta offset = (phase - value) % interval;
    if (offset != fml::TimeDelta::Zero()) {
      offset = offset + interval;
    }
    return value + offset;
  }

  fml::TimeDelta FrameInterval() {
    const fml::TimePoint now = fml::TimePoint::Now();
    if (interval_ != fml::TimeDelta::Zero() &&
        now - interval_read_at_ <= fml::TimeDelta::FromSeconds(1)) {
      return interval_;
    }
    const double hz = DisplayRefreshRate();
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

    std::weak_ptr<VsyncWaiterIos> weak_this =
        std::static_pointer_cast<VsyncWaiterIos>(shared_from_this());
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

  FML_DISALLOW_COPY_AND_ASSIGN(VsyncWaiterIos);
};

//------------------------------------------------------------------------------
// Platform channels. `flutter/platform` speaks JSON, as on every other host.

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

std::string ClipboardText() {
  NSString* text = [UIPasteboard generalPasteboard].string;
  return text == nil ? std::string() : std::string([text UTF8String]);
}

bool ClipboardHasText() {
  return [UIPasteboard generalPasteboard].hasStrings;
}

void SetClipboardText(const std::string& text) {
  [UIPasteboard generalPasteboard].string = @(text.c_str());
}

/// Handles one call on `flutter/platform`.
///
/// Returns the reply, or nothing for a method this host does not implement --
/// answered with an empty message, which the framework reads as "nobody
/// implements this". The set is the mobile minimum the Android host settled
/// on: the clipboard, and polite null answers for the chrome and sound calls
/// a Material widget makes on its own.
std::optional<std::string> HandlePlatformCall(const std::string& method,
                                              const rapidjson::Value* args) {
  if (method == "Clipboard.getData") {
    if (args == nullptr || !args->IsString() ||
        std::string(args->GetString()) != kTextPlainFormat) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    const std::string text = ClipboardText();
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
    if (args != nullptr && args->IsString() && std::string(args->GetString()) != kTextPlainFormat) {
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

  if (method == "SystemSound.play" || method == "HapticFeedback.vibrate" ||
      method.rfind("SystemChrome.", 0) == 0) {
    // Heard and acknowledged: a widget that asked for a click or a status-bar
    // style must not read silence as "channel unserved".
    return NullEnvelope();
  }

  if (method == "SystemNavigator.pop") {
    // An iOS application does not exit itself; the home gesture does. Upstream
    // answers the same way: acknowledged, nothing done.
    return NullEnvelope();
  }

  return std::nullopt;
}

//------------------------------------------------------------------------------
/// What the view, the delegate and the shell share.
struct WindowState {
  Shell* shell = nullptr;
  class HostPlatformView* platform_view = nullptr;
  fml::RefPtr<fml::TaskRunner> platform_task_runner;
  FrameBuffer frame_buffer;
  double device_pixel_ratio = 1.0;
  int32_t physical_width = 0;
  int32_t physical_height = 0;
  /// The safe area, in physical pixels, refreshed by the view controller.
  double padding_top = 0.0;
  double padding_left = 0.0;
  double padding_right = 0.0;
  double padding_bottom = 0.0;
  std::string lifecycle_state;
  /// The keyboard's overlap with the view, in physical pixels.
  double view_inset_bottom = 0.0;
  /// Where each touch was last seen, for the delta a move carries. Keyed by
  /// the touch's identity, which is the UITouch pointer value.
  std::map<int64_t, std::pair<double, double>> last_positions;
  /// The platform half of `flutter/textinput`: the editing model typing is
  /// applied to. Channel calls reach it on the platform thread, keyboard
  /// events on the main thread; it locks internally.
  TextInputHandler text_input;
  RfHostView* view = nil;
  RfTextInputView* input_view = nil;
};

/// The one state this process has. UIKit instantiates the app delegate by
/// class name, so there is no constructor argument to pass it through; a
/// file-scope instance is how the two sides meet.
WindowState* GlobalState() {
  static WindowState state;
  return &state;
}

/// The options `rf_host_run` was called with, held for the delegate that
/// UIApplicationMain will create. The strings are copied: the caller's
/// pointers do not outlive the call.
struct HeldOptions {
  bool enable_impeller = false;
  std::string icu_data_path;
};

HeldOptions* GlobalOptions() {
  static HeldOptions options;
  return &options;
}

std::string DefaultIcuDataPath() {
  auto directory = fml::paths::GetExecutableDirectoryPath();
  if (!directory.first) {
    return "";
  }
  return fml::paths::JoinPaths({directory.second, "icudtl.dat"});
}

//------------------------------------------------------------------------------
// Settings and locales.

bool AlwaysUse24HourFormat() {
  NSString* format = [NSDateFormatter dateFormatFromTemplate:@"j"
                                                     options:0
                                                      locale:[NSLocale currentLocale]];
  return format != nil && [format rangeOfString:@"a"].location == NSNotFound;
}

bool PrefersDarkTheme() {
  return UITraitCollection.currentTraitCollection.userInterfaceStyle == UIUserInterfaceStyleDark;
}

std::string SettingsPayload() {
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartObject();
  writer.Key("alwaysUse24HourFormat");
  writer.Bool(AlwaysUse24HourFormat());
  // UIKit's Dynamic Type would be the real answer; reading it is the
  // accessibility round's work. One is honest until then.
  writer.Key("textScaleFactor");
  writer.Double(1.0);
  writer.Key("platformBrightness");
  writer.String(PrefersDarkTheme() ? "dark" : "light");
  writer.EndObject();
  return buffer.GetString();
}

/// The `flutter/localization` payload, the same four strings per locale the
/// macOS host sends -- both read NSLocale, which is Foundation on either.
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
    writer.String(language == nil ? "" : [language UTF8String]);
    writer.String(country == nil ? "" : [country UTF8String]);
    writer.String(script == nil ? "" : [script UTF8String]);
    writer.String(variant == nil ? "" : [variant UTF8String]);
  }
  writer.EndArray();
  writer.EndObject();
  return buffer.GetString();
}

//------------------------------------------------------------------------------
/// The platform view: the shell's window onto this host.
class HostPlatformView final : public PlatformView, public GPUSurfaceSoftwareDelegate {
 public:
  HostPlatformView(PlatformView::Delegate& delegate,
                   const TaskRunners& task_runners,
                   WindowState* state)
      : PlatformView(delegate, task_runners), state_(state) {}

  ~HostPlatformView() override = default;

  // |PlatformView|
  std::unique_ptr<Surface> CreateRenderingSurface() override {
    // Where a Metal/Impeller surface would attach. The software surface needs
    // nothing from the platform.
    return std::make_unique<GPUSurfaceSoftware>(this, /*render_to_surface=*/true);
  }

  // |PlatformView|
  std::unique_ptr<VsyncWaiter> CreateVSyncWaiter() override {
    return std::make_unique<VsyncWaiterIos>(task_runners_);
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
  bool PresentBackingStore(sk_sp<SkSurface> backing_store) override {
    if (backing_store == nullptr || state_ == nullptr) {
      return false;
    }
    SkPixmap pixmap;
    if (!backing_store->peekPixels(&pixmap)) {
      return false;
    }
    const bool blue_first = pixmap.colorType() == kBGRA_8888_SkColorType;
    state_->frame_buffer.Store(pixmap.addr(), pixmap.width(), pixmap.height(), blue_first);
    RfHostView* view = state_->view;
    dispatch_async(dispatch_get_main_queue(), ^{
      [view setNeedsDisplay];
    });
    return true;
  }

  /// Sends one pointer event to the engine. Called from the main thread; the
  /// pointer dispatcher expects the platform thread.
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

  void SendPlatformSettings() {
    SendOnChannel(kSettingsChannel, SettingsPayload());
    if (auto locales = LocalizationPayload()) {
      SendOnChannel(kLocalizationChannel, *locales);
    }
  }

  void SendLifecycleState(const char* state) {
    SendOnChannel(kLifecycleChannel, std::string(state));
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

  // |PlatformView|
  void HandlePlatformMessage(std::unique_ptr<PlatformMessage> message) override {
    const auto& data = message->data();
    std::optional<std::vector<uint8_t>> reply;

    if (message->channel() == kPlatformChannel || message->channel() == kTextInputChannel) {
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
                                : HandlePlatformCall(method_name, arguments);
          if (answer) {
            reply = std::vector<uint8_t>(answer->begin(), answer->end());
          }
          if (editing) {
            // The keyboard itself is the host's business: the handler owns
            // the text, this owns the first responder.
            if (method_name == "TextInput.show") {
              RfSetKeyboardVisible(state_->input_view, true);
            } else if (method_name == "TextInput.hide" || method_name == "TextInput.clearClient") {
              RfSetKeyboardVisible(state_->input_view, false);
            }
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

void SendViewportMetrics(WindowState* state) {
  if (state->shell == nullptr || state->physical_width <= 0 || state->physical_height <= 0) {
    return;
  }
  ViewportMetrics metrics;
  metrics.device_pixel_ratio = state->device_pixel_ratio;
  metrics.physical_width = state->physical_width;
  metrics.physical_height = state->physical_height;
  metrics.physical_max_width_constraint = state->physical_width;
  metrics.physical_max_height_constraint = state->physical_height;
  // The safe area: the notch, the home indicator, the status bar. The
  // framework's SafeArea reads these as its padding.
  metrics.physical_padding_top = state->padding_top;
  metrics.physical_padding_left = state->padding_left;
  metrics.physical_padding_right = state->padding_right;
  metrics.physical_padding_bottom = state->padding_bottom;
  // The keyboard, which the framework reads as `viewInsets` -- what a
  // scrollable page adds at the bottom so the focused field can be revealed
  // above it.
  metrics.physical_view_inset_bottom = state->view_inset_bottom;

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

/// What outlives `didFinishLaunching`: the threads and the shell. Never torn
/// down, because an iOS process is ended by the system, not by a return.
struct RunningShell {
  std::unique_ptr<ThreadHost> thread_host;
  std::unique_ptr<Shell> shell;
};

RunningShell* GlobalShell() {
  static RunningShell running;
  return &running;
}

}  // namespace
}  // namespace flutter

//------------------------------------------------------------------------------
// The view.

@interface RfHostView : UIView
@property(nonatomic, assign) flutter::WindowState* state;
@end

@implementation RfHostView

- (instancetype)initWithFrame:(CGRect)frame {
  self = [super initWithFrame:frame];
  if (self) {
    self.backgroundColor = [UIColor blackColor];
    self.multipleTouchEnabled = YES;
    // Redraw on demand: `setNeedsDisplay` from the raster thread's present is
    // what schedules a draw, and the view must not clear itself in between.
    self.contentMode = UIViewContentModeRedraw;
  }
  return self;
}

- (void)drawRect:(CGRect)rect {
  CGContextRef context = UIGraphicsGetCurrentContext();
  if (context == nullptr || _state == nullptr) {
    return;
  }
  _state->frame_buffer.Paint(context, self.bounds);
}

// -- touches ------------------------------------------------------------------
//
// Upstream's `FlutterViewController` shape: the touch's own identity is the
// device (`reinterpret_cast` of the pointer, which UIKit keeps stable for the
// touch's whole life), the position is in points scaled to physical pixels,
// and an ended or cancelled touch is followed by a synthesized Remove so the
// framework forgets the pointer. The delta bookkeeping is the Android host's.

- (void)dispatchTouches:(NSSet<UITouch*>*)touches change:(flutter::PointerData::Change)change {
  if (_state == nullptr || _state->platform_view == nullptr) {
    return;
  }
  const double scale = self.contentScaleFactor > 0 ? self.contentScaleFactor : 1.0;
  for (UITouch* touch in touches) {
    const CGPoint point = [touch locationInView:self];
    const double x = point.x * scale;
    const double y = point.y * scale;
    const int64_t device = reinterpret_cast<int64_t>(touch);

    flutter::PointerData data;
    data.Clear();
    data.time_stamp = fml::TimePoint::Now().ToEpochDelta().ToMicroseconds();
    data.change = change;
    data.kind = flutter::PointerData::DeviceKind::kTouch;
    data.signal_kind = flutter::PointerData::SignalKind::kNone;
    data.device = device;
    data.pointer_identifier = 0;
    data.physical_x = x;
    data.physical_y = y;

    auto previous = _state->last_positions.find(device);
    if (change == flutter::PointerData::Change::kMove && previous != _state->last_positions.end()) {
      data.physical_delta_x = x - previous->second.first;
      data.physical_delta_y = y - previous->second.second;
    }

    const bool down = change == flutter::PointerData::Change::kDown ||
                      change == flutter::PointerData::Change::kMove;
    data.buttons = down ? flutter::kPointerButtonTouchContact : 0;
    data.pressure = down ? 1.0 : 0.0;
    data.pressure_max = 1.0;
    data.view_id = flutter::kFlutterImplicitViewId;
    _state->platform_view->SendPointer(data);

    if (change == flutter::PointerData::Change::kUp ||
        change == flutter::PointerData::Change::kCancel) {
      // The pointer is gone; without the Remove the framework keeps hover
      // state for a finger that no longer exists. Upstream synthesizes the
      // same event in the same place.
      _state->last_positions.erase(device);
      flutter::PointerData remove = data;
      remove.change = flutter::PointerData::Change::kRemove;
      remove.buttons = 0;
      remove.pressure = 0.0;
      _state->platform_view->SendPointer(remove);
    } else {
      _state->last_positions[device] = {x, y};
    }
  }
}

- (void)touchesBegan:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event {
  [self dispatchTouches:touches change:flutter::PointerData::Change::kDown];
}

- (void)touchesMoved:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event {
  [self dispatchTouches:touches change:flutter::PointerData::Change::kMove];
}

- (void)touchesEnded:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event {
  [self dispatchTouches:touches change:flutter::PointerData::Change::kUp];
}

- (void)touchesCancelled:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event {
  [self dispatchTouches:touches change:flutter::PointerData::Change::kCancel];
}

@end

//------------------------------------------------------------------------------
// Text input.
//
// Upstream's `FlutterTextInputPlugin` shape: a hidden view that adopts the
// whole of `UITextInput` over the editing model, becomes first responder when
// the framework says `TextInput.show`, and resigns when it says hide. The
// system keyboard -- and any input method riding it, pinyin included -- talks
// to this view; every edit it makes lands in the shared `TextInputHandler`
// and returns to the framework as `TextInputClient.updateEditingState`.
//
// Positions and ranges are indices into the model's text, in UTF-16 units,
// which are `NSRange`'s units too. The heavyweight parts of upstream's
// implementation -- per-character selection rects, the floating cursor,
// scribble, dictation placeholders -- are not here, and text fields work
// without them; they are the accessibility round's work.

@interface RfTextPosition : UITextPosition
@property(nonatomic, assign) NSUInteger index;
+ (instancetype)positionWithIndex:(NSUInteger)index;
@end

@implementation RfTextPosition
+ (instancetype)positionWithIndex:(NSUInteger)index {
  // Autoreleased: manual reference counting, and the keyboard asks for
  // positions constantly.
  RfTextPosition* position = [[[RfTextPosition alloc] init] autorelease];
  position.index = index;
  return position;
}
@end

@interface RfTextRange : UITextRange
@property(nonatomic, assign) NSRange range;
+ (instancetype)rangeWithNSRange:(NSRange)range;
@end

@implementation RfTextRange
+ (instancetype)rangeWithNSRange:(NSRange)range {
  RfTextRange* wrapped = [[[RfTextRange alloc] init] autorelease];
  wrapped.range = range;
  return wrapped;
}
- (UITextPosition*)start {
  return [RfTextPosition positionWithIndex:self.range.location];
}
- (UITextPosition*)end {
  return [RfTextPosition positionWithIndex:NSMaxRange(self.range)];
}
- (BOOL)isEmpty {
  return self.range.length == 0;
}
@end

@interface RfTextInputView : UIView <UITextInput>
@property(nonatomic, assign) flutter::WindowState* state;
// `assign` rather than `weak`: this file compiles under manual reference
// counting, where a synthesized weak is a compile error. The delegate is
// UIKit's keyboard machinery, which outlives any moment it is used.
@property(nonatomic, assign) id<UITextInputDelegate> inputDelegate;
@property(nonatomic, strong) UITextInputStringTokenizer* tokenizer;
@end

@implementation RfTextInputView

- (instancetype)initWithFrame:(CGRect)frame {
  self = [super initWithFrame:frame];
  if (self) {
    self.tokenizer = [[[UITextInputStringTokenizer alloc] initWithTextInput:self] autorelease];
    self.hidden = YES;
  }
  return self;
}

- (BOOL)canBecomeFirstResponder {
  return YES;
}

/// The whole text as the keyboard should see it, UTF-16 indexed the way the
/// model's ranges are.
- (NSString*)wholeText {
  if (_state == nullptr) {
    return @"";
  }
  const std::string text = _state->text_input.GetText();
  NSString* result = [NSString stringWithUTF8String:text.c_str()];
  return result == nil ? @"" : result;
}

// -- UIKeyInput ---------------------------------------------------------------

- (BOOL)hasText {
  return [self wholeText].length > 0;
}

- (void)insertText:(NSString*)text {
  if (_state == nullptr || text == nil) {
    return;
  }
  // Return in a single-line field is the action, not a character. A multiline
  // field takes the newline through `OnAction` too, which inserts it and
  // reports the action, upstream's `EnterPressed`.
  if ([text isEqualToString:@"\n"]) {
    _state->text_input.OnAction();
    return;
  }
  std::u16string units;
  units.reserve(text.length);
  for (NSUInteger i = 0; i < text.length; i++) {
    units.push_back([text characterAtIndex:i]);
  }
  _state->text_input.OnInsertText(units);
}

- (void)deleteBackward {
  if (_state != nullptr) {
    _state->text_input.OnDeleteBackward();
  }
}

// -- UITextInput: text and ranges ----------------------------------------------

- (NSString*)textInRange:(UITextRange*)range {
  if (![range isKindOfClass:[RfTextRange class]]) {
    return nil;
  }
  NSString* text = [self wholeText];
  NSRange wanted = ((RfTextRange*)range).range;
  if (wanted.location > text.length) {
    return nil;
  }
  wanted.length = MIN(wanted.length, text.length - wanted.location);
  return [text substringWithRange:wanted];
}

- (void)replaceRange:(UITextRange*)range withText:(NSString*)text {
  if (_state == nullptr || ![range isKindOfClass:[RfTextRange class]] || text == nil) {
    return;
  }
  const NSRange target = ((RfTextRange*)range).range;
  std::u16string units;
  units.reserve(text.length);
  for (NSUInteger i = 0; i < text.length; i++) {
    units.push_back([text characterAtIndex:i]);
  }
  _state->text_input.OnReplaceRange(static_cast<long>(target.location),
                                    static_cast<long>(target.length), units);
}

- (UITextRange*)selectedTextRange {
  if (_state == nullptr) {
    return nil;
  }
  long location = -1;
  long length = 0;
  _state->text_input.GetSelectedRange(&location, &length);
  if (location < 0) {
    return nil;
  }
  return [RfTextRange rangeWithNSRange:NSMakeRange(static_cast<NSUInteger>(location),
                                                   static_cast<NSUInteger>(length))];
}

- (void)setSelectedTextRange:(UITextRange*)range {
  if (_state == nullptr || ![range isKindOfClass:[RfTextRange class]]) {
    return;
  }
  const NSRange target = ((RfTextRange*)range).range;
  _state->text_input.OnSetSelection(static_cast<long>(target.location),
                                    static_cast<long>(NSMaxRange(target)));
}

- (UITextRange*)markedTextRange {
  if (_state == nullptr) {
    return nil;
  }
  long location = -1;
  long length = 0;
  _state->text_input.GetMarkedRange(&location, &length);
  if (location < 0) {
    return nil;
  }
  return [RfTextRange rangeWithNSRange:NSMakeRange(static_cast<NSUInteger>(location),
                                                   static_cast<NSUInteger>(length))];
}

- (NSDictionary<NSAttributedStringKey, id>*)markedTextStyle {
  return nil;
}

- (void)setMarkedTextStyle:(NSDictionary<NSAttributedStringKey, id>*)style {
}

- (void)setMarkedText:(NSString*)markedText selectedRange:(NSRange)selectedRange {
  if (_state == nullptr) {
    return;
  }
  NSString* text = markedText == nil ? @"" : markedText;
  std::u16string units;
  units.reserve(text.length);
  for (NSUInteger i = 0; i < text.length; i++) {
    units.push_back([text characterAtIndex:i]);
  }
  _state->text_input.OnSetMarkedText(units, static_cast<long>(selectedRange.location),
                                     static_cast<long>(selectedRange.length));
}

- (void)unmarkText {
  if (_state != nullptr) {
    _state->text_input.OnUnmarkText();
  }
}

// -- UITextInput: positions -----------------------------------------------------

- (UITextPosition*)beginningOfDocument {
  return [RfTextPosition positionWithIndex:0];
}

- (UITextPosition*)endOfDocument {
  return [RfTextPosition positionWithIndex:[self wholeText].length];
}

- (UITextPosition*)positionFromPosition:(UITextPosition*)position offset:(NSInteger)offset {
  if (![position isKindOfClass:[RfTextPosition class]]) {
    return nil;
  }
  const NSInteger index = static_cast<NSInteger>(((RfTextPosition*)position).index) + offset;
  const NSInteger length = static_cast<NSInteger>([self wholeText].length);
  if (index < 0 || index > length) {
    return nil;
  }
  return [RfTextPosition positionWithIndex:static_cast<NSUInteger>(index)];
}

- (UITextPosition*)positionFromPosition:(UITextPosition*)position
                            inDirection:(UITextLayoutDirection)direction
                                 offset:(NSInteger)offset {
  // One line of text as far as this view knows, so vertical motion pins to
  // the ends and horizontal motion is the plain offset.
  switch (direction) {
    case UITextLayoutDirectionLeft:
      return [self positionFromPosition:position offset:-offset];
    case UITextLayoutDirectionRight:
      return [self positionFromPosition:position offset:offset];
    case UITextLayoutDirectionUp:
      return [self beginningOfDocument];
    case UITextLayoutDirectionDown:
      return [self endOfDocument];
  }
  return nil;
}

- (UITextRange*)textRangeFromPosition:(UITextPosition*)fromPosition
                           toPosition:(UITextPosition*)toPosition {
  if (![fromPosition isKindOfClass:[RfTextPosition class]] ||
      ![toPosition isKindOfClass:[RfTextPosition class]]) {
    return nil;
  }
  const NSUInteger a = ((RfTextPosition*)fromPosition).index;
  const NSUInteger b = ((RfTextPosition*)toPosition).index;
  return [RfTextRange rangeWithNSRange:NSMakeRange(MIN(a, b), MAX(a, b) - MIN(a, b))];
}

- (NSComparisonResult)comparePosition:(UITextPosition*)position toPosition:(UITextPosition*)other {
  if (![position isKindOfClass:[RfTextPosition class]] ||
      ![other isKindOfClass:[RfTextPosition class]]) {
    return NSOrderedSame;
  }
  const NSUInteger a = ((RfTextPosition*)position).index;
  const NSUInteger b = ((RfTextPosition*)other).index;
  if (a < b) {
    return NSOrderedAscending;
  }
  if (a > b) {
    return NSOrderedDescending;
  }
  return NSOrderedSame;
}

- (NSInteger)offsetFromPosition:(UITextPosition*)from toPosition:(UITextPosition*)toPosition {
  if (![from isKindOfClass:[RfTextPosition class]] ||
      ![toPosition isKindOfClass:[RfTextPosition class]]) {
    return 0;
  }
  return static_cast<NSInteger>(((RfTextPosition*)toPosition).index) -
         static_cast<NSInteger>(((RfTextPosition*)from).index);
}

- (UITextPosition*)positionWithinRange:(UITextRange*)range
                   farthestInDirection:(UITextLayoutDirection)direction {
  if (![range isKindOfClass:[RfTextRange class]]) {
    return nil;
  }
  const NSRange wrapped = ((RfTextRange*)range).range;
  const bool forward =
      direction == UITextLayoutDirectionRight || direction == UITextLayoutDirectionDown;
  return [RfTextPosition positionWithIndex:forward ? NSMaxRange(wrapped) : wrapped.location];
}

- (UITextRange*)characterRangeByExtendingPosition:(UITextPosition*)position
                                      inDirection:(UITextLayoutDirection)direction {
  if (![position isKindOfClass:[RfTextPosition class]]) {
    return nil;
  }
  const NSUInteger index = ((RfTextPosition*)position).index;
  const bool forward =
      direction == UITextLayoutDirectionRight || direction == UITextLayoutDirectionDown;
  if (forward) {
    const NSUInteger length = [self wholeText].length;
    return [RfTextRange rangeWithNSRange:NSMakeRange(index, index < length ? 1 : 0)];
  }
  return [RfTextRange rangeWithNSRange:NSMakeRange(index > 0 ? index - 1 : 0, index > 0 ? 1 : 0)];
}

- (NSWritingDirection)baseWritingDirectionForPosition:(UITextPosition*)position
                                          inDirection:(UITextStorageDirection)direction {
  return NSWritingDirectionLeftToRight;
}

- (void)setBaseWritingDirection:(NSWritingDirection)writingDirection forRange:(UITextRange*)range {
}

// -- UITextInput: geometry -------------------------------------------------------
//
// The keyboard asks where text is to place its own chrome. The one rectangle
// the framework reports is the caret's; every question is answered with it,
// which is enough for the keyboard, the magnifier and the candidate bar to
// stay near the field.

- (CGRect)caretRect {
  if (_state == nullptr) {
    return CGRectZero;
  }
  double x = 0;
  double y = 0;
  double width = 0;
  double height = 0;
  if (!_state->text_input.GetCaretRect(&x, &y, &width, &height)) {
    return CGRectZero;
  }
  return CGRectMake(x, y, width, height);
}

- (CGRect)firstRectForRange:(UITextRange*)range {
  return [self caretRect];
}

- (CGRect)caretRectForPosition:(UITextPosition*)position {
  return [self caretRect];
}

- (NSArray<UITextSelectionRect*>*)selectionRectsForRange:(UITextRange*)range {
  return @[];
}

- (UITextPosition*)closestPositionToPoint:(CGPoint)point {
  return [self endOfDocument];
}

- (UITextPosition*)closestPositionToPoint:(CGPoint)point withinRange:(UITextRange*)range {
  if ([range isKindOfClass:[RfTextRange class]]) {
    return [RfTextPosition positionWithIndex:NSMaxRange(((RfTextRange*)range).range)];
  }
  return [self endOfDocument];
}

- (UITextRange*)characterRangeAtPoint:(CGPoint)point {
  return nil;
}

- (UIView*)textInputView {
  return self;
}

// -- UITextInputTraits ------------------------------------------------------------

- (UIKeyboardType)keyboardType {
  if (_state == nullptr) {
    return UIKeyboardTypeDefault;
  }
  const std::string type = _state->text_input.input_type();
  if (type == "TextInputType.emailAddress") {
    return UIKeyboardTypeEmailAddress;
  }
  if (type == "TextInputType.number") {
    return UIKeyboardTypeDecimalPad;
  }
  if (type == "TextInputType.phone") {
    return UIKeyboardTypePhonePad;
  }
  if (type == "TextInputType.url") {
    return UIKeyboardTypeURL;
  }
  return UIKeyboardTypeDefault;
}

- (UIReturnKeyType)returnKeyType {
  if (_state == nullptr) {
    return UIReturnKeyDefault;
  }
  const std::string action = _state->text_input.input_action();
  if (action == "TextInputAction.done") {
    return UIReturnKeyDone;
  }
  if (action == "TextInputAction.go") {
    return UIReturnKeyGo;
  }
  if (action == "TextInputAction.search") {
    return UIReturnKeySearch;
  }
  if (action == "TextInputAction.send") {
    return UIReturnKeySend;
  }
  if (action == "TextInputAction.next") {
    return UIReturnKeyNext;
  }
  return UIReturnKeyDefault;
}

- (BOOL)isSecureTextEntry {
  return _state != nullptr && _state->text_input.obscure_text();
}

- (UITextAutocorrectionType)autocorrectionType {
  if (_state != nullptr && !_state->text_input.autocorrect()) {
    return UITextAutocorrectionTypeNo;
  }
  return UITextAutocorrectionTypeDefault;
}

- (UITextAutocapitalizationType)autocapitalizationType {
  return UITextAutocapitalizationTypeNone;
}

@end

static void RfSetKeyboardVisible(RfTextInputView* view, bool visible) {
  dispatch_async(dispatch_get_main_queue(), ^{
    if (visible) {
      // The traits are read when the responder arrives, so a field that
      // changed the keyboard type gets it re-read.
      [view reloadInputViews];
      [view becomeFirstResponder];
    } else {
      [view resignFirstResponder];
    }
  });
}

static void RfNotifyFrameworkStateChanged(RfTextInputView* view) {
  dispatch_async(dispatch_get_main_queue(), ^{
    // The keyboard keeps its own idea of the text -- autocorrect state, the
    // shift key, the candidate bar. This is how it is told to re-read.
    id<UITextInputDelegate> delegate = view.inputDelegate;
    [delegate selectionWillChange:view];
    [delegate textWillChange:view];
    [delegate textDidChange:view];
    [delegate selectionDidChange:view];
  });
}

//------------------------------------------------------------------------------
// The view controller: geometry, and where the safe area is learned.

@interface RfHostViewController : UIViewController
@property(nonatomic, assign) flutter::WindowState* state;
@end

@implementation RfHostViewController

- (void)loadView {
  RfHostView* view = [[RfHostView alloc] initWithFrame:[UIScreen mainScreen].bounds];
  view.state = self.state;
  self.view = view;
}

/// Reads the view's geometry into the shared state and reports it. Called
/// whenever UIKit says something moved: the first layout, a rotation, the
/// safe area resolving after attach.
- (void)updateMetrics {
  flutter::WindowState* state = self.state;
  if (state == nullptr) {
    return;
  }
  UIView* view = self.view;
  const double scale = view.contentScaleFactor > 0 ? view.contentScaleFactor : 1.0;
  state->device_pixel_ratio = scale;
  state->physical_width = static_cast<int32_t>(view.bounds.size.width * scale);
  state->physical_height = static_cast<int32_t>(view.bounds.size.height * scale);
  const UIEdgeInsets safe = view.safeAreaInsets;
  state->padding_top = safe.top * scale;
  state->padding_left = safe.left * scale;
  state->padding_right = safe.right * scale;
  state->padding_bottom = safe.bottom * scale;
  flutter::SendViewportMetrics(state);
}

- (void)viewDidLayoutSubviews {
  [super viewDidLayoutSubviews];
  [self updateMetrics];
}

- (void)viewSafeAreaInsetsDidChange {
  [super viewSafeAreaInsetsDidChange];
  [self updateMetrics];
}

- (BOOL)prefersStatusBarHidden {
  return NO;
}

@end

//------------------------------------------------------------------------------
// The application delegate: where the shell is born.

@interface RfAppDelegate : UIResponder <UIApplicationDelegate>
@property(nonatomic, strong) UIWindow* window;
@end

@implementation RfAppDelegate

- (BOOL)application:(UIApplication*)application
    didFinishLaunchingWithOptions:(NSDictionary*)launchOptions {
  using namespace flutter;  // NOLINT(build/namespaces)

  WindowState* state = GlobalState();
  HeldOptions* options = GlobalOptions();

  // -- Window (main thread) ---------------------------------------------------

  self.window = [[UIWindow alloc] initWithFrame:[UIScreen mainScreen].bounds];
  RfHostViewController* controller = [[RfHostViewController alloc] init];
  controller.state = state;
  self.window.rootViewController = controller;
  [self.window makeKeyAndVisible];

  RfHostView* view = (RfHostView*)controller.view;
  state->view = view;

  // The hidden view the keyboard talks to. A subview so UIKit lets it become
  // first responder; hidden because the framework draws the text itself.
  RfTextInputView* input_view = [[RfTextInputView alloc] initWithFrame:CGRectZero];
  input_view.state = state;
  [view addSubview:input_view];
  state->input_view = input_view;
  state->text_input.SetOnFrameworkStateChanged(
      [input_view]() { RfNotifyFrameworkStateChanged(input_view); });

  // The keyboard's overlap with the view is the framework's `viewInsets`:
  // what a page scrolls a focused field up above.
  [[NSNotificationCenter defaultCenter]
      addObserverForName:UIKeyboardWillChangeFrameNotification
                  object:nil
                   queue:[NSOperationQueue mainQueue]
              usingBlock:^(NSNotification* note) {
                NSValue* frame = note.userInfo[UIKeyboardFrameEndUserInfoKey];
                if (frame == nil || state->view == nil) {
                  return;
                }
                UIView* host_view = state->view;
                const CGRect keyboard = [host_view convertRect:frame.CGRectValue fromView:nil];
                const CGFloat overlap =
                    MAX(0.0, CGRectGetMaxY(host_view.bounds) - CGRectGetMinY(keyboard));
                const double scale =
                    host_view.contentScaleFactor > 0 ? host_view.contentScaleFactor : 1.0;
                state->view_inset_bottom = overlap * scale;
                SendViewportMetrics(state);
              }];
  [[NSNotificationCenter defaultCenter] addObserverForName:UIKeyboardWillHideNotification
                                                    object:nil
                                                     queue:[NSOperationQueue mainQueue]
                                                usingBlock:^(NSNotification* note) {
                                                  state->view_inset_bottom = 0.0;
                                                  SendViewportMetrics(state);
                                                }];
  const double scale = view.contentScaleFactor > 0 ? view.contentScaleFactor : 1.0;
  state->device_pixel_ratio = scale;
  state->physical_width = static_cast<int32_t>(view.bounds.size.width * scale);
  state->physical_height = static_cast<int32_t>(view.bounds.size.height * scale);

  // -- Settings ---------------------------------------------------------------

  Settings settings;
  if (options->enable_impeller) {
    FML_LOG(IMPORTANT) << "Impeller was requested; this host renders with the Skia software "
                          "surface. See rustflutter_host_ios.mm.";
  }
  settings.enable_impeller = false;
  settings.enable_software_rendering = true;
  settings.icu_initialization_required = true;
  settings.icu_data_path =
      !options->icu_data_path.empty() ? options->icu_data_path : DefaultIcuDataPath();
  settings.warn_on_impeller_opt_out = false;

  // Text and images are recorded for the backend that will draw them, and
  // this is the software one.
  rf_set_impeller_backend(0);

  // -- Threads ----------------------------------------------------------------

  RunningShell* running = GlobalShell();
  running->thread_host =
      std::make_unique<ThreadHost>("rf", ThreadHost::Type::kPlatform | ThreadHost::Type::kUi |
                                             ThreadHost::Type::kRaster | ThreadHost::Type::kIo);

  TaskRunners task_runners("rustflutter", running->thread_host->platform_thread->GetTaskRunner(),
                           running->thread_host->raster_thread->GetTaskRunner(),
                           running->thread_host->ui_thread->GetTaskRunner(),
                           running->thread_host->io_thread->GetTaskRunner());

  // -- Shell ------------------------------------------------------------------

  PlatformData platform_data;
  running->shell = Shell::Create(
      platform_data, task_runners, settings,
      [state](Shell& shell) {
        auto view = std::make_unique<HostPlatformView>(shell, shell.GetTaskRunners(), state);
        state->platform_view = view.get();
        state->text_input.SetSender(
            [sender = view.get()](const std::string& method, const std::string& arguments) {
              sender->SendMethodCall(kTextInputChannel, method, arguments);
            });
        return view;
      },
      [](Shell& shell) { return std::make_unique<Rasterizer>(shell); });

  if (running->shell == nullptr || !running->shell->IsSetup()) {
    FML_LOG(ERROR) << "The shell could not be set up.";
    return NO;
  }
  state->shell = running->shell.get();
  state->platform_task_runner = task_runners.GetPlatformTaskRunner();

  task_runners.GetPlatformTaskRunner()->PostTask(
      fml::MakeCopyable([shell = running->shell.get(), state]() mutable {
        shell->RunEngine(RunConfiguration{});
        if (auto view = shell->GetPlatformView()) {
          view->NotifyCreated();
        }
        std::vector<std::unique_ptr<Display>> displays;
        displays.push_back(std::make_unique<Display>(
            /*display_id=*/0, DisplayRefreshRate(), state->physical_width, state->physical_height,
            state->device_pixel_ratio));
        shell->OnDisplayUpdates(std::move(displays));
        SendViewportMetrics(state);
        // Before the first frame, so an application choosing between the
        // light and the dark theme in its first build chooses correctly.
        state->platform_view->SendPlatformSettings();
      }));

  return YES;
}

// -- lifecycle ----------------------------------------------------------------
//
// Upstream `FlutterViewController`'s mapping: active is resumed, resigning is
// inactive, background is paused, returning to the foreground is inactive
// until the activation lands, and termination is detached.

- (void)applicationDidBecomeActive:(UIApplication*)application {
  flutter::SendLifecycle(flutter::GlobalState(), "AppLifecycleState.resumed");
}

- (void)applicationWillResignActive:(UIApplication*)application {
  flutter::SendLifecycle(flutter::GlobalState(), "AppLifecycleState.inactive");
}

- (void)applicationDidEnterBackground:(UIApplication*)application {
  flutter::SendLifecycle(flutter::GlobalState(), "AppLifecycleState.paused");
}

- (void)applicationWillEnterForeground:(UIApplication*)application {
  flutter::SendLifecycle(flutter::GlobalState(), "AppLifecycleState.inactive");
}

- (void)applicationWillTerminate:(UIApplication*)application {
  flutter::SendLifecycle(flutter::GlobalState(), "AppLifecycleState.detached");
}

@end

//------------------------------------------------------------------------------
// The entry point.

int32_t rf_host_run(const RfHostOptions* options) {
  // The width, height and title are the desktop hosts' vocabulary; a phone's
  // window is the screen and its title is nowhere. They are accepted and
  // ignored rather than rejected, so one application runs everywhere.
  flutter::HeldOptions* held = flutter::GlobalOptions();
  if (options != nullptr) {
    held->enable_impeller = options->enable_impeller != 0;
    if (options->icu_data_path != nullptr) {
      held->icu_data_path = options->icu_data_path;
    }
  }

  @autoreleasepool {
    static char process_name[] = "rustflutter";
    static char* fake_argv[] = {process_name, nullptr};
    // Never returns: UIKit owns the process from here, and the delegate above
    // builds the shell once the application has finished launching.
    return UIApplicationMain(1, fake_argv, nil, @"RfAppDelegate");
  }
}
