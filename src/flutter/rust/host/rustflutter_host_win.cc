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

#include <dwmapi.h>
#include <imm.h>
#include <objbase.h>
#include <windows.h>
#include <windowsx.h>

#include <atomic>
#include <cstdlib>
#include <deque>
#include <map>
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
#include "flutter/fml/string_conversion.h"
#include "flutter/fml/synchronization/waitable_event.h"
#include "flutter/fml/task_runner.h"
#include "flutter/impeller/renderer/context.h"
#include "flutter/lib/ui/semantics/semantics_node.h"
#include "flutter/lib/ui/window/key_data.h"
#include "flutter/lib/ui/window/key_data_packet.h"
#include "flutter/lib/ui/window/platform_message.h"
#include "flutter/lib/ui/window/pointer_data.h"
#include "flutter/lib/ui/window/pointer_data_packet.h"
#include "flutter/lib/ui/window/viewport_metrics.h"
#include "flutter/rust/ffi/rustflutter_ffi.h"
#include "flutter/rust/ffi/rustflutter_ffi_internal.h"
#include "flutter/rust/host/rustflutter_gl.h"
#include "flutter/rust/host/rustflutter_host_a11y_win.h"
#include "flutter/rust/host/rustflutter_key_map_win.h"
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
#include "flutter/shell/platform/common/client_wrapper/include/flutter/standard_method_codec.h"
#include "flutter/shell/platform/common/text_input_model.h"
#include "flutter/shell/platform/common/text_range.h"
#include "rapidjson/document.h"
#include "rapidjson/stringbuffer.h"
#include "rapidjson/writer.h"
#include "third_party/skia/include/core/SkSurface.h"

namespace flutter {
namespace {

constexpr wchar_t kWindowClass[] = L"RustflutterHostWindow";

/// Where key events go. Matched by RuntimeController, which is the only reader.
/// Upstream this same string is in embedder.cc, platform_dispatcher.dart,
/// KeyData.java and FlutterEngine.mm -- an embedder is expected to spell it
/// out.
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
    HANDLE kPerMonitorAwareV2 =
        reinterpret_cast<HANDLE>(static_cast<intptr_t>(-4));
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
  BOOL(WINAPI* adjust_window_rect_ex_for_dpi_)(LPRECT,
                                               DWORD,
                                               BOOL,
                                               DWORD,
                                               UINT) = nullptr;
};

// Posted by the raster thread once a frame has been presented -- copied into
// the shared buffer on the software path, swapped onto the window on the
// Impeller one. The software path posts one per frame; the Impeller path
// posts only the first, because the first frame is the cue that shows the
// window (rf_host_run leaves it hidden until then).
// WM_APP is the first message id reserved for applications.
constexpr UINT kMessageFramePresented = WM_APP + 1;

// One-shot fallback for the deferred ShowWindow: if no frame is ever
// presented, this timer shows the window anyway, so a broken engine does not
// leave the process running invisibly.
constexpr UINT_PTR kTimerShowWindowFallback = 1;

// Posted by the platform thread when the framework has chosen a different
// mouse cursor, so the window thread can apply it without waiting for the
// pointer to move. See HandleMouseCursorCall.
constexpr UINT kMessageCursorChanged = WM_APP + 2;

// Posted by the platform thread when the framework has answered a
// `System.requestAppExit` with "exit". See HandleExitResponse.
constexpr UINT kMessageQuitApproved = WM_APP + 3;

// Posted by the platform thread when the framework has said whether it handled
// a key. wparam is the sequence id the key was sent with, lparam is 1 for
// handled. See HandleKeyResult -- this message is what makes it possible for
// the framework to consume a key at all, because Win32 wants a synchronous
// answer and the framework's is three threads away.
constexpr UINT kMessageKeyResult = WM_APP + 4;

/// Posted by the platform thread after a semantics tree arrives, so that
/// the events it owes are raised on the thread that owns the window --
/// which is the apartment a provider asking for COM threading belongs to.
constexpr UINT kMessageSemanticsUpdated = WM_APP + 5;

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
    begin_period_ =
        reinterpret_cast<PeriodFn>(GetProcAddress(winmm_, "timeBeginPeriod"));
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
  HGLOBAL handle =
      GlobalAlloc(GMEM_MOVEABLE, (wide.size() + 1) * sizeof(wchar_t));
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

//------------------------------------------------------------------------------
// Text input.
//
// `flutter/textinput` is the channel a text field talks to the platform on, and
// on Windows the platform's half of it is the IME. Upstream this is
// `TextInputPlugin` plus `TextInputManager` plus `FlutterWindow`'s WM_IME_*
// handling; the arrangement here is the same, minus the parts that need a
// second view or a delta model.
//
// The editing model is the engine's own `flutter::TextInputModel` -- the same
// class the Windows embedder edits, not a copy of it. Getting to it is the
// reason `common_cpp_accessibility` had to go: it named a third-party target
// this fork removed, and a label GN cannot resolve stops the whole BUILD file
// from loading, which took the text input model down with it.
//
// # Which thread
//
// Upstream the plugin and the window are both on the platform thread. Here they
// are not: channel calls arrive on the platform thread and window messages on
// the window thread, and both touch the model. So it is behind a mutex, and the
// IMM calls stay on the window thread where the composition context lives --
// `ImmGetCompositionString` is only meaningful during the message that reports
// it.

constexpr char kTextInputChannel[] = "flutter/textinput";

/// The composition window, and what the IME is currently saying.
///
/// Upstream's `TextInputManager`, cut down to what one window needs. Every
/// method here must run on the window thread.
class ImeContext {
 public:
  explicit ImeContext(HWND window) : window_(window) {}

  /// Opens a composition and puts its window where the caret is.
  ///
  /// The position matters more than it looks: the candidate list is what the
  /// reader is choosing characters from, and an IME that cannot be told where
  /// the text is puts it in the corner of the window.
  ///
  /// The system caret is upstream's, and it is not decoration. Some IMEs
  /// ignore `ImmSetCandidateWindow` entirely and ask `GetCaretPos()` instead,
  /// so a one-pixel caret is created for the length of the composition and
  /// moved along with the candidate window.
  void CreateImeWindow() {
    if (!active_) {
      CreateCaret(window_, nullptr, 1, 1);
    }
    active_ = true;
    MoveImeWindow();
  }

  void UpdateImeWindow() { MoveImeWindow(); }

  void DestroyImeWindow() {
    if (active_) {
      DestroyCaret();
    }
    active_ = false;
  }

  /// Where the caret is, in physical pixels relative to the client area.
  ///
  /// Comes from the framework, which is the only side that knows: it is
  /// `TextInput.setMarkedTextRect` put through the transform from
  /// `TextInput.setEditableSizeAndTransform`, scaled from the framework's
  /// logical pixels to physical ones by the caller (`UpdateImePosition`,
  /// which is the side that knows the DPI).
  void SetCaretRect(double x, double y, double width, double height) {
    caret_ = {x, y, width, height};
    MoveImeWindow();
  }

  /// The text the IME has committed, if this message carries any.
  std::optional<std::u16string> ResultString() const {
    return ReadString(GCS_RESULTSTR);
  }

  /// The text the IME is still composing.
  std::optional<std::u16string> ComposingString() const {
    return ReadString(GCS_COMPSTR);
  }

  /// Where the IME's own caret sits inside the composing text.
  long ComposingCursorPosition() const {
    HIMC context = ImmGetContext(window_);
    if (context == nullptr) {
      return -1;
    }
    LONG position = ImmGetCompositionString(context, GCS_CURSORPOS, nullptr, 0);
    ImmReleaseContext(window_, context);
    return position;
  }

  /// Throws away whatever is being composed. Used when the framework detaches
  /// a client mid-composition.
  void AbortComposing() {
    HIMC context = ImmGetContext(window_);
    if (context == nullptr) {
      return;
    }
    ImmNotifyIME(context, NI_COMPOSITIONSTR, CPS_CANCEL, 0);
    ImmReleaseContext(window_, context);
    DestroyImeWindow();
  }

 private:
  struct CaretRect {
    double x = 0;
    double y = 0;
    double width = 0;
    double height = 0;
  };

  void MoveImeWindow() {
    if (!active_ || GetFocus() != window_) {
      return;
    }
    HIMC context = ImmGetContext(window_);
    if (context == nullptr) {
      return;
    }
    const LONG left = static_cast<LONG>(caret_.x);
    const LONG top = static_cast<LONG>(caret_.y);
    const LONG right = static_cast<LONG>(caret_.x + caret_.width);
    const LONG bottom = static_cast<LONG>(caret_.y + caret_.height);
    SetCaretPos(left, bottom);

    COMPOSITIONFORM composition = {CFS_POINT, {left, top}, {}};
    ImmSetCompositionWindow(context, &composition);

    // CFS_EXCLUDE rather than CFS_CANDIDATEPOS: it names the rectangle the
    // candidate list must *not* cover, so the list is placed clear of the line
    // being typed instead of on top of it.
    CANDIDATEFORM candidate = {
        0, CFS_EXCLUDE, {left, bottom}, {left, top, right, bottom}};
    ImmSetCandidateWindow(context, &candidate);

    ImmReleaseContext(window_, context);
  }

  std::optional<std::u16string> ReadString(DWORD type) const {
    HIMC context = ImmGetContext(window_);
    if (context == nullptr) {
      return std::nullopt;
    }
    std::optional<std::u16string> result;
    // Length first, in bytes, then the characters. A zero-length composition
    // is a real state -- the reader deleted back to nothing -- and is not the
    // same as there being no composition.
    LONG bytes = ImmGetCompositionString(context, type, nullptr, 0);
    if (bytes >= 0) {
      std::u16string text(static_cast<size_t>(bytes) / sizeof(wchar_t), u'\0');
      if (bytes > 0) {
        ImmGetCompositionString(context, type, text.data(), bytes);
      }
      result = std::move(text);
    }
    ImmReleaseContext(window_, context);
    return result;
  }

  HWND window_ = nullptr;
  /// Whether the temporary system caret exists. Upstream's `ime_active_`.
  bool active_ = false;
  CaretRect caret_;
};

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
                                              const rapidjson::Value* args);

  // -- What the window thread reports -----------------------------------------

  /// A character the reader typed directly, with no IME involved.
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
  /// Returns true if the field used it, so the window proc can leave it alone
  /// afterwards. Enter is handled separately because it is an *action* rather
  /// than an edit.
  bool OnEditingKey(WPARAM virtual_key, bool shift);

  /// Enter, which submits rather than edits.
  void OnAction();

  /// Whether a `clearClient` arrived mid-composition. Window thread only.
  bool TakeAbortComposing() { return abort_composing_.exchange(false); }

  void OnComposeBegin() {
    Edit([](TextInputModel& model) {
      model.BeginComposing();
      return true;
    });
  }

  void OnComposeChange(const std::u16string& text, int cursor_position) {
    if (Edit([&](TextInputModel& model) {
          model.UpdateComposingText(
              text, TextRange(static_cast<size_t>(cursor_position)));
          return true;
        })) {
      SendStateUpdate();
    }
  }

  void OnComposeCommit() {
    if (Edit([](TextInputModel& model) {
          model.CommitComposing();
          return true;
        })) {
      SendStateUpdate();
    }
  }

  void OnComposeEnd() {
    if (Edit([](TextInputModel& model) {
          model.EndComposing();
          return true;
        })) {
      SendStateUpdate();
    }
  }

  /// Where the caret is in the window, for the IME to place its candidates.
  ///
  /// The two halves arrive on separate calls and either can be last, so they
  /// are kept apart and composed here -- which is upstream's `GetCursorRect`:
  /// the marked rectangle's origin put through the editable's transform, with
  /// the marked rectangle's own size. Read on the window thread, written on the
  /// platform thread.
  bool TakeCaretRect(double* x, double* y, double* width, double* height) {
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

  void SendStateUpdate();

  mutable std::mutex mutex_;
  std::unique_ptr<TextInputModel> model_;
  /// Set when a client is cleared mid-composition; read and reset by the
  /// window thread, which is the only one that may talk to the IME.
  std::atomic<bool> abort_composing_{false};
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

//------------------------------------------------------------------------------
// The user's settings, and their languages.
//
// Two channels the framework never sees as channels: `Engine` takes both on the
// way past and hands the contents to the framework directly, which is what
// upstream does too. See rf_app_set_user_settings in rust_app_api.h.
//
// The readers are upstream's `settings_plugin.cc` and `system_utils.cc`, key
// for key. What is *not* upstream is how a change is noticed: upstream asks the
// registry to notify it (`RegNotifyChangeKeyValue` on two keys, each watched by
// an `EventWatcher` thread). Here the window is already being told -- Windows
// broadcasts `WM_SETTINGCHANGE` to every top-level window when the theme or the
// regional settings change -- so the window proc re-reads and re-sends, and no
// second thread is needed. The registry watcher exists upstream because a
// headless engine may have no window; this host always has one.

constexpr char kSettingsChannel[] = "flutter/settings";
constexpr char kLocalizationChannel[] = "flutter/localization";

/// Whether the reader has asked for dark mode.
///
/// Upstream's `GetThemeBrightness`. A machine too old to have the value is
/// light, which is what a Windows without dark mode is.
bool PrefersDarkTheme() {
  DWORD use_light_theme = 0;
  DWORD size = sizeof(use_light_theme);
  LONG result = RegGetValueW(
      HKEY_CURRENT_USER,
      L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
      L"AppsUseLightTheme", RRF_RT_REG_DWORD, nullptr, &use_light_theme, &size);
  return result == ERROR_SUCCESS && use_light_theme == 0;
}

/// The accessibility text scale, as a multiplier.
///
/// Upstream's `SettingsPlugin::GetTextScaleFactor`. The registry keeps it as a
/// percentage; a machine without the value has never been asked to scale.
double TextScaleFactor() {
  DWORD percent = 0;
  DWORD size = sizeof(percent);
  LONG result = RegGetValueW(
      HKEY_CURRENT_USER, L"Software\\Microsoft\\Accessibility",
      L"TextScaleFactor", RRF_RT_REG_DWORD, nullptr, &percent, &size);
  if (result != ERROR_SUCCESS || percent == 0) {
    return 1.0;
  }
  return percent / 100.0;
}

/// Whether times should be written 13:00 rather than 1:00 PM.
///
/// Upstream's `GetUserTimeFormat` and `Prefer24HourTime`: the user's time
/// format string, and whether it contains an `H`. Capital H is the 24-hour
/// hour in every Windows format string; lowercase h is the 12-hour one.
bool AlwaysUse24HourFormat() {
  // Large enough for any reasonable format string, which is how upstream sizes
  // it too rather than doing the call-allocate-call dance.
  constexpr int kBufferSize = 100;
  wchar_t buffer[kBufferSize] = {};
  if (GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_STIMEFORMAT, buffer,
                      kBufferSize) == 0) {
    return false;
  }
  return std::wstring(buffer).find(L'H') != std::wstring::npos;
}

/// The `flutter/settings` payload.
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
  return std::string(buffer.GetString(), buffer.GetSize());
}

/// The reader's languages, most preferred first.
///
/// Upstream's `GetPreferredLanguages`: a double-null-terminated buffer of BCP
/// 47 names from `GetThreadPreferredUILanguages`. `MUI_UI_FALLBACK` is what
/// makes the list non-empty on a machine whose preferred language has no
/// installed language pack.
std::vector<std::wstring> PreferredLanguages() {
  ULONG count = 0;
  ULONG size = 0;
  const DWORD flags = MUI_LANGUAGE_NAME | MUI_UI_FALLBACK;
  if (!GetThreadPreferredUILanguages(flags, &count, nullptr, &size)) {
    return {};
  }
  std::wstring buffer(size, L'\0');
  if (!GetThreadPreferredUILanguages(flags, &count, buffer.data(), &size)) {
    return {};
  }

  std::vector<std::wstring> languages;
  size_t start = 0;
  while (start < buffer.size() && buffer[start] != L'\0') {
    std::wstring language(buffer.c_str() + start);
    if (language.empty()) {
      break;
    }
    start += language.size() + 1;
    languages.push_back(std::move(language));
  }
  return languages;
}

/// One BCP 47 name split into the four parts the channel carries.
///
/// Upstream's `ParseLanguageName`, including the two rules that are not
/// obvious: a `-x-` suffix is private-use and is dropped, and a two-part name
/// is `language-script` if the second part is four letters long and
/// `language-region` otherwise -- because `zh-Hans` and `zh-CN` are the same
/// shape and only the length tells them apart.
struct LanguageInfo {
  std::string language;
  std::string region;
  std::string script;
};

LanguageInfo ParseLanguageName(const std::wstring& name) {
  std::vector<std::string> components;
  const std::string utf8 = Narrow(name);
  size_t start = 0;
  while (start <= utf8.size()) {
    size_t end = utf8.find('-', start);
    std::string component = utf8.substr(
        start, end == std::string::npos ? std::string::npos : end - start);
    if (component == "x") {
      break;
    }
    components.push_back(std::move(component));
    if (end == std::string::npos) {
      break;
    }
    start = end + 1;
  }

  LanguageInfo info;
  if (components.empty()) {
    return info;
  }
  info.language = components[0];
  if (components.size() >= 3) {
    info.script = components[1];
    info.region = components[2];
  } else if (components.size() == 2) {
    if (components[1].size() == 4) {
      info.script = components[1];
    } else {
      info.region = components[1];
    }
  }
  return info;
}

/// The `flutter/localization` payload.
///
/// The same JSON `FlutterEngineUpdateLocales` builds in embedder.cc: a
/// `setLocale` call whose arguments are a flat array of four strings per
/// locale, in language / country / script / variant order. Flat rather than
/// nested because that is the shape `Engine::HandleLocalizationPlatformMessage`
/// reads, and it is the shape because dart:ui reads it the same way.
std::optional<std::string> LocalizationPayload() {
  std::vector<std::wstring> languages = PreferredLanguages();
  if (languages.empty()) {
    return std::nullopt;
  }
  rapidjson::StringBuffer buffer;
  rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
  writer.StartObject();
  writer.Key("method");
  writer.String("setLocale");
  writer.Key("args");
  writer.StartArray();
  for (const auto& language : languages) {
    LanguageInfo info = ParseLanguageName(language);
    if (info.language.empty()) {
      continue;
    }
    writer.String(info.language.c_str());
    writer.String(info.region.c_str());
    writer.String(info.script.c_str());
    // Windows has no variant code to report. The slot is still written,
    // because the reader counts in fours.
    writer.String("");
  }
  writer.EndArray();
  writer.EndObject();
  return std::string(buffer.GetString(), buffer.GetSize());
}

//------------------------------------------------------------------------------
// Application exit.
//
// The strings and the shape of the exchange are upstream's
// `platform_handler.cc` and `windows_lifecycle_manager.cc`.
//
// Two methods and two directions. The framework can ask to exit
// (`System.exitApplication`), and the platform can ask the framework whether it
// *may* close (`System.requestAppExit`) -- which is the whole point of the
// pair: a desktop window has a close button, and an application with unsaved
// work needs a say in what it does.
//
// A close is only ever offered to the framework once the framework has said it
// is listening on `flutter/platform`. Before that there is nobody to answer,
// and a window whose close button silently did nothing would be worse than one
// that closes. That signal is the channel update -- see
// HostPlatformView::SendChannelUpdate.

constexpr char kExitRequestError[] = "ExitApplication error";
constexpr char kInvalidExitRequestMessage[] =
    "Invalid application exit request";
constexpr char kExitTypeRequired[] = "required";
constexpr char kExitTypeCancelable[] = "cancelable";

//------------------------------------------------------------------------------
/// What `flutter/platform` needs from the shell to answer an exit request.
///
/// An interface rather than a direct call because the handler below is defined
/// before the platform view that implements it, and because it is the whole of
/// what that handler is allowed to reach.
class ExitRequester {
 public:
  virtual ~ExitRequester() = default;

  /// Asks the framework whether the application may close, and closes it if
  /// the answer is yes. Upstream's `PlatformHandler::RequestAppExit`.
  virtual void RequestAppExit(bool cancelable, UINT exit_code) = 0;

  /// Closes without asking. Upstream's `PlatformHandler::QuitApplication`.
  virtual void QuitApplication(UINT exit_code) = 0;
};

/// Handles one call on `flutter/platform`.
///
/// Returns the reply, or nothing for a method this host does not implement --
/// which is answered with an empty message rather than an error, because that
/// is what tells the framework nobody served it. `SystemChannels.platform` is
/// an `OptionalMethodChannel` precisely so that an unimplemented method is
/// quiet rather than an exception.
std::optional<std::string> HandlePlatformCall(ExitRequester* requester,
                                              HWND window,
                                              const std::string& method,
                                              const rapidjson::Value* args) {
  if (method == "System.exitApplication") {
    // Upstream's `PlatformHandler::HandleMethodCall` and
    // `SystemExitApplication`. Both arguments are required and both are
    // checked, because a malformed request that closed the window anyway
    // would be a worse failure than an error.
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
    const auto exit_code = static_cast<UINT>(code->value.GetInt());
    // Anything that is not "cancelable" is required, as upstream's
    // `StringToAppExitType` decides it.
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

  if (method == "SystemChrome.setApplicationSwitcherDescription") {
    // The label is what the task switcher shows, which on Windows is the
    // window title. The colour has nowhere to go here.
    if (args != nullptr && args->IsObject()) {
      auto label = args->FindMember("label");
      if (label != args->MemberEnd() && label->value.IsString()) {
        SetWindowTextW(window, WidenText(label->value.GetString()).c_str());
      }
    }
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
// The mouse cursor.
//
// `flutter/mousecursor` is the first channel here that does not speak JSON. It
// speaks the binary standard codec, which is why it waited: the codec is
// `client_wrapper`'s, and reaching it meant first removing the dead
// `common_cpp_accessibility` target that stopped the whole BUILD file from
// loading. The handler below is upstream's `cursor_handler.cc`.

constexpr char kMouseCursorChannel[] = "flutter/mousecursor";

/// Upstream's `FlutterWindowsEngine::GetCursorByName`, name for name.
///
/// The framework picks the names, so this table is a protocol and not a
/// preference: `SystemMouseCursors.click` sends `"click"` and expects a hand.
/// An unknown name falls back to the arrow rather than failing, as upstream
/// does -- a newer framework naming a cursor this table has never heard of
/// should still leave the application usable.
HCURSOR CursorByName(const std::string& name) {
  static const auto* cursors = new std::map<std::string, const wchar_t*>{
      {"allScroll", IDC_SIZEALL},
      {"basic", IDC_ARROW},
      {"click", IDC_HAND},
      {"forbidden", IDC_NO},
      {"help", IDC_HELP},
      {"move", IDC_SIZEALL},
      // Not a cursor: `SetCursor(nullptr)` hides it, which is what "none"
      // means.
      {"none", nullptr},
      {"noDrop", IDC_NO},
      {"precise", IDC_CROSS},
      {"progress", IDC_APPSTARTING},
      {"text", IDC_IBEAM},
      {"resizeColumn", IDC_SIZEWE},
      {"resizeDown", IDC_SIZENS},
      {"resizeDownLeft", IDC_SIZENESW},
      {"resizeDownRight", IDC_SIZENWSE},
      {"resizeLeft", IDC_SIZEWE},
      {"resizeLeftRight", IDC_SIZEWE},
      {"resizeRight", IDC_SIZEWE},
      {"resizeRow", IDC_SIZENS},
      {"resizeUp", IDC_SIZENS},
      {"resizeUpDown", IDC_SIZENS},
      {"resizeUpLeft", IDC_SIZENWSE},
      {"resizeUpRight", IDC_SIZENESW},
      {"resizeUpLeftDownRight", IDC_SIZENWSE},
      {"resizeUpRightDownLeft", IDC_SIZENESW},
      {"wait", IDC_WAIT},
  };
  const wchar_t* idc_name = IDC_ARROW;
  auto found = cursors->find(name);
  if (found != cursors->end()) {
    idc_name = found->second;
  }
  return LoadCursor(nullptr, idc_name);
}

/// Handles one call on `flutter/mousecursor`.
///
/// Returns the encoded reply, or nothing for a method this host does not
/// serve, which the caller answers with an empty message.
///
/// Only `activateSystemCursor` is here. Upstream also has
/// `createCustomCursor/windows` and friends, which turn a rawBGRA buffer into
/// an `HCURSOR`; they are a separate feature with a separate framework API
/// (`SystemMouseCursors` versus a custom cursor asset), and a framework that
/// only ever sends system cursors never reaches them.
///
/// # Which thread
///
/// This runs on the platform thread and the window is on another one, so it
/// does not call `SetCursor` itself. Upstream can, because upstream's platform
/// thread *is* the window thread. Here the chosen cursor is stored where the
/// window thread can read it and the window is nudged, which reproduces both
/// halves of upstream's behaviour: the cursor changes at once even if the
/// pointer is not moving, and it survives the next `WM_SETCURSOR`.
std::optional<std::vector<uint8_t>> HandleMouseCursorCall(
    HWND window,
    std::atomic<HCURSOR>* cursor,
    const MethodCall<EncodableValue>& call) {
  const auto& codec = StandardMethodCodec::GetInstance();
  if (call.method_name() != "activateSystemCursor") {
    return std::nullopt;
  }

  const auto* arguments = std::get_if<EncodableMap>(call.arguments());
  if (arguments == nullptr) {
    return *codec.EncodeErrorEnvelope(
        "Argument error",
        "Missing argument while trying to activate system cursor");
  }
  auto kind = arguments->find(EncodableValue("kind"));
  if (kind == arguments->end()) {
    return *codec.EncodeErrorEnvelope(
        "Argument error",
        "Missing argument while trying to activate system cursor");
  }
  const auto* name = std::get_if<std::string>(&kind->second);
  if (name == nullptr) {
    return *codec.EncodeErrorEnvelope(
        "Argument error",
        "Missing argument while trying to activate system cursor");
  }

  cursor->store(CursorByName(*name), std::memory_order_relaxed);
  PostMessage(window, kMessageCursorChanged, 0, 0);
  return *codec.EncodeSuccessEnvelope();
}

std::optional<std::string> TextInputHandler::HandleMethodCall(
    const std::string& method,
    const rapidjson::Value* args) {
  if (method == "TextInput.show" || method == "TextInput.hide") {
    // No-ops, as upstream: there is no on-screen keyboard to raise, and the
    // IME is enabled by the window having focus.
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
    // A composition in flight is committed first, and the framework is told,
    // before the client goes. Upstream does the same: half-typed text that the
    // reader has already seen on screen must not vanish because focus moved.
    bool was_composing = false;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (model_ != nullptr && model_->composing()) {
        model_->CommitComposing();
        model_->EndComposing();
        was_composing = true;
      }
    }
    if (was_composing) {
      SendStateUpdate();
    }
    // The IME still thinks it is composing; the window thread ends that.
    abort_composing_.store(true);
    {
      std::lock_guard<std::mutex> lock(mutex_);
      model_.reset();
      caret_valid_ = false;
    }
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
      return ErrorEnvelope("TextInput.noClient",
                           "the editing state was set with no client attached");
    }
    // The framework is the authority here: this is it telling the platform
    // what the field now holds, which is how a programmatic edit -- a paste, a
    // clear button -- reaches the IME.
    model_->SetText(text->value.GetString(),
                    TextRange(static_cast<size_t>(base < 0 ? 0 : base),
                              static_cast<size_t>(extent < 0 ? 0 : extent)),
                    composing_base < 0 || composing_extent < 0
                        ? TextRange(0, 0)
                        : TextRange(static_cast<size_t>(composing_base),
                                    static_cast<size_t>(composing_extent)));
    return NullEnvelope();
  }

  if (method == "TextInput.setMarkedTextRect") {
    // Where the composing text sits, in the editable's own coordinates.
    if (args == nullptr || !args->IsObject()) {
      return ErrorEnvelope("TextInput.badArgument",
                           "Method invoked without args");
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
      return ErrorEnvelope("TextInput.badArgument",
                           "Composing rect values invalid.");
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
    // A 4x4 matrix, row-major, sixteen numbers -- not one member per cell.
    // Only its translation is used, which is entries 12 and 13: an IME window
    // cannot be rotated or scaled, so where the editable's origin lands is the
    // whole of what the platform can act on.
    if (args == nullptr || !args->IsObject()) {
      return ErrorEnvelope("TextInput.badArgument",
                           "Method invoked without args");
    }
    auto transform = args->FindMember("transform");
    if (transform == args->MemberEnd() || !transform->value.IsArray() ||
        transform->value.Size() != 16) {
      return ErrorEnvelope("TextInput.badArgument",
                           "EditableText transform invalid.");
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

  return std::nullopt;
}

bool TextInputHandler::OnEditingKey(WPARAM virtual_key, bool shift) {
  bool changed = false;
  const bool handled = Edit([&](TextInputModel& model) {
    switch (virtual_key) {
      case VK_BACK:
        changed = model.Backspace();
        return true;
      case VK_DELETE:
        changed = model.Delete();
        return true;
      case VK_LEFT:
        changed = model.MoveCursorBack();
        return true;
      case VK_RIGHT:
        changed = model.MoveCursorForward();
        return true;
      case VK_HOME:
        changed =
            shift ? model.SelectToBeginning() : model.MoveCursorToBeginning();
        return true;
      case VK_END:
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

void TextInputHandler::OnAction() {
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
    // action, which is upstream's `EnterPressed`. Anything else only gets the
    // action -- Enter in a single-line field submits, it does not insert.
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

void TextInputHandler::SendStateUpdate() {
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
  // The keys, and their order, are upstream's. A field the framework does not
  // find is a field it substitutes a default for, silently.
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

//------------------------------------------------------------------------------
/// The state the window thread and the platform thread both touch.
///
/// Everything else the window keeps belongs to the window thread alone. These
/// do not: the framework changes them from the platform thread and the window
/// proc reads them, so they are atomics rather than plain fields. Upstream has
/// no equivalent because upstream's window and platform thread are the same
/// thread.
struct SharedWindowState {
  /// What `WM_SETCURSOR` applies. Null hides the cursor, which is what the
  /// framework's `"none"` asks for.
  std::atomic<HCURSOR> cursor{nullptr};

  /// Whether the framework is listening on `flutter/platform`, and so whether
  /// there is anybody to ask before closing the window.
  std::atomic<bool> exit_processing{false};

  /// Set once an exit has been approved, so that the `WM_CLOSE` which follows
  /// is not offered to the framework a second time. Upstream keeps a multiset
  /// of the close messages it re-dispatched; with a single window a flag says
  /// the same thing.
  std::atomic<bool> quit_approved{false};

  /// What the process exits with, from `System.exitApplication`.
  std::atomic<UINT> exit_code{0};
};

//------------------------------------------------------------------------------
/// A reply to a message this host sent the framework.
///
/// The mirror image of `RustPlatformMessageResponse` in runtime_controller.cc:
/// that one carries the framework's answer out to an embedder, this one carries
/// the framework's answer back to the host. Completed on the UI thread, so the
/// callback is posted to the platform thread rather than run where it lands.
///
/// Called exactly once, either way: the framework not handling the message
/// completes it empty rather than dropping it, and a caller that must clean up
/// after itself can therefore do so on the null.
class HostPlatformMessageResponse : public PlatformMessageResponse {
 public:
  using Callback = std::function<void(const uint8_t*, size_t)>;

  void Complete(std::unique_ptr<fml::Mapping> data) override {
    if (data == nullptr) {
      CompleteEmpty();
      return;
    }
    Post(std::vector<uint8_t>(data->GetMapping(),
                              data->GetMapping() + data->GetSize()));
  }

  void CompleteEmpty() override { Post({}); }

 private:
  HostPlatformMessageResponse(fml::RefPtr<fml::TaskRunner> task_runner,
                              Callback callback)
      : task_runner_(std::move(task_runner)), callback_(std::move(callback)) {}

  void Post(std::vector<uint8_t> reply) {
    if (is_complete_) {
      FML_LOG(ERROR) << "Platform message response completed more than once.";
      return;
    }
    is_complete_ = true;
    if (!task_runner_) {
      return;
    }
    task_runner_->PostTask(fml::MakeCopyable(
        [callback = callback_, reply = std::move(reply)]() mutable {
          callback(reply.empty() ? nullptr : reply.data(), reply.size());
        }));
  }

  fml::RefPtr<fml::TaskRunner> task_runner_;
  Callback callback_;

  FML_FRIEND_MAKE_REF_COUNTED(HostPlatformMessageResponse);
  FML_FRIEND_REF_COUNTED_THREAD_SAFE(HostPlatformMessageResponse);
  FML_DISALLOW_COPY_AND_ASSIGN(HostPlatformMessageResponse);
};

//------------------------------------------------------------------------------
/// The shell's view of the window.
///
/// Lives on the platform thread. AcquireBackingStore and PresentBackingStore
/// are called on the raster thread; both are safe there because the surface is
/// per-frame and the frame buffer is locked.
class HostPlatformView final : public PlatformView,
                               public GPUSurfaceSoftwareDelegate,
                               public ExitRequester {
 public:
  HostPlatformView(PlatformView::Delegate& delegate,
                   const TaskRunners& task_runners,
                   HWND window,
                   FrameBuffer* frame_buffer,
                   TextInputHandler* text_input,
                   SharedWindowState* shared,
                   AccessibilityBridgeWin* a11y,
                   bool prefer_impeller)
      : PlatformView(delegate, task_runners),
        window_(window),
        frame_buffer_(frame_buffer),
        text_input_(text_input),
        shared_(shared),
        a11y_(a11y),
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
  /// on PlatformView to add one to. RuntimeController unpacks it.
  ///
  /// No response handle is asked for. The reply says whether the framework used
  /// the key, and the only thing to do with that answer is to re-post the
  /// unhandled ones so the system still sees them -- which this host does not
  /// do, because it never withheld them in the first place.
  // |PlatformView|
  //
  // A message from the framework, on the platform thread. Upstream this is
  // where an embedder's plugins are dispatched to; here it is the three
  // channels this host serves. Anything else falls through to an empty reply,
  // which the framework reads as "nobody implements this".
  void HandlePlatformMessage(
      std::unique_ptr<PlatformMessage> message) override {
    const auto& data = message->data();
    std::optional<std::vector<uint8_t>> reply;

    if (message->channel() == kMouseCursorChannel) {
      // The one channel here that is binary rather than JSON.
      auto call = StandardMethodCodec::GetInstance().DecodeMethodCall(
          data.GetMapping(), data.GetSize());
      if (call) {
        reply = HandleMouseCursorCall(window_, &shared_->cursor, *call);
      }
    } else {
      const bool platform = message->channel() == kPlatformChannel;
      const bool editing = message->channel() == kTextInputChannel;
      if (!platform && !editing) {
        PlatformView::HandlePlatformMessage(std::move(message));
        return;
      }

      rapidjson::Document document;
      document.Parse(reinterpret_cast<const char*>(data.GetMapping()),
                     data.GetSize());
      if (!document.HasParseError() && document.IsObject()) {
        auto method = document.FindMember("method");
        if (method != document.MemberEnd() && method->value.IsString()) {
          auto found = document.FindMember("args");
          const rapidjson::Value* args =
              found == document.MemberEnd() ? nullptr : &found->value;
          std::optional<std::string> json =
              platform ? HandlePlatformCall(this, window_,
                                            method->value.GetString(), args)
                       : text_input_->HandleMethodCall(
                             method->value.GetString(), args);
          if (json.has_value()) {
            reply.emplace(json->begin(), json->end());
          }
        }
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
    response->Complete(std::make_unique<fml::DataMapping>(std::move(*reply)));
  }

  //----------------------------------------------------------------------------
  /// Hands one frame's semantics tree to the bridge.
  ///
  /// Upstream this is `FlutterWindowsView::UpdateSemantics`, which feeds an
  /// `AXTree` and lets the event generator work out what changed. The bridge
  /// here compares the two snapshots itself, which is the same job on a tree
  /// small enough that a comparison is cheaper than a change log.
  ///
  /// Runs on the platform thread. The window thread is the one UI Automation
  /// talks to, so the events are raised there instead -- hence the post.
  ///
  /// |PlatformView|
  void UpdateSemantics(int64_t view_id,
                       SemanticsNodeUpdates update,
                       CustomAccessibilityActionUpdates actions) override {
    if (a11y_ == nullptr) {
      return;
    }
    a11y_->Update(update);
    PostMessage(window_, kMessageSemanticsUpdated, 0, 0);
  }

  // |PlatformView|
  //
  // The framework has started or stopped listening on a channel.
  //
  // Upstream's `FlutterWindowsEngine::OnChannelUpdate`, and it exists for one
  // reason: a close button must not hang. Asking the framework whether it is
  // all right to close is only meaningful once something over there is
  // listening for the question, and until then the window closes the ordinary
  // way. The framework sends this the moment `ServicesBinding` installs its
  // `flutter/platform` handler.
  void SendChannelUpdate(const std::string& name, bool listening) override {
    if (name == kPlatformChannel) {
      shared_->exit_processing.store(listening, std::memory_order_relaxed);
    }
  }

  // |ExitRequester|
  void RequestAppExit(bool cancelable, UINT exit_code) override {
    // Upstream's `PlatformHandler::RequestAppExit`. The type is sent even
    // though only "cancelable" is ever asked about, because the framework
    // branches on it: a required exit is reported to
    // `AppLifecycleListener.onExitRequested` as something it may not veto.
    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.StartObject();
    writer.Key("type");
    writer.String(cancelable ? kExitTypeCancelable : kExitTypeRequired);
    writer.EndObject();

    SendMethodCallWithReply(
        kPlatformChannel, "System.requestAppExit",
        std::string(buffer.GetString(), buffer.GetSize()),
        [this, exit_code](const uint8_t* reply, size_t length) {
          HandleExitResponse(reply, length, exit_code);
        });
  }

  // |ExitRequester|
  void QuitApplication(UINT exit_code) override {
    shared_->exit_code.store(exit_code, std::memory_order_relaxed);
    // Upstream's `WindowsLifecycleManager::Quit` re-dispatches the WM_CLOSE it
    // swallowed and lets it through the second time. Same thing, minus the
    // bookkeeping a multi-window embedder needs: the flag the window proc
    // checks is set before the message is posted.
    shared_->quit_approved.store(true, std::memory_order_relaxed);
    PostMessage(window_, kMessageQuitApproved, 0, 0);
  }

  //----------------------------------------------------------------------------
  /// Tells the framework the reader's settings and languages.
  ///
  /// Sent once at startup and again whenever Windows says they changed. Both
  /// are consumed by `Engine` rather than delivered to a channel handler; see
  /// the readers above.
  void SendPlatformSettings() {
    SendOnChannel(kSettingsChannel, SettingsPayload());
    if (auto locales = LocalizationPayload()) {
      SendOnChannel(kLocalizationChannel, *locales);
    }
  }

  //----------------------------------------------------------------------------
  /// Tells the framework what the application is doing.
  ///
  /// One bare string on `flutter/lifecycle`, with no codec and no envelope --
  /// there is nothing else the channel could ever need to say. `Engine` records
  /// it on the way past and deliberately does not consume it, so the framework
  /// sees it too.
  void SendLifecycleState(const char* state) {
    SendOnChannel(kLifecycleChannel, std::string(state));
  }

  //----------------------------------------------------------------------------
  /// Sends a JSON method call, with no reply asked for.
  ///
  /// What `TextInputClient.updateEditingState` travels on. No reply because
  /// there is nothing to do with one: the framework applying an editing state
  /// is not something the platform can fail at or wait for.
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

 private:
  //----------------------------------------------------------------------------
  /// Sends a JSON method call and waits for its reply.
  ///
  /// The only caller is the exit request, and that is not a coincidence: a
  /// platform message the *host* needs an answer to is rare, because almost
  /// everything the platform has to say is a statement rather than a question.
  ///
  /// The callback runs on the platform thread, with null for "nobody handled
  /// it" -- which for an exit request is the same as "no objection", because a
  /// framework with no exit listener has nothing to save.
  void SendMethodCallWithReply(const char* channel,
                               const std::string& method,
                               const std::string& arguments_json,
                               HostPlatformMessageResponse::Callback callback) {
    rapidjson::StringBuffer buffer;
    rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
    writer.StartObject();
    writer.Key("method");
    writer.String(method.c_str());
    writer.Key("args");
    writer.RawValue(arguments_json.c_str(), arguments_json.size(),
                    rapidjson::kObjectType);
    writer.EndObject();
    std::string payload(buffer.GetString(), buffer.GetSize());

    auto response = fml::MakeRefCounted<HostPlatformMessageResponse>(
        task_runners_.GetPlatformTaskRunner(), std::move(callback));
    auto message = std::make_unique<PlatformMessage>(
        channel, fml::MallocMapping::Copy(payload.data(), payload.size()),
        std::move(response));
    task_runners_.GetPlatformTaskRunner()->PostTask(fml::MakeCopyable(
        [weak = GetWeakPtr(), message = std::move(message)]() mutable {
          if (weak) {
            weak->DispatchPlatformMessage(std::move(message));
          }
        }));
  }

  //----------------------------------------------------------------------------
  /// What the framework said when asked whether it may close.
  ///
  /// Upstream's `PlatformHandler::RequestAppExitSuccess`, and its one rule:
  /// only `"exit"` closes the window. Anything else -- a cancel, a malformed
  /// answer, no answer at all -- leaves the window open, which is the safe way
  /// round for a question whose whole purpose is to protect unsaved work.
  void HandleExitResponse(const uint8_t* reply, size_t length, UINT exit_code) {
    if (reply == nullptr || length == 0) {
      // Nobody handled it. Upstream never sees this case, because it only
      // asks once the framework has said it is listening; here the listener
      // could have gone away between the channel update and the question.
      QuitApplication(exit_code);
      return;
    }
    rapidjson::Document envelope;
    envelope.Parse(reinterpret_cast<const char*>(reply), length);
    // A success envelope, which is a one-element array holding the result.
    if (envelope.HasParseError() || !envelope.IsArray() ||
        envelope.Size() != 1 || !envelope[0].IsObject()) {
      FML_LOG(ERROR) << "Application exit request response did not contain a "
                        "valid response value.";
      return;
    }
    auto response = envelope[0].FindMember("response");
    if (response == envelope[0].MemberEnd() || !response->value.IsString()) {
      FML_LOG(ERROR) << "Application exit request response did not contain a "
                        "valid response value.";
      return;
    }
    if (std::string(response->value.GetString()) == "exit") {
      QuitApplication(exit_code);
    }
  }

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

 public:
  //----------------------------------------------------------------------------
  /// Sends one key event and brings the framework's verdict back.
  ///
  /// `sequence_id` is the window thread's name for this key. It travels out
  /// with the message and comes back with the answer, because by then the
  /// window thread has moved on and the key it refers to is one of several it
  /// may be holding.
  ///
  /// The answer is one byte: 1 if the framework consumed the key. An empty
  /// reply -- no listener, or the runtime shutting down -- reads as "not
  /// consumed", which is the safe way round: a key nobody wanted goes back to
  /// the system rather than disappearing.
  void SendKey(const KeyData& data,
               const std::string& character,
               uint64_t sequence_id) {
    KeyDataPacket packet(data, character.empty() ? nullptr : character.c_str());
    HWND window = window_;
    auto response = fml::MakeRefCounted<HostPlatformMessageResponse>(
        task_runners_.GetPlatformTaskRunner(),
        [window, sequence_id](const uint8_t* reply, size_t length) {
          const bool handled = length > 0 && reply != nullptr && reply[0] != 0;
          PostMessage(window, kMessageKeyResult,
                      static_cast<WPARAM>(sequence_id),
                      static_cast<LPARAM>(handled ? 1 : 0));
        });
    auto message = std::make_unique<PlatformMessage>(
        kKeyDataChannel,
        fml::MallocMapping::Copy(packet.data().data(), packet.data().size()),
        std::move(response));
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
    gl_delegate_ = std::make_unique<ImpellerGlDelegate>(
        gl_context_.get(), reinterpret_cast<EGLNativeWindowType>(window_));
    if (!gl_delegate_->IsValid()) {
      gl_delegate_.reset();
      return nullptr;
    }
    // The first swap is what makes the deferred ShowWindow happen; the
    // window proc does the rest. Only the first matters, and the delegate
    // drops the callback after firing it.
    gl_delegate_->SetFirstPresentCallback([window = window_]() {
      PostMessage(window, kMessageFramePresented, 0, 0);
    });

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
  TextInputHandler* text_input_ = nullptr;
  /// What a screen reader is answered from. Owned by the window, which
  /// outlives the shell.
  AccessibilityBridgeWin* a11y_ = nullptr;
  SharedWindowState* shared_ = nullptr;
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
/// One key or character message, exactly as Windows delivered it.
///
/// Kept whole because an unhandled key is put back into the queue verbatim;
/// anything reconstructed from the decoded fields would be a different message.
/// Upstream's `Win32Message`.
struct Win32Message {
  UINT action = 0;
  WPARAM wparam = 0;
  LPARAM lparam = 0;
};

struct PendingKey {
  UINT action = 0;
  uint16_t virtual_key = 0;
  uint8_t scan_code = 0;
  bool extended = false;
  bool was_down = false;
};

/// A key session that has gone to the framework and is waiting for an answer.
///
/// Upstream's `KeyboardManager::PendingEvent`. The session is every Win32
/// message that made up the key -- a key down and the character it produced,
/// or a surrogate pair's two halves -- because they all go back together if
/// the framework declines.
struct PendingKeyEvent {
  uint64_t sequence_id = 0;
  std::vector<Win32Message> session;
  /// The text this session would enter, if it enters any. Held rather than
  /// delivered, because a key the framework consumes must not also type.
  std::u16string text;
  /// A key down that should reach the attached field's editing if nobody else
  /// wants it: an arrow, Backspace, Enter. Zero for anything else.
  WPARAM editing_key = 0;
  bool editing_shift = false;
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
  /// Whether Windows has been asked to report the mouse leaving this window.
  /// The request is one-shot -- it is spent by the WM_MOUSELEAVE it produces --
  /// so it is re-armed by the next hover.
  bool tracking_mouse_leave = false;

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
  /// The Win32 messages that make up the session being assembled.
  std::vector<Win32Message> current_session;
  /// Sessions sent to the framework whose answer has not come back yet.
  std::map<uint64_t, PendingKeyEvent> unanswered_keys;
  uint64_t last_key_sequence = 0;
  /// Messages this host put back into the queue after the framework declined
  /// them. The first one that matches on the way back in is a message that has
  /// already been offered, and is let through untouched. Upstream's
  /// `pending_redispatches_`, matched the same way -- on action and wparam,
  /// because the lparam of a synthesised message is not bit-identical.
  std::deque<Win32Message> pending_redispatches;
  /// What the framework has last been told about Shift and Control, so
  /// SyncModifiers knows what it has to correct.
  bool shift_reported = false;
  bool control_reported = false;
  /// The last lifecycle state the framework was told about, so a window that
  /// is activated twice does not say so twice. The states are level-triggered:
  /// each one replaces the last, and repeating one carries no information.
  std::string lifecycle_state;
  /// The window itself, so the key machinery can put a declined message back
  /// into its own queue from a task that has no `hwnd` to hand.
  HWND window = nullptr;
  /// The text field the framework has attached, and the IME serving it.
  TextInputHandler text_input;
  /// What a screen reader is answered from. Created with the window rather
  /// than when a reader arrives, because it costs a mutex and two empty maps
  /// and the alternative is a lock around its own creation.
  std::optional<AccessibilityBridgeWin> a11y;
  std::optional<ImeContext> ime;
  /// The cursor and the exit handshake, which the platform thread also writes.
  SharedWindowState shared;
};

/// Puts the IME's window where the framework says the caret is.
void UpdateImePosition(WindowState* state) {
  if (!state->ime.has_value()) {
    return;
  }
  double x = 0;
  double y = 0;
  double width = 0;
  double height = 0;
  if (state->text_input.TakeCaretRect(&x, &y, &width, &height)) {
    // The framework works in logical pixels and the IMM APIs want physical
    // client-area pixels, so the caret rect crosses scaled by the DPI -- the
    // step upstream's `FlutterWindow::OnCursorRectUpdated` applies
    // `GetDpiScale()` at. Without it the composition and candidate windows
    // land at 1/dpr of the caret's real place.
    const double dpr = state->device_pixel_ratio;
    state->ime->SetCaretRect(x * dpr, y * dpr, width * dpr, height * dpr);
  } else {
    state->ime->UpdateImeWindow();
  }
}

/// The IME's caret inside the composing text.
///
/// Some IMEs change the composition without saying where their caret went. The
/// framework still needs one, and the end of the text is where the reader is
/// looking. Upstream's `GetCursorPositionForComposition`.
int ComposingCursor(const ImeContext& ime, LPARAM lparam, size_t length) {
  if (!(lparam & GCS_CURSORPOS)) {
    return static_cast<int>(length);
  }
  const long position = ime.ComposingCursorPosition();
  if (position < 0 || static_cast<size_t>(position) > length) {
    return static_cast<int>(length);
  }
  return static_cast<int>(position);
}

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
PointerData MakeScrollData(WindowState* state,
                           double x,
                           double y,
                           double notches) {
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
// this is `KeyboardManager`, and the shape is the same, because the problem is:
// Win32 asks whether a key was handled and wants the answer before the window
// proc returns, while the framework's answer is on another thread and arrives
// whenever it arrives.
//
// The answer to that is upstream's redispatch algorithm, ported here. A key is
// taken on the way in and sent to the framework with a sequence id. When the
// verdict comes back -- `kMessageKeyResult`, posted to this thread -- a key the
// framework wanted is simply dropped, and a key it declined is put back into
// this window's own queue verbatim, remembered in `pending_redispatches`, and
// recognised on the way back in so that the second copy falls through to
// `DefWindowProc`. That is what makes a consumed key actually consumed: Tab can
// move the focus without also being typed into the field it left, and a
// shortcut can be taken without also entering a character.
//
// Three things are deliberately outside it, all of them upstream's rules too.
// System keys (`IsSysAction`) are reported but never taken and never put back,
// because their default processing is Alt+F4 and the window menu. Text is
// withheld until the verdict rather than delivered on the way in, for the same
// reason as the key itself. And a key that arrives with no framework to ask --
// during start-up or shutdown -- is applied at once rather than dropped.
//
// The rest is about Windows telling the truth awkwardly: pairing a key down
// with the character it turns out to produce, surrogate pairs, dead keys, and
// the modifier bookkeeping below.

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
  return 0x10000 + ((static_cast<char32_t>(high) & 0x03FF) << 10) +
         (low & 0x3FF);
}

/// Which Shift, which Control, which Alt.
///
/// `VK_SHIFT` does not say. For Shift the scan code is asked, because the two
/// sides sit at different positions; for Control and Alt the extended flag is
/// what separates right from left. Straight from upstream's `ResolveKeyCode`.
uint16_t ResolveVirtualKey(uint16_t virtual_key,
                           bool extended,
                           uint8_t scan_code) {
  switch (virtual_key) {
    case VK_SHIFT:
    case VK_LSHIFT:
      return static_cast<uint16_t>(
          MapVirtualKey(scan_code, MAPVK_VSC_TO_VK_EX));
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

/// Whether a message is a system-key one -- Alt is held, or this is Alt itself.
///
/// They are treated differently throughout: never withheld from
/// `DefWindowProc`, and never put back, because their originals were passed to
/// the system's default processing on the way in. Upstream's `IsSysAction`,
/// used in the same three places.
bool IsSysAction(UINT action) {
  return action == WM_SYSKEYDOWN || action == WM_SYSKEYUP ||
         action == WM_SYSCHAR || action == WM_SYSDEADCHAR;
}

/// Builds and sends one key event. Returns the sequence id it was sent with,
/// or zero when there is nothing to send it to.
uint64_t SendKeyEvent(WindowState* state,
                      KeyEventType type,
                      uint16_t virtual_key,
                      uint8_t scan_code,
                      bool extended,
                      const std::string& character,
                      bool synthesized) {
  if (state->platform_view == nullptr) {
    return 0;
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
  const uint64_t sequence_id = ++state->last_key_sequence;
  state->platform_view->SendKey(data, character, sequence_id);

  // Whatever was just reported is now what the framework believes, which is
  // what SyncModifiers compares against.
  if (virtual_key == VK_LSHIFT || virtual_key == VK_RSHIFT) {
    state->shift_reported = (type != KeyEventType::kUp);
  } else if (virtual_key == VK_LCONTROL || virtual_key == VK_RCONTROL) {
    state->control_reported = (type != KeyEventType::kUp);
  }
  return sequence_id;
}

/// Everything a key does when nobody in the framework wanted it.
///
/// Text is entered, an editing key is applied, and the raw messages go back
/// into the queue for the system to make of them what it will. All three are
/// held until the answer arrives rather than done on the way in, because that
/// is the whole difference a redispatch makes: a shortcut the framework
/// consumes must not also type a character, and Tab moving the focus must not
/// also reach the field it left.
///
/// Upstream this is `KeyboardManager::HandleOnKeyResult` plus `DispatchText`
/// and `RedispatchEvent`, in this order and for these reasons.
void ApplyDeclinedKey(WindowState* state, HWND hwnd, PendingKeyEvent& event) {
  if (!event.text.empty()) {
    state->text_input.OnText(event.text);
  }
  if (event.editing_key != 0 && state->text_input.attached()) {
    if (event.editing_key == VK_RETURN) {
      state->text_input.OnAction();
    } else {
      state->text_input.OnEditingKey(event.editing_key, event.editing_shift);
    }
  }

  for (const Win32Message& message : event.session) {
    // System keys are never put back: their originals already went to
    // DefWindowProc on the way in, so a second copy would be a second Alt+F4.
    if (IsSysAction(message.action)) {
      continue;
    }
    state->pending_redispatches.push_back(message);
    SendMessage(hwnd, message.action, message.wparam, message.lparam);
  }
}

/// The framework's verdict, arriving on the window thread.
void HandleKeyResult(WindowState* state,
                     HWND hwnd,
                     uint64_t sequence_id,
                     bool handled) {
  auto found = state->unanswered_keys.find(sequence_id);
  if (found == state->unanswered_keys.end()) {
    // An answer to a key from before the last window, or a duplicate. Neither
    // is worth doing anything about; both are worth not crashing on.
    return;
  }
  PendingKeyEvent event = std::move(found->second);
  state->unanswered_keys.erase(found);
  if (handled) {
    return;
  }
  ApplyDeclinedKey(state, hwnd, event);
}

/// Turns one completed session into a key event, and remembers it until the
/// framework answers.
void SendKeyFromSession(WindowState* state,
                        const PendingKey& key,
                        const std::string& character,
                        PendingKeyEvent&& event) {
  const bool is_down = key.action == WM_KEYDOWN || key.action == WM_SYSKEYDOWN;

  // An editing key reaches the attached field only if nobody in the framework
  // wanted it first. That ordering is the point of the whole exercise: it is
  // what lets Tab move the focus out of a field rather than being delivered to
  // the field it is leaving.
  //
  // Recorded here rather than where the session started, because which of the
  // two branches ends a session is not the same question as which key it was:
  // Backspace and Enter both produce a WM_CHAR, so both conclude on the
  // character side, and both are editing keys.
  if (is_down) {
    event.editing_key = key.virtual_key;
    event.editing_shift = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
  }
  const KeyEventType type =
      !is_down ? KeyEventType::kUp
               : (key.was_down ? KeyEventType::kRepeat : KeyEventType::kDown);
  const uint64_t sequence_id =
      SendKeyEvent(state, type, key.virtual_key, key.scan_code, key.extended,
                   character, /*synthesized=*/false);
  if (sequence_id == 0) {
    // There is no framework to ask -- the shell is coming up or going down.
    // The key still has to do what it would have done, or the window would
    // swallow everything typed into it during those two windows of time.
    ApplyDeclinedKey(state, state->window, event);
    return;
  }
  event.sequence_id = sequence_id;
  state->unanswered_keys.emplace(sequence_id, std::move(event));

  // Upstream keeps the same watch on the same queue. A backlog here means keys
  // are being taken and never given back, which reads to the person typing as
  // a keyboard that has stopped working -- worth a line in the log rather than
  // a silent pile.
  constexpr size_t kMaxUnansweredKeys = 1000;
  if (state->unanswered_keys.size() > kMaxUnansweredKeys) {
    FML_LOG(ERROR) << state->unanswered_keys.size()
                   << " key events have had no answer from the framework.";
  }
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
    SendKeyEvent(state, shift_held ? KeyEventType::kDown : KeyEventType::kUp,
                 VK_LSHIFT, kScanCodeShiftLeft, /*extended=*/false,
                 /*character=*/"", /*synthesized=*/true);
  }
  if (control_held != state->control_reported) {
    SendKeyEvent(state, control_held ? KeyEventType::kDown : KeyEventType::kUp,
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
bool HandleKeyMessage(WindowState* state,
                      UINT action,
                      WPARAM wparam,
                      LPARAM lparam) {
  state->current_session.push_back(
      Win32Message{.action = action, .wparam = wparam, .lparam = lparam});

  switch (action) {
    case WM_CHAR:
    case WM_SYSCHAR:
    case WM_DEADCHAR:
    case WM_SYSDEADCHAR: {
      const auto unit = static_cast<wchar_t>(wparam);

      // A code point outside the basic plane arrives as two messages. The high
      // half is kept and the low half completes it -- and both halves stay in
      // the session, because both go back if nobody wants them.
      char32_t code_point;
      if (unit >= 0xD800 && unit <= 0xDBFF) {
        state->pending_high_surrogate = unit;
        return true;
      }
      if (unit >= 0xDC00 && unit <= 0xDFFF) {
        if (state->pending_high_surrogate == 0) {
          // A low surrogate with no high one before it. Malformed, and not
          // this host's to hold on to.
          state->current_session.clear();
          return false;
        }
        code_point =
            CodePointFromSurrogatePair(state->pending_high_surrogate, unit);
        state->pending_high_surrogate = 0;
      } else {
        state->pending_high_surrogate = 0;
        code_point = unit;
      }

      // The text this session would enter, if the framework turns out not to
      // want the key. Held rather than delivered: a shortcut the framework
      // consumes must not also type a character. Decided here whether or not a
      // key down preceded it -- Alt with the numeric keypad produces a
      // character on its own, and it is still text.
      PendingKeyEvent event;
      if (action == WM_CHAR && IsPrintable(code_point) &&
          state->text_input.attached()) {
        if (code_point > 0xFFFF) {
          const char32_t offset = code_point - 0x10000;
          event.text.push_back(static_cast<char16_t>(0xD800 + (offset >> 10)));
          event.text.push_back(
              static_cast<char16_t>(0xDC00 + (offset & 0x3FF)));
        } else {
          event.text.push_back(static_cast<char16_t>(code_point));
        }
      }
      event.session = std::move(state->current_session);
      state->current_session.clear();

      if (!state->pending_key.has_value()) {
        // A character with no key down before it: Alt and the numeric keypad,
        // or an IME committing. There is no key to ask the framework about, so
        // the text goes straight in -- and nothing is put back, because there
        // was no key event to decline. Upstream's `PerformProcessEvent` takes
        // the same early exit for a bare `WM_CHAR`.
        if (!event.text.empty()) {
          state->text_input.OnText(event.text);
        }
        return true;
      }
      const PendingKey key = *state->pending_key;
      state->pending_key.reset();

      // Only WM_CHAR is text. WM_SYS*CHAR is a system-menu accelerator, and
      // WM_DEADCHAR is half of a character that a later WM_CHAR will complete.
      std::string character;
      if (action == WM_CHAR && IsPrintable(code_point)) {
        character = Utf8FromCodePoint(code_point);
      }
      SendKeyFromSession(state, key, character, std::move(event));
      return true;
    }

    case WM_KEYDOWN:
    case WM_SYSKEYDOWN:
    case WM_KEYUP:
    case WM_SYSKEYUP: {
      // VK_PACKET is an injected Unicode character wearing a key's clothes. It
      // has no scan code and no key to speak of; its WM_CHAR carries the point.
      if (wparam == VK_PACKET) {
        state->current_session.clear();
        return false;
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
      // attaching its character to this key. Everything in the session before
      // this message belongs to it.
      if (state->pending_key.has_value()) {
        const PendingKey stale = *state->pending_key;
        state->pending_key.reset();
        PendingKeyEvent orphan;
        orphan.session.assign(state->current_session.begin(),
                              state->current_session.end() - 1);
        state->current_session.erase(state->current_session.begin(),
                                     state->current_session.end() - 1);
        SendKeyFromSession(state, stale, "", std::move(orphan));
      }

      const bool is_down = action == WM_KEYDOWN || action == WM_SYSKEYDOWN;
      if (is_down) {
        const uint32_t mapped =
            MapVirtualKey(key.virtual_key, MAPVK_VK_TO_CHAR);
        // The dead-key bit means the mapping is real but deferred; either way a
        // character message follows, so peek for one rather than trusting the
        // mapping. Ctrl+digit maps to a character and produces no WM_CHAR.
        if ((mapped & ~kDeadKeyCharMask) != 0) {
          MSG next = {};
          if (PeekMessage(&next, nullptr, WM_KEYFIRST, WM_KEYLAST,
                          PM_NOREMOVE) &&
              (next.message == WM_CHAR || next.message == WM_SYSCHAR ||
               next.message == WM_DEADCHAR || next.message == WM_SYSDEADCHAR)) {
            // The character decides what this key produced, so the session
            // waits for it. Taken for now; the character message concludes it.
            state->pending_key = key;
            return true;
          }
        }
      }

      PendingKeyEvent event;
      event.session = std::move(state->current_session);
      state->current_session.clear();
      SendKeyFromSession(state, key, "", std::move(event));
      // System keys are reported but never taken: their default processing is
      // the window menu, Alt+F4, and the rest of what Alt means to Windows.
      return !IsSysAction(action);
    }

    default:
      state->current_session.clear();
      return false;
  }
}

/// Whether this message is one this host put back, and so has already been
/// offered to the framework once.
///
/// Matched on action and wparam only, which is upstream's
/// `RemoveRedispatchedMessage`: a message that has been through `SendMessage`
/// does not come back with a bit-identical lparam.
bool IsRedispatched(WindowState* state, UINT action, WPARAM wparam) {
  for (auto it = state->pending_redispatches.begin();
       it != state->pending_redispatches.end(); ++it) {
    if (it->action == action && it->wparam == wparam) {
      state->pending_redispatches.erase(it);
      return true;
    }
  }
  return false;
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
      // The first presented frame is the cue to show the window, which
      // rf_host_run left hidden so the blank startup window is never on
      // screen. The timer is the fallback for a first frame that never
      // comes.
      if (!IsWindowVisible(hwnd)) {
        KillTimer(hwnd, kTimerShowWindowFallback);
        ShowWindow(hwnd, SW_SHOWNORMAL);
      }
      InvalidateRect(hwnd, nullptr, FALSE);
      return 0;

    case WM_TIMER:
      if (wparam == kTimerShowWindowFallback) {
        KillTimer(hwnd, kTimerShowWindowFallback);
        if (!IsWindowVisible(hwnd)) {
          ShowWindow(hwnd, SW_SHOWNORMAL);
        }
      }
      return 0;

    case kMessageSemanticsUpdated:
      if (state != nullptr && state->a11y.has_value()) {
        state->a11y->RaisePendingEvents();
      }
      return 0;

    case WM_GETOBJECT: {
      // The only notice Windows ever gives that something is reading the
      // screen. There is a system parameter for it and Narrator does not set
      // it, so the question itself is the answer -- upstream's
      // `FlutterWindow::OnGetObject` says the same at greater length.
      if (state == nullptr || !state->a11y.has_value()) {
        break;
      }
      bool reading = false;
      LRESULT provider = state->a11y->GetObject(wparam, lparam, &reading);
      if (reading && state->shell != nullptr) {
        state->platform_task_runner->PostTask(
            [view = state->shell->GetPlatformView()]() {
              if (view) {
                view->SetSemanticsEnabled(true);
              }
            });
      }
      if (provider != 0) {
        return provider;
      }
      break;
    }

    case WM_SETTINGCHANGE:
    case WM_THEMECHANGED:
      // The reader changed something in Settings. Windows does not say what,
      // beyond a section name that is not documented per-setting, so both
      // messages are re-read in full -- which is cheap, and is what makes a
      // dark-mode toggle repaint the application while it is running.
      //
      // Upstream watches the two registry keys from a helper thread instead;
      // see the note on the readers. The window is already being told, so this
      // host listens where it is standing.
      if (state != nullptr && state->platform_view != nullptr) {
        state->platform_view->SendPlatformSettings();
      }
      break;

    case kMessageCursorChanged:
      // The framework chose a different cursor. Applied here as well as in
      // WM_SETCURSOR because the pointer may not be moving: content can change
      // under a stationary cursor, and upstream -- whose platform thread is
      // this thread -- calls SetCursor straight from the channel handler.
      if (state != nullptr) {
        SetCursor(state->shared.cursor.load(std::memory_order_relaxed));
      }
      return 0;

    case WM_SETCURSOR:
      // Only inside the client area. Everywhere else -- the border, the title
      // bar, the resize corners -- is the system's to draw, and returning TRUE
      // there would leave a resize edge showing an I-beam.
      if (LOWORD(lparam) == HTCLIENT) {
        if (state != nullptr) {
          SetCursor(state->shared.cursor.load(std::memory_order_relaxed));
        }
        // Upstream returns TRUE here for the same reason: DefWindowProc would
        // otherwise put the window class's cursor back, undoing the choice on
        // the next mouse move.
        return TRUE;
      }
      break;

    case kMessageQuitApproved:
      // The framework said the application may close, or asked it to. The flag
      // is already set, so this WM_CLOSE goes through rather than being asked
      // about a second time.
      DestroyWindow(hwnd);
      return 0;

    case WM_CLOSE:
      // Upstream's `WindowsLifecycleManager::HandleCloseMessage`. The close is
      // only offered to the framework when the framework is listening and has
      // not already approved it; otherwise it falls through to DefWindowProc,
      // which destroys the window.
      if (state != nullptr && state->platform_view != nullptr &&
          state->shared.exit_processing.load(std::memory_order_relaxed) &&
          !state->shared.quit_approved.load(std::memory_order_relaxed)) {
        state->platform_view->RequestAppExit(/*cancelable=*/true,
                                             /*exit_code=*/0);
        // Swallowed. If the framework agrees, kMessageQuitApproved arrives.
        return 0;
      }
      break;
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
          state->raster_task_runner->PostTask(
              [view = state->platform_view]() { view->OnWindowResized(); });
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
      if (state->a11y.has_value()) {
        // A reader's rectangles are in physical pixels, so the scale between
        // them and the framework's has to follow the display the window is on.
        state->a11y->SetDevicePixelRatio(state->device_pixel_ratio);
      }
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
          if (state->pressed) {
            state->platform_view->SendPointer(
                MakePointerData(state, PointerData::Change::kMove, x, y));
          } else {
            // A move with nothing pressed is a hover. It is a packet per mouse
            // pixel, which is what every Flutter embedder sends and what the
            // framework's hover tracking needs -- there is no other way to
            // know that the pointer has entered something. Asking Windows to
            // tell us when it leaves is part of the same bargain: without
            // WM_MOUSELEAVE the last region stays highlighted after the mouse
            // has gone somewhere else entirely.
            if (!state->tracking_mouse_leave) {
              TRACKMOUSEEVENT track = {};
              track.cbSize = sizeof(track);
              track.dwFlags = TME_LEAVE;
              track.hwndTrack = hwnd;
              TrackMouseEvent(&track);
              state->tracking_mouse_leave = true;
            }
            state->last_x = x;
            state->last_y = y;
            state->platform_view->SendPointer(
                MakePointerData(state, PointerData::Change::kHover, x, y));
          }
          break;
        }
        case WM_MOUSELEAVE: {
          state->tracking_mouse_leave = false;
          if (state->pressed) {
            state->platform_view->SendPointer(
                MakePointerData(state, PointerData::Change::kCancel, x, y));
            state->pressed = false;
          } else {
            // Nothing is under the pointer any more. Remove is what the
            // framework reads as "this pointer is gone", and is what upstream
            // sends when a mouse leaves a view.
            state->platform_view->SendPointer(
                MakePointerData(state, PointerData::Change::kRemove, x, y));
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
      state->platform_view->SendPointer(
          MakeScrollData(state, static_cast<double>(point.x),
                         static_cast<double>(point.y), notches));
      return 0;
    }
    case WM_CAPTURECHANGED:
      // Something else took the mouse. A press that ends this way is cancelled
      // rather than completed.
      if (state != nullptr && state->pressed &&
          state->platform_view != nullptr) {
        state->platform_view->SendPointer(MakePointerData(
            state, PointerData::Change::kCancel, state->last_x, state->last_y));
        state->pressed = false;
      }
      return 0;
    case kMessageKeyResult:
      // The framework has said whether it wanted a key. See HandleKeyResult.
      if (state != nullptr) {
        HandleKeyResult(state, hwnd, static_cast<uint64_t>(wparam),
                        lparam != 0);
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
      // Asked about, then acted on. Win32 wants a synchronous answer to "did
      // you handle this?" and the framework's answer is three threads away, so
      // upstream's redispatch algorithm is what bridges the two: the message is
      // taken now, offered to the framework, and -- if the framework declines
      // -- put back into this window's queue afterwards. The second time round
      // it is recognised and falls through to DefWindowProc, which is what
      // makes Alt+F4 and the rest still work.
      //
      // The cost is that a key the framework declines reaches the system one
      // round trip late. Upstream pays the same cost for the same reason, and
      // it is what a consumable key costs: there is no way to ask first
      // without waiting for the answer.
      if (state != nullptr) {
        if (IsRedispatched(state, msg, wparam)) {
          break;  // Already offered once. The system's turn.
        }
        if (HandleKeyMessage(state, msg, wparam, lparam)) {
          return 0;
        }
      }
      break;
    // -- The IME
    // ---------------------------------------------------------------
    //
    // Upstream's FlutterWindow does exactly this, and the three return values
    // are the load-bearing part: DefWindowProc would otherwise draw the system
    // composition window over the one the framework draws, and would re-emit
    // the committed string as WM_CHAR after the model has already taken it.
    case WM_IME_SETCONTEXT:
      if (state != nullptr && state->ime.has_value() && wparam != 0) {
        state->ime->CreateImeWindow();
        UpdateImePosition(state);
      }
      // Clearing this bit is what hides the system's composition window. The
      // framework draws the composing text itself, underlined, in the field.
      return DefWindowProc(
          hwnd, msg, wparam,
          lparam & ~static_cast<LPARAM>(ISC_SHOWUICOMPOSITIONWINDOW));

    case WM_IME_STARTCOMPOSITION:
      if (state != nullptr && state->ime.has_value()) {
        state->ime->CreateImeWindow();
        UpdateImePosition(state);
        state->text_input.OnComposeBegin();
      }
      // Suppressed so the default composition style is not used.
      return TRUE;

    case WM_IME_COMPOSITION:
      if (state != nullptr && state->ime.has_value()) {
        UpdateImePosition(state);
        if (state->text_input.TakeAbortComposing()) {
          state->ime->AbortComposing();
          return TRUE;
        }
        if (lparam == 0) {
          state->text_input.OnComposeChange(std::u16string(), 0);
          state->text_input.OnComposeCommit();
        }
        // The committed string first: Google Japanese Input and ATOK send both
        // flags at once, committing what was composed and starting the next
        // composition in the same message.
        if (lparam & GCS_RESULTSTR) {
          if (auto text = state->ime->ResultString()) {
            state->text_input.OnComposeChange(
                *text, ComposingCursor(*state->ime, lparam, text->length()));
            state->text_input.OnComposeCommit();
          }
        }
        if (lparam & GCS_COMPSTR) {
          if (auto text = state->ime->ComposingString()) {
            state->text_input.OnComposeChange(
                *text, ComposingCursor(*state->ime, lparam, text->length()));
          }
        }
        if (lparam & (GCS_RESULTSTR | GCS_COMPSTR)) {
          // Suppressed, or DefWindowProc emits the committed string again as
          // WM_CHAR and the model takes it twice.
          return TRUE;
        }
      }
      break;

    case WM_IME_ENDCOMPOSITION:
      if (state != nullptr && state->ime.has_value()) {
        state->ime->DestroyImeWindow();
        state->text_input.OnComposeEnd();
      }
      return TRUE;

    case WM_DESTROY:
      if (state != nullptr && state->a11y.has_value()) {
        // Before the window goes: a client holding a fragment of a destroyed
        // window is what the documented teardown call prevents.
        state->a11y->Shutdown();
      }
      // The last thing the framework hears. It is sent rather than skipped
      // because an application may have something to write down before it
      // goes -- upstream's `detached` is where state restoration saves.
      SendLifecycle(state, "AppLifecycleState.detached");
      // The code `System.exitApplication` asked for, which is zero for every
      // other way of getting here.
      PostQuitMessage(state == nullptr
                          ? 0
                          : static_cast<int>(state->shared.exit_code.load()));
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
  const bool software_forced = force_software != nullptr &&
                               force_software[0] != '\0' &&
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

  // A single-threaded apartment on the thread that owns the window, which is
  // what a UI Automation provider asking for `ProviderOptions_UseComThreading`
  // is asking to be called on. Upstream's Win32 runner does this in `main` for
  // the same reason. Failing is not fatal: it means COM was already
  // initialised, and only a conflicting apartment model would matter.
  const HRESULT com = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
  const bool owns_com = SUCCEEDED(com);

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
  HWND window = CreateWindowEx(
      0, kWindowClass, title.c_str(), WS_OVERLAPPEDWINDOW, CW_USEDEFAULT,
      CW_USEDEFAULT, rect.right - rect.left, rect.bottom - rect.top, nullptr,
      nullptr, instance, &state);
  if (window == nullptr) {
    return -3;
  }

  state.window = window;

  // The IME needs the window handle, so it cannot exist before this point.
  state.ime.emplace(window);

  // Nor can the accessibility bridge: a UI Automation fragment root *is* a
  // window as far as the desktop's tree is concerned.
  state.a11y.emplace(window);

  // What WM_SETCURSOR applies until the framework says otherwise. The window
  // class carries the same cursor, but the class's copy stops being consulted
  // the moment WM_SETCURSOR starts returning TRUE.
  state.shared.cursor.store(LoadCursor(nullptr, IDC_ARROW));

  // Only now is there a window to ask which display it landed on. Redo the
  // sizing at that display's DPI; at 100% this is the same rectangle and the
  // SetWindowPos is a no-op.
  state.device_pixel_ratio = DpiApi::Get().ScaleForWindow(window);
  state.a11y->SetDevicePixelRatio(state.device_pixel_ratio);
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
      [window, &state, impeller = settings.enable_impeller](Shell& shell) {
        auto view = std::make_unique<HostPlatformView>(
            shell, shell.GetTaskRunners(), window, &state.frame_buffer,
            &state.text_input, &state.shared, &state.a11y.value(), impeller);
        // The window proc needs to reach the view to send pointers. The shell
        // owns it and outlives the message loop, so a raw pointer is enough.
        state.platform_view = view.get();
        // How an editing state gets back to the framework. The view outlives
        // the handler's use of this: both die with the message loop, and the
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
    DestroyWindow(window);
    return -4;
  }
  state.shell = shell.get();
  state.platform_task_runner = task_runners.GetPlatformTaskRunner();

  // How a reader's action reaches the framework. UI Automation calls in on
  // whichever thread it likes, and `DispatchSemanticsAction` belongs to the
  // platform thread, so this is where the hop happens -- upstream's
  // `AccessibilityBridgeWindows::DispatchAccessibilityAction` reaches the same
  // engine method from a thread that is already the right one.
  state.a11y->SetActionDispatcher(
      [runner = task_runners.GetPlatformTaskRunner(), shell = shell.get()](
          int32_t node_id, SemanticsAction action) {
        runner->PostTask([shell, node_id, action]() {
          if (auto view = shell->GetPlatformView()) {
            view->DispatchSemanticsAction(kFlutterImplicitViewId, node_id,
                                          action, {});
          }
        });
      });
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
        // Before the first frame, so that an application choosing between the
        // light and the dark theme in its first `build` chooses correctly
        // rather than showing one frame of the wrong one. Upstream sends both
        // from the engine's `Run` for the same reason.
        state.platform_view->SendPlatformSettings();
      }));

  // The window is left hidden until the first frame has been presented:
  // showing it now would put a blank window on screen for as long as the
  // engine takes to start and the first frame to rasterise, because WM_PAINT
  // has nothing to draw before then. The frame-presented message does the
  // showing (see the window proc); this timer is the fallback for a first
  // frame that never arrives, so a broken engine does not leave the process
  // running invisibly.
  SetTimer(window, kTimerShowWindowFallback, 5000, nullptr);

  // Every lifecycle report is made from this thread, including this first one.
  // The window proc makes the other four, and `lifecycle_state` is a plain
  // string with no lock on it -- reporting the initial state from the task
  // posted above instead would have the platform thread writing it while the
  // window thread reads it.
  //
  // Ordering still holds: the report is posted to the platform task runner,
  // which runs it after the RunEngine task queued before it. With the window
  // still hidden no WM_ACTIVATE has happened yet, so this first report is the
  // one that reaches the framework; the activation that comes with the
  // deferred ShowWindow repeats the same state and is deduplicated.
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

  if (owns_com) {
    CoUninitialize();
  }

  // What PostQuitMessage was given, which is zero for every way of closing the
  // window except a `System.exitApplication` that asked for something else.
  return static_cast<int32_t>(message.wParam);
}
