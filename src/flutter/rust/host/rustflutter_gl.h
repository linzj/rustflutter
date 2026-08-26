// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_HOST_RUSTFLUTTER_GL_H_
#define FLUTTER_RUST_HOST_RUSTFLUTTER_GL_H_

// Impeller on the window, through EGL.
//
// The software surface is portable and needs nothing; this is the production
// path. What it takes is an EGL display, two contexts and a window surface.
// Where the EGL comes from is the only per-platform part: on Windows it is
// ANGLE, translating GLES onto D3D11, and on Android it is the system's own.
// Neither is named below, which is why this file is not per-platform.
//
// Two contexts rather than one, matching every other embedder: the onscreen one
// draws frames on the raster thread, the offscreen one uploads textures on the
// IO thread. They share objects, so an image decoded on IO is drawable on
// raster without a copy.

#include <functional>
#include <memory>

#include "flutter/impeller/renderer/context.h"
#include "flutter/impeller/toolkit/egl/config.h"
#include "flutter/impeller/toolkit/egl/context.h"
#include "flutter/impeller/toolkit/egl/display.h"
#include "flutter/impeller/toolkit/egl/surface.h"
#include "flutter/shell/gpu/gpu_surface_gl_delegate.h"

namespace flutter {

//------------------------------------------------------------------------------
/// The EGL objects and the Impeller context built on them.
///
/// Created once, on the raster thread, the first time a surface is asked for.
/// Making the Impeller context needs a current GL context, which needs some
/// surface, which is why a 1x1 pixel buffer exists here.
class ImpellerGlContext {
 public:
  /// Returns nullptr if EGL, a config, a context or Impeller could not be had.
  /// Every failure logs its own reason; the caller falls back to software.
  static std::unique_ptr<ImpellerGlContext> Create();

  ~ImpellerGlContext();

  const std::shared_ptr<impeller::Context>& GetImpellerContext() const {
    return impeller_context_;
  }

  /// Creates the surface that frames are presented to.
  std::unique_ptr<impeller::egl::Surface> CreateWindowSurface(
      EGLNativeWindowType window);

  bool MakeCurrent(const impeller::egl::Surface& surface) const;
  bool ClearCurrent() const;

  /// Makes the offscreen context current on the calling thread, and leaves it
  /// current.
  ///
  /// Called once on the IO thread, so uploads posted there have somewhere to
  /// run. It also tells the reactor that this thread may issue GL commands --
  /// the lifecycle listener installed in Create() does that -- without which
  /// queued work would sit until the raster thread happened to flush it.
  bool MakeResourceCurrent() const;

 private:
  ImpellerGlContext() = default;

  class ReactorWorker;

  std::unique_ptr<impeller::egl::Display> display_;
  std::unique_ptr<impeller::egl::Config> onscreen_config_;
  std::unique_ptr<impeller::egl::Config> offscreen_config_;
  std::unique_ptr<impeller::egl::Context> onscreen_context_;
  std::unique_ptr<impeller::egl::Context> offscreen_context_;
  std::unique_ptr<impeller::egl::Surface> offscreen_surface_;
  std::shared_ptr<ReactorWorker> reactor_worker_;
  std::shared_ptr<impeller::Context> impeller_context_;

  ImpellerGlContext(const ImpellerGlContext&) = delete;
  ImpellerGlContext& operator=(const ImpellerGlContext&) = delete;
};

//------------------------------------------------------------------------------
/// What GPUSurfaceGLImpeller asks of the window.
///
/// Owns the onscreen surface, because that is the part that has to be thrown
/// away and remade when the window resizes.
class ImpellerGlDelegate final : public GPUSurfaceGLDelegate {
 public:
  ImpellerGlDelegate(ImpellerGlContext* context, EGLNativeWindowType window);

  // The base destructor is non-virtual, so this is not an override.
  ~ImpellerGlDelegate();

  bool IsValid() const { return surface_ != nullptr; }

  /// Drops and remakes the window surface. EGL surfaces do not follow a window
  /// that changed size; presenting to a stale one stretches the frame.
  bool Resize();

  /// Installs a one-shot callback fired after the first successful present.
  /// The host uses it to show the window only once a frame is on it; this
  /// file stays platform-neutral, so the window message itself lives with
  /// the caller.
  void SetFirstPresentCallback(std::function<void()> callback) {
    on_first_present_ = std::move(callback);
  }

  // |GPUSurfaceGLDelegate|
  std::unique_ptr<GLContextResult> GLContextMakeCurrent() override;

  // |GPUSurfaceGLDelegate|
  bool GLContextClearCurrent() override;

  // |GPUSurfaceGLDelegate|
  bool GLContextPresent(const GLPresentInfo& present_info) override;

  // |GPUSurfaceGLDelegate|
  GLFBOInfo GLContextFBO(GLFrameInfo frame_info) const override;

  // |GPUSurfaceGLDelegate|
  sk_sp<const GrGLInterface> GetGLInterface() const override;

 private:
  ImpellerGlContext* context_ = nullptr;
  EGLNativeWindowType window_ = {};
  std::unique_ptr<impeller::egl::Surface> surface_;
  std::function<void()> on_first_present_;

  ImpellerGlDelegate(const ImpellerGlDelegate&) = delete;
  ImpellerGlDelegate& operator=(const ImpellerGlDelegate&) = delete;
};

}  // namespace flutter

#endif  // FLUTTER_RUST_HOST_RUSTFLUTTER_GL_H_
