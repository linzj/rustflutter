// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUNTIME_RUNTIME_CONTROLLER_H_
#define FLUTTER_RUNTIME_RUNTIME_CONTROLLER_H_

#include <limits>
#include <memory>
#include <string>
#include <unordered_set>
#include <vector>

#include "flutter/common/task_runners.h"
#include "flutter/fml/macros.h"
#include "flutter/fml/mapping.h"
#include "flutter/lib/ui/semantics/semantics_node.h"
#include "flutter/lib/ui/window/hit_test_response.h"
#include "flutter/lib/ui/window/platform_message.h"
#include "flutter/lib/ui/window/point_data.h"
#include "flutter/lib/ui/window/pointer_data_packet.h"
#include "flutter/lib/ui/window/viewport_metrics.h"
#include "flutter/runtime/platform_data.h"
#include "flutter/runtime/runtime_delegate.h"
#include "flutter/runtime/rust_app_api.h"

namespace flutter {

//------------------------------------------------------------------------------
/// The engine's window into the framework layer.
///
/// Upstream this class owns the root DartIsolate and reaches the framework
/// through PlatformConfiguration's twenty tonic::DartPersistentValue callbacks.
/// Here it owns an RfApp -- a statically linked Rust framework instance -- and
/// reaches it through the rf_app_* C ABI. The direction of every call and the
/// order they happen in is unchanged, which is what lets Engine, Animator,
/// Rasterizer and the platform embedders stay stock.
///
/// Everything below runs on the UI task runner.
class RuntimeController {
 public:
  using AddViewCallback = std::function<void(bool added)>;

  RuntimeController(RuntimeDelegate& client,
                    const TaskRunners& task_runners,
                    const PlatformData& platform_data);

  ~RuntimeController();

  //----------------------------------------------------------------------------
  /// Creates the Rust application instance and runs its entry point. Replaces
  /// upstream's LaunchRootIsolate. Returns false if the app is already running
  /// or could not be created.
  bool LaunchApplication();

  bool IsRunning() const;

  // -- Views ------------------------------------------------------------------

  void AddView(int64_t view_id,
               const ViewportMetrics& view_metrics,
               AddViewCallback callback);

  bool RemoveView(int64_t view_id);

  bool ViewExists(int64_t view_id) const;

  bool SetViewportMetrics(int64_t view_id, const ViewportMetrics& metrics);

  //----------------------------------------------------------------------------
  /// Tells the framework a view gained or lost focus. Focus traversal lives in
  /// the widgets layer (M6); until then this is accepted and recorded so the
  /// embedders' call sites stay intact.
  bool SendViewFocusEvent(const ViewFocusEvent& event);

  // -- Platform state ---------------------------------------------------------
  //
  // These are recorded in platform_data_ whether or not the app is running, so
  // that an app launched later starts from the current state. That mirrors
  // upstream's FlushRuntimeStateToIsolate.

  bool SetDisplays(const std::vector<DisplayData>& displays);
  bool SetLocales(const std::vector<std::string>& locale_data);
  bool SetUserSettingsData(const std::string& data);
  bool SetInitialLifecycleState(const std::string& data);
  bool SetSemanticsEnabled(bool enabled);
  bool SetAccessibilityFeatures(int32_t flags);

  const PlatformData& GetPlatformData() const { return platform_data_; }

  // -- Frames -----------------------------------------------------------------

  //----------------------------------------------------------------------------
  /// Runs one frame: the animation phase, then build/layout/paint. Each view
  /// the framework paints comes back through RfAppHost::render and is forwarded
  /// to RuntimeDelegate::Render. Called by Animator on vsync.
  bool BeginFrame(fml::TimePoint frame_time, uint64_t frame_number);

  bool ReportTimings(std::vector<int64_t> timings);

  bool NotifyIdle(fml::TimeDelta deadline);

  // -- Input ------------------------------------------------------------------

  bool DispatchPlatformMessage(std::unique_ptr<PlatformMessage> message);

  bool DispatchPointerDataPacket(const PointerDataPacket& packet);

  HitTestResponse HitTest(int64_t view_id, const PointData offset);

  bool DispatchSemanticsAction(int64_t view_id,
                               int32_t node_id,
                               SemanticsAction action,
                               fml::MallocMapping args);

 private:
  // RfAppHost trampolines. `user_data` is the RuntimeController.
  static void OnRender(void* user_data,
                       int64_t view_id,
                       RfLayerTree* tree,
                       double device_pixel_ratio);
  static void OnScheduleFrame(void* user_data);

  // Fires RuntimeDelegate::OnAllViewsRendered once every view that has metrics
  // has produced a layer tree this frame. The rasterizer waits on this to know
  // a multi-view frame is complete.
  void CheckIfAllViewsRendered();

  static RfViewMetrics ToRfViewMetrics(const ViewportMetrics& metrics);

  RuntimeDelegate& client_;
  TaskRunners task_runners_;
  PlatformData platform_data_;
  RfApp* app_ = nullptr;

  std::unordered_set<int64_t> rendered_views_during_frame_;
  bool frame_in_progress_ = false;

  // The last values handed to the framework, so a frame cannot report a time
  // earlier than the one before it. See BeginFrame.
  int64_t last_frame_micros_ = std::numeric_limits<int64_t>::min();
  uint64_t last_frame_number_ = 0;

  FML_DISALLOW_COPY_AND_ASSIGN(RuntimeController);
};

}  // namespace flutter

#endif  // FLUTTER_RUNTIME_RUNTIME_CONTROLLER_H_
