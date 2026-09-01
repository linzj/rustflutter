// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// On Linux the window surface is made with vkCreateXlibSurfaceKHR, whose
// vulkan.hpp wrapper only exists under this define. It has to be set before
// the first vulkan.hpp include, which happens inside the Impeller headers
// below.
#if defined(__linux__) && !defined(__ANDROID__)
#define VK_USE_PLATFORM_XLIB_KHR 1
#endif

#include "flutter/rust/host/rustflutter_vk.h"

#include <vector>

#include "flutter/fml/logging.h"
#include "flutter/fml/paths.h"
#include "flutter/impeller/renderer/backend/vulkan/context_vk.h"
#include "flutter/impeller/renderer/backend/vulkan/driver_info_vk.h"
#include "flutter/impeller/renderer/backend/vulkan/surface_context_vk.h"
#include "impeller/entity/vk/entity_shaders_vk.h"
#include "impeller/entity/vk/framebuffer_blend_shaders_vk.h"
#include "impeller/entity/vk/modern_shaders_vk.h"

// Xlib, which vulkan_xlib.h drags in, leaks single-word macros over anything
// included after it. Everything above is already in; only the typedefs
// (Display, Window) are needed below, and they survive.
#if defined(VK_USE_PLATFORM_XLIB_KHR)
#undef Bool
#undef Status
#undef True
#undef False
#undef None
#undef Success
#undef Always
#endif

namespace flutter {

namespace {

/// The Vulkan loader's name on this platform. Loaded rather than linked, so
/// the binary starts on a machine with no Vulkan at all.
const char* VulkanLoaderName() {
#if defined(_WIN32)
  return "vulkan-1.dll";
#else
  return "libvulkan.so.1";
#endif
}

}  // namespace

std::unique_ptr<ImpellerVkContext> ImpellerVkContext::Create() {
  auto self = std::unique_ptr<ImpellerVkContext>(new ImpellerVkContext());

  // The loader is loaded by name rather than linked: every call into Vulkan
  // below goes through the function pointers this resolves, so the binary
  // keeps starting on a machine with no Vulkan driver at all -- where the
  // answer is simply the GL fallback.
  self->vulkan_library_ = fml::NativeLibrary::Create(VulkanLoaderName());
  if (!self->vulkan_library_) {
    FML_LOG(ERROR) << "Could not load " << VulkanLoaderName()
                   << "; no Vulkan driver is installed.";
    return nullptr;
  }
  auto instance_proc_addr =
      self->vulkan_library_->ResolveFunction<PFN_vkGetInstanceProcAddr>(
          "vkGetInstanceProcAddr");
  if (!instance_proc_addr.has_value()) {
    FML_LOG(ERROR) << "Could not resolve vkGetInstanceProcAddr.";
    return nullptr;
  }

  // The loader has to outlive every Vulkan object dialed through it, and some
  // of those objects outlive this context: the FFI layer's upload target holds
  // the Impeller context in a static, and decoded-image textures sit in
  // Rust-side caches that only let go at process exit. A FreeLibrary under
  // live driver objects is the one thing the loader does not promise to
  // survive, so one reference is leaked on purpose -- the OS reclaims the
  // mapping with the process.
  static auto* const leaked_library =
      new fml::RefPtr<fml::NativeLibrary>(self->vulkan_library_);
  (void)leaked_library;

  // The shaders Impeller's entity renderer is built out of, as SPIR-V blobs
  // compiled into the binary. Vulkan cannot compile GLSL at runtime without
  // shaderc, which is why these are baked in rather than shipped as sources.
  std::vector<std::shared_ptr<fml::Mapping>> shader_mappings = {
      std::make_shared<fml::NonOwnedMapping>(impeller_entity_shaders_vk_data,
                                             impeller_entity_shaders_vk_length),
      std::make_shared<fml::NonOwnedMapping>(
          impeller_framebuffer_blend_shaders_vk_data,
          impeller_framebuffer_blend_shaders_vk_length),
      std::make_shared<fml::NonOwnedMapping>(impeller_modern_shaders_vk_data,
                                             impeller_modern_shaders_vk_length),
  };

  impeller::ContextVK::Settings settings;
  settings.proc_address_callback = instance_proc_addr.value();
  settings.shader_libraries_data = std::move(shader_mappings);
  // Where the pipeline cache is persisted between runs. Validation layers and
  // GPU tracing stay off, matching the build flags.
  settings.cache_directory = fml::paths::GetCachesDirectory();

  auto context = impeller::ContextVK::Create(std::move(settings));
  if (!context || !context->IsValid()) {
    FML_LOG(ERROR) << "Could not create the Impeller Vulkan context.";
    return nullptr;
  }

  // A driver on Impeller's own blocklist is a rendering bug waiting to be
  // reported against us; the GL path is the answer that already works. The
  // list is Android-shaped today, so this normally passes on Windows -- it is
  // checked anyway because the day it grows a Windows entry is the day it was
  // worth it.
  if (context->GetDriverInfo()->IsKnownBadDriver()) {
    FML_LOG(IMPORTANT) << "Known bad Vulkan driver encountered, falling back "
                          "to OpenGL ES.";
    return nullptr;
  }

  // The surface context is the handle the swapchain hangs off; it is created
  // now, without a window, and SetWindow gives it one.
  self->surface_context_ = context->CreateSurfaceContext();
  if (!self->surface_context_) {
    FML_LOG(ERROR) << "Could not create the Vulkan surface context.";
    return nullptr;
  }
  self->context_ = std::move(context);

  FML_LOG(IMPORTANT) << "Rendering with Impeller (Vulkan).";
  return self;
}

ImpellerVkContext::~ImpellerVkContext() {
  // The swapchain holds device objects; dropping it before the context's
  // members wind down keeps the teardown ordered.
  if (surface_context_) {
    surface_context_->TeardownSwapchain();
  }
  // Shutdown stops the context's worker threads -- the fence waiter and the
  // resource manager. They have to be joined here, while the loader below is
  // still mapped: a worker that wakes after the library is unloaded calls
  // through a function pointer into unmapped memory.
  if (context_) {
    context_->Shutdown();
  }
}

std::shared_ptr<impeller::Context> ImpellerVkContext::GetImpellerContext()
    const {
  return context_;
}

#if defined(_WIN32)

bool ImpellerVkContext::SetWindow(HWND window, impeller::ISize size) {
  if (window == nullptr || !context_ || !surface_context_) {
    return false;
  }

  // vkCreateWin32SurfaceKHR wants the HINSTANCE the window class was
  // registered against; the window itself remembers it.
  impeller::vk::Win32SurfaceCreateInfoKHR surface_info;
  surface_info.setHinstance(
      reinterpret_cast<HINSTANCE>(GetWindowLongPtr(window, GWLP_HINSTANCE)));
  surface_info.setHwnd(window);

  auto result =
      context_->GetInstance().createWin32SurfaceKHRUnique(surface_info);
  if (result.result != impeller::vk::Result::eSuccess) {
    FML_LOG(ERROR) << "Could not create the Vulkan surface for the window: "
                   << impeller::vk::to_string(result.result);
    return false;
  }

  if (!surface_context_->SetWindowSurface(std::move(result.value), size)) {
    FML_LOG(ERROR) << "Could not build the Vulkan swapchain on the window.";
    return false;
  }
  return true;
}

#else

bool ImpellerVkContext::SetWindow(void* xdisplay,
                                  uint64_t window,
                                  impeller::ISize size) {
  if (xdisplay == nullptr || window == 0 || !context_ || !surface_context_) {
    return false;
  }

  // The Xlib surface rather than the xcb one, because the window came from
  // GTK, which speaks Xlib. The extension was enabled -- or not -- when
  // Impeller created the instance; a loader without it fails here and the
  // caller falls back to GL.
  impeller::vk::XlibSurfaceCreateInfoKHR surface_info;
  surface_info.setDpy(static_cast<Display*>(xdisplay));
  surface_info.setWindow(static_cast<::Window>(window));

  auto result =
      context_->GetInstance().createXlibSurfaceKHRUnique(surface_info);
  if (result.result != impeller::vk::Result::eSuccess) {
    FML_LOG(ERROR) << "Could not create the Vulkan surface for the window: "
                   << impeller::vk::to_string(result.result);
    return false;
  }

  if (!surface_context_->SetWindowSurface(std::move(result.value), size)) {
    FML_LOG(ERROR) << "Could not build the Vulkan swapchain on the window.";
    return false;
  }
  return true;
}

#endif

void ImpellerVkContext::UpdateSize(impeller::ISize size) {
  if (surface_context_) {
    surface_context_->UpdateSurfaceSize(size);
  }
}

}  // namespace flutter
