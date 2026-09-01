// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_HOST_RUSTFLUTTER_VK_H_
#define FLUTTER_RUST_HOST_RUSTFLUTTER_VK_H_

// Impeller on the window, through Vulkan itself.
//
// The GL path next door (rustflutter_gl.h) renders through EGL -- on Windows
// that is ANGLE translating OpenGL ES onto D3D11, on Linux it is Mesa. This
// one talks to the machine's Vulkan driver directly and skips the translation.
// Unlike the GL path it cannot be window-system-agnostic: there is no neutral
// way to ask Vulkan for a window, so SetWindow names HWND on Windows and an
// X11 display and window id on Linux, where its sibling names
// EGLNativeWindowType for both.
//
// One context rather than the GL path's two, because Vulkan has no "current
// context" to begin with. Command buffers are submitted to queues from
// whichever thread holds them, so the IO thread's texture uploads need nothing
// made current -- only the context itself.

#include <cstdint>
#include <memory>

#if defined(_WIN32)
#include <windows.h>
#endif

#include "flutter/fml/memory/ref_ptr.h"
#include "flutter/fml/native_library.h"
#include "flutter/impeller/geometry/size.h"
#include "flutter/impeller/renderer/backend/vulkan/context_vk.h"
#include "flutter/impeller/renderer/backend/vulkan/surface_context_vk.h"
#include "flutter/impeller/renderer/context.h"

namespace flutter {

//------------------------------------------------------------------------------
/// The Vulkan instance, device and the surface context built on them.
///
/// Created once, on the raster thread, the first time a surface is asked for.
/// Where the GL sibling needs a 1x1 pixel buffer to compile shaders against, a
/// Vulkan context needs no surface at all: shader modules are built from SPIR-V
/// blobs compiled into the binary, so by the time a window arrives the context
/// is already complete.
class ImpellerVkContext {
 public:
  /// Returns nullptr if the Vulkan loader, a usable device or Impeller could
  /// not be had. Every failure logs its own reason; the caller falls back to
  /// GL and then to software.
  static std::unique_ptr<ImpellerVkContext> Create();

  ~ImpellerVkContext();

  /// The parent context, for the shell to publish to the IO thread and for
  /// texture uploads to submit against.
  std::shared_ptr<impeller::Context> GetImpellerContext() const;

  /// What GPUSurfaceVulkanImpeller draws through. Kept separate from the
  /// parent because the swapchain hangs off this one; see the class comment on
  /// SurfaceContextVK.
  const std::shared_ptr<impeller::SurfaceContextVK>& GetSurfaceContext() const {
    return surface_context_;
  }

#if defined(_WIN32)
  /// Creates the Win32 surface for the window and builds the swapchain on it.
  /// Called once, before the first frame; a later resize only needs
  /// UpdateSize.
  bool SetWindow(HWND window, impeller::ISize size);
#else
  /// Creates the Xlib surface for the window and builds the swapchain on it.
  /// `xdisplay` is the X11 Display* the window lives on, untyped so this
  /// header does not drag Xlib -- and its macros -- into every includer.
  /// Called once, before the first frame; a later resize only needs
  /// UpdateSize.
  bool SetWindow(void* xdisplay, uint64_t window, impeller::ISize size);
#endif

  /// Marks the swapchain stale at the new size; the next frame rebuilds it.
  /// The swapchain owns the images sized to the window, and unlike an EGL
  /// surface it can rebuild itself rather than being thrown away.
  void UpdateSize(impeller::ISize size);

 private:
  ImpellerVkContext() = default;

  // Held for the life of the context: vkGetInstanceProcAddr was resolved out
  // of this library, and unloading it under a live instance is not something
  // the loader promises to survive.
  fml::RefPtr<fml::NativeLibrary> vulkan_library_;
  // Declared before surface_context_ so the swapchain's owner dies first.
  std::shared_ptr<impeller::ContextVK> context_;
  std::shared_ptr<impeller::SurfaceContextVK> surface_context_;

  ImpellerVkContext(const ImpellerVkContext&) = delete;
  ImpellerVkContext& operator=(const ImpellerVkContext&) = delete;
};

}  // namespace flutter

#endif  // FLUTTER_RUST_HOST_RUSTFLUTTER_VK_H_
