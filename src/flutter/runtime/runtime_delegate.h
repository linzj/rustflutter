// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUNTIME_RUNTIME_DELEGATE_H_
#define FLUTTER_RUNTIME_RUNTIME_DELEGATE_H_

// What the runtime asks of its owner (the Engine).
//
// This is upstream's runtime_delegate.h with the four Dart-VM-specific members
// removed -- OnRootIsolateCreated, UpdateIsolateDescription and
// RequestDartDeferredLibrary have no counterpart when the framework is a
// statically linked Rust library, and dart_api.h is gone with them. Everything
// else is unchanged, including the one member that matters most:
//
//     virtual void Render(int64_t view_id,
//                         std::unique_ptr<flutter::LayerTree> layer_tree,
//                         float device_pixel_ratio) = 0;
//
// A layer tree is the framework layer's only output. Everything downstream of
// this call -- rasterizer, display list, Impeller -- is stock engine code.

#include <memory>
#include <string>
#include <vector>

#include "flutter/assets/asset_manager.h"
#include "flutter/flow/layers/layer_tree.h"
#include "flutter/lib/ui/semantics/custom_accessibility_action.h"
#include "flutter/lib/ui/semantics/semantics_node.h"
#include "flutter/lib/ui/text/font_collection.h"
#include "flutter/lib/ui/window/platform_message.h"
#include "flutter/lib/ui/window/view_focus.h"
#include "flutter/shell/common/platform_message_handler.h"

namespace flutter {

class RuntimeDelegate {
 public:
  virtual std::string DefaultRouteName() = 0;

  virtual void ScheduleFrame(bool regenerate_layer_trees = true) = 0;

  virtual void OnAllViewsRendered() = 0;

  virtual void Render(int64_t view_id,
                      std::unique_ptr<flutter::LayerTree> layer_tree,
                      float device_pixel_ratio) = 0;

  virtual void UpdateSemantics(int64_t view_id,
                               SemanticsNodeUpdates update,
                               CustomAccessibilityActionUpdates actions) = 0;

  virtual void SetApplicationLocale(std::string locale) = 0;

  virtual void SetSemanticsTreeEnabled(bool enabled) = 0;

  virtual void HandlePlatformMessage(
      std::unique_ptr<PlatformMessage> message) = 0;

  virtual FontCollection& GetFontCollection() = 0;

  virtual std::shared_ptr<AssetManager> GetAssetManager() = 0;

  virtual void SetNeedsReportTimings(bool value) = 0;

  virtual std::unique_ptr<std::vector<std::string>>
  ComputePlatformResolvedLocale(
      const std::vector<std::string>& supported_locale_data) = 0;

  virtual std::weak_ptr<PlatformMessageHandler> GetPlatformMessageHandler()
      const = 0;

  virtual void SendChannelUpdate(std::string name, bool listening) = 0;

  virtual double GetScaledFontSize(double unscaled_font_size,
                                   int configuration_id) const = 0;

  virtual void RequestViewFocusChange(
      const ViewFocusChangeRequest& request) = 0;

 protected:
  virtual ~RuntimeDelegate();
};

}  // namespace flutter

#endif  // FLUTTER_RUNTIME_RUNTIME_DELEGATE_H_
