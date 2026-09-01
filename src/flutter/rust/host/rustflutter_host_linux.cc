// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// The Linux host: a GTK3 window, the engine's own thread model, and a real
// Shell driving the Rust framework.
//
// Structure, and why:
//
//   * The window lives on the process's main thread and owns the GLib main
//     loop, because GTK refuses to be driven from anywhere else -- every
//     gtk_* and gdk_* call below is a main-thread call.
//
//   * The shell's platform / UI / raster / IO threads come from ThreadHost, so
//     they are the same fml threads the engine uses everywhere else. The window
//     thread is deliberately *not* the platform thread, for the reason the
//     Windows host gives: making it so would mean interleaving fml::MessageLoop
//     with the GLib main loop, and the two want to own the same thread.
//
//   * Everything the window learns (size, close, input) is posted to the
//     platform task runner; everything the raster thread produces is posted
//     back with g_idle_add or g_main_context_invoke, which are the two GLib
//     calls documented safe from any thread. Neither side touches the other's
//     state directly.
//
// Rendering is Impeller when asked for -- on Vulkan when
// RUSTFLUTTER_BACKEND=vulkan, otherwise over Mesa's EGL, straight onto the
// drawing area's X11 window, which is why the GDK backend is pinned to X11
// below -- and the Skia software surface otherwise, blitted through cairo in
// the `draw` signal. The two attach at the same seam they do on Windows:
// `HostPlatformView::CreateRenderingSurface`.
//
// What this host does not do yet, stated rather than implied: no
// accessibility tree, and no Wayland (the EGL surface needs an X window; WSLg
// and every desktop provide one).

#include "flutter/rust/host/rustflutter_host.h"

#include <gdk/gdkkeysyms.h>
#include <gdk/gdkx.h>
#include <gtk/gtk.h>

// Xlib, which gdkx.h drags in, leaks single-word macros that collide with
// method names below -- rapidjson's writer.Bool() most of all. The X types
// (Window, XID) are typedefs and survive; only the macros have to go.
#undef Bool
#undef Status
#undef True
#undef False
#undef None
#undef Success
#undef Always

#include <atomic>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
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
#include "flutter/rust/ffi/rustflutter_ffi_internal.h"
#include "flutter/rust/host/rustflutter_gl.h"
#include "flutter/rust/host/rustflutter_key_map_linux.h"
#include "flutter/rust/host/rustflutter_vk.h"
#include "flutter/shell/common/display.h"
#include "flutter/shell/common/platform_view.h"
#include "flutter/shell/common/rasterizer.h"
#include "flutter/shell/common/run_configuration.h"
#include "flutter/shell/common/shell.h"
#include "flutter/shell/common/thread_host.h"
#include "flutter/shell/common/vsync_waiter.h"
#include "flutter/shell/gpu/gpu_surface_gl_impeller.h"
#include "flutter/shell/gpu/gpu_surface_software.h"
#include "flutter/shell/gpu/gpu_surface_software_delegate.h"
#include "flutter/shell/gpu/gpu_surface_vulkan_impeller.h"
#include "flutter/shell/platform/common/client_wrapper/include/flutter/standard_method_codec.h"
#include "flutter/shell/platform/common/text_input_model.h"
#include "flutter/shell/platform/common/text_range.h"
#include "rapidjson/document.h"
#include "rapidjson/stringbuffer.h"
#include "rapidjson/writer.h"
#include "third_party/skia/include/core/SkStream.h"
#include "third_party/skia/include/core/SkSurface.h"
#include "third_party/skia/include/encode/SkPngEncoder.h"

namespace flutter {
namespace {

/// Where key events go. Matched by RuntimeController, which is the only reader.
/// Upstream this same string is in embedder.cc, platform_dispatcher.dart,
/// KeyData.java and FlutterEngine.mm -- an embedder is expected to spell it
/// out.
constexpr char kKeyDataChannel[] = "flutter/keydata";
constexpr char kPlatformChannel[] = "flutter/platform";
constexpr char kLifecycleChannel[] = "flutter/lifecycle";
constexpr char kSettingsChannel[] = "flutter/settings";
constexpr char kLocalizationChannel[] = "flutter/localization";
constexpr char kMouseCursorChannel[] = "flutter/mousecursor";
constexpr char kTextInputChannel[] = "flutter/textinput";

constexpr char kClipboardError[] = "Clipboard error";
constexpr char kUnknownClipboardFormatMessage[] = "Unknown clipboard format";
constexpr char kTextPlainFormat[] = "text/plain";
constexpr char kExitRequestError[] = "ExitApplication error";
constexpr char kInvalidExitRequestMessage[] =
    "Invalid application exit request";
constexpr char kExitTypeCancelable[] = "cancelable";

/// The display's refresh rate, read on the GTK thread at startup and consumed
/// by the vsync waiter on the UI thread -- GDK is not thread-safe, so the
/// waiter must not ask it directly.
std::atomic<double> g_display_refresh_hz{60.0};

//------------------------------------------------------------------------------
/// The last frame the rasterizer produced, waiting for the window to draw it.
///
/// Two threads meet here and nowhere else: the raster thread stores, the main
/// thread paints. Both under one lock, because a frame half-replaced while it
/// is being drawn is a torn frame.
class FrameBuffer {
 public:
  /// Stores one frame. `blue_first` says whether the pixels are BGRA rather
  /// than RGBA; cairo only draws the former, so an RGBA frame is swizzled on
  /// the way in -- once, here, rather than every paint.
  void Store(const void* pixels,
             int32_t width,
             int32_t height,
             bool blue_first) {
    if (pixels == nullptr || width <= 0 || height <= 0) {
      return;
    }
    const size_t bytes = static_cast<size_t>(width) * height * 4;
    std::lock_guard<std::mutex> lock(mutex_);
    pixels_.resize(bytes);
    std::memcpy(pixels_.data(), pixels, bytes);
    if (!blue_first) {
      for (size_t i = 0; i < bytes; i += 4) {
        std::swap(pixels_[i], pixels_[i + 2]);
      }
    }
    width_ = width;
    height_ = height;
  }

  /// Draws the stored frame into `cr`, the drawing area's own cairo context.
  ///
  /// CAIRO_FORMAT_ARGB32 is premultiplied ARGB in native-endian words, which on
  /// a little-endian machine is BGRA in memory order -- exactly Skia's
  /// kBGRA_8888 premul, which is what the backing store is pinned to. The frame
  /// is in physical pixels and the context draws in logical ones; the device
  /// scale is what reconciles them, and letting cairo do the division is both
  /// correct and free. No y-flip: cairo and the engine are both top-down.
  ///
  /// Returns false when there is nothing to draw, which is every moment before
  /// the first frame arrives.
  bool Paint(cairo_t* cr, int scale_factor) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (pixels_.empty() || width_ <= 0 || height_ <= 0) {
      return false;
    }
    const int stride =
        cairo_format_stride_for_width(CAIRO_FORMAT_ARGB32, width_);
    if (stride != width_ * 4) {
      return false;
    }
    cairo_surface_t* surface = cairo_image_surface_create_for_data(
        pixels_.data(), CAIRO_FORMAT_ARGB32, width_, height_, stride);
    if (cairo_surface_status(surface) != CAIRO_STATUS_SUCCESS) {
      cairo_surface_destroy(surface);
      return false;
    }
    cairo_surface_set_device_scale(surface, scale_factor, scale_factor);
    cairo_set_source_surface(cr, surface, 0, 0);
    cairo_paint(cr);
    cairo_surface_destroy(surface);
    return true;
  }

  /// Writes the stored frame to `path` as a PNG.
  ///
  /// A window is one way to look at this host's output; a build machine has no
  /// screen, and this is how the blit gets checked instead: channel order and
  /// orientation are both visible in the file, and both are the kind of
  /// mistake that still produces a picture.
  ///
  /// Enabled by RUSTFLUTTER_DUMP_FRAME=<path>, and it writes the first frame
  /// only.
  bool WritePng(const char* path) {
    std::vector<uint8_t> pixels;
    int32_t width = 0;
    int32_t height = 0;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      pixels = pixels_;
      width = width_;
      height = height_;
    }
    if (pixels.empty() || width <= 0 || height <= 0) {
      return false;
    }
    SkImageInfo info = SkImageInfo::Make(width, height, kBGRA_8888_SkColorType,
                                         kPremul_SkAlphaType);
    SkPixmap pixmap(info, pixels.data(), static_cast<size_t>(width) * 4);
    SkFILEWStream stream(path);
    return stream.isValid() &&
           SkPngEncoder::Encode(&stream, pixmap, SkPngEncoder::Options{});
  }

 private:
  std::mutex mutex_;
  std::vector<uint8_t> pixels_;
  int32_t width_ = 0;
  int32_t height_ = 0;
};

//------------------------------------------------------------------------------
/// A vsync waiter paced by the display rather than by a fixed sixty hertz.
///
/// The algorithm is `VsyncWaiterFallback`'s, and the other hosts': a phase
/// fixed at construction, each frame snapped forward onto that grid, and the
/// callback posted for that time. What changes is only where the interval comes
/// from.
///
/// Not a GdkFrameClock. The frame clock only ticks while the widget is mapped
/// and being painted, calls back on the GTK thread -- so the callback would
/// have to hop to the UI task runner anyway -- and under WSLg its pacing is the
/// remote compositor's, which is erratic. A snapped timer is what the Windows
/// and macOS hosts already ship.
class VsyncWaiterLinux final : public VsyncWaiter {
 public:
  explicit VsyncWaiterLinux(const TaskRunners& task_runners)
      : VsyncWaiter(task_runners), phase_(fml::TimePoint::Now()) {}

  ~VsyncWaiterLinux() override = default;

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

  fml::TimeDelta FrameInterval() {
    if (interval_ != fml::TimeDelta::Zero()) {
      return interval_;
    }

    // A rate this machine does not have, for testing the pacing itself.
    double hz = 0.0;
    if (const char* forced = std::getenv("RUSTFLUTTER_FORCE_HZ")) {
      hz = std::atof(forced);
    }
    if (hz <= 0.0) {
      hz = g_display_refresh_hz.load();
    }
    if (hz <= 1.0) {
      hz = 60.0;
    }

    FML_LOG(IMPORTANT) << "Pacing frames at " << hz << " Hz.";
    interval_ = fml::TimeDelta::FromSecondsF(1.0 / hz);
    return interval_;
  }

  // |VsyncWaiter|
  void AwaitVSync() override {
    const fml::TimeDelta interval = FrameInterval();
    const fml::TimePoint frame_start_time =
        SnapToNextTick(fml::TimePoint::Now(), phase_, interval);
    const fml::TimePoint frame_target_time = frame_start_time + interval;

    std::weak_ptr<VsyncWaiterLinux> weak_this =
        std::static_pointer_cast<VsyncWaiterLinux>(shared_from_this());
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

  FML_DISALLOW_COPY_AND_ASSIGN(VsyncWaiterLinux);
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
  writer.String(message.c_str(),
                static_cast<rapidjson::SizeType>(message.size()));
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

/// Runs `function(data)` on the GTK thread. The two GLib entry points
/// documented safe from any thread are this and g_idle_add; everything the
/// host asks of GTK funnels through them.
void InvokeOnGtkThread(GSourceFunc function, gpointer data) {
  g_main_context_invoke(nullptr, function, data);
}

void SetClipboardText(const std::string& text) {
  char* copy = g_strdup(text.c_str());
  InvokeOnGtkThread(
      [](gpointer data) -> gboolean {
        char* text = static_cast<char*>(data);
        gtk_clipboard_set_text(gtk_clipboard_get(GDK_SELECTION_CLIPBOARD), text,
                               -1);
        g_free(text);
        return G_SOURCE_REMOVE;
      },
      copy);
}

/// Handles one call on `flutter/platform`.
///
/// Returns the reply, or nothing for a method this host does not implement --
/// which is answered with an empty message rather than an error, because that
/// is what tells the framework nobody served it. The clipboard *reads* are not
/// here: GTK answers those asynchronously, so they carry the response with
/// them instead of returning one. See HandleClipboardRead.
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
    const bool cancelable =
        std::string(type->value.GetString()) == kExitTypeCancelable;

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
      InvokeOnGtkThread(
          [](gpointer) -> gboolean {
            if (GdkDisplay* display = gdk_display_get_default()) {
              gdk_display_beep(display);
            }
            return G_SOURCE_REMOVE;
          },
          nullptr);
      return NullEnvelope();
    }
    // A click and a tick have no system sound on Linux either, and succeeding
    // silently is what upstream's GTK handler does.
    if (sound == "SystemSoundType.click" || sound == "SystemSoundType.tick") {
      return NullEnvelope();
    }
    return std::nullopt;
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

  if (method == "System.initializationComplete") {
    // Deprecated upstream, and answered rather than refused for the reason the
    // comment there gives: an application that still sends it should not see an
    // error for it.
    return NullEnvelope();
  }

  return std::nullopt;
}

//------------------------------------------------------------------------------
// The clipboard reads, which cannot return a reply because GTK does not have
// one to give yet: gtk_clipboard_request_text asks the selection's owner --
// another process -- and calls back when the answer arrives. So the response
// travels with the request, and the reply is completed from the platform
// thread when GTK is done.

struct ClipboardRead {
  /// Which question was asked: hasStrings wants a boolean, getData the text.
  bool has_strings_query = false;
  fml::RefPtr<PlatformMessageResponse> response;
  fml::RefPtr<fml::TaskRunner> platform_task_runner;
};

void FinishClipboardRead(GtkClipboard* clipboard,
                         const gchar* text,
                         gpointer data) {
  std::unique_ptr<ClipboardRead> read(static_cast<ClipboardRead*>(data));
  std::string reply;
  if (read->has_strings_query) {
    const bool has_text = text != nullptr && text[0] != '\0';
    reply = SuccessEnvelope([has_text](auto& writer) {
      writer.StartObject();
      writer.Key("value");
      writer.Bool(has_text);
      writer.EndObject();
    });
  } else if (text == nullptr || text[0] == '\0') {
    // Nothing to paste, which the framework reads as "no data" rather than as
    // an error.
    reply = NullEnvelope();
  } else {
    const std::string content(text);
    reply = SuccessEnvelope([&content](auto& writer) {
      writer.StartObject();
      writer.Key("text");
      writer.String(content.c_str(),
                    static_cast<rapidjson::SizeType>(content.size()));
      writer.EndObject();
    });
  }
  // Complete on the platform thread, where every other reply comes from.
  read->platform_task_runner->PostTask(
      fml::MakeCopyable([response = read->response, reply]() {
        response->Complete(std::make_unique<fml::DataMapping>(
            std::vector<uint8_t>(reply.begin(), reply.end())));
      }));
}

/// Starts an asynchronous clipboard read whose eventual answer completes
/// `response`. Returns false -- with no reply sent -- for malformed arguments,
/// so the caller can answer the error inline.
bool HandleClipboardRead(bool has_strings_query,
                         const rapidjson::Value* args,
                         fml::RefPtr<PlatformMessageResponse> response,
                         fml::RefPtr<fml::TaskRunner> platform_task_runner) {
  // "text/plain" is the only format the channel has ever defined.
  if (args == nullptr || !args->IsString() ||
      std::string(args->GetString()) != kTextPlainFormat) {
    return false;
  }
  auto* read = new ClipboardRead{has_strings_query, std::move(response),
                                 std::move(platform_task_runner)};
  InvokeOnGtkThread(
      [](gpointer data) -> gboolean {
        gtk_clipboard_request_text(gtk_clipboard_get(GDK_SELECTION_CLIPBOARD),
                                   FinishClipboardRead, data);
        return G_SOURCE_REMOVE;
      },
      read);
  return true;
}

//------------------------------------------------------------------------------
// The mouse cursor.
//
// `flutter/mousecursor` speaks the binary standard codec rather than JSON.
// The kind-to-name table is upstream's `fl_mouse_cursor_handler.cc`, name for
// name: the framework picks the kinds, so the table is a protocol and not a
// preference.

const char* CursorNameByKind(const std::string& kind) {
  static const std::map<std::string, const char*> table = {
      {"alias", "alias"},
      {"allScroll", "all-scroll"},
      {"basic", "default"},
      {"cell", "cell"},
      {"click", "pointer"},
      {"contextMenu", "context-menu"},
      {"copy", "copy"},
      {"forbidden", "not-allowed"},
      {"grab", "grab"},
      {"grabbing", "grabbing"},
      {"help", "help"},
      {"move", "move"},
      {"none", "none"},
      {"noDrop", "no-drop"},
      {"precise", "crosshair"},
      {"progress", "progress"},
      {"text", "text"},
      {"resizeColumn", "col-resize"},
      {"resizeDown", "s-resize"},
      {"resizeDownLeft", "sw-resize"},
      {"resizeDownRight", "se-resize"},
      {"resizeLeft", "w-resize"},
      {"resizeLeftRight", "ew-resize"},
      {"resizeRight", "e-resize"},
      {"resizeRow", "row-resize"},
      {"resizeUp", "n-resize"},
      {"resizeUpDown", "ns-resize"},
      {"resizeUpLeft", "nw-resize"},
      {"resizeUpLeftDownRight", "nwse-resize"},
      {"resizeUpRight", "ne-resize"},
      {"resizeUpRightDownLeft", "nesw-resize"},
      {"verticalText", "vertical-text"},
      {"wait", "wait"},
      {"zoomIn", "zoom-in"},
      {"zoomOut", "zoom-out"},
  };
  auto found = table.find(kind);
  // An unknown kind falls back to the default rather than failing, as upstream
  // does: a cursor is a hint, and refusing one is worse than showing the
  // arrow.
  return found != table.end() ? found->second : "default";
}

/// What a cursor change needs when it reaches the GTK thread.
struct CursorChange {
  GtkWidget* window = nullptr;
  std::string name;
};

std::vector<uint8_t> HandleMouseCursorCall(GtkWidget* window,
                                           const MethodCall<>& call) {
  auto& codec = StandardMethodCodec::GetInstance();
  if (call.method_name() != "activateSystemCursor") {
    return *codec.EncodeErrorEnvelope("unimplemented", "", nullptr);
  }
  const auto* arguments = std::get_if<EncodableMap>(call.arguments());
  if (arguments == nullptr) {
    return *codec.EncodeErrorEnvelope("error", "Missing arguments", nullptr);
  }
  auto kind = arguments->find(EncodableValue("kind"));
  if (kind == arguments->end() ||
      !std::holds_alternative<std::string>(kind->second)) {
    return *codec.EncodeErrorEnvelope("error", "Missing 'kind'", nullptr);
  }

  auto* change = new CursorChange{
      window, CursorNameByKind(std::get<std::string>(kind->second))};
  // The reference keeps a change that arrives after the window died harmless.
  g_object_ref(window);
  InvokeOnGtkThread(
      [](gpointer data) -> gboolean {
        std::unique_ptr<CursorChange> change(static_cast<CursorChange*>(data));
        GdkWindow* gdk_window = gtk_widget_get_window(change->window);
        if (gdk_window != nullptr) {
          GdkCursor* cursor = gdk_cursor_new_from_name(
              gdk_window_get_display(gdk_window), change->name.c_str());
          // A theme without the name gives null, which resets to the parent's
          // cursor -- the default, which is the right fallback anyway.
          gdk_window_set_cursor(gdk_window, cursor);
          if (cursor != nullptr) {
            g_object_unref(cursor);
          }
        }
        g_object_unref(change->window);
        return G_SOURCE_REMOVE;
      },
      change);
  return *codec.EncodeSuccessEnvelope();
}

//------------------------------------------------------------------------------
// Platform settings. Every reader below is a GTK call, so the payload is built
// on the GTK thread -- at startup before the first frame, and again when the
// theme notify:: fires -- and handed to the platform view as a string.

/// Whether the reader has asked for dark mode. What upstream's
/// `fl_gnome_settings` effectively reports: the explicit preference first, and
/// the theme's own name as the tiebreak, because most desktops spell dark mode
/// "Adwaita-dark" rather than setting the flag.
bool PrefersDarkTheme() {
  GtkSettings* settings = gtk_settings_get_default();
  if (settings == nullptr) {
    return false;
  }
  gboolean prefer_dark = FALSE;
  g_object_get(settings, "gtk-application-prefer-dark-theme", &prefer_dark,
               nullptr);
  if (prefer_dark) {
    return true;
  }
  gchar* theme_name = nullptr;
  g_object_get(settings, "gtk-theme-name", &theme_name, nullptr);
  bool dark = false;
  if (theme_name != nullptr) {
    gchar* lowered = g_ascii_strdown(theme_name, -1);
    dark = std::strstr(lowered, "dark") != nullptr;
    g_free(lowered);
    g_free(theme_name);
  }
  return dark;
}

/// The reader's text scale. GTK spells it as a font DPI in 1024ths -- Xft.dpi
/// -- against a 96 baseline, which is how GNOME's "Large Text" accessibility
/// switch reaches applications.
double TextScaleFactor() {
  GtkSettings* settings = gtk_settings_get_default();
  if (settings == nullptr) {
    return 1.0;
  }
  gint xft_dpi = 0;
  g_object_get(settings, "gtk-xft-dpi", &xft_dpi, nullptr);
  if (xft_dpi <= 0) {
    return 1.0;
  }
  const double scale = (xft_dpi / 1024.0) / 96.0;
  return scale > 0.1 && scale < 10.0 ? scale : 1.0;
}

/// Whether times should be written 13:00 rather than 1:00 PM. GNOME keeps the
/// switch in GSettings; a desktop without the schema -- WSLg, most likely --
/// gets the 24-hour default, which is what most of the world reads.
bool AlwaysUse24HourFormat() {
  GSettingsSchemaSource* source = g_settings_schema_source_get_default();
  if (source == nullptr) {
    return true;
  }
  GSettingsSchema* schema = g_settings_schema_source_lookup(
      source, "org.gnome.desktop.interface", TRUE);
  if (schema == nullptr) {
    return true;
  }
  bool result = true;
  if (g_settings_schema_has_key(schema, "clock-format")) {
    GSettings* settings = g_settings_new("org.gnome.desktop.interface");
    gchar* format = g_settings_get_string(settings, "clock-format");
    result = format == nullptr || std::strcmp(format, "12h") != 0;
    g_free(format);
    g_object_unref(settings);
  }
  g_settings_schema_unref(schema);
  return result;
}

/// GTK thread only; see the section comment.
std::string SettingsPayload() {
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartObject();
  writer.Key("alwaysUse24HourFormat");
  writer.Bool(AlwaysUse24HourFormat());
  writer.Key("textScaleFactor");
  writer.Double(TextScaleFactor());
  writer.Key("platformBrightness");
  writer.String(PrefersDarkTheme() ? "dark" : "light");
  writer.EndObject();
  return buffer.GetString();
}

/// The `flutter/localization` payload: the locales the reader has chosen, in
/// order, each as the four strings `setLocale` expects. `g_get_language_names`
/// reads the environment, so unlike the settings this is safe from any thread.
std::optional<std::string> LocalizationPayload() {
  const gchar* const* names = g_get_language_names();
  if (names == nullptr) {
    return std::nullopt;
  }
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartObject();
  writer.Key("method");
  writer.String("setLocale");
  writer.Key("args");
  writer.StartArray();
  std::set<std::string> seen;
  int written = 0;
  for (const gchar* const* name = names; *name != nullptr; ++name) {
    // "zh_CN.UTF-8@variant" -> language "zh", country "CN". The C locale goes
    // through as the language "C", which is what upstream's `setup_locales`
    // sends too -- on a machine with LANG=C it is the only locale there is,
    // and no locale at all is worse than an odd one.
    std::string entry(*name);
    entry = entry.substr(0, entry.find('@'));
    entry = entry.substr(0, entry.find('.'));
    if (entry.empty() || !seen.insert(entry).second) {
      continue;
    }
    std::string language = entry;
    std::string country;
    if (auto underscore = entry.find('_'); underscore != std::string::npos) {
      language = entry.substr(0, underscore);
      country = entry.substr(underscore + 1);
    }
    // Four strings per locale, in this order, empty where there is nothing --
    // `PlatformDispatcher.setLocale` reads them positionally.
    writer.String(language.c_str());
    writer.String(country.c_str());
    writer.String("");
    writer.String("");
    ++written;
  }
  writer.EndArray();
  writer.EndObject();
  if (written == 0) {
    return std::nullopt;
  }
  return std::string(buffer.GetString());
}

//------------------------------------------------------------------------------
// Text input.
//
// `flutter/textinput` is the channel a text field talks to the platform on.
// The arrangement is the Windows host's: the editing model is the engine's own
// `flutter::TextInputModel`, channel calls arrive on the platform thread and
// keys on the window thread, both touch the model, so it is behind a mutex.
//
// The IME is GtkIMContext, as upstream: every key press is offered to the
// context first, and what it composes or commits comes back through the
// `preedit-*` and `commit` signals on the GTK thread. A key the context does
// not want falls through to the plain path, where it edits or types directly.

/// The framework's text field, as the platform sees it.
///
/// Upstream's `TextInputPlugin`. It holds the editing model, answers
/// `flutter/textinput`, and reports every change back as
/// `TextInputClient.updateEditingState`.
class TextInputHandler {
 public:
  /// How a state update leaves here. Set once, by the platform view.
  using Sender = std::function<void(const std::string& method,
                                    const std::string& arguments_json)>;

  void SetSender(Sender sender) { sender_ = std::move(sender); }

  /// True once the framework has attached a field. Everything typed while this
  /// is false goes nowhere, which is correct: there is nothing to type into.
  bool attached() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return model_ != nullptr;
  }

  /// Handles one call on `flutter/textinput`. Platform thread.
  std::optional<std::string> HandleMethodCall(const std::string& method,
                                              const rapidjson::Value* args) {
    if (method == "TextInput.show" || method == "TextInput.hide") {
      // There is no on-screen keyboard to raise, but the input method's focus
      // follows: candidates windows appear for a focused context only.
      FocusIm(method == "TextInput.show");
      return NullEnvelope();
    }

    if (method == "TextInput.setClient") {
      // `[clientId, config]`. The config carries the action and the type; the
      // delta model and the view id are not supported here.
      if (args == nullptr || !args->IsArray() || args->Size() < 2) {
        return ErrorEnvelope("TextInput.badArgument",
                             "setClient needs a client id and a configuration");
      }
      const rapidjson::Value& client = (*args)[0];
      const rapidjson::Value& config = (*args)[1];
      if (!client.IsInt()) {
        return ErrorEnvelope("TextInput.badArgument",
                             "the client id is not a number");
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
      {
        std::lock_guard<std::mutex> lock(mutex_);
        model_.reset();
      }
      // A context left focused would keep composing into a field that is
      // gone.
      FocusIm(false);
      return NullEnvelope();
    }

    if (method == "TextInput.setEditingState") {
      if (args == nullptr || !args->IsObject()) {
        return ErrorEnvelope("TextInput.badArgument",
                             "setEditingState needs a state");
      }
      auto text = args->FindMember("text");
      if (text == args->MemberEnd() || !text->value.IsString()) {
        return ErrorEnvelope("TextInput.badArgument", "the state has no text");
      }
      auto number = [args](const char* key, int fallback) {
        auto found = args->FindMember(key);
        return found != args->MemberEnd() && found->value.IsInt()
                   ? found->value.GetInt()
                   : fallback;
      };
      const int base = number("selectionBase", -1);
      const int extent = number("selectionExtent", -1);
      const int composing_base = number("composingBase", -1);
      const int composing_extent = number("composingExtent", -1);

      std::lock_guard<std::mutex> lock(mutex_);
      if (model_ == nullptr) {
        return ErrorEnvelope(
            "TextInput.noClient",
            "the editing state was set with no client attached");
      }
      // The framework is the authority here: this is it telling the platform
      // what the field now holds, which is how a programmatic edit -- a paste,
      // a clear button -- reaches the platform's copy.
      model_->SetText(text->value.GetString(),
                      TextRange(static_cast<size_t>(base < 0 ? 0 : base),
                                static_cast<size_t>(extent < 0 ? 0 : extent)),
                      composing_base < 0 || composing_extent < 0
                          ? TextRange(0, 0)
                          : TextRange(static_cast<size_t>(composing_base),
                                      static_cast<size_t>(composing_extent)));
      return NullEnvelope();
    }

    if (method == "TextInput.setEditableSizeAndTransform") {
      // Where the field sits in the view, as a 4x4 transform of which only
      // the translation means anything here. Logical pixels, which is also
      // what GTK's window coordinates are.
      if (args != nullptr && args->IsObject()) {
        auto transform = args->FindMember("transform");
        if (transform != args->MemberEnd() && transform->value.IsArray() &&
            transform->value.Size() == 16 && transform->value[12].IsNumber() &&
            transform->value[13].IsNumber()) {
          std::lock_guard<std::mutex> lock(mutex_);
          editable_dx_ = transform->value[12].GetDouble();
          editable_dy_ = transform->value[13].GetDouble();
        }
      }
      UpdateImCursorLocation();
      return NullEnvelope();
    }

    if (method == "TextInput.setMarkedTextRect") {
      // Where the caret is inside the field. This plus the transform is where
      // the input method puts its candidates window.
      if (args != nullptr && args->IsObject()) {
        auto number = [args](const char* key) {
          auto found = args->FindMember(key);
          return found != args->MemberEnd() && found->value.IsNumber()
                     ? found->value.GetDouble()
                     : 0.0;
        };
        std::lock_guard<std::mutex> lock(mutex_);
        caret_x_ = number("x");
        caret_y_ = number("y");
        caret_width_ = number("width");
        caret_height_ = number("height");
      }
      UpdateImCursorLocation();
      return NullEnvelope();
    }

    return std::nullopt;
  }

  // -- What the window thread reports -----------------------------------------

  /// A character the reader typed.
  void OnText(const std::u16string& text) {
    if (Edit([&text](TextInputModel& model) {
          model.AddText(text);
          return true;
        })) {
      SendStateUpdate();
    }
  }

  /// An editing key: backspace, delete, the arrows, home and end.
  ///
  /// Returns true if the field used it. Enter is handled separately because it
  /// is an *action* rather than an edit.
  bool OnEditingKey(guint keyval, bool shift) {
    bool changed = false;
    const bool handled = Edit([&](TextInputModel& model) {
      switch (keyval) {
        case GDK_KEY_BackSpace:
          changed = model.Backspace();
          return true;
        case GDK_KEY_Delete:
        case GDK_KEY_KP_Delete:
          changed = model.Delete();
          return true;
        case GDK_KEY_Left:
        case GDK_KEY_KP_Left:
          changed = model.MoveCursorBack();
          return true;
        case GDK_KEY_Right:
        case GDK_KEY_KP_Right:
          changed = model.MoveCursorForward();
          return true;
        case GDK_KEY_Home:
        case GDK_KEY_KP_Home:
          changed =
              shift ? model.SelectToBeginning() : model.MoveCursorToBeginning();
          return true;
        case GDK_KEY_End:
        case GDK_KEY_KP_End:
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

  /// Enter, which submits rather than edits.
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
      // A multiline field whose action is newline gets a newline *and* the
      // action, which is upstream's `EnterPressed`. Anything else only gets
      // the action -- Enter in a single-line field submits, it does not
      // insert.
      newline = input_type_ == "TextInputType.multiline" &&
                action == "TextInputAction.newline";
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
      sender_("TextInputClient.performAction",
              std::string(buffer.GetString(), buffer.GetSize()));
    }
  }

  // -- What the input method reports (GTK thread) -----------------------------

  /// The context this handler drives. Set on the GTK thread before the shell
  /// starts, cleared before the context is destroyed.
  void SetImContext(GtkIMContext* im_context) {
    std::lock_guard<std::mutex> lock(mutex_);
    im_context_ = im_context;
  }

  /// Finished text, composed or plain -- with the default context even an
  /// ordinary `a` arrives here rather than through the key's fallthrough.
  void ImCommit(const gchar* text) {
    if (Edit([text](TextInputModel& model) {
          model.AddText(std::string(text));
          if (model.composing()) {
            model.CommitComposing();
          }
          return true;
        })) {
      SendStateUpdate();
    }
  }

  void ImPreeditStart() {
    Edit([](TextInputModel& model) {
      model.BeginComposing();
      return true;
    });
  }

  /// The composing text changed. Upstream's `im_preedit_changed_cb`: the
  /// preedit replaces the composing region and the cursor lands where the
  /// context says, counted from the region's start.
  void ImPreeditChanged(GtkIMContext* im_context) {
    gchar* preedit = nullptr;
    gint cursor_offset = 0;
    gtk_im_context_get_preedit_string(im_context, &preedit, nullptr,
                                      &cursor_offset);
    if (Edit([&](TextInputModel& model) {
          cursor_offset += static_cast<gint>(
              model.composing() ? model.composing_range().start()
                                : model.selection().start());
          model.UpdateComposingText(std::string(preedit));
          model.SetSelection(TextRange(static_cast<size_t>(cursor_offset)));
          return true;
        })) {
      SendStateUpdate();
    }
    g_free(preedit);
  }

  void ImPreeditEnd() {
    if (Edit([](TextInputModel& model) {
          model.EndComposing();
          return true;
        })) {
      SendStateUpdate();
    }
  }

  /// The context wants the text around the cursor -- what a dead key composes
  /// against, and what some methods use for context.
  bool ImRetrieveSurrounding(GtkIMContext* im_context) {
    std::string text;
    int cursor_offset = 0;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (model_ == nullptr) {
        return false;
      }
      text = model_->GetText();
      cursor_offset = model_->GetCursorOffset();
    }
    gtk_im_context_set_surrounding(im_context, text.c_str(), -1, cursor_offset);
    return true;
  }

  /// The context deletes around the cursor rather than through a key.
  bool ImDeleteSurrounding(gint offset, gint n_chars) {
    if (Edit([offset, n_chars](TextInputModel& model) {
          return model.DeleteSurrounding(offset, n_chars);
        })) {
      SendStateUpdate();
    }
    return true;
  }

 private:
  /// Runs `edit` against the model, if there is one. Returns what it returned,
  /// or false when no client is attached.
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

  /// Focuses or unfocuses the context, from the platform thread. The context
  /// is a GTK object, so the call is marshalled; the reference keeps it alive
  /// across the hop.
  void FocusIm(bool focused) {
    GtkIMContext* im_context = nullptr;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (im_context_ == nullptr) {
        return;
      }
      im_context = GTK_IM_CONTEXT(g_object_ref(im_context_));
    }
    struct Request {
      GtkIMContext* im_context;
      bool focused;
    };
    InvokeOnGtkThread(
        [](gpointer data) -> gboolean {
          Request* request = static_cast<Request*>(data);
          if (request->focused) {
            gtk_im_context_focus_in(request->im_context);
          } else {
            gtk_im_context_focus_out(request->im_context);
          }
          g_object_unref(request->im_context);
          delete request;
          return G_SOURCE_REMOVE;
        },
        new Request{im_context, focused});
  }

  /// Tells the context where the caret is, so its candidates window opens
  /// next to the text rather than in a corner. Platform thread; marshalled.
  void UpdateImCursorLocation() {
    GtkIMContext* im_context = nullptr;
    GdkRectangle location = {};
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (im_context_ == nullptr) {
        return;
      }
      im_context = GTK_IM_CONTEXT(g_object_ref(im_context_));
      // The trailing corner, as upstream: the candidates hang below and after
      // the composing text.
      location.x = static_cast<gint>(editable_dx_ + caret_x_ + caret_width_);
      location.y = static_cast<gint>(editable_dy_ + caret_y_ + caret_height_);
    }
    struct Request {
      GtkIMContext* im_context;
      GdkRectangle location;
    };
    InvokeOnGtkThread(
        [](gpointer data) -> gboolean {
          Request* request = static_cast<Request*>(data);
          gtk_im_context_set_cursor_location(request->im_context,
                                             &request->location);
          g_object_unref(request->im_context);
          delete request;
          return G_SOURCE_REMOVE;
        },
        new Request{im_context, location});
  }

  mutable std::mutex mutex_;
  std::unique_ptr<TextInputModel> model_;
  int client_id_ = 0;
  std::string input_action_;
  std::string input_type_;
  Sender sender_;
  GtkIMContext* im_context_ = nullptr;
  /// Where the field is in the view and the caret in the field, in logical
  /// pixels, for the candidates window.
  double editable_dx_ = 0.0;
  double editable_dy_ = 0.0;
  double caret_x_ = 0.0;
  double caret_y_ = 0.0;
  double caret_width_ = 0.0;
  double caret_height_ = 0.0;
};

//------------------------------------------------------------------------------
/// A reply that has to reach a particular thread.
class HostPlatformMessageResponse : public PlatformMessageResponse {
 public:
  using Handler = std::function<void(const uint8_t*, size_t)>;

  HostPlatformMessageResponse(fml::RefPtr<fml::TaskRunner> runner,
                              Handler handler)
      : runner_(std::move(runner)), handler_(std::move(handler)) {}

  void Complete(std::unique_ptr<fml::Mapping> data) override {
    if (data == nullptr) {
      CompleteEmpty();
      return;
    }
    std::vector<uint8_t> reply(data->GetMapping(),
                               data->GetMapping() + data->GetSize());
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
    runner_->PostTask(fml::MakeCopyable([handler, reply = std::move(reply)]() {
      handler(reply.data(), reply.size());
    }));
  }

  fml::RefPtr<fml::TaskRunner> runner_;
  Handler handler_;
};

//------------------------------------------------------------------------------
/// Which GPU backend renders the frames.
///
/// kSoftware is the Skia bitmap surface, which needs nothing and works
/// everywhere; kGles is Impeller through Mesa's EGL, the default; kVulkan is
/// Impeller on the machine's Vulkan driver directly. The choice is made once
/// from the environment in rf_host_run, and a failed Vulkan or GL context
/// walks down the list rather than failing to start.
enum class RenderBackend { kSoftware, kGles, kVulkan };

//------------------------------------------------------------------------------
/// What the window and the shell share. The window thread writes the geometry,
/// the platform thread reads it.
struct WindowState {
  Shell* shell = nullptr;
  class HostPlatformView* platform_view = nullptr;
  fml::RefPtr<fml::TaskRunner> platform_task_runner;
  FrameBuffer frame_buffer;
  TextInputHandler text_input;
  double device_pixel_ratio = 1.0;
  int32_t physical_width = 0;
  int32_t physical_height = 0;
  std::string lifecycle_state;
  /// The mouse's last position, in physical pixels, for the pointer deltas.
  double last_x = 0.0;
  double last_y = 0.0;
  /// Which mouse buttons are down, as the PointerData bit flags.
  int64_t buttons = 0;
  /// The physical keys currently held, for telling a repeat from a press --
  /// GDK reports both as key-press events and nothing in them says which.
  std::set<uint64_t> pressed_physical_keys;
  /// Set by the raster thread once an Impeller surface exists, read by the
  /// draw handler: a cairo paint over a GL swap would erase the frame.
  std::atomic<bool> gpu_active{false};
  /// The input method, created and used on the GTK thread only. The text
  /// handler holds its own guarded copy for the platform thread's calls.
  GtkIMContext* im_context = nullptr;
  GtkWidget* window = nullptr;
  GtkWidget* drawing_area = nullptr;
  GMainLoop* loop = nullptr;
  EGLNativeWindowType xid = 0;
  /// The X11 Display* the window lives on, untyped because `Display` is also
  /// a flutter:: name in this file. For the Vulkan surface.
  void* xdisplay = nullptr;
};

/// Quits the window's main loop, from any thread.
void QuitMainLoop(GMainLoop* loop) {
  if (loop == nullptr) {
    return;
  }
  g_main_loop_ref(loop);
  InvokeOnGtkThread(
      [](gpointer data) -> gboolean {
        GMainLoop* loop = static_cast<GMainLoop*>(data);
        if (g_main_loop_is_running(loop)) {
          g_main_loop_quit(loop);
        }
        g_main_loop_unref(loop);
        return G_SOURCE_REMOVE;
      },
      loop);
}

//------------------------------------------------------------------------------
/// The platform view: the shell's window onto this host.
///
/// Lives on the platform thread. SetupImpellerContext, CreateRenderingSurface,
/// AcquireBackingStore and PresentBackingStore are called on the raster
/// thread; all are safe there because the GL context is the raster thread's
/// own and the frame buffer is locked.
class HostPlatformView final : public PlatformView,
                               public GPUSurfaceSoftwareDelegate,
                               public ExitRequester {
 public:
  HostPlatformView(PlatformView::Delegate& delegate,
                   const TaskRunners& task_runners,
                   WindowState* state,
                   RenderBackend backend)
      : PlatformView(delegate, task_runners),
        state_(state),
        backend_(backend) {}

  ~HostPlatformView() override {
    // The upload target holds a shared_ptr to the Impeller context. Left in
    // place it would keep a Vulkan context alive past the unloading of the
    // Vulkan library it calls through, and the context's own destructor --
    // run at process exit -- would jump into unmapped memory.
    if (vk_context_ || gl_context_) {
      RfSetImageUploadTarget(nullptr, nullptr);
    }
  }

  // |PlatformView|
  //
  // Called on the raster thread during startup, before anything asks for the
  // Impeller context. That ordering is the reason this hook exists: the shell
  // publishes GetImpellerContext() to the IO thread right after this returns.
  void SetupImpellerContext() override {
    // Vulkan first when it was asked for: it is the more specific request,
    // and a machine that cannot do it almost always still has the EGL stack
    // the GL path wants.
    if (backend_ == RenderBackend::kVulkan && !vk_context_) {
      vk_context_ = ImpellerVkContext::Create();
      if (!vk_context_) {
        FML_LOG(IMPORTANT) << "Falling back to OpenGL ES; see the error above.";
      }
    }
    if (backend_ != RenderBackend::kSoftware && !vk_context_ && !gl_context_) {
      gl_context_ = ImpellerGlContext::Create();
      if (!gl_context_) {
        FML_LOG(IMPORTANT)
            << "Falling back to software rendering; see the error above.";
      }
    }
    // Text ops and images both have to be recorded for the backend that will
    // draw them, and this runs before the engine is launched, so the first
    // frame already gets it right. The recording is the same for both Impeller
    // backends, so this only distinguishes Impeller from software.
    rf_set_impeller_backend((vk_context_ || gl_context_) ? 1 : 0);
  }

  // |PlatformView|
  //
  // Also on the raster thread, after SetupImpellerContext.
  std::unique_ptr<Surface> CreateRenderingSurface() override {
    if (vk_context_) {
      if (auto surface = CreateVulkanSurface()) {
        state_->gpu_active.store(true);
        return surface;
      }
      // The context is up but the window would not take a swapchain. There is
      // no reason GL would fare worse, and the raster thread is the right
      // place to find out.
      FML_LOG(IMPORTANT) << "Falling back to OpenGL ES; see the error above.";
      vk_context_.reset();
      gl_context_ = ImpellerGlContext::Create();
      // A GL context that failed to come up falls through to software below,
      // same as if it had failed in SetupImpellerContext.
      rf_set_impeller_backend(gl_context_ != nullptr ? 1 : 0);
    }
    if (gl_context_) {
      if (auto surface = CreateImpellerSurface()) {
        state_->gpu_active.store(true);
        return surface;
      }
      FML_LOG(IMPORTANT)
          << "Falling back to software rendering; see the error above.";
    }
    return std::make_unique<GPUSurfaceSoftware>(this,
                                                /*render_to_surface=*/true);
  }

  // |PlatformView|
  std::shared_ptr<impeller::Context> GetImpellerContext() const override {
    if (vk_context_) {
      return vk_context_->GetImpellerContext();
    }
    return gl_context_ ? gl_context_->GetImpellerContext() : nullptr;
  }

  // |PlatformView|
  //
  // Called on the IO thread, once, after the Impeller context is ready. What
  // is wanted is the side effect: the offscreen GL context becomes current on
  // this thread and stays current, so texture uploads posted here have a
  // context to run in and the reactor knows this thread may issue GL commands.
  sk_sp<GrDirectContext> CreateResourceContext() const override {
    if (vk_context_) {
      // Vulkan has no "current context" to make on this thread -- command
      // buffers go to queues from wherever they are built. So an upload posted
      // to the IO runner needs only the runner and the context itself.
      RfSetImageUploadTarget(task_runners_.GetIOTaskRunner(),
                             vk_context_->GetImpellerContext());
      return nullptr;
    }
    if (!gl_context_) {
      return nullptr;
    }
    if (!gl_context_->MakeResourceCurrent()) {
      // Not fatal. Nothing is registered as an upload target, so uploads keep
      // happening on the raster thread on first draw, as they did before.
      FML_LOG(IMPORTANT) << "Could not make the offscreen GL context current "
                            "on the IO thread; textures will upload while "
                            "rasterising instead.";
      return nullptr;
    }
    RfSetImageUploadTarget(task_runners_.GetIOTaskRunner(),
                           gl_context_->GetImpellerContext());
    return nullptr;
  }

  /// Rebuilds whatever the frames are presented to after a resize. An EGL
  /// surface does not follow a window that changed size, so it is remade; a
  /// Vulkan swapchain rebuilds itself on the next frame once it knows the new
  /// size. Presenting to either one stale stretches the frame.
  void OnWindowResized() {
    if (vk_context_) {
      vk_context_->UpdateSize(
          impeller::ISize{state_->physical_width, state_->physical_height});
    }
    if (gl_delegate_) {
      gl_delegate_->Resize();
    }
  }

  // |PlatformView|
  std::unique_ptr<VsyncWaiter> CreateVSyncWaiter() override {
    return std::make_unique<VsyncWaiterLinux>(task_runners_);
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
    // BGRA rather than N32: cairo's ARGB32 is BGRA bytes on this machine, so
    // pinning the backing store means the blit never swizzles. See
    // FrameBuffer::Paint.
    SkImageInfo info = SkImageInfo::Make(
        size.width, size.height, kBGRA_8888_SkColorType, kPremul_SkAlphaType);
    backing_store_ = SkSurfaces::Raster(info);
    return backing_store_;
  }

  // |GPUSurfaceSoftwareDelegate|
  bool PresentBackingStore(sk_sp<SkSurface> backing_store) override {
    if (backing_store == nullptr) {
      return false;
    }
    SkPixmap pixmap;
    if (!backing_store->peekPixels(&pixmap)) {
      return false;
    }
    const bool blue_first = pixmap.colorType() == kBGRA_8888_SkColorType;
    // Said once, because the alternative is a swapped-channel picture that
    // still looks like a picture.
    static bool reported = false;
    if (!reported) {
      reported = true;
      FML_LOG(IMPORTANT) << "Presenting " << (blue_first ? "BGRA" : "RGBA")
                         << " frames.";
    }
    state_->frame_buffer.Store(pixmap.addr(), pixmap.width(), pixmap.height(),
                               blue_first);

    // The first frame, to a file, when asked. See FrameBuffer::WritePng.
    static bool dumped = false;
    if (!dumped) {
      if (const char* path = std::getenv("RUSTFLUTTER_DUMP_FRAME")) {
        dumped = true;
        FML_LOG(IMPORTANT) << "Wrote the first frame to " << path << ": "
                           << (state_->frame_buffer.WritePng(path) ? "ok"
                                                                   : "failed");
      }
    }
    // Wakes the main thread, which repaints from the buffer. The idle source
    // holds its own reference to the drawing area, so a wake that arrives
    // after the window died is harmless.
    GtkWidget* area = state_->drawing_area;
    if (area != nullptr) {
      g_idle_add_full(
          G_PRIORITY_DEFAULT,
          [](gpointer data) -> gboolean {
            gtk_widget_queue_draw(GTK_WIDGET(data));
            return G_SOURCE_REMOVE;
          },
          g_object_ref(area), g_object_unref);
    }
    return true;
  }

  /// Sends one pointer event to the engine. Called from the main thread, which
  /// is why it hops: PlatformView is not thread safe, and the pointer
  /// dispatcher expects to run on the platform thread.
  void SendPointer(const PointerData& data) {
    auto packet = std::make_unique<PointerDataPacket>(1);
    packet->SetPointerData(0, data);
    task_runners_.GetPlatformTaskRunner()->PostTask(fml::MakeCopyable(
        [weak = GetWeakPtr(), packet = std::move(packet)]() mutable {
          if (weak) {
            static_cast<HostPlatformView*>(weak.get())
                ->DispatchPointerDataPacket(std::move(packet));
          }
        }));
  }

  /// Sends one key event to the framework.
  ///
  /// Keys are a platform message rather than a call of their own, which is what
  /// every Flutter embedder does: the packet on `flutter/keydata` is the same
  /// bytes on Windows, Android, iOS and Linux, and no key-shaped method exists
  /// on PlatformView to add one to.
  void SendKey(const KeyData& data, const std::string& character) {
    KeyDataPacket packet(data, character.empty() ? nullptr : character.c_str());
    auto message = std::make_unique<PlatformMessage>(
        kKeyDataChannel,
        fml::MallocMapping::Copy(packet.data().data(), packet.data().size()),
        /*response=*/nullptr);
    task_runners_.GetPlatformTaskRunner()->PostTask(fml::MakeCopyable(
        [weak = GetWeakPtr(), message = std::move(message)]() mutable {
          if (weak) {
            weak->DispatchPlatformMessage(std::move(message));
          }
        }));
  }

  /// Sends a settings payload built on the GTK thread. See SettingsPayload.
  void SendSettingsJson(const std::string& payload) {
    SendOnChannel(kSettingsChannel, payload);
  }

  void SendLocalization() {
    if (auto locales = LocalizationPayload()) {
      SendOnChannel(kLocalizationChannel, *locales);
    }
  }

  /// Tells the framework what the application is doing. One bare string on
  /// `flutter/lifecycle`, with no codec and no envelope -- there is nothing
  /// else the channel could ever need to say.
  void SendLifecycleState(const char* state) {
    SendOnChannel(kLifecycleChannel, std::string(state));
  }

  /// Sends a JSON method call the host initiates, e.g. an editing state.
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
    writer.RawValue(arguments_json.c_str(), arguments_json.size(),
                    rapidjson::kArrayType);
    writer.EndObject();
    SendOnChannel(channel, std::string(buffer.GetString(), buffer.GetSize()));
  }

  // |PlatformView|
  //
  // A message from the framework, on the platform thread. Upstream this is
  // where an embedder's plugins are dispatched to; here it is the channels
  // this host serves. Anything else falls through to an empty reply, which the
  // framework reads as "nobody implements this".
  void HandlePlatformMessage(
      std::unique_ptr<PlatformMessage> message) override {
    const auto& data = message->data();
    std::optional<std::vector<uint8_t>> reply;

    if (message->channel() == kMouseCursorChannel) {
      auto call = StandardMethodCodec::GetInstance().DecodeMethodCall(
          data.GetMapping(), data.GetSize());
      if (call && state_->window != nullptr) {
        reply = HandleMouseCursorCall(state_->window, *call);
      }
    } else if (message->channel() == kPlatformChannel ||
               message->channel() == kTextInputChannel) {
      const bool platform = message->channel() == kPlatformChannel;
      rapidjson::Document document;
      document.Parse(reinterpret_cast<const char*>(data.GetMapping()),
                     data.GetSize());
      if (!document.HasParseError() && document.IsObject()) {
        auto method = document.FindMember("method");
        if (method != document.MemberEnd() && method->value.IsString()) {
          auto found = document.FindMember("args");
          const rapidjson::Value* args =
              found == document.MemberEnd() ? nullptr : &found->value;
          const std::string name = method->value.GetString();

          // The clipboard reads answer later, from GTK's callback, so the
          // response leaves with them and this method must not touch it
          // again.
          if (platform &&
              (name == "Clipboard.getData" || name == "Clipboard.hasStrings")) {
            if (HandleClipboardRead(name == "Clipboard.hasStrings", args,
                                    message->response(),
                                    task_runners_.GetPlatformTaskRunner())) {
              return;
            }
            const std::string error =
                ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
            reply.emplace(error.begin(), error.end());
          } else {
            std::optional<std::string> json =
                platform ? HandlePlatformCall(this, name, args)
                         : state_->text_input.HandleMethodCall(name, args);
            if (json.has_value()) {
              reply.emplace(json->begin(), json->end());
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

  // |ExitRequester|
  void RequestAppExit(bool cancelable, int exit_code) override {
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

    GMainLoop* loop = state_->loop;
    auto response = fml::MakeRefCounted<HostPlatformMessageResponse>(
        task_runners_.GetPlatformTaskRunner(),
        [loop](const uint8_t* reply, size_t length) {
          // A reply of `["exit"]`-shaped JSON means go; anything else,
          // including no reply at all, means stay. Parsing is deliberately
          // forgiving: the only decision is whether the word "exit" is in the
          // answer.
          const std::string text(reinterpret_cast<const char*>(reply), length);
          if (text.find("exit") == std::string::npos) {
            return;
          }
          QuitMainLoop(loop);
        });

    auto message = std::make_unique<PlatformMessage>(
        kPlatformChannel,
        fml::MallocMapping::Copy(payload.data(), payload.size()),
        std::move(response));
    task_runners_.GetPlatformTaskRunner()->PostTask(fml::MakeCopyable(
        [weak = GetWeakPtr(), message = std::move(message)]() mutable {
          if (weak) {
            weak->DispatchPlatformMessage(std::move(message));
          }
        }));
  }

  // |ExitRequester|
  void QuitApplication(int exit_code) override { QuitMainLoop(state_->loop); }

 private:
  void SendOnChannel(const char* channel, const std::string& payload) {
    auto message = std::make_unique<PlatformMessage>(
        channel, fml::MallocMapping::Copy(payload.data(), payload.size()),
        /*response=*/nullptr);
    task_runners_.GetPlatformTaskRunner()->PostTask(fml::MakeCopyable(
        [weak = GetWeakPtr(), message = std::move(message)]() mutable {
          if (weak) {
            weak->DispatchPlatformMessage(std::move(message));
          }
        }));
  }

  /// Builds the Impeller Vulkan surface, or returns nullptr with a logged
  /// reason.
  std::unique_ptr<Surface> CreateVulkanSurface() {
    if (state_->xid == 0 || state_->xdisplay == nullptr) {
      FML_LOG(ERROR) << "No X11 window to put a Vulkan swapchain on.";
      return nullptr;
    }
    if (!vk_context_->SetWindow(
            state_->xdisplay, static_cast<uint64_t>(state_->xid),
            impeller::ISize{state_->physical_width, state_->physical_height})) {
      return nullptr;
    }

    // A null delegate is the self-managed path: GPUSurfaceVulkanImpeller pulls
    // each frame's surface straight from the surface context's swapchain, the
    // same arrangement as upstream's Android surface.
    auto surface = std::make_unique<GPUSurfaceVulkanImpeller>(
        /*delegate=*/nullptr, vk_context_->GetSurfaceContext());
    if (!surface->IsValid()) {
      FML_LOG(ERROR) << "The Impeller Vulkan surface came up invalid.";
      return nullptr;
    }
    return surface;
  }

  /// Builds the Impeller surface, or returns nullptr with a logged reason.
  std::unique_ptr<Surface> CreateImpellerSurface() {
    if (state_->xid == 0) {
      FML_LOG(ERROR) << "No X11 window to put an EGL surface on.";
      return nullptr;
    }
    gl_delegate_ =
        std::make_unique<ImpellerGlDelegate>(gl_context_.get(), state_->xid);
    if (!gl_delegate_->IsValid()) {
      gl_delegate_.reset();
      return nullptr;
    }

    // GPUSurfaceGLImpeller's constructor builds an AiksContext, which compiles
    // pipelines through Impeller's reactor -- and the reactor refuses to run on
    // a thread with no current GL context. So the context goes current before
    // the surface is built, and stays that way; the rasterizer makes it current
    // again each frame regardless.
    auto made_current = gl_delegate_->GLContextMakeCurrent();
    if (!made_current || !made_current->GetResult()) {
      FML_LOG(ERROR) << "Could not make the GL context current on the raster "
                        "thread.";
      gl_delegate_.reset();
      return nullptr;
    }

    auto surface = std::make_unique<GPUSurfaceGLImpeller>(
        gl_delegate_.get(), gl_context_->GetImpellerContext(),
        /*render_to_surface=*/true);
    if (!surface->IsValid()) {
      FML_LOG(ERROR) << "The Impeller GL surface came up invalid.";
      gl_delegate_.reset();
      return nullptr;
    }
    return surface;
  }

  WindowState* state_ = nullptr;
  RenderBackend backend_ = RenderBackend::kSoftware;
  std::unique_ptr<ImpellerVkContext> vk_context_;
  std::unique_ptr<ImpellerGlContext> gl_context_;
  std::unique_ptr<ImpellerGlDelegate> gl_delegate_;
  sk_sp<SkSurface> backing_store_;

  FML_DISALLOW_COPY_AND_ASSIGN(HostPlatformView);
};

//------------------------------------------------------------------------------
/// Builds a PointerData for one mouse event.
///
/// A desktop reports a single system mouse, so device and pointer identity are
/// both constant. Coordinates arrive in logical pixels and are converted to
/// the physical pixels the engine works in by the callers.
PointerData MakePointerData(WindowState* state,
                            PointerData::Change change,
                            double x,
                            double y) {
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
  data.pressure = state->buttons != 0 ? 1.0 : 0.0;
  data.pressure_max = 1.0;
  data.view_id = kFlutterImplicitViewId;
  state->last_x = x;
  state->last_y = y;
  return data;
}

/// A wheel turn, as a hover carrying a scroll signal.
///
/// It is not its own change: the pointer did not go anywhere, and a recogniser
/// that read the change would see a mouse being moved. The signal is what says
/// otherwise.
PointerData MakeScrollData(WindowState* state,
                           double x,
                           double y,
                           double delta_x,
                           double delta_y) {
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

  state->platform_task_runner->PostTask(
      [view = state->shell->GetPlatformView(), metrics]() {
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

//------------------------------------------------------------------------------
// The window: GTK signal handlers, all on the main thread. Each takes the
// WindowState through the user-data pointer; teardown nulls the state's shell
// and view fields before the widgets go, which is the mac host's arrangement.

WindowState* StateOf(gpointer user_data) {
  return static_cast<WindowState*>(user_data);
}

void SendPointerIfLive(WindowState* state, const PointerData& data) {
  if (state != nullptr && state->platform_view != nullptr) {
    state->platform_view->SendPointer(data);
  }
}

gboolean OnDraw(GtkWidget* widget, cairo_t* cr, gpointer user_data) {
  WindowState* state = StateOf(user_data);
  if (state == nullptr) {
    return FALSE;
  }
  // On the GL path the frame is already on the window, put there by
  // eglSwapBuffers; painting anything here would erase it.
  if (state->gpu_active.load()) {
    return TRUE;
  }
  if (!state->frame_buffer.Paint(cr, gtk_widget_get_scale_factor(widget))) {
    // Nothing has been rasterised yet. Painting the background rather than
    // leaving whatever was there means the first moment of the app is a blank
    // window rather than a torn one.
    cairo_set_source_rgb(cr, 0, 0, 0);
    cairo_paint(cr);
  }
  return TRUE;
}

gboolean OnButton(GtkWidget* widget, GdkEventButton* event, gpointer user) {
  WindowState* state = StateOf(user);
  if (state == nullptr || event->type == GDK_2BUTTON_PRESS ||
      event->type == GDK_3BUTTON_PRESS) {
    // The synthetic double- and triple-click events repeat a press that was
    // already sent; the framework's own recognisers count clicks themselves.
    return TRUE;
  }
  int64_t bit = 0;
  switch (event->button) {
    case 1:
      bit = kPointerButtonMousePrimary;
      break;
    case 2:
      bit = kPointerButtonMouseMiddle;
      break;
    case 3:
      bit = kPointerButtonMouseSecondary;
      break;
    default:
      return TRUE;
  }
  const int64_t before = state->buttons;
  PointerData::Change change;
  if (event->type == GDK_BUTTON_PRESS) {
    state->buttons |= bit;
    change =
        before == 0 ? PointerData::Change::kDown : PointerData::Change::kMove;
    gtk_widget_grab_focus(widget);
  } else {
    state->buttons &= ~bit;
    change = state->buttons == 0 ? PointerData::Change::kUp
                                 : PointerData::Change::kMove;
  }
  const double scale = state->device_pixel_ratio;
  SendPointerIfLive(state, MakePointerData(state, change, event->x * scale,
                                           event->y * scale));
  return TRUE;
}

gboolean OnMotion(GtkWidget* widget, GdkEventMotion* event, gpointer user) {
  WindowState* state = StateOf(user);
  if (state == nullptr) {
    return TRUE;
  }
  const double scale = state->device_pixel_ratio;
  const PointerData::Change change = state->buttons != 0
                                         ? PointerData::Change::kMove
                                         : PointerData::Change::kHover;
  SendPointerIfLive(state, MakePointerData(state, change, event->x * scale,
                                           event->y * scale));
  return TRUE;
}

gboolean OnCrossing(GtkWidget* widget, GdkEventCrossing* event, gpointer user) {
  WindowState* state = StateOf(user);
  if (state == nullptr) {
    return TRUE;
  }
  const double scale = state->device_pixel_ratio;
  const PointerData::Change change = event->type == GDK_ENTER_NOTIFY
                                         ? PointerData::Change::kAdd
                                         : PointerData::Change::kRemove;
  SendPointerIfLive(state, MakePointerData(state, change, event->x * scale,
                                           event->y * scale));
  return TRUE;
}

gboolean OnScroll(GtkWidget* widget, GdkEventScroll* event, gpointer user) {
  WindowState* state = StateOf(user);
  if (state == nullptr) {
    return TRUE;
  }
  // Upstream's `fl_scrolling_manager`: a discrete wheel turn is one notch in
  // its direction, a smooth event carries its own deltas, and both are scaled
  // by Chromium's 53-pixels-per-notch. The sign is not inverted for a mouse --
  // positive y is the wheel rolling down, which is the scroll offset growing.
  double delta_x = 0.0;
  double delta_y = 0.0;
  switch (event->direction) {
    case GDK_SCROLL_UP:
      delta_y = -1.0;
      break;
    case GDK_SCROLL_DOWN:
      delta_y = 1.0;
      break;
    case GDK_SCROLL_LEFT:
      delta_x = -1.0;
      break;
    case GDK_SCROLL_RIGHT:
      delta_x = 1.0;
      break;
    case GDK_SCROLL_SMOOTH:
      gdk_event_get_scroll_deltas(reinterpret_cast<GdkEvent*>(event), &delta_x,
                                  &delta_y);
      break;
  }
  const double scale = state->device_pixel_ratio;
  constexpr double kScrollOffsetMultiplier = 53.0;
  SendPointerIfLive(state,
                    MakeScrollData(state, event->x * scale, event->y * scale,
                                   delta_x * kScrollOffsetMultiplier * scale,
                                   delta_y * kScrollOffsetMultiplier * scale));
  return TRUE;
}

/// One code point as UTF-16, surrogates included.
std::u16string CodePointToUtf16(uint32_t code_point) {
  std::u16string out;
  if (code_point < 0x10000) {
    out.push_back(static_cast<char16_t>(code_point));
  } else {
    const uint32_t v = code_point - 0x10000;
    out.push_back(static_cast<char16_t>(0xd800 + (v >> 10)));
    out.push_back(static_cast<char16_t>(0xdc00 + (v & 0x3ff)));
  }
  return out;
}

// -- The input method's signals, all on the GTK thread ------------------------

void OnImCommit(GtkIMContext* im_context, gchar* text, gpointer user) {
  WindowState* state = StateOf(user);
  if (state != nullptr && text != nullptr) {
    state->text_input.ImCommit(text);
  }
}

void OnImPreeditStart(GtkIMContext* im_context, gpointer user) {
  WindowState* state = StateOf(user);
  if (state != nullptr) {
    state->text_input.ImPreeditStart();
  }
}

void OnImPreeditChanged(GtkIMContext* im_context, gpointer user) {
  WindowState* state = StateOf(user);
  if (state != nullptr) {
    state->text_input.ImPreeditChanged(im_context);
  }
}

void OnImPreeditEnd(GtkIMContext* im_context, gpointer user) {
  WindowState* state = StateOf(user);
  if (state != nullptr) {
    state->text_input.ImPreeditEnd();
  }
}

gboolean OnImRetrieveSurrounding(GtkIMContext* im_context, gpointer user) {
  WindowState* state = StateOf(user);
  return state != nullptr &&
         state->text_input.ImRetrieveSurrounding(im_context);
}

gboolean OnImDeleteSurrounding(GtkIMContext* im_context,
                               gint offset,
                               gint n_chars,
                               gpointer user) {
  WindowState* state = StateOf(user);
  return state != nullptr &&
         state->text_input.ImDeleteSurrounding(offset, n_chars);
}

gboolean OnKey(GtkWidget* widget, GdkEventKey* event, gpointer user) {
  WindowState* state = StateOf(user);
  if (state == nullptr || state->platform_view == nullptr) {
    return TRUE;
  }
  const bool is_press = event->type == GDK_KEY_PRESS;
  const uint64_t physical = PhysicalKeyForKeycode(event->hardware_keycode);
  const uint32_t lowered_code_point =
      gdk_keyval_to_unicode(gdk_keyval_to_lower(event->keyval));
  const uint64_t logical =
      LogicalKeyForKeyval(event->keyval, lowered_code_point);
  const uint32_t code_point = gdk_keyval_to_unicode(event->keyval);
  const bool printable = code_point >= 0x20 && code_point != 0x7f;

  // GDK reports a held key as a stream of identical presses and says nothing
  // about which are repeats; whether this physical key is already down is what
  // says. A release that was eaten by a focus change would wedge the set, so
  // focus-out clears it.
  KeyEventType type;
  if (is_press) {
    type = state->pressed_physical_keys.count(physical) > 0
               ? KeyEventType::kRepeat
               : KeyEventType::kDown;
    state->pressed_physical_keys.insert(physical);
  } else {
    type = KeyEventType::kUp;
    state->pressed_physical_keys.erase(physical);
  }

  // The input method first, presses and releases both, as upstream: a
  // composing method eats the keys it builds preedit from and answers through
  // its signals. Only with a field attached -- otherwise a bare context would
  // swallow dead keys with nothing to type into.
  bool im_handled = false;
  if (state->im_context != nullptr && state->text_input.attached()) {
    im_handled = gtk_im_context_filter_keypress(state->im_context, event);
  }

  // The focused text field next, which is what the Windows host's window
  // proc does: an editing key edits, Enter submits, and a printable character
  // types -- unless a modifier makes it a shortcut instead.
  if (is_press && !im_handled && state->text_input.attached()) {
    const bool shift = (event->state & GDK_SHIFT_MASK) != 0;
    const bool shortcut =
        (event->state & (GDK_CONTROL_MASK | GDK_MOD1_MASK)) != 0;
    if (event->keyval == GDK_KEY_Return || event->keyval == GDK_KEY_KP_Enter) {
      state->text_input.OnAction();
    } else if (state->text_input.OnEditingKey(event->keyval, shift)) {
      // Consumed as an edit.
    } else if (printable && !shortcut) {
      state->text_input.OnText(CodePointToUtf16(code_point));
    }
  }

  KeyData data;
  data.Clear();
  data.timestamp = static_cast<uint64_t>(event->time) * 1000;
  data.type = type;
  data.physical = physical;
  data.logical = logical;
  data.synthesized = 0;

  // The character is what the key produced, and only a press produces one. A
  // repeat carries it too, which is what makes held keys type. Control
  // characters are what a key *is*, not what it typed: enter is not a
  // carriage return in a text field, it is an action.
  std::string character;
  if (is_press && printable) {
    gchar utf8[8] = {};
    const gint length = g_unichar_to_utf8(code_point, utf8);
    character.assign(utf8, static_cast<size_t>(length));
  }
  state->platform_view->SendKey(data, character);
  return TRUE;
}

void UpdateMetrics(WindowState* state) {
  if (state == nullptr || state->drawing_area == nullptr) {
    return;
  }
  const int scale = gtk_widget_get_scale_factor(state->drawing_area);
  state->device_pixel_ratio = scale > 0 ? scale : 1;
  state->physical_width =
      gtk_widget_get_allocated_width(state->drawing_area) * scale;
  state->physical_height =
      gtk_widget_get_allocated_height(state->drawing_area) * scale;
  SendViewportMetrics(state, state->physical_width, state->physical_height);
  if (state->platform_view != nullptr) {
    state->platform_view->OnWindowResized();
  }
}

void OnSizeAllocate(GtkWidget* widget,
                    GdkRectangle* allocation,
                    gpointer user) {
  UpdateMetrics(StateOf(user));
}

void OnScaleFactorChanged(GObject* object, GParamSpec* pspec, gpointer user) {
  UpdateMetrics(StateOf(user));
}

gboolean OnFocusChange(GtkWidget* widget, GdkEventFocus* event, gpointer user) {
  WindowState* state = StateOf(user);
  if (state == nullptr) {
    return FALSE;
  }
  if (event->in) {
    // The input method follows the window's focus, as upstream's view does --
    // a context that stayed unfocused would never open a candidates window.
    if (state->im_context != nullptr) {
      gtk_im_context_focus_in(state->im_context);
    }
    SendLifecycle(state, "AppLifecycleState.resumed");
  } else {
    if (state->im_context != nullptr) {
      gtk_im_context_focus_out(state->im_context);
    }
    // A release delivered to whoever took the focus never reaches this
    // window; without this the key would look held forever and every
    // following press of it would be a repeat.
    state->pressed_physical_keys.clear();
    SendLifecycle(state, "AppLifecycleState.inactive");
  }
  return FALSE;
}

gboolean OnWindowStateEvent(GtkWidget* widget,
                            GdkEventWindowState* event,
                            gpointer user) {
  WindowState* state = StateOf(user);
  if (state == nullptr ||
      (event->changed_mask & GDK_WINDOW_STATE_ICONIFIED) == 0) {
    return FALSE;
  }
  const bool iconified =
      (event->new_window_state & GDK_WINDOW_STATE_ICONIFIED) != 0;
  SendLifecycle(state, iconified ? "AppLifecycleState.paused"
                                 : "AppLifecycleState.resumed");
  return FALSE;
}

gboolean OnDeleteEvent(GtkWidget* widget, GdkEvent* event, gpointer user) {
  WindowState* state = StateOf(user);
  if (state != nullptr && state->loop != nullptr) {
    // Closing the window ends the application, which is what a single-window
    // desktop app means by it. TRUE keeps GTK from destroying the window now:
    // the rasterizer may still be presenting to it, and teardown destroys it
    // once the shell is gone.
    g_main_loop_quit(state->loop);
  }
  return TRUE;
}

void OnThemeChanged(GObject* object, GParamSpec* pspec, gpointer user) {
  WindowState* state = StateOf(user);
  if (state != nullptr && state->platform_view != nullptr) {
    state->platform_view->SendSettingsJson(SettingsPayload());
  }
}

/// The display's refresh rate in hertz, best effort. GTK thread.
double ReadDisplayRefreshRate() {
  GdkDisplay* display = gdk_display_get_default();
  if (display == nullptr) {
    return 60.0;
  }
  GdkMonitor* monitor = gdk_display_get_primary_monitor(display);
  if (monitor == nullptr && gdk_display_get_n_monitors(display) > 0) {
    monitor = gdk_display_get_monitor(display, 0);
  }
  if (monitor == nullptr) {
    return 60.0;
  }
  // Millihertz; zero when the backend does not know, which under WSLg it
  // often does not, and sixty is the right guess there.
  const int millihertz = gdk_monitor_get_refresh_rate(monitor);
  return millihertz > 1000 ? millihertz / 1000.0 : 60.0;
}

}  // namespace
}  // namespace flutter

int32_t rf_host_run(const RfHostOptions* options) {
  using namespace flutter;  // NOLINT(build/namespaces)

  if (options == nullptr || options->width <= 0 || options->height <= 0) {
    return -1;
  }

  Settings settings;
  // The environment wins over the application's preference, so a rendering
  // problem can be bisected without rebuilding: RUSTFLUTTER_SOFTWARE=1 forces
  // the Skia software surface.
  const char* force_software = std::getenv("RUSTFLUTTER_SOFTWARE");
  const bool software_forced = force_software != nullptr &&
                               force_software[0] != '\0' &&
                               force_software[0] != '0';
  RenderBackend backend = RenderBackend::kGles;
  if (options->enable_impeller == 0 || software_forced) {
    backend = RenderBackend::kSoftware;
  } else {
    // RUSTFLUTTER_BACKEND picks between the GPU backends: "vulkan" asks for
    // Impeller on Vulkan, "software" is the software surface spelled the
    // other way, and anything else -- including unset -- is the GLES default.
    const char* requested = std::getenv("RUSTFLUTTER_BACKEND");
    if (requested != nullptr) {
      if (std::strcmp(requested, "vulkan") == 0) {
        backend = RenderBackend::kVulkan;
      } else if (std::strcmp(requested, "software") == 0) {
        backend = RenderBackend::kSoftware;
      } else if (std::strcmp(requested, "gles") != 0) {
        FML_LOG(IMPORTANT) << "Unknown RUSTFLUTTER_BACKEND value \""
                           << requested << "\"; using OpenGL ES.";
      }
    }
  }
  if (software_forced) {
    FML_LOG(IMPORTANT) << "RUSTFLUTTER_SOFTWARE is set; using the software "
                          "surface.";
  }
  settings.enable_impeller = backend != RenderBackend::kSoftware;
  settings.enable_software_rendering = !settings.enable_impeller;
  settings.icu_initialization_required = true;
  settings.icu_data_path = options->icu_data_path != nullptr
                               ? std::string(options->icu_data_path)
                               : DefaultIcuDataPath();
  // Nothing to prefetch and nothing to warn about: there is no Dart snapshot,
  // and the Impeller opt-out warning is aimed at applications that still have
  // a choice.
  settings.warn_on_impeller_opt_out = false;

  WindowState state;

  // -- Window (this thread) ---------------------------------------------------

  // X11 rather than Wayland, before gtk_init decides for itself: the EGL
  // surface is made from an X window id, and under WSLg -- where both exist --
  // GTK would pick Wayland. Every desktop that has Wayland has XWayland.
  gdk_set_allowed_backends("x11");
  if (!gtk_init_check(nullptr, nullptr)) {
    FML_LOG(ERROR) << "Could not initialise GTK. Is DISPLAY set?";
    return -2;
  }

  g_display_refresh_hz.store(ReadDisplayRefreshRate());

  GtkWidget* window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
  gtk_window_set_title(GTK_WINDOW(window), options->title != nullptr
                                               ? options->title
                                               : "rustflutter");
  gtk_window_set_default_size(GTK_WINDOW(window), options->width,
                              options->height);

  GtkWidget* drawing_area = gtk_drawing_area_new();
  // The host paints every pixel of every frame, so GTK's own background would
  // only ever be painted over.
  gtk_widget_set_app_paintable(drawing_area, TRUE);
  gtk_widget_set_can_focus(drawing_area, TRUE);
  gtk_widget_add_events(drawing_area,
                        GDK_POINTER_MOTION_MASK | GDK_BUTTON_PRESS_MASK |
                            GDK_BUTTON_RELEASE_MASK | GDK_SCROLL_MASK |
                            GDK_SMOOTH_SCROLL_MASK | GDK_ENTER_NOTIFY_MASK |
                            GDK_LEAVE_NOTIFY_MASK);
  gtk_container_add(GTK_CONTAINER(window), drawing_area);

  state.window = window;
  state.drawing_area = drawing_area;
  state.loop = g_main_loop_new(nullptr, FALSE);

  g_signal_connect(drawing_area, "draw", G_CALLBACK(OnDraw), &state);
  g_signal_connect(drawing_area, "button-press-event", G_CALLBACK(OnButton),
                   &state);
  g_signal_connect(drawing_area, "button-release-event", G_CALLBACK(OnButton),
                   &state);
  g_signal_connect(drawing_area, "motion-notify-event", G_CALLBACK(OnMotion),
                   &state);
  g_signal_connect(drawing_area, "enter-notify-event", G_CALLBACK(OnCrossing),
                   &state);
  g_signal_connect(drawing_area, "leave-notify-event", G_CALLBACK(OnCrossing),
                   &state);
  g_signal_connect(drawing_area, "scroll-event", G_CALLBACK(OnScroll), &state);
  g_signal_connect(drawing_area, "size-allocate", G_CALLBACK(OnSizeAllocate),
                   &state);
  g_signal_connect(drawing_area, "notify::scale-factor",
                   G_CALLBACK(OnScaleFactorChanged), &state);
  g_signal_connect(window, "key-press-event", G_CALLBACK(OnKey), &state);
  g_signal_connect(window, "key-release-event", G_CALLBACK(OnKey), &state);
  g_signal_connect(window, "focus-in-event", G_CALLBACK(OnFocusChange), &state);
  g_signal_connect(window, "focus-out-event", G_CALLBACK(OnFocusChange),
                   &state);
  g_signal_connect(window, "window-state-event", G_CALLBACK(OnWindowStateEvent),
                   &state);
  g_signal_connect(window, "delete-event", G_CALLBACK(OnDeleteEvent), &state);
  gulong theme_handler = 0;
  GtkSettings* gtk_settings = gtk_settings_get_default();
  if (gtk_settings != nullptr) {
    theme_handler = g_signal_connect(
        gtk_settings, "notify::gtk-application-prefer-dark-theme",
        G_CALLBACK(OnThemeChanged), &state);
  }

  // Realized rather than shown: the X window has to exist for the EGL surface
  // to be made from it, but showing it before the shell exists would flash an
  // empty rectangle.
  gtk_widget_realize(window);
  gtk_widget_realize(drawing_area);

  // The input method. A multicontext follows GTK_IM_MODULE -- ibus, fcitx,
  // or the built-in simple context that handles dead keys and Compose.
  GtkIMContext* im_context = gtk_im_multicontext_new();
  gtk_im_context_set_client_window(im_context, gtk_widget_get_window(window));
  g_signal_connect(im_context, "commit", G_CALLBACK(OnImCommit), &state);
  g_signal_connect(im_context, "preedit-start", G_CALLBACK(OnImPreeditStart),
                   &state);
  g_signal_connect(im_context, "preedit-changed",
                   G_CALLBACK(OnImPreeditChanged), &state);
  g_signal_connect(im_context, "preedit-end", G_CALLBACK(OnImPreeditEnd),
                   &state);
  g_signal_connect(im_context, "retrieve-surrounding",
                   G_CALLBACK(OnImRetrieveSurrounding), &state);
  g_signal_connect(im_context, "delete-surrounding",
                   G_CALLBACK(OnImDeleteSurrounding), &state);
  state.im_context = im_context;
  state.text_input.SetImContext(im_context);

  if (settings.enable_impeller) {
    GdkWindow* gdk_window = gtk_widget_get_window(drawing_area);
    if (gdk_window != nullptr && GDK_IS_X11_WINDOW(gdk_window)) {
      // A C-style cast because EGLNativeWindowType is an unsigned long with
      // X11 headers in scope and a pointer without them.
      state.xid = (EGLNativeWindowType)GDK_WINDOW_XID(gdk_window);
      state.xdisplay = GDK_WINDOW_XDISPLAY(gdk_window);
      // Cairo must not double-buffer over the GL swap. Deprecated because
      // GTK4 removed the alternative, not because GTK3 minds.
      G_GNUC_BEGIN_IGNORE_DEPRECATIONS
      gtk_widget_set_double_buffered(drawing_area, FALSE);
      G_GNUC_END_IGNORE_DEPRECATIONS
    } else {
      FML_LOG(IMPORTANT) << "The GDK window is not X11; Impeller needs an X "
                            "window and will fall back to software.";
    }
  }

  const int scale = gtk_widget_get_scale_factor(drawing_area);
  state.device_pixel_ratio = scale > 0 ? scale : 1;
  state.physical_width = options->width * scale;
  state.physical_height = options->height * scale;

  // -- Threads ----------------------------------------------------------------

  ThreadHost thread_host("rf",
                         ThreadHost::Type::kPlatform | ThreadHost::Type::kUi |
                             ThreadHost::Type::kRaster | ThreadHost::Type::kIo);

  TaskRunners task_runners("rustflutter",
                           thread_host.platform_thread->GetTaskRunner(),
                           thread_host.raster_thread->GetTaskRunner(),
                           thread_host.ui_thread->GetTaskRunner(),
                           thread_host.io_thread->GetTaskRunner());

  // -- Shell ------------------------------------------------------------------

  PlatformData platform_data;
  std::unique_ptr<Shell> shell = Shell::Create(
      platform_data, task_runners, settings,
      [&state, backend](Shell& shell) {
        auto view = std::make_unique<HostPlatformView>(
            shell, shell.GetTaskRunners(), &state, backend);
        // The window needs to reach the view to send pointers and keys. The
        // shell owns it and outlives the run loop, so a raw pointer is
        // enough.
        state.platform_view = view.get();
        // How an editing state gets back to the framework. The view outlives
        // the handler's use of this: both die with the main loop, and the
        // shell is torn down after it.
        state.text_input.SetSender(
            [sender = view.get()](const std::string& method,
                                  const std::string& arguments) {
              sender->SendMethodCall(kTextInputChannel, method, arguments);
            });
        return view;
      },
      [](Shell& shell) { return std::make_unique<Rasterizer>(shell); });

  if (shell == nullptr || !shell->IsSetup()) {
    gtk_widget_destroy(window);
    g_main_loop_unref(state.loop);
    return -4;
  }
  state.shell = shell.get();
  state.platform_task_runner = task_runners.GetPlatformTaskRunner();

  // The settings payload reads GTK, so it is built here on the GTK thread and
  // travels into the startup task as a string.
  const std::string settings_payload = SettingsPayload();

  // Everything below belongs to the platform thread: RunEngine checks for it,
  // and NotifyCreated / SetViewportMetrics reach the platform view directly.
  // Ordering matters -- the surface has to exist before the first frame is
  // rasterized, and the framework needs a size before it can lay anything
  // out.
  task_runners.GetPlatformTaskRunner()->PostTask(
      fml::MakeCopyable([shell = shell.get(), &state, settings_payload]() {
        shell->RunEngine(RunConfiguration{});
        if (auto view = shell->GetPlatformView()) {
          view->NotifyCreated();
        }
        // The engine asks the display manager for the refresh rate when it
        // reports frame timings and when it decides how far ahead to
        // schedule. Without this it has no displays at all and falls back to
        // a guess. Written out as flutter::Display because Xlib typedefs the
        // bare name to its own connection type.
        std::vector<std::unique_ptr<flutter::Display>> displays;
        displays.push_back(std::make_unique<flutter::Display>(
            /*display_id=*/0, g_display_refresh_hz.load(), state.physical_width,
            state.physical_height, state.device_pixel_ratio));
        shell->OnDisplayUpdates(std::move(displays));
        SendViewportMetrics(&state, state.physical_width,
                            state.physical_height);
        // Before the first frame, so that an application choosing between the
        // light and the dark theme in its first `build` chooses correctly
        // rather than showing one frame of the wrong one.
        state.platform_view->SendSettingsJson(settings_payload);
        state.platform_view->SendLocalization();
      }));

  gtk_widget_show_all(window);
  gtk_widget_grab_focus(drawing_area);

  // Every lifecycle report is made from this thread, including this first
  // one. `focus-in-event` has usually made it by now, in which case this is a
  // no-op.
  SendLifecycle(&state, "AppLifecycleState.resumed");

  // -- Run loop ---------------------------------------------------------------

  g_main_loop_run(state.loop);

  // The shell must be destroyed on the platform thread -- its destructor
  // checks, because it drains the UI, raster and IO threads in order and
  // would deadlock if it were not the one holding the platform thread. The
  // window outlives the shell: the rasterizer's surface presents into it
  // until NotifyDestroyed.
  state.shell = nullptr;
  state.platform_view = nullptr;
  state.im_context = nullptr;
  state.text_input.SetImContext(nullptr);
  g_object_unref(im_context);
  if (theme_handler != 0 && gtk_settings != nullptr) {
    g_signal_handler_disconnect(gtk_settings, theme_handler);
  }
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

  gtk_widget_destroy(window);
  g_main_loop_unref(state.loop);

  return 0;
}
