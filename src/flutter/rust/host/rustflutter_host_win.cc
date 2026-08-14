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
// Rendering goes through GPUSurfaceSoftware. Impeller needs a GL or Vulkan
// context on the window, which is the next step; the point of this file is that
// the frame now travels Animator -> Engine -> RuntimeController -> Rust ->
// LayerTree -> Pipeline -> Rasterizer -> Surface, driven by vsync.

#include "flutter/rust/host/rustflutter_host.h"

#include <windows.h>

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
#include "flutter/lib/ui/window/viewport_metrics.h"
#include "flutter/shell/common/platform_view.h"
#include "flutter/shell/common/rasterizer.h"
#include "flutter/shell/common/run_configuration.h"
#include "flutter/shell/common/shell.h"
#include "flutter/shell/common/thread_host.h"
#include "flutter/shell/common/vsync_waiter_fallback.h"
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
                   FrameBuffer* frame_buffer)
      : PlatformView(delegate, task_runners),
        window_(window),
        frame_buffer_(frame_buffer) {}

  ~HostPlatformView() override = default;

  // |PlatformView|
  std::unique_ptr<Surface> CreateRenderingSurface() override {
    return std::make_unique<GPUSurfaceSoftware>(this,
                                                /*render_to_surface=*/true);
  }

  // |PlatformView|
  std::unique_ptr<VsyncWaiter> CreateVSyncWaiter() override {
    // A timer at the display's refresh rate. The real thing reads the DWM
    // composition clock; this is enough to prove the frame loop is driven by
    // the engine rather than by an application call.
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
  HWND window_ = nullptr;
  FrameBuffer* frame_buffer_ = nullptr;
  sk_sp<SkSurface> backing_store_;

  FML_DISALLOW_COPY_AND_ASSIGN(HostPlatformView);
};

//------------------------------------------------------------------------------
/// What the window proc needs to reach. Owned by rf_host_run's stack frame.
struct WindowState {
  FrameBuffer frame_buffer;
  Shell* shell = nullptr;
  fml::RefPtr<fml::TaskRunner> platform_task_runner;
  double device_pixel_ratio = 1.0;
};

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
        SendViewportMetrics(state, LOWORD(lparam), HIWORD(lparam));
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
  settings.enable_impeller = options->enable_impeller != 0;
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
      [window, frame_buffer = &state.frame_buffer](Shell& shell) {
        return std::make_unique<HostPlatformView>(
            shell, shell.GetTaskRunners(), window, frame_buffer);
      },
      [](Shell& shell) { return std::make_unique<Rasterizer>(shell); });

  if (shell == nullptr || !shell->IsSetup()) {
    DestroyWindow(window);
    return -4;
  }
  state.shell = shell.get();
  state.platform_task_runner = task_runners.GetPlatformTaskRunner();

  // Everything below belongs to the platform thread: RunEngine checks for it,
  // and NotifyCreated / SetViewportMetrics reach the platform view directly.
  // Ordering matters -- the surface has to exist before the first frame is
  // rasterized, and the framework needs a size before it can lay anything out.
  task_runners.GetPlatformTaskRunner()->PostTask(
      fml::MakeCopyable([shell = shell.get(), &state, width = options->width,
                         height = options->height]() mutable {
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
