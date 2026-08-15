// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "flutter/rust/host/rustflutter_gl_win.h"

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
#include "third_party/skia/include/core/SkData.h"
#include "third_party/skia/include/core/SkImage.h"
#include "third_party/skia/include/core/SkStream.h"
#include "third_party/skia/include/core/SkSurface.h"
#include "third_party/skia/include/encode/SkPngEncoder.h"
#include "impeller/entity/gles/entity_shaders_gles.h"
#include "impeller/entity/gles/framebuffer_blend_shaders_gles.h"
#include "impeller/entity/gles3/entity_shaders_gles.h"
#include "impeller/entity/gles3/framebuffer_blend_shaders_gles.h"

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

  auto context = impeller::ContextGLES::Create(
      impeller::Flags{}, std::move(proc_table), shaders,
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

  impeller::egl::ConfigDescriptor desc;
  desc.api = impeller::egl::API::kOpenGLES3;
  desc.color_format = impeller::egl::ColorFormat::kRGBA8888;
  desc.depth_bits = impeller::egl::DepthBits::kTwentyFour;
  desc.stencil_bits = impeller::egl::StencilBits::kEight;
  desc.samples = impeller::egl::Samples::kFour;
  desc.surface_type = impeller::egl::SurfaceType::kWindow;

  self->onscreen_config_ = self->display_->ChooseConfig(desc);
  if (!self->onscreen_config_) {
    desc.api = impeller::egl::API::kOpenGLES2;
    self->onscreen_config_ = self->display_->ChooseConfig(desc);
  }
  if (!self->onscreen_config_) {
    // Software ANGLE and some virtual display drivers have no multisampled
    // window config. One sample is worse looking, not broken.
    desc.samples = impeller::egl::Samples::kOne;
    self->onscreen_config_ = self->display_->ChooseConfig(desc);
    if (self->onscreen_config_) {
      FML_LOG(INFO) << "No multisampled window config; falling back to one "
                       "sample.";
    } else {
      FML_LOG(ERROR) << "Could not choose an onscreen EGL config.";
      return nullptr;
    }
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

  FML_LOG(IMPORTANT) << "Rendering with Impeller (OpenGL ES via ANGLE).";
  return self;
}

ImpellerGlContext::~ImpellerGlContext() = default;

std::unique_ptr<impeller::egl::Surface> ImpellerGlContext::CreateWindowSurface(
    HWND window) {
  if (window == nullptr || !onscreen_config_) {
    return nullptr;
  }
  return display_->CreateWindowSurface(
      *onscreen_config_, reinterpret_cast<EGLNativeWindowType>(window));
}

bool ImpellerGlContext::MakeCurrent(
    const impeller::egl::Surface& surface) const {
  return onscreen_context_ && onscreen_context_->MakeCurrent(surface);
}

bool ImpellerGlContext::ClearCurrent() const {
  return onscreen_context_ && onscreen_context_->ClearCurrent();
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

ImpellerGlDelegate::ImpellerGlDelegate(ImpellerGlContext* context, HWND window)
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
  return surface_->Present();
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
