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
#include <windowsx.h>

#include <cstdlib>
#include <memory>
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
#include "flutter/lib/ui/window/pointer_data.h"
#include "flutter/lib/ui/window/pointer_data_packet.h"
#include "flutter/lib/ui/window/viewport_metrics.h"
#include "flutter/shell/common/platform_view.h"
#include "flutter/shell/common/rasterizer.h"
#include "flutter/shell/common/run_configuration.h"
#include "flutter/shell/common/shell.h"
#include "flutter/shell/common/thread_host.h"
#include "flutter/shell/common/vsync_waiter_fallback.h"
#include "flutter/rust/ffi/rustflutter_ffi.h"
#include "flutter/rust/host/rustflutter_gl_win.h"
#include "flutter/shell/gpu/gpu_surface_gl_impeller.h"
#include "flutter/shell/gpu/gpu_surface_software.h"
#include "flutter/shell/gpu/gpu_surface_software_delegate.h"
#include "third_party/skia/include/core/SkSurface.h"

namespace flutter {
namespace {

constexpr wchar_t kWindowClass[] = L"RustflutterHostWindow";

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
    // A timer at sixty hertz. It does not know when the display actually
    // refreshes, so it drifts against it -- which is harmless now that the swap
    // no longer waits for the display as well. See the comment on
    // eglSwapInterval in rustflutter_gl_win.cc: it was the two waits stacking,
    // not the timer, that halved the frame rate.
    //
    // Reading the composition clock instead would be the better clock, and a
    // DWM-backed waiter was written and then deleted: with composition off --
    // which is how a Remote Desktop session arrives -- DwmFlush returns
    // immediately and the loop has to be paced by hand anyway, and the pacing
    // was worse than this. It is worth doing when there is a machine to test
    // the composition path on.
    return std::make_unique<VsyncWaiterFallback>(task_runners_);
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
};

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

/// How far one notch of the wheel scrolls, in logical pixels.
///
/// Flutter's own Windows embedder uses twenty and ignores the system's
/// lines-per-notch setting; matching it means a wheel turn here moves a list by
/// the same amount it would in a Flutter app on the same machine.
constexpr double kScrollPixelsPerNotch = 20.0;

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
  data.scroll_delta_y = -notches * kScrollPixelsPerNotch;
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
      if (wparam == VK_ESCAPE) {
        PostMessage(hwnd, WM_CLOSE, 0, 0);
      }
      return 0;
    case WM_DESTROY:
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

  WindowState state;

  // -- Window (this thread) ---------------------------------------------------

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
        SendViewportMetrics(&state, width, height);
      }));

  ShowWindow(window, SW_SHOWNORMAL);
  UpdateWindow(window);

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
