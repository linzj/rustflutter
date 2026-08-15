// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// The Windows host: a Win32 window, the engine's own thread model, and a real
// Shell driving the Rust framework.
//
// Structure, and why:
//
//   * The window lives on the process's main thread and owns the Win32 message
//     loop, because Win32 delivers messages only to the thread that created the
//     window.
//
//   * The shell's platform / UI / raster / IO threads come from ThreadHost, so
//     they are the same fml threads the engine uses everywhere else. The window
//     thread is deliberately *not* the platform thread: making it so would mean
//     interleaving fml::MessageLoop with GetMessage, which is the one piece of
//     bookkeeping the Windows embedder spends real complexity on.
//
//   * Everything the window learns (size, close) is posted to the platform task
//     runner; everything the raster thread produces is posted back with
//     PostMessage. Neither side touches the other's state directly.
//
// Rendering goes through Impeller on ANGLE when the app asks for it, and
// through GPUSurfaceSoftware otherwise. The software path needs nothing and
// works everywhere; the Impeller path needs a D3D11 device and falls back to
// software rather than failing to start if it cannot get one.

#include "flutter/rust/host/rustflutter_host.h"

#include <windows.h>
#include <dwmapi.h>
#include <windowsx.h>

#include <cstdlib>
#include <memory>
#include <optional>
#include <mutex>
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
#include "flutter/impeller/renderer/context.h"
#include "flutter/lib/ui/window/key_data.h"
#include "flutter/lib/ui/window/key_data_packet.h"
#include "flutter/lib/ui/window/platform_message.h"
#include "flutter/lib/ui/window/pointer_data.h"
#include "flutter/lib/ui/window/pointer_data_packet.h"
#include "flutter/lib/ui/window/viewport_metrics.h"
#include "flutter/shell/common/platform_view.h"
#include "flutter/shell/common/rasterizer.h"
#include "flutter/shell/common/run_configuration.h"
#include "flutter/shell/common/shell.h"
#include "flutter/shell/common/thread_host.h"
#include "flutter/shell/common/display.h"
#include "flutter/shell/common/vsync_waiter.h"
#include "flutter/rust/ffi/rustflutter_ffi.h"
#include "flutter/rust/ffi/rustflutter_ffi_internal.h"
#include "flutter/rust/host/rustflutter_gl_win.h"
#include "flutter/rust/host/rustflutter_key_map_win.h"
#include "flutter/shell/gpu/gpu_surface_gl_impeller.h"
#include "flutter/shell/gpu/gpu_surface_software.h"
#include "flutter/shell/gpu/gpu_surface_software_delegate.h"
#include "rapidjson/document.h"
#include "rapidjson/stringbuffer.h"
#include "rapidjson/writer.h"
#include "third_party/skia/include/core/SkSurface.h"

namespace flutter {
namespace {

constexpr wchar_t kWindowClass[] = L"RustflutterHostWindow";

/// Where key events go. Matched by RuntimeController, which is the only reader.
/// Upstream this same string is in embedder.cc, platform_dispatcher.dart,
/// KeyData.java and FlutterEngine.mm -- an embedder is expected to spell it out.
constexpr char kKeyDataChannel[] = "flutter/keydata";

//------------------------------------------------------------------------------
/// Per-monitor DPI, bound at run time.
///
/// A window that does not say otherwise is DPI-unaware: Windows lies to it
/// about its own size and bitmap-scales whatever it draws, which on a 200%
/// display means every glyph goes through a stretch. Saying otherwise means
/// declaring per-monitor awareness *before* the first window exists, and then
/// tracking the scale as the window moves between displays.
///
/// The three entry points arrived in Windows 10 1607 / 1703. They are looked up
/// rather than linked so that an older machine keeps the previous behaviour --
/// unaware, blurry, but running -- instead of failing to start.
///
/// Upstream this is `WindowsProcTable` plus `Win32Window`, doing the same three
/// things for the same reasons.
class DpiApi {
 public:
  static const DpiApi& Get() {
    static const DpiApi instance;
    return instance;
  }

  /// Declares the process per-monitor aware. Must run before any window is
  /// created; Windows ignores it afterwards.
  void MakeProcessPerMonitorAware() const {
    // ((DPI_AWARENESS_CONTEXT)-4), spelled out because the constant only
    // exists in headers new enough to declare the function too.
    HANDLE kPerMonitorAwareV2 = reinterpret_cast<HANDLE>(static_cast<intptr_t>(-4));
    if (set_process_dpi_awareness_context_ != nullptr) {
      set_process_dpi_awareness_context_(kPerMonitorAwareV2);
    }
  }

  /// The window's scale factor: 1.0 at 96 DPI, 1.5 at 144, and so on. This is
  /// the `devicePixelRatio` the framework lays out against.
  double ScaleForWindow(HWND window) const {
    if (get_dpi_for_window_ == nullptr) {
      return 1.0;
    }
    const UINT dpi = get_dpi_for_window_(window);
    return dpi > 0 ? static_cast<double>(dpi) / USER_DEFAULT_SCREEN_DPI : 1.0;
  }

  /// Grows a client rectangle by the window frame at a given DPI. The frame is
  /// itself scaled, so doing this at 96 leaves the client area short by the
  /// difference on any display that is not at 100%.
  void AdjustForDpi(RECT* rect, DWORD style, UINT dpi) const {
    if (adjust_window_rect_ex_for_dpi_ != nullptr) {
      adjust_window_rect_ex_for_dpi_(rect, style, FALSE, 0, dpi);
    } else {
      AdjustWindowRect(rect, style, FALSE);
    }
  }

 private:
  DpiApi() {
    HMODULE user32 = GetModuleHandleW(L"user32.dll");
    if (user32 == nullptr) {
      return;
    }
    auto load = [user32](const char* name) {
      return GetProcAddress(user32, name);
    };
    set_process_dpi_awareness_context_ =
        reinterpret_cast<BOOL(WINAPI*)(HANDLE)>(
            load("SetProcessDpiAwarenessContext"));
    get_dpi_for_window_ =
        reinterpret_cast<UINT(WINAPI*)(HWND)>(load("GetDpiForWindow"));
    adjust_window_rect_ex_for_dpi_ =
        reinterpret_cast<BOOL(WINAPI*)(LPRECT, DWORD, BOOL, DWORD, UINT)>(
            load("AdjustWindowRectExForDpi"));
  }

  BOOL(WINAPI* set_process_dpi_awareness_context_)(HANDLE) = nullptr;
  UINT(WINAPI* get_dpi_for_window_)(HWND) = nullptr;
  BOOL(WINAPI* adjust_window_rect_ex_for_dpi_)(LPRECT, DWORD, BOOL, DWORD,
                                               UINT) = nullptr;
};

// Posted by the raster thread once a frame has been copied into the shared
// buffer. WM_APP is the first message id reserved for applications.
constexpr UINT kMessageFramePresented = WM_APP + 1;

//------------------------------------------------------------------------------
/// The pixels the window paints, and the lock that lets two threads share them.
///
/// The raster thread writes; the window thread reads inside WM_PAINT. Copying
/// rather than blitting straight from the raster thread keeps all GDI calls on
/// the thread that owns the window, which is the rule that is easiest to keep.
class FrameBuffer {
 public:
  void Store(const void* pixels, int32_t width, int32_t height) {
    const size_t bytes =
        static_cast<size_t>(width) * static_cast<size_t>(height) * 4u;
    std::lock_guard<std::mutex> lock(mutex_);
    pixels_.resize(bytes);
    memcpy(pixels_.data(), pixels, bytes);
    width_ = width;
    height_ = height;
  }

  // Paints into `hdc`, scaling to `client`. Returns false if no frame has
  // arrived yet.
  bool Paint(HDC hdc, const RECT& client) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (pixels_.empty() || width_ <= 0 || height_ <= 0) {
      return false;
    }

    BITMAPINFO info = {};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = width_;
    // Negative height means a top-down DIB, which matches the row order Skia
    // hands back. Without this the frame appears vertically mirrored.
    info.bmiHeader.biHeight = -height_;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;

    SetStretchBltMode(hdc, HALFTONE);
    StretchDIBits(hdc,
                  /*xDest=*/0, /*yDest=*/0,
                  /*DestWidth=*/client.right - client.left,
                  /*DestHeight=*/client.bottom - client.top,
                  /*xSrc=*/0, /*ySrc=*/0,
                  /*SrcWidth=*/width_, /*SrcHeight=*/height_, pixels_.data(),
                  &info, DIB_RGB_COLORS, SRCCOPY);
    return true;
  }

 private:
  std::mutex mutex_;
  std::vector<uint8_t> pixels_;
  int32_t width_ = 0;
  int32_t height_ = 0;
};

//------------------------------------------------------------------------------
/// Asks Windows for a one millisecond timer, for as long as this object lives.
///
/// The default resolution is about fifteen point six milliseconds, which is
/// most of a frame, so the vsync waiter cannot ask to be woken on a frame
/// boundary and be woken on one. Measured, with RUSTFLUTTER_FORCE_HZ driving
/// the waiter at a rate this machine does not have:
///
///     rate     without      with
///     59 Hz    63-66 fps    58.6-59.4 fps
///     75 Hz    74.7 fps     74.7 fps
///
/// Seventy-five survives because its interval is close to five ticks. The
/// error is not a clean doubling or halving -- the frames come out *fast* at
/// fifty-nine, because a callback that arrives late lands back on the grid
/// point it was aiming at rather than the next one. Either way the app runs at
/// a rate the display does not have.
class HighResolutionTimer {
 public:
  HighResolutionTimer() {
    winmm_ = LoadLibraryW(L"winmm.dll");
    if (winmm_ == nullptr) {
      return;
    }
    begin_period_ = reinterpret_cast<PeriodFn>(
        GetProcAddress(winmm_, "timeBeginPeriod"));
    end_period_ =
        reinterpret_cast<PeriodFn>(GetProcAddress(winmm_, "timeEndPeriod"));
    if (begin_period_ != nullptr && begin_period_(1) == 0) {
      held_ = true;
    }
  }

  ~HighResolutionTimer() {
    if (held_ && end_period_ != nullptr) {
      end_period_(1);
    }
    if (winmm_ != nullptr) {
      FreeLibrary(winmm_);
    }
  }

 private:
  using PeriodFn = UINT(WINAPI*)(UINT);

  HMODULE winmm_ = nullptr;
  PeriodFn begin_period_ = nullptr;
  PeriodFn end_period_ = nullptr;
  bool held_ = false;

  FML_DISALLOW_COPY_AND_ASSIGN(HighResolutionTimer);
};

//------------------------------------------------------------------------------
/// What the display actually runs at, in hertz.
///
/// Three sources, in the order they can be trusted:
///
///   1. The composition clock. This is the number the desktop is really being
///      refreshed at, and the one upstream's Windows embedder uses.
///   2. The display mode. Composition can be off -- a Remote Desktop session
///      arrives that way -- and then the first source reports nothing, but the
///      mode is still there to read.
///   3. Sixty, which is a guess, and is what this used to assume always.
///
/// Assuming sixty is not harmless on hardware that is not sixty: a hundred and
/// twenty hertz display gets half the frames it could, and a fifty hertz one
/// gets asked for frames it cannot show.
double DisplayRefreshRate() {
  DWM_TIMING_INFO timing = {};
  timing.cbSize = sizeof(timing);
  if (DwmGetCompositionTimingInfo(nullptr, &timing) == S_OK &&
      timing.rateRefresh.uiDenominator > 0 &&
      timing.rateRefresh.uiNumerator > 0) {
    return static_cast<double>(timing.rateRefresh.uiNumerator) /
           timing.rateRefresh.uiDenominator;
  }

  DEVMODEW mode = {};
  mode.dmSize = sizeof(mode);
  if (EnumDisplaySettingsW(nullptr, ENUM_CURRENT_SETTINGS, &mode) &&
      mode.dmDisplayFrequency > 1) {
    // One means "hardware default" rather than one hertz.
    return static_cast<double>(mode.dmDisplayFrequency);
  }

  return 60.0;
}

//------------------------------------------------------------------------------
/// A vsync waiter paced by the display rather than by a fixed sixty hertz.
///
/// The algorithm is `VsyncWaiterFallback`'s, and deliberately so: a phase fixed
/// at construction, each frame snapped forward onto that grid, and the callback
/// posted for that time. What changes is only where the interval comes from.
///
/// It does not block on the display. An earlier attempt did -- `DwmFlush` --
/// and it was worse: with composition off the call returns immediately, so the
/// loop has to be paced by hand, and hand pacing on Windows means `sleep_until`
/// against a fifteen millisecond timer. A snapped timer at the right interval
/// is both simpler and more accurate, and the swap no longer waits either, so
/// nothing in the frame is blocked on the display at all.
class VsyncWaiterWin final : public VsyncWaiter {
 public:
  explicit VsyncWaiterWin(const TaskRunners& task_runners)
      : VsyncWaiter(task_runners), phase_(fml::TimePoint::Now()) {}

  ~VsyncWaiterWin() override = default;

 private:
  /// Rounds `value` up onto the grid that passes through `phase` every
  /// `interval`. Same as the fallback waiter's, and as the Windows embedder's
  /// `SnapToNextTick`.
  static fml::TimePoint SnapToNextTick(fml::TimePoint value,
                                       fml::TimePoint phase,
                                       fml::TimeDelta interval) {
    fml::TimeDelta offset = (phase - value) % interval;
    if (offset != fml::TimeDelta::Zero()) {
      offset = offset + interval;
    }
    return value + offset;
  }

  /// The frame interval, re-read about once a second.
  ///
  /// Not cached forever, because a display's rate changes -- a laptop switching
  /// to battery, a monitor swapped, a session moving between local and remote.
  /// Not read every frame either: it is two syscalls to answer a question whose
  /// answer almost never changes.
  fml::TimeDelta FrameInterval() {
    const fml::TimePoint now = fml::TimePoint::Now();
    if (interval_ != fml::TimeDelta::Zero() &&
        now - interval_read_at_ <= fml::TimeDelta::FromSeconds(1)) {
      return interval_;
    }

    // A rate this machine does not have, for testing the pacing itself. There
    // is no other way to find out whether a hundred and twenty hertz display
    // would be driven correctly from a machine that runs at sixty.
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
      // Worth a line: it is not always the number the display's *mode* says.
      // A Remote Desktop session composites at around thirty-two hertz whatever
      // the virtual adapter reports, and pacing to the mode instead would mean
      // rendering twice as many frames as anyone sees.
      FML_LOG(IMPORTANT) << "Pacing frames at " << hz << " Hz.";
    }
    interval_ = fml::TimeDelta::FromSecondsF(1.0 / hz);
    interval_read_at_ = now;
    return interval_;
  }

  // |VsyncWaiter|
  void AwaitVSync() override {
    const fml::TimeDelta interval = FrameInterval();
    const fml::TimePoint frame_start_time =
        SnapToNextTick(fml::TimePoint::Now(), phase_, interval);
    const fml::TimePoint frame_target_time = frame_start_time + interval;

    std::weak_ptr<VsyncWaiterWin> weak_this =
        std::static_pointer_cast<VsyncWaiterWin>(shared_from_this());
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

  FML_DISALLOW_COPY_AND_ASSIGN(VsyncWaiterWin);
};

//------------------------------------------------------------------------------
// Platform channels.
//
// The half of a platform message that runs outside the framework. Upstream
// this is a plugin per channel registered on a `BinaryMessenger`; here it is
// one switch, because there is no plugin system to register with and only the
// channels the engine itself defines are served.
//
// `flutter/platform` speaks JSON -- `{"method": ..., "args": ...}` in, a
// one-element array out on success and a three-element one on failure. Not a
// choice: the channel predates the binary codec and its Android, iOS, Linux and
// macOS halves are all written against JSON.

constexpr char kPlatformChannel[] = "flutter/platform";
constexpr char kLifecycleChannel[] = "flutter/lifecycle";

// The strings an application branches on. Copied from platform_handler.cc
// rather than invented: a Flutter app catching PlatformException checks
// `code`, and a code of our own devising would match nothing anyone has
// written.
constexpr char kClipboardError[] = "Clipboard error";
constexpr char kUnknownClipboardFormatMessage[] = "Unknown clipboard format";
constexpr char kTextPlainFormat[] = "text/plain";

/// UTF-16 to UTF-8, for text coming out of the clipboard.
std::string Narrow(const std::wstring& wide) {
  if (wide.empty()) {
    return {};
  }
  int length = WideCharToMultiByte(CP_UTF8, 0, wide.c_str(),
                                   static_cast<int>(wide.size()), nullptr, 0,
                                   nullptr, nullptr);
  if (length <= 0) {
    return {};
  }
  std::string out(static_cast<size_t>(length), '\0');
  WideCharToMultiByte(CP_UTF8, 0, wide.c_str(), static_cast<int>(wide.size()),
                      out.data(), length, nullptr, nullptr);
  return out;
}

std::wstring WidenText(const std::string& utf8) {
  if (utf8.empty()) {
    return {};
  }
  int length = MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(),
                                   static_cast<int>(utf8.size()), nullptr, 0);
  if (length <= 0) {
    return {};
  }
  std::wstring out(static_cast<size_t>(length), L'\0');
  MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(), static_cast<int>(utf8.size()),
                      out.data(), length);
  return out;
}

/// Opens the clipboard for as long as the scope lasts.
///
/// One attempt, and no sleeping, which is what upstream's `ScopedClipboard`
/// does: `Open` calls `OpenClipboard` once and returns `GetLastError()` if it
/// fails. The clipboard is a global lock another process can be holding, so
/// failure is a real outcome rather than a bug -- but this runs on the platform
/// thread, and a retry loop would stall every shell task behind it for as long
/// as the other process kept the lock.
class ScopedClipboard {
 public:
  explicit ScopedClipboard(HWND owner) {
    if (OpenClipboard(owner)) {
      opened_ = true;
    } else {
      error_ = GetLastError();
    }
  }

  ~ScopedClipboard() {
    if (opened_) {
      CloseClipboard();
    }
  }

  bool opened() const { return opened_; }

  /// Why it could not be opened, for the error the caller reports.
  DWORD error() const { return error_; }

  ScopedClipboard(const ScopedClipboard&) = delete;
  ScopedClipboard& operator=(const ScopedClipboard&) = delete;

 private:
  bool opened_ = false;
  DWORD error_ = 0;
};

/// What a clipboard read came back with.
///
/// Three outcomes, not two, and upstream reports all three differently: the
/// clipboard could not be opened (an error), it holds no text (a successful
/// null), or here is the text. Folding the first into the second would tell an
/// application that the clipboard is empty when it is in fact busy.
struct ClipboardRead {
  enum class Status { kText, kEmpty, kFailed } status;
  std::string text;
};

ClipboardRead ReadClipboardText(HWND window) {
  ScopedClipboard clipboard(window);
  if (!clipboard.opened()) {
    return {ClipboardRead::Status::kFailed, {}};
  }
  // Either plain-text format counts, as upstream's HasString does: a CF_TEXT-
  // only clipboard still has a string in it, and GetClipboardData converts.
  if (!IsClipboardFormatAvailable(CF_UNICODETEXT) &&
      !IsClipboardFormatAvailable(CF_TEXT)) {
    return {ClipboardRead::Status::kEmpty, {}};
  }
  HANDLE data = GetClipboardData(CF_UNICODETEXT);
  if (data == nullptr) {
    return {ClipboardRead::Status::kFailed, {}};
  }
  auto* text = static_cast<wchar_t*>(GlobalLock(data));
  if (text == nullptr) {
    return {ClipboardRead::Status::kFailed, {}};
  }
  std::string utf8 = Narrow(std::wstring(text));
  GlobalUnlock(data);
  return {ClipboardRead::Status::kText, std::move(utf8)};
}

bool SetClipboardText(HWND window, const std::string& utf8) {
  ScopedClipboard clipboard(window);
  if (!clipboard.opened() || !EmptyClipboard()) {
    return false;
  }
  std::wstring wide = WidenText(utf8);
  // The clipboard takes ownership of the handle on success, so it is only
  // freed on the paths where SetClipboardData was not reached.
  HGLOBAL handle = GlobalAlloc(GMEM_MOVEABLE, (wide.size() + 1) * sizeof(wchar_t));
  if (handle == nullptr) {
    return false;
  }
  auto* destination = static_cast<wchar_t*>(GlobalLock(handle));
  if (destination == nullptr) {
    GlobalFree(handle);
    return false;
  }
  memcpy(destination, wide.c_str(), (wide.size() + 1) * sizeof(wchar_t));
  GlobalUnlock(handle);
  if (SetClipboardData(CF_UNICODETEXT, handle) == nullptr) {
    GlobalFree(handle);
    return false;
  }
  return true;
}

/// A JSON success envelope: the result, wrapped in a one-element array.
std::string SuccessEnvelope(
    const std::function<void(rapidjson::Writer<rapidjson::StringBuffer>&)>&
        write_result) {
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartArray();
  write_result(writer);
  writer.EndArray();
  return std::string(buffer.GetString(), buffer.GetSize());
}

std::string NullEnvelope() {
  return SuccessEnvelope([](auto& writer) { writer.Null(); });
}

/// A JSON error envelope: code, message and details, in that order.
std::string ErrorEnvelope(const char* code, const char* message) {
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartArray();
  writer.String(code);
  writer.String(message);
  writer.Null();
  writer.EndArray();
  return std::string(buffer.GetString(), buffer.GetSize());
}

/// Handles one call on `flutter/platform`.
///
/// Returns the reply, or nothing for a method this host does not implement --
/// which is answered with an empty message rather than an error, because that
/// is what tells the framework nobody served it. `SystemChannels.platform` is
/// an `OptionalMethodChannel` precisely so that an unimplemented method is
/// quiet rather than an exception.
std::optional<std::string> HandlePlatformCall(HWND window,
                                              const std::string& method,
                                              const rapidjson::Value* args) {
  if (method == "SystemNavigator.pop") {
    // Posted rather than sent: this runs on the platform thread and the window
    // belongs to another one, and closing a window from underneath the shell
    // that is mid-message would tear down the thread answering it.
    PostMessage(window, WM_CLOSE, 0, 0);
    return NullEnvelope();
  }

  if (method == "SystemSound.play") {
    // Upstream's table, and the surprising entry is deliberate: on Windows a
    // click has no system sound, so `SystemSoundPlay` succeeds without making
    // one. Beeping on every keystroke instead would be an application-audible
    // difference from Flutter, not a detail.
    const std::string sound =
        args != nullptr && args->IsString() ? args->GetString() : std::string();
    if (sound == "SystemSoundType.alert") {
      MessageBeep(MB_OK);
      return NullEnvelope();
    }
    if (sound == "SystemSoundType.click" || sound == "SystemSoundType.tick") {
      return NullEnvelope();
    }
    // A sound nobody defined. Empty, which reads as not implemented.
    return std::nullopt;
  }

  if (method == "Clipboard.getData") {
    // "text/plain" is the only format the channel has ever defined.
    if (args == nullptr || !args->IsString() ||
        std::string(args->GetString()) != kTextPlainFormat) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    ClipboardRead read = ReadClipboardText(window);
    switch (read.status) {
      case ClipboardRead::Status::kFailed:
        return ErrorEnvelope(kClipboardError, "Unable to open clipboard");
      case ClipboardRead::Status::kEmpty:
        // Nothing to paste. Distinct from the clipboard being unavailable.
        return NullEnvelope();
      case ClipboardRead::Status::kText:
        return SuccessEnvelope([&read](auto& writer) {
          writer.StartObject();
          writer.Key("text");
          writer.String(read.text.c_str(),
                        static_cast<rapidjson::SizeType>(read.text.size()));
          writer.EndObject();
        });
    }
  }

  if (method == "Clipboard.setData") {
    if (args == nullptr || !args->IsObject()) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    auto text = args->FindMember("text");
    if (text == args->MemberEnd() || !text->value.IsString()) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    if (!SetClipboardText(window, text->value.GetString())) {
      return ErrorEnvelope(kClipboardError, "Unable to set clipboard data");
    }
    return NullEnvelope();
  }

  if (method == "Clipboard.hasStrings") {
    // Deliberately not a read. On some platforms reading the clipboard is
    // visible to the user, and a paste button only needs to know whether to be
    // enabled -- which is the whole reason this method exists apart from
    // getData. The format is still checked, as upstream checks it.
    if (args == nullptr || !args->IsString() ||
        std::string(args->GetString()) != kTextPlainFormat) {
      return ErrorEnvelope(kClipboardError, kUnknownClipboardFormatMessage);
    }
    const bool has_text = IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 ||
                          IsClipboardFormatAvailable(CF_TEXT) != 0;
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
/// The shell's view of the window.
///
/// Lives on the platform thread. AcquireBackingStore and PresentBackingStore
/// are called on the raster thread; both are safe there because the surface is
/// per-frame and the frame buffer is locked.
class HostPlatformView final : public PlatformView,
                               public GPUSurfaceSoftwareDelegate {
 public:
  HostPlatformView(PlatformView::Delegate& delegate,
                   const TaskRunners& task_runners,
                   HWND window,
                   FrameBuffer* frame_buffer,
                   bool prefer_impeller)
      : PlatformView(delegate, task_runners),
        window_(window),
        frame_buffer_(frame_buffer),
        prefer_impeller_(prefer_impeller) {}

  ~HostPlatformView() override = default;

  // |PlatformView|
  //
  // Called on the raster thread during startup, before anything asks for the
  // Impeller context. That ordering is the reason this hook exists: the shell
  // publishes GetImpellerContext() to the IO thread right after this returns.
  void SetupImpellerContext() override {
    if (prefer_impeller_ && !gl_context_) {
      gl_context_ = ImpellerGlContext::Create();
      if (!gl_context_) {
        FML_LOG(IMPORTANT)
            << "Falling back to software rendering; see the error above.";
      }
    }
    // Text ops and images both have to be recorded for the backend that will
    // draw them, and this runs before the engine is launched, so the first
    // frame already gets it right.
    rf_set_impeller_backend(gl_context_ != nullptr ? 1 : 0);
  }

  // |PlatformView|
  //
  // Also on the raster thread, after SetupImpellerContext.
  std::unique_ptr<Surface> CreateRenderingSurface() override {
    if (gl_context_) {
      if (auto surface = CreateImpellerSurface()) {
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
    return gl_context_ ? gl_context_->GetImpellerContext() : nullptr;
  }

  // |PlatformView|
  //
  // Called on the IO thread, once, after the Impeller context is ready -- which
  // is exactly what an upload needs and the reason the shell calls this here.
  //
  // Upstream returns a Skia GrDirectContext from this hook; under Impeller
  // there is none, and the return value is only passed to
  // ShellIOManager::NotifyResourceContextAvailable, which tolerates null. What
  // is wanted is the side effect: the offscreen GL context becomes current on
  // this thread and stays current, so texture uploads posted here have a
  // context to run in and the reactor knows this thread may issue GL commands.
  sk_sp<GrDirectContext> CreateResourceContext() const override {
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

  /// Remakes the EGL window surface after a resize. An EGL surface does not
  /// follow a window that changed size; presenting to a stale one stretches
  /// the frame.
  void OnWindowResized() {
    if (gl_delegate_) {
      gl_delegate_->Resize();
    }
  }

  // |PlatformView|
  std::unique_ptr<VsyncWaiter> CreateVSyncWaiter() override {
    return std::make_unique<VsyncWaiterWin>(task_runners_);
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

  /// Sends one pointer event to the engine. Called from the window thread,
  /// which is why it hops to the platform task runner: PlatformView is not
  /// thread safe, and the pointer dispatcher expects to run there.
  void SendPointer(const PointerData& data) {
    auto packet = std::make_unique<PointerDataPacket>(1);
    packet->SetPointerData(0, data);
    task_runners_.GetPlatformTaskRunner()->PostTask(
        fml::MakeCopyable([weak = GetWeakPtr(), packet = std::move(packet)]() mutable {
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
  /// on PlatformView to add one to. RuntimeController unpacks it.
  ///
  /// No response handle is asked for. The reply says whether the framework used
  /// the key, and the only thing to do with that answer is to re-post the
  /// unhandled ones so the system still sees them -- which this host does not
  /// do, because it never withheld them in the first place.
  // |PlatformView|
  //
  // A message from the framework, on the platform thread. Upstream this is
  // where an embedder's plugins are dispatched to; here it is the one channel
  // this host serves. Everything else -- including `flutter/mousecursor`, which
  // needs the binary codec -- falls through to an empty reply, which the
  // framework reads as "nobody implements this".
  void HandlePlatformMessage(std::unique_ptr<PlatformMessage> message) override {
    if (message->channel() != kPlatformChannel) {
      PlatformView::HandlePlatformMessage(std::move(message));
      return;
    }

    const auto& data = message->data();
    rapidjson::Document document;
    document.Parse(reinterpret_cast<const char*>(data.GetMapping()),
                   data.GetSize());

    std::optional<std::string> reply;
    if (!document.HasParseError() && document.IsObject()) {
      auto method = document.FindMember("method");
      if (method != document.MemberEnd() && method->value.IsString()) {
        auto args = document.FindMember("args");
        reply = HandlePlatformCall(
            window_, method->value.GetString(),
            args == document.MemberEnd() ? nullptr : &args->value);
      }
    }

    auto response = message->response();
    if (!response) {
      return;
    }
    if (!reply.has_value()) {
      response->CompleteEmpty();
      return;
    }
    response->Complete(std::make_unique<fml::DataMapping>(
        std::vector<uint8_t>(reply->begin(), reply->end())));
  }

  //----------------------------------------------------------------------------
  /// Tells the framework what the application is doing.
  ///
  /// One bare string on `flutter/lifecycle`, with no codec and no envelope --
  /// there is nothing else the channel could ever need to say. `Engine` records
  /// it on the way past and deliberately does not consume it, so the framework
  /// sees it too.
  void SendLifecycleState(const char* state) {
    auto message = std::make_unique<PlatformMessage>(
        kLifecycleChannel,
        fml::MallocMapping::Copy(state, strlen(state)),
        /*response=*/nullptr);
    task_runners_.GetPlatformTaskRunner()->PostTask(fml::MakeCopyable(
        [weak = GetWeakPtr(), message = std::move(message)]() mutable {
          if (weak) {
            weak->DispatchPlatformMessage(std::move(message));
          }
        }));
  }

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

  // |GPUSurfaceSoftwareDelegate|
  bool PresentBackingStore(sk_sp<SkSurface> backing_store) override {
    if (backing_store == nullptr) {
      return false;
    }
    SkPixmap pixmap;
    if (!backing_store->peekPixels(&pixmap)) {
      return false;
    }
    frame_buffer_->Store(pixmap.addr(), pixmap.width(), pixmap.height());
    // Wakes the window thread, which repaints from the buffer.
    PostMessage(window_, kMessageFramePresented, 0, 0);
    return true;
  }

 private:
  /// Builds the Impeller surface, or returns nullptr with a logged reason.
  std::unique_ptr<Surface> CreateImpellerSurface() {
    gl_delegate_ = std::make_unique<ImpellerGlDelegate>(gl_context_.get(),
                                                        window_);
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

  HWND window_ = nullptr;
  FrameBuffer* frame_buffer_ = nullptr;
  bool prefer_impeller_ = false;
  std::unique_ptr<ImpellerGlContext> gl_context_;
  std::unique_ptr<ImpellerGlDelegate> gl_delegate_;
  sk_sp<SkSurface> backing_store_;

  FML_DISALLOW_COPY_AND_ASSIGN(HostPlatformView);
};

//------------------------------------------------------------------------------
/// A key message held until the rest of its session arrives.
///
/// Upstream calls one to three messages that belong together a *session*: a key
/// down, then the character messages it turns out to produce. See
/// HandleKeyMessage.
struct PendingKey {
  UINT action = 0;
  uint16_t virtual_key = 0;
  uint8_t scan_code = 0;
  bool extended = false;
  bool was_down = false;
};

//------------------------------------------------------------------------------
/// What the window proc needs to reach. Owned by rf_host_run's stack frame.
struct WindowState {
  FrameBuffer frame_buffer;
  Shell* shell = nullptr;
  HostPlatformView* platform_view = nullptr;
  fml::RefPtr<fml::TaskRunner> platform_task_runner;
  fml::RefPtr<fml::TaskRunner> raster_task_runner;
  double device_pixel_ratio = 1.0;
  /// Whether the primary button is currently down, so a WM_MOUSEMOVE can be
  /// told apart from a drag.
  bool pressed = false;
  /// Where the pointer was last seen, for the delta that Move carries.
  double last_x = 0.0;
  double last_y = 0.0;
  /// A key down waiting to learn whether it produces a character.
  std::optional<PendingKey> pending_key;
  /// The first half of a character that takes two WM_CHAR messages.
  wchar_t pending_high_surrogate = 0;
  /// What the framework has last been told about Shift and Control, so
  /// SyncModifiers knows what it has to correct.
  bool shift_reported = false;
  bool control_reported = false;
  /// The last lifecycle state the framework was told about, so a window that
  /// is activated twice does not say so twice. The states are level-triggered:
  /// each one replaces the last, and repeating one carries no information.
  std::string lifecycle_state;
};

/// Reports a lifecycle state, if it is a change.
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

//------------------------------------------------------------------------------
/// Builds a PointerData for one Win32 mouse message.
///
/// Windows reports a single system mouse, so device and pointer identity are
/// both constant. Coordinates arrive in client pixels, which are already the
/// physical pixels the engine wants.
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
  data.buttons = state->pressed ? kPointerButtonMousePrimary : 0;
  data.pressure = state->pressed ? 1.0 : 0.0;
  data.pressure_max = 1.0;
  data.view_id = kFlutterImplicitViewId;
  state->last_x = x;
  state->last_y = y;
  return data;
}

/// What Windows reports when the wheel setting has never been touched.
constexpr UINT kLinesPerScrollWindowsDefault = 3;

/// How far one notch of the wheel scrolls, in logical pixels.
///
/// The system's lines-per-notch setting, times Chromium's hundred-pixels-per-
/// three-lines -- which is what Flutter's own Windows embedder computes in
/// `FlutterWindow::UpdateScrollOffsetMultiplier`, so a wheel turn here moves a
/// list by the same amount it would in a Flutter application on the same
/// machine. At the default of three lines that is a hundred pixels.
///
/// Read per message rather than cached. It is a user32 call against a value the
/// system already has in memory, wheel messages arrive at human speed, and
/// caching it would mean handling WM_SETTINGCHANGE to keep the cache honest --
/// a moving part, and a stale one whenever it was forgotten.
double ScrollPixelsPerNotch() {
  UINT lines = kLinesPerScrollWindowsDefault;
  if (!SystemParametersInfo(SPI_GETWHEELSCROLLLINES, 0, &lines, 0)) {
    lines = kLinesPerScrollWindowsDefault;
  }
  // WHEEL_PAGESCROLL means "a screen at a time". Nothing here knows how tall
  // the scrollable is, so it is taken as a large number of lines rather than
  // as a page, which is what Chromium's fallback amounts to as well.
  if (lines == WHEEL_PAGESCROLL) {
    lines = 20;
  }
  return static_cast<double>(lines) * 100.0 / 3.0;
}

/// A wheel turn, as a hover carrying a scroll signal.
///
/// It is not its own change: the pointer did not go anywhere, and a recogniser
/// that read the change would see a mouse being moved. The signal is what says
/// otherwise.
PointerData MakeScrollData(WindowState* state, double x, double y, double notches) {
  PointerData data = MakePointerData(state, PointerData::Change::kHover, x, y);
  data.signal_kind = PointerData::SignalKind::kScroll;
  // Positive means the content moves up -- the direction the reader is going,
  // which is the opposite of the way the wheel turned.
  data.scroll_delta_x = 0.0;
  data.scroll_delta_y = -notches * ScrollPixelsPerNotch();
  return data;
}

// -- Keyboard -----------------------------------------------------------------
//
// The half of key handling that is Windows' rather than Flutter's. Upstream
// this is `KeyboardManager`, and most of that class is the part deliberately
// left out here: because upstream withholds every key until the framework has
// answered, it then has to re-post the unhandled ones and recognise them coming
// back. This host never withholds -- every key message also reaches
// `DefWindowProc` -- so there is nothing to re-post and no queue to keep.
//
// What is kept is the part that is about Windows telling the truth awkwardly:
// pairing a key down with the character it turns out to produce, surrogate
// pairs, dead keys, and the modifier bookkeeping below.

/// The mask Win32 sets on a mapped character to mean "this is a dead key".
constexpr uint32_t kDeadKeyCharMask = 0x80000000;

/// Scan codes for the modifiers SyncModifiers reconciles. Left-hand codes: when
/// the two sides disagree with what Windows reports it is the left one that is
/// invented, because Windows only reports the pair.
constexpr uint8_t kScanCodeShiftLeft = 0x2a;
constexpr uint8_t kScanCodeControlLeft = 0x1d;

/// Whether a code point is something a person would see. Control characters
/// arrive as WM_CHAR too -- Ctrl+A is 0x01 -- and are not text.
bool IsPrintable(char32_t code_point) {
  return code_point >= ' ' && code_point != 0x7F;
}

std::string Utf8FromCodePoint(char32_t code_point) {
  std::string out;
  if (code_point < 0x80) {
    out += static_cast<char>(code_point);
  } else if (code_point < 0x800) {
    out += static_cast<char>(0xC0 | (code_point >> 6));
    out += static_cast<char>(0x80 | (code_point & 0x3F));
  } else if (code_point < 0x10000) {
    out += static_cast<char>(0xE0 | (code_point >> 12));
    out += static_cast<char>(0x80 | ((code_point >> 6) & 0x3F));
    out += static_cast<char>(0x80 | (code_point & 0x3F));
  } else {
    out += static_cast<char>(0xF0 | (code_point >> 18));
    out += static_cast<char>(0x80 | ((code_point >> 12) & 0x3F));
    out += static_cast<char>(0x80 | ((code_point >> 6) & 0x3F));
    out += static_cast<char>(0x80 | (code_point & 0x3F));
  }
  return out;
}

char32_t CodePointFromSurrogatePair(wchar_t high, wchar_t low) {
  return 0x10000 + ((static_cast<char32_t>(high) & 0x03FF) << 10) + (low & 0x3FF);
}

/// Which Shift, which Control, which Alt.
///
/// `VK_SHIFT` does not say. For Shift the scan code is asked, because the two
/// sides sit at different positions; for Control and Alt the extended flag is
/// what separates right from left. Straight from upstream's `ResolveKeyCode`.
uint16_t ResolveVirtualKey(uint16_t virtual_key, bool extended, uint8_t scan_code) {
  switch (virtual_key) {
    case VK_SHIFT:
    case VK_LSHIFT:
      return static_cast<uint16_t>(MapVirtualKey(scan_code, MAPVK_VSC_TO_VK_EX));
    case VK_MENU:
    case VK_LMENU:
      return extended ? VK_RMENU : VK_LMENU;
    case VK_CONTROL:
    case VK_LCONTROL:
      return extended ? VK_RCONTROL : VK_LCONTROL;
    default:
      return virtual_key;
  }
}

/// Builds and sends one key event.
void SendKeyEvent(WindowState* state,
                  KeyEventType type,
                  uint16_t virtual_key,
                  uint8_t scan_code,
                  bool extended,
                  const std::string& character,
                  bool synthesized) {
  if (state->platform_view == nullptr) {
    return;
  }
  KeyData data;
  data.Clear();
  data.timestamp = static_cast<uint64_t>(
      fml::TimePoint::Now().ToEpochDelta().ToMicroseconds());
  data.type = type;
  data.physical = PhysicalKeyForScanCode(scan_code, extended);
  data.logical = LogicalKeyForVirtualKey(virtual_key, scan_code, extended);
  data.synthesized = synthesized ? 1 : 0;
  data.device_type = KeyEventDeviceType::kKeyboard;
  state->platform_view->SendKey(data, character);

  // Whatever was just reported is now what the framework believes, which is
  // what SyncModifiers compares against.
  if (virtual_key == VK_LSHIFT || virtual_key == VK_RSHIFT) {
    state->shift_reported = (type != KeyEventType::kUp);
  } else if (virtual_key == VK_LCONTROL || virtual_key == VK_RCONTROL) {
    state->control_reported = (type != KeyEventType::kUp);
  }
}

/// Turns one completed session into a key event.
void SendKeyFromSession(WindowState* state,
                        const PendingKey& key,
                        const std::string& character) {
  const bool is_down = key.action == WM_KEYDOWN || key.action == WM_SYSKEYDOWN;
  const KeyEventType type =
      !is_down ? KeyEventType::kUp
               : (key.was_down ? KeyEventType::kRepeat : KeyEventType::kDown);
  SendKeyEvent(state, type, key.virtual_key, key.scan_code, key.extended,
               character, /*synthesized=*/false);
}

/// Reconciles what the framework was told about Shift and Control against what
/// Windows says is held, making up the difference.
///
/// Two things make this necessary and neither is avoidable. A modifier released
/// while another window had the focus sends its up message there, so this one
/// never sees it. And pressing AltGr on a layout that has one makes Win32 emit
/// a *fake* left-Control down with no matching up, which would otherwise leave
/// Control held for good.
///
/// Called on mouse moves, which is where upstream calls it from
/// (`FlutterWindowsView::OnPointerMove` -> `SyncModifiersIfNeeded`) and for the
/// same reason: it is the cheapest event that happens often enough to correct a
/// stale state before anybody notices, and costs nothing when nothing changed.
void SyncModifiers(WindowState* state) {
  // The high bit of GetKeyState is "currently down".
  const bool shift_held = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
  const bool control_held = (GetKeyState(VK_CONTROL) & 0x8000) != 0;

  if (shift_held != state->shift_reported) {
    SendKeyEvent(state,
                 shift_held ? KeyEventType::kDown : KeyEventType::kUp,
                 VK_LSHIFT, kScanCodeShiftLeft, /*extended=*/false,
                 /*character=*/"", /*synthesized=*/true);
  }
  if (control_held != state->control_reported) {
    SendKeyEvent(state,
                 control_held ? KeyEventType::kDown : KeyEventType::kUp,
                 VK_LCONTROL, kScanCodeControlLeft, /*extended=*/false,
                 /*character=*/"", /*synthesized=*/true);
  }
}

/// Handles one of the eight key and character messages.
///
/// The caller falls through to `DefWindowProc` afterwards, always. That is the
/// whole difference from upstream: a key is reported, never taken.
///
/// The awkward part is that a key down does not know whether it will produce a
/// character. `A` yields WM_KEYDOWN then WM_CHAR; `F1` yields WM_KEYDOWN alone;
/// and Ctrl+1 yields WM_KEYDOWN alone even though `MapVirtualKey` says it maps
/// to a character. So the queue is peeked: if a character message is really
/// coming, this session waits for it, and the character message finishes it.
void HandleKeyMessage(WindowState* state,
                      UINT action,
                      WPARAM wparam,
                      LPARAM lparam) {
  switch (action) {
    case WM_CHAR:
    case WM_SYSCHAR:
    case WM_DEADCHAR:
    case WM_SYSDEADCHAR: {
      const auto unit = static_cast<wchar_t>(wparam);

      // A code point outside the basic plane arrives as two messages. The high
      // half is kept and the low half completes it.
      char32_t code_point;
      if (unit >= 0xD800 && unit <= 0xDBFF) {
        state->pending_high_surrogate = unit;
        return;
      }
      if (unit >= 0xDC00 && unit <= 0xDFFF) {
        if (state->pending_high_surrogate == 0) {
          return;  // A low surrogate with no high one before it. Malformed.
        }
        code_point =
            CodePointFromSurrogatePair(state->pending_high_surrogate, unit);
        state->pending_high_surrogate = 0;
      } else {
        state->pending_high_surrogate = 0;
        code_point = unit;
      }

      if (!state->pending_key.has_value()) {
        // A character with no key down before it: Alt and the numeric keypad,
        // or an IME committing. There is nothing to report it as until there is
        // text input to report it to.
        return;
      }
      const PendingKey key = *state->pending_key;
      state->pending_key.reset();

      // Only WM_CHAR is text. WM_SYS*CHAR is a system-menu accelerator, and
      // WM_DEADCHAR is half of a character that a later WM_CHAR will complete.
      std::string character;
      if (action == WM_CHAR && IsPrintable(code_point)) {
        character = Utf8FromCodePoint(code_point);
      }
      SendKeyFromSession(state, key, character);
      return;
    }

    case WM_KEYDOWN:
    case WM_SYSKEYDOWN:
    case WM_KEYUP:
    case WM_SYSKEYUP: {
      // VK_PACKET is an injected Unicode character wearing a key's clothes. It
      // has no scan code and no key to speak of; its WM_CHAR carries the point.
      if (wparam == VK_PACKET) {
        return;
      }

      const auto scan_code = static_cast<uint8_t>((lparam >> 16) & 0xff);
      const bool extended = ((lparam >> 24) & 0x01) != 0;
      const bool was_down = (lparam & 0x40000000) != 0;
      const PendingKey key = {
          .action = action,
          .virtual_key = ResolveVirtualKey(static_cast<uint16_t>(wparam),
                                           extended, scan_code),
          .scan_code = scan_code,
          .extended = extended,
          .was_down = was_down,
      };

      // A session left open by a key down whose character never came -- the
      // window lost the focus in between, say. Report it now rather than
      // attaching its character to this key.
      if (state->pending_key.has_value()) {
        const PendingKey stale = *state->pending_key;
        state->pending_key.reset();
        SendKeyFromSession(state, stale, "");
      }

      const bool is_down = action == WM_KEYDOWN || action == WM_SYSKEYDOWN;
      if (is_down) {
        const uint32_t mapped = MapVirtualKey(key.virtual_key, MAPVK_VK_TO_CHAR);
        // The dead-key bit means the mapping is real but deferred; either way a
        // character message follows, so peek for one rather than trusting the
        // mapping. Ctrl+digit maps to a character and produces no WM_CHAR.
        if ((mapped & ~kDeadKeyCharMask) != 0) {
          MSG next = {};
          if (PeekMessage(&next, nullptr, WM_KEYFIRST, WM_KEYLAST, PM_NOREMOVE) &&
              (next.message == WM_CHAR || next.message == WM_SYSCHAR ||
               next.message == WM_DEADCHAR || next.message == WM_SYSDEADCHAR)) {
            state->pending_key = key;
            return;
          }
        }
      }

      SendKeyFromSession(state, key, "");
      return;
    }

    default:
      return;
  }
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

LRESULT CALLBACK WindowProc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam) {
  auto* state =
      reinterpret_cast<WindowState*>(GetWindowLongPtr(hwnd, GWLP_USERDATA));

  switch (msg) {
    case WM_CREATE: {
      auto* create = reinterpret_cast<CREATESTRUCT*>(lparam);
      SetWindowLongPtr(hwnd, GWLP_USERDATA,
                       reinterpret_cast<LONG_PTR>(create->lpCreateParams));
      return 0;
    }
    case kMessageFramePresented:
      InvalidateRect(hwnd, nullptr, FALSE);
      return 0;
    case WM_PAINT: {
      PAINTSTRUCT ps;
      HDC hdc = BeginPaint(hwnd, &ps);
      if (state != nullptr) {
        RECT client;
        GetClientRect(hwnd, &client);
        state->frame_buffer.Paint(hdc, client);
      }
      EndPaint(hwnd, &ps);
      return 0;
    }
    case WM_ERASEBKGND:
      // The frame covers the whole client area, so skipping the background
      // erase avoids a flash of white on resize.
      return 1;
    case WM_SIZE:
      if (state != nullptr) {
        // Minimised is `hidden` rather than `paused`: the application is still
        // running and still owns its window, it simply cannot be seen. That is
        // the distinction an animation should stop on and a timer should not.
        if (wparam == SIZE_MINIMIZED) {
          SendLifecycle(state, "AppLifecycleState.hidden");
        } else if (state->lifecycle_state == "AppLifecycleState.hidden") {
          SendLifecycle(state, "AppLifecycleState.inactive");
        }
        // The EGL surface has to be remade on the raster thread, where the GL
        // context lives, before the next frame is presented to it.
        if (state->platform_view != nullptr && state->raster_task_runner) {
          state->raster_task_runner->PostTask([view = state->platform_view]() {
            view->OnWindowResized();
          });
        }
        SendViewportMetrics(state, LOWORD(lparam), HIWORD(lparam));
      }
      return 0;
    case WM_ACTIVATE:
      if (state != nullptr) {
        SendLifecycle(state, LOWORD(wparam) == WA_INACTIVE
                                 ? "AppLifecycleState.inactive"
                                 : "AppLifecycleState.resumed");
      }
      break;
    case WM_DPICHANGED: {
      // The window was dragged onto a display with a different scale, or the
      // display's scale changed under it. Windows hands over the rectangle the
      // window should move to; taking it keeps the window the same *apparent*
      // size, which is the whole point of the message.
      if (state == nullptr) {
        return 0;
      }
      state->device_pixel_ratio =
          static_cast<double>(HIWORD(wparam)) / USER_DEFAULT_SCREEN_DPI;
      const auto* suggested = reinterpret_cast<const RECT*>(lparam);
      if (suggested != nullptr) {
        // This resizes, so WM_SIZE follows and re-sends the metrics -- with the
        // new ratio, because it was stored first.
        SetWindowPos(hwnd, nullptr, suggested->left, suggested->top,
                     suggested->right - suggested->left,
                     suggested->bottom - suggested->top,
                     SWP_NOZORDER | SWP_NOACTIVATE);
      }
      return 0;
    }
    case WM_LBUTTONDOWN:
    case WM_LBUTTONUP:
    case WM_MOUSEMOVE:
    case WM_MOUSELEAVE: {
      if (state == nullptr || state->platform_view == nullptr) {
        return 0;
      }
      const double x = static_cast<double>(GET_X_LPARAM(lparam));
      const double y = static_cast<double>(GET_Y_LPARAM(lparam));

      switch (msg) {
        case WM_LBUTTONDOWN: {
          // Capture so a drag that leaves the window still reports its up,
          // which is what keeps a button from getting stuck pressed.
          SetCapture(hwnd);
          state->pressed = true;
          state->last_x = x;
          state->last_y = y;
          state->platform_view->SendPointer(
              MakePointerData(state, PointerData::Change::kDown, x, y));
          break;
        }
        case WM_LBUTTONUP: {
          state->platform_view->SendPointer(
              MakePointerData(state, PointerData::Change::kUp, x, y));
          state->pressed = false;
          ReleaseCapture();
          break;
        }
        case WM_MOUSEMOVE: {
          // Where a stale modifier gets corrected. Upstream syncs from the same
          // message, for the same reason: it is frequent enough to fix the
          // state before anybody notices and free when nothing has changed.
          SyncModifiers(state);
          // A move with no button down is a hover, which no recogniser wants
          // yet; sending it anyway would be a packet per mouse pixel.
          if (state->pressed) {
            state->platform_view->SendPointer(
                MakePointerData(state, PointerData::Change::kMove, x, y));
          } else {
            state->last_x = x;
            state->last_y = y;
          }
          break;
        }
        case WM_MOUSELEAVE: {
          if (state->pressed) {
            state->platform_view->SendPointer(
                MakePointerData(state, PointerData::Change::kCancel, x, y));
            state->pressed = false;
          }
          break;
        }
        default:
          break;
      }
      return 0;
    }
    case WM_MOUSEWHEEL: {
      if (state == nullptr || state->platform_view == nullptr) {
        return 0;
      }
      // Unlike the button messages, the wheel reports screen coordinates.
      POINT point = {GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
      ScreenToClient(hwnd, &point);
      const double notches =
          static_cast<double>(GET_WHEEL_DELTA_WPARAM(wparam)) / WHEEL_DELTA;
      state->platform_view->SendPointer(MakeScrollData(
          state, static_cast<double>(point.x), static_cast<double>(point.y),
          notches));
      return 0;
    }
    case WM_CAPTURECHANGED:
      // Something else took the mouse. A press that ends this way is cancelled
      // rather than completed.
      if (state != nullptr && state->pressed && state->platform_view != nullptr) {
        state->platform_view->SendPointer(MakePointerData(
            state, PointerData::Change::kCancel, state->last_x, state->last_y));
        state->pressed = false;
      }
      return 0;
    case WM_KEYDOWN:
    case WM_SYSKEYDOWN:
    case WM_KEYUP:
    case WM_SYSKEYUP:
    case WM_CHAR:
    case WM_SYSCHAR:
    case WM_DEADCHAR:
    case WM_SYSDEADCHAR:
      // Reported, never taken. Escape used to close the window from here, which
      // was a debugging shortcut that stopped being harmless the moment an
      // application had its own use for the key; an app that wants that
      // behaviour can ask for it in `on_key`. Every message falls through to
      // DefWindowProc, so Alt+F4, Alt+Space and the rest still work, and
      // TranslateMessage still produces the WM_CHAR this pairs keys with.
      if (state != nullptr) {
        HandleKeyMessage(state, msg, wparam, lparam);
      }
      break;
    case WM_DESTROY:
      // The last thing the framework hears. It is sent rather than skipped
      // because an application may have something to write down before it
      // goes -- upstream's `detached` is where state restoration saves.
      SendLifecycle(state, "AppLifecycleState.detached");
      PostQuitMessage(0);
      return 0;
    default:
      break;
  }
  return DefWindowProc(hwnd, msg, wparam, lparam);
}

std::wstring Widen(const char* utf8) {
  if (utf8 == nullptr || utf8[0] == '\0') {
    return L"rustflutter";
  }
  int length = MultiByteToWideChar(CP_UTF8, 0, utf8, -1, nullptr, 0);
  if (length <= 0) {
    return L"rustflutter";
  }
  std::wstring out(static_cast<size_t>(length - 1), L'\0');
  MultiByteToWideChar(CP_UTF8, 0, utf8, -1, out.data(), length);
  return out;
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
  const bool software_forced =
      force_software != nullptr && force_software[0] != '\0' &&
      force_software[0] != '0';
  settings.enable_impeller = options->enable_impeller != 0 && !software_forced;
  if (software_forced) {
    FML_LOG(IMPORTANT) << "RUSTFLUTTER_SOFTWARE is set; using the software "
                          "surface.";
  }
  settings.enable_software_rendering = !settings.enable_impeller;
  settings.icu_initialization_required = true;
  settings.icu_data_path = options->icu_data_path != nullptr
                               ? std::string(options->icu_data_path)
                               : DefaultIcuDataPath();
  // Nothing to prefetch and nothing to warn about: there is no Dart snapshot,
  // and the Impeller opt-out warning is aimed at applications that still have
  // a choice.
  settings.warn_on_impeller_opt_out = false;

  // Held for the life of the process: every frame boundary is a timer wait.
  HighResolutionTimer fine_timer;

  WindowState state;

  // -- Window (this thread) ---------------------------------------------------

  // Before anything creates a window: after that, Windows has already decided
  // what this process is and will not be told otherwise.
  DpiApi::Get().MakeProcessPerMonitorAware();

  HINSTANCE instance = GetModuleHandle(nullptr);
  WNDCLASSEX window_class = {};
  window_class.cbSize = sizeof(window_class);
  window_class.lpfnWndProc = WindowProc;
  window_class.hInstance = instance;
  window_class.hCursor = LoadCursor(nullptr, IDC_ARROW);
  window_class.lpszClassName = kWindowClass;
  if (RegisterClassEx(&window_class) == 0 &&
      GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
    return -2;
  }

  // Size the window so the *client* area matches the requested size exactly,
  // otherwise the border and title bar eat into it.
  //
  // The requested size is in logical pixels, which is what a caller asking for
  // "a thousand by seven hundred" means: a window that looks the same on every
  // display rather than one that shrinks as the display gets denser. Upstream's
  // Win32Window scales its requested size the same way.
  RECT rect{0, 0, options->width, options->height};
  AdjustWindowRect(&rect, WS_OVERLAPPEDWINDOW, FALSE);

  std::wstring title = Widen(options->title);
  HWND window = CreateWindowEx(0, kWindowClass, title.c_str(),
                               WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT,
                               rect.right - rect.left, rect.bottom - rect.top,
                               nullptr, nullptr, instance, &state);
  if (window == nullptr) {
    return -3;
  }

  // Only now is there a window to ask which display it landed on. Redo the
  // sizing at that display's DPI; at 100% this is the same rectangle and the
  // SetWindowPos is a no-op.
  state.device_pixel_ratio = DpiApi::Get().ScaleForWindow(window);
  if (state.device_pixel_ratio != 1.0) {
    const UINT dpi =
        static_cast<UINT>(state.device_pixel_ratio * USER_DEFAULT_SCREEN_DPI);
    RECT scaled{0, 0,
                static_cast<LONG>(options->width * state.device_pixel_ratio),
                static_cast<LONG>(options->height * state.device_pixel_ratio)};
    DpiApi::Get().AdjustForDpi(&scaled, WS_OVERLAPPEDWINDOW, dpi);
    SetWindowPos(window, nullptr, 0, 0, scaled.right - scaled.left,
                 scaled.bottom - scaled.top,
                 SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
  }

  // -- Threads ----------------------------------------------------------------

  ThreadHost thread_host("rf", ThreadHost::Type::kPlatform |
                                   ThreadHost::Type::kUi |
                                   ThreadHost::Type::kRaster |
                                   ThreadHost::Type::kIo);

  TaskRunners task_runners("rustflutter",
                           thread_host.platform_thread->GetTaskRunner(),
                           thread_host.raster_thread->GetTaskRunner(),
                           thread_host.ui_thread->GetTaskRunner(),
                           thread_host.io_thread->GetTaskRunner());

  // -- Shell ------------------------------------------------------------------

  PlatformData platform_data;
  std::unique_ptr<Shell> shell = Shell::Create(
      platform_data, task_runners, settings,
      [window, &state, impeller = settings.enable_impeller](Shell& shell) {
        auto view = std::make_unique<HostPlatformView>(
            shell, shell.GetTaskRunners(), window, &state.frame_buffer,
            impeller);
        // The window proc needs to reach the view to send pointers. The shell
        // owns it and outlives the message loop, so a raw pointer is enough.
        state.platform_view = view.get();
        return view;
      },
      [](Shell& shell) { return std::make_unique<Rasterizer>(shell); });

  if (shell == nullptr || !shell->IsSetup()) {
    DestroyWindow(window);
    return -4;
  }
  state.shell = shell.get();
  state.platform_task_runner = task_runners.GetPlatformTaskRunner();
  state.raster_task_runner = task_runners.GetRasterTaskRunner();

  // The requested size is a request: Windows clamps a window that will not fit
  // on the display, and creating one at 700 tall on a 500 tall screen quietly
  // gives back 461. Telling the framework the number we asked for rather than
  // the one we got makes it lay out for a viewport that does not exist, and the
  // difference is only invisible while something downstream scales the result.
  RECT client = {};
  GetClientRect(window, &client);
  const int32_t client_width = client.right - client.left;
  const int32_t client_height = client.bottom - client.top;

  // Everything below belongs to the platform thread: RunEngine checks for it,
  // and NotifyCreated / SetViewportMetrics reach the platform view directly.
  // Ordering matters -- the surface has to exist before the first frame is
  // rasterized, and the framework needs a size before it can lay anything out.
  task_runners.GetPlatformTaskRunner()->PostTask(
      fml::MakeCopyable([shell = shell.get(), &state, width = client_width,
                         height = client_height]() mutable {
        shell->RunEngine(RunConfiguration{});
        if (auto view = shell->GetPlatformView()) {
          view->NotifyCreated();
        }
        // The engine asks the display manager for the refresh rate when it
        // reports frame timings and when it decides how far ahead to schedule.
        // Without this it has no displays at all and falls back to a guess.
        std::vector<std::unique_ptr<Display>> displays;
        displays.push_back(std::make_unique<Display>(
            /*display_id=*/0, DisplayRefreshRate(), width, height,
            state.device_pixel_ratio));
        shell->OnDisplayUpdates(std::move(displays));
        SendViewportMetrics(&state, width, height);
      }));

  ShowWindow(window, SW_SHOWNORMAL);
  UpdateWindow(window);

  // Every lifecycle report is made from this thread, including this first one.
  // The window proc makes the other four, and `lifecycle_state` is a plain
  // string with no lock on it -- reporting the initial state from the task
  // posted above instead would have the platform thread writing it while the
  // window thread reads it.
  //
  // Ordering still holds: the report is posted to the platform task runner,
  // which runs it after the RunEngine task queued before it. ShowWindow has
  // usually produced a WM_ACTIVATE by now, in which case this is a no-op.
  SendLifecycle(&state, "AppLifecycleState.resumed");

  // -- Message loop -----------------------------------------------------------

  MSG message;
  while (GetMessage(&message, nullptr, 0, 0) > 0) {
    TranslateMessage(&message);
    DispatchMessage(&message);
  }

  // The shell must be destroyed on the platform thread -- its destructor
  // checks, because it drains the UI, raster and IO threads in order and would
  // deadlock if it were not the one holding the platform thread. Tearing the
  // surface down first stops the rasterizer before the window it draws into
  // goes away.
  state.shell = nullptr;
  state.platform_view = nullptr;
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

  return 0;
}
