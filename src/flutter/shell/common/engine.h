// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_SHELL_COMMON_ENGINE_H_
#define FLUTTER_SHELL_COMMON_ENGINE_H_

#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "flutter/assets/asset_manager.h"
#include "flutter/common/task_runners.h"
#include "flutter/fml/macros.h"
#include "flutter/fml/mapping.h"
#include "flutter/fml/memory/weak_ptr.h"
#include "flutter/lib/ui/painting/image_generator_registry.h"
#include "flutter/lib/ui/semantics/custom_accessibility_action.h"
#include "flutter/lib/ui/semantics/semantics_node.h"
#include "flutter/lib/ui/snapshot_delegate.h"
#include "flutter/lib/ui/text/font_collection.h"
#include "flutter/lib/ui/window/platform_message.h"
#include "flutter/lib/ui/window/viewport_metrics.h"
#include "flutter/runtime/runtime_controller.h"
#include "flutter/runtime/runtime_delegate.h"
#include "flutter/shell/common/animator.h"
#include "flutter/shell/common/pointer_data_dispatcher.h"
#include "flutter/shell/common/run_configuration.h"

namespace flutter {

//------------------------------------------------------------------------------
/// The component owned by the shell that lives on the UI task runner and hosts
/// the framework.
///
/// Upstream the engine's whole reason to exist is the root isolate: it creates
/// it, launches its entry point, restarts it for hot restart, and tears it down.
/// Here the framework is a statically linked Rust library, so all of that
/// collapses into `RuntimeController::LaunchApplication`. What is left is what
/// the class was always also doing, and what actually matters to the pipeline:
///
///   * Routing platform messages, pointer packets and semantics actions inward.
///   * Taking the framework's layer trees outward, through `Animator`, into the
///     `Rasterizer`.
///   * Owning the font collection and asset manager.
///
/// Every method runs on the UI task runner unless a comment says otherwise.
class Engine final : public RuntimeDelegate,
                     public PointerDataDispatcher::Delegate {
 public:
  //----------------------------------------------------------------------------
  /// Result of `Engine::Run`.
  enum class RunStatus {
    /// The application was launched and is running.
    Success,

    /// An application was already running. Upstream this mattered because a
    /// second isolate could not be hosted; here it simply means Run was called
    /// twice. Not necessarily an error for the embedder -- resuming from a
    /// paused state can reach this path.
    FailureAlreadyRunning,

    /// The configuration was invalid or the framework's entry point failed.
    Failure,
  };

  //----------------------------------------------------------------------------
  /// What the engine asks of the shell. Upstream's OnRootIsolateCreated,
  /// UpdateIsolateDescription and RequestDartDeferredLibrary are gone: they
  /// exist to report VM state to the service protocol and to load deferred Dart
  /// libraries, neither of which has a counterpart here.
  class Delegate {
   public:
    virtual void OnEngineUpdateSemantics(
        int64_t view_id,
        SemanticsNodeUpdates updates,
        CustomAccessibilityActionUpdates actions) = 0;

    virtual void OnEngineSetApplicationLocale(std::string locale) = 0;

    virtual void OnEngineSetSemanticsTreeEnabled(bool enabled) = 0;

    virtual void OnEngineHandlePlatformMessage(
        std::unique_ptr<PlatformMessage> message) = 0;

    virtual void OnPreEngineRestart() = 0;

    virtual void SetNeedsReportTimings(bool needs_reporting) = 0;

    virtual std::unique_ptr<std::vector<std::string>>
    ComputePlatformResolvedLocale(
        const std::vector<std::string>& supported_locale_data) = 0;

    virtual fml::TimePoint GetCurrentTimePoint() = 0;

    virtual const std::shared_ptr<PlatformMessageHandler>&
    GetPlatformMessageHandler() const = 0;

    virtual void OnEngineChannelUpdate(std::string name, bool listening) = 0;

    virtual double GetScaledFontSize(double unscaled_font_size,
                                     int configuration_id) const = 0;

    virtual void RequestViewFocusChange(
        const ViewFocusChangeRequest& request) = 0;
  };

  //----------------------------------------------------------------------------
  /// Creates an engine with an externally supplied runtime controller. Used by
  /// tests that need to substitute one.
  Engine(Delegate& delegate,
         const PointerDataDispatcherMaker& dispatcher_maker,
         const TaskRunners& task_runners,
         const Settings& settings,
         std::unique_ptr<Animator> animator,
         const std::shared_ptr<FontCollection>& font_collection,
         std::unique_ptr<RuntimeController> runtime_controller);

  //----------------------------------------------------------------------------
  /// Creates an engine and its runtime controller. This is what the shell uses.
  Engine(Delegate& delegate,
         const PointerDataDispatcherMaker& dispatcher_maker,
         const TaskRunners& task_runners,
         const PlatformData& platform_data,
         const Settings& settings,
         std::unique_ptr<Animator> animator);

  ~Engine() override;

  fml::TaskRunnerAffineWeakPtr<Engine> GetWeakPtr() const;

  //----------------------------------------------------------------------------
  /// Launches the framework. Idempotent in the sense that a second call while
  /// running returns `FailureAlreadyRunning` rather than restarting.
  [[nodiscard]] RunStatus Run(RunConfiguration configuration);

  //----------------------------------------------------------------------------
  /// Tears the framework down and launches it again. Upstream this was hot
  /// restart, which cloned the isolate; here the Rust application object is
  /// simply rebuilt, so app state is lost but engine state is not.
  [[nodiscard]] bool Restart(RunConfiguration configuration);

  void SetupDefaultFontManager();

  bool UpdateAssetManager(const std::shared_ptr<AssetManager>& asset_manager);

  // -- Frames -----------------------------------------------------------------

  void BeginFrame(fml::TimePoint frame_time, uint64_t frame_number);

  void NotifyIdle(fml::TimeDelta deadline);

  void ReportTimings(std::vector<int64_t> timings);

  // -- Views ------------------------------------------------------------------

  void AddView(int64_t view_id,
               const ViewportMetrics& view_metrics,
               std::function<void(bool added)> callback);

  bool RemoveView(int64_t view_id);

  bool SendViewFocusEvent(const ViewFocusEvent& event);

  void SetViewportMetrics(int64_t view_id, const ViewportMetrics& metrics);

  void SetDisplays(const std::vector<DisplayData>& displays);

  // -- Input ------------------------------------------------------------------

  void DispatchPlatformMessage(std::unique_ptr<PlatformMessage> message);

  void DispatchPointerDataPacket(std::unique_ptr<PointerDataPacket> packet,
                                 uint64_t trace_flow_id);

  HitTestResponse HitTest(int64_t view_id, const flutter::PointData offset);

  void DispatchSemanticsAction(int64_t view_id,
                               int node_id,
                               SemanticsAction action,
                               fml::MallocMapping args);

  void SetSemanticsEnabled(bool enabled);

  void SetAccessibilityFeatures(int32_t flags);

  // -- Accessors --------------------------------------------------------------

  fml::TaskRunnerAffineWeakPtr<ImageGeneratorRegistry>
  GetImageGeneratorRegistry();

  // |PointerDataDispatcher::Delegate|
  void ScheduleSecondaryVsyncCallback(uintptr_t id,
                                      const fml::closure& callback) override;

  const std::string& GetLastEntrypoint() const;

  std::optional<int64_t> GetLastEngineId() const;

  const std::vector<std::string>& GetLastEntrypointArgs() const;

  const std::string& InitialRoute() const { return initial_route_; }

  const RuntimeController* GetRuntimeController() const {
    return runtime_controller_.get();
  }

  const std::weak_ptr<VsyncWaiter> GetVsyncWaiter() const;

  // |RuntimeDelegate|
  void ScheduleFrame(bool regenerate_layer_trees) override;

  void ScheduleFrame() { ScheduleFrame(true); }

  // |RuntimeDelegate|
  void OnAllViewsRendered() override;

  // |RuntimeDelegate|
  FontCollection& GetFontCollection() override;

  // |RuntimeDelegate|
  std::shared_ptr<AssetManager> GetAssetManager() override;

  // |PointerDataDispatcher::Delegate|
  void DoDispatchPacket(std::unique_ptr<PointerDataPacket> packet,
                        uint64_t trace_flow_id) override;

 private:
  // |RuntimeDelegate|
  std::string DefaultRouteName() override;

  // |RuntimeDelegate|
  void Render(int64_t view_id,
              std::unique_ptr<flutter::LayerTree> layer_tree,
              float device_pixel_ratio) override;

  // |RuntimeDelegate|
  void UpdateSemantics(int64_t view_id,
                       SemanticsNodeUpdates update,
                       CustomAccessibilityActionUpdates actions) override;

  // |RuntimeDelegate|
  void SetApplicationLocale(std::string locale) override;

  // |RuntimeDelegate|
  void SetSemanticsTreeEnabled(bool enabled) override;

  // |RuntimeDelegate|
  void HandlePlatformMessage(std::unique_ptr<PlatformMessage> message) override;

  // |RuntimeDelegate|
  std::unique_ptr<std::vector<std::string>> ComputePlatformResolvedLocale(
      const std::vector<std::string>& supported_locale_data) override;

  // |RuntimeDelegate|
  std::weak_ptr<PlatformMessageHandler> GetPlatformMessageHandler()
      const override;

  // |RuntimeDelegate|
  void SendChannelUpdate(std::string name, bool listening) override;

  // |RuntimeDelegate|
  double GetScaledFontSize(double unscaled_font_size,
                           int configuration_id) const override;

  // |RuntimeDelegate|
  void RequestViewFocusChange(const ViewFocusChangeRequest& request) override;

  // |RuntimeDelegate|
  void SetNeedsReportTimings(bool value) override;

  bool HandleLifecyclePlatformMessage(PlatformMessage* message);
  bool HandleNavigationPlatformMessage(
      std::unique_ptr<PlatformMessage> message);
  bool HandleLocalizationPlatformMessage(PlatformMessage* message);
  void HandleSettingsPlatformMessage(PlatformMessage* message);
  void HandleAssetPlatformMessage(std::unique_ptr<PlatformMessage> message);

  Delegate& delegate_;
  const Settings settings_;
  std::unique_ptr<Animator> animator_;
  std::unique_ptr<RuntimeController> runtime_controller_;

  std::string initial_route_;
  std::string last_entry_point_;
  std::vector<std::string> last_entry_point_args_;
  std::optional<int64_t> last_engine_id_;

  std::shared_ptr<AssetManager> asset_manager_;
  std::shared_ptr<FontCollection> font_collection_;
  ImageGeneratorRegistry image_generator_registry_;
  std::unique_ptr<PointerDataDispatcher> pointer_data_dispatcher_;

  TaskRunners task_runners_;

  // Must be the last member so that weak pointers are invalidated first.
  fml::TaskRunnerAffineWeakPtrFactory<Engine> weak_factory_;

  FML_DISALLOW_COPY_AND_ASSIGN(Engine);
};

}  // namespace flutter

#endif  // FLUTTER_SHELL_COMMON_ENGINE_H_
