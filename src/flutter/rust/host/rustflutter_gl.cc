// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "flutter/rust/host/rustflutter_gl.h"

#include <algorithm>
#include <chrono>
#include <cstdlib>
#include <map>
#include <string>
#include <thread>
#include <vector>

#include <GLES2/gl2.h>

#include "flutter/fml/logging.h"
#include "flutter/impeller/base/thread.h"
#include "flutter/impeller/renderer/backend/gles/context_gles.h"
#include "flutter/impeller/renderer/backend/gles/proc_table_gles.h"
#include "flutter/impeller/renderer/backend/gles/reactor_gles.h"
#include "flutter/impeller/toolkit/egl/egl.h"
#include "impeller/entity/gles/entity_shaders_gles.h"
#include "impeller/entity/gles/framebuffer_blend_shaders_gles.h"
#include "impeller/entity/gles3/entity_shaders_gles.h"
#include "impeller/entity/gles3/framebuffer_blend_shaders_gles.h"
#include "third_party/skia/include/core/SkData.h"
#include "third_party/skia/include/core/SkImage.h"
#include "third_party/skia/include/core/SkStream.h"
#include "third_party/skia/include/core/SkSurface.h"
#include "third_party/skia/include/encode/SkPngEncoder.h"

namespace flutter {

//------------------------------------------------------------------------------
/// Tells Impeller's reactor which threads may run GL commands right now.
///
/// The reactor batches up GL work and flushes it whenever a thread that holds a
/// current context asks. It cannot know which those are, so the context tells
/// it: made current means allowed, cleared means not.
class ImpellerGlContext::ReactorWorker final
    : public impeller::ReactorGLES::Worker {
 public:
  ReactorWorker() = default;

  ~ReactorWorker() override = default;

  // |impeller::ReactorGLES::Worker|
  bool CanReactorReactOnCurrentThreadNow(
      const impeller::ReactorGLES& reactor) const override {
    impeller::ReaderLock lock(mutex_);
    auto found = allowed_.find(std::this_thread::get_id());
    return found != allowed_.end() && found->second;
  }

  void SetAllowedOnCurrentThread(bool allowed) {
    impeller::WriterLock lock(mutex_);
    allowed_[std::this_thread::get_id()] = allowed;
  }

 private:
  mutable impeller::RWMutex mutex_;
  std::map<std::thread::id, bool> allowed_ IPLR_GUARDED_BY(mutex_);
};

namespace {

std::shared_ptr<impeller::Context> CreateImpellerContext(
    const std::shared_ptr<impeller::ReactorGLES::Worker>& worker) {
  auto proc_table = std::make_unique<impeller::ProcTableGLES>(
      impeller::egl::CreateProcAddressResolver());
  if (!proc_table->IsValid()) {
    FML_LOG(ERROR) << "Could not resolve the OpenGL ES proc table.";
    return nullptr;
  }

  // GLES3 has the shaders Impeller would rather use; GLES2 is the floor. ANGLE
  // on D3D11 gives 3 on anything modern, so this normally takes the first
  // branch, but the fallback costs one comparison.
  const bool is_gles3 = proc_table->GetDescription()->GetGlVersion().IsAtLeast(
      impeller::Version{3, 0, 0});

  std::vector<std::shared_ptr<fml::Mapping>> shaders;
  if (is_gles3) {
    shaders = {
        std::make_shared<fml::NonOwnedMapping>(
            impeller_entity_shaders_gles3_data,
            impeller_entity_shaders_gles3_length),
        std::make_shared<fml::NonOwnedMapping>(
            impeller_framebuffer_blend_shaders_gles3_data,
            impeller_framebuffer_blend_shaders_gles3_length),
    };
  } else {
    shaders = {
        std::make_shared<fml::NonOwnedMapping>(
            impeller_entity_shaders_gles_data,
            impeller_entity_shaders_gles_length),
        std::make_shared<fml::NonOwnedMapping>(
            impeller_framebuffer_blend_shaders_gles_data,
            impeller_framebuffer_blend_shaders_gles_length),
    };
  }

  auto context = impeller::ContextGLES::Create(impeller::Flags{},
                                               std::move(proc_table), shaders,
                                               /*enable_gpu_tracing=*/false);
  if (!context) {
    FML_LOG(ERROR) << "Could not create the Impeller OpenGL ES context.";
    return nullptr;
  }
  if (!context->AddReactorWorker(worker).has_value()) {
    FML_LOG(ERROR) << "Could not register the Impeller reactor worker.";
    return nullptr;
  }
  return context;
}

}  // namespace

std::unique_ptr<ImpellerGlContext> ImpellerGlContext::Create() {
  auto self = std::unique_ptr<ImpellerGlContext>(new ImpellerGlContext());

  self->display_ = std::make_unique<impeller::egl::Display>();
  if (!self->display_->IsValid()) {
    FML_LOG(ERROR) << "Could not open an EGL display. ANGLE is linked "
                      "statically, so this normally means no D3D11 device.";
    return nullptr;
  }

  // One sample and eight bits of depth, which is upstream's Windows config --
  // `shell/platform/windows/egl/manager.cc:188`: RGBA8888, EGL_DEPTH_SIZE 8,
  // EGL_STENCIL_SIZE 8, and no EGL_SAMPLES at all.
  //
  // Impeller renders the root pass straight into this surface, and it asks
  // for it at one sample: `SurfaceGLES::WrapFBO` sets
  // `color0_tex.sample_count = SampleCount::kCount1`. What antialiases that
  // pass is Impeller's own coverage, computed per fragment from the geometry;
  // the subpasses that do want multisampling -- save layers, advanced blends
  // -- make an offscreen 4x target of their own and resolve it. Asking EGL
  // for a multisampled *window* on top of that buys a second, hardware round
  // of it that Impeller neither asked for nor knows about.
  //
  // Which would be a defensible thing to buy if it were cheap, and it is not.
  // A 4x window surface makes ANGLE hold a 4x colour buffer and a 4x
  // depth-stencil buffer at window size and resolve both at present: 28 bytes
  // per window pixel above this config, or 220 MB at 3840x2054. That is more
  // video memory than a 544-photograph album holds in thumbnails, map tiles
  // and full-size decodes put together, spent on the one part of the frame
  // Impeller had already antialiased.
  //
  // Twenty-four bits of depth was the same in miniature. Nothing here draws
  // with depth; the eight upstream asks for are what the stencil is packed
  // beside.
  impeller::egl::ConfigDescriptor desc;
  desc.api = impeller::egl::API::kOpenGLES3;
  desc.color_format = impeller::egl::ColorFormat::kRGBA8888;
  desc.depth_bits = impeller::egl::DepthBits::kEight;
  desc.stencil_bits = impeller::egl::StencilBits::kEight;
  desc.samples = impeller::egl::Samples::kOne;
  desc.surface_type = impeller::egl::SurfaceType::kWindow;

  self->onscreen_config_ = self->display_->ChooseConfig(desc);
  if (!self->onscreen_config_) {
    desc.api = impeller::egl::API::kOpenGLES2;
    self->onscreen_config_ = self->display_->ChooseConfig(desc);
  }
  if (!self->onscreen_config_) {
    FML_LOG(ERROR) << "Could not choose an onscreen EGL config.";
    return nullptr;
  }

  desc.surface_type = impeller::egl::SurfaceType::kPBuffer;
  self->offscreen_config_ = self->display_->ChooseConfig(desc);
  if (!self->offscreen_config_) {
    FML_LOG(ERROR) << "Could not choose an offscreen EGL config.";
    return nullptr;
  }

  self->onscreen_context_ =
      self->display_->CreateContext(*self->onscreen_config_, nullptr);
  if (!self->onscreen_context_) {
    FML_LOG(ERROR) << "Could not create the onscreen GL context.";
    return nullptr;
  }
  self->offscreen_context_ = self->display_->CreateContext(
      *self->offscreen_config_, self->onscreen_context_.get());
  if (!self->offscreen_context_) {
    FML_LOG(ERROR) << "Could not create the offscreen GL context.";
    return nullptr;
  }

  self->reactor_worker_ = std::make_shared<ReactorWorker>();

  // Both contexts report their currency to the reactor, so work queued on
  // either thread is flushed by whichever one is actually holding GL.
  impeller::egl::Context::LifecycleListener listener =
      [worker = self->reactor_worker_](
          impeller::egl::Context::LifecycleEvent event) {
        worker->SetAllowedOnCurrentThread(
            event == impeller::egl::Context::LifecycleEvent::kDidMakeCurrent);
      };
  if (!self->onscreen_context_->AddLifecycleListener(listener).has_value() ||
      !self->offscreen_context_->AddLifecycleListener(listener).has_value()) {
    FML_LOG(ERROR) << "Could not add GL context lifecycle listeners.";
    return nullptr;
  }

  // Building the Impeller context compiles shaders, which needs a current
  // context, which needs a surface. A 1x1 pixel buffer is the cheapest one.
  self->offscreen_surface_ = self->display_->CreatePixelBufferSurface(
      *self->offscreen_config_, 1u, 1u);
  if (!self->offscreen_surface_ ||
      !self->offscreen_context_->MakeCurrent(*self->offscreen_surface_)) {
    FML_LOG(ERROR) << "Could not make the offscreen context current.";
    return nullptr;
  }

  self->impeller_context_ = CreateImpellerContext(self->reactor_worker_);
  const bool cleared = self->offscreen_context_->ClearCurrent();
  if (!self->impeller_context_) {
    return nullptr;
  }
  if (!cleared) {
    FML_LOG(ERROR) << "Could not clear the offscreen context.";
    return nullptr;
  }

  // Which EGL answered is worth saying: on Windows it is ANGLE on top of D3D11,
  // and on Android it is the device's own driver, so the same message from the
  // same code means two different stacks underneath.
#if defined(__ANDROID__)
  FML_LOG(IMPORTANT) << "Rendering with Impeller (OpenGL ES).";
#else
  FML_LOG(IMPORTANT) << "Rendering with Impeller (OpenGL ES via ANGLE).";
#endif
  return self;
}

ImpellerGlContext::~ImpellerGlContext() = default;

std::unique_ptr<impeller::egl::Surface> ImpellerGlContext::CreateWindowSurface(
    EGLNativeWindowType window) {
  if (window == 0 || !onscreen_config_) {
    return nullptr;
  }
  return display_->CreateWindowSurface(*onscreen_config_, window);
}

bool ImpellerGlContext::MakeCurrent(
    const impeller::egl::Surface& surface) const {
  if (!onscreen_context_ || !onscreen_context_->MakeCurrent(surface)) {
    return false;
  }
#if !defined(__ANDROID__)
  // Do not block in the swap. Something has to wait for the display, but only
  // one thing should: the vsync waiter already does, and a swap that waited as
  // well would stack a second wait onto every frame -- which is a scene that
  // takes two milliseconds to draw arriving every thirty. Windows composites
  // through the DWM, so nothing tears for the want of it.
  //
  // Set on every make-current rather than once, because the interval belongs to
  // the surface and a resize replaces it.
  //
  // Android is left at the default, as upstream's Android surface is: there the
  // swap is how the raster thread learns that the compositor has taken the
  // previous buffer, and racing ahead of it only fills the queue.
  static bool warned = false;
  if (eglSwapInterval(display_->GetHandle(), 0) != EGL_TRUE && !warned) {
    warned = true;
    FML_LOG(ERROR) << "Could not turn the swap interval off; frames will wait "
                      "for the display twice.";
  }
#endif
  return true;
}

bool ImpellerGlContext::ClearCurrent() const {
  return onscreen_context_ && onscreen_context_->ClearCurrent();
}

bool ImpellerGlContext::MakeResourceCurrent() const {
  // The 1x1 pixel buffer built during Create(). A GL context needs some surface
  // to be current against; nothing is ever drawn to this one, and uploads do
  // not care what it is.
  return offscreen_context_ && offscreen_surface_ &&
         offscreen_context_->MakeCurrent(*offscreen_surface_);
}

// -- Delegate -----------------------------------------------------------------

namespace {

/// What GLContextMakeCurrent returns. Nothing needs undoing on the way out --
/// the raster thread keeps the context current between frames -- so this only
/// reports whether making it current worked.
class MadeCurrent final : public GLContextResult {
 public:
  explicit MadeCurrent(bool success) : GLContextResult(success) {}
  ~MadeCurrent() override = default;
};

//------------------------------------------------------------------------------
/// Reports how far apart the presented frames are, every sixty of them.
///
/// Set RUSTFLUTTER_FRAME_STATS to any value. It reports the median rather than
/// the mean because one long frame -- the first paint of a screen, a shader
/// being compiled -- would otherwise be indistinguishable from a build that is
/// uniformly slow, and those want different answers.
struct FrameSample {
  double interval_ms;  // present to present
  double idle_ms;      // last swap returning to this frame's work starting
  double raster_ms;    // the context going current to the swap being asked for
  double swap_ms;      // inside eglSwapBuffers
};

bool FrameStatsEnabled() {
  static const bool enabled = std::getenv("RUSTFLUTTER_FRAME_STATS") != nullptr;
  return enabled;
}

std::vector<FrameSample>& FrameSamples() {
  static std::vector<FrameSample> samples;
  return samples;
}

std::chrono::steady_clock::time_point& LastSwapEnd() {
  static auto value = std::chrono::steady_clock::now();
  return value;
}

/// When the rasterizer took the context, which is where a frame's work starts.
///
/// Everything before it on this thread is waiting: the layer tree for the next
/// frame has not arrived. Keeping the two apart is the whole point -- a raster
/// thread that is idle three quarters of the time and one that is saturated
/// both present every sixteen milliseconds, and only one of them has headroom.
std::chrono::steady_clock::time_point& RasterStart() {
  static auto value = std::chrono::steady_clock::now();
  return value;
}

/// Splits a frame into the part that is work and the part that is waiting.
///
/// The two want different answers. Time spent inside the swap is time spent
/// blocked on the vblank, which is what a frame that is comfortably ahead looks
/// like; time spent before it is rasterising, which is what a frame that is
/// behind looks like. A single frame-interval number cannot tell them apart --
/// both make it sixteen milliseconds or more.
void ReportFrameStats(double idle_ms, double raster_ms, double swap_ms) {
  if (!FrameStatsEnabled()) {
    return;
  }
  std::vector<FrameSample>& samples = FrameSamples();
  const auto now = std::chrono::steady_clock::now();
  static auto previous_present = now;
  const double interval_ms =
      std::chrono::duration<double, std::milli>(now - previous_present).count();
  previous_present = now;

  // The first interval is measured from process start rather than from a
  // frame, so it says nothing about the frame rate.
  if (samples.empty() && interval_ms > 1000.0) {
    return;
  }
  samples.push_back({interval_ms, idle_ms, raster_ms, swap_ms});
  if (samples.size() < 60) {
    return;
  }

  auto median = [&samples](double FrameSample::* field) {
    std::vector<double> values;
    values.reserve(samples.size());
    for (const FrameSample& sample : samples) {
      values.push_back(sample.*field);
    }
    std::sort(values.begin(), values.end());
    return values[values.size() / 2];
  };

  // The worst frame in the window, alongside the typical one. A median hides
  // exactly the events worth knowing about here: an image's pixels being
  // uploaded to a texture the first time it is drawn happens on this thread,
  // once per image, and shows up as one slow frame rather than as a shifted
  // median.
  auto worst = [&samples](double FrameSample::* field) {
    double top = 0.0;
    for (const FrameSample& sample : samples) {
      top = std::max(top, sample.*field);
    }
    return top;
  };

  const double interval = median(&FrameSample::interval_ms);
  FML_LOG(IMPORTANT) << "raster thread: idle " << median(&FrameSample::idle_ms)
                     << " ms, rasterise " << median(&FrameSample::raster_ms)
                     << " ms (worst " << worst(&FrameSample::raster_ms)
                     << " ms), swap " << median(&FrameSample::swap_ms)
                     << " ms, frame " << interval << " ms ("
                     << (1000.0 / interval) << " fps), median of "
                     << samples.size() << ".";
  samples.clear();
}

//------------------------------------------------------------------------------
/// Writes the frame that is about to be presented to a PNG, once.
///
/// A GPU-composited window cannot be captured with PrintWindow -- that reads
/// the GDI layer, which is empty here -- so proving that Impeller drew anything
/// means reading it back from the framebuffer. Set RUSTFLUTTER_CAPTURE_FRAME to
/// a path to get one; it is a diagnostic, not a code path an application takes.
void MaybeCaptureFrame() {
  static std::string path = [] {
    const char* value = std::getenv("RUSTFLUTTER_CAPTURE_FRAME");
    return value != nullptr ? std::string(value) : std::string();
  }();
  if (path.empty()) {
    return;
  }
  // Every present overwrites the file, so what is left behind is the last
  // frame the process drew. Waiting for a particular frame number would be
  // wrong either way: frames are on demand, so a static page might only ever
  // produce one, while a page that resizes produces a second at the corrected
  // size and that is the one worth looking at.

  GLint viewport[4] = {0, 0, 0, 0};
  glGetIntegerv(GL_VIEWPORT, viewport);
  const int width = viewport[2];
  const int height = viewport[3];
  if (width <= 0 || height <= 0) {
    return;
  }

  std::vector<uint8_t> pixels(static_cast<size_t>(width) * height * 4u);
  glReadPixels(0, 0, width, height, GL_RGBA, GL_UNSIGNED_BYTE, pixels.data());

  // GL reads bottom-up; flip into an SkPixmap that is top-down.
  std::vector<uint8_t> flipped(pixels.size());
  const size_t stride = static_cast<size_t>(width) * 4u;
  for (int row = 0; row < height; ++row) {
    memcpy(&flipped[row * stride], &pixels[(height - 1 - row) * stride],
           stride);
  }

  SkImageInfo info = SkImageInfo::Make(width, height, kRGBA_8888_SkColorType,
                                       kUnpremul_SkAlphaType);
  SkPixmap pixmap(info, flipped.data(), stride);
  SkFILEWStream stream(path.c_str());
  if (!stream.isValid() ||
      !SkPngEncoder::Encode(&stream, pixmap, SkPngEncoder::Options{})) {
    FML_LOG(ERROR) << "Could not write the captured frame to " << path << ".";
  } else {
    // Logged once; every later frame quietly overwrites.
    static bool announced = false;
    if (!announced) {
      announced = true;
      FML_LOG(IMPORTANT) << "Capturing Impeller frames to " << path << " ("
                         << width << "x" << height << "). The file holds the "
                         << "last frame drawn.";
    }
  }
}

}  // namespace

ImpellerGlDelegate::ImpellerGlDelegate(ImpellerGlContext* context,
                                       EGLNativeWindowType window)
    : context_(context), window_(window) {
  surface_ = context_->CreateWindowSurface(window_);
  if (!surface_) {
    FML_LOG(ERROR) << "Could not create an EGL window surface.";
  }
}

ImpellerGlDelegate::~ImpellerGlDelegate() = default;

bool ImpellerGlDelegate::Resize() {
  // The old surface has to go before the new one is made: ANGLE will not hand
  // out two window surfaces for the same HWND.
  context_->ClearCurrent();
  surface_.reset();
  surface_ = context_->CreateWindowSurface(window_);
  return surface_ != nullptr;
}

std::unique_ptr<GLContextResult> ImpellerGlDelegate::GLContextMakeCurrent() {
  if (!surface_) {
    return std::make_unique<MadeCurrent>(false);
  }
  if (FrameStatsEnabled()) {
    RasterStart() = std::chrono::steady_clock::now();
  }
  return std::make_unique<MadeCurrent>(context_->MakeCurrent(*surface_));
}

bool ImpellerGlDelegate::GLContextClearCurrent() {
  return context_->ClearCurrent();
}

bool ImpellerGlDelegate::GLContextPresent(const GLPresentInfo& present_info) {
  if (surface_ == nullptr) {
    return false;
  }
  // Before the swap: after it, the back buffer's contents are undefined.
  MaybeCaptureFrame();

  bool presented;
  if (!FrameStatsEnabled()) {
    presented = surface_->Present();
  } else {
    const auto asked = std::chrono::steady_clock::now();
    const double idle_ms =
        std::chrono::duration<double, std::milli>(RasterStart() - LastSwapEnd())
            .count();
    const double raster_ms =
        std::chrono::duration<double, std::milli>(asked - RasterStart())
            .count();
    presented = surface_->Present();
    const auto done = std::chrono::steady_clock::now();
    LastSwapEnd() = done;
    ReportFrameStats(
        idle_ms, raster_ms,
        std::chrono::duration<double, std::milli>(done - asked).count());
  }
  if (presented && on_first_present_) {
    auto callback = std::move(on_first_present_);
    on_first_present_ = nullptr;
    callback();
  }
  return presented;
}

GLFBOInfo ImpellerGlDelegate::GLContextFBO(GLFrameInfo frame_info) const {
  // Zero is the window surface's own framebuffer, which is what a window
  // surface renders into.
  return GLFBOInfo{.fbo_id = 0, .existing_damage = std::nullopt};
}

sk_sp<const GrGLInterface> ImpellerGlDelegate::GetGLInterface() const {
  // Only Skia's GL backend needs this, and Impeller is not it.
  return nullptr;
}

}  // namespace flutter
